//! The driver aids: the aids slot [`AidsModel`] and [`VanillaAids`], AC's `TractionControl`,
//! `ABS`, `EDL` (electronic differential lock), `StabilityControl` and `SpeedLimiter` (the pit
//! limiter).
//!
//! Each of them is a few compares; what matters is where in the step they run and when their
//! output acts:
//!
//! | Aid | Position in `Car::stepComponents` | Writes | Acts |
//! |---|---|---|---|
//! | EDL | 2, after the brakes | adds to `Tyre::inputs.brakeTorque` of the faster driven wheel | this step's tyres |
//! | ABS | 16 | `Tyre::absOverride` (0 or 1) | the next step's tyres |
//! | traction control | 17 | `Engine::electronicOverride` = 0 | the next step's engine (which resets it to 1) |
//! | pit limiter | 18 | `Engine::electronicOverride` = 0, `BrakeSystem::electronicOverride` | the next step |
//! | stability control | 27 | a yaw torque on the car body | this step's rigid-body step |

use std::path::Path;

use super::body::ForceSource;
use super::chassis::RollingChassis;
use super::drivetrain::TractionType;
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::{append_path, IniReader};
use crate::math::{atanf, powf, sqrtf};
use crate::tyre::VanillaTyre;
use crate::vecmath::Vec3f;

/// The level table shared by traction control and ABS: `valueCurve` with `currentMode`.
fn find_mode(curve: &Curve, slip_ratio_limit: f32) -> u32 {
    // the first level whose table value is exactly the car file's limit; 0 when none is
    // (`ucomiss` + `je`: a table value that is not a number also counts as a match)
    for index in 0..curve.get_count() {
        let value = curve.get_value(index as f32);
        if !(value < slip_ratio_limit || value > slip_ratio_limit) {
            return index as u32;
        }
    }
    0
}

/// `TractionControl::cycleMode` @ 0x14028f8e0 and `ABS::cycleMode`: the cockpit's level key.
/// Returns the new `(isActive, currentMode, slipRatioLimit)`.
fn cycle_mode(is_present: bool, is_active: &mut bool, current_mode: &mut u32, slip_ratio_limit: &mut f32, curve: &Curve, dir: i32) {
    if !is_present {
        return;
    }
    let count = curve.get_count();
    if count == 0 {
        // a single level: plain on / off
        *is_active = !*is_active;
        return;
    }
    let mode;
    if !*is_active {
        *is_active = true;
        mode = if dir < 1 { (count - 1) as u32 } else { 0 };
    } else if dir > 0 {
        mode = (current_mode.wrapping_add(1)) % count as u32;
        *current_mode = mode;
        if mode == 0 {
            *is_active = false;
            return;
        }
    } else {
        if *current_mode == 0 {
            *is_active = false;
            return;
        }
        mode = *current_mode - 1;
    }
    *current_mode = mode;
    *slip_ratio_limit = curve.get_value(mode as f32);
}

/// AC's `TractionControl` (0xa8 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct TractionControl {
    /// `isPresent`
    pub is_present: bool,
    /// `isActive`
    pub is_active: bool,
    /// `slipRatioLimit`
    pub slip_ratio_limit: f32,
    /// `isInAction`: the engine is being cut
    pub is_in_action: bool,
    /// `frequency`: seconds between two checks
    pub frequency: f32,
    /// `minSpeedMS`
    pub min_speed_ms: f32,
    /// `timeAccumulator`
    pub time_accumulator: f32,
    /// `currentMode`: the level, a line of `valueCurve`
    pub current_mode: u32,
    /// `valueCurve`: the slip limit of each level
    pub value_curve: Curve,
}

impl Default for TractionControl {
    /// The constructor's values (inside `Car::Car`).
    fn default() -> TractionControl {
        TractionControl {
            is_present: true,
            is_active: true,
            slip_ratio_limit: 0.2,
            is_in_action: false,
            frequency: 0.05,
            min_speed_ms: 0.0,
            time_accumulator: 0.0,
            current_mode: 0,
            value_curve: Curve::new(),
        }
    }
}

impl TractionControl {
    /// `TractionControl::init` @ 0x14028fa40: `electronics.ini [TRACTION_CONTROL]`.
    pub fn new(data_path: &Path) -> Result<TractionControl, String> {
        let mut tc = TractionControl::default();
        let path = data_path.join("electronics.ini");
        if !path.is_file() {
            return Ok(tc);
        }
        let ini = IniReader::load(&path)?;
        if !ini.ready {
            return Ok(tc);
        }
        let section = "TRACTION_CONTROL";
        tc.slip_ratio_limit = ini.get_float(section, "SLIP_RATIO_LIMIT")?;
        // `Speed::fromKMH`: a multiplication by 0.2777778
        tc.min_speed_ms = ini.get_float(section, "MIN_SPEED_KMH")? * 0.277_777_8;
        let curve = ini.get_string(section, "CURVE");
        if !curve.is_empty() {
            tc.value_curve.load(&append_path(data_path, &curve))?;
            tc.current_mode = find_mode(&tc.value_curve, tc.slip_ratio_limit);
        }
        tc.is_present = ini.get_int(section, "PRESENT")? != 0;
        tc.is_active = ini.get_int(section, "ACTIVE")? != 0;
        let rate = ini.get_float(section, "RATE_HZ")?;
        if rate < 0.0 || rate > 0.0 {
            tc.frequency = 1.0 / rate;
        }
        Ok(tc)
    }

    /// `TractionControl::cycleMode` @ 0x14028f8e0.
    pub fn cycle_mode(&mut self, dir: i32) {
        cycle_mode(self.is_present, &mut self.is_active, &mut self.current_mode, &mut self.slip_ratio_limit, &self.value_curve, dir);
    }

    /// `TractionControl::getCurrentMode` @ 0x14028f9b0: (level from 1, number of levels);
    /// (0, 0) while switched off.
    pub fn get_current_mode(&self) -> (u32, u32) {
        mode_pair(self.is_active, self.current_mode, &self.value_curve)
    }

    /// `TractionControl::step` @ 0x140290200.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        if !self.is_present {
            self.is_in_action = false;
            self.is_active = false;
            return;
        }
        // `Car::getVelocity`: the body's velocity now, not the cached speed
        let v = car.core.get_velocity(car.body);
        let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
        let speed = if squared < 0.0 || squared > 0.0 { sqrtf(squared) } else { 0.0 };
        if !(speed >= self.min_speed_ms) {
            self.is_in_action = false;
            self.time_accumulator = 0.0;
            return;
        }
        self.time_accumulator = dt + self.time_accumulator;
        if !(self.time_accumulator >= self.frequency) {
            // between two checks a cut is held: the engine resets the override every step
            if self.is_in_action {
                set_engine_override(car, 0.0);
            }
            return;
        }
        self.time_accumulator = 0.0;
        if self.is_active && !(1.0 >= car.speed) {
            let mut largest = 0.0f32;
            for tyre in &car.tyres {
                if tyre.driven && tyre.status.slip_ratio > largest {
                    largest = tyre.status.slip_ratio;
                }
            }
            if largest > self.slip_ratio_limit {
                set_engine_override(car, 0.0);
            }
        }
        let value = engine_override(car);
        self.is_in_action = value < 1.0 || value > 1.0;
    }
}

fn mode_pair(is_active: bool, current_mode: u32, curve: &Curve) -> (u32, u32) {
    if !is_active {
        return (0, 0);
    }
    let count = curve.get_count() as u32;
    if count == 0 {
        (1, 1)
    } else {
        (current_mode + 1, count)
    }
}

/// `car->drivetrain.acEngine.electronicOverride` (1 for a chassis without a drivetrain).
fn engine_override(car: &RollingChassis) -> f32 {
    match &car.drivetrain {
        Some(drivetrain) => drivetrain.engine().base().electronic_override,
        None => 1.0,
    }
}

fn set_engine_override(car: &mut RollingChassis, value: f32) {
    if let Some(drivetrain) = &mut car.drivetrain {
        drivetrain.engine_mut().base_mut().electronic_override = value;
    }
}

/// AC's `ABS` (0xa8 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Abs {
    /// `isPresent`
    pub is_present: bool,
    /// `isActive`
    pub is_active: bool,
    /// `slipRatioLimit`: a wheel counts as locking below minus this slip ratio
    pub slip_ratio_limit: f32,
    /// `frequency`: seconds between two checks
    pub frequency: f32,
    /// `channels`: 4 (each wheel alone), 2 (per axle) or 1 (all wheels together)
    pub channels: i32,
    /// `timeAccumulator`
    pub time_accumulator: f32,
    /// `valueCurve`
    pub value_curve: Curve,
    /// `currentMode`
    pub current_mode: u32,
    /// `currentValue`: only ever written as 1
    pub current_value: f32,
}

impl Default for Abs {
    /// The constructor's values (inside `Car::Car`).
    fn default() -> Abs {
        Abs {
            is_present: true,
            is_active: true,
            slip_ratio_limit: 0.2,
            frequency: 0.05,
            channels: 4,
            time_accumulator: 0.0,
            value_curve: Curve::new(),
            current_mode: 0,
            current_value: 1.0,
        }
    }
}

impl Abs {
    /// `ABS::init` @ 0x14028ec50: `electronics.ini [ABS_V2]` if it exists, else `[ABS]`.
    pub fn new(data_path: &Path) -> Result<Abs, String> {
        let mut abs = Abs::default();
        let path = data_path.join("electronics.ini");
        if !path.is_file() {
            return Ok(abs);
        }
        let ini = IniReader::load(&path)?;
        if !ini.ready {
            return Ok(abs);
        }
        let version2 = ini.has_section("ABS_V2");
        let section = if version2 { "ABS_V2" } else { "ABS" };
        abs.slip_ratio_limit = ini.get_float(section, "SLIP_RATIO_LIMIT")?;
        abs.is_present = ini.get_int(section, "PRESENT")? != 0;
        abs.is_active = ini.get_int(section, "ACTIVE")? != 0;
        // the "is it empty" test reads the chosen section, the file name comes from `[ABS]`
        if !ini.get_string(section, "CURVE").is_empty() {
            abs.value_curve.load(&append_path(data_path, &ini.get_string("ABS", "CURVE")))?;
            abs.current_mode = find_mode(&abs.value_curve, abs.slip_ratio_limit);
        }
        let rate = ini.get_float(section, "RATE_HZ")?;
        if rate < 0.0 || rate > 0.0 {
            abs.frequency = 1.0 / rate;
        }
        if version2 {
            abs.channels = ini.get_int(section, "CHANNELS")?;
            if !matches!(abs.channels, 1 | 2 | 4) {
                return Err(format!(
                    "{}: [ABS_V2] CHANNELS={}: only 1, 2 and 4 supported (a critical error in the game)",
                    path.display(),
                    abs.channels
                ));
            }
        }
        Ok(abs)
    }

    /// `ABS::cycleMode` @ 0x14028eae0.
    pub fn cycle_mode(&mut self, dir: i32) {
        cycle_mode(self.is_present, &mut self.is_active, &mut self.current_mode, &mut self.slip_ratio_limit, &self.value_curve, dir);
    }

    /// `ABS::getCurrentMode` @ 0x14028ebc0.
    pub fn get_current_mode(&self) -> (u32, u32) {
        mode_pair(self.is_active, self.current_mode, &self.value_curve)
    }

    /// `ABS::isInAction` @ 0x14028f5d0: any wheel's brake is released.
    pub fn is_in_action(car: &RollingChassis) -> bool {
        car.tyres.iter().any(|tyre| tyre.abs_override < 1.0 || tyre.abs_override > 1.0)
    }

    /// `ABS::step` @ 0x14028f610.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        self.time_accumulator = dt + self.time_accumulator;
        if !(self.time_accumulator >= self.frequency) {
            return;
        }
        self.time_accumulator = 0.0;
        self.is_active = self.is_active && self.is_present;
        if self.is_active && car.speed * 3.6 > 20.0 {
            let limit = -self.slip_ratio_limit;
            let slip: [f32; 4] = std::array::from_fn(|wheel| car.tyres[wheel].status.slip_ratio);
            let locking = |wheel: usize| limit > slip[wheel];
            match self.channels {
                1 => {
                    let front = if locking(0) || locking(1) { 0.0f32 } else { 1.0 };
                    let rear = if locking(2) || locking(3) { 0.0f32 } else { 1.0 };
                    let both = if !(front < rear) { rear } else { front };
                    for tyre in &mut car.tyres {
                        tyre.abs_override = both;
                    }
                }
                2 => {
                    let front = if locking(0) || locking(1) { 0.0f32 } else { 1.0 };
                    let rear = if locking(2) || locking(3) { 0.0f32 } else { 1.0 };
                    car.tyres[0].abs_override = front;
                    car.tyres[1].abs_override = front;
                    car.tyres[2].abs_override = rear;
                    car.tyres[3].abs_override = rear;
                }
                4 => {
                    for wheel in 0..4 {
                        car.tyres[wheel].abs_override = if locking(wheel) { 0.0 } else { 1.0 };
                    }
                }
                _ => {}
            }
        } else {
            for tyre in &mut car.tyres {
                tyre.abs_override = 1.0;
            }
            self.current_value = 1.0;
        }
    }
}

/// AC's `EDL` (0x38 bytes): the electronic differential lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edl {
    /// `isPresent`
    pub is_present: bool,
    /// `isActive`
    pub is_active: bool,
    /// `wheelSpeedGainPower`: 1 / (`MAX_SPIN_POWER` - `DEAD_ZONE_POWER`)
    pub wheel_speed_gain_power: f32,
    /// `wheelSpeedGainCoast`
    pub wheel_speed_gain_coast: f32,
    /// `deadZonePower`
    pub dead_zone_power: f32,
    /// `deadZoneCoast`
    pub dead_zone_coast: f32,
    /// `brakeTorquePower`, Nm at full level
    pub brake_torque_power: f32,
    /// `brakeTorqueCoast`
    pub brake_torque_coast: f32,
    /// `outLevel`: 0..1
    pub out_level: f32,
    /// `outBrakeTorque`
    pub out_brake_torque: f32,
    /// `speedDiff`: faster wheel over slower wheel, minus 1
    pub speed_diff: f32,
    /// `leftTyreIndex`
    pub left_tyre_index: usize,
    /// `rightTyreIndex`
    pub right_tyre_index: usize,
}

impl Default for Edl {
    /// The constructor's values (inside `Car::Car`).
    fn default() -> Edl {
        Edl {
            is_present: false,
            is_active: false,
            wheel_speed_gain_power: 0.01,
            wheel_speed_gain_coast: 0.01,
            dead_zone_power: 0.0,
            dead_zone_coast: 0.0,
            brake_torque_power: 2000.0,
            brake_torque_coast: 2000.0,
            out_level: 0.0,
            out_brake_torque: 0.0,
            speed_diff: 0.0,
            left_tyre_index: 2,
            right_tyre_index: 3,
        }
    }
}

impl Edl {
    /// `EDL::init` @ 0x1402babf0: `electronics.ini [EDL]`; the driven pair from the
    /// drivetrain's layout.
    pub fn new(data_path: &Path, traction_type: TractionType) -> Result<Edl, String> {
        let mut edl = Edl::default();
        if matches!(traction_type, TractionType::Fwd | TractionType::Awd) {
            edl.left_tyre_index = 0;
            edl.right_tyre_index = 1;
        }
        let path = data_path.join("electronics.ini");
        if !path.is_file() {
            return Ok(edl);
        }
        let ini = IniReader::load(&path)?;
        if !ini.ready || !ini.has_section("EDL") {
            return Ok(edl);
        }
        edl.is_present = ini.get_int("EDL", "PRESENT")? != 0;
        edl.is_active = ini.get_int("EDL", "ACTIVE")? != 0;
        edl.brake_torque_power = ini.get_float("EDL", "BRAKE_TORQUE_POWER")?;
        edl.brake_torque_coast = ini.get_float("EDL", "BRAKE_TORQUE_COAST")?;
        edl.dead_zone_coast = ini.get_float("EDL", "DEAD_ZONE_COAST")?;
        edl.dead_zone_power = ini.get_float("EDL", "DEAD_ZONE_POWER")?;
        edl.wheel_speed_gain_power = 1.0 / (ini.get_float("EDL", "MAX_SPIN_POWER")? - edl.dead_zone_power);
        edl.wheel_speed_gain_coast = 1.0 / (ini.get_float("EDL", "MAX_SPIN_COAST")? - edl.dead_zone_coast);
        if !edl.wheel_speed_gain_power.is_finite() || !edl.wheel_speed_gain_coast.is_finite() {
            return Err(format!(
                "{}: [EDL] MAX_SPIN equals DEAD_ZONE: the gain is not finite (a critical error in the game)",
                path.display()
            ));
        }
        Ok(edl)
    }

    /// `EDL::step` @ 0x1402bb460.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis) {
        if !self.is_active || !self.is_present {
            return;
        }
        let (left, right) = (self.left_tyre_index, self.right_tyre_index);
        let wl = car.tyres[left].status.angular_velocity.abs();
        let wr = car.tyres[right].status.angular_velocity.abs();
        if !(wl < 0.0 || wl > 0.0) || !(wr < 0.0 || wr > 0.0) {
            return;
        }
        let larger = if wl > wr { wl } else { wr };
        let smaller = if !(wr >= wl) { wr } else { wl };
        self.speed_diff = larger / smaller - 1.0;
        // power or coast by the sign of the longitudinal acceleration
        let (torque, dead_zone, gain) = if 0.0 >= car.acc_g.z {
            (self.brake_torque_coast, self.dead_zone_coast, self.wheel_speed_gain_coast)
        } else {
            (self.brake_torque_power, self.dead_zone_power, self.wheel_speed_gain_power)
        };
        let (excess, fast) = if wl > wr { ((wl / wr - 1.0) - dead_zone, left) } else { ((wr / wl - 1.0) - dead_zone, right) };
        let level = gain * if excess > 0.0 { excess } else { 0.0 };
        let level = if level > 1.0 {
            1.0
        } else if level >= 0.0 {
            level
        } else {
            0.0
        };
        self.out_level = level;
        let inputs = &mut car.tyres[fast].inputs;
        inputs.brake_torque = level * torque + inputs.brake_torque;
        self.out_brake_torque = torque * self.out_level;
    }
}

/// AC's `StabilityControl` (0x18 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StabilityControl {
    /// `gain`: the game option in 0..1 (0 = off); 2 on an AI car
    pub gain: f32,
    /// `useBeta`: the AI's form, from the body's side-slip angle
    pub use_beta: bool,
    /// `maxGain`
    pub max_gain: f32,
}

impl Default for StabilityControl {
    /// `StabilityControl::init` @ 0x1402bfa30.
    fn default() -> StabilityControl {
        StabilityControl { gain: 0.0, use_beta: false, max_gain: 30.0 }
    }
}

impl StabilityControl {
    /// `StabilityControl::step` @ 0x1402bfa50: a yaw torque about the body's own up axis.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis) {
        let gain = self.gain;
        if !(gain > 0.0) {
            return;
        }
        let torque = if self.use_beta {
            let v = car.core.get_local_velocity(car.body);
            if !(v.z > 5.0) {
                return;
            }
            let beta = atanf(v.x / v.z);
            let beta = if beta > 0.523_589_97 {
                0.523_589_97
            } else if !(beta >= -0.523_589_97) {
                -0.523_589_97
            } else {
                beta
            };
            let mass = car.core.get_mass(car.body);
            mass * ((beta * gain) * 10.0)
        } else {
            let angle = |wheel: usize| car.tyres[wheel].status.slip_angle_rad;
            let front = (angle(1) + angle(0)).abs();
            let rear = (angle(3) + angle(2)).abs();
            let difference = (rear - front) * 0.5;
            if !(difference > 0.0 && gain > 0.0 && car.speed * 3.6 > 5.0) {
                return;
            }
            let rear_left = angle(2);
            let sign = if rear_left > 0.0 {
                1.0
            } else if rear_left >= 0.0 {
                0.0
            } else {
                -1.0
            };
            let mass = car.core.get_mass(car.body);
            -((((mass * difference) * gain) * self.max_gain) * sign)
        };
        let previous = std::mem::replace(&mut car.core.source, ForceSource::Stability);
        car.core.add_local_torque(car.body, &Vec3f::new(0.0, torque, 0.0));
        car.core.source = previous;
    }
}

/// AC's `SpeedLimiter` (0x10 bytes): the pit limiter. It is not a button: it works whenever a
/// tyre stands on a surface marked as pit lane.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpeedLimiter {
    /// `shoudLimit`: a tyre is on a pit-lane surface
    pub shoud_limit: bool,
    /// `isLimiting`: the engine is being cut
    pub is_limiting: bool,
}

impl SpeedLimiter {
    /// `SpeedLimiter::step` @ 0x1402bb910.
    pub fn step(&mut self, car: &mut RollingChassis) {
        self.shoud_limit = false;
        self.is_limiting = false;
        for tyre in &car.tyres {
            if let Some(surface) = &tyre.surface_def {
                self.shoud_limit |= surface.is_pitlane;
            }
        }
        if !self.shoud_limit {
            return;
        }
        let kmh = car.speed * 3.6;
        if kmh > 80.0 {
            set_engine_override(car, 0.0);
            self.is_limiting = true;
        }
        if kmh > 81.0 {
            let brake = get_optimal_brake(car);
            if let Some(brakes) = &mut car.brake_system {
                brakes.base_mut().electronic_override = brake;
            }
        }
    }
}

/// `Tyre::getDX` @ 0x140280240: the longitudinal grip coefficient at a load, N.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn tyre_get_dx(tyre: &VanillaTyre, load: f32) -> f32 {
    let blister = || -> f64 {
        let b = tyre.status.blister * 0.01;
        if b > 1.0 {
            1.0
        } else if !(b >= 0.0) {
            0.0
        } else {
            b
        }
    };
    let model = &tyre.model_data;
    if model.dx_load_curve.get_count() > 0 {
        let b = blister();
        let d = model.dx_load_curve.get_cubic_spline_value(load);
        return (d as f64 / (b * 0.200_000_002_980_232_24 + 1.0)) as f32;
    }
    let exponent = model.ls_exp_x;
    if exponent < 0.0 || exponent > 0.0 {
        let mut d = 0.0f32;
        if load < 0.0 || load > 0.0 {
            d = powf(load, exponent) * model.ls_mult_x / load;
        }
        let b = blister();
        return (d as f64 / (b * 0.200_000_002_980_232_24 + 1.0)) as f32;
    }
    let b = blister();
    let d = (load * 0.0005) * model.dx1 + model.dx0;
    (d as f64 / (b * 0.200_000_002_980_232_24 + 1.0)) as f32
}

/// `RaceEngineer::getOptimalBrake` @ 0x14027c320: the brake pedal that just reaches the grip
/// of the axle that locks first.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn get_optimal_brake(car: &mut RollingChassis) -> f32 {
    let mut grip = [0.0f32; 4];
    for (wheel, value) in grip.iter_mut().enumerate() {
        let load = car.tyres[wheel].status.load;
        let d = tyre_get_dx(&car.tyres[wheel], load);
        *value = car.tyres[wheel].get_corrected_d(d, false);
    }
    let load = |wheel: usize| car.tyres[wheel].status.load;
    let front = ((grip[1] * load(1) + grip[0] * load(0)) * 0.5) * car.tyres[0].status.loaded_radius;
    let rear = ((grip[3] * load(3) + grip[2] * load(2)) * 0.5) * car.tyres[2].status.loaded_radius;
    let (power, bias) = match &car.brake_system {
        Some(brakes) => (brakes.get_brake_power(), brakes.base().front_bias),
        None => (0.0, 0.0),
    };
    let front = front / (power * bias);
    let rear = rear / (power * (1.0 - bias));
    if front >= rear {
        rear
    } else {
        front
    }
}

/// The aids of one car, as members of AC's `Car`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AidsBase {
    /// `Car::tractionControl`
    pub traction_control: TractionControl,
    /// `Car::abs`
    pub abs: Abs,
    /// `Car::edl`
    pub edl: Edl,
    /// `Car::stabilityControl`
    pub stability_control: StabilityControl,
    /// `Car::speedLimiter`
    pub speed_limiter: SpeedLimiter,
}

impl AidsBase {
    /// What the game's options (`cfg/assists.ini [ASSISTS]`, applied to the player's car by
    /// `DrivingAssistManager::DrivingAssistManager` @ 0x1400fbd90 through three jobs for the
    /// physics thread) do to the aids when a session starts. `abs` and `traction_control`:
    /// 0 = off (not active, not present), 1 = factory (active; present as the car's file
    /// says), 2 = on (active and present, also on a car that does not have the aid).
    /// `stability_percent`: 0 to 100. Read from the listing, not run: the recordings are made
    /// without that manager, so there the car's file decides alone.
    pub fn apply_driving_assists(&mut self, abs: i32, traction_control: i32, stability_percent: f32) {
        let on = abs != 0;
        self.abs.is_active = on;
        if abs == 0 || abs == 2 {
            self.abs.is_present = on;
        }
        let on = traction_control != 0;
        self.traction_control.is_active = on;
        if traction_control == 0 || traction_control == 2 {
            self.traction_control.is_present = on;
        }
        let gain = stability_percent * 0.01;
        self.stability_control.gain = if 0.0 < gain { gain } else { 0.0 };
    }
}

/// The aids slot.
pub trait AidsModel {
    /// `Car::stepComponents` position 2: `EDL::step` @ 0x1402bb460, right after the brakes.
    fn step_edl(&mut self, car: &mut RollingChassis, dt: f32);
    /// Positions 16 to 18: `ABS::step` @ 0x14028f610, `TractionControl::step` @ 0x140290200,
    /// `SpeedLimiter::step` @ 0x1402bb910.
    fn step(&mut self, car: &mut RollingChassis, dt: f32);
    /// Position 27: `StabilityControl::step` @ 0x1402bfa50.
    fn step_stability(&mut self, car: &mut RollingChassis, dt: f32);
    fn base(&self) -> &AidsBase;
    fn base_mut(&mut self) -> &mut AidsBase;
    /// The values of the model under the names of the recordings.
    fn trace(&self, out: &mut Vec<TraceValue>);
    /// The state a run needs to go on from the middle, as 32-bit words.
    fn save_state(&self, out: &mut Vec<u32>);
    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String>;
}

/// AC's aids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VanillaAids {
    pub base: AidsBase,
}

impl VanillaAids {
    /// The aids as `Car::Car` builds them from `electronics.ini`: `EDL::init`, `ABS::init`,
    /// `TractionControl::init`, `SpeedLimiter::init`, `StabilityControl::init`.
    pub fn new(data_path: &Path, traction_type: TractionType) -> Result<VanillaAids, String> {
        Ok(VanillaAids {
            base: AidsBase {
                edl: Edl::new(data_path, traction_type)?,
                abs: Abs::new(data_path)?,
                traction_control: TractionControl::new(data_path)?,
                speed_limiter: SpeedLimiter::default(),
                stability_control: StabilityControl::default(),
            },
        })
    }
}

impl AidsModel for VanillaAids {
    fn step_edl(&mut self, car: &mut RollingChassis, _dt: f32) {
        self.base.edl.step(car);
    }

    fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        self.base.abs.step(car, dt);
        self.base.traction_control.step(car, dt);
        self.base.speed_limiter.step(car);
    }

    fn step_stability(&mut self, car: &mut RollingChassis, _dt: f32) {
        self.base.stability_control.step(car);
    }

    fn base(&self) -> &AidsBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AidsBase {
        &mut self.base
    }

    fn trace(&self, out: &mut Vec<TraceValue>) {
        let b = &self.base;
        out.push(TraceValue::i("tc.isActive", b.traction_control.is_active as i32));
        out.push(TraceValue::i("tc.isInAction", b.traction_control.is_in_action as i32));
        out.push(TraceValue::f("tc.slipRatioLimit", b.traction_control.slip_ratio_limit));
        out.push(TraceValue::i("abs.isPresent", b.abs.is_present as i32));
        out.push(TraceValue::i("abs.isActive", b.abs.is_active as i32));
        out.push(TraceValue::f("abs.currentValue", b.abs.current_value));
        out.push(TraceValue::f("edl.outLevel", b.edl.out_level));
        out.push(TraceValue::f("stability.gain", b.stability_control.gain));
        out.push(TraceValue::i("stability.useBeta", b.stability_control.use_beta as i32));
        out.push(TraceValue::i("speedLimiter.isLimiting", b.speed_limiter.is_limiting as i32));
        out.push(TraceValue::f("tc.timeAccumulator", b.traction_control.time_accumulator).extra());
        out.push(TraceValue::i("tc.currentMode", b.traction_control.current_mode as i32).extra());
        out.push(TraceValue::f("abs.timeAccumulator", b.abs.time_accumulator).extra());
        out.push(TraceValue::f("abs.slipRatioLimit", b.abs.slip_ratio_limit).extra());
        out.push(TraceValue::i("abs.currentMode", b.abs.current_mode as i32).extra());
        out.push(TraceValue::f("edl.outBrakeTorque", b.edl.out_brake_torque).extra());
        out.push(TraceValue::f("edl.speedDiff", b.edl.speed_diff).extra());
        out.push(TraceValue::i("speedLimiter.shoudLimit", b.speed_limiter.shoud_limit as i32).extra());
    }

    fn save_state(&self, out: &mut Vec<u32>) {
        let b = &self.base;
        let tc = &b.traction_control;
        out.extend([tc.is_present as u32, tc.is_active as u32, tc.is_in_action as u32, tc.current_mode]);
        out.extend([tc.slip_ratio_limit, tc.time_accumulator].map(f32::to_bits));
        let abs = &b.abs;
        out.extend([abs.is_present as u32, abs.is_active as u32, abs.current_mode]);
        out.extend([abs.slip_ratio_limit, abs.time_accumulator, abs.current_value].map(f32::to_bits));
        let edl = &b.edl;
        out.extend([edl.is_present as u32, edl.is_active as u32]);
        out.extend([edl.out_level, edl.out_brake_torque, edl.speed_diff].map(f32::to_bits));
        out.extend([b.stability_control.gain.to_bits(), b.stability_control.use_beta as u32]);
        out.extend([b.speed_limiter.shoud_limit as u32, b.speed_limiter.is_limiting as u32]);
    }

    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut next = || words.next().ok_or("the aids' state is too short".to_string());
        let b = &mut self.base;
        let tc = &mut b.traction_control;
        tc.is_present = next()? != 0;
        tc.is_active = next()? != 0;
        tc.is_in_action = next()? != 0;
        tc.current_mode = next()?;
        tc.slip_ratio_limit = f32::from_bits(next()?);
        tc.time_accumulator = f32::from_bits(next()?);
        let abs = &mut b.abs;
        abs.is_present = next()? != 0;
        abs.is_active = next()? != 0;
        abs.current_mode = next()?;
        abs.slip_ratio_limit = f32::from_bits(next()?);
        abs.time_accumulator = f32::from_bits(next()?);
        abs.current_value = f32::from_bits(next()?);
        let edl = &mut b.edl;
        edl.is_present = next()? != 0;
        edl.is_active = next()? != 0;
        edl.out_level = f32::from_bits(next()?);
        edl.out_brake_torque = f32::from_bits(next()?);
        edl.speed_diff = f32::from_bits(next()?);
        b.stability_control.gain = f32::from_bits(next()?);
        b.stability_control.use_beta = next()? != 0;
        b.speed_limiter.shoud_limit = next()? != 0;
        b.speed_limiter.is_limiting = next()? != 0;
        Ok(())
    }
}
