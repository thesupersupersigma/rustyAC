// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! A single-wheel test rig: scripted inputs for one tyre, a fake hub, a flat (or not so
//! flat) road and a stand-in car, plus the file format `tools/tyre_oracle` records AC's own
//! `Tyre::step` in. The same [`StepInput`]s drive the game's tyre (in the oracle) and
//! [`VanillaTyre`] (here), and every recorded number is compared bit for bit.
//!
//! Nothing in here is part of the physics; it exists for the oracle, `tyre_compare` and the
//! golden test.

use std::path::Path;

use super::{
    RayCastResult, RayTrackCollisionProvider, SurfaceDef, Suspension, TorqueModeEx, TyreCar,
    VanillaTyre,
};
use crate::data::tyres_ini::Axle;
use crate::vecmath::{Mat44f, Vec3f};

/// AC's physics step, s.
pub const DT: f32 = 0.003;
/// How many hub / body calls of one step are recorded.
pub const MAX_CALLS: usize = 5;

/// Everything the rig feeds one `Tyre::step` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepInput {
    /// What `getHubWorldMatrix` returns, row-major.
    pub hub_matrix: [f32; 16],
    /// What `getPointVelocity` returns (for any point).
    pub hub_velocity: [f32; 3],
    /// What `getHubAngularVelocity` returns.
    pub hub_angular_velocity: [f32; 3],
    pub brake_torque: f32,
    pub hand_brake_torque: f32,
    pub electric_torque: f32,
    pub abs_override: f32,
    pub ai_mult: f32,
    pub driven: bool,
    /// Overwrite `status.angularVelocity` before the step (what the drivetrain does for a
    /// driven wheel).
    pub set_angular_velocity: Option<f32>,
    /// Overwrite `tyreBlanketsOn` before the step.
    pub set_blankets: Option<bool>,
    pub ext_active: bool,
    pub ext_load: f32,
    pub ext_slip_angle: f32,
    pub ext_slip_ratio: f32,
    /// False: the ray finds nothing.
    pub has_hit: bool,
    /// Height of the road under the wheel.
    pub ground_y: f32,
    pub ground_normal: [f32; 3],
    pub grip_mod: f32,
    pub dirt_additive_k: f32,
    pub sin_height: f32,
    pub sin_length: f32,
    pub damping: f32,
    pub granularity: f32,
    /// False: the tyre's `car` pointer is null (the tyre test bench).
    pub has_car: bool,
    /// `Car::torqueModeEx`
    pub torque_mode: i32,
    pub car_speed: f32,
    pub car_sleeping: bool,
    pub dynamic_grip_level: f32,
    pub tyre_consumption_rate: f32,
    pub mechanical_damage_rate: f32,
    pub ambient_temperature: f32,
    pub road_temperature: f32,
    pub allow_tyre_blankets: bool,
    pub body_velocity: [f32; 3],
    pub body_mass: f32,
}

/// Column names of [`StepInput::to_words`].
pub fn input_fields() -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..16 {
        names.push(format!("in_hub_m{}{}", i / 4 + 1, i % 4 + 1));
    }
    for group in ["in_hub_vel", "in_hub_avel"] {
        for axis in ["x", "y", "z"] {
            names.push(format!("{group}_{axis}"));
        }
    }
    for name in [
        "in_brake_torque",
        "in_hand_brake_torque",
        "in_electric_torque",
        "in_abs_override",
        "in_ai_mult",
        "in_driven",
        "in_set_av",
        "in_set_av_value",
        "in_set_blankets",
        "in_ext_active",
        "in_ext_load",
        "in_ext_slip_angle",
        "in_ext_slip_ratio",
        "in_has_hit",
        "in_ground_y",
        "in_ground_nx",
        "in_ground_ny",
        "in_ground_nz",
        "in_grip_mod",
        "in_dirt_additive_k",
        "in_sin_height",
        "in_sin_length",
        "in_damping",
        "in_granularity",
        "in_has_car",
        "in_torque_mode",
        "in_car_speed",
        "in_car_sleeping",
        "in_dynamic_grip_level",
        "in_tyre_consumption_rate",
        "in_mechanical_damage_rate",
        "in_ambient_temperature",
        "in_road_temperature",
        "in_allow_tyre_blankets",
        "in_body_vel_x",
        "in_body_vel_y",
        "in_body_vel_z",
        "in_body_mass",
    ] {
        names.push(name.to_string());
    }
    names
}

fn f(x: f32) -> u64 {
    x.to_bits() as u64
}

fn b(x: bool) -> u64 {
    x as u64
}

fn fw(word: u64) -> f32 {
    f32::from_bits(word as u32)
}

impl StepInput {
    pub fn to_words(&self) -> Vec<u64> {
        let mut w: Vec<u64> = self.hub_matrix.iter().map(|&x| f(x)).collect();
        w.extend(self.hub_velocity.iter().map(|&x| f(x)));
        w.extend(self.hub_angular_velocity.iter().map(|&x| f(x)));
        w.extend([
            f(self.brake_torque),
            f(self.hand_brake_torque),
            f(self.electric_torque),
            f(self.abs_override),
            f(self.ai_mult),
            b(self.driven),
            b(self.set_angular_velocity.is_some()),
            f(self.set_angular_velocity.unwrap_or(0.0)),
            match self.set_blankets {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            },
            b(self.ext_active),
            f(self.ext_load),
            f(self.ext_slip_angle),
            f(self.ext_slip_ratio),
            b(self.has_hit),
            f(self.ground_y),
            f(self.ground_normal[0]),
            f(self.ground_normal[1]),
            f(self.ground_normal[2]),
            f(self.grip_mod),
            f(self.dirt_additive_k),
            f(self.sin_height),
            f(self.sin_length),
            f(self.damping),
            f(self.granularity),
            b(self.has_car),
            self.torque_mode as u32 as u64,
            f(self.car_speed),
            b(self.car_sleeping),
            f(self.dynamic_grip_level),
            f(self.tyre_consumption_rate),
            f(self.mechanical_damage_rate),
            f(self.ambient_temperature),
            f(self.road_temperature),
            b(self.allow_tyre_blankets),
            f(self.body_velocity[0]),
            f(self.body_velocity[1]),
            f(self.body_velocity[2]),
            f(self.body_mass),
        ]);
        w
    }

    pub fn from_words(w: &[u64]) -> StepInput {
        let mut hub_matrix = [0.0; 16];
        for (value, &word) in hub_matrix.iter_mut().zip(w) {
            *value = fw(word);
        }
        let v = |i: usize| fw(w[i]);
        StepInput {
            hub_matrix,
            hub_velocity: [v(16), v(17), v(18)],
            hub_angular_velocity: [v(19), v(20), v(21)],
            brake_torque: v(22),
            hand_brake_torque: v(23),
            electric_torque: v(24),
            abs_override: v(25),
            ai_mult: v(26),
            driven: w[27] != 0,
            set_angular_velocity: (w[28] != 0).then(|| v(29)),
            set_blankets: match w[30] {
                0 => None,
                1 => Some(false),
                _ => Some(true),
            },
            ext_active: w[31] != 0,
            ext_load: v(32),
            ext_slip_angle: v(33),
            ext_slip_ratio: v(34),
            has_hit: w[35] != 0,
            ground_y: v(36),
            ground_normal: [v(37), v(38), v(39)],
            grip_mod: v(40),
            dirt_additive_k: v(41),
            sin_height: v(42),
            sin_length: v(43),
            damping: v(44),
            granularity: v(45),
            has_car: w[46] != 0,
            torque_mode: w[47] as u32 as i32,
            car_speed: v(48),
            car_sleeping: w[49] != 0,
            dynamic_grip_level: v(50),
            tyre_consumption_rate: v(51),
            mechanical_damage_rate: v(52),
            ambient_temperature: v(53),
            road_temperature: v(54),
            allow_tyre_blankets: w[55] != 0,
            body_velocity: [v(56), v(57), v(58)],
            body_mass: v(59),
        }
    }

    pub fn hub_world_matrix(&self) -> Mat44f {
        let mut m = Mat44f::default();
        for (i, &value) in self.hub_matrix.iter().enumerate() {
            m.m[i / 4][i % 4] = value;
        }
        m
    }

    pub fn surface_def(&self) -> SurfaceDef {
        SurfaceDef {
            grip_mod: self.grip_mod,
            dirt_additive_k: self.dirt_additive_k,
            sin_height: self.sin_height,
            sin_length: self.sin_length,
            damping: self.damping,
            granularity: self.granularity,
            ..SurfaceDef::default()
        }
    }
}

/// One call the tyre made on the hub (or the car body) during a step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Call {
    /// 1 `addForceAtPos(a = force, b = pos)`, 2 `addTorque(a)`,
    /// 3 `addLocalForceAndTorque(a = force, b = torque, c = driveTorque)`,
    /// 4 body `addForceAtLocalPos(a = f, b = p)`.
    pub kind: u32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub c: [f32; 3],
    /// For kind 1: bit 0 `driven`, bit 1 `addToSteerTorque`.
    pub flags: u32,
}

fn arr(v: &Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// The rig's hub: answers from the [`StepInput`], records what it is told.
pub struct RigHub<'a> {
    pub input: &'a StepInput,
    pub calls: &'a mut Vec<Call>,
}

impl Suspension for RigHub<'_> {
    fn get_hub_world_matrix(&mut self) -> Mat44f {
        self.input.hub_world_matrix()
    }

    fn get_point_velocity(&mut self, _p: &Vec3f) -> Vec3f {
        let v = self.input.hub_velocity;
        Vec3f::new(v[0], v[1], v[2])
    }

    fn add_force_at_pos(
        &mut self,
        force: &Vec3f,
        pos: &Vec3f,
        driven: bool,
        add_to_steer_torque: bool,
    ) {
        self.calls.push(Call {
            kind: 1,
            a: arr(force),
            b: arr(pos),
            flags: driven as u32 | (add_to_steer_torque as u32) << 1,
            ..Call::default()
        });
    }

    fn add_torque(&mut self, torque: &Vec3f) {
        self.calls.push(Call {
            kind: 2,
            a: arr(torque),
            ..Call::default()
        });
    }

    fn get_hub_angular_velocity(&mut self) -> Vec3f {
        let v = self.input.hub_angular_velocity;
        Vec3f::new(v[0], v[1], v[2])
    }

    fn add_local_force_and_torque(&mut self, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        self.calls.push(Call {
            kind: 3,
            a: arr(force),
            b: arr(torque),
            c: arr(drive_torque),
            ..Call::default()
        });
    }
}

/// The rig's road: a plane at `ground_y` straight below the ray's origin.
pub struct RigGround<'a>(pub &'a StepInput);

impl RayTrackCollisionProvider for RigGround<'_> {
    fn ray_cast(&self, org: &Vec3f, _dir: &Vec3f, _length: f32) -> Option<RayCastResult> {
        let input = self.0;
        input.has_hit.then(|| RayCastResult {
            surface_def: input.surface_def(),
            pos: Vec3f::new(org.x, input.ground_y, org.z),
            normal: Vec3f::new(
                input.ground_normal[0],
                input.ground_normal[1],
                input.ground_normal[2],
            ),
        })
    }
}

/// The rig's car: constants from the [`StepInput`]; records the body force.
pub struct RigCar<'a> {
    pub input: &'a StepInput,
    pub calls: Vec<Call>,
}

impl TyreCar for RigCar<'_> {
    fn torque_mode_ex(&self) -> TorqueModeEx {
        TorqueModeEx::from_i32(self.input.torque_mode)
    }
    fn is_sleeping(&self) -> bool {
        self.input.car_sleeping
    }
    fn get_speed(&self) -> f32 {
        self.input.car_speed
    }
    fn dynamic_grip_level(&self) -> f32 {
        self.input.dynamic_grip_level
    }
    fn tyre_consumption_rate(&self) -> f32 {
        self.input.tyre_consumption_rate
    }
    fn mechanical_damage_rate(&self) -> f32 {
        self.input.mechanical_damage_rate
    }
    fn ambient_temperature(&self) -> f32 {
        self.input.ambient_temperature
    }
    fn road_temperature(&self) -> f32 {
        self.input.road_temperature
    }
    fn allow_tyre_blankets(&self) -> bool {
        self.input.allow_tyre_blankets
    }
    fn body_get_velocity(&mut self) -> Vec3f {
        let v = self.input.body_velocity;
        Vec3f::new(v[0], v[1], v[2])
    }
    fn body_get_mass(&mut self) -> f32 {
        self.input.body_mass
    }
    fn body_add_force_at_local_pos(&mut self, force: &Vec3f, p: &Vec3f) {
        self.calls.push(Call {
            kind: 4,
            a: arr(force),
            b: arr(p),
            ..Call::default()
        });
    }
}

/// `TyreStatus` members in struct order, as recorded.
pub const STATUS_FIELDS: [&str; 40] = [
    "depth",
    "load",
    "camberRAD",
    "slipAngleRAD",
    "slipRatio",
    "angularVelocity",
    "Fy",
    "Fx",
    "Mz",
    "isLocked",
    "slipFactor",
    "ndSlip",
    "distToGround",
    "Dy",
    "Dx",
    "D",
    "dirtyLevel",
    "rollingResistence",
    "thermalInput",
    "feedbackTorque",
    "loadedRadius",
    "effectiveRadius",
    "liveRadius",
    "pressureStatic",
    "pressureDynamic",
    "virtualKM",
    "lastTempIMO0",
    "lastTempIMO1",
    "lastTempIMO2",
    "peakSA",
    "grain",
    "blister",
    "inflation",
    "flatSpot",
    "lastGrain",
    "lastBlister",
    "normalizedSlideX",
    "normalizedSlideY",
    "finalDY",
    "wearMult",
];

/// `Tyre` members recorded after the status.
pub const TYRE_FIELDS: [&str; 31] = [
    "worldPosition.x",
    "worldPosition.y",
    "worldPosition.z",
    "unmodifiedContactPoint.x",
    "unmodifiedContactPoint.y",
    "unmodifiedContactPoint.z",
    "contactPoint.x",
    "contactPoint.y",
    "contactPoint.z",
    "contactNormal.x",
    "contactNormal.y",
    "contactNormal.z",
    "roadRight.x",
    "roadRight.y",
    "roadRight.z",
    "roadHeading.x",
    "roadHeading.y",
    "roadHeading.z",
    "hasSurfaceDef",
    "absOverride",
    "oldAngularVelocity",
    "totalSlideVelocity",
    "slidingVelocityY",
    "slidingVelocityX",
    "roadVelocityX",
    "roadVelocityY",
    "totalHubVelocity",
    "rSlidingVelocityX",
    "rSlidingVelocityY",
    "tyreBlanketsOn",
    "localMX",
];

/// `TyreThermalModel` members recorded before the 36 patches.
pub const THERMAL_FIELDS: [&str; 5] = [
    "thermal.phase",
    "thermal.coreTemp",
    "thermal.thermalMultD",
    "thermal.practicalTemp",
    "thermal.coreTInput",
];

/// Column names of [`snapshot`]: the calls, then `TyreStatus`, the `Tyre` members, the
/// wheel rotation matrix and the thermal model.
pub fn output_fields() -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..MAX_CALLS {
        names.push(format!("call{i}.kind"));
        for vector in ["a", "b", "c"] {
            for axis in ["x", "y", "z"] {
                names.push(format!("call{i}.{vector}{axis}"));
            }
        }
        names.push(format!("call{i}.flags"));
    }
    names.push("calls".to_string());
    names.extend(STATUS_FIELDS.iter().map(|n| format!("status.{n}")));
    names.extend(TYRE_FIELDS.iter().map(|n| n.to_string()));
    for i in 0..16 {
        names.push(format!("localWheelRotation.M{}{}", i / 4 + 1, i % 4 + 1));
    }
    names.extend(THERMAL_FIELDS.iter().map(|n| n.to_string()));
    for i in 0..36 {
        names.push(format!("thermal.patch{i}.T"));
        names.push(format!("thermal.patch{i}.inputT"));
    }
    names
}

/// How a recorded word is to be read: `d` f64 bits, `i` an integer / flag, `f` f32 bits.
pub fn field_kind(name: &str) -> char {
    match name {
        "status.virtualKM" | "status.grain" | "status.blister" | "status.flatSpot"
        | "thermal.phase" => 'd',
        "status.isLocked" | "hasSurfaceDef" | "tyreBlanketsOn" | "calls" => 'i',
        _ if name.ends_with(".kind") || name.ends_with(".flags") => 'i',
        _ if name.starts_with("in_") => match name {
            "in_driven"
            | "in_set_av"
            | "in_set_blankets"
            | "in_ext_active"
            | "in_has_hit"
            | "in_has_car"
            | "in_torque_mode"
            | "in_car_sleeping"
            | "in_allow_tyre_blankets" => 'i',
            _ => 'f',
        },
        _ => 'f',
    }
}

/// Human-readable value of a recorded word.
pub fn describe_word(name: &str, word: u64) -> String {
    match field_kind(name) {
        'd' => format!("{:?} ({word:#018x})", f64::from_bits(word)),
        'i' => format!("{word}"),
        _ => format!("{:?} ({:#010x})", fw(word), word as u32),
    }
}

/// The recorded state of the tyre after a step, in [`output_fields`] order.
pub fn snapshot(tyre: &VanillaTyre, calls: &[Call]) -> Vec<u64> {
    let mut w = Vec::new();
    for i in 0..MAX_CALLS {
        let call = calls.get(i).copied().unwrap_or_default();
        w.push(call.kind as u64);
        for vector in [call.a, call.b, call.c] {
            w.extend(vector.iter().map(|&x| f(x)));
        }
        w.push(call.flags as u64);
    }
    w.push(calls.len() as u64);

    let s = &tyre.status;
    w.extend([
        f(s.depth),
        f(s.load),
        f(s.camber_rad),
        f(s.slip_angle_rad),
        f(s.slip_ratio),
        f(s.angular_velocity),
        f(s.fy),
        f(s.fx),
        f(s.mz),
        b(s.is_locked),
        f(s.slip_factor),
        f(s.nd_slip),
        f(s.dist_to_ground),
        f(s.dy),
        f(s.dx),
        f(s.d),
        f(s.dirty_level),
        f(s.rolling_resistence),
        f(s.thermal_input),
        f(s.feedback_torque),
        f(s.loaded_radius),
        f(s.effective_radius),
        f(s.live_radius),
        f(s.pressure_static),
        f(s.pressure_dynamic),
        s.virtual_km.to_bits(),
        f(s.last_temp_imo[0]),
        f(s.last_temp_imo[1]),
        f(s.last_temp_imo[2]),
        f(s.peak_sa),
        s.grain.to_bits(),
        s.blister.to_bits(),
        f(s.inflation),
        s.flat_spot.to_bits(),
        f(s.last_grain),
        f(s.last_blister),
        f(s.normalized_slide_x),
        f(s.normalized_slide_y),
        f(s.final_dy),
        f(s.wear_mult),
    ]);
    for v in [
        &tyre.world_position,
        &tyre.unmodified_contact_point,
        &tyre.contact_point,
        &tyre.contact_normal,
        &tyre.road_right,
        &tyre.road_heading,
    ] {
        w.extend([f(v.x), f(v.y), f(v.z)]);
    }
    w.extend([
        b(tyre.surface_def.is_some()),
        f(tyre.abs_override),
        f(tyre.old_angular_velocity),
        f(tyre.total_slide_velocity),
        f(tyre.sliding_velocity_y),
        f(tyre.sliding_velocity_x),
        f(tyre.road_velocity_x),
        f(tyre.road_velocity_y),
        f(tyre.total_hub_velocity),
        f(tyre.r_sliding_velocity_x),
        f(tyre.r_sliding_velocity_y),
        b(tyre.tyre_blankets_on),
        f(tyre.local_mx),
    ]);
    for row in &tyre.local_wheel_rotation.m {
        w.extend(row.iter().map(|&x| f(x)));
    }
    let t = &tyre.thermal_model;
    w.extend([
        t.phase.to_bits(),
        f(t.core_temp),
        f(t.thermal_mult_d),
        f(t.practical_temp),
        f(t.core_t_input),
    ]);
    for patch in &t.patches {
        w.push(f(patch.t));
        w.push(f(patch.input_t));
    }
    w
}

/// The inverse of [`snapshot`]: puts a tyre back into the state a snapshot was taken from.
/// The recorded calls are not state and are skipped; `surfaceDef` is rebuilt by the next
/// step anyway. Everything a compound or the car's files set (data, curves, the force
/// model) is left as it is, so the tyre must have been built the same way.
pub fn restore(tyre: &mut VanillaTyre, words: &[u64]) {
    let mut at = MAX_CALLS * 11 + 1;
    let mut next = || {
        let word = words[at];
        at += 1;
        word
    };
    let s = &mut tyre.status;
    s.depth = fw(next());
    s.load = fw(next());
    s.camber_rad = fw(next());
    s.slip_angle_rad = fw(next());
    s.slip_ratio = fw(next());
    s.angular_velocity = fw(next());
    s.fy = fw(next());
    s.fx = fw(next());
    s.mz = fw(next());
    s.is_locked = next() != 0;
    s.slip_factor = fw(next());
    s.nd_slip = fw(next());
    s.dist_to_ground = fw(next());
    s.dy = fw(next());
    s.dx = fw(next());
    s.d = fw(next());
    s.dirty_level = fw(next());
    s.rolling_resistence = fw(next());
    s.thermal_input = fw(next());
    s.feedback_torque = fw(next());
    s.loaded_radius = fw(next());
    s.effective_radius = fw(next());
    s.live_radius = fw(next());
    s.pressure_static = fw(next());
    s.pressure_dynamic = fw(next());
    s.virtual_km = f64::from_bits(next());
    s.last_temp_imo = [fw(next()), fw(next()), fw(next())];
    s.peak_sa = fw(next());
    s.grain = f64::from_bits(next());
    s.blister = f64::from_bits(next());
    s.inflation = fw(next());
    s.flat_spot = f64::from_bits(next());
    s.last_grain = fw(next());
    s.last_blister = fw(next());
    s.normalized_slide_x = fw(next());
    s.normalized_slide_y = fw(next());
    s.final_dy = fw(next());
    s.wear_mult = fw(next());
    for v in [
        &mut tyre.world_position,
        &mut tyre.unmodified_contact_point,
        &mut tyre.contact_point,
        &mut tyre.contact_normal,
        &mut tyre.road_right,
        &mut tyre.road_heading,
    ] {
        *v = Vec3f::new(fw(next()), fw(next()), fw(next()));
    }
    let _has_surface_def = next();
    tyre.abs_override = fw(next());
    tyre.old_angular_velocity = fw(next());
    tyre.total_slide_velocity = fw(next());
    tyre.sliding_velocity_y = fw(next());
    tyre.sliding_velocity_x = fw(next());
    tyre.road_velocity_x = fw(next());
    tyre.road_velocity_y = fw(next());
    tyre.total_hub_velocity = fw(next());
    tyre.r_sliding_velocity_x = fw(next());
    tyre.r_sliding_velocity_y = fw(next());
    tyre.tyre_blankets_on = next() != 0;
    tyre.local_mx = fw(next());
    for row in &mut tyre.local_wheel_rotation.m {
        for value in row.iter_mut() {
            *value = fw(next());
        }
    }
    let t = &mut tyre.thermal_model;
    t.phase = f64::from_bits(next());
    t.core_temp = fw(next());
    t.thermal_mult_d = fw(next());
    t.practical_temp = fw(next());
    t.core_t_input = fw(next());
    for patch in &mut t.patches {
        patch.t = fw(next());
        patch.input_t = fw(next());
    }
}

/// A [`VanillaTyre`] on the rig.
pub struct Rig {
    pub tyre: VanillaTyre,
}

impl Rig {
    /// Builds the tyre the way the oracle builds AC's: `Tyre::init` without a car (like the
    /// game's own tyre test bench), then `setCompound(compound)` with the car in place.
    pub fn new(
        data_path: &Path,
        axle: Axle,
        compound: i32,
        first: &StepInput,
    ) -> Result<Rig, String> {
        let mut tyre = VanillaTyre::new();
        let mut calls = Vec::new();
        let mut hub = RigHub {
            input: first,
            calls: &mut calls,
        };
        tyre.init(&mut hub, data_path, axle.tyre_index(), None)?;
        let car = RigCar {
            input: first,
            calls: Vec::new(),
        };
        let car: Option<&dyn TyreCar> = if first.has_car { Some(&car) } else { None };
        if !tyre.set_compound(compound, &mut hub, car) {
            return Err(format!("compound index {compound} does not exist"));
        }
        Ok(Rig { tyre })
    }

    /// Applies the inputs, runs one `step(0.003)` and returns the [`snapshot`].
    pub fn step(&mut self, input: &StepInput) -> Vec<u64> {
        let tyre = &mut self.tyre;
        tyre.inputs.brake_torque = input.brake_torque;
        tyre.inputs.hand_brake_torque = input.hand_brake_torque;
        tyre.inputs.electric_torque = input.electric_torque;
        tyre.abs_override = input.abs_override;
        tyre.ai_mult = input.ai_mult;
        tyre.driven = input.driven;
        tyre.external_inputs.is_active = input.ext_active;
        tyre.external_inputs.load = input.ext_load;
        tyre.external_inputs.slip_angle = input.ext_slip_angle;
        tyre.external_inputs.slip_ratio = input.ext_slip_ratio;
        if let Some(value) = input.set_angular_velocity {
            tyre.status.angular_velocity = value;
        }
        if let Some(on) = input.set_blankets {
            tyre.tyre_blankets_on = on;
        }

        let mut calls = Vec::new();
        let mut car = RigCar {
            input,
            calls: Vec::new(),
        };
        {
            let mut hub = RigHub {
                input,
                calls: &mut calls,
            };
            let ground = RigGround(input);
            let car: Option<&mut dyn TyreCar> = if input.has_car { Some(&mut car) } else { None };
            tyre.step(DT, &mut hub, Some(&ground), car);
        }
        // the body call always comes after the tyre's own hub calls
        calls.append(&mut car.calls);
        snapshot(tyre, &calls)
    }
}

/// A recording made by `tools/tyre_oracle`.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    /// The `key=value` pairs of the first line (`scenario`, `axle`, `car`, `compound`, ...).
    pub header: Vec<(String, String)>,
    pub inputs: Vec<StepInput>,
    /// AC's [`snapshot`] after each step.
    pub outputs: Vec<Vec<u64>>,
}

impl Recording {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The text form: a `# key=value ...` line, the column names, one line of hex words per
    /// step (f32 as 8 digits, f64 as 16).
    pub fn to_text(&self) -> String {
        let mut text = String::from("#");
        for (key, value) in &self.header {
            text.push_str(&format!(" {key}={value}"));
        }
        text.push('\n');
        let mut names = input_fields();
        names.extend(output_fields());
        text.push_str(&names.join(","));
        text.push('\n');
        for (input, output) in self.inputs.iter().zip(&self.outputs) {
            let words = input.to_words().into_iter().chain(output.iter().copied());
            let cells: Vec<String> = words
                .zip(&names)
                .map(|(word, name)| match field_kind(name) {
                    'd' => format!("{word:016x}"),
                    'i' => format!("{word:x}"),
                    _ => format!("{word:08x}"),
                })
                .collect();
            text.push_str(&cells.join(","));
            text.push('\n');
        }
        text
    }

    pub fn parse(text: &str) -> Result<Recording, String> {
        let mut lines = text.lines();
        let first = lines.next().ok_or("empty recording")?;
        let header = first
            .trim_start_matches('#')
            .split_whitespace()
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let mut expected = input_fields();
        let input_count = expected.len();
        expected.extend(output_fields());
        let names: Vec<&str> = lines.next().ok_or("no column line")?.split(',').collect();
        if names != expected {
            return Err(
                "the recording's columns are not this version's (regenerate it with tyre_oracle)"
                    .to_string(),
            );
        }
        let mut recording = Recording {
            header,
            ..Recording::default()
        };
        for (number, line) in lines.enumerate() {
            let words: Result<Vec<u64>, _> = line
                .split(',')
                .map(|cell| u64::from_str_radix(cell, 16))
                .collect();
            let words = words.map_err(|e| format!("step {number}: {e}"))?;
            if words.len() != expected.len() {
                return Err(format!("step {number}: {} columns", words.len()));
            }
            recording
                .inputs
                .push(StepInput::from_words(&words[..input_count]));
            recording.outputs.push(words[input_count..].to_vec());
        }
        Ok(recording)
    }
}

/// A compact recording for a checked-in test: the inputs of every step (only the values
/// that changed since the step before) and one hash of AC's outputs per step.
#[derive(Clone, Debug, Default)]
pub struct Golden {
    pub header: Vec<(String, String)>,
    pub inputs: Vec<StepInput>,
    /// [`output_hash`] of AC's snapshot after each step.
    pub hashes: Vec<u64>,
}

/// FNV-1a over the recorded words, with every NaN replaced by one canonical NaN first (see
/// [`same_value`]).
pub fn output_hash(kinds: &[char], words: &[u64]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for (&kind, &word) in kinds.iter().zip(words) {
        let word = match kind {
            'f' if f32::from_bits(word as u32).is_nan() => 0x7fc0_0000,
            'd' if f64::from_bits(word).is_nan() => 0x7ff8_0000_0000_0000,
            _ => word,
        };
        for byte in word.to_le_bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

impl Golden {
    pub fn from_recording(recording: &Recording) -> Golden {
        let kinds: Vec<char> = output_fields().iter().map(|n| field_kind(n)).collect();
        Golden {
            header: recording.header.clone(),
            inputs: recording.inputs.clone(),
            hashes: recording
                .outputs
                .iter()
                .map(|words| output_hash(&kinds, words))
                .collect(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// One line per step: `column:hex` for every input word that differs from the step
    /// before (all of them on the first line), then `;` and the output hash.
    pub fn to_text(&self) -> String {
        let mut text = String::from("#");
        for (key, value) in &self.header {
            text.push_str(&format!(" {key}={value}"));
        }
        text.push('\n');
        let mut previous: Vec<u64> = Vec::new();
        for (input, hash) in self.inputs.iter().zip(&self.hashes) {
            let words = input.to_words();
            let cells: Vec<String> = words
                .iter()
                .enumerate()
                .filter(|(i, word)| previous.get(*i) != Some(word))
                .map(|(i, word)| format!("{i:x}:{word:x}"))
                .collect();
            text.push_str(&cells.join(","));
            text.push_str(&format!(";{hash:x}"));
            text.push('\n');
            previous = words;
        }
        text
    }

    pub fn parse(text: &str) -> Result<Golden, String> {
        let mut lines = text.lines();
        let first = lines.next().ok_or("empty golden file")?;
        let mut golden = Golden {
            header: first
                .trim_start_matches('#')
                .split_whitespace()
                .filter_map(|pair| pair.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Golden::default()
        };
        let mut words = vec![0u64; input_fields().len()];
        for (number, line) in lines.enumerate() {
            let bad = || format!("golden step {number}: malformed line");
            let (cells, hash) = line.split_once(';').ok_or_else(bad)?;
            for cell in cells.split(',').filter(|c| !c.is_empty()) {
                let (index, word) = cell.split_once(':').ok_or_else(bad)?;
                let index = usize::from_str_radix(index, 16).map_err(|_| bad())?;
                *words.get_mut(index).ok_or_else(bad)? =
                    u64::from_str_radix(word, 16).map_err(|_| bad())?;
            }
            golden.inputs.push(StepInput::from_words(&words));
            golden
                .hashes
                .push(u64::from_str_radix(hash, 16).map_err(|_| bad())?);
        }
        Ok(golden)
    }

    /// Replays the inputs through a fresh [`VanillaTyre`]; `Err` names the first step whose
    /// outputs do not hash to AC's.
    pub fn check(&self, data_path: &Path) -> Result<(), String> {
        let axle = Axle::parse(self.get("axle").ok_or("golden file has no axle")?)?;
        let compound: i32 = self
            .get("compound")
            .and_then(|c| c.parse().ok())
            .ok_or("golden file has no compound index")?;
        let first = self.inputs.first().ok_or("golden file has no steps")?;
        let mut rig = Rig::new(data_path, axle, compound, first)?;
        let kinds: Vec<char> = output_fields().iter().map(|n| field_kind(n)).collect();
        for (step, (input, &expected)) in self.inputs.iter().zip(&self.hashes).enumerate() {
            let got = output_hash(&kinds, &rig.step(input));
            if got != expected {
                return Err(format!(
                    "{} {}: step {step} of {} is not AC's (record the scenario with \
                     tyre_oracle and run tyre_compare to see which value differs)",
                    self.get("scenario").unwrap_or("?"),
                    axle.name(),
                    self.inputs.len()
                ));
            }
        }
        Ok(())
    }
}

/// Where a replay first left AC's recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    pub step: usize,
    pub field: String,
    pub expected: u64,
    pub got: u64,
}

/// The result of replaying a [`Recording`] through [`VanillaTyre`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replay {
    pub steps: usize,
    /// Steps whose every recorded value has exactly AC's bits.
    pub exact_steps: usize,
    /// Steps whose every recorded value is the same as AC's, where "the same" is identical
    /// bits or both NaN (see [`same_value`]).
    pub matching_steps: usize,
    /// The first value that is not the same.
    pub first_divergence: Option<Divergence>,
    /// How many steps each field differed in, for the fields that ever did.
    pub field_mismatches: Vec<(String, usize)>,
}

/// Identical bits, or two NaNs. Which sign and payload a NaN carries depends on the order
/// the compiler happens to give the operands of an addition, which neither C++ nor Rust
/// promises; whether a value *is* NaN (and every branch taken because of it) is compared.
pub fn same_value(kind: char, expected: u64, got: u64) -> bool {
    expected == got
        || match kind {
            'f' => f32::from_bits(expected as u32).is_nan() && f32::from_bits(got as u32).is_nan(),
            'd' => f64::from_bits(expected).is_nan() && f64::from_bits(got).is_nan(),
            _ => false,
        }
}

/// Runs the recorded inputs through a fresh [`VanillaTyre`] and compares every value of
/// every step with AC's.
pub fn replay(recording: &Recording, data_path: &Path) -> Result<Replay, String> {
    let axle = Axle::parse(recording.get("axle").ok_or("recording has no axle")?)?;
    let compound: i32 = recording
        .get("compound")
        .and_then(|c| c.parse().ok())
        .ok_or("recording has no compound index")?;
    let first = recording.inputs.first().ok_or("recording has no steps")?;
    let mut rig = Rig::new(data_path, axle, compound, first)?;
    let names = output_fields();
    let kinds: Vec<char> = names.iter().map(|name| field_kind(name)).collect();
    let mut mismatches = vec![0usize; names.len()];
    let mut result = Replay {
        steps: recording.inputs.len(),
        ..Replay::default()
    };
    for (step, (input, expected)) in recording.inputs.iter().zip(&recording.outputs).enumerate() {
        let got = rig.step(input);
        result.exact_steps += (*expected == got) as usize;
        let mut exact = true;
        for (i, (&e, &g)) in expected.iter().zip(&got).enumerate() {
            if !same_value(kinds[i], e, g) {
                exact = false;
                mismatches[i] += 1;
                if result.first_divergence.is_none() {
                    result.first_divergence = Some(Divergence {
                        step,
                        field: names[i].clone(),
                        expected: e,
                        got: g,
                    });
                }
            }
        }
        result.matching_steps += exact as usize;
    }
    result.field_mismatches = names
        .into_iter()
        .zip(mismatches)
        .filter(|(_, count)| *count > 0)
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_round_trip() {
        let mut input =
            scenarios::scenario("random", Axle::Front, &scenarios::Wheel::default()).unwrap()[777];
        input.set_angular_velocity = Some(12.5);
        input.set_blankets = Some(true);
        let words = input.to_words();
        assert_eq!(words.len(), input_fields().len());
        assert_eq!(StepInput::from_words(&words), input);
    }

    #[test]
    fn snapshot_matches_its_column_names() {
        let tyre = VanillaTyre::new();
        // a tyre that has not been initialised has no patches yet
        assert_eq!(snapshot(&tyre, &[]).len() + 72, output_fields().len());
    }
}

pub mod scenarios;
