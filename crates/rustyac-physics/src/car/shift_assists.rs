// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The small helpers around clutch and gearbox, members of AC's `Car`:
//!
//! * [`Autoclutch`]: works the clutch for the driver when pulling away and during paddle
//!   shifts (it rewrites `controls.clutch`);
//! * [`AutoBlip`]: opens the throttle for a moment after a down-shift request;
//! * [`AutoShifter`]: the "automatic gearbox" aid, presses the paddles;
//! * [`GearChanger`]: turns the paddles or the H-shifter's lever into gearbox requests.
//!
//! They only run in a car that has its drivetrain
//! ([`RollingChassis::install_drivetrain`](super::RollingChassis::install_drivetrain)).

use std::path::Path;

use super::chassis::RollingChassis;
use super::drivetrain::{cvttss2si, has_session_started, DrivetrainModel, GearChangeRequest, OnGearRequestEvent, TractionType, K2PI};
use super::feed::CarControls;
use crate::curve::Curve;
use crate::data::ini::IniReader;

/// The three-way clamp to 0..1 of the machine code: above 1 gives 1, anything that is not at
/// least 0 (a NaN too) gives 0.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn saturate(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if !(x >= 0.0) {
        0.0
    } else {
        x
    }
}

/// AC's `ClutchSequence` (0x88 bytes): one clutch profile being played back.
#[derive(Clone, Debug, PartialEq)]
pub struct ClutchSequence {
    /// `clutchCurve`: time (s) -> clutch value
    pub clutch_curve: Curve,
    /// `currentTime`, s
    pub current_time: f32,
    /// `isDone`
    pub is_done: bool,
}

impl Default for ClutchSequence {
    /// Nothing is playing.
    fn default() -> ClutchSequence {
        ClutchSequence { clutch_curve: Curve::new(), current_time: 0.0, is_done: true }
    }
}

/// AC's `Autoclutch` (0x1a8 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Autoclutch {
    /// `rpmMin`, `rpmMax` (+0x00): between them the clutch closes in proportion
    pub rpm_min: f32,
    pub rpm_max: f32,
    /// `clutchSpeed` (+0x08): how fast the clutch value may move, 1/s
    pub clutch_speed: f32,
    /// `useAutoOnStart` (+0x0c): the "automatic clutch" driving aid
    pub use_auto_on_start: bool,
    /// `useAutoOnChange` (+0x0d): play the car's shift profiles
    pub use_auto_on_change: bool,
    /// `isForced` (+0x0e): the car always has an automatic clutch
    pub is_forced: bool,
    /// `clutchSequence` (+0x18)
    pub clutch_sequence: ClutchSequence,
    /// `upshiftProfile`, `downshiftProfile` (+0xa0, +0x120)
    pub upshift_profile: Curve,
    pub downshift_profile: Curve,
    /// `clutchValueSignal` (+0x1a0)
    pub clutch_value_signal: f32,
}

impl Autoclutch {
    /// `Autoclutch::init` @ 0x1402b87d0 with `Autoclutch::loadINI` @ 0x1402b8890.
    pub fn new(data_path: &Path) -> Result<Autoclutch, String> {
        let mut a = Autoclutch {
            rpm_min: 1500.0,
            rpm_max: 2500.0,
            clutch_speed: 1.0,
            use_auto_on_start: true,
            use_auto_on_change: true,
            ..Autoclutch::default()
        };
        let ini = IniReader::load(&data_path.join("drivetrain.ini"))?;
        if ini.has_section("HEADER") && ini.get_int("HEADER", "VERSION")? > 2 {
            a.is_forced = ini.get_int("AUTOCLUTCH", "FORCED_ON")? != 0;
        }
        let upshift = ini.get_string("AUTOCLUTCH", "UPSHIFT_PROFILE");
        let downshift = ini.get_string("AUTOCLUTCH", "DOWNSHIFT_PROFILE");
        a.use_auto_on_change = ini.get_int("AUTOCLUTCH", "USE_ON_CHANGES")? != 0;
        // a profile is a section with three times in ms: clutch fully open at POINT_0, starts
        // to close at POINT_1, closed again at POINT_2
        for (name, profile) in [(&upshift, &mut a.upshift_profile), (&downshift, &mut a.downshift_profile)] {
            if name != "NONE" {
                profile.add_value(0.0, 1.0);
                profile.add_value(ini.get_float(name, "POINT_0")? * 0.001, 0.0);
                profile.add_value(ini.get_float(name, "POINT_1")? * 0.001, 0.0);
                profile.add_value(ini.get_float(name, "POINT_2")? * 0.001, 1.0);
            }
        }
        a.rpm_min = ini.get_int("AUTOCLUTCH", "MIN_RPM")? as f32;
        a.rpm_max = ini.get_int("AUTOCLUTCH", "MAX_RPM")? as f32;
        if a.rpm_min == 0.0 || a.rpm_max == 0.0 {
            a.rpm_min = 1500.0;
            a.rpm_max = 2500.0;
        }
        Ok(a)
    }

    /// `Autoclutch::step` @ 0x1402b9590, called from `Car::step` before the components.
    /// `speed` is the car's cached speed (m/s), `engine_velocity` and `current_gear` are the
    /// drivetrain's `engine.velocity` (rad/s) and `currentGear`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, controls: &mut CarControls, speed: f32, engine_velocity: f64, current_gear: i32, dt: f32) {
        if !self.clutch_sequence.is_done {
            if speed * 3.6 > 5.0 {
                self.step_sequence(controls, dt);
                return;
            }
            self.clutch_sequence.is_done = true;
        }
        if !self.use_auto_on_start && !self.is_forced {
            return;
        }
        let mut target = 1.0f32;
        // double precision, one rounding at the end
        let rpm = ((engine_velocity * K2PI) * 60.0) as f32;
        let mut to_zero = false;
        if current_gear == 0 || current_gear == 2 {
            // reverse and first: the clutch closes between rpmMin and rpmMax
            let (min, max) = (self.rpm_min, self.rpm_max);
            if rpm >= min && !(rpm > max) {
                target = (rpm - min) / (max - min);
                self.clutch_value_signal = target;
            }
            if rpm > max {
                target = 1.0;
            }
            to_zero = !(rpm >= min);
        } else if current_gear == 1 {
            // neutral, nearly standing: closed with the throttle open, else open
            if !(speed * 3.6 >= 5.0) {
                if 0.2 >= controls.gas {
                    to_zero = true;
                } else {
                    self.clutch_value_signal = 1.0;
                }
            }
        } else {
            to_zero = !(rpm >= self.rpm_min);
        }
        if to_zero {
            target = 0.0;
            self.clutch_value_signal = 0.0;
        }
        // the value moves towards the target at `clutchSpeed`
        let signal = self.clutch_value_signal;
        let step = dt * self.clutch_speed;
        if (target - signal).abs() >= step {
            if target > signal {
                self.clutch_value_signal = step + signal;
            } else {
                self.clutch_value_signal = signal - step;
            }
        } else {
            self.clutch_value_signal = target;
        }
        controls.clutch = saturate(self.clutch_value_signal);
    }

    /// `Autoclutch::stepSequence` @ 0x1402b97a0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_sequence(&mut self, controls: &mut CarControls, dt: f32) {
        let sequence = &mut self.clutch_sequence;
        let value = sequence.clutch_curve.get_value(sequence.current_time);
        sequence.current_time = dt + sequence.current_time;
        self.clutch_value_signal = value;
        // `Curve::getMaxReference`: the last time of the profile
        let end = sequence.clutch_curve.references().last().copied().unwrap_or(0.0);
        if !(end >= sequence.current_time) {
            sequence.is_done = true;
        }
        controls.clutch = saturate(self.clutch_value_signal);
    }

    /// `Autoclutch::onGearRequest` @ 0x1402b9350: a paddle shift starts the car's profile for
    /// it (if the clutch is not already open, and the profile has its four points).
    pub fn on_gear_request(&mut self, event: &OnGearRequestEvent) {
        if !self.use_auto_on_change {
            return;
        }
        // a double comparison with the double 0.01
        let closed = self.clutch_value_signal as f64 > 0.01;
        if event.request == GearChangeRequest::Down && closed && self.downshift_profile.get_count() == 4 {
            self.clutch_sequence =
                ClutchSequence { clutch_curve: self.downshift_profile.clone(), current_time: 0.0, is_done: false };
        }
        if event.request == GearChangeRequest::Up && closed && self.upshift_profile.get_count() == 4 {
            self.clutch_sequence = ClutchSequence { clutch_curve: self.upshift_profile.clone(), current_time: 0.0, is_done: false };
        }
    }
}

/// AC's `AutoBlip` (0xa8 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutoBlip {
    /// `isActive` (+0x00): the driving aid
    pub is_active: bool,
    /// `blipProfile` (+0x10): ms since the request -> throttle
    pub blip_profile: Curve,
    /// `blipStartTime` (+0x90): physics clock of the last down-shift request, ms
    pub blip_start_time: f64,
    /// `isElectronic` (+0x98): the car blips by itself
    pub is_electronic: bool,
    /// `blipPerformTime` (+0xa0), ms
    pub blip_perform_time: f64,
}

impl AutoBlip {
    /// `AutoBlip::init` @ 0x1402b9910 with `AutoBlip::loadINI` @ 0x1402b99b0.
    pub fn new(data_path: &Path) -> Result<AutoBlip, String> {
        let mut blip = AutoBlip::default();
        let ini = IniReader::load(&data_path.join("drivetrain.ini"))?;
        let level = ini.get_float("AUTOBLIP", "LEVEL")?;
        blip.blip_profile.add_value(0.0, 0.0);
        blip.blip_profile.add_value(ini.get_float("AUTOBLIP", "POINT_0")?, level);
        blip.blip_profile.add_value(ini.get_float("AUTOBLIP", "POINT_1")?, level);
        blip.blip_profile.add_value(ini.get_float("AUTOBLIP", "POINT_2")?, 0.0);
        blip.blip_perform_time = blip.blip_profile.references().last().copied().unwrap_or(0.0) as f64;
        // `ucomiss` + `jne`: not zero and a number
        let electronic = ini.get_float("AUTOBLIP", "ELECTRONIC")?;
        blip.is_electronic = electronic < 0.0 || electronic > 0.0;
        blip.blip_start_time = 0.0;
        blip.is_active = true;
        Ok(blip)
    }

    /// The gear-request handler `AutoBlip::init` installs (lambda @ 0x1402b9880): a down-shift
    /// request with the clutch closed starts the clock.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn on_gear_request(&mut self, event: &OnGearRequestEvent, clutch: f32, physics_time: f64) {
        if event.request == GearChangeRequest::Down && !(0.1 >= clutch) {
            self.blip_start_time = physics_time;
        }
    }

    /// `AutoBlip::step` @ 0x1402b9ef0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, controls: &mut CarControls, is_controls_locked: bool, speed: f32, physics_time: f64) {
        if is_controls_locked || !(speed * 3.6 >= 5.0) {
            return;
        }
        if !self.is_electronic && !self.is_active {
            return;
        }
        let since = physics_time - self.blip_start_time;
        if !(since >= 0.0) || since >= self.blip_perform_time {
            return;
        }
        if self.blip_profile.get_count() != 4 {
            return;
        }
        let blip = self.blip_profile.get_value(since as f32);
        let gas = if controls.gas > blip { controls.gas } else { blip };
        controls.gas = if gas > 1.0 {
            1.0
        } else if gas < 0.0 || gas.is_nan() {
            0.0
        } else {
            gas
        };
    }
}

/// AC's `AutoShifter` (0x28 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AutoShifter {
    /// `isActive` (+0x00): the "automatic gearbox" driving aid
    pub is_active: bool,
    /// `changeUpRpm`, `changeDnRpm` (+0x04)
    pub change_up_rpm: i32,
    pub change_dn_rpm: i32,
    /// `slipThreshold` (+0x0c)
    pub slip_threshold: f32,
    /// `butGearUp`, `butGearDn` (+0x18): never used
    pub but_gear_up: bool,
    pub but_gear_dn: bool,
    /// `gasCutoff` (+0x1c): seconds of throttle cut left
    pub gas_cutoff: f32,
    /// `gasCutoffTime` (+0x20)
    pub gas_cutoff_time: f32,
}

impl AutoShifter {
    /// `AutoShifter::init` @ 0x1402ba020 with `AutoShifter::loadINI` @ 0x1402ba050.
    pub fn new(data_path: &Path) -> Result<AutoShifter, String> {
        let mut shifter = AutoShifter {
            is_active: false,
            change_up_rpm: 0,
            change_dn_rpm: 4000,
            slip_threshold: 0.8,
            but_gear_up: false,
            but_gear_dn: false,
            gas_cutoff: 0.0,
            gas_cutoff_time: 0.5,
        };
        let ini = IniReader::load(&data_path.join("drivetrain.ini"))?;
        if ini.ready && ini.has_section("AUTO_SHIFTER") {
            shifter.change_up_rpm = ini.get_int("AUTO_SHIFTER", "UP")?;
            shifter.change_dn_rpm = ini.get_int("AUTO_SHIFTER", "DOWN")?;
            shifter.slip_threshold = ini.get_float("AUTO_SHIFTER", "SLIP_THRESHOLD")?;
            shifter.gas_cutoff_time = ini.get_float("AUTO_SHIFTER", "GAS_CUTOFF_TIME")?;
        } else {
            let ai = IniReader::load(&data_path.join("ai.ini"))?;
            if ai.ready {
                shifter.change_up_rpm = ai.get_int("GEARS", "UP")?;
                shifter.change_dn_rpm = ai.get_int("GEARS", "DOWN")?;
                shifter.slip_threshold = ai.get_float("GEARS", "SLIP_THRESHOLD")?;
                shifter.gas_cutoff_time = ai.get_float("GEARS", "GAS_CUTOFF_TIME")?;
            }
        }
        Ok(shifter)
    }

    /// `AutoShifter::step` @ 0x1402ba7f0. `drivetrain` is the car's (taken out of its slot).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis, drivetrain: &dyn DrivetrainModel, dt: f32) {
        if !has_session_started(car, 300.0) || !self.is_active {
            return;
        }
        let gear = drivetrain.base().current_gear;
        if gear == 0 {
            return;
        }
        if self.change_up_rpm == 0 {
            // first use without shift points in the car's files: from the engine
            let engine = drivetrain.engine();
            let max_power = cvttss2si(engine.get_max_power_rpm());
            let limiter = engine.get_limiter_rpm();
            let lower = if limiter < max_power { limiter } else { max_power };
            self.change_up_rpm = cvttss2si(lower as f32 * 0.98);
            self.change_dn_rpm = cvttss2si(engine.get_max_torque_rpm() * 1.1);
        }
        // the driver's own paddle wins
        if car.controls.gear_up || car.controls.gear_dn {
            return;
        }
        let slipping = get_driving_tyres_slip(car, drivetrain.base().traction_type) > self.slip_threshold && car.speed > 5.0;
        if !drivetrain.is_changing_gear() && (car.controls.clutch > 0.99 || gear == 1) && !slipping {
            let rpm = drivetrain.get_engine_rpm();
            let top = drivetrain.base().gears.len() as i32 - 1;
            if rpm > self.change_up_rpm as f32 && gear < top && car.controls.gas > 0.2 && 0.0 >= self.gas_cutoff {
                car.controls.gear_up = true;
                self.gas_cutoff = self.gas_cutoff_time;
            }
            // second gear is held longer
            let down = if gear == 3 { cvttss2si(self.change_dn_rpm as f32 * 0.65) } else { self.change_dn_rpm };
            if !(rpm >= down as f32) && gear > 2 && car.controls.clutch > 0.85 && 0.0 >= self.gas_cutoff {
                car.controls.gear_dn = true;
            }
        }
        // nearly standing with the throttle shut: down through the box
        if !(car.speed >= 2.0)
            && !drivetrain.is_changing_gear()
            && !(car.controls.gas >= 0.1)
            && 0.0 >= self.gas_cutoff
            && gear > 2
        {
            car.controls.gear_dn = true;
        }
        if self.gas_cutoff > 0.0 {
            self.gas_cutoff -= dt;
            car.controls.gas = 0.0;
        }
    }
}

/// `RaceEngineer::getDrivingTyresSlip` @ 0x14027bc20: the larger `ndSlip` of the driven pair
/// (the front pair for front-wheel drive, the rear pair for everything else).
pub fn get_driving_tyres_slip(car: &RollingChassis, traction_type: TractionType) -> f32 {
    let first = if traction_type == TractionType::Fwd { 0 } else { 2 };
    let (a, b) = (car.tyres[first].status.nd_slip, car.tyres[first + 1].status.nd_slip);
    if a > b {
        a
    } else {
        b
    }
}

/// AC's `GearChanger` (0x18 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearChanger {
    /// `wasGearUpTriggered`, `wasGearDnTriggered` (+0x00): a shift began in this step
    pub was_gear_up_triggered: bool,
    pub was_gear_dn_triggered: bool,
    /// `lastGearUp`, `lastGearDn` (+0x10): the paddles in the step before
    pub last_gear_up: bool,
    pub last_gear_dn: bool,
}

impl GearChanger {
    /// `GearChanger::step` @ 0x1402bab50: the H-shifter's lever goes straight to the gearbox;
    /// a paddle counts once, when it goes down.
    pub fn step(&mut self, car: &mut RollingChassis, drivetrain: &mut dyn DrivetrainModel) {
        self.was_gear_up_triggered = false;
        self.was_gear_dn_triggered = false;
        if car.controls.requested_gear_index != -1 {
            drivetrain.set_current_gear(car.controls.requested_gear_index, false, car);
            return;
        }
        if car.controls.gear_up && !self.last_gear_up {
            self.was_gear_up_triggered = drivetrain.gear_up(car);
        }
        if car.controls.gear_dn && !self.last_gear_dn {
            self.was_gear_dn_triggered = drivetrain.gear_down(car);
        }
        self.last_gear_up = car.controls.gear_up;
        self.last_gear_dn = car.controls.gear_dn;
    }
}
