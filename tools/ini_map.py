"""Map tyres.ini keys read in Tyre::initCompounds to TyreCompoundDef fields.

The decompiler leaves the function's local TyreCompoundDef untyped, so key values land in
stack slots (local_XXX). WIDTH -> local_4d0 pins the local def at stack 0x7a8 (data.width is
def+0x2d8), so def offset = 0x7a8 - XXX; offsets are resolved with re/tyre/types/*.txt.
"""
import re, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from re_cache import load

T = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "re", "tyre", "types")


def layout(name):
    rows = []
    for l in open(os.path.join(T, name + ".txt"), encoding="utf-8").read().splitlines()[5:]:
        m = re.match(r"(\d+)\s+0x[0-9a-f]+\s+(\d+)\s+(.*?)\s{2,}(\S.*)$", l)
        if m:
            rows.append((int(m[1]), int(m[2]), m[3].strip(), m[4].strip()))
        else:
            m = re.match(r"(\d+)\s+0x[0-9a-f]+\s+(\d+)\s+(.*\S)\s+(\S+)$", l)
            if m:
                rows.append((int(m[1]), int(m[2]), m[3].strip(), m[4].strip()))
    return rows


NEST = {"TyreModelData", "TyreData", "BrushSlipProvider", "TyrePatchData", "BrushTyreModel", "BrushTyreModelData"}


def resolve(tname, off):
    for o, size, ty, nm in layout(tname):
        if o <= off < o + max(size, 1):
            if ty in NEST:
                return nm + "." + resolve(ty, off - o)
            return nm + ("+0x%x" % (off - o) if off != o else "")
    return "?+0x%x" % off


if __name__ == "__main__":
    code = load()["140280800"]["code"].splitlines()
    key = None
    for i, l in enumerate(code):
        m = re.search(r'L"([A-Z0-9_]+)"', l)
        if m:
            key = m[1]
        m = re.search(r"(\S+) = INIReader::get(\w+)\(", l)
        if m and key:
            dst = m[1]
            lm = re.match(r"local_([0-9a-f]+)$", dst)
            if lm and int(lm[1], 16) <= 0x7a8:
                off = 0x7a8 - int(lm[1], 16)
                dst += " -> TyreCompoundDef+0x%x = %s" % (off, resolve("TyreCompoundDef", off))
            print("%4d %-26s get%-6s %s" % (i, key, m[2], dst))
