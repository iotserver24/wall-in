# Wall-in

> **Status: working prototype, development abandoned.** Local video wallpapers, a limited live clock widget, and a built-in rotating cube worked in testing, but the GNOME Wayland renderer still appeared as a separate app window in the overview. I gave up on this approach and uninstalled it from my desktop. This repository is published as-is, without a promise of maintenance or full Wallpaper Engine compatibility.

A Linux live-wallpaper prototype with a **Rust core**, a React/TypeScript management interface, and small desktop-specific integrations. This is an early implementation, **not full Wallpaper Engine feature parity**.

The core manages local imports, the wallpaper library, persistent preferences and supervised renderer processes. A Python GTK4/GStreamer helper handles native image/video rendering. GNOME integration uses a JavaScript Shell extension; the experimental Plasma package uses QML. An optional Python GTK/WebKit window hosts the same management interface without exposing native APIs to wallpaper content.

## Install for this user

Build and install the app from this checkout:

```sh
make build
python3 scripts/install-user.py install
```

Launch **Wall-in** from the Applications menu or run `wall-in`. The launcher starts the user service and opens the management interface in a dedicated Chrome app window when Chrome is available, falling back to the default browser. `wall-in --help` accesses the command-line interface. The installed copy lives under `~/.local/share/wall-in/app`; your wallpaper library remains in `~/.local/share/wall-in` and is preserved by reinstalling. The service starts at graphical login and continues running when the management window closes. Check the installation with `python3 scripts/install-user.py status` and `systemctl --user status wall-in.service`.

The optional GTK/WebKit window can be tried with `wall-in window`; on some systems WebKitGTK does not render the interface, so the Applications-menu entry uses Chrome or the default browser instead.

## Run the application

Requirements: Rust/Cargo supporting edition 2024 and let-chains (Rust 1.88+), Node.js 20+, npm, Python 3 with PyGObject/PyCairo, GTK 4.8+, and a GTK GStreamer media backend with codecs for your files. The optional native management window also needs WebKitGTK 6.0. Desktop playback additionally requires the appropriate integration described below.

```sh
# From this directory:
make build
make run
```

Open **http://127.0.0.1:3977** in your browser. For a native management window, run this in another terminal while the service is running:

```sh
make window
```

Closing the management window does not stop the separately running service. Stop a wallpaper using the playback bar. Stop the service with Ctrl+C in its terminal; it cleans up its renderer while retaining the last successful assignment and paused state for the next start. Explicitly stopping playback or removing its wallpaper clears that assignment. Restoration errors are reported rather than treated as successful playback. The service listens on loopback only and refuses non-loopback bind addresses.

The first launch intentionally has an empty library. **Import wallpaper** accepts an absolute path to an image, video, `project.json`, or a local Wallpaper Engine project directory. Imported files stay in their original locations; removing an item from the library does not delete its source. Keep source files accessible after importing.

**Scan Steam** scans local Workshop folders discovered from Steam library manifests, including secondary libraries. It does not download, subscribe to or publish Workshop items. Existing projects are deduplicated. Wallpaper Engine scene, web and application projects can be cataloged but cannot currently be rendered. A separate Wall-in-native `.wallin-scene.json` format renders one animated 3D cube; it is not Wallpaper Engine scene compatibility.

## Desktop integration

The app shows live diagnostics on the **Compatibility** page. Apply remains disabled when the necessary integration is missing.

### GNOME 50 on Wayland

The extension is never installed merely by launching the app. Install and enable it explicitly if you choose to test it. Video playback and the limited clock widget were observed on GNOME 50 Wayland, but the renderer still appeared as a separate window in GNOME's overview after restarts. That unresolved issue was why development stopped.

```sh
/usr/bin/python3 integrations/install-gnome-extension.py install
# If GNOME cannot discover the extension, log out and log back in first.
/usr/bin/python3 integrations/install-gnome-extension.py enable
/usr/bin/python3 integrations/renderer.py --diagnostics
```

Then use the refresh button (or reload the management page) to refresh desktop diagnostics, choose your display, import an image/video, and apply it. A first-time Wayland extension installation can require a new login session. There is no need to change your existing static wallpaper.

The extension targets **GNOME 50**, not every historical GNOME release. An HEVC video was visibly placed behind a normal window, with playback status and pause/resume confirmed. The renderer's unresolved GNOME overview window means this is not a polished background-only application. Fullscreen, lock/unlock, multi-monitor behavior and performance under gaming loads remain unverified.

To remove Wall-in's managed extension:

```sh
/usr/bin/python3 integrations/install-gnome-extension.py uninstall
```

### Other desktops

- **Compatible Wayland compositors:** experimental background-layer implementation requiring the Gtk4LayerShell GI binding and compositor protocol support.
- **X11:** experimental desktop-window integration; the window manager must support the necessary EWMH capabilities. This is not an XWayland workaround for GNOME.
- **KDE Plasma 6:** experimental standalone wallpaper package and configuration adapter. It is not yet wired into the Rust service as an acknowledged, supervised playback backend.

See [desktop integration](docs/desktop-integration.md) for architecture, prerequisites, installation/removal, limitations, and manual acceptance checks. These other environments have not been visually verified on the current GNOME host.

## What this version does

- Local image/video imports and Wallpaper Engine project classification.
- Steam library discovery and local video-project imports.
- Search, sorting, type filters, favorites and source-preserving removal.
- Image/video preview, renderer apply/pause/resume/stop, fit, volume and monitor selection.
- Persistent settings, timed saved-playlist rotation and shuffle while playing.
- A reversible managed systemd user-service option to start Wall-in at graphical login and restore the last successfully applied wallpaper.
- Native video pause policies for battery power and GNOME fullscreen windows; behavior depends on the desktop environment and has not been visually verified on this host.
- Responsive library, favorites, playlist, display, widgets, settings and compatibility pages. Rainmeter-style static Text and bounded `Measure=Time` clock skins can be imported disabled, positioned and managed. A supported clock was observed updating on the GNOME desktop. Image, Shape, other measures and plugins are not rendered.
- A versioned Wall-in-native rotating 3D cube scene with fixed built-in shaders and colors. Import `examples/scenes/RotatingCube.wallin-scene.json` to try it; Wallpaper Engine `.pkg` and project scenes remain unsupported.
- Explicit errors for unavailable integrations, unsafe paths, missing media and renderer failures.

**Important limitations:** one supervised wallpaper/display at a time. Rotation starts only while a queued wallpaper is playing; time-of-day rules, transitions and multi-monitor assignments remain unfinished. The optional autostart unit can restore a successfully applied wallpaper on restart; when integration is unavailable, a supported wallpaper can instead be explicitly scheduled with **Use after next login** for a later startup attempt. GNOME extension discovery may require a new login. Wallpaper Engine scene/web runtimes, arbitrary 3D assets/materials, audio-reactive effects, the visual scene editor and online Workshop integration remain unfinished. Browser preview support depends on the browser's codecs: an HEVC test video decoded in GStreamer but not in headless Chrome. Unavailable features are distinguished from working controls.

The complete remaining scope is tracked in [feature coverage](docs/implementation-status.md). Test evidence and environment-specific verification are tracked in [verification](docs/verification.md).

## Data and configuration

- Library/settings: `$WALL_IN_DATA_DIR`, otherwise `$XDG_DATA_HOME/wall-in` or `~/.local/share/wall-in`.
- `WALL_IN_ROOT`: override the directory containing `apps/desktop/dist` and `integrations` when distributing the binary separately from the source tree.
- `WALL_IN_PYTHON`: override the renderer interpreter; it defaults to `/usr/bin/python3` when present, avoiding virtual environments without system GTK bindings.
- `cargo run -- serve --port 3978`: use another local port.
- `cargo run -- serve --data-dir /absolute/path`: use a separate library.

Imported project resources are confined to their project directory; traversal and escaping symlinks are rejected. Web/application content is not executed. The management service validates Host and Origin headers, rejects cross-site requests, and does not expose permissive CORS access. It is a local desktop service, not a multi-user remote server.

## Development and tests

```sh
make test
make check
# Optional live frontend development; keep the Rust service on port 3977:
cd apps/desktop && npm run dev
```

For repeatable browser checks, use a separate test library. These commands create original test media and a fake local Steam library, never download Workshop content:

```sh
make fixtures
HOME="$PWD/tests/generated/home" \
  WALL_IN_DATA_DIR="$PWD/.wall-in-test-data/browser" \
  target/debug/wall-in serve
# In another terminal:
make browser-test
```

Browser tests use a fresh headless Chrome instance (`/usr/bin/google-chrome`, override with `CHROME_BIN`) and interact with the real Rust service. `WALL_IN_TEST_URL` selects a different test port. They reject cleanup of library entries outside the generated fixture directory. Screenshots go to `artifacts/`. A second browser suite starts an isolated service with a **test-only renderer protocol fixture** to check controls, restart restoration and crash recovery. That suite verifies service/interface coordination, not actual desktop output.

Cargo is limited to two compiler jobs via `.cargo/config.toml`. This limits parallelism, **not RAM by itself**.

## Layout

```text
src/main.rs                 Rust CLI and local server startup
crates/core/                Library, imports, persistence, HTTP API, supervision
apps/desktop/               React/TypeScript interface
integrations/renderer.py    GTK4 native renderer
integrations/gnome/         GNOME Shell background bridge
integrations/plasma/        Experimental Plasma wallpaper package
scripts/window.py          Optional native management window
tests/                     Generated fixtures and real-browser checks
docs/                      Support details and remaining scope
```

Wall-in is independent of Wallpaper Engine and Valve. Access to third-party Workshop projects remains subject to Steam's requirements and the projects' licenses.
