# Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

"""Read-only: unpack an Assetto Corsa car's data.acd into plain files.

Reimplements acs.exe's own reader:
  FolderEncrypter::decryptFile @ 0x14023bdd0  (container walk + per-byte decrypt)
  ksSecurity::keyFromString    @ 0x1402cfe00  (key derived from the car folder name)

Usage: acd_extract.py --car <car folder> --out <dir>
Only <car folder>/data.acd is opened, and only for reading.
"""
import argparse, ctypes, struct, sys
from pathlib import Path

ACD_MAGIC = -1111  # 0xfffffba9: optional header, followed by one int32 key id


def i32(v):
    return ctypes.c_int32(v).value


def idiv(a, b):
    """x86 IDIV on 32-bit operands: quotient truncated toward zero, remainder has a's sign."""
    q = abs(a) // abs(b)
    if (a < 0) != (b < 0):
        q = -q
    return i32(q), i32(a - q * b)


def key_from_string(name):
    """ksSecurity::keyFromString: eight bytes printed as "%d-%d-%d-%d-%d-%d-%d-%d"."""
    # the game reads the low byte of each wchar_t, sign-extended
    c = [ctypes.c_int8(ord(ch) & 0xFF).value for ch in name]
    n = len(c)

    k1 = 0
    for ch in c:
        k1 = i32(k1 + ch)

    k2 = 0
    for i in range(0, n - 1, 2):
        k2 = i32(i32(k2 * c[i]) - c[i + 1])

    k3 = 0
    for i in range(1, n - 3, 3):
        k3 = i32(k3 * c[i])
        k3 = idiv(k3, c[i + 1] + 0x1B)[0]
        k3 = i32(k3 + (-0x1B - c[i - 1]))

    k4 = 0x1683
    for i in range(1, n):
        k4 = i32(k4 - c[i])

    k5 = 0x42
    for i in range(1, n - 4, 4):
        k5 = i32((c[i] + 0xF) * k5)
        k5 = i32(i32((c[i - 1] + 0xF) * k5) + 0x16)

    k6 = 0x65
    for i in range(0, n - 2, 2):
        k6 = i32(k6 - c[i])

    k7 = 0xAB
    for i in range(0, n - 2, 2):
        k7 = idiv(k7, c[i])[1]

    k8 = 0xAB
    for i in range(0, n - 1):
        k8 = i32(idiv(k8, c[i])[0] + c[i + 1])

    return "-".join(str(k & 0xFF) for k in (k1, k2, k3, k4, k5, k6, k7, k8))


def read_acd(path, key):
    """Yield (name, bytes) for every file stored in the container."""
    d = Path(path).read_bytes()
    kb = [ord(ch) & 0xFF for ch in key]
    pos = 0
    if struct.unpack_from("<i", d, 0)[0] == ACD_MAGIC:
        pos = 8
    while pos + 4 <= len(d):
        (nlen,) = struct.unpack_from("<i", d, pos)
        pos += 4
        name = d[pos:pos + nlen].decode("utf-8")
        pos += nlen
        (size,) = struct.unpack_from("<i", d, pos)
        pos += 4
        # every plain byte is stored as a 4-byte int; only its low byte is used
        raw = d[pos:pos + size * 4:4]
        pos += size * 4
        yield name, bytes((b - kb[i % len(kb)]) & 0xFF for i, b in enumerate(raw))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--car", required=True, help="car folder containing data.acd")
    ap.add_argument("--out", required=True, help="output directory")
    ap.add_argument("--name", help="folder name to derive the key from, for a car folder that "
                                   "was renamed after packing (default: the folder's own name)")
    a = ap.parse_args()

    car = Path(a.car)
    acd = car / "data.acd"
    if not acd.is_file():
        sys.exit(f"not found: {acd}")
    out = Path(a.out).resolve()
    if car.resolve() in (out, *out.parents):
        sys.exit("refusing to write inside the car folder")
    out.mkdir(parents=True, exist_ok=True)

    key = key_from_string(a.name or car.resolve().name)
    count = 0
    for name, data in read_acd(acd, key):
        target = (out / name).resolve()
        if out not in target.parents:
            sys.exit(f"unsafe entry name in container: {name!r}")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        count += 1
    print(f"key {key}: {count} files -> {out}")


if __name__ == "__main__":
    main()
