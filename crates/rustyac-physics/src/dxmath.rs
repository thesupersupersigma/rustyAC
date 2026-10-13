// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `mat44f lerp(const mat44f&, const mat44f&, float)` @ 0x140108600 and the DirectXMath
//! functions it is made of, as they are compiled into the game, lane by lane:
//! `XMQuaternionRotationMatrix` 0x1400c4540, `XMQuaternionSlerpV` 0x140107300,
//! `XMVectorATan2` 0x140107420, `XMVectorATan` 0x140107670, `XMVectorSin` 0x140107790,
//! `XMMatrixRotationQuaternion` 0x140107260.
//!
//! The same code is in `rustyac-render` (`animator.rs`, where the render oracle proves it
//! against the game's suspension animation); the multiplayer and the replays cannot use the
//! renderer's (they have to build where there is no Direct3D), so the functions are here a
//! second time.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use rustyac_math::sqrtf;
use crate::vecmath::Mat44f;

/// `DirectX::XMVectorATan` 0x140107670, one lane.
fn xm_atan(v: f32) -> f32 {
    const TC0: [u32; 4] = [0xbeaa_aa6c, 0x3e4c_bbe5, 0xbe11_7fc7, 0x3dda_3d83];
    const TC1: [u32; 4] = [0xbd9a_3174, 0x3d2f_c1fe, 0xbc84_6e02, 0x3b3b_d74a];
    let f = f32::from_bits;
    let neg = 0.0f32 - v;
    // maxps(neg, v): the second operand when they are equal or unordered
    let abs_v = if neg > v { neg } else { v };
    let mut sign = if 1.0 < v { 1.0f32 } else { -1.0 };
    let comp = abs_v <= 1.0;
    if comp {
        sign = 0.0;
    }
    let inv = 1.0f32 / v;
    let x = if comp { v } else { inv };
    let x2 = x * x;
    let mut r = f(TC1[3]) * x2;
    r = (r + f(TC1[2])) * x2;
    r = (r + f(TC1[1])) * x2;
    r = (r + f(TC1[0])) * x2;
    r = (r + f(TC0[3])) * x2;
    r = (r + f(TC0[2])) * x2;
    r = (r + f(TC0[1])) * x2;
    r = (r + f(TC0[0])) * x2;
    r += 1.0;
    r *= x;
    let r1 = (f32::from_bits(0x3fc9_0fdb) * sign) - r;
    if sign == 0.0 {
        r
    } else {
        r1
    }
}

/// `DirectX::XMVectorATan2` 0x140107420, one lane.
fn xm_atan2(y: f32, x: f32) -> f32 {
    const PI: u32 = 0x4049_0fdb;
    const PI_OVER_TWO: u32 = 0x3fc9_0fdb;
    const PI_OVER_FOUR: u32 = 0x3f49_0fdb;
    const THREE_PI_OVER_FOUR: u32 = 0x4016_cbe4;
    let y_sign = y.to_bits() & 0x8000_0000;
    let x_is_pos = x.to_bits() & 0x8000_0000 == 0;
    let y_is_zero = y == 0.0;
    let x_is_zero = x == 0.0;
    let x_is_inf = x.abs() == f32::INFINITY;
    let y_is_inf = y.abs() == f32::INFINITY;
    let with_sign = |bits: u32| f32::from_bits(bits | y_sign);
    let special = if y_is_inf {
        Some(if x_is_inf {
            if x_is_pos {
                with_sign(PI_OVER_FOUR)
            } else {
                with_sign(THREE_PI_OVER_FOUR)
            }
        } else {
            with_sign(PI_OVER_TWO)
        })
    } else if y_is_zero {
        Some(if x_is_pos { f32::from_bits(y_sign) } else { with_sign(PI) })
    } else if x_is_zero {
        Some(with_sign(PI_OVER_TWO))
    } else {
        None
    };
    let v = y / x;
    let r0 = xm_atan(v);
    let r1 = if x_is_pos { -0.0f32 } else { with_sign(PI) };
    let r2 = r0 + r1;
    special.unwrap_or(r2)
}

/// `DirectX::XMVectorSin` 0x140107790, one lane.
fn xm_sin(v: f32) -> f32 {
    let f = f32::from_bits;
    let mut q = f(0x3e22_f983) * v;
    // XMVectorRound: the sum with 2^23 and back does the rounding
    if q.abs() <= 8_388_608.0 {
        let magic = f(0x4b00_0000 | (q.to_bits() & 0x8000_0000));
        q = (magic + q) - magic;
    }
    let mut x = v - (q * f(0x40c9_0fdb));
    let sgn = x.to_bits() & 0x8000_0000;
    let c = f(0x4049_0fdb | sgn);
    let abs_x = f(x.to_bits() & 0x7fff_ffff);
    let reflected = c - x;
    if !(abs_x <= f(0x3fc9_0fdb)) {
        x = reflected;
    }
    let x2 = x * x;
    let mut r = f(0xb2cd_365b) * x2;
    r = (r + f(0x3638_b88e)) * x2;
    r = (r + f(0xb950_0bf1)) * x2;
    r = (r + f(0x3c08_8886)) * x2;
    r = (r + f(0xbe2a_aaab)) * x2;
    r += 1.0;
    r * x
}

/// `DirectX::XMQuaternionSlerpV` 0x140107300. The result is not normalised again.
fn xm_quaternion_slerp(q0: &[f32; 4], q1: &[f32; 4], t: f32) -> [f32; 4] {
    let p: [f32; 4] = std::array::from_fn(|k| q0[k] * q1[k]);
    let mut cos = (p[0] + p[2]) + (p[1] + p[3]);
    let v0 = (-t) + 1.0;
    let v1 = t + 0.0;
    let sign = if cos < 0.0 { -1.0f32 } else { 1.0 };
    cos *= sign;
    let linear = !(cos < f32::from_bits(0x3f7f_ff58));
    let sin_omega = (1.0f32 - (cos * cos)).sqrt();
    let omega = xm_atan2(sin_omega, cos);
    let mut s0 = xm_sin(omega * v0) / sin_omega;
    let mut s1 = xm_sin(omega * v1) / sin_omega;
    if linear {
        s0 = v0;
        s1 = v1;
    }
    s1 *= sign;
    std::array::from_fn(|k| (s1 * q1[k]) + (s0 * q0[k]))
}

/// `DirectX::XMQuaternionRotationMatrix` 0x1400c4540: the rotation of a matrix whose rows are
/// taken as they are.
fn xm_quaternion_rotation_matrix(m: &Mat44f) -> [f32; 4] {
    let r = &m.m;
    let (r00, r11, r22) = (r[0][0], r[1][1], r[2][2]);
    let x2 = ((r00 + (-r11)) + (-r22)) + 1.0;
    let y2 = (((-r00) + r11) + (-r22)) + 1.0;
    let z2 = (((-r00) + (-r11)) + r22) + 1.0;
    let w2 = ((r00 + r11) + r22) + 1.0;
    let (xy, xz, yz) = (r[0][1] + r[1][0], r[0][2] + r[2][0], r[1][2] + r[2][1]);
    let (xw, yw, zw) = (-(r[2][1] - r[1][2]), r[2][0] - r[0][2], -(r[1][0] - r[0][1]));
    let t = if r22 <= 0.0 {
        if (r11 - r00) <= 0.0 {
            [x2, xy, xz, xw]
        } else {
            [xy, y2, yz, yw]
        }
    } else if (r11 + r00) <= 0.0 {
        [xz, yz, z2, zw]
    } else {
        [xw, yw, zw, w2]
    };
    let len = sqrtf(((t[0] * t[0]) + (t[2] * t[2])) + ((t[1] * t[1]) + (t[3] * t[3])));
    [t[0] / len, t[1] / len, t[2] / len, t[3] / len]
}

/// `DirectX::XMQuaternionNormalize`, as it is inlined.
fn xm_quaternion_normalize(q: &[f32; 4]) -> [f32; 4] {
    let len_sq = ((q[0] * q[0]) + (q[2] * q[2])) + ((q[1] * q[1]) + (q[3] * q[3]));
    let len = sqrtf(len_sq);
    let mut quat = [q[0] / len, q[1] / len, q[2] / len, q[3] / len];
    if len == 0.0 {
        quat = [0.0; 4];
    }
    if len_sq == f32::INFINITY {
        quat = [f32::from_bits(0x7fc0_0000); 4];
    }
    quat
}

/// `DirectX::XMMatrixRotationQuaternion` 0x140107260.
fn xm_matrix_rotation_quaternion(q: &[f32; 4]) -> Mat44f {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, yy, zz) = (x2 * x, y2 * y, z2 * z);
    Mat44f {
        m: [
            [(1.0 - yy) - zz, (z2 * w) + (y2 * x), (z2 * x) - (y2 * w), 0.0],
            [(y2 * x) - (z2 * w), (1.0 - xx) - zz, (x2 * w) + (z2 * y), 0.0],
            [(y2 * w) + (z2 * x), (z2 * y) - (x2 * w), (1.0 - xx) - yy, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

/// `mat44f lerp(const mat44f&, const mat44f&, float)` 0x140108600: between two matrices by
/// their rotations and their places. The scale is lost on the way.
pub fn mat44f_lerp(a: &Mat44f, b: &Mat44f, t: f32) -> Mat44f {
    if t == 0.0 || t.is_nan() {
        return *a;
    }
    if t == 1.0 {
        return *b;
    }
    let qa = xm_quaternion_normalize(&xm_quaternion_rotation_matrix(a));
    let qb = xm_quaternion_normalize(&xm_quaternion_rotation_matrix(b));
    let mut r = xm_matrix_rotation_quaternion(&xm_quaternion_slerp(&qa, &qb, t));
    for k in 0..3 {
        r.m[3][k] = ((b.m[3][k] - a.m[3][k]) * t) + a.m[3][k];
    }
    r.m[3][3] = 1.0;
    r
}
