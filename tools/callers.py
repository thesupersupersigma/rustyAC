# SPDX-License-Identifier: MIT OR Apache-2.0

"""Read-only: find callers of functions by scanning acs.exe for direct CALL/JMP rel32 and
for 8-byte pointers (vtables / function-pointer tables). Caller entry points are resolved
through the x64 unwind table (.pdata), following chained unwind info to the function start.
"""
import struct, bisect

EXE = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe"


class Image:
    def __init__(self):
        with open(EXE, "rb") as f:
            self.d = d = f.read()
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        nsec = struct.unpack_from("<H", d, pe + 6)[0]
        opt = pe + 24
        self.base = struct.unpack_from("<Q", d, opt + 24)[0]
        exc_rva, exc_size = struct.unpack_from("<II", d, opt + 112 + 3 * 8)
        sec = opt + struct.unpack_from("<H", d, pe + 20)[0]
        self.secs = []
        for i in range(nsec):
            name = d[sec + i * 40: sec + i * 40 + 8].rstrip(b"\0").decode()
            vsize, va, rsize, raw = struct.unpack_from("<IIII", d, sec + i * 40 + 8)
            self.secs.append((name, va, vsize, raw, rsize))
        off = self.off(exc_rva)
        self.rf = sorted(struct.unpack_from("<III", d, off + i * 12) for i in range(exc_size // 12))
        self.rf = [r for r in self.rf if r[0]]
        self.begins = [r[0] for r in self.rf]

    def off(self, rva):
        for name, va, vsize, raw, rsize in self.secs:
            if va <= rva < va + rsize:
                return raw + rva - va
        return None

    def entry_of(self, va):
        """Entry VA of the function containing va (None for leaf functions without unwind data)."""
        rva = va - self.base
        i = bisect.bisect_right(self.begins, rva) - 1
        if i < 0 or not (self.rf[i][0] <= rva < self.rf[i][1]):
            return None
        b, e, u = self.rf[i]
        for _ in range(16):
            o = self.off(u)
            flags = self.d[o] >> 3
            if not flags & 4:
                break
            n = self.d[o + 2]
            b, e, u = struct.unpack_from("<III", self.d, o + 4 + 2 * ((n + 1) & ~1))
        return self.base + b

    def scan(self, targets):
        """-> {target: {"calls": [(site, kind)], "ptrs": [va]}}"""
        tset = set(targets)
        out = {t: {"calls": [], "ptrs": []} for t in targets}
        d = self.d
        for name, va, vsize, raw, rsize in self.secs:
            blob = d[raw: raw + rsize]
            if name == ".text":
                for op, kind in ((0xE8, "CALL"), (0xE9, "JMP")):
                    p = blob.find(bytes([op]))
                    while p != -1 and p + 5 <= len(blob):
                        rel = struct.unpack_from("<i", blob, p + 1)[0]
                        t = self.base + va + p + 5 + rel
                        if t in tset:
                            out[t]["calls"].append((self.base + va + p, kind))
                        p = blob.find(bytes([op]), p + 1)
            elif name in (".rdata", ".data"):
                for p in range(0, len(blob) - 7, 8):
                    t = struct.unpack_from("<Q", blob, p)[0]
                    if t in tset:
                        out[t]["ptrs"].append(self.base + va + p)
        return out
