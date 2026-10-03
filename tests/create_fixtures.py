#!/usr/bin/env python3
"""Create original, disposable media for manual and browser integration tests."""

import json
import shutil
from pathlib import Path
import struct
import subprocess
import sys
import zlib


def png(path, width=1280, height=720):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data) & 0xffffffff)

    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            ridge = int(height * (0.50 + 0.14 * abs((x / width * 4) % 2 - 1)))
            if y > ridge:
                color = (14 + y // 40, 33 + y // 25, 45 + y // 20)
            else:
                color = (22 + y // 8, 45 + y // 7, 65 + y // 6)
            rows.extend(color)
    data = b'\x89PNG\r\n\x1a\n'
    data += chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 2, 0, 0, 0))
    data += chunk(b'IDAT', zlib.compress(bytes(rows), 6))
    data += chunk(b'IEND', b'')
    path.write_bytes(data)


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else 'tests/generated').resolve()
    root.mkdir(parents=True, exist_ok=True)
    png(root / 'Alpine dusk.png')
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-f', 'lavfi', '-i', 'testsrc2=size=960x540:rate=24', '-t', '4', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', str(root / 'Motion test.mp4')], check=True)
    project = root / 'workshop-video'
    project.mkdir(exist_ok=True)
    (project / 'project.json').write_text(json.dumps({'title': 'Workshop motion test', 'type': 'video', 'file': '../Motion test.mp4'}))
    # This fixture intentionally references a parent path and must be rejected.
    valid = root / 'valid-project'
    valid.mkdir(exist_ok=True)
    (valid / 'wallpaper.mp4').write_bytes((root / 'Motion test.mp4').read_bytes())
    png(valid / 'preview.png', 480, 270)
    (valid / 'project.json').write_text(json.dumps({'title': 'Workshop sample', 'type': 'video', 'file': 'wallpaper.mp4', 'preview': 'preview.png', 'description': 'Original test media, not downloaded from Steam.'}))
    scene = root / 'unsupported-scene'
    scene.mkdir(exist_ok=True)
    (scene / 'project.json').write_text(json.dumps({'title': 'Scene compatibility test', 'type': 'scene', 'file': 'scene.pkg'}))
    (scene / 'scene.pkg').write_bytes(b'wall-in-test-fixture')
    (root / 'not-media.txt').write_text('This file is not a wallpaper.')
    steam = root / 'home/.local/share/Steam'
    library = root / 'steam-library'
    (steam / 'steamapps').mkdir(parents=True, exist_ok=True)
    (steam / 'steamapps/libraryfolders.vdf').write_text('"libraryfolders" { "0" { "path" "' + str(library) + '" } }')
    destination = library / 'steamapps/workshop/content/431960/1000001'
    shutil.copytree(valid, destination, dirs_exist_ok=True)
    metadata = json.loads((destination / 'project.json').read_text())
    metadata['title'] = 'Steam scan sample'
    (destination / 'project.json').write_text(json.dumps(metadata))
    print(root)


if __name__ == '__main__':
    main()
