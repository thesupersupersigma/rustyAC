// SPDX-License-Identifier: BSD-3-Clause

//! ODE master 66615b8a dense kernels in f64, as Custom Shaders Patch 0.3.2 (build 4305)
//! compiled them: `fastldltfactor_impl.h`, `fastlsolve_impl.h`, `fastltsolve_impl.h`,
//! `fastdot_impl.h` and `matrix.cpp`'s `dxLDLTAddTL` / `dxLDLTRemove`.
//!
//! The f32 path keeps the 0.13.1 kernels in `matrix.rs`; master's are laid out differently
//! (blocks of 2 rows in the factoriser, a row stride for `d` and `b`).
//!
//! CSP was built with a fast floating-point model, so the sums follow the machine code, not
//! the source (docs/csp/core.md, Task C5d): in every unrolled loop the first columns add one
//! by one, but the big block's products are added as `((z + a0) + a1) + (a2 + a3)` (4 columns)
//! or `(((z + a0) + a1) + (a2 + a3)) + ((a4 + a5) + (a6 + a7))` (8 columns).

use crate::matrix::remove_row_col;

#[inline(always)]
fn sum4(z: f64, a: [f64; 4]) -> f64 {
    ((z + a[0]) + a[1]) + (a[2] + a[3])
}

#[inline(always)]
fn sum8(z: f64, a: [f64; 8]) -> f64 {
    (((z + a[0]) + a[1]) + (a[2] + a[3])) + ((a[4] + a[5]) + (a[6] + a[7]))
}

/// `calculateLargeVectorDot<S>` (`dxDot` is `S = 1`, CSP +0x16cb540; `S = 2` +0x1703b80):
/// `a . b` over `n` values, `b` with stride `S`. Blocks of four add `((m3 + m2) + (m1 + m0))`.
pub fn dot<const S: usize>(a: &[f64], b: &[f64], n: usize) -> f64 {
    let mut sum = 0.0;
    let n4 = n & !3;
    let mut i = 0;
    while i < n4 {
        let m0 = a[i] * b[i * S];
        let m1 = a[i + 1] * b[(i + 1) * S];
        let m2 = a[i + 2] * b[(i + 2) * S];
        let m3 = a[i + 3] * b[(i + 3) * S];
        sum += (m3 + m2) + (m1 + m0);
        i += 4;
    }
    while i < n {
        sum += a[i] * b[i * S];
        i += 1;
    }
    sum
}

/// `factorMatrixAsLDLT<DS>` (CSP `DS = 1` +0x1700e90, `DS = 2` +0x1701f60): `A = L*D*L^T` in
/// place for the `n` x `n` matrix `a` (row skip `rs`); `d[i * DS]` gets `1 / D[i]`.
pub fn factor_ldlt<const DS: usize>(a: &mut [f64], d: &mut [f64], n: usize, rs: usize) {
    if n < 1 {
        return;
    }
    let last = if n >= 2 { n - 1 } else { 0 };
    let mut row = 0;
    let mut sub = false;
    while row < last {
        let ar = row * rs;
        if sub {
            solve_l1_stripe_2(a, ar, row, rs);
            factor_stripe_2::<DS>(a, ar, d, row, rs);
        } else {
            let dd = 1.0 / a[0];
            d[0] = dd;
            let y21 = a[rs];
            let q2 = y21 * dd;
            a[rs] = q2;
            d[DS] = 1.0 / (a[1 + rs] - y21 * q2);
        }
        sub = true;
        row += 2;
    }
    if !sub || row == last {
        let ar = row * rs;
        if sub {
            solve_stripe_l1_1(a, ar, row, rs);
            factor_stripe_1::<DS>(a, ar, d, row);
        } else {
            d[0] = 1.0 / a[0];
        }
    }
}

/// `solveL1Stripe_2` (CSP +0x16fe080): solves `L*X = B` for the two rows of `B` at `b0`
/// (both inside `a`, the `n` rows of `L` start at 0).
fn solve_l1_stripe_2(a: &mut [f64], b0: usize, n: usize, rs: usize) {
    let mut bsr = 0;
    loop {
        let (mut z11, mut z12, mut z21, mut z22) = (0.0, 0.0, 0.0, 0.0);
        let mut l = bsr * rs;
        let mut b = b0;
        if bsr != 0 {
            let mut cc = bsr;
            loop {
                for k in 0..2 {
                    let p1 = a[l + k];
                    let q1 = a[b + k];
                    z11 += p1 * q1;
                    let q2 = a[b + k + rs];
                    z12 += p1 * q2;
                    let p2 = a[l + k + rs];
                    z21 += p2 * q1;
                    z22 += p2 * q2;
                }
                if cc > 6 {
                    cc -= 6;
                    let (mut x11, mut x12, mut x21, mut x22) = ([0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]);
                    for j in 0..4 {
                        let k = 2 + j;
                        let (p1, q1, q2, p2) = (a[l + k], a[b + k], a[b + k + rs], a[l + k + rs]);
                        x11[j] = p1 * q1;
                        x12[j] = p1 * q2;
                        x21[j] = p2 * q1;
                        x22[j] = p2 * q2;
                    }
                    z11 = sum4(z11, x11);
                    z12 = sum4(z12, x12);
                    z21 = sum4(z21, x21);
                    z22 = sum4(z22, x22);
                    l += 6;
                    b += 6;
                } else {
                    l += 2;
                    b += 2;
                    cc -= 2;
                    if cc == 0 {
                        break;
                    }
                }
            }
        }
        let y11 = a[b] - z11;
        let y12 = a[b + rs] - z12;
        let p2 = a[l + rs];
        a[b] = y11;
        a[b + rs] = y12;
        a[b + 1] = (a[b + 1] - z21) - p2 * y11;
        a[b + 1 + rs] = (a[b + 1 + rs] - z22) - p2 * y12;
        bsr += 2;
        if bsr == n {
            break;
        }
    }
}

/// `scaleAndFactorizeL1Stripe_2<DS>` (CSP +0x1702440 / +0x1703870).
fn factor_stripe_2<const DS: usize>(a: &mut [f64], ar: usize, d: &mut [f64], n: usize, rs: usize) {
    let (mut z11, mut z21, mut z22) = (0.0, 0.0, 0.0);
    let mut p = ar;
    let mut dp = 0;
    let mut cc = n;
    loop {
        for k in 0..2 {
            let (p1, p2, dd) = (a[p + k], a[p + k + rs], d[dp + k * DS]);
            let (q1, q2) = (p1 * dd, p2 * dd);
            a[p + k] = q1;
            a[p + k + rs] = q2;
            z11 += p1 * q1;
            z21 += p2 * q1;
            z22 += p2 * q2;
        }
        if cc > 6 {
            cc -= 6;
            let (mut x11, mut x21, mut x22) = ([0.0; 4], [0.0; 4], [0.0; 4]);
            for j in 0..4 {
                let k = 2 + j;
                let (p1, p2, dd) = (a[p + k], a[p + k + rs], d[dp + k * DS]);
                let (q1, q2) = (p1 * dd, p2 * dd);
                a[p + k] = q1;
                a[p + k + rs] = q2;
                x11[j] = p1 * q1;
                x21[j] = p2 * q1;
                x22[j] = p2 * q2;
            }
            z11 = sum4(z11, x11);
            z21 = sum4(z21, x21);
            z22 = sum4(z22, x22);
            p += 6;
            dp += 6 * DS;
        } else {
            p += 2;
            dp += 2 * DS;
            cc -= 2;
            if cc == 0 {
                break;
            }
        }
    }
    let y11 = a[p] - z11;
    let y21 = a[p + rs] - z21;
    let y22 = a[p + 1 + rs] - z22;
    let dd = 1.0 / y11;
    d[dp] = dd;
    let q2 = y21 * dd;
    a[p + rs] = q2;
    d[dp + DS] = 1.0 / (y22 - y21 * q2);
}

/// `solveStripeL1_1` (CSP +0x16fe3d0): like [`solve_l1_stripe_2`] for one row of `B`. The
/// compiler packed the 4-column block: `z + ((a0 + a2) + (a1 + a3))`.
fn solve_stripe_l1_1(a: &mut [f64], b0: usize, n: usize, rs: usize) {
    let mut bsr = 0;
    loop {
        let (mut z11, mut z21) = (0.0, 0.0);
        let mut l = bsr * rs;
        let mut b = b0;
        if bsr != 0 {
            let mut cc = bsr;
            loop {
                for k in 0..2 {
                    let q1 = a[b + k];
                    z11 += a[l + k] * q1;
                    z21 += a[l + k + rs] * q1;
                }
                if cc > 6 {
                    cc -= 6;
                    let (mut x11, mut x21) = ([0.0; 4], [0.0; 4]);
                    for j in 0..4 {
                        let k = 2 + j;
                        let q1 = a[b + k];
                        x11[j] = a[l + k] * q1;
                        x21[j] = a[l + k + rs] * q1;
                    }
                    z11 += (x11[0] + x11[2]) + (x11[1] + x11[3]);
                    z21 += (x21[0] + x21[2]) + (x21[1] + x21[3]);
                    l += 6;
                    b += 6;
                } else {
                    l += 2;
                    b += 2;
                    cc -= 2;
                    if cc == 0 {
                        break;
                    }
                }
            }
        }
        let y11 = a[b] - z11;
        let p2 = a[l + rs];
        a[b] = y11;
        a[b + 1] = (a[b + 1] - z21) - p2 * y11;
        bsr += 2;
        if bsr == n {
            break;
        }
    }
}

/// `scaleAndFactorizeL1Stripe_1<DS>` (inlined in the factorisers).
fn factor_stripe_1<const DS: usize>(a: &mut [f64], ar: usize, d: &mut [f64], n: usize) {
    let (mut z11, mut z22) = (0.0, 0.0);
    let mut p = ar;
    let mut dp = 0;
    let mut cc = n;
    let pair = |a: &mut [f64], p: usize, dp: usize, z11: &mut f64, z22: &mut f64| {
        let (p1, dd1, dd2) = (a[p], d[dp], d[dp + DS]);
        let q1 = p1 * dd1;
        a[p] = q1;
        *z11 += p1 * q1;
        let p2 = a[p + 1];
        let q2 = p2 * dd2;
        a[p + 1] = q2;
        *z22 += p2 * q2;
    };
    loop {
        pair(a, p, dp, &mut z11, &mut z22);
        if cc > 6 {
            cc -= 6;
            pair(a, p + 2, dp + 2 * DS, &mut z11, &mut z22);
            pair(a, p + 4, dp + 4 * DS, &mut z11, &mut z22);
            p += 6;
            dp += 6 * DS;
        } else {
            p += 2;
            dp += 2 * DS;
            cc -= 2;
            if cc == 0 {
                break;
            }
        }
    }
    d[dp] = 1.0 / (a[p] - (z11 + z22));
}

/// `solveL1Straight<BS>` (CSP `BS = 1` +0x1701090, `BS = 2` +0x1702750): solves `L*X = B`,
/// `L` unit lower triangular (`n` rows, row skip `rs`), `b[i * BS]` overwritten with `X`.
pub fn solve_l1<const BS: usize>(l: &[f64], b: &mut [f64], n: usize, rs: usize) {
    let mut bsr = 0;
    let mut sub = false;
    let mut go = n >= 4;
    let last4 = if go { n - 4 } else { 0 };
    while go {
        let (mut z1, mut z2, mut z3, mut z4) = (0.0, 0.0, 0.0, 0.0);
        let mut lp;
        let mut bp = 0;
        if sub {
            // rows bsr..bsr+3 of L start at lp - rs, lp, lp + rs, lp + 2 rs
            lp = (1 + bsr) * rs;
            let mut cc = bsr;
            loop {
                for k in 0..4 {
                    let q1 = b[bp + k * BS];
                    z1 += l[lp - rs + k] * q1;
                    z2 += l[lp + k] * q1;
                    z3 += l[lp + rs + k] * q1;
                    z4 += l[lp + 2 * rs + k] * q1;
                }
                if cc > 12 {
                    cc -= 12;
                    let mut x = [[0.0; 8]; 4];
                    for j in 0..8 {
                        let k = 4 + j;
                        let q1 = b[bp + k * BS];
                        x[0][j] = l[lp - rs + k] * q1;
                        x[1][j] = l[lp + k] * q1;
                        x[2][j] = l[lp + rs + k] * q1;
                        x[3][j] = l[lp + 2 * rs + k] * q1;
                    }
                    z1 = sum8(z1, x[0]);
                    z2 = sum8(z2, x[1]);
                    z3 = sum8(z3, x[2]);
                    z4 = sum8(z4, x[3]);
                    lp += 12;
                    bp += 12 * BS;
                } else {
                    lp += 4;
                    bp += 4 * BS;
                    cc -= 4;
                    if cc == 0 {
                        break;
                    }
                }
            }
        } else {
            lp = rs;
        }
        let y11 = b[bp] - z1;
        b[bp] = y11;
        let y21 = (b[bp + BS] - z2) - l[lp] * y11;
        b[bp + BS] = y21;
        lp += rs;
        let y31 = ((b[bp + 2 * BS] - z3) - l[lp] * y11) - l[lp + 1] * y21;
        b[bp + 2 * BS] = y31;
        let y41 = (((b[bp + 3 * BS] - z4) - l[lp + rs] * y11) - l[lp + rs + 1] * y21)
            - l[lp + rs + 2] * y31;
        b[bp + 3 * BS] = y41;
        sub = true;
        bsr += 4;
        go = bsr <= last4;
    }
    while !sub || bsr < n {
        if sub {
            let (mut z11, mut z12) = (0.0, 0.0);
            let mut bp = 0;
            let mut lp = bsr * rs;
            let mut cc = bsr;
            if cc >= 4 {
                loop {
                    for k in [0, 2] {
                        z11 += l[lp + k] * b[bp + k * BS];
                        z12 += l[lp + k + 1] * b[bp + (k + 1) * BS];
                    }
                    if cc >= 16 {
                        cc -= 12;
                        for k in [4, 6, 8, 10] {
                            z11 += l[lp + k] * b[bp + k * BS];
                            z12 += l[lp + k + 1] * b[bp + (k + 1) * BS];
                        }
                        lp += 12;
                        bp += 12 * BS;
                    } else {
                        lp += 4;
                        bp += 4 * BS;
                        cc -= 4;
                        if cc < 4 {
                            break;
                        }
                    }
                }
            }
            if cc & 2 != 0 {
                z11 += l[lp] * b[bp];
                z12 += l[lp + 1] * b[bp + BS];
                lp += 2;
                bp += 2 * BS;
            }
            if cc & 1 != 0 {
                z11 += l[lp] * b[bp];
                bp += BS;
            }
            b[bp] -= z11 + z12;
        }
        sub = true;
        bsr += 1;
    }
}

/// `solveL1Transposed<BS>` (CSP `BS = 1` +0x17017b0, `BS = 2` +0x1702e80): solves
/// `L^T*X = B`, same layout as [`solve_l1`].
pub fn solve_l1t<const BS: usize>(l: &[f64], b: &mut [f64], n: usize, rs: usize) {
    let last_l = (n - 1) * (rs + 1);
    let last_b = (n - 1) * BS;
    let x1 = n % 4;
    let mut bsr = x1;
    let mut sub = false;
    if x1 != 0 {
        sub = true;
        let y11 = b[last_b];
        if x1 >= 2 {
            let y21 = b[last_b - BS] - l[last_l - 1] * y11;
            b[last_b - BS] = y21;
            if x1 > 2 {
                b[last_b - 2 * BS] =
                    (b[last_b - 2 * BS] - l[last_l - 2] * y11) - l[last_l - rs - 2] * y21;
            }
        }
    }
    while !sub || bsr < n {
        let (mut z4, mut z3, mut z2, mut z1) = (0.0, 0.0, 0.0, 0.0);
        let mut lp;
        let mut bp = last_b;
        if sub {
            lp = last_l - bsr;
            let mut rc = bsr;
            if !rc.is_multiple_of(2) {
                // the compiler dropped the `0 +`: matters for -0.0
                let q1 = b[bp];
                z4 = l[lp - 3] * q1;
                z3 = l[lp - 2] * q1;
                z2 = l[lp - 1] * q1;
                z1 = l[lp] * q1;
                lp -= rs;
                bp -= BS;
                rc -= 1;
            }
            let add = |lp: &mut usize, q1: f64, z: &mut [&mut f64; 4]| {
                *z[0] += l[*lp - 3] * q1;
                *z[1] += l[*lp - 2] * q1;
                *z[2] += l[*lp - 1] * q1;
                *z[3] += l[*lp] * q1;
                *lp -= rs;
            };
            if !rc.is_multiple_of(4) {
                add(&mut lp, b[bp], &mut [&mut z4, &mut z3, &mut z2, &mut z1]);
                add(&mut lp, b[bp - BS], &mut [&mut z4, &mut z3, &mut z2, &mut z1]);
                bp -= 2 * BS;
                rc -= 2;
            }
            if rc != 0 {
                loop {
                    for k in 0..4 {
                        add(&mut lp, b[bp - k * BS], &mut [&mut z4, &mut z3, &mut z2, &mut z1]);
                    }
                    if rc > 12 {
                        rc -= 12;
                        bp -= 12 * BS;
                        let mut x = [[0.0; 8]; 4];
                        for j in 0..8 {
                            let q1 = b[bp + (8 - j) * BS];
                            x[0][j] = l[lp - 3] * q1;
                            x[1][j] = l[lp - 2] * q1;
                            x[2][j] = l[lp - 1] * q1;
                            x[3][j] = l[lp] * q1;
                            lp -= rs;
                        }
                        z4 = sum8(z4, x[0]);
                        z3 = sum8(z3, x[1]);
                        z2 = sum8(z2, x[2]);
                        z1 = sum8(z1, x[3]);
                    } else {
                        bp -= 4 * BS;
                        rc -= 4;
                        if rc == 0 {
                            break;
                        }
                    }
                }
            }
        } else {
            lp = last_l;
        }
        let y11 = b[bp] - z1;
        b[bp] = y11;
        let y21 = (b[bp - BS] - z2) - l[lp - 1] * y11;
        b[bp - BS] = y21;
        let y31 = ((b[bp - 2 * BS] - z3) - l[lp - 2] * y11) - l[lp - rs - 2] * y21;
        b[bp - 2 * BS] = y31;
        b[bp - 3 * BS] = (((b[bp - 3 * BS] - z4) - l[lp - 3] * y11) - l[lp - rs - 3] * y21)
            - l[lp - 2 * rs - 3] * y31;
        sub = true;
        bsr += 4;
    }
}

/// `dxLDLTAddTL` (CSP +0x168ded0): rank-2 update of the factors `L` (from `l_off`, row skip
/// `nskip`) and `d` for `A + a*e0^T + e0*a^T`. `w` holds `2 * nskip` values.
#[allow(clippy::too_many_arguments)]
pub fn ldlt_add_tl(l: &mut [f64], l_off: usize, d: &mut [f64], a: &[f64], n: usize, nskip: usize, w: &mut [f64]) {
    if n < 2 {
        return;
    }
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let (w1, w2) = w.split_at_mut(nskip);
    w1[0] = 0.0;
    w2[0] = 0.0;
    for j in 1..n {
        let v = a[j] * s;
        w1[j] = v;
        w2[j] = v;
    }
    let a0h = a[0] * 0.5;
    let w11 = (a0h + 1.0) * s;
    let w21 = (a0h - 1.0) * s;
    let mut alpha1;
    let mut alpha2;
    {
        // the compiler folded alpha1 = alpha2 = 1 and wrote W21*W21 as (a0/2 - 1)^2 * s^2
        let alphanew = (w11 * w11) * d[0] + 1.0;
        let dee = d[0] / alphanew;
        let gamma1 = dee * w11;
        alpha2 = 1.0 - ((a0h - 1.0) * (a0h - 1.0)) * (dee * f64::from_bits(0x3fe0000000000001));
        alpha1 = alphanew;
        let k1 = 1.0 - gamma1 * w21;
        let k2 = (gamma1 * w21) * w11 - w21;
        let mut ll = l_off + nskip;
        for p in 1..n {
            let wp = w1[p];
            let ell = l[ll];
            w1[p] = wp - w11 * ell;
            w2[p] = k1 * wp + k2 * ell;
            ll += nskip;
        }
    }
    let mut ll = l_off + nskip + 1;
    for j in 1..n {
        let k1 = w1[j];
        let k2 = w2[j];
        let mut dee = d[j];
        let mut alphanew = alpha1 + (k1 * k1) * dee;
        dee /= alphanew;
        let gamma1 = k1 * dee;
        dee *= alpha1;
        alpha1 = alphanew;
        alphanew = alpha2 - (k2 * k2) * dee;
        dee /= alphanew;
        let gamma2 = k2 * dee;
        dee *= alpha2;
        d[j] = dee;
        alpha2 = alphanew;
        let mut lp = ll + nskip;
        for p in j + 1..n {
            let mut ell = l[lp];
            let mut wp = w1[p] - k1 * ell;
            ell += gamma1 * wp;
            w1[p] = wp;
            wp = w2[p] - k2 * ell;
            ell -= gamma2 * wp;
            w2[p] = wp;
            l[lp] = ell;
            lp += nskip;
        }
        ll += nskip + 1;
    }
}

/// `dxLDLTRemove` (CSP +0x168e5d0): removes row and column `r` from the factors of the `n2` x
/// `n2` matrix made of rows and columns `p[0..n2]` of `A` (`arows[i]`: where row `i` starts
/// in `a`; lower triangle).
#[allow(clippy::too_many_arguments)]
pub fn ldlt_remove(
    a: &[f64],
    arows: &[usize],
    p: &[u32],
    l: &mut [f64],
    d: &mut [f64],
    n2: usize,
    r: usize,
    nskip: usize,
    tmpbuf: &mut Vec<f64>,
) {
    if r == n2 - 1 {
        return;
    }
    let geta = |i: usize, j: usize| if i > j { a[arows[i] + j] } else { a[arows[j] + i] };
    tmpbuf.clear();
    tmpbuf.resize(2 * nskip + n2, 0.0);
    let (w, rest) = tmpbuf.split_at_mut(2 * nskip);
    if r == 0 {
        let p_0 = p[0] as usize;
        for i in 0..n2 {
            rest[i] = -geta(p[i] as usize, p_0);
        }
        rest[0] += 1.0;
        ldlt_add_tl(l, 0, d, rest, n2, nskip, w);
    } else {
        let (t, aa) = rest.split_at_mut(r);
        for i in 0..r {
            t[i] = l[r * nskip + i] / d[i];
        }
        let p_r = p[r] as usize;
        for i in 0..n2 - r {
            aa[i] = dot::<1>(&l[(r + i) * nskip..], t, r) - geta(p[r + i] as usize, p_r);
        }
        aa[0] += 1.0;
        ldlt_add_tl(l, (nskip + 1) * r, &mut d[r..], aa, n2 - r, nskip, w);
    }
    remove_row_col(l, n2, nskip, r);
    d.copy_within(r + 1..n2, r);
}
