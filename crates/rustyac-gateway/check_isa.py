# SPDX-License-Identifier: MIT OR Apache-2.0
"""Which processor does a Linux build of the gateway need?

    python crates/rustyac-gateway/check_isa.py <the ELF binary>

Disassembles every function of a static x86-64 ELF file (it must not be stripped: the
functions come from its symbol table) and lists every instruction that is not in the
x86-64 baseline (SSE and SSE2): SSE3, SSSE3, SSE4, AVX, AVX2, AVX-512, FMA, BMI, AES ...
with the function it is in. The gateway's servers have processors without AVX (Xeon E5620:
up to SSE4.2), so no AVX instruction may be reachable. Exit code 1 when an AVX, AVX2,
AVX-512 or FMA instruction is found.

Needs the `capstone` Python package (the repository keeps one in tools/_pydeps).
"""
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'tools', '_pydeps'))
import capstone  # noqa: E402
from capstone import x86_const as x  # noqa: E402

BASELINE_OK = {'sse1', 'sse2', 'mmx', 'cmov', 'fxsr', 'mode64', 'jump', 'call', 'ret', 'int', 'iret', 'branch_relative', 'privilege', 'not64bitmode', 'fpu'}
NEEDS_AVX = ('avx', 'fma', 'f16c', 'xop', 'vaes', 'vpclmul')


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    data = open(sys.argv[1], 'rb').read()
    if data[:4] != b'\x7fELF' or data[4] != 2 or struct.unpack_from('<H', data, 18)[0] != 62:
        sys.exit('not a 64-bit x86-64 ELF file')
    shoff, = struct.unpack_from('<Q', data, 0x28)
    shentsize, shnum, _ = struct.unpack_from('<HHH', data, 0x3a)
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * shentsize) for i in range(shnum)]
    symtab = next((s for s in sections if s[1] == 2), None)
    if symtab is None:
        sys.exit('no symbol table: build without stripping for this check')
    strtab = sections[symtab[6]]
    name_of = lambda at: data[strtab[4] + at:data.index(b'\0', strtab[4] + at)].decode('utf-8', 'replace')
    code = [s for s in sections if s[1] == 1 and s[2] & 4]  # PROGBITS, executable
    functions = []
    for i in range(symtab[5] // 24):
        name, info, _other, shndx, value, size = struct.unpack_from('<IBBHQQ', data, symtab[4] + i * 24)
        if info & 15 == 2 and size and shndx < len(sections):
            functions.append((value, size, name_of(name)))
    functions.sort()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    found = {}
    total = 0
    covered = 0
    for address, size, name in functions:
        section = next((s for s in code if s[3] <= address < s[3] + s[5]), None)
        if section is None:
            continue
        start = section[4] + address - section[3]
        covered += size
        for ins in md.disasm(data[start:start + size], address):
            total += 1
            groups = {md.group_name(g) for g in ins.groups}
            extra = sorted(g for g in groups if g not in BASELINE_OK)
            if extra:
                found.setdefault(' '.join(extra), []).append((name, ins.mnemonic))
    text = sum(s[5] for s in code)
    print(f'{sys.argv[1]}: {len(functions)} functions, {total} instructions, {covered} of {text} bytes of code are inside named functions')
    bad = 0
    for groups in sorted(found):
        uses = found[groups]
        where = sorted({name for name, _ in uses})
        print(f'  {groups}: {len(uses)} instructions ({", ".join(sorted({m for _, m in uses})[:8])}) in {len(where)} functions, e.g. {", ".join(where[:4])}')
        if any(key in groups for key in NEEDS_AVX):
            bad += len(uses)
    if any('bmi' in groups for groups in found):
        print('  (tzcnt is written as "rep bsf": a processor without BMI runs it as bsf, and the compiler only')
        print('   uses it where the two give the same answer; it is part of every baseline x86-64 build)')
    if not found:
        print('  nothing but the x86-64 baseline (SSE, SSE2)')
    print('NO AVX, AVX2, AVX-512 or FMA instruction' if bad == 0 else f'{bad} instructions need AVX or newer')
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
