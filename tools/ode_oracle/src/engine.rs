// SPDX-License-Identifier: MIT OR Apache-2.0

//! One interface, two rigid-body engines: Assetto Corsa's own ODE (functions called by
//! address inside the mapped `acs.exe`) and the Rust port `rustyac-ode`. A synthetic world is
//! described once and built in both through this interface.

use crate::acs::Acs;
use rustyac_ode::{JointKind, Mass, World};

/// Which joint to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ball,
    DBall,
    Fixed,
    Slider,
}

/// How a body's mass is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MassSpec {
    /// `dMassSetBoxTotal(mass, lx, ly, lz)`, what AC's `setMassBox` does.
    Box { mass: f32, lx: f32, ly: f32, lz: f32 },
    /// `dMassSetParameters(mass, 0, 0, 0, i11, i22, i33, i12, i13, i23)`.
    Parameters { mass: f32, i: [f32; 6] },
    /// What AC's `setMassExplicitInertia` does: the three values go to `I[0]`, `I[4]`, `I[8]`
    /// of a zeroed `dMass` (not the diagonal), so `dBodySetMass` falls back to an identity
    /// inverse inertia.
    ExplicitBug { mass: f32, i: [f32; 3] },
}

/// State of one body, as compared after every step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyState {
    pub pos: [f32; 3],
    pub r: [f32; 12],
    pub q: [f32; 4],
    pub lvel: [f32; 3],
    pub avel: [f32; 3],
    pub facc: [f32; 3],
    pub tacc: [f32; 3],
    pub tag: i32,
    pub flags: u32,
}

/// Mass data of one body, as compared after set-up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassState {
    pub mass: f32,
    pub i: [f32; 12],
    pub inv_i: [f32; 12],
    pub inv_mass: f32,
}

/// State of one joint: its solver tag, its own parameters (anchors …) and the constraint
/// force of the last step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JointState {
    pub tag: i32,
    pub flags: u32,
    /// `dJointGetBody(j, 0)` and `(j, 1)` as body indices, -1 for none.
    pub bodies: [i32; 2],
    pub params: Vec<f32>,
    /// f1, t1, f2, t2 (three values each)
    pub feedback: [f32; 12],
}

/// What the feedback values are set to before every step: a value neither engine computes,
/// so that "not written" shows as such on both sides.
pub const FEEDBACK_SENTINEL: f32 = -12345.678;

pub trait Engine {
    /// The calls of `PhysicsCore::PhysicsCore` that matter without collision, with the given
    /// values (the game's: gravity (0, -9.806, 0), ERP 0.3, CFM 1e-7, damping 0).
    fn set_world(&mut self, gravity: [f32; 3], erp: f32, cfm: f32, damping: [f32; 2]);
    /// Whether joints created from now on get a feedback buffer (the game's never do).
    fn set_feedback(&mut self, on: bool);
    /// Overwrites every joint's feedback values with [`FEEDBACK_SENTINEL`].
    fn feedback_sentinel(&mut self);
    fn body_create(&mut self) -> usize;
    /// `dBodyDestroy`; the body must not be used afterwards.
    fn body_destroy(&mut self, b: usize);
    fn body_set_mass(&mut self, b: usize, mass: &MassSpec);
    fn body_set_position(&mut self, b: usize, p: [f32; 3]);
    fn body_set_rotation(&mut self, b: usize, r: &[f32; 12]);
    fn body_set_linear_vel(&mut self, b: usize, v: [f32; 3]);
    fn body_set_angular_vel(&mut self, b: usize, v: [f32; 3]);
    fn body_set_finite_rotation(&mut self, b: usize, mode: bool, axis: [f32; 3]);
    fn body_set_damping(&mut self, b: usize, linear: f32, angular: f32);
    fn body_set_auto_disable(&mut self, b: usize, on: bool);
    /// `dBodySetAutoDisableAverageSamplesCount`.
    fn body_set_samples(&mut self, b: usize, count: u32);
    fn body_set_enabled(&mut self, b: usize, on: bool);
    /// Sets and clears bits of `dxBody::flags` directly, and the body's `max_angular_speed`.
    /// No function linked into the game can set "no gravity" (8), "max angular speed" (0x80)
    /// or clear "gyroscopic" (0x100); the stepper's code for them is still there.
    fn body_poke(&mut self, b: usize, set: u32, clear: u32, max_angular_speed: f32);
    /// Writes `invMass` directly (0 makes the body kinematic, which no linked setter can do).
    fn body_poke_inv_mass(&mut self, b: usize, inv_mass: f32);
    /// Writes the body's auto-disable idle time and idle step count directly (their setters
    /// are not linked).
    fn body_poke_idle(&mut self, b: usize, idle_time: f32, idle_steps: i32);
    /// `kind`: 0 `dBodyAddForce`, 1 `dBodyAddTorque`, 2 `dBodyAddRelTorque`, 3
    /// `dBodyAddForceAtPos`, 4 `dBodyAddForceAtRelPos`, 5 `dBodyAddRelForceAtPos`, 6
    /// `dBodyAddRelForceAtRelPos`.
    fn body_add(&mut self, b: usize, kind: u32, a: [f32; 3], p: [f32; 3]);
    /// `dBodySetForce` / `dBodySetTorque`.
    fn body_set_accumulators(&mut self, b: usize, f: [f32; 3], t: [f32; 3]);
    fn body_get_rel_point_pos(&mut self, b: usize, p: [f32; 3]) -> [f32; 3];
    /// The six frame-conversion getters for one point, in this order: `dBodyGetRelPointPos`,
    /// `dBodyGetPosRelPoint`, `dBodyVectorToWorld`, `dBodyVectorFromWorld`,
    /// `dBodyGetPointVel`, `dBodyGetRelPointVel`.
    fn body_probe(&mut self, b: usize, p: [f32; 3]) -> [f32; 18];
    fn joint_create(&mut self, kind: Kind) -> usize;
    fn joint_attach(&mut self, j: usize, b1: Option<usize>, b2: Option<usize>);
    fn joint_set_ball_anchor(&mut self, j: usize, p: [f32; 3]);
    fn joint_set_dball_anchor(&mut self, j: usize, which: u32, p: [f32; 3]);
    fn joint_set_dball_distance(&mut self, j: usize, d: f32);
    fn joint_get_dball_distance(&mut self, j: usize) -> f32;
    fn joint_set_fixed(&mut self, j: usize);
    fn joint_set_slider_axis(&mut self, j: usize, axis: [f32; 3]);
    fn joint_set_param(&mut self, j: usize, parameter: i32, value: f32);
    /// Sets and clears bits of `dxJoint::flags` directly (8 = disabled; `dJointDisable` is
    /// not linked).
    fn joint_poke_flags(&mut self, j: usize, set: u32, clear: u32);
    fn step(&mut self, h: f32);
    /// Constraint rows of the largest island in the last step (0 if the engine cannot tell).
    fn rows(&self) -> u32 {
        0
    }
    /// The last step needed something the engine does not have (the Rust port, stage 1: a
    /// bounded constraint row, which a slider gets when it reaches one of its stops). The
    /// engine's state is undefined afterwards.
    fn unsupported(&self) -> bool {
        false
    }
    fn body_state(&self, b: usize) -> BodyState;
    fn mass_state(&self, b: usize) -> MassState;
    fn joint_state(&self, j: usize) -> JointState;
    /// Writes a body's kinematic state directly (no API call exists for the quaternion).
    fn body_write_state(&mut self, b: usize, s: &BodyState);
    /// Writes a joint's own parameters directly, in the layout of [`JointState::params`].
    fn joint_write_params(&mut self, j: usize, params: &[f32]);
}

// ---------------------------------------------------------------------------------------------
// The Rust port

pub struct RustEngine {
    pub world: World,
    bodies: Vec<rustyac_ode::BodyId>,
    joints: Vec<rustyac_ode::JointId>,
    rows: u32,
    feedback: bool,
    unsupported: bool,
}

impl RustEngine {
    pub fn new() -> RustEngine {
        RustEngine {
            world: World::new(),
            bodies: Vec::new(),
            joints: Vec::new(),
            rows: 0,
            feedback: true,
            unsupported: false,
        }
    }
}

/// The message of the port's `unimplemented!` for bounded rows.
pub const BOUNDED_ROWS_MESSAGE: &str = "bounded constraint rows";

fn take3(v: &[f32]) -> [f32; 3] {
    [v[0], v[1], v[2]]
}

/// The meaningful parameters of a joint, in one layout for both engines:
/// Ball: anchor1, anchor2, erp, cfm; DBall: anchor1, anchor2, erp, cfm, distance;
/// Fixed: qrel (4), offset, erp, cfm; Slider: axis1, qrel (4), offset, then the limit-motor
/// block (vel, fmax, lostop, histop, fudge factor, normal cfm, stop erp, stop cfm, bounce).
/// The slider's position (`dJointGetSliderPosition`) and its stop state (`limot.limit`: 0, or
/// 1 / 2 at the low / high stop, set by `getInfo1` in every step) are appended by `joint_state`.
pub fn joint_params(kind: &JointKind) -> Vec<f32> {
    let mut p = Vec::new();
    match kind {
        JointKind::Ball { anchor1, anchor2, erp, cfm } => {
            p.extend_from_slice(&anchor1[..3]);
            p.extend_from_slice(&anchor2[..3]);
            p.extend_from_slice(&[*erp, *cfm]);
        }
        JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
            p.extend_from_slice(&anchor1[..3]);
            p.extend_from_slice(&anchor2[..3]);
            p.extend_from_slice(&[*erp, *cfm, *target_distance]);
        }
        JointKind::Fixed { qrel, offset, erp, cfm } => {
            p.extend_from_slice(qrel);
            p.extend_from_slice(&offset[..3]);
            p.extend_from_slice(&[*erp, *cfm]);
        }
        JointKind::Slider { axis1, qrel, offset, limot } => {
            p.extend_from_slice(&axis1[..3]);
            p.extend_from_slice(qrel);
            p.extend_from_slice(&offset[..3]);
            p.extend_from_slice(&[
                limot.vel,
                limot.fmax,
                limot.lostop,
                limot.histop,
                limot.fudge_factor,
                limot.normal_cfm,
                limot.stop_erp,
                limot.stop_cfm,
                limot.bounce,
            ]);
        }
        // contacts are checked through tools/car_oracle (collide); no world of this tool makes one
        JointKind::Contact { .. } => unimplemented!("contact joints are not part of the ode_oracle worlds"),
    }
    p
}

impl Engine for RustEngine {
    fn set_world(&mut self, gravity: [f32; 3], erp: f32, cfm: f32, damping: [f32; 2]) {
        self.world.set_gravity(gravity[0], gravity[1], gravity[2]);
        self.world.set_erp(erp);
        self.world.set_cfm(cfm);
        self.world.set_damping(damping[0], damping[1]);
    }
    fn set_feedback(&mut self, on: bool) {
        self.feedback = on;
    }
    fn feedback_sentinel(&mut self) {
        for &j in &self.joints {
            if let Some(fb) = &mut self.world.joint_mut(j).feedback {
                fb.f1 = [FEEDBACK_SENTINEL; 4];
                fb.t1 = [FEEDBACK_SENTINEL; 4];
                fb.f2 = [FEEDBACK_SENTINEL; 4];
                fb.t2 = [FEEDBACK_SENTINEL; 4];
            }
        }
    }
    fn body_create(&mut self) -> usize {
        self.bodies.push(self.world.body_create());
        self.bodies.len() - 1
    }
    fn body_destroy(&mut self, b: usize) {
        self.world.body_destroy(self.bodies[b]);
    }
    fn body_set_mass(&mut self, b: usize, mass: &MassSpec) {
        let m = match *mass {
            MassSpec::Box { mass, lx, ly, lz } => Mass::box_total(mass, lx, ly, lz),
            MassSpec::Parameters { mass, i } => {
                Mass::parameters(mass, 0.0, 0.0, 0.0, i[0], i[1], i[2], i[3], i[4], i[5])
            }
            MassSpec::ExplicitBug { mass, i } => {
                let mut m = Mass::zero();
                m.mass = mass;
                m.i[0] = i[0];
                m.i[4] = i[1];
                m.i[8] = i[2];
                m
            }
        };
        self.world.body_set_mass(self.bodies[b], &m);
    }
    fn body_set_position(&mut self, b: usize, p: [f32; 3]) {
        self.world.body_set_position(self.bodies[b], p[0], p[1], p[2]);
    }
    fn body_set_rotation(&mut self, b: usize, r: &[f32; 12]) {
        self.world.body_set_rotation(self.bodies[b], r);
    }
    fn body_set_linear_vel(&mut self, b: usize, v: [f32; 3]) {
        self.world.body_set_linear_vel(self.bodies[b], v[0], v[1], v[2]);
    }
    fn body_set_angular_vel(&mut self, b: usize, v: [f32; 3]) {
        self.world.body_set_angular_vel(self.bodies[b], v[0], v[1], v[2]);
    }
    fn body_set_finite_rotation(&mut self, b: usize, mode: bool, axis: [f32; 3]) {
        // the order of RigidBodyODE::RigidBodyODE: mode first, then the axis
        self.world.body_set_finite_rotation_mode(self.bodies[b], mode);
        self.world.body_set_finite_rotation_axis(self.bodies[b], axis[0], axis[1], axis[2]);
    }
    fn body_set_damping(&mut self, b: usize, linear: f32, angular: f32) {
        self.world.body_set_linear_damping(self.bodies[b], linear);
        self.world.body_set_angular_damping(self.bodies[b], angular);
    }
    fn body_set_auto_disable(&mut self, b: usize, on: bool) {
        self.world.body_set_auto_disable_flag(self.bodies[b], on);
    }
    fn body_set_samples(&mut self, b: usize, count: u32) {
        self.world.body_set_auto_disable_average_samples_count(self.bodies[b], count);
    }
    fn body_set_enabled(&mut self, b: usize, on: bool) {
        if on {
            self.world.body_enable(self.bodies[b]);
        } else {
            self.world.body_disable(self.bodies[b]);
        }
    }
    fn body_poke(&mut self, b: usize, set: u32, clear: u32, max_angular_speed: f32) {
        let body = self.world.body_mut(self.bodies[b]);
        body.flags = (body.flags | set) & !clear;
        body.max_angular_speed = max_angular_speed;
    }
    fn body_poke_inv_mass(&mut self, b: usize, inv_mass: f32) {
        self.world.body_mut(self.bodies[b]).inv_mass = inv_mass;
    }
    fn body_poke_idle(&mut self, b: usize, idle_time: f32, idle_steps: i32) {
        let body = self.world.body_mut(self.bodies[b]);
        body.adis.idle_time = idle_time;
        body.adis.idle_steps = idle_steps;
    }
    fn body_add(&mut self, b: usize, kind: u32, a: [f32; 3], p: [f32; 3]) {
        let id = self.bodies[b];
        match kind {
            0 => self.world.body_add_force(id, a),
            1 => self.world.body_add_torque(id, a),
            2 => self.world.body_add_rel_torque(id, a),
            3 => self.world.body_add_force_at_pos(id, a, p),
            4 => self.world.body_add_force_at_rel_pos(id, a, p),
            5 => self.world.body_add_rel_force_at_pos(id, a, p),
            _ => self.world.body_add_rel_force_at_rel_pos(id, a, p),
        }
    }
    fn body_set_accumulators(&mut self, b: usize, f: [f32; 3], t: [f32; 3]) {
        self.world.body_set_force(self.bodies[b], f[0], f[1], f[2]);
        self.world.body_set_torque(self.bodies[b], t[0], t[1], t[2]);
    }
    fn body_get_rel_point_pos(&mut self, b: usize, p: [f32; 3]) -> [f32; 3] {
        self.world.body_get_rel_point_pos(self.bodies[b], p)
    }
    fn body_probe(&mut self, b: usize, p: [f32; 3]) -> [f32; 18] {
        let id = self.bodies[b];
        let parts = [
            self.world.body_get_rel_point_pos(id, p),
            self.world.body_get_pos_rel_point(id, p),
            self.world.body_vector_to_world(id, p),
            self.world.body_vector_from_world(id, p),
            self.world.body_get_point_vel(id, p),
            self.world.body_get_rel_point_vel(id, p),
        ];
        let mut out = [0.0f32; 18];
        for (k, part) in parts.iter().enumerate() {
            out[3 * k..3 * k + 3].copy_from_slice(part);
        }
        out
    }
    fn joint_create(&mut self, kind: Kind) -> usize {
        let id = match kind {
            Kind::Ball => self.world.joint_create_ball(),
            Kind::DBall => self.world.joint_create_dball(),
            Kind::Fixed => self.world.joint_create_fixed(),
            Kind::Slider => self.world.joint_create_slider(),
        };
        self.world.joint_set_feedback(id, self.feedback);
        self.joints.push(id);
        self.joints.len() - 1
    }
    fn joint_attach(&mut self, j: usize, b1: Option<usize>, b2: Option<usize>) {
        self.world.joint_attach(self.joints[j], b1.map(|b| self.bodies[b]), b2.map(|b| self.bodies[b]));
    }
    fn joint_set_ball_anchor(&mut self, j: usize, p: [f32; 3]) {
        self.world.joint_set_ball_anchor(self.joints[j], p[0], p[1], p[2]);
    }
    fn joint_set_dball_anchor(&mut self, j: usize, which: u32, p: [f32; 3]) {
        if which == 0 {
            self.world.joint_set_dball_anchor1(self.joints[j], p[0], p[1], p[2]);
        } else {
            self.world.joint_set_dball_anchor2(self.joints[j], p[0], p[1], p[2]);
        }
    }
    fn joint_set_dball_distance(&mut self, j: usize, d: f32) {
        self.world.joint_set_dball_distance(self.joints[j], d);
    }
    fn joint_get_dball_distance(&mut self, j: usize) -> f32 {
        self.world.joint_get_dball_distance(self.joints[j])
    }
    fn joint_set_fixed(&mut self, j: usize) {
        self.world.joint_set_fixed(self.joints[j]);
    }
    fn joint_set_slider_axis(&mut self, j: usize, axis: [f32; 3]) {
        self.world.joint_set_slider_axis(self.joints[j], axis[0], axis[1], axis[2]);
    }
    fn joint_set_param(&mut self, j: usize, parameter: i32, value: f32) {
        self.world.joint_set_param(self.joints[j], parameter, value);
    }
    fn joint_poke_flags(&mut self, j: usize, set: u32, clear: u32) {
        let joint = self.world.joint_mut(self.joints[j]);
        joint.flags = (joint.flags | set) & !clear;
    }
    fn step(&mut self, h: f32) {
        // the stage 1 stepper stops with `unimplemented!` at a bounded row; any other panic
        // is a fault and goes on
        let world = &mut self.world;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.step(h))) {
            Ok(stats) => self.rows = stats.max_rows,
            Err(payload) => {
                let text = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                if !text.contains(BOUNDED_ROWS_MESSAGE) {
                    std::panic::resume_unwind(payload);
                }
                self.unsupported = true;
            }
        }
    }
    fn rows(&self) -> u32 {
        self.rows
    }
    fn unsupported(&self) -> bool {
        self.unsupported
    }
    fn body_state(&self, b: usize) -> BodyState {
        let body = self.world.body(self.bodies[b]);
        BodyState {
            pos: take3(&body.pos),
            r: body.r,
            q: body.q,
            lvel: take3(&body.lvel),
            avel: take3(&body.avel),
            facc: take3(&body.facc),
            tacc: take3(&body.tacc),
            tag: body.tag,
            flags: body.flags,
        }
    }
    fn mass_state(&self, b: usize) -> MassState {
        let body = self.world.body(self.bodies[b]);
        MassState { mass: body.mass.mass, i: body.mass.i, inv_i: body.inv_i, inv_mass: body.inv_mass }
    }
    fn joint_state(&self, j: usize) -> JointState {
        let id = self.joints[j];
        let joint = self.world.joint(id);
        let fb = joint.feedback.unwrap_or_default();
        let mut feedback = [0.0f32; 12];
        feedback[0..3].copy_from_slice(&fb.f1[..3]);
        feedback[3..6].copy_from_slice(&fb.t1[..3]);
        feedback[6..9].copy_from_slice(&fb.f2[..3]);
        feedback[9..12].copy_from_slice(&fb.t2[..3]);
        let mut params = joint_params(&joint.kind);
        if let JointKind::Slider { limot, .. } = &joint.kind {
            params.push(self.world.joint_get_slider_position(id));
            params.push(limot.limit as f32);
        }
        let index = |body: Option<rustyac_ode::BodyId>| {
            body.and_then(|b| self.bodies.iter().position(|&x| x == b)).map_or(-1, |i| i as i32)
        };
        JointState {
            tag: joint.tag,
            flags: joint.flags,
            bodies: [index(self.world.joint_get_body(id, 0)), index(self.world.joint_get_body(id, 1))],
            params,
            feedback,
        }
    }
    fn body_write_state(&mut self, b: usize, s: &BodyState) {
        let body = self.world.body_mut(self.bodies[b]);
        body.pos[..3].copy_from_slice(&s.pos);
        body.r = s.r;
        body.q = s.q;
        body.lvel[..3].copy_from_slice(&s.lvel);
        body.avel[..3].copy_from_slice(&s.avel);
    }
    fn joint_write_params(&mut self, j: usize, p: &[f32]) {
        match &mut self.world.joint_mut(self.joints[j]).kind {
            JointKind::Ball { anchor1, anchor2, erp, cfm } => {
                anchor1[..3].copy_from_slice(&p[0..3]);
                anchor2[..3].copy_from_slice(&p[3..6]);
                *erp = p[6];
                *cfm = p[7];
            }
            JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                anchor1[..3].copy_from_slice(&p[0..3]);
                anchor2[..3].copy_from_slice(&p[3..6]);
                *erp = p[6];
                *cfm = p[7];
                *target_distance = p[8];
            }
            JointKind::Fixed { qrel, offset, erp, cfm } => {
                qrel.copy_from_slice(&p[0..4]);
                offset[..3].copy_from_slice(&p[4..7]);
                *erp = p[7];
                *cfm = p[8];
            }
            JointKind::Slider { axis1, qrel, offset, .. } => {
                axis1[..3].copy_from_slice(&p[0..3]);
                qrel.copy_from_slice(&p[3..7]);
                offset[..3].copy_from_slice(&p[7..10]);
            }
            // contacts are checked through tools/car_oracle (collide); no world of this tool makes one
            JointKind::Contact { .. } => unimplemented!("contact joints are not part of the ode_oracle worlds"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Assetto Corsa's ODE, by address

const VA_INIT_ODE2: usize = 0x1_4034_0bc0; // int dInitODE2(unsigned flags)
const VA_ALLOCATE_DATA_FOR_THREAD: usize = 0x1_4034_0b90; // int dAllocateODEDataForThread(unsigned flags)
const VA_WORLD_CREATE: usize = 0x1_4034_01f0;
const VA_WORLD_DESTROY: usize = 0x1_4034_0220;
const VA_WORLD_SET_GRAVITY: usize = 0x1_4034_04a0;
const VA_WORLD_SET_ERP: usize = 0x1_4034_0490;
const VA_WORLD_SET_CFM: usize = 0x1_4034_0420;
const VA_WORLD_SET_DAMPING: usize = 0x1_4034_0450;
const VA_WORLD_SET_CONTACT_MAX_CORRECTING_VEL: usize = 0x1_4034_0430;
const VA_WORLD_SET_CONTACT_SURFACE_LAYER: usize = 0x1_4034_0440;
const VA_WORLD_SET_QUICK_STEP_NUM_ITERATIONS: usize = 0x1_4034_04b0;
const VA_WORLD_STEP: usize = 0x1_4034_04c0;
const VA_BODY_CREATE: usize = 0x1_4033_ef50;
const VA_BODY_DESTROY: usize = 0x1_4033_f340;
const VA_BODY_SET_POSITION: usize = 0x1_4033_fbb0;
const VA_BODY_SET_ROTATION: usize = 0x1_4033_fc00;
const VA_BODY_SET_LINEAR_VEL: usize = 0x1_4033_fb10;
const VA_BODY_SET_ANGULAR_VEL: usize = 0x1_4033_f8f0;
const VA_BODY_SET_FORCE: usize = 0x1_4033_fad0;
const VA_BODY_SET_TORQUE: usize = 0x1_4033_fc90;
const VA_BODY_SET_MASS: usize = 0x1_4033_fb30;
const VA_BODY_SET_FINITE_ROTATION_MODE: usize = 0x1_4033_fa90;
const VA_BODY_SET_FINITE_ROTATION_AXIS: usize = 0x1_4033_fa40;
const VA_BODY_SET_LINEAR_DAMPING: usize = 0x1_4033_faf0;
const VA_BODY_SET_ANGULAR_DAMPING: usize = 0x1_4033_f8d0;
const VA_BODY_SET_AUTO_DISABLE_FLAG: usize = 0x1_4033_fa00;
const VA_BODY_SET_AUTO_DISABLE_AVERAGE_SAMPLES_COUNT: usize = 0x1_4033_f910;
const VA_BODY_ENABLE: usize = 0x1_4033_f4b0;
const VA_BODY_DISABLE: usize = 0x1_4033_f4a0;
const VA_BODY_ADD: [usize; 7] = [
    0x1_4033_e860, // dBodyAddForce
    0x1_4033_ef10, // dBodyAddTorque
    0x1_4033_ee50, // dBodyAddRelTorque
    0x1_4033_e8a0, // dBodyAddForceAtPos
    0x1_4033_e990, // dBodyAddForceAtRelPos
    0x1_4033_eb00, // dBodyAddRelForceAtPos
    0x1_4033_ec70, // dBodyAddRelForceAtRelPos
];
const VA_BODY_GET_REL_POINT_POS: usize = 0x1_4033_f6c0;
const VA_BODY_PROBES: [usize; 6] = [
    0x1_4033_f6c0, // dBodyGetRelPointPos
    0x1_4033_f5e0, // dBodyGetPosRelPoint
    0x1_4033_fd60, // dBodyVectorToWorld
    0x1_4033_fcb0, // dBodyVectorFromWorld
    0x1_4033_f520, // dBodyGetPointVel
    0x1_4033_f780, // dBodyGetRelPointVel
];
const VA_MASS_SET_BOX_TOTAL: usize = 0x1_4034_6bc0;
const VA_MASS_SET_PARAMETERS: usize = 0x1_4034_6c60;
const VA_JOINT_CREATE: [usize; 4] = [
    0x1_4033_fed0, // dJointCreateBall
    0x1_4033_ff60, // dJointCreateDBall
    0x1_4033_ff70, // dJointCreateFixed
    0x1_4033_ff80, // dJointCreateSlider
];
const VA_JOINT_ATTACH: usize = 0x1_4033_fe20;
const VA_JOINT_GET_BODY: usize = 0x1_4033_ff90;
const VA_JOINT_SET_BALL_ANCHOR: usize = 0x1_4034_0de0;
const VA_JOINT_SET_DBALL_ANCHOR1: usize = 0x1_4034_26a0;
const VA_JOINT_SET_DBALL_ANCHOR2: usize = 0x1_4034_2750;
const VA_JOINT_SET_DBALL_DISTANCE: usize = 0x1_4034_2800;
const VA_JOINT_GET_DBALL_DISTANCE: usize = 0x1_4034_2690;
const VA_JOINT_SET_FIXED: usize = 0x1_4034_1e30;
const VA_JOINT_SET_SLIDER_AXIS: usize = 0x1_4034_19c0;
const VA_JOINT_GET_SLIDER_POSITION: usize = 0x1_4034_17c0;
const VA_JOINT_SET_BALL_PARAM: usize = 0x1_4034_0e10; // = DBall, Fixed
const VA_JOINT_SET_SLIDER_PARAM: usize = 0x1_4034_1a00;

// dxBody
const B_TAG: usize = 0x20;
const B_FLAGS: usize = 0x38;
const B_MASS: usize = 0x48;
const B_INV_I: usize = 0x8c;
const B_INV_MASS: usize = 0xbc;
const B_POS: usize = 0xc0;
const B_R: usize = 0xd0;
const B_Q: usize = 0x100;
const B_LVEL: usize = 0x110;
const B_AVEL: usize = 0x120;
const B_FACC: usize = 0x130;
const B_TACC: usize = 0x140;
const B_ADIS_IDLE_TIME: usize = 0x160;
const B_ADIS_IDLE_STEPS: usize = 0x164;
const B_MAX_ANGULAR_SPEED: usize = 0x1b0;
// dxJoint
const J_TAG: usize = 0x20;
const J_FLAGS: usize = 0x30;
const J_BODY1: usize = 0x40;
const J_FEEDBACK: usize = 0x68;
const J_OWN: usize = 0x88;

type F0 = extern "C" fn(*mut u8);
type F3 = extern "C" fn(*mut u8, f32, f32, f32);
type F6 = extern "C" fn(*mut u8, f32, f32, f32, f32, f32, f32);

pub struct AcEngine<'a> {
    acs: &'a Acs,
    world: *mut u8,
    bodies: Vec<*mut u8>,
    /// joint, its kind, its feedback buffer (null when the world runs without feedback)
    joints: Vec<(*mut u8, Kind, *mut [f32; 16])>,
    feedback: bool,
}

unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    base.add(offset).cast::<T>().read_unaligned()
}

unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    base.add(offset).cast::<T>().write_unaligned(value)
}

/// ODE's process-wide start-up, once per process (what `PhysicsCore::PhysicsCore` does first).
pub fn init_ode(acs: &Acs) {
    unsafe {
        let init: extern "C" fn(u32) -> i32 = std::mem::transmute(acs.va(VA_INIT_ODE2));
        assert_eq!(init(0), 1, "dInitODE2 failed");
        let allocate: extern "C" fn(u32) -> i32 = std::mem::transmute(acs.va(VA_ALLOCATE_DATA_FOR_THREAD));
        assert_eq!(allocate(!0), 1, "dAllocateODEDataForThread failed");
    }
}

impl<'a> AcEngine<'a> {
    /// A new world of the game's ODE. [`init_ode`] must have run.
    pub fn new(acs: &'a Acs) -> AcEngine<'a> {
        let create: extern "C" fn() -> *mut u8 = unsafe { std::mem::transmute(acs.va(VA_WORLD_CREATE)) };
        AcEngine { acs, world: create(), bodies: Vec::new(), joints: Vec::new(), feedback: true }
    }

    fn f<T: Copy>(&self, va: usize) -> T {
        debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
        let address = self.acs.va(va);
        unsafe { std::mem::transmute_copy::<usize, T>(&address) }
    }
}

impl Drop for AcEngine<'_> {
    fn drop(&mut self) {
        // dWorldDestroy frees the bodies and joints with the world
        let destroy: F0 = self.f(VA_WORLD_DESTROY);
        destroy(self.world);
        for &(_, _, feedback) in &self.joints {
            if !feedback.is_null() {
                drop(unsafe { Box::from_raw(feedback) });
            }
        }
    }
}

impl Engine for AcEngine<'_> {
    fn set_world(&mut self, gravity: [f32; 3], erp: f32, cfm: f32, damping: [f32; 2]) {
        // the ODE calls of PhysicsCore::PhysicsCore @ 0x1402cba80, in its order
        let set_gravity: F3 = self.f(VA_WORLD_SET_GRAVITY);
        set_gravity(self.world, gravity[0], gravity[1], gravity[2]);
        let set1: extern "C" fn(*mut u8, f32) = self.f(VA_WORLD_SET_ERP);
        set1(self.world, erp);
        let set1: extern "C" fn(*mut u8, f32) = self.f(VA_WORLD_SET_CFM);
        set1(self.world, cfm);
        let set1: extern "C" fn(*mut u8, f32) = self.f(VA_WORLD_SET_CONTACT_MAX_CORRECTING_VEL);
        set1(self.world, 3.0);
        let set1: extern "C" fn(*mut u8, f32) = self.f(VA_WORLD_SET_CONTACT_SURFACE_LAYER);
        set1(self.world, 0.0);
        let set2: extern "C" fn(*mut u8, f32, f32) = self.f(VA_WORLD_SET_DAMPING);
        set2(self.world, damping[0], damping[1]);
        let seti: extern "C" fn(*mut u8, i32) = self.f(VA_WORLD_SET_QUICK_STEP_NUM_ITERATIONS);
        seti(self.world, 48);
    }
    fn set_feedback(&mut self, on: bool) {
        self.feedback = on;
    }
    fn feedback_sentinel(&mut self) {
        for &(_, _, feedback) in &self.joints {
            if !feedback.is_null() {
                unsafe { *feedback = [FEEDBACK_SENTINEL; 16] };
            }
        }
    }
    fn body_create(&mut self) -> usize {
        let create: extern "C" fn(*mut u8) -> *mut u8 = self.f(VA_BODY_CREATE);
        self.bodies.push(create(self.world));
        self.bodies.len() - 1
    }
    fn body_destroy(&mut self, b: usize) {
        let destroy: F0 = self.f(VA_BODY_DESTROY);
        destroy(self.bodies[b]);
        self.bodies[b] = std::ptr::null_mut();
    }
    fn body_set_mass(&mut self, b: usize, mass: &MassSpec) {
        // dMass: float mass, c[4], I[12]
        let mut m = [0.0f32; 17];
        match *mass {
            MassSpec::Box { mass, lx, ly, lz } => {
                let f: extern "C" fn(*mut f32, f32, f32, f32, f32) = self.f(VA_MASS_SET_BOX_TOTAL);
                f(m.as_mut_ptr(), mass, lx, ly, lz);
            }
            MassSpec::Parameters { mass, i } => {
                #[allow(clippy::type_complexity)]
                let f: extern "C" fn(*mut f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32) =
                    self.f(VA_MASS_SET_PARAMETERS);
                f(m.as_mut_ptr(), mass, 0.0, 0.0, 0.0, i[0], i[1], i[2], i[3], i[4], i[5]);
            }
            MassSpec::ExplicitBug { mass, i } => {
                m[0] = mass;
                m[5] = i[0];
                m[5 + 4] = i[1];
                m[5 + 8] = i[2];
            }
        }
        let set: extern "C" fn(*mut u8, *const f32) = self.f(VA_BODY_SET_MASS);
        set(self.bodies[b], m.as_ptr());
    }
    fn body_set_position(&mut self, b: usize, p: [f32; 3]) {
        let f: F3 = self.f(VA_BODY_SET_POSITION);
        f(self.bodies[b], p[0], p[1], p[2]);
    }
    fn body_set_rotation(&mut self, b: usize, r: &[f32; 12]) {
        let f: extern "C" fn(*mut u8, *const f32) = self.f(VA_BODY_SET_ROTATION);
        f(self.bodies[b], r.as_ptr());
    }
    fn body_set_linear_vel(&mut self, b: usize, v: [f32; 3]) {
        let f: F3 = self.f(VA_BODY_SET_LINEAR_VEL);
        f(self.bodies[b], v[0], v[1], v[2]);
    }
    fn body_set_angular_vel(&mut self, b: usize, v: [f32; 3]) {
        let f: F3 = self.f(VA_BODY_SET_ANGULAR_VEL);
        f(self.bodies[b], v[0], v[1], v[2]);
    }
    fn body_set_finite_rotation(&mut self, b: usize, mode: bool, axis: [f32; 3]) {
        let f: extern "C" fn(*mut u8, i32) = self.f(VA_BODY_SET_FINITE_ROTATION_MODE);
        f(self.bodies[b], mode as i32);
        let f: F3 = self.f(VA_BODY_SET_FINITE_ROTATION_AXIS);
        f(self.bodies[b], axis[0], axis[1], axis[2]);
    }
    fn body_set_damping(&mut self, b: usize, linear: f32, angular: f32) {
        let f: extern "C" fn(*mut u8, f32) = self.f(VA_BODY_SET_LINEAR_DAMPING);
        f(self.bodies[b], linear);
        let f: extern "C" fn(*mut u8, f32) = self.f(VA_BODY_SET_ANGULAR_DAMPING);
        f(self.bodies[b], angular);
    }
    fn body_set_auto_disable(&mut self, b: usize, on: bool) {
        let f: extern "C" fn(*mut u8, i32) = self.f(VA_BODY_SET_AUTO_DISABLE_FLAG);
        f(self.bodies[b], on as i32);
    }
    fn body_set_samples(&mut self, b: usize, count: u32) {
        let f: extern "C" fn(*mut u8, u32) = self.f(VA_BODY_SET_AUTO_DISABLE_AVERAGE_SAMPLES_COUNT);
        f(self.bodies[b], count);
    }
    fn body_set_enabled(&mut self, b: usize, on: bool) {
        let f: F0 = self.f(if on { VA_BODY_ENABLE } else { VA_BODY_DISABLE });
        f(self.bodies[b]);
    }
    fn body_poke(&mut self, b: usize, set: u32, clear: u32, max_angular_speed: f32) {
        let p = self.bodies[b];
        unsafe {
            let flags: u32 = rd(p, B_FLAGS);
            wr(p, B_FLAGS, (flags | set) & !clear);
            wr(p, B_MAX_ANGULAR_SPEED, max_angular_speed);
        }
    }
    fn body_poke_inv_mass(&mut self, b: usize, inv_mass: f32) {
        unsafe { wr(self.bodies[b], B_INV_MASS, inv_mass) };
    }
    fn body_poke_idle(&mut self, b: usize, idle_time: f32, idle_steps: i32) {
        unsafe {
            wr(self.bodies[b], B_ADIS_IDLE_TIME, idle_time);
            wr(self.bodies[b], B_ADIS_IDLE_STEPS, idle_steps);
        }
    }
    fn body_add(&mut self, b: usize, kind: u32, a: [f32; 3], p: [f32; 3]) {
        let kind = (kind as usize).min(6);
        if kind < 3 {
            let f: F3 = self.f(VA_BODY_ADD[kind]);
            f(self.bodies[b], a[0], a[1], a[2]);
        } else {
            let f: F6 = self.f(VA_BODY_ADD[kind]);
            f(self.bodies[b], a[0], a[1], a[2], p[0], p[1], p[2]);
        }
    }
    fn body_set_accumulators(&mut self, b: usize, f: [f32; 3], t: [f32; 3]) {
        let set: F3 = self.f(VA_BODY_SET_FORCE);
        set(self.bodies[b], f[0], f[1], f[2]);
        let set: F3 = self.f(VA_BODY_SET_TORQUE);
        set(self.bodies[b], t[0], t[1], t[2]);
    }
    fn body_get_rel_point_pos(&mut self, b: usize, p: [f32; 3]) -> [f32; 3] {
        let f: extern "C" fn(*mut u8, f32, f32, f32, *mut f32) = self.f(VA_BODY_GET_REL_POINT_POS);
        let mut out = [0.0f32; 4];
        f(self.bodies[b], p[0], p[1], p[2], out.as_mut_ptr());
        [out[0], out[1], out[2]]
    }
    fn body_probe(&mut self, b: usize, p: [f32; 3]) -> [f32; 18] {
        let mut out = [0.0f32; 18];
        for (k, va) in VA_BODY_PROBES.iter().enumerate() {
            let f: extern "C" fn(*mut u8, f32, f32, f32, *mut f32) = self.f(*va);
            let mut part = [0.0f32; 4];
            f(self.bodies[b], p[0], p[1], p[2], part.as_mut_ptr());
            out[3 * k..3 * k + 3].copy_from_slice(&part[..3]);
        }
        out
    }
    fn joint_create(&mut self, kind: Kind) -> usize {
        let index = match kind {
            Kind::Ball => 0,
            Kind::DBall => 1,
            Kind::Fixed => 2,
            Kind::Slider => 3,
        };
        let create: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = self.f(VA_JOINT_CREATE[index]);
        let joint = create(self.world, std::ptr::null_mut());
        // dJointSetFeedback is not linked: the pointer is written directly. Without feedback
        // the joint keeps the null pointer its constructor wrote, as every joint of the game.
        let mut feedback = std::ptr::null_mut();
        if self.feedback {
            feedback = Box::into_raw(Box::new([0.0f32; 16]));
            unsafe { wr(joint, J_FEEDBACK, feedback) };
        }
        self.joints.push((joint, kind, feedback));
        self.joints.len() - 1
    }
    fn joint_attach(&mut self, j: usize, b1: Option<usize>, b2: Option<usize>) {
        let f: extern "C" fn(*mut u8, *mut u8, *mut u8) = self.f(VA_JOINT_ATTACH);
        let body = |b: Option<usize>| b.map_or(std::ptr::null_mut(), |b| self.bodies[b]);
        f(self.joints[j].0, body(b1), body(b2));
    }
    fn joint_set_ball_anchor(&mut self, j: usize, p: [f32; 3]) {
        let f: F3 = self.f(VA_JOINT_SET_BALL_ANCHOR);
        f(self.joints[j].0, p[0], p[1], p[2]);
    }
    fn joint_set_dball_anchor(&mut self, j: usize, which: u32, p: [f32; 3]) {
        let f: F3 = self.f(if which == 0 { VA_JOINT_SET_DBALL_ANCHOR1 } else { VA_JOINT_SET_DBALL_ANCHOR2 });
        f(self.joints[j].0, p[0], p[1], p[2]);
    }
    fn joint_set_dball_distance(&mut self, j: usize, d: f32) {
        let f: extern "C" fn(*mut u8, f32) = self.f(VA_JOINT_SET_DBALL_DISTANCE);
        f(self.joints[j].0, d);
    }
    fn joint_get_dball_distance(&mut self, j: usize) -> f32 {
        let f: extern "C" fn(*mut u8) -> f32 = self.f(VA_JOINT_GET_DBALL_DISTANCE);
        f(self.joints[j].0)
    }
    fn joint_set_fixed(&mut self, j: usize) {
        let f: F0 = self.f(VA_JOINT_SET_FIXED);
        f(self.joints[j].0);
    }
    fn joint_set_slider_axis(&mut self, j: usize, axis: [f32; 3]) {
        let f: F3 = self.f(VA_JOINT_SET_SLIDER_AXIS);
        f(self.joints[j].0, axis[0], axis[1], axis[2]);
    }
    fn joint_set_param(&mut self, j: usize, parameter: i32, value: f32) {
        let (joint, kind, _) = self.joints[j];
        let va = if kind == Kind::Slider { VA_JOINT_SET_SLIDER_PARAM } else { VA_JOINT_SET_BALL_PARAM };
        let f: extern "C" fn(*mut u8, i32, f32) = self.f(va);
        f(joint, parameter, value);
    }
    fn joint_poke_flags(&mut self, j: usize, set: u32, clear: u32) {
        let p = self.joints[j].0;
        unsafe {
            let flags: u32 = rd(p, J_FLAGS);
            wr(p, J_FLAGS, (flags | set) & !clear);
        }
    }
    fn step(&mut self, h: f32) {
        let f: extern "C" fn(*mut u8, f32) -> i32 = self.f(VA_WORLD_STEP);
        assert_eq!(f(self.world, h), 1, "dWorldStep failed");
    }
    fn body_state(&self, b: usize) -> BodyState {
        let p = self.bodies[b];
        unsafe {
            BodyState {
                pos: rd(p, B_POS),
                r: rd(p, B_R),
                q: rd(p, B_Q),
                lvel: rd(p, B_LVEL),
                avel: rd(p, B_AVEL),
                facc: rd(p, B_FACC),
                tacc: rd(p, B_TACC),
                tag: rd(p, B_TAG),
                flags: rd(p, B_FLAGS),
            }
        }
    }
    fn mass_state(&self, b: usize) -> MassState {
        let p = self.bodies[b];
        unsafe {
            MassState {
                mass: rd(p, B_MASS),
                i: rd(p, B_MASS + 4 + 16),
                inv_i: rd(p, B_INV_I),
                inv_mass: rd(p, B_INV_MASS),
            }
        }
    }
    fn joint_state(&self, j: usize) -> JointState {
        let (p, kind, feedback) = self.joints[j];
        unsafe {
            // the joint classes have different sizes: read only what each one owns
            let own = |from: usize, count: usize| -> Vec<f32> {
                (0..count).map(|k| rd::<f32>(p, J_OWN + 4 * (from + k))).collect()
            };
            let mut params = Vec::new();
            match kind {
                Kind::Ball => {
                    params.extend(own(0, 3));
                    params.extend(own(4, 3));
                    params.extend(own(8, 2));
                }
                Kind::DBall => {
                    params.extend(own(0, 3));
                    params.extend(own(4, 3));
                    params.extend(own(8, 3));
                }
                Kind::Fixed => {
                    params.extend(own(0, 4));
                    params.extend(own(4, 3));
                    params.extend(own(8, 2));
                }
                Kind::Slider => {
                    params.extend(own(0, 3));
                    params.extend(own(4, 4));
                    params.extend(own(8, 3));
                    // the limit-motor block at +0xb8: vel, fmax, lostop, histop, fudge factor,
                    // normal cfm, stop erp, stop cfm, bounce
                    params.extend(own(12, 9));
                    // dJointGetSliderPosition dereferences body 1 without a test
                    let position: extern "C" fn(*mut u8) -> f32 = self.f(VA_JOINT_GET_SLIDER_POSITION);
                    params.push(if rd::<usize>(p, J_BODY1) != 0 { position(p) } else { 0.0 });
                    // limot.limit, the int after the nine floats (+0xdc)
                    params.push(rd::<i32>(p, J_OWN + 4 * 21) as f32);
                }
            }
            let mut out = [0.0f32; 12];
            if !feedback.is_null() {
                let fb = *feedback;
                for k in 0..4 {
                    out[3 * k..3 * k + 3].copy_from_slice(&fb[4 * k..4 * k + 3]);
                }
            }
            let get_body: extern "C" fn(*mut u8, i32) -> *mut u8 = self.f(VA_JOINT_GET_BODY);
            let index = |body: *mut u8| {
                if body.is_null() {
                    -1
                } else {
                    self.bodies.iter().position(|&x| x == body).map_or(-2, |i| i as i32)
                }
            };
            JointState {
                tag: rd(p, J_TAG),
                flags: rd(p, J_FLAGS),
                bodies: [index(get_body(p, 0)), index(get_body(p, 1))],
                params,
                feedback: out,
            }
        }
    }
    fn body_write_state(&mut self, b: usize, s: &BodyState) {
        let p = self.bodies[b];
        unsafe {
            wr(p, B_POS, s.pos);
            wr(p, B_R, s.r);
            wr(p, B_Q, s.q);
            wr(p, B_LVEL, s.lvel);
            wr(p, B_AVEL, s.avel);
        }
    }
    fn joint_write_params(&mut self, j: usize, params: &[f32]) {
        let (p, kind, _) = self.joints[j];
        unsafe {
            let at = |k: usize| p.add(J_OWN + 4 * k).cast::<f32>();
            let put = |from: &[f32], to: usize| {
                for (i, v) in from.iter().enumerate() {
                    at(to + i).write_unaligned(*v);
                }
            };
            match kind {
                Kind::Ball => {
                    put(&params[0..3], 0);
                    put(&params[3..6], 4);
                    put(&params[6..8], 8);
                }
                Kind::DBall => {
                    put(&params[0..3], 0);
                    put(&params[3..6], 4);
                    put(&params[6..9], 8);
                }
                Kind::Fixed => {
                    put(&params[0..4], 0);
                    put(&params[4..7], 4);
                    put(&params[7..9], 8);
                }
                Kind::Slider => {
                    put(&params[0..3], 0);
                    put(&params[3..7], 4);
                    put(&params[7..10], 8);
                }
            }
        }
    }
}
