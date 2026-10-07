# SPDX-License-Identifier: MIT OR Apache-2.0

"""Writes src/physics_functions.tsv: address range and short name of every hand-written function
of the game's physics libraries, taken from the local index (re/index/functions.tsv, built by
tools/pdb_index.py). The oracle embeds the table to say which function a recorded force call
came from. Run from the repo root:  python tools/car_oracle/gen_functions.py
"""
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, "tools"))
import bulk_decomp

LIBS = {"ksPhysicsAC.lib", "ksPhysicsCoreODE.lib"}
rows = []
with open(os.path.join(ROOT, "re", "index", "functions.tsv"), encoding="utf-8") as f:
    next(f)
    for line in f:
        va, size, lib, obj, name = line.rstrip("\n").split("\t")
        if lib not in LIBS:
            continue
        q = bulk_decomp.qualified(name)
        # library templates and compiler helpers never hand a force to a body
        if q.startswith(("std::", "`", "Concurrency::")) or "dynamic initializer" in name \
                or "dynamic atexit" in name:
            continue
        q = re.sub(r"\s+", "", q)
        rows.append((int(va, 16), int(size), q))
rows.sort()
out = os.path.join(ROOT, "tools", "car_oracle", "src", "physics_functions.tsv")
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for va, size, q in rows:
        f.write(f"{va:x}\t{size}\t{q}\n")
print(len(rows), "functions ->", out)
