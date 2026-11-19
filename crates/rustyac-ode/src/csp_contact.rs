// SPDX-License-Identifier: BSD-3-Clause

//! ODE master 66615b8a `dxJointContact::getInfo1` / `getInfo2` (`joints/contact.cpp`) in f64 as
//! Custom Shaders Patch 0.3.2 (build 4305) compiled them (CSP +0x16c79d0 / +0x16c7b30). CSP's
//! differences from the source: `dInfinity` is 1e300; the normal's rhs caps `k * depth` before it
//! adds `motionN`; NaN friction coefficients count as negative (zeroed) in `getInfo1`; the
//! bounce's dot products with body 0 sum as `(z + x) + y`; `dPlaneSpace` writes `sqrt(a)` where the
//! source has `a * (1 / sqrt(a))`. Everything else keeps the source order.

use crate::common::Vector3;
use crate::csp_lcp::D_INFINITY;
use crate::joint::Info2;
use crate::world::Body;

pub const MU2: u32 = 0x001;
pub const FDIR1: u32 = 0x002;
pub const BOUNCE: u32 = 0x004;
pub const SOFT_ERP: u32 = 0x008;
pub const SOFT_CFM: u32 = 0x010;
pub const MOTION1: u32 = 0x020;
pub const MOTION2: u32 = 0x040;
pub const MOTION_N: u32 = 0x080;
pub const SLIP1: u32 = 0x100;
pub const SLIP2: u32 = 0x200;
pub const ROLLING: u32 = 0x400;
pub const APPROX1_1: u32 = 0x1000;
pub const APPROX1_2: u32 = 0x2000;
pub const APPROX1_N: u32 = 0x4000;

/// Row skip and angular offset of the port's Jacobian blocks (as `joint.rs`).
const S: usize = 8;
const A: usize = 4;

/// `dContact` (surface parameters, geometry, fdir1) in f64, as CSP's joint holds it at +0xa8.
#[derive(Clone, Copy, Debug, Default)]
pub struct Contact {
    pub mode: u32,
    pub mu: f64,
    pub mu2: f64,
    pub rho: f64,
    pub rho2: f64,
    pub rho_n: f64,
    pub bounce: f64,
    pub bounce_vel: f64,
    pub soft_erp: f64,
    pub soft_cfm: f64,
    pub motion1: f64,
    pub motion2: f64,
    pub motion_n: f64,
    pub slip1: f64,
    pub slip2: f64,
    pub pos: Vector3<f64>,
    pub normal: Vector3<f64>,
    pub depth: f64,
    pub fdir1: Vector3<f64>,
}

/// `getInfo1`: `(m, nub)`; zeroes negative (and NaN) coefficients as CSP does.
pub fn get_info1(c: &mut Contact) -> (u32, u32) {
    // `!(x >= 0)`: NaN is zeroed too
    fn clamp(x: &mut f64) -> bool {
        if *x >= 0.0 {
            true
        } else {
            *x = 0.0;
            false
        }
    }
    let (mut m, mut nub) = (1, 0);
    if c.mode & MU2 == 0 {
        if clamp(&mut c.mu) && c.mu > 0.0 {
            if c.mu == D_INFINITY {
                nub += 2;
            }
            m += 2;
        }
        if c.mode & ROLLING != 0 && clamp(&mut c.rho) {
            if c.rho == D_INFINITY {
                nub += 3;
            }
            m += 3;
        }
    } else {
        for (x, rolling) in [(&mut c.mu, false), (&mut c.mu2, false)].into_iter().chain([(&mut c.rho, true), (&mut c.rho2, true), (&mut c.rho_n, true)]) {
            if rolling && c.mode & ROLLING == 0 {
                break;
            }
            // friction rows need > 0, rolling rows >= 0
            if clamp(x) && (rolling || *x > 0.0) {
                if *x == D_INFINITY {
                    nub += 1;
                }
                m += 1;
            }
        }
    }
    (m, nub)
}

fn cross(a: &[f64], b: &[f64]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `getInfo2`. `the_m` is `getInfo1`'s `m`; `reverse` the joint's `dJOINT_REVERSE` flag;
/// `max_vel` / `min_depth` the world's `contactp`.
#[allow(clippy::too_many_arguments)]
pub fn get_info2(
    c: &Contact,
    the_m: u32,
    reverse: bool,
    b0: &Body<f64>,
    b1: Option<&Body<f64>>,
    world_fps: f64,
    world_erp: f64,
    max_vel: f64,
    min_depth: f64,
    info: &mut Info2<f64>,
) {
    let mode = c.mode;
    let erp = if mode & SOFT_ERP != 0 { c.soft_erp } else { world_erp };
    // maxsd / minsd: the second operand wins on NaN or equality
    let d = c.depth - min_depth;
    let depth = if d > 0.0 { d } else { 0.0 };
    let motion_n = if mode & MOTION_N != 0 { c.motion_n } else { 0.0 };
    let apply_bounce = mode & BOUNCE != 0 && !(0.0 > c.bounce_vel);
    let kd = erp * world_fps * depth;
    let mut rhs = (if kd < max_vel { kd } else { max_vel }) + motion_n;
    let n: [f64; 3] = if reverse { [-c.normal[0], -c.normal[1], -c.normal[2]] } else { [c.normal[0], c.normal[1], c.normal[2]] };

    let mut c2 = [0.0; 3];
    let mut outgoing1 = 0.0;
    if let Some(b1) = b1 {
        c2 = [c.pos[0] - b1.pos[0], c.pos[1] - b1.pos[1], c.pos[2] - b1.pos[2]];
        for k in 0..3 {
            info.j2[k] = -n[k];
        }
        let ja = cross(&n, &c2);
        info.j2[A..A + 3].copy_from_slice(&ja);
        if apply_bounce {
            outgoing1 = ((ja[1] * b1.avel[1] + ja[0] * b1.avel[0]) + ja[2] * b1.avel[2]) - ((n[1] * b1.lvel[1] + n[0] * b1.lvel[0]) + n[2] * b1.lvel[2]);
        }
    }
    let c1 = [c.pos[0] - b0.pos[0], c.pos[1] - b0.pos[1], c.pos[2] - b0.pos[2]];
    info.j1[..3].copy_from_slice(&n);
    let ja = cross(&c1, &n);
    info.j1[A..A + 3].copy_from_slice(&ja);
    if apply_bounce {
        let dl = (n[2] * b0.lvel[2] + n[0] * b0.lvel[0]) + n[1] * b0.lvel[1];
        let da = (ja[2] * b0.avel[2] + ja[0] * b0.avel[0]) + ja[1] * b0.avel[1];
        let negated_outgoing = motion_n - ((dl + da) + outgoing1);
        if negated_outgoing > c.bounce_vel {
            let newc = negated_outgoing * c.bounce + motion_n;
            if newc > rhs {
                rhs = newc;
            }
        }
    }
    info.c[0] = rhs;
    if mode & SOFT_CFM != 0 {
        info.cfm[0] = c.soft_cfm;
    }
    info.lo[0] = 0.0;
    info.hi[0] = D_INFINITY;
    if the_m as i32 <= 1 {
        return;
    }

    let (t1, t2) = if mode & FDIR1 != 0 {
        let t1 = [c.fdir1[0], c.fdir1[1], c.fdir1[2]];
        (t1, cross(&n, &t1))
    } else if n[2].abs() > std::f64::consts::FRAC_1_SQRT_2 {
        let a = n[1] * n[1] + n[2] * n[2];
        let s = a.sqrt();
        let k = 1.0 / s;
        let p = [0.0, -(k * n[2]), k * n[1]];
        (p, [s, -(n[0] * p[2]), n[0] * p[1]])
    } else {
        // NaN takes this branch (comisd + jbe)
        let a = n[1] * n[1] + n[0] * n[0];
        let s = a.sqrt();
        let k = 1.0 / s;
        let p = [-(k * n[1]), k * n[0], 0.0];
        (p, [-(n[2] * p[1]), n[2] * p[0], s])
    };

    let mut row = 1;
    let friction = |info: &mut Info2<f64>, row: usize, t: &[f64; 3]| {
        info.j1[row * S..row * S + 3].copy_from_slice(t);
        info.j1[row * S + A..row * S + A + 3].copy_from_slice(&cross(&c1, t));
        if b1.is_some() {
            for k in 0..3 {
                info.j2[row * S + k] = -t[k];
            }
            info.j2[row * S + A..row * S + A + 3].copy_from_slice(&cross(t, &c2));
        }
    };
    let mu = c.mu;
    if mu > 0.0 {
        friction(info, row, &t1);
        if mode & MOTION1 != 0 {
            info.c[row] = c.motion1;
        }
        if mode & SLIP1 != 0 {
            info.cfm[row] = c.slip1;
        }
        info.lo[row] = -mu;
        info.hi[row] = mu;
        if mode & APPROX1_1 != 0 {
            info.findex[row] = 0;
        }
        row += 1;
    }
    let mu2 = if mode & MU2 != 0 { c.mu2 } else { mu };
    if mu2 > 0.0 {
        friction(info, row, &t2);
        if mode & MOTION2 != 0 {
            info.c[row] = c.motion2;
        }
        if mode & SLIP2 != 0 {
            info.cfm[row] = c.slip2;
        }
        info.lo[row] = -mu2;
        info.hi[row] = mu2;
        if mode & APPROX1_2 != 0 {
            info.findex[row] = 0;
        }
        row += 1;
    }
    if mode & ROLLING != 0 {
        let rho = if mode & MU2 != 0 { [c.rho, c.rho2, c.rho_n] } else { [c.rho; 3] };
        let approx = [APPROX1_1, APPROX1_2, APPROX1_N];
        for (i, ax) in [t1, t2, n].iter().enumerate() {
            if rho[i] > 0.0 {
                info.j1[row * S + A..row * S + A + 3].copy_from_slice(ax);
                if b1.is_some() {
                    for k in 0..3 {
                        info.j2[row * S + A + k] = -ax[k];
                    }
                }
                info.lo[row] = -rho[i];
                info.hi[row] = rho[i];
                if mode & approx[i] != 0 {
                    info.findex[row] = 0;
                }
                row += 1;
            }
        }
    }
}
