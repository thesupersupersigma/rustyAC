# SPDX-License-Identifier: GPL-3.0-or-later
# What the Settings session wrote, desktop vs page: each side's player folder at `log MARK` and at
# the end; the changes (file, section, key -> new value; files made or removed) must be the same.
#   python delta.py <desktop docs at MARK> <desktop docs at end> <page.json>
import json
import os
import re
import sys


def ini(text):
    out, section = {}, ''
    for line in text.splitlines():
        line = line.strip()
        m = re.match(r'^\[(.*)\]$', line)
        if m:
            section = m.group(1).upper()
        elif '=' in line and not line.startswith((';', '#')):
            k, v = line.split('=', 1)
            out[(section, k.strip().upper())] = v.strip()
    return out


def folder(root):
    files = {}
    for dirpath, dirs, names in os.walk(root):
        dirs[:] = [d for d in dirs if d not in ('EBWebView', 'backups')]
        for n in names:
            p = os.path.join(dirpath, n)
            files[os.path.relpath(p, root).replace('\\', '/').lower()] = open(p, 'rb').read().decode('latin1')
    return files


def snap(value):
    s = json.loads(value)
    root = min((f for f in s['folders'] if f.lower().endswith('assetto corsa')), key=len)
    return {p[len(root) + 1 :].replace('\\', '/').lower(): t for p, t in s['files'].items() if '/backups/' not in p.replace('\\', '/').lower()}


def delta(a, b):
    out = set()
    for f in sorted(set(a) | set(b)):
        if f not in a:
            out.add((f, 'made'))
        elif f not in b:
            out.add((f, 'removed'))
        elif a[f] != b[f]:
            if f.endswith('.ini'):
                x, y = ini(a[f]), ini(b[f])
                for k in set(x) | set(y):
                    if x.get(k) != y.get(k):
                        out.add((f, k[0], k[1], y.get(k)))
            else:
                out.add((f, 'changed'))
    return out


desk = delta(folder(sys.argv[1]), folder(sys.argv[2]))
r = json.load(open(sys.argv[3], encoding='utf-8'))
snaps = [x['value'] for x in r['flow'] if isinstance(x.get('value'), str) and x['value'].startswith('{"folders"')]
page = delta(snap(snaps[0]), snap(snaps[-1]))
for name, d in (('desktop', desk), ('page', page)):
    print(f'{name}: {len(d)} changes')
    for c in sorted(d, key=str):
        print('  ', *c)
# a key one side changed and the other already had at that value (their starting files differ:
# desktop copies AC's, the page seeds rustyAC's own) counts when both end with the same value
ends = (folder(sys.argv[2]), snap(snaps[-1]))
value = lambda files, f, sec, key: ini(files.get(f, '')).get((sec, key))
odd = [c for c in desk ^ page if len(c) != 4 or value(ends[0], *c[:3]) != value(ends[1], *c[:3])]
print('SAME' if not odd else f'DIFFERENT: {sorted(odd, key=str)}')
sys.exit(1 if odd else 0)
