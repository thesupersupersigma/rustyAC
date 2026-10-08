// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `Kers` (`Car::kers`, kers.ini): a battery that fills under braking and an electric
//! motor on the crankshaft or on the driven wheels that empties it, on a button or by a
//! controller file.
//!
//! The motor's torque is a table over the speed of what it is attached to, times `input`
//! (0..1). `Kers::step` runs before the drivetrain in the car's step and settles `input` and
//! the battery; the drivetrain then asks [`Kers::get_output_torque`] and adds the answer to the
//! engine's torque (inside `Engine::step`) or to the driven tyres' feedback torque.
//!
//! A car whose `ers.ini` can be read has no KERS, whatever its `kers.ini` says.

use std::path::Path;

use super::chassis::RollingChassis;
use super::drivetrain::{DrivetrainBase, K2PI};
use super::dynamic_controller::{CarSignals, DynamicController};
use super::engine::saturate;
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::IniReader;

/// AC's `KersAttachment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum KersAttachment {
    /// The motor turns the crankshaft: its torque is added to the engine's.
    #[default]
    Engine = 0,
    /// The motor turns the driven wheels directly.
    Wheels = 1,
}

/// AC's `Kers` (0xf8 bytes) of a car that has one (`present`).
#[derive(Clone, Debug, PartialEq)]
pub struct Kers {
    /// `attachment` (+0x08)
    pub attachment: KersAttachment,
    /// `input` (+0x1c): how much of the motor is used, 0..1, as the last step left it
    pub input: f32,
    /// `brakeForMaxCharge` (+0x20): brake torque at which the battery fills at its full rate
    pub brake_for_max_charge: f32,
    /// `charge` (+0x24), 0..1
    pub charge: f32,
    /// `chargeK` (+0x28)
    pub charge_k: f32,
    /// `dischargeK` (+0x2c): the share of the battery a second at full input costs
    pub discharge_k: f32,
    /// `negativeInputChargeK` (+0x34): read from the file, used by a branch nothing reaches
    pub negative_input_charge_k: f32,
    /// `hasButtonOverride` (+0x38): with a controller, the button still gives full input
    pub has_button_override: bool,
    /// `currentJ` (+0x3c): energy handed out in this lap, J
    pub current_j: f32,
    /// `maxJ` (+0x40): the most per lap, J; 0: no limit
    pub max_j: f32,
    /// `torqueLUT` (+0x48): rpm of the attachment -> Nm
    pub torque_lut: Curve,
    /// `controller` (+0xc8), `hasController` (+0xf0): the file named by `CONTROLLER`
    pub controller: DynamicController,
    pub has_controller: bool,
}

impl Kers {
    /// `Kers::init` @ 0x1402b7360 for a car whose brakes are in their slot (the battery's
    /// filling rate is set against the brakes' torque). `None`: the car has no kers.ini.
    pub fn load(car: &RollingChassis, data_path: &Path) -> Result<Option<Kers>, String> {
        let ini = IniReader::load(&data_path.join("kers.ini"))?;
        if !ini.ready {
            return Ok(None);
        }
        let version = ini.get_int("HEADER", "VERSION")?;
        let brake_power = match car.brake_system.as_deref() {
            Some(brakes) => brakes.get_brake_power(),
            // (a chassis that is fed its brakes: the number its brake system would hold)
            None => IniReader::load(&data_path.join("brakes.ini"))?.get_float("DATA", "MAX_TORQUE")?,
        };
        let mut kers = Kers {
            attachment: KersAttachment::Engine,
            input: 0.0,
            brake_for_max_charge: (brake_power * ini.get_float("KERS", "BRAKE_LEVEL")?) * 2.0,
            charge: 0.0,
            charge_k: ini.get_float("KERS", "CHARGE_K")?,
            discharge_k: 0.0,
            negative_input_charge_k: 0.0,
            has_button_override: false,
            current_j: 0.0,
            max_j: 0.0,
            torque_lut: Curve::default(),
            controller: DynamicController::default(),
            has_controller: false,
        };
        let curve = ini.get_string("KERS", "TORQUE_CURVE");
        if !curve.is_empty() {
            kers.torque_lut.load(&data_path.join(&curve))?;
        }
        // milliseconds for a full battery at full input; a missing key divides by zero
        kers.discharge_k = 1000.0 / ini.get_float("KERS", "DISCHARGE_TIME")?;
        if version >= 2 {
            kers.negative_input_charge_k = ini.get_float("KERS", "NEGATIVE_INPUT_CHARGE_K")?;
        }
        let controller = ini.get_string("KERS", "CONTROLLER");
        if !controller.is_empty() {
            // a file that is not there gives a controller without stages (always 0), and the
            // car still "has a controller"
            kers.controller = DynamicController::load(&data_path.join(&controller))?;
            kers.has_controller = true;
        }
        // exact, case-sensitive; anything else stays "engine"
        match ini.get_string("KERS", "ATTACH").as_str() {
            "ENGINE" => kers.attachment = KersAttachment::Engine,
            "WHEELS" => kers.attachment = KersAttachment::Wheels,
            _ => {}
        }
        kers.charge = 1.0;
        if version >= 3 {
            kers.has_button_override = ini.get_int("KERS", "HAS_BUTTON_OVERRIDE")? != 0;
            kers.max_j = ini.get_float("KERS", "MAX_KJ_PER_LAP")? * 1000.0;
        }
        Ok(Some(kers))
    }

    /// `Kers::step` @ 0x1402b7e10, with the drivetrain in its slot (its speeds and gear are
    /// the last step's).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &RollingChassis, dt: f32) {
        let Some(drivetrain) = car.drivetrain.as_deref() else { return };
        let d = drivetrain.base();
        // in the pit box the battery is full
        if car.is_in_pits() {
            self.charge = 1.0;
        }

        // filling under braking: all four brakes, the speed of the left driven shaft alone
        let mut w = (d.out_shaft_l.velocity.abs() * 2.0) as f32;
        if !(w >= 6.0) {
            w = 0.0;
        }
        let mut sum = 0.0f32;
        for tyre in car.tyres.iter().take(4) {
            sum += tyre.abs_override * tyre.inputs.brake_torque;
        }
        let mut x = sum / self.brake_for_max_charge;
        x *= w;
        x *= 0.5;
        x *= self.charge_k;
        x *= dt;
        x += self.charge;
        self.charge = saturate(x);

        // what is asked for: the controller (never on the brakes), or the button
        if self.has_controller {
            let value = self.controller.eval(&CarSignals::of(car));
            self.input = saturate(value);
            if !(0.01f32 >= car.controls.brake) {
                self.input = 0.0;
            }
        } else {
            self.input = if car.controls.kers { 1.0 } else { 0.0 };
        }
        if self.has_button_override && car.controls.kers {
            self.input = 1.0;
        }

        // what forbids it before the battery is touched: empty, pit limiter, the lap's limit
        let charge = self.charge;
        let limiting = car.aids.as_deref().is_some_and(|aids| aids.base().speed_limiter.is_limiting);
        if !(charge > 0.0) || limiting {
            self.input = 0.0;
        } else {
            let most = self.max_j;
            if (most < 0.0 || most > 0.0) && !(most >= self.current_j) {
                self.input = 0.0;
            }
        }

        // the battery pays
        let input = self.input;
        if input >= 0.0 {
            let used = (input * self.discharge_k) * dt;
            self.charge = saturate(charge - used);
        } else if w > 0.0 {
            // (never reached: the input cannot be negative here)
            let used = (input * self.negative_input_charge_k) * dt;
            self.charge = saturate(charge - used);
        }

        // on the rev limiter, in neutral, in reverse and during a paddle shift the motor gives
        // nothing (the battery has paid all the same)
        let rpm = drivetrain.get_engine_rpm();
        let limit = drivetrain.engine().get_limiter_rpm() as f32;
        if rpm >= limit || d.current_gear <= 1 {
            self.input = 0.0;
        }

        // the lap's energy: single precision, the speed narrowed first
        if !(0.0 >= self.input) {
            let speed: f32 = match self.attachment {
                KersAttachment::Engine => d.engine.velocity as f32,
                KersAttachment::Wheels => ((d.out_shaft_r.velocity.abs() + d.out_shaft_l.velocity.abs()) * 0.5) as f32,
            };
            let rpm = (speed * 0.159_155_07) * 60.0;
            let torque = self.torque_lut.get_value(rpm);
            self.current_j = (((torque * self.input) * speed) * dt) + self.current_j;
        }
    }

    /// `Kers::getOutputTorque` @ 0x1402b72b0: the motor's torque at the shaft it is attached
    /// to, Nm. The speed is converted in double precision here (not as in [`Kers::step`]).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_output_torque(&self, d: &DrivetrainBase) -> f32 {
        if 0.0 >= self.charge {
            return 0.0;
        }
        let rpm = match self.attachment {
            KersAttachment::Engine => ((d.engine.velocity * K2PI) * 60.0) as f32,
            KersAttachment::Wheels => ((((d.out_shaft_r.velocity.abs() + d.out_shaft_l.velocity.abs()) * 0.5) * K2PI) * 60.0) as f32,
        };
        self.torque_lut.get_value(rpm) * self.input
    }

    /// `Kers::reset` @ 0x1402b7e00: a teleport, or the first crossing of the line in a hot-lap
    /// session. The input and the controller's smoothed values stay.
    pub fn reset(&mut self) {
        self.charge = 1.0;
        self.current_j = 0.0;
    }

    /// The handler on `Car::evOnLapCompleted` (0x1402b7270): a lap was counted.
    pub fn on_lap_completed(&mut self) {
        self.current_j = 0.0;
    }

    /// The values `tools/car_oracle` records of a KERS, under its names.
    pub fn trace(&self, out: &mut Vec<TraceValue>) {
        out.push(TraceValue::f("kers.input", self.input).extra());
        out.push(TraceValue::f("kers.charge", self.charge).extra());
        out.push(TraceValue::f("kers.currentJ", self.current_j).extra());
        for (k, stage) in self.controller.stages.iter().enumerate() {
            out.push(TraceValue::f(&format!("kers.controller.{k}.currentValue"), stage.current_value).extra());
        }
    }

    /// Everything that changes from step to step.
    pub fn save_state(&self, out: &mut Vec<u32>) {
        out.extend([self.input, self.charge, self.current_j].map(f32::to_bits));
        out.extend(self.controller.stages.iter().map(|stage| stage.current_value.to_bits()));
    }

    /// The inverse of [`Kers::save_state`].
    pub fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut next = || words.next().map(f32::from_bits).ok_or("the saved KERS state is too short".to_string());
        self.input = next()?;
        self.charge = next()?;
        self.current_j = next()?;
        for stage in &mut self.controller.stages {
            stage.current_value = next()?;
        }
        Ok(())
    }
}
