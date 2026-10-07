# SPDX-License-Identifier: MIT OR Apache-2.0

"""Read-only lookups over the acs.exe index (re/index, re/decomp, re/types). No Ghidra needed.

Build the index first: tools/pdb_index.py, tools/xref_index.py, tools/bulk_decomp.py.

  re_query.py find <regex> [...]         functions whose qualified name matches: va, size, obj, name
  re_query.py decomp <name-regex|va>     path(s) of the pseudo-C file(s) in re/decomp
  re_query.py callers <name-regex|va>    direct callers (call / tail jmp / address taken)
  re_query.py callees <name-regex|va>    what a function calls directly, in address order
  re_query.py strings <name-regex|va>    string literals a function refers to (ini sections/keys ...)
  re_query.py uses <text-regex>          functions that refer to a string literal matching the regex
  re_query.py globals <name-regex|va>    named global data a function touches
  re_query.py vtable <Class>             vtable slots of a class (offset, va, method)
  re_query.py slot <hex-offset> <Class-regex>   which method sits at a vtable offset, per class
  re_query.py type <TypeName>            struct layout from the PDB (offset, size, type, member)
  re_query.py size <Obj> [...]           function count / code bytes of object files (stems or regex)
  re_query.py objs <regex>               object files (source files) matching, with their library
  re_query.py src <regex>                source file paths recorded in the PDB
  re_query.py checkdoc <file.md> [...]   verify every 0x14....... address quoted in a document
  re_query.py coverage <docs-dir> [...]  object files that no .md under those folders mentions

Notes: tiny identical functions are merged by the linker, so one address can carry many names
(shown as "+N aliases"). Virtual calls are not in the call index: use `vtable` / `slot`.
"""
import collections, os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IDX = os.path.join(ROOT, "re", "index")
DEC = os.path.join(ROOT, "re", "decomp")
TYP = os.path.join(ROOT, "re", "types")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def tsv(name, base=IDX):
    with open(os.path.join(base, name), encoding="utf-8") as f:
        next(f)
        for l in f:
            yield l.rstrip("\n").split("\t")


def qualified(und):
    import bulk_decomp
    return bulk_decomp.qualified(und)


_F = None


def funcs():
    """va -> (size, lib, obj, [names])"""
    global _F
    if _F is None:
        _F = {}
        for va, size, lib, obj, name in tsv("functions.tsv"):
            _F[int(va, 16)] = [int(size), lib, obj, [qualified(name)], name]
        lo, hi = 0x140001000, 0x1404a4000
        for va, size, lib, obj, name in tsv("publics.tsv"):
            v = int(va, 16)
            if not (lo <= v < hi) or name == "`string'":
                continue
            q = qualified(name)
            if v in _F:
                if q not in _F[v][3]:
                    _F[v][3].append(q)
            else:
                _F[v] = [int(size), lib, obj, [q], name]
    return _F


def resolve(arg):
    """-> list of va"""
    try:
        v = int(arg, 16)
        if v > 0x140000000:
            return [v]
    except ValueError:
        pass
    rx = re.compile(arg)
    hits = [va for va, f in funcs().items() if any(rx.search(n) for n in f[3])]
    exact = [va for va in hits if any(n == arg for n in funcs()[va][3])]
    return sorted(exact or hits)


def label(va, prefer=None):
    """Name of the function at va. Merged functions carry several names: prefer the one of
    class `prefer`, and say how many other names share the address."""
    f = funcs().get(va)
    if not f:
        return "?"
    name = f[3][0]
    if prefer:
        for n in f[3]:
            if n.startswith(prefer + "::"):
                name = n
                break
    extra = " (+%d aliases)" % (len(f[3]) - 1) if len(f[3]) > 1 else ""
    return name + extra


def containing(va):
    import bisect
    keys = containing.keys = getattr(containing, "keys", None) or sorted(funcs())
    i = bisect.bisect_right(keys, va) - 1
    if i >= 0 and keys[i] <= va < keys[i] + max(funcs()[keys[i]][0], 1):
        return keys[i]
    return None


def cmd_find(args):
    for a in args:
        for va in resolve(a):
            f = funcs()[va]
            print("%x %6d  %-28s %s" % (va, f[0], f[2], label(va)))


def decomp_paths(vas):
    want = set("%x" % v for v in vas)
    return [(r[0], r[5]) for r in tsv("INDEX.tsv", DEC) if r[0] in want]


def cmd_decomp(args):
    for a in args:
        vas = resolve(a)
        got = decomp_paths(vas)
        for va, rel in got:
            print("re/decomp/" + rel)
        missing = set("%x" % v for v in vas) - set(v for v, _ in got)
        for m in sorted(missing):
            print("# no pseudo-C for 0x%s %s  -> python tools/disasm.py dis %s" % (m, label(int(m, 16)), m))


def cmd_callers(args):
    for a in args:
        vas = set(resolve(a))
        for cva, cname, kind, tva, tname in tsv("calls.tsv"):
            if int(tva, 16) in vas:
                print("%-4s %s  %s   -> %s" % (kind, cva, qualified(cname), label(int(tva, 16))))


def cmd_callees(args):
    for a in args:
        vas = set(resolve(a))
        for cva, cname, kind, tva, tname in tsv("calls.tsv"):
            if int(cva, 16) in vas:
                t = int(tva, 16)
                print("%-6s %s  %s" % (kind, tva, tname if kind == "import" else label(t)))


def cmd_strings(args):
    for a in args:
        vas = set(resolve(a))
        for va, name, kind, text in tsv("func_strings.tsv"):
            if int(va, 16) in vas:
                print("%s %s  %s\"%s\"" % (va, qualified(name), "L" if kind == "W" else "", text))


def cmd_uses(args):
    for a in args:
        rx = re.compile(a)
        for va, name, kind, text in tsv("func_strings.tsv"):
            if rx.search(text):
                print("%s %-60s %s\"%s\"" % (va, qualified(name)[:60], "L" if kind == "W" else "", text))


def cmd_globals(args):
    for a in args:
        vas = set(resolve(a))
        for va, name, dva, sym in tsv("func_globals.tsv"):
            if int(va, 16) in vas:
                print("%s %s  %s %s" % (va, qualified(name), dva, sym))


def vtables():
    out, cur = collections.OrderedDict(), None
    with open(os.path.join(IDX, "vtables.txt"), encoding="utf-8") as f:
        for l in f:
            if not l.strip():
                continue
            if not l.startswith(" "):
                va, name = l.strip().split("  ", 1)
                cur = out.setdefault(name, (va, []))
            else:
                p = l.split(None, 2)
                cur[1].append((int(p[0], 16), int(p[1], 16)))
    return out


def cmd_vtable(args):
    for a in args:
        for name, (va, slots) in vtables().items():
            if name in ("const %s::`vftable'" % a,) or name.startswith("const %s::`vftable'{" % a):
                print("%s  %s" % (va, name))
                for off, t in slots:
                    print("  +0x%03x  %x  %s" % (off, t, label(t, a)))


def cmd_slot(args):
    off = int(args[0], 16)
    rx = re.compile(args[1])
    for name, (va, slots) in vtables().items():
        if rx.search(name):
            for o, t in slots:
                if o == off:
                    cls = name[6:].split("::`vftable'")[0]
                    print("%-52s +0x%x  %x  %s" % (name[6:58], off, t, label(t, cls)))


def cmd_type(args):
    for a in args:
        p = os.path.join(TYP, re.sub(r"[^A-Za-z0-9_]+", "_", a) + ".txt")
        if os.path.exists(p):
            sys.stdout.write(open(p, encoding="utf-8").read())
        else:
            print("no layout for %s; try: python tools/pdb_types.py re/types %s" % (a, a))


def cmd_size(args):
    comp = list(tsv("compilands.tsv"))
    real = collections.Counter(r[3].replace(".obj", "") for r in tsv("INDEX.tsv", DEC))
    tot = [0, 0, 0]
    for a in args:
        rx = re.compile("^(%s)$" % a)
        for lib, obj, n, b in comp:
            stem = obj.replace(".obj", "")
            if rx.match(stem):
                print("%-30s %-22s symbols %5s  code bytes %7s  hand-written functions %d"
                      % (stem, lib, n, b, real.get(stem, 0)))
                tot[0] += int(n); tot[1] += int(b); tot[2] += real.get(stem, 0)
    print("TOTAL symbols %d, code bytes %d, hand-written functions %d  (symbols include templates"
          " and compiler helpers; 'hand-written' = functions with pseudo-C in re/decomp)" % tuple(tot))


def cmd_objs(args):
    for a in args:
        rx = re.compile(a, re.I)
        for lib, obj, n, b in tsv("compilands.tsv"):
            if rx.search(obj):
                print("%-34s %-24s symbols %5s  code bytes %7s" % (obj, lib, n, b))


def cmd_src(args):
    for a in args:
        rx = re.compile(a, re.I)
        for l in open(os.path.join(IDX, "source_files.txt"), encoding="utf-8"):
            if rx.search(l):
                sys.stdout.write(l)


def cmd_checkdoc(args):
    data = {}
    for va, size, lib, obj, name in tsv("publics.tsv"):
        data.setdefault(int(va, 16), name)
    bad_total = 0
    for path in args:
        text = open(path, encoding="utf-8").read()
        addrs = sorted(set(int(m, 16) for m in re.findall(r"0x(14[0-9a-fA-F]{7})\b", text)))
        start = inside = other = 0
        bad = []
        for a in addrs:
            if a in funcs():
                start += 1
            elif containing(a) is not None:
                inside += 1
            elif a in data:
                other += 1
            else:
                bad.append("  UNKNOWN 0x%x (not a function, not inside one, not a named symbol)" % a)
        # name @ address pairs
        pairs = []
        for line in text.splitlines():
            one = len(re.findall(r"0x14[0-9a-fA-F]{7}\b", line)) == 1
            for m in re.finditer(r"`([A-Za-z_~][\w:~<>]*)`\s*(@|at|\(|\|)\s*`?0x(14[0-9a-fA-F]{7})", line):
                if m.group(2) in ("@", "at") or one:
                    pairs.append((m.group(1), int(m.group(3), 16)))
        for name, a in pairs:
            if a not in funcs() or "::" not in name:
                continue
            names = funcs()[a][3]
            short = name.split("(")[0]
            if not any(n == short or n.endswith("::" + short) or n.endswith(short) for n in names):
                bad.append("  NAME MISMATCH `%s` @ 0x%x is really %s" % (name, a, label(a)))
        print("%s: %d addresses: %d function starts, %d inside functions (call sites), %d data/vtables, %d problems"
              % (path, len(addrs), start, inside, other, len(bad)))
        for b in bad:
            print(b)
        bad_total += len(bad)
    sys.exit(1 if bad_total else 0)


def cmd_coverage(args):
    """Object files (source files) that no document under the given folders mentions."""
    text = ""
    for d in args:
        for base, _, files in os.walk(d):
            for f in files:
                if f.endswith(".md"):
                    text += open(os.path.join(base, f), encoding="utf-8").read() + "\n"
    words = set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text))
    real = collections.Counter()
    lib = {}
    for r in tsv("INDEX.tsv", DEC):
        stem = r[3].replace(".obj", "")
        real[stem] += 1
        lib[stem] = r[2]
    missing = [(n, s) for s, n in real.items() if s not in words]
    for n, s in sorted(missing, reverse=True):
        print("%4d  %-32s %s" % (n, s, lib[s]))
    print("%d of %d object files are not mentioned (%d of %d hand-written functions)"
          % (len(missing), len(real), sum(n for n, _ in missing), sum(real.values())))


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    if sys.argv[1] == "coverage":
        cmd_coverage(sys.argv[2:])
        sys.exit()
    {"find": cmd_find, "decomp": cmd_decomp, "callers": cmd_callers, "callees": cmd_callees,
     "strings": cmd_strings, "uses": cmd_uses, "globals": cmd_globals, "vtable": cmd_vtable,
     "slot": cmd_slot, "type": cmd_type, "size": cmd_size, "objs": cmd_objs, "src": cmd_src,
     "checkdoc": cmd_checkdoc}[sys.argv[1]](sys.argv[2:])
