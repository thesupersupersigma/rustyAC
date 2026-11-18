# SPDX-License-Identifier: MIT OR Apache-2.0
"""rustyac_math::pure's fma (fma32) against the exact, correctly rounded a*b+c (fractions).

Usage: python fma_check.py <math_proof.exe> <work folder>

Task F1's 300,000 double triples (random, and the error-term shapes FMA algorithms use: c close
to -a*b), the same for floats, and the cross product of special values (zeros, subnormals, the
smallest normal, one, the largest finite, infinities, NaN) at both widths. Bits are compared; a
NaN only has to be a NaN. Exit 1 when any result is not the correctly rounded one.
"""
import math, os, random, struct, subprocess, sys
from fractions import Fraction

# (bits, struct code, mantissa digits, smallest normal exponent, largest exponent)
WIDTHS = {64: ("d", 53, -1022, 1023), 32: ("f", 24, -126, 127)}


def nearest(q, p, emin, emax):
    """q (a Fraction, not zero) rounded to p digits, ties to even, subnormals below 2^emin;
    None when it overflows."""
    a = abs(q)
    e = max(a.numerator.bit_length() - a.denominator.bit_length(), emin)
    while Fraction(2) ** e > a and e > emin:
        e -= 1
    while Fraction(2) ** (e + 1) <= a:
        e += 1
    ulp = Fraction(2) ** (e - p + 1)
    m, r = divmod(a, ulp)
    if r * 2 > ulp or (r * 2 == ulp and m % 2 == 1):
        m += 1
    v = m * ulp
    if v >= Fraction(2) ** (emax + 1):
        return None
    return v if q > 0 else -v


def exact(a, b, c, width):
    code, p, emin, emax = WIDTHS[width]
    if any(math.isnan(x) for x in (a, b, c)):
        return math.nan
    if math.isinf(a) or math.isinf(b):
        return a * b + c  # inf * 0, inf - inf: NaN; Python's doubles follow IEEE here
    if math.isinf(c):
        return c  # a * b is finite (exactly), whatever a double product would overflow to
    q = Fraction(a) * Fraction(b) + Fraction(c)
    if q == 0:
        prod_neg = (math.copysign(1, a) * math.copysign(1, b)) < 0
        if Fraction(a) * Fraction(b) == 0 and c == 0 and prod_neg == (math.copysign(1, c) < 0):
            return c  # the same signed zero on both sides; otherwise +0 (round to nearest)
        return 0.0
    v = nearest(q, p, emin, emax)
    return math.copysign(math.inf if v is None else float(v), -1.0 if q < 0 else 1.0)  # an underflow keeps its sign


def rnd(lo, hi):
    return random.uniform(1, 2) * 2.0 ** random.randint(lo, hi) * random.choice((1, -1))


def triples(width):
    code = WIDTHS[width][0]
    to = lambda x: struct.unpack("<" + code, struct.pack("<" + code, x))[0]
    random.seed(1)
    out = []
    span = 30 if width == 64 else 14
    for i in range(300000):
        a, b = to(rnd(-span, span)), to(rnd(-span, span))
        kind = i % 3
        if kind == 0:
            c = rnd(-2 * span, 2 * span)
        elif kind == 1:
            c = -(a * b)  # (rounded to the width below: its rounding error is what is left)
        else:
            c = -(a * b) * (1 + random.choice((-1, 1)) * 2.0 ** -random.randint(10, WIDTHS[width][1] - 1))
        out.append((a, b, to(c)))
    p, emin, emax = WIDTHS[width][1:]
    tiny = 2.0 ** (emin - p + 1)
    big = (2 - 2.0 ** (1 - p)) * 2.0 ** emax
    special = [0.0, tiny, 2.0 ** emin - tiny, 2.0 ** emin, 1.0, 1 + 2.0 ** (1 - p), big, math.inf, math.nan]
    special += [-x for x in special]
    out += [(a, b, c) for a in special for b in special for c in special]
    return out


def main():
    exe, work = os.path.abspath(sys.argv[1]), sys.argv[2]
    os.makedirs(work, exist_ok=True)
    bad_all = 0
    for width in (64, 32):
        code = WIDTHS[width][0]
        ts = triples(width)
        src, dst = os.path.join(work, f"fma{width}.in"), os.path.join(work, f"fma{width}.out")
        with open(src, "wb") as f:
            f.write(b"".join(struct.pack("<3" + code, *t) for t in ts))
        subprocess.run([exe, "fma", str(width), src, dst], check=True)
        n = struct.calcsize(code)
        got = open(dst, "rb").read()
        assert len(got) == n * len(ts), "math_proof wrote the wrong number of results"
        bad = []
        for i, t in enumerate(ts):
            g = got[i * n:(i + 1) * n]
            w = exact(*t, width)
            if math.isnan(w) and math.isnan(struct.unpack("<" + code, g)[0]):
                continue
            if g != struct.pack("<" + code, w):
                bad.append((t, g.hex(), struct.pack("<" + code, w).hex()))
        print(f"fma{width}: {len(ts)} triples ({len(ts) - 300000} special), {len(bad)} not correctly rounded")
        for t, g, w in bad[:5]:
            print(f"  {[x.hex() for x in t]} got {g} want {w}")
        bad_all += len(bad)
    sys.exit(1 if bad_all else 0)


if __name__ == "__main__":
    main()
