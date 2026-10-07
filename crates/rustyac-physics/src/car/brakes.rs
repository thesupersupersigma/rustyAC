//! The brake slot ([`BrakeModel`]) and AC's `BrakeSystem` as its Vanilla implementation
//! ([`VanillaBrakes`]): pedal to brake torque per wheel, front / rear bias (the setup's, the
//! cockpit's, or an electronic one), handbrake, steer-brake and disc temperatures with fade.
//!
//! The brakes do not act on a rigid body. Their whole output is `Tyre::inputs.brakeTorque` and
//! `inputs.handBrakeTorque` of the four tyres, positive numbers in Nm; the tyre applies them
//! against the wheel's rotation.

use std::path::Path;

use super::chassis::RollingChassis;
use super::dynamic_controller::{CarSignals, DynamicController};
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::IniReader;

/// The members of AC's `BrakeSystem` that other systems write: the setup items (`frontBias`,
/// `brakePowerMultiplier`), the speed limiter (`electronicOverride`), the hybrid system
/// (`rearCorrectionTorque`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrakeBase {
    /// `frontBias` (+0x00): share of the torque that goes to the front wheels, 0..1
    pub front_bias: f32,
    /// `brakePowerMultiplier` (+0x04)
    pub brake_power_multiplier: f32,
    /// `electronicOverride` (+0x08): a pedal value an aid asks for; the larger of it and the
    /// driver's pedal is used, and it is cleared at the end of every step
    pub electronic_override: f32,
    /// `rearCorrectionTorque` (+0x260): taken off each rear wheel's torque, Nm
    pub rear_correction_torque: f32,
}

/// The brake slot: AC's `BrakeSystem` as the rest of the car sees it.
pub trait BrakeModel {
    /// `BrakeSystem::step` @ 0x14028e640: writes the brake and handbrake torques into the four
    /// tyres. First component of `Car::stepComponents`.
    fn step(&mut self, car: &mut RollingChassis, dt: f32);
    /// `BrakeSystem::reset` @ 0x14028e490 (`Car::forcePosition`): the cockpit bias is
    /// forgotten, the discs take the air temperature.
    fn reset(&mut self, ambient_temperature: f32);
    /// `BrakeSystem::getFrontBias` @ 0x14028d630: the cockpit bias if one is set, else the
    /// setup's.
    fn get_front_bias(&self) -> f32;
    /// `BrakeSystem::getBrakePower` @ 0x14028d620
    fn get_brake_power(&self) -> f32;
    /// `BrakeSystem::isUsingEBB` @ 0x1402befa0
    fn is_using_ebb(&self) -> bool;
    /// `BrakeSystem::setManualFrontBias` @ 0x14028e5d0: `clicks` steps of the cockpit control.
    fn set_manual_front_bias(&mut self, clicks: i32);
    /// `discs[i].t`: the disc temperatures in tyre order, deg C (the telemetry shows them).
    fn disc_temperatures(&self) -> [f32; 4];
    fn base(&self) -> &BrakeBase;
    fn base_mut(&mut self) -> &mut BrakeBase;
    /// The values `tools/car_oracle` records of the brakes, under its names.
    fn trace(&self, out: &mut Vec<TraceValue>);
    /// Everything that changes from step to step, for a test that starts in mid-run.
    fn save_state(&self, out: &mut Vec<u32>);
    /// The inverse of [`BrakeModel::save_state`].
    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String>;
}

/// AC's `EBBMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum EbbMode {
    #[default]
    Disabled = 0,
    /// brakes.ini has an `[EBB]` section: bias from the front axle's share of the load
    Internal = 1,
    /// the car has a `ctrl_ebb.ini`
    DynamicController = 2,
}

/// AC's `BrakeDisc` (0x90 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeDisc {
    /// `t`: temperature, deg C
    pub t: f32,
    /// `coolTransfer`: cooling rate at standstill, 1/s
    pub cool_transfer: f32,
    /// `torqueK`: deg C per kJ of brake work
    pub torque_k: f32,
    /// `coolSpeedFactor`: extra cooling per km/h
    pub cool_speed_factor: f32,
    /// `perfCurve`: temperature -> torque multiplier
    pub perf_curve: Curve,
}

impl Default for BrakeDisc {
    /// `BrakeDisc::BrakeDisc` @ 0x14026bde0.
    fn default() -> BrakeDisc {
        BrakeDisc { t: 0.0, cool_transfer: 0.005, torque_k: 0.1, cool_speed_factor: 0.001, perf_curve: Curve::new() }
    }
}

/// AC's `SteerBrake` (0x30 bytes): a controller whose output is extra torque on one rear wheel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SteerBrake {
    /// `isActive`
    pub is_active: bool,
    /// `controller` (`steer_brake_controller.ini`)
    pub controller: DynamicController,
}

/// AC's `BrakeSystem` (0x3f0 bytes), without the developer's temperature log (`tempRunFile`).
#[derive(Clone, Debug, PartialEq)]
pub struct VanillaBrakes {
    pub base: BrakeBase,
    /// `handBrakeTorque` (+0x0c): Nm per rear wheel at full handbrake
    pub hand_brake_torque: f32,
    /// `ebbInstant` (+0x10): the bias the internal EBB computed last
    pub ebb_instant: f32,
    /// `discs` (+0x18), in tyre order
    pub discs: [BrakeDisc; 4],
    /// `limitDown` (+0x258), `limitUp` (+0x25c): the range every bias is clamped to
    pub limit_down: f32,
    pub limit_up: f32,
    /// `brakePower` (+0x270): Nm at full pedal for one front plus one rear wheel
    pub brake_power: f32,
    /// `biasOverride` (+0x274): the cockpit bias, -1 = none
    pub bias_override: f32,
    /// `hasCockpitBias` (+0x278)
    pub has_cockpit_bias: bool,
    /// `biasStep` (+0x27c): bias per cockpit click
    pub bias_step: f32,
    /// `ebbMode` (+0x280)
    pub ebb_mode: EbbMode,
    /// `ebbFrontMultiplier` (+0x284)
    pub ebb_front_multiplier: f32,
    /// `steerBrake` (+0x288)
    pub steer_brake: SteerBrake,
    /// `hasBrakeTempsData` (+0x2b8)
    pub has_brake_temps_data: bool,
    /// `ebbController` (+0x3c8, `ctrl_ebb.ini`)
    pub ebb_controller: DynamicController,
}

impl Default for VanillaBrakes {
    /// `BrakeSystem::BrakeSystem` @ 0x14026be20.
    fn default() -> VanillaBrakes {
        VanillaBrakes {
            base: BrakeBase {
                front_bias: 0.7,
                brake_power_multiplier: 1.0,
                electronic_override: 1.0,
                rear_correction_torque: 0.0,
            },
            hand_brake_torque: 0.0,
            ebb_instant: 0.5,
            discs: Default::default(),
            limit_down: 0.0,
            limit_up: 1.0,
            // not written by the constructor; `init` sets 2000 before the file is read
            brake_power: 0.0,
            bias_override: -1.0,
            has_cockpit_bias: true,
            bias_step: 0.005,
            ebb_mode: EbbMode::Disabled,
            ebb_front_multiplier: 1.1,
            steer_brake: SteerBrake::default(),
            has_brake_temps_data: false,
            ebb_controller: DynamicController::default(),
        }
    }
}

/// `ucomiss a, b` + `jne`: different and both numbers.
fn ordered_ne(a: f32, b: f32) -> bool {
    a < b || a > b
}

impl VanillaBrakes {
    /// `BrakeSystem::init` @ 0x14028d690 with `BrakeSystem::loadINI` @ 0x14028d870, for the
    /// car whose data folder is `data_path`.
    pub fn new(data_path: &Path) -> Result<VanillaBrakes, String> {
        let mut brakes = VanillaBrakes::default();
        brakes.brake_power = 2000.0;
        brakes.base.front_bias = 0.7;
        brakes.base.brake_power_multiplier = 1.0;
        brakes.base.electronic_override = 0.0;
        brakes.load_ini(data_path)?;
        let ebb = data_path.join("ctrl_ebb.ini");
        if ebb.is_file() {
            brakes.ebb_controller = DynamicController::load(&ebb)?;
            brakes.ebb_mode = EbbMode::DynamicController;
        }
        Ok(brakes)
    }

    /// `BrakeSystem::loadINI` @ 0x14028d870. A missing key reads as 0 and is stored as that.
    fn load_ini(&mut self, data_path: &Path) -> Result<(), String> {
        let ini = IniReader::load(&data_path.join("brakes.ini"))?;
        self.brake_power = ini.get_float("DATA", "MAX_TORQUE")?;
        self.base.front_bias = ini.get_float("DATA", "FRONT_SHARE")?;
        self.hand_brake_torque = ini.get_float("DATA", "HANDBRAKE_TORQUE")?;
        self.has_cockpit_bias = ini.get_int("DATA", "COCKPIT_ADJUSTABLE")? != 0;
        self.bias_step = ini.get_float("DATA", "ADJUST_STEP")? * 0.01;
        if ini.has_section("EBB") {
            self.ebb_mode = EbbMode::Internal;
            let multiplier = ini.get_float("EBB", "FRONT_SHARE_MULTIPLIER")?;
            // `comiss` + `jb`: anything below 1.1 (or not a number) is 1.1
            self.ebb_front_multiplier = if multiplier >= 1.1 { multiplier } else { 1.1 };
        }
        let steer_brake = data_path.join("steer_brake_controller.ini");
        if steer_brake.is_file() {
            self.steer_brake.is_active = true;
            self.steer_brake.controller = DynamicController::load(&steer_brake)?;
        }
        // the flag is written for the front section and then again for the rear one, so only
        // `[TEMPS_REAR]` decides it
        for (axle, section) in ["TEMPS_FRONT", "TEMPS_REAR"].into_iter().enumerate() {
            if ini.has_section(section) {
                for disc in &mut self.discs[2 * axle..2 * axle + 2] {
                    disc.torque_k = ini.get_float(section, "TORQUE_K")?;
                    disc.perf_curve = ini.get_curve(section, "PERF_CURVE")?;
                    disc.cool_transfer = ini.get_float(section, "COOL_TRANSFER")?;
                    disc.cool_speed_factor = ini.get_float(section, "COOL_SPEED_FACTOR")?;
                }
                self.has_brake_temps_data = true;
            } else {
                self.has_brake_temps_data = false;
            }
        }
        let setup = IniReader::load(&data_path.join("setup.ini"))?;
        if setup.ready && setup.has_section("FRONT_BIAS") {
            self.limit_down = setup.get_float("FRONT_BIAS", "MIN")? * 0.01;
            self.limit_up = setup.get_float("FRONT_BIAS", "MAX")? * 0.01;
        }
        Ok(())
    }

    /// `BrakeSystem::stepTemps` @ 0x14028e920: fade from the disc temperature of before this
    /// step (written back into the tyre's brake torque), then cooling and heating.
    fn step_temps(&mut self, car: &mut RollingChassis, dt: f32) {
        for (disc, tyre) in self.discs.iter_mut().zip(car.tyres.iter_mut()) {
            let torque = disc.perf_curve.get_value(disc.t) * tyre.inputs.brake_torque;
            tyre.inputs.brake_torque = torque;
            let cool = ((car.speed * 3.6) * disc.cool_speed_factor + 1.0) * disc.cool_transfer;
            let cooled = ((car.env.ambient_temperature - disc.t) * cool) * dt + disc.t;
            disc.t = cooled;
            let heat = tyre.inputs.brake_torque * disc.torque_k;
            disc.t = ((tyre.status.angular_velocity.abs() * heat) * 0.001) * dt + cooled;
        }
    }
}

impl BrakeModel for VanillaBrakes {
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        let mut bias = self.base.front_bias;
        if ordered_ne(self.bias_override, -1.0) {
            bias = self.bias_override;
        }
        match self.ebb_mode {
            EbbMode::Internal => {
                let load = |wheel: usize| car.tyres[wheel].status.load;
                let front = load(1) + load(0);
                let total = (load(3) + load(2)) + front;
                // `ucomiss total, 0` + `je`, then `comiss kmh, 10` + `jbe`
                if ordered_ne(total, 0.0) && car.speed * 3.6 > 10.0 {
                    let mut share = front / total * self.ebb_front_multiplier;
                    if share > 1.0 {
                        share = 1.0;
                    } else if !(share >= 0.0) {
                        share = 0.0;
                    }
                    self.ebb_instant = share;
                } else {
                    // the setup's bias, not the cockpit's
                    self.ebb_instant = self.base.front_bias;
                }
                bias = self.ebb_instant;
            }
            EbbMode::DynamicController => bias = self.ebb_controller.eval(&CarSignals::of(car)),
            EbbMode::Disabled => {}
        }
        if bias > self.limit_up {
            bias = self.limit_up;
        } else if !(bias >= self.limit_down) {
            bias = self.limit_down;
        }

        let mut pedal = self.base.electronic_override;
        if !(pedal > car.controls.brake) {
            pedal = car.controls.brake;
        }
        let torque = (self.brake_power * self.base.brake_power_multiplier) * pedal;
        let front = torque * bias;
        car.tyres[0].inputs.brake_torque = front;
        car.tyres[1].inputs.brake_torque = front;
        let mut rear = (1.0 - bias) * torque - self.base.rear_correction_torque;
        if !(rear >= 0.0) {
            rear = 0.0;
        }
        car.tyres[2].inputs.brake_torque = rear;
        car.tyres[3].inputs.brake_torque = rear;
        // the front wheels' handbrake torque is never written (0 from the tyre's constructor)
        car.tyres[2].inputs.hand_brake_torque = car.controls.hand_brake * self.hand_brake_torque;
        car.tyres[3].inputs.hand_brake_torque = car.controls.hand_brake * self.hand_brake_torque;

        if self.steer_brake.is_active {
            let extra = self.steer_brake.controller.eval(&CarSignals::of(car));
            if extra >= 0.0 {
                car.tyres[3].inputs.brake_torque = extra + car.tyres[3].inputs.brake_torque;
            } else {
                car.tyres[2].inputs.brake_torque = car.tyres[2].inputs.brake_torque - extra;
            }
        }
        // an AI driver sets `aiMult` above 1: its discs then keep their temperature
        if self.has_brake_temps_data && 1.0 >= car.tyres[0].ai_mult {
            self.step_temps(car, dt);
        }
        self.base.electronic_override = 0.0;
    }

    fn reset(&mut self, ambient_temperature: f32) {
        self.bias_override = -1.0;
        for disc in &mut self.discs {
            disc.t = ambient_temperature;
        }
    }

    fn get_front_bias(&self) -> f32 {
        if ordered_ne(self.bias_override, -1.0) {
            self.bias_override
        } else {
            self.base.front_bias
        }
    }

    fn get_brake_power(&self) -> f32 {
        self.brake_power
    }

    fn is_using_ebb(&self) -> bool {
        self.ebb_mode != EbbMode::Disabled
    }

    fn set_manual_front_bias(&mut self, clicks: i32) {
        if !self.has_cockpit_bias {
            return;
        }
        if !ordered_ne(-1.0, self.bias_override) {
            self.bias_override = self.base.front_bias;
        }
        let bias = clicks as f32 * self.bias_step + self.bias_override;
        self.bias_override = if bias > self.limit_up {
            self.limit_up
        } else if bias >= self.limit_down {
            bias
        } else {
            self.limit_down
        };
    }

    fn disc_temperatures(&self) -> [f32; 4] {
        [self.discs[0].t, self.discs[1].t, self.discs[2].t, self.discs[3].t]
    }

    fn base(&self) -> &BrakeBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BrakeBase {
        &mut self.base
    }

    fn trace(&self, out: &mut Vec<TraceValue>) {
        out.push(TraceValue::f("brakes.frontBias", self.base.front_bias));
        out.push(TraceValue::f("brakes.brakePowerMultiplier", self.base.brake_power_multiplier));
        out.push(TraceValue::f("brakes.electronicOverride", self.base.electronic_override));
        out.push(TraceValue::f("brakes.brakePower", self.brake_power));
        for (disc, wheel) in self.discs.iter().zip(super::replay::WHEELS) {
            out.push(TraceValue::f(&format!("brakes.disc.{wheel}.t"), disc.t));
        }
        out.push(TraceValue::f("brakes.biasOverride", self.bias_override).extra());
        out.push(TraceValue::f("brakes.ebbInstant", self.ebb_instant).extra());
        out.push(TraceValue::f("brakes.rearCorrectionTorque", self.base.rear_correction_torque).extra());
    }

    fn save_state(&self, out: &mut Vec<u32>) {
        let b = &self.base;
        out.extend(
            [b.front_bias, b.brake_power_multiplier, b.electronic_override, b.rear_correction_torque, self.ebb_instant, self.bias_override]
                .map(f32::to_bits),
        );
        out.extend(self.discs.iter().map(|disc| disc.t.to_bits()));
        for controller in [&self.ebb_controller, &self.steer_brake.controller] {
            out.extend(controller.stages.iter().map(|stage| stage.current_value.to_bits()));
        }
    }

    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut f = || words.next().map(f32::from_bits).ok_or("the saved brake state is too short".to_string());
        self.base.front_bias = f()?;
        self.base.brake_power_multiplier = f()?;
        self.base.electronic_override = f()?;
        self.base.rear_correction_torque = f()?;
        self.ebb_instant = f()?;
        self.bias_override = f()?;
        for disc in &mut self.discs {
            disc.t = f()?;
        }
        for controller in [&mut self.ebb_controller, &mut self.steer_brake.controller] {
            for stage in &mut controller.stages {
                stage.current_value = f()?;
            }
        }
        Ok(())
    }
}
