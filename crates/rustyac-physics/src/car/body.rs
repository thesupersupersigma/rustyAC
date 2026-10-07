// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's thin layer over the rigid-body library: `PhysicsCore` (the `IPhysicsCore`
//! implementation) and `RigidBodyODE` (`IRigidBody`), on top of `rustyac-ode`.
//!
//! The game's `RigidBodyODE` objects are handles here ([`RigidBody`]); their methods are
//! methods of [`PhysicsCore`] that take the handle. Every call that hands a force to a body
//! (or stops it) can be written to a "force tape" in call order, with the system it came
//! from, so that a run can be compared with the tape `tools/car_oracle` records.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustyac_ode::collision::TriMeshData;
use rustyac_ode::geom::{BroadPhase, GeomInfo, NearCallback, CLASS_BOX, CLASS_TRIMESH};
use rustyac_ode::{
    BodyId, Contact, ContactGeom, GeomId, GeomRef, JointGroupId, JointId, JointKind, Mass, StaticWorld, SurfaceParameters,
    World, PARAM_CFM, PARAM_ERP,
};

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

/// AC's `PhysicsCore` @ ctor 0x1402cba80: one ODE world, the dynamic collision space with
/// its numbered sub-spaces (one per car), the two groups of contact joints, and what the
/// game does with every contact. The static space (the track's meshes) belongs to the track
/// and is handed in when a step looks for contacts.
pub struct PhysicsCore {
    pub world: World,
    bodies: Vec<BodyId>,
    /// `spaceDynamic`: everything that moves.
    pub space_dynamic: GeomId,
    /// `dynamicSubSpaces`
    dynamic_sub_spaces: BTreeMap<u32, GeomId>,
    /// `contactGroup`: the contacts of the even frames (dynamic against dynamic).
    contact_group: JointGroupId,
    /// `contactGroupDynamic`: the contacts of the odd frames (dynamic against static).
    contact_group_dynamic: JointGroupId,
    /// `currentContactGroup`
    current_contact_group: JointGroupId,
    /// `noCollisionCounter`: steps left without a collision pass.
    pub no_collision_counter: i32,
    /// `currentFrame`: counts the collision passes; its parity picks the pass.
    pub current_frame: u32,
    colliders: Vec<BodyColliders>,
    /// A static world for cores that have no track (a test floor). A car on a track hands the
    /// track's own world to [`PhysicsCore::collision_step`].
    pub statics: Option<Arc<StaticWorld>>,
    /// `dSurfaceParameters::bounce_vel` of a mesh contact. The game never writes the field:
    /// its contact joints get the 32 bits the stack held, which are the upper half of the
    /// address of the first mesh's geom (a small positive number of the heap's, read as a
    /// float: +0 or a denormal below 1e-40). The oracle hands over what its run had; the
    /// game itself can only be met with +0, which differs only for a closing speed that is
    /// itself a denormal.
    pub mesh_bounce_vel: f32,
    /// The same field for a box contact: the upper half of register r12, which is zero when
    /// the game's `PhysicsEngine::step` runs. Such a contact has no bounce, so the value
    /// never reaches a result.
    pub box_bounce_vel: f32,
    /// When set, every contact `dCollide` returns is noted here (for the oracles).
    pub contact_log: Option<Vec<ContactRecord>>,
    /// The collision callbacks of the last [`PhysicsCore::step`].
    pub events: Vec<CollisionEvent>,
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
        let mut world = World::assetto_corsa();
        // contactGroup, contactGroupDynamic; spaceStatic is the track's; spaceDynamic
        let contact_group = world.joint_group_create();
        let contact_group_dynamic = world.joint_group_create();
        let space_dynamic = world.collision.simple_space_create(None);
        PhysicsCore {
            world,
            bodies: Vec::new(),
            space_dynamic,
            dynamic_sub_spaces: BTreeMap::new(),
            contact_group,
            contact_group_dynamic,
            current_contact_group: contact_group,
            no_collision_counter: 0,
            current_frame: 0,
            colliders: Vec::new(),
            statics: None,
            mesh_bounce_vel: 0.0,
            box_bounce_vel: 0.0,
            contact_log: None,
            events: Vec::new(),
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

    /// `PhysicsCore::step` @ 0x1402cd690: the collision pass (against the core's own static
    /// world, if it has one), then `dWorldStep`. The collision callbacks of the pass are left
    /// in [`PhysicsCore::events`]; a car that has to hear of them before the world moves
    /// calls [`PhysicsCore::collision_step`] and [`PhysicsCore::world_step`] itself.
    pub fn step(&mut self, dt: f32) {
        let statics = self.statics.clone();
        self.events = self.collision_step(statics.as_deref());
        self.world_step(dt);
    }

    /// The second half of `PhysicsCore::step`: `dWorldStep`.
    pub fn world_step(&mut self, dt: f32) -> rustyac_ode::StepStats {
        self.world.step(dt)
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

// --- collisions --------------------------------------------------------------------------

/// The category bits the game gives its shapes (`docs/map/body.md` 5.5).
pub mod category {
    /// A track surface listed in `surfaces.ini`.
    pub const SURFACE: u32 = 1;
    /// A track mesh of the built-in `WALL` surface.
    pub const WALL: u32 = 2;
    /// A car of this machine: its floor boxes and its collider mesh.
    pub const CAR: u32 = 4;
    /// A car of another machine.
    pub const REMOTE_CAR: u32 = 8;
    /// A loose object of the track.
    pub const OBJECT: u32 = 0x10;
}

/// What the game's `ICollisionObject` of a shape answers: `getGroup()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub geom: GeomRef,
    /// `RBCollisionMesh::group` (the category the mesh was made with) or, for a mesh of the
    /// track, its geom's category bits.
    pub group: u32,
}

/// One call of the game's collision callback (`ICollisionCallback::onCollisionCallBack`):
/// a contact point that became a contact joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    /// The body of the first geom (`None`: the static world).
    pub body_a: Option<RigidBody>,
    /// The first geom's shape object (`None` for a box: boxes have no user data).
    pub shape_a: Option<Shape>,
    pub body_b: Option<RigidBody>,
    pub shape_b: Option<Shape>,
    pub normal: Vec3f,
    pub pos: Vec3f,
    pub depth: f32,
}

/// One contact as `dCollide` returned it, with what `PhysicsCore::onCollision` did with it
/// (for the oracles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactRecord {
    /// The pair as it was handed to the near callback.
    pub o1: GeomRef,
    pub o2: GeomRef,
    pub geom: ContactGeom,
    /// False: a box contact whose normal does not point up enough in the body's frame; no
    /// joint was made.
    pub kept: bool,
}

/// The boxes and meshes of one body (`RigidBodyODE::geoms`, `::collisionMeshes`).
#[derive(Clone, Debug, Default)]
struct BodyColliders {
    boxes: Vec<GeomId>,
    meshes: Vec<GeomId>,
}

/// Collects the pairs the broad phase finds, in the order the game's `nearCallback`
/// @ 0x1402ccc70 would be called with two geoms that are not spaces. (The narrow phase does
/// not change anything the broad phase reads, so it can run afterwards.)
#[derive(Default)]
struct PairCollector {
    pairs: Vec<(GeomRef, GeomRef)>,
    /// `broadTestCount`
    broad_tests: u32,
}

impl NearCallback for PairCollector {
    fn near(&mut self, broad: &mut BroadPhase, o1: GeomRef, o2: GeomRef) {
        if broad.is_space(o1) || broad.is_space(o2) {
            self.broad_tests += 1;
            broad.space_collide2(o1, o2, self);
            return;
        }
        self.pairs.push((o1, o2));
    }
}

impl PhysicsCore {
    /// `PhysicsCore::getDynamicSubSpace` @ 0x1402cc920: the numbered space inside the dynamic
    /// space (made on first use); number 0 is the dynamic space itself.
    pub fn get_dynamic_sub_space(&mut self, index: u32) -> GeomId {
        if index == 0 {
            return self.space_dynamic;
        }
        if let Some(&space) = self.dynamic_sub_spaces.get(&index) {
            return space;
        }
        let space = self.world.collision.simple_space_create(Some(self.space_dynamic));
        self.dynamic_sub_spaces.insert(index, space);
        space
    }

    fn colliders_mut(&mut self, body: RigidBody) -> &mut BodyColliders {
        if self.colliders.len() <= body.index as usize {
            self.colliders.resize(body.index as usize + 1, BodyColliders::default());
        }
        &mut self.colliders[body.index as usize]
    }

    /// `RigidBodyODE::addBoxCollider` @ 0x1402cddb0: a box of `size` with its middle at
    /// `centre` in the body's frame.
    pub fn add_box_collider(&mut self, body: RigidBody, centre: &Vec3f, size: &Vec3f, category: u32, mask: u32, space: u32) -> GeomId {
        let sub = self.get_dynamic_sub_space(space);
        let geom = self.world.collision.create_box(Some(sub), size.x, size.y, size.z);
        self.world.geom_set_body(geom, body.id);
        self.world.collision.geom_set_offset_position(geom, centre.x, centre.y, centre.z);
        // dGeomSetRotation(geom, dBodyGetRotation(body)): for a geom with an offset this
        // re-seats the BODY (rotation through its quaternion, position from the geom's)
        let r = self.world.body(body.id).r;
        self.world.geom_set_rotation(geom, &r);
        self.colliders_mut(body).boxes.push(geom);
        self.world.collision.set_collide_bits(geom, mask);
        self.world.collision.set_category_bits(geom, category);
        geom
    }

    /// `RigidBodyODE::addMeshCollider` @ 0x1402ce080: a triangle mesh on the body, placed by
    /// `matrix` (rotation and translation) in the body's frame.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh_collider(
        &mut self,
        body: RigidBody,
        vertices: Vec<[f32; 3]>,
        indices: Vec<u16>,
        matrix: &Mat44f,
        category: u32,
        mask: u32,
        space: u32,
    ) -> GeomId {
        let data = Arc::new(TriMeshData::build(vertices, indices));
        let geom = self.world.collision.create_tri_mesh(None, data);
        let sub = self.get_dynamic_sub_space(space);
        self.world.collision.space_add(sub, geom);
        self.world.geom_set_body(geom, body.id);
        // the user data: an RBCollisionMesh { group = category, mask }
        self.world.collision.geom_mut(geom).data = MESH_DATA | category as u64;
        let m = &matrix.m;
        let r = [
            m[0][0], m[1][0], m[2][0], 0.0, //
            m[0][1], m[1][1], m[2][1], 0.0, //
            m[0][2], m[1][2], m[2][2], 0.0,
        ];
        self.world.collision.geom_set_offset_rotation(geom, &r);
        self.world.collision.geom_set_offset_position(geom, m[3][0], m[3][1], m[3][2]);
        self.world.collision.set_collide_bits(geom, mask);
        self.world.collision.set_category_bits(geom, category);
        self.colliders_mut(body).meshes.push(geom);
        geom
    }

    /// The body's boxes in the order they were added.
    pub fn box_colliders(&self, body: RigidBody) -> &[GeomId] {
        self.colliders.get(body.index as usize).map_or(&[], |c| &c.boxes)
    }

    /// The body's meshes in the order they were added.
    pub fn mesh_colliders(&self, body: RigidBody) -> &[GeomId] {
        self.colliders.get(body.index as usize).map_or(&[], |c| &c.meshes)
    }

    /// `RigidBodyODE::setMeshCollideMask` @ 0x1402ce9c0. (The game reads the mesh unchecked;
    /// a body without that mesh is left alone here.)
    pub fn set_mesh_collide_mask(&mut self, body: RigidBody, index: usize, mask: u32) {
        if let Some(&geom) = self.colliders.get(body.index as usize).and_then(|c| c.meshes.get(index)) {
            self.world.collision.set_collide_bits(geom, mask);
        }
    }

    /// `RigidBodyODE::getMeshCollideMask` @ 0x1402ce5d0.
    pub fn get_mesh_collide_mask(&self, body: RigidBody, index: usize) -> Option<u32> {
        let geom = *self.colliders.get(body.index as usize)?.meshes.get(index)?;
        Some(self.world.collision.geom(geom).collide_bits)
    }

    /// `PhysicsCore::resetCollisions` @ 0x1402cd570: both contact groups are emptied.
    pub fn reset_collisions(&mut self) {
        self.world.joint_group_empty(self.contact_group_dynamic);
        self.world.joint_group_empty(self.contact_group);
    }

    /// `PhysicsCore::setNoCollisionSteps` @ 0x1402cd660: the next `n` steps look for no
    /// contacts (the contacts that exist stay).
    pub fn set_no_collision_steps(&mut self, n: i32) {
        self.no_collision_counter = n;
    }

    /// For the oracles: throws away every contact joint and makes these instead, in this
    /// order (the oldest first), each attached to its two bodies as given. A step that
    /// follows with [`PhysicsCore::world_step`] then solves somebody else's contacts.
    pub fn set_contacts(&mut self, contacts: &[(Contact, Option<BodyId>, Option<BodyId>)]) {
        self.reset_collisions();
        for (contact, body1, body2) in contacts {
            let joint = self.world.joint_create_contact(Some(self.contact_group), contact);
            self.world.joint_attach(joint, *body1, *body2);
        }
    }

    /// The contact joints that exist now, newest first (the order of the world's joint list).
    pub fn contact_joints(&self) -> Vec<JointId> {
        let mut out = Vec::new();
        let mut j = self.world.first_joint;
        while let Some(id) = j {
            let joint = self.world.joint(id);
            if matches!(joint.kind, JointKind::Contact { .. }) {
                out.push(id);
            }
            j = joint.next;
        }
        out
    }

    fn rigid_body_of(&self, body: Option<BodyId>) -> Option<RigidBody> {
        let id = body?;
        let index = self.bodies.iter().position(|&b| b == id)?;
        Some(RigidBody { id, index: index as u32 })
    }

    fn shape_of(&self, g: GeomRef, statics: Option<&StaticWorld>) -> Option<Shape> {
        match g {
            GeomRef::Dyn(id) => {
                let data = self.world.collision.geom(id).data;
                (data & MESH_DATA != 0).then_some(Shape { geom: g, group: data as u32 })
            }
            // CollisionMeshODE::getGroup @ 0x1402ced70: dGeomGetCategoryBits
            GeomRef::StaticMesh(i) => Some(Shape { geom: g, group: statics?.meshes[i as usize].category_bits }),
            _ => None,
        }
    }

    /// The first half of `PhysicsCore::step` @ 0x1402cd690: `collisionStep` @ 0x1402cbf90
    /// unless collisions are switched off for some more steps. Returns the calls of the
    /// game's collision callback in order (they happen before `dWorldStep`).
    ///
    /// Even frames test the dynamic space against itself (car against car), odd frames the
    /// dynamic space against the static one (car against track); each frame empties and
    /// refills its own group of contact joints, so a contact lives for two steps.
    pub fn collision_step(&mut self, statics: Option<&StaticWorld>) -> Vec<CollisionEvent> {
        let mut events = Vec::new();
        if self.no_collision_counter != 0 {
            self.no_collision_counter -= 1;
            return events;
        }
        let mut collector = PairCollector::default();
        if self.current_frame & 1 != 0 {
            self.world.joint_group_empty(self.contact_group_dynamic);
            self.current_contact_group = self.contact_group_dynamic;
            if statics.is_some() {
                self.world.space_collide2(GeomRef::Dyn(self.space_dynamic), GeomRef::StaticSpace, statics, &mut collector);
            }
        } else {
            self.world.joint_group_empty(self.contact_group);
            self.current_contact_group = self.contact_group;
            self.world.space_collide(GeomRef::Dyn(self.space_dynamic), statics, &mut collector);
        }
        self.current_frame = self.current_frame.wrapping_add(1);

        // nearCallback @ 0x1402ccc70 for each pair of geoms
        let mut contacts: Vec<ContactGeom> = Vec::new();
        for (o1, o2) in collector.pairs {
            let i1 = self.world.geom_info(o1, statics);
            let i2 = self.world.geom_info(o2, statics);
            // both directions must match
            if i1.category_bits & i2.collide_bits == 0 {
                continue;
            }
            if i2.category_bits & i1.collide_bits == 0 {
                continue;
            }
            // room for 4 contacts between two bodies, for 32 against the static world
            let flags = if i1.body.is_some() && i2.body.is_some() { 4 } else { 0x20 };
            contacts.clear();
            let n = self.world.collide(o1, o2, statics, flags, &mut contacts);
            if n != 0 {
                self.on_collision(statics, &contacts, o1, o2, &i1, &i2, &mut events);
            }
        }
        events
    }

    /// `PhysicsCore::onCollision` @ 0x1402ccda0: every contact of one pair of geoms becomes
    /// a contact joint with the game's material, except a box contact that does not come
    /// from below.
    #[allow(clippy::too_many_arguments)]
    fn on_collision(
        &mut self,
        statics: Option<&StaticWorld>,
        contacts: &[ContactGeom],
        o1: GeomRef,
        o2: GeomRef,
        i1: &GeomInfo,
        i2: &GeomInfo,
        events: &mut Vec<CollisionEvent>,
    ) {
        let box_mesh = (i2.class == CLASS_TRIMESH && i1.class == CLASS_BOX) || (i1.class == CLASS_TRIMESH && i2.class == CLASS_BOX);
        for c in contacts {
            let mut keep = true;
            // the default material: dContactApprox1 | dContactSoftCFM | dContactBounce
            let mut surface = SurfaceParameters {
                mode: 0x7014,
                mu: f32::from_bits(0x3e80_0000),       // 0.25
                bounce: f32::from_bits(0x3c23_d70a),   // 0.01
                soft_cfm: f32::from_bits(0x38d1_b717), // 1e-4
                // never written by the game: whatever the stack held (see the field)
                bounce_vel: self.mesh_bounce_vel,
                ..SurfaceParameters::default()
            };
            if box_mesh {
                // the body of the first geom, or of the second
                match i1.body.or(i2.body) {
                    None => {
                        // "Warning, box collision with no body attached": the contact stays
                    }
                    Some(body) => {
                        let r = self.world.body_vector_from_world(body, [c.normal[0], c.normal[1], c.normal[2]]);
                        // comiss y, 0.9 / jae keep: a NaN drops the contact
                        if !(r[1] >= f32::from_bits(0x3f66_6666)) {
                            keep = false;
                        }
                    }
                }
                // a floor box on the road: adds dContactSoftERP; a spring of 250,000 N/m with
                // a damper of 300 N s/m at this step size, no bounce
                surface.mode = 0x701c;
                surface.soft_cfm = f32::from_bits(0x3a79_a934); // 1 / 1050
                surface.soft_erp = f32::from_bits(0x3f36_db6e); // 5 / 7
                surface.mu = f32::from_bits(0x3dcc_cccd); // 0.1
                surface.bounce = 0.0;
                surface.bounce_vel = self.box_bounce_vel;
            }
            if let Some(log) = &mut self.contact_log {
                log.push(ContactRecord { o1, o2, geom: *c, kept: keep });
            }
            if !keep {
                // no joint, no callback
                continue;
            }
            let contact = Contact { surface, geom: *c, fdir1: [0.0; 3] };
            let joint = self.world.joint_create_contact(Some(self.current_contact_group), &contact);
            self.world.joint_attach(joint, i1.body, i2.body);
            events.push(CollisionEvent {
                body_a: self.rigid_body_of(i1.body),
                shape_a: self.shape_of(o1, statics),
                body_b: self.rigid_body_of(i2.body),
                shape_b: self.shape_of(o2, statics),
                normal: Vec3f::new(c.normal[0], c.normal[1], c.normal[2]),
                pos: Vec3f::new(c.pos[0], c.pos[1], c.pos[2]),
                depth: c.depth,
            });
        }
    }
}

/// Marks the user data of a mesh geom (the low 32 bits are its `RBCollisionMesh::group`).
const MESH_DATA: u64 = 1 << 32;
