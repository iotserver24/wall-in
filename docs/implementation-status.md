# Feature coverage

Wall-in is an independent Linux wallpaper application. It is not affiliated with Wallpaper Engine or Valve. Full Wallpaper Engine parity is the product goal, not the current compatibility claim.

This file tracks scope that must not disappear behind a working interface. Actual verification results belong in `docs/verification.md`.

## Implemented foundation

- Rust library and local management service.
- Image/video import and persistent library metadata.
- Local Wallpaper Engine project classification and supported video project imports.
- Steam library discovery, including additional Steam library folders.
- Favorites, search, type filters, preview, scaling, volume and playback controls.
- Durable restoration of the last successful wallpaper and paused state after service restart.
- Explicit platform/monitor diagnostics refresh without restarting the service.
- Saved playlist membership with timed sequential rotation and shuffle during active playback; additional scheduling rules remain unfinished.
- Managed systemd user autostart on future graphical login, restoring a previously successful assignment or attempting an explicitly scheduled wallpaper once desktop integration is available.
- Native pause on battery; GNOME bridge fullscreen status for pause on fullscreen. Live behavior still requires desktop verification.
- GNOME extension and external renderer; environment-specific desktop diagnostics. The user's HEVC video visibly played as a GNOME background and restored on a service restart, with pause/resume acknowledged. Broader compositor acceptance and performance remain open.
- Independent Rainmeter INI inspection and persistent native widget API/UI for static Text and bounded Time-measure clocks. A clock actor changed text each second in an isolated nested GNOME virtual monitor; the updated extension is not loaded into the user's current Shell until a later login. Image/Shape, other dynamic measures, plugins and actions are not rendered.
- A versioned Wall-in-native 3D scene with one rotating, depth-tested colored cube and fixed built-in shaders. This does not decode or render Wallpaper Engine scenes.
- Experimental Plasma, layer-shell and X11 integration paths.
- Browser management interface and optional GTK/WebKit native management window.

## Remaining work toward parity

| Feature group | Required work |
| --- | --- |
| Wallpaper Engine scene projects | Package format compatibility, material/shader conversion, particles, animation, scripting, resource compatibility and reference tests. |
| Web wallpapers | Isolated runtime, local resource policies, Wallpaper Engine JavaScript API compatibility, pointer interaction and permission controls. |
| Native scene editor | Versioned project format, asset management, layers, transforms, timeline, effects, undo/redo, export and reload. |
| Audio reactivity | PipeWire capture with explicit consent, spectrum data and renderer bindings. |
| Scheduling and playlists | Time-of-day rules, transitions, multiple independent playlists, hotplug behavior and policy precedence. Timed rotation and shuffle are implemented for a single active assignment. |
| Monitor configurations | True independent per-monitor players, spanning, duplicated playback, hotplug and mixed scaling verification. |
| Performance policies | Live verification of GNOME battery/fullscreen pauses; other compositors, frame-rate/quality limits, suspend/resume, memory and GPU measurements. |
| Desktop controls | Global shortcuts, tray/shell controls and a real graphical-login acceptance test of the managed autostart unit. |
| Rainmeter-style widgets | Live clock test in the user's GNOME session after a natural login; native Image/Shape rendering, other safe dynamic measures, more animations, interactivity and broader desktop integration. Windows DLL plugins and arbitrary script execution are deliberately excluded. |
| 3D wallpapers | Verify the native cube scene on the user's compositor; then add bounded assets, materials, cameras, lighting and effects separately. Native cube support is not Wallpaper Engine scene support. |
| Steam Workshop online access | Verify supported APIs, authentication, ownership requirements, downloading, subscriptions and publishing permissions. Local imports do not imply these work. |
| Windows application wallpapers | Native execution is unsupported. A future isolated, explicitly enabled Wine path requires a separate security and compatibility design. |
| Companion/mobile features | Separate platform implementation and transfer protocol; not included in the first Linux playback implementation. |
| Distribution | Reproducible release artifacts, distro dependencies, signed updates, translations, accessibility audit and Flatpak host integration. |

## Support labels

- **Implemented:** code exists for the described behavior; this alone does not mean it was exercised on every desktop.
- **Verified:** the named behavior passed on the exact environment recorded in the verification report.
- **Experimental:** an implementation exists but live desktop compatibility has not been established.
- **Unavailable:** missing prerequisites or unsupported behavior is surfaced to the user; it must not be reported as active.

No compatibility claim applies to every Workshop item. Third-party projects remain subject to their own licenses, required assets, runtime assumptions and Steam access rules.
