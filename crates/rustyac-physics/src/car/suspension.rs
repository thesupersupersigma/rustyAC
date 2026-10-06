//! The suspension slot ([`SuspensionModel`], AC's `ISuspension`) and its double-wishbone
//! implementation [`VanillaDwb`] (AC's `Suspension`, "DWB" in suspensions.ini), with the
//! [`Damper`] every suspension type uses.
//!
//! A double-wishbone corner is one rigid body (the hub) held to the car body by five rods of
//! fixed length: four wishbone arms and the steering rod. The rods only fix the geometry;
//! spring, damper, packer and bump stops are forces computed here every step and pushed into
//! the hub and the body.

use std::path::Path;

use super::body::{DistanceJoint, ForceSource, PhysicsCore, RigidBody};
use crate::data::ini::IniReader;
use crate::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

/// AC's `SuspensionStatus` (8 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SuspensionStatus {
    /// `travel`: hub height above its design position plus the rod length, m.
    pub travel: f32,
    /// `damperSpeedMS`: hub speed along the body's up axis relative to the body, m/s
    /// (positive = bump).
    pub damper_speed_ms: f32,
}

/// AC's `Damper` (0x18 bytes). Field order is the original's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damper {
    /// `reboundSlow`, N s/m
    pub rebound_slow: f32,
    /// `reboundFast`
    pub rebound_fast: f32,
    /// `bumpSlow`
    pub bump_slow: f32,
    /// `bumpFast`
    pub bump_fast: f32,
    /// `fastThresholdBump`, m/s
    pub fast_threshold_bump: f32,
    /// `fastThresholdRebound`, m/s
    pub fast_threshold_rebound: f32,
}

impl Default for Damper {
    /// `Damper::Damper` @ 0x1402b3250.
    fn default() -> Damper {
        Damper {
            rebound_slow: 5000.0,
            rebound_fast: 300.0,
            bump_slow: 2000.0,
            bump_fast: 300.0,
            fast_threshold_bump: 0.2,
            fast_threshold_rebound: 0.2,
        }
    }
}

impl Damper {
    /// `Damper::getForce` @ 0x1402b3280. `v > 0` is bump.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_force(&self, v: f32) -> f32 {
        if v > 0.0 {
            let t = self.fast_threshold_bump;
            if v > t {
                -((v - t) * self.bump_fast + t * self.bump_slow)
            } else {
                -(v * self.bump_slow)
            }
        } else {
            let t = self.fast_threshold_rebound;
            if v.abs() > t {
                t * self.rebound_slow - (t + v) * self.rebound_fast
            } else {
                -(v * self.rebound_slow)
            }
        }
    }

    /// The six `DAMP_*` keys of a wheel or heave section, with the fall-backs
    /// `Suspension::loadINI` @ 0x1402c1d90 and `HeaveSpring::initData` @ 0x1402b3340 apply:
    /// a fast rate of 0 becomes the slow rate, a threshold of 0 becomes 0.2.
    pub fn load(ini: &IniReader, section: &str) -> Result<Damper, String> {
        let mut damper = Damper {
            bump_slow: ini.get_float(section, "DAMP_BUMP")?,
            rebound_slow: ini.get_float(section, "DAMP_REBOUND")?,
            bump_fast: ini.get_float(section, "DAMP_FAST_BUMP")?,
            rebound_fast: ini.get_float(section, "DAMP_FAST_REBOUND")?,
            fast_threshold_bump: ini.get_float(section, "DAMP_FAST_BUMPTHRESHOLD")?,
            fast_threshold_rebound: ini.get_float(section, "DAMP_FAST_REBOUNDTHRESHOLD")?,
        };
        // each `ucomiss 0, x` + `jne`: the fall-back is taken for 0 and for a NaN
        if !ordered_nonzero(damper.fast_threshold_bump) {
            damper.fast_threshold_bump = 0.2;
        }
        if !ordered_nonzero(damper.fast_threshold_rebound) {
            damper.fast_threshold_rebound = 0.2;
        }
        if !ordered_nonzero(damper.bump_fast) {
            damper.bump_fast = damper.bump_slow;
        }
        if !ordered_nonzero(damper.rebound_fast) {
            damper.rebound_fast = damper.rebound_slow;
        }
        Ok(damper)
    }
}

/// The numbers every suspension type shares (the data members of AC's `ISuspension`, which
/// the setup items point into).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuspensionBase {
    /// `k`: wheel rate, N/m
    pub k: f32,
    /// `progressiveK`, N/m per m
    pub progressive_k: f32,
    /// `bumpStopRate`, N/m (also the packer's rate)
    pub bump_stop_rate: f32,
    /// `bumpStopProgressive`
    pub bump_stop_progressive: f32,
    /// `staticCamber`, rad (already with the sign of the wheel's side)
    pub static_camber: f32,
    /// `bumpStopUp`, m
    pub bump_stop_up: f32,
    /// `bumpStopDn`, m (negative)
    pub bump_stop_dn: f32,
    /// `rodLength`, m
    pub rod_length: f32,
    /// `toeOUT_Linear`, m of steering-rod shift
    pub toe_out_linear: f32,
    /// `packerRange`, m
    pub packer_range: f32,
    /// `baseCFM`
    pub base_cfm: f32,
}

impl Default for SuspensionBase {
    /// The inlined `ISuspension` constructor plus what `Suspension::Suspension` adds.
    fn default() -> SuspensionBase {
        SuspensionBase {
            k: 90000.0,
            progressive_k: 0.0,
            bump_stop_rate: 0.0,
            bump_stop_progressive: 0.0,
            static_camber: 0.0,
            bump_stop_up: 0.0,
            bump_stop_dn: 0.0,
            rod_length: 0.0,
            toe_out_linear: 0.0,
            packer_range: 0.0,
            base_cfm: 1e-7,
        }
    }
}

/// The suspension slot: AC's `ISuspension` (25 virtual functions, vtables at 0x1404ff870
/// DWB, 0x1404ffc80 STRUT, 0x1404ffe90 AXLE, 0x1405001a0 ML). Method names are the PDB's in
/// snake_case. The rigid bodies live in the [`PhysicsCore`], which every method that touches
/// one is handed.
pub trait SuspensionModel {
    /// +0x08 `getHubWorldMatrix`: the hub's world matrix with the static camber applied.
    fn get_hub_world_matrix(&self, core: &PhysicsCore) -> Mat44f;
    /// +0x10 `getPointVelocity`: velocity of the hub body at a world point.
    fn get_point_velocity(&self, core: &PhysicsCore, p: &Vec3f) -> Vec3f;
    /// +0x18 `addForceAtPos(force, pos, driven, addToSteerTorque)`: a world force at a world
    /// point of the hub; optionally its moment about the steering axis goes into the steer
    /// torque.
    fn add_force_at_pos(&mut self, core: &mut PhysicsCore, force: &Vec3f, pos: &Vec3f, driven: bool, add_to_steer_torque: bool);
    /// +0x20 `addTorque`: a world torque on the hub.
    fn add_torque(&mut self, core: &mut PhysicsCore, torque: &Vec3f);
    /// +0x28 `setSteerLengthOffset`: moves the body end of the steering rod sideways.
    fn set_steer_length_offset(&mut self, core: &mut PhysicsCore, offset: f32);
    /// +0x30 `getSteerTorque`: the torque about the steering axis collected this step.
    fn get_steer_torque(&self) -> f32;
    /// +0x38 `getHubAngularVelocity`
    fn get_hub_angular_velocity(&self, core: &PhysicsCore) -> Vec3f;
    /// +0x40 `attach`: puts the hub at its design pose; the first call creates the joints.
    fn attach(&mut self, core: &mut PhysicsCore);
    /// +0x48 `getStatus`
    fn get_status(&self) -> SuspensionStatus;
    /// +0x50 `getBasePosition`: the design position of the wheel centre in body coordinates.
    fn get_base_position(&self) -> Vec3f;
    /// +0x58 `getK`
    fn get_k(&self) -> f32 {
        self.base().k
    }
    /// +0x60 `getDamper`
    fn damper(&self) -> &Damper;
    /// +0x60 `getDamper`, for the setup.
    fn damper_mut(&mut self) -> &mut Damper;
    /// +0x68 `getPackerRange`
    fn get_packer_range(&self) -> f32 {
        self.base().packer_range
    }
    /// +0x78 `setDamage`
    fn set_damage(&mut self, amount: f32);
    /// +0x80 `resetDamage`
    fn reset_damage(&mut self);
    /// +0x88 `getDamage`
    fn get_damage(&self) -> f32;
    /// +0x90 `getMass`: the mass of the hub body.
    fn get_mass(&self, core: &PhysicsCore) -> f32;
    /// +0x98 `stop`
    fn stop(&mut self, core: &mut PhysicsCore);
    /// +0xa0 `getVelocity`: the hub's linear velocity.
    fn get_velocity(&self, core: &PhysicsCore) -> Vec3f;
    /// +0xa8 `getSteerBasis(centre, axis)`: the steering axis in world space.
    fn get_steer_basis(&self, core: &PhysicsCore) -> (Vec3f, Vec3f);
    /// +0xb0 `step`: spring, damper, packer and bump stops of this step.
    fn step(&mut self, core: &mut PhysicsCore, dt: f32);
    /// +0xb8 `setERPCFM`: forwards to every joint.
    fn set_erp_cfm(&mut self, core: &mut PhysicsCore, erp: f32, cfm: f32);
    /// +0xc0 `addLocalForceAndTorque(force, torque, driveTorque)`
    fn add_local_force_and_torque(&mut self, core: &mut PhysicsCore, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f);

    /// The shared members (`k`, `rodLength`, …).
    fn base(&self) -> &SuspensionBase;
    /// The shared members, for the setup.
    fn base_mut(&mut self) -> &mut SuspensionBase;
    /// The hub body (the heave spring and the comparison with the game read it).
    fn hub(&self) -> RigidBody;
    /// The joints, in creation order.
    fn joints(&self) -> Vec<DistanceJoint>;
    /// Puts back what a step leaves behind (`status` and the steer torque): for restoring a
    /// saved state, not part of AC's interface.
    fn restore_step_state(&mut self, status: SuspensionStatus, steer_torque: f32);
}

/// AC's `SDWSuspensionData` (0x7c bytes): the pick-up points of a double-wishbone corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DwbSuspensionData {
    /// `carTopWB_F`
    pub car_top_wb_f: Vec3f,
    /// `carTopWB_R`
    pub car_top_wb_r: Vec3f,
    /// `carBottomWB_F`
    pub car_bottom_wb_f: Vec3f,
    /// `carBottomWB_R`
    pub car_bottom_wb_r: Vec3f,
    /// `tyreTopWB`
    pub tyre_top_wb: Vec3f,
    /// `tyreBottomWB`
    pub tyre_bottom_wb: Vec3f,
    /// `carSteer`
    pub car_steer: Vec3f,
    /// `tyreSteer`
    pub tyre_steer: Vec3f,
    /// `refPoint`: the design position of the wheel centre, body coordinates.
    pub ref_point: Vec3f,
    /// `hubMass`, kg
    pub hub_mass: f32,
    /// `hubInertiaBox`: never set by a file; zero selects the box (0.2, 0.6, 0.6) m.
    pub hub_inertia_box: Vec3f,
}

/// AC's `SusDamageDef` (0x1c bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SusDamageDef {
    /// `damageAmount`: m of steering-rod shift.
    pub damage_amount: f32,
    /// `damageDirection`: +1 or -1, drawn with `rand()` in the constructor.
    pub damage_direction: f32,
    /// `minVelocity`, km/h
    pub min_velocity: f32,
    /// `damageGain`
    pub damage_gain: f32,
    /// `maxDamage`
    pub max_damage: f32,
    /// `isDebug`
    pub is_debug: bool,
    /// `lastAmount`
    pub last_amount: f32,
}

/// AC's `Suspension` (0x210 bytes): the double-wishbone suspension of one wheel.
#[derive(Clone, Debug)]
pub struct VanillaDwb {
    pub base: SuspensionBase,
    /// `carBody`
    pub car_body: RigidBody,
    /// `hub`
    pub hub: RigidBody,
    /// `basePosition`
    pub base_position: Vec3f,
    /// `joints`: top rear, top front, bottom rear, bottom front, steering rod.
    pub joints: Vec<DistanceJoint>,
    /// `dataRelToWheel`: the points as in the file (after rim offset and mirroring).
    pub data_rel_to_wheel: DwbSuspensionData,
    /// `dataRelToBody`: the points in body coordinates.
    pub data_rel_to_body: DwbSuspensionData,
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
    /// `steerAngle` (written 0 by the constructor, never again)
    pub steer_angle: f32,
    /// `damageData`
    pub damage_data: SusDamageDef,
}

/// `sign()` as the compiled code spells it (`comiss` + `jbe` / `jae`): a NaN counts as
/// negative.
fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

/// `ucomiss x, 0` + `je`: true for an ordered value that is not zero.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

impl VanillaDwb {
    /// `Suspension::Suspension(Car*, int index)` @ 0x1402c0720.
    ///
    /// `rand` is the C runtime's `rand()` result the constructor draws for the damage
    /// direction (0..=0x7fff). `data_path` is the car's data folder.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn new(core: &mut PhysicsCore, car_body: RigidBody, data_path: &Path, index: i32, rand: i32) -> Result<VanillaDwb, String> {
        let draw = rand as f32 * 3.051_851e-5 * 100.0;
        let damage_direction = if draw >= 50.0 { 1.0 } else { -1.0 };
        let mut sus = VanillaDwb {
            base: SuspensionBase::default(),
            car_body,
            hub: car_body,
            base_position: Vec3f::default(),
            joints: Vec::new(),
            data_rel_to_wheel: DwbSuspensionData::default(),
            data_rel_to_body: DwbSuspensionData::default(),
            damper: Damper::default(),
            status: SuspensionStatus::default(),
            index,
            steer_link_base_length: 0.0,
            steer_torque: 0.0,
            base_car_steer_position: Vec3f::default(),
            steer_angle: 0.0,
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
            d.car_top_wb_f.x = -d.car_top_wb_f.x;
            d.car_top_wb_r.x = -d.car_top_wb_r.x;
            d.tyre_bottom_wb.x = -d.tyre_bottom_wb.x;
            d.tyre_steer.x = -d.tyre_steer.x;
            d.tyre_top_wb.x = -d.tyre_top_wb.x;
        }
        sus.hub = core.create_rigid_body();
        let mut mass = sus.data_rel_to_wheel.hub_mass;
        if !(mass > 0.0) {
            mass = 20.0;
        }
        let mut inertia_box = sus.data_rel_to_wheel.hub_inertia_box;
        if !ordered_nonzero((inertia_box.x * inertia_box.x + inertia_box.y * inertia_box.y) + inertia_box.z * inertia_box.z) {
            inertia_box = Vec3f::new(0.2, 0.6, 0.6);
        }
        core.set_mass_box(sus.hub, mass, inertia_box.x, inertia_box.y, inertia_box.z);
        sus.base_position = sus.data_rel_to_wheel.ref_point;
        sus.attach(core);
        sus.base_car_steer_position = sus.data_rel_to_body.car_steer;
        sus.set_steer_length_offset(core, 0.0);
        let base_cfm = sus.base.base_cfm;
        for joint in sus.joints.clone() {
            core.joint_set_erp_cfm(joint.id, 0.3, base_cfm);
        }
        Ok(sus)
    }

    /// `Suspension::loadINI` @ 0x1402c1d90.
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
        d.car_top_wb_f = point("WBCAR_TOP_FRONT")?;
        d.car_top_wb_r = point("WBCAR_TOP_REAR")?;
        d.car_bottom_wb_f = point("WBCAR_BOTTOM_FRONT")?;
        d.car_bottom_wb_r = point("WBCAR_BOTTOM_REAR")?;
        d.tyre_top_wb = point("WBTYRE_TOP")?;
        d.tyre_bottom_wb = point("WBTYRE_BOTTOM")?;
        d.tyre_steer = point("WBTYRE_STEER")?;
        d.car_steer = point("WBCAR_STEER")?;
        if version > 1 {
            let rim_offset = -ini.get_float(section, "RIM_OFFSET")?;
            if ordered_nonzero(rim_offset) {
                // "APPLYING RIM OFFSET"
                d.car_top_wb_f.x = rim_offset + d.car_top_wb_f.x;
                d.car_top_wb_r.x = rim_offset + d.car_top_wb_r.x;
                d.car_bottom_wb_f.x = rim_offset + d.car_bottom_wb_f.x;
                d.car_bottom_wb_r.x = rim_offset + d.car_bottom_wb_r.x;
                d.tyre_top_wb.x = rim_offset + d.tyre_top_wb.x;
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
        if ini.has_key(section, "BUMP_STOP_PROGRESSIVE") {
            base.bump_stop_progressive = ini.get_float(section, "BUMP_STOP_PROGRESSIVE")?;
        }
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
}

fn add(a: &Vec3f, b: &Vec3f) -> Vec3f {
    Vec3f::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

impl SuspensionModel for VanillaDwb {
    /// `Suspension::getHubWorldMatrix` @ 0x1402c4c50: `rotation((0,0,1), staticCamber) x hub`.
    fn get_hub_world_matrix(&self, core: &PhysicsCore) -> Mat44f {
        let hub = core.get_world_matrix(self.hub);
        let camber = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), self.base.static_camber);
        xm_matrix_multiply(&camber, &hub)
    }

    /// `Suspension::getPointVelocity` @ 0x1402c4d40.
    fn get_point_velocity(&self, core: &PhysicsCore, p: &Vec3f) -> Vec3f {
        core.get_point_velocity(self.hub, p)
    }

    /// `Suspension::addForceAtPos` @ 0x1402c0bf0. `driven` is not used.
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

    /// `Suspension::addTorque` @ 0x1402c0f10.
    fn add_torque(&mut self, core: &mut PhysicsCore, torque: &Vec3f) {
        core.add_torque(self.hub, torque);
        let (_, axis) = self.get_steer_basis(core);
        self.steer_torque = ((axis.y * torque.y + axis.x * torque.x) + axis.z * torque.z) + self.steer_torque;
    }

    /// `Suspension::setSteerLengthOffset` @ 0x1402c3290.
    fn set_steer_length_offset(&mut self, core: &mut PhysicsCore, offset: f32) {
        let side = sign(self.data_rel_to_wheel.ref_point.x) * self.base.toe_out_linear;
        let x = ((self.damage_data.damage_direction * self.damage_data.damage_amount + offset) + side)
            + self.base_car_steer_position.x;
        self.data_rel_to_body.car_steer = Vec3f::new(x, self.base_car_steer_position.y, self.base_car_steer_position.z);
        core.reseat_distance_joint_local(&self.joints[4], &self.data_rel_to_body.car_steer, &self.data_rel_to_wheel.tyre_steer);
    }

    /// `Suspension::getSteerTorque` @ 0x1402c1d60.
    fn get_steer_torque(&self) -> f32 {
        self.steer_torque
    }

    /// `Suspension::getHubAngularVelocity` @ 0x1402c1bc0.
    fn get_hub_angular_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_angular_velocity(self.hub)
    }

    /// `Suspension::attach` @ 0x1402c0fb0.
    fn attach(&mut self, core: &mut PhysicsCore) {
        let body_matrix = core.get_world_matrix(self.car_body);
        let position = core.local_to_world(self.car_body, &self.base_position);
        core.set_rotation(self.hub, &body_matrix);
        core.set_position(self.hub, &position);
        if self.joints.is_empty() {
            let w = self.data_rel_to_wheel;
            let body = self.car_body;
            let to_body = |core: &PhysicsCore, p: &Vec3f| core.local_to_world(body, &add(&w.ref_point, p));
            let b = &mut self.data_rel_to_body;
            b.car_bottom_wb_f = to_body(core, &w.car_bottom_wb_f);
            b.car_bottom_wb_r = to_body(core, &w.car_bottom_wb_r);
            b.car_top_wb_f = to_body(core, &w.car_top_wb_f);
            b.car_top_wb_r = to_body(core, &w.car_top_wb_r);
            b.tyre_bottom_wb = to_body(core, &w.tyre_bottom_wb);
            b.tyre_top_wb = to_body(core, &w.tyre_top_wb);
            b.car_steer = to_body(core, &w.car_steer);
            b.tyre_steer = to_body(core, &w.tyre_steer);
            let b = self.data_rel_to_body;
            for (car, tyre) in [
                (b.car_top_wb_r, b.tyre_top_wb),
                (b.car_top_wb_f, b.tyre_top_wb),
                (b.car_bottom_wb_r, b.tyre_bottom_wb),
                (b.car_bottom_wb_f, b.tyre_bottom_wb),
                (b.car_steer, b.tyre_steer),
            ] {
                let joint = core.create_distance_joint(self.car_body, self.hub, &car, &tyre);
                self.joints.push(joint);
            }
            let (dx, dy, dz) = (b.car_steer.x - b.tyre_steer.x, b.car_steer.y - b.tyre_steer.y, b.car_steer.z - b.tyre_steer.z);
            let squared = dy * dy + dx * dx + dz * dz;
            self.steer_link_base_length = if ordered_nonzero(squared) { crate::math::sqrtf(squared) } else { 0.0 };
            // `createBumpJoint` is a stub in this build: no bump stop joint
        }
    }

    fn get_status(&self) -> SuspensionStatus {
        self.status
    }

    /// `Suspension::getBasePosition` @ 0x1402c4690.
    fn get_base_position(&self) -> Vec3f {
        self.base_position
    }

    fn damper(&self) -> &Damper {
        &self.damper
    }

    fn damper_mut(&mut self) -> &mut Damper {
        &mut self.damper
    }

    /// `Suspension::setDamage` @ 0x1402c31e0.
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

    /// `Suspension::resetDamage` @ 0x1402c31d0.
    fn reset_damage(&mut self) {
        self.damage_data.damage_amount = 0.0;
    }

    /// `Suspension::getDamage` @ 0x1402c1500.
    fn get_damage(&self) -> f32 {
        if ordered_nonzero(self.damage_data.max_damage) {
            self.damage_data.damage_amount / self.damage_data.max_damage
        } else {
            0.0
        }
    }

    /// `Suspension::getMass` @ 0x1402c4d30.
    fn get_mass(&self, core: &PhysicsCore) -> f32 {
        core.get_mass(self.hub)
    }

    /// `Suspension::stop` @ 0x1402c6b20.
    fn stop(&mut self, core: &mut PhysicsCore) {
        core.stop(self.hub);
    }

    /// `Suspension::getVelocity` @ 0x1402c1d70.
    fn get_velocity(&self, core: &PhysicsCore) -> Vec3f {
        core.get_velocity(self.hub)
    }

    /// `Suspension::getSteerBasis` @ 0x1402c1c00: the line through the hub's top and bottom
    /// ball joints; `centre` is their midpoint, `axis` points up.
    #[allow(clippy::double_comparisons)]
    fn get_steer_basis(&self, core: &PhysicsCore) -> (Vec3f, Vec3f) {
        let a = core.local_to_world(self.hub, &self.data_rel_to_wheel.tyre_top_wb);
        let b = core.local_to_world(self.hub, &self.data_rel_to_wheel.tyre_bottom_wb);
        let mut axis = Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z);
        let length = crate::math::sqrtf((axis.y * axis.y + axis.x * axis.x) + axis.z * axis.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            axis = Vec3f::new(inverse * axis.x, axis.y * inverse, axis.z * inverse);
        }
        let centre = Vec3f::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5);
        (centre, axis)
    }

    /// `Suspension::step` @ 0x1402c3390. The active-actuator branch (`useActiveActuator`,
    /// which nothing ever sets) is not ported.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step(&mut self, core: &mut PhysicsCore, _dt: f32) {
        self.steer_torque = 0.0;
        let hub_pos = core.get_position(self.hub);
        let hub_local = core.world_to_local(self.car_body, &hub_pos);
        let body_matrix = core.get_world_matrix(self.car_body);
        let up = Vec3f::new(body_matrix.m[1][0], body_matrix.m[1][1], body_matrix.m[1][2]);
        let ref_point = self.data_rel_to_wheel.ref_point;
        let base = self.base;
        let dy = hub_local.y - ref_point.y;
        let travel = dy + base.rod_length;
        self.status.travel = travel;

        let previous = core.source;
        // spring and packer
        core.source = ForceSource::Spring;
        let mut force = (travel * base.progressive_k + base.k) * travel;
        if ordered_nonzero(base.packer_range) && travel > base.packer_range && ordered_nonzero(base.k) {
            let d = travel - base.packer_range;
            force = force + (d * base.bump_stop_progressive + base.bump_stop_rate) * d;
        }
        if force > 0.0 {
            let n = -force;
            let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
            self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &ref_point);
        }

        // damper
        core.source = ForceSource::Damper;
        let hub_velocity = core.get_velocity(self.hub);
        let point_velocity = core.get_local_point_velocity(self.car_body, &ref_point);
        let speed = ((hub_velocity.y - point_velocity.y) * up.y + (hub_velocity.x - point_velocity.x) * up.x)
            + (hub_velocity.z - point_velocity.z) * up.z;
        let f = self.damper.get_force(speed);
        self.status.damper_speed_ms = speed;
        let on_hub = Vec3f::new(up.x * f, up.y * f, up.z * f);
        self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
        core.add_force_at_local_pos(self.car_body, &Vec3f::new(-on_hub.x, -on_hub.y, -on_hub.z), &ref_point);

        // bump stops: they compare the height without the rod length
        core.source = ForceSource::Bumpstop;
        let dy = hub_local.y - ref_point.y;
        if ordered_nonzero(base.bump_stop_up) && dy > base.bump_stop_up && ordered_nonzero(base.k) {
            let d = dy - base.bump_stop_up;
            let force = (d * base.bump_stop_progressive + base.bump_stop_rate) * d;
            let n = -force;
            let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
            self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &hub_local);
        }
        if ordered_nonzero(base.bump_stop_dn) && !(dy >= base.bump_stop_dn) && ordered_nonzero(base.k) {
            let d = dy - base.bump_stop_dn;
            let force = (d * base.bump_stop_progressive + base.bump_stop_rate) * d;
            let n = -force;
            let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
            self.add_force_at_pos(core, &on_hub, &hub_pos, false, false);
            core.add_local_force_at_local_pos(self.car_body, &Vec3f::new(0.0, force, 0.0), &hub_local);
        }
        core.source = previous;
    }

    /// `Suspension::setERPCFM` @ 0x1402c3230.
    fn set_erp_cfm(&mut self, core: &mut PhysicsCore, erp: f32, cfm: f32) {
        for joint in &self.joints {
            core.joint_set_erp_cfm(joint.id, erp, cfm);
        }
    }

    /// `Suspension::addLocalForceAndTorque` @ 0x1402c0d40.
    #[allow(clippy::double_comparisons)]
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
        // three `ucomiss 0, x`: the first two skip on "not equal", the last on "equal or NaN"
        let d = drive_torque;
        if ordered_nonzero(d.x) || ordered_nonzero(d.y) || ordered_nonzero(d.z) {
            core.add_torque(self.car_body, d);
        }
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

    fn joints(&self) -> Vec<DistanceJoint> {
        self.joints.clone()
    }

    fn restore_step_state(&mut self, status: SuspensionStatus, steer_torque: f32) {
        self.status = status;
        self.steer_torque = steer_torque;
    }
}
