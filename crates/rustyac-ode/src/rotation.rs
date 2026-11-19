// SPDX-License-Identifier: BSD-3-Clause

//! Quaternion and rotation-matrix helpers: `ode/src/rotation.cpp`.

use crate::common::{Matrix3, Quaternion, Real};

/// `dRSetIdentity` @ 0x1403465f0.
pub fn r_set_identity<R: Real>(r: &mut Matrix3<R>) {
    let (o, l) = (R::ZERO, R::ONE);
    *r = [l, o, o, o, o, l, o, o, o, o, l, o];
}

/// `dQMultiply0` @ 0x140346040 (CSP +0x168b680, same order): `qa = qb * qc`.
pub fn q_multiply0<R: Real>(qb: &Quaternion<R>, qc: &Quaternion<R>) -> Quaternion<R> {
    [
        qb[0] * qc[0] - qb[1] * qc[1] - qb[2] * qc[2] - qb[3] * qc[3],
        qb[0] * qc[1] + qb[1] * qc[0] + qb[2] * qc[3] - qb[3] * qc[2],
        qb[0] * qc[2] + qb[2] * qc[0] + qb[3] * qc[1] - qb[1] * qc[3],
        qb[0] * qc[3] + qb[3] * qc[0] + qb[1] * qc[2] - qb[2] * qc[1],
    ]
}

/// `dQMultiply1` @ 0x140346130 (CSP +0x168b7b0): `qa = inverse(qb) * qc`.
pub fn q_multiply1<R: Real>(qb: &Quaternion<R>, qc: &Quaternion<R>) -> Quaternion<R> {
    [
        if R::FAST {
            (qb[2] * qc[2] + qb[3] * qc[3]) + (qb[0] * qc[0] + qb[1] * qc[1])
        } else {
            qb[0] * qc[0] + qb[1] * qc[1] + qb[2] * qc[2] + qb[3] * qc[3]
        },
        qb[0] * qc[1] - qb[1] * qc[0] - qb[2] * qc[3] + qb[3] * qc[2],
        qb[0] * qc[2] - qb[2] * qc[0] - qb[3] * qc[1] + qb[1] * qc[3],
        qb[0] * qc[3] - qb[3] * qc[0] - qb[1] * qc[2] + qb[2] * qc[1],
    ]
}

/// `dQMultiply2` @ 0x140346220: `qa = qb * inverse(qc)`.
pub fn q_multiply2<R: Real>(qb: &Quaternion<R>, qc: &Quaternion<R>) -> Quaternion<R> {
    if R::FAST {
        // CSP's only copy, inlined in setFixedOrientation (+0x16c8b0f): row 0 regrouped, 1-3 ODE's
        let (b, c) = (qb, qc);
        return [
            (c[3] * b[3] + c[2] * b[2]) + (c[0] * b[0] + c[1] * b[1]),
            ((c[0] * b[1] - c[1] * b[0]) - c[3] * b[2]) + c[2] * b[3],
            ((c[0] * b[2] - c[2] * b[0]) - c[1] * b[3]) + c[3] * b[1],
            ((c[0] * b[3] - c[3] * b[0]) - c[2] * b[1]) + c[1] * b[2],
        ];
    }
    [
        qb[0] * qc[0] + qb[1] * qc[1] + qb[2] * qc[2] + qb[3] * qc[3],
        -qb[0] * qc[1] + qb[1] * qc[0] - qb[2] * qc[3] + qb[3] * qc[2],
        -qb[0] * qc[2] + qb[2] * qc[0] - qb[3] * qc[1] + qb[1] * qc[3],
        -qb[0] * qc[3] + qb[3] * qc[0] - qb[1] * qc[2] + qb[2] * qc[1],
    ]
}

/// `dQMultiply3` @ 0x140346310 (CSP +0x168b8a0, same order): `qa = inverse(qb) * inverse(qc)`.
///
/// The source says `-qb0*qc1 - qb1*qc0 + qb2*qc3 - qb3*qc2`; the compiled code groups the
/// two negative products first (`addss` @ 0x140346364) and subtracts their sum, which gives
/// the other sign on an exact zero.
pub fn q_multiply3<R: Real>(qb: &Quaternion<R>, qc: &Quaternion<R>) -> Quaternion<R> {
    [
        qb[0] * qc[0] - qb[1] * qc[1] - qb[2] * qc[2] - qb[3] * qc[3],
        (qb[2] * qc[3] - (qb[1] * qc[0] + qb[0] * qc[1])) - qb[3] * qc[2],
        (qb[3] * qc[1] - (qb[2] * qc[0] + qb[0] * qc[2])) - qb[1] * qc[3],
        (qb[1] * qc[2] - (qb[3] * qc[0] + qb[0] * qc[3])) - qb[2] * qc[1],
    ]
}

/// `dRfromQ` @ 0x140346620 (`dQtoR` in the stepper; CSP +0x168b990): rotation matrix of a
/// unit quaternion. CSP doubles each factor of three sums (`(q0+q0)*q2 + (q1+q1)*q3`).
pub fn r_from_q<R: Real>(r: &mut Matrix3<R>, q: &Quaternion<R>) {
    let two = R::TWO;
    let qq1 = two * q[1] * q[1];
    let qq2 = two * q[2] * q[2];
    let qq3 = two * q[3] * q[3];
    let d = |a: R| a + a;
    r[0] = R::ONE - qq2 - qq3;
    r[1] = two * (q[1] * q[2] - q[0] * q[3]);
    r[2] = if R::FAST { d(q[0]) * q[2] + d(q[1]) * q[3] } else { two * (q[1] * q[3] + q[0] * q[2]) };
    r[3] = R::ZERO;
    r[4] = if R::FAST { d(q[1]) * q[2] + d(q[0]) * q[3] } else { two * (q[1] * q[2] + q[0] * q[3]) };
    r[5] = R::ONE - qq1 - qq3;
    r[6] = two * (q[2] * q[3] - q[0] * q[1]);
    r[7] = R::ZERO;
    r[8] = two * (q[1] * q[3] - q[0] * q[2]);
    r[9] = if R::FAST { d(q[0]) * q[1] + d(q[2]) * q[3] } else { two * (q[2] * q[3] + q[0] * q[1]) };
    r[10] = R::ONE - qq1 - qq2;
    r[11] = R::ZERO;
}

/// `dQfromR` @ 0x140346400 (CSP +0x168bae0): quaternion of a rotation matrix. CSP's `0.5 / s`
/// has the same bits as `0.5 * (1 / s)` here: halving is exact and `s >= 2^-538`.
pub fn q_from_r<R: Real>(r: &Matrix3<R>) -> Quaternion<R> {
    let m = |i: usize, j: usize| r[i * 4 + j];
    let mut q = [R::ZERO; 4];
    let tr = m(0, 0) + m(1, 1) + m(2, 2);
    if tr >= R::ZERO {
        let s = R::sqrt(tr + R::ONE);
        q[0] = R::HALF * s;
        let s = R::HALF * (R::ONE / s);
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
            let s = R::sqrt((m(0, 0) - (m(1, 1) + m(2, 2))) + R::ONE);
            q[1] = R::HALF * s;
            let s = R::HALF * (R::ONE / s);
            q[2] = (m(0, 1) + m(1, 0)) * s;
            q[3] = (m(2, 0) + m(0, 2)) * s;
            q[0] = (m(2, 1) - m(1, 2)) * s;
        }
        1 => {
            let s = R::sqrt((m(1, 1) - (m(2, 2) + m(0, 0))) + R::ONE);
            q[2] = R::HALF * s;
            let s = R::HALF * (R::ONE / s);
            q[3] = (m(1, 2) + m(2, 1)) * s;
            q[1] = (m(0, 1) + m(1, 0)) * s;
            q[0] = (m(0, 2) - m(2, 0)) * s;
        }
        _ => {
            let s = R::sqrt((m(2, 2) - (m(0, 0) + m(1, 1))) + R::ONE);
            q[3] = R::HALF * s;
            let s = R::HALF * (R::ONE / s);
            q[1] = (m(2, 0) + m(0, 2)) * s;
            q[2] = (m(1, 2) + m(2, 1)) * s;
            q[0] = (m(1, 0) - m(0, 1)) * s;
        }
    }
    q
}

/// `dDQfromW` @ 0x140345f70 (`dWtoDQ`; CSP +0x168bca0): time derivative of `q` for angular
/// velocity `w`. CSP multiplies each product of the third row by one half before summing.
pub fn dq_from_w<R: Real>(w: &[R], q: &Quaternion<R>) -> [R; 4] {
    let h = R::HALF;
    [
        h * (-w[0] * q[1] - w[1] * q[2] - w[2] * q[3]),
        h * (w[0] * q[0] + w[1] * q[3] - w[2] * q[2]),
        if R::FAST { (h * (w[1] * q[0]) - h * (w[0] * q[3])) + h * (w[2] * q[1]) } else { h * (-w[0] * q[3] + w[1] * q[0] + w[2] * q[1]) },
        h * (w[0] * q[2] - w[1] * q[1] + w[2] * q[0]),
    ]
}
