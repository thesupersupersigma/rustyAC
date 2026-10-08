// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `SetupManager` for the values of the ported systems (chassis, brakes, drivetrain,
//! engine): every setup item is a name, a pointer to one float of the car and a multiplier; `SetupManager::step` (late in every
//! physics step) writes `multiplier * newValue` into the float whenever the two differ.
//!
//! Also here: what the game's setup screen does to the items when a session starts with the
//! default setup. The screen shows every item that has a section in the car's `setup.ini` as
//! a spinner holding a whole number, sets the spinner from the car's value and writes the
//! spinner's value back, so a value that is not on the spinner's grid comes back changed
//! (the F2004's 3.0 degrees of front camber become 2.9).

use std::path::Path;

use super::chassis::RollingChassis;
use crate::data::ini::{append_path, IniReader};

/// Which float of the car a setup item is connected to (`SetupItem::connectedFloat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupTarget {
    /// `antirollBars[axle].k`
    ArbK(usize),
    /// `tyres[wheel].status.pressureStatic`
    TyrePressure(usize),
    /// `suspensions[wheel]->getDamper()->bumpFast`
    DamperBumpFast(usize),
    /// `…->bumpSlow`
    DamperBumpSlow(usize),
    /// `…->reboundFast`
    DamperReboundFast(usize),
    /// `…->reboundSlow`
    DamperReboundSlow(usize),
    /// `suspensions[wheel]->bumpStopRate`
    BumpStopRate(usize),
    /// `suspensions[wheel]->k`
    SpringRate(usize),
    /// `suspensions[wheel]->progressiveK`
    ProgressiveSpringRate(usize),
    /// `suspensions[wheel]->rodLength`
    RodLength(usize),
    /// `suspensions[wheel]->staticCamber`
    StaticCamber(usize),
    /// `suspensions[wheel]->toeOUT_Linear`
    ToeOut(usize),
    /// `suspensions[wheel]->packerRange`
    PackerRange(usize),
    /// `heaveSprings[axle].damper.bumpFast`
    HeaveDamperBumpFast(usize),
    HeaveDamperBumpSlow(usize),
    HeaveDamperReboundFast(usize),
    HeaveDamperReboundSlow(usize),
    /// `heaveSprings[axle].bumpStopRate`
    HeaveBumpStopRate(usize),
    /// `heaveSprings[axle].k`
    HeaveSpringRate(usize),
    /// `heaveSprings[axle].progressiveK`
    HeaveProgressiveSpringRate(usize),
    /// `heaveSprings[axle].rodLength`
    HeaveRodLength(usize),
    /// `heaveSprings[axle].packerRange`
    HeavePackerRange(usize),
    /// `aeroMap.wings[i].status.inputAngle` for a wing with controllers, else `status.angle`
    WingAngle(usize),
    /// `Car::steerAssist`
    SteerAssist,
    /// `drivetrain.diffPowerRamp`, `diffCoastRamp`, `diffPreLoad`
    DiffPowerRamp,
    DiffCoastRamp,
    DiffPreLoad,
    /// `brakeSystem.brakePowerMultiplier`, `brakeSystem.frontBias`
    BrakePowerMultiplier,
    FrontBias,
    /// `SetupManager::gearSettings[i]`: a float copy of `drivetrain.gears[i].ratio`; a change
    /// is handed on by the item's `onValueChanged` (`Drivetrain::setGearRatio`)
    GearSetting(usize),
    /// `drivetrain.finalRatio`
    FinalRatio,
    /// `drivetrain.acEngine.limiterMultiplier`, `coastTorqueMultiplier`
    EngineLimiter,
    CoastTorqueMult,
}

impl SetupTarget {
    fn get(self, chassis: &RollingChassis) -> f32 {
        use SetupTarget::*;
        match self {
            ArbK(a) => chassis.antiroll_bars[a].k,
            TyrePressure(w) => chassis.tyres[w].status.pressure_static,
            DamperBumpFast(w) => chassis.suspensions[w].damper().bump_fast,
            DamperBumpSlow(w) => chassis.suspensions[w].damper().bump_slow,
            DamperReboundFast(w) => chassis.suspensions[w].damper().rebound_fast,
            DamperReboundSlow(w) => chassis.suspensions[w].damper().rebound_slow,
            BumpStopRate(w) => chassis.suspensions[w].base().bump_stop_rate,
            SpringRate(w) => chassis.suspensions[w].base().k,
            ProgressiveSpringRate(w) => chassis.suspensions[w].base().progressive_k,
            RodLength(w) => chassis.suspensions[w].base().rod_length,
            StaticCamber(w) => chassis.suspensions[w].base().static_camber,
            ToeOut(w) => chassis.suspensions[w].base().toe_out_linear,
            PackerRange(w) => chassis.suspensions[w].base().packer_range,
            HeaveDamperBumpFast(a) => chassis.heave_springs[a].damper.bump_fast,
            HeaveDamperBumpSlow(a) => chassis.heave_springs[a].damper.bump_slow,
            HeaveDamperReboundFast(a) => chassis.heave_springs[a].damper.rebound_fast,
            HeaveDamperReboundSlow(a) => chassis.heave_springs[a].damper.rebound_slow,
            HeaveBumpStopRate(a) => chassis.heave_springs[a].bump_stop_rate,
            HeaveSpringRate(a) => chassis.heave_springs[a].k,
            HeaveProgressiveSpringRate(a) => chassis.heave_springs[a].progressive_k,
            HeaveRodLength(a) => chassis.heave_springs[a].rod_length,
            HeavePackerRange(a) => chassis.heave_springs[a].packer_range,
            WingAngle(i) => {
                let wing = &aero(chassis).base().wings[i];
                if wing.data.has_controller {
                    wing.status.input_angle
                } else {
                    wing.status.angle
                }
            }
            SteerAssist => chassis.steer_assist,
            DiffPowerRamp => drivetrain(chassis).base().diff_power_ramp,
            DiffCoastRamp => drivetrain(chassis).base().diff_coast_ramp,
            DiffPreLoad => drivetrain(chassis).base().diff_pre_load,
            BrakePowerMultiplier => brakes(chassis).base().brake_power_multiplier,
            FrontBias => brakes(chassis).base().front_bias,
            GearSetting(_) => unreachable!("the gear items point into the setup manager"),
            FinalRatio => drivetrain(chassis).base().final_ratio,
            EngineLimiter => drivetrain(chassis).engine().base().limiter_multiplier,
            CoastTorqueMult => drivetrain(chassis).engine().base().coast_torque_multiplier,
        }
    }

    fn set(self, chassis: &mut RollingChassis, value: f32) {
        use SetupTarget::*;
        match self {
            ArbK(a) => chassis.antiroll_bars[a].k = value,
            TyrePressure(w) => chassis.tyres[w].status.pressure_static = value,
            DamperBumpFast(w) => chassis.suspensions[w].damper_mut().bump_fast = value,
            DamperBumpSlow(w) => chassis.suspensions[w].damper_mut().bump_slow = value,
            DamperReboundFast(w) => chassis.suspensions[w].damper_mut().rebound_fast = value,
            DamperReboundSlow(w) => chassis.suspensions[w].damper_mut().rebound_slow = value,
            BumpStopRate(w) => chassis.suspensions[w].base_mut().bump_stop_rate = value,
            SpringRate(w) => chassis.suspensions[w].base_mut().k = value,
            ProgressiveSpringRate(w) => chassis.suspensions[w].base_mut().progressive_k = value,
            RodLength(w) => chassis.suspensions[w].base_mut().rod_length = value,
            StaticCamber(w) => chassis.suspensions[w].base_mut().static_camber = value,
            ToeOut(w) => chassis.suspensions[w].base_mut().toe_out_linear = value,
            PackerRange(w) => chassis.suspensions[w].base_mut().packer_range = value,
            HeaveDamperBumpFast(a) => chassis.heave_springs[a].damper.bump_fast = value,
            HeaveDamperBumpSlow(a) => chassis.heave_springs[a].damper.bump_slow = value,
            HeaveDamperReboundFast(a) => chassis.heave_springs[a].damper.rebound_fast = value,
            HeaveDamperReboundSlow(a) => chassis.heave_springs[a].damper.rebound_slow = value,
            HeaveBumpStopRate(a) => chassis.heave_springs[a].bump_stop_rate = value,
            HeaveSpringRate(a) => chassis.heave_springs[a].k = value,
            HeaveProgressiveSpringRate(a) => chassis.heave_springs[a].progressive_k = value,
            HeaveRodLength(a) => chassis.heave_springs[a].rod_length = value,
            HeavePackerRange(a) => chassis.heave_springs[a].packer_range = value,
            WingAngle(i) => {
                let wing = &mut aero_mut(chassis).base_mut().wings[i];
                if wing.data.has_controller {
                    wing.status.input_angle = value;
                } else {
                    wing.status.angle = value;
                }
            }
            SteerAssist => chassis.steer_assist = value,
            DiffPowerRamp => drivetrain_mut(chassis).base_mut().diff_power_ramp = value,
            DiffCoastRamp => drivetrain_mut(chassis).base_mut().diff_coast_ramp = value,
            DiffPreLoad => drivetrain_mut(chassis).base_mut().diff_pre_load = value,
            BrakePowerMultiplier => brakes_mut(chassis).base_mut().brake_power_multiplier = value,
            FrontBias => brakes_mut(chassis).base_mut().front_bias = value,
            GearSetting(_) => unreachable!("the gear items point into the setup manager"),
            FinalRatio => drivetrain_mut(chassis).base_mut().final_ratio = value,
            EngineLimiter => drivetrain_mut(chassis).engine_mut().base_mut().limiter_multiplier = value,
            CoastTorqueMult => drivetrain_mut(chassis).engine_mut().base_mut().coast_torque_multiplier = value,
        }
    }
}

// the items of a system exist only in a car that has the system
fn drivetrain(chassis: &RollingChassis) -> &dyn super::DrivetrainModel {
    chassis.drivetrain.as_deref().expect("a drivetrain setup item without a drivetrain")
}
fn drivetrain_mut(chassis: &mut RollingChassis) -> &mut dyn super::DrivetrainModel {
    chassis.drivetrain.as_deref_mut().expect("a drivetrain setup item without a drivetrain")
}
fn aero(chassis: &RollingChassis) -> &dyn super::AeroModel {
    chassis.aero.as_deref().expect("a wing setup item without an aero model")
}
fn aero_mut(chassis: &mut RollingChassis) -> &mut dyn super::AeroModel {
    chassis.aero.as_deref_mut().expect("a wing setup item without an aero model")
}
fn brakes(chassis: &RollingChassis) -> &dyn super::BrakeModel {
    chassis.brake_system.as_deref().expect("a brake setup item without a brake system")
}
fn brakes_mut(chassis: &mut RollingChassis) -> &mut dyn super::BrakeModel {
    chassis.brake_system.as_deref_mut().expect("a brake setup item without a brake system")
}

/// AC's `SetupItem` (0x88 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct SetupItem {
    /// `name`
    pub name: String,
    /// `connectedFloat`
    pub target: SetupTarget,
    /// `multiplier`: the car's value is `multiplier * newValue`.
    pub multiplier: f32,
    /// `newValue`: the value in the item's own unit.
    pub new_value: f32,
    /// `attached`: only then `SetupManager::step` writes the value.
    pub attached: bool,
    /// `labelMultiplier`: display factor of the setup screen.
    pub label_multiplier: f32,
}

/// One value `SetupManager::step` changed.
#[derive(Clone, Debug, PartialEq)]
pub struct SetupChange {
    pub name: String,
    pub from: f32,
    pub to: f32,
}

/// AC's `SetupManager` (0x50 bytes), the items connected to values of the ported systems.
/// Items of systems that are not ported (the four-wheel-drive differentials, the
/// force-feedback gain) are left out; the items of wings, brakes, drivetrain and engine exist
/// in a car that has those systems.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupManager {
    /// `items`, in the order `SetupManager::initItems` registers them.
    pub items: Vec<SetupItem>,
    /// `gearSettings`: the gear ratios as floats, the targets of the `INTERNAL_GEAR_n` items.
    pub gear_settings: Vec<f32>,
    /// Every value written so far ("Setup change for Car … Changing: … from … to …").
    pub changes: Vec<SetupChange>,
}

const WHEELS: [&str; 4] = ["LF", "RF", "LR", "RR"];

/// `cvttsd2si` / `cvttss2si`, the C cast of the setup screen: toward zero; a NaN and anything
/// outside the range of a 32-bit integer give `i32::MIN`. (`tools/car_oracle` uses Rust's
/// saturating `as i32` at this place; the two agree for every value a car in `cardata`
/// produces and differ only for a NaN or an overflow, e.g. `SHOW_CLICKS=1` with a `STEP`
/// that reads as 0.)
fn cvtt(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `Spinner::setValue` @ 0x14024de60: the value kept within the spinner's range.
fn clamp_position(value: i32, low: i32, high: i32) -> i32 {
    if value > high {
        high
    } else {
        value.max(low)
    }
}

/// The setup screen's whole-number spinner of an item (`SetupTab::addItem` @ 0x140183850
/// with the `SetupItemDef` of `SetupScreen::loadINI` @ 0x14017d950).
struct Spinner {
    /// `[DISPLAY_METHOD]` + `SHOW_CLICKS`: 1 clicks, 2 clicks from the minimum, else plain
    mode: i32,
    /// `STEP`, `MIN` (the latter divided by the item's display factor)
    step: f64,
    min: f64,
    /// The spinner's range.
    low: i32,
    high: i32,
}

impl Spinner {
    /// The spinner of an item, if `setup.ini` has a section of its name.
    fn of(item: &SetupItem, setup: &IniReader, clicks: bool) -> Option<Spinner> {
        let name = item.name.as_str();
        if !setup.has_section(name) {
            return None;
        }
        // a missing key reads as 0, as in the game's INIReader
        let float = |key: &str| setup.get_float(name, key).unwrap_or(0.0);
        let label = item.label_multiplier.abs();
        let min = (float("MIN") / label) as f64;
        let max = (float("MAX") / label) as f64;
        let step = setup.get_int(name, "STEP").unwrap_or(0) as f64;
        let mode = if clicks { setup.get_int(name, "SHOW_CLICKS").unwrap_or(0) } else { 0 };
        // C casts: toward zero
        let (low, high) = match mode {
            1 => (cvtt(min / step), cvtt(max / step)),
            2 => (0, cvtt((max - min) / step)),
            _ => (cvtt(min), cvtt(max)),
        };
        Some(Spinner { mode, step, min, low, high })
    }

    fn clamp(&self, position: i32) -> i32 {
        clamp_position(position, self.low, self.high)
    }

    /// The item's `newValue` for a spinner position (the job @ 0x140183620).
    fn value(&self, position: i32) -> f32 {
        match self.mode {
            1 => self.step as f32 * position as f32,
            2 => self.step as f32 * position as f32 + self.min as f32,
            _ => position as f32,
        }
    }
}

/// The table of a gear's `.rto` file as the setup screen's gear tab reads it
/// (`SetupGearManager::populateSpinner` @ 0x140173810, `loadGearRatiosFromFile`
/// @ 0x14029e7f0): one `label|ratio` per line, up to the first line without a `|`.
pub fn load_gear_ratios(path: &Path) -> Result<Vec<(String, f32)>, String> {
    let Some(bytes) = crate::data::read(path)? else {
        return Ok(Vec::new());
    };
    let text: String = crate::data::ini::text_mode(&bytes).iter().map(|&b| b as char).collect();
    let mut ratios = Vec::new();
    for line in text.split('\n') {
        let Some((label, ratio)) = line.split_once('|') else { break };
        let parsed = crate::math::wcstod(ratio);
        if parsed.consumed == 0 {
            break;
        }
        ratios.push((label.to_string(), parsed.value as f32));
    }
    Ok(ratios)
}

impl SetupManager {
    /// `SetupManager::init` @ 0x140289290 with `SetupManager::initItems` @ 0x140289570 and
    /// `SetupItem::SetupItem` @ 0x1402cb170: every item starts detached, with
    /// `newValue = value / multiplier`. (`[RULES]` of car.ini, the minimum ride height check,
    /// is not ported.)
    pub fn init(chassis: &RollingChassis, _data_path: &Path) -> Result<SetupManager, String> {
        use SetupTarget::*;
        let mut manager = SetupManager::default();
        let mut add = |name: String, target: SetupTarget, multiplier: f32, label_multiplier: f32| {
            let new_value = target.get(chassis) / multiplier;
            manager.items.push(SetupItem { name, target, multiplier, new_value, attached: false, label_multiplier });
        };
        add("ARB_FRONT".into(), ArbK(0), 1.0, 1.0);
        add("ARB_REAR".into(), ArbK(1), 1.0, 1.0);
        add("ARB_FRONT_NMM".into(), ArbK(0), 1000.0, 1.0);
        add("ARB_REAR_NMM".into(), ArbK(1), 1000.0, 1.0);
        if let Some(aero) = &chassis.aero {
            for index in 0..aero.base().wings.len() {
                add(format!("WING_{index}"), WingAngle(index), 1.0, 1.0);
            }
        }
        if let Some(drivetrain) = &chassis.drivetrain {
            // a car with a differential controller has no differential items
            if !drivetrain.has_dynamic_controllers() {
                add("DIFF_POWER".into(), DiffPowerRamp, 0.01, 1.0);
                add("DIFF_COAST".into(), DiffCoastRamp, 0.01, 1.0);
                add("DIFF_PRELOAD".into(), DiffPreLoad, 1.0, 1.0);
            }
        }
        // (the items of the four-wheel-drive differentials are registered here)
        for (w, wheel) in WHEELS.iter().enumerate() {
            add(format!("PRESSURE_{wheel}"), TyrePressure(w), 1.0, 1.0);
        }
        if chassis.brake_system.is_some() {
            add("BRAKE_POWER_MULT".into(), BrakePowerMultiplier, 0.01, 1.0);
            add("FRONT_BIAS".into(), FrontBias, 0.01, 1.0);
        }
        for (w, wheel) in WHEELS.iter().enumerate() {
            add(format!("DAMP_FAST_BUMP_{wheel}"), DamperBumpFast(w), 1.0, 1.0);
            add(format!("DAMP_BUMP_{wheel}"), DamperBumpSlow(w), 1.0, 1.0);
            add(format!("DAMP_FAST_REBOUND_{wheel}"), DamperReboundFast(w), 1.0, 1.0);
            add(format!("DAMP_REBOUND_{wheel}"), DamperReboundSlow(w), 1.0, 1.0);
            add(format!("BUMP_STOP_RATE_{wheel}"), BumpStopRate(w), 1000.0, 1.0);
            add(format!("SPRING_RATE_{wheel}"), SpringRate(w), 1000.0, 1.0);
            add(format!("PROGRESSIVE_SPRING_RATE_{wheel}"), ProgressiveSpringRate(w), 1000.0, 1.0);
            add(format!("ROD_LENGTH_{wheel}"), RodLength(w), 0.0001, 1.0);
            // tenths of a degree; the sign of the stored angle depends on the side
            let camber = if w % 2 == 0 { -0.001_745_329_2 } else { 0.001_745_329_2 };
            add(format!("CAMBER_{wheel}"), StaticCamber(w), camber, 0.1);
            add(format!("TOE_OUT_{wheel}"), ToeOut(w), 0.00001, 1.0);
            add(format!("PACKER_RANGE_{wheel}"), PackerRange(w), 0.001, 1.0);
        }
        for (a, axle) in ["HF", "HR"].iter().enumerate() {
            if chassis.heave_springs[a].is_present {
                add(format!("DAMP_FAST_BUMP_{axle}"), HeaveDamperBumpFast(a), 1.0, 1.0);
                add(format!("DAMP_BUMP_{axle}"), HeaveDamperBumpSlow(a), 1.0, 1.0);
                add(format!("DAMP_FAST_REBOUND_{axle}"), HeaveDamperReboundFast(a), 1.0, 1.0);
                add(format!("DAMP_REBOUND_{axle}"), HeaveDamperReboundSlow(a), 1.0, 1.0);
                add(format!("BUMP_STOP_RATE_{axle}"), HeaveBumpStopRate(a), 1000.0, 1.0);
                add(format!("SPRING_RATE_{axle}"), HeaveSpringRate(a), 1000.0, 1.0);
                add(format!("PROGRESSIVE_SPRING_RATE_{axle}"), HeaveProgressiveSpringRate(a), 1000.0, 1.0);
                add(format!("ROD_LENGTH_{axle}"), HeaveRodLength(a), 0.0001, 1.0);
                add(format!("PACKER_RANGE_{axle}"), HeavePackerRange(a), 0.001, 1.0);
            }
        }
        if let Some(drivetrain) = &chassis.drivetrain {
            // one item per entry of `gears` (reverse and neutral included), attached from the
            // start, as is the final ratio
            let ratios: Vec<f32> = drivetrain.base().gears.iter().map(|gear| gear.ratio as f32).collect();
            for (index, ratio) in ratios.iter().enumerate() {
                manager.items.push(SetupItem {
                    name: format!("INTERNAL_GEAR_{index}"),
                    target: GearSetting(index),
                    multiplier: 1.0,
                    new_value: ratio / 1.0,
                    attached: true,
                    label_multiplier: 1.0,
                });
            }
            manager.gear_settings = ratios;
        }
        let mut add = |name: String, target: SetupTarget, multiplier: f32, label_multiplier: f32, attached: bool| {
            let new_value = target.get(chassis) / multiplier;
            manager.items.push(SetupItem { name, target, multiplier, new_value, attached, label_multiplier });
        };
        if chassis.drivetrain.is_some() {
            add("FINAL_RATIO".into(), FinalRatio, 1.0, 1.0, true);
            add("ENGINE_LIMITER".into(), EngineLimiter, 0.01, 1.0, false);
            add("COAST_TORQUE_MULT".into(), CoastTorqueMult, 0.01, 1.0, false);
        }
        // (`FF_GAIN` is registered here)
        add("STEER_ASSIST".into(), SteerAssist, 0.01, 1.0, false);
        Ok(manager)
    }

    /// What the game's setup screen does to the items when a session starts with the default
    /// setup (`SetupScreen::loadINI` @ 0x14017d950, `SetupTab::addItem` @ 0x140183850 and the
    /// spinner job @ 0x140183620, as `tools/car_oracle` mirrors them): every item with a
    /// section in `setup.ini` is attached and its value goes through the spinner once.
    pub fn apply_setup_screen_defaults(&mut self, _chassis: &RollingChassis, setup: &IniReader) {
        let clicks = setup.has_section("DISPLAY_METHOD");
        for item in &mut self.items {
            let Some(spinner) = Spinner::of(item, setup, clicks) else { continue };
            let value = item.new_value;
            let (step, min) = (spinner.step, spinner.min);
            // the spinner's position (C casts: toward zero)
            let position = match spinner.mode {
                1 => cvtt(value as f64 / step + 0.5),
                2 => cvtt(((value - min as f32) / step as f32 + 0.5f32) as f64),
                _ => cvtt(value as f64),
            };
            item.new_value = spinner.value(spinner.clamp(position));
            item.attached = true;
        }
    }

    /// A saved setup loaded the way the setup screen's "Load" does it
    /// (`SetupScreen::loadSetupAbsolutePath` @ 0x14017f2c0 and the tabs' `loadFromINI`): every
    /// `[NAME] VALUE=n` of the file is the whole-number position of the spinner of that name,
    /// kept within the spinner's range; the spinner turns it into the item's value exactly as
    /// for the default setup. What the file does not name keeps its value. The tabs in the
    /// screen's order: gears (`SetupGearManager::loadFromINI` @ 0x1401734a0), tyres
    /// (`SetupTyresManager` @ 0x140184eb0), fuel (`SetupFuelManager` @ 0x14016e8f0), traction
    /// control (`SetupElectronics` @ 0x14016d050), then the generic items
    /// (`SetupTab::loadFromINI` @ 0x140184120).
    ///
    /// The item values reach the car at the end of the next step ([`SetupManager::step`]); the
    /// compound, the fuel and the traction-control level are set at once, as the screen's jobs
    /// do. Call it after the default round trip and before the first step. Returns one line
    /// per thing set.
    ///
    /// Not ported: gear sets (`[GEARS] USE_GEARSET`), the ABS, turbo, ERS and engine-brake
    /// spinners, the pit-stop presets (`.sp`).
    pub fn load_setup_file(&mut self, chassis: &mut RollingChassis, setup: &IniReader, saved: &IniReader) -> Result<Vec<String>, String> {
        let mut log = Vec::new();
        if !saved.ready {
            return Err(format!("{}: the setup file cannot be read", saved.filename.display()));
        }
        // a missing VALUE reads as 0, as in the game's INIReader
        let value_of = |name: &str| saved.get_int(name, "VALUE").unwrap_or(0);
        // the gear tab: VALUE is a line of the gear's .rto file
        if chassis.drivetrain.is_some() {
            if setup.get_int("GEARS", "USE_GEARSET").unwrap_or(0) > 0 && saved.has_section("GEARSET") {
                return Err("the setup chooses a gear set ([GEARSET]): gear sets are not ported".to_string());
            }
            for item in &mut self.items {
                let section = match item.target {
                    SetupTarget::GearSetting(gear) if gear >= 2 => format!("GEAR_{}", gear - 1),
                    SetupTarget::FinalRatio => "FINAL_GEAR_RATIO".to_string(),
                    _ => continue,
                };
                if !setup.has_section(&section) || !saved.has_section(&item.name) {
                    continue;
                }
                let file = setup.get_string(&section, "RATIOS");
                let ratios = load_gear_ratios(&append_path(&chassis.data_path, &file))?;
                if ratios.is_empty() {
                    continue;
                }
                let wanted = value_of(&item.name);
                let line = clamp_position(wanted, 0, ratios.len() as i32 - 1);
                item.new_value = ratios[line as usize].1;
                log.push(format!("{} = line {line} of {file}: {} ({})", item.name, ratios[line as usize].0, item.new_value));
            }
        }
        // the tyres tab: VALUE is the compound's index
        if saved.has_section("TYRES") {
            let count = chassis.tyres.first().map_or(0, |tyre| tyre.compound_defs.len()) as i32;
            if count > 0 {
                let index = clamp_position(value_of("TYRES"), 0, count - 1);
                chassis.set_compound(index)?;
                log.push(format!("TYRES = compound {index} ({})", chassis.tyres[0].compound_defs[index as usize].name));
            }
        }
        // the fuel tab: whole litres, at most the tank (`Car::setRequestedFuel(v, true)`)
        if saved.has_section("FUEL") {
            let litres = clamp_position(value_of("FUEL"), 0, cvtt(chassis.max_fuel)) as f32;
            chassis.fuel = litres as f64;
            chassis.requested_fuel = litres;
            log.push(format!("FUEL = {litres} l"));
        }
        // the electronics tab: the traction-control level, stepped to from the current one
        if saved.has_section("TRACTION_CONTROL") {
            if let Some(aids) = &mut chassis.aids {
                let tc = &mut aids.base_mut().traction_control;
                let (current, levels) = tc.get_current_mode();
                if tc.is_present && levels > 0 {
                    let wanted = clamp_position(value_of("TRACTION_CONTROL"), 0, levels as i32);
                    let steps = (wanted as f32 - current as f32) as i32;
                    for _ in 0..steps.abs() {
                        tc.cycle_mode(if steps < 0 { -1 } else { 1 });
                    }
                    log.push(format!("TRACTION_CONTROL = level {wanted} of {levels} (was {current})"));
                }
            }
        }
        // the generic tabs
        let clicks = setup.has_section("DISPLAY_METHOD");
        for item in &mut self.items {
            let Some(spinner) = Spinner::of(item, setup, clicks) else { continue };
            if !saved.has_section(&item.name) {
                continue;
            }
            let wanted = value_of(&item.name);
            let position = spinner.clamp(wanted);
            item.new_value = spinner.value(position);
            item.attached = true;
            let note = if position != wanted { format!(" (the file says {wanted}: outside {}..{})", spinner.low, spinner.high) } else { String::new() };
            log.push(format!("{} = {position}{note}", item.name));
        }
        Ok(log)
    }

    /// `SetupManager::step` @ 0x14028d090: for every attached item, `multiplier * newValue`
    /// is written to the car when it differs from the value there. A changed rear toe also
    /// reseats that wheel's steering rod (the items' `onValueChanged`, 0x140288f00 and
    /// 0x140288ee0); a changed gear item hands its `newValue` to `Drivetrain::setGearRatio`
    /// (lambda 0x140288f20).
    pub fn step(&mut self, chassis: &mut RollingChassis) {
        for item in &self.items {
            let value = item.multiplier * item.new_value;
            if !item.attached {
                continue;
            }
            if let SetupTarget::GearSetting(index) = item.target {
                let current = self.gear_settings[index];
                if value < current || value > current {
                    self.changes.push(SetupChange { name: item.name.clone(), from: current, to: value });
                    self.gear_settings[index] = value;
                    if let Some(drivetrain) = &mut chassis.drivetrain {
                        drivetrain.set_gear_ratio(index as i32, item.new_value);
                    }
                }
                continue;
            }
            let current = item.target.get(chassis);
            // `ucomiss` + `je`: nothing happens when they are equal or one is a NaN
            if !(value < current || value > current) {
                continue;
            }
            self.changes.push(SetupChange { name: item.name.clone(), from: current, to: value });
            item.target.set(chassis, value);
            if let SetupTarget::ToeOut(wheel @ (2 | 3)) = item.target {
                chassis.suspensions[wheel].set_steer_length_offset(&mut chassis.core, 0.0);
            }
        }
    }
}
