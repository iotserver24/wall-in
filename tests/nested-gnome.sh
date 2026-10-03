#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="${WALL_IN_NESTED_DIR:-$ROOT/.wall-in-test-data/nested}"
export XDG_CONFIG_HOME="$TEST_ROOT/config"
export XDG_DATA_HOME="$TEST_ROOT/data"
export XDG_CACHE_HOME="$TEST_ROOT/cache"
export XDG_DATA_DIRS=/usr/local/share:/usr/share
export WALL_IN_NESTED_DIR="$TEST_ROOT"
export WALL_IN_NESTED_DISPLAY=wall-in-test-0
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME/gnome-shell/extensions/wall-in@r3ap3r"
cp "$ROOT/integrations/gnome/wall-in@r3ap3r/extension.js" "$ROOT/integrations/gnome/wall-in@r3ap3r/metadata.json" "$XDG_DATA_HOME/gnome-shell/extensions/wall-in@r3ap3r/"
exec dbus-run-session -- bash -c '
    set -euo pipefail
    gsettings set org.gnome.shell enabled-extensions "[\"wall-in@r3ap3r\"]"
    gsettings set org.gnome.shell disable-user-extensions false
    /usr/bin/python3 -c '\''import json, os, pathlib; env = {key: os.environ[key] for key in ["DBUS_SESSION_BUS_ADDRESS", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_DATA_DIRS"]}; env.update(WAYLAND_DISPLAY=os.environ["WALL_IN_NESTED_DISPLAY"], XDG_CURRENT_DESKTOP="GNOME", XDG_SESSION_TYPE="wayland"); pathlib.Path(os.environ["WALL_IN_NESTED_DIR"], "environment.json").write_text(json.dumps(env))'\''
    exec gnome-shell --devkit --wayland --virtual-monitor 1280x720 --wayland-display "$WALL_IN_NESTED_DISPLAY" --no-x11
'
