//! Joints: `ode/src/joints/{joint,ball,dball,fixed,slider}.cpp` and the joint part of
//! `ode/src/ode.cpp`.
//!
//! The four joint types linked into `acs.exe` besides the contact joint:
//!
//! | type | rows | used by AC for |
//! |---|---|---|
//! | DBall (fixed distance between two points) | 1 | every suspension link |
//! | Ball | 3 | strut top |
//! | Slider | 5 | strut body to hub |
//! | Fixed | 6 | fuel tank to car body |
//!
//! A Jacobian block is `rows` rows of 8 values: linear part in columns 0..3, angular part in
//! columns 4..7 (the fourth of each is padding), one block per attached body.

use crate::common::{Quaternion, Vector3};
use crate::odemath::{
    cross3, length3, multiply0_331, multiply1_331, plane_space, safe_normalize3, set_cross_matrix_minus,
    set_cross_matrix_plus,
};
use crate::rotation::{q_multiply1, q_multiply2, q_multiply3};
use crate::world::{get_point_vel, get_pos_rel_point, get_rel_point_pos, Body, BodyId, JointId, NodeRef, World};

/// `dJOINT_INGROUP`
pub const JOINT_INGROUP: u32 = 1;
/// `dJOINT_REVERSE`: the joint was attached with a null first body, so the bodies are swapped.
pub const JOINT_REVERSE: u32 = 2;
/// `dJOINT_DISABLED`
pub const JOINT_DISABLED: u32 = 8;

/// `dParamCFM`
pub const PARAM_CFM: i32 = 8;
/// `dParamERP`
pub const PARAM_ERP: i32 = 0xd;
/// `dParamLoStop`
pub const PARAM_LO_STOP: i32 = 0;
/// `dParamHiStop`
pub const PARAM_HI_STOP: i32 = 1;
/// `dParamVel`
pub const PARAM_VEL: i32 = 2;
/// `dParamFMax`
pub const PARAM_FMAX: i32 = 5;
/// `dParamFudgeFactor`
pub const PARAM_FUDGE_FACTOR: i32 = 6;
/// `dParamBounce`
pub const PARAM_BOUNCE: i32 = 7;
/// `dParamStopERP`
pub const PARAM_STOP_ERP: i32 = 9;
/// `dParamStopCFM`
pub const PARAM_STOP_CFM: i32 = 10;

/// `dxJointNode`: one end of a joint. `body` is the body at this end; the node itself is
/// linked into the joint list of the **other** body.
#[derive(Clone, Copy, Debug, Default)]
pub struct JointNode {
    pub body: Option<BodyId>,
    pub next: Option<NodeRef>,
}

/// `dJointFeedback`: the force and torque a joint applied to its two bodies in the last
/// step (world axes, torque about each body's centre).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JointFeedback {
    pub f1: Vector3,
    pub t1: Vector3,
    pub f2: Vector3,
    pub t2: Vector3,
}

/// `dxJointLimitMotor`: the limit / motor block of a slider. AC never sets a limit or a
/// motor, so it only holds defaults; a powered or limited slider is stage 2 (bounded rows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LimitMotor {
    pub vel: f32,
    pub fmax: f32,
    pub lostop: f32,
    pub histop: f32,
    pub fudge_factor: f32,
    pub normal_cfm: f32,
    pub stop_erp: f32,
    pub stop_cfm: f32,
    pub bounce: f32,
    pub limit: i32,
    pub limit_err: f32,
}

impl LimitMotor {
    /// `dxJointLimitMotor::init` @ 0x14034dd40.
    fn new(world: &World) -> LimitMotor {
        LimitMotor {
            vel: 0.0,
            fmax: 0.0,
            lostop: f32::NEG_INFINITY,
            histop: f32::INFINITY,
            fudge_factor: 1.0,
            normal_cfm: world.global_cfm,
            stop_erp: world.global_erp,
            stop_cfm: world.global_cfm,
            bounce: 0.0,
            limit: 0,
            limit_err: 0.0,
        }
    }

    /// `dxJointLimitMotor::set` @ 0x14034ddb0. Parameters 3, 4 and everything above 10
    /// (`PARAM_ERP` = 13 included) are ignored.
    fn set(&mut self, num: i32, value: f32) {
        match num {
            PARAM_LO_STOP => self.lostop = value,
            PARAM_HI_STOP => self.histop = value,
            PARAM_VEL => self.vel = value,
            PARAM_FMAX => {
                if value >= 0.0 {
                    self.fmax = value
                }
            }
            PARAM_FUDGE_FACTOR => {
                if value >= 0.0 && value <= 1.0 {
                    self.fudge_factor = value
                }
            }
            PARAM_BOUNCE => self.bounce = value,
            PARAM_CFM => self.normal_cfm = value,
            PARAM_STOP_ERP => self.stop_erp = value,
            PARAM_STOP_CFM => self.stop_cfm = value,
            _ => {}
        }
    }
}

/// The type-specific part of a joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointKind {
    /// `dxJointBall` (type 1): anchors in the frames of body 1 and body 2.
    Ball { anchor1: Vector3, anchor2: Vector3, erp: f32, cfm: f32 },
    /// `dxJointSlider` (type 3): axis in body 1's frame, initial relative rotation and offset.
    Slider { axis1: Vector3, qrel: Quaternion, offset: Vector3, limot: LimitMotor },
    /// `dxJointFixed` (type 7).
    Fixed { qrel: Quaternion, offset: Vector3, erp: f32, cfm: f32 },
    /// `dxJointDBall` (type 15): two anchors kept `target_distance` apart.
    DBall { anchor1: Vector3, anchor2: Vector3, erp: f32, cfm: f32, target_distance: f32 },
}

/// `dxJoint`.
#[derive(Clone, Debug)]
pub struct Joint {
    /// Next joint in the world's list.
    pub next: Option<JointId>,
    /// After a step: block index of the joint in the solver's row order (-1: not used).
    pub tag: i32,
    pub flags: u32,
    pub node: [JointNode; 2],
    /// `Some` when the caller asked for the constraint forces (`dJointSetFeedback`).
    pub feedback: Option<JointFeedback>,
    pub kind: JointKind,
}

/// `dxJoint::Info1`: number of constraint rows and how many of them are unbounded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info1 {
    pub m: u8,
    pub nub: u8,
}

/// `dxJoint::Info2Descr` with slices in place of pointers. `j1` and `j2` are the Jacobian
/// blocks of body 1 and body 2 (`rows * 8` values each, zeroed by the caller).
pub struct Info2<'a> {
    pub j1: &'a mut [f32],
    pub j2: &'a mut [f32],
    pub c: &'a mut [f32],
    pub cfm: &'a mut [f32],
}

/// Row skip of the Jacobian blocks (`Info2Descr::rowskip`).
const S: usize = 8;
/// Offset of the angular part inside a Jacobian row.
const A: usize = 4;

impl Joint {
    /// ODE's joint type number (`dJointType`).
    pub fn type_number(&self) -> u32 {
        match self.kind {
            JointKind::Ball { .. } => 1,
            JointKind::Slider { .. } => 3,
            JointKind::Fixed { .. } => 7,
            JointKind::DBall { .. } => 15,
        }
    }

    /// `getSureMaxInfo`: the largest number of rows the joint can ever ask for.
    pub fn sure_max_m(&self) -> u32 {
        match self.kind {
            JointKind::Ball { .. } => 3,
            JointKind::Slider { .. } => 6,
            JointKind::Fixed { .. } => 6,
            JointKind::DBall { .. } => 1,
        }
    }

    /// `dxJoint::isEnabled` @ 0x14034dd80.
    pub(crate) fn is_enabled(&self, bodies: &[Body]) -> bool {
        self.flags & JOINT_DISABLED == 0
            && (self.node[0].body.is_some_and(|b| bodies[b.0 as usize].inv_mass > 0.0)
                || self.node[1].body.is_some_and(|b| bodies[b.0 as usize].inv_mass > 0.0))
    }

    /// `getInfo1` (DBall @ 0x140342070, Ball @ 0x140340cf0, Slider @ 0x140341110, Fixed @
    /// 0x140341b90).
    pub(crate) fn get_info1(&mut self) -> Info1 {
        match &mut self.kind {
            JointKind::Ball { .. } => Info1 { m: 3, nub: 3 },
            JointKind::Fixed { .. } => Info1 { m: 6, nub: 6 },
            JointKind::DBall { .. } => Info1 { m: 1, nub: 1 },
            JointKind::Slider { limot, .. } => {
                let mut info = Info1 { m: 5, nub: 5 };
                // `comiss 0, fmax` + `setb`: a NaN counts as powered
                if !(0.0 >= limot.fmax) {
                    info.m = 6; // powered slider needs an extra constraint row
                }
                limot.limit = 0;
                if (limot.lostop > f32::NEG_INFINITY || limot.histop < f32::INFINITY) && !(limot.lostop > limot.histop)
                {
                    // a slider with stops: bounded rows, not part of stage 1
                    unimplemented!("slider joint with limit stops (bounded constraint rows are stage 2)");
                }
                info
            }
        }
    }

    /// `getInfo2`: fills the Jacobian, the right-hand side `c` (position error correction,
    /// before the division by the step size) and the row softness `cfm`.
    pub(crate) fn get_info2(&self, bodies: &[Body], world_fps: f32, world_erp: f32, info: &mut Info2) {
        let b0 = &bodies[self.node[0].body.expect("an attached joint has a first body").0 as usize];
        let b1 = self.node[1].body.map(|b| &bodies[b.0 as usize]);
        match &self.kind {
            JointKind::Ball { anchor1, anchor2, erp, cfm } => {
                // dxJointBall::getInfo2 @ 0x140340d00
                info.cfm[0] = *cfm;
                info.cfm[1] = *cfm;
                info.cfm[2] = *cfm;
                set_ball(b0, b1, world_fps, *erp, info, anchor1, anchor2);
            }
            JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                dball_get_info2(b0, b1, world_fps, *erp, *cfm, *target_distance, anchor1, anchor2, info);
            }
            JointKind::Fixed { qrel, offset, erp, cfm } => {
                // dxJointFixed::getInfo2 @ 0x140341ba0
                // Three rows for orientation
                set_fixed_orientation(b0, b1, world_fps, world_erp, info, qrel, 3);
                // Three rows for position. set jacobian
                info.j1[0] = 1.0;
                info.j1[S + 1] = 1.0;
                info.j1[2 * S + 2] = 1.0;
                info.cfm[0] = *cfm;
                info.cfm[1] = *cfm;
                info.cfm[2] = *cfm;
                let ofs = multiply0_331(&b0.r, offset);
                if b1.is_some() {
                    set_cross_matrix_plus(&mut info.j1[A..], &ofs, S);
                    info.j2[0] = -1.0;
                    info.j2[S + 1] = -1.0;
                    info.j2[2 * S + 2] = -1.0;
                }
                // set right hand side for the first three rows (linear)
                let k = world_fps * *erp;
                if let Some(b1) = b1 {
                    for j in 0..3 {
                        info.c[j] = k * (b1.pos[j] - b0.pos[j] + ofs[j]);
                    }
                } else {
                    for j in 0..3 {
                        info.c[j] = k * (offset[j] - b0.pos[j]);
                    }
                }
            }
            JointKind::Slider { axis1, qrel, offset, limot } => {
                slider_get_info2(self.flags, b0, b1, world_fps, world_erp, axis1, qrel, offset, limot, info);
            }
        }
    }
}

/// `setBall` @ 0x14034e230: the three position rows of a ball joint.
fn set_ball(
    b0: &Body,
    b1: Option<&Body>,
    fps: f32,
    erp: f32,
    info: &mut Info2,
    anchor1: &Vector3,
    anchor2: &Vector3,
) {
    // set jacobian
    info.j1[0] = 1.0;
    info.j1[S + 1] = 1.0;
    info.j1[2 * S + 2] = 1.0;
    let a1 = multiply0_331(&b0.r, anchor1);
    set_cross_matrix_minus(&mut info.j1[A..], &a1, S);
    let mut a2 = [0.0f32; 3];
    if let Some(b1) = b1 {
        info.j2[0] = -1.0;
        info.j2[S + 1] = -1.0;
        info.j2[2 * S + 2] = -1.0;
        a2 = multiply0_331(&b1.r, anchor2);
        set_cross_matrix_plus(&mut info.j2[A..], &a2, S);
    }
    // set right hand side
    let k = fps * erp;
    if let Some(b1) = b1 {
        for j in 0..3 {
            info.c[j] = k * (a2[j] + b1.pos[j] - a1[j] - b0.pos[j]);
        }
    } else {
        for j in 0..3 {
            info.c[j] = k * (anchor2[j] - a1[j] - b0.pos[j]);
        }
    }
}

/// `setFixedOrientation` @ 0x14034e5a0: three rows that keep the relative orientation of the
/// two bodies at `qrel`, starting at row `start_row`.
fn set_fixed_orientation(
    b0: &Body,
    b1: Option<&Body>,
    fps: f32,
    erp: f32,
    info: &mut Info2,
    qrel: &Quaternion,
    start_row: usize,
) {
    let start_index = start_row * S;
    // 3 rows to make body rotations equal
    info.j1[A + start_index] = 1.0;
    info.j1[A + start_index + S + 1] = 1.0;
    info.j1[A + start_index + S * 2 + 2] = 1.0;
    if b1.is_some() {
        info.j2[A + start_index] = -1.0;
        info.j2[A + start_index + S + 1] = -1.0;
        info.j2[A + start_index + S * 2 + 2] = -1.0;
    }
    // compute the right hand side. the first three elements will result in
    // relative angular velocity of the two bodies - this is set to bring them
    // back into alignment. the correcting angular velocity is
    //   |angular_velocity| = angle/time = erp*theta / stepsize
    //                      = (erp*fps) * theta
    //    angular_velocity  = |angular_velocity| * u
    //                      = (erp*fps) * theta * u
    // where rotation along unit length axis u by theta brings body 2's frame
    // to qrel with respect to body 1's frame. using a small angle approximation
    // for sin(), this gives
    //    angular_velocity  = (erp*fps) * 2 * v
    // where the quaternion of the relative rotation between the two bodies is
    //    q = [cos(theta/2) sin(theta/2)*u] = [s v]
    //
    // get qerr = relative rotation (rotation error) between two bodies
    let mut qerr = if let Some(b1) = b1 {
        let qq = q_multiply1(&b0.q, &b1.q);
        q_multiply2(&qq, qrel)
    } else {
        q_multiply3(&b0.q, qrel)
    };
    // `comiss qerr0, 0` + `jae`: negative or NaN (a -0.0 is left alone)
    if !(qerr[0] >= 0.0) {
        qerr[1] = -qerr[1]; // adjust sign of qerr to make theta small
        qerr[2] = -qerr[2];
        qerr[3] = -qerr[3];
    }
    let e = multiply0_331(&b0.r, &qerr[1..]);
    let k = fps * erp;
    info.c[start_row] = 2.0 * k * e[0];
    info.c[start_row + 1] = 2.0 * k * e[1];
    info.c[start_row + 2] = 2.0 * k * e[2];
}

/// `dxJointDBall::getInfo2` @ 0x140342080.
#[allow(clippy::too_many_arguments)]
fn dball_get_info2(
    b0: &Body,
    b1: Option<&Body>,
    world_fps: f32,
    erp: f32,
    cfm: f32,
    target_distance: f32,
    anchor1: &Vector3,
    anchor2: &Vector3,
    info: &mut Info2,
) {
    info.cfm[0] = cfm;

    let global_a1 = get_rel_point_pos(b0, anchor1);
    let global_a2 = match b1 {
        Some(b1) => get_rel_point_pos(b1, anchor2),
        None => [anchor2[0], anchor2[1], anchor2[2]],
    };

    let mut q = [global_a1[0] - global_a2[0], global_a1[1] - global_a2[1], global_a1[2] - global_a2[2]];

    const MIN_LENGTH: f32 = 1e-7;

    // both length tests are `comiss` + `jae`: a NaN length takes the fallback too
    if !(length3(&q) >= MIN_LENGTH) {
        // too small, let's choose an arbitrary direction
        // heuristic: difference in velocities at anchors
        let v1 = get_point_vel(b0, &global_a1);
        let v2 = match b1 {
            Some(b1) => get_point_vel(b1, &global_a2),
            None => [0.0; 3],
        };
        q = [v1[0] - v2[0], v1[1] - v2[1], v1[2] - v2[2]];

        if !(length3(&q) >= MIN_LENGTH) {
            // this direction is as good as any
            q = [1.0, 0.0, 0.0];
        }
    }
    safe_normalize3(&mut q);

    info.j1[0] = q[0];
    info.j1[1] = q[1];
    info.j1[2] = q[2];

    let rel_a1 = multiply0_331(&b0.r, anchor1);

    let mut a1m = [0.0f32; 12];
    set_cross_matrix_minus(&mut a1m, &rel_a1, 4);

    let j1a = multiply1_331(&a1m, &q);
    info.j1[A] = j1a[0];
    info.j1[A + 1] = j1a[1];
    info.j1[A + 2] = j1a[2];

    if let Some(b1) = b1 {
        info.j2[0] = -q[0];
        info.j2[1] = -q[1];
        info.j2[2] = -q[2];

        let rel_a2 = multiply0_331(&b1.r, anchor2);
        let mut a2m = [0.0f32; 12];
        set_cross_matrix_plus(&mut a2m, &rel_a2, 4);
        let j2a = multiply1_331(&a2m, &q);
        info.j2[A] = j2a[0];
        info.j2[A + 1] = j2a[1];
        info.j2[A + 2] = j2a[2];
    }

    let k = world_fps * erp;
    let d = [global_a1[0] - global_a2[0], global_a1[1] - global_a2[1], global_a1[2] - global_a2[2]];
    info.c[0] = k * (target_distance - length3(&d));
}

/// `dxJointSlider::getInfo2` @ 0x1403411c0.
#[allow(clippy::too_many_arguments)]
fn slider_get_info2(
    flags: u32,
    b0: &Body,
    b1: Option<&Body>,
    world_fps: f32,
    world_erp: f32,
    axis1: &Vector3,
    qrel: &Quaternion,
    offset: &Vector3,
    limot: &LimitMotor,
    info: &mut Info2,
) {
    let s3 = 3 * S;
    let s4 = 4 * S;

    // pull out pos and R for both bodies. also get the `connection'
    // vector pos2-pos1.
    let pos1 = &b0.pos;
    let r1 = &b0.r;
    let mut c = [0.0f32; 3];
    if let Some(b1) = b1 {
        for i in 0..3 {
            c[i] = b1.pos[i] - pos1[i];
        }
    }

    // 3 rows to make body rotations equal
    set_fixed_orientation(b0, b1, world_fps, world_erp, info, qrel, 0);

    // remaining two rows. we want: vel2 = vel1 + w1 x c ... but this would
    // result in three equations, so we project along the planespace vectors
    // so that sliding along the slider axis is disregarded. for symmetry we
    // also substitute (w1+w2)/2 for w1, as w1 is supposed to equal w2.
    let ax1v = multiply0_331(r1, axis1);
    let mut ax1 = [ax1v[0], ax1v[1], ax1v[2], 0.0];
    let mut p = [0.0f32; 4];
    let mut q = [0.0f32; 4];
    plane_space(&ax1, &mut p, &mut q);
    if b1.is_some() {
        let mut tmp = cross3(&c, &p);
        tmp[0] *= 0.5;
        tmp[1] *= 0.5;
        tmp[2] *= 0.5;
        for i in 0..3 {
            info.j1[A + s3 + i] = tmp[i];
        }
        for i in 0..3 {
            info.j2[A + s3 + i] = tmp[i];
        }
        let mut tmp = cross3(&c, &q);
        tmp[0] *= 0.5;
        tmp[1] *= 0.5;
        tmp[2] *= 0.5;
        for i in 0..3 {
            info.j1[A + s4 + i] = tmp[i];
        }
        for i in 0..3 {
            info.j2[A + s4 + i] = tmp[i];
        }
        for i in 0..3 {
            info.j2[s3 + i] = -p[i];
        }
        for i in 0..3 {
            info.j2[s4 + i] = -q[i];
        }
    }
    for i in 0..3 {
        info.j1[s3 + i] = p[i];
    }
    for i in 0..3 {
        info.j1[s4 + i] = q[i];
    }

    // compute last two elements of right hand side. we want to align the offset
    // point (in body 2's frame) with the center of body 1.
    let k = world_fps * world_erp;
    if let Some(b1) = b1 {
        let ofs = multiply0_331(&b1.r, offset); // offset point in global coordinates
        for i in 0..3 {
            c[i] += ofs[i];
        }
        info.c[3] = k * crate::odemath::dot3(&p, &c);
        info.c[4] = k * crate::odemath::dot3(&q, &c);
    } else {
        let mut ofs = [0.0f32; 3]; // offset point in global coordinates
        for i in 0..3 {
            ofs[i] = offset[i] - pos1[i];
        }
        info.c[3] = k * crate::odemath::dot3(&p, &ofs);
        info.c[4] = k * crate::odemath::dot3(&q, &ofs);

        if flags & JOINT_REVERSE != 0 {
            for v in ax1.iter_mut().take(3) {
                *v = -*v;
            }
        }
    }

    // if the slider is powered, or has joint limits, add in the extra row
    // (dxJointLimitMotor::addLimot @ 0x14034d500 returns at once when neither is the case)
    debug_assert!(0.0 >= limot.fmax && limot.limit == 0, "powered or limited slider: stage 2");
    let _ = ax1;
}

impl World {
    /// `createJoint<T>` (0x14033e3f0 …) + `dxJoint::dxJoint` @ 0x14034d3f0.
    fn joint_create(&mut self, kind: JointKind) -> JointId {
        let id = JointId(self.joints.len() as u32);
        self.joints.push(Joint {
            next: self.first_joint,
            tag: 0,
            flags: 0,
            node: [JointNode::default(); 2],
            feedback: None,
            kind,
        });
        self.first_joint = Some(id);
        self.nj += 1;
        id
    }

    /// `dJointCreateBall` @ 0x14033fed0.
    pub fn joint_create_ball(&mut self) -> JointId {
        let kind =
            JointKind::Ball { anchor1: [0.0; 4], anchor2: [0.0; 4], erp: self.global_erp, cfm: self.global_cfm };
        self.joint_create(kind)
    }

    /// `dJointCreateDBall` @ 0x14033ff60.
    pub fn joint_create_dball(&mut self) -> JointId {
        let kind = JointKind::DBall {
            anchor1: [0.0; 4],
            anchor2: [0.0; 4],
            erp: self.global_erp,
            cfm: self.global_cfm,
            target_distance: 0.0,
        };
        self.joint_create(kind)
    }

    /// `dJointCreateFixed` @ 0x14033ff70.
    pub fn joint_create_fixed(&mut self) -> JointId {
        let kind = JointKind::Fixed { qrel: [0.0; 4], offset: [0.0; 4], erp: self.global_erp, cfm: self.global_cfm };
        self.joint_create(kind)
    }

    /// `dJointCreateSlider` @ 0x14033ff80.
    pub fn joint_create_slider(&mut self) -> JointId {
        let kind = JointKind::Slider {
            axis1: [1.0, 0.0, 0.0, 0.0],
            qrel: [0.0; 4],
            offset: [0.0; 4],
            limot: LimitMotor::new(self),
        };
        self.joint_create(kind)
    }

    fn node(&self, r: NodeRef) -> &JointNode {
        &self.joints[r.joint.0 as usize].node[r.node as usize]
    }

    /// `removeJointReferencesFromAttachedBodies`: unlinks the joint's two nodes from the
    /// joint lists of its bodies.
    pub(crate) fn remove_joint_references_from_attached_bodies(&mut self, j: JointId) {
        for i in 0..2 {
            let body = self.joints[j.0 as usize].node[i].body;
            if let Some(body) = body {
                // the node that sits in this body's list is the joint's other node
                let mut previous: Option<NodeRef> = None;
                let mut current = self.bodies[body.0 as usize].first_joint;
                while let Some(n) = current {
                    let next = self.node(n).next;
                    if n.joint == j {
                        match previous {
                            Some(p) => self.joints[p.joint.0 as usize].node[p.node as usize].next = next,
                            None => self.bodies[body.0 as usize].first_joint = next,
                        }
                        break;
                    }
                    previous = current;
                    current = next;
                }
            }
        }
        let joint = &mut self.joints[j.0 as usize];
        joint.node[0].body = None;
        joint.node[0].next = None;
        joint.node[1].body = None;
        joint.node[1].next = None;
    }

    /// `dJointAttach` @ 0x14033fe20. `None` is "the world"; a `None` first body swaps the two
    /// and marks the joint reversed. The joint's node is pushed at the head of each body's
    /// joint list, then the joint recomputes its relative values (`setRelativeValues`).
    pub fn joint_attach(&mut self, j: JointId, body1: Option<BodyId>, body2: Option<BodyId>) {
        if self.joints[j.0 as usize].node[0].body.is_some() || self.joints[j.0 as usize].node[1].body.is_some() {
            self.remove_joint_references_from_attached_bodies(j);
        }
        let (body1, body2) = {
            let joint = &mut self.joints[j.0 as usize];
            if body1.is_none() {
                joint.flags |= JOINT_REVERSE;
                (body2, None)
            } else {
                joint.flags &= !JOINT_REVERSE;
                (body1, body2)
            }
        };
        self.joints[j.0 as usize].node[0].body = body1;
        self.joints[j.0 as usize].node[1].body = body2;
        if let Some(b1) = body1 {
            self.joints[j.0 as usize].node[1].next = self.bodies[b1.0 as usize].first_joint;
            self.bodies[b1.0 as usize].first_joint = Some(NodeRef { joint: j, node: 1 });
        } else {
            self.joints[j.0 as usize].node[1].next = None;
        }
        if let Some(b2) = body2 {
            self.joints[j.0 as usize].node[0].next = self.bodies[b2.0 as usize].first_joint;
            self.bodies[b2.0 as usize].first_joint = Some(NodeRef { joint: j, node: 0 });
        } else {
            self.joints[j.0 as usize].node[0].next = None;
        }
        if body1.is_some() || body2.is_some() {
            self.joint_set_relative_values(j);
        }
    }

    /// The two bodies of a joint as stored (`node[0].body`, `node[1].body`).
    fn joint_bodies(&self, j: JointId) -> (Option<&Body>, Option<&Body>) {
        let joint = &self.joints[j.0 as usize];
        (
            joint.node[0].body.map(|b| &self.bodies[b.0 as usize]),
            joint.node[1].body.map(|b| &self.bodies[b.0 as usize]),
        )
    }

    /// `dxJoint::setRelativeValues` of each joint class.
    fn joint_set_relative_values(&mut self, j: JointId) {
        match self.joints[j.0 as usize].kind {
            JointKind::DBall { .. } => self.dball_update_target_distance(j),
            JointKind::Ball { anchor1, anchor2, .. } => {
                // dxJointBall::setRelativeValues @ 0x140340d60: dJointGetBallAnchor, then setAnchors
                let flags = self.joints[j.0 as usize].flags;
                let (b0, b1) = self.joint_bodies(j);
                let mut anchor = [0.0f32; 3];
                if flags & JOINT_REVERSE != 0 {
                    // getAnchor2
                    match b1 {
                        Some(b1) => anchor = get_rel_point_pos(b1, &anchor2),
                        None => anchor = [anchor2[0], anchor2[1], anchor2[2]],
                    }
                } else if let Some(b0) = b0 {
                    // getAnchor (leaves the result alone without a body)
                    anchor = get_rel_point_pos(b0, &anchor1);
                }
                self.set_anchors(j, anchor[0], anchor[1], anchor[2]);
            }
            JointKind::Fixed { .. } => {}
            JointKind::Slider { .. } => {
                self.slider_compute_offset(j);
                self.compute_initial_relative_rotation(j);
            }
        }
    }

    /// `dxJointDBall::updateTargetDistance` @ 0x140342590.
    fn dball_update_target_distance(&mut self, j: JointId) {
        let (b0, b1) = self.joint_bodies(j);
        let JointKind::DBall { anchor1, anchor2, .. } = self.joints[j.0 as usize].kind else { return };
        let p1 = match b0 {
            Some(b) => get_rel_point_pos(b, &anchor1),
            None => [anchor1[0], anchor1[1], anchor1[2]],
        };
        let p2 = match b1 {
            Some(b) => get_rel_point_pos(b, &anchor2),
            None => [anchor2[0], anchor2[1], anchor2[2]],
        };
        let d = [p1[0] - p2[0], p1[1] - p2[1], p1[2] - p2[2]];
        let distance = length3(&d);
        if let JointKind::DBall { target_distance, .. } = &mut self.joints[j.0 as usize].kind {
            *target_distance = distance;
        }
    }

    /// `dJointSetDBallAnchor1` @ 0x1403426a0: world point on the first body.
    pub fn joint_set_dball_anchor1(&mut self, j: JointId, x: f32, y: f32, z: f32) {
        let reverse = self.joints[j.0 as usize].flags & JOINT_REVERSE != 0;
        self.dball_set_anchor(j, if reverse { 1 } else { 0 }, [x, y, z]);
        self.dball_update_target_distance(j);
    }

    /// `dJointSetDBallAnchor2` @ 0x140342750: world point on the second body.
    pub fn joint_set_dball_anchor2(&mut self, j: JointId, x: f32, y: f32, z: f32) {
        let reverse = self.joints[j.0 as usize].flags & JOINT_REVERSE != 0;
        self.dball_set_anchor(j, if reverse { 0 } else { 1 }, [x, y, z]);
        self.dball_update_target_distance(j);
    }

    fn dball_set_anchor(&mut self, j: JointId, node: usize, p: [f32; 3]) {
        let body = self.joints[j.0 as usize].node[node].body;
        let local = match body {
            Some(b) => get_pos_rel_point(&self.bodies[b.0 as usize], &p),
            None => p,
        };
        if let JointKind::DBall { anchor1, anchor2, .. } = &mut self.joints[j.0 as usize].kind {
            let anchor = if node == 0 { anchor1 } else { anchor2 };
            anchor[0] = local[0];
            anchor[1] = local[1];
            anchor[2] = local[2];
        }
    }

    /// `dJointSetDBallDistance` @ 0x140342800.
    pub fn joint_set_dball_distance(&mut self, j: JointId, distance: f32) {
        if let JointKind::DBall { target_distance, .. } = &mut self.joints[j.0 as usize].kind {
            *target_distance = distance;
        }
    }

    /// `dJointGetDBallDistance` @ 0x140342690.
    pub fn joint_get_dball_distance(&self, j: JointId) -> f32 {
        match self.joints[j.0 as usize].kind {
            JointKind::DBall { target_distance, .. } => target_distance,
            _ => 0.0,
        }
    }

    /// `setAnchors` @ 0x14034de50 for a ball joint: a world point becomes the anchor in each
    /// body's frame.
    fn set_anchors(&mut self, j: JointId, x: f32, y: f32, z: f32) {
        let (b0, b1) = self.joint_bodies(j);
        let mut new1: Option<[f32; 3]> = None;
        let mut new2: Option<[f32; 3]> = None;
        if let Some(b0) = b0 {
            new1 = Some(get_pos_rel_point(b0, &[x, y, z]));
            new2 = Some(match b1 {
                Some(b1) => get_pos_rel_point(b1, &[x, y, z]),
                None => [x, y, z],
            });
        }
        if let JointKind::Ball { anchor1, anchor2, .. } = &mut self.joints[j.0 as usize].kind {
            if let Some(a) = new1 {
                anchor1[..3].copy_from_slice(&a);
            }
            if let Some(a) = new2 {
                anchor2[..3].copy_from_slice(&a);
            }
            anchor1[3] = 0.0;
            anchor2[3] = 0.0;
        }
    }

    /// `dJointSetBallAnchor` @ 0x140340de0: world point.
    pub fn joint_set_ball_anchor(&mut self, j: JointId, x: f32, y: f32, z: f32) {
        self.set_anchors(j, x, y, z);
    }

    /// `dJointSetFixed` @ 0x140341e30: remember the current relative position and orientation
    /// of the two bodies as the ones to keep.
    pub fn joint_set_fixed(&mut self, j: JointId) {
        let (b0, b1) = self.joint_bodies(j);
        let mut new_offset: Option<[f32; 3]> = None;
        if let Some(b0) = b0 {
            if let Some(b1) = b1 {
                let ofs = [b0.pos[0] - b1.pos[0], b0.pos[1] - b1.pos[1], b0.pos[2] - b1.pos[2]];
                new_offset = Some(multiply1_331(&b0.r, &ofs));
            } else {
                new_offset = Some([b0.pos[0], b0.pos[1], b0.pos[2]]);
            }
        }
        if let (Some(o), JointKind::Fixed { offset, .. }) = (new_offset, &mut self.joints[j.0 as usize].kind) {
            offset[..3].copy_from_slice(&o);
        }
        self.compute_initial_relative_rotation(j);
    }

    /// `dxJointFixed::computeInitialRelativeRotation` @ 0x140341b10 and the slider's twin @
    /// 0x140340f80: `qrel` = rotation of body 2 relative to body 1.
    fn compute_initial_relative_rotation(&mut self, j: JointId) {
        let (b0, b1) = self.joint_bodies(j);
        let Some(b0) = b0 else { return };
        let new_qrel = match b1 {
            Some(b1) => q_multiply1(&b0.q, &b1.q),
            // set qrel to the transpose of the first body q
            None => [b0.q[0], -b0.q[1], -b0.q[2], -b0.q[3]],
        };
        match &mut self.joints[j.0 as usize].kind {
            JointKind::Fixed { qrel, .. } | JointKind::Slider { qrel, .. } => *qrel = new_qrel,
            _ => {}
        }
    }

    /// `dxJointSlider::computeOffset` @ 0x140341000.
    fn slider_compute_offset(&mut self, j: JointId) {
        let (b0, b1) = self.joint_bodies(j);
        let mut new_offset: Option<[f32; 3]> = None;
        if let Some(b1) = b1 {
            let b0 = b0.expect("a slider with a second body has a first body");
            let c = [b0.pos[0] - b1.pos[0], b0.pos[1] - b1.pos[1], b0.pos[2] - b1.pos[2]];
            new_offset = Some(multiply1_331(&b1.r, &c));
        } else if let Some(b0) = b0 {
            new_offset = Some([b0.pos[0], b0.pos[1], b0.pos[2]]);
        }
        if let (Some(o), JointKind::Slider { offset, .. }) = (new_offset, &mut self.joints[j.0 as usize].kind) {
            offset[..3].copy_from_slice(&o);
        }
    }

    /// `dJointSetSliderAxis` @ 0x1403419c0: world direction of the slider (normalised here).
    pub fn joint_set_slider_axis(&mut self, j: JointId, x: f32, y: f32, z: f32) {
        // setAxes @ 0x14034e030
        let (b0, _) = self.joint_bodies(j);
        let mut new_axis: Option<[f32; 3]> = None;
        if let Some(b0) = b0 {
            let mut q = [x, y, z, 0.0];
            safe_normalize3(&mut q);
            new_axis = Some(multiply1_331(&b0.r, &q));
        }
        if let (Some(a), JointKind::Slider { axis1, .. }) = (new_axis, &mut self.joints[j.0 as usize].kind) {
            axis1[..3].copy_from_slice(&a);
            axis1[3] = 0.0;
        }
        self.slider_compute_offset(j);
        self.compute_initial_relative_rotation(j);
    }

    /// `dJointSetBallParam` = `dJointSetDBallParam` = `dJointSetFixedParam` @ 0x140340e10 and
    /// `dJointSetSliderParam` @ 0x140341a00. For Ball, DBall and Fixed only `PARAM_CFM` and
    /// `PARAM_ERP` exist; for the slider the value goes to its limit-motor block, which has no
    /// ERP of its own (so `PARAM_ERP` is dropped there).
    pub fn joint_set_param(&mut self, j: JointId, parameter: i32, value: f32) {
        match &mut self.joints[j.0 as usize].kind {
            JointKind::Ball { erp, cfm, .. } | JointKind::DBall { erp, cfm, .. } | JointKind::Fixed { erp, cfm, .. } => {
                match parameter {
                    PARAM_CFM => *cfm = value,
                    PARAM_ERP => *erp = value,
                    _ => {}
                }
            }
            JointKind::Slider { limot, .. } => limot.set(parameter, value),
        }
    }

    /// `dJointSetFeedback`: ask for (or stop asking for) the constraint forces of a joint.
    /// (Not linked into `acs.exe`; the game never asks. It does not change the simulation.)
    pub fn joint_set_feedback(&mut self, j: JointId, on: bool) {
        self.joints[j.0 as usize].feedback = on.then(JointFeedback::default);
    }

    /// `dJointGetBody` @ 0x14033ff90: the body given as first (0) or second (1) at attach time.
    pub fn joint_get_body(&self, j: JointId, index: usize) -> Option<BodyId> {
        let joint = &self.joints[j.0 as usize];
        if index > 1 {
            return None;
        }
        if joint.flags & JOINT_REVERSE != 0 {
            joint.node[1 - index].body
        } else {
            joint.node[index].body
        }
    }
}
