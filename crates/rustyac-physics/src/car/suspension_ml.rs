// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! [`VanillaMultilink`]: AC's `SuspensionML` ("ML" in suspensions.ini), a hub held by five
//! free rods (`JOINT0_CAR` / `JOINT0_TYRE` … `JOINT4_*`; the fifth is the steering rod).
//!
//! No car of the game uses it, but the class is complete and cars made by others do. Spring
//! and damper act like the double wishbone's, along the body's up axis between the hub and
//! the wheel's design position; there are no bump stops, and the spring may pull.
//!
//! Two of the numbers its step reads, `packerRange` and `bumpStopRate`, are never set by the
//! game's constructor or loader: the game reads whatever the memory block held until a setup
//! writes them. Here both start at 0 (no packer), the only value that makes sense.

use std::path::Path;

use rustyac_ode::JointId;

use super::body::{DistanceJoint, ForceSource, PhysicsCore, RigidBody};
use super::suspension::{ordered_nonzero, sign, Damper, SusDamageDef, SuspensionBase, SuspensionModel, SuspensionStatus, SuspensionType};
use crate::data::ini::IniReader;
use crate::math::sqrtf;
use crate::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

/// AC's `MLBall` (0x18 bytes): one end of a rod.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MlBall {
    /// `relToTyre`: the point relative to the wheel centre, as in the file (mirrored for the
    /// left side)
    pub rel_to_tyre: Vec3f,
    /// `relToCar`: the same point in car-body coordinates
    pub rel_to_car: Vec3f,
}

/// AC's `MLJoint` (0x38 bytes): one rod.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MlJoint {
    /// `ballCar`
    pub ball_car: MlBall,
    /// `ballTyre`
    pub ball_tyre: MlBall,
    /// `joint`
    pub joint: DistanceJoint,
}

/// AC's `SuspensionML` (0xc8 bytes).
#[derive(Clone, Debug)]
pub struct VanillaMultilink {
    pub base: SuspensionBase,
    /// `car->body`
    pub car_body: RigidBody,
    /// `hub`
    pub hub: RigidBody,
    pub status: SuspensionStatus,
    pub damper: Damper,
    /// `index`: 0..3 = LF, RF, LR, RR.
    pub index: i32,
    /// `joints`: five rods; `[4]` is the steering rod, `[0]` and `[2]` give the steering axis.
    pub joints: Vec<MlJoint>,
    /// `hubMass`
    pub hub_mass: f32,
    /// `basePosition`: the design position of the wheel centre, body coordinates.
    pub base_position: Vec3f,
    /// `steerTorque`
    pub steer_torque: f32,
    /// `damageData`: never loaded from `[DAMAGE]`, so gain and maximum stay 0 and a multilink
    /// never bends.
    pub damage_data: SusDamageDef,
    /// `baseCarSteerPosition`
    pub base_car_steer_position: Vec3f,
}

impl VanillaMultilink {
    /// `SuspensionML::SuspensionML(Car*, int index)` @ 0x1402c8e20.
    ///
    /// `rand` is the C runtime's `rand()` result the constructor draws for the damage
    /// direction (it draws one although the damage can never be more than 0).
    pub fn new(core: &mut PhysicsCore, car_body: RigidBody, data_path: &Path, index: i32, rand: i32) -> Result<VanillaMultilink, String> {
        let draw = rand as f32 * 3.051_851e-5 * 100.0;
        let damage_direction = if draw >= 50.0 { 1.0 } else { -1.0 };
        let mut sus = VanillaMultilink {
            // the constructor writes bumpStopProgressive, baseCFM, staticCamber and rodLength;
            // everything the loader does not read stays 0 here
            base: SuspensionBase { k: 0.0, ..SuspensionBase::default() },
            car_body,
            hub: car_body,
            status: SuspensionStatus::default(),
            damper: Damper::default(),
            index,
            joints: Vec::new(),
            hub_mass: 0.0,
            base_position: Vec3f::default(),
            steer_torque: 0.0,
            damage_data: SusDamageDef {
                damage_amount: 0.0,
                damage_direction,
                min_velocity: 15.0,
                damage_gain: 0.0,
                max_damage: 0.0,
                is_debug: false,
                last_amount: 0.0,
            },
            base_car_steer_position: Vec3f::default(),
        };
        sus.load_ini(core, data_path, index)?;
        sus.base_car_steer_position = sus.joints[4].ball_car.rel_to_car;
        // (no `setSteerLengthOffset(0)` here: the static toe reaches the rod with the first
        // steering step or setup change. No ERP/CFM call either, and `setERPCFM` is empty.)
        Ok(sus)
    }

    /// `SuspensionML::loadINI` @ 0x1402c9970: it also creates the hub and the five rods.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn load_ini(&mut self, core: &mut PhysicsCore, data_path: &Path, index: i32) -> Result<(), String> {
        let ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        // (`[HEADER] VERSION` is read and not used)
        let _version = ini.get_int("HEADER", "VERSION")?;
        let wheelbase = ini.get_float("BASIC", "WHEELBASE")?;
        let cg_location = ini.get_float("BASIC", "CG_LOCATION")?;
        let front_base_y = ini.get_float("FRONT", "BASEY")?;
        let front_track = ini.get_float("FRONT", "TRACK")? * 0.5;
        let rear_base_y = ini.get_float("REAR", "BASEY")?;
        let rear_track = ini.get_float("REAR", "TRACK")? * 0.5;
        let front_z = (1.0 - cg_location) * wheelbase;
        let rear_z = -(cg_location * wheelbase);
        let ref_points = [
            Vec3f::new(front_track, front_base_y, front_z),
            Vec3f::new(-front_track, front_base_y, front_z),
            Vec3f::new(rear_track, rear_base_y, rear_z),
            Vec3f::new(-rear_track, rear_base_y, rear_z),
        ];
        let section = ["FRONT", "FRONT", "REAR", "REAR"][index as usize];
        self.base_position = ref_points[index as usize];
        // (no "0 or less becomes 20 kg" rule here)
        self.hub_mass = ini.get_float(section, "HUB_MASS")?;
        self.hub = core.create_rigid_body();
        core.set_mass_box(self.hub, self.hub_mass, 0.2, 0.6, 0.6);
        self.set_positions(core);
        for i in 0..5 {
            let point = |key: String| -> Result<Vec3f, String> {
                let v = ini.get_float3(section, &key)?;
                Ok(Vec3f::new(v[0], v[1], v[2]))
            };
            let mut on_car = point(format!("JOINT{i}_CAR"))?;
            let mut on_tyre = point(format!("JOINT{i}_TYRE"))?;
            // the file describes the wheel on the negative-x side; mirror for the other one
            if !(0.0 >= self.base_position.x) {
                on_car.x = -on_car.x;
                on_tyre.x = -on_tyre.x;
            }
            let (body, hub) = (self.car_body, self.hub);
            let to_car = |core: &PhysicsCore, p: &Vec3f| core.world_to_local(body, &core.local_to_world(hub, p));
            let ball_car = MlBall { rel_to_tyre: on_car, rel_to_car: to_car(core, &on_car) };
            let ball_tyre = MlBall { rel_to_tyre: on_tyre, rel_to_car: to_car(core, &on_tyre) };
            let w1 = core.local_to_world(body, &ball_car.rel_to_car);
            let w2 = core.local_to_world(body, &ball_tyre.rel_to_car);
            let joint = core.create_distance_joint(body, hub, &w1, &w2);
            self.joints.push(MlJoint { ball_car, ball_tyre, joint });
        }
        let base = &mut self.base;
        base.rod_length = ini.get_float(section, "ROD_LENGTH")?;
        base.toe_out_linear = ini.get_float(section, "TOE_OUT")?;
        base.k = ini.get_float(section, "SPRING_RATE")?;
        base.progressive_k = ini.get_float(section, "PROGRESSIVE_SPRING_RATE")?;
        // the damper values as they are in the file: no fall-back for a fast rate or a
        // threshold of 0 (a threshold of 0 sends every speed down the "fast" branch)
        self.damper = Damper {
            bump_slow: ini.get_float(section, "DAMP_BUMP")?,
            rebound_slow: ini.get_float(section, "DAMP_REBOUND")?,
            bump_fast: ini.get_float(section, "DAMP_FAST_BUMP")?,
            rebound_fast: ini.get_float(section, "DAMP_FAST_REBOUND")?,
            fast_threshold_bump: ini.get_float(section, "DAMP_FAST_BUMPTHRESHOLD")?,
            fast_threshold_rebound: ini.get_float(section, "DAMP_FAST_REBOUNDTHRESHOLD")?,
        };
        base.static_camber = -(ini.get_float(section, "STATIC_CAMBER")? * 0.017453);
        if index % 2 != 0 {
            base.static_camber = -base.static_camber;
        }
        Ok(())
    }

    /// `SuspensionML::setPositions` @ 0x1402ca9d0.
    fn set_positions(&mut self, core: &mut PhysicsCore) {
        let body_matrix = core.get_world_matrix(self.car_body);
        let position = core.local_to_world(self.car_body, &self.base_position);
        core.set_rotation(self.hub, &body_matrix);
        core.set_position(self.hub, &position);
    }
}

impl SuspensionModel for VanillaMultilink {
    /// `SuspensionML::getHubWorldMatrix` @ 0x1402c96c0: `rotation((0,0,1), staticCamber) x hub`.
    fn get_hub_world_matrix(&self, core: &PhysicsCore) -> Mat44f {
        let hub = core.get_world_matrix(self.hub);
        let camber = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), self.base.static_camber);
        xm_matrix_multiply(&camber, &hub)
    }

    /// @ 0x1402c97b0.
    fn get_point_velocity(&self, core: &PhysicsCore, p: &Vec3f) -> Vec3f {
        core.get_point_velocity(self.hub, p)
    }

    /// `SuspensionML::addForceAtPos` @ 0x1402c91d0. `driven` is not used.
    fn add_force_at_pos(&mut self, core: &mut PhysicsCore, force: &Vec3f, pos: &Vec3f, _driven: bool, add_to_steer_torque: bool) {
        core.add_force_at_pos(self.hub, force, pos);
        if add_to_steer_torque {
            let (centre, axis) = self.get_steer_basis(core);
            let f = force;
            let r = Vec3f::new(pos.x - centre.x, pos.y - centre.y, pos.z - centre.z);
            self.steer_torque = (((f.x * r.z - f.z * r.x) * axis.y + (f.z * r.y - f.y * r.z) * axis.x)
                + (f.y * r.x - f.x * r.y) * axis.z)
                + self.steer_torque;
        }
    }

    /// `SuspensionML::addTorque` @ 0x1402c9380.
    fn add_torque(&mut self, core: &mut PhysicsCore, torque: &Vec3f) {
        core.add_torque(self.hub, torque);
        let (_, axis) = self.get_steer_basis(core);
        self.steer_torque = ((axis.y * torque.y + axis.x * torque.x) + axis.z * torque.z) + self.steer_torque;
    }

    /// `SuspensionML::setSteerLengthOffset` @ 0x1402caa40.
    fn set_steer_length_offset(&mut self, core: &mut PhysicsCore, offset: f32) {
        let side = sign(self.base_position.x) * self.base.toe_out_linear;
        let x = ((self.damage_data.damage_direction * self.damage_data.damage_amount + offset) + side)
            + self.base_car_steer_position.x;
        let rod = &mut self.joints[4];
        rod.ball_car.rel_to_car = Vec3f::new(x, self.base_car_steer_position.y, self.base_car_steer_position.z);
        core.reseat_distance_joint_local(&rod.joint, &rod.ball_car.rel_to_car, &rod.ball_tyre.rel_to_tyre);
    }

    /// @ 0x1402c9940.
    fn get_steer_torque(&self) -> f32 {
        self.steer_torque
    }

    /// @ 0x1402c96a0.
    fn get_hub_angular_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_angular_velocity(self.hub)
    }

    /// `SuspensionML::attach` @ 0x1402c9420: the hub back to its design pose.
    fn attach(&mut self, core: &mut PhysicsCore) {
        self.set_positions(core);
    }

    fn get_status(&self) -> SuspensionStatus {
        self.status
    }

    /// @ 0x1402c9430.
    fn get_base_position(&self) -> Vec3f {
        self.base_position
    }

    fn damper(&self) -> &Damper {
        &self.damper
    }

    fn damper_mut(&mut self) -> &mut Damper {
        &mut self.damper
    }

    /// `SuspensionML::setDamage` @ 0x1402ca980.
    #[allow(clippy::double_comparisons)]
    fn set_damage(&mut self, amount: f32) {
        let d = &mut self.damage_data;
        if amount > d.min_velocity {
            let damage = (amount - d.min_velocity) * d.damage_gain;
            d.damage_amount = if d.max_damage >= damage { damage } else { d.max_damage };
        }
        if d.is_debug && (amount < d.last_amount || amount > d.last_amount) {
            d.last_amount = amount;
        }
    }

    /// `SuspensionML::resetDamage` @ 0x1402ca970.
    fn reset_damage(&mut self) {
        self.damage_data.damage_amount = 0.0;
    }

    fn damage_amount(&self) -> f32 {
        self.damage_data.damage_amount
    }

    fn set_damage_amount(&mut self, amount: f32) {
        self.damage_data.damage_amount = amount;
    }

    /// `SuspensionML::getDamage` @ 0x1402c9450.
    fn get_damage(&self) -> f32 {
        if ordered_nonzero(self.damage_data.max_damage) {
            self.damage_data.damage_amount / self.damage_data.max_damage
        } else {
            0.0
        }
    }

    /// @ 0x1402c97a0.
    fn get_mass(&self, core: &PhysicsCore) -> f32 {
        core.get_mass(self.hub)
    }

    /// @ 0x1402cad90.
    fn stop(&mut self, core: &mut PhysicsCore) {
        core.stop(self.hub);
    }

    /// @ 0x1402c9950.
    fn get_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_velocity(self.hub)
    }

    /// `SuspensionML::getSteerBasis` @ 0x1402c97e0: the line through the hub ends of rods 0
    /// and 2; `centre` is their midpoint.
    fn get_steer_basis(&self, core: &PhysicsCore) -> (Vec3f, Vec3f) {
        let a = core.local_to_world(self.hub, &self.joints[0].ball_tyre.rel_to_tyre);
        let b = core.local_to_world(self.hub, &self.joints[2].ball_tyre.rel_to_tyre);
        let mut axis = Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z);
        let length = sqrtf((axis.y * axis.y + axis.x * axis.x) + axis.z * axis.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            axis = Vec3f::new(inverse * axis.x, axis.y * inverse, axis.z * inverse);
        }
        let centre = Vec3f::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5);
        (centre, axis)
    }

    /// `SuspensionML::step` @ 0x1402cab00: spring (with the packer), then damper. The spring
    /// force is applied whatever its sign, and there are no bump stops.
    fn step(&mut self, core: &mut PhysicsCore, _dt: f32) {
        self.steer_torque = 0.0;
        let hub_pos = core.get_position(self.hub);
        let hub_local = core.world_to_local(self.car_body, &hub_pos);
        let body_matrix = core.get_world_matrix(self.car_body);
        let up = Vec3f::new(body_matrix.m[1][0], body_matrix.m[1][1], body_matrix.m[1][2]);
        let base = self.base;
        let travel = (hub_local.y - self.base_position.y) + base.rod_length;
        self.status.travel = travel;

        let previous = core.source;
        core.source = ForceSource::Spring;
        let mut force = (travel * base.progressive_k + base.k) * travel;
        if ordered_nonzero(base.packer_range) && travel > base.packer_range && ordered_nonzero(base.k) {
            force += (travel - base.packer_range) * base.bump_stop_rate;
        }
        let n = -force;
        let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
        self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
        core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &self.base_position);

        core.source = ForceSource::Damper;
        let hub_velocity = core.get_velocity(self.hub);
        let point_velocity = core.get_local_point_velocity(self.car_body, &self.base_position);
        let speed = ((hub_velocity.x - point_velocity.x) * up.x + (hub_velocity.y - point_velocity.y) * up.y)
            + (hub_velocity.z - point_velocity.z) * up.z;
        let f = self.damper.get_force(speed);
        self.status.damper_speed_ms = speed;
        let on_hub = Vec3f::new(up.x * f, up.y * f, up.z * f);
        self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
        core.add_force_at_local_pos(self.car_body, &Vec3f::new(-on_hub.x, -on_hub.y, -on_hub.z), &self.base_position);
        core.source = previous;
    }

    /// The shared empty function @ 0x140017870: the rods keep the values they were made with.
    fn set_erp_cfm(&mut self, _core: &mut PhysicsCore, _erp: f32, _cfm: f32) {}

    /// `SuspensionML::addLocalForceAndTorque` @ 0x1402c92f0: nothing goes into the steer
    /// torque here.
    fn add_local_force_and_torque(&mut self, core: &mut PhysicsCore, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        core.add_force_at_local_pos(self.hub, force, &Vec3f::default());
        core.add_torque(self.hub, torque);
        let d = drive_torque;
        if ordered_nonzero(d.x) || ordered_nonzero(d.y) || ordered_nonzero(d.z) {
            core.add_torque(self.car_body, d);
        }
    }

    fn kind(&self) -> SuspensionType {
        SuspensionType::Multilink
    }

    fn base(&self) -> &SuspensionBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut SuspensionBase {
        &mut self.base
    }

    fn hub(&self) -> RigidBody {
        self.hub
    }

    fn joints(&self) -> Vec<JointId> {
        self.joints.iter().map(|rod| rod.joint.id).collect()
    }

    fn joint_names(&self) -> Vec<String> {
        (0..self.joints.len()).map(|i| format!("joint{i}")).collect()
    }

    fn restore_step_state(&mut self, status: SuspensionStatus, steer_torque: f32) {
        self.status = status;
        self.steer_torque = steer_torque;
    }
}
