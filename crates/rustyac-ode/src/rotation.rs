//! Quaternion and rotation-matrix helpers: `ode/src/rotation.cpp`.

use crate::common::{Matrix3, Quaternion};
use rustyac_math::sqrtf;

/// `dRSetIdentity` @ 0x1403465f0.
pub fn r_set_identity(r: &mut Matrix3) {
    *r = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
}

/// `dQMultiply0` @ 0x140346040: `qa = qb * qc`.
pub fn q_multiply0(qb: &Quaternion, qc: &Quaternion) -> Quaternion {
    [
        qb[0] * qc[0] - qb[1] * qc[1] - qb[2] * qc[2] - qb[3] * qc[3],
        qb[0] * qc[1] + qb[1] * qc[0] + qb[2] * qc[3] - qb[3] * qc[2],
        qb[0] * qc[2] + qb[2] * qc[0] + qb[3] * qc[1] - qb[1] * qc[3],
        qb[0] * qc[3] + qb[3] * qc[0] + qb[1] * qc[2] - qb[2] * qc[1],
    ]
}

/// `dQMultiply1` @ 0x140346130: `qa = inverse(qb) * qc`.
pub fn q_multiply1(qb: &Quaternion, qc: &Quaternion) -> Quaternion {
    [
        qb[0] * qc[0] + qb[1] * qc[1] + qb[2] * qc[2] + qb[3] * qc[3],
        qb[0] * qc[1] - qb[1] * qc[0] - qb[2] * qc[3] + qb[3] * qc[2],
        qb[0] * qc[2] - qb[2] * qc[0] - qb[3] * qc[1] + qb[1] * qc[3],
        qb[0] * qc[3] - qb[3] * qc[0] - qb[1] * qc[2] + qb[2] * qc[1],
    ]
}

/// `dQMultiply2` @ 0x140346220: `qa = qb * inverse(qc)`.
pub fn q_multiply2(qb: &Quaternion, qc: &Quaternion) -> Quaternion {
    [
        qb[0] * qc[0] + qb[1] * qc[1] + qb[2] * qc[2] + qb[3] * qc[3],
        -qb[0] * qc[1] + qb[1] * qc[0] - qb[2] * qc[3] + qb[3] * qc[2],
        -qb[0] * qc[2] + qb[2] * qc[0] - qb[3] * qc[1] + qb[1] * qc[3],
        -qb[0] * qc[3] + qb[3] * qc[0] - qb[1] * qc[2] + qb[2] * qc[1],
    ]
}

/// `dQMultiply3` @ 0x140346310: `qa = inverse(qb) * inverse(qc)`.
pub fn q_multiply3(qb: &Quaternion, qc: &Quaternion) -> Quaternion {
    [
        qb[0] * qc[0] - qb[1] * qc[1] - qb[2] * qc[2] - qb[3] * qc[3],
        -qb[0] * qc[1] - qb[1] * qc[0] + qb[2] * qc[3] - qb[3] * qc[2],
        -qb[0] * qc[2] - qb[2] * qc[0] + qb[3] * qc[1] - qb[1] * qc[3],
        -qb[0] * qc[3] - qb[3] * qc[0] + qb[1] * qc[2] - qb[2] * qc[1],
    ]
}

/// `dRfromQ` @ 0x140346620 (`dQtoR` in the stepper): rotation matrix of a unit quaternion.
pub fn r_from_q(r: &mut Matrix3, q: &Quaternion) {
    let qq1 = 2.0 * q[1] * q[1];
    let qq2 = 2.0 * q[2] * q[2];
    let qq3 = 2.0 * q[3] * q[3];
    r[0] = 1.0 - qq2 - qq3;
    r[1] = 2.0 * (q[1] * q[2] - q[0] * q[3]);
    r[2] = 2.0 * (q[1] * q[3] + q[0] * q[2]);
    r[3] = 0.0;
    r[4] = 2.0 * (q[1] * q[2] + q[0] * q[3]);
    r[5] = 1.0 - qq1 - qq3;
    r[6] = 2.0 * (q[2] * q[3] - q[0] * q[1]);
    r[7] = 0.0;
    r[8] = 2.0 * (q[1] * q[3] - q[0] * q[2]);
    r[9] = 2.0 * (q[2] * q[3] + q[0] * q[1]);
    r[10] = 1.0 - qq1 - qq2;
    r[11] = 0.0;
}

/// `dQfromR` @ 0x140346400: quaternion of a rotation matrix.
pub fn q_from_r(r: &Matrix3) -> Quaternion {
    let m = |i: usize, j: usize| r[i * 4 + j];
    let mut q = [0f32; 4];
    let tr = m(0, 0) + m(1, 1) + m(2, 2);
    if tr >= 0.0 {
        let s = sqrtf(tr + 1.0);
        q[0] = 0.5 * s;
        let s = 0.5 * (1.0f32 / s);
        q[1] = (m(2, 1) - m(1, 2)) * s;
        q[2] = (m(0, 2) - m(2, 0)) * s;
        q[3] = (m(1, 0) - m(0, 1)) * s;
        return q;
    }
    // find the largest diagonal element and jump to the appropriate case
    let case = if m(1, 1) > m(0, 0) {
        if m(2, 2) > m(1, 1) {
            2
        } else {
            1
        }
    } else if m(2, 2) > m(0, 0) {
        2
    } else {
        0
    };
    match case {
        0 => {
            let s = sqrtf((m(0, 0) - (m(1, 1) + m(2, 2))) + 1.0);
            q[1] = 0.5 * s;
            let s = 0.5 * (1.0f32 / s);
            q[2] = (m(0, 1) + m(1, 0)) * s;
            q[3] = (m(2, 0) + m(0, 2)) * s;
            q[0] = (m(2, 1) - m(1, 2)) * s;
        }
        1 => {
            let s = sqrtf((m(1, 1) - (m(2, 2) + m(0, 0))) + 1.0);
            q[2] = 0.5 * s;
            let s = 0.5 * (1.0f32 / s);
            q[3] = (m(1, 2) + m(2, 1)) * s;
            q[1] = (m(0, 1) + m(1, 0)) * s;
            q[0] = (m(0, 2) - m(2, 0)) * s;
        }
        _ => {
            let s = sqrtf((m(2, 2) - (m(0, 0) + m(1, 1))) + 1.0);
            q[3] = 0.5 * s;
            let s = 0.5 * (1.0f32 / s);
            q[1] = (m(2, 0) + m(0, 2)) * s;
            q[2] = (m(1, 2) + m(2, 1)) * s;
            q[0] = (m(1, 0) - m(0, 1)) * s;
        }
    }
    q
}

/// `dDQfromW` @ 0x140345f70 (`dWtoDQ`): time derivative of `q` for angular velocity `w`.
pub fn dq_from_w(w: &[f32], q: &Quaternion) -> [f32; 4] {
    [
        0.5 * (-w[0] * q[1] - w[1] * q[2] - w[2] * q[3]),
        0.5 * (w[0] * q[0] + w[1] * q[3] - w[2] * q[2]),
        0.5 * (-w[0] * q[3] + w[1] * q[0] + w[2] * q[1]),
        0.5 * (w[0] * q[2] - w[1] * q[1] + w[2] * q[0]),
    ]
}
