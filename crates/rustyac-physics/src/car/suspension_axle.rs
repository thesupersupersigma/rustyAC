// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! [`VanillaAxle`]: AC's `SuspensionAxle` ("AXLE" in suspensions.ini, rear wheels only), one
//! side of a rigid axle.
//!
//! Both rear wheels sit on one rigid body, `Car::rigidAxle`, held to the car body by a handful
//! of rods (`[AXLE] LINK_COUNT`, `J<i>_CAR`, `J<i>_AXLE`). The car has two `SuspensionAxle`
//! objects, Left for wheel 2 and Right for wheel 3; the Left one sets up the body and creates
//! the rods, and each pushes its own spring and damper into the shared body. An axle wheel has
//! no steering, no toe, no camber and no collision damage.

use std::path::Path;

use rustyac_ode::JointId;

use super::body::{DistanceJoint, ForceSource, PhysicsCore, RigidBody};
use super::suspension::{ordered_nonzero, Damper, SuspensionBase, SuspensionModel, SuspensionStatus, SuspensionType};
use crate::data::ini::IniReader;
use crate::math::sqrtf;
use crate::vecmath::{Mat44f, Vec3f};

/// AC's `AxleBall` (0x20 bytes): one end of a link.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxleBall {
    /// `relToAxle`: the point as in the file, axle coordinates
    pub rel_to_axle: Vec3f,
    /// `relToCar`: the same point in car-body coordinates
    pub rel_to_car: Vec3f,
}

/// AC's `AxleJoint` (0x40 bytes): one link between the car body and the axle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxleJoint {
    /// `ballCar`
    pub ball_car: AxleBall,
    /// `ballAxle`
    pub ball_axle: AxleBall,
    /// `ballAxle.joint`
    pub joint: DistanceJoint,
}

/// AC's `SuspensionAxle` (0xb0 bytes).
#[derive(Clone, Debug)]
pub struct VanillaAxle {
    /// The members of `ISuspension`. `staticCamber` and `packerRange` are never written by
    /// the game's constructor (heap garbage there, 0 here); nothing in the axle reads them.
    pub base: SuspensionBase,
    /// `side`: `RigidAxleSide::Left` (wheel 2) or `Right` (wheel 3)
    pub left: bool,
    pub damper: Damper,
    /// `car->body`
    pub car_body: RigidBody,
    /// `axle` (= `Car::rigidAxle`, shared by both sides)
    pub axle: RigidBody,
    pub status: SuspensionStatus,
    /// `axleBasePos`: the design position of the axle's centre, body coordinates
    pub axle_base_pos: Vec3f,
    /// `track`: **half** the rear track, m
    pub track: f32,
    /// `referenceY`
    pub reference_y: f32,
    /// `joints`: the links (on the Left instance only)
    pub joints: Vec<AxleJoint>,
    /// `leafSpringK`: only x is ever set (`[AXLE] LEAF_SPRING_LAT_K`)
    pub leaf_spring_k: Vec3f,
    /// `attachRelativePos`: where along the half track the spring sits
    pub attach_relative_pos: f32,
}

impl VanillaAxle {
    /// `SuspensionAxle::SuspensionAxle(Car*, RigidAxleSide, const wstring& dataPath)`
    /// @ 0x1402c6b90. It draws no `rand()` (the axle has no damage data).
    pub fn new(core: &mut PhysicsCore, car_body: RigidBody, axle: RigidBody, data_path: &Path, left: bool) -> Result<VanillaAxle, String> {
        let ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        // every per-wheel key comes from [REAR]
        let section = "REAR";
        let version = ini.get_int("HEADER", "VERSION")?;
        let track = ini.get_float(section, "TRACK")? * 0.5;
        let reference_y = ini.get_float(section, "BASEY")?;
        let wheelbase = ini.get_float("BASIC", "WHEELBASE")?;
        let cg_location = ini.get_float("BASIC", "CG_LOCATION")?;
        let mut sus = VanillaAxle {
            base: SuspensionBase { k: 0.0, ..SuspensionBase::default() },
            left,
            damper: Damper::default(),
            car_body,
            axle,
            status: SuspensionStatus::default(),
            axle_base_pos: Vec3f::new(0.0, reference_y, -(cg_location * wheelbase)),
            track,
            reference_y,
            joints: Vec::new(),
            leaf_spring_k: Vec3f::default(),
            attach_relative_pos: 1.0,
        };
        if version >= 4 {
            sus.attach_relative_pos = ini.get_float("AXLE", "ATTACH_REL_POS")?;
        }
        if left {
            let width = track * 2.0;
            // (no "0 or less becomes 20 kg" rule here)
            let mass = ini.get_float(section, "HUB_MASS")?;
            core.set_mass_box(axle, mass, width, 0.2, 0.5);
            sus.set_positions(core);
            let count = ini.get_int("AXLE", "LINK_COUNT")?;
            for i in 0..count {
                let point = |key: String| -> Result<Vec3f, String> {
                    let v = ini.get_float3("AXLE", &key)?;
                    Ok(Vec3f::new(v[0], v[1], v[2]))
                };
                // both ends are given in axle coordinates
                let on_car = point(format!("J{i}_CAR"))?;
                let on_axle = point(format!("J{i}_AXLE"))?;
                let to_car = |core: &PhysicsCore, p: &Vec3f| core.world_to_local(car_body, &core.local_to_world(axle, p));
                let ball_car = AxleBall { rel_to_axle: on_car, rel_to_car: to_car(core, &on_car) };
                let ball_axle = AxleBall { rel_to_axle: on_axle, rel_to_car: to_car(core, &on_axle) };
                let w1 = core.local_to_world(car_body, &ball_car.rel_to_car);
                let w2 = core.local_to_world(car_body, &ball_axle.rel_to_car);
                let joint = core.create_distance_joint(car_body, axle, &w1, &w2);
                sus.joints.push(AxleJoint { ball_car, ball_axle, joint });
            }
        }
        let base = &mut sus.base;
        base.bump_stop_up = ini.get_float(section, "BUMPSTOP_UP")?;
        base.bump_stop_dn = -ini.get_float(section, "BUMPSTOP_DN")?;
        base.rod_length = ini.get_float(section, "ROD_LENGTH")?;
        base.toe_out_linear = ini.get_float(section, "TOE_OUT")?;
        base.k = ini.get_float(section, "SPRING_RATE")?;
        base.progressive_k = ini.get_float(section, "PROGRESSIVE_SPRING_RATE")?;
        sus.damper = Damper::load(&ini, section)?;
        // (read, but the axle's bump stops use a fixed 500000 N/m)
        base.bump_stop_rate = ini.get_float(section, "BUMP_STOP_RATE")?;
        if !ordered_nonzero(base.bump_stop_rate) {
            base.bump_stop_rate = 500000.0;
        }
        if version >= 3 {
            sus.leaf_spring_k.x = ini.get_float("AXLE", "LEAF_SPRING_LAT_K")?;
        }
        // (`hasSection("RIGIDITY")`, thrown away)
        let base_cfm = sus.base.base_cfm;
        sus.set_erp_cfm(core, 0.3, base_cfm);
        Ok(sus)
    }

    /// `SuspensionAxle::setPositions` @ 0x1402c86e0: the Left instance puts the axle body at
    /// its design pose; the Right one does nothing.
    fn set_positions(&mut self, core: &mut PhysicsCore) {
        if !self.left {
            return;
        }
        let body_matrix = core.get_world_matrix(self.car_body);
        let position = core.local_to_world(self.car_body, &self.axle_base_pos);
        core.set_rotation(self.axle, &body_matrix);
        core.set_position(self.axle, &position);
    }

    /// `+track` for the Left side, `-track` for the Right.
    fn side_offset(&self) -> f32 {
        if self.left {
            self.track
        } else {
            -self.track
        }
    }
}

impl SuspensionModel for VanillaAxle {
    /// `SuspensionAxle::getHubWorldMatrix` @ 0x1402c8440: the axle's matrix moved to the wheel
    /// end along the axle's own x axis. No camber, no toe.
    fn get_hub_world_matrix(&self, core: &PhysicsCore) -> Mat44f {
        let mut m = core.get_world_matrix(self.axle);
        let s = self.side_offset();
        m.m[3][0] = m.m[0][0] * s + m.m[3][0];
        m.m[3][2] = m.m[0][2] * s + m.m[3][2];
        m.m[3][1] = m.m[0][1] * s + m.m[3][1];
        m
    }

    /// @ 0x1402c84e0.
    fn get_point_velocity(&self, core: &PhysicsCore, p: &Vec3f) -> Vec3f {
        core.get_point_velocity(self.axle, p)
    }

    /// `SuspensionAxle::addForceAtPos` @ 0x1402c7f00: a tail jump to the axle body; both flags
    /// are ignored.
    fn add_force_at_pos(&mut self, core: &mut PhysicsCore, force: &Vec3f, pos: &Vec3f, _driven: bool, _add_to_steer_torque: bool) {
        core.add_force_at_pos(self.axle, force, pos);
    }

    /// `SuspensionAxle::addTorque` @ 0x1402c7fa0.
    fn add_torque(&mut self, core: &mut PhysicsCore, torque: &Vec3f) {
        core.add_torque(self.axle, torque);
    }

    /// The shared empty function @ 0x140017870: an axle wheel does not steer.
    fn set_steer_length_offset(&mut self, _core: &mut PhysicsCore, _offset: f32) {}

    /// The shared `return 0.0f` @ 0x14044f230.
    fn get_steer_torque(&self) -> f32 {
        0.0
    }

    /// @ 0x1402c8420.
    fn get_hub_angular_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_angular_velocity(self.axle)
    }

    /// `SuspensionAxle::attach` @ 0x1402c7fb0 (a jump to `setPositions`).
    fn attach(&mut self, core: &mut PhysicsCore) {
        self.set_positions(core);
    }

    fn get_status(&self) -> SuspensionStatus {
        self.status
    }

    /// `SuspensionAxle::getBasePosition` @ 0x1402c7fc0.
    fn get_base_position(&self) -> Vec3f {
        Vec3f::new(self.side_offset(), self.axle_base_pos.y, self.axle_base_pos.z)
    }

    fn damper(&self) -> &Damper {
        &self.damper
    }

    fn damper_mut(&mut self) -> &mut Damper {
        &mut self.damper
    }

    /// The shared `return 0.0f` (not the `packerRange` member).
    fn get_packer_range(&self) -> f32 {
        0.0
    }

    /// The shared empty function.
    fn set_damage(&mut self, _amount: f32) {}

    /// The shared empty function.
    fn reset_damage(&mut self) {}

    /// The shared `return 0.0f`.
    fn get_damage(&self) -> f32 {
        0.0
    }

    fn damage_amount(&self) -> f32 {
        0.0
    }

    fn set_damage_amount(&mut self, _amount: f32) {}

    /// `SuspensionAxle::getMass` @ 0x1402c84c0: half the axle.
    fn get_mass(&self, core: &PhysicsCore) -> f32 {
        core.get_mass(self.axle) * 0.5
    }

    /// `SuspensionAxle::stop` @ 0x1402c8db0 (once per side, so twice per car).
    fn stop(&mut self, core: &mut PhysicsCore) {
        core.stop(self.axle);
    }

    /// `SuspensionAxle::getVelocity` @ 0x1402c85a0: the velocity of the wheel end of the axle.
    fn get_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_local_point_velocity(self.axle, &Vec3f::new(self.side_offset(), 0.0, 0.0))
    }

    /// `SuspensionAxle::getSteerBasis` @ 0x1402c8510: the game prints a critical error and
    /// raises an exception. Nothing calls it for an axle.
    fn get_steer_basis(&self, _core: &PhysicsCore) -> (Vec3f, Vec3f) {
        panic!("Kunos Simulazioni: CRITICAL ERROR\nSuspensionAxle::getSteerBasis not implemented");
    }

    /// `SuspensionAxle::step` @ 0x1402c8770.
    ///
    /// The spring runs from the seat on the axle (`attachRelativePos` of the way out to the
    /// wheel) to a point 0.2 m above the seat's design position on the body; its rest length
    /// is 0.2 m. The damper speed is measured at the wheel end, its force acts at the seat.
    /// The bump stops are a fixed 500000 N/m and there is no packer.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step(&mut self, core: &mut PhysicsCore, _dt: f32) {
        let s = self.side_offset();
        let a = self.attach_relative_pos;
        let m = core.get_world_matrix(self.axle);
        let seat = Vec3f::new((m.m[0][0] * s) * a + m.m[3][0], (m.m[0][1] * s) * a + m.m[3][1], (m.m[0][2] * s) * a + m.m[3][2]);
        let seat_local = core.world_to_local(self.car_body, &seat);
        let base_position = self.get_base_position();
        let top = core.local_to_world(self.car_body, &Vec3f::new(base_position.x * a, base_position.y + 0.2, base_position.z));
        let d = Vec3f::new(top.x - seat.x, top.y - seat.y, top.z - seat.z);
        let squared = (d.y * d.y + d.x * d.x) + d.z * d.z;
        let length = sqrtf(squared);
        let mut dir = d;
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            dir = Vec3f::new(d.x * inverse, d.y * inverse, d.z * inverse);
        }
        let length = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };
        let base = self.base;
        let travel = (0.2 - length) + base.rod_length;
        self.status.travel = travel;

        let previous = core.source;
        // spring: it only pushes
        core.source = ForceSource::Spring;
        let force = -((travel * base.progressive_k + base.k) * travel);
        if !(force >= 0.0) {
            self.add_force_at_pos(core, &Vec3f::new(dir.x * force, dir.y * force, dir.z * force), &seat, false, false);
            let n = -force;
            core.add_force_at_pos(self.car_body, &Vec3f::new(dir.x * n, dir.y * n, dir.z * n), &top);
        }
        // the sideways leaf spring that keeps the axle centred under the body
        if ordered_nonzero(self.leaf_spring_k.x) {
            let base_position = self.get_base_position();
            let force = (seat_local.x - a * base_position.x) * self.leaf_spring_k.x;
            let body_matrix = core.get_world_matrix(self.car_body);
            let n = -force;
            let on_axle = Vec3f::new(body_matrix.m[0][0] * n, body_matrix.m[0][1] * n, body_matrix.m[0][2] * n);
            self.add_force_at_pos(core, &on_axle, &seat, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(force, 0.0, 0.0), &seat_local);
        }

        // bump stops
        core.source = ForceSource::Bumpstop;
        let dy = seat_local.y - self.reference_y;
        if ordered_nonzero(base.bump_stop_up) && dy > base.bump_stop_up && ordered_nonzero(base.k) {
            let force = (dy - base.bump_stop_up) * 500000.0;
            let body_matrix = core.get_world_matrix(self.car_body);
            let n = -force;
            let on_axle = Vec3f::new(body_matrix.m[1][0] * n, body_matrix.m[1][1] * n, body_matrix.m[1][2] * n);
            self.add_force_at_pos(core, &on_axle, &seat, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &seat_local);
        }
        if ordered_nonzero(base.bump_stop_dn) && !(dy >= base.bump_stop_dn) && ordered_nonzero(base.k) {
            let force = (dy - base.bump_stop_dn) * 500000.0;
            let body_matrix = core.get_world_matrix(self.car_body);
            let n = -force;
            let on_axle = Vec3f::new(body_matrix.m[1][0] * n, body_matrix.m[1][1] * n, body_matrix.m[1][2] * n);
            self.add_force_at_pos(core, &on_axle, &seat, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &seat_local);
        }

        // damper
        core.source = ForceSource::Damper;
        let v1 = self.get_velocity(core);
        let v2 = core.get_point_velocity(self.car_body, &top);
        let speed = ((v1.x - v2.x) * dir.x + (v1.y - v2.y) * dir.y) + (v1.z - v2.z) * dir.z;
        self.status.damper_speed_ms = speed;
        let f = self.damper.get_force(speed);
        let on_axle = Vec3f::new(dir.x * f, dir.y * f, dir.z * f);
        self.add_force_at_pos(core, &on_axle, &seat, false, false);
        core.add_force_at_pos(self.car_body, &Vec3f::new(-on_axle.x, -on_axle.y, -on_axle.z), &top);
        core.source = previous;
    }

    /// `SuspensionAxle::setERPCFM` @ 0x1402c8680: every link (none on the Right instance).
    fn set_erp_cfm(&mut self, core: &mut PhysicsCore, erp: f32, cfm: f32) {
        for link in &self.joints {
            core.joint_set_erp_cfm(link.joint.id, erp, cfm);
        }
    }

    /// `SuspensionAxle::addLocalForceAndTorque` @ 0x1402c7f10: the force acts at the centre of
    /// the axle body, not at the wheel.
    fn add_local_force_and_torque(&mut self, core: &mut PhysicsCore, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        core.add_force_at_local_pos(self.axle, force, &Vec3f::default());
        core.add_torque(self.axle, torque);
        let d = drive_torque;
        if ordered_nonzero(d.x) || ordered_nonzero(d.y) || ordered_nonzero(d.z) {
            core.add_torque(self.car_body, d);
        }
    }

    fn kind(&self) -> SuspensionType {
        SuspensionType::Axle
    }

    fn base(&self) -> &SuspensionBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut SuspensionBase {
        &mut self.base
    }

    /// The axle body (both sides share it).
    fn hub(&self) -> RigidBody {
        self.axle
    }

    fn joints(&self) -> Vec<JointId> {
        self.joints.iter().map(|link| link.joint.id).collect()
    }

    fn joint_names(&self) -> Vec<String> {
        (0..self.joints.len()).map(|i| format!("link{i}")).collect()
    }

    /// None: the axle body is `Car::rigidAxle`, created by the car before the wheels.
    fn bodies(&self) -> Vec<RigidBody> {
        Vec::new()
    }

    fn restore_step_state(&mut self, status: SuspensionStatus, _steer_torque: f32) {
        self.status = status;
    }
}
