# Desktop integration

## Capability and verification status

Wall-in does not treat an ordinary always-below window as a Wayland wallpaper.
The renderer refuses to start unless it finds a suitable compositor integration.
All installation and activation is explicit; diagnostics do not install extensions,
change wallpaper settings, create windows, or enable anything.

| Session | Implementation | Status |
| --- | --- | --- |
| GNOME 50 Wayland | GTK4 renderer surface cloned into Shell background actors by an optional extension | Video playback, limited clock output and a rotating cube were observed, but the renderer still appeared as an app window in overview after repeated logins. The prototype was uninstalled; this remains unresolved. |
| Other Wayland compositors | GTK4 layer-shell `BACKGROUND` surface, when the binding **and** compositor protocol are present | Experimental; not exercised on this host. |
| X11 | GTK4 window with EWMH `DESKTOP`, skip-taskbar/pager, sticky and below hints, empty input region and no input focus | Experimental; requires the WM to advertise the relevant EWMH capabilities. Not an XWayland fallback. |
| Plasma 6 wallpaper package | QML `WallpaperItem`, `Image`, `MediaPlayer`, `VideoOutput`, persisted KConfig state | Experimental; separate package/configuration adapter, not advertised as a supervised renderer backend. Plasma's runtime module is absent on this host. |

Development-host checks found GTK 4.22, PyGObject/PyCairo, GTK's built-in
GStreamer media backend, and a 1920×1080 logical display. GNOME did not support
the installed Gtk4LayerShell binding's compositor protocol. During testing, the
managed `wall-in@r3ap3r` extension was enabled, a video apply returned playing,
and desktop capture showed video behind a normal window. A later login also
confirmed a live Text/Time clock widget. The extension and service were then
uninstalled because the renderer remained visible in GNOME overview.

`available` means prerequisites and a live compatible bridge are present, not
that every possible media codec or extension combination has been tested. Media
preparation, decoder errors, and GNOME surface attachment are checked at launch.

## Renderer contract for the Rust supervisor

Use `/usr/bin/python3 integrations/renderer.py` where available. A Python from a
virtual environment may lack system GI modules; that interpreter must report
unavailable rather than silently falling back to a normal window.

- Probe with `--diagnostics`. Stdout contains one JSON object with exactly
  `desktop`, `session`, `backend`, `available`, `message`, and `monitors`.
  Each monitor contains `id`, `name`, `width`, and `height`. Connector IDs are
  opaque strings; dimensions are logical, not necessarily physical pixels.
  A completed probe exits zero even when `available` is false.
- Launch with `--file PATH --kind image|video --monitor ID --fit cover|contain|fill
  --volume N [--muted] [--no-pause-on-battery] [--no-pause-on-fullscreen]`.
  The path must be a readable local regular file. Volume is **0–100**, not 0–1.
  Default volume is zero. Both pause policies default to enabled; Rust passes
  each `--no-pause-*` flag only when the corresponding saved setting is false.
  Image decoding uses `Gdk.Texture`; video decoding/looping uses `Gtk.MediaFile`
  as the `Gtk.Picture` paintable.
- Start one child per monitor and keep its stdin pipe open. GNOME allows one
  registered renderer per monitor. Configure the existing child or stop it before
  launching its replacement; a second simultaneous registration is rejected.
- Stdout emits exactly one `{"event":"ready"}` after the window is mapped, the
  image/video is prepared, and (on GNOME) the extension confirms a textured source
  actor and background clone. Readiness is not a substitute for visual testing.
- Send newline-delimited JSON on stdin: `{"action":"pause"}`,
  `{"action":"resume"}`, `{"action":"stop"}`, or, for example,
  `{"action":"configure","volume":25,"muted":false,"fit":"contain"}`.
  Configure can instead contain one `settings` object. Both forms support the
  fields `file`, `kind`, `monitor`, `fit`, `volume`, `muted`, `pause_on_battery`,
  and `pause_on_fullscreen`; omitted fields retain their values. Rust includes
  both policy booleans when configuring. Do not mix flat fields with a `settings`
  object.
- Pause/resume affect the actual media stream. They are harmless for images.
  Manual pause remains effective while either policy changes. Battery status is
  read from UPower, falling back to sysfs when unavailable; GNOME Shell reports
  visible fullscreen windows on the selected monitor. Policy-paused video resumes
  only when no enabled pause condition remains.
  Reconfiguration preserves the pause state, updates audio/fit immediately, and
  replaces the decoder when the media file or kind changes. Configure does not
  emit a second ready event or an invented playback-state acknowledgement.
- Invalid JSON, unknown fields/actions, invalid configuration, a disconnected
  monitor, decoder failure, lost bridge, or a 15-second preparation/attachment
  timeout produces stderr output and nonzero exit. Commands are capped at 64 KiB.
  Audio-only files are rejected as video wallpapers.
- Stop, stdin EOF, SIGINT, and SIGTERM cleanly release media, destroy the surface,
  unregister the GNOME source, and exit zero. Unexpected desktop closure is an
  error. Drain stderr continuously and use child exit, not a UI boolean, as the
  failure signal. A killed child loses its D-Bus registration automatically.
- Relative paths are resolved in the helper's working directory. Bundled
  deployments must ship the entire `integrations` directory, including `x11.py`.

The helper never starts GNOME Shell, restarts the session, installs packages,
sets a static wallpaper, or auto-enables its extension.

## GNOME installation and removal

Run these commands yourself only when desktop modification is authorized:

1. `/usr/bin/python3 integrations/install-gnome-extension.py install`
2. If GNOME does not discover the new extension, **log out and back in**. Wayland
   cannot reload the running Shell with Alt+F2 `r`. First-time user-local extension
   discovery can require a new session even if files were installed successfully.
3. `/usr/bin/python3 integrations/install-gnome-extension.py enable`
4. `/usr/bin/python3 integrations/renderer.py --diagnostics`

`install` copies only Wall-in's extension to
`$XDG_DATA_HOME/gnome-shell/extensions/wall-in@r3ap3r` (default
`~/.local/share/gnome-shell/extensions/…`) and **does not enable it**. The separate
`enable` action is an explicit request to GNOME and checks its enabled list.
`status` is read-only. Existing installations are never overwritten; uninstall
an existing script-managed copy before upgrading, then start a fresh GNOME
session if necessary to discard cached ES modules.

Remove with `/usr/bin/python3 integrations/install-gnome-extension.py uninstall`.
It disables only Wall-in if active, verifies that state, and removes only its
marked directory. Unmanaged or symlinked extension directories are refused.
No existing installation is overwritten, no backup wallpaper needs restoring,
and no unrelated extension or desktop setting is changed. Removing files may
still require logout/login for GNOME to forget cached extension metadata.

### How the bridge works

The extension exports `io.wallin.Integration` at `/io/wallin/Integration` on the
existing `org.gnome.Shell` session-bus name. `Probe` returns protocol version 2
and physical connector IDs mapped to logical monitor indices. Authenticated
`Status` also reports fullscreen state for the registered monitor. Renderer windows
use the dedicated application ID `io.wallin.Renderer` and a random registration
nonce in their title. `Register` authenticates the caller's PID against D-Bus
credentials, and window matching requires that PID, nonce, and application ID.
Status/configuration/unregistration require the original bus sender.

The original Mutter window actor remains parented and realized but is hidden;
`Clutter.Clone` explicitly supports painting an unmapped/hidden source. Clones
are non-reactive children of GNOME's actual `Meta.BackgroundActor` objects,
including backgrounds created for the overview and workspace transitions.
This is the Shell background layer, **not** a normal window merely lowered under
other windows. GTK also supplies an empty surface input region and maps without
requesting activation. Its initial opacity is 0.01 to ensure Wayland allocates a nonzero source surface; the bridge hides the source during attachment, then clones it into the background. Starting at zero opacity produced a 0×0 source and timed out on this host. The bridge corrects accidental renderer focus.

GNOME 50's native `Meta.Window.hide_from_window_list()` removed the original from
the normal window list, but did not reliably remove its overview preview on the
actual desktop. The source is sticky across workspaces. A narrowly scoped workspace-animation filter prevents GNOME from
independently cloning the hidden source as an ordinary sticky window. Mapping
animations are skipped only for authenticated renderer windows, and visible
notifications re-hide their originals. A background-factory hook attaches clones
to replacement/overview/workspace background actors; already-existing background
actors are also discovered when the extension is enabled.

Unredirect is inhibited while registered renderers exist, then balanced on the
last removal/disable so fullscreen direct scanout cannot bypass the clone path.
This has a performance/power tradeoff; it is not a claim of zero-copy playback.
Monitor changes rebuild clones, and monitor removal is reported as an error.
Extension disable destroys clones, closes its renderer windows, disconnects
signals, restores patched Shell methods, and releases the unredirect inhibition.
The normal static GNOME wallpaper was never changed and becomes visible again.

The metadata intentionally supports **GNOME 50 only**. Private Shell JavaScript
APIs must be reviewed before adding another major version. Default user-session
extension behavior means locking the session can disable the bridge and close
the renderer; this is not a lock-screen wallpaper implementation. The supervisor
must handle a lost bridge and should not repeatedly launch when diagnostics say
unavailable. Compatibility with tiling, blur, animation, or alternative switcher
extensions requires real desktop testing; none were disabled to make tests pass.

The installed Shell resources inspected were `layout.js`, `background.js`,
`workspace.js`, `workspaceAnimation.js`, `windowManager.js`, and `altTab.js` from
`/usr/lib64/gnome-shell/libshell-18.so`. Native API behavior was cross-checked with
[Mutter 50.5 window.c](https://gitlab.gnome.org/GNOME/mutter/-/blob/50.5/src/core/window.c)
and [ClutterClone](https://gitlab.gnome.org/GNOME/mutter/-/blob/50.5/clutter/clutter/clutter-clone.c).

## Experimental non-GNOME paths

### Layer-shell and X11

These use the same renderer CLI and stdin protocol. Layer-shell requires the
`Gtk4LayerShell-1.0` GI binding and a native compositor supporting the protocol;
GNOME deliberately does not fall through to it. The surface is assigned to the
requested monitor, anchored on all four sides, placed on `BACKGROUND`, given no
keyboard interactivity or pointer region, and does not reserve desktop space.

X11 requires `GdkX11-4.0`, libX11, and advertised desktop-window EWMH capabilities.
Properties and input hints are set before the first map, including
`_NET_WM_WINDOW_TYPE_DESKTOP`, `_NET_WM_STATE_BELOW`, skip-taskbar/pager, and
`_NET_WM_DESKTOP = 0xffffffff`. Diagnostics read existing atoms/properties without
creating them. The compositor/WM still decides how to honor EWMH; another
wallpaper manager may cover the surface. Neither this path nor mixed-DPI X11
monitor movement was visually validated on the GNOME Wayland host.

### Plasma package and persisted state

With Plasma 6 and Qt Multimedia installed, an explicitly authorized user can
install `integrations/plasma/io.wallin.wallpaper` using
`kpackagetool6 --type Plasma/Wallpaper --install integrations/plasma/io.wallin.wallpaper`,
then select **Wall-in Local Media (Experimental)** in wallpaper settings.
This is a real Plasma wallpaper item, not a floating player. The configuration
page selects a local file, image/video kind, cover/contain/fill, volume, mute,
and pause. These persist per wallpaper containment in KConfig and bind to actual
image/video/audio playback. File/decoder errors remain visible and are logged.
Network URLs are not accepted. Remove using
`kpackagetool6 --type Plasma/Wallpaper --remove io.wallin.wallpaper` after selecting
another wallpaper; previous wallpaper selection is not automatically restored.

`integrations/plasma-control.py` is an experimental one-shot adapter for that
persisted state. It accepts `--screen N --action configure|pause|resume|stop` and,
for configure, `--settings` containing a JSON object with the common renderer
fields except `monitor`. `N` is Plasma's screen index, **not** a GDK connector ID.
It refuses to change a different selected wallpaper unless `--activate` is
explicitly passed. All values are JSON-encoded into the Plasma scripting call.
Pause/resume persist `Paused`; stop clears the media file and pauses playback.
The adapter uses `org.kde.PlasmaShell.evaluateScript`; if that interface is absent
or restricted it fails with nonzero exit. It emits `event: "configured"`, **not**
`ready`, because writing configuration does not prove frames were displayed.
It is not automatically selected by the Rust renderer protocol. Plasma activity
selection, acknowledgement of actual playback, and installation detection need
validation on a real Plasma session before advertising supervised support.

## Validation and authorized desktop acceptance

Non-mutating checks:

- `/usr/bin/python3 -m unittest discover -s integrations/tests -v`
- `node --test integrations/tests/gnome-extension.test.mjs`
- `/usr/bin/python3 -m py_compile integrations/renderer.py integrations/x11.py integrations/install-gnome-extension.py integrations/plasma-control.py`
- `node --input-type=module --check < integrations/gnome/wall-in@r3ap3r/extension.js`
- `qmlformat -n` on each QML file checks parsing without rewriting files.
- `qmllint` on both QML files. On this host parsing succeeds, but full semantic
  validation of `WallpaperItem` is blocked by the absent
  `org.kde.plasma.plasmoid` import. Qt Multimedia and the Qt QML tools are present.

The Python tests cover import without GI/site packages, stdout/exit semantics,
strict protocol validation, actual control dispatch, readiness gating, timeout,
media rejection, capability failures, and reversible installation in temporary
directories. Node tests exercise the bridge against fake Shell actors: credential
checks, duplicate registration, real background parent selection, hidden source,
workspace filtering, new background lifecycle, monitor loss, and cleanup. These
are not compositor rendering tests.

After explicit GNOME installation/enablement, the parent/manual tester must:

1. Confirm diagnostics become available, then launch an image and a locally
   decodable video with stdin held open. Confirm ready and actual moving frames.
2. Open, move, maximize, and fullscreen normal windows: wallpaper stays behind
   them, panel/desktop input works, and the original renderer never appears as
   an interactive window or steals focus.
3. Check Alt+Tab, application switching, overview, and animated workspace changes:
   no renderer entry or normal-window clone, and backgrounds remain present.
4. Exercise pause/resume, all fits, audio controls, media replacement, multiple
   monitors if available, and unplug/replug/error handling.
5. Stop normally, terminate/kill the child, disable the extension, and lock/unlock.
   Verify cleanup, return to the untouched static wallpaper, no orphan window,
   no surviving task entry, and truthful supervisor error handling.
6. Inspect `journalctl --user -b` for Shell extension exceptions and the child's
   stderr for decoder failures. Do not call syntax/unit success proof of desktop
   playback; record these acceptance checks separately.
