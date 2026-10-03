#!/usr/bin/env python3
"""Test-only renderer protocol fixture. This does not render any desktop output."""
import argparse
import json
import os
from pathlib import Path
import sys

if '--diagnostics' in sys.argv:
    print(json.dumps({'desktop':'TEST FIXTURE','session':'test','backend':'protocol-fixture','available':True,'message':'Test-only protocol fixture; no actual desktop output.','monitors':[{'id':'test-0','name':'Test display','width':960,'height':540}]}))
    sys.exit(0)

parser = argparse.ArgumentParser()
parser.add_argument('--file', required=True)
parser.add_argument('--kind', required=True)
parser.add_argument('--monitor', required=True)
parser.add_argument('--fit', required=True)
parser.add_argument('--volume', required=True)
parser.add_argument('--muted', action='store_true')
args = parser.parse_args()
if not Path(args.file).is_file():
    sys.exit('Missing test media')
log = Path(os.environ['WALL_IN_PROTOCOL_LOG'])
def record(value):
    with log.open('a') as output:
        output.write(json.dumps(value)+'\n')
record({'action':'started','pid':os.getpid(),'file':args.file})
print(json.dumps({'event':'ready'}),flush=True)
for line in sys.stdin:
    command = json.loads(line)
    record(command)
    if command['action']=='stop':
        break
