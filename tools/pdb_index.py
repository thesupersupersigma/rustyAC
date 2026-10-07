# SPDX-License-Identifier: MIT OR Apache-2.0

"""Read-only: build a grep-able index of acs.exe / acs.pdb in re/index/ (git-ignored).

Usage: pdb_index.py [out_dir]          (default: re/index)

Writes:
  functions.tsv   va, size, lib, obj, name      every function symbol, with the .lib/.obj it
                                                was compiled into (from the PDB compilands)
  publics.tsv     va, size, lib, obj, name      public symbols (covers libraries that were
                                                linked without debug info, e.g. ODE)
  compilands.tsv  lib, obj, functions, code_bytes
  types.tsv       size, name                    classes / structs
  vtables.txt     every `vftable' with its slots resolved to function names
  strings.tsv     va, kind (A = ASCII, W = UTF-16), text   literals in .rdata / .data
  exe_info.txt    PE timestamp, sections, version resource
"""
import bisect, os, re, struct, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pdb_types
from disasm import Image, EXE

T_COMPILAND, T_FUNCTION, T_PUBLIC, T_UDT = 2, 5, 10, 11


def short_lib(path):
    return os.path.basename(path or "") or "-"


def short_obj(path):
    return os.path.basename((path or "").replace("/", "\\")) or "-"


def main(out):
    os.makedirs(out, exist_ok=True)
    img = Image()
    sess = pdb_types.session()
    g = sess.globalScope
    base = img.base

    # --- section contributions: rva range -> compiland (works without debug info) ---
    from comtypes.gen import Dia2Lib as dia
    contribs = []
    tables = sess.getEnumTables()
    for ti in range(tables.count):
        try:
            sc = tables.Item(ti).QueryInterface(dia.IDiaEnumSectionContribs)
        except Exception:
            continue
        names = {}
        for i in range(sc.count):
            c = sc.Item(i)
            cid = c.compilandId
            if cid not in names:
                comp = c.compiland
                names[cid] = (comp.libraryName, comp.name)
            contribs.append((c.relativeVirtualAddress, c.length) + names[cid])
        break
    contribs.sort()
    cstarts = [c[0] for c in contribs]

    def owner(rva):
        i = bisect.bisect_right(cstarts, rva) - 1
        if i >= 0 and contribs[i][0] <= rva < contribs[i][0] + contribs[i][1]:
            return short_lib(contribs[i][2]), short_obj(contribs[i][3])
        return "-", "-"

    # --- functions ---
    funcs = {}
    en = g.findChildren(T_FUNCTION, None, 0)
    for i in range(en.count):
        s = en.Item(i)
        rva = s.relativeVirtualAddress
        if not rva:
            continue
        funcs[rva] = (s.length, s.undecoratedName or s.name)
    comp_stats = {}
    with open(os.path.join(out, "functions.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("va\tsize\tlib\tobj\tname\n")
        for rva in sorted(funcs):
            size, name = funcs[rva]
            lib, obj = owner(rva)
            st = comp_stats.setdefault((lib, obj), [0, 0])
            st[0] += 1
            st[1] += size
            f.write("%x\t%d\t%s\t%s\t%s\n" % (base + rva, size, lib, obj, name))

    # --- publics ---
    pubs = {}
    en = g.findChildren(T_PUBLIC, None, 0)
    for i in range(en.count):
        s = en.Item(i)
        rva = s.relativeVirtualAddress
        if not rva:
            continue
        pubs.setdefault(rva, []).append((s.length, s.undecoratedName or s.name, s.name))
    with open(os.path.join(out, "publics.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("va\tsize\tlib\tobj\tname\n")
        for rva in sorted(pubs):
            lib, obj = owner(rva)
            for size, und, dec in pubs[rva]:
                f.write("%x\t%d\t%s\t%s\t%s\n" % (base + rva, size, lib, obj, und))
                if rva not in funcs and lib != "-" and img.off(rva) is not None:
                    pass
    # functions known only as publics (no debug info) count for their compiland too
    text = [s for s in img.secs if s[0] <= 0x1000 < s[0] + s[3]] or img.secs[:1]
    tlo, thi = text[0][0], text[0][0] + text[0][1]
    for rva in pubs:
        if rva in funcs or not (tlo <= rva < thi):
            continue
        lib, obj = owner(rva)
        st = comp_stats.setdefault((lib, obj), [0, 0])
        st[0] += 1
        st[1] += pubs[rva][0][0]
    with open(os.path.join(out, "compilands.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("lib\tobj\tfunctions\tcode_bytes\n")
        for (lib, obj), (n, b) in sorted(comp_stats.items()):
            f.write("%s\t%s\t%d\t%d\n" % (lib, obj, n, b))

    # --- types ---
    seen = {}
    en = g.findChildren(T_UDT, None, 0)
    for i in range(en.count):
        s = en.Item(i)
        if s.length and s.name not in seen:
            seen[s.name] = s.length
    with open(os.path.join(out, "types.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("size\tname\n")
        for n in sorted(seen):
            f.write("0x%x\t%s\n" % (seen[n], n))

    # --- vtables ---
    def label(va):
        rva = va - base
        if rva in funcs:
            return funcs[rva][1]
        if rva in pubs:
            return pubs[rva][0][1]
        return None

    vts = sorted((rva, p[0][1]) for rva, p in pubs.items() if "`vftable'" in p[0][1])
    vstarts = set(r for r, _ in vts)
    with open(os.path.join(out, "vtables.txt"), "w", encoding="utf-8", newline="\n") as f:
        for rva, name in vts:
            f.write("%x  %s\n" % (base + rva, name))
            for i in range(512):
                if i and (rva + i * 8) in vstarts:
                    break
                raw = img.read(base + rva + i * 8, 8)
                if raw is None:
                    break
                t = struct.unpack("<Q", raw)[0]
                nm = label(t)
                if nm is None:
                    break
                f.write("    +0x%03x  %x  %s\n" % (i * 8, t, nm))
            f.write("\n")

    # --- strings ---
    with open(os.path.join(out, "strings.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("va\tkind\ttext\n")
        d = img.d
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        nsec = struct.unpack_from("<H", d, pe + 6)[0]
        sec = pe + 24 + struct.unpack_from("<H", d, pe + 20)[0]
        for i in range(nsec):
            name = d[sec + i * 40: sec + i * 40 + 8].rstrip(b"\0").decode()
            vsize, va, rsize, raw = struct.unpack_from("<IIII", d, sec + i * 40 + 8)
            if name not in (".rdata", ".data"):
                continue
            blob = d[raw: raw + rsize]
            rows = []
            for m in re.finditer(rb"(?:[\x20-\x7e]\x00){3,}\x00\x00", blob):
                if m.start() % 2 == 0:
                    rows.append((m.start(), "W", m.group(0)[:-2].decode("utf-16le")))
            for m in re.finditer(rb"[\x20-\x7e]{4,}\x00", blob):
                rows.append((m.start(), "A", m.group(0)[:-1].decode("latin1")))
            for o, kind, text in sorted(rows):
                f.write("%x\t%s\t%s\n" % (base + va + o, kind, text))

    # --- exe info ---
    with open(os.path.join(out, "exe_info.txt"), "w", encoding="utf-8", newline="\n") as f:
        d = img.d
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        ts = struct.unpack_from("<I", d, pe + 8)[0]
        f.write("file        : %s\nsize        : %d bytes\nimage base  : 0x%x\n" % (EXE, len(d), base))
        f.write("PE timestamp: %d = %s UTC\n" % (ts, time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(ts))))
        m = re.search("F\0i\0l\0e\0V\0e\0r\0s\0i\0o\0n\0".encode("latin1"), d)
        for key in ("FileVersion", "ProductVersion", "ProductName", "FileDescription", "CompanyName"):
            k = key.encode("utf-16le")
            p = d.find(k + b"\0\0")
            if p != -1:
                q = p + len(k) + 2
                q += (4 - q % 4) % 4
                val = d[q:q + 200].decode("utf-16le", "replace").split("\0")[0]
                f.write("%-12s: %s\n" % (key, val))
        f.write("functions   : %d (PDB function symbols), %d public symbols\n" % (len(funcs), len(pubs)))
    print("functions %d, publics %d, compilands %d, types %d, vtables %d"
          % (len(funcs), len(pubs), len(comp_stats), len(seen), len(vts)))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else
         os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "re", "index"))
