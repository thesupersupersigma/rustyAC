# SPDX-License-Identifier: GPL-3.0-or-later
"""render_wgpu.sh's comparisons (numpy and Pillow).
  render_wgpu.py frame <work> <name> <adapter>: the kgl logs of <work>/d3d and <work>/<adapter> line
    by line, and the wgpu port's pixels against the game's WARP pixels (PSNR, largest channel
    difference, pixels off by more than 8); writes <work>/<name>[.f<i>].side.jpg (game | wgpu | x8, <=300 KB)
  render_wgpu.py table <out>: the report's tables from <out>/*.res"""
import glob, os, re, sys


def frame(out, name, wdir):
    import numpy as np
    from PIL import Image

    def one(pattern):
        hits = sorted(glob.glob(pattern))
        return hits[0] if hits else None

    def psnr(a, b):
        mse = np.mean((a.astype(np.float64) - b.astype(np.float64)) ** 2)
        return float('inf') if mse == 0 else 10 * np.log10(255.0 ** 2 / mse)

    ld = one(f'{out}/d3d/*{name}*.port.kgllog')
    lw = one(f'{out}/{wdir}/*{name}*.port.kgllog')
    if ld and lw:
        a, b = open(ld, encoding='utf-8').read().splitlines(), open(lw, encoding='utf-8').read().splitlines()
        first = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), None)
        if first is None and len(a) == len(b):
            print(f'kgl log: IDENTICAL ({len(a)} lines)')
        else:
            i = first if first is not None else min(len(a), len(b))
            print(f'kgl log: DIFFERENT at line {i + 1} of {len(a)} / {len(b)}')
            print(f'  d3d : {a[i] if i < len(a) else "<end>"}')
            print(f'  wgpu: {b[i] if i < len(b) else "<end>"}')
    else:
        print(f'kgl log: MISSING ({ld}, {lw})')

    # single frames: <frame>.<side>.rgba; sequences: <frame>.<side>.f<i>.rgba
    rows = []
    for ac in sorted(glob.glob(f'{out}/d3d/*{name}*.ac*.rgba')):
        tail = os.path.basename(ac).split('.ac', 1)
        wg = f'{out}/{wdir}/{tail[0]}.port{tail[1]}'
        png = ac[:-5] + '.png'
        if not os.path.exists(wg) or not os.path.exists(png):
            print(f'{os.path.basename(ac)}: no wgpu frame')
            continue
        w, h = Image.open(png).size
        g = np.fromfile(ac, np.uint8).reshape(h, w, 4)[:, :, :3]
        p = np.fromfile(wg, np.uint8).reshape(h, w, 4)[:, :, :3]
        d = np.abs(g.astype(np.int16) - p.astype(np.int16))
        off = int(np.count_nonzero(d.max(axis=2) > 8))
        label = tail[1].strip('.').replace('rgba', '').strip('.') or 'frame'
        rows.append((label, psnr(g, p), int(d.max()), off, w * h))
        if len(rows) == 1 or label.endswith('0'):
            side = np.concatenate([g, p, np.minimum(d * 8, 255).astype(np.uint8)], axis=1)
            img = Image.fromarray(side)
            img.thumbnail((1500, 1500))
            path = f'{out}/{name}{"" if label == "frame" else "." + label}.side.jpg'
            for q in (85, 70, 55, 40):
                img.save(path, quality=q)
                if os.path.getsize(path) <= 300_000:
                    break
    for label, ps, mx, off, n in rows:
        print(f'{label}: PSNR {ps:.2f} dB, largest diff {mx}, {off} of {n} pixels off by more than 8')
    if rows:
        print(f'frames {len(rows)}: PSNR min {min(r[1] for r in rows):.2f} dB, largest diff {max(r[2] for r in rows)}')


def table(B):
    rows, sums = [], {}
    for done in sorted(glob.glob(f'{B}/*.done')):
        name = os.path.basename(done)[:-5]
        d3d = open(f'{B}/{name}.d3d.txt', encoding='utf-8', errors='replace').read()
        d3d_ok = 'command log: IDENTICAL' in d3d and 'pixels: IDENTICAL' in d3d
        for res in sorted(glob.glob(f'{B}/{name}.*.res')):
            a = res.split('.')[-2]
            t = open(res, encoding='utf-8').read()
            log = re.search(r'kgl log: (IDENTICAL \((\d+) lines\)|DIFFERENT.*|MISSING.*)', t)
            fr = re.findall(r'^(\w+): PSNR ([\d.inf]+) dB, largest diff (\d+), (\d+) of (\d+) pixels', t, re.M)
            secs = re.search(r'seconds d3d=(\d+) \w+=(\d+)', t)
            same = bool(log and log.group(1).startswith('IDENTICAL'))
            psnr = min(float(f[1]) for f in fr) if fr else None
            big = max(int(f[2]) for f in fr) if fr else None
            off = max(int(f[3]) / int(f[4]) for f in fr) if fr else None
            rows.append((name, a, d3d_ok, log.group(1) if log else 'none', len(fr), psnr, big, off, secs.groups() if secs else ('', '')))
            s = sums.setdefault(a, [0, 0, 0, [], 0])
            s[0] += 1
            s[1] += same
            s[2] += d3d_ok
            if psnr is not None:
                s[3].append(psnr)
            s[4] += len(fr)

    print('| case | adapter | D3D11 port vs game | kgl log D3D11 vs wgpu | frames | PSNR min (dB) | largest diff | pixels off >8 | s (d3d / wgpu) |')
    print('|---|---|---|---|---|---|---|---|---|')
    for name, a, d3d_ok, log, n, psnr, big, off, secs in rows:
        print(f'| {name} | {a} | {"same bits" if d3d_ok else "DIFFERENT"} | {log} | {n} | {psnr if psnr is None else f"{psnr:.2f}"} | {big} | {"" if off is None else f"{off * 100:.1f} %"} | {secs[0]} / {secs[1]} |')
    print()
    print('| adapter | cases | kgl log identical | D3D11 port = game | frames compared | PSNR min / median (dB) |')
    print('|---|---|---|---|---|---|')
    for a, (n, same, d3d, ps, frames) in sorted(sums.items()):
        ps.sort()
        print(f'| {a} | {n} | {same} | {d3d} | {frames} | {ps[0]:.2f} / {ps[len(ps) // 2]:.2f} |' if ps else f'| {a} | {n} | {same} | {d3d} | {frames} | - |')


if sys.argv[1] == "frame":
    frame(*sys.argv[2:5])
else:
    table(sys.argv[2])
