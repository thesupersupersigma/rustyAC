//! World, bodies and the body API: `ode/src/ode.cpp`, `objects.h`.
//!
//! ODE keeps its objects in intrusive linked lists and the order of those lists decides the
//! order of bodies and joints inside an island, and with it the row order of the constraint
//! matrix (so: the rounding). The lists are kept here exactly as ODE keeps them, with
//! handles ([`BodyId`], [`JointId`]) in place of pointers:
//!
//! * the world's body list and joint list: newest object first (`addObjectToList`);
//! * each body's list of joint nodes: newest attached joint first (`dJointAttach`).

use crate::common::{Matrix3, Quaternion, Vector3};
use crate::joint::Joint;
use crate::mass::Mass;
use crate::matrix::invert_pd_matrix;
use crate::odemath::{
    cross3, is_nonzero, multiply0_331, multiply1_331, orthogonalize_r, safe_normalize3, safe_normalize4,
};
use crate::rotation::{q_from_r, r_set_identity};

/// Handle of a body (`dBodyID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(pub u32);

/// Handle of a joint (`dJointID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JointId(pub u32);

/// A reference to one of the two `dxJointNode`s of a joint (ODE's `dxJointNode*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub joint: JointId,
    pub node: u8,
}

// body flags (`objects.h`)
/// `dxBodyFlagFiniteRotation`
pub const BODY_FINITE_ROTATION: u32 = 1;
/// `dxBodyFlagFiniteRotationAxis`
pub const BODY_FINITE_ROTATION_AXIS: u32 = 2;
/// `dxBodyDisabled`
pub const BODY_DISABLED: u32 = 4;
/// `dxBodyNoGravity`
pub const BODY_NO_GRAVITY: u32 = 8;
/// `dxBodyAutoDisable`
pub const BODY_AUTO_DISABLE: u32 = 16;
/// `dxBodyLinearDamping`
pub const BODY_LINEAR_DAMPING: u32 = 32;
/// `dxBodyAngularDamping`
pub const BODY_ANGULAR_DAMPING: u32 = 64;
/// `dxBodyMaxAngularSpeed`
pub const BODY_MAX_ANGULAR_SPEED: u32 = 128;
/// `dxBodyGyroscopic`
pub const BODY_GYROSCOPIC: u32 = 256;

/// `dxAutoDisable`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoDisable {
    pub idle_time: f32,
    pub idle_steps: i32,
    pub linear_average_threshold: f32,
    pub angular_average_threshold: f32,
    pub average_samples: u32,
}

/// `dxDampingParameters`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damping {
    pub linear_scale: f32,
    pub angular_scale: f32,
    pub linear_threshold: f32,
    pub angular_threshold: f32,
}

/// `dxBody` (0x1b8 bytes in `acs.exe`).
#[derive(Clone, Debug)]
pub struct Body {
    /// False once the body was destroyed ([`World::body_destroy`]); its slot stays, unused.
    pub alive: bool,
    /// Next body in the world's list (`dObject::next`).
    pub next: Option<BodyId>,
    /// After a step: index of the body inside its island (-1: disabled, never stepped).
    pub tag: i32,
    /// First of the joint nodes that point at this body (`firstjoint`).
    pub first_joint: Option<NodeRef>,
    pub flags: u32,
    pub mass: Mass,
    /// Inverse of `mass.i` in the body frame.
    pub inv_i: Matrix3,
    pub inv_mass: f32,
    pub pos: Vector3,
    pub r: Matrix3,
    /// (w, x, y, z)
    pub q: Quaternion,
    pub lvel: Vector3,
    pub avel: Vector3,
    /// Force accumulator.
    pub facc: Vector3,
    /// Torque accumulator.
    pub tacc: Vector3,
    pub finite_rot_axis: Vector3,
    pub adis: AutoDisable,
    pub adis_timeleft: f32,
    pub adis_stepsleft: i32,
    pub average_lvel_buffer: Vec<[f32; 3]>,
    pub average_avel_buffer: Vec<[f32; 3]>,
    pub average_counter: u32,
    pub average_ready: bool,
    pub dampingp: Damping,
    pub max_angular_speed: f32,
}

impl Body {
    /// `dBodyIsEnabled` @ 0x14033f8c0.
    pub fn is_enabled(&self) -> bool {
        self.flags & BODY_DISABLED == 0
    }
}

/// `dxWorld` (0xa8 bytes in `acs.exe`).
#[derive(Clone, Debug)]
pub struct World {
    pub(crate) bodies: Vec<Body>,
    pub(crate) joints: Vec<Joint>,
    /// Head of the body list (newest body).
    pub first_body: Option<BodyId>,
    /// Head of the joint list (newest joint).
    pub first_joint: Option<JointId>,
    /// Number of bodies (`nb`).
    pub nb: u32,
    /// Number of joints (`nj`).
    pub nj: u32,
    pub gravity: Vector3,
    pub global_erp: f32,
    pub global_cfm: f32,
    pub adis: AutoDisable,
    /// Flags new bodies start with (auto-disable, damping, max angular speed).
    pub body_flags: u32,
    pub dampingp: Damping,
    pub max_angular_speed: f32,
    pub(crate) step_memory: crate::step::StepMemory,
}

impl Default for World {
    fn default() -> Self {
        World::new()
    }
}

impl World {
    /// `dWorldCreate` @ 0x1403401f0 (constructor `dxWorld::dxWorld` @ 0x14034cfb0), with the
    /// defaults of the single-precision build: gravity 0, ERP 0.2, CFM 1e-5.
    pub fn new() -> World {
        World {
            bodies: Vec::new(),
            joints: Vec::new(),
            first_body: None,
            first_joint: None,
            nb: 0,
            nj: 0,
            gravity: [0.0; 4],
            global_erp: 0.2,
            global_cfm: 1e-5,
            adis: AutoDisable {
                idle_time: 0.0,
                idle_steps: 10,
                linear_average_threshold: 0.01 * 0.01,
                angular_average_threshold: 0.01 * 0.01,
                average_samples: 1,
            },
            body_flags: 0,
            dampingp: Damping {
                linear_scale: 0.0,
                angular_scale: 0.0,
                linear_threshold: 0.01 * 0.01,
                angular_threshold: 0.01 * 0.01,
            },
            max_angular_speed: f32::INFINITY,
            step_memory: crate::step::StepMemory::default(),
        }
    }

    /// A world with the settings `PhysicsCore::PhysicsCore` @ 0x1402cba80 gives AC's world:
    /// gravity (0, -9.806, 0), ERP 0.3, CFM 1e-7, damping 0.
    pub fn assetto_corsa() -> World {
        let mut world = World::new();
        world.set_gravity(0.0, -9.806, 0.0);
        world.set_erp(0.3);
        world.set_cfm(1e-7);
        world.set_damping(0.0, 0.0);
        world
    }

    /// `dWorldSetGravity` @ 0x1403404a0.
    pub fn set_gravity(&mut self, x: f32, y: f32, z: f32) {
        self.gravity[0] = x;
        self.gravity[1] = y;
        self.gravity[2] = z;
    }

    /// `dWorldSetERP` @ 0x140340490.
    pub fn set_erp(&mut self, erp: f32) {
        self.global_erp = erp;
    }

    /// `dWorldSetCFM` @ 0x140340420.
    pub fn set_cfm(&mut self, cfm: f32) {
        self.global_cfm = cfm;
    }

    /// `dWorldSetDamping` @ 0x140340450: defaults for bodies created afterwards.
    pub fn set_damping(&mut self, linear_scale: f32, angular_scale: f32) {
        if is_nonzero(linear_scale) {
            self.body_flags |= BODY_LINEAR_DAMPING;
        } else {
            self.body_flags &= !BODY_LINEAR_DAMPING;
        }
        self.dampingp.linear_scale = linear_scale;
        if is_nonzero(angular_scale) {
            self.body_flags |= BODY_ANGULAR_DAMPING;
        } else {
            self.body_flags &= !BODY_ANGULAR_DAMPING;
        }
        self.dampingp.angular_scale = angular_scale;
    }

    pub fn body(&self, id: BodyId) -> &Body {
        debug_assert!(self.bodies[id.0 as usize].alive, "use of a destroyed body");
        &self.bodies[id.0 as usize]
    }

    pub fn body_mut(&mut self, id: BodyId) -> &mut Body {
        debug_assert!(self.bodies[id.0 as usize].alive, "use of a destroyed body");
        &mut self.bodies[id.0 as usize]
    }

    pub fn joint(&self, id: JointId) -> &Joint {
        &self.joints[id.0 as usize]
    }

    pub fn joint_mut(&mut self, id: JointId) -> &mut Joint {
        &mut self.joints[id.0 as usize]
    }

    /// The bodies that exist, in creation order (ODE's own list, `first_body` / `next`, runs
    /// the other way: newest first).
    pub fn body_ids(&self) -> impl Iterator<Item = BodyId> + '_ {
        (0..self.bodies.len() as u32).map(BodyId).filter(|b| self.bodies[b.0 as usize].alive)
    }

    /// Every joint ever created, in creation order. A joint whose body was destroyed stays in
    /// the world, attached to nothing.
    pub fn joint_ids(&self) -> impl Iterator<Item = JointId> {
        (0..self.joints.len() as u32).map(JointId)
    }

    /// `dBodyCreate` @ 0x14033ef50: mass 1, identity inertia, at the origin, at rest, gravity
    /// and the gyroscopic term on, infinitesimal rotation update.
    pub fn body_create(&mut self) -> BodyId {
        let id = BodyId(self.bodies.len() as u32);
        let mut flags = 0;
        // dBodySetAutoDisableDefaults, dBodySetDampingDefaults
        flags |= self.body_flags & BODY_AUTO_DISABLE;
        flags |= self.body_flags & (BODY_LINEAR_DAMPING | BODY_ANGULAR_DAMPING);
        flags |= self.body_flags & BODY_MAX_ANGULAR_SPEED;
        flags |= BODY_GYROSCOPIC;
        let samples = self.adis.average_samples as usize;
        let body = Body {
            alive: true,
            next: self.first_body,
            tag: 0,
            first_joint: None,
            flags,
            mass: Mass::parameters(1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0),
            inv_i: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            inv_mass: 1.0,
            pos: [0.0; 4],
            r: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            q: [1.0, 0.0, 0.0, 0.0],
            lvel: [0.0; 4],
            avel: [0.0; 4],
            facc: [0.0; 4],
            tacc: [0.0; 4],
            finite_rot_axis: [0.0; 4],
            adis: self.adis,
            adis_timeleft: self.adis.idle_time,
            adis_stepsleft: self.adis.idle_steps,
            average_lvel_buffer: vec![[0.0; 3]; samples],
            average_avel_buffer: vec![[0.0; 3]; samples],
            average_counter: 0,
            average_ready: false,
            dampingp: self.dampingp,
            max_angular_speed: self.max_angular_speed,
        };
        self.bodies.push(body);
        self.first_body = Some(id);
        self.nb += 1;
        id
    }

    /// `dBodyDestroy` @ 0x14033f340: detaches every joint of the body (the joints stay in the
    /// world, attached to nothing, and are never visited again) and takes the body out of the
    /// world's list. The handle must not be used afterwards.
    pub fn body_destroy(&mut self, b: BodyId) {
        // detach all neighbouring joints, then delete this body.
        let mut n = self.bodies[b.0 as usize].first_joint;
        while let Some(node) = n {
            let joint = &mut self.joints[node.joint.0 as usize];
            // the node in this body's list is the joint's other node; the reference to this
            // body is cleared first so that only the other body's list has to be searched
            joint.node[1 - node.node as usize].body = None;
            let next = joint.node[node.node as usize].next;
            joint.node[node.node as usize].next = None;
            self.remove_joint_references_from_attached_bodies(node.joint);
            n = next;
        }
        self.bodies[b.0 as usize].first_joint = None;
        // removeObjectFromList
        let next = self.bodies[b.0 as usize].next;
        if self.first_body == Some(b) {
            self.first_body = next;
        } else {
            let mut current = self.first_body;
            while let Some(id) = current {
                if self.bodies[id.0 as usize].next == Some(b) {
                    self.bodies[id.0 as usize].next = next;
                    break;
                }
                current = self.bodies[id.0 as usize].next;
            }
        }
        self.bodies[b.0 as usize].next = None;
        self.bodies[b.0 as usize].alive = false;
        self.nb -= 1;
    }

    /// `dBodySetAutoDisableAverageSamplesCount` @ 0x14033f910: how many velocity samples the
    /// auto-disable test averages over (0: the body never falls asleep). Empties the sample
    /// buffers.
    pub fn body_set_auto_disable_average_samples_count(&mut self, b: BodyId, average_samples_count: u32) {
        let body = self.body_mut(b);
        body.adis.average_samples = average_samples_count;
        let samples = average_samples_count as usize;
        body.average_lvel_buffer = vec![[0.0; 3]; samples];
        body.average_avel_buffer = vec![[0.0; 3]; samples];
        // new buffer is empty
        body.average_counter = 0;
        body.average_ready = false;
    }

    /// `dBodySetPosition` @ 0x14033fbb0.
    pub fn body_set_position(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.pos[0] = x;
        body.pos[1] = y;
        body.pos[2] = z;
    }

    /// `dBodySetRotation` @ 0x14033fc00: copies the matrix and orthogonalises the copy; the
    /// quaternion is taken from the matrix **as given** (not from the cleaned copy) and
    /// normalised.
    pub fn body_set_rotation(&mut self, b: BodyId, r: &Matrix3) {
        let body = self.body_mut(b);
        body.r = *r;
        orthogonalize_r(&mut body.r);
        body.q = q_from_r(r);
        safe_normalize4(&mut body.q);
    }

    /// `dBodySetLinearVel` @ 0x14033fb10.
    pub fn body_set_linear_vel(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.lvel[0] = x;
        body.lvel[1] = y;
        body.lvel[2] = z;
    }

    /// `dBodySetAngularVel` @ 0x14033f8f0.
    pub fn body_set_angular_vel(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.avel[0] = x;
        body.avel[1] = y;
        body.avel[2] = z;
    }

    /// `dBodySetForce` @ 0x14033fad0.
    pub fn body_set_force(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.facc[0] = x;
        body.facc[1] = y;
        body.facc[2] = z;
    }

    /// `dBodySetTorque` @ 0x14033fc90.
    pub fn body_set_torque(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.tacc[0] = x;
        body.tacc[1] = y;
        body.tacc[2] = z;
    }

    /// `dBodySetMass` @ 0x14033fb30. An inertia that is not positive definite gives an
    /// identity inverse inertia (ODE's fallback).
    pub fn body_set_mass(&mut self, b: BodyId, mass: &Mass) {
        let body = self.body_mut(b);
        body.mass = *mass;
        let inertia = body.mass.i;
        if !invert_pd_matrix(&inertia, &mut body.inv_i, 3) {
            r_set_identity(&mut body.inv_i);
        }
        body.inv_mass = 1.0f32 / body.mass.mass;
    }

    /// `dBodySetFiniteRotationMode` @ 0x14033fa90.
    pub fn body_set_finite_rotation_mode(&mut self, b: BodyId, mode: bool) {
        let body = self.body_mut(b);
        body.flags &= !(BODY_FINITE_ROTATION | BODY_FINITE_ROTATION_AXIS);
        if mode {
            body.flags |= BODY_FINITE_ROTATION;
            if is_nonzero(body.finite_rot_axis[0])
                || is_nonzero(body.finite_rot_axis[1])
                || is_nonzero(body.finite_rot_axis[2])
            {
                body.flags |= BODY_FINITE_ROTATION_AXIS;
            }
        }
    }

    /// `dBodySetFiniteRotationAxis` @ 0x14033fa40.
    pub fn body_set_finite_rotation_axis(&mut self, b: BodyId, x: f32, y: f32, z: f32) {
        let body = self.body_mut(b);
        body.finite_rot_axis[0] = x;
        body.finite_rot_axis[1] = y;
        body.finite_rot_axis[2] = z;
        if is_nonzero(x) || is_nonzero(y) || is_nonzero(z) {
            safe_normalize3(&mut body.finite_rot_axis);
            body.flags |= BODY_FINITE_ROTATION_AXIS;
        } else {
            body.flags &= !BODY_FINITE_ROTATION_AXIS;
        }
    }

    /// `dBodySetLinearDamping` @ 0x14033faf0.
    pub fn body_set_linear_damping(&mut self, b: BodyId, scale: f32) {
        let body = self.body_mut(b);
        if is_nonzero(scale) {
            body.flags |= BODY_LINEAR_DAMPING;
        } else {
            body.flags &= !BODY_LINEAR_DAMPING;
        }
        body.dampingp.linear_scale = scale;
    }

    /// `dBodySetAngularDamping` @ 0x14033f8d0.
    pub fn body_set_angular_damping(&mut self, b: BodyId, scale: f32) {
        let body = self.body_mut(b);
        if is_nonzero(scale) {
            body.flags |= BODY_ANGULAR_DAMPING;
        } else {
            body.flags &= !BODY_ANGULAR_DAMPING;
        }
        body.dampingp.angular_scale = scale;
    }

    /// `dBodyEnable` @ 0x14033f4b0.
    pub fn body_enable(&mut self, b: BodyId) {
        let body = self.body_mut(b);
        body.flags &= !BODY_DISABLED;
        body.adis_stepsleft = body.adis.idle_steps;
        body.adis_timeleft = body.adis.idle_time;
    }

    /// `dBodyDisable` @ 0x14033f4a0.
    pub fn body_disable(&mut self, b: BodyId) {
        self.body_mut(b).flags |= BODY_DISABLED;
    }

    /// `dBodySetAutoDisableFlag` @ 0x14033fa00. Switching it off also wakes the body, takes
    /// the world's idle settings and empties the velocity sample buffers.
    pub fn body_set_auto_disable_flag(&mut self, b: BodyId, do_auto_disable: bool) {
        let world_adis = self.adis;
        let body = self.body_mut(b);
        if !do_auto_disable {
            body.flags &= !(BODY_AUTO_DISABLE | BODY_DISABLED);
            body.adis.idle_steps = world_adis.idle_steps;
            body.adis.idle_time = world_adis.idle_time;
            // dBodySetAutoDisableAverageSamplesCount(b, world's count): resets the averages
            body.adis.average_samples = world_adis.average_samples;
            let samples = world_adis.average_samples as usize;
            body.average_lvel_buffer = vec![[0.0; 3]; samples];
            body.average_avel_buffer = vec![[0.0; 3]; samples];
            body.average_counter = 0;
            body.average_ready = false;
        } else {
            body.flags |= BODY_AUTO_DISABLE;
        }
    }

    // --- force accumulation (exact arithmetic of the compiled functions) ---------------

    /// `dBodyAddForce` @ 0x14033e860.
    pub fn body_add_force(&mut self, b: BodyId, f: [f32; 3]) {
        let body = self.body_mut(b);
        for i in 0..3 {
            body.facc[i] = f[i] + body.facc[i];
        }
    }

    /// `dBodyAddTorque` @ 0x14033ef10.
    pub fn body_add_torque(&mut self, b: BodyId, t: [f32; 3]) {
        let body = self.body_mut(b);
        for i in 0..3 {
            body.tacc[i] = t[i] + body.tacc[i];
        }
    }

    /// `dBodyAddRelTorque` @ 0x14033ee50.
    pub fn body_add_rel_torque(&mut self, b: BodyId, t: [f32; 3]) {
        let body = self.body_mut(b);
        let t = multiply0_331(&body.r, &t);
        for i in 0..3 {
            body.tacc[i] = t[i] + body.tacc[i];
        }
    }

    /// `dBodyAddForceAtPos` @ 0x14033e8a0: force and point in world coordinates.
    pub fn body_add_force_at_pos(&mut self, b: BodyId, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(b);
        let q = [p[0] - body.pos[0], p[1] - body.pos[1], p[2] - body.pos[2]];
        add_force_and_arm(body, f, q);
    }

    /// `dBodyAddForceAtRelPos` @ 0x14033e990: world force, body-frame point.
    pub fn body_add_force_at_rel_pos(&mut self, b: BodyId, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(b);
        let q = multiply0_331(&body.r, &p);
        add_force_and_arm(body, f, q);
    }

    /// `dBodyAddRelForceAtPos` @ 0x14033eb00: body-frame force, world point.
    pub fn body_add_rel_force_at_pos(&mut self, b: BodyId, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(b);
        let f = multiply0_331(&body.r, &f);
        let q = [p[0] - body.pos[0], p[1] - body.pos[1], p[2] - body.pos[2]];
        add_force_and_arm(body, f, q);
    }

    /// `dBodyAddRelForceAtRelPos` @ 0x14033ec70: force and point in the body frame.
    pub fn body_add_rel_force_at_rel_pos(&mut self, b: BodyId, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(b);
        let f = multiply0_331(&body.r, &f);
        let q = multiply0_331(&body.r, &p);
        add_force_and_arm(body, f, q);
    }

    // --- frame conversions ---------------------------------------------------------------

    /// `dBodyGetRelPointPos` @ 0x14033f6c0: body-frame point to world coordinates.
    pub fn body_get_rel_point_pos(&self, b: BodyId, p: [f32; 3]) -> [f32; 3] {
        get_rel_point_pos(self.body(b), &p)
    }

    /// `dBodyGetPosRelPoint` @ 0x14033f5e0: world point to body-frame coordinates.
    pub fn body_get_pos_rel_point(&self, b: BodyId, p: [f32; 3]) -> [f32; 3] {
        get_pos_rel_point(self.body(b), &p)
    }

    /// `dBodyVectorToWorld` @ 0x14033fd60.
    pub fn body_vector_to_world(&self, b: BodyId, v: [f32; 3]) -> [f32; 3] {
        multiply0_331(&self.body(b).r, &v)
    }

    /// `dBodyVectorFromWorld` @ 0x14033fcb0.
    pub fn body_vector_from_world(&self, b: BodyId, v: [f32; 3]) -> [f32; 3] {
        multiply1_331(&self.body(b).r, &v)
    }

    /// `dBodyGetPointVel` @ 0x14033f520: velocity of a world point fixed to the body.
    pub fn body_get_point_vel(&self, b: BodyId, p: [f32; 3]) -> [f32; 3] {
        get_point_vel(self.body(b), &p)
    }

    /// `dBodyGetRelPointVel` @ 0x14033f780: the same for a body-frame point.
    pub fn body_get_rel_point_vel(&self, b: BodyId, p: [f32; 3]) -> [f32; 3] {
        let body = self.body(b);
        let q = multiply0_331(&body.r, &p);
        let c = cross3(&body.avel, &q);
        [c[0] + body.lvel[0], c[1] + body.lvel[1], c[2] + body.lvel[2]]
    }
}

/// `facc += f`, `tacc += q x f` as the `dBodyAdd…` functions do it.
#[inline(always)]
fn add_force_and_arm(body: &mut Body, f: [f32; 3], q: [f32; 3]) {
    for i in 0..3 {
        body.facc[i] = f[i] + body.facc[i];
    }
    let c = cross3(&q, &f);
    for i in 0..3 {
        body.tacc[i] = c[i] + body.tacc[i];
    }
}

/// `dBodyGetRelPointPos`: `R * p + pos`.
#[inline(always)]
pub(crate) fn get_rel_point_pos(body: &Body, p: &[f32]) -> [f32; 3] {
    let r = multiply0_331(&body.r, p);
    [r[0] + body.pos[0], r[1] + body.pos[1], r[2] + body.pos[2]]
}

/// `dBodyGetPosRelPoint`: `R^T * (p - pos)`.
#[inline(always)]
pub(crate) fn get_pos_rel_point(body: &Body, p: &[f32]) -> [f32; 3] {
    let q = [p[0] - body.pos[0], p[1] - body.pos[1], p[2] - body.pos[2]];
    multiply1_331(&body.r, &q)
}

/// `dBodyGetPointVel`: `lvel + avel x (p - pos)`.
#[inline(always)]
pub(crate) fn get_point_vel(body: &Body, p: &[f32]) -> [f32; 3] {
    let q = [p[0] - body.pos[0], p[1] - body.pos[1], p[2] - body.pos[2]];
    let c = cross3(&body.avel, &q);
    [c[0] + body.lvel[0], c[1] + body.lvel[1], c[2] + body.lvel[2]]
}
