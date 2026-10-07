// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! AC's thin layer over the rigid-body library: `PhysicsCore` (the `IPhysicsCore`
//! implementation) and `RigidBodyODE` (`IRigidBody`), on top of `rustyac-ode`.
//!
//! The game's `RigidBodyODE` objects are handles here ([`RigidBody`]); their methods are
//! methods of [`PhysicsCore`] that take the handle. Every call that hands a force to a body
//! (or stops it) can be written to a "force tape" in call order, with the system it came
//! from, so that a run can be compared with the tape `tools/car_oracle` records.

use rustyac_ode::{BodyId, JointId, Mass, World, PARAM_CFM, PARAM_ERP};

use crate::vecmath::{Mat44f, Vec3f};

/// Which system a force call belongs to. The names are the labels of
/// `tools/car_oracle/src/sites.rs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ForceSource {
    Tyre,
    Surface,
    Spring,
    Damper,
    Bumpstop,
    HeaveSpring,
    HeaveDamper,
    HeaveBumpstop,
    Arb,
    AeroDrag,
    AeroLift,
    Drivetrain,
    Brake,
    Steering,
    Stability,
    Sleep,
    Teleport,
    #[default]
    Other,
}

impl ForceSource {
    /// The label `car_oracle` books the call under.
    pub fn name(self) -> &'static str {
        match self {
            ForceSource::Tyre => "tyre",
            ForceSource::Surface => "surface",
            ForceSource::Spring => "spring",
            ForceSource::Damper => "damper",
            ForceSource::Bumpstop => "bumpstop",
            ForceSource::HeaveSpring => "heave_spring",
            ForceSource::HeaveDamper => "heave_damper",
            ForceSource::HeaveBumpstop => "heave_bumpstop",
            ForceSource::Arb => "arb",
            ForceSource::AeroDrag => "aero_drag",
            ForceSource::AeroLift => "aero_lift",
            ForceSource::Drivetrain => "drivetrain",
            ForceSource::Brake => "brake",
            ForceSource::Steering => "steering",
            ForceSource::Stability => "stability",
            ForceSource::Sleep => "sleep",
            ForceSource::Teleport => "teleport",
            ForceSource::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<ForceSource> {
        use ForceSource::*;
        [
            Tyre, Surface, Spring, Damper, Bumpstop, HeaveSpring, HeaveDamper, HeaveBumpstop, Arb, AeroDrag,
            AeroLift, Drivetrain, Brake, Steering, Stability, Sleep, Teleport, Other,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }
}

/// Kinds of calls on the force tape (the numbers of `car_oracle`'s recordings).
pub mod kind {
    /// `addForceAtPos`: a = force (world), b = position (world)
    pub const ADD_FORCE_AT_POS: u32 = 1;
    /// `addForceAtLocalPos`: a = force (world), b = position (body)
    pub const ADD_FORCE_AT_LOCAL_POS: u32 = 2;
    /// `addLocalForce`: a = force (body), at the centre of mass
    pub const ADD_LOCAL_FORCE: u32 = 3;
    /// `addLocalForceAtPos`: a = force (body), b = position (world)
    pub const ADD_LOCAL_FORCE_AT_POS: u32 = 4;
    /// `addLocalForceAtLocalPos`: a = force (body), b = position (body)
    pub const ADD_LOCAL_FORCE_AT_LOCAL_POS: u32 = 5;
    /// `addTorque`: a = torque (world)
    pub const ADD_TORQUE: u32 = 6;
    /// `addLocalTorque`: a = torque (body)
    pub const ADD_LOCAL_TORQUE: u32 = 7;
    /// `stop`: velocities and accumulators zeroed
    pub const STOP: u32 = 8;
}

/// One call that handed a force or torque to a rigid body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TapeCall {
    /// The body, in creation order (car body 0, fuel tank 1, hubs 2..).
    pub body: u32,
    /// One of [`kind`].
    pub kind: u32,
    pub source: ForceSource,
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// The body's force accumulator right after the call.
    pub facc: [f32; 3],
    /// The body's torque accumulator right after the call.
    pub tacc: [f32; 3],
}

/// AC's `RigidBodyODE` (an `IRigidBody`): a handle to one body of the [`PhysicsCore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RigidBody {
    pub id: BodyId,
    /// Creation order within the core (the body index of the force tape).
    pub index: u32,
}

/// AC's `DistanceJointODE`: an ODE DBall joint and the rod length measured at creation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceJoint {
    pub id: JointId,
    /// `DistanceJointODE::distance`: read back from ODE when the joint was made;
    /// `reseatDistanceJointLocal` restores it.
    pub distance: f32,
}

/// AC's `FixedJointODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedJoint {
    pub id: JointId,
}

fn v3(a: [f32; 3]) -> Vec3f {
    Vec3f::new(a[0], a[1], a[2])
}

fn a3(v: &Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// AC's `PhysicsCore` @ ctor 0x1402cba80, without collisions (stage 2 and 3 of the
/// rigid-body port).
pub struct PhysicsCore {
    pub world: World,
    bodies: Vec<BodyId>,
    /// The force tape of the current step, when recording is on.
    pub tape: Option<Vec<TapeCall>>,
    /// The system the next force calls are booked under.
    pub source: ForceSource,
    /// Give every new joint a feedback buffer (the constraint forces can then be read).
    pub joint_feedback: bool,
}

impl Default for PhysicsCore {
    fn default() -> PhysicsCore {
        PhysicsCore::new()
    }
}

impl PhysicsCore {
    /// `PhysicsCore::PhysicsCore` @ 0x1402cba80: gravity (0, -9.806, 0), ERP 0.3, CFM 1e-7,
    /// no damping.
    pub fn new() -> PhysicsCore {
        PhysicsCore {
            world: World::assetto_corsa(),
            bodies: Vec::new(),
            tape: None,
            source: ForceSource::Other,
            joint_feedback: false,
        }
    }

    /// The bodies in creation order.
    pub fn bodies(&self) -> impl Iterator<Item = RigidBody> + '_ {
        self.bodies.iter().enumerate().map(|(index, &id)| RigidBody { id, index: index as u32 })
    }

    fn record(&mut self, body: RigidBody, kind: u32, a: [f32; 3], b: [f32; 3]) {
        let source = self.source;
        if let Some(tape) = &mut self.tape {
            let state = self.world.body(body.id);
            tape.push(TapeCall {
                body: body.index,
                kind,
                source,
                a,
                b,
                facc: [state.facc[0], state.facc[1], state.facc[2]],
                tacc: [state.tacc[0], state.tacc[1], state.tacc[2]],
            });
        }
    }

    /// `PhysicsCore::createRigidBody` @ 0x1402cc3b0 + `RigidBodyODE::RigidBodyODE`
    /// @ 0x1402cd800: `dBodyCreate`, finite rotation mode 1 with axis (0,0,0), linear and
    /// angular damping 0.
    pub fn create_rigid_body(&mut self) -> RigidBody {
        let id = self.world.body_create();
        self.world.body_set_finite_rotation_mode(id, true);
        self.world.body_set_finite_rotation_axis(id, 0.0, 0.0, 0.0);
        self.world.body_set_linear_damping(id, 0.0);
        self.world.body_set_angular_damping(id, 0.0);
        self.bodies.push(id);
        RigidBody { id, index: self.bodies.len() as u32 - 1 }
    }

    /// `PhysicsCore::step` @ 0x1402cd690 without the collision step: `dWorldStep`.
    pub fn step(&mut self, dt: f32) {
        self.world.step(dt);
    }

    // --- RigidBodyODE: state ---------------------------------------------------------------

    /// `RigidBodyODE::getWorldMatrix` @ 0x1402ce6c0: ODE's rotation transposed into the rows
    /// of a `mat44f` (row 0 = the body's x axis in world space …), position in row 3.
    pub fn get_world_matrix(&self, body: RigidBody) -> Mat44f {
        let b = self.world.body(body.id);
        let r = &b.r;
        Mat44f {
            m: [
                [r[0], r[4], r[8], 0.0],
                [r[1], r[5], r[9], 0.0],
                [r[2], r[6], r[10], 0.0],
                [b.pos[0], b.pos[1], b.pos[2], 1.0],
            ],
        }
    }

    /// `RigidBodyODE::localToWorld` @ 0x1402ce780: `dBodyGetRelPointPos`.
    pub fn local_to_world(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.world.body_get_rel_point_pos(body.id, a3(p)))
    }

    /// `RigidBodyODE::worldToLocal` @ 0x1402ceb30: `dBodyGetPosRelPoint`.
    pub fn world_to_local(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.world.body_get_pos_rel_point(body.id, a3(p)))
    }

    /// `RigidBodyODE::localToWorldNormal` @ 0x1402ce7c0: `dBodyVectorToWorld`.
    pub fn local_to_world_normal(&self, body: RigidBody, v: &Vec3f) -> Vec3f {
        v3(self.world.body_vector_to_world(body.id, a3(v)))
    }

    /// `RigidBodyODE::worldToLocalNormal` @ 0x1402ceb70: `dBodyVectorFromWorld`.
    pub fn world_to_local_normal(&self, body: RigidBody, v: &Vec3f) -> Vec3f {
        v3(self.world.body_vector_from_world(body.id, a3(v)))
    }

    /// `RigidBodyODE::getPosition` @ 0x1402ce640 (the interpolation argument is not used).
    pub fn get_position(&self, body: RigidBody) -> Vec3f {
        let b = self.world.body(body.id);
        Vec3f::new(b.pos[0], b.pos[1], b.pos[2])
    }

    /// `RigidBodyODE::getVelocity` @ 0x1402ce680: not the stored linear velocity but
    /// `dBodyGetRelPointVel(0, 0, 0)`, the velocity of the body's origin computed like that
    /// of any other point (the same value, except for the sign of a zero).
    pub fn get_velocity(&self, body: RigidBody) -> Vec3f {
        v3(self.world.body_get_rel_point_vel(body.id, [0.0; 3]))
    }

    /// `RigidBodyODE::getLocalVelocity` @ 0x1402ce520: `worldToLocalNormal(getVelocity())`.
    pub fn get_local_velocity(&self, body: RigidBody) -> Vec3f {
        let v = self.get_velocity(body);
        self.world_to_local_normal(body, &v)
    }

    /// `RigidBodyODE::getLocalAngularVelocity` @ 0x1402ce430:
    /// `worldToLocalNormal(getAngularVelocity())`.
    pub fn get_local_angular_velocity(&self, body: RigidBody) -> Vec3f {
        let w = self.get_angular_velocity(body);
        self.world_to_local_normal(body, &w)
    }

    /// `RigidBodyODE::getAngularVelocity` @ 0x1402ce3f0: `dBodyGetAngularVel`.
    pub fn get_angular_velocity(&self, body: RigidBody) -> Vec3f {
        let b = self.world.body(body.id);
        Vec3f::new(b.avel[0], b.avel[1], b.avel[2])
    }

    /// `RigidBodyODE::getLocalPointVelocity` @ 0x1402ce4e0: `dBodyGetRelPointVel`, the
    /// world velocity of a body-frame point.
    pub fn get_local_point_velocity(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.world.body_get_rel_point_vel(body.id, a3(p)))
    }

    /// `RigidBodyODE::getPointVelocity` @ 0x1402ce600: `dBodyGetPointVel`, the world
    /// velocity of a world point.
    pub fn get_point_velocity(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.world.body_get_point_vel(body.id, a3(p)))
    }

    /// `RigidBodyODE::getMass` @ 0x1402ce570: `dBodyGetMass`, the total mass.
    pub fn get_mass(&self, body: RigidBody) -> f32 {
        self.world.body(body.id).mass.mass
    }

    /// `RigidBodyODE::setPosition` @ 0x1402ce9e0: `dBodySetPosition`.
    pub fn set_position(&mut self, body: RigidBody, p: &Vec3f) {
        self.world.body_set_position(body.id, p.x, p.y, p.z);
    }

    /// `RigidBodyODE::setRotation` @ 0x1402cea00: the upper 3x3 of the `mat44f`, transposed
    /// into ODE's layout, to `dBodySetRotation`.
    pub fn set_rotation(&mut self, body: RigidBody, m: &Mat44f) {
        let m = &m.m;
        let r = [
            m[0][0], m[1][0], m[2][0], 0.0, //
            m[0][1], m[1][1], m[2][1], 0.0, //
            m[0][2], m[1][2], m[2][2], 0.0,
        ];
        self.world.body_set_rotation(body.id, &r);
    }

    /// `RigidBodyODE::setMassBox` @ 0x1402ce890: `dMassSetBoxTotal(mass, x, y, z)`, then
    /// `dBodySetMass`.
    pub fn set_mass_box(&mut self, body: RigidBody, mass: f32, x: f32, y: f32, z: f32) {
        let m = Mass::box_total(mass, x, y, z);
        self.world.body_set_mass(body.id, &m);
    }

    /// `RigidBodyODE::stop` @ 0x1402cead0 (its argument is ignored): linear velocity,
    /// angular velocity, force and torque to zero.
    pub fn stop(&mut self, body: RigidBody) {
        self.world.body_set_linear_vel(body.id, 0.0, 0.0, 0.0);
        self.world.body_set_angular_vel(body.id, 0.0, 0.0, 0.0);
        self.world.body_set_force(body.id, 0.0, 0.0, 0.0);
        self.world.body_set_torque(body.id, 0.0, 0.0, 0.0);
        self.record(body, kind::STOP, [0.0; 3], [0.0; 3]);
    }

    // --- RigidBodyODE: forces --------------------------------------------------------------

    /// `RigidBodyODE::addForceAtPos` @ 0x1402cdf30: `dBodyAddForceAtPos`.
    pub fn add_force_at_pos(&mut self, body: RigidBody, f: &Vec3f, p: &Vec3f) {
        self.world.body_add_force_at_pos(body.id, a3(f), a3(p));
        self.record(body, kind::ADD_FORCE_AT_POS, a3(f), a3(p));
    }

    /// `RigidBodyODE::addForceAtLocalPos` @ 0x1402cdee0: `dBodyAddForceAtRelPos`.
    pub fn add_force_at_local_pos(&mut self, body: RigidBody, f: &Vec3f, p: &Vec3f) {
        self.world.body_add_force_at_rel_pos(body.id, a3(f), a3(p));
        self.record(body, kind::ADD_FORCE_AT_LOCAL_POS, a3(f), a3(p));
    }

    /// `RigidBodyODE::addLocalForce` @ 0x1402cdf80: `dBodyAddRelForceAtRelPos(f, 0, 0, 0)`.
    pub fn add_local_force(&mut self, body: RigidBody, f: &Vec3f) {
        self.world.body_add_rel_force_at_rel_pos(body.id, a3(f), [0.0; 3]);
        self.record(body, kind::ADD_LOCAL_FORCE, a3(f), [0.0; 3]);
    }

    /// `RigidBodyODE::addLocalForceAtPos` @ 0x1402ce010: `dBodyAddRelForceAtPos`.
    pub fn add_local_force_at_pos(&mut self, body: RigidBody, f: &Vec3f, p: &Vec3f) {
        self.world.body_add_rel_force_at_pos(body.id, a3(f), a3(p));
        self.record(body, kind::ADD_LOCAL_FORCE_AT_POS, a3(f), a3(p));
    }

    /// `RigidBodyODE::addLocalForceAtLocalPos` @ 0x1402cdfc0: `dBodyAddRelForceAtRelPos`.
    pub fn add_local_force_at_local_pos(&mut self, body: RigidBody, f: &Vec3f, p: &Vec3f) {
        self.world.body_add_rel_force_at_rel_pos(body.id, a3(f), a3(p));
        self.record(body, kind::ADD_LOCAL_FORCE_AT_LOCAL_POS, a3(f), a3(p));
    }

    /// `RigidBodyODE::addTorque` @ 0x1402ce3d0: `dBodyAddTorque`.
    pub fn add_torque(&mut self, body: RigidBody, t: &Vec3f) {
        self.world.body_add_torque(body.id, a3(t));
        self.record(body, kind::ADD_TORQUE, a3(t), [0.0; 3]);
    }

    /// `RigidBodyODE::addLocalTorque` @ 0x1402ce060: `dBodyAddRelTorque`.
    pub fn add_local_torque(&mut self, body: RigidBody, t: &Vec3f) {
        self.world.body_add_rel_torque(body.id, a3(t));
        self.record(body, kind::ADD_LOCAL_TORQUE, a3(t), [0.0; 3]);
    }

    /// One call of a force tape (kinds 1 to 8) on a body: how the feed hands the forces of
    /// a system that is not ported yet to the chassis.
    pub fn apply_call(&mut self, body: RigidBody, kind: u32, a: &Vec3f, b: &Vec3f) -> Result<(), String> {
        match kind {
            kind::ADD_FORCE_AT_POS => self.add_force_at_pos(body, a, b),
            kind::ADD_FORCE_AT_LOCAL_POS => self.add_force_at_local_pos(body, a, b),
            kind::ADD_LOCAL_FORCE => self.add_local_force(body, a),
            kind::ADD_LOCAL_FORCE_AT_POS => self.add_local_force_at_pos(body, a, b),
            kind::ADD_LOCAL_FORCE_AT_LOCAL_POS => self.add_local_force_at_local_pos(body, a, b),
            kind::ADD_TORQUE => self.add_torque(body, a),
            kind::ADD_LOCAL_TORQUE => self.add_local_torque(body, a),
            kind::STOP => self.stop(body),
            other => return Err(format!("a call of kind {other} cannot be applied to a body")),
        }
        Ok(())
    }

    // --- joints ----------------------------------------------------------------------------

    /// `PhysicsCore::createDistanceJoint` @ 0x1402cc190: a DBall joint between two bodies
    /// with its two anchors given in world coordinates; the rod length ODE then reports is
    /// kept.
    pub fn create_distance_joint(&mut self, body1: RigidBody, body2: RigidBody, p1: &Vec3f, p2: &Vec3f) -> DistanceJoint {
        let id = self.world.joint_create_dball();
        self.world.joint_attach(id, Some(body1.id), Some(body2.id));
        self.world.joint_set_dball_anchor1(id, p1.x, p1.y, p1.z);
        self.world.joint_set_dball_anchor2(id, p2.x, p2.y, p2.z);
        if self.joint_feedback {
            self.world.joint_set_feedback(id, true);
        }
        let distance = self.world.joint_get_dball_distance(id);
        DistanceJoint { id, distance }
    }

    /// `PhysicsCore::createFixedJoint` @ 0x1402cc2a0: a fixed joint that keeps the two
    /// bodies as they are now (`dJointSetFixed`).
    pub fn create_fixed_joint(&mut self, body1: RigidBody, body2: RigidBody) -> FixedJoint {
        let id = self.world.joint_create_fixed();
        self.world.joint_attach(id, Some(body1.id), Some(body2.id));
        self.world.joint_set_fixed(id);
        if self.joint_feedback {
            self.world.joint_set_feedback(id, true);
        }
        FixedJoint { id }
    }

    /// `PhysicsCore::reseatDistanceJointLocal` @ 0x1402cd480: both anchors, given in their
    /// body's own coordinates, are converted to world (`dBodyGetRelPointPos`) and set again;
    /// then the rod gets its original length back (`dJointSetDBallDistance`).
    pub fn reseat_distance_joint_local(&mut self, joint: &DistanceJoint, p1: &Vec3f, p2: &Vec3f) {
        let body1 = self.world.joint_get_body(joint.id, 0).expect("a distance joint has two bodies");
        let body2 = self.world.joint_get_body(joint.id, 1).expect("a distance joint has two bodies");
        let w1 = self.world.body_get_rel_point_pos(body1, a3(p1));
        let w2 = self.world.body_get_rel_point_pos(body2, a3(p2));
        self.world.joint_set_dball_anchor1(joint.id, w1[0], w1[1], w1[2]);
        self.world.joint_set_dball_anchor2(joint.id, w2[0], w2[1], w2[2]);
        self.world.joint_set_dball_distance(joint.id, joint.distance);
    }

    /// `DistanceJointODE::setERPCFM` / `FixedJointODE::setERPCFM` @ 0x1402cd5a0: each value is
    /// only written when it is above 0.
    pub fn joint_set_erp_cfm(&mut self, joint: JointId, erp: f32, cfm: f32) {
        if erp > 0.0 {
            self.world.joint_set_param(joint, PARAM_ERP, erp);
        }
        if cfm > 0.0 {
            self.world.joint_set_param(joint, PARAM_CFM, cfm);
        }
    }
}
