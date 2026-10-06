"""Read-only: disassemble functions of acs.exe with names from acs.pdb (DIA SDK + capstone).

Usage:
  disasm.py find <pattern> [...]      -> address, size, name of functions matching a DIA
                                          wildcard pattern (e.g. "INIReader::get*")
  disasm.py dis <addr|name> [...]     -> annotated listing: call/jump targets are named,
                                          rip-relative constants are shown as float/double/int
  disasm.py range <lo> <hi>           -> listing of an address range (for local functions
                                          that have no name in the PDB; bounds from pdata.py)
  disasm.py vtable <addr> [count]     -> names of the functions a vtable points at
  disasm.py who <addr> [...]          -> name of the symbol containing each address

Needs `pip install --target tools/_pydeps capstone` (tools/_pydeps is gitignored).
"""
import os, struct, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "_pydeps"))
import capstone
from capstone import x86_const as X
import pdb_types

EXE = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe"
T_FUNCTION, T_DATA, T_PUBLIC = 5, 7, 10


class Image:
    def __init__(self):
        with open(EXE, "rb") as f:
            self.d = d = f.read()
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        nsec = struct.unpack_from("<H", d, pe + 6)[0]
        opt = pe + 24
        self.base = struct.unpack_from("<Q", d, opt + 24)[0]
        sec = opt + struct.unpack_from("<H", d, pe + 20)[0]
        self.secs = []
        for i in range(nsec):
            vsize, va, rsize, raw = struct.unpack_from("<IIII", d, sec + i * 40 + 8)
            self.secs.append((va, vsize, raw, rsize))
        imp = struct.unpack_from("<I", d, opt + 112 + 8)[0]
        self.imports = {}
        o = self.off(imp)
        while True:
            ilt, _, _, name, iat = struct.unpack_from("<IIIII", d, o)
            if not name:
                break
            dll = self.cstr(name)
            t = self.off(ilt or iat)
            i = 0
            while True:
                e = struct.unpack_from("<Q", d, t + i * 8)[0]
                if not e:
                    break
                fn = "#%d" % (e & 0xffff) if e >> 63 else self.cstr(e + 2)
                self.imports[self.base + iat + i * 8] = "%s!%s" % (dll, fn)
                i += 1
            o += 20

    def off(self, rva):
        for va, vsize, raw, rsize in self.secs:
            if va <= rva < va + rsize:
                return raw + rva - va
        return None

    def cstr(self, rva):
        o = self.off(rva)
        return self.d[o:self.d.index(b"\0", o)].decode("latin1")

    def read(self, va, n):
        o = self.off(va - self.base)
        return None if o is None else self.d[o:o + n]


class Syms:
    def __init__(self):
        self.sess = pdb_types.session()
        self.cache = {}

    def at(self, va, base):
        """(name, displacement) of the function/data/public symbol containing va."""
        if va in self.cache:
            return self.cache[va]
        out = None
        for tag in (T_FUNCTION, T_PUBLIC, T_DATA):
            try:
                s, disp = self.sess.findSymbolByRVAEx(va - base, tag)
                name = s.undecoratedName if tag == T_PUBLIC and s.undecoratedName else s.name
            except Exception:
                continue
            if name:
                out = (name, disp)
                break
        self.cache[va] = out
        return out

    def label(self, va, base):
        r = self.at(va, base)
        if r is None:
            return None
        return r[0] if r[1] == 0 else "%s+0x%x" % r

    def find(self, pattern):
        en = self.sess.globalScope.findChildren(T_FUNCTION, pattern, 8 | 2)
        out = []
        for i in range(en.count):
            s = en.Item(i)
            out.append((s.relativeVirtualAddress, s.length, s.undecoratedName or s.name))
        return sorted(set(out))


def resolve(arg, img, syms):
    try:
        return int(arg, 16), None
    except ValueError:
        hits = [h for h in syms.find(arg) if h[0]]
        if not hits:
            sys.exit("no function matches %r" % arg)
        if len(hits) > 1:
            for rva, size, name in hits:
                print("  %x %5d %s" % (img.base + rva, size, name))
            sys.exit("%r is ambiguous" % arg)
        return img.base + hits[0][0], hits[0][1]


def string_note(img, va):
    """Text of a string literal (UTF-16 or ASCII), printable characters only."""
    raw = img.read(va, 96) or b""
    if len(raw) > 3 and raw[1] == 0 and raw[0]:
        text = raw.decode("utf-16le", "replace").split(chr(0))[0]
        kind = 'L"%s"'
    else:
        text = raw.split(bytes(1))[0].decode("latin1")
        kind = '"%s"'
    text = "".join(c if 32 <= ord(c) < 127 else "?" for c in text)
    return kind % text


def const_note(img, va, size):
    raw = img.read(va, 8)
    if raw is None or len(raw) < 8:
        return ""
    f = struct.unpack_from("<f", raw)[0]
    dq = struct.unpack_from("<d", raw)[0]
    i = struct.unpack_from("<I", raw)[0]
    if size == 8:
        return "double %r / 0x%016x" % (dq, struct.unpack_from("<Q", raw)[0])
    return "float %r / 0x%08x" % (f, i)


def dis(img, syms, va, size):
    if size is None:
        size = 0
        for tag in (T_FUNCTION, T_PUBLIC):      # libraries without debug info only have publics
            try:
                s, _ = syms.sess.findSymbolByRVAEx(va - img.base, tag)
                size = s.length
            except Exception:
                continue
            if size:
                break
        size = size or 0x400
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    code = img.read(va, size)
    print("; %s  @ 0x%x  (%d bytes)" % (syms.label(va, img.base), va, size))
    for ins in md.disasm(code, va):
        note = ""
        for op in ins.operands:
            if op.type == X.X86_OP_IMM and (ins.group(X.X86_GRP_CALL) or ins.group(X.X86_GRP_JUMP)):
                t = op.imm
                if not (va <= t < va + size):
                    nm = syms.label(t, img.base)
                    # a thunk: jmp qword ptr [rip+x] to an import
                    raw = img.read(t, 6)
                    if raw and raw[:2] == b"\xff\x25":
                        slot = t + 6 + struct.unpack_from("<i", raw, 2)[0]
                        nm = img.imports.get(slot, nm)
                    note = nm or ""
            elif op.type == X.X86_OP_MEM and op.mem.base == X.X86_REG_RIP:
                t = ins.address + ins.size + op.mem.disp
                if t in img.imports:
                    note = img.imports[t]
                else:
                    nm = syms.label(t, img.base)
                    if nm and nm.startswith("`string'"):
                        note = string_note(img, t)
                    else:
                        note = "%s%s" % ((nm + "  ") if nm else "", const_note(img, t, op.size))
        print("%x  %-7s %-44s%s" % (ins.address, ins.mnemonic, ins.op_str,
                                     ("  ; " + note) if note else ""))


if __name__ == "__main__":
    img, syms = Image(), Syms()
    cmd = sys.argv[1]
    if cmd == "find":
        for pat in sys.argv[2:]:
            for rva, size, name in syms.find(pat):
                print("%x %5d %s" % (img.base + rva, size, name))
    elif cmd == "dis":
        for a in sys.argv[2:]:
            va, size = resolve(a, img, syms)
            dis(img, syms, va, size)
            print()
    elif cmd == "range":
        lo, hi = int(sys.argv[2], 16), int(sys.argv[3], 16)
        dis(img, syms, lo, hi - lo)
    elif cmd == "vtable":
        va = int(sys.argv[2], 16)
        n = int(sys.argv[3]) if len(sys.argv) > 3 else 32
        for i in range(n):
            t = struct.unpack("<Q", img.read(va + i * 8, 8))[0]
            nm = syms.label(t, img.base)
            if nm is None:
                break
            print("+0x%02x  %x  %s" % (i * 8, t, nm))
    elif cmd == "who":
        for a in sys.argv[2:]:
            print(a, syms.label(int(a, 16), img.base))
