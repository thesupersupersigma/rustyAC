"""Read-only: cross-reference index of acs.exe, built by disassembling every function once.

Needs re/index/functions.tsv + publics.tsv (run tools/pdb_index.py first).
Usage: xref_index.py [index_dir]       (default: re/index)

Writes (tab separated, grep-able):
  calls.tsv         caller_va, caller, kind, callee_va, callee
                    kind: call / jmp (tail call) / ptr (address of a function taken) / import
  func_strings.tsv  func_va, func, kind (A/W), text      string literals a function refers to
                                                          (ini section/key names are W literals)
  func_vtables.tsv  func_va, func, vtable_va, vtable     vtables a function installs/uses
  func_globals.tsv  func_va, func, data_va, symbol       named global data a function touches
Virtual calls (call [reg+off]) cannot be resolved here; use vtables.txt for those.
"""
import os, re, struct, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "_pydeps"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import capstone
from disasm import Image

RIP = re.compile(r"\[rip ([+-]) (0x[0-9a-f]+|\d+)\]")


def load_tsv(path):
    rows = []
    with open(path, encoding="utf-8") as f:
        next(f)
        for l in f:
            p = l.rstrip("\n").split("\t")
            rows.append(p)
    return rows


def text_at(img, va):
    raw = img.read(va, 400) or b""
    if len(raw) > 3 and raw[1] == 0 and raw[0]:
        return "W", raw.decode("utf-16le", "replace").split("\0")[0]
    return "A", raw.split(b"\0")[0].decode("latin1")


def clean(s):
    return "".join(c if 32 <= ord(c) < 127 else "?" for c in s)[:300]


def main(idx):
    img = Image()
    funcs = {}
    for va, size, lib, obj, name in load_tsv(os.path.join(idx, "functions.tsv")):
        funcs[int(va, 16)] = (int(size), name)
    names = {va: n for va, (s, n) in funcs.items()}
    strings, vtables, data = set(), {}, {}
    text_lo = img.base + img.secs[0][0]
    text_hi = text_lo + img.secs[0][1]
    for va, size, lib, obj, name in load_tsv(os.path.join(idx, "publics.tsv")):
        va = int(va, 16)
        if name == "`string'":
            strings.add(va)
        elif "`vftable'" in name:
            vtables[va] = name
        elif text_lo <= va < text_hi:
            if va not in funcs:
                funcs[va] = (int(size), name)
                names[va] = name
        else:
            data.setdefault(va, name)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    out_c = open(os.path.join(idx, "calls.tsv"), "w", encoding="utf-8", newline="\n")
    out_s = open(os.path.join(idx, "func_strings.tsv"), "w", encoding="utf-8", newline="\n")
    out_v = open(os.path.join(idx, "func_vtables.tsv"), "w", encoding="utf-8", newline="\n")
    out_g = open(os.path.join(idx, "func_globals.tsv"), "w", encoding="utf-8", newline="\n")
    out_c.write("caller_va\tcaller\tkind\tcallee_va\tcallee\n")
    out_s.write("func_va\tfunc\tkind\ttext\n")
    out_v.write("func_va\tfunc\tvtable_va\tvtable\n")
    out_g.write("func_va\tfunc\tdata_va\tsymbol\n")
    n = 0
    for va in sorted(funcs):
        size, name = funcs[va]
        if size <= 0:
            continue
        code = img.read(va, size)
        if not code:
            continue
        seen = set()
        for addr, isz, mn, ops in md.disasm_lite(code, va):
            if mn in ("call", "jmp") and ops.startswith("0x"):
                t = int(ops, 16)
                if not (va <= t < va + size):
                    # jmp thunk to an import?
                    raw = img.read(t, 6)
                    if raw and raw[:2] == b"\xff\x25":
                        slot = t + 6 + struct.unpack_from("<i", raw, 2)[0]
                        if slot in img.imports:
                            key = ("import", slot)
                            if key not in seen:
                                seen.add(key)
                                out_c.write("%x\t%s\timport\t%x\t%s\n" % (va, name, slot, img.imports[slot]))
                            continue
                    key = (mn, t)
                    if key not in seen:
                        seen.add(key)
                        out_c.write("%x\t%s\t%s\t%x\t%s\n" % (va, name, mn, t, names.get(t, "?")))
                continue
            m = RIP.search(ops)
            if not m:
                continue
            disp = int(m.group(2), 0)
            t = addr + isz + (disp if m.group(1) == "+" else -disp)
            if t in img.imports:
                key = ("import", t)
                if key not in seen:
                    seen.add(key)
                    out_c.write("%x\t%s\timport\t%x\t%s\n" % (va, name, t, img.imports[t]))
            elif t in strings:
                key = ("s", t)
                if key not in seen:
                    seen.add(key)
                    kind, txt = text_at(img, t)
                    out_s.write("%x\t%s\t%s\t%s\n" % (va, name, kind, clean(txt)))
            elif t in vtables:
                key = ("v", t)
                if key not in seen:
                    seen.add(key)
                    out_v.write("%x\t%s\t%x\t%s\n" % (va, name, t, vtables[t]))
            elif t in names and mn == "lea":
                key = ("ptr", t)
                if key not in seen and t != va:
                    seen.add(key)
                    out_c.write("%x\t%s\tptr\t%x\t%s\n" % (va, name, t, names[t]))
            elif t in data:
                key = ("g", t)
                if key not in seen:
                    seen.add(key)
                    out_g.write("%x\t%s\t%x\t%s\n" % (va, name, t, data[t]))
        n += 1
    for f in (out_c, out_s, out_v, out_g):
        f.close()
    print("functions scanned:", n)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else
         os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "re", "index"))
