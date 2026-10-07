// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `DynamicController`: a small programmable formula loaded from a `ctrl_*.ini` file. A
//! list of stages, each "take one signal of the car, pass it through a look-up table, smooth
//! it, add it to or multiply it into the running result, clamp". The brakes use it for the
//! electronic brake balance and the steer-brake, the drivetrain for the differential lock,
//! the engine for its turbos, the anti-roll bars for their rate (and, not ported yet, rear
//! steering, KERS, ERS).

use std::path::Path;

use super::chassis::RollingChassis;
use super::drivetrain::TractionType;
use crate::curve::Curve;
use crate::data::ini::IniReader;

/// AC's `DynamicControllerInput`; the numbers are the game's (the switch of
/// `DynamicController::getInput`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum DynamicControllerInput {
    #[default]
    Undefined = 0,
    Brake = 1,
    Gas = 2,
    LatG = 3,
    LonG = 4,
    Steer = 5,
    SpeedKmh = 6,
    Gear = 7,
    SlipRatioMax = 8,
    SlipRatioAvg = 9,
    SlipAngleFrontAvg = 10,
    SlipAngleRearAvg = 11,
    SlipAngleFrontMax = 12,
    SlipAngleRearMax = 13,
    OversteerFactor = 14,
    RearSpeedRatio = 15,
    SteerDeg = 16,
    Const = 17,
    Rpms = 18,
    WheelSteerDeg = 19,
    LoadSpreadLf = 20,
    LoadSpreadRf = 21,
    AvgTravelRear = 22,
    SusTravelLr = 23,
    SusTravelRr = 24,
}

impl DynamicControllerInput {
    /// The `INPUT=` names of the constructor's map.
    pub fn from_name(name: &str) -> Option<DynamicControllerInput> {
        use DynamicControllerInput::*;
        Some(match name {
            "BRAKE" => Brake,
            "GAS" => Gas,
            "STEER" => Steer,
            "LATG" => LatG,
            "LONG" => LonG,
            "SPEED_KMH" => SpeedKmh,
            "GEAR" => Gear,
            "SLIPRATIO_MAX" => SlipRatioMax,
            "SLIPRATIO_AVG" => SlipRatioAvg,
            "SLIPANGLE_FRONT_AVG" => SlipAngleFrontAvg,
            "SLIPANGLE_FRONT_MAX" => SlipAngleFrontMax,
            "SLIPANGLE_REAR_AVG" => SlipAngleRearAvg,
            "SLIPANGLE_REAR_MAX" => SlipAngleRearMax,
            "OVERSTEER_FACTOR" => OversteerFactor,
            "REAR_SPEED_RATIO" => RearSpeedRatio,
            "STEER_DEG" => SteerDeg,
            "CONST" => Const,
            "RPMS" => Rpms,
            "WHEEL_STEER_DEG" => WheelSteerDeg,
            "LOAD_SPREAD_LF" => LoadSpreadLf,
            "LOAD_SPREAD_RF" => LoadSpreadRf,
            "AVG_TRAVEL_REAR" => AvgTravelRear,
            "SUS_TRAVEL_LR" => SusTravelLr,
            "SUS_TRAVEL_RR" => SusTravelRr,
            _ => return None,
        })
    }
}

/// AC's `DynamicControllerCombinatorMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum DynamicControllerCombinatorMode {
    /// `eUndefinedMode`: the running result becomes 0.
    #[default]
    Undefined = 0,
    /// `eAdd`
    Add = 1,
    /// `eMult`
    Mult = 2,
}

/// AC's `DynamicControllerStage` (0xa0 bytes). The default is the constructor's
/// (0x1402b0780): everything zero, an empty table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicControllerStage {
    /// `inputVar`
    pub input_var: DynamicControllerInput,
    /// `combinatorMode`
    pub combinator_mode: DynamicControllerCombinatorMode,
    /// `lut`
    pub lut: Curve,
    /// `filter`: `lagToLerpDeltaK(FILTER, 0.004, 0.003)`, a rate per second
    pub filter: f32,
    /// `upLimit`
    pub up_limit: f32,
    /// `downLimit`
    pub down_limit: f32,
    /// `currentValue`: the smoothed value of this stage; advances once per `eval`
    pub current_value: f32,
    /// `constValue`
    pub const_value: f32,
}

/// What `DynamicController::getInput` reads through its `Car` pointer: the chassis, and three
/// values of the drivetrain (which is not in its slot of the chassis while it steps).
#[derive(Clone, Copy)]
pub struct CarSignals<'a> {
    pub chassis: &'a RollingChassis,
    /// `car->drivetrain.tractionType`
    pub traction_type: TractionType,
    /// `car->drivetrain.currentGear` (0 reverse, 1 neutral, 2 first ...)
    pub current_gear: i32,
    /// `Drivetrain::getEngineRPM()`
    pub engine_rpm: f32,
}

impl<'a> CarSignals<'a> {
    /// The signals of a chassis whose drivetrain sits in its slot. A chassis without a
    /// drivetrain reads as rear-wheel drive, in neutral, engine stopped.
    pub fn of(chassis: &'a RollingChassis) -> CarSignals<'a> {
        match &chassis.drivetrain {
            Some(drivetrain) => CarSignals {
                chassis,
                traction_type: drivetrain.base().traction_type,
                current_gear: drivetrain.base().current_gear,
                engine_rpm: drivetrain.get_engine_rpm(),
            },
            None => CarSignals { chassis, traction_type: TractionType::Rwd, current_gear: 1, engine_rpm: 0.0 },
        }
    }
}

/// `lagToLerpDeltaK` @ 0x14005d7c0: `((1 / b) * a) * (1 - lag)) * (1 / b)`, all single
/// precision, one division.
pub fn lag_to_lerp_delta_k(lag: f32, a: f32, b: f32) -> f32 {
    let one_minus = 1.0 - lag;
    let inverse = 1.0 / b;
    ((inverse * a) * one_minus) * inverse
}

/// AC's `DynamicController` (0x28 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicController {
    /// `stages`, in file order
    pub stages: Vec<DynamicControllerStage>,
    /// `ready`: the file could be opened. Only the anti-roll bar tests it; every other user
    /// evaluates its controller regardless (one without stages gives 0).
    pub ready: bool,
}

impl DynamicController {
    /// `DynamicController::DynamicController(Car*, const std::wstring&)` @ 0x1402af330:
    /// sections `[CONTROLLER_0]`, `[CONTROLLER_1]`, ... up to the first index without a
    /// section. A file that cannot be opened gives a controller without stages (it then
    /// evaluates to 0).
    ///
    /// The game goes on after an unknown `INPUT` or `COMBINATOR` (the stage keeps the
    /// "undefined" value 0) unless its debug switch `INIReader::crashAtError` is set; so does
    /// this.
    pub fn load(path: &Path) -> Result<DynamicController, String> {
        let mut controller = DynamicController::default();
        if !path.is_file() {
            return Ok(controller);
        }
        let ini = IniReader::load(path)?;
        if !ini.ready {
            return Ok(controller);
        }
        let mut index = 0;
        loop {
            let section = format!("CONTROLLER_{index}");
            if !ini.has_section(&section) {
                break;
            }
            let mut stage = DynamicControllerStage::default();
            let input = ini.get_string(&section, "INPUT");
            if let Some(input_var) = DynamicControllerInput::from_name(&input) {
                stage.input_var = input_var;
            }
            stage.combinator_mode = match ini.get_string(&section, "COMBINATOR").as_str() {
                "ADD" => DynamicControllerCombinatorMode::Add,
                "MULT" => DynamicControllerCombinatorMode::Mult,
                _ => DynamicControllerCombinatorMode::Undefined,
            };
            if stage.input_var == DynamicControllerInput::Const {
                stage.const_value = ini.get_float(&section, "CONST_VALUE")?;
            } else {
                stage.lut = ini.get_curve(&section, "LUT")?;
            }
            stage.filter = lag_to_lerp_delta_k(ini.get_float(&section, "FILTER")?, 0.004, 0.003);
            stage.up_limit = ini.get_float(&section, "UP_LIMIT")?;
            stage.down_limit = ini.get_float(&section, "DOWN_LIMIT")?;
            controller.stages.push(stage);
            index += 1;
        }
        controller.ready = true;
        Ok(controller)
    }

    /// `DynamicController::eval` @ 0x1402b0c00.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn eval(&mut self, car: &CarSignals) -> f32 {
        let mut out = 0.0f32;
        for stage in &mut self.stages {
            let mut value = if stage.input_var == DynamicControllerInput::Const {
                stage.const_value
            } else {
                stage.lut.get_value(Self::get_input(stage.input_var, car))
            };
            let previous = stage.current_value;
            // `comiss |value - previous|, 0.001` + `jb`: smoothed only when the step is not
            // below 0.001 (a NaN is not smoothed)
            if (value - previous).abs() >= 0.001 {
                // 0.003 is a literal here, not the step length
                let mut k = stage.filter * 0.003;
                if k > 1.0 {
                    k = 1.0;
                } else if !(k >= 0.0) {
                    k = 0.0;
                }
                value = (value - previous) * k + previous;
            }
            stage.current_value = value;
            match stage.combinator_mode {
                DynamicControllerCombinatorMode::Undefined => out = 0.0,
                DynamicControllerCombinatorMode::Add => out += stage.current_value,
                DynamicControllerCombinatorMode::Mult => out *= value,
            }
            // two `ucomiss`: no clamp when both limits are exactly 0 (or not a number)
            let (up, down) = (stage.up_limit, stage.down_limit);
            if (down < 0.0 || down > 0.0) || (0.0 < up || 0.0 > up) {
                if out > up {
                    out = up;
                } else if !(out >= down) {
                    out = down;
                }
            }
        }
        out
    }

    /// `DynamicController::getInput` @ 0x1402b0d70.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_input(input: DynamicControllerInput, car: &CarSignals) -> f32 {
        use DynamicControllerInput::*;
        let c = car.chassis;
        let slip_ratio = |wheel: usize| c.tyres[wheel].status.slip_ratio;
        let slip_angle = |wheel: usize| c.tyres[wheel].status.slip_angle_rad;
        let load = |wheel: usize| c.tyres[wheel].status.load;
        let travel = |wheel: usize| c.suspensions[wheel].get_status().travel;
        let larger = |a: f32, b: f32| if a > b { a } else { b };
        match input {
            Undefined | Const => 0.0,
            Brake => c.controls.brake,
            Gas => c.controls.gas,
            LatG => c.acc_g.x,
            LonG => c.acc_g.z,
            Steer => c.controls.steer,
            SpeedKmh => c.speed * 3.6,
            // `dec` + `cvtdq2ps`
            Gear => car.current_gear.wrapping_sub(1) as f32,
            SlipRatioMax => match car.traction_type {
                TractionType::Rwd => larger(slip_ratio(2), slip_ratio(3)),
                TractionType::Fwd => larger(slip_ratio(0), slip_ratio(1)),
                TractionType::Awd => {
                    let rear = larger(slip_ratio(2), slip_ratio(3));
                    let front = larger(slip_ratio(0), slip_ratio(1));
                    if rear > front {
                        larger(slip_ratio(2), slip_ratio(3))
                    } else {
                        larger(slip_ratio(0), slip_ratio(1))
                    }
                }
                TractionType::AwdNew => 0.0,
            },
            SlipRatioAvg => match car.traction_type {
                TractionType::Rwd => (slip_ratio(3) + slip_ratio(2)) * 0.5,
                TractionType::Fwd => (slip_ratio(1) + slip_ratio(0)) * 0.5,
                TractionType::Awd => (((slip_ratio(1) + slip_ratio(0)) + slip_ratio(2)) + slip_ratio(3)) * 0.25,
                TractionType::AwdNew => 0.0,
            },
            SlipAngleFrontAvg => ((slip_angle(1) + slip_angle(0)) * 57.29578) * 0.5,
            SlipAngleRearAvg => ((slip_angle(3) + slip_angle(2)) * 57.29578) * 0.5,
            SlipAngleFrontMax => larger(slip_angle(0).abs(), slip_angle(1).abs()) * 57.29578,
            SlipAngleRearMax => larger(slip_angle(2).abs(), slip_angle(3).abs()) * 57.29578,
            OversteerFactor => Self::get_oversteer_factor(c),
            RearSpeedRatio => Self::get_rear_speed_ratio(c),
            SteerDeg => c.steer_lock * c.controls.steer,
            Rpms => car.engine_rpm,
            WheelSteerDeg => c.final_steer_angle_signal,
            LoadSpreadLf => {
                let lf = load(0);
                lf / (lf + load(1))
            }
            LoadSpreadRf => {
                let rf = load(1);
                rf / (rf + load(0))
            }
            // `ISuspension::getStatus()` (+0x48) of the right rear first, mm
            AvgTravelRear => ((travel(3) + travel(2)) * 0.5) * 1000.0,
            SusTravelLr => travel(2) * 1000.0,
            SusTravelRr => travel(3) * 1000.0,
        }
    }

    /// `DynamicController::getOversteerFactor` @ 0x1402b11d0: mean rear slip angle minus mean
    /// front slip angle (magnitudes), degrees.
    pub fn get_oversteer_factor(car: &RollingChassis) -> f32 {
        let slip_angle = |wheel: usize| car.tyres[wheel].status.slip_angle_rad.abs();
        let rear = (slip_angle(3) + slip_angle(2)) * 0.5;
        let front = (slip_angle(0) + slip_angle(1)) * 0.5;
        (rear - front) * 57.29578
    }

    /// `DynamicController::getRearSpeedRatio` @ 0x1402b1230: mean rear wheel speed over mean
    /// front wheel speed, 0 while the front wheels stand.
    pub fn get_rear_speed_ratio(car: &RollingChassis) -> f32 {
        let speed = |wheel: usize| car.tyres[wheel].status.angular_velocity;
        let front = (speed(1) + speed(0)) * 0.5;
        // `ucomiss` + `je`: 0 for a zero or a NaN
        if !(front < 0.0 || front > 0.0) {
            return 0.0;
        }
        (speed(3) + speed(2)) * 0.5 / front
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_rate() {
        // FILTER=0.95: 6.67 % of the way per step; FILTER=0: the table is followed at once
        let slow = lag_to_lerp_delta_k(0.95, 0.004, 0.003);
        assert!((slow * 0.003 - 0.066_666).abs() < 1e-5, "{slow}");
        let fast = lag_to_lerp_delta_k(0.0, 0.004, 0.003);
        assert!(fast * 0.003 > 1.0);
        assert_eq!(fast.to_bits(), (((1.0f32 / 0.003) * 0.004) * 1.0 * (1.0f32 / 0.003)).to_bits());
    }

    #[test]
    fn input_names() {
        assert_eq!(DynamicControllerInput::from_name("CONST"), Some(DynamicControllerInput::Const));
        assert_eq!(DynamicControllerInput::Const as i32, 0x11);
        assert_eq!(DynamicControllerInput::from_name("SUS_TRAVEL_RR").map(|i| i as i32), Some(24));
        assert_eq!(DynamicControllerInput::from_name("SCRIPT_11"), None);
    }
}
