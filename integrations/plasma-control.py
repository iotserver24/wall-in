#!/usr/bin/env python3
"""Experimental one-shot Plasma wallpaper configuration, not a renderer readiness probe."""

import argparse
import json
from pathlib import Path
import sys

PLUGIN = "io.wallin.wallpaper"
KEYS = {"file": "File", "kind": "Kind", "fit": "Fit", "volume": "Volume", "muted": "Muted"}


def build_script(screen, action, settings, activate=False):
    if isinstance(screen, bool) or not isinstance(screen, int) or screen < 0:
        raise ValueError("screen must be a non-negative Plasma screen index")
    if action not in ("configure", "pause", "resume", "stop"):
        raise ValueError("unknown action")
    if not isinstance(settings, dict) or settings.keys() - KEYS.keys():
        raise ValueError("unsupported Plasma setting")
    if action != "configure" and settings:
        raise ValueError("only configure accepts settings")
    values = {}
    if action == "configure":
        for key, value in settings.items():
            if key == "file":
                if not isinstance(value, str):
                    raise ValueError("file must be a local path")
                path = Path(value).expanduser().resolve(strict=True)
                if not path.is_file():
                    raise ValueError("file must be a regular file")
                value = path.as_uri()
            elif key == "kind" and value not in ("image", "video"):
                raise ValueError("kind must be image or video")
            elif key == "fit" and value not in ("cover", "contain", "fill"):
                raise ValueError("fit must be cover, contain, or fill")
            elif key == "volume" and (isinstance(value, bool) or not isinstance(value, int)
                                       or not 0 <= value <= 100):
                raise ValueError("Plasma volume must be an integer from 0 to 100")
            elif key == "muted" and not isinstance(value, bool):
                raise ValueError("muted must be boolean")
            values[KEYS[key]] = value
    elif action == "stop":
        values = {"File": "", "Paused": True}
    else:
        values = {"Paused": action == "pause"}
    # All caller-controlled data is JSON, never executable script fragments.
    return f"""(function() {{
        var targets = desktops().filter(function(d) {{ return d.screen === {screen}; }});
        if (targets.length !== 1) throw new Error('Expected one desktop for this screen');
        var desktop = targets[0];
        var plugin = {json.dumps(PLUGIN)};
        if (desktop.wallpaperPlugin !== plugin) {{
            if (!{json.dumps(activate)}) throw new Error('Select Wall-in wallpaper first, or pass --activate');
            desktop.wallpaperPlugin = plugin;
        }}
        desktop.currentConfigGroup = ['Wallpaper', plugin, 'General'];
        var values = {json.dumps(values, ensure_ascii=True)};
        Object.keys(values).forEach(function(key) {{ desktop.writeConfig(key, values[key]); }});
        print('WALL_IN_CONFIGURED');
    }})()"""


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--screen", type=int, required=True)
    parser.add_argument("--action", choices=("configure", "pause", "resume", "stop"), required=True)
    parser.add_argument("--settings", default="{}", help="JSON object: file, kind, fit, volume, muted")
    parser.add_argument("--activate", action="store_true", help="Explicitly select Wall-in on this screen")
    args = parser.parse_args(argv)
    try:
        script = build_script(args.screen, args.action, json.loads(args.settings), args.activate)
        import gi
        from gi.repository import Gio, GLib
        connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        result = connection.call_sync("org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell",
                                      "evaluateScript", GLib.Variant("(s)", (script,)), None,
                                      Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
        if "WALL_IN_CONFIGURED" not in result:
            raise RuntimeError(f"Plasma did not confirm configuration: {result}")
        print(json.dumps({"event": "configured", "screen": args.screen,
                          "message": "Configuration written; playback has not been verified"}))
        return 0
    except Exception as error:
        print(f"Wall-in Plasma: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
