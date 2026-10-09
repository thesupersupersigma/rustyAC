// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The cockpit's moving parts, all of which only write node matrices:
//!
//! * `AnalogInstruments` (0x490 bytes; constructor 0x140056480, `initFuel` 0x1400572e0,
//!   `initRPM` 0x140057e70, `initSpeed` 0x1400584e0, `initTurbo` 0x140058930 /
//!   `readTurboSection` 0x140059080, `initWater` 0x140058b40, `initPlaceHolders` 0x140057ae0,
//!   `update` 0x140059770 and its `updateRPM` 0x140059ba0, `updateFuel` 0x140059810,
//!   `updateSpeed` 0x140059e30, `updateTurbo` 0x140059f70, `updateWater` 0x14005a460,
//!   `updatePlaceHolders` 0x140059970): the needles of `data/analog_instruments.ini`.
//! * `GearShiftShake` (0x128 bytes; constructor 0x1401048f0, `update` 0x140104ca0): the lever
//!   `SHIFT_HD` leans to its gear and trembles with the revs.
//! * `CarAnimations` (0xa8 bytes; constructor 0x140060ba0, `update` 0x140062160): the car's own
//!   `car_shift*.ksanim`, `car_steer_extra.ksanim`, the wings of `data/wing_animations.ini` and
//!   a door. (The game has no code for wipers: `car_wiper.ksanim` is never opened.)
//! * `RotatingObjects` (0x78 bytes; constructor 0x1400ba560, `update` 0x1400bae20): the
//!   `[ROTATING_OBJECT_n]` of `data/extra_animations.ini` (a fan).

use std::path::Path;

use rustyac_math::{sinf, sqrtf};
use rustyac_physics::curve::Curve;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::MsvcRand;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::animator::{Animation, AnimationPlayer};
use crate::scene::{NodeId, Scene};
use crate::state::CarPhysicsState;

const DEG: f32 = f32::from_bits(0x3c8e_f998);

/// `AnalogNeedle` (0x58 bytes).
#[derive(Clone, Copy)]
struct AnalogNeedle {
    target: Option<NodeId>,
    zero: f32,
    step: f32,
    min: f32,
    base_matrix: Mat44f,
}

impl AnalogNeedle {
    fn new() -> AnalogNeedle {
        AnalogNeedle { target: None, zero: 0.0, step: 0.0, min: 0.0, base_matrix: Mat44f { m: [[0.0; 4]; 4] } }
    }

    /// The needle's node, and its own matrix as the turn starts from.
    fn aim(&mut self, scene: &Scene, target: Option<NodeId>) {
        self.target = target;
        if let Some(n) = target {
            self.base_matrix = scene.nodes[n].matrix;
        }
    }

    fn apply(&self, scene: &mut Scene, angle: f32) {
        if let Some(n) = self.target {
            let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), angle);
            scene.nodes[n].matrix = xm_matrix_multiply(&r, &self.base_matrix);
        }
    }
}

pub struct AnalogInstruments {
    max_rpm_recorded: f32,
    current_rpm: f32,
    fuel_lut: Curve,
    rpm_lut: Curve,
    speed_lut: Curve,
    water_lut: Curve,
    fuel: AnalogNeedle,
    rpm: AnalogNeedle,
    rpm_max: AnalogNeedle,
    speed: AnalogNeedle,
    water: AnalogNeedle,
    turbos: Vec<AnalogNeedle>,
    turbo_limiters: Vec<AnalogNeedle>,
    /// the smoothed boost and the highest seen
    turbo_values: Vec<(f32, f32)>,
    turbo_use_bar: bool,
    place_holders: Vec<AnalogNeedle>,
    place_holder_set: bool,
}

impl Default for AnalogInstruments {
    fn default() -> AnalogInstruments {
        AnalogInstruments {
            max_rpm_recorded: 0.0,
            current_rpm: 0.0,
            fuel_lut: Curve::new(),
            rpm_lut: Curve::new(),
            speed_lut: Curve::new(),
            water_lut: Curve::new(),
            fuel: AnalogNeedle::new(),
            rpm: AnalogNeedle::new(),
            rpm_max: AnalogNeedle::new(),
            speed: AnalogNeedle::new(),
            water: AnalogNeedle::new(),
            turbos: Vec::new(),
            turbo_limiters: Vec::new(),
            turbo_values: Vec::new(),
            turbo_use_bar: false,
            place_holders: Vec::new(),
            place_holder_set: false,
        }
    }
}

fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

impl AnalogInstruments {
    /// `AnalogInstruments::AnalogInstruments` 0x140056480.
    pub fn new(scene: &Scene, folder: &Path, car_node: NodeId, body_transform: NodeId) -> Result<AnalogInstruments, String> {
        let mut a = AnalogInstruments::default();
        let Ok(ini) = IniReader::load(&folder.join("data/analog_instruments.ini")) else {
            return Ok(a);
        };
        let get = |section: &str, key: &str| ini.get_float(section, key).unwrap_or(0.0);
        let find = |name: &str| scene.find_child_by_name(car_node, name, true);
        // initFuel
        if ini.has_section("FUEL_INDICATOR") {
            let name = ini.get_string("FUEL_INDICATOR", "OBJECT_NAME");
            let target = scene.find_child_by_name(body_transform, &name, true);
            if target.is_none() {
                return Err(format!("ERROR: Cannot find FUEL_INDICATOR target: {name}"));
            }
            a.fuel.zero = get("FUEL_INDICATOR", "ZERO");
            a.fuel.step = get("FUEL_INDICATOR", "STEP");
            a.fuel.aim(scene, target);
            if ini.has_key("FUEL_INDICATOR", "MIN_VALUE") {
                a.fuel.min = get("FUEL_INDICATOR", "MIN_VALUE");
            }
            if ini.has_key("FUEL_INDICATOR", "LUT") {
                a.fuel_lut = ini.get_curve("FUEL_INDICATOR", "LUT").unwrap_or_else(|_| Curve::new());
            }
        }
        // initRPM
        a.rpm.aim(scene, find("ARROW_RPM"));
        a.rpm_max.aim(scene, find("ARROW_LIMITER"));
        a.rpm.step = f32::from_bits(0x3ca3_d70a);
        a.rpm.zero = -110.0;
        if ini.has_section("RPM_INDICATOR") {
            a.rpm.step = get("RPM_INDICATOR", "STEP");
            a.rpm.zero = get("RPM_INDICATOR", "ZERO");
            a.rpm.min = get("RPM_INDICATOR", "MIN_VALUE");
            if ini.has_key("RPM_INDICATOR", "LUT") {
                a.rpm_lut = ini.get_curve("RPM_INDICATOR", "LUT").unwrap_or_else(|_| Curve::new());
            }
            let n = ini.get_string("RPM_INDICATOR", "OBJECT_NAME");
            if !n.is_empty() {
                a.rpm.aim(scene, find(&n));
            }
            let n = ini.get_string("RPM_INDICATOR", "OBJECT_NAME_MAX");
            if !n.is_empty() {
                a.rpm_max.aim(scene, find(&n));
            }
        }
        // initSpeed
        a.speed.aim(scene, find("ARROW_SPEED"));
        a.speed.step = f32::from_bits(0x3ca3_d70a);
        a.speed.zero = -110.0;
        if ini.has_section("SPEED_INDICATOR") {
            a.speed.step = get("SPEED_INDICATOR", "STEP");
            a.speed.zero = get("SPEED_INDICATOR", "ZERO");
            let n = ini.get_string("SPEED_INDICATOR", "OBJECT_NAME");
            if !n.is_empty() {
                a.speed.aim(scene, find(&n));
            }
        }
        let lut = folder.join("data/analog_speed_curve.lut");
        if lut.is_file() {
            let _ = a.speed_lut.load(&lut);
        }
        // initTurbo
        a.read_turbo_section(scene, &ini, car_node, "TURBO_INDICATOR");
        let mut n = 0;
        loop {
            let section = format!("TURBO_INDICATOR_{n}");
            if !ini.has_section(&section) {
                break;
            }
            a.read_turbo_section(scene, &ini, car_node, &section);
            n += 1;
        }
        // initWater
        if ini.has_section("WATER_TEMP") {
            let name = ini.get_string("WATER_TEMP", "OBJECT_NAME");
            a.water.aim(scene, find(&name));
            a.water.zero = get("WATER_TEMP", "ZERO");
            a.water.step = get("WATER_TEMP", "STEP");
            a.water.min = get("WATER_TEMP", "MIN_VALUE");
            a.water_lut = ini.get_curve("WATER_TEMP", "LUT").unwrap_or_else(|_| Curve::new());
        }
        // initPlaceHolders
        let mut n = 0;
        loop {
            let section = format!("PLACE_HOLDER_{n}");
            if !ini.has_section(&section) {
                break;
            }
            let mut needle = AnalogNeedle::new();
            needle.aim(scene, find(&ini.get_string(&section, "OBJECT_NAME")));
            needle.zero = get(&section, "ZERO");
            needle.step = get(&section, "STEP");
            needle.min = get(&section, "MIN_VALUE");
            a.place_holders.push(needle);
            n += 1;
        }
        Ok(a)
    }

    /// `readTurboSection` 0x140059080.
    fn read_turbo_section(&mut self, scene: &Scene, ini: &IniReader, car_node: NodeId, section: &str) {
        let find = |name: &str| scene.find_child_by_name(car_node, name, true);
        let mut turbo = AnalogNeedle::new();
        let mut limiter = AnalogNeedle::new();
        turbo.aim(scene, find("ARROW_TURBO"));
        limiter.aim(scene, find("ARROW_TURBO_LIMITER"));
        turbo.step = f32::from_bits(0x3ca3_d70a);
        turbo.zero = -110.0;
        if ini.has_section(section) {
            turbo.step = ini.get_float(section, "STEP").unwrap_or(0.0);
            turbo.zero = ini.get_float(section, "ZERO").unwrap_or(0.0);
            turbo.min = ini.get_float(section, "MIN_VALUE").unwrap_or(0.0);
            self.turbo_use_bar = ini.get_int(section, "USE_BAR").unwrap_or(0) != 0;
            let n = ini.get_string(section, "OBJECT_NAME");
            if !n.is_empty() {
                turbo.aim(scene, find(&n));
            }
            if ini.has_key(section, "OBJECT_NAME_MAX") {
                let n = ini.get_string(section, "OBJECT_NAME_MAX");
                if !n.is_empty() {
                    limiter.aim(scene, find(&n));
                }
            }
        }
        self.turbos.push(turbo);
        self.turbo_limiters.push(limiter);
        self.turbo_values.push((0.0, 0.0));
    }

    /// `AnalogInstruments::update` 0x140059770; `focused`: the car is the one the cameras
    /// follow.
    pub fn update(&mut self, scene: &mut Scene, s: &CarPhysicsState, dt: f32, focused: bool) {
        if !focused {
            return;
        }
        // updateRPM
        let k = clamp01(dt * 10.0);
        self.current_rpm = ((s.engine_rpm - self.current_rpm) * k) + self.current_rpm;
        let mut clamped = 0.0f32;
        if self.rpm.target.is_some() {
            clamped = if self.current_rpm > 40000.0 {
                40000.0
            } else if self.current_rpm >= self.rpm.min {
                self.current_rpm
            } else {
                self.rpm.min
            };
            self.max_rpm_recorded = if clamped < self.max_rpm_recorded { self.max_rpm_recorded } else { clamped };
        }
        let (mut a1, mut a2) = (0.0f32, 0.0f32);
        if self.rpm_lut.get_count() != 0 {
            if self.rpm.target.is_some() {
                a1 = self.rpm_lut.get_value(clamped);
            }
            if self.rpm_max.target.is_some() {
                a2 = self.rpm_lut.get_value(self.max_rpm_recorded);
            }
        } else {
            if self.rpm.target.is_some() {
                a1 = ((clamped - self.rpm.min) * self.rpm.step) + self.rpm.zero;
            }
            if self.rpm_max.target.is_some() {
                a2 = ((self.max_rpm_recorded - self.rpm.min) * self.rpm.step) + self.rpm.zero;
            }
        }
        self.rpm.apply(scene, a1 * DEG);
        self.rpm_max.apply(scene, a2 * DEG);
        // updateFuel
        if self.fuel.target.is_some() {
            let v = if self.fuel.min < s.fuel { s.fuel } else { self.fuel.min };
            let a = if self.fuel_lut.get_count() != 0 { self.fuel_lut.get_value(v) * DEG } else { (((v - self.fuel.min) * self.fuel.step) + self.fuel.zero) * DEG };
            self.fuel.apply(scene, a);
        }
        // updateSpeed
        if self.speed.target.is_some() {
            let v = s.speed * f32::from_bits(0x4066_6666);
            let a = if self.speed_lut.get_count() != 0 { self.speed_lut.get_value(v) * DEG } else { ((v * self.speed.step) + self.speed.zero) * DEG };
            self.speed.apply(scene, a);
        }
        // updateTurbo
        let x = dt * 10.0;
        for i in 0..self.turbos.len() {
            let k = clamp01(x);
            self.turbo_values[i].0 = ((s.turbo_boost - self.turbo_values[i].0) * k) + self.turbo_values[i].0;
            let t = self.turbos[i];
            if t.target.is_some() {
                let b = if self.turbo_use_bar { s.turbo_boost + 1.0 } else { s.turbo_boost };
                let d = b - t.min;
                let m = if d >= 0.0 { d } else { 0.0 };
                t.apply(scene, ((m * t.step) + t.zero) * DEG);
            }
            let limiter = self.turbo_limiters[i];
            if limiter.target.is_some() {
                let sm = self.turbo_values[i].0;
                let cl = if sm > 100.0 {
                    100.0
                } else if sm >= t.min {
                    sm
                } else {
                    t.min
                };
                self.turbo_values[i].1 = if cl < self.turbo_values[i].1 { self.turbo_values[i].1 } else { cl };
                limiter.apply(scene, ((self.turbo_values[i].1 * t.step) + t.zero) * DEG);
            }
        }
        // updateWater
        if self.water.target.is_some() {
            let v = if self.water.min < s.water { s.water } else { self.water.min };
            let a = if self.water_lut.get_count() != 0 { self.water_lut.get_value(v) * DEG } else { (((v - self.water.min) * self.water.step) + self.water.zero) * DEG };
            self.water.apply(scene, a);
        }
        if !self.place_holder_set {
            for needle in &self.place_holders {
                needle.apply(scene, needle.zero * DEG);
            }
            self.place_holder_set = true;
        }
    }
}

/// `GearShiftShake`.
pub struct GearShiftShake {
    target: Option<NodeId>,
    base_matrix: Mat44f,
    /// `SignalGenerator3D`: what the three sine generators run at, and where they are
    freq_scale: [f32; 3],
    sin_freq_scale: [f32; 3],
    sin_value: [i32; 3],
}

const GEAR_ROTATIONS: [[f32; 3]; 9] = [[-15.0, 0.0, 30.0], [0.0, 0.0, 0.0], [15.0, 0.0, -15.0], [-15.0, 0.0, -15.0], [15.0, 0.0, 0.0], [-15.0, 0.0, 0.0], [15.0, 0.0, 15.0], [-15.0, 0.0, 15.0], [15.0, 0.0, 30.0]];

impl GearShiftShake {
    /// `GearShiftShake::GearShiftShake` 0x1401048f0.
    pub fn new(scene: &Scene, car_node: NodeId) -> GearShiftShake {
        let target = scene.find_child_by_name(car_node, "SHIFT_HD", true);
        if target.is_some() {
            println!("GearShiftShake: SHIFT_HD found");
        }
        GearShiftShake {
            target,
            base_matrix: target.map(|n| scene.nodes[n].matrix).unwrap_or(Mat44f { m: [[0.0; 4]; 4] }),
            freq_scale: [100.0; 3],
            sin_freq_scale: [1.0; 3],
            // the generators' own constructor steps two of them: 0.5 s and 1.5 s at 1 kHz
            sin_value: [500, 1500, 0],
        }
    }

    /// `GearShiftShake::update` 0x140104ca0. Three numbers are drawn from the C runtime's
    /// generator every time (they change nothing: the generator's random blend is 0).
    pub fn update(&mut self, scene: &mut Scene, s: &CarPhysicsState, dt: f32, rand: &mut MsvcRand) {
        let Some(target) = self.target else {
            return;
        };
        for i in 0..3 {
            let r = rand.next();
            let dti = (((r as f32 * f32::from_bits(0x3800_0100)) * dt - dt) * 0.0) + dt;
            self.sin_value[i] = self.sin_value[i].wrapping_add(((dti * 1000.0) * self.sin_freq_scale[i]) as i32);
        }
        // getValue: the generators take this frame's frequency for the next step
        self.sin_freq_scale = self.freq_scale;
        let scale = f32::from_bits(0x3c23_d70a);
        let wave = |v: i32| sinf(v as f32 * f32::from_bits(0x3a83_126f));
        let (z, y, x) = (wave(self.sin_value[2]), wave(self.sin_value[1]), wave(self.sin_value[0]));
        let (mut vx, mut vy, mut vz) = (x * scale, y * scale, z * scale);
        self.freq_scale = [s.engine_rpm * f32::from_bits(0x3dcc_cccd); 3];
        let len2 = ((vy * vy) + (vx * vx)) + (vz * vz);
        let len = if len2 == 0.0 || len2.is_nan() { 0.0 } else { sqrtf(len2) };
        let shake_angle = (s.gas + f32::from_bits(0x3e4c_cccd)) * len;
        let l2 = sqrtf(len2);
        if l2 != 0.0 && !l2.is_nan() {
            let inv = 1.0 / l2;
            vx *= inv;
            vy *= inv;
            vz *= inv;
        }
        let t = GEAR_ROTATIONS.get(s.gear as usize).copied().unwrap_or([0.0; 3]);
        let b = f32::from_bits(0x3f4c_cccd);
        let (mut rx, mut ry, mut rz) = (((0.0 - t[0]) * b) + t[0], ((0.0 - t[1]) * b) + t[1], ((0.0 - t[2]) * b) + t[2]);
        let rl2 = ((ry * ry) + (rx * rx)) + (rz * rz);
        let rlen = if rl2 == 0.0 || rl2.is_nan() { 0.0 } else { sqrtf(rl2) };
        let root = sqrtf(rl2);
        if root != 0.0 && !root.is_nan() {
            let inv = 1.0 / root;
            ry *= inv;
            rx *= inv;
            rz *= inv;
        }
        let shake = Mat44f::create_from_axis_angle(&Vec3f::new(vx, vy, vz), shake_angle);
        let gear = Mat44f::create_from_axis_angle(&Vec3f::new(rx, ry, rz), rlen * DEG);
        let m = xm_matrix_multiply(&gear, &shake);
        scene.nodes[target].matrix = xm_matrix_multiply(&m, &self.base_matrix);
    }
}

/// `WingAnimation` (0x48 bytes).
struct WingAnimation {
    player: AnimationPlayer,
    zero: f32,
    one: f32,
    wing_index: u32,
    inverted: bool,
    speed: f32,
}

/// What the car's own animations follow of the driver: the positions of his shift players.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverShiftPositions {
    pub shift: f32,
    pub shift_down: f32,
}

#[derive(Default)]
pub struct CarAnimations {
    shift_up: Option<AnimationPlayer>,
    shift_down: Option<AnimationPlayer>,
    shift: Option<AnimationPlayer>,
    car_steer_extra: Option<AnimationPlayer>,
    wings: Vec<WingAnimation>,
    door: Option<AnimationPlayer>,
    /// 1: the door opens (the player's car in the start camera), 0: it shuts
    pub door_animation_target: f32,
}

impl CarAnimations {
    /// `CarAnimations::CarAnimations` 0x140060ba0. `driver_eyes_x`: `car.ini [GRAPHICS]
    /// DRIVEREYES` x, which picks the door.
    pub fn new(scene: &Scene, folder: &Path, body_transform: NodeId, driver_eyes_x: f32) -> CarAnimations {
        let animations = folder.join("animations");
        let load_one = |name: &str| -> Option<AnimationPlayer> {
            let path = animations.join(name);
            if !path.is_file() {
                return None;
            }
            let a = Animation::load(&path);
            Some(AnimationPlayer::new(&a, scene, body_transform))
        };
        let mut car = CarAnimations { shift_down: load_one("car_shift_dw.ksanim"), shift_up: load_one("car_shift_up.ksanim"), shift: load_one("car_shift.ksanim"), car_steer_extra: load_one("car_steer_extra.ksanim"), ..CarAnimations::default() };
        if let Ok(ini) = IniReader::load(&folder.join("data/wing_animations.ini")) {
            let mut version = 1;
            if ini.has_section("HEADER") {
                version = ini.get_int("HEADER", "VERSION").unwrap_or(0);
            }
            let mut n = 0;
            loop {
                let section = format!("ANIMATION_{n}");
                if !ini.has_section(&section) {
                    break;
                }
                let a = Animation::load(&animations.join(ini.get_string(&section, "FILE")));
                let mut wing = WingAnimation {
                    player: AnimationPlayer::new(&a, scene, body_transform),
                    zero: ini.get_float(&section, "MIN").unwrap_or(0.0),
                    one: ini.get_float(&section, "MAX").unwrap_or(0.0),
                    wing_index: ini.get_int(&section, "WING").unwrap_or(0) as u32,
                    inverted: false,
                    speed: 0.0,
                };
                if ini.has_key(&section, "INVERTED") {
                    wing.inverted = ini.get_int(&section, "INVERTED").unwrap_or(0) != 0;
                }
                if version >= 2 {
                    wing.speed = ini.get_float(&section, "SPEED").unwrap_or(0.0);
                }
                car.wings.push(wing);
                n += 1;
            }
        }
        car.door = load_one(if 0.0 > driver_eyes_x { "car_door_R.ksanim" } else { "car_door_L.ksanim" });
        car
    }

    /// `CarAnimations::update` 0x140062160. `steer_lock`: `CarPhysicsInfo::steerLock`;
    /// `wing_angles`: `CarAvatar::wingsStatus[i].angle`.
    pub fn update(&mut self, scene: &mut Scene, s: &CarPhysicsState, dt: f32, steer_lock: f32, wing_angles: &[f32], driver: Option<DriverShiftPositions>) {
        if let Some(player) = &mut self.car_steer_extra {
            player.set_current_pos(scene, 0.5 - ((s.steer / steer_lock) * 0.5), false);
        }
        for w in &mut self.wings {
            let Some(&angle) = wing_angles.get(w.wing_index as usize) else {
                continue;
            };
            let mut pos = if w.inverted {
                let v = if w.one < angle { w.one } else { angle };
                (((v - w.zero) / w.one) - 1.0).abs()
            } else {
                (angle - w.zero) / w.one
            };
            pos = clamp01(pos);
            if w.speed > 0.0 {
                let step = w.speed * dt;
                let cur = w.player.get_current_pos();
                if (cur - pos).abs() > step {
                    pos = if pos > cur { cur + step } else { cur - step };
                }
            }
            w.player.set_current_pos(scene, pos, false);
        }
        if let Some(d) = driver {
            if let Some(player) = &mut self.shift_up {
                player.set_current_pos(scene, d.shift, false);
            }
            if let Some(player) = &mut self.shift_down {
                player.set_current_pos(scene, d.shift_down, false);
            }
            if let Some(player) = &mut self.shift {
                player.set_current_pos(scene, d.shift, false);
            }
        }
        if let Some(player) = &mut self.door {
            let cur = player.get_current_pos();
            let diff = self.door_animation_target - cur;
            let mut pos = self.door_animation_target;
            if diff.abs() >= dt {
                let sign = if diff > 0.0 {
                    1.0
                } else if diff == 0.0 {
                    0.0
                } else {
                    -1.0
                };
                pos = (sign * dt) + cur;
            }
            player.set_current_pos(scene, pos, false);
        }
    }
}

/// `RotatingObject` (0x18 bytes).
struct RotatingObject {
    target: Option<NodeId>,
    /// degrees a second
    rotation_step: f32,
    axis: [f32; 3],
}

#[derive(Default)]
pub struct RotatingObjects {
    objects: Vec<RotatingObject>,
}

impl RotatingObjects {
    /// `RotatingObjects::RotatingObjects` 0x1400ba560.
    pub fn new(scene: &Scene, folder: &Path, car_node: NodeId) -> RotatingObjects {
        let mut r = RotatingObjects::default();
        if let Ok(ini) = IniReader::load(&folder.join("data/extra_animations.ini")) {
            let mut n = 0;
            loop {
                let section = format!("ROTATING_OBJECT_{n}");
                if !ini.has_section(&section) {
                    break;
                }
                r.objects.push(RotatingObject {
                    target: scene.find_child_by_name(car_node, &ini.get_string(&section, "NAME"), true),
                    axis: ini.get_float3(&section, "AXIS").unwrap_or([0.0; 3]),
                    rotation_step: ini.get_int(&section, "RPM").unwrap_or(0) as f32 * 6.0,
                });
                n += 1;
            }
        }
        r
    }

    /// `RotatingObjects::update` 0x1400bae20; `dt` times the replay's multiplier in a replay.
    pub fn update(&self, scene: &mut Scene, s: &CarPhysicsState, dt: f32) {
        if 0.0 >= s.engine_life_left || 0.0 >= s.fuel {
            return;
        }
        for o in &self.objects {
            // (the game has no test here: a node that is not found stops it)
            let Some(target) = o.target else {
                continue;
            };
            let r = Mat44f::create_from_axis_angle(&Vec3f::new(o.axis[0], o.axis[1], o.axis[2]), (dt * o.rotation_step) * DEG);
            scene.nodes[target].matrix = xm_matrix_multiply(&r, &scene.nodes[target].matrix);
        }
    }
}
