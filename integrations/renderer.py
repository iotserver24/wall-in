#!/usr/bin/env python3
"""Wall-in's supervised renderer. GI imports are deliberately deferred."""

import argparse
from dataclasses import asdict, dataclass, replace
import json
import math
import os
from pathlib import Path
import signal
import sys
import time
import uuid

APP_ID = "io.wallin.Renderer"
EXTENSION_UUID = "wall-in@r3ap3r"
BUS_NAME = "org.gnome.Shell"
OBJECT_PATH = "/io/wallin/Integration"
INTERFACE = "io.wallin.Integration"
PROTOCOL_VERSION = 2
MAX_COMMAND_BYTES = 65536


@dataclass(frozen=True)
class Settings:
    file: str
    kind: str = "video"
    monitor: str = ""
    fit: str = "cover"
    volume: float = 0.0
    muted: bool = False
    pause_on_battery: bool = True
    pause_on_fullscreen: bool = True

    def validated(self):
        if not isinstance(self.file, str) or not self.file:
            raise ValueError("file must be a local file path")
        path = Path(self.file).expanduser().resolve(strict=True)
        if not path.is_file() or not os.access(path, os.R_OK):
            raise ValueError("file must be a readable regular file")
        if self.kind not in ("image", "video", "scene"):
            raise ValueError("kind must be image, video or scene")
        if self.kind == "scene":
            from native_scene import load
            load(path)
        if self.fit not in ("cover", "contain", "fill"):
            raise ValueError("fit must be cover, contain, or fill")
        if not isinstance(self.monitor, str) or not self.monitor:
            raise ValueError("monitor must be a connector ID from diagnostics")
        if (isinstance(self.volume, bool) or not isinstance(self.volume, (int, float))
                or not math.isfinite(self.volume) or not 0 <= self.volume <= 100):
            raise ValueError("volume must be a finite number from 0 to 100")
        for name in ("muted", "pause_on_battery", "pause_on_fullscreen"):
            if not isinstance(getattr(self, name), bool):
                raise ValueError(f"{name} must be a boolean")
        return replace(self, file=str(path))


def parse_command(line, current):
    """Accept flat configure fields or a nested settings object, never both."""
    if len(line.encode("utf-8")) > MAX_COMMAND_BYTES:
        raise ValueError("command exceeds 64 KiB")
    command = json.loads(line)
    if not isinstance(command, dict):
        raise ValueError("command must be an object")
    action = command.get("action")
    if action not in ("pause", "resume", "stop", "configure"):
        raise ValueError("unknown action")
    fields = {key: value for key, value in command.items() if key != "action"}
    if action != "configure":
        if fields:
            raise ValueError(f"{action} does not take settings")
        return action, current
    if "settings" in fields:
        if len(fields) != 1 or not isinstance(fields["settings"], dict):
            raise ValueError("configure expects flat fields or one settings object")
        fields = fields["settings"]
    unknown = fields.keys() - asdict(current).keys()
    if unknown:
        raise ValueError(f"unknown settings: {', '.join(sorted(unknown))}")
    return action, replace(current, **fields).validated()


def load_gtk():
    try:
        import gi
        gi.require_version("Gtk", "4.0")
        gi.require_version("Gdk", "4.0")
        gi.require_foreign("cairo")
        import cairo
        if not hasattr(cairo, "Region"):
            raise ImportError("PyCairo region support is unavailable")
        from gi.repository import Gdk, Gio, GLib, Gtk
    except (ImportError, ValueError) as error:
        raise RuntimeError("GTK4/PyGObject unavailable; try /usr/bin/python3") from error
    if Gtk.get_minor_version() < 8:
        raise RuntimeError("GTK 4.8 or newer is required for content-fit")
    if not Gtk.init_check() or Gdk.Display.get_default() is None:
        raise RuntimeError("No accessible graphical display")
    return Gdk, Gio, GLib, Gtk


def dbus_call(Gio, GLib, method, args=None, signature=None,
              destination=BUS_NAME, path=OBJECT_PATH, interface=INTERFACE):
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    parameters = GLib.Variant(signature, args) if signature else None
    result = connection.call_sync(destination, path, interface, method, parameters,
                                  None, Gio.DBusCallFlags.NONE, 2500, None)
    return result.unpack()


def battery_from_sysfs(root=Path("/sys/class/power_supply")):
    try:
        supplies = list(root.iterdir())
        batteries = [s for s in supplies if (s / "type").read_text().strip() == "Battery"]
        if not batteries:
            return False
        adapters = [s for s in supplies if (s / "type").read_text().strip() == "Mains"]
        if adapters:
            return not any((s / "online").read_text().strip() == "1" for s in adapters)
        return any((s / "status").read_text().strip() == "Discharging" for s in batteries)
    except (OSError, ValueError):
        return False


def on_battery(Gio, GLib):
    try:
        value = dbus_call(Gio, GLib, "Get", ("org.freedesktop.UPower", "OnBattery"),
                          "(ss)", "org.freedesktop.UPower", "/org/freedesktop/UPower",
                          "org.freedesktop.DBus.Properties")[0]
        return value.unpack() is True
    except Exception:
        return battery_from_sysfs()


def monitor_list(Gdk):
    monitors = []
    for index, monitor in enumerate(Gdk.Display.get_default().get_monitors()):
        geometry = monitor.get_geometry()
        connector = monitor.get_connector() or f"monitor-{index}"
        monitors.append({"id": connector, "name": connector,
                         "width": geometry.width, "height": geometry.height})
    return monitors


def diagnostics():
    desktop = os.environ.get("XDG_CURRENT_DESKTOP", "unknown")
    session = os.environ.get("XDG_SESSION_TYPE", "unknown").lower()
    result = {"desktop": desktop, "session": session, "backend": "unsupported",
              "available": False, "message": "Unsupported desktop session", "monitors": []}
    try:
        Gdk, Gio, GLib, Gtk = load_gtk()
        result["monitors"] = monitor_list(Gdk)
        if not result["monitors"]:
            raise RuntimeError("No connected monitors")
        display_type = Gdk.Display.get_default().__gtype__.name
        if "Wayland" in display_type and session == "x11":
            raise RuntimeError("XDG_SESSION_TYPE and GTK display backend disagree")
        if "X11" in display_type and session == "wayland":
            raise RuntimeError("GTK selected XWayland; a native Wayland renderer is required")
        if "GNOME" in desktop.upper() and session == "wayland":
            result["backend"] = "gnome-shell"
            info = dbus_call(Gio, GLib, "GetExtensionInfo", (EXTENSION_UUID,), "(s)",
                             "org.gnome.Shell.Extensions", "/org/gnome/Shell/Extensions",
                             "org.gnome.Shell.Extensions")[0]
            if info.get("state") != 1:
                raise RuntimeError("Wall-in GNOME extension is not enabled. Install and enable it "
                                   "explicitly; a first install on Wayland may require logout/login.")
            probe = json.loads(dbus_call(Gio, GLib, "Probe")[0])
            if probe.get("version") != PROTOCOL_VERSION or not probe.get("supported"):
                raise RuntimeError(probe.get("message", "Incompatible Wall-in extension"))
            shell_monitors = {m["id"] for m in probe.get("monitors", [])}
            if not all(m["id"] in shell_monitors for m in result["monitors"]):
                raise RuntimeError("GTK and GNOME monitor connectors disagree")
            result["message"] = "GNOME 50 background bridge enabled; each launch verifies attachment"
        elif session == "x11" and "X11" in display_type:
            result["backend"] = "x11-desktop"
            from x11 import check_support
            check_support()
            result["message"] = "Experimental EWMH desktop window; not verified on this host"
        elif session == "wayland" and "Wayland" in display_type:
            result["backend"] = "layer-shell"
            import gi
            gi.require_version("Gtk4LayerShell", "1.0")
            from gi.repository import Gtk4LayerShell
            if not Gtk4LayerShell.is_supported():
                if "KDE" in desktop.upper() or "PLASMA" in desktop.upper():
                    raise RuntimeError("Use the experimental Plasma wallpaper package; "
                                       "this session has no supported GTK layer-shell protocol")
                raise RuntimeError("Compositor does not support GTK4 layer-shell")
            result["message"] = "Experimental native background layer; not verified on this host"
        else:
            return result
        if Gtk.MediaFile.new().__gtype__.name == "GtkNoMediaFile":
            raise RuntimeError("GTK has no media backend; install GTK's GStreamer media support")
        result["available"] = True
    except Exception as error:
        result["message"] = str(error)
    return result


class Renderer:
    def __init__(self, settings, backend):
        self.Gdk, self.Gio, self.GLib, self.Gtk = load_gtk()
        self.settings = settings
        self.backend = backend
        self.token = uuid.uuid4().hex
        self.app = self.Gtk.Application(application_id=APP_ID,
                                        flags=self.Gio.ApplicationFlags.NON_UNIQUE)
        self.app.connect("activate", self.activate)
        self.window = None
        self.media = None
        self.scene = None
        self.playback_media = None
        self.playback_paused = None
        self.last_media_timestamp = None
        self.last_media_progress = time.monotonic()
        self.media_recovery_attempts = 0
        self.paused = False
        self.battery = False
        self.fullscreen = False
        self.last_battery_check = 0
        self.ready = False
        self.registered = False
        self.stopping = False
        self.exit_code = 0
        self.started = time.monotonic()
        self.control_buffer = bytearray()

    def bridge(self, method, args=None, signature=None):
        return dbus_call(self.Gio, self.GLib, method, args, signature)

    def find_monitor(self, identifier):
        for index, monitor in enumerate(self.Gdk.Display.get_default().get_monitors()):
            if (monitor.get_connector() or f"monitor-{index}") == identifier:
                return monitor
        raise RuntimeError(f"Monitor disconnected or unknown: {identifier}")

    def activate(self, app):
        try:
            Gtk = self.Gtk
            monitor = self.find_monitor(self.settings.monitor)
            geometry = monitor.get_geometry()
            if self.backend == "gnome-shell":
                accepted = self.bridge("Register", (self.token, os.getpid(), self.settings.monitor),
                                       "(sus)")[0]
                if not accepted:
                    raise RuntimeError("GNOME extension refused renderer registration")
                self.registered = True
            self.window = Gtk.ApplicationWindow(application=app, title=f"Wall-in:{self.token}")
            self.window.set_decorated(False)
            self.window.set_focusable(False)
            self.window.set_can_target(False)
            self.window.set_default_size(geometry.width, geometry.height)
            if self.backend == "gnome-shell":
                # Do not flash a normal window while Shell matches its metadata.
                self.window.set_opacity(0.01)
            self.window.connect("close-request", self.on_close)
            self.picture = Gtk.Picture(can_shrink=True, hexpand=True, vexpand=True)
            self.picture.set_size_request(geometry.width, geometry.height)
            self.picture.set_can_target(False)
            self.window.set_child(self.picture)
            css = Gtk.CssProvider()
            css.load_from_data(b"window { background: black; }")
            Gtk.StyleContext.add_provider_for_display(self.Gdk.Display.get_default(), css,
                                                       Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
            if self.backend == "layer-shell":
                import gi
                gi.require_version("Gtk4LayerShell", "1.0")
                from gi.repository import Gtk4LayerShell as Layer
                Layer.init_for_window(self.window)
                Layer.set_namespace(self.window, APP_ID)
                Layer.set_layer(self.window, Layer.Layer.BACKGROUND)
                Layer.set_keyboard_mode(self.window, Layer.KeyboardMode.NONE)
                Layer.set_exclusive_zone(self.window, -1)
                Layer.set_monitor(self.window, monitor)
                for edge in (Layer.Edge.TOP, Layer.Edge.BOTTOM, Layer.Edge.LEFT, Layer.Edge.RIGHT):
                    Layer.set_anchor(self.window, edge, True)
            self.window.realize()
            surface = self.window.get_surface()
            import cairo
            surface.set_input_region(cairo.Region())
            if self.backend == "x11-desktop":
                from x11 import configure_window
                configure_window(surface, geometry)
            if self.settings.pause_on_battery:
                self.battery = on_battery(self.Gio, self.GLib)
                self.last_battery_check = time.monotonic()
            self.configure(self.settings)
            # set_visible maps without requesting activation through Gtk.Window.present().
            self.window.set_visible(True)
            self.GLib.timeout_add(200, self.check_state)
            self.GLib.unix_signal_add(self.GLib.PRIORITY_DEFAULT, signal.SIGTERM, self.on_signal)
            self.GLib.unix_signal_add(self.GLib.PRIORITY_DEFAULT, signal.SIGINT, self.on_signal)
            os.set_blocking(0, False)
            self.GLib.io_add_watch(0, self.GLib.IO_IN | self.GLib.IO_HUP | self.GLib.IO_ERR,
                                   self.read_control_ready)
        except Exception as error:
            self.fail(error)

    def configure(self, settings):
        monitor = self.find_monitor(settings.monitor)
        if settings.monitor != self.settings.monitor:
            if self.backend == "gnome-shell":
                if not self.bridge("Configure", (self.token, settings.monitor), "(ss)")[0]:
                    raise RuntimeError("GNOME extension refused monitor change")
            elif self.backend == "layer-shell":
                from gi.repository import Gtk4LayerShell
                Gtk4LayerShell.set_monitor(self.window, monitor)
            elif self.backend == "x11-desktop":
                from x11 import configure_window
                configure_window(self.window.get_surface(), monitor.get_geometry())
            geometry = monitor.get_geometry()
            self.window.set_default_size(geometry.width, geometry.height)
            self.picture.set_size_request(geometry.width, geometry.height)
        settings = settings.validated()
        if settings.kind != "scene":
            self.picture.set_content_fit({"cover": self.Gtk.ContentFit.COVER,
                                          "contain": self.Gtk.ContentFit.CONTAIN,
                                          "fill": self.Gtk.ContentFit.FILL}[settings.fit])
        changed_media = (settings.kind == "scene" or settings.file != self.settings.file
                         or settings.kind != self.settings.kind
                         or (self.media is None and not self.picture.get_paintable()))
        if changed_media:
            if self.scene:
                self.scene.stop()
                self.scene = None
            if self.media:
                self.media.pause()
                self.media.clear()
                self.media = None
            if settings.kind == "scene":
                from native_scene import CubeArea, load
                self.scene = CubeArea(self.Gtk, self.GLib, load(settings.file), self.fail)
                self.window.set_child(self.scene.area)
            else:
                self.window.set_child(self.picture)
                file = self.Gio.File.new_for_path(settings.file)
                if settings.kind == "image":
                    self.picture.set_paintable(self.Gdk.Texture.new_from_file(file))
                else:
                    self.media = self.Gtk.MediaFile.new_for_file(file)
                    self.media.set_loop(True)
                    self.picture.set_paintable(self.media)
        if self.media:
            self.media.set_volume(settings.volume / 100.0)
            self.media.set_muted(settings.muted)
        self.settings = settings
        self.update_playback()
        if changed_media:
            self.started = time.monotonic()
            self.last_media_timestamp = None
            self.last_media_progress = self.started
            self.media_recovery_attempts = 0

    def maintain_media(self):
        if not self.media or self.playback_paused or not self.ready or not self.media.is_prepared():
            self.last_media_timestamp = None
            self.last_media_progress = time.monotonic()
            self.media_recovery_attempts = 0
            return
        now = time.monotonic()
        timestamp = self.media.get_timestamp()
        duration = self.media.get_duration()
        if timestamp != self.last_media_timestamp:
            self.last_media_timestamp = timestamp
            if not self.media_recovery_attempts or 0 < timestamp < duration - 1_000_000:
                self.last_media_progress = now
                self.media_recovery_attempts = 0
        elif duration > 0 and now - self.last_media_progress > 3:
            if self.media_recovery_attempts >= 3:
                raise RuntimeError("Video stopped advancing after three playback recovery attempts")
            self.media_recovery_attempts += 1
            self.last_media_progress = now
            self.media.seek(0)
            self.media.play()

    def update_playback(self):
        paused = (self.paused or (self.settings.pause_on_battery and self.battery) or
                  (self.settings.pause_on_fullscreen and self.fullscreen))
        if self.media and (self.media is not self.playback_media or paused != self.playback_paused):
            self.media.pause() if paused else self.media.play()
        if getattr(self, "scene", None):
            self.scene.set_paused(paused)
        self.playback_media = self.media
        self.playback_paused = paused

    def check_state(self):
        if self.stopping:
            return False
        try:
            self.find_monitor(self.settings.monitor)
            if time.monotonic() - self.last_battery_check >= 2:
                self.battery = on_battery(self.Gio, self.GLib) if self.settings.pause_on_battery else False
                self.last_battery_check = time.monotonic()
            attached = True
            status = {}
            if self.backend == "gnome-shell":
                status = json.loads(self.bridge("Status", (self.token,), "(s)")[0])
                if status.get("error"):
                    raise RuntimeError(status["error"])
                attached = status.get("attached", False)
                self.fullscreen = status.get("fullscreen", False) is True
            self.update_playback()
            if self.media and self.media.get_error():
                raise RuntimeError(self.media.get_error().message)
            prepared = (self.scene.rendered if getattr(self, "scene", None) else
                        self.media is None or self.media.is_prepared())
            if self.media and prepared and (self.media.get_intrinsic_width() <= 0
                                            or self.media.get_intrinsic_height() <= 0):
                raise RuntimeError("Selected video has no decodable video track")
            if (not prepared or not attached or not self.window.get_mapped()) and time.monotonic() - self.started > 15:
                raise RuntimeError(
                    "Timed out waiting for media preparation or background attachment "
                    f"(prepared={prepared}, attached={attached}, mapped={self.window.get_mapped()}, "
                    f"bridge={status})"
                )
            if not self.ready and prepared and attached and self.window.get_mapped():
                self.window.set_opacity(1)
                self.ready = True
                print(json.dumps({"event": "ready"}), flush=True)
            self.maintain_media()
        except Exception as error:
            self.fail(error)
            return False
        return True

    def read_control_ready(self, _source, condition):
        try:
            chunk = os.read(0, 65536)
            if not chunk:
                self.stop()
                return False
            self.control_buffer.extend(chunk)
            while b"\n" in self.control_buffer:
                line, _, remaining = self.control_buffer.partition(b"\n")
                if len(line) > MAX_COMMAND_BYTES:
                    raise ValueError("command exceeds 64 KiB")
                self.control_buffer = bytearray(remaining)
                text = line.decode("utf-8")
                if text.strip():
                    self.control(text)
            if len(self.control_buffer) > MAX_COMMAND_BYTES:
                raise ValueError("command exceeds 64 KiB")
            if condition & (self.GLib.IO_HUP | self.GLib.IO_ERR):
                self.stop()
                return False
            return not self.stopping
        except BlockingIOError:
            return True
        except Exception as error:
            self.fail(error)
            return False

    def control(self, line):
        if self.stopping:
            return False
        try:
            action, settings = parse_command(line, self.settings)
            if action == "stop":
                self.stop()
            elif action == "configure":
                self.configure(settings)
            else:
                self.paused = action == "pause"
                self.update_playback()
        except Exception as error:
            self.fail(error)
        return False

    def on_close(self, _window):
        if not self.stopping:
            if self.backend == "gnome-shell":
                return True
            self.fail(RuntimeError("Renderer surface was closed by the desktop"))
        return True

    def on_signal(self):
        self.stop()
        return False

    def fail(self, error):
        print(f"Wall-in renderer: {error}", file=sys.stderr, flush=True)
        self.exit_code = 1
        self.stop()
        return False

    def stop(self):
        if self.stopping:
            return False
        self.stopping = True
        if getattr(self, "scene", None):
            self.scene.stop()
        if self.media:
            self.media.pause()
            self.media.clear()
        if self.window:
            self.window.destroy()
        if self.registered:
            try:
                self.bridge("Unregister", (self.token,), "(s)")
            except Exception:
                pass
            self.registered = False
        self.app.quit()
        return False

    def run(self):
        status = self.app.run([sys.argv[0]])
        return self.exit_code or status


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--diagnostics", action="store_true")
    parser.add_argument("--file")
    parser.add_argument("--kind", choices=("image", "video", "scene"), default="video")
    parser.add_argument("--monitor")
    parser.add_argument("--fit", choices=("cover", "contain", "fill"), default="cover")
    parser.add_argument("--volume", type=float, default=0)
    parser.add_argument("--muted", action="store_true")
    parser.add_argument("--no-pause-on-battery", action="store_true")
    parser.add_argument("--no-pause-on-fullscreen", action="store_true")
    args = parser.parse_args(argv)
    if args.diagnostics:
        print(json.dumps(diagnostics()), flush=True)
        return 0
    try:
        settings = Settings(args.file, args.kind, args.monitor, args.fit,
                            args.volume, args.muted, not args.no_pause_on_battery,
                            not args.no_pause_on_fullscreen).validated()
        capability = diagnostics()
        if not capability["available"]:
            raise RuntimeError(capability["message"])
        return Renderer(settings, capability["backend"]).run()
    except Exception as error:
        print(f"Wall-in renderer: {error}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    sys.exit(main())
