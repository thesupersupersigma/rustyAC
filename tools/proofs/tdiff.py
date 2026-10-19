# SPDX-License-Identifier: GPL-3.0-or-later
# Two launcher transcripts (blocks starting "== <request>") compared answer for answer.
import re
import sys


def undated(block):
    # getresults dates its answer with the file's time stamp, "now" on each side (docs/port/launcher.md)
    return re.sub(rb'"date": ?"?[^,}"]*"?', b'"date":_', block) if b'getresults' in block.split(b'\n', 1)[0] else block


def blocks(path):
    data = open(path, 'rb').read()
    out = []
    for part in data.split(b'\n== ')[0:]:
        out.append(part if part.startswith(b'== ') else b'== ' + part)
    return out


a, b = blocks(sys.argv[1]), blocks(sys.argv[2])
same = 0
for x, y in zip(a, b):
    head = x.split(b'\n', 1)[0].decode('utf-8', 'replace')
    if undated(x) == undated(y):
        same += 1
    else:
        print('DIFFERENT', head, len(x), len(y))
if len(a) != len(b):
    print('DIFFERENT number of answers', len(a), len(b))
print(f'{same} of {len(a)} the same (port {len(b)} answers)')
