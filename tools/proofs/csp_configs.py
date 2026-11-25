# SPDX-License-Identifier: GPL-3.0-or-later
# csp_configs.py <ac> <inipp_flat --configs output>: CSP's own loader results, the ConfigsCache the
# real game wrote (<ac>/cache/csp_configs/<car or track>.zip, one entry per config file, writer
# dll+0x1510a00), against rustyAC's car/track loader, key by key (Task C6a-2c). An entry the game
# wrote under an older CSP install (older common files) is stale: skipped, counted.
# Prints "IDENTICAL | n/n cached configs load the same (...)" or "DIFFERENT | k of n differ, first: ...".
import os, struct, sys, zipfile


class R:
    def __init__(s, b): s.b, s.i = b, 0
    def u8(s): s.i += 1; return s.b[s.i - 1]
    def n(s):  # CSP's length: < 0xFB itself, 0xFB..0xFE high bits of two bytes, 0xFF a u32
        v = s.u8()
        if v < 0xFB: return v
        if v == 0xFF:
            r = struct.unpack_from('<I', s.b, s.i)[0]; s.i += 4; return r
        return 251 + ((v - 0xFB) << 8 | s.u8())
    def s(s):
        l = s.n(); r = s.b[s.i:s.i + l]; s.i += l; return r.decode('utf-8', 'replace')


def decode(b):
    """flags u8, version u8, stamp u64, deps (type, name, mtime), then sections / keys / values."""
    r = R(b); r.i = 10
    deps = []
    for _ in range(r.n()):
        t = r.u8(); name = r.s(); ts = struct.unpack_from('<I', b, r.i)[0]; r.i += 4
        deps.append((t, name, ts))
    lines = []
    for _ in range(r.n()):
        sec = r.s()
        for _ in range(r.n()):
            k = r.s(); vals = [r.s() for _ in range(r.n())]
            lines.append('\x1f'.join(x.replace('\n', '\\n') for x in (sec, k, '\x1e'.join(vals))))
    return deps, sorted(lines)


# cars CSP reads through files it alone can decrypt (never decrypted here): their entries are counted, not compared
KNOWN = {'urd_darche_992_23': 'tyres.ini encrypted by CSP, meta/has_rain_tyres unknown'}

ac, ours_path = sys.argv[1], sys.argv[2]
ours = {}  # (id, entry name) -> [(file, sorted lines)], one per skin / layout run
cur = None
for line in open(ours_path, encoding='utf-8', errors='replace', newline='\n').read().split('\n'):
    if line.startswith('== '):
        kind, cid, which, cached, name, file = line[3:].split(' ', 5)
        cur = [] if cached == '1' else None
        if cur is not None:
            ours.setdefault((cid, name), []).append((file, cur))
    elif cur is not None and line:
        cur.append(line)

total, same, bad, ids, stale, known = 0, 0, [], set(), 0, 0
folder = f'{ac}/cache/csp_configs'
zips = sorted(os.listdir(folder)) if os.path.isdir(folder) else []
install = max((s for z in zips for e in zipfile.ZipFile(f'{folder}/{z}').infolist()
               for ty, _, s in decode(zipfile.ZipFile(f'{folder}/{z}').read(e))[0] if ty in (2, 3, 4)), default=0)
for z in zips:
    cid = z[:-4]
    for e in zipfile.ZipFile(f'{folder}/{z}').infolist():
        deps, lines = decode(zipfile.ZipFile(f'{folder}/{z}').read(e))
        runs = ours.get((cid, e.filename), [])
        # stamps of extension\config files (types 2-4) are the CSP install's, not file times:
        # an entry under an older install read older common files
        if any(s < install for ty, _, s in deps if ty in (2, 3, 4)):
            stale += 1
            continue
        if cid in KNOWN:
            known += 1
            continue
        total += 1
        ids.add(cid)
        if any(sorted(l) == lines for _, l in runs):
            same += 1
        else:
            bad.append((cid, e.filename, runs, lines))

note = f'{len(ids)} cars and tracks the game loaded, {stale} entries from an older CSP install skipped, {known} entries of cars with encrypted data not compared'
if total and not bad:
    print(f'IDENTICAL | {same}/{total} cached configs load the same ({note})')
else:
    first = ''
    if bad:
        cid, name, runs, lines = bad[0]
        if not runs:
            first = f', first: {cid} {name}: rustyAC loads no such cached file'
        else:
            file, l = runs[0]
            a, b = set(lines), set(l)
            first = f', first: {cid} {file}: CSP only {sorted(a - b)[:2]} ours only {sorted(b - a)[:2]}'
    print(f'DIFFERENT | {len(bad)} of {total} cached configs differ ({note}){first}')
