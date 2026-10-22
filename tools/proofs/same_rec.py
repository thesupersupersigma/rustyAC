# SPDX-License-Identifier: GPL-3.0-or-later
"""same_rec.py <kept> <new>: exit 0 when two recordings of the game are the same bytes, apart
from the game's heap address. The game never writes `bounce_vel` of its mesh contact joints: they
get the upper half of the address of a car's mesh geom, which changes from run to run. It shows in
the header lines mesh_bounce_vel= / mesh_geom_high= and, in a several-car recording (.mcrec), in
the row words world.contactHash (a hash over every contact joint, bounce_vel among them) and
world.meshBounceVel of the steps where a contact bounces. Those two words may differ only in a step
where world.meshBounceVel is each run's own address; everything else (the number of contact
joints, every car's columns) must be the same bytes. Prints the first other difference."""
import re
import struct
import sys

ADDR = re.compile(rb"(?<=\n)(mesh_bounce_vel|mesh_geom_high)=([0-9a-f,]*)(?=\n)")
MCREC_WORLD = 5  # WORLD_COLUMNS of crates/rustyac-physics/src/engine/record.rs


def header_end(b):
    """(start of the rows, words a row, the mesh_bounce_vel of the header) of an .mcrec, else None."""
    if len(b) < 12 or not b.startswith(b"RYMCREC"):
        return None
    text_len = struct.unpack_from("<I", b, 8)[0]
    text = b[12:12 + text_len]
    words = struct.unpack_from("<I", b, 12 + text_len)[0]
    vel = re.search(rb"(?:^|\n)mesh_bounce_vel=([0-9a-f]+)\n", text)
    return 12 + text_len + 8, words, int(vel.group(1), 16) if vel else None


def norm_header(b):
    head, tail = b[:65536], b[65536:]
    return ADDR.sub(lambda m: m.group(1) + b"=*", head) + tail


def first_difference(a, b):
    return next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))


def main(kept, new):
    a, b = open(kept, "rb").read(), open(new, "rb").read()
    if a == b:
        return None
    na, nb = norm_header(a), norm_header(b)
    if na == nb:
        return None
    ha, hb = header_end(a), header_end(b)
    if ha is None or hb is None or ha[0] != hb[0] or ha[1] != hb[1] or len(a) != len(b) \
            or na[:ha[0]] != nb[:hb[0]]:
        return f"first difference at byte {first_difference(na, nb)} (sizes {len(a)}, {len(b)})"
    start, words, vel_a = ha
    vel_b = hb[2]
    row = words * 4
    rows_a, rows_b = a[start:], b[start:]
    for at in range(0, len(rows_a), row):
        ra, rb = rows_a[at:at + row], rows_b[at:at + row]
        if ra == rb:
            continue
        # words 0..4: contactJoints, contactHash, meshBounceVel, noCollisionCounter, currentFrame
        wa = struct.unpack_from("<5I", ra)
        wb = struct.unpack_from("<5I", rb)
        explained = (wa[0] == wb[0] and wa[3:] == wb[3:] and wa[2] == vel_a and wb[2] == vel_b
                     and vel_a != vel_b and ra[MCREC_WORLD * 4:] == rb[MCREC_WORLD * 4:])
        if not explained:
            return f"first difference in step {at // row} at byte {start + at + first_difference(ra, rb)}"
    return None


if __name__ == "__main__":
    why = main(sys.argv[1], sys.argv[2])
    if why:
        print(why)
        sys.exit(1)
