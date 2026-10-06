//! Dense matrix routines: `ode/src/matrix.cpp`, `fastdot.c`, `fastldlt.c`, `fastlsolve.c`,
//! `fastltsolve.c`.
//!
//! Matrices are row-major with `nskip` values per row (`nskip = pad(n)`). The C sources are
//! machine-generated, hand-unrolled loops (2 rows by 2 / 6 columns, 4 rows by 12 columns …).
//! The unrolling only repeats the same statement for consecutive columns, and every
//! accumulator (`Z11`, `Z21` …) receives its products in ascending column order, so the
//! loops below are written once per column and still add in exactly the same order.
//!
//! The exceptions are the loops with a single accumulator over unit-stride arrays: the
//! library in `acs.exe` was built with the compiler's fast floating-point model, which
//! turned those into packed sums (four lanes plus four lanes, folded at the end). They are
//! the last `n % 4` rows of `_dSolveL1` ([`solve_l1`]) and, for 8 or more columns, the sums
//! of the Cholesky routines; each is written out below as the machine code computes it.

use crate::common::pad;
use rustyac_math::sqrtf;

/// `_dDot` @ 0x140390530: plain dot product, products added in index order.
pub fn dot(a: &[f32], b: &[f32], n: usize) -> f32 {
    let mut sum = 0.0f32;
    for k in 0..n {
        sum += a[k] * b[k];
    }
    sum
}

/// `dSolveL1_2` @ 0x140399190 (static in `fastldlt.c`, called by `_dFactorLDLT`): solves `L * X = B`
/// for two right-hand sides at once. `L` is the top-left `n` x `n` unit lower triangle of
/// `a`; the two right-hand sides are the rows of `a` starting at `b_off` and
/// `b_off + lskip1`. `n` is even.
fn solve_l1_2(a: &mut [f32], b_off: usize, n: usize, lskip1: usize) {
    let mut i = 0;
    while i < n {
        let (mut z11, mut z12, mut z21, mut z22) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let ell = i * lskip1;
        for k in 0..i {
            let p1 = a[ell + k];
            let q1 = a[b_off + k];
            let q2 = a[b_off + k + lskip1];
            let p2 = a[ell + k + lskip1];
            z11 += p1 * q1;
            z12 += p1 * q2;
            z21 += p2 * q1;
            z22 += p2 * q2;
        }
        // finish computing the X(i) block
        let ex = b_off + i;
        z11 = a[ex] - z11;
        a[ex] = z11;
        z12 = a[ex + lskip1] - z12;
        a[ex + lskip1] = z12;
        let p1 = a[ell + i + lskip1];
        z21 = a[ex + 1] - z21 - p1 * z11;
        a[ex + 1] = z21;
        z22 = a[ex + 1 + lskip1] - z22 - p1 * z12;
        a[ex + 1 + lskip1] = z22;
        i += 2;
    }
}

/// `dSolveL1_1` @ 0x140398fc0 (static in `fastldlt.c`): as [`solve_l1_2`] for one right-hand side.
fn solve_l1_1(a: &mut [f32], b_off: usize, n: usize, lskip1: usize) {
    let mut i = 0;
    while i < n {
        let (mut z11, mut z21) = (0.0f32, 0.0f32);
        let ell = i * lskip1;
        for k in 0..i {
            let p1 = a[ell + k];
            let q1 = a[b_off + k];
            let p2 = a[ell + k + lskip1];
            z11 += p1 * q1;
            z21 += p2 * q1;
        }
        let ex = b_off + i;
        z11 = a[ex] - z11;
        a[ex] = z11;
        let p1 = a[ell + i + lskip1];
        z21 = a[ex + 1] - z21 - p1 * z11;
        a[ex + 1] = z21;
        i += 2;
    }
}

/// `_dFactorLDLT` @ 0x1403988d0: factorises the symmetric matrix `A` (lower triangle used)
/// in place into `L * D * L^T`. `L` (unit diagonal, not stored) replaces the strict lower
/// triangle, `d` receives the **reciprocals** of the diagonal of `D`.
pub fn factor_ldlt(a: &mut [f32], d: &mut [f32], n: usize, nskip1: usize) {
    if n < 1 {
        return;
    }
    let mut i = 0;
    while i + 2 <= n {
        // solve L*(D*l)=a, l is scaled elements in 2 x i block at A(i,0)
        solve_l1_2(a, i * nskip1, i, nskip1);
        // scale the elements in a 2 x i block at A(i,0), and also
        // compute Z = the outer product matrix that we'll need.
        let (mut z11, mut z21, mut z22) = (0.0f32, 0.0f32, 0.0f32);
        let ell = i * nskip1;
        for k in 0..i {
            let p1 = a[ell + k];
            let p2 = a[ell + k + nskip1];
            let dd = d[k];
            let q1 = p1 * dd;
            let q2 = p2 * dd;
            a[ell + k] = q1;
            a[ell + k + nskip1] = q2;
            z11 += p1 * q1;
            z21 += p2 * q1;
            z22 += p2 * q2;
        }
        // solve for diagonal 2 x 2 block at A(i,i)
        let ell = ell + i;
        z11 = a[ell] - z11;
        z21 = a[ell + nskip1] - z21;
        z22 = a[ell + 1 + nskip1] - z22;
        // factorize 2 x 2 block Z,dee
        // factorize row 1
        d[i] = 1.0f32 / z11;
        // factorize row 2 (the source's `sum = 0; sum += q1*q2` is just `q1*q2` in the binary)
        let q1 = z21;
        let q2 = q1 * d[i];
        z21 = q2;
        d[i + 1] = 1.0f32 / (z22 - q1 * q2);
        // done factorizing 2 x 2 block
        a[ell + nskip1] = z21;
        i += 2;
    }
    // compute the (less than 2) rows at the bottom
    if n - i == 1 {
        solve_l1_1(a, i * nskip1, i, nskip1);
        // scale the elements in a 1 x i block at A(i,0), and also
        // compute Z = the outer product matrix that we'll need.
        let mut z11 = 0.0f32;
        let ell = i * nskip1;
        for k in 0..i {
            let p1 = a[ell + k];
            let dd = d[k];
            let q1 = p1 * dd;
            a[ell + k] = q1;
            z11 += p1 * q1;
        }
        // solve for diagonal 1 x 1 block at A(i,i)
        z11 = a[ell + i] - z11;
        d[i] = 1.0f32 / z11;
    }
}

/// `_dSolveL1` @ 0x140390610: solves `L * x = b` in place, `L` unit lower triangular
/// (`n` x `n`, row skip `lskip1`, the diagonal is not read).
pub fn solve_l1(l: &[f32], b: &mut [f32], n: usize, lskip1: usize) {
    let lskip2 = 2 * lskip1;
    let lskip3 = 3 * lskip1;
    let mut i = 0;
    // compute all 4 x 1 blocks of X
    while i + 4 <= n {
        let (mut z11, mut z21, mut z31, mut z41) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let row = i * lskip1;
        for k in 0..i {
            let q1 = b[k];
            z11 += l[row + k] * q1;
            z21 += l[row + k + lskip1] * q1;
            z31 += l[row + k + lskip2] * q1;
            z41 += l[row + k + lskip3] * q1;
        }
        // finish computing the X(i) block
        let ell = row + i;
        z11 = b[i] - z11;
        b[i] = z11;
        let p1 = l[ell + lskip1];
        z21 = b[i + 1] - z21 - p1 * z11;
        b[i + 1] = z21;
        let p1 = l[ell + lskip2];
        let p2 = l[ell + 1 + lskip2];
        z31 = b[i + 2] - z31 - p1 * z11 - p2 * z21;
        b[i + 2] = z31;
        let p1 = l[ell + lskip3];
        let p2 = l[ell + 1 + lskip3];
        let p3 = l[ell + 2 + lskip3];
        z41 = b[i + 3] - z41 - p1 * z11 - p2 * z21 - p3 * z31;
        b[i + 3] = z41;
        i += 4;
    }
    // compute rows at end that are not a multiple of block size
    while i < n {
        let z11 = dot_blocks_of_12(&l[i * lskip1..], b, i);
        b[i] -= z11;
        i += 1;
    }
}

/// How the compiled code folds two packed accumulators into one number:
/// `(w2 + w0) + (w3 + w1)` with `w = v3 + v4` lane by lane (`addps`, `movhlps`, `addps`,
/// `shufps`, `addss`).
#[inline(always)]
fn fold_lanes(v3: &[f32; 4], v4: &[f32; 4]) -> f32 {
    let w = [v3[0] + v4[0], v3[1] + v4[1], v3[2] + v4[2], v3[3] + v4[3]];
    (w[2] + w[0]) + (w[3] + w[1])
}

/// The single-row sum `sum(a[k] * b[k], k < n)` as the compiler built it for the last rows
/// of `_dSolveL1` (0x140390db0 … 0x140390fb4). The source adds the products one by one in
/// blocks of twelve; the compiled loop (fast floating-point model) splits each block of
/// twelve over three accumulators:
///
/// * products 0..3 go into the four lanes of one packed accumulator (`addps xmm3`),
/// * products 4..7 into the four lanes of a second one (`addps xmm4`),
/// * products 8..11 are added one after the other to the scalar sum,
///
/// and after the last whole block the lanes are folded as
/// `scalar + (((x2 + x0)) + (x3 + x1))` with `x[k] = lane3[k] + lane4[k]`. What is left
/// (fewer than twelve products) is added to the scalar sum one by one.
fn dot_blocks_of_12(a: &[f32], b: &[f32], n: usize) -> f32 {
    let mut sum = 0.0f32;
    let blocks = n / 12;
    if blocks > 0 {
        let mut lane3 = [0.0f32; 4];
        let mut lane4 = [0.0f32; 4];
        for block in 0..blocks {
            let base = 12 * block;
            for k in 0..4 {
                lane3[k] += a[base + k] * b[base + k];
                lane4[k] += a[base + 4 + k] * b[base + 4 + k];
            }
            for k in 8..12 {
                sum += a[base + k] * b[base + k];
            }
        }
        sum += fold_lanes(&lane3, &lane4);
    }
    for k in 12 * blocks..n {
        sum += a[k] * b[k];
    }
    sum
}

/// `_dSolveL1T` @ 0x140390fd0: solves `L^T * x = b` in place (same `L` as [`solve_l1`]),
/// working from the last unknown backwards.
pub fn solve_l1t(l: &[f32], b: &mut [f32], n: usize, lskip1: usize) {
    if n == 0 {
        return;
    }
    let last = n - 1;
    // element (row, col) of L
    let at = |row: usize, col: usize| l[row * lskip1 + col];
    let mut i = 0;
    // compute all 4 x 1 blocks of X (the unknowns last - i, last - i - 1, ...)
    while i + 4 <= n {
        let (mut z11, mut z21, mut z31, mut z41) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let col = last - i;
        for k in 0..i {
            let row = last - k;
            let q1 = b[row];
            z11 += at(row, col) * q1;
            z21 += at(row, col - 1) * q1;
            z31 += at(row, col - 2) * q1;
            z41 += at(row, col - 3) * q1;
        }
        // finish computing the X(i) block
        z11 = b[col] - z11;
        b[col] = z11;
        let p1 = at(col, col - 1);
        z21 = b[col - 1] - z21 - p1 * z11;
        b[col - 1] = z21;
        let p1 = at(col, col - 2);
        let p2 = at(col - 1, col - 2);
        z31 = b[col - 2] - z31 - p1 * z11 - p2 * z21;
        b[col - 2] = z31;
        let p1 = at(col, col - 3);
        let p2 = at(col - 1, col - 3);
        let p3 = at(col - 2, col - 3);
        z41 = b[col - 3] - z41 - p1 * z11 - p2 * z21 - p3 * z31;
        b[col - 3] = z41;
        i += 4;
    }
    // compute rows at end that are not a multiple of block size
    while i < n {
        let mut z11 = 0.0f32;
        let col = last - i;
        for k in 0..i {
            let row = last - k;
            z11 += at(row, col) * b[row];
        }
        z11 = b[col] - z11;
        b[col] = z11;
        i += 1;
    }
}

/// `_dVectorScale` @ 0x14034ce60: `a[i] *= d[i]`.
pub fn vector_scale(a: &mut [f32], d: &[f32], n: usize) {
    for i in 0..n {
        a[i] *= d[i];
    }
}

/// `_dSolveLDLT` @ 0x14034cdf0: solves `L * D * L^T * x = b` in place with the factors made
/// by [`factor_ldlt`].
pub fn solve_ldlt(l: &[f32], d: &[f32], b: &mut [f32], n: usize, nskip: usize) {
    solve_l1(l, b, n, nskip);
    vector_scale(b, d, n);
    solve_l1t(l, b, n, nskip);
}

/// `_dFactorCholesky` @ 0x14034b7f0: in-place Cholesky factorisation (lower triangle) of an
/// `n` x `n` matrix with row skip `pad(n)`. Returns false if the matrix is not positive
/// definite.
///
/// A sum over 8 or more columns starts with packed blocks of eight (two four-lane
/// accumulators that start at zero and are folded into the sum afterwards); the game's only
/// caller, `dBodySetMass`, passes n = 3, where everything is in source order.
pub fn factor_cholesky(a: &mut [f32], n: usize) -> bool {
    let nskip = pad(n);
    let mut recip = vec![0.0f32; n];
    for i in 0..n {
        let aa = i * nskip;
        for j in 0..i {
            let bb = j * nskip;
            let mut sum = a[aa + j];
            let packed = (j / 8) * 8;
            if packed > 0 {
                let (mut v3, mut v4) = ([0.0f32; 4], [0.0f32; 4]);
                for o in (0..packed).step_by(8) {
                    for k in 0..4 {
                        v3[k] -= a[bb + o + k] * a[aa + o + k];
                        v4[k] -= a[bb + o + 4 + k] * a[aa + o + 4 + k];
                    }
                }
                sum += fold_lanes(&v3, &v4);
            }
            for k in packed..j {
                sum -= a[aa + k] * a[bb + k];
            }
            a[aa + j] = sum * recip[j];
        }
        let mut sum = a[aa + i];
        let packed = (i / 8) * 8;
        if packed > 0 {
            let (mut v3, mut v4) = ([0.0f32; 4], [0.0f32; 4]);
            for o in (0..packed).step_by(8) {
                for k in 0..4 {
                    v3[k] -= a[aa + o + k] * a[aa + o + k];
                    v4[k] -= a[aa + o + 4 + k] * a[aa + o + 4 + k];
                }
            }
            sum += fold_lanes(&v3, &v4);
        }
        for k in packed..i {
            sum -= a[aa + k] * a[aa + k];
        }
        // `comiss sum, 0` + `jbe`: zero, negative or NaN
        if !(sum > 0.0) {
            return false;
        }
        let sumsqrt = sqrtf(sum);
        a[aa + i] = sumsqrt;
        recip[i] = 1.0f32 / sumsqrt;
    }
    true
}

/// `_dSolveCholesky` @ 0x14034ca70: solves `L * L^T * x = b` in place. The forward sums use
/// packed blocks of eight for 8 or more columns (see [`factor_cholesky`]); here the folded
/// lanes **are** the start of the sum.
pub fn solve_cholesky(l: &[f32], b: &mut [f32], n: usize) {
    let nskip = pad(n);
    let mut y = vec![0.0f32; n];
    for i in 0..n {
        let ll = i * nskip;
        let mut sum = 0.0f32;
        let packed = (i / 8) * 8;
        if packed > 0 {
            let (mut v2, mut v3) = ([0.0f32; 4], [0.0f32; 4]);
            for o in (0..packed).step_by(8) {
                for k in 0..4 {
                    v2[k] += l[ll + o + k] * y[o + k];
                    v3[k] += l[ll + o + 4 + k] * y[o + 4 + k];
                }
            }
            sum = fold_lanes(&v2, &v3);
        }
        for k in packed..i {
            sum += l[ll + k] * y[k];
        }
        y[i] = (b[i] - sum) / l[ll + i];
    }
    for i in (0..n).rev() {
        let mut sum = 0.0f32;
        for k in i + 1..n {
            sum += l[k * nskip + i] * b[k];
        }
        b[i] = (y[i] - sum) / l[i * nskip + i];
    }
}

/// `_dInvertPDMatrix` @ 0x14034bac0: inverse of a positive definite matrix through its
/// Cholesky factors, one unit vector at a time. Returns false (and leaves `ainv` alone) if
/// `a` is not positive definite.
pub fn invert_pd_matrix(a: &[f32], ainv: &mut [f32], n: usize) -> bool {
    let nskip = pad(n);
    let mut l = a[..nskip * n].to_vec();
    if !factor_cholesky(&mut l, n) {
        return false;
    }
    for v in ainv[..nskip * n].iter_mut() {
        *v = 0.0;
    }
    let mut x = vec![0.0f32; n];
    for i in 0..n {
        for v in x.iter_mut() {
            *v = 0.0;
        }
        x[i] = 1.0;
        solve_cholesky(&l, &mut x, n);
        for j in 0..n {
            ainv[j * nskip + i] = x[j];
        }
    }
    true
}

/// `_dIsPositiveDefinite` @ 0x14034bce0.
pub fn is_positive_definite(a: &[f32], n: usize) -> bool {
    let nskip = pad(n);
    let mut copy = a[..nskip * n].to_vec();
    factor_cholesky(&mut copy, n)
}
