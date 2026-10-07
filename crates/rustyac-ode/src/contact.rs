// SPDX-License-Identifier: BSD-3-Clause

//! The contact joint: `ode/src/joints/contact.cpp`, `include/ode/contact.h`.
//!
//! One contact point between two geoms becomes one joint for (in the game) two steps: a
//! normal row that only pushes (`0 <= lambda`), and two friction rows whose limits are the
//! friction coefficient times the normal force (`dContactApprox1`: the rows name the normal
//! row through `findex`).

use crate::common::Vector3;
use crate::geom::GeomRef;
use crate::joint::{Info1, Info2, JOINT_REVERSE};
use crate::odemath::plane_space;
use crate::world::Body;

/// `dContactMu2` / `dContactAxisDep`
pub const CONTACT_MU2: i32 = 0x001;
/// `dContactFDir1`
pub const CONTACT_FDIR1: i32 = 0x002;
/// `dContactBounce`
pub const CONTACT_BOUNCE: i32 = 0x004;
/// `dContactSoftERP`
pub const CONTACT_SOFT_ERP: i32 = 0x008;
/// `dContactSoftCFM`
pub const CONTACT_SOFT_CFM: i32 = 0x010;
/// `dContactMotion1`
pub const CONTACT_MOTION1: i32 = 0x020;
/// `dContactMotion2`
pub const CONTACT_MOTION2: i32 = 0x040;
/// `dContactMotionN`
pub const CONTACT_MOTION_N: i32 = 0x080;
/// `dContactSlip1`
pub const CONTACT_SLIP1: i32 = 0x100;
/// `dContactSlip2`
pub const CONTACT_SLIP2: i32 = 0x200;
/// `dContactRolling`
pub const CONTACT_ROLLING: i32 = 0x400;
/// `dContactApprox1_1`
pub const CONTACT_APPROX1_1: i32 = 0x1000;
/// `dContactApprox1_2`
pub const CONTACT_APPROX1_2: i32 = 0x2000;
/// `dContactApprox1_N`
pub const CONTACT_APPROX1_N: i32 = 0x4000;
/// `dContactApprox1`
pub const CONTACT_APPROX1: i32 = 0x7000;

/// `dSurfaceParameters` (0x3c bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceParameters {
    pub mode: i32,
    pub mu: f32,
    pub mu2: f32,
    pub rho: f32,
    pub rho2: f32,
    pub rho_n: f32,
    pub bounce: f32,
    pub bounce_vel: f32,
    pub soft_erp: f32,
    pub soft_cfm: f32,
    pub motion1: f32,
    pub motion2: f32,
    pub motion_n: f32,
    pub slip1: f32,
    pub slip2: f32,
}

/// `dContactGeom` (0x40 bytes): what the narrow phase found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactGeom {
    pub pos: [f32; 3],
    /// Unit vector; by ODE's convention it points into `g1`.
    pub normal: [f32; 3],
    pub depth: f32,
    pub g1: GeomRef,
    pub g2: GeomRef,
    /// A triangle index for a mesh, -1 otherwise.
    pub side1: i32,
    pub side2: i32,
}

/// `dContact` (0x90 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub surface: SurfaceParameters,
    pub geom: ContactGeom,
    pub fdir1: [f32; 3],
}

/// Row skip of the Jacobian blocks.
const S: usize = 8;
/// Offset of the angular part inside a Jacobian row.
const A: usize = 4;

/// `dxJointContact::getSureMaxInfo` @ 0x14034f490.
pub(crate) fn sure_max_m(contact: &Contact) -> u32 {
    if contact.surface.mode & CONTACT_ROLLING != 0 {
        6
    } else {
        3
    }
}

/// `x == dInfinity` as compiled (`ucomiss` + `jne`, no parity test): true for +inf and for NaN.
#[inline]
fn inf_or_nan(x: f32) -> bool {
    x == f32::INFINITY || x.is_nan()
}

/// `dxJointContact::getInfo1` @ 0x14034e810: makes sure the coefficients are not negative
/// (writing them back), then counts the rows and the unbounded rows.
pub(crate) fn get_info1(contact: &mut Contact, the_m: &mut i32) -> Info1 {
    let s = &mut contact.surface;
    let roll = s.mode & CONTACT_ROLLING != 0;
    let mut m: i32 = 1;
    let mut nub: i32 = 0;
    // comiss 0, mu / jbe skip: a NaN is left alone
    if 0.0 > s.mu {
        s.mu = 0.0;
    }
    // Anisotropic sliding and rolling and spinning friction
    if s.mode & CONTACT_MU2 != 0 {
        if 0.0 > s.mu2 {
            s.mu2 = 0.0;
        }
        if s.mu > 0.0 {
            m = 2;
        }
        if s.mu2 > 0.0 {
            m += 1;
        }
        if inf_or_nan(s.mu) {
            nub = 1;
        }
        if inf_or_nan(s.mu2) {
            nub += 1;
        }
        if roll {
            if 0.0 > s.rho {
                s.rho = 0.0;
            } else {
                m += 1;
            }
            if 0.0 > s.rho2 {
                s.rho2 = 0.0;
            } else {
                m += 1;
            }
            if 0.0 > s.rho_n {
                s.rho_n = 0.0;
            } else {
                m += 1;
            }
            if inf_or_nan(s.rho) {
                nub += 1;
            }
            if inf_or_nan(s.rho2) {
                nub += 1;
            }
            if inf_or_nan(s.rho_n) {
                nub += 1;
            }
        }
    } else {
        if s.mu > 0.0 {
            m = 3;
        }
        if inf_or_nan(s.mu) {
            nub = 2;
        }
        if roll {
            if 0.0 > s.rho {
                s.rho = 0.0;
            } else {
                m += 3;
            }
            if inf_or_nan(s.rho) {
                nub += 3;
            }
        }
    }
    *the_m = m;
    Info1 { m: m as u8, nub: nub as u8 }
}

#[inline]
fn cross(a: &[f32; 3], b: &[f32]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// One tangential (or the normal) direction into row `row`: `J1l = t`, `J1a = c1 x t`, and
/// the same negated for the second body.
#[inline]
fn set_direction(info: &mut Info2, row: usize, t: &[f32], c1: &[f32; 3], c2: &[f32; 3], has_b1: bool) {
    let o = row * S;
    info.j1[o] = t[0];
    info.j1[o + 1] = t[1];
    info.j1[o + 2] = t[2];
    let x = cross(c1, t);
    info.j1[o + A] = x[0];
    info.j1[o + A + 1] = x[1];
    info.j1[o + A + 2] = x[2];
    if has_b1 {
        info.j2[o] = -t[0];
        info.j2[o + 1] = -t[1];
        info.j2[o + 2] = -t[2];
        let x = cross(c2, t);
        info.j2[o + A] = -x[0];
        info.j2[o + A + 1] = -x[1];
        info.j2[o + A + 2] = -x[2];
    }
}

/// `dxJointContact::getInfo2` @ 0x14034e970.
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_info2(
    contact: &Contact,
    the_m: i32,
    flags: u32,
    b0: &Body,
    b1: Option<&Body>,
    world_fps: f32,
    world_erp: f32,
    contact_max_vel: f32,
    contact_min_depth: f32,
    info: &mut Info2,
) {
    let surface = &contact.surface;
    let geom = &contact.geom;
    let mode = surface.mode;
    let mut row_friction2 = 2usize;

    // get normal, with sign adjusted for body1/body2 polarity
    let normal: Vector3 = if flags & JOINT_REVERSE != 0 {
        [-geom.normal[0], -geom.normal[1], -geom.normal[2], 0.0]
    } else {
        [geom.normal[0], geom.normal[1], geom.normal[2], 0.0]
    };

    // c1,c2 = contact points with respect to body PORs
    let c1 = [geom.pos[0] - b0.pos[0], geom.pos[1] - b0.pos[1], geom.pos[2] - b0.pos[2]];
    let mut c2 = [0.0f32; 3];
    if let Some(b1) = b1 {
        c2 = [geom.pos[0] - b1.pos[0], geom.pos[1] - b1.pos[1], geom.pos[2] - b1.pos[2]];
    }

    // set jacobian for normal
    set_direction(info, 0, &normal, &c1, &c2, b1.is_some());

    // set right hand side and cfm value for normal
    let mut erp = world_erp;
    if mode & CONTACT_SOFT_ERP != 0 {
        erp = surface.soft_erp;
    }
    let k = erp * world_fps;
    let mut depth = geom.depth - contact_min_depth;
    // comiss depth, 0 / jae keep: a NaN becomes 0
    if !(depth >= 0.0) {
        depth = 0.0;
    }
    if mode & CONTACT_SOFT_CFM != 0 {
        info.cfm[0] = surface.soft_cfm;
    }
    let mut motion_n = 0.0f32;
    if mode & CONTACT_MOTION_N != 0 {
        motion_n = surface.motion_n;
    }
    info.c[0] = depth * k + motion_n;

    // note: this cap should not limit bounce velocity
    // comiss maxvel, c[0] / jae keep: a NaN pushout becomes maxvel
    if !(contact_max_vel >= info.c[0]) {
        info.c[0] = contact_max_vel;
    }

    // deal with bounce
    if mode & CONTACT_BOUNCE != 0 {
        // calculate outgoing velocity (-ve for incoming contact)
        let ang = (info.j1[A] * b0.avel[0] + info.j1[A + 1] * b0.avel[1]) + info.j1[A + 2] * b0.avel[2];
        let lin = (info.j1[0] * b0.lvel[0] + info.j1[1] * b0.lvel[1]) + info.j1[2] * b0.lvel[2];
        let mut outgoing = ang + lin;
        if let Some(b1) = b1 {
            let ang2 = (info.j2[A] * b1.avel[0] + info.j2[A + 1] * b1.avel[1]) + info.j2[A + 2] * b1.avel[2];
            let lin2 = (info.j2[0] * b1.lvel[0] + info.j2[1] * b1.lvel[1]) + info.j2[2] * b1.lvel[2];
            outgoing = outgoing + (ang2 + lin2);
        }
        outgoing = outgoing - motion_n;
        // only apply bounce if the outgoing velocity is greater than the threshold, and if
        // the resulting c[rowNormal] exceeds what we already have.
        let bv = surface.bounce_vel;
        if bv >= 0.0 && (-outgoing) > bv {
            let newc = motion_n - outgoing * surface.bounce;
            if newc > info.c[0] {
                info.c[0] = newc;
            }
        }
    }

    // set LCP limits for normal
    info.lo[0] = 0.0;
    info.hi[0] = f32::INFINITY;

    if the_m == 1 {
        // no friction, there is nothing else to do
        return;
    }

    // now do jacobian for tangential forces
    let mut t1: Vector3 = [0.0; 4];
    let mut t2: Vector3 = [0.0; 4];
    if mode & CONTACT_FDIR1 != 0 {
        // use fdir1 ?
        t1 = [contact.fdir1[0], contact.fdir1[1], contact.fdir1[2], 0.0];
        let n = [normal[0], normal[1], normal[2]];
        let x = cross(&n, &t1);
        t2 = [x[0], x[1], x[2], 0.0];
    } else {
        plane_space(&normal, &mut t1, &mut t2);
    }

    // first friction direction
    let mut second_row = 2usize;
    // comiss 0, mu / jae else: taken for mu > 0 and for a NaN
    if !(0.0 >= surface.mu) {
        set_direction(info, 1, &t1, &c1, &c2, b1.is_some());
        // set right hand side
        if mode & CONTACT_MOTION1 != 0 {
            info.c[1] = surface.motion1;
        }
        // set LCP bounds and friction index. this depends on the approximation mode
        info.lo[1] = -surface.mu;
        info.hi[1] = surface.mu;
        if mode & CONTACT_APPROX1_1 != 0 {
            info.findex[1] = 0;
        }
        // set slip (constraint force mixing)
        if mode & CONTACT_SLIP1 != 0 {
            info.cfm[1] = surface.slip1;
        }
    } else {
        // there was no friction for direction 1, so the second friction constraint has to
        // be on this line instead
        second_row = 1;
        row_friction2 = 1;
    }

    let mu2 = if mode & CONTACT_MU2 != 0 { surface.mu2 } else { surface.mu };
    let mut roll_row = row_friction2;

    // second friction direction
    if mu2 > 0.0 {
        set_direction(info, second_row, &t2, &c1, &c2, b1.is_some());
        // set right hand side
        if mode & CONTACT_MOTION2 != 0 {
            info.c[row_friction2] = surface.motion2;
        }
        // set LCP bounds and friction index. this depends on the approximation mode
        info.lo[row_friction2] = -mu2;
        info.hi[row_friction2] = mu2;
        if mode & CONTACT_APPROX1_2 != 0 {
            info.findex[row_friction2] = 0;
        }
        // set slip (constraint force mixing)
        if mode & CONTACT_SLIP2 != 0 {
            info.cfm[row_friction2] = surface.slip2;
        }
        roll_row = row_friction2 + 1;
    }

    // Handle rolling/spinning friction
    if mode & CONTACT_ROLLING == 0 {
        return;
    }
    let rho0 = surface.rho;
    let (rho1, rho2) = if mode & CONTACT_MU2 != 0 { (surface.rho2, surface.rho_n) } else { (rho0, rho0) };
    let axes: [(f32, [f32; 3], i32); 3] = [
        (rho0, [t1[0], t1[1], t1[2]], CONTACT_APPROX1_1),
        (rho1, [t2[0], t2[1], t2[2]], CONTACT_APPROX1_2),
        (rho2, [normal[0], normal[1], normal[2]], CONTACT_APPROX1_N),
    ];
    for (rho, ax, approx) in axes {
        if rho > 0.0 {
            // Set the angular axis
            let o = roll_row * S + A;
            info.j1[o] = ax[0];
            info.j1[o + 1] = ax[1];
            info.j1[o + 2] = ax[2];
            if b1.is_some() {
                info.j2[o] = -ax[0];
                info.j2[o + 1] = -ax[1];
                info.j2[o + 2] = -ax[2];
            }
            // Set the lcp limits
            info.lo[roll_row] = -rho;
            info.hi[roll_row] = rho;
            // Make limits proportional to normal force
            if mode & approx != 0 {
                info.findex[roll_row] = 0;
            }
            roll_row += 1;
        }
    }
}
