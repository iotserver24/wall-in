#!/usr/bin/env python3
"""Explicit, reversible, user-local installation of Wall-in's GNOME extension."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

UUID = "wall-in@r3ap3r"
MARKER = ".wall-in-managed"


def enabled_extensions():
    result = subprocess.run(["gnome-extensions", "list", "--enabled"], check=True,
                            capture_output=True, text=True, timeout=10)
    return set(result.stdout.splitlines())


def target_path():
    data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share")))
    if not data.is_absolute():
        raise RuntimeError("XDG_DATA_HOME must be absolute")
    return data / "gnome-shell/extensions" / UUID


def install(target):
    if target.exists() or target.is_symlink():
        raise RuntimeError(f"Refusing to overwrite {target}. Uninstall this managed copy first; "
                           "existing third-party or manual installations are never replaced.")
    source = Path(__file__).resolve().parent / "gnome" / UUID
    metadata = json.loads((source / "metadata.json").read_text())
    if metadata.get("uuid") != UUID:
        raise RuntimeError("Extension metadata UUID mismatch")
    target.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".wall-in-install-", dir=target.parent))
    try:
        for name in ("metadata.json", "extension.js"):
            shutil.copy2(source / name, staging / name)
        (staging / MARKER).write_text(UUID + "\n")
        staging.rename(target)
    finally:
        if staging.exists():
            shutil.rmtree(staging)
    print(f"Installed {target}. Nothing was enabled.")
    print("On Wayland, log out and back in if GNOME does not discover the extension.")
    print("Then run this script with 'enable', or enable Wall-in Background Bridge in Extensions.")


def require_managed(target):
    if target.is_symlink() or not target.is_dir():
        raise RuntimeError(f"No managed extension directory at {target}")
    marker = target / MARKER
    if marker.is_symlink() or not marker.is_file() or marker.read_text().strip() != UUID:
        raise RuntimeError("Refusing to modify an extension not installed by this script")


def upgrade(target):
    require_managed(target)
    source = Path(__file__).resolve().parent / "gnome" / UUID
    metadata = json.loads((source / "metadata.json").read_text())
    if metadata.get("uuid") != UUID:
        raise RuntimeError("Extension metadata UUID mismatch")
    for name in ("metadata.json", "extension.js"):
        temporary = target / (name + ".wall-in-upgrade")
        try:
            shutil.copy2(source / name, temporary)
            os.replace(temporary, target / name)
        finally:
            temporary.unlink(missing_ok=True)
    print("Updated managed Wall-in extension files. GNOME may load them at the next natural login.")


def enable(target):
    require_managed(target)
    result = subprocess.run(["gnome-extensions", "enable", UUID], capture_output=True,
                            text=True, timeout=10)
    if result.returncode or UUID not in enabled_extensions():
        raise RuntimeError("GNOME could not enable Wall-in. Log out and back in, then retry. "
                           + result.stderr.strip())
    print("Wall-in was explicitly enabled. Run renderer.py --diagnostics to verify the bridge.")


def uninstall(target):
    require_managed(target)
    if UUID in enabled_extensions():
        subprocess.run(["gnome-extensions", "disable", UUID], check=True, timeout=10)
        if UUID in enabled_extensions():
            raise RuntimeError("GNOME still reports Wall-in enabled; refusing to remove active files")
    shutil.rmtree(target)
    print("Removed Wall-in's extension. No wallpaper settings or other extensions were changed.")
    print("A logout/login may be needed for GNOME to forget cached extension code.")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "upgrade", "enable", "uninstall", "status"))
    args = parser.parse_args(argv)
    try:
        target = target_path()
        if args.action == "status":
            print(json.dumps({"uuid": UUID, "path": str(target), "installed": target.is_dir(),
                              "enabled": UUID in enabled_extensions()}))
        else:
            {"install": install, "upgrade": upgrade, "enable": enable,
             "uninstall": uninstall}[args.action](target)
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Wall-in extension: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
