# SPDX-License-Identifier: GPL-3.0-or-later
"""Writes an AI line of file version 6 from an installed version 7 one (Task 27's "fast lane of
file version 6": the game rebuilds such a line when it loads it, the port does the same in memory).
  python tools/proofs/ai_v6.py <fast_lane.ai of version 7> <out.ai>
Version 6 keeps per point: position, speed, gas, brake, lateral g, radius, two sides, camber,
direction, normal, length (16 floats); version 7 keeps the positions and these as two tables."""
import struct
import sys

src, dst = sys.argv[1], sys.argv[2]
b = open(src, "rb").read()
version, count, lap_time, _samples = struct.unpack_from("<iIIi", b, 0)
assert version == 7, f"{src}: version {version}, not 7"
at = 16
points = [struct.unpack_from("<3f", b, at + 20 * i) for i in range(count)]
at += 20 * count
(payloads,) = struct.unpack_from("<I", b, at)
assert payloads == count, f"{count} points but {payloads} payloads"
at += 4
out = [struct.pack("<iII", 6, count, lap_time)]
for i in range(count):
    p = struct.unpack_from("<18f", b, at + 72 * i)
    out.append(struct.pack("<3f", *points[i]) + struct.pack("<13f", *p[:13]))
open(dst, "wb").write(b"".join(out))
assert len(b"".join(out)) == 12 + 64 * count
print(f"{dst}: version 6, {count} points")
