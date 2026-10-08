// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! [`VanillaStrut`]: AC's `SuspensionStrut` ("STRUT" in suspensions.ini), a MacPherson strut.
//!
//! A strut corner is two rigid bodies. The hub is held by two lower-arm rods and the steering
//! rod (fixed lengths) and slides along the strut body; the small strut body hangs from the car
//! body by a ball joint at the top mount. Spring and damper act along the line from the top
//! mount to the strut's foot on the hub, so the strut's angle gives a real motion ratio.

use std::path::Path;

use rustyac_ode::JointId;

use super::body::{DistanceJoint, ForceSource, PhysicsCore, RigidBody};
use super::suspension::{add, ordered_nonzero, sign, Damper, SusDamageDef, SuspensionBase, SuspensionModel, SuspensionStatus, SuspensionType};
use crate::data::ini::IniReader;
use crate::math::sqrtf;
use crate::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

/// AC's `SStrutSuspensionData` (0x70 bytes): the pick-up points of a strut corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrutSuspensionData {
    /// `carStrut`: the top mount
    pub car_strut: Vec3f,
    /// `tyreStrut`: the strut's foot on the hub
    pub tyre_strut: Vec3f,
    /// `carBottomWB_F`
    pub car_bottom_wb_f: Vec3f,
    /// `carBottomWB_R`
    pub car_bottom_wb_r: Vec3f,
    /// `tyreBottomWB`
    pub tyre_bottom_wb: Vec3f,
    /// `carSteer`
    pub car_steer: Vec3f,
    /// `tyreSteer`
    pub tyre_steer: Vec3f,
    /// `refPoint`: the design position of the wheel centre, body coordinates.
    pub ref_point: Vec3f,
    /// `hubMass`, kg (hub and strut body together)
    pub hub_mass: f32,
    /// `hubInertiaBox`: never set by a file; zero selects the box (0.2, 0.6, 0.6) m.
    pub hub_inertia_box: Vec3f,
}

/// AC's `SuspensionStrut` (0x1e8 bytes): the strut suspension of one wheel.
#[derive(Clone, Debug)]
pub struct VanillaStrut {
    pub base: SuspensionBase,
    /// `carBody`
    pub car_body: RigidBody,
    /// `hub`: 80 % of `HUB_MASS`
    pub hub: RigidBody,
    /// `strutBody`: 20 % of `HUB_MASS`
    pub strut_body: RigidBody,
    /// `basePosition`
    pub base_position: Vec3f,
    /// `joints[0..3]`: lower arm rear, lower arm front, steering rod.
    pub rods: Vec<DistanceJoint>,
    /// `joints[3]`: strut body to hub along the strut.
    pub slider: Option<JointId>,
    /// `joints[4]`: car body to strut body at the top mount.
    pub ball: Option<JointId>,
    /// `dataRelToWheel`: the points as in the file (after rim offset and mirroring).
    pub data_rel_to_wheel: StrutSuspensionData,
    /// `dataRelToBody`: the points in body coordinates.
    pub data_rel_to_body: StrutSuspensionData,
    pub damper: Damper,
    pub status: SuspensionStatus,
    /// `index`: 0..3 = LF, RF, LR, RR.
    pub index: i32,
    /// `steerLinkBaseLength`
    pub steer_link_base_length: f32,
    /// `steerTorque`
    pub steer_torque: f32,
    /// `baseCarSteerPosition`
    pub base_car_steer_position: Vec3f,
    /// `strutBaseLength`: top mount to foot at the design position, m
    pub strut_base_length: f32,
    /// `strutBodyLength`: 0.2 m
    pub strut_body_length: f32,
    /// `damageData` (`SusStrutDamageDef`, the same seven members as the double wishbone's)
    pub damage_data: SusDamageDef,
}

impl VanillaStrut {
    /// `SuspensionStrut::SuspensionStrut(Car*, int index)` @ 0x1402c38d0.
    ///
    /// `rand` is the C runtime's `rand()` result the constructor draws for the damage
    /// direction (0..=0x7fff). `data_path` is the car's data folder.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn new(core: &mut PhysicsCore, car_body: RigidBody, data_path: &Path, index: i32, rand: i32) -> Result<VanillaStrut, String> {
        let draw = rand as f32 * 3.051_851e-5 * 100.0;
        let damage_direction = if draw >= 50.0 { 1.0 } else { -1.0 };
        let mut sus = VanillaStrut {
            base: SuspensionBase::default(),
            car_body,
            hub: car_body,
            strut_body: car_body,
            base_position: Vec3f::default(),
            rods: Vec::new(),
            slider: None,
            ball: None,
            data_rel_to_wheel: StrutSuspensionData::default(),
            data_rel_to_body: StrutSuspensionData::default(),
            damper: Damper::default(),
            status: SuspensionStatus::default(),
            index,
            steer_link_base_length: 0.0,
            steer_torque: 0.0,
            base_car_steer_position: Vec3f::default(),
            strut_base_length: 0.0,
            strut_body_length: 0.0,
            damage_data: SusDamageDef {
                damage_amount: 0.0,
                damage_direction,
                min_velocity: 15.0,
                damage_gain: 0.0,
                max_damage: 0.0,
                is_debug: false,
                last_amount: 0.0,
            },
        };
        sus.load_ini(data_path, index)?;
        // the file describes the wheel on the negative-x side; mirror for the other one
        // (`comiss 0, x` + `jae`: also for a NaN)
        if !(0.0 >= sus.data_rel_to_wheel.ref_point.x) {
            let d = &mut sus.data_rel_to_wheel;
            d.car_bottom_wb_f.x = -d.car_bottom_wb_f.x;
            d.car_bottom_wb_r.x = -d.car_bottom_wb_r.x;
            d.car_steer.x = -d.car_steer.x;
            d.car_strut.x = -d.car_strut.x;
            d.tyre_bottom_wb.x = -d.tyre_bottom_wb.x;
            d.tyre_steer.x = -d.tyre_steer.x;
            d.tyre_strut.x = -d.tyre_strut.x;
        }
        sus.hub = core.create_rigid_body();
        sus.strut_body = core.create_rigid_body();
        let mut mass = sus.data_rel_to_wheel.hub_mass;
        if !(mass > 0.0) {
            mass = 20.0;
        }
        let mut inertia_box = sus.data_rel_to_wheel.hub_inertia_box;
        if !ordered_nonzero((inertia_box.x * inertia_box.x + inertia_box.y * inertia_box.y) + inertia_box.z * inertia_box.z) {
            inertia_box = Vec3f::new(0.2, 0.6, 0.6);
        }
        core.set_mass_box(sus.hub, mass * 0.8, inertia_box.x, inertia_box.y, inertia_box.z);
        sus.strut_body_length = 0.2;
        core.set_mass_box(sus.strut_body, mass * 0.2, 0.05, 0.5, 0.2);
        sus.base_position = sus.data_rel_to_wheel.ref_point;
        sus.attach(core);
        sus.base_car_steer_position = sus.data_rel_to_body.car_steer;
        sus.set_steer_length_offset(core, 0.0);
        // (no ERP/CFM call here: the joints keep the world's values until `Car::step` sets them)
        Ok(sus)
    }

    /// `SuspensionStrut::loadINI` @ 0x1402c4ee0.
    fn load_ini(&mut self, data_path: &Path, index: i32) -> Result<(), String> {
        let ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        let version = ini.get_int("HEADER", "VERSION")?;
        let wheelbase = ini.get_float("BASIC", "WHEELBASE")?;
        let cg_location = ini.get_float("BASIC", "CG_LOCATION")?;
        let front_base_y = ini.get_float("FRONT", "BASEY")?;
        let front_track = ini.get_float("FRONT", "TRACK")?;
        let rear_base_y = ini.get_float("REAR", "BASEY")?;
        let rear_track = ini.get_float("REAR", "TRACK")?;
        let front_z = (1.0 - cg_location) * wheelbase;
        let rear_z = -(cg_location * wheelbase);
        let ref_points = [
            Vec3f::new(front_track * 0.5, front_base_y, front_z),
            Vec3f::new(-(front_track * 0.5), front_base_y, front_z),
            Vec3f::new(rear_track * 0.5, rear_base_y, rear_z),
            Vec3f::new(-(rear_track * 0.5), rear_base_y, rear_z),
        ];
        let section = ["FRONT", "FRONT", "REAR", "REAR"][index as usize];
        let point = |key: &str| -> Result<Vec3f, String> {
            let v = ini.get_float3(section, key)?;
            Ok(Vec3f::new(v[0], v[1], v[2]))
        };
        let d = &mut self.data_rel_to_wheel;
        d.ref_point = ref_points[index as usize];
        d.car_strut = point("STRUT_CAR")?;
        d.tyre_strut = point("STRUT_TYRE")?;
        d.car_bottom_wb_f = point("WBCAR_BOTTOM_FRONT")?;
        d.car_bottom_wb_r = point("WBCAR_BOTTOM_REAR")?;
        d.tyre_bottom_wb = point("WBTYRE_BOTTOM")?;
        d.tyre_steer = point("WBTYRE_STEER")?;
        d.car_steer = point("WBCAR_STEER")?;
        if version > 1 {
            let rim_offset = -ini.get_float(section, "RIM_OFFSET")?;
            if ordered_nonzero(rim_offset) {
                // "APPLYING RIM OFFSET"
                d.car_strut.x = rim_offset + d.car_strut.x;
                d.car_bottom_wb_f.x = rim_offset + d.car_bottom_wb_f.x;
                d.car_bottom_wb_r.x = rim_offset + d.car_bottom_wb_r.x;
                d.tyre_strut.x = rim_offset + d.tyre_strut.x;
                d.tyre_bottom_wb.x = rim_offset + d.tyre_bottom_wb.x;
                d.tyre_steer.x = rim_offset + d.tyre_steer.x;
                d.car_steer.x = rim_offset + d.car_steer.x;
            }
        }
        d.hub_mass = ini.get_float(section, "HUB_MASS")?;
        let base = &mut self.base;
        base.bump_stop_up = ini.get_float(section, "BUMPSTOP_UP")?;
        base.bump_stop_dn = -ini.get_float(section, "BUMPSTOP_DN")?;
        base.rod_length = ini.get_float(section, "ROD_LENGTH")?;
        base.toe_out_linear = ini.get_float(section, "TOE_OUT")?;
        base.k = ini.get_float(section, "SPRING_RATE")?;
        base.progressive_k = ini.get_float(section, "PROGRESSIVE_SPRING_RATE")?;
        self.damper = Damper::load(&ini, section)?;
        base.bump_stop_rate = ini.get_float(section, "BUMP_STOP_RATE")?;
        if !ordered_nonzero(base.bump_stop_rate) {
            base.bump_stop_rate = 500000.0;
        }
        // (no BUMP_STOP_PROGRESSIVE for a strut)
        // degrees to radians with the constant 0.017453 (0x3c8ef998), sign by side
        base.static_camber = -(ini.get_float(section, "STATIC_CAMBER")? * 0.017453);
        if index % 2 != 0 {
            base.static_camber = -base.static_camber;
        }
        base.packer_range = ini.get_float(section, "PACKER_RANGE")?;
        if ini.has_section("DAMAGE") {
            self.damage_data.min_velocity = ini.get_float("DAMAGE", "MIN_VELOCITY")?;
            self.damage_data.damage_gain = ini.get_float("DAMAGE", "GAIN")?;
            self.damage_data.max_damage = ini.get_float("DAMAGE", "MAX_DAMAGE")?;
            self.damage_data.is_debug = ini.get_int("DAMAGE", "DEBUG_LOG")? != 0;
        }
        Ok(())
    }

    /// `SuspensionStrut::setPositions` @ 0x1402c6210: the hub to its design pose; the strut
    /// body onto the line from the top mount to the strut's foot, 0.1 m below the mount.
    fn set_positions(&mut self, core: &mut PhysicsCore) {
        let body_matrix = core.get_world_matrix(self.car_body);
        let position = core.local_to_world(self.car_body, &self.base_position);
        core.set_rotation(self.hub, &body_matrix);
        core.set_position(self.hub, &position);
        let top = core.local_to_world(self.car_body, &self.data_rel_to_body.car_strut);
        let foot = core.local_to_world(self.hub, &self.data_rel_to_wheel.tyre_strut);
        let mut d = Vec3f::new(foot.x - top.x, foot.y - top.y, foot.z - top.z);
        let length = sqrtf((d.y * d.y + d.x * d.x) + d.z * d.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            d = Vec3f::new(d.x * inverse, d.y * inverse, d.z * inverse);
        }
        // the body's forward axis, reversed
        let n = Vec3f::new(-body_matrix.m[2][0], -body_matrix.m[2][1], -body_matrix.m[2][2]);
        // side = d x n (not normalised), front = d x side (normalised)
        let side = Vec3f::new(n.z * d.y - n.y * d.z, n.x * d.z - n.z * d.x, n.y * d.x - n.x * d.y);
        let mut front = Vec3f::new(side.z * d.y - side.y * d.z, side.x * d.z - side.z * d.x, side.y * d.x - side.x * d.y);
        let length = sqrtf((front.y * front.y + front.x * front.x) + front.z * front.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            front = Vec3f::new(front.x * inverse, front.y * inverse, front.z * inverse);
        }
        let rotation = Mat44f {
            m: [
                [front.x, front.y, front.z, 0.0],
                [side.x, side.y, side.z, 0.0],
                [-d.x, -d.y, -d.z, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        core.set_rotation(self.strut_body, &rotation);
        let l = self.strut_body_length;
        let position = Vec3f::new((d.x * l) * 0.5 + top.x, (d.y * l) * 0.5 + top.y, (d.z * l) * 0.5 + top.z);
        core.set_position(self.strut_body, &position);
    }

    /// The line the spring and the damper act along: top mount, foot, the unit direction from
    /// the mount to the foot and the squared distance.
    fn strut_line(&self, core: &PhysicsCore) -> (Vec3f, Vec3f, Vec3f, f32) {
        let top = core.local_to_world(self.car_body, &self.data_rel_to_body.car_strut);
        let foot = core.local_to_world(self.hub, &self.data_rel_to_wheel.tyre_strut);
        let d = Vec3f::new(foot.x - top.x, foot.y - top.y, foot.z - top.z);
        let squared = (d.y * d.y + d.x * d.x) + d.z * d.z;
        (top, foot, d, squared)
    }
}

impl SuspensionModel for VanillaStrut {
    /// `Suspension::getHubWorldMatrix` @ 0x1402c4c50 (the strut's vtable has the same
    /// function): `rotation((0,0,1), staticCamber) x hub`.
    fn get_hub_world_matrix(&self, core: &PhysicsCore) -> Mat44f {
        let hub = core.get_world_matrix(self.hub);
        let camber = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), self.base.static_camber);
        xm_matrix_multiply(&camber, &hub)
    }

    /// @ 0x1402c4d40 (shared with the double wishbone).
    fn get_point_velocity(&self, core: &PhysicsCore, p: &Vec3f) -> Vec3f {
        core.get_point_velocity(self.hub, p)
    }

    /// `SuspensionStrut::addForceAtPos` @ 0x1402c3d60. `driven` is not used. Every term of the
    /// moment is the negation of the double wishbone's and the sum is subtracted.
    fn add_force_at_pos(&mut self, core: &mut PhysicsCore, force: &Vec3f, pos: &Vec3f, _driven: bool, add_to_steer_torque: bool) {
        core.add_force_at_pos(self.hub, force, pos);
        if add_to_steer_torque {
            let (centre, axis) = self.get_steer_basis(core);
            let f = force;
            let r = Vec3f::new(pos.x - centre.x, pos.y - centre.y, pos.z - centre.z);
            self.steer_torque = self.steer_torque
                - (((f.z * r.x - f.x * r.z) * axis.y + (f.y * r.z - f.z * r.y) * axis.x) + (f.x * r.y - f.y * r.x) * axis.z);
        }
    }

    /// `SuspensionStrut::addTorque` @ 0x1402c4060.
    fn add_torque(&mut self, core: &mut PhysicsCore, torque: &Vec3f) {
        core.add_torque(self.hub, torque);
        let (_, axis) = self.get_steer_basis(core);
        self.steer_torque = ((axis.y * torque.y + axis.x * torque.x) + axis.z * torque.z) + self.steer_torque;
    }

    /// `SuspensionStrut::setSteerLengthOffset` @ 0x1402c6540.
    fn set_steer_length_offset(&mut self, core: &mut PhysicsCore, offset: f32) {
        let side = sign(self.data_rel_to_wheel.ref_point.x) * self.base.toe_out_linear;
        let x = ((self.damage_data.damage_direction * self.damage_data.damage_amount + offset) + side)
            + self.base_car_steer_position.x;
        self.data_rel_to_body.car_steer = Vec3f::new(x, self.base_car_steer_position.y, self.base_car_steer_position.z);
        core.reseat_distance_joint_local(&self.rods[2], &self.data_rel_to_body.car_steer, &self.data_rel_to_wheel.tyre_steer);
    }

    /// @ 0x1402c4ed0.
    fn get_steer_torque(&self) -> f32 {
        self.steer_torque
    }

    /// @ 0x1402c1bc0 (shared with the double wishbone).
    fn get_hub_angular_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_angular_velocity(self.hub)
    }

    /// `SuspensionStrut::attach` @ 0x1402c4100.
    fn attach(&mut self, core: &mut PhysicsCore) {
        self.set_positions(core);
        if self.rods.is_empty() {
            let w = self.data_rel_to_wheel;
            let body = self.car_body;
            let to_body = |core: &PhysicsCore, p: &Vec3f| core.local_to_world(body, &add(&w.ref_point, p));
            let b = &mut self.data_rel_to_body;
            b.car_bottom_wb_f = to_body(core, &w.car_bottom_wb_f);
            b.car_bottom_wb_r = to_body(core, &w.car_bottom_wb_r);
            b.car_strut = to_body(core, &w.car_strut);
            b.tyre_bottom_wb = to_body(core, &w.tyre_bottom_wb);
            b.tyre_strut = to_body(core, &w.tyre_strut);
            b.car_steer = to_body(core, &w.car_steer);
            b.tyre_steer = to_body(core, &w.tyre_steer);
            // again, now that the top mount is known
            self.set_positions(core);
            let b = self.data_rel_to_body;
            for (car, tyre) in [(b.car_bottom_wb_r, b.tyre_bottom_wb), (b.car_bottom_wb_f, b.tyre_bottom_wb), (b.car_steer, b.tyre_steer)] {
                let joint = core.create_distance_joint(self.car_body, self.hub, &car, &tyre);
                self.rods.push(joint);
            }
            let (dx, dy, dz) = (b.car_steer.x - b.tyre_steer.x, b.car_steer.y - b.tyre_steer.y, b.car_steer.z - b.tyre_steer.z);
            let squared = (dy * dy + dx * dx) + dz * dz;
            self.steer_link_base_length = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };
            // `createBumpJoint` is a stub in this build: no bump stop joint
            let (_, _, d, squared) = self.strut_line(core);
            self.strut_base_length = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };
            self.slider = Some(core.create_slider_joint(self.strut_body, self.hub, &d));
            let top = core.local_to_world(self.car_body, &self.data_rel_to_body.car_strut);
            self.ball = Some(core.create_ball_joint(self.car_body, self.strut_body, &top));
        }
    }

    fn get_status(&self) -> SuspensionStatus {
        self.status
    }

    /// @ 0x1402c4690 (shared with the double wishbone).
    fn get_base_position(&self) -> Vec3f {
        self.base_position
    }

    fn damper(&self) -> &Damper {
        &self.damper
    }

    fn damper_mut(&mut self) -> &mut Damper {
        &mut self.damper
    }

    /// `SuspensionStrut::setDamage` @ 0x1402c6160.
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

    /// `SuspensionStrut::resetDamage` @ 0x1402c6150.
    fn reset_damage(&mut self) {
        self.damage_data.damage_amount = 0.0;
    }

    fn damage_amount(&self) -> f32 {
        self.damage_data.damage_amount
    }

    fn set_damage_amount(&mut self, amount: f32) {
        self.damage_data.damage_amount = amount;
    }

    /// `SuspensionStrut::getDamage` @ 0x1402c46b0.
    fn get_damage(&self) -> f32 {
        if ordered_nonzero(self.damage_data.max_damage) {
            self.damage_data.damage_amount / self.damage_data.max_damage
        } else {
            0.0
        }
    }

    /// @ 0x1402c4d30 (shared with the double wishbone): the hub alone, without the strut body.
    fn get_mass(&self, core: &PhysicsCore) -> f32 {
        core.get_mass(self.hub)
    }

    /// @ 0x1402c6b20 (shared with the double wishbone): the hub alone; the strut body keeps
    /// its speed.
    fn stop(&mut self, core: &mut PhysicsCore) {
        core.stop(self.hub);
    }

    /// @ 0x1402c1d70 (shared with the double wishbone).
    fn get_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_velocity(self.hub)
    }

    /// `SuspensionStrut::getSteerBasis` @ 0x1402c4d70: the line from the strut's foot up to
    /// the top mount; `centre` is their midpoint.
    fn get_steer_basis(&self, core: &PhysicsCore) -> (Vec3f, Vec3f) {
        let a = core.local_to_world(self.car_body, &self.data_rel_to_body.car_strut);
        let b = core.local_to_world(self.hub, &self.data_rel_to_wheel.tyre_strut);
        let mut axis = Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z);
        let length = sqrtf((axis.y * axis.y + axis.x * axis.x) + axis.z * axis.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            axis = Vec3f::new(inverse * axis.x, axis.y * inverse, axis.z * inverse);
        }
        let centre = Vec3f::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5);
        (centre, axis)
    }

    /// `SuspensionStrut::step` @ 0x1402c6600.
    ///
    /// The damper speed is the rate of change of the strut's length, positive while the strut
    /// gets longer: the opposite sign to every other suspension type, so `DAMP_BUMP` acts in
    /// extension here. The bump stops are a fixed 500000 N/m without any "is it zero" test.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step(&mut self, core: &mut PhysicsCore, _dt: f32) {
        self.steer_torque = 0.0;
        let (top, foot, mut dir, squared) = self.strut_line(core);
        let length = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };
        let again = sqrtf(squared);
        if ordered_nonzero(again) {
            let inverse = 1.0 / again;
            dir = Vec3f::new(dir.x * inverse, dir.y * inverse, dir.z * inverse);
        }
        let base = self.base;
        let travel = (self.strut_base_length + base.rod_length) - length;
        self.status.travel = travel;

        let previous = core.source;
        // spring and packer
        core.source = ForceSource::Spring;
        let spring = (travel * base.progressive_k + base.k) * travel;
        let mut force = if spring > 0.0 { spring } else { 0.0 };
        if ordered_nonzero(base.packer_range) && travel > base.packer_range {
            force += (travel - base.packer_range) * base.bump_stop_rate;
        }
        if force > 0.0 {
            let on_hub = Vec3f::new(dir.x * force, dir.y * force, dir.z * force);
            self.add_force_at_pos(core, &on_hub, &foot, false, false);
            let n = -force;
            core.add_force_at_pos(self.car_body, &Vec3f::new(dir.x * n, dir.y * n, dir.z * n), &top);
        }

        // bump stops
        core.source = ForceSource::Bumpstop;
        let hub_pos = core.get_position(self.hub);
        let hub_local = core.world_to_local(self.car_body, &hub_pos);
        let body_matrix = core.get_world_matrix(self.car_body);
        let up = Vec3f::new(body_matrix.m[1][0], body_matrix.m[1][1], body_matrix.m[1][2]);
        let dy = hub_local.y - self.data_rel_to_wheel.ref_point.y;
        if dy > base.bump_stop_up {
            let force = (dy - base.bump_stop_up) * 500000.0;
            let n = -force;
            let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
            let pos = core.get_position(self.hub);
            self.add_force_at_pos(core, &on_hub, &pos, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &hub_local);
        }
        if !(dy >= base.bump_stop_dn) {
            let force = (dy - base.bump_stop_dn) * 500000.0;
            let n = -force;
            let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
            let pos = core.get_position(self.hub);
            self.add_force_at_pos(core, &on_hub, &pos, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &hub_local);
        }

        // damper
        core.source = ForceSource::Damper;
        let foot_velocity = core.get_local_point_velocity(self.hub, &self.data_rel_to_wheel.tyre_strut);
        let top_velocity = core.get_local_point_velocity(self.car_body, &self.data_rel_to_body.car_strut);
        let speed = ((foot_velocity.y - top_velocity.y) * dir.y + (foot_velocity.x - top_velocity.x) * dir.x)
            + (foot_velocity.z - top_velocity.z) * dir.z;
        self.status.damper_speed_ms = speed;
        let f = self.damper.get_force(speed);
        let on_hub = Vec3f::new(dir.x * f, dir.y * f, dir.z * f);
        self.add_force_at_pos(core, &on_hub, &foot, false, false);
        core.add_force_at_pos(self.car_body, &Vec3f::new(on_hub.x * -1.0, on_hub.y * -1.0, on_hub.z * -1.0), &top);
        core.source = previous;
    }

    /// `SuspensionStrut::setERPCFM` @ 0x1402c61b0: all five joints. The slider takes the CFM
    /// only (its limit-motor block has no ERP).
    fn set_erp_cfm(&mut self, core: &mut PhysicsCore, erp: f32, cfm: f32) {
        for id in self.joints() {
            core.joint_set_erp_cfm(id, erp, cfm);
        }
    }

    /// `SuspensionStrut::addLocalForceAndTorque` @ 0x1402c3e80.
    fn add_local_force_and_torque(&mut self, core: &mut PhysicsCore, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        core.add_force_at_local_pos(self.hub, force, &Vec3f::default());
        core.add_torque(self.hub, torque);
        let (centre, axis) = self.get_steer_basis(core);
        let pos = core.get_position(self.hub);
        let f = force;
        let r = Vec3f::new(pos.x - centre.x, pos.y - centre.y, pos.z - centre.z);
        self.steer_torque = (((f.x * r.z - f.z * r.x) * axis.y + (f.z * r.y - f.y * r.z) * axis.x)
            + (f.y * r.x - f.x * r.y) * axis.z)
            + self.steer_torque;
        self.steer_torque = ((axis.y * torque.y + axis.x * torque.x) + axis.z * torque.z) + self.steer_torque;
        let d = drive_torque;
        if ordered_nonzero(d.x) || ordered_nonzero(d.y) || ordered_nonzero(d.z) {
            core.add_torque(self.car_body, d);
        }
    }

    fn kind(&self) -> SuspensionType {
        SuspensionType::Strut
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
        self.rods.iter().map(|joint| joint.id).chain(self.slider).chain(self.ball).collect()
    }

    fn joint_names(&self) -> Vec<String> {
        ["bottom_rear", "bottom_front", "steer_rod", "slider", "ball"].map(String::from).to_vec()
    }

    fn bodies(&self) -> Vec<RigidBody> {
        vec![self.hub, self.strut_body]
    }

    fn restore_step_state(&mut self, status: SuspensionStatus, steer_torque: f32) {
        self.status = status;
        self.steer_torque = steer_torque;
    }
}
