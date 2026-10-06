//! Small vector / matrix helpers: ODE's `include/ode/odemath.h` inline functions and
//! `ode/src/odemath.cpp`.
//!
//! Every function keeps the summation order of the code in `acs.exe` (ODE 0.13.1, single
//! precision, Visual C++ 2013 x64). A three-term sum `a + b + c` is always `(a + b) + c`.
//! Vectors are `[f32; 4]` (`dVector3`) or longer slices, matrices are 3x4 row-major
//! `[f32; 12]` (`dMatrix3`); the fourth column is padding that no function here reads.

use crate::common::{Matrix3, Vector3};
use rustyac_math::sqrtf;

/// `dCalcVectorDot3`.
#[inline(always)]
pub fn dot3(a: &[f32], b: &[f32]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `_dCalcVectorDot3(a, b, step_a, step_b)`: elements `step` apart.
#[inline(always)]
pub fn dot3_step(a: &[f32], b: &[f32], step_a: usize, step_b: usize) -> f32 {
    a[0] * b[0] + a[step_a] * b[step_b] + a[2 * step_a] * b[2 * step_b]
}

/// `dCalcVectorCross3`: `a x b`.
#[inline(always)]
pub fn cross3(a: &[f32], b: &[f32]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `dCalcVectorLength3`.
#[inline(always)]
pub fn length3(a: &[f32]) -> f32 {
    sqrtf(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
}

/// `dMultiply0_331`: `A * b` for a 3x3 `A` (stored 3x4) and a 3-vector.
#[inline(always)]
pub fn multiply0_331(a: &[f32], b: &[f32]) -> [f32; 3] {
    [dot3(&a[0..], b), dot3(&a[4..], b), dot3(&a[8..], b)]
}

/// `dMultiply1_331`: `A^T * b`.
#[inline(always)]
pub fn multiply1_331(a: &[f32], b: &[f32]) -> [f32; 3] {
    [dot3_step(&a[0..], b, 4, 1), dot3_step(&a[1..], b, 4, 1), dot3_step(&a[2..], b, 4, 1)]
}

/// `dMultiply0_133`: the row vector `a` times the 3x3 `B` (= `B^T * a`).
#[inline(always)]
pub fn multiply0_133(a: &[f32], b: &[f32]) -> [f32; 3] {
    multiply1_331(b, a)
}

/// `dMultiply0_333`: `A * B`. Only the nine 3x3 entries of the result are written.
#[inline(always)]
pub fn multiply0_333(res: &mut [f32], a: &[f32], b: &[f32]) {
    for row in 0..3 {
        let r = multiply0_133(&a[4 * row..], b);
        res[4 * row] = r[0];
        res[4 * row + 1] = r[1];
        res[4 * row + 2] = r[2];
    }
}

/// `dMultiply2_333`: `A * B^T`.
#[inline(always)]
pub fn multiply2_333(res: &mut [f32], a: &[f32], b: &[f32]) {
    for row in 0..3 {
        let r = multiply0_331(b, &a[4 * row..]);
        res[4 * row] = r[0];
        res[4 * row + 1] = r[1];
        res[4 * row + 2] = r[2];
    }
}

/// `dSetCrossMatrixPlus(res, a, skip)`: writes the six off-diagonal entries of the matrix
/// `[a]x` (so that `res * b = a x b`) into a row-major block with `skip` values per row.
#[inline(always)]
pub fn set_cross_matrix_plus(res: &mut [f32], a: &[f32], skip: usize) {
    let (a0, a1, a2) = (a[0], a[1], a[2]);
    res[1] = -a2;
    res[2] = a1;
    res[skip] = a2;
    res[skip + 2] = -a0;
    res[2 * skip] = -a1;
    res[2 * skip + 1] = a0;
}

/// `dSetCrossMatrixMinus(res, a, skip)`: the negated version.
#[inline(always)]
pub fn set_cross_matrix_minus(res: &mut [f32], a: &[f32], skip: usize) {
    let (a0, a1, a2) = (a[0], a[1], a[2]);
    res[1] = a2;
    res[2] = -a1;
    res[skip] = -a2;
    res[skip + 2] = a0;
    res[2 * skip] = a1;
    res[2 * skip + 1] = -a0;
}

/// `dInvertMatrix3` (local copy in `step.obj` at 0x14034fb10): closed-form inverse of a 3x3
/// matrix. Returns the determinant; 0 means "singular, `dst` not written".
///
/// The compiled code computes the three cofactors of the first row once, uses them for the
/// determinant and reuses the first one for `dst[0]`.
pub fn invert_matrix3(dst: &mut Matrix3, ma: &Matrix3) -> f32 {
    // det = ma0*(ma5*ma10 - ma6*ma9) - ma1*(ma4*ma10 - ma8*ma6) + ma2*(ma4*ma9 - ma8*ma5)
    let c0 = ma[5] * ma[10] - ma[6] * ma[9];
    let c1 = ma[4] * ma[10] - ma[8] * ma[6];
    let c2 = ma[4] * ma[9] - ma[8] * ma[5];
    let det = (ma[0] * c0 - ma[1] * c1) + ma[2] * c2;
    // `ucomiss det, 0` / `je`: a NaN determinant also counts as singular
    if det == 0.0 || det.is_nan() {
        return 0.0;
    }
    let det_recip = 1.0f32 / det;
    dst[0] = c0 * det_recip;
    dst[1] = (ma[9] * ma[2] - ma[1] * ma[10]) * det_recip;
    dst[2] = (ma[1] * ma[6] - ma[5] * ma[2]) * det_recip;
    dst[4] = (ma[6] * ma[8] - ma[4] * ma[10]) * det_recip;
    dst[5] = (ma[0] * ma[10] - ma[8] * ma[2]) * det_recip;
    dst[6] = (ma[4] * ma[2] - ma[0] * ma[6]) * det_recip;
    dst[8] = (ma[4] * ma[9] - ma[8] * ma[5]) * det_recip;
    dst[9] = (ma[8] * ma[1] - ma[0] * ma[9]) * det_recip;
    dst[10] = (ma[0] * ma[5] - ma[1] * ma[4]) * det_recip;
    det
}

/// `_dSafeNormalize3` @ 0x14034b410. Returns false (and sets `a` to (1, 0, 0)) for a zero
/// vector.
///
/// The source divides the three components by the largest one; the compiled code takes one
/// reciprocal (`divss` @ 0x14034b469) and multiplies three times, which is not the same in
/// the last bit. A NaN first component ends in the "zero vector" case (`comiss` + `jbe`).
pub fn safe_normalize3(a: &mut [f32]) -> bool {
    let aa = [a[0].abs(), a[1].abs(), a[2].abs()];
    let idx;
    if aa[1] > aa[0] {
        idx = if aa[2] > aa[1] { 2 } else { 1 };
    } else if aa[2] > aa[0] {
        idx = 2;
    } else {
        if !(aa[0] > 0.0) {
            a[0] = 1.0;
            a[1] = 0.0;
            a[2] = 0.0;
            return false;
        }
        idx = 0;
    }
    let r = 1.0f32 / aa[idx];
    let t0 = r * a[0];
    let t1 = r * a[1];
    let t2 = r * a[2];
    let l = 1.0f32 / sqrtf(t0 * t0 + t1 * t1 + t2 * t2);
    a[0] = t0 * l;
    a[1] = t1 * l;
    a[2] = t2 * l;
    true
}

/// `x != 0` as the compiled code tests it (`ucomiss x, 0` + `jne`): false for a NaN.
#[inline(always)]
pub fn is_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// `_dSafeNormalize4` @ 0x14034b500 (`dNormalize4` @ 0x14034b570 is a jump to it).
pub fn safe_normalize4(a: &mut [f32; 4]) -> bool {
    let l = dot3(a, a) + a[3] * a[3];
    if l > 0.0 {
        let l = 1.0f32 / sqrtf(l);
        a[0] *= l;
        a[1] *= l;
        a[2] *= l;
        a[3] *= l;
        true
    } else {
        *a = [1.0, 0.0, 0.0, 0.0];
        false
    }
}

/// `dPlaneSpace` @ 0x14034b6e0: two vectors `p`, `q` that span the plane perpendicular to the
/// unit vector `n`.
pub fn plane_space(n: &[f32], p: &mut Vector3, q: &mut Vector3) {
    // the compiled test is in double: (double)|n2| > 0.70710678118654757 (same outcome for
    // every f32 as the f32 test against 0x3f3504f3)
    if (n[2].abs() as f64) > std::f64::consts::FRAC_1_SQRT_2 {
        // choose p in y-z plane
        let a = n[1] * n[1] + n[2] * n[2];
        let k = 1.0f32 / sqrtf(a);
        p[0] = 0.0;
        p[1] = -n[2] * k;
        p[2] = n[1] * k;
        // set q = n x p
        q[0] = a * k;
        q[1] = -n[0] * p[2];
        q[2] = n[0] * p[1];
    } else {
        // choose p in x-y plane
        let a = n[0] * n[0] + n[1] * n[1];
        let k = 1.0f32 / sqrtf(a);
        p[0] = -n[1] * k;
        p[1] = n[0] * k;
        p[2] = 0.0;
        // set q = n x p
        q[0] = -n[2] * p[1];
        q[1] = n[2] * p[0];
        q[2] = a * k;
    }
}

/// `dOrthogonalizeR` @ 0x14034b580: makes the 3x3 part of `m` a proper rotation
/// (Gram-Schmidt on the first two rows, third row = cross product).
pub fn orthogonalize_r(m: &mut Matrix3) {
    // the three `!=` tests are `ucomiss` + `je`: a NaN counts as "equal" and skips the work
    let n0 = m[0] * m[0] + m[1] * m[1] + m[2] * m[2];
    if n0 != 1.0 && !n0.is_nan() {
        safe_normalize3(&mut m[0..4]);
    }
    // project row[0] on row[1], should be zero
    let proj = dot3(&m[0..], &m[4..]);
    if is_nonzero(proj) {
        // Gram-Schmidt step on row[1]
        m[4] -= proj * m[0];
        m[5] -= proj * m[1];
        m[6] -= proj * m[2];
    }
    let n1 = m[4] * m[4] + m[5] * m[5] + m[6] * m[6];
    if n1 != 1.0 && !n1.is_nan() {
        safe_normalize3(&mut m[4..8]);
    }
    // just overwrite row[2], this makes sure the matrix is not a reflection
    let c = cross3(&m[0..], &m[4..]);
    m[8] = c[0];
    m[9] = c[1];
    m[10] = c[2];
    m[3] = 0.0;
    m[7] = 0.0;
    m[11] = 0.0;
}
