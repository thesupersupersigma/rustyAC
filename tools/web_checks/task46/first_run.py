# SPDX-License-Identifier: GPL-3.0-or-later
# Task 46 step 0.3: a new player's first launcher start, desktop against page. Desktop:
# `rustyac-launcher --docs <empty> --player-cfg none --script first_desk.txt --log run.log`; page:
# first.txt through task44b/txt2flow.py from an empty page store (the page opens Drive by itself,
# first_desk.txt clicks it). Compares every request (counted) and every file of the player's folder.
#   python first_run.py <run.log> <docs folder> <page flow report.json>
import collections
import difflib
import json
import os
import re
import subprocess
import sys
import urllib.parse

log, docs, report = sys.argv[1:4]
values = [s.get('value') for s in json.load(open(report, encoding='utf-8'))['flow'] if isinstance(s.get('value'), str)]
page = json.loads([v for v in values if v.startswith('[')][-1])
snap = json.loads([v for v in values if v.startswith('{"folders"')][-1])
subprocess.run([sys.executable, 'tools/proofs/log2req.py', log, log + '.req'], check=True, capture_output=True)
desk = open(log + '.req', encoding='utf-8').read().splitlines()[1:]


def norm(l):
    l = urllib.parse.unquote(l.replace('+', ' ') if l.startswith('POST') else l).replace('\r', '').replace('\n', '')
    l = re.sub(r'^(\S+ ac://[^/?\s]+)\?', r'\1/?', l)
    l = re.sub(r'([?&]_=)\d+|(?<=/\?)\d{13}$', lambda m: (m.group(1) or '') + 'T', l)
    return re.sub(r'(DRIVER_NAME|lastselected)=[^&\t]*', r'\1=X', l)


# what only differs by machine: the player's GUID and servers (none asked here)
keep = lambda l: re.match(r'\S+ ac://', l) and not re.match(r'\S+ ac://(getguid|getserver|pingserver)', l)
desk = sorted(norm(l) for l in desk if keep(l))
page = sorted(norm(l) for l in page if keep(l))
print(f'requests: desktop {len(desk)}, page {len(page)}: {"IDENTICAL" if desk == page else "DIFFERENT"}')
cd, cp = collections.Counter(desk), collections.Counter(page)
for l in sorted(set(cd) | set(cp)):
    if cd[l] != cp[l]:
        print(f'  desktop {cd[l]} page {cp[l]}: {l[:180]}')
files = {k.split('Assetto Corsa\\', 1)[-1].replace('\\', '/'): v for k, v in snap['files'].items()}
mine = {}
for top, _, names in os.walk(docs):
    if 'EBWebView' in top:  # WebView2's own browser data: the page's browser keeps its own
        continue
    for n in names:
        p = os.path.join(top, n)
        mine[os.path.relpath(p, docs).replace(os.sep, '/')] = open(p, 'rb').read().decode('latin-1')
# the player's name the theme writes (the page's player is "Player" here)
clean = lambda t: re.sub(r'(?m)^(DRIVER_NAME|NAME)=.*$', '', t)
bad = [r for r in sorted(set(files) | set(mine)) if clean(files.get(r, '?')) != clean(mine.get(r, '!'))]
print(f'files: desktop {len(mine)}, page {len(files)}: {"IDENTICAL" if not bad else "DIFFERENT"}')
for r in bad:
    print(f'  {r}: ' + ('desktop only' if r not in files else 'page only' if r not in mine else 'content differs'))
    if r in files and r in mine:
        for d in list(difflib.unified_diff(clean(mine[r]).splitlines(), clean(files[r]).splitlines(), 'desktop', 'page', n=0, lineterm=''))[2:12]:
            print('    ' + d[:160])
print('SAME' if desk == page and not bad else 'DIFFERENT')
