# SPDX-License-Identifier: GPL-3.0-or-later
# A click script (rustyac-launcher --script form, with one `game-exit`) as a check.mjs flow: the
# steps before and after the drive through task44b/txt2flow.py; the drive itself is the page's
# own (until driving, `secs` seconds, leave, until the launcher again). The request list is
# read before the drive and at the end (a reload in between would start it again).
#   python mkflow.py <script.txt> <out.json> <shot folder> <secs>
import json
import os
import subprocess
import sys

script, out, shots, secs = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
lines = open(script, encoding='utf-8').read().splitlines()
cut = next(i for i, l in enumerate(lines) if l.startswith('game-exit'))
here = os.path.dirname(os.path.abspath(__file__))
txt2flow = os.path.join(here, '..', 'task44b', 'txt2flow.py')


def flow(part, name):
    txt, js = f'{out}.{name}.txt', f'{out}.{name}.json'
    open(txt, 'w', encoding='utf-8').write('\n'.join(part) + '\n')
    subprocess.run([sys.executable, txt2flow, txt, js, shots], check=True)
    return json.load(open(js, encoding='utf-8'))


before, after = flow(lines[:cut], 'before'), flow(lines[cut + 1:], 'after')
drive = [{'until': 'driving'}, {'wait': secs}, {'shot': f'{shots}/drive_end.jpg'}, {'js': "window.rustyac.leave(), 'left'"}, {'until': 'launcher'}, {'wait': 3}]
# (`after` starts with its own until-launcher and ends with the request list and the folder)
json.dump(before + drive + after[1:], open(out, 'w'), indent=1)
