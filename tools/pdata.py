# SPDX-License-Identifier: MIT OR Apache-2.0

"""Read-only: list function start addresses from acs.exe's .pdata (x64 unwind table).

Usage:
  pdata.py containing <addr> [...]   -> start of the function chunk containing each address
  pdata.py range <lo> <hi>           -> every function chunk start in [lo, hi)
"""
import struct, sys

EXE = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe"


def load():
    with open(EXE, "rb") as f:
        d = f.read()
    pe = struct.unpack_from("<I", d, 0x3C)[0]
    nsec = struct.unpack_from("<H", d, pe + 6)[0]
    opt = pe + 24
    base = struct.unpack_from("<Q", d, opt + 24)[0]
    exc_rva, exc_size = struct.unpack_from("<II", d, opt + 112 + 3 * 8)
    sec = opt + struct.unpack_from("<H", d, pe + 20)[0]
    off = None
    for i in range(nsec):
        vsize, va, rsize, raw = struct.unpack_from("<IIII", d, sec + i * 40 + 8)
        if va <= exc_rva < va + max(vsize, rsize):
            off = raw + exc_rva - va
    ents = [struct.unpack_from("<III", d, off + i * 12) for i in range(exc_size // 12)]
    return base, [(base + b, base + e) for b, e, _ in ents if b]


if __name__ == "__main__":
    base, ents = load()
    if sys.argv[1] == "containing":
        for a in sys.argv[2:]:
            a = int(a, 16)
            hit = [(b, e) for b, e in ents if b <= a < e]
            print(hex(a), "->", ", ".join(f"{b:x}..{e:x}" for b, e in hit) or "none")
    elif sys.argv[1] == "range":
        lo, hi = int(sys.argv[2], 16), int(sys.argv[3], 16)
        for b, e in ents:
            if lo <= b < hi:
                print(f"{b:x} {e - b}")
