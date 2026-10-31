# SPDX-License-Identifier: GPL-3.0-or-later
# Desktop run (rustyac-launcher --script, its run.log and --docs folder) against the page's flow
# report (check.mjs --flow of mkflow.py): the bridge requests from `log MARK` on, and the player's
# files both sides end with (race.ini and the rest, the page's from its snapshot).
#   python cmp45.py <run.log> <docs folder> <page flow report.json> <out folder>
# Requests are compared URL-decoded, without line breaks inside a value (the desktop's log keeps
# none), with the player's name, the clock's lastselected, the theme's Math.random road temperature and the drive's own
# results blanked; in order, and as a multiset (the theme's timers interleave differently).
import collections
import difflib
import json
import os
import re
import subprocess
import sys
import urllib.parse

log, docs, report, out = sys.argv[1:5]
values = [s.get('value') for s in json.load(open(report, encoding='utf-8'))['flow'] if isinstance(s.get('value'), str)]
dumps = [json.loads(v) for v in values if v.startswith('[')]
snaps = [json.loads(v) for v in values if v.startswith('{"folders"')]
# (the page keeps its list across the drive, when it has the MARK; else a reload started it again)
page = dumps[-1] if 'LOG MARK' in dumps[-1] else dumps[0] + dumps[-1]
page = page[len(page) - page[::-1].index('LOG MARK'):]

lines = open(log, encoding='utf-8', errors='replace').read().split('\n')
cut = os.path.join(out, 'after_mark.log')
open(cut, 'w', encoding='utf-8').write('\n'.join(lines[next(i for i, l in enumerate(lines) if l.strip() == 'log MARK'):]))
subprocess.run([sys.executable, 'tools/proofs/log2req.py', cut, cut + '.req'], check=True, capture_output=True)
desk = open(cut + '.req', encoding='utf-8').read().splitlines()[1:]

DROP = {'start', 'getguid', 'getachievements', 'getserver', 'getserverlist', 'getservercached', 'getlanservers', 'pingserver', 'eventcount'}


def norm(l):
    l = urllib.parse.unquote(l.replace('+', ' ') if l.startswith('POST') else l).replace('\r', '').replace('\n', '')
    l = re.sub(r'^(\S+ ac://[^/?\s]+)\?', r'\1/?', l)
    l = re.sub(r'([?&]_=)\d+|(?<=/\?)\d{13}$', lambda m: (m.group(1) or '') + 'T', l)
    l = re.sub(r'(DRIVER_NAME|TEMPERATURE/ROAD|lastselected)=[^&\t]*', r'\1=X', l)
    # (a new championship's GUID: ac://getguid)
    l = re.sub(r'[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}', 'GUID', l)
    # what the drive itself gave (lap times, wins: two live drives) and the messages naming them
    l = re.sub(r'(ac://(setdata|savejson)/\S*\t).*', r'\1<data>', l)
    return re.sub(r'(msg=(TIERS|Results)).*', r'\1 ...', l)


def keep(l):
    m = re.match(r'\S+ ac://([^/?]*)', l)
    return m and m.group(1) not in DROP


desk = [norm(l) for l in desk if keep(l)]
page = [norm(l) for l in page if keep(l)]
open(os.path.join(out, 'desk.txt'), 'w', encoding='utf-8').write('\n'.join(desk) + '\n')
open(os.path.join(out, 'page.txt'), 'w', encoding='utf-8').write('\n'.join(page) + '\n')
diff = list(difflib.unified_diff(desk, page, 'desktop', 'page', lineterm='', n=0))
bag = collections.Counter(desk) - collections.Counter(page), collections.Counter(page) - collections.Counter(desk)
print(f'desktop {len(desk)}, page {len(page)} requests after MARK; in order: {"same" if not diff else f"{len(diff)} diff lines"}; as a multiset: {"same" if not any(bag) else "DIFFERENT"}')
for side, extra in zip(('desktop only', 'page only'), bag):
    for l, n in list(extra.items())[:20]:
        print(f'  {side} x{n}: {l[:220]}')
# (the page names its files C:\Documents\Assetto Corsa\...)
files = {k.split('Assetto Corsa\\', 1)[-1].replace('\\', '/'): v for k, v in snaps[-1]['files'].items()}
for rel in sorted(files):
    path = os.path.join(docs, rel.replace('/', os.sep))
    if rel.lower().startswith(('cfg/race.ini', 'launcherdata/filestore/', 'champs/')):
        mine = open(path, 'rb').read().decode('latin-1') if os.path.isfile(path) else None
        if mine is not None and mine != files[rel]:
            a = [l for l in mine.splitlines() if not re.match(r'(DRIVER_NAME|ROAD|LASTSELECTED)=', l)]
            b = [l for l in files[rel].splitlines() if not re.match(r'(DRIVER_NAME|ROAD|LASTSELECTED)=', l)]
            state = 'same but the name and road temperature' if a == b else 'DIFFERENT: ' + ' | '.join(list(difflib.unified_diff(a, b, lineterm='', n=0))[2:8])
        else:
            state = 'same' if mine == files[rel] else 'desktop has none'
        print(f'file {rel}: {state}')
print('SAME' if not any(bag) else 'DIFFERENT requests')
