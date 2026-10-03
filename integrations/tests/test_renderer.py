"""Protocol/capability tests; importing the renderer never imports GI or opens a display."""

from contextlib import redirect_stdout
from dataclasses import replace
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace as NS
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import renderer
import native_scene


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.file = Path(self.directory.name) / "wall paper.png"
        self.file.write_bytes(b"test")
        self.settings = renderer.Settings(str(self.file), "image", "eDP-1").validated()

    def test_import_without_site_packages(self):
        result = subprocess.run([sys.executable, "-S", "-c",
                                 "import sys; sys.path.insert(0, sys.argv[1]); import renderer; "
                                 "assert 'gi' not in sys.modules", str(ROOT)],
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_diagnostics_without_gi_is_json_and_successful_probe(self):
        result = subprocess.run([sys.executable, "-S", str(ROOT / "renderer.py"), "--diagnostics"],
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0)
        payload = json.loads(result.stdout)
        self.assertFalse(payload["available"])
        self.assertIn("GTK4", payload["message"])
        self.assertEqual(set(payload), {"desktop", "session", "backend", "available", "message", "monitors"})

    def test_headless_diagnostics_still_returns_json(self):
        environment = dict(os.environ, DISPLAY=":9876", WAYLAND_DISPLAY="wall-in-no-display",
                           GDK_BACKEND="x11")
        result = subprocess.run([sys.executable, str(ROOT / "renderer.py"), "--diagnostics"],
                                env=environment, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertFalse(payload["available"])
        self.assertEqual(payload["monitors"], [])

    def test_missing_file_never_reports_ready(self):
        result = subprocess.run([sys.executable, "-S", str(ROOT / "renderer.py"),
                                 "--file", str(self.file) + "missing", "--monitor", "eDP-1"],
                                capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertTrue(result.stderr)

    def test_simple_actions(self):
        for action in ("pause", "resume", "stop"):
            with self.subTest(action=action):
                actual, settings = renderer.parse_command(json.dumps({"action": action}), self.settings)
                self.assertEqual(actual, action)
                self.assertIs(settings, self.settings)

    def test_partial_flat_configuration(self):
        action, settings = renderer.parse_command(
            '{"action":"configure","volume":42,"muted":true,"fit":"contain"}', self.settings)
        self.assertEqual(action, "configure")
        self.assertEqual(settings.volume, 42)
        self.assertTrue(settings.muted)
        self.assertEqual(settings.fit, "contain")
        self.assertEqual(settings.file, self.settings.file)

    def test_nested_configuration(self):
        _, settings = renderer.parse_command(
            '{"action":"configure","settings":{"monitor":"HDMI-A-1","fit":"fill"}}', self.settings)
        self.assertEqual(settings.monitor, "HDMI-A-1")
        self.assertEqual(settings.fit, "fill")

    def test_rejects_malformed_commands(self):
        commands = ["null", "[]", "not json", "{}", '{"action":"other"}',
                    '{"action":"pause","volume":2}', '{"action":"configure","unknown":1}',
                    '{"action":"configure","settings":[]}',
                    '{"action":"configure","settings":{},"volume":2}',
                    '{"action":"configure","volume":NaN}',
                    '{"action":"configure","volume":true}',
                    '{"action":"configure","muted":"false"}',
                    '{"action":"configure","pause_on_battery":0}',
                    '{"action":"configure","pause_on_fullscreen":"false"}',
                    '{"action":"configure","fit":"stretch"}',
                    '{"action":"configure","monitor":0}',
                    '{"action":"configure","kind":"url"}',
                    '{"action":"configure","file":"https://example.com/video.mp4"}']
        for command in commands:
            with self.subTest(command=command), self.assertRaises((ValueError, OSError)):
                renderer.parse_command(command, self.settings)

    def test_command_size_limit(self):
        with self.assertRaisesRegex(ValueError, "64 KiB"):
            renderer.parse_command(" " * (renderer.MAX_COMMAND_BYTES + 1), self.settings)

    def test_volume_bounds(self):
        for volume in (-1, 101, float("inf"), "50", None, True):
            with self.subTest(volume=volume), self.assertRaises(ValueError):
                replace(self.settings, volume=volume).validated()
        for volume in (0, 0.5, 100):
            self.assertEqual(replace(self.settings, volume=volume).validated().volume, volume)

    def test_file_must_be_regular(self):
        with self.assertRaises(ValueError):
            replace(self.settings, file=self.directory.name).validated()

    def test_control_drives_media(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.settings = self.settings
        instance.stopping = False
        instance.media = Mock()
        instance.paused = False
        instance.battery = False
        instance.fullscreen = False
        instance.playback_media = instance.media
        instance.playback_paused = False
        instance.control('{"action":"pause"}')
        self.assertTrue(instance.paused)
        instance.media.pause.assert_called_once()
        instance.control('{"action":"resume"}')
        self.assertFalse(instance.paused)
        instance.media.play.assert_called_once()
        instance.stop = Mock()
        instance.control('{"action":"stop"}')
        instance.stop.assert_called_once()

    def test_policy_pauses_and_restores_without_overriding_manual_pause(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.settings = self.settings
        instance.media = Mock()
        instance.playback_media = instance.media
        instance.playback_paused = False
        instance.stopping = False
        instance.paused = False
        instance.battery = False
        instance.fullscreen = False
        instance.battery = True
        instance.update_playback()
        instance.media.pause.assert_called_once()
        instance.fullscreen = True
        instance.battery = False
        instance.update_playback()
        instance.media.pause.assert_called_once()
        instance.control('{"action":"pause"}')
        instance.fullscreen = False
        instance.update_playback()
        instance.media.play.assert_not_called()
        instance.control('{"action":"resume"}')
        instance.media.play.assert_called_once()
        instance.settings = replace(self.settings, pause_on_battery=False, pause_on_fullscreen=False)
        instance.battery = instance.fullscreen = True
        instance.update_playback()
        self.assertEqual(instance.media.pause.call_count, 1)

    def test_video_stall_seeks_to_start_without_restarting_a_paused_stream(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.media = Mock()
        instance.media.is_prepared.return_value = True
        instance.media.get_duration.return_value = 30_000_000
        instance.media.get_timestamp.return_value = 30_000_000
        instance.ready = True
        instance.playback_paused = False
        instance.last_media_timestamp = None
        instance.last_media_progress = 100.0
        instance.media_recovery_attempts = 0
        with patch.object(renderer.time, "monotonic", return_value=101.0):
            instance.maintain_media()
        with patch.object(renderer.time, "monotonic", return_value=105.0):
            instance.maintain_media()
        instance.media.seek.assert_called_once_with(0)
        instance.media.play.assert_called_once()
        instance.playback_paused = True
        with patch.object(renderer.time, "monotonic", return_value=110.0):
            instance.maintain_media()
        self.assertEqual(instance.media.seek.call_count, 1)
        self.assertIsNone(instance.last_media_timestamp)

    def test_unrecoverable_video_stall_reports_error(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.media = Mock()
        instance.media.is_prepared.return_value = True
        instance.media.get_duration.return_value = 30_000_000
        instance.media.get_timestamp.return_value = 30_000_000
        instance.ready = True
        instance.playback_paused = False
        instance.last_media_timestamp = 30_000_000
        instance.last_media_progress = 0.0
        instance.media_recovery_attempts = 0
        for second in (4.0, 8.0, 12.0):
            with patch.object(renderer.time, "monotonic", return_value=second):
                instance.maintain_media()
        self.assertEqual(instance.media.seek.call_count, 3)
        with patch.object(renderer.time, "monotonic", return_value=16.0), \
                self.assertRaisesRegex(RuntimeError, "stopped advancing"):
            instance.maintain_media()

    def test_video_progress_does_not_seek(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.media = Mock()
        instance.media.is_prepared.return_value = True
        instance.media.get_duration.return_value = 30_000_000
        instance.media.get_timestamp.side_effect = [2_000_000, 3_000_000, 4_000_000]
        instance.ready = True
        instance.playback_paused = False
        instance.last_media_timestamp = None
        instance.last_media_progress = 0.0
        instance.media_recovery_attempts = 0
        for second in (5.0, 9.0, 13.0):
            with patch.object(renderer.time, "monotonic", return_value=second):
                instance.maintain_media()
        instance.media.seek.assert_not_called()

    def test_upower_on_battery_and_unavailable_fail_open(self):
        with patch.object(renderer, "dbus_call", return_value=(NS(unpack=lambda: True),)) as call:
            self.assertTrue(renderer.on_battery(None, None))
        self.assertEqual(call.call_args.args[2:4], ("Get", ("org.freedesktop.UPower", "OnBattery")))
        with patch.object(renderer, "dbus_call", side_effect=RuntimeError("UPower absent")), \
                patch.object(renderer, "battery_from_sysfs", return_value=True):
            self.assertTrue(renderer.on_battery(None, None))

    def test_sysfs_battery_fallback_reads_mains_without_guessing_from_battery_level(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            battery = root / "BAT1"
            adapter = root / "ACAD"
            battery.mkdir()
            adapter.mkdir()
            (battery / "type").write_text("Battery\n")
            (battery / "status").write_text("Discharging\n")
            (adapter / "type").write_text("Mains\n")
            (adapter / "online").write_text("0\n")
            self.assertTrue(renderer.battery_from_sysfs(root))
            (adapter / "online").write_text("1\n")
            self.assertFalse(renderer.battery_from_sysfs(root))
            (adapter / "online").unlink()
            self.assertFalse(renderer.battery_from_sysfs(root))

    def test_gnome_renderer_refuses_window_close_but_explicit_stop_still_works(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.backend = "gnome-shell"
        instance.stopping = False
        instance.fail = Mock()
        self.assertTrue(instance.on_close(None))
        instance.fail.assert_not_called()
        instance.stopping = True
        self.assertTrue(instance.on_close(None))
        instance.fail.assert_not_called()

    def test_other_renderer_surface_close_reports_failure(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.backend = "layer-shell"
        instance.stopping = False
        instance.fail = Mock()
        self.assertTrue(instance.on_close(None))
        instance.fail.assert_called_once()

    def test_eof_stops_supervised_child(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.GLib = NS(IO_HUP=1, IO_ERR=2)
        instance.stop = Mock()
        instance.control_buffer = bytearray()
        with patch.object(renderer.os, "read", return_value=b""):
            self.assertFalse(instance.read_control_ready(0, 1))
        instance.stop.assert_called_once()

    def test_oversized_stdin_fails(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.GLib = NS(IO_HUP=1, IO_ERR=2)
        instance.fail = Mock()
        instance.stopping = False
        instance.control_buffer = bytearray()
        with patch.object(renderer.os, "read", return_value=b"x" * 65536):
            self.assertTrue(instance.read_control_ready(0, 0))
        with patch.object(renderer.os, "read", return_value=b"x"):
            self.assertFalse(instance.read_control_ready(0, 0))
        self.assertIn("command exceeds 64 KiB", str(instance.fail.call_args.args[0]))

    def test_incremental_commands_stay_on_main_loop(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.GLib = NS(IO_HUP=1, IO_ERR=2)
        instance.stopping = False
        instance.control_buffer = bytearray()
        instance.control = Mock()
        with patch.object(renderer.os, "read", side_effect=[b'{"action":', b'"pause"}\n']):
            self.assertTrue(instance.read_control_ready(0, 0))
            instance.control.assert_not_called()
            self.assertTrue(instance.read_control_ready(0, 0))
        instance.control.assert_called_once_with('{"action":"pause"}')


class NativeSceneTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'cube.wallin-scene.json'
        self.valid = {'version': 1, 'camera': {'fov': 60, 'distance': 5},
                      'cube': {'size': 1.5, 'rpm': 12,
                               'colors': ['#ff0000', '#00ff00', '#0000ff', '#ffff00',
                                          '#ff00ff', '#00ffff']}, 'background': '#101020'}
        self.path.write_text(json.dumps(self.valid))

    def test_parser_projection_and_revalidation(self):
        scene = native_scene.load(self.path)
        self.assertEqual(len(native_scene.vertices(scene, 1920, 1080, 0)), 36 * 7)
        self.assertNotEqual(native_scene.vertices(scene, 1920, 1080, 0),
                            native_scene.vertices(scene, 1920, 1080, 1))
        self.assertEqual(scene['colors'][0], (1, 0, 0))
        settings = renderer.Settings(str(self.path), 'scene', 'eDP-1').validated()
        self.path.write_text(json.dumps({**self.valid, 'version': 2}))
        with self.assertRaises(ValueError):
            settings.validated()
        with self.assertRaises(ValueError):
            renderer.parse_command('{"action":"configure","volume":20}', settings)

    def test_rejects_nonfinite_duplicate_unknown_and_unbounded_inputs(self):
        for payload in [json.dumps({**self.valid, 'shader': 'evil'}),
                        json.dumps({**self.valid, 'camera': {'fov': float('nan'), 'distance': 5}}),
                        json.dumps({**self.valid, 'cube': {**self.valid['cube'], 'rpm': True}}),
                        json.dumps({**self.valid, 'cube': {**self.valid['cube'], 'colors': []}}),
                        json.dumps({**self.valid, 'cube': {**self.valid['cube'], 'size': 200}}),
                        json.dumps(self.valid).replace('"version": 1', '"version": 1, "version": 1'),
                        ' ' * 4097]:
            with self.subTest(payload=payload[:80]):
                self.path.write_text(payload)
                with self.assertRaises(ValueError):
                    native_scene.load(self.path)
        self.path.write_text(json.dumps(self.valid))
        with self.assertRaises(ValueError):
            native_scene.load(self.path.with_name('cube.pkg'))

    def test_scene_playback_pause_stop_and_readiness_are_real(self):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.scene = Mock(rendered=False)
        instance.media = None
        instance.settings = renderer.Settings(str(self.path), 'scene', 'eDP-1').validated()
        instance.paused = False
        instance.battery = instance.fullscreen = False
        instance.playback_media = instance.playback_paused = None
        instance.update_playback()
        instance.scene.set_paused.assert_called_with(False)
        instance.paused = True
        instance.update_playback()
        instance.scene.set_paused.assert_called_with(True)
        instance.stopping = False
        instance.backend = 'x11-desktop'
        instance.find_monitor = Mock()
        instance.last_battery_check = renderer.time.monotonic()
        instance.window = Mock()
        instance.window.get_mapped.return_value = True
        instance.ready = False
        instance.started = renderer.time.monotonic()
        instance.maintain_media = Mock()
        with redirect_stdout(io.StringIO()) as output:
            instance.check_state()
        self.assertEqual(output.getvalue(), '')
        instance.scene.rendered = True
        with redirect_stdout(io.StringIO()) as output:
            instance.check_state()
        self.assertIn('"ready"', output.getvalue())
        instance.registered = False
        instance.app = Mock()
        instance.stop()
        instance.scene.stop.assert_called_once()

    def test_frame_timer_and_pause_stop(self):
        area = native_scene.CubeArea.__new__(native_scene.CubeArea)
        area.area = Mock()
        area.GLib = Mock()
        area.timer = 23
        area.paused = False
        area.elapsed = 0
        area.last = 10
        with patch.object(native_scene.time, 'monotonic', return_value=12):
            area.set_paused(True)
        self.assertEqual(area.elapsed, 2)
        self.assertTrue(area.tick())
        area.area.queue_render.assert_not_called()
        with patch.object(native_scene.time, 'monotonic', return_value=20):
            area.set_paused(False)
        self.assertEqual(area.elapsed, 2)
        self.assertTrue(area.tick())
        self.assertEqual(area.area.queue_render.call_count, 2)
        area.stop()
        area.GLib.source_remove.assert_called_once_with(23)
        self.assertEqual(area.timer, None)

    def test_real_gtk_glarea_when_display_is_available(self):
        try:
            import gi
            gi.require_version('Gtk', '4.0')
            from gi.repository import Gtk, GLib
            if not Gtk.init_check():
                self.skipTest('GTK display unavailable')
        except (ImportError, ValueError):
            self.skipTest('GTK4 unavailable')
        failures = []
        window = Gtk.Window()
        window.set_default_size(320, 240)
        area = native_scene.CubeArea(Gtk, GLib, native_scene.load(self.path), failures.append)
        window.set_child(area.area)
        window.present()
        loop = GLib.MainLoop()
        GLib.timeout_add(1200, lambda: (loop.quit(), False)[1])
        try:
            loop.run()
            self.assertEqual(failures, [])
            self.assertTrue(area.rendered)
        finally:
            area.stop()
            window.destroy()

    def test_gpu_render_path_issues_depth_test_and_triangles(self):
        scene = native_scene.load(self.path)
        area = native_scene.CubeArea.__new__(native_scene.CubeArea)
        area.scene = scene
        area.gl = object()
        area.paused = True
        area.last = renderer.time.monotonic()
        area.elapsed = 0
        area.rendered = False
        area.set_paused = Mock()
        area.viewport = Mock()
        area.clear_color = Mock()
        area.clear = Mock()
        area.enable = Mock()
        area.use_program = Mock()
        area.program = 3
        area.bind_buffer = Mock()
        area.buffer = NS(value=4)
        area.buffer_data = Mock()
        area.enable_attrib = Mock()
        area.vertex_attrib = Mock()
        area.draw = Mock()
        area.get_error = Mock(return_value=0)
        area.fail = Mock()
        widget = NS(get_width=lambda: 640, get_height=lambda: 480, get_scale_factor=lambda: 1)
        self.assertTrue(area.render(widget, None))
        area.enable.assert_called_with(0x0B71)
        area.draw.assert_called_once_with(0x0004, 0, 36)
        self.assertTrue(area.rendered)


class DiagnosticsTests(unittest.TestCase):
    def setUp(self):
        monitor = Mock()
        monitor.get_connector.return_value = "eDP-1"
        monitor.get_geometry.return_value = NS(width=1920, height=1080)
        self.display = Mock()
        self.display.__gtype__ = NS(name="GdkWaylandDisplay")
        self.display.get_monitors.return_value = [monitor]
        self.Gdk = NS(Display=NS(get_default=lambda: self.display))
        self.Gtk = NS(MediaFile=NS(new=lambda: NS(__gtype__=NS(name="GtkGstMediaFileBuiltin"))))
        loader = patch.object(renderer, "load_gtk", return_value=(self.Gdk, Mock(), Mock(), self.Gtk))
        loader.start()
        self.addCleanup(loader.stop)
        env = patch.dict(os.environ, {"XDG_CURRENT_DESKTOP": "GNOME", "XDG_SESSION_TYPE": "wayland"})
        env.start()
        self.addCleanup(env.stop)

    def test_disabled_extension_is_unavailable(self):
        with patch.object(renderer, "dbus_call", return_value=({"state": 2},)) as call:
            result = renderer.diagnostics()
        self.assertFalse(result["available"])
        self.assertIn("not enabled", result["message"])
        self.assertEqual(call.call_count, 1)
        self.assertEqual(result["monitors"], [{"id": "eDP-1", "name": "eDP-1", "width": 1920, "height": 1080}])

    def test_enabled_extension_needs_live_bridge(self):
        with patch.object(renderer, "dbus_call", side_effect=[({"state": 1},), RuntimeError("No bridge")]):
            result = renderer.diagnostics()
        self.assertFalse(result["available"])
        self.assertIn("No bridge", result["message"])

    def test_live_matching_bridge_is_available(self):
        probe = {"version": 2, "supported": True, "monitors": [{"id": "eDP-1"}]}
        with patch.object(renderer, "dbus_call", side_effect=[({"state": 1},), (json.dumps(probe),)]) as call:
            result = renderer.diagnostics()
        self.assertTrue(result["available"])
        self.assertEqual(result["backend"], "gnome-shell")
        self.assertEqual([c.args[2] for c in call.call_args_list], ["GetExtensionInfo", "Probe"])

    def test_mismatched_bridge_version_or_monitors(self):
        for probe in ({"version": 2, "supported": True},
                      {"version": 1, "supported": False},
                      {"version": 2, "supported": True, "monitors": [{"id": "wrong"}]}):
            with patch.object(renderer, "dbus_call", side_effect=[({"state": 1},), (json.dumps(probe),)]):
                self.assertFalse(renderer.diagnostics()["available"])

    def test_no_monitors_is_unavailable(self):
        self.display.get_monitors.return_value = []
        self.assertFalse(renderer.diagnostics()["available"])

    def test_xwayland_is_not_a_wayland_wallpaper(self):
        self.display.__gtype__.name = "GdkX11Display"
        result = renderer.diagnostics()
        self.assertFalse(result["available"])
        self.assertIn("XWayland", result["message"])

    def test_media_backend_missing_is_unavailable(self):
        self.Gtk.MediaFile.new = lambda: NS(__gtype__=NS(name="GtkNoMediaFile"))
        probe = {"version": 2, "supported": True, "monitors": [{"id": "eDP-1"}]}
        with patch.object(renderer, "dbus_call", side_effect=[({"state": 1},), (json.dumps(probe),)]):
            result = renderer.diagnostics()
        self.assertFalse(result["available"])
        self.assertIn("media backend", result["message"])


class ReadinessTests(unittest.TestCase):
    def instance(self, attached=True):
        instance = renderer.Renderer.__new__(renderer.Renderer)
        instance.stopping = False
        instance.settings = NS(monitor="eDP-1")
        instance.backend = "gnome-shell"
        instance.token = "test"
        instance.find_monitor = Mock()
        instance.bridge = Mock(return_value=(json.dumps({"attached": attached, "error": ""}),))
        instance.media = None
        instance.window = Mock()
        instance.window.get_mapped.return_value = True
        instance.ready = False
        instance.started = renderer.time.monotonic()
        instance.last_battery_check = instance.started
        instance.battery = False
        instance.fullscreen = False
        instance.update_playback = Mock()
        instance.fail = Mock()
        return instance

    def test_ready_requires_attached_surface(self):
        instance = self.instance(attached=False)
        with redirect_stdout(io.StringIO()) as output:
            self.assertTrue(instance.check_state())
        self.assertFalse(instance.ready)
        self.assertEqual(output.getvalue(), "")

    def test_ready_is_exactly_one_json_event(self):
        instance = self.instance()
        with redirect_stdout(io.StringIO()) as output:
            instance.check_state()
            instance.check_state()
        self.assertEqual(output.getvalue(), '{"event": "ready"}\n')

    def test_prepare_timeout_is_failure(self):
        instance = self.instance(attached=False)
        instance.started -= 20
        self.assertFalse(instance.check_state())
        instance.fail.assert_called_once()

    def test_audio_only_file_is_not_a_video_wallpaper(self):
        instance = self.instance()
        instance.media = Mock()
        instance.media.get_error.return_value = None
        instance.media.is_prepared.return_value = True
        instance.media.get_intrinsic_width.return_value = 0
        instance.media.get_intrinsic_height.return_value = 0
        self.assertFalse(instance.check_state())
        self.assertIn("video track", str(instance.fail.call_args.args[0]))


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("installer", ROOT / "install-gnome-extension.py")
        cls.installer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.installer)

    def test_install_uninstall_is_reversible_and_never_auto_enables(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / self.installer.UUID
            with patch.object(self.installer.subprocess, "run") as run, redirect_stdout(io.StringIO()):
                self.installer.install(target)
                run.assert_not_called()
                self.assertTrue((target / "extension.js").is_file())
                with patch.object(self.installer, "enabled_extensions", return_value=set()):
                    self.installer.uninstall(target)
                self.assertFalse(target.exists())

    def test_upgrade_changes_only_managed_files_without_toggling_shell(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / self.installer.UUID
            with patch.object(self.installer.subprocess, "run") as run, redirect_stdout(io.StringIO()):
                self.installer.install(target)
                (target / "extension.js").write_text("stale")
                (target / "user-note").write_text("keep")
                self.installer.upgrade(target)
                run.assert_not_called()
            self.assertEqual((target / "extension.js").read_bytes(),
                             (ROOT / "gnome" / self.installer.UUID / "extension.js").read_bytes())
            self.assertEqual((target / "user-note").read_text(), "keep")

    def test_unmanaged_installation_is_not_overwritten_or_deleted(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / self.installer.UUID
            target.mkdir()
            (target / "keep").write_text("untouched")
            with self.assertRaises(RuntimeError):
                self.installer.install(target)
            with self.assertRaises(RuntimeError):
                self.installer.uninstall(target)
            with self.assertRaises(RuntimeError):
                self.installer.upgrade(target)
            self.assertEqual((target / "keep").read_text(), "untouched")

    def test_symlink_installation_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / self.installer.UUID
            target.symlink_to(Path(directory), target_is_directory=True)
            with self.assertRaises(RuntimeError):
                self.installer.install(target)
            with self.assertRaises(RuntimeError):
                self.installer.uninstall(target)


class PlasmaControlTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("plasma_control", ROOT / "plasma-control.py")
        cls.control = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.control)

    def test_pause_and_stop_write_real_persistent_state(self):
        pause = self.control.build_script(0, "pause", {})
        self.assertIn('"Paused": true', pause)
        self.assertIn('desktop.writeConfig', pause)
        self.assertIn('if (!false)', pause)
        stop = self.control.build_script(0, "stop", {})
        self.assertIn('"File": ""', stop)

    def test_configure_file_is_encoded_data_not_script(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'quote";video.mp4'
            path.write_bytes(b"test")
            script = self.control.build_script(1, "configure", {"file": str(path), "muted": True})
            self.assertIn(json.dumps(path.as_uri()), script)
            self.assertIn('d.screen === 1', script)

    def test_invalid_settings_and_screen_are_rejected(self):
        for screen, action, settings in [(True, "pause", {}), (-1, "stop", {}),
                                         (0, "configure", {"monitor": "eDP-1"}),
                                         (0, "configure", {"volume": True}),
                                         (0, "configure", {"muted": "false"}),
                                         (0, "pause", {"volume": 5})]:
            with self.subTest(settings=settings), self.assertRaises(ValueError):
                self.control.build_script(screen, action, settings)


if __name__ == "__main__":
    unittest.main()
