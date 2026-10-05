"""Write re/tyre/decomp/<Class>__<method>.c from the kawaiidra decompile cache.

The pseudo-C body is Ghidra's output byte-for-byte (line endings normalised to LF). The header
(address, size, callers) is computed read-only from acs.exe: direct CALL/JMP sites and pointer
slots, with caller entry points resolved through the unwind table.
"""
import os, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from re_cache import load
from callers import Image

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "re", "tyre", "decomp")
PREFIX = ("Tyre::", "TyreThermalModel::", "TyreModelData::", "TyreCompoundDef::", "SCTM::",
          "BrushTyreModel::", "BrushSlipProvider::", "TyreTester::", "ksCalc", "calcLoadSensMult")
EXTRA = {"Car::step", "Car::stepComponents", "PhysicsEngine::step", "PhysicsDriveThread::step",
         "PhysicsDriveThread::run", "Car::onTyresStepCompleted", "Car::getTyreThermalState",
         "Car::getTyreMatrix", "RaceEngineer::evalTyreLoad", "RaceEngineer::getDrivingTyresSlip",
         "RaceEngineer::findTyreCompound"}


def fname(q):
    if "::" in q:
        cls, m = q.rsplit("::", 1)
    else:
        cls, m = "global", q
    m = "dtor" if m.startswith("~") else m
    m = "operator_assign" if m == "operator=" else m
    return re.sub(r"[^A-Za-z0-9_]+", "_", cls) + "__" + re.sub(r"[^A-Za-z0-9_]+", "_", m)


if __name__ == "__main__":
    fns = load()
    img = Image()
    sel = sorted(a for a, f in fns.items() if f["qual"].startswith(PREFIX) or f["qual"] in EXTRA)
    res = img.scan([int(a, 16) for a in sel])
    sizes = {}
    for b, e, _ in img.rf:
        ent = img.entry_of(img.base + b)
        sizes[ent] = sizes.get(ent, 0) + (e - b)
    ents = sorted(int(a, 16) for a in fns)
    os.makedirs(OUT, exist_ok=True)
    used, index = {}, []
    for a in sel:
        f, va = fns[a], int(a, 16)
        base = fname(f["qual"])
        if base in used:
            base += "_" + a
        used[base] = 1
        rows = []
        for site, kind in sorted(res[va]["calls"]):
            ent = img.entry_of(site)
            if ent is None:          # leaf caller without unwind data: nearest decompiled function
                c = [e for e in ents if e <= site < e + 0x100]
                ent = c[-1] if c else None
            who = fns.get("%x" % ent, {}).get("qual") if ent else None
            if ent == va:
                continue             # recursion / internal jump
            rows.append("%s @ 0x%x  (%s at 0x%x)" % (who or "<not decompiled>", ent or 0, kind.lower(), site) if ent else "<thunk or leaf code without unwind data>  (%s at 0x%x)" % (kind.lower(), site))
        size = sizes.get(va)
        h = ["/*", " * Function : " + f["qual"], " * Signature: " + f["sig"], " * Address  : 0x" + a,
             " * Size     : " + ("%d bytes (sum of unwind-table chunks)" % size if size else
                                 "n/a (leaf function, no unwind-table entry)"),
             " * Callers  :"]
        h += [" *   " + r for r in rows] or [" *   (no direct call sites)"]
        if res[va]["ptrs"]:
            h.append(" * Pointer slots (vtable / function-pointer tables) holding this address:")
            h += [" *   0x%x" % p for p in res[va]["ptrs"]]
        h += [" * Source   : Ghidra 12.1.2 pseudo-C of acs.exe with acs.pdb loaded, via kawaiidra MCP.",
              " *            Body below is unmodified decompiler output.", " */", ""]
        with open(os.path.join(OUT, base + ".c"), "w", encoding="utf-8", newline="\n") as o:
            o.write("\n".join(h) + f["code"].lstrip("\n"))
        index.append((a, size or 0, f["qual"], base + ".c", len(rows)))
    with open(os.path.join(OUT, "..", "functions.tsv"), "w", encoding="utf-8", newline="\n") as o:
        o.write("address\tsize\tqualified_name\tfile\tdirect_call_sites\n")
        for r in index:
            o.write("0x%s\t%d\t%s\t%s\t%d\n" % r)
    print(len(index), "files written")
