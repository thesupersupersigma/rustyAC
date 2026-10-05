"""Read-only: dump struct/class layouts straight from acs.pdb via the DIA SDK (msdia140.dll).

Usage: pdb_types.py <out_dir> <TypeName> [<TypeName> ...]
Writes <out_dir>/<TypeName>.txt with offset / size / type / name for every data member.
"""
import ctypes, glob, os, re, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "_pydeps"))
import comtypes, comtypes.client
from comtypes.server import IClassFactory

PDB = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.pdb"
DIA = glob.glob(r"C:\Program Files\Microsoft Visual Studio\*\*\Common7\IDE\msdia140.dll")[0]

comtypes.client.GetModule(DIA)
from comtypes.gen import Dia2Lib as dia

BASIC = {1: "void", 2: "char", 3: "wchar_t", 6: "int", 7: "uint", 8: "float", 10: "bool",
         13: "long", 14: "ulong", 9: "bcd", 31: "hresult"}
T_UDT, T_ENUM, T_FUNC, T_PTR, T_ARR, T_BASE, T_TYPEDEF, T_BASECLASS, T_DATA = 11, 12, 13, 14, 15, 16, 17, 18, 7


def session():
    dll = ctypes.OleDLL(DIA)
    cf = ctypes.POINTER(IClassFactory)()
    dll.DllGetClassObject(ctypes.byref(dia.DiaSource._reg_clsid_), ctypes.byref(IClassFactory._iid_),
                          ctypes.byref(cf))
    src = cf.CreateInstance(interface=dia.IDiaDataSource)
    src.loadDataFromPdb(PDB)
    return src.openSession()


def tname(t):
    if t is None:
        return "?"
    tag = t.symTag
    if tag in (T_UDT, T_ENUM, T_TYPEDEF):
        return t.name
    if tag == T_BASE:
        b, n = t.baseType, t.length
        if b in (6, 7, 13, 14):
            return ("u" if b in (7, 14) else "") + {1: "int8", 2: "int16", 4: "int32", 8: "int64"}.get(n, "int?")
        if b == 8:
            return "float" if n == 4 else "double"
        return BASIC.get(b, "base%d" % b)
    if tag == T_PTR:
        return tname(t.type) + (" &" if t.reference else " *")
    if tag == T_ARR:
        return "%s[%d]" % (tname(t.type), t.count)
    if tag == T_FUNC:
        return "<function>"
    return "<tag%d>" % tag


def children(sym, tag):
    en = sym.findChildren(tag, None, 0)
    out = []
    for i in range(en.count):
        out.append(en.Item(i))
    return out


def dump(sess, name, out_dir):
    en = sess.globalScope.findChildren(T_UDT, name, 1)
    best = None
    for i in range(en.count):
        s = en.Item(i)
        if s.length and (best is None or len(children(s, T_DATA)) > len(children(best, T_DATA))):
            best = s
    if best is None:
        print("NOT FOUND", name)
        return
    rows = []
    for b in children(best, T_BASECLASS):
        rows.append((b.offset, b.type.length, b.type.name, "<base class>"))
    if best.virtualTableShape is not None and not children(best, T_BASECLASS):
        pass
    for d in children(best, T_DATA):
        if d.locationType not in (4, 6):      # this-relative or bitfield only (skip statics)
            continue
        t = d.type
        nm = d.name + (" : %d bits @ bit %d" % (d.length, d.bitPosition) if d.locationType == 6 else "")
        rows.append((d.offset, t.length if t is not None else 0, tname(t), nm))
    rows.sort(key=lambda r: r[0])
    path = os.path.join(out_dir, re.sub(r"[^A-Za-z0-9_]+", "_", name) + ".txt")
    with open(path, "w", encoding="utf-8") as f:
        f.write("Type   : %s\nSize   : %d (0x%x) bytes\nSource : acs.pdb type records (read with the DIA SDK, read-only)\n\n"
                % (name, best.length, best.length))
        f.write("%-8s %-8s %-6s %-58s %s\n" % ("offset", "hex", "size", "type", "name"))
        for off, size, ty, nm in rows:
            f.write("%-8d 0x%-6x %-6d %-58s %s\n" % (off, off, size, ty, nm))
    print("%-22s size 0x%-5x members %d" % (name, best.length, len(rows)))


if __name__ == "__main__":
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    s = session()
    for n in sys.argv[2:]:
        dump(s, n, out)
