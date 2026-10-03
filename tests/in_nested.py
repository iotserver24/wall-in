#!/usr/bin/env python3
"""Run a command in the isolated session started by nested-gnome.sh."""
import json
import os
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
environment_file = Path(os.environ.get('WALL_IN_NESTED_DIR', root / '.wall-in-test-data/nested')) / 'environment.json'
if len(sys.argv) < 2:
    sys.exit('Usage: python3 tests/in_nested.py COMMAND [ARG ...]')
if not environment_file.is_file():
    sys.exit('Start tests/nested-gnome.sh first.')
environment = os.environ.copy()
environment.update(json.loads(environment_file.read_text()))
environment['GDK_BACKEND'] = 'wayland'
os.execvpe(sys.argv[1], sys.argv[1:], environment)
