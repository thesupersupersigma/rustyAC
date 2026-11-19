// SPDX-License-Identifier: BSD-3-Clause

//! ODE master 66615b8a joints in f64 as Custom Shaders Patch 0.3.2 (build 4305) compiled them,
//! beyond ball and fixed (`joint.rs`) and contact (`csp_contact.rs`).

use crate::common::Vector3;
use crate::joint::Info2;
use crate::odemath::safe_normalize3;
use crate::world::Body;

const A: usize = 4;

/// `dxJointDBall::getInfo2` (`joints/dball.cpp`, CSP +0x16a2fd0, vtable 0x1823829e8). CSP's
/// differences from the source: body 1's anchor sums as `(a2 R2 + a0 R0) + (a1 R1 + pos)`; the
/// short-q test is `!(|q| >= 1e-12)` (NaN takes it); body 0's point velocity uses `R a1`, not
/// `globalA1 - pos`; the distance in the rhs is `|q|` from before the velocity fallback.
#[allow(clippy::too_many_arguments)]
pub fn dball_get_info2(
    b0: &Body<f64>,
    b1: Option<&Body<f64>>,
    world_fps: f64,
    anchor1: &Vector3<f64>,
    anchor2: &Vector3<f64>,
    erp: f64,
    cfm: f64,
    target_distance: f64,
    info: &mut Info2<f64>,
) {
    let row = |r: &[f64], a: &[f64]| (a[1] * r[1] + a[0] * r[0]) + a[2] * r[2];
    let rel1 = [row(&b0.r[0..], anchor1), row(&b0.r[4..], anchor1), row(&b0.r[8..], anchor1)];
    let g1 = [rel1[0] + b0.pos[0], rel1[1] + b0.pos[1], rel1[2] + b0.pos[2]];
    let g2 = match b1 {
        Some(b1) => {
            let a = anchor2;
            let p = |k: usize| (a[2] * b1.r[4 * k + 2] + a[0] * b1.r[4 * k]) + (a[1] * b1.r[4 * k + 1] + b1.pos[k]);
            [p(0), p(1), p(2)]
        }
        None => [anchor2[0], anchor2[1], anchor2[2]],
    };
    let mut q = [g1[0] - g2[0], g1[1] - g2[1], g1[2] - g2[2], 0.0];
    let len = ((q[1] * q[1] + q[0] * q[0]) + q[2] * q[2]).sqrt();
    if !(len >= 1e-12) {
        let w = &b0.avel;
        let v1 = [(rel1[2] * w[1] - rel1[1] * w[2]) + b0.lvel[0], (rel1[0] * w[2] - rel1[2] * w[0]) + b0.lvel[1], (rel1[1] * w[0] - rel1[0] * w[1]) + b0.lvel[2]];
        let v2 = match b1 {
            Some(b1) => {
                let (p, w) = ([g2[0] - b1.pos[0], g2[1] - b1.pos[1], g2[2] - b1.pos[2]], &b1.avel);
                [(p[2] * w[1] - p[1] * w[2]) + b1.lvel[0], (p[0] * w[2] - p[2] * w[0]) + b1.lvel[1], (p[1] * w[0] - p[0] * w[1]) + b1.lvel[2]]
            }
            None => [0.0; 3],
        };
        q = [v1[0] - v2[0], v1[1] - v2[1], v1[2] - v2[2], 0.0];
        if !(((q[1] * q[1] + q[0] * q[0]) + q[2] * q[2]).sqrt() >= 1e-12) {
            q = [1.0, 0.0, 0.0, 0.0];
        }
    }
    if !safe_normalize3(&mut q) {
        q = [1.0, 0.0, 0.0, 0.0];
    }
    info.j1[..3].copy_from_slice(&q[..3]);
    let r = rel1;
    info.j1[A..A + 3].copy_from_slice(&[q[2] * r[1] - q[1] * r[2], q[0] * r[2] - q[2] * r[0], q[1] * r[0] - q[0] * r[1]]);
    if let Some(b1) = b1 {
        info.j2[..3].copy_from_slice(&[-q[0], -q[1], -q[2]]);
        let r = [row(&b1.r[0..], anchor2), row(&b1.r[4..], anchor2), row(&b1.r[8..], anchor2)];
        info.j2[A..A + 3].copy_from_slice(&[q[1] * r[2] - r[1] * q[2], q[2] * r[0] - q[0] * r[2], r[1] * q[0] - q[1] * r[0]]);
    }
    info.c[0] = world_fps * erp * (target_distance - len);
    info.cfm[0] = cfm;
}
