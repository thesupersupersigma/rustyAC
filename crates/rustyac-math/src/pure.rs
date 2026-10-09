// SPDX-License-Identifier: MIT OR Apache-2.0
// Ported from MSVCR120.dll 12.0.21005.1 (the Microsoft Visual C++ 2013 runtime Assetto Corsa
// imports its maths from; the float functions in it are AMD's LibM).

//! `sinf cosf tanf expf asinf acosf atanf atan2f powf` and the double `sin` of `MSVCR120.dll`
//! in plain Rust, read from the DLL's machine code instruction by instruction, so a target
//! that cannot load the DLL (the browser) still gets AC's exact bits.
//!
//! **Which path.** `sinf`, `cosf`, `tanf`, `expf`, `powf` and `sin` start with
//! `cmp [__use_fma3_lib], 0`: the runtime sets that flag when the CPU has FMA3 and the OS saves
//! the AVX state, and `acs.exe` never calls `_set_FMA3_enable`. So on every CPU since about
//! 2013 (Haswell / Piledriver) the game runs the FMA3 versions, and those are what is ported
//! here. The four inverse functions are compiled C with a single path.
//!
//! **Fused multiply-add.** Where the DLL executes `vfmadd…sd` this file calls
//! [`f64::mul_add`], which is one correctly rounded operation on every target (an instruction
//! where there is one, exact software arithmetic in wasm). Nothing else here may be fused or
//! reordered: every `a * b + c` written with separate operators is separate in the DLL too.
//!
//! **Proof.** `tools/math_proof` runs every one of the 2^32 inputs of each one-argument
//! function, and random plus special pairs for `atan2f` / `powf`, against the real DLL
//! (results: `docs/port/web.md`).

use super::pure_tables::{ATAN_JBY256, LOG_256, LOG_F_INV, PIBITS, TWO_TO_JBY64};

const SIGN64: u64 = 0x8000_0000_0000_0000;
const ABS64: u64 = 0x7fff_ffff_ffff_ffff;
const INF32: u32 = 0x7f80_0000;

/// `vfmadd`: `a * b + c` with one rounding.
#[inline(always)]
fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

#[inline(always)]
fn d(bits: u64) -> f64 {
    f64::from_bits(bits)
}

/// `cvtpd2dq` / `cvtss2si` under the default MXCSR: round to nearest even; the "integer
/// indefinite" `0x8000_0000` when out of range.
#[inline(always)]
fn nearest_i32(x: f64) -> i32 {
    let r = x.round_ties_even();
    if (-2147483648.0..2147483648.0).contains(&r) {
        r as i32
    } else {
        i32::MIN
    }
}

/// `cvttpd2dq` / `cvttsd2si`: truncate; `0x8000_0000` when out of range.
#[inline(always)]
fn trunc_i32(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// What the trigonometric functions return for an infinity (the "indefinite" NaN) or a NaN
/// (the same NaN, made quiet): `_sinf_special` and friends, 0x1800a1a48.
#[inline(never)]
fn trig_special(bits: u32) -> f32 {
    if bits & 0x007f_ffff == 0 {
        f32::from_bits(0xffc0_0000)
    } else {
        f32::from_bits(bits | 0x0040_0000)
    }
}

// sinf / cosf / tanf constants (0x1800b9f80.., 0x1800bccc0..)
const PI_BY_4: u64 = 0x3fe9_21fb_5444_2d18;
const TWO_BY_PI: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883);
const PIBY2_1: f64 = f64::from_bits(0x3ff9_21fb_5440_0000);
const PIBY2_1TAIL: f64 = f64::from_bits(0x3dd0_b461_1a62_6331);
const PIBY2: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
const S1: f64 = f64::from_bits(0xbfc5_5555_5555_5555);
const S2: f64 = f64::from_bits(0x3f81_1111_1111_1111);
const S3: f64 = f64::from_bits(0xbf2a_01a0_1a01_a01a);
const S4: f64 = f64::from_bits(0x3ec7_1de3_a556_c734);
const C1: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
const C2: f64 = f64::from_bits(0xbf56_c16c_16c1_6c16);
const C3: f64 = f64::from_bits(0x3efa_01a0_1a01_a019);
const C4: f64 = f64::from_bits(0xbe92_7e4f_b778_9f5c);

/// The sine polynomial of the float functions on a reduced argument.
#[inline(always)]
fn sinf_poly(r: f64) -> f64 {
    let x2 = r * r;
    let mut p = fma(x2, S4, S3);
    p = fma(p, x2, S2);
    p = fma(p, x2, S1);
    let x3 = r * x2;
    fma(p, x3, r)
}

/// The cosine polynomial's tail: `t + x^4 * (C1 + C2 x^2 + ...)`.
#[inline(always)]
fn cosf_poly(x2: f64, t: f64) -> f64 {
    let mut p = fma(x2, C4, C3);
    p = fma(p, x2, C2);
    p = fma(p, x2, C1);
    let x4 = x2 * x2;
    fma(p, x4, t)
}

/// The medium-range reduction shared by `sinf`, `cosf` and `tanf`: `|x| - n * pi/2` with
/// `n = trunc(|x| * 2/pi + 0.5)`; returns the remainder and `n & 3`.
#[inline(always)]
fn reduce_medium(ax: f64) -> (f64, u64) {
    let npi2 = trunc_i32(fma(TWO_BY_PI, ax, 0.5));
    let region = (npi2 as i64 as u64) & 3;
    let dn = npi2 as f64;
    let rhead = fma(-dn, PIBY2_1, ax);
    let rtail = dn * PIBY2_1TAIL;
    (rhead - rtail, region)
}

#[inline(always)]
fn load64(offset: u64) -> u64 {
    let o = offset as usize;
    u64::from_le_bytes([
        PIBITS[o],
        PIBITS[o + 1],
        PIBITS[o + 2],
        PIBITS[o + 3],
        PIBITS[o + 4],
        PIBITS[o + 5],
        PIBITS[o + 6],
        PIBITS[o + 7],
    ])
}

/// `bsr` (the destination keeps its old value when the source is zero).
#[inline(always)]
fn bsr(value: u64, old: u64) -> u64 {
    if value == 0 {
        old
    } else {
        63 - value.leading_zeros() as u64
    }
}

/// What the integer part of the large-argument reduction leaves in the registers.
struct Reduced {
    /// `|x| * 2/pi` minus the nearest integer, as the bits of a double (sign included).
    dx: u64,
    /// The 64 bits below the ones that went into `dx`.
    low: u64,
    /// The biased exponent of `dx`.
    exponent: u64,
    /// `SIGN64` when the fraction was above one half (the remainder is negative).
    sign: u64,
    region: u64,
}

/// `__remainder_piby2`'s integer part (inlined in `sinf` 0x180099190 and `tanf`
/// 0x18009aa57, a function at 0x1800a16d0): multiply the mantissa by 192 bits of 2/pi picked
/// by the exponent, take the two bits above the binary point as the region, round to the
/// nearest quarter turn and normalise what is left.
fn reduce_bits(ax: u64) -> Reduced {
    let xexp = (ax >> 52).wrapping_sub(0x3ff);
    let offset = 0x86u64.wrapping_sub(xexp >> 3);
    let mant = ((ax << 12) >> 12) | (1 << 52);
    let p0 = load64(offset) as u128 * mant as u128;
    let mut r8 = p0 as u64;
    let p1 = load64(offset + 8) as u128 * mant as u128 + (p0 >> 64);
    let mut r9 = p1 as u64;
    let p2 = load64(offset + 16).wrapping_mul(mant);
    let mut r10 = ((p1 >> 64) as u64).wrapping_add(p2);
    let e7 = xexp & 7;
    let cl = 0x36 - e7;
    let mut rax = r10 >> cl;
    let carry = (r10 >> (cl - 1)) & 1;
    let mut sign = 0;
    if carry != 0 {
        r10 = !r10;
        r9 = !r9;
        r8 = !r8;
        sign = SIGN64;
    }
    rax = rax.wrapping_add(carry);
    let region = rax & 3;
    let cl = e7 + 10;
    r10 = (r10 << cl) >> cl;
    let mut r11 = cl.wrapping_sub(0x40);
    let mut rcx = r11;
    if r10 == 0 {
        r10 = r9;
        r9 = r8;
        r8 = 0;
        rcx = bsr(r10, rcx);
        r11 = r11.wrapping_sub(0x40);
    } else {
        rcx = bsr(r10, rcx);
    }
    r11 = r11.wrapping_add(rcx);
    let shift = rcx.wrapping_sub(0x34) as i64;
    if shift < 0 {
        let n = (shift.wrapping_neg() as u64 & 63) as u32;
        let m = (64u64.wrapping_sub(n as u64) & 63) as u32;
        let old9 = r9;
        r10 <<= n;
        r9 <<= n;
        r10 |= old9 >> m;
        r9 |= r8 >> m;
    } else if shift > 0 {
        let n = (shift as u64 & 63) as u32;
        let m = (64u64.wrapping_sub(n as u64) & 63) as u32;
        let old10 = r10;
        r10 >>= n;
        r9 >>= n;
        r9 |= old10 << m;
    }
    r11 = r11.wrapping_add(0x3ff);
    r10 &= !(1 << 52);
    r10 |= sign | (r11 << 52);
    Reduced {
        dx: r10,
        low: r9,
        exponent: r11,
        sign,
        region,
    }
}

/// The float functions' large-argument reduction: one multiplication by pi/2.
#[inline(always)]
fn reduce_large_single(ax: u64) -> (f64, u64) {
    let r = reduce_bits(ax);
    (d(r.dx) * PIBY2, r.region)
}

/// `__remainder_piby2_fma3` (0x1800a16d0), used by `cosf` and the double `sin`: the remainder
/// as a head and a tail.
fn reduce_large_double(ax: u64) -> (f64, f64, u64) {
    let r = reduce_bits(ax);
    let dx = d(r.dx);
    // the second word: the next 52 bits, as a double of its own
    let mut r9 = r.low;
    let rcx = bsr(r9, r.exponent << 52);
    let rcx = 0x40u64.wrapping_sub(rcx);
    r9 <<= (rcx & 63) as u32;
    r9 >>= 12;
    let rcx = rcx.wrapping_add(0x34);
    let r11 = r.exponent.wrapping_sub(rcx) << 52;
    let ddx = d(r9 | r.sign | r11);
    let hi = d(r.dx & 0xffff_ffff_f800_0000);
    let lo = dx - hi;
    let c = dx * PIBY2;
    const HEAD: f64 = f64::from_bits(0x3ff9_21fb_5000_0000);
    const TAIL1: f64 = f64::from_bits(0x3e51_10b4_6000_0000);
    const PIBY2_TAIL: f64 = f64::from_bits(0x3c91_a626_3314_5c06);
    let mut cc = hi * HEAD - c;
    cc = fma(lo, HEAD, cc);
    cc = fma(hi, TAIL1, cc);
    cc = fma(lo, TAIL1, cc);
    let t = fma(dx, PIBY2_TAIL, ddx * PIBY2);
    cc += t;
    let head = c + cc;
    let tail = (c - head) + cc;
    (head, tail, r.region)
}

/// `sinf`, the FMA3 path (0x180099070).
pub fn sinf(x: f32) -> f32 {
    let ux = x.to_bits();
    if ux & INF32 == INF32 {
        return trig_special(ux);
    }
    let xd = x as f64;
    let ax = xd.to_bits() & ABS64;
    if ax <= PI_BY_4 {
        if ax >= 0x3f80_0000_0000_0000 {
            return sinf_poly(xd) as f32;
        }
        if ax >= 0x3f20_0000_0000_0000 {
            let x3 = (xd * xd) * xd;
            return fma(-x3, d(0x3fc5_5555_5555_5555), xd) as f32;
        }
        return x;
    }
    let (r, region) = if ax < 0x4170_008a_c000_0000 {
        reduce_medium(d(ax))
    } else {
        reduce_large_single(ax)
    };
    let value = if region & 1 == 0 {
        sinf_poly(r)
    } else {
        let x2 = r * r;
        cosf_poly(x2, fma(x2, -0.5, 1.0))
    };
    let mut bits = value.to_bits();
    if region < 2 {
        bits ^= SIGN64;
    }
    bits ^= !xd.to_bits() & SIGN64;
    d(bits) as f32
}

/// `cosf`, the FMA3 path (0x180088dd0).
pub fn cosf(x: f32) -> f32 {
    let ux = x.to_bits();
    if ux & INF32 == INF32 {
        return trig_special(ux);
    }
    let xd = x as f64;
    let ax = xd.to_bits() & ABS64;
    if ax <= PI_BY_4 {
        if ax >= 0x3f80_0000_0000_0000 {
            let x2 = xd * xd;
            return cosf_poly(x2, 1.0 - x2 * 0.5) as f32;
        }
        if ax >= 0x3f20_0000_0000_0000 {
            return fma(-(xd * 0.5), xd, 1.0) as f32;
        }
        return 1.0;
    }
    let (r, region) = if ax < 0x41e9_21fb_6000_0000 {
        reduce_medium(d(ax))
    } else {
        let (head, _, region) = reduce_large_double(ax);
        (head, region)
    };
    let value = if region & 1 != 0 {
        sinf_poly(r)
    } else {
        let x2 = r * r;
        cosf_poly(x2, 1.0 - x2 * 0.5)
    };
    let sign = ((region + 1) >> 1) << 63;
    d(value.to_bits() ^ sign) as f32
}

/// `tanf`, the FMA3 path (0x18009a950).
pub fn tanf(x: f32) -> f32 {
    const A0: f64 = f64::from_bits(0x3fd8_a8b0_da56_cb17);
    const A1: f64 = f64::from_bits(0xbf91_9dba_6efd_6aad);
    const B0: f64 = f64::from_bits(0x3ff2_7e84_a3e7_3a2e);
    const B1: f64 = f64::from_bits(0xbfe0_7266_d7b3_511b);
    const B2: f64 = f64::from_bits(0x3f92_e290_03c6_92d9);
    #[inline(always)]
    fn rational(r: f64) -> f64 {
        let x2 = r * r;
        let a = fma(x2, A1, A0);
        let mut b = fma(x2, B2, B1);
        b = fma(x2, b, B0);
        let q = a / b;
        let x3 = x2 * r;
        fma(x3, q, r)
    }
    let ux = x.to_bits();
    if ux & INF32 == INF32 {
        return trig_special(ux);
    }
    let xd = x as f64;
    let ax = xd.to_bits() & ABS64;
    if ax <= PI_BY_4 {
        if ax >= 0x3f20_0000_0000_0000 {
            return rational(xd) as f32;
        }
        if ax >= 0x3e40_0000_0000_0000 {
            let x3 = (xd * xd) * xd;
            return fma(x3, d(0x3fd5_5555_5555_5555), xd) as f32;
        }
        return x;
    }
    let (r, region) = if ax < 0x41e9_21fb_4000_0000 {
        reduce_medium(d(ax))
    } else {
        reduce_large_single(ax)
    };
    let mut t = rational(r);
    if region & 1 == 1 {
        t = -1.0 / t;
    }
    d(t.to_bits() ^ (xd.to_bits() & SIGN64)) as f32
}

const LN2_BY_64: f64 = f64::from_bits(0x3f86_2e42_fefa_39ef);
const SIXTY_FOUR_BY_LN2: f64 = f64::from_bits(0x4057_1547_652b_82fe);
const ONE_SIXTH: f64 = f64::from_bits(0x3fc5_5555_5555_5555);

/// `expf`, the FMA3 path (0x18008d800). An infinity of either sign comes back unchanged and
/// a NaN loses its sign: that is what the DLL does.
pub fn expf(x: f32) -> f32 {
    let ax = x.to_bits() & 0x7fff_ffff;
    if ax >= INF32 {
        if ax == INF32 {
            return x;
        }
        return f32::from_bits(ax | 0x0040_0000);
    }
    let xd = x as f64;
    let t = xd * SIXTY_FOUR_BY_LN2;
    if t >= 8192.0 {
        return f32::INFINITY;
    }
    if t < -9600.0 {
        return 0.0;
    }
    let n = nearest_i32(t);
    let r = fma(-(n as f64), LN2_BY_64, xd);
    let j = n & 0x3f;
    let m = n.wrapping_sub(j) >> 6;
    let p = fma(r, ONE_SIXTH, 0.5);
    let q = fma(p, r * r, r);
    let f = d(TWO_TO_JBY64[j as usize]);
    let value = fma(q, f, f);
    let scale = d((m as u32 as u64).wrapping_add(0x3ff) << 52);
    (value * scale) as f32
}

/// `powf`'s "is `y` an odd whole number" test for a `y` below 2^24 (`cvtss2si`,
/// `cvtsi2ss`, `ucomiss`, `rcr`): `None` when `y` is not whole.
#[inline(always)]
fn whole_and_odd(y: f32) -> Option<bool> {
    let i = nearest_i32(y as f64);
    if i as f32 != y {
        None
    } else {
        Some(i & 1 != 0)
    }
}

/// `powf`, the FMA3 path (0x180096cd5), including the piece of the SSE2 code it jumps into
/// for a base within 1/16 of 1 (0x180096801).
pub fn powf(x: f32, y: f32) -> f32 {
    const LN2: f64 = f64::from_bits(0x3fe6_2e42_fefa_39ef);
    const ONE_THIRD: f64 = f64::from_bits(0x3fd5_5555_5555_5555);
    const OVERFLOW: f64 = f64::from_bits(0x4056_2e43_0000_0000);
    const UNDERFLOW: f64 = f64::from_bits(0xc059_d1da_0000_0000);
    let ux = x.to_bits();
    let uy = y.to_bits();
    let ay = uy & 0x7fff_ffff;
    let ax = ux & 0x7fff_ffff;
    if ay >= INF32 {
        if ay > INF32 {
            // y is a NaN
            if ux == 0x3f80_0000 {
                return 1.0;
            }
            return y + y;
        }
        if ax > INF32 {
            return x + x;
        }
        if ax == 0x3f80_0000 {
            return f32::from_bits(ux & (uy | 0x7fff_ffff));
        }
        let small = ax < 0x3f80_0000;
        return if (uy >> 31 != 0) == small {
            f32::INFINITY
        } else {
            0.0
        };
    }
    if ay <= 0x3f80_0000 {
        if ay == 0 {
            if ax > INF32 {
                return x + x;
            }
            return 1.0;
        }
        if uy == 0x3f80_0000 {
            return x;
        }
    }
    if ax >= INF32 {
        if ux == INF32 {
            return if (uy as i32) < 0 { 0.0 } else { f32::INFINITY };
        }
        if ux == 0xff80_0000 {
            let magnitude = if (uy as i32) > 0 { INF32 } else { 0 };
            let mut sign = 0;
            if ay & INF32 <= 0x4b00_0000 && whole_and_odd(y) == Some(true) {
                sign = 0x8000_0000;
            }
            return f32::from_bits(sign | magnitude);
        }
        return x + x;
    }
    let xd = x as f64;
    let yd = y as f64;
    let mut sign = 0u32;
    if (ux as i32) < 0x3f88_0000 {
        if (ux as i32) <= 0 {
            if ax == 0 {
                let magnitude = if (uy as i32) < 0 { INF32 } else { 0 };
                let mut sign = 0;
                if ay & INF32 <= 0x4b00_0000 && whole_and_odd(y) == Some(true) {
                    sign = ux & 0x8000_0000;
                }
                return f32::from_bits(sign | magnitude);
            }
            if ay & INF32 <= 0x4b00_0000 {
                match whole_and_odd(y) {
                    None => return f32::from_bits(0x7fc0_0000),
                    Some(true) => sign = 0x8000_0000,
                    Some(false) => {}
                }
            }
        }
        let f = xd - 1.0;
        if f.to_bits() & ABS64 < 0x3fb0_0000_0000_0000 {
            // log(x) near 1, then the SSE2 exponential (no fused operations in it)
            const CA1: f64 = f64::from_bits(0x3fb5_5555_5555_54e6);
            const CA2: f64 = f64::from_bits(0x3f89_9999_99ba_c6d4);
            const CA3: f64 = f64::from_bits(0x3f62_4923_07f1_519f);
            const CA4: f64 = f64::from_bits(0x3f3c_8034_c85d_fff0);
            let u = f / (f + 2.0);
            let correction = u * f;
            let v = u + u;
            let v2 = v * v;
            let v3 = v * v2;
            let v7 = (v2 * v2) * v3;
            let low = v3 * (v2 * CA2 + CA1);
            let high = v7 * (v2 * CA4 + CA3);
            let logx = f + ((high + low) - correction);
            let ylogx = yd * logx;
            if ylogx > OVERFLOW {
                return f32::from_bits(INF32 | sign);
            }
            if ylogx.partial_cmp(&UNDERFLOW) != Some(std::cmp::Ordering::Greater) {
                return f32::from_bits(sign);
            }
            let n = nearest_i32(ylogx * SIXTY_FOUR_BY_LN2);
            let r = ylogx - (n as f64) * LN2_BY_64;
            let q = (r * r) * (ONE_SIXTH * r + 0.5) + r;
            let table = d(TWO_TO_JBY64[(n & 0x3f) as usize]);
            let value = q * table + table;
            let bits = value.to_bits().wrapping_add(((n >> 6) as u32 as u64) << 52);
            return f32::from_bits((d(bits) as f32).to_bits() | sign);
        }
    }
    // log(x) from a 257-entry table
    let xbits = xd.to_bits();
    let mantissa = xbits & 0x000f_ffff_ffff_ffff;
    let index = (mantissa >> 44) + ((mantissa >> 43) & 1);
    let big_f = d((index | 0x3fe00) << 44);
    let frac = d(mantissa | 0x3fe0_0000_0000_0000);
    let exponent = (((xbits & 0x7ff0_0000_0000_0000) >> 52) as i32 - 0x3ff) as f64;
    let r = (big_f - frac) * d(LOG_F_INV[index as usize]);
    let mut p = fma(r, ONE_THIRD, 0.5);
    p = fma(r, p, 1.0);
    let poly = r * p;
    let logx = (exponent * LN2 + d(LOG_256[index as usize])) - poly;
    let ylogx = yd * logx;
    if ylogx > OVERFLOW {
        return f32::from_bits(INF32 | sign);
    }
    if ylogx.partial_cmp(&UNDERFLOW) != Some(std::cmp::Ordering::Greater) {
        return f32::from_bits(sign);
    }
    let n = nearest_i32(ylogx * SIXTY_FOUR_BY_LN2);
    let r = fma(-(n as f64), LN2_BY_64, ylogx);
    let mut q = fma(r, ONE_SIXTH, 0.5);
    q = fma(r, q, 1.0);
    let z = r * q;
    let table = d(TWO_TO_JBY64[(n & 0x3f) as usize]);
    let value = fma(z, table, table);
    let bits = value.to_bits().wrapping_add(((n >> 6) as u32 as u64) << 52);
    f32::from_bits((d(bits) as f32).to_bits() | sign)
}

// asinf / acosf (single precision C code, 0x180082608 / 0x180081e7c)
const AS_C0: f32 = f32::from_bits(0x3b81_ce6b);
const AS_C1: f32 = f32::from_bits(0xbc5b_3fe1);
const AS_C2: f32 = f32::from_bits(0x3d67_8bdd);
const AS_C3: f32 = f32::from_bits(0x3e3c_94dc);
const AS_D0: f32 = f32::from_bits(0x3f8d_6fa5);
const AS_D1: f32 = f32::from_bits(0x3f56_1f0d);

/// The rational part both inverse functions share.
#[inline(always)]
fn asin_rational(r: f32) -> f32 {
    let mut n = AS_C1 - r * AS_C0;
    n *= r;
    n -= AS_C2;
    n *= r;
    n += AS_C3;
    n *= r;
    n / (AS_D0 - r * AS_D1)
}

/// `asinf`.
pub fn asinf(x: f32) -> f32 {
    let ux = x.to_bits();
    let negative = ux & 0x8000_0000 != 0;
    let xexp = ((ux >> 23) & 0xff) as i32 - 0x7f;
    let ax = ux & 0x7fff_ffff;
    if ax > INF32 {
        return f32::from_bits(ux | 0x0040_0000);
    }
    if xexp < -14 {
        return x;
    }
    if xexp >= 0 {
        if x == 1.0 {
            return f32::from_bits(0x3fc9_0fdb);
        }
        if x == -1.0 {
            return f32::from_bits(0xbfc9_0fdb);
        }
        return f32::from_bits(0xffc0_0000);
    }
    let mut y = f32::from_bits(ax);
    let transform = xexp >= -1;
    let r;
    let mut s = 0.0f32;
    if transform {
        r = (1.0 - y) * 0.5;
        s = r.sqrt();
        y = s;
    } else {
        r = y * y;
    }
    let u = asin_rational(r);
    let v = if transform {
        let s1 = f32::from_bits(s.to_bits() & 0xffff_0000);
        let p = (s * 2.0) * u;
        let c = (r - s1 * s1) / (s1 + s);
        let q = f32::from_bits(0x33a2_2168) - c * 2.0;
        let piby4 = f32::from_bits(0x3f49_0fda);
        let t = (p - q) - (piby4 - s1 * 2.0);
        piby4 - t
    } else {
        u * y + y
    };
    if negative {
        -v
    } else {
        v
    }
}

/// `acosf`.
pub fn acosf(x: f32) -> f32 {
    const PIBY2_TAIL: f64 = f64::from_bits(0x3c91_a626_3314_5c07);
    let ux = x.to_bits();
    let negative = ux & 0x8000_0000 != 0;
    let xexp = ((ux >> 23) & 0xff) as i32 - 0x7f;
    let ax = ux & 0x7fff_ffff;
    if ax > INF32 {
        return f32::from_bits(ux | 0x0040_0000);
    }
    if xexp < -26 {
        return f32::from_bits(0x3fc9_0fdb);
    }
    if xexp >= 0 {
        if x == 1.0 {
            return 0.0;
        }
        if x == -1.0 {
            return f32::from_bits(0x4049_0fdb);
        }
        return f32::from_bits(0xffc0_0000);
    }
    let mut y = f32::from_bits(ax);
    let transform = xexp >= -1;
    let r;
    let mut s = 0.0f32;
    if transform {
        r = (1.0 - y) * 0.5;
        s = r.sqrt();
        y = s;
    } else {
        r = y * y;
    }
    let u = asin_rational(r);
    if transform {
        if negative {
            let t = (((u * y) as f64 - PIBY2_TAIL) + s as f64) * 2.0;
            (d(0x4009_21fb_5444_2d18) - t) as f32
        } else {
            let s1 = f32::from_bits(s.to_bits() & 0xffff_0000);
            let p = (y * 2.0) * u;
            let c = (r - s1 * s1) / (s1 + s);
            (c * 2.0 + p) + s1 * 2.0
        }
    } else {
        let t = PIBY2_TAIL - (u * x) as f64;
        (PIBY2 - (x as f64 - t)) as f32
    }
}

/// `atanf` (double precision C code, 0x180083930).
pub fn atanf(x: f32) -> f32 {
    let xd = x as f64;
    let bits = xd.to_bits();
    let aux = bits & ABS64;
    let negative = bits & SIGN64 != 0;
    let mut v = d(aux);
    let c;
    if aux < 0x3fdc_0000_0000_0000 {
        c = 0.0;
    } else if aux < 0x3fe6_0000_0000_0000 {
        c = d(0x3fdd_ac67_0561_bb4f);
        v = (v * 2.0 - 1.0) / (v + 2.0);
    } else if aux < 0x3ff3_0000_0000_0000 {
        c = d(PI_BY_4);
        v = (v - 1.0) / (v + 1.0);
    } else if aux < 0x4003_8000_0000_0000 {
        c = d(0x3fef_730b_d281_f69b);
        v = (v - 1.5) / (v * 1.5 + 1.0);
    } else {
        if aux > 0x7ff0_0000_0000_0000 {
            return f32::from_bits(x.to_bits() | 0x0040_0000);
        }
        if v > d(0x43d3_2000_0000_0000) {
            return if negative {
                (-PIBY2) as f32
            } else {
                PIBY2 as f32
            };
        }
        c = PIBY2;
        v = -1.0 / v;
    }
    let s = v * v;
    let mut n = s * d(0x3f73_476a_758d_a22a);
    let mut q = s * d(0x3fd3_27e3_df2c_f2aa);
    let sv = s * v;
    n += d(0x3fc8_9e17_3a81_ee7f);
    q += d(0x3ff1_c587_93da_6ea4);
    n *= s;
    q *= s;
    n += d(0x3fd2_fa53_1690_7834);
    q += d(0x3fec_777c_a210_53f0);
    n *= sv;
    n /= q;
    let value = c - (n - v);
    (if negative { -value } else { value }) as f32
}

/// `atan2f(y, x)` (double precision C code, 0x1800834b0).
// the DLL's `comisd` / `ja` skips on "greater", so the negation stays as it is written there
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn atan2f(y: f32, x: f32) -> f32 {
    const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
    let yd = y as f64;
    let xd = x as f64;
    let ux = xd.to_bits();
    let uy = yd.to_bits();
    let xneg = ux & SIGN64 != 0;
    let yneg = uy & SIGN64 != 0;
    let aux = ux & ABS64;
    let auy = uy & ABS64;
    const INF: u64 = 0x7ff0_0000_0000_0000;
    let diffexp = ((uy >> 52) & 0x7ff) as i32 - ((ux >> 52) & 0x7ff) as i32;
    if aux > INF {
        return f32::from_bits(x.to_bits() | 0x0040_0000);
    }
    if auy > INF {
        return f32::from_bits(y.to_bits() | 0x0040_0000);
    }
    let signed = |value: f64| (if yneg { -value } else { value }) as f32;
    if auy == 0 {
        if !xneg {
            return yd as f32;
        }
        return signed(PI);
    }
    if aux == 0 && yneg {
        return (-PIBY2) as f32;
    }
    if diffexp > 26 {
        return signed(PIBY2);
    }
    if diffexp < -13 && !xneg {
        if diffexp < -150 {
            return if yneg { -0.0 } else { 0.0 };
        }
        if diffexp >= -126 {
            return (yd / xd) as f32;
        }
        let t = (d(0x4630_0000_0000_0000) * yd / xd).to_bits();
        let sign = t & SIGN64;
        let mut rax = t & ABS64;
        let e = ((rax >> 52) & 0x7ff) as i32 - 100;
        let mut rbx = 0u64;
        if e > 0 {
            rbx = ((e as i64 as u64) << 52) | (rax & 0x800f_ffff_ffff_ffff);
        } else {
            rax = (rax & 0x801f_ffff_ffff_ffff) | 0x0010_0000_0000_0000;
            let shift = 1 - e;
            if shift <= 0x36 {
                rax >>= ((shift - 1) & 63) as u32;
                rbx = (rax >> 1) + (rax & 1);
            }
        }
        return d(rbx | sign) as f32;
    }
    if diffexp < -26 && xneg {
        return signed(PI);
    }
    if auy == INF && aux == INF {
        return signed(if xneg {
            d(0x4002_d97c_7f33_21d2)
        } else {
            d(PI_BY_4)
        });
    }
    let mut u = d(auy);
    let mut v = d(aux);
    let swap = u > v;
    if swap {
        std::mem::swap(&mut u, &mut v);
    }
    let mut q = u / v;
    if q > 0.0625 {
        let index = trunc_i32(q * 256.0 + 0.5);
        let di = index as u32 as f64;
        let r = (u * 256.0 - di * v) / (di * u + v * 256.0);
        let table = d(ATAN_JBY256[(index - 16) as usize]);
        q = (r + table) - ((r * r) * r) * d(0x3fd5_5555_5555_0877);
    } else if !(d(0x3f1a_36e2_eb1c_432d) > q) {
        let s = q * q;
        let mut p = d(0x3fc9_9999_9996_43a3) - s * d(0x3fc2_4924_82bd_6be1);
        p *= s;
        q -= (d(0x3fd5_5555_5555_5538) - p) * (s * q);
    }
    if swap {
        q = PIBY2 - q;
    }
    if xneg {
        q = PI - q;
    }
    signed(q)
}

/// The double `sin`, the FMA3 path (0x180098b00).
pub fn sin(x: f64) -> f64 {
    const DS1: f64 = f64::from_bits(0xbfc5_5555_5555_5555);
    const DS2: f64 = f64::from_bits(0x3f81_1111_1111_0bb3);
    const DS3: f64 = f64::from_bits(0xbf2a_01a0_19e8_3e5c);
    const DS4: f64 = f64::from_bits(0x3ec7_1de3_796c_de01);
    const DS5: f64 = f64::from_bits(0xbe5a_e600_b42f_dfa7);
    const DS6: f64 = f64::from_bits(0x3de5_e0b2_f9a4_3bb8);
    const DC1: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
    const DC2: f64 = f64::from_bits(0xbf56_c16c_16c1_6967);
    const DC3: f64 = f64::from_bits(0x3efa_01a0_19f4_ec91);
    const DC4: f64 = f64::from_bits(0xbe92_7e4f_a17f_667b);
    const DC5: f64 = f64::from_bits(0x3e21_eeb6_9038_2eec);
    const DC6: f64 = f64::from_bits(0xbda9_07db_4725_8aa7);
    let bits = x.to_bits();
    let ax = bits & ABS64;
    if ax < PI_BY_4 {
        if ax >= 0x3f20_0000_0000_0000 {
            let x2 = x * x;
            let mut p = fma(DS6, x2, DS5);
            p = fma(p, x2, DS4);
            p = fma(p, x2, DS3);
            p = fma(p, x2, DS2);
            let x3 = x * x2;
            p = fma(p, x2, DS1);
            return fma(x3, p, x);
        }
        if ax >= 0x3e40_0000_0000_0000 {
            let x3 = (x * x) * x;
            return fma(-x3, d(0x3fc5_5555_5555_5555), x);
        }
        return x;
    }
    if ax >= 0x7ff0_0000_0000_0000 {
        // `_sin_special` 0x1800a1974
        if ax == 0x7ff0_0000_0000_0000 {
            return d(0xfff8_0000_0000_0000);
        }
        return d(bits | 0x0008_0000_0000_0000);
    }
    let (r, rr, region) = if ax >= 0x4173_12d0_0000_0000 {
        reduce_large_double(ax)
    } else {
        // `__remainder_piby2_fma3` for medium arguments (0x1800a18c0)
        const TAIL: f64 = f64::from_bits(0x3c91_a626_3314_5c00);
        const TAIL2: f64 = f64::from_bits(0x397b_839a_2520_49c0);
        const MAGIC: f64 = f64::from_bits(0x4338_0000_0000_0000);
        let a = d(ax);
        let npi2 = fma(a, TWO_BY_PI, MAGIC) - MAGIC;
        let region = (trunc_i32(npi2) as u32 as u64) & 3;
        let rhead = fma(-npi2, PIBY2, a);
        let rtail = npi2 * TAIL;
        let error = fma(npi2, TAIL, -rtail);
        let t = rhead - rtail;
        let mut w = (rhead - t) - rtail;
        let r = fma(-npi2, TAIL, rhead);
        w = ((t - r) + w) - error;
        let rr = fma(-npi2, TAIL2, w);
        (r, rr, region)
    };
    let x2 = r * r;
    let value = if region & 1 == 0 {
        let mut p = fma(DS6, x2, DS5);
        p = fma(p, x2, DS4);
        p = fma(p, x2, DS3);
        p = fma(p, x2, DS2);
        let x3 = r * x2;
        let mut t = rr * 0.5 - x3 * p;
        t = x2 * t - rr;
        t = fma(-x3, DS1, t);
        r - t
    } else {
        let half = x2 * 0.5;
        let t = 1.0 - half;
        let mut e = (1.0 - t) - half;
        e = fma(-r, rr, e);
        let x4 = x2 * x2;
        let mut p = fma(DC6, x2, DC5);
        p = fma(p, x2, DC4);
        p = fma(p, x2, DC3);
        p = fma(p, x2, DC2);
        p = fma(p, x2, DC1);
        fma(p, x4, e) + t
    };
    let mut sign = bits & SIGN64;
    if region & 2 != 0 {
        sign ^= SIGN64;
    }
    d(value.to_bits() ^ sign)
}
