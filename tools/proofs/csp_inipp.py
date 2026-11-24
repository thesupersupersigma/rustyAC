# SPDX-License-Identifier: GPL-3.0-or-later
# csp_inipp.py <csp_oracle --inipp output> <inipp_flat output>: file by file, CSP's INIpp flatten
# against rustyAC's (Task C6a-2a). Prints "IDENTICAL | n/n files" or "DIFFERENT | k of n files differ, first: ..."
import re, sys

def load(path):
    text = open(path, encoding='utf-8', errors='replace', newline='').read().replace('\r\n', '\n')
    parts = re.split(r'^== (.*)$', text, flags=re.M)
    # [1:] drops the newline after the path; the oracle's run ends with its T0 STOP line
    return {k.strip(): re.split(r'\n*T0 STOP', v[1:])[0].rstrip('\n') for k, v in zip(parts[1::2], parts[2::2])}

csp, ours = load(sys.argv[1]), load(sys.argv[2])
bad = [p for p in csp if csp[p] != ours.get(p)]
if csp and not bad:
    print(f'IDENTICAL | {len(csp)}/{len(csp)} files flatten the same')
else:
    first = ''
    if bad:
        a, b = csp[bad[0]].split('\n'), (ours.get(bad[0]) or '').split('\n')
        i = next((i for i in range(max(len(a), len(b))) if a[i:i + 1] != b[i:i + 1]), 0)
        first = f', first: {bad[0]} line {i + 1}: CSP {a[i:i + 1]} ours {b[i:i + 1]}'
    print(f'DIFFERENT | {len(bad)} of {len(csp)} files differ{first}')
