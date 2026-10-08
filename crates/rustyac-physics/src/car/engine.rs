// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The engine slot ([`EngineModel`]) and AC's `Engine` as its Vanilla implementation
//! ([`VanillaEngine`]): throttle maps, rev limiter, full-throttle torque from `power.lut`,
//! turbos, engine braking, damage, and the blend of the two by the throttle.
//!
//! The engine is a torque source: `Engine::step` gets a throttle and an engine speed and
//! leaves `status.outTorque` (Nm, a double). The drivetrain owns it, calls it once per step
//! and integrates the engine speed itself.

use std::path::Path;

use super::dynamic_controller::{CarSignals, DynamicController};
use super::replay::TraceValue;
use crate::curve::Curve;
use crate::data::ini::{append_path, IniReader};
use crate::math::{powf, sin};

/// AC's `SACEngineInput` (0x10 bytes). Only `gasInput` and `rpm` are ever filled.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SACEngineInput {
    /// `gasInput`
    pub gas_input: f32,
    /// `carSpeed`
    pub car_speed: f32,
    /// `altitude`
    pub altitude: f32,
    /// `rpm`
    pub rpm: f32,
}

/// AC's `EngineStatus` (0x18 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineStatus {
    /// `outTorque`, Nm
    pub out_torque: f64,
    /// `externalCoastTorque`: what the hybrid system adds to the engine braking
    pub external_coast_torque: f64,
    /// `turboBoost`: torque multiplier minus one
    pub turbo_boost: f32,
    /// `isLimiterOn`
    pub is_limiter_on: bool,
}

/// The members of AC's `Engine` the rest of the car reads or writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineBase {
    /// `status` (+0x130)
    pub status: EngineStatus,
    /// `coastTorqueMultiplier` (+0x148): setup item `COAST_TORQUE_MULT`; only
    /// `Engine::getTorqueAtRPM` uses it
    pub coast_torque_multiplier: f32,
    /// `limiterMultiplier` (+0x14c): setup item `ENGINE_LIMITER`
    pub limiter_multiplier: f32,
    /// `fuelPressure` (+0x150): 1, or 0 with an empty tank or a dead engine. `Car::step`
    /// writes it every step
    pub fuel_pressure: f32,
    /// `bov` (+0x154): blow-off valve flag for the sound, 0 or 1
    pub bov: f32,
    /// `restrictor` (+0x17c)
    pub restrictor: f32,
    /// `lastInput` (+0x1dc): the input of the last step; its `gas_input` ends up as the
    /// throttle after maps, limiter and aids
    pub last_input: SACEngineInput,
    /// `inertia` (+0x1f0), kg m^2
    pub inertia: f32,
    /// `limiterOn` (+0x1f4): steps the limiter still cuts
    pub limiter_on: i32,
    /// `electronicOverride` (+0x1f8): factor on the throttle; traction control and the pit
    /// limiter write 0, the engine re-arms it to 1 at the end of its step
    pub electronic_override: f32,
    /// `maxPowerW_Dynamic` (+0x1fc)
    pub max_power_w_dynamic: f32,
    /// `gasUsage` (+0x31c): the throttle the engine really used (fuel burn)
    pub gas_usage: f32,
    /// `lifeLeft` (+0x320): 1000 when new; at or below 0 the engine is dead
    pub life_left: f64,
}

/// The engine slot: AC's `Engine` as the drivetrain and the rest of the car see it.
pub trait EngineModel {
    /// `Engine::step` @ 0x1402880e0. `car` supplies what the engine reads through its `Car`
    /// and `PhysicsEngine` pointers (air temperature, damage rate, clock, the KERS button,
    /// the inputs of the turbo controllers).
    fn step(&mut self, input: &SACEngineInput, dt: f32, car: &CarSignals);
    /// `Engine::reset` @ 0x140287f90: turbos stopped, a new engine.
    fn reset(&mut self);
    /// `Engine::getLimiterRPM` @ 0x140285a50 (vtable +0x08)
    fn get_limiter_rpm(&self) -> i32;
    /// `Engine::isLimiterOn` @ 0x140285e90 (vtable +0x10)
    fn is_limiter_on(&self) -> bool;
    /// `acEngineData::minimum`: idle speed, rpm
    fn minimum(&self) -> i32;
    /// `Engine::getMaxPowerRPM` @ 0x140285a70
    fn get_max_power_rpm(&self) -> f32;
    /// `Engine::getMaxTorqueRPM` @ 0x140285ae0
    fn get_max_torque_rpm(&self) -> f32;
    /// `Engine::getMaxPowerW` @ 0x140285a80: the largest power seen so far, or, before the
    /// engine has run, the curve's peak times one plus the turbos' full boost.
    fn get_max_power_w(&self) -> f32;
    /// `Engine::p2p`: the push-to-pass state, for the telemetry (`None`: an engine without).
    fn push_to_pass(&self) -> Option<&PushToPass> {
        None
    }
    /// `Engine::coastSettingsDefaultIndex` (`engine.ini [COAST_SETTINGS] DEFAULT`): where the
    /// cockpit's engine-brake setting starts.
    fn coast_settings_default_index(&self) -> i32 {
        0
    }
    /// `Engine::setTurboBoostLevel` @ 0x140288090: the cockpit boost control.
    fn set_turbo_boost_level(&mut self, level: f32);
    /// `Engine::setCoastSettings` @ 0x140288010: the cockpit engine-brake control.
    fn set_coast_settings(&mut self, index: i32);
    /// `Engine::blowUp` @ 0x140285a30
    fn blow_up(&mut self);
    fn base(&self) -> &EngineBase;
    fn base_mut(&mut self) -> &mut EngineBase;
    /// The values `tools/car_oracle` records of the engine, under its names.
    fn trace(&self, out: &mut Vec<TraceValue>);
    /// Everything that changes from step to step, for a test that starts in mid-run.
    fn save_state(&self, out: &mut Vec<u32>);
    /// The inverse of [`EngineModel::save_state`].
    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String>;
}

/// AC's `acEngineData` (0x128 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct AcEngineData {
    /// `powerCurve`: rpm -> Nm at full throttle, without boost
    pub power_curve: Curve,
    /// `coast2`: quadratic engine-braking term
    pub coast2: f32,
    /// `coast1`: linear engine-braking term, Nm per rpm above idle (negative)
    pub coast1: f32,
    /// `coast0`: not used by the step
    pub coast0: f32,
    /// `minimum`: idle speed, rpm
    pub minimum: i32,
    /// `limiter`, rpm; 0 = none
    pub limiter: i32,
    /// `limiterCycles`: physics steps one limiter cut lasts
    pub limiter_cycles: i32,
    /// `overlapFreq`, `overlapGain`, `overlapIdealRPM`: camshaft-overlap roughness
    pub overlap_freq: f32,
    pub overlap_gain: f32,
    pub overlap_ideal_rpm: f32,
}

impl Default for AcEngineData {
    /// `acEngineData::acEngineData` @ 0x1402854a0.
    fn default() -> AcEngineData {
        AcEngineData {
            power_curve: Curve::new(),
            coast2: 1.0e-6,
            coast1: 0.0,
            coast0: 0.0,
            minimum: 1000,
            limiter: 18000,
            limiter_cycles: 50,
            overlap_freq: 1.0,
            overlap_gain: 0.0,
            overlap_ideal_rpm: 6000.0,
        }
    }
}

/// AC's `TurboDef` (0x1c bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurboDef {
    /// `maxBoost`
    pub max_boost: f32,
    /// `lagUP`, `lagDN`: rates per second
    pub lag_up: f32,
    pub lag_dn: f32,
    /// `rpmRef`
    pub rpm_ref: f32,
    /// `gamma`
    pub gamma: f32,
    /// `wastegate`: boost cap; 0 = none
    pub wastegate: f32,
    /// `isAdjustable`
    pub is_adjustable: bool,
}

impl Default for TurboDef {
    /// The values `Engine::loadINI` starts every `[TURBO_n]` from.
    fn default() -> TurboDef {
        TurboDef { max_boost: 0.0, lag_up: 0.0, lag_dn: 0.0, rpm_ref: 6000.0, gamma: 1.0, wastegate: 0.0, is_adjustable: false }
    }
}

/// AC's `Turbo` (0x24 bytes): one number between 0 and 1 (`rotation`) that chases a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbo {
    /// `userSetting`: the cockpit boost level, 1 for a turbo that cannot be adjusted
    pub user_setting: f32,
    /// `rotation`
    pub rotation: f32,
    /// `data`
    pub data: TurboDef,
}

impl Turbo {
    /// `Turbo::Turbo(const TurboDef&)` @ 0x1402ae730.
    pub fn new(data: TurboDef) -> Turbo {
        Turbo { user_setting: 1.0, rotation: 0.0, data }
    }

    /// `Turbo::step` @ 0x1402ae7c0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, gas: f32, rpm: f32, dt: f32) {
        let mut target = 0.0f32;
        if rpm > 0.0 && gas > 0.0 {
            let mut x = gas * rpm / self.data.rpm_ref;
            if x > 1.0 {
                x = 1.0;
            } else if !(x >= 0.0) {
                x = 0.0;
            }
            target = powf(x, self.data.gamma);
        }
        let rotation = self.rotation;
        let lag = if target > rotation { dt * self.data.lag_up } else { dt * self.data.lag_dn };
        let k = if lag > 1.0 {
            1.0
        } else if lag >= 0.0 {
            lag
        } else {
            0.0
        };
        self.rotation = (target - rotation) * k + rotation;
        let wastegate = self.data.wastegate;
        if wastegate < 0.0 || wastegate > 0.0 {
            let limit = wastegate * self.user_setting;
            if self.data.max_boost * self.rotation > limit {
                self.rotation = limit / self.data.max_boost;
            }
        }
    }

    /// `Turbo::getBoost` @ 0x1402ae780.
    pub fn get_boost(&self) -> f32 {
        self.data.max_boost * self.rotation
    }

    /// `Turbo::setTurboBoostLevel` @ 0x1402ae7a0.
    pub fn set_turbo_boost_level(&mut self, level: f32) {
        self.user_setting = if self.data.is_adjustable { level } else { 1.0 };
    }
}

/// AC's `TurboDynamicController` (0x38 bytes): a controller that drives one turbo's
/// `maxBoost` (`ctrl_turboN.ini`) or its `wastegate` (`ctrl_wastegateN.ini`).
#[derive(Clone, Debug, PartialEq)]
pub struct TurboDynamicController {
    /// `turbo`: index into [`VanillaEngine::turbos`] (a pointer in the game)
    pub turbo: usize,
    /// `controller`
    pub controller: DynamicController,
    /// `isWastegate`
    pub is_wastegate: bool,
}

/// AC's `PushToPass` (0x28 bytes): a timed overboost on the KERS button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PushToPass {
    /// `enabled`: engine.ini has `[PUSH_TO_PASS]`
    pub enabled: bool,
    /// `active`
    pub active: bool,
    /// `overboost`
    pub overboost: f32,
    /// `timeS`: how long one push lasts
    pub time_s: f32,
    /// `coolDownS`
    pub cool_down_s: f32,
    /// `timeAccum`
    pub time_accum: f32,
    /// `activations`: pushes left (0 after loading; the game's race code hands them out)
    pub activations: i32,
    /// `baseWastegate`
    pub base_wastegate: f32,
    /// `baseActivations`, `basePositionCoeff`, `maxActivations`
    pub base_activations: i32,
    pub base_position_coeff: i32,
    pub max_activations: i32,
}

/// AC's `Engine` (0x3e8 bytes), without the hybrid systems' torque and coast generators
/// (`torqueGenerators`, `coastGenerators`: KERS and ERS, not ported; the lists are empty for
/// a car without them).
#[derive(Clone, Debug, PartialEq)]
pub struct VanillaEngine {
    pub base: EngineBase,
    /// `data` (+0x08)
    pub data: AcEngineData,
    /// `turbos` (+0x158)
    pub turbos: Vec<Turbo>,
    /// `isEngineStallEnabled` (+0x170): nothing in the game ever sets it
    pub is_engine_stall_enabled: bool,
    /// The starter key of the stall branch (`GetAsyncKeyState(VK_BACK)` in the game).
    pub starter_key_down: bool,
    /// `starterTorque` (+0x174)
    pub starter_torque: f32,
    /// `rpmDamageThreshold` (+0x178)
    pub rpm_damage_threshold: f32,
    /// `p2p` (+0x180)
    pub p2p: PushToPass,
    /// `turboAdjustableFromCockpit` (+0x1d8)
    pub turbo_adjustable_from_cockpit: bool,
    /// `defaultEngineLimiter` (+0x1ec)
    pub default_engine_limiter: i32,
    /// `maxPowerW`, `maxTorqueNM`, `maxPowerRPM`, `maxTorqueRPM` (+0x200): without boost
    pub max_power_w: f32,
    pub max_torque_nm: f32,
    pub max_power_rpm: f32,
    pub max_torque_rpm: f32,
    /// `throttleResponseCurve` (+0x218, `throttle.lut`): pedal % -> throttle %
    pub throttle_response_curve: Curve,
    /// `throttleResponseCurveMax` (+0x298) and its reference rpm (+0x318)
    pub throttle_response_curve_max: Curve,
    pub throttle_response_curve_max_ref: f32,
    /// `turboBoostDamageThreshold`, `turboBoostDamageK`, `rpmDamageK` (+0x328)
    pub turbo_boost_damage_threshold: f32,
    pub turbo_boost_damage_k: f32,
    pub rpm_damage_k: f32,
    /// `bovThreshold` (+0x334)
    pub bov_threshold: f32,
    /// `turboControllers` (+0x340)
    pub turbo_controllers: Vec<TurboDynamicController>,
    /// `gasCoastOffset` (+0x358): the engine-brake setting, a floor under the throttle
    pub gas_coast_offset: f32,
    /// `gasCoastOffsetCurve` (+0x360)
    pub gas_coast_offset_curve: Curve,
    /// `coastSettingsDefaultIndex`, `coastEntryRpm` (+0x3e0)
    pub coast_settings_default_index: i32,
    pub coast_entry_rpm: i32,
}

impl Default for VanillaEngine {
    /// `Engine::Engine` @ 0x1402852b0.
    fn default() -> VanillaEngine {
        VanillaEngine {
            base: EngineBase {
                status: EngineStatus::default(),
                coast_torque_multiplier: 0.0,
                limiter_multiplier: 0.0,
                fuel_pressure: 1.0,
                bov: 0.0,
                restrictor: 0.0,
                last_input: SACEngineInput::default(),
                inertia: 1.0,
                limiter_on: 0,
                electronic_override: 0.0,
                max_power_w_dynamic: -1.0,
                gas_usage: 0.0,
                life_left: 0.0,
            },
            data: AcEngineData::default(),
            turbos: Vec::new(),
            is_engine_stall_enabled: false,
            starter_key_down: false,
            starter_torque: 20.0,
            rpm_damage_threshold: 0.0,
            p2p: PushToPass { base_position_coeff: 1, ..PushToPass::default() },
            turbo_adjustable_from_cockpit: false,
            default_engine_limiter: 0,
            max_power_w: 0.0,
            max_torque_nm: 0.0,
            max_power_rpm: 0.0,
            max_torque_rpm: 0.0,
            throttle_response_curve: Curve::new(),
            throttle_response_curve_max: Curve::new(),
            throttle_response_curve_max_ref: 6000.0,
            turbo_boost_damage_threshold: 0.0,
            turbo_boost_damage_k: 0.0,
            rpm_damage_k: 0.0,
            bov_threshold: 0.2,
            turbo_controllers: Vec::new(),
            gas_coast_offset: 0.0,
            gas_coast_offset_curve: Curve::new(),
            coast_settings_default_index: 0,
            coast_entry_rpm: 0,
        }
    }
}

/// `ucomiss x, 0` + `jne`: not zero and a number.
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// The three-way clamp to 0..1 of the machine code: above 1 gives 1, anything that is not
/// at least 0 (a NaN too) gives 0.
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

/// `PhysicsEngine::getAirDensity` @ 0x140263a60, kg/m^3.
pub fn get_air_density(ambient_temperature: f32) -> f32 {
    1.2922 - ambient_temperature * 0.0041
}

impl VanillaEngine {
    /// `Engine::init` @ 0x140285e10 with `Engine::loadINI` @ 0x140286100, for the car whose
    /// data folder is `data_path`.
    pub fn new(data_path: &Path) -> Result<VanillaEngine, String> {
        let mut engine = VanillaEngine::default();
        engine.is_engine_stall_enabled = false;
        engine.base.limiter_multiplier = 1.0;
        engine.base.coast_torque_multiplier = 1.0;
        engine.base.limiter_on = 0;
        engine.base.electronic_override = 1.0;
        engine.load_ini(data_path)?;
        engine.reset();
        engine.precalculate_power_and_torque();
        engine.base.restrictor = 0.0;
        Ok(engine)
    }

    /// `Engine::loadINI` @ 0x140286100.
    fn load_ini(&mut self, data_path: &Path) -> Result<(), String> {
        let ini = IniReader::load(&data_path.join("engine.ini"))?;
        if ini.ready {
            // only printed by the game, but parsed: a version that is not a number stops it
            ini.get_int("HEADER", "VERSION")?;
        }
        let power_curve = ini.get_string("HEADER", "POWER_CURVE");
        // the name is appended to the data path as text, whatever it looks like
        self.data.power_curve.load(&append_path(data_path, &power_curve))?;
        if self.data.power_curve.get_count() == 0 {
            // `Engine::precalculatePowerAndTorque` reads in front of an empty table: the game
            // dies while it creates such a car (no engine.ini, no or an empty power file)
            return Err(format!(
                "{}: the power curve {power_curve:?} is missing or empty (the game crashes on such a car)",
                ini.filename.display()
            ));
        }
        self.data.minimum = ini.get_int("ENGINE_DATA", "MINIMUM")?;
        if self.data.minimum == 0 {
            self.data.minimum = 1000;
        }
        if ini.get_string("HEADER", "COAST_CURVE") == "FROM_COAST_REF" {
            let (coast1, coast2) = self.load_coast_settings(&ini, "COAST_REF")?;
            self.data.coast1 = coast1;
            self.data.coast2 = coast2;
        }
        self.base.inertia = ini.get_float("ENGINE_DATA", "INERTIA")?;
        self.default_engine_limiter = ini.get_int("ENGINE_DATA", "LIMITER")?;
        self.data.limiter = ini.get_int("ENGINE_DATA", "LIMITER")?;
        let hz = ini.get_int("ENGINE_DATA", "LIMITER_HZ")?;
        // integer divisions: milliseconds per cut, then physics steps
        self.data.limiter_cycles = if hz == 0 { 50 } else { (1000 / hz) / 3 };
        if ini.has_section("COAST_SETTINGS") {
            self.gas_coast_offset_curve = ini.get_curve("COAST_SETTINGS", "LUT")?;
            self.coast_settings_default_index = ini.get_int("COAST_SETTINGS", "DEFAULT")?;
            self.set_coast_settings(self.coast_settings_default_index);
            self.coast_entry_rpm = ini.get_int("COAST_SETTINGS", "ACTIVATION_RPM")?.wrapping_add(self.data.minimum);
        }
        let mut index = 0;
        loop {
            let section = format!("TURBO_{index}");
            if !ini.has_section(&section) {
                break;
            }
            let mut def = TurboDef::default();
            def.lag_dn = ((1.0 - ini.get_float(&section, "LAG_DN")?) * 1.333_333_4) * 333.333_34;
            def.lag_up = ((1.0 - ini.get_float(&section, "LAG_UP")?) * 1.333_333_4) * 333.333_34;
            def.max_boost = ini.get_float(&section, "MAX_BOOST")?;
            def.rpm_ref = ini.get_float(&section, "REFERENCE_RPM")?;
            def.gamma = ini.get_float(&section, "GAMMA")?;
            def.wastegate = ini.get_float(&section, "WASTEGATE")?;
            def.is_adjustable = ini.get_int(&section, "COCKPIT_ADJUSTABLE")? != 0;
            if def.is_adjustable {
                self.turbo_adjustable_from_cockpit = true;
            }
            self.turbos.push(Turbo::new(def));
            index += 1;
        }
        if self.turbo_adjustable_from_cockpit {
            self.set_turbo_boost_level(ini.get_float("ENGINE_DATA", "DEFAULT_TURBO_ADJUSTMENT")?);
        }
        if ini.has_section("OVERLAP") {
            self.data.overlap_freq = ini.get_float("OVERLAP", "FREQUENCY")?;
            self.data.overlap_gain = ini.get_float("OVERLAP", "GAIN")?;
            self.data.overlap_ideal_rpm = ini.get_float("OVERLAP", "IDEAL_RPM")?;
        }
        let throttle = data_path.join("throttle.lut");
        if crate::data::exists(&throttle) {
            self.throttle_response_curve.load(&throttle)?;
        }
        if self.default_engine_limiter != 0 {
            self.rpm_damage_threshold = self.default_engine_limiter as f32 * 1.05;
            self.rpm_damage_k = 10.0;
        }
        if ini.has_section("DAMAGE") {
            if !self.turbos.is_empty() {
                self.turbo_boost_damage_threshold = ini.get_float("DAMAGE", "TURBO_BOOST_THRESHOLD")?;
                self.turbo_boost_damage_k = ini.get_float("DAMAGE", "TURBO_DAMAGE_K")?;
            }
            self.rpm_damage_threshold = ini.get_float("DAMAGE", "RPM_THRESHOLD")?;
            self.rpm_damage_k = ini.get_float("DAMAGE", "RPM_DAMAGE_K")?;
        }
        if ini.has_section("BOV") {
            self.bov_threshold = ini.get_float("BOV", "PRESSURE_THRESHOLD")?;
        }
        for turbo in 0..self.turbos.len() {
            for (name, is_wastegate) in [("ctrl_turbo", false), ("ctrl_wastegate", true)] {
                let path = data_path.join(format!("{name}{turbo}.ini"));
                if crate::data::exists(&path) {
                    let controller = DynamicController::load(&path)?;
                    self.turbo_controllers.push(TurboDynamicController { turbo, controller, is_wastegate });
                }
            }
        }
        if ini.has_section("PUSH_TO_PASS") {
            if self.turbos.is_empty() {
                return Err(format!("{}: [PUSH_TO_PASS] without a turbo (a critical error in the game)", ini.filename.display()));
            }
            self.p2p.enabled = true;
            self.p2p.cool_down_s = ini.get_float("PUSH_TO_PASS", "COOLDOWN_SECONDS")?;
            // the first push does not have to wait
            self.p2p.time_accum = self.p2p.cool_down_s;
            self.p2p.activations = 0;
            self.p2p.time_s = ini.get_float("PUSH_TO_PASS", "TIME_SECONDS")?;
            self.p2p.overboost = ini.get_float("PUSH_TO_PASS", "OVERBOOST")?;
            self.p2p.base_wastegate = self.turbos[0].data.wastegate;
            self.p2p.base_activations = ini.get_int("PUSH_TO_PASS", "ACTIVATION_BASE")?;
            self.p2p.base_position_coeff = ini.get_int("PUSH_TO_PASS", "ACTIVATION_POS")?;
            self.p2p.max_activations = ini.get_int("PUSH_TO_PASS", "ACTIVATION_MAX")?;
        }
        if ini.has_section("THROTTLE_RESPONSE") {
            self.throttle_response_curve_max_ref = ini.get_float("THROTTLE_RESPONSE", "RPM_REFERENCE")?;
            self.throttle_response_curve_max = ini.get_curve("THROTTLE_RESPONSE", "LUT")?;
        }
        Ok(())
    }

    /// `Engine::loadCoastSettings` @ 0x140285ea0: (`coast1`, `coast2`) from a reference
    /// engine-braking torque at a reference speed.
    fn load_coast_settings(&self, ini: &IniReader, section: &str) -> Result<(f32, f32), String> {
        let rpm = ini.get_float(section, "RPM")?;
        let torque = ini.get_float(section, "TORQUE")?;
        let non_linearity = ini.get_float(section, "NON_LINEARITY")?;
        let linear = (1.0 - non_linearity) * rpm - self.data.minimum as f32;
        let quadratic = non_linearity * rpm;
        let coast1 = if ordered_nonzero(linear) { -(torque / linear) } else { 0.0 };
        let coast2 = if ordered_nonzero(quadratic) { torque / (quadratic * quadratic) } else { 0.0 };
        Ok((coast1, coast2))
    }

    /// `Engine::precalculatePowerAndTorque` @ 0x140287d10: scans the torque curve in 50 rpm
    /// steps for the largest torque and the largest power.
    fn precalculate_power_and_torque(&mut self) {
        self.max_power_w = 0.0;
        self.max_torque_nm = 0.0;
        self.max_power_rpm = 0.0;
        self.max_torque_rpm = 0.0;
        // `Curve::getMaxReference` @ 0x140206930: the last reference of the table (an empty
        // table is refused by the loader; the game reads in front of it here)
        let Some(&last) = self.data.power_curve.references().last() else {
            return;
        };
        if !(last > 0.0) {
            return;
        }
        let mut step = 0i32;
        let mut rpm = 0.0f32;
        loop {
            let torque = self.data.power_curve.get_value(rpm);
            if torque > self.max_torque_nm {
                self.max_torque_nm = torque;
                self.max_torque_rpm = rpm;
            }
            let power = (rpm * torque) * 0.1047;
            if power > self.max_power_w {
                self.max_power_w = power;
                self.max_power_rpm = rpm;
            }
            step += 50;
            rpm = step as f32;
            // `comiss rpm, last` + `jb`: goes on while below the last reference (or a NaN)
            if rpm >= last {
                break;
            }
        }
    }

    /// `Engine::getThrottleResponseGas` @ 0x140285b90: the pedal through `throttle.lut`, and
    /// with a `[THROTTLE_RESPONSE]` section a second map blended in with engine speed.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_throttle_response_gas(&self, gas: f32, rpm: f32) -> f32 {
        if self.throttle_response_curve.get_count() != 0 && self.throttle_response_curve_max.get_count() != 0 {
            let percent = gas * 100.0;
            let low = saturate(self.throttle_response_curve.get_value(percent) * 0.01);
            let high = saturate(self.throttle_response_curve_max.get_value(percent) * 0.01);
            let ratio = rpm / self.throttle_response_curve_max_ref;
            let k = if ratio > 1.0 {
                1.0
            } else if ratio >= 0.0 {
                ratio
            } else {
                0.0
            };
            (high - low) * k + low
        } else if self.throttle_response_curve.get_count() != 0 {
            let value = self.throttle_response_curve.get_value(gas * 100.0) * 0.01;
            if value > 1.0 {
                1.0
            } else if value < 0.0 || value.is_nan() {
                0.0
            } else {
                value
            }
        } else {
            gas
        }
    }

    /// `Engine::stepP2P` @ 0x140288750. (The game also fires `Car::evOnPush2Pass` for its
    /// display when a push starts.)
    fn step_p2p(&mut self, dt: f32, kers_button: bool) {
        let accum = dt + self.p2p.time_accum;
        self.p2p.time_accum = if accum > 1000.0 { 1000.0 } else { accum };
        if !self.p2p.active {
            if self.p2p.activations > 0 && kers_button && self.p2p.time_accum > self.p2p.cool_down_s {
                self.p2p.activations -= 1;
                self.p2p.time_accum = 0.0;
                self.p2p.active = true;
                for turbo in &mut self.turbos {
                    turbo.data.wastegate = self.p2p.overboost + self.p2p.base_wastegate;
                }
            }
        } else if self.p2p.time_accum > self.p2p.time_s {
            self.p2p.time_accum = 0.0;
            self.p2p.active = false;
            for turbo in &mut self.turbos {
                turbo.data.wastegate = self.p2p.base_wastegate;
            }
        }
    }

    /// `Engine::stepTurbos` @ 0x140288900.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_turbos(&mut self, car: &CarSignals) {
        for entry in &mut self.turbo_controllers {
            let value = entry.controller.eval(car);
            let turbo = &mut self.turbos[entry.turbo];
            if entry.is_wastegate {
                turbo.data.wastegate = value;
            } else {
                turbo.data.max_boost = value;
            }
        }
        self.base.status.turbo_boost = 0.0;
        // `comiss 0, rpm` + `cmovb`: the engine speed when it is above 0 (or a NaN), else 0
        let rpm = if !(0.0 >= self.base.last_input.rpm) { self.base.last_input.rpm } else { 0.0 };
        let gas = self.base.last_input.gas_input;
        for turbo in &mut self.turbos {
            // 0.003 is a literal here, not the step length
            turbo.step(gas, rpm, 0.003);
            self.base.status.turbo_boost = turbo.get_boost() * self.base.fuel_pressure + self.base.status.turbo_boost;
        }
    }
}

impl EngineModel for VanillaEngine {
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step(&mut self, input: &SACEngineInput, dt: f32, car: &CarSignals) {
        self.base.last_input = *input;
        let rpm = input.rpm;
        let minimum = self.data.minimum as f32;
        self.base.last_input.gas_input = self.get_throttle_response_gas(input.gas_input, rpm);
        if self.p2p.enabled {
            self.step_p2p(dt, car.chassis.controls.kers);
        }
        // the engine-brake setting: a little throttle held open on the overrun
        if self.gas_coast_offset > 0.0 {
            let k = saturate((rpm - minimum) / self.coast_entry_rpm as f32);
            let offset = self.gas_coast_offset * k;
            self.base.last_input.gas_input = saturate((1.0 - offset) * self.base.last_input.gas_input + offset);
        }
        // the rev limiter cuts for `limiterCycles` steps
        if self.data.limiter != 0 {
            let limit = self.data.limiter as f32 * self.base.limiter_multiplier;
            if !(limit >= rpm) {
                self.base.limiter_on = self.data.limiter_cycles;
            }
        }
        if self.base.limiter_on > 0 {
            self.base.limiter_on -= 1;
            self.base.last_input.gas_input = 0.0;
        }
        if 0.0 >= self.base.life_left {
            self.base.fuel_pressure = 0.0;
        }
        // traction control and the pit limiter
        let gas = self.base.electronic_override * self.base.last_input.gas_input;
        self.base.last_input.gas_input = gas;
        self.base.gas_usage = gas;

        let mut torque = self.data.power_curve.get_value(rpm);
        let mut coast = 0.0f32;
        if ordered_nonzero(self.data.coast1) {
            coast = (rpm - minimum) * self.data.coast1;
        }
        self.step_turbos(car);
        let boost = self.base.status.turbo_boost;
        if ordered_nonzero(boost) {
            torque *= boost + 1.0;
        }
        if ordered_nonzero(self.data.coast2) {
            let above = rpm - minimum;
            let sign = if rpm > 0.0 {
                1.0
            } else if rpm >= 0.0 {
                0.0
            } else {
                -1.0
            };
            coast -= ((above * above) * self.data.coast2) * sign;
        }
        // (the hybrid system's coast generators would add to `coast` here)
        if !(rpm > minimum) {
            coast = 0.0;
            self.base.status.external_coast_torque = 0.0;
        }
        self.base.bov = if (1.0 - gas) * boost > self.bov_threshold { 1.0 } else { 0.0 };

        // damage; 0.003 is a literal
        let damage_rate = car.chassis.env.mechanical_damage_rate;
        let threshold = self.turbo_boost_damage_threshold;
        if ordered_nonzero(threshold) && boost > threshold {
            let wear = (((boost - threshold) * self.turbo_boost_damage_k) * 0.003) * damage_rate;
            self.base.life_left -= wear as f64;
        }
        let threshold = self.rpm_damage_threshold;
        if ordered_nonzero(threshold) && rpm > threshold {
            let wear = (((rpm - threshold) * self.rpm_damage_k) * 0.003) * damage_rate;
            self.base.life_left -= wear as f64;
        }

        // hot air makes the engine weaker; 1.0 at 20 deg C
        let mut air = car.chassis.env.air_density() * 0.826_309_74;
        if self.base.restrictor > 0.0 {
            air -= ((self.base.restrictor * rpm) * 0.0001) * gas;
            if !(air >= 0.0) {
                air = 0.0;
            }
        }
        let has_fuel = !(0.0 >= self.base.fuel_pressure);
        let blended = (((torque - coast) * gas + coast) * air) as f64;
        self.base.status.out_torque = blended;
        if has_fuel {
            if rpm >= minimum {
                let gain = self.data.overlap_gain;
                if ordered_nonzero(gain) {
                    // 1 / 3000 as the constant 0x3f35d867c3ece2a5
                    let phase = (((car.chassis.physics_time * 0.001) * self.data.overlap_freq as f64) * rpm as f64)
                        * f64::from_bits(0x3f35_d867_c3ec_e2a5);
                    let wave = (sin(phase) * 0.5 - 0.5) as f32;
                    let amplitude = (rpm - self.data.overlap_ideal_rpm).abs() * gain;
                    self.base.status.out_torque = (wave * amplitude) as f64 + blended;
                }
            } else if self.is_engine_stall_enabled {
                let mut stalled = rpm * -0.01;
                if self.starter_key_down {
                    stalled = self.starter_torque;
                }
                self.base.status.out_torque = stalled as f64;
            } else {
                // below idle: never less than 15 Nm. This floor is the whole idle control
                self.base.status.out_torque = if blended >= 15.0 { blended } else { 15.0 };
            }
        }
        // no fuel, or a dead engine: only drag
        let fuel_pressure = self.base.fuel_pressure;
        if !(fuel_pressure >= 1.0) {
            let drag = rpm as f64 * -0.01;
            self.base.status.out_torque = (self.base.status.out_torque - drag) * fuel_pressure as f64 + drag;
        }
        // (the hybrid system's torque generators would add to `outTorque` here)
        self.base.status.is_limiter_on = self.base.limiter_on != 0;
        self.base.electronic_override = 1.0;
        // 0.1047f widened: 0x3fbacd9e80000000
        let power = (rpm as f64 * self.base.status.out_torque) * f64::from_bits(0x3fba_cd9e_8000_0000);
        if power > self.base.max_power_w_dynamic as f64 {
            self.base.max_power_w_dynamic = power as f32;
        }
    }

    fn reset(&mut self) {
        for turbo in &mut self.turbos {
            turbo.rotation = 0.0;
        }
        self.base.life_left = 1000.0;
    }

    fn get_limiter_rpm(&self) -> i32 {
        crate::car::drivetrain::cvttss2si(self.data.limiter as f32 * self.base.limiter_multiplier)
    }

    fn is_limiter_on(&self) -> bool {
        self.base.limiter_on > 0
    }

    fn minimum(&self) -> i32 {
        self.data.minimum
    }

    fn get_max_power_rpm(&self) -> f32 {
        self.max_power_rpm
    }

    fn get_max_torque_rpm(&self) -> f32 {
        self.max_torque_rpm
    }

    fn push_to_pass(&self) -> Option<&PushToPass> {
        Some(&self.p2p)
    }

    fn coast_settings_default_index(&self) -> i32 {
        self.coast_settings_default_index
    }

    fn get_max_power_w(&self) -> f32 {
        if self.base.max_power_w_dynamic > 0.0 {
            return self.base.max_power_w_dynamic;
        }
        let mut boost = 0.0f32;
        for turbo in &self.turbos {
            boost += turbo.data.max_boost;
        }
        (boost + 1.0) * self.max_power_w
    }

    fn set_turbo_boost_level(&mut self, level: f32) {
        for turbo in &mut self.turbos {
            turbo.set_turbo_boost_level(level);
        }
    }

    fn set_coast_settings(&mut self, index: i32) {
        if index < 0 || index > self.gas_coast_offset_curve.get_count() {
            return;
        }
        self.gas_coast_offset = self.gas_coast_offset_curve.get_value(index as f32);
    }

    fn blow_up(&mut self) {
        self.base.life_left = -100.0;
    }

    fn base(&self) -> &EngineBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut EngineBase {
        &mut self.base
    }

    fn trace(&self, out: &mut Vec<TraceValue>) {
        let b = &self.base;
        out.push(TraceValue::d("engine.status.outTorque", b.status.out_torque));
        out.push(TraceValue::d("engine.status.externalCoastTorque", b.status.external_coast_torque));
        out.push(TraceValue::f("engine.status.turboBoost", b.status.turbo_boost));
        out.push(TraceValue::i("engine.status.isLimiterOn", b.status.is_limiter_on as i32));
        out.push(TraceValue::f("engine.fuelPressure", b.fuel_pressure));
        out.push(TraceValue::i("engine.limiterOn", b.limiter_on));
        out.push(TraceValue::f("engine.electronicOverride", b.electronic_override));
        out.push(TraceValue::d("engine.lifeLeft", b.life_left));
        out.push(TraceValue::f("engine.lastInput.gas", b.last_input.gas_input));
        out.push(TraceValue::f("engine.gasUsage", b.gas_usage));
        out.push(TraceValue::f("engine.bov", b.bov).extra());
        out.push(TraceValue::f("engine.maxPowerW_Dynamic", b.max_power_w_dynamic).extra());
        out.push(TraceValue::f("engine.gasCoastOffset", self.gas_coast_offset).extra());
        out.push(TraceValue::f("engine.limiterMultiplier", b.limiter_multiplier).extra());
        out.push(TraceValue::i("engine.turbos", self.turbos.len() as i32).extra());
        for k in 0..2 {
            let turbo = self.turbos.get(k);
            let value = |f: fn(&Turbo) -> f32| turbo.map(f).unwrap_or(0.0);
            out.push(TraceValue::f(&format!("engine.turbo{k}.userSetting"), value(|t| t.user_setting)).extra());
            out.push(TraceValue::f(&format!("engine.turbo{k}.rotation"), value(|t| t.rotation)).extra());
            out.push(TraceValue::f(&format!("engine.turbo{k}.maxBoost"), value(|t| t.data.max_boost)).extra());
            out.push(TraceValue::f(&format!("engine.turbo{k}.wastegate"), value(|t| t.data.wastegate)).extra());
        }
    }

    fn save_state(&self, out: &mut Vec<u32>) {
        let b = &self.base;
        for value in [b.status.out_torque, b.status.external_coast_torque, b.life_left] {
            out.push(value.to_bits() as u32);
            out.push((value.to_bits() >> 32) as u32);
        }
        out.extend(
            [
                b.status.turbo_boost,
                b.coast_torque_multiplier,
                b.limiter_multiplier,
                b.fuel_pressure,
                b.bov,
                b.restrictor,
                b.last_input.gas_input,
                b.last_input.car_speed,
                b.last_input.altitude,
                b.last_input.rpm,
                b.electronic_override,
                b.max_power_w_dynamic,
                b.gas_usage,
                self.gas_coast_offset,
                self.p2p.time_accum,
            ]
            .map(f32::to_bits),
        );
        out.extend([b.status.is_limiter_on as u32, b.limiter_on as u32, self.p2p.active as u32, self.p2p.activations as u32]);
        for turbo in &self.turbos {
            out.extend([turbo.user_setting, turbo.rotation, turbo.data.max_boost, turbo.data.wastegate].map(f32::to_bits));
        }
        for entry in &self.turbo_controllers {
            out.extend(entry.controller.stages.iter().map(|stage| stage.current_value.to_bits()));
        }
    }

    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        let mut next = || words.next().ok_or("the saved engine state is too short".to_string());
        let double = |next: &mut dyn FnMut() -> Result<u32, String>| -> Result<f64, String> {
            let low = next()? as u64;
            let high = next()? as u64;
            Ok(f64::from_bits(low | high << 32))
        };
        self.base.status.out_torque = double(&mut next)?;
        self.base.status.external_coast_torque = double(&mut next)?;
        self.base.life_left = double(&mut next)?;
        let mut f = || next().map(f32::from_bits);
        self.base.status.turbo_boost = f()?;
        self.base.coast_torque_multiplier = f()?;
        self.base.limiter_multiplier = f()?;
        self.base.fuel_pressure = f()?;
        self.base.bov = f()?;
        self.base.restrictor = f()?;
        self.base.last_input = SACEngineInput { gas_input: f()?, car_speed: f()?, altitude: f()?, rpm: f()? };
        self.base.electronic_override = f()?;
        self.base.max_power_w_dynamic = f()?;
        self.base.gas_usage = f()?;
        self.gas_coast_offset = f()?;
        self.p2p.time_accum = f()?;
        self.base.status.is_limiter_on = next()? != 0;
        self.base.limiter_on = next()? as i32;
        self.p2p.active = next()? != 0;
        self.p2p.activations = next()? as i32;
        for turbo in &mut self.turbos {
            turbo.user_setting = f32::from_bits(next()?);
            turbo.rotation = f32::from_bits(next()?);
            turbo.data.max_boost = f32::from_bits(next()?);
            turbo.data.wastegate = f32::from_bits(next()?);
        }
        for entry in &mut self.turbo_controllers {
            for stage in &mut entry.controller.stages {
                stage.current_value = f32::from_bits(next()?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turbo() -> Turbo {
        Turbo::new(TurboDef { max_boost: 0.5, lag_up: 4.0, lag_dn: 8.0, rpm_ref: 9000.0, gamma: 2.0, wastegate: 0.42, is_adjustable: true })
    }

    #[test]
    fn turbo_spools_up_to_the_wastegate_and_down_again() {
        let mut t = turbo();
        t.set_turbo_boost_level(0.7);
        for _ in 0..3000 {
            t.step(1.0, 12_000.0, 0.003);
        }
        // the wastegate (0.42 at level 0.7) caps the boost below the turbo's 0.5
        assert_eq!(t.get_boost().to_bits(), (0.5f32 * (0.42f32 * 0.7 / 0.5)).to_bits());
        let spun = t.rotation;
        t.step(0.0, 12_000.0, 0.003);
        // closed throttle: the target is 0 and the faster lag applies
        assert_eq!(t.rotation.to_bits(), ((0.0 - spun) * (0.003f32 * 8.0) + spun).to_bits());
        // a turbo that cannot be adjusted ignores the cockpit level
        let mut fixed = Turbo::new(TurboDef { is_adjustable: false, ..turbo().data });
        fixed.set_turbo_boost_level(0.3);
        assert_eq!(fixed.user_setting, 1.0);
    }

    #[test]
    fn throttle_maps() {
        let mut engine = VanillaEngine::default();
        // no map: the pedal passes through
        assert_eq!(engine.get_throttle_response_gas(0.37, 5000.0), 0.37);
        engine.throttle_response_curve = Curve::from_pairs(&[(0.0, 0.0), (50.0, 20.0), (100.0, 120.0)]);
        assert_eq!(engine.get_throttle_response_gas(0.5, 5000.0), 20.0f32 * 0.01);
        // a table value above 100 % is clamped
        assert_eq!(engine.get_throttle_response_gas(1.0, 5000.0), 1.0);
        // with a second map the two are blended by engine speed over its reference
        engine.throttle_response_curve_max = Curve::from_pairs(&[(0.0, 0.0), (100.0, 100.0)]);
        engine.throttle_response_curve_max_ref = 10_000.0;
        let (low, high) = (20.0f32 * 0.01, 50.0f32 * 0.01);
        assert_eq!(engine.get_throttle_response_gas(0.5, 2500.0), (high - low) * 0.25 + low);
        assert_eq!(engine.get_throttle_response_gas(0.5, 20_000.0), (high - low) * 1.0 + low);
        assert_eq!(engine.get_throttle_response_gas(0.5, -100.0), (high - low) * 0.0 + low);
    }

    #[test]
    fn air_density_is_one_at_twenty_degrees() {
        let factor = get_air_density(20.0) * 0.826_309_74;
        assert!((factor - 1.0).abs() < 1e-6, "{factor}");
    }
}
