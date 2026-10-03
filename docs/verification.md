# Verification report

**Final outcome:** The prototype was uninstalled after repeated GNOME logins still showed its renderer as an app window in the overview. These checks record what worked during development; they do not establish a finished background-only desktop integration. The user service and extension are no longer installed on the test machine.

Wall-in is an independent early Linux wallpaper application, not full Wallpaper Engine or Rainmeter compatibility. Checks below were run on 2026-10-03 on GNOME Shell 50.5 Wayland with Rust 1.98, Node.js 22, GTK 4.22, PyGObject and GStreamer.

## Resource limit

The shared agent/build/test scope and installed `wall-in.service` have `MemoryMax=16000000000` and `MemorySwapMax=0`; Cargo uses at most two compiler jobs. This is a cap on these units, not on unrelated terminals or all system processes. Earlier foundation checks passed under the then-required 3.8 GB limit; no current live performance or gaming benchmark has been made under the 16 GB cap.

## Automated checks

- `cargo test --workspace --jobs 2 -- --test-threads=2`: 45 Rust tests passed (1 CLI, 4 core, 31 backend integration, 9 Rainmeter parser). Formatting, strict Clippy and build passed.
- `/usr/bin/python3 -m unittest discover -s integrations/tests -p 'test_*.py'`: 47 helper tests passed, including video-stall recovery, native-scene validation and GL drawing. Python compilation passed.
- `node --test integrations/tests/gnome-extension.test.mjs`: 24 GNOME bridge tests passed against fake Shell actors; this did not resolve the user's overview window.
- `npm run --prefix apps/desktop test`: 16 React tests passed. Production build passed.
- `WALL_IN_TEST_URL=http://127.0.0.1:3978 npm run --prefix tests browser`: passed with a separate test library and generated media. Existing library, favorites, playlists, displays, settings, compatibility, import/preview, error and source-removal flows were exercised in headless Chrome.
- `WALL_IN_TEST_URL=http://127.0.0.1:3978 node tests/widgets-browser.mjs`: imports a local INI fixture, checks path validation, static Text and Time-measure clock import, omission diagnostics, position persistence across routes/reload, unsupported current bridge error without persisting enablement, native 3D scene classification and compatibility labels, removal/cancellation, and all seven routes at desktop/mobile widths. Screenshots are in `artifacts/`.
- `npm run --prefix tests browser:playback`: uses its own isolated service and a test-only renderer protocol fixture; proves management coordination, not native desktop output.

The isolated browser service used a temporary library and port 3978. Its tests must never point at a real library on port 3977, since the management suite deletes entries.

## Live GNOME desktop

During testing, the `wall-in@r3ap3r` extension was enabled and the user service ran on `127.0.0.1:3977` under its 16 GB cap. A local HEVC test video was 3840×2160, 30 fps, 30 seconds. It fully decoded in GStreamer; headless Chrome cannot preview its codec, and the UI explains that limitation.

The initial renderer opacity was changed from zero to `0.01` after repeated GNOME attachment timeouts with a 0×0 Wayland source window. An explicit apply then returned `playing` in 2.42 seconds. A desktop capture showed the video's red-cloaked figure behind a normal terminal window, unlike the user's unchanged static GNOME wallpaper. Native pause/resume requests returned `paused` then `playing`, and restarting the managed service restored the saved video with `playing` and no error. A later user report exposed a serious gap in that check: after a 30-second clip ended, its image stopped advancing while the service still reported `playing`. Reapplying the video immediately restored visible movement. The renderer now watches its media timestamp while playback is expected and seeks to the beginning if it stalls for over three seconds; three failed recovery attempts terminate the helper so the service reports an error instead of stale `playing`. After deploying the watchdog, the same renderer remained `playing` for over a minute—more than two 30-second cycles—and desktop captures showed different video frames several seconds apart. This is a live short-duration loop check, not an overnight reliability or frame-rate benchmark. Earlier visual placement alone did not establish sustained playback.

After the user's next login, the updated GNOME extension reported widget support. Enabling the imported `Clock.ini` in the installed app displayed live time and date on the desktop over the rotating cube; separate desktop captures showed the seconds advancing. The service reported the clock enabled with no widget error, including after a later service restart. An isolated nested GNOME Shell with a 1280×720 virtual monitor also loaded the extension and displayed clock actor updates. Other Rainmeter features remain outside the supported subset.

The Wall-in-native `RotatingCube.wallin-scene.json` was imported into the real library and applied through the browser's **Apply wallpaper** control. The service reported playing and GNOME desktop captures showed a depth-rendered colored cube at distinct rotations behind a normal terminal window. The video was explicitly reapplied afterwards and visually confirmed. The cube is a built-in bounded scene, not a Wallpaper Engine `.pkg` renderer or evidence of general 3D-project compatibility. Headless browser checks covered its distinct native-scene label and kept Wallpaper Engine scene projects unsupported.

The GNOME renderer appeared as a closable overview window even though its surface was hidden from the normal window list. GNOME 50's workspace overview checks `skip_taskbar`, which `hide_from_window_list()` does not set. Filtering new overview previews alone failed on the user's next login: a preview can already exist by the time the renderer attaches. The extension now also removes a preexisting preview from each GNOME workspace when it attaches the renderer to the background. Twenty-four bridge tests passed, including a preview-created-before-attachment case. A fresh isolated GNOME session with a real image renderer reported zero renderer previews while overview was active, using test-only diagnostics. Repeated logins on the actual desktop still showed the renderer in overview, so this isolated result did not resolve the issue. The renderer now refuses GTK close requests on GNOME, and an actual accessibility `window.close` request left the service reporting `playing` with the clock enabled. Explicit service stop still exits. Closing only the Chrome management window independently left playback active.

With the rotating cube active and the management window closed, a five-second sample measured renderer CPU at 29.4% of one core; the service and widget sender rounded to 0.0%. The user service's cgroup used 96 MB of RAM at that moment (121 MB peak since restart), while the three processes' proportional memory totalled about 148 MB; these measures account for shared memory differently. A separate Chrome app-window process tree measured about 755 MB proportional memory in the user's existing browser profile and exited when the window closed. No GPU or gaming benchmark was run.

## Remaining acceptance work

- A future implementation would need to keep the renderer out of GNOME overview while retaining wallpaper and clock output; this remained unresolved when development stopped.
- Exercise video motion, fit, workspace changes, window focus, fullscreen and battery policies, lock/unlock, monitor changes and stop/cleanup visually; measure GPU use while gaming.
- Check the optional GTK/WebKit management window and Plasma/other Wayland/X11 implementations on their respective desktops.
- Image/Shape meters, Rainmeter measures other than bounded Time, Windows plugins/scripts, arbitrary native 3D assets/effects, and full Wallpaper Engine scene/web/editor/Workshop features are not implemented or verified.
