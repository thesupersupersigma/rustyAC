// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `vec3f` / `mat44f` and the handful of vector functions the tyre calls, in the
//! original operation order. The two DirectXMath functions (`XMMatrixMultiply`,
//! `XMMatrixInverse`) are four-lane SSE code in the game; here every lane is computed
//! separately with the same operations, which gives the same bits.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::math::{cosf, sinf, sqrtf};

/// AC's `vec3f`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3f {
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3f {
        Vec3f { x, y, z }
    }

    /// `vec3f::normalize` @ 0x140024000: divides by the length unless the length is 0.
    pub fn normalize(&mut self) {
        let length = sqrtf(self.x * self.x + self.y * self.y + self.z * self.z);
        if length != 0.0 {
            self.x /= length;
            self.y /= length;
            self.z /= length;
        }
    }
}

/// AC's `mat44f`: row-major, `m[0][0]` is `M11`, `m[3][0..3]` (`M41..M43`) the translation.
/// Row 0 is the x axis (right), row 1 the y axis (up), row 2 the z axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mat44f {
    pub m: [[f32; 4]; 4],
}

impl Mat44f {
    pub const IDENTITY: Mat44f = Mat44f {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// `mat44f::createFromAxisAngle` @ 0x1400571a0. Parameter names are the PDB's.
    pub fn create_from_axis_angle(axis: &Vec3f, angle: f32) -> Mat44f {
        let c = cosf(angle);
        let s = sinf(angle);
        let (x, y, z) = (axis.x, axis.y, axis.z);
        let t = 1.0 - c;
        let mut out = Mat44f::default();
        out.m[3][3] = 1.0;
        let xy = y * x * t;
        out.m[0][0] = x * x * t + c;
        out.m[1][1] = y * y * t + c;
        out.m[2][2] = z * z * t + c;
        let zs = z * s;
        out.m[0][1] = zs + xy;
        out.m[1][0] = xy - zs;
        let ys = y * s;
        let zy = z * y * t;
        let zx = z * x * t;
        let xs = x * s;
        out.m[0][2] = zx - ys;
        out.m[2][0] = ys + zx;
        out.m[1][2] = xs + zy;
        out.m[2][1] = zy - xs;
        out
    }
}

type V = [f32; 4];

/// `_mm_shuffle_ps(a, b, _MM_SHUFFLE(w, z, y, x))`: lanes `[a[x], a[y], b[z], b[w]]`.
/// `XM_PERMUTE_PS(v, ..)` is the same with `a == b`.
macro_rules! sh {
    ($a:expr, $b:expr, $w:expr, $z:expr, $y:expr, $x:expr) => {
        [$a[$x], $a[$y], $b[$z], $b[$w]]
    };
}

fn mul(a: V, b: V) -> V {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

/// `DirectX::XMMatrixMultiply` @ 0x140056cf0 (`m1 * m2`): each row of the result is
/// `(x * r0 + z * r2) + (y * r1 + w * r3)` with `x..w` the row of `m1` and `r0..r3` the
/// rows of `m2`.
pub fn xm_matrix_multiply(m1: &Mat44f, m2: &Mat44f) -> Mat44f {
    let mut out = Mat44f::default();
    for (row, result) in m1.m.iter().zip(out.m.iter_mut()) {
        let x = mul([row[0]; 4], m2.m[0]);
        let y = mul([row[1]; 4], m2.m[1]);
        let z = mul([row[2]; 4], m2.m[2]);
        let w = mul([row[3]; 4], m2.m[3]);
        *result = add(add(z, x), add(w, y));
    }
    out
}

/// `mat44f::createFromAxisAngle` @ 0x1400571a0: the axis is used as it is (not normalised).
pub fn create_from_axis_angle(axis: [f32; 3], angle: f32) -> Mat44f {
    let [x, y, z] = axis;
    let c = crate::math::cosf(angle);
    let s = crate::math::sinf(angle);
    let t = 1.0f32 - c;
    let mut out = Mat44f::default();
    out.m[0][0] = (x * x) * t + c;
    out.m[1][1] = (y * y) * t + c;
    out.m[2][2] = (z * z) * t + c;
    let yx = (y * x) * t;
    let zs = z * s;
    out.m[0][1] = zs + yx;
    out.m[1][0] = yx - zs;
    let ys = y * s;
    let zx = (z * x) * t;
    let xs = x * s;
    let zy = (z * y) * t;
    out.m[0][2] = zx - ys;
    out.m[2][0] = ys + zx;
    out.m[1][2] = xs + zy;
    out.m[2][1] = zy - xs;
    out.m[3][3] = 1.0;
    out
}

/// `mat44f::createFromEuler` @ 0x140118ef0: heading about -Y, then pitch about X, then roll
/// about Z (radians), four signs flipped at the end, and the position in row 3.
pub fn create_from_euler(rot: [f32; 3], pos: [f32; 3]) -> Mat44f {
    let mut out = Mat44f::IDENTITY;
    out = xm_matrix_multiply(&create_from_axis_angle([0.0, 1.0, 0.0], -rot[0]), &out);
    out = xm_matrix_multiply(&create_from_axis_angle([1.0, 0.0, 0.0], rot[1]), &out);
    out = xm_matrix_multiply(&create_from_axis_angle([0.0, 0.0, 1.0], rot[2]), &out);
    out.m[0][1] = -out.m[0][1];
    out.m[1][0] = -out.m[1][0];
    out.m[1][2] = -out.m[1][2];
    out.m[2][1] = -out.m[2][1];
    out.m[3][0] = pos[0];
    out.m[3][1] = pos[1];
    out.m[3][2] = pos[2];
    out
}

/// `DirectX::XMMatrixInverse` @ 0x14006c280 (the SSE version of DirectXMath 3.0x, cofactor
/// expansion). A singular matrix divides by zero like the original.
pub fn xm_matrix_inverse(m: &Mat44f) -> Mat44f {
    // XMMatrixTranspose
    let mut mt = [[0.0f32; 4]; 4];
    for (r, row) in mt.iter_mut().enumerate() {
        for (c, value) in row.iter_mut().enumerate() {
            *value = m.m[c][r];
        }
    }

    let mut v00 = sh!(mt[2], mt[2], 1, 1, 0, 0);
    let mut v10 = sh!(mt[3], mt[3], 3, 2, 3, 2);
    let mut v01 = sh!(mt[0], mt[0], 1, 1, 0, 0);
    let mut v11 = sh!(mt[1], mt[1], 3, 2, 3, 2);
    let mut v02 = sh!(mt[2], mt[0], 2, 0, 2, 0);
    let mut v12 = sh!(mt[3], mt[1], 3, 1, 3, 1);

    let mut d0 = mul(v00, v10);
    let mut d1 = mul(v01, v11);
    let mut d2 = mul(v02, v12);

    v00 = sh!(mt[2], mt[2], 3, 2, 3, 2);
    v10 = sh!(mt[3], mt[3], 1, 1, 0, 0);
    v01 = sh!(mt[0], mt[0], 3, 2, 3, 2);
    v11 = sh!(mt[1], mt[1], 1, 1, 0, 0);
    v02 = sh!(mt[2], mt[0], 3, 1, 3, 1);
    v12 = sh!(mt[3], mt[1], 2, 0, 2, 0);

    v00 = mul(v00, v10);
    v01 = mul(v01, v11);
    v02 = mul(v02, v12);
    d0 = sub(d0, v00);
    d1 = sub(d1, v01);
    d2 = sub(d2, v02);

    // V11 = D0Y,D0W,D2Y,D2Y
    v11 = sh!(d0, d2, 1, 1, 3, 1);
    v00 = sh!(mt[1], mt[1], 1, 0, 2, 1);
    v10 = sh!(v11, d0, 0, 3, 0, 2);
    v01 = sh!(mt[0], mt[0], 0, 1, 0, 2);
    v11 = sh!(v11, d0, 2, 1, 2, 1);
    // V13 = D1Y,D1W,D2W,D2W
    let mut v13 = sh!(d1, d2, 3, 3, 3, 1);
    v02 = sh!(mt[3], mt[3], 1, 0, 2, 1);
    v12 = sh!(v13, d1, 0, 3, 0, 2);
    let mut v03 = sh!(mt[2], mt[2], 0, 1, 0, 2);
    v13 = sh!(v13, d1, 2, 1, 2, 1);

    let mut c0 = mul(v00, v10);
    let mut c2 = mul(v01, v11);
    let mut c4 = mul(v02, v12);
    let mut c6 = mul(v03, v13);

    // V11 = D0X,D0Y,D2X,D2X
    v11 = sh!(d0, d2, 0, 0, 1, 0);
    v00 = sh!(mt[1], mt[1], 2, 1, 3, 2);
    v10 = sh!(d0, v11, 2, 1, 0, 3);
    v01 = sh!(mt[0], mt[0], 1, 3, 2, 3);
    v11 = sh!(d0, v11, 0, 2, 1, 2);
    // V13 = D1X,D1Y,D2Z,D2Z
    v13 = sh!(d1, d2, 2, 2, 1, 0);
    v02 = sh!(mt[3], mt[3], 2, 1, 3, 2);
    v12 = sh!(d1, v13, 2, 1, 0, 3);
    v03 = sh!(mt[2], mt[2], 1, 3, 2, 3);
    v13 = sh!(d1, v13, 0, 2, 1, 2);

    v00 = mul(v00, v10);
    v01 = mul(v01, v11);
    v02 = mul(v02, v12);
    v03 = mul(v03, v13);
    c0 = sub(c0, v00);
    c2 = sub(c2, v01);
    c4 = sub(c4, v02);
    c6 = sub(c6, v03);

    v00 = sh!(mt[1], mt[1], 0, 3, 0, 3);
    // V10 = D0Z,D0Z,D2X,D2Y
    v10 = sh!(d0, d2, 1, 0, 2, 2);
    v10 = sh!(v10, v10, 0, 2, 3, 0);
    v01 = sh!(mt[0], mt[0], 2, 0, 3, 1);
    // V11 = D0X,D0W,D2X,D2Y
    v11 = sh!(d0, d2, 1, 0, 3, 0);
    v11 = sh!(v11, v11, 2, 1, 0, 3);
    v02 = sh!(mt[3], mt[3], 0, 3, 0, 3);
    // V12 = D1Z,D1Z,D2Z,D2W
    v12 = sh!(d1, d2, 3, 2, 2, 2);
    v12 = sh!(v12, v12, 0, 2, 3, 0);
    v03 = sh!(mt[2], mt[2], 2, 0, 3, 1);
    // V13 = D1X,D1W,D2Z,D2W
    v13 = sh!(d1, d2, 3, 2, 3, 0);
    v13 = sh!(v13, v13, 2, 1, 0, 3);

    v00 = mul(v00, v10);
    v01 = mul(v01, v11);
    v02 = mul(v02, v12);
    v03 = mul(v03, v13);
    let c1 = sub(c0, v00);
    c0 = add(c0, v00);
    let c3 = add(c2, v01);
    c2 = sub(c2, v01);
    let c5 = sub(c4, v02);
    c4 = add(c4, v02);
    let c7 = add(c6, v03);
    c6 = sub(c6, v03);

    c0 = sh!(c0, c1, 3, 1, 2, 0);
    c2 = sh!(c2, c3, 3, 1, 2, 0);
    c4 = sh!(c4, c5, 3, 1, 2, 0);
    c6 = sh!(c6, c7, 3, 1, 2, 0);
    c0 = sh!(c0, c0, 3, 1, 2, 0);
    c2 = sh!(c2, c2, 3, 1, 2, 0);
    c4 = sh!(c4, c4, 3, 1, 2, 0);
    c6 = sh!(c6, c6, 3, 1, 2, 0);

    // XMVector4Dot(C0, MT.r[0]): (x + z) + (y + w) of the lane products
    let p = mul(c0, mt[0]);
    let determinant = (p[1] + p[3]) + (p[0] + p[2]);
    let reciprocal = [1.0 / determinant; 4];
    Mat44f {
        m: [
            mul(c0, reciprocal),
            mul(c2, reciprocal),
            mul(c4, reciprocal),
            mul(c6, reciprocal),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_angle_about_x_keeps_x() {
        let m = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), 0.5);
        assert_eq!(m.m[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(m.m[3], [0.0, 0.0, 0.0, 1.0]);
        assert!((m.m[1][1] - 0.5f32.cos()).abs() < 1e-6);
        assert!((m.m[1][2] - 0.5f32.sin()).abs() < 1e-6);
    }

    #[test]
    fn inverse_times_matrix_is_identity() {
        let mut m = Mat44f::create_from_axis_angle(&Vec3f::new(0.6, 0.0, 0.8), 0.7);
        m.m[3] = [3.0, -2.0, 5.0, 1.0];
        let product = xm_matrix_multiply(&m, &xm_matrix_inverse(&m));
        for r in 0..4 {
            for c in 0..4 {
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (product.m[r][c] - expected).abs() < 1e-5,
                    "[{r}][{c}] = {}",
                    product.m[r][c]
                );
            }
        }
        assert_eq!(xm_matrix_multiply(&m, &Mat44f::IDENTITY), m);
    }

    #[test]
    fn normalize_leaves_zero_alone() {
        let mut v = Vec3f::new(3.0, 0.0, 4.0);
        v.normalize();
        assert_eq!(v, Vec3f::new(0.6, 0.0, 0.8));
        let mut zero = Vec3f::default();
        zero.normalize();
        assert_eq!(zero, Vec3f::default());
    }
}
