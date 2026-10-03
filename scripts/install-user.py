#!/usr/bin/python3
"""Install Wall-in for the current user without changing the library or other apps."""

import argparse
import os
import shlex
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

SOURCE = Path(__file__).resolve().parents[1]
MARKER = ".wall-in-managed-app"


def paths():
    home = Path.home()
    data = Path(os.environ.get("XDG_DATA_HOME", home / ".local/share")).expanduser()
    config = Path(os.environ.get("XDG_CONFIG_HOME", home / ".config")).expanduser()
    if not data.is_absolute() or not config.is_absolute():
        raise RuntimeError("XDG data and config directories must be absolute")
    return {
        "app": data / "wall-in/app",
        "library": data / "wall-in",
        "command": home / ".local/bin/wall-in",
        "desktop": data / "applications/io.wallin.Manager.desktop",
        "icon": data / "icons/hicolor/scalable/apps/io.wallin.Manager.svg",
        "unit": config / "systemd/user/wall-in.service",
    }


def managed(path, marker):
    if not path.exists() and not path.is_symlink():
        return
    if path.is_symlink():
        raise RuntimeError(f"Refusing to replace symlink: {path}")
    if path.is_dir():
        if not (path / marker).is_file():
            raise RuntimeError(f"Refusing to replace unmanaged directory: {path}")
    elif marker not in path.read_text(errors="replace"):
        raise RuntimeError(f"Refusing to replace unmanaged file: {path}")


def atomic_text(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                     prefix=".wall-in-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(text)
    try:
        temporary.chmod(mode)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def service_text(p):
    app, library = p["app"], p["library"]
    for value in (app, library):
        if any(char in str(value) for char in ('"', '%', '\n', '\r', '\\')):
            raise RuntimeError("Install path contains unsupported systemd unit characters")
    return f'''# Managed by Wall-in app installer; remove only through Wall-in settings.
[Unit]
Description=Wall-in wallpaper service
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
Environment="WALL_IN_ROOT={app}"
ExecStart="{app}/bin/wall-in" serve --data-dir "{library}"
MemoryMax=16000000000
MemorySwapMax=0
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=graphical-session.target
'''


def stage_app(p):
    app = p["app"]
    app.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".wall-in-app-", dir=app.parent))
    try:
        (stage / "bin").mkdir()
        shutil.copy2(SOURCE / "target/debug/wall-in", stage / "bin/wall-in")
        (stage / "bin/wall-in").chmod(0o755)
        shutil.copytree(SOURCE / "apps/desktop/dist", stage / "apps/desktop/dist")
        shutil.copytree(SOURCE / "integrations", stage / "integrations",
                        ignore=shutil.ignore_patterns("__pycache__", "tests"))
        (stage / "scripts").mkdir()
        shutil.copy2(SOURCE / "scripts/window.py", stage / "scripts/window.py")
        (stage / MARKER).write_text("Wall-in user-local application v1\n")
        return stage
    except Exception:
        shutil.rmtree(stage)
        raise


def install():
    p = paths()
    for key, marker in (("app", MARKER), ("command", "Managed by Wall-in app installer"),
                        ("desktop", "X-Wall-In-Managed=true"),
                        ("icon", "Managed by Wall-in app installer"),
                        ("unit", "Managed by Wall-in")):
        managed(p[key], marker)
    if not (SOURCE / "target/debug/wall-in").is_file() or not (SOURCE / "apps/desktop/dist/index.html").is_file():
        raise RuntimeError("Build first: make build")
    stage = stage_app(p)
    old = p["app"].with_name(".wall-in-app-previous")
    if old.exists():
        shutil.rmtree(stage)
        raise RuntimeError(f"Previous installation backup remains: {old}")
    former_unit = p["unit"].read_text() if p["unit"].exists() else None
    replaced = False
    try:
        if p["app"].exists():
            os.replace(p["app"], old)
            replaced = True
        os.replace(stage, p["app"])
        script = f'''#!/bin/sh
# Managed by Wall-in app installer
set -eu
APP={shlex.quote(str(p["app"]))}
if [ "$#" -eq 0 ]; then
    set -- open
fi
if [ "$1" = open ]; then
    shift
    systemctl --user start wall-in.service
    if command -v google-chrome >/dev/null 2>&1; then
        exec google-chrome --app=http://127.0.0.1:3977 "$@"
    fi
    exec xdg-open http://127.0.0.1:3977
fi
if [ "$1" = window ]; then
    shift
    systemctl --user start wall-in.service
    exec /usr/bin/python3 "$APP/scripts/window.py" "$@"
fi
exec "$APP/bin/wall-in" "$@"
'''
        atomic_text(p["command"], script, 0o755)
        if any(char.isspace() for char in str(p["command"])):
            raise RuntimeError("Launcher path with whitespace is not supported")
        atomic_text(p["desktop"], f'''[Desktop Entry]
Type=Application
Name=Wall-in
Comment=Manage animated desktop wallpapers and widgets
Exec={p["command"]} open
TryExec={p["command"]}
Icon=io.wallin.Manager
Terminal=false
Categories=Utility;
StartupWMClass=io.wallin.Manager
X-Wall-In-Managed=true
''')
        icon = (SOURCE / "assets/io.wallin.Manager.svg").read_text()
        atomic_text(p["icon"], "<!-- Managed by Wall-in app installer -->\n" + icon)
        atomic_text(p["unit"], service_text(p))
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=True)
        subprocess.run(["systemctl", "--user", "enable", "wall-in.service"], check=True)
        subprocess.run(["systemctl", "--user", "restart", "wall-in.service"], check=True)
        for attempt in range(20):
            try:
                with urllib.request.urlopen("http://127.0.0.1:3977/api/health", timeout=2) as response:
                    if response.status == 200:
                        break
            except urllib.error.URLError:
                pass
            time.sleep(0.5)
        else:
            raise RuntimeError("Installed service did not become healthy")
        if replaced:
            shutil.rmtree(old)
    except Exception:
        if former_unit is not None:
            atomic_text(p["unit"], former_unit)
        else:
            p["unit"].unlink(missing_ok=True)
        if replaced:
            shutil.rmtree(p["app"], ignore_errors=True)
            os.replace(old, p["app"])
        else:
            shutil.rmtree(p["app"], ignore_errors=True)
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=False)
        if former_unit is not None:
            subprocess.run(["systemctl", "--user", "restart", "wall-in.service"], check=False)
        raise
    print(f"Installed Wall-in: {p['desktop']} and {p['command']}")


def status():
    p = paths()
    for name in ("app", "command", "desktop", "icon", "unit"):
        print(f"{name}: {p[name]} ({'present' if p[name].exists() else 'missing'})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "status"))
    args = parser.parse_args()
    try:
        {"install": install, "status": status}[args.action]()
        return 0
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Wall-in installation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
