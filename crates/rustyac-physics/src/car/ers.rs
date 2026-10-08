// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `ERS` (`Car::ers`, ers.ini with `ctrl_ers_N.ini` and `ctrl_ers_front_N.ini`): the
//! hybrid system of a modern Grand Prix car or a hybrid road car.
//!
//! - **MGU-K**: an electric motor on the crankshaft. A delivery map (one of the car's
//!   `ctrl_ers_N.ini` controllers, chosen in the cockpit) says how much of it is used; the
//!   battery pays for it and a per-lap allowance limits it. Under braking and on the overrun
//!   it fills the battery again, as much as the cockpit's recovery level asks for, and drags
//!   on the engine while it does.
//! - **MGU-H**: the turbo's share. Either it fills the battery with the boost, or (the other
//!   cockpit mode, and always while the button is held on a car that has the override) it adds
//!   to the motor for free.
//! - **Front motors**: a second map and a second torque table that turn the two front wheels.
//!
//! `ERS::step` runs before the drivetrain in the car's step and settles the battery and the
//! inputs; the engine then asks the two getters while it steps.

use std::path::Path;

use super::chassis::RollingChassis;
use super::drivetrain::{DrivetrainBase, K2PI};
use super::dynamic_controller::{CarSignals, DynamicController};
use super::engine::saturate;
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::IniReader;

/// AC's `ERSPowerController` (0x48 bytes): one delivery map of the cockpit's list.
#[derive(Clone, Debug, PartialEq)]
pub struct ErsPowerController {
    /// `[HEADER] NAME` of its file
    pub name: String,
    /// The map. It is a template: never evaluated itself, copied into the car's active
    /// controller when chosen.
    pub ctrl: DynamicController,
}

/// AC's `ERS` (0x2b0 bytes) of a car that has one (`present`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ers {
    /// `kineticRecovery` (+0x14): the cockpit's MGU-K recovery level, 0..1 (0.5 at the start)
    pub kinetic_recovery: f32,
    /// `status.kineticRecovery`, `status.heatRecovery` (+0x18): charge per second from braking
    /// and from the turbo in the last step (for the displays)
    pub status_kinetic_recovery: f32,
    pub status_heat_recovery: f32,
    /// `isHeatCharginBattery` (+0x20): the cockpit's MGU-H mode: the battery (true) or the motor
    pub is_heat_charging_battery: bool,
    /// `ersPowerControllers`, `ersPowerControllersFront` (+0x28, +0x40)
    pub power_controllers: Vec<ErsPowerController>,
    pub power_controllers_front: Vec<ErsPowerController>,
    /// `defaultPowerControllerIndex` (+0x58)
    pub default_power_controller_index: i32,
    /// `isCharging` (+0x5c)
    pub is_charging: bool,
    /// `cockpitControls` (+0x5d): which of the three settings the cockpit offers
    pub cockpit_recovery: bool,
    pub cockpit_mgu_h_mode: bool,
    pub cockpit_delivery_profile: bool,
    /// `chargeK`, `dischargeK`, `dischargeKFront` (+0x68)
    pub charge_k: f64,
    pub discharge_k: f64,
    pub discharge_k_front: f64,
    /// `hasButtonOverride` (+0x80)
    pub has_button_override: bool,
    /// `torqueLUT`, `coastLUT` (+0x88, +0x108): engine rpm -> Nm
    pub torque_lut: Curve,
    pub coast_lut: Curve,
    /// `controller`, `controllerFront` (+0x188, +0x1b0): the delivery maps in use
    pub controller: DynamicController,
    pub controller_front: DynamicController,
    /// `charge` (+0x1d8), 0..1, a double
    pub charge: f64,
    /// `maxJ`, `currentJ` (+0x1e0): the lap's allowance and what was used of it, J
    pub max_j: f32,
    pub current_j: f32,
    /// `input` (+0x1e8): how much of the MGU-K is used, 0..1, the MGU-H's help included
    pub input: f32,
    /// `heatChargeK`, `heatTorque` (+0x1f0)
    pub heat_charge_k: f64,
    pub heat_torque: f32,
    /// `rearCorrectionTorque` (+0x1fc): rear brake torque given back at full recovery, Nm
    pub rear_correction_torque: f32,
    /// `frontTorqueLUT` (+0x228): front axle rpm -> Nm; empty for a car without front motors
    pub front_torque_lut: Curve,
    /// `frontTorqueVectoringBias` (+0x2a8)
    pub front_torque_vectoring_bias: f32,
    /// Not a member of the game's `ERS` (the game keeps it with the car's avatar,
    /// `currentERSPowerIndex`): the delivery map in use, -1 while none was ever chosen.
    pub power_controller_index: i32,
}

/// `ucomisd x, 0` + `jne`: not zero and a number.
fn ordered_nonzero(x: f64) -> bool {
    x < 0.0 || x > 0.0
}

impl Ers {
    /// `ERS::ERS` @ 0x140291500 and `ERS::init` @ 0x140291c30. `None`: the car has no
    /// ers.ini. A car with one must have at least one `ctrl_ers_N.ini` (the game stops
    /// without).
    pub fn load(data_path: &Path) -> Result<Option<Ers>, String> {
        let path = data_path.join("ers.ini");
        let ini = IniReader::load(&path)?;
        if !ini.ready {
            return Ok(None);
        }
        let mut ers = Ers {
            kinetic_recovery: 1.0,
            status_kinetic_recovery: 0.0,
            status_heat_recovery: 0.0,
            is_heat_charging_battery: true,
            power_controllers: Vec::new(),
            power_controllers_front: Vec::new(),
            default_power_controller_index: 0,
            is_charging: false,
            cockpit_recovery: true,
            cockpit_mgu_h_mode: true,
            cockpit_delivery_profile: true,
            // widened, then times the double 0.001
            charge_k: ini.get_float("KINETIC", "CHARGE_K")? as f64 * 0.001,
            discharge_k: 0.0,
            discharge_k_front: 0.0,
            has_button_override: false,
            torque_lut: ini.get_curve("KINETIC", "TORQUE_CURVE")?,
            coast_lut: ini.get_curve("KINETIC", "COAST_CURVE")?,
            controller: DynamicController::default(),
            controller_front: DynamicController::default(),
            charge: 1.0,
            max_j: 0.0,
            current_j: 0.0,
            input: 0.0,
            heat_charge_k: 0.0,
            heat_torque: 0.3,
            rear_correction_torque: 0.0,
            front_torque_lut: Curve::default(),
            front_torque_vectoring_bias: 1.0,
            power_controller_index: -1,
        };
        // milliseconds for a full battery: a float division, widened afterwards
        ers.discharge_k = (1000.0f32 / ini.get_float("KINETIC", "DISCHARGE_TIME")?) as f64;
        // a controller built from ers.ini itself (it has no stages in any car); the default
        // delivery map replaces it below unless its index is out of range
        ers.controller = DynamicController::load(&path)?;
        ers.has_button_override = ini.get_int("KINETIC", "HAS_BUTTON_OVERRIDE")? != 0;
        ers.max_j = ini.get_float("KINETIC", "MAX_KJ_PER_LAP")? * 1000.0;
        ers.heat_charge_k = ini.get_float("HEAT", "CHARGE_K")? as f64;
        ers.heat_torque = ini.get_float("HEAT", "TORQUE_PERC")? * 0.01;
        ers.default_power_controller_index = ini.get_int("KINETIC", "DEFAULT_CONTROLLER")?;
        ers.kinetic_recovery = 0.5;

        // ctrl_ers_0.ini, ctrl_ers_1.ini ... until one is missing; a front map is looked for
        // only next to a rear one
        for n in 0.. {
            let file = data_path.join(format!("ctrl_ers_{n}.ini"));
            if !crate::data::exists(&file) {
                break;
            }
            let ctrl = DynamicController::load(&file)?;
            let name = IniReader::load(&file)?.get_string("HEADER", "NAME");
            ers.power_controllers.push(ErsPowerController { name, ctrl });
            let front = data_path.join(format!("ctrl_ers_front_{n}.ini"));
            if crate::data::exists(&front) {
                let ctrl = DynamicController::load(&front)?;
                let name = IniReader::load(&front)?.get_string("HEADER", "NAME");
                ers.power_controllers_front.push(ErsPowerController { name, ctrl });
            }
        }
        if ers.power_controllers.is_empty() {
            return Err(format!("{}: no ctrl_ers_*.ini controllers found (the game stops with this error)", path.display()));
        }
        ers.set_power_controller(ers.default_power_controller_index);
        ers.rear_correction_torque = ini.get_float("KINETIC", "BRAKE_REAR_CORRECTION")?;

        if ini.has_section("COCKPIT_CONTROLS") {
            ers.cockpit_delivery_profile = ini.get_int("COCKPIT_CONTROLS", "DELIVERY_PROFILE")? != 0;
            ers.cockpit_mgu_h_mode = ini.get_int("COCKPIT_CONTROLS", "MGU_H_MODE")? != 0;
            ers.cockpit_recovery = ini.get_int("COCKPIT_CONTROLS", "RECOVERY")? != 0;
        }
        if ini.has_section("FRONT_MOTORS") {
            if ers.power_controllers_front.len() != ers.power_controllers.len() {
                return Err(format!(
                    "{}: an ERS with front motors needs as many ctrl_ers_front_N.ini as ctrl_ers_N.ini ({} and {}; the game stops with this error)",
                    path.display(),
                    ers.power_controllers_front.len(),
                    ers.power_controllers.len()
                ));
            }
            ers.front_torque_lut = ini.get_curve("FRONT_MOTORS", "TORQUE_CURVE")?;
            ers.discharge_k_front = (1000.0f32 / ini.get_float("FRONT_MOTORS", "DISCHARGE_TIME")?) as f64;
            // `saturate` @ 0x1400244b0: unlike the clamps of the step, a NaN stays
            let bias = ini.get_float("FRONT_MOTORS", "FRONT_TORQUE_VECTORING_BIAS")?;
            #[allow(clippy::manual_clamp)]
            let bias = if bias > 1.0 {
                1.0
            } else if 0.0 > bias {
                0.0
            } else {
                bias
            };
            ers.front_torque_vectoring_bias = bias;
        }
        Ok(Some(ers))
    }

    /// `ERS::setPowerController` @ 0x140292fc0: the cockpit's delivery profile. The copy
    /// brings the template's untouched stages, so the map's filters start again from 0 (also
    /// when the same index is chosen again). An index the car does not have changes nothing.
    pub fn set_power_controller(&mut self, index: i32) {
        if index < 0 || index as usize >= self.power_controllers.len() {
            return;
        }
        self.power_controller_index = index;
        self.controller = self.power_controllers[index as usize].ctrl.clone();
        if (index as usize) < self.power_controllers_front.len() {
            self.controller_front = self.power_controllers_front[index as usize].ctrl.clone();
        }
    }

    /// `ERS::step` @ 0x1402930e0, with the drivetrain in its slot: its speeds, gear and the
    /// engine's torque are the last step's; the brake torques are this step's.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        let Some(drivetrain) = car.drivetrain.as_deref() else { return };
        let d = drivetrain.base();
        let engine = drivetrain.engine().base();
        // in the pit box the battery is full
        if car.is_in_pits() {
            self.charge = 1.0;
        }

        // what can be harvested: the engine's drag of the last step and the brakes (of the
        // left rear and the right front wheel), seen at the crankshaft
        let mut coast = (-(engine.status.out_torque - engine.status.external_coast_torque)) as f32;
        if !(coast >= 0.0) {
            coast = 0.0;
        }
        let ratio = d.ratio;
        let brake: f32;
        if ordered_nonzero(ratio) {
            let (rear, front) = (&car.tyres[2], &car.tyres[1]);
            let sum = (rear.inputs.brake_torque.abs() * rear.abs_override) + (front.inputs.brake_torque.abs() * front.abs_override);
            brake = (sum as f64 / ratio) as f32;
        } else {
            // neutral: nothing is harvested
            brake = 0.0;
            coast = 0.0;
        }

        // the MGU-H of this step: to the battery, or to the motor
        let button = car.controls.kers;
        let heat_to_battery = self.is_heat_charging_battery && !(self.has_button_override && button);
        let torque = brake + coast;
        let max_boost = drivetrain.engine().get_max_turbo_boost();
        let boost = if max_boost > 0.0 { engine.status.turbo_boost / max_boost } else { 0.0 };

        // recovery, while the battery is not full
        let charge = self.charge;
        if !(charge >= 1.0) {
            let kinetic = ((((torque as f64) * self.charge_k) * d.root_velocity) * self.kinetic_recovery as f64) as f32;
            self.status_kinetic_recovery = kinetic;
            self.status_heat_recovery = if heat_to_battery { (boost as f64 * self.heat_charge_k) as f32 } else { 0.0 };
            let mut rate = kinetic + self.status_heat_recovery;
            if !(rate >= 0.0) {
                rate = 0.0;
            }
            let x = (rate as f64 * dt as f64) + charge;
            self.charge = if x > 1.0 {
                1.0
            } else if x >= 0.0 {
                x
            } else {
                0.0
            };
        } else {
            self.status_kinetic_recovery = 0.0;
            self.status_heat_recovery = 0.0;
        }
        self.is_charging = !(0.0 >= self.status_kinetic_recovery);

        // the delivery maps (each evaluated exactly once a step: their filters move)
        let signals = CarSignals::of(car);
        self.input = saturate(self.controller.eval(&signals));
        let mut input_front = saturate(self.controller_front.eval(&signals));

        // the MGU-H's help; the button gives everything
        let mut heat_assist = 0.0f32;
        if !heat_to_battery {
            heat_assist = (self.heat_torque * car.controls.gas) * boost;
        }
        if self.has_button_override && button {
            self.input = 1.0;
            input_front = 1.0;
            heat_assist = boost * self.heat_torque;
        }

        // nothing from an empty battery, on the pit limiter, or past the lap's allowance
        let limiting = car.aids.as_deref().is_some_and(|aids| aids.base().speed_limiter.is_limiting);
        if 0.0 >= self.charge || limiting || !(self.max_j >= self.current_j) {
            self.input = 0.0;
            input_front = 0.0;
        }
        // on the rev limiter, in neutral, in reverse: no MGU-K (the front motors go on)
        let rpm = drivetrain.get_engine_rpm();
        let limit = drivetrain.engine().get_limiter_rpm() as f32;
        if rpm >= limit || d.current_gear <= 1 {
            self.input = 0.0;
        }

        // the battery pays for the MGU-K's own share and for the front motors; when it
        // cannot, both inputs shrink to what is left
        let input = self.input;
        if input >= 0.0 {
            let own = saturate(input - heat_assist);
            let need = ((own as f64 * self.discharge_k) + (input_front as f64 * self.discharge_k_front)) * dt as f64;
            let charge = self.charge;
            if charge >= need {
                let left = charge - need;
                self.charge = if left > 1.0 {
                    1.0
                } else if left >= 0.0 {
                    left
                } else {
                    0.0
                };
            } else {
                let k = charge / need;
                self.charge = 0.0;
                self.input = (input as f64 * k) as f32;
                input_front = (input_front as f64 * k) as f32;
            }
        }

        // the input the engine sees (the MGU-H's help included), and the lap's energy (the
        // battery's share alone): single precision, the speed narrowed first
        let total = saturate(heat_assist + self.input);
        self.input = total;
        if total > 0.0 {
            let own = saturate(total - heat_assist);
            let speed = d.engine.velocity as f32;
            let table = self.torque_lut.get_value((speed * 0.159_155_07) * 60.0);
            self.current_j = (((table * own) * speed) * dt) + self.current_j;
        }

        // the rear brakes get this much less from the next step on (not rewritten in neutral)
        let rear_correction = ordered_nonzero(ratio).then_some(((self.rear_correction_torque * self.kinetic_recovery) as f64 * ratio) as f32);

        // the front motors: the map's torque split by the front wheels' loads
        let mut front_torques: Option<(f32, f32)> = None;
        if self.front_torque_lut.get_count() > 0 {
            let (left, right) = (&car.tyres[0], &car.tyres[1]);
            let speed = (right.status.angular_velocity + left.status.angular_velocity) * 0.5;
            let load = left.status.load + right.status.load;
            let rpm = (speed * 0.159_155_07) * 60.0;
            if load > 0.0 {
                let bias = saturate((((left.status.load / load) - 0.5) * self.front_torque_vectoring_bias) + 0.5);
                let torque = self.front_torque_lut.get_value(rpm) * input_front;
                self.current_j = ((torque * speed) * dt) + self.current_j;
                front_torques = Some((torque * bias, (1.0 - bias) * torque));
            } else {
                front_torques = Some((0.0, 0.0));
            }
        }

        if let (Some(torque), Some(brakes)) = (rear_correction, car.brake_system.as_mut()) {
            brakes.base_mut().rear_correction_torque = torque;
        }
        if let Some((left, right)) = front_torques {
            car.tyres[0].inputs.electric_torque = left;
            car.tyres[1].inputs.electric_torque = right;
        }
    }

    /// `ERS::getOutputTorque` @ 0x140291bd0: the MGU-K's torque on the crankshaft, Nm. The
    /// speed is converted in double precision here (not as in [`Ers::step`]).
    pub fn get_output_torque(&self, d: &DrivetrainBase) -> f32 {
        let rpm = ((d.engine.velocity * K2PI) * 60.0) as f32;
        self.torque_lut.get_value(rpm) * self.input
    }

    /// `ERS::getCoastTorque` @ 0x140291b40: the drag of the MGU-K while it harvests on the
    /// overrun (negative), Nm: only in gear and while the battery is not full; scaled by the
    /// clutch pedal and the recovery level.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_coast_torque(&self, d: &DrivetrainBase, clutch: f32) -> f32 {
        if !(1.0 > self.charge) || !ordered_nonzero(d.ratio) {
            return 0.0;
        }
        let rpm = ((d.engine.velocity * K2PI) * 60.0) as f32;
        -((self.coast_lut.get_value(rpm) * clutch) * self.kinetic_recovery)
    }

    /// `ERS::reset` @ 0x140292fa0: a teleport, or the first crossing of the line in a hot-lap
    /// session. Everything else (the input, the cockpit settings, the maps' filters) stays.
    pub fn reset(&mut self) {
        self.current_j = 0.0;
        self.charge = 1.0;
    }

    /// The handler on `Car::evOnLapCompleted` (0x140291980): a lap was counted.
    pub fn on_lap_completed(&mut self) {
        self.current_j = 0.0;
    }

    /// The values `tools/car_oracle` records of an ERS, under its names.
    pub fn trace(&self, out: &mut Vec<TraceValue>) {
        out.push(TraceValue::f("ers.kineticRecovery", self.kinetic_recovery).extra());
        out.push(TraceValue::f("ers.status.kineticRecovery", self.status_kinetic_recovery).extra());
        out.push(TraceValue::f("ers.status.heatRecovery", self.status_heat_recovery).extra());
        out.push(TraceValue::i("ers.isHeatCharginBattery", self.is_heat_charging_battery as i32).extra());
        out.push(TraceValue::i("ers.isCharging", self.is_charging as i32).extra());
        out.push(TraceValue::d("ers.charge", self.charge).extra());
        out.push(TraceValue::f("ers.currentJ", self.current_j).extra());
        out.push(TraceValue::f("ers.input", self.input).extra());
        for (name, controller) in [("controller", &self.controller), ("controllerFront", &self.controller_front)] {
            out.push(TraceValue::i(&format!("ers.{name}.stages"), controller.stages.len() as i32).extra());
            for n in 0..4 {
                let value = controller.stages.get(n).map(|stage| stage.current_value).unwrap_or(0.0);
                out.push(TraceValue::f(&format!("ers.{name}.{n}.currentValue"), value).extra());
            }
        }
    }

    /// Everything that changes from step to step.
    pub fn save_state(&self, out: &mut Vec<u32>) {
        out.extend([self.charge.to_bits() as u32, (self.charge.to_bits() >> 32) as u32]);
        out.extend([self.kinetic_recovery, self.status_kinetic_recovery, self.status_heat_recovery, self.current_j, self.input].map(f32::to_bits));
        out.extend([self.is_heat_charging_battery as u32, self.is_charging as u32, self.power_controller_index as u32]);
        for controller in [&self.controller, &self.controller_front] {
            out.push(controller.stages.len() as u32);
            out.extend(controller.stages.iter().map(|stage| stage.current_value.to_bits()));
        }
    }

    /// The inverse of [`Ers::save_state`].
    pub fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut next = || words.next().ok_or("the saved ERS state is too short".to_string());
        let low = next()? as u64;
        self.charge = f64::from_bits(low | (next()? as u64) << 32);
        self.kinetic_recovery = f32::from_bits(next()?);
        self.status_kinetic_recovery = f32::from_bits(next()?);
        self.status_heat_recovery = f32::from_bits(next()?);
        self.current_j = f32::from_bits(next()?);
        self.input = f32::from_bits(next()?);
        self.is_heat_charging_battery = next()? != 0;
        self.is_charging = next()? != 0;
        let index = next()? as i32;
        if index != self.power_controller_index {
            self.set_power_controller(index);
        }
        for controller in [&mut self.controller, &mut self.controller_front] {
            let count = next()? as usize;
            if count != controller.stages.len() {
                return Err(format!("the saved ERS state has a delivery map of {count} stages, the car's has {}", controller.stages.len()));
            }
            for stage in &mut controller.stages {
                stage.current_value = f32::from_bits(next()?);
            }
        }
        Ok(())
    }
}
