# SPDX-License-Identifier: GPL-3.0-or-later
"""Task W3: a Full-mode screenshot at a render scale below 100 % is one whole picture.

    python tools/proofs/scale_check.py <shot.png>
Prints `ok <sha256>` or `bad <why> <sha256>`, exit 0 / 1. Bad: a quarter of the window mostly
black or a hard edge along the middle row or column (the 3D picture in the top-left quarter,
over black or older frames, as before W3), or AC's app bar's red arrow at the top away from the
right edge (the GUI laid out at the smaller size; it flickers, so a shot can miss it).
"""
import hashlib
import sys

import numpy as np
from PIL import Image

path = sys.argv[1]
digest = hashlib.sha256(open(path, 'rb').read()).hexdigest()[:16]
px = np.asarray(Image.open(path).convert('RGB')).astype(int)
h, w, _ = px.shape
why = []
for name, q in (('top left', px[: h // 2, : w // 2]), ('top right', px[: h // 2, w // 2 :]), ('bottom left', px[h // 2 :, : w // 2]), ('bottom right', px[h // 2 :, w // 2 :])):
    dark = (q.max(axis=2) < 12).mean()
    if dark > 0.5:
        why.append(f'{name} {dark:.0%} black')
# the picture in the top-left quarter over older frames: a hard edge along the middle row and column
g = px.mean(axis=2)
for name, a in (('row', g), ('column', g.T)):
    n, m = a.shape
    step = lambda k: np.abs(a[k + 2] - a[k - 1])[: m // 2].mean()  # noqa: E731
    seam, typical = step(n // 2 - 1), np.median([step(k) for k in range(n // 2 - 60, n // 2 + 60)])
    if seam > 3 * typical + 2:
        why.append(f'an edge along the middle {name} ({seam:.0f} against {typical:.0f})')
top = px[:26, : w - 80]
red = ((top[..., 0] > 180) & (top[..., 1] < 40) & (top[..., 2] < 40)).sum()
if red > 100:
    why.append(f'{red} app-bar red pixels at the top left of x={w - 80}')
print(f"bad {'; '.join(why)} {digest}" if why else f'ok {digest}')
sys.exit(1 if why else 0)
