//! What the tyre talks to: the suspension/hub, the ground ray cast and the car it sits on.
//!
//! Method names are AC's virtual function names from `acs.pdb` in snake_case, parameter
//! names are the PDB's too. Vtable offsets are those of build 0x5a55e7a8.

use crate::vecmath::{Mat44f, Vec3f};

/// AC's `ISuspension`: the wheel hub as the tyre sees it. Only the six slots `Tyre` calls
/// are here; the full vtable (25 slots) is:
///
/// | slot | offset | name | | slot | offset | name |
/// |---|---|---|---|---|---|---|
/// | 0 | +0x00 | destructor | | 13 | +0x68 | `getPackerRange` |
/// | **1** | **+0x08** | **`getHubWorldMatrix`** | | 14 | +0x70 | `getDebugLines` |
/// | **2** | **+0x10** | **`getPointVelocity`** | | 15 | +0x78 | `setDamage` |
/// | **3** | **+0x18** | **`addForceAtPos`** | | 16 | +0x80 | `resetDamage` |
/// | **4** | **+0x20** | **`addTorque`** | | 17 | +0x88 | `getDamage` |
/// | 5 | +0x28 | `setSteerLengthOffset` | | 18 | +0x90 | `getMass` |
/// | 6 | +0x30 | `getSteerTorque` | | 19 | +0x98 | `stop` |
/// | **7** | **+0x38** | **`getHubAngularVelocity`** | | 20 | +0xa0 | `getVelocity` |
/// | 8 | +0x40 | `attach` | | 21 | +0xa8 | `getSteerBasis` |
/// | 9 | +0x48 | `getStatus` | | 22 | +0xb0 | `step` |
/// | 10 | +0x50 | `getBasePosition` | | 23 | +0xb8 | `setERPCFM` |
/// | 11 | +0x58 | `getK` | | **24** | **+0xc0** | **`addLocalForceAndTorque`** |
/// | 12 | +0x60 | `getDamper` | | | | |
pub trait Suspension {
    /// Slot 1 (+0x08) `getHubWorldMatrix`: the hub's world transform. Row 0 is the wheel's
    /// axle (x), row 1 its up axis (y), row 2 its z axis, row 3 the wheel centre.
    fn get_hub_world_matrix(&mut self) -> Mat44f;

    /// Slot 2 (+0x10) `getPointVelocity(p)`: world velocity of the hub at world point `p`.
    fn get_point_velocity(&mut self, p: &Vec3f) -> Vec3f;

    /// Slot 3 (+0x18) `addForceAtPos(force, pos, driven, addToSteerTorque)`: a world force
    /// at a world point. The tyre passes its own `driven` flag; `addToSteerTorque` is false
    /// for the vertical load and true for the grip force.
    fn add_force_at_pos(
        &mut self,
        force: &Vec3f,
        pos: &Vec3f,
        driven: bool,
        add_to_steer_torque: bool,
    );

    /// Slot 4 (+0x20) `addTorque(torque)`: a world torque on the hub (aligning torque, and
    /// the brake reaction in the `reactionTorques` mode).
    fn add_torque(&mut self, torque: &Vec3f);

    /// Slot 7 (+0x38) `getHubAngularVelocity`: world angular velocity of the hub, rad/s.
    fn get_hub_angular_velocity(&mut self) -> Vec3f;

    /// Slot 24 (+0xc0) `addLocalForceAndTorque(force, torque, driveTorque)`: used instead of
    /// `addForceAtPos` for the grip force when the car's torque mode is not `original`.
    /// `force` is the world force; `torque` and `driveTorque` are world torques.
    fn add_local_force_and_torque(&mut self, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f);
}

/// AC's `SurfaceDef` (0xc8 bytes), the members the tyre reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceDef {
    /// `gripMod`: grip multiplier of the surface (1 = tarmac).
    pub grip_mod: f32,
    /// `dirtAdditiveK`: how fast the tyre picks up dirt here (0 = it cleans itself).
    pub dirt_additive_k: f32,
    /// `sinHeight`: amplitude of the procedural bumps, m (0 = none).
    pub sin_height: f32,
    /// `sinLength`: spatial frequency of those bumps, rad/m.
    pub sin_length: f32,
    /// Drag of the surface on the car body (sand, grass); 0 = none.
    pub damping: f32,
    /// Non-zero adds a fixed three-wave roughness to the ground height.
    pub granularity: f32,
}

impl Default for SurfaceDef {
    /// The values `TyreTester::TyreTester` @ 0x14044ee40 gives its surface.
    fn default() -> SurfaceDef {
        SurfaceDef {
            grip_mod: 1.0,
            dirt_additive_k: 0.0,
            sin_height: 0.0,
            sin_length: 0.0,
            damping: 0.0,
            granularity: 0.0,
        }
    }
}

/// AC's `RayCastResult` (0x30 bytes) without the raw collision-object pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayCastResult {
    /// `surfaceDef`
    pub surface_def: SurfaceDef,
    /// `pos`: where the ray hit, world.
    pub pos: Vec3f,
    /// Unit normal of the ground at the hit, world.
    pub normal: Vec3f,
}

/// AC's `IRayTrackCollisionProvider`: the ground under the wheel.
///
/// | slot | offset | name |
/// |---|---|---|
/// | 0 | +0x00 | destructor |
/// | **1** | **+0x08** | **`rayCast`** |
/// | 2 | +0x10 | `rayCastWithRayCaster` |
/// | 3 | +0x18 | `createRayCaster` |
///
/// In the game `Tyre::init` asks `createRayCaster(3.0)` for a per-wheel `IRayCaster` and,
/// when it gets one, casts through that (`IRayCaster::rayCast`, slot 1) and takes the
/// surface from the hit object's user pointer; without one it calls `rayCast` directly.
/// Both give the tyre the same three things, so one method stands for both here.
pub trait RayTrackCollisionProvider {
    /// Slot 1 (+0x08) `rayCast(org, dir, result, length)`: `None` is `hasHit == false`.
    /// The tyre always casts from 2 m above the wheel centre straight down (`dir` is
    /// `(0, -1, 0)`) with `length` 2.
    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult>;
}

/// AC's `TorqueModeEX` (`Car::torqueModeEx`): how the tyre hands its forces to the hub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TorqueModeEx {
    /// `original` (0): one force at the contact point.
    #[default]
    Original,
    /// `reactionTorques` (1)
    ReactionTorques,
    /// `driveTorques` (2)
    DriveTorques,
    /// Any other stored value: the game then passes zero torques to the hub.
    Other,
}

impl TorqueModeEx {
    pub fn from_i32(value: i32) -> TorqueModeEx {
        match value {
            0 => TorqueModeEx::Original,
            1 => TorqueModeEx::ReactionTorques,
            2 => TorqueModeEx::DriveTorques,
            _ => TorqueModeEx::Other,
        }
    }
}

/// The parts of AC's `Car` (and, through `Car::ksPhysics`, of `PhysicsEngine` and `Track`)
/// that `Tyre::step` reads. In the game the tyre holds a `Car*` that may be null (the tyre
/// test bench); here that is `Option<&mut dyn TyreCar>`.
pub trait TyreCar {
    /// `car->torqueModeEx`
    fn torque_mode_ex(&self) -> TorqueModeEx;
    /// `Car::isSleeping` @ 0x1402745e0
    fn is_sleeping(&self) -> bool;
    /// `Car::getSpeed` @ 0x140272160, m/s.
    fn get_speed(&self) -> f32;
    /// `car->ksPhysics->track->dynamicGripLevel`: the track's rubbered-in grip level.
    fn dynamic_grip_level(&self) -> f32;
    /// `car->ksPhysics->tyreConsumptionRate` (the session's tyre wear multiplier).
    fn tyre_consumption_rate(&self) -> f32;
    /// `car->ksPhysics->mechanicalDamageRate`
    fn mechanical_damage_rate(&self) -> f32;
    /// `car->ksPhysics->ambientTemperature`, deg C.
    fn ambient_temperature(&self) -> f32;
    /// `car->ksPhysics->roadTemperature`, deg C.
    fn road_temperature(&self) -> f32;
    /// `car->ksPhysics->allowTyreBlankets`
    fn allow_tyre_blankets(&self) -> bool;

    /// `car->body->getVelocity()` (`IRigidBody` slot +0x78). Only used on a surface with
    /// `damping > 0`.
    fn body_get_velocity(&mut self) -> Vec3f;
    /// `car->body->getMass()` (`IRigidBody` slot +0x28).
    fn body_get_mass(&mut self) -> f32;
    /// `car->body->addForceAtLocalPos(f, p)` (`IRigidBody` slot +0xf8).
    fn body_add_force_at_local_pos(&mut self, f: &Vec3f, p: &Vec3f);
}
