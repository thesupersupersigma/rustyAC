// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Aerodynamics: the aero slot [`AeroModel`] and [`VanillaAero`], AC's `AeroMap` with its
//! `Wing`s (angle-of-attack and ride-height tables, damage factor, yaw sensitivity), the wing
//! controllers of active aero (`DynamicWingController`), `DRS`, and the car-level helpers the
//! wings call (`Car::getGroundWindVector`, `RaceEngineer::getPointGroundHeight`,
//! `Car::updateAirPressure` with the slipstream hook).
//!
//! All arithmetic is single precision; the order of every sum and product is the machine
//! code's (`acs.exe` 1.16.4).

use std::path::Path;

use super::body::ForceSource;
use super::chassis::RollingChassis;
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::{append_path, IniReader};
use crate::math::{atanf, sinf, sqrtf};
use crate::vecmath::Vec3f;

/// AC's `WingData` (0x250 bytes): what `aero.ini` says about one wing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WingData {
    /// `name`: a label only
    pub name: String,
    /// `chord`, m
    pub chord: f32,
    /// `span`, m
    pub span: f32,
    /// `position`: where the air speed is measured and the forces act, body axes, m
    pub position: Vec3f,
    /// `lutAOA_CL`: lift coefficient over the angle in degrees
    pub lut_aoa_cl: Curve,
    /// `lutAOA_CD`
    pub lut_aoa_cd: Curve,
    /// `lutGH_CL`: factor on CL over the height above the road, m (empty: none)
    pub lut_gh_cl: Curve,
    /// `lutGH_CD`
    pub lut_gh_cd: Curve,
    /// `clGain`
    pub cl_gain: f32,
    /// `cdGain`
    pub cd_gain: f32,
    /// `hasController`: a `[DYNAMIC_CONTROLLER_n]` section names this wing
    pub has_controller: bool,
    /// `yawGain` (`YAW_CL_GAIN`)
    pub yaw_gain: f32,
    /// `area`: chord times span
    pub area: f32,
    /// `isVertical`: a `[FIN_n]`
    pub is_vertical: bool,
}

/// AC's `WingState` (0x44 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WingState {
    /// `aoa`: angle of the air flow in the car's vertical plane, degrees
    pub aoa: f32,
    /// `cd`
    pub cd: f32,
    /// `cl`
    pub cl: f32,
    /// `angle`: the wing's own angle, degrees (setup, controllers)
    pub angle: f32,
    /// `inputAngle`: the angle the controllers start from
    pub input_angle: f32,
    /// `groundHeight`: height of the wing's point above the road, m
    pub ground_height: f32,
    /// `frontShare`: a display value
    pub front_share: f32,
    /// `dragKG`
    pub drag_kg: f32,
    /// `liftKG`
    pub lift_kg: f32,
    /// `angleMult`: 1, or a DRS connection's `EFFECT` while the DRS is open
    pub angle_mult: f32,
    /// `groundEffectLift`: the last value of the ride-height table of CL
    pub ground_effect_lift: f32,
    /// `groundEffectDrag`
    pub ground_effect_drag: f32,
    /// `yawAngle`: angle of the air flow in the car's horizontal plane, degrees
    pub yaw_angle: f32,
    /// `isVertical`
    pub is_vertical: bool,
    /// `liftVector`: the last lift force, body axes, N
    pub lift_vector: Vec3f,
}

/// AC's `WingOverrideDef`: an angle a DRS connection in `ANGLE` mode forces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WingOverrideDef {
    /// `overrideAngle`
    pub override_angle: f32,
    /// `isActive`
    pub is_active: bool,
}

/// `DynamicWingController::eInputVar`; the numbers are the game's (the switch of
/// `DynamicWingController::getInput`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum WingControllerInput {
    #[default]
    Undefined = 0,
    Brake = 1,
    Gas = 2,
    LatG = 3,
    LonG = 4,
    Steer = 5,
    SpeedKmh = 6,
    SusTravelLr = 7,
    SusTravelRr = 8,
}

/// `DynamicWingController::eCombinatorMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum WingCombinatorMode {
    #[default]
    Undefined = 0,
    Add = 1,
    Mult = 2,
}

/// AC's `DynamicWingController` (0xb0 bytes): one stage of a wing's active-aero formula.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicWingController {
    /// `outputAngle`: the smoothed table value
    pub output_angle: f32,
    /// `upLimit`
    pub up_limit: f32,
    /// `downLimit`
    pub down_limit: f32,
    /// `combinatorMode`
    pub combinator_mode: WingCombinatorMode,
    /// `inputVar`
    pub input_var: WingControllerInput,
    /// `lut`
    pub lut: Curve,
    /// `filter`: a rate per second
    pub filter: f32,
}

impl DynamicWingController {
    /// `DynamicWingController::getInput` @ 0x1402a9ef0, the branch of a controller that
    /// belongs to a simulated car (the other branch reads a remote car's state snapshot).
    pub fn get_input(&self, car: &RollingChassis) -> f32 {
        use WingControllerInput::*;
        match self.input_var {
            Undefined => 0.0,
            Brake => car.controls.brake,
            Gas => car.controls.gas,
            LatG => car.acc_g.x,
            LonG => car.acc_g.z,
            Steer => car.controls.steer,
            SpeedKmh => car.speed * 3.6,
            SusTravelLr => car.suspensions[2].get_status().travel * 1000.0,
            SusTravelRr => car.suspensions[3].get_status().travel * 1000.0,
        }
    }

    /// `DynamicWingController::step` @ 0x1402aaa70.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &RollingChassis) {
        let mut value = self.lut.get_value(self.get_input(car));
        let previous = self.output_angle;
        // `comiss |value - previous|, 0.001` + `jb`: a NaN is not smoothed
        if (value - previous).abs() >= 0.001 {
            // 0.003 is a literal, not the step length
            let k = self.filter * 0.003;
            let k = if k > 1.0 {
                1.0
            } else if !(k >= 0.0) {
                0.0
            } else {
                k
            };
            value = (value - previous) * k + previous;
        }
        self.output_angle = value;
    }
}

/// AC's `Wing` (0x310 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wing {
    /// `data`
    pub data: WingData,
    /// `status`
    pub status: WingState,
    /// `dynamicControllers`
    pub dynamic_controllers: Vec<DynamicWingController>,
    /// `damageCL`: front, rear, left, right (the fifth is never read)
    pub damage_cl: [f32; 5],
    /// `damageCD`
    pub damage_cd: [f32; 5],
    /// `hasDamage`: `[HEADER] VERSION >= 2`
    pub has_damage: bool,
    /// `overrideStatus`
    pub override_status: WingOverrideDef,
    /// `SPEED_DAMAGE_COEFF`
    pub speed_damage_coeff: f32,
    /// `SURFACE_DAMAGE_COEFF`
    pub surface_damage_coeff: f32,
}

/// `plane4f::plane4f(p0, p1, p2)` @ 0x140117ed0: the plane through three points as unit
/// normal and offset; all zero for points on one line.
pub(crate) fn plane4f(p0: &Vec3f, p1: &Vec3f, p2: &Vec3f) -> [f32; 4] {
    let (ax, ay, az) = (p2.x - p0.x, p2.y - p0.y, p2.z - p0.z);
    let (bx, by, bz) = (p1.x - p0.x, p1.y - p0.y, p1.z - p0.z);
    let nx = az * by - ay * bz;
    let ny = ax * bz - az * bx;
    let nz = ay * bx - ax * by;
    let squared = (ny * ny + nx * nx) + nz * nz;
    if squared > 0.0 {
        let inverse = 1.0 / sqrtf(squared);
        let z = inverse * nz;
        let y = inverse * ny;
        let x = inverse * nx;
        [x, y, z, -((y * p0.y + x * p0.x) + z * p0.z)]
    } else {
        [0.0; 4]
    }
}

/// `Car::getGroundWindVector` @ 0x140270c00: the wind without its part along the normal of
/// the road under the first three tyres, times 0.44 (the wind near the ground).
pub fn get_ground_wind_vector(car: &RollingChassis) -> Vec3f {
    let t = &car.tyres;
    let plane =
        plane4f(&t[0].unmodified_contact_point, &t[1].unmodified_contact_point, &t[2].unmodified_contact_point);
    let w = car.env.wind;
    let dot = (w.x * plane[0] + w.y * plane[1]) + w.z * plane[2];
    let z = w.z - plane[2] * dot;
    let x = w.x - dot * plane[0];
    let y = w.y - dot * plane[1];
    Vec3f::new(x * 0.44, y * 0.44, z * 0.44)
}

/// `RaceEngineer::getPointGroundHeight` @ 0x14027c550: the height of a world point above the
/// road, the mean over the two planes through the contact points of (LF, RF, LR) and of
/// (LF, RF, RR).
pub fn get_point_ground_height(car: &RollingChassis, p: &Vec3f) -> f32 {
    let t = &car.tyres;
    let a = plane4f(&t[0].contact_point, &t[1].contact_point, &t[2].contact_point);
    let b = plane4f(&t[0].contact_point, &t[1].contact_point, &t[3].contact_point);
    // a ray straight down, (0, -1, 0), against each plane
    let height = |plane: &[f32; 4]| -> f32 {
        let denominator = (plane[0] * 0.0 + plane[1] * -1.0) + plane[2] * 0.0;
        // `ucomiss` + `je`: 0 for a zero or a NaN
        if denominator < 0.0 || denominator > 0.0 {
            let t = ((p.y * plane[1] + plane[0] * p.x) + plane[2] * p.z) + plane[3];
            -(t / denominator) * -1.0 + p.y
        } else {
            0.0
        }
    };
    let first = height(&a);
    let second = height(&b);
    ((p.y - second) + (p.y - first)) * 0.5
}

/// The damage factor of `Wing::addDrag` / `Wing::addLift`: the largest of
/// `SURFACE_DAMAGE_COEFF * zone gain * (damage / SPEED_DAMAGE_COEFF)^2` over the four zones.
fn damage_factor(wing: &Wing, gains: &[f32; 5], damage_zone_level: &[f32; 5]) -> f32 {
    let inverse = 1.0 / wing.speed_damage_coeff;
    let mut largest = 0.0f32;
    for zone in 0..4 {
        let x = inverse * damage_zone_level[zone];
        let gain = wing.surface_damage_coeff * gains[zone];
        // `comiss largest, candidate` + `ja`: a NaN candidate replaces the largest
        if !(largest > gain * (1.0 * (x * x))) {
            largest = gain * (1.0 * (x * x));
        }
    }
    largest
}

/// The three-way clamp of the machine code to 0..1: above 1: 1; at least 0: itself; else 0.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

impl Wing {
    /// `Wing::getCurrentModifiedAngle` @ 0x1402b2b80.
    pub fn get_current_modified_angle(&self) -> f32 {
        if self.override_status.is_active {
            self.override_status.override_angle
        } else {
            self.status.angle_mult * self.status.angle
        }
    }

    /// `Wing::setOverrideAngle` @ 0x1402b2bb0.
    pub fn set_override_angle(&mut self, angle: f32) {
        self.override_status.override_angle = angle;
        self.override_status.is_active = true;
    }

    /// `Wing::clearOverrides` @ 0x1402b2b70.
    pub fn clear_overrides(&mut self) {
        self.override_status.is_active = false;
    }

    /// `Wing::stepDynamicControllers` @ 0x1402b2dd0: the wing's angle from `inputAngle` and
    /// its controller stages. A wing without stages keeps its angle.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_dynamic_controllers(&mut self, car: &RollingChassis) {
        if self.dynamic_controllers.is_empty() {
            return;
        }
        let mut angle = self.status.input_angle;
        for controller in &mut self.dynamic_controllers {
            controller.step(car);
            match controller.combinator_mode {
                WingCombinatorMode::Add => angle += controller.output_angle,
                WingCombinatorMode::Mult => angle *= controller.output_angle,
                WingCombinatorMode::Undefined => {}
            }
            let (up, down) = (controller.up_limit, controller.down_limit);
            if angle > up {
                angle = up;
            } else if !(angle >= down) {
                angle = down;
            }
        }
        self.status.angle = angle;
    }

    /// `Wing::step` @ 0x1402b2bc0.
    pub fn step(&mut self, car: &mut RollingChassis, air_density: f32) {
        if !self.override_status.is_active {
            self.step_dynamic_controllers(car);
        }
        // the air speed at the wing: the point's velocity plus the wind near the ground, in
        // body axes
        let v = car.core.get_local_point_velocity(car.body, &self.data.position);
        let wind = get_ground_wind_vector(car);
        let z = v.z + wind.z;
        let x = v.x + wind.x;
        let y = v.y + wind.y;
        let v = car.core.world_to_local_normal(car.body, &Vec3f::new(x, y, z));
        let point = car.core.local_to_world(car.body, &self.data.position);
        let saved_angle = self.status.angle;
        self.status.ground_height = get_point_ground_height(car, &point);
        if self.override_status.is_active {
            self.status.angle = self.override_status.override_angle;
        }
        // `ucomiss` + `je`: no force for a zero (or NaN) forward speed
        if v.z < 0.0 || v.z > 0.0 {
            let inverse = 1.0 / v.z;
            self.status.aoa = atanf(inverse * v.y) * 57.29578;
            self.status.yaw_angle = atanf(inverse * v.x) * 57.29578;
            self.add_drag(car, &v, air_density);
            self.add_lift(car, &v, air_density);
        } else {
            self.status.aoa = 0.0;
            self.status.cd = 0.0;
            self.status.yaw_angle = 0.0;
            self.status.cl = 0.0;
        }
        if self.override_status.is_active {
            self.status.angle = saved_angle;
        }
    }

    /// `Wing::addDrag` @ 0x1402b2420. `v` is the air speed in body axes.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn add_drag(&mut self, car: &mut RollingChassis, v: &Vec3f, air_density: f32) {
        let flow_angle = if self.data.is_vertical { self.status.yaw_angle } else { self.status.aoa };
        let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
        let (mut nx, mut ny, mut nz) = (v.x, v.y, v.z);
        let length = sqrtf((nx * nx + ny * ny) + nz * nz);
        if length < 0.0 || length > 0.0 {
            let inverse = 1.0 / length;
            nx *= inverse;
            ny *= inverse;
            nz *= inverse;
        }
        let cd = self.data.lut_aoa_cd.get_value(self.status.angle_mult * self.status.angle + flow_angle) * self.data.cd_gain;
        self.status.cd = cd;
        if self.has_damage && !(0.0 >= car.env.mechanical_damage_rate) {
            let damage = clamp01(damage_factor(self, &self.damage_cd, &car.damage_zone_level));
            self.status.cd = (damage + 1.0) * cd;
        }
        if self.data.lut_gh_cd.get_count() != 0 {
            let factor = self.data.lut_gh_cd.get_value(self.status.ground_height);
            self.status.ground_effect_drag = factor;
            self.status.cd = factor * self.status.cd;
        }
        let force = (((squared * self.status.cd) * air_density) * self.data.area) * 0.5;
        self.status.drag_kg = force * 0.101_978_384;
        if squared < 0.0 || squared > 0.0 {
            let f = -force;
            let previous = std::mem::replace(&mut car.core.source, ForceSource::AeroDrag);
            car.core.add_local_force_at_local_pos(car.body, &Vec3f::new(nx * f, ny * f, nz * f), &self.data.position);
            car.core.source = previous;
        }
    }

    /// `Wing::addLift` @ 0x1402b2730.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn add_lift(&mut self, car: &mut RollingChassis, v: &Vec3f, air_density: f32) {
        let vertical = self.data.is_vertical;
        let flow_angle = if vertical { self.status.yaw_angle } else { self.status.aoa };
        let zz = v.z * v.z;
        let across = if vertical { v.x } else { v.y };
        let squared = across * across + zz;
        self.status.cl = self.data.lut_aoa_cl.get_value(self.status.angle_mult * self.status.angle + flow_angle) * self.data.cl_gain;
        // no lift while the air comes from behind
        if 0.0 > v.z {
            self.status.cl = 0.0;
        }
        let yaw_gain = self.data.yaw_gain;
        if (yaw_gain < 0.0 || yaw_gain > 0.0) && !vertical {
            let factor = clamp01(sinf(self.status.yaw_angle.abs() * 0.017_453) * yaw_gain + 1.0);
            self.status.cl = factor * self.status.cl;
        }
        if self.data.lut_gh_cl.get_count() != 0 {
            let factor = self.data.lut_gh_cl.get_value(self.status.ground_height);
            self.status.ground_effect_lift = factor;
            self.status.cl = factor * self.status.cl;
        }
        if self.has_damage && !(0.0 >= car.env.mechanical_damage_rate) {
            let damage = damage_factor(self, &self.damage_cl, &car.damage_zone_level);
            if !vertical {
                // only a wing that pushes down loses lift
                let cl = self.status.cl;
                if cl > 0.0 {
                    self.status.cl = (1.0 - clamp01(damage)) * cl;
                }
            } else {
                self.status.cl = (1.0 - clamp01(damage)) * self.status.cl;
            }
        }
        let force = (((squared * self.status.cl) * air_density) * self.data.area) * 0.5;
        self.status.lift_kg = force * 0.101_978_384;
        if !(squared < 0.0 || squared > 0.0) {
            return;
        }
        // the direction: the unit air speed turned a quarter turn, written as the cross
        // products of the machine code (the multiplications by zero included)
        let (mut vx, mut vy, mut vz) = (v.x, v.y, v.z);
        let yy = vy * vy;
        let length = sqrtf((vx * vx + yy) + vz * vz);
        if length < 0.0 || length > 0.0 {
            let inverse = 1.0 / length;
            vx *= inverse;
            vy *= inverse;
            vz *= inverse;
        }
        let (x, y, z);
        if !vertical {
            let x0 = vx * 0.0;
            let z0 = vz * 0.0;
            let y0 = vy * 0.0;
            y = vz - x0;
            z = x0 - vy;
            x = y0 - z0;
        } else {
            let y0 = vy * 0.0;
            let x0 = vx * 0.0;
            z = vx - y0;
            x = y0 - vz;
            y = vz * 0.0 - x0;
        }
        let f = -force;
        self.status.lift_vector = Vec3f::new(f * x, f * y, z * f);
        let previous = std::mem::replace(&mut car.core.source, ForceSource::AeroLift);
        car.core.add_local_force_at_local_pos(car.body, &self.status.lift_vector, &self.data.position);
        car.core.source = previous;
    }
}

/// `DRWWingConnectionMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum DrsMode {
    /// The open DRS scales the wing's angle (`angleMult = EFFECT`).
    #[default]
    Effect = 0,
    /// The open DRS forces the wing's angle (`ANGLE`).
    Angle = 1,
}

/// AC's `DRSWingConnection` (0x18 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrsWingConnection {
    /// `wing`: index into the wings
    pub wing: usize,
    /// `effect`
    pub effect: f32,
    /// `angle`
    pub angle: f32,
    /// `mode`
    pub mode: DrsMode,
}

/// AC's `DRS` (0x30 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Drs {
    /// `isPresent`: `drs.ini` has a `[WING_n]`
    pub is_present: bool,
    /// `isActive`: open
    pub is_active: bool,
    /// `isAvailable`: may be opened here
    pub is_available: bool,
    /// `ignoreZones`
    pub ignore_zones: bool,
    /// `wings`
    pub wings: Vec<DrsWingConnection>,
    /// `lastState`: the button in the step before
    pub last_state: bool,
    /// `limitG`: lateral g above which the DRS closes (0: no limit)
    pub limit_g: f32,
}

impl Default for Drs {
    /// As `Car::Car` leaves it.
    fn default() -> Drs {
        Drs {
            is_present: false,
            is_active: false,
            is_available: true,
            ignore_zones: false,
            wings: Vec::new(),
            last_state: false,
            limit_g: 0.0,
        }
    }
}

impl Drs {
    /// `DRS::init` @ 0x1402b4330: `drs.ini`. `wing_count` is the number of wings (fins
    /// included) of the car's `AeroMap`.
    pub fn load(data_path: &Path, wing_count: usize) -> Result<Drs, String> {
        let mut drs = Drs::default();
        let path = data_path.join("drs.ini");
        if !path.is_file() {
            return Ok(drs);
        }
        let ini = IniReader::load(&path)?;
        if !ini.ready {
            return Ok(drs);
        }
        let mut version = 1;
        if ini.has_section("HEADER") {
            version = ini.get_int("HEADER", "VERSION")?;
        }
        if ini.has_section("DRS_ZONES") {
            drs.ignore_zones = ini.get_int("DRS_ZONES", "IGNORE_ZONES")? != 0;
        }
        if ini.has_section("DEACTIVATION") {
            drs.limit_g = ini.get_float("DEACTIVATION", "LIMIT_G")?;
        }
        for index in 0..wing_count {
            let section = format!("WING_{index}");
            if !ini.has_section(&section) {
                continue;
            }
            let mut connection = DrsWingConnection { wing: index, ..DrsWingConnection::default() };
            connection.effect = ini.get_float(&section, "EFFECT")?;
            if version > 1 {
                match ini.get_string(&section, "MODE").as_str() {
                    "EFFECT" => connection.mode = DrsMode::Effect,
                    "ANGLE" => {
                        connection.mode = DrsMode::Angle;
                        connection.angle = ini.get_float(&section, "ANGLE")?;
                    }
                    other => return Err(format!("{}: [{section}] MODE={other}: unknown DRS mode", path.display())),
                }
            }
            drs.wings.push(connection);
            drs.is_present = true;
        }
        Ok(drs)
    }

    /// `DRS::step` @ 0x1402b4e60. `zone_available` is `DRSManager::isDRSAvailable` of the
    /// track for this car (true on a track without DRS zones).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &RollingChassis, wings: &mut [Wing], zone_available: bool) {
        if !self.is_present {
            return;
        }
        let available = self.ignore_zones || zone_available;
        self.is_available = available;
        if car.controls.drs && !self.last_state && available {
            self.is_active = !self.is_active;
        }
        self.last_state = car.controls.drs;
        if !available || !(0.1 >= car.controls.brake) {
            self.is_active = false;
        } else {
            let limit = self.limit_g;
            if limit > 0.0 && car.acc_g.x.abs() > limit {
                self.is_active = false;
            }
        }
        for connection in &self.wings {
            let wing = &mut wings[connection.wing];
            match (self.is_active, connection.mode) {
                (true, DrsMode::Effect) => wing.status.angle_mult = connection.effect,
                (true, DrsMode::Angle) => wing.set_override_angle(connection.angle),
                (false, DrsMode::Effect) => wing.status.angle_mult = 1.0,
                (false, DrsMode::Angle) => wing.clear_overrides(),
            }
        }
    }
}

/// The data members of AC's `AeroMap` and `DRS` that other systems read or write: the setup
/// items point at the wings' angles, `Car::updateAirPressure` writes the air density, the
/// telemetry reads the wings' states and the DRS flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AeroBase {
    /// `AeroMap::referenceArea`, `CD`, `CL`, `frontShare`, `CDX`, `CDY`, `CDA`: the old
    /// one-body format (`aero.ini [DATA]`, a car without `[WING_n]`); unused with wings
    pub reference_area: f32,
    pub cd: f32,
    pub cl: f32,
    pub front_share: f32,
    pub cdx: f32,
    pub cdy: f32,
    pub cda: f32,
    /// `AeroMap::dynamicCD`: written by the old one-body format only
    pub dynamic_cd: f32,
    /// `AeroMap::dynamicCL`
    pub dynamic_cl: f32,
    /// `AeroMap::airDensity`, kg/m^3: 1.221 until `Car::updateAirPressure` runs
    pub air_density: f32,
    /// `AeroMap::wings`: the `[WING_n]`, then the `[FIN_n]`
    pub wings: Vec<Wing>,
    /// `Car::drs`
    pub drs: Drs,
}

impl AeroBase {
    /// `AeroMap::addDrag` @ 0x1402b5860 (the old one-body format): drag along the air speed,
    /// more of it when the car moves sideways or vertically, and a torque against the body's
    /// rotation. `v` is the body's velocity in its own axes (no wind here).
    fn add_drag(&mut self, car: &mut RollingChassis, v: &Vec3f) {
        let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
        if !(squared < 0.0 || squared > 0.0) {
            return;
        }
        let (mut nx, mut ny, mut nz) = (v.x, v.y, v.z);
        let length = sqrtf((ny * ny + nx * nx) + nz * nz);
        if length < 0.0 || length > 0.0 {
            let inverse = 1.0 / length;
            nx *= inverse;
            ny *= inverse;
            nz *= inverse;
        }
        let cd = self.cd;
        let sideways = (nx.abs() * cd) * self.cdx;
        let vertical = (ny.abs() * cd) * self.cdy;
        self.dynamic_cd = (sideways + cd) + vertical;
        let force = -((((self.dynamic_cd * squared) * self.air_density) * self.reference_area) * 0.5);
        let previous = std::mem::replace(&mut car.core.source, ForceSource::AeroDrag);
        car.core.add_local_force(car.body, &Vec3f::new(force * nx, force * ny, nz * force));
        // the game hands the world-axes spin to a body-axes torque call as it is
        let w = car.core.get_angular_velocity(car.body);
        let spin = (w.x * w.x + w.y * w.y) + w.z * w.z;
        if spin < 0.0 || spin > 0.0 {
            let (mut wx, mut wy, mut wz) = (w.x, w.y, w.z);
            let length = sqrtf(spin);
            if length < 0.0 || length > 0.0 {
                let inverse = 1.0 / length;
                wx *= inverse;
                wy *= inverse;
                wz *= inverse;
            }
            let against = -spin;
            let torque = Vec3f::new((wx * against) * self.cda, (wy * against) * self.cda, (wz * against) * self.cda);
            car.core.add_local_torque(car.body, &torque);
        }
        car.core.source = previous;
    }

    /// `AeroMap::addLift` @ 0x1402b5a90 (the old one-body format): downforce from the forward
    /// speed, split front / rear (both parts act at the body's origin: the two application
    /// points are never set).
    fn add_lift(&mut self, car: &mut RollingChassis, v: &Vec3f) {
        let squared = v.z * v.z;
        let lift = (((squared * self.cl) * self.air_density) * self.reference_area) * 0.5;
        if !(squared < 0.0 || squared > 0.0) {
            return;
        }
        let origin = Vec3f::new(0.0, 0.0, 0.0);
        let previous = std::mem::replace(&mut car.core.source, ForceSource::AeroLift);
        car.core.add_local_force_at_local_pos(car.body, &Vec3f::new(0.0, -(lift * self.front_share), 0.0), &origin);
        car.core.add_local_force_at_local_pos(car.body, &Vec3f::new(0.0, -((1.0 - self.front_share) * lift), 0.0), &origin);
        car.core.source = previous;
    }
}

/// `RaceEngineer::getPointFrontShare` @ 0x14027c4d0: where a point of the body lies between
/// the axles, 1 at the front axle and 0 at the rear (a display value of each wing).
pub fn get_point_front_share(car: &RollingChassis, p: &Vec3f) -> f32 {
    let front = car.suspensions[0].get_base_position().z;
    let rear = car.suspensions[2].get_base_position().z;
    1.0 - (p.z - front) / (rear - front)
}

/// The aero slot: everything between "the air and the car's motion" and "forces on the car
/// body".
pub trait AeroModel {
    /// `Car::stepComponents` positions 6 and 7: `DRS::step` @ 0x1402b4e60, then
    /// `AeroMap::step` @ 0x1402b7150 (each wing's two force calls on the car body).
    fn step(&mut self, car: &mut RollingChassis, dt: f32);
    fn base(&self) -> &AeroBase;
    fn base_mut(&mut self) -> &mut AeroBase;
    /// The values of the model under the names of the recordings.
    fn trace(&self, out: &mut Vec<TraceValue>);
    /// The state a run needs to go on from the middle, as 32-bit words.
    fn save_state(&self, out: &mut Vec<u32>);
    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String>;
}

/// AC's `AeroMap` (with the car's `DRS`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VanillaAero {
    pub base: AeroBase,
}

impl VanillaAero {
    /// `Car::initAeroMap` @ 0x140272a80: `AeroMap::init` @ 0x1402b5ca0 with
    /// `AeroMap::loadINI` @ 0x1402b5d50 (`aero.ini`: `Wing::Wing` @ 0x1402b1340 per
    /// `[WING_n]` and `[FIN_n]`, `DynamicWingController::initCommon` @ 0x1402aa0d0 per
    /// `[DYNAMIC_CONTROLLER_n]`), then `DRS::init` @ 0x1402b4330.
    ///
    /// Returns the model and `[SLIPSTREAM]`'s two factors (`effectGainMult`,
    /// `speedFactorMult`), which belong to the car's wake.
    pub fn new(data_path: &Path) -> Result<(VanillaAero, Option<(f32, f32)>), String> {
        let path = data_path.join("aero.ini");
        let ini = IniReader::load(&path)?;
        // AeroMap::init's values
        let mut base =
            AeroBase { reference_area: 1.0, front_share: 0.5, cda: 0.1, air_density: 1.221, ..AeroBase::default() };
        let mut slipstream = None;
        if ini.has_section("SLIPSTREAM") {
            slipstream = Some((
                ini.get_float("SLIPSTREAM", "EFFECT_GAIN_MULT")?,
                ini.get_float("SLIPSTREAM", "SPEED_FACTOR_MULT")?,
            ));
        }
        for (prefix, vertical) in [("WING", false), ("FIN", true)] {
            let mut index = 0;
            loop {
                let section = format!("{prefix}_{index}");
                if !ini.has_section(&section) {
                    break;
                }
                base.wings.push(load_wing(&ini, data_path, &section, vertical)?);
                index += 1;
            }
        }
        if base.wings.is_empty() {
            // the old one-body format: six numbers, a missing one reads 0
            if !ini.has_section("DATA") {
                return Err(format!("{}: aero.ini does not contain WINGS nor DATA", path.display()));
            }
            base.reference_area = ini.get_float("DATA", "REFERENCE_AREA")?;
            base.cd = ini.get_float("DATA", "CD")?;
            base.cl = ini.get_float("DATA", "CL")?;
            base.front_share = ini.get_float("DATA", "FRONT_SHARE")?;
            base.cdx = ini.get_float("DATA", "CDX")?;
            base.cdy = ini.get_float("DATA", "CDY")?;
        } else if ini.has_section("DATA") {
            return Err(format!("{}: aero.ini contains DATA with WINGS, which is redundant", path.display()));
        }
        let mut index = 0;
        loop {
            let section = format!("DYNAMIC_CONTROLLER_{index}");
            if !ini.has_section(&section) {
                break;
            }
            index += 1;
            let wing = ini.get_int(&section, "WING")?;
            if wing < 0 || wing as usize >= base.wings.len() {
                // "ERROR: wing index out of range": the controller is skipped
                continue;
            }
            let controller = load_wing_controller(&ini, data_path, &section)?;
            let wing = &mut base.wings[wing as usize];
            wing.data.has_controller = true;
            wing.dynamic_controllers.push(controller);
        }
        base.drs = Drs::load(data_path, base.wings.len())?;
        Ok((VanillaAero { base }, slipstream))
    }
}

/// `Wing::Wing(Car*, INIReader&, int, bool)` @ 0x1402b1340.
fn load_wing(ini: &IniReader, data_path: &Path, section: &str, vertical: bool) -> Result<Wing, String> {
    let mut wing = Wing::default();
    wing.speed_damage_coeff = 300.0;
    wing.surface_damage_coeff = 300.0;
    wing.status.angle_mult = 1.0;
    // `status.isVertical` stays false in the wing itself: only the copy `AeroMap::getWingStatus`
    // hands out gets `data.isVertical`
    wing.data.is_vertical = vertical;
    wing.data.name = ini.get_string(section, "NAME");
    wing.data.chord = ini.get_float(section, "CHORD")?;
    wing.data.span = ini.get_float(section, "SPAN")?;
    wing.data.area = wing.data.chord * wing.data.span;
    let position = ini.get_float3(section, "POSITION")?;
    wing.data.position = Vec3f::new(position[0], position[1], position[2]);
    wing.data.lut_aoa_cl.load(&append_path(data_path, &ini.get_string(section, "LUT_AOA_CL")))?;
    wing.data.lut_aoa_cd.load(&append_path(data_path, &ini.get_string(section, "LUT_AOA_CD")))?;
    let gh_cl = append_path(data_path, &ini.get_string(section, "LUT_GH_CL"));
    if gh_cl.is_file() {
        wing.data.lut_gh_cl.load(&gh_cl)?;
    }
    let gh_cd = append_path(data_path, &ini.get_string(section, "LUT_GH_CD"));
    if gh_cd.is_file() {
        wing.data.lut_gh_cd.load(&gh_cd)?;
    }
    wing.data.cd_gain = ini.get_float(section, "CD_GAIN")?;
    wing.data.cl_gain = ini.get_float(section, "CL_GAIN")?;
    wing.status.angle = ini.get_float(section, "ANGLE")?;
    wing.status.input_angle = wing.status.angle;
    // the file's version is read here, once per wing
    let version = ini.get_int("HEADER", "VERSION")?;
    if version >= 2 {
        for (zone, name) in ["FRONT", "REAR", "LEFT", "RIGHT"].iter().enumerate() {
            wing.damage_cd[zone] = ini.get_float(section, &format!("ZONE_{name}_CD"))?;
            wing.damage_cl[zone] = ini.get_float(section, &format!("ZONE_{name}_CL"))?;
        }
        wing.has_damage = true;
    }
    if version >= 3 {
        wing.data.yaw_gain = ini.get_float(section, "YAW_CL_GAIN")?;
    }
    Ok(wing)
}

/// `DynamicWingController::initCommon` @ 0x1402aa0d0.
fn load_wing_controller(ini: &IniReader, data_path: &Path, section: &str) -> Result<DynamicWingController, String> {
    let mut controller = DynamicWingController::default();
    controller.input_var = match ini.get_string(section, "INPUT").as_str() {
        "BRAKE" => WingControllerInput::Brake,
        "GAS" => WingControllerInput::Gas,
        "LATG" => WingControllerInput::LatG,
        "LONG" => WingControllerInput::LonG,
        "STEER" => WingControllerInput::Steer,
        "SPEED_KMH" => WingControllerInput::SpeedKmh,
        "SUS_TRAVEL_LR" => WingControllerInput::SusTravelLr,
        "SUS_TRAVEL_RR" => WingControllerInput::SusTravelRr,
        // "ERROR: INPUT UNDEFINED": the stage stays and reads 0
        _ => WingControllerInput::Undefined,
    };
    controller.combinator_mode = match ini.get_string(section, "COMBINATOR").as_str() {
        "ADD" => WingCombinatorMode::Add,
        "MULT" => WingCombinatorMode::Mult,
        _ => WingCombinatorMode::Undefined,
    };
    controller.lut.load(&append_path(data_path, &ini.get_string(section, "LUT")))?;
    // the inlined `lagToLerpDeltaK(FILTER, 0.004, 0.003)` with its two factors folded
    controller.filter = ((1.0 - ini.get_float(section, "FILTER")?) * 1.333_333_4) * 333.333_34;
    controller.up_limit = ini.get_float(section, "UP_LIMIT")?;
    controller.down_limit = ini.get_float(section, "DOWN_LIMIT")?;
    Ok(controller)
}

impl AeroModel for VanillaAero {
    fn step(&mut self, car: &mut RollingChassis, _dt: f32) {
        let base = &mut self.base;
        // 6: DRS
        let zone_available = car.env.drs_zone_available;
        base.drs.step(car, &mut base.wings, zone_available);
        // 7: AeroMap::step. A car without wings has the old one-body aero.
        if base.wings.is_empty() {
            let v = car.core.get_local_velocity(car.body);
            base.add_drag(car, &v);
            base.add_lift(car, &v);
        }
        let air_density = base.air_density;
        for wing in &mut base.wings {
            wing.step(car, air_density);
        }
    }

    fn base(&self) -> &AeroBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AeroBase {
        &mut self.base
    }

    fn trace(&self, out: &mut Vec<TraceValue>) {
        let base = &self.base;
        out.push(TraceValue::f("aero.airDensity", base.air_density));
        out.push(TraceValue::f("aero.dynamicCD", base.dynamic_cd));
        out.push(TraceValue::f("aero.dynamicCL", base.dynamic_cl));
        for (k, wing) in base.wings.iter().enumerate() {
            let s = &wing.status;
            for (name, value) in [
                ("aoa", s.aoa),
                ("cd", s.cd),
                ("cl", s.cl),
                ("angle", s.angle),
                ("groundHeight", s.ground_height),
                ("dragKG", s.drag_kg),
                ("liftKG", s.lift_kg),
                ("yawAngle", s.yaw_angle),
            ] {
                out.push(TraceValue::f(&format!("wing{k}.{name}"), value));
            }
        }
        out.push(TraceValue::i("drs.isActive", base.drs.is_active as i32));
    }

    fn save_state(&self, out: &mut Vec<u32>) {
        let base = &self.base;
        out.push(base.air_density.to_bits());
        out.extend([base.drs.is_active as u32, base.drs.is_available as u32, base.drs.last_state as u32]);
        for wing in &base.wings {
            let s = &wing.status;
            out.extend(
                [
                    s.aoa,
                    s.cd,
                    s.cl,
                    s.angle,
                    s.input_angle,
                    s.ground_height,
                    s.drag_kg,
                    s.lift_kg,
                    s.angle_mult,
                    s.ground_effect_lift,
                    s.ground_effect_drag,
                    s.yaw_angle,
                    s.lift_vector.x,
                    s.lift_vector.y,
                    s.lift_vector.z,
                    wing.override_status.override_angle,
                ]
                .map(f32::to_bits),
            );
            out.push(wing.override_status.is_active as u32);
            for controller in &wing.dynamic_controllers {
                out.push(controller.output_angle.to_bits());
            }
        }
    }

    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut next = || words.next().ok_or("the aero state is too short".to_string());
        let base = &mut self.base;
        base.air_density = f32::from_bits(next()?);
        base.drs.is_active = next()? != 0;
        base.drs.is_available = next()? != 0;
        base.drs.last_state = next()? != 0;
        for wing in &mut base.wings {
            let mut f = || next().map(f32::from_bits);
            let s = &mut wing.status;
            s.aoa = f()?;
            s.cd = f()?;
            s.cl = f()?;
            s.angle = f()?;
            s.input_angle = f()?;
            s.ground_height = f()?;
            s.drag_kg = f()?;
            s.lift_kg = f()?;
            s.angle_mult = f()?;
            s.ground_effect_lift = f()?;
            s.ground_effect_drag = f()?;
            s.yaw_angle = f()?;
            s.lift_vector = Vec3f::new(f()?, f()?, f()?);
            wing.override_status.override_angle = f()?;
            wing.override_status.is_active = next()? != 0;
            for controller in &mut wing.dynamic_controllers {
                controller.output_angle = f32::from_bits(next()?);
            }
        }
        Ok(())
    }
}

/// AC's `SlipStream` as far as one car alone needs it: the wake cone other cars would read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlipStream {
    /// `triangle.points[0]`: the tip, at the car body
    pub tip: Vec3f,
    /// `dir`: unit vector against the direction of travel
    pub dir: Vec3f,
    /// `length`, m
    pub length: f32,
    /// `speedFactor`: 0.5 after `SlipStream::init`
    pub speed_factor: f32,
    /// `effectGainMult` (`aero.ini [SLIPSTREAM]`)
    pub effect_gain_mult: f32,
    /// `speedFactorMult`
    pub speed_factor_mult: f32,
    /// `triangle.points[1]`, `[2]`: the far corners of the cone's outline (nothing in the
    /// physics reads them)
    pub corners: [Vec3f; 2],
}

impl Default for SlipStream {
    /// `Car::Car` and `SlipStream::init` @ 0x1402aadc0.
    fn default() -> SlipStream {
        SlipStream {
            tip: Vec3f::default(),
            dir: Vec3f::default(),
            length: 0.0,
            speed_factor: 0.5,
            effect_gain_mult: 1.0,
            speed_factor_mult: 1.0,
            corners: [Vec3f::default(); 2],
        }
    }
}

impl SlipStream {
    /// `SlipStream::setPosition` @ 0x1402aae80: the wake of a car at `pos` moving with `vel`
    /// (world axes). Called from `Car::postStep`, after the rigid-body step.
    pub fn set_position(&mut self, pos: &Vec3f, vel: &Vec3f) {
        self.tip = *pos;
        let (mut hx, mut hz, mut hy) = (vel.x, vel.z, 0.0f32);
        let flat = sqrtf(vel.z * vel.z + vel.x * vel.x);
        if flat < 0.0 || flat > 0.0 {
            let inverse = 1.0 / flat;
            hx = vel.x * inverse;
            hz = vel.z * inverse;
            hy = inverse * 0.0;
        }
        let squared = (vel.y * vel.y + vel.x * vel.x) + vel.z * vel.z;
        let speed = if squared < 0.0 || squared > 0.0 { sqrtf(squared) } else { 0.0 };
        let length = (speed * self.speed_factor) * self.speed_factor_mult;
        self.length = length;
        // to the side: the heading crossed with "up", a quarter of the length
        let sx = ((hy * 0.0 - hz) * length) * 0.25;
        let sy = ((hz * 0.0 - hx * 0.0) * length) * 0.25;
        let sz = ((hx - hy * 0.0) * length) * 0.25;
        // against the direction of travel
        let (mut rx, mut ry, mut rz) = (vel.x * -1.0, vel.y * -1.0, vel.z * -1.0);
        let back = sqrtf((ry * ry + rx * rx) + rz * rz);
        if back < 0.0 || back > 0.0 {
            let inverse = 1.0 / back;
            rx = inverse * rx;
            ry *= inverse;
            rz *= inverse;
        }
        self.dir = Vec3f::new(rx, ry, rz);
        let nl = -length;
        self.corners[0] = Vec3f::new((nl * hx + pos.x) + sx, (hy * nl + pos.y) + sy, (hz * nl + pos.z) + sz);
        self.corners[1] = Vec3f::new((nl * hx + pos.x) - sx, (hy * nl + pos.y) - sy, (hz * nl + pos.z) - sz);
    }

    /// `SlipStream::getSlipEffect` @ 0x1402aac60: how much of the air this wake takes away at
    /// the world point `p` (0 outside the cone).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_slip_effect(&self, p: &Vec3f) -> f32 {
        let (mut dx, mut dy, mut dz) = (p.x - self.tip.x, p.y - self.tip.y, p.z - self.tip.z);
        let squared = (dy * dy + dx * dx) + dz * dz;
        let distance = if squared < 0.0 || squared > 0.0 { sqrtf(squared) } else { 0.0 };
        // `comiss` + `jae`: a NaN distance goes on
        if distance >= self.length {
            return 0.0;
        }
        let length = sqrtf((dy * dy + dx * dx) + dz * dz);
        if length < 0.0 || length > 0.0 {
            let inverse = 1.0 / length;
            dx *= inverse;
            dy *= inverse;
            dz *= inverse;
        }
        let cosine = (dy * self.dir.y + dx * self.dir.x) + dz * self.dir.z;
        if !(cosine > 0.7) {
            return 0.0;
        }
        (((1.0 - distance / self.length) * (cosine - 0.7)) * 3.333_333_3) * self.effect_gain_mult
    }
}

impl RollingChassis {
    /// `Car::updateAirPressure` @ 0x140276ae0: the air density at the car, thinned by the
    /// wakes of other cars. `wakes` are the other cars' `SlipStream`s (none for a car alone).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn update_air_pressure(&mut self, wakes: &[SlipStream]) {
        let mut density = super::engine::get_air_density(self.env.ambient_temperature);
        let gain = self.slip_stream_effect_gain;
        if !(0.0 >= gain) {
            let position = self.core.get_position(self.body);
            let mut least = 1.0f32;
            for wake in wakes {
                let left = 1.0 - wake.get_slip_effect(&position) * gain;
                if !(clamp01(left) >= least) {
                    least = clamp01(left);
                }
            }
            let thin = least * density;
            density = (density - thin) * (0.75 / gain) + thin;
        }
        if let Some(aero) = &mut self.aero {
            aero.base_mut().air_density = density;
        }
        self.air_density = density;
    }
}
