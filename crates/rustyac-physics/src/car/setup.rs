//! AC's `SetupManager` for the values that live in the chassis: every setup item is a name,
//! a pointer to one float of the car and a multiplier; `SetupManager::step` (late in every
//! physics step) writes `multiplier * newValue` into the float whenever the two differ.
//!
//! Also here: what the game's setup screen does to the items when a session starts with the
//! default setup. The screen shows every item that has a section in the car's `setup.ini` as
//! a spinner holding a whole number, sets the spinner from the car's value and writes the
//! spinner's value back, so a value that is not on the spinner's grid comes back changed
//! (the F2004's 3.0 degrees of front camber become 2.9).

use std::path::Path;

use super::chassis::RollingChassis;
use crate::data::ini::IniReader;

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
    /// `Car::steerAssist`
    SteerAssist,
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
            SteerAssist => chassis.steer_assist,
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
            SteerAssist => chassis.steer_assist = value,
        }
    }
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

/// AC's `SetupManager` (0x50 bytes), the items connected to chassis values. Items of systems
/// that are not ported (wings, differential, brakes, gears, engine) are left out; they come
/// with their systems.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupManager {
    /// `items`, in the order `SetupManager::initItems` registers them.
    pub items: Vec<SetupItem>,
    /// Every value written so far ("Setup change for Car … Changing: … from … to …").
    pub changes: Vec<SetupChange>,
}

const WHEELS: [&str; 4] = ["LF", "RF", "LR", "RR"];

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
        for (w, wheel) in WHEELS.iter().enumerate() {
            add(format!("PRESSURE_{wheel}"), TyrePressure(w), 1.0, 1.0);
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
        add("STEER_ASSIST".into(), SteerAssist, 0.01, 1.0);
        Ok(manager)
    }

    /// What the game's setup screen does to the items when a session starts with the default
    /// setup (`SetupScreen::loadINI` @ 0x14017d950, `SetupTab::addItem` @ 0x140183850 and the
    /// spinner job @ 0x140183620, as `tools/car_oracle` mirrors them): every item with a
    /// section in `setup.ini` is attached and its value goes through the spinner once.
    pub fn apply_setup_screen_defaults(&mut self, _chassis: &RollingChassis, setup: &IniReader) {
        let clicks = setup.has_section("DISPLAY_METHOD");
        for item in &mut self.items {
            let name = item.name.as_str();
            if !setup.has_section(name) {
                continue;
            }
            // a missing key reads as 0, as in the game's INIReader
            let float = |key: &str| setup.get_float(name, key).unwrap_or(0.0);
            let label = item.label_multiplier.abs();
            let min = (float("MIN") / label) as f64;
            let max = (float("MAX") / label) as f64;
            let step = setup.get_int(name, "STEP").unwrap_or(0) as f64;
            let mode = if clicks { setup.get_int(name, "SHOW_CLICKS").unwrap_or(0) } else { 0 };
            let value = item.new_value;
            // the spinner's range and position (C casts: toward zero)
            let (low, high, position) = match mode {
                1 => ((min / step) as i32, (max / step) as i32, (value as f64 / step + 0.5) as i32),
                2 => (0, ((max - min) / step) as i32, ((value - min as f32) / step as f32 + 0.5f32) as i32),
                _ => (min as i32, max as i32, value as i32),
            };
            let position = if position > high { high } else { position.max(low) };
            item.new_value = match mode {
                1 => step as f32 * position as f32,
                2 => step as f32 * position as f32 + min as f32,
                _ => position as f32,
            };
            item.attached = true;
        }
    }

    /// `SetupManager::step` @ 0x14028d090: for every attached item, `multiplier * newValue`
    /// is written to the car when it differs from the value there. A changed rear toe also
    /// reseats that wheel's steering rod (the items' `onValueChanged`, 0x140288f00 and
    /// 0x140288ee0).
    pub fn step(&mut self, chassis: &mut RollingChassis) {
        for item in &self.items {
            let value = item.multiplier * item.new_value;
            if !item.attached {
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
