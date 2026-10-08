// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The drivetrain slot ([`DrivetrainModel`]) and AC's `Drivetrain` as its Vanilla implementation
//! ([`VanillaDrivetrain`]): clutch, gearbox with its shift timing, differential, and the
//! integration of engine speed and driven-wheel speed, for two driven wheels (`RWD`, `FWD`) and
//! for four (`AWD`: three differentials; `AWD2`: a rear-wheel drive with a coupling to the
//! front axle).
//!
//! The tyres do not spin the driven wheels up themselves. `Tyre::step` only leaves the net
//! torque on each driven wheel in `status.feedbackTorque` (road reaction, brake, rolling
//! drag); the drivetrain adds the engine, turns the whole block of engine, gearbox and
//! wheels, lets the differential decide how far the two wheels may differ, and writes the
//! wheel speeds back for the tyres' next step.
//!
//! Speeds and inertias are doubles, torques from the tyres and most parameters are floats;
//! every conversion below sits where the machine code has it.

use std::path::Path;

use super::chassis::RollingChassis;
use super::dynamic_controller::{CarSignals, DynamicController};
use super::engine::{EngineModel, SACEngineInput};
use super::replay::TraceValue;
use crate::data::ini::IniReader;
use crate::math::powf;

/// AC's `TractionType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum TractionType {
    #[default]
    Rwd = 0,
    Fwd = 1,
    /// Three differentials (`step4WD`).
    Awd = 2,
    /// Rear drive with a coupling to the front axle (`step4WD_new`, `TYPE=AWD2`).
    AwdNew = 3,
}

/// AC's `DifferentialType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum DifferentialType {
    #[default]
    Lsd = 0,
    Spool = 1,
}

/// AC's `GearChangeRequest`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum GearChangeRequest {
    #[default]
    None = 0,
    Up = 1,
    Down = 2,
}

/// AC's `GearElement` (0x18 bytes): one rotating part.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GearElement {
    /// `velocity`, rad/s
    pub velocity: f64,
    /// `inertia`, kg m^2
    pub inertia: f64,
    /// `oldVelocity`: the speed at the top of the last `Drivetrain::step` (out-shafts only)
    pub old_velocity: f64,
}

/// AC's `DifferentialSetting` (0x10 bytes) of the `AWD` layout's three differentials. Its
/// fourth member, `type`, is never read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DifferentialSetting {
    /// `power`, `coast`: fraction of the input torque that locks
    pub power: f32,
    pub coast: f32,
    /// `preload`, Nm
    pub preload: f32,
}

/// AC's `AWD2Data` (0x18 bytes): the coupling between the axles of an `AWD2` car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Awd2Data {
    /// `ramp`: Nm per rad/s the rear axle turns faster than the front
    pub ramp: f64,
    /// `maxTorque`, Nm: the most the coupling passes on; `ctrl_awd2.ini` rewrites it every step
    pub max_torque: f64,
    /// `currentLockTorque`: what it passed on in the last step
    pub current_lock_torque: f32,
}

/// AC's `SGearRatio` (0x28 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SGearRatio {
    pub ratio: f64,
    pub name: String,
}

/// AC's `GearRequestStatus` (0x20 bytes): a paddle shift in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GearRequestStatus {
    pub request: GearChangeRequest,
    /// `timeAccumulator`, s
    pub time_accumulator: f64,
    /// `timeout`: how long the box stays in neutral, s
    pub timeout: f64,
    /// `requestedGear`
    pub requested_gear: i32,
}

/// AC's `DownshiftProtection` (0xc bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownshiftProtection {
    pub is_active: bool,
    /// `isDebug`: prints a line for every refused down-shift
    pub is_debug: bool,
    /// `overrev`: rpm above the limiter a down-shift may reach
    pub overrev: i32,
    /// `lockN`: no neutral above 2 km/h
    pub lock_n: bool,
}

/// AC's `OnGearRequestEvent` (8 bytes): what the handlers of `evOnGearRequest` (the automatic
/// clutch, then the automatic throttle blip) are told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnGearRequestEvent {
    pub request: GearChangeRequest,
    /// `nextGear`
    pub next_gear: i32,
}

/// The data members of AC's `Drivetrain` (0x648 bytes) for two driven wheels: what the shift
/// helpers, the controllers, the setup items and the telemetry read or write.
#[derive(Clone, Debug, PartialEq)]
pub struct DrivetrainBase {
    /// `isGearGrinding` (+0x000)
    pub is_gear_grinding: bool,
    /// `finalRatio` (+0x004)
    pub final_ratio: f32,
    /// `engine` (+0x008): the crankshaft
    pub engine: GearElement,
    /// `drive` (+0x020): the differential carrier, wheel-side rad/s
    pub drive: GearElement,
    /// `outShaftL`, `outShaftR` (+0x038, +0x050): the two driven half shafts (the rear ones of
    /// a four-wheel-drive car)
    pub out_shaft_l: GearElement,
    pub out_shaft_r: GearElement,
    /// `outShaftLF`, `outShaftRF` (+0x068, +0x080): the front half shafts of a four-wheel-drive
    /// car (their inertia stays 0 otherwise)
    pub out_shaft_lf: GearElement,
    pub out_shaft_rf: GearElement,
    /// `gears` (+0x098): `[0]` reverse, `[1]` neutral (ratio 0), `[2]` first ...
    pub gears: Vec<SGearRatio>,
    /// `rootVelocity` (+0x0b0): the gearbox input shaft (clutch output), engine-side rad/s
    pub root_velocity: f64,
    /// `clutchOpenState` (+0x0b8): the clutch slips (or is open)
    pub clutch_open_state: bool,
    /// `ratio` (+0x0c0): `finalRatio * gears[currentGear].ratio`; exactly 0 in neutral
    pub ratio: f64,
    /// `diffPowerRamp`, `diffCoastRamp`, `diffPreLoad` (+0x0c8)
    pub diff_power_ramp: f32,
    pub diff_coast_ramp: f32,
    pub diff_pre_load: f32,
    /// `diffType` (+0x0d4)
    pub diff_type: DifferentialType,
    /// `cutOff` (+0x0d8): seconds of throttle cut left after an up-shift
    pub cut_off: f64,
    /// `isShifterSupported` (+0x4c8)
    pub is_shifter_supported: bool,
    /// `clutchMaxTorque` (+0x4d0), Nm
    pub clutch_max_torque: f64,
    /// `totalTorque` (+0x4d8): display value
    pub total_torque: f32,
    /// `awdFrontShare` (+0x4dc): the front axle's share of the engine's torque (`AWD`), 0..1
    pub awd_front_share: f32,
    /// `awdFrontDiff`, `awdRearDiff`, `awdCenterDiff` (+0x4e0, +0x4f0, +0x500): `AWD`
    pub awd_front_diff: DifferentialSetting,
    pub awd_rear_diff: DifferentialSetting,
    pub awd_center_diff: DifferentialSetting,
    /// `awd2` (+0x520): `AWD2`
    pub awd2: Awd2Data,
    /// `downshiftProtection` (+0x510)
    pub downshift_protection: DownshiftProtection,
    /// `currentClutchTorque` (+0x538)
    pub current_clutch_torque: f32,
    /// `tractionType` (+0x580)
    pub traction_type: TractionType,
    /// `currentGear` (+0x584): index into `gears`; 1 (neutral) while a paddle shift runs
    pub current_gear: i32,
    /// `lastRatio` (+0x588)
    pub last_ratio: f64,
    /// `tyreLeft`, `tyreRight` (+0x590): the driven tyres, as indices into `Car::tyres`
    pub tyre_left: usize,
    pub tyre_right: usize,
    /// `gearRequest` (+0x5a0)
    pub gear_request: GearRequestStatus,
    /// `gearUpTime`, `gearDnTime` (+0x5c0), s
    pub gear_up_time: f64,
    pub gear_dn_time: f64,
    /// `autoCutOffTime` (+0x5d0), s
    pub auto_cut_off_time: f64,
    /// `validShiftRPMWindow` (+0x5d8): H-shifter tolerance; shrinks with gearbox wear
    pub valid_shift_rpm_window: f64,
    /// `controlsWindowGain` (+0x5e0)
    pub controls_window_gain: f64,
    /// `damageRpmWindow` (+0x600)
    pub damage_rpm_window: f64,
    /// `orgRpmWindow` (+0x608): the raw `VALID_SHIFT_RPM_WINDOW`
    pub org_rpm_window: f64,
    /// `clutchInertia` (+0x640)
    pub clutch_inertia: f32,
    /// `locClutch` (+0x644): `powf(controls.clutch, 1.5)` of this step
    pub loc_clutch: f32,
}

/// The drivetrain slot: AC's `Drivetrain` as the rest of the car sees it.
pub trait DrivetrainModel {
    /// `Drivetrain::step` @ 0x14026b130: the engine runs inside it; leaves the speed and the
    /// spin matrix of the driven wheels in the tyres.
    fn step(&mut self, car: &mut RollingChassis, dt: f32);
    /// `Drivetrain::gearUp` @ 0x1402669f0. The gear-request handlers of the car run inside.
    fn gear_up(&mut self, car: &mut RollingChassis) -> bool;
    /// `Drivetrain::gearDown` @ 0x140266660.
    fn gear_down(&mut self, car: &mut RollingChassis) -> bool;
    /// `Drivetrain::setCurrentGear` @ 0x1402692e0 (H-shifter, spawn).
    fn set_current_gear(&mut self, index: i32, force: bool, car: &RollingChassis);
    /// `Drivetrain::setGearRatio` @ 0x140269470 (the gear setup items).
    fn set_gear_ratio(&mut self, index: i32, ratio: f32);
    /// `Drivetrain::reset` @ 0x140269280 (`Car::forcePosition`).
    fn reset(&mut self);
    /// `Drivetrain::getEngineRPM` @ 0x140266b80: single precision from the start.
    fn get_engine_rpm(&self) -> f32;
    /// `Drivetrain::isChangingGear` @ 0x1402673b0
    fn is_changing_gear(&self) -> bool;
    /// `Drivetrain::hasDynamicControllers` @ 0x140266d90
    fn has_dynamic_controllers(&self) -> bool;
    /// `Drivetrain::projectRPMAtDownshift` @ 0x140268ee0
    fn project_rpm_at_downshift(&self) -> f32;
    /// `Drivetrain::acEngine`: the engine slot.
    fn engine(&self) -> &dyn EngineModel;
    fn engine_mut(&mut self) -> &mut dyn EngineModel;
    fn base(&self) -> &DrivetrainBase;
    fn base_mut(&mut self) -> &mut DrivetrainBase;
    /// The values `tools/car_oracle` records of drivetrain and engine, under its names.
    fn trace(&self, out: &mut Vec<TraceValue>);
    /// Everything that changes from step to step, for a test that starts in mid-run.
    fn save_state(&self, out: &mut Vec<u32>);
    /// The inverse of [`DrivetrainModel::save_state`].
    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String>;
}

/// `0.15915507152579872`: rad/s to revolutions per second, the double literal of the gear
/// logic (not the widened float of [`VanillaDrivetrain::get_engine_rpm`]).
pub const K2PI: f64 = f64::from_bits(0x3fc4_5f31_8199_117e);
/// `1.0 / (double)0.003f`: a constant of the down-shift protection, not `1 / dt`.
const INV_DT: f64 = f64::from_bits(0x4074_d555_524b_8e39);
/// Floats the compiler widened at build time.
const F64C_0_1F: f64 = f64::from_bits(0x3fb9_9999_a000_0000);
const F64C_0_15F: f64 = f64::from_bits(0x3fc3_3333_4000_0000);
const F64C_0_01F: f64 = f64::from_bits(0x3f84_7ae1_4000_0000);
const F64C_0_003F: f64 = f64::from_bits(0x3f68_9374_c000_0000);

/// `cvttss2si`: toward zero; a NaN or a value outside the range gives `i32::MIN`.
pub fn cvttss2si(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `ucomisd x, 0` + `jne`: not zero and a number.
fn ordered_nonzero(x: f64) -> bool {
    x < 0.0 || x > 0.0
}

/// `PhysicsEngine::hasSessionStarted` @ 0x140263c70.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn has_session_started(car: &RollingChassis, after_ms: f64) -> bool {
    !((after_ms + car.env.session_start_time_ms) >= car.physics_time)
}

/// `Drivetrain::isGearboxLocked` @ 0x1402673c0: the race rules hold the gearbox on the grid.
pub fn is_gearbox_locked(car: &RollingChassis) -> bool {
    if has_session_started(car, 0.0) {
        return car.is_gearbox_locked;
    }
    if car.env.session_start_time_ms - car.env.lock_gearbox_at_start_time_ms > car.physics_time {
        return true;
    }
    // `eLockOnGridMode`
    car.env.jump_start_penalty_mode == 0
}

/// AC's `Drivetrain` with its engine slot. The body reaction torque of a rigid rear axle is
/// not ported (the chassis has no such axle).
pub struct VanillaDrivetrain {
    pub base: DrivetrainBase,
    /// `acEngine` (+0x0e0)
    pub ac_engine: Box<dyn EngineModel>,
    /// `controllers.singleDiffLock` (`ctrl_single_lock.ini`, rear-wheel drive only): its
    /// output is the differential's locking torque
    pub single_diff_lock: Option<DynamicController>,
    /// `controllers.awdFrontShare` (`ctrl_awd_front_share.ini`, `AWD` only): its output is
    /// `awdFrontShare`
    pub awd_front_share_ctrl: Option<DynamicController>,
    /// `controllers.awdCenterLock` (`ctrl_awd_center_lock.ini`, `AWD` only): its output is the
    /// centre differential's preload
    pub awd_center_lock: Option<DynamicController>,
    /// `controllers.awd2` (`ctrl_awd2.ini`, `AWD2` only): its output is `awd2.maxTorque`
    pub awd2_ctrl: Option<DynamicController>,
}

impl VanillaDrivetrain {
    /// `Drivetrain::Drivetrain` @ 0x14026d870, `Drivetrain::init` @ 0x140266dc0 and
    /// `Drivetrain::loadINI` @ 0x140267440 for the car `car` (its tyres must exist: the wheel
    /// inertias are copied from them here, once, and their `driven` flags are set).
    /// `engine` is the already initialised engine (`Engine::init`).
    pub fn new(car: &mut RollingChassis, engine: Box<dyn EngineModel>) -> Result<VanillaDrivetrain, String> {
        let data_path = car.data_path.clone();
        let base = DrivetrainBase {
            is_gear_grinding: false,
            final_ratio: 4.0,
            engine: GearElement { velocity: 0.0, inertia: F64C_0_01F, old_velocity: 0.0 },
            drive: GearElement { velocity: 0.0, inertia: F64C_0_01F, old_velocity: 0.0 },
            out_shaft_l: GearElement { velocity: 0.0, inertia: car.tyres[2].data.angular_inertia as f64, old_velocity: 0.0 },
            out_shaft_r: GearElement { velocity: 0.0, inertia: car.tyres[3].data.angular_inertia as f64, old_velocity: 0.0 },
            out_shaft_lf: GearElement::default(),
            out_shaft_rf: GearElement::default(),
            gears: Vec::new(),
            root_velocity: 0.0,
            clutch_open_state: false,
            ratio: 12.0,
            diff_power_ramp: 0.7,
            diff_coast_ramp: 0.2,
            diff_pre_load: 0.0,
            diff_type: DifferentialType::Lsd,
            cut_off: 0.0,
            is_shifter_supported: false,
            clutch_max_torque: 0.0,
            total_torque: 0.0,
            // 0.1 in the constructor, 0.3 in `init`
            awd_front_share: 0.3,
            awd_front_diff: DifferentialSetting::default(),
            awd_rear_diff: DifferentialSetting::default(),
            awd_center_diff: DifferentialSetting::default(),
            awd2: Awd2Data { ramp: 20.0, max_torque: 800.0, current_lock_torque: 0.0 },
            downshift_protection: DownshiftProtection { is_active: false, is_debug: false, overrev: 0, lock_n: true },
            current_clutch_torque: 0.0,
            traction_type: TractionType::Rwd,
            current_gear: 0,
            last_ratio: -1.0,
            tyre_left: 2,
            tyre_right: 3,
            gear_request: GearRequestStatus {
                request: GearChangeRequest::None,
                time_accumulator: 0.0,
                timeout: 200.0,
                requested_gear: -1,
            },
            gear_up_time: F64C_0_1F,
            gear_dn_time: F64C_0_15F,
            auto_cut_off_time: 0.0,
            valid_shift_rpm_window: 0.0,
            controls_window_gain: 0.0,
            damage_rpm_window: 0.0,
            org_rpm_window: 0.0,
            clutch_inertia: 1.0,
            loc_clutch: 1.0,
        };
        let mut drivetrain = VanillaDrivetrain {
            base,
            ac_engine: engine,
            single_diff_lock: None,
            awd_front_share_ctrl: None,
            awd_center_lock: None,
            awd2_ctrl: None,
        };
        drivetrain.load_ini(car, &data_path)?;
        // Drivetrain::initControllers @ 0x140267070: one file for rear-wheel drive, two for
        // `AWD` (the front share first), none for the others (an `AWD2` car's
        // ctrl_single_lock.ini is not opened; its ctrl_awd2.ini was opened by the loader)
        let controller = |name: &str| -> Result<Option<DynamicController>, String> {
            let path = data_path.join(name);
            if crate::data::exists(&path) {
                Ok(Some(DynamicController::load(&path)?))
            } else {
                Ok(None)
            }
        };
        match drivetrain.base.traction_type {
            TractionType::Rwd => drivetrain.single_diff_lock = controller("ctrl_single_lock.ini")?,
            TractionType::Awd => {
                drivetrain.awd_front_share_ctrl = controller("ctrl_awd_front_share.ini")?;
                drivetrain.awd_center_lock = controller("ctrl_awd_center_lock.ini")?;
            }
            TractionType::Fwd | TractionType::AwdNew => {}
        }
        Ok(drivetrain)
    }

    /// `Drivetrain::loadINI` @ 0x140267440, in the loader's order.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn load_ini(&mut self, car: &mut RollingChassis, data_path: &Path) -> Result<(), String> {
        let ini = IniReader::load(&data_path.join("drivetrain.ini"))?;
        let b = &mut self.base;
        if ini.has_section("HEADER") && ini.get_int("HEADER", "VERSION")? >= 2 && ini.has_section("DAMAGE") {
            b.damage_rpm_window = ini.get_float("DAMAGE", "RPM_WINDOW_K")? as f64;
        }
        let count = ini.get_int("GEARS", "COUNT")?;
        b.gears.push(SGearRatio { ratio: ini.get_float("GEARS", "GEAR_R")? as f64, name: "R".into() });
        b.gears.push(SGearRatio { ratio: 0.0, name: "N".into() });
        for i in 1..=count {
            let ratio = ini.get_float("GEARS", &format!("GEAR_{i}"))? as f64;
            b.gears.push(SGearRatio { ratio, name: i.to_string() });
        }
        b.final_ratio = ini.get_float("GEARS", "FINAL")?;
        // setCurrentGear(1, true)
        b.is_gear_grinding = false;
        b.current_gear = 1;
        b.diff_power_ramp = ini.get_float("DIFFERENTIAL", "POWER")?;
        b.diff_coast_ramp = ini.get_float("DIFFERENTIAL", "COAST")?;
        // two `comiss 1.0, ramp` + `ja`: a spool when neither ramp is below 1
        if !(1.0 > b.diff_power_ramp) && !(1.0 > b.diff_coast_ramp) {
            b.diff_type = DifferentialType::Spool;
        }
        b.diff_pre_load = ini.get_float("DIFFERENTIAL", "PRELOAD")?;
        let traction = ini.get_string("TRACTION", "TYPE");
        match traction.as_str() {
            "FWD" => {
                b.traction_type = TractionType::Fwd;
                b.tyre_left = 0;
                b.tyre_right = 1;
                b.out_shaft_l.inertia = car.tyres[0].data.angular_inertia as f64;
                b.out_shaft_r.inertia = car.tyres[1].data.angular_inertia as f64;
                for (tyre, driven) in car.tyres.iter_mut().zip([true, true, false, false]) {
                    tyre.driven = driven;
                }
            }
            "RWD" => {
                b.traction_type = TractionType::Rwd;
                for (tyre, driven) in car.tyres.iter_mut().zip([false, false, true, true]) {
                    tyre.driven = driven;
                }
            }
            "AWD" => {
                b.traction_type = TractionType::Awd;
                for tyre in car.tyres.iter_mut() {
                    tyre.driven = true;
                }
                // the four wheels get the mean of the rear left's and the front left's
                // inertia: written into the TYRES, and the shafts read it back from there
                let mean = (car.tyres[2].data.angular_inertia + car.tyres[0].data.angular_inertia) * 0.5;
                for tyre in car.tyres.iter_mut() {
                    tyre.data.angular_inertia = mean;
                }
                b.out_shaft_lf.inertia = car.tyres[0].data.angular_inertia as f64;
                b.out_shaft_rf.inertia = car.tyres[1].data.angular_inertia as f64;
                b.out_shaft_l.inertia = car.tyres[2].data.angular_inertia as f64;
                b.out_shaft_r.inertia = car.tyres[3].data.angular_inertia as f64;
                // percent; the keys say CENTRE, the setup items CENTER
                b.awd_front_share = ini.get_float("AWD", "FRONT_SHARE")? * 0.01;
                b.awd_front_diff.power = ini.get_float("AWD", "FRONT_DIFF_POWER")?;
                b.awd_front_diff.coast = ini.get_float("AWD", "FRONT_DIFF_COAST")?;
                b.awd_front_diff.preload = ini.get_float("AWD", "FRONT_DIFF_PRELOAD")?;
                b.awd_rear_diff.power = ini.get_float("AWD", "REAR_DIFF_POWER")?;
                b.awd_rear_diff.coast = ini.get_float("AWD", "REAR_DIFF_COAST")?;
                b.awd_rear_diff.preload = ini.get_float("AWD", "REAR_DIFF_PRELOAD")?;
                b.awd_center_diff.power = ini.get_float("AWD", "CENTRE_DIFF_POWER")?;
                b.awd_center_diff.coast = ini.get_float("AWD", "CENTRE_DIFF_COAST")?;
                b.awd_center_diff.preload = ini.get_float("AWD", "CENTRE_DIFF_PRELOAD")?;
            }
            "AWD2" => {
                b.traction_type = TractionType::AwdNew;
                for tyre in car.tyres.iter_mut() {
                    tyre.driven = true;
                }
                b.out_shaft_lf.inertia = car.tyres[0].data.angular_inertia as f64;
                b.out_shaft_rf.inertia = car.tyres[1].data.angular_inertia as f64;
                b.out_shaft_l.inertia = car.tyres[2].data.angular_inertia as f64;
                b.out_shaft_r.inertia = car.tyres[3].data.angular_inertia as f64;
                // read, never used: the front axle of this layout has no locking
                b.awd_front_diff.power = ini.get_float("AWD2", "FRONT_DIFF_POWER")?;
                b.awd_front_diff.coast = ini.get_float("AWD2", "FRONT_DIFF_COAST")?;
                b.awd_front_diff.preload = ini.get_float("AWD2", "FRONT_DIFF_PRELOAD")?;
                // the rear differential is the 2WD one; the spool test above was made on
                // [DIFFERENTIAL] and is not made again
                b.diff_power_ramp = ini.get_float("AWD2", "REAR_DIFF_POWER")?;
                b.diff_coast_ramp = ini.get_float("AWD2", "REAR_DIFF_COAST")?;
                b.diff_pre_load = ini.get_float("AWD2", "REAR_DIFF_PRELOAD")?;
                b.awd2.ramp = ini.get_float("AWD2", "CENTRE_RAMP_TORQUE")? as f64;
                b.awd2.max_torque = ini.get_float("AWD2", "CENTRE_MAX_TORQUE")? as f64;
                let path = data_path.join("ctrl_awd2.ini");
                if crate::data::exists(&path) {
                    self.awd2_ctrl = Some(DynamicController::load(&path)?);
                }
            }
            _ => {
                return Err(format!(
                    "{}: [TRACTION] TYPE={traction:?} (the game reports \"TRACTION NOT FOUND\")",
                    ini.filename.display()
                ))
            }
        }
        // milliseconds to seconds: a float product widened ...
        b.gear_up_time = (ini.get_float("GEARBOX", "CHANGE_UP_TIME")? * 0.001) as f64;
        b.gear_dn_time = (ini.get_float("GEARBOX", "CHANGE_DN_TIME")? * 0.001) as f64;
        if !ordered_nonzero(b.gear_dn_time) {
            b.gear_dn_time = 0.15;
        }
        if !ordered_nonzero(b.gear_up_time) {
            b.gear_up_time = 0.1;
        }
        // ... and a widened float times the double 0.001
        b.auto_cut_off_time = ini.get_float("GEARBOX", "AUTO_CUTOFF_TIME")? as f64 * 0.001;
        b.is_shifter_supported = ini.get_int("GEARBOX", "SUPPORTS_SHIFTER")? != 0;
        if ini.has_section("DOWNSHIFT_PROTECTION") && !b.is_shifter_supported {
            b.downshift_protection.is_active = ini.get_int("DOWNSHIFT_PROTECTION", "ACTIVE")? != 0;
            let _ = ini.get_int("DOWNSHIFT_PROTECTION", "DEBUG")?;
            b.downshift_protection.overrev = ini.get_int("DOWNSHIFT_PROTECTION", "OVERREV")?;
            b.downshift_protection.lock_n = ini.get_int("DOWNSHIFT_PROTECTION", "LOCK_N")? != 0;
        } else {
            b.downshift_protection.is_active = !b.is_shifter_supported;
        }
        // whatever the file says: the first car talks
        b.downshift_protection.is_debug = car.env.is_first_car;
        b.clutch_max_torque = ini.get_float("CLUTCH", "MAX_TORQUE")? as f64;
        if !ordered_nonzero(b.clutch_max_torque) {
            b.clutch_max_torque = 450.0;
        }
        b.valid_shift_rpm_window = ini.get_float("GEARBOX", "VALID_SHIFT_RPM_WINDOW")? as f64;
        // the raw value, before the fall-back
        b.org_rpm_window = b.valid_shift_rpm_window;
        b.controls_window_gain = ini.get_float("GEARBOX", "CONTROLS_WINDOW_GAIN")? as f64;
        if !ordered_nonzero(b.valid_shift_rpm_window) {
            b.valid_shift_rpm_window = 500.0;
        }
        let inertia = ini.get_float("GEARBOX", "INERTIA")?;
        if inertia < 0.0 || inertia > 0.0 {
            b.clutch_inertia = inertia;
            b.drive.inertia = inertia as f64;
        }
        Ok(())
    }

    /// The four-wheel-drive controllers the car has, under the names of the recordings.
    fn awd_controllers(&self) -> impl Iterator<Item = (&'static str, &DynamicController)> {
        [("awdFrontShare", &self.awd_front_share_ctrl), ("awdCenterLock", &self.awd_center_lock), ("awd2", &self.awd2_ctrl)]
            .into_iter()
            .filter_map(|(name, controller)| controller.as_ref().map(|c| (name, c)))
    }

    /// What a controller or the engine reads of the car while the drivetrain is at work.
    fn signals<'a>(&self, car: &'a RollingChassis) -> CarSignals<'a> {
        CarSignals {
            chassis: car,
            traction_type: self.base.traction_type,
            current_gear: self.base.current_gear,
            engine_rpm: self.get_engine_rpm(),
            torque_generator: None,
            coast_generator: None,
        }
    }

    /// The signals for `Engine::step`: with what the engine's torque generators
    /// (`Engine::torqueGenerators`: a KERS on the crankshaft, an ERS) and coast generators
    /// (an ERS) answer at this moment. They read nothing the engine's step changes, so asking
    /// before the step is asking inside it.
    fn engine_signals<'a>(&self, car: &'a RollingChassis) -> CarSignals<'a> {
        let mut signals = self.signals(car);
        if let Some(kers) = car.kers.as_ref().filter(|kers| kers.attachment == super::kers::KersAttachment::Engine) {
            signals.torque_generator = Some(kers.get_output_torque(&self.base));
        }
        // an ERS is a torque generator and a coast generator (never together with a KERS).
        // Its drag is asked for on every step, also when it answers 0
        if let Some(ers) = &car.ers {
            signals.torque_generator = Some(ers.get_output_torque(&self.base));
            signals.coast_generator = Some(ers.get_coast_torque(&self.base, car.controls.clutch));
        }
        signals
    }

    /// `Drivetrain::wheelTorqueGenerators` (a KERS on the wheels): its torque at this moment.
    fn wheel_generator_torque(&self, car: &RollingChassis) -> Option<f32> {
        let kers = car.kers.as_ref().filter(|kers| kers.attachment == super::kers::KersAttachment::Wheels)?;
        Some(kers.get_output_torque(&self.base))
    }

    /// `Drivetrain::stepControllers` @ 0x14026b200: front share, centre lock, single lock, in
    /// this order (the `AWD2` controller is evaluated inside `step4WD_new`).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_controllers(&mut self, car: &RollingChassis) {
        if self.awd_front_share_ctrl.is_some() {
            let signals = self.signals(car);
            self.base.awd_front_share = self.awd_front_share_ctrl.as_mut().unwrap().eval(&signals);
        }
        if self.awd_center_lock.is_some() {
            self.base.awd_center_diff.power = 0.0;
            self.base.awd_center_diff.coast = 0.0;
            // 20 Nm below 5 km/h, the controller's value from 25 km/h on
            let mut k = ((car.speed * 3.6) - 5.0) * 0.05;
            if k > 1.0 {
                k = 1.0;
            } else if !(k >= 0.0) {
                k = 0.0;
            }
            let signals = self.signals(car);
            let value = self.awd_center_lock.as_mut().unwrap().eval(&signals);
            self.base.awd_center_diff.preload = ((value - 20.0) * k) + 20.0;
        }
        if self.single_diff_lock.is_some() {
            self.base.diff_power_ramp = 0.0;
            self.base.diff_coast_ramp = 0.0;
            let signals = self.signals(car);
            self.base.diff_pre_load = self.single_diff_lock.as_mut().unwrap().eval(&signals);
        }
    }

    /// `Drivetrain::getInertiaFromEngine` @ 0x140266ba0: everything, as seen from the engine.
    fn get_inertia_from_engine(&self) -> f64 {
        let b = &self.base;
        if !ordered_nonzero(b.ratio) {
            return b.engine.inertia;
        }
        let mut wheels = (b.out_shaft_l.inertia + b.drive.inertia) + b.out_shaft_r.inertia;
        // an `AWD2` engine "sees" the rear axle only
        if b.traction_type == TractionType::Awd {
            wheels = (wheels + b.out_shaft_lf.inertia) + b.out_shaft_rf.inertia;
        }
        (wheels / (b.ratio * b.ratio).abs()) + (b.clutch_inertia as f64 + b.engine.inertia)
    }

    /// `Drivetrain::getInertiaFromWheels` @ 0x140266c20: everything that turns with the
    /// wheels, as seen from them. Note the two orders of the sums.
    fn get_inertia_from_wheels(&self) -> f64 {
        let b = &self.base;
        let rear = if !ordered_nonzero(b.ratio) {
            (b.out_shaft_l.inertia + b.drive.inertia) + b.out_shaft_r.inertia
        } else {
            let r2 = (b.ratio * b.ratio).abs();
            if !b.clutch_open_state {
                (((r2 * (b.clutch_inertia as f64 + b.engine.inertia)) + b.drive.inertia) + b.out_shaft_l.inertia)
                    + b.out_shaft_r.inertia
            } else {
                (((r2 * b.clutch_inertia as f64) + b.drive.inertia) + b.out_shaft_l.inertia) + b.out_shaft_r.inertia
            }
        };
        if b.traction_type == TractionType::Awd {
            (rear + b.out_shaft_lf.inertia) + b.out_shaft_rf.inertia
        } else {
            rear
        }
    }

    /// `Drivetrain::accelerateDrivetrainBlock` @ 0x1402664c0. `AWD`: the engine's part goes
    /// to the axles by `awdFrontShare` (`from_engine`), a realignment half and half; the
    /// front shafts of an `AWD2` car are not touched.
    fn accelerate_drivetrain_block(&mut self, acc: f64, from_engine: bool) {
        let b = &mut self.base;
        b.drive.velocity = acc + b.drive.velocity;
        if b.traction_type == TractionType::Awd {
            let share: f32 = if from_engine { b.awd_front_share } else { 0.5 };
            let front = (share as f64 * acc) * 2.0;
            b.out_shaft_rf.velocity = front + b.out_shaft_rf.velocity;
            b.out_shaft_lf.velocity = front + b.out_shaft_lf.velocity;
            let rear = ((1.0f32 - share) as f64 * acc) * 2.0;
            b.out_shaft_r.velocity = rear + b.out_shaft_r.velocity;
            b.out_shaft_l.velocity = rear + b.out_shaft_l.velocity;
            return;
        }
        b.out_shaft_r.velocity = acc + b.out_shaft_r.velocity;
        b.out_shaft_l.velocity = acc + b.out_shaft_l.velocity;
    }

    /// `Drivetrain::reallignSpeeds` @ 0x140269100: a gear has just gone in; engine side and
    /// wheel side are brought to a common speed. Runs with this step's `locClutch` and the
    /// previous step's `clutchOpenState`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn reallign_speeds(&mut self) {
        let r = self.base.ratio;
        if !ordered_nonzero(r) {
            return;
        }
        let drive = self.base.drive.velocity;
        if !(0.9f32 >= self.base.loc_clutch) {
            // the clutch holds: an inertia-weighted common speed
            let root = self.base.root_velocity;
            let inertia = self.get_inertia_from_engine();
            let k = 1.0 - (self.base.engine.inertia / inertia);
            self.base.root_velocity = root - ((k * ((root / r) - drive)) * r.abs());
        } else {
            // the engine is free: the shaft follows the wheels
            self.base.root_velocity = drive * r;
        }
        // narrowed, then widened again
        let acc = ((self.base.root_velocity / r) - drive) as f32;
        self.accelerate_drivetrain_block(acc as f64, false);
        if !self.base.clutch_open_state {
            self.base.engine.velocity = self.base.root_velocity;
        }
    }

    /// `Drivetrain::step2WD` @ 0x1402694e0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_2wd(&mut self, car: &mut RollingChassis, dt: f32) {
        let dtd = dt as f64;
        let (tyre_left, tyre_right) = (self.base.tyre_left, self.base.tyre_right);

        // a pending paddle shift: the gear goes in once the time is over
        {
            let b = &mut self.base;
            if b.gear_request.request != GearChangeRequest::None && !(b.gear_request.timeout >= b.gear_request.time_accumulator) {
                b.gear_request.request = GearChangeRequest::None;
                b.current_gear = b.gear_request.requested_gear;
            }
            if b.gear_request.request != GearChangeRequest::None {
                b.gear_request.time_accumulator = dtd + b.gear_request.time_accumulator;
            }
            b.ratio = b.final_ratio as f64 * b.gears[b.current_gear as usize].ratio;
            b.engine.inertia = self.ac_engine.base().inertia as f64;
        }
        // a KERS on the wheels: half its torque into each driven tyre's feedback torque, the
        // right one first
        if let Some(torque) = self.wheel_generator_torque(car) {
            let half = torque * 0.5;
            car.tyres[tyre_right].status.feedback_torque = half + car.tyres[tyre_right].status.feedback_torque;
            car.tyres[tyre_left].status.feedback_torque = half + car.tyres[tyre_left].status.feedback_torque;
        }
        if self.base.last_ratio < self.base.ratio || self.base.last_ratio > self.base.ratio {
            self.reallign_speeds();
            self.base.last_ratio = self.base.ratio;
        }

        // the throttle cut of an up-shift, then the engine
        let mut input = SACEngineInput::default();
        if self.base.cut_off > 0.0 {
            input.gas_input = 0.0;
            self.base.cut_off -= dtd;
        } else {
            input.gas_input = car.controls.gas;
        }
        input.rpm = ((self.base.engine.velocity as f32) * 0.159_155_07) * 60.0;
        let signals = self.engine_signals(car);
        self.ac_engine.step(&input, dt, &signals);
        let torque = self.ac_engine.base().status.out_torque;

        // the clutch: open while the pedal is not fully up or the two sides differ by 10 %
        let lc = self.base.loc_clutch;
        {
            let b = &mut self.base;
            b.clutch_open_state = if !(lc >= 1.0) {
                true
            } else if ordered_nonzero(b.engine.velocity) {
                ((b.root_velocity / b.engine.velocity) - 1.0).abs() >= 0.1
            } else {
                0.0 < b.root_velocity || 0.0 > b.root_velocity
            };
        }
        let engine_inertia = self.base.engine.inertia;
        let r = self.base.ratio;
        let inertia_from_engine = self.get_inertia_from_engine();
        let inertia_from_wheels = self.get_inertia_from_wheels();
        let mut clutch_torque = 0.0f64;
        let in_gear = ordered_nonzero(r);

        // the engine side
        if self.base.clutch_open_state {
            let engine_speed = self.base.engine.velocity;
            let root = self.base.root_velocity;
            let slip = engine_speed - root;
            let capacity = lc as f64 * self.base.clutch_max_torque;
            clutch_torque = -((slip / (slip.abs() + 4.0)) * capacity);
            self.base.current_clutch_torque = clutch_torque as f32;
            if in_gear {
                let gearbox_inertia = inertia_from_engine - engine_inertia;
                self.base.engine.velocity = (((clutch_torque + torque) / engine_inertia) * dtd) + engine_speed;
                let d = dtd * ((-clutch_torque) / gearbox_inertia);
                self.base.root_velocity = d + root;
                self.accelerate_drivetrain_block(d / r, true);
            } else {
                // the clutch torque is not applied in neutral (but the differential still sees it)
                let speed = ((torque / engine_inertia) * dtd) + engine_speed;
                self.base.engine.velocity = speed;
                self.base.root_velocity = speed;
            }
        } else if in_gear {
            let d = dtd * (torque / inertia_from_engine);
            self.base.root_velocity = d + self.base.root_velocity;
            self.accelerate_drivetrain_block(d / r, true);
        } else {
            self.base.root_velocity = ((torque / inertia_from_engine) * dtd) + self.base.root_velocity;
        }

        // the wheel side: the sum of the two tyres' torques turns the whole block
        let feedback_left = car.tyres[tyre_left].status.feedback_torque;
        let feedback_right = car.tyres[tyre_right].status.feedback_torque;
        {
            let b = &mut self.base;
            let feedback = feedback_right + feedback_left;
            if in_gear {
                let a = dtd * (feedback as f64 / inertia_from_wheels);
                b.root_velocity = (a * b.ratio) + b.root_velocity;
                b.drive.velocity = a + b.drive.velocity;
                b.out_shaft_r.velocity = a + b.out_shaft_r.velocity;
                b.out_shaft_l.velocity = a + b.out_shaft_l.velocity;
            } else {
                let a = (feedback as f64 / inertia_from_wheels) * dtd;
                b.drive.velocity = a + b.drive.velocity;
                b.out_shaft_r.velocity = a + b.out_shaft_r.velocity;
                b.out_shaft_l.velocity = a + b.out_shaft_l.velocity;
            }
        }

        // the differential: how far the two wheels may differ
        {
            let b = &mut self.base;
            let mut input_torque = b.loc_clutch as f64 * torque;
            match b.diff_type {
                DifferentialType::Spool => {
                    b.out_shaft_l.velocity = b.drive.velocity;
                    b.out_shaft_r.velocity = b.drive.velocity;
                }
                DifferentialType::Lsd => {
                    if ordered_nonzero(clutch_torque) {
                        input_torque = -clutch_torque;
                    }
                    let abs_ratio = b.ratio.abs();
                    let mut lock = if input_torque > 0.0 {
                        abs_ratio * (b.diff_power_ramp as f64 * input_torque)
                    } else {
                        (abs_ratio * (b.diff_coast_ramp as f64 * input_torque)).abs()
                    };
                    let mut left = b.out_shaft_l.velocity;
                    let mut right = b.out_shaft_r.velocity;
                    let carrier = b.drive.velocity;
                    lock += b.diff_pre_load as f64;
                    let friction = -(((left - right) / ((left - right).abs() + F64C_0_01F)) * lock);
                    let slipping = (left - carrier).abs() >= F64C_0_1F || ((feedback_right - feedback_left) as f64).abs() > lock;
                    if !slipping {
                        b.out_shaft_r.velocity = carrier;
                        b.out_shaft_l.velocity = carrier;
                    } else {
                        let x = dtd * ((friction / b.out_shaft_l.inertia) * 0.5);
                        left += x;
                        right -= x;
                        let y = dtd * ((((feedback_right - feedback_left) as f64) / b.out_shaft_r.inertia) * 0.5);
                        b.out_shaft_l.velocity = left - y;
                        b.out_shaft_r.velocity = y + right;
                    }
                }
            }
        }

        // both driven wheels flagged locked by their tyres: released when the engine is
        // stronger than the brakes or the car moves; else, with the clutch open, held
        if car.tyres[tyre_left].status.is_locked && car.tyres[tyre_right].status.is_locked {
            let (l, rt) = (&car.tyres[tyre_left], &car.tyres[tyre_right]);
            let brake: f32 = (((l.abs_override * l.inputs.brake_torque) + l.inputs.hand_brake_torque)
                + (rt.abs_override * rt.inputs.brake_torque))
                + rt.inputs.hand_brake_torque;
            let engine = (self.base.ratio.abs() * torque).abs();
            if engine > brake as f64 || !(1.0 >= car.speed) {
                car.tyres[tyre_left].status.is_locked = false;
                car.tyres[tyre_right].status.is_locked = false;
            } else if self.base.clutch_open_state {
                let b = &mut self.base;
                b.root_velocity = 0.0;
                b.drive.velocity = 0.0;
                b.out_shaft_l.velocity = 0.0;
                b.out_shaft_r.velocity = 0.0;
            }
        }

        // write-back: the doubles stay the state, the tyres get them narrowed. Not for
        // `AWD2`: `step4WD_new` does it for all four wheels after its coupling (so a held
        // clutch leaves the engine's speed one step old until then)
        if self.base.traction_type != TractionType::AwdNew {
            if !self.base.clutch_open_state {
                self.base.engine.velocity = self.base.root_velocity;
            }
            car.tyres[tyre_left].status.angular_velocity = self.base.out_shaft_l.velocity as f32;
            car.tyres[tyre_right].status.angular_velocity = self.base.out_shaft_r.velocity as f32;
            car.step_wheel_rotation(tyre_left, dt);
            car.step_wheel_rotation(tyre_right, dt);
        }

        // display value
        let b = &mut self.base;
        b.total_torque = if ordered_nonzero(b.ratio) {
            ((b.ratio as f32).abs() * ((torque as f32) * b.loc_clutch) - (feedback_left + feedback_right)).abs()
        } else {
            ((torque as f32) * b.loc_clutch).abs()
        };
        // (the reaction torque on the body exists only for a rigid rear axle, which the
        // chassis does not have, and for `torqueModeEx == reactionTorques`, which nothing sets)
    }

    /// The narrowed speeds of the four shafts go to the tyres, each tyre done completely (speed,
    /// spin matrix, the locked flag cleared) before the next: the end of both 4WD steps.
    fn write_back_four(&mut self, car: &mut RollingChassis, dt: f32) {
        let b = &self.base;
        let speeds =
            [b.out_shaft_lf.velocity as f32, b.out_shaft_rf.velocity as f32, b.out_shaft_l.velocity as f32, b.out_shaft_r.velocity as f32];
        for (index, speed) in speeds.into_iter().enumerate() {
            car.tyres[index].status.angular_velocity = speed;
            car.step_wheel_rotation(index, dt);
            car.tyres[index].status.is_locked = false;
        }
    }

    /// `totalTorque` of both 4WD steps (single precision; note the order of the tyres).
    fn total_torque_four(&mut self, car: &RollingChassis, torque: f64) {
        let fb = [0, 1, 2, 3].map(|i| car.tyres[i].status.feedback_torque);
        let b = &mut self.base;
        b.total_torque = if ordered_nonzero(b.ratio) {
            ((b.ratio as f32).abs() * ((torque as f32) * b.loc_clutch) - (((fb[1] + fb[0]) + fb[2]) + fb[3])).abs()
        } else {
            ((torque as f32) * b.loc_clutch).abs()
        };
    }

    /// `Drivetrain::step4WD` @ 0x14026a220 (`AWD`): the engine and the clutch as for two driven
    /// wheels, then three differentials (rear, front, centre) that only ever slip against a
    /// locking torque: no hold, no spool, no locked wheels, no reaction on the body.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_4wd(&mut self, car: &mut RollingChassis, dt: f32) {
        let dtd = dt as f64;

        // a pending paddle shift: the gear goes in once the time is over
        {
            let b = &mut self.base;
            if b.gear_request.request != GearChangeRequest::None && !(b.gear_request.timeout >= b.gear_request.time_accumulator) {
                b.gear_request.request = GearChangeRequest::None;
                b.current_gear = b.gear_request.requested_gear;
            }
            if b.gear_request.request != GearChangeRequest::None {
                b.gear_request.time_accumulator = dtd + b.gear_request.time_accumulator;
            }
            b.ratio = b.final_ratio as f64 * b.gears[b.current_gear as usize].ratio;
            b.engine.inertia = self.ac_engine.base().inertia as f64;
        }
        // a KERS on the wheels: a quarter of its torque into each tyre's feedback torque
        if let Some(torque) = self.wheel_generator_torque(car) {
            let quarter = torque * 0.25;
            for tyre in car.tyres.iter_mut().take(4) {
                tyre.status.feedback_torque = quarter + tyre.status.feedback_torque;
            }
        }
        if self.base.last_ratio < self.base.ratio || self.base.last_ratio > self.base.ratio {
            self.reallign_speeds();
            self.base.last_ratio = self.base.ratio;
        }

        // the throttle cut of an up-shift, then the engine
        let mut input = SACEngineInput::default();
        if self.base.cut_off > 0.0 {
            input.gas_input = 0.0;
            self.base.cut_off -= dtd;
        } else {
            input.gas_input = car.controls.gas;
        }
        input.rpm = ((self.base.engine.velocity as f32) * 0.159_155_07) * 60.0;
        let signals = self.engine_signals(car);
        self.ac_engine.step(&input, dt, &signals);
        let torque = self.ac_engine.base().status.out_torque;

        // the clutch
        let lc = self.base.loc_clutch;
        {
            let b = &mut self.base;
            b.clutch_open_state = if !(lc >= 1.0) {
                true
            } else if ordered_nonzero(b.engine.velocity) {
                ((b.root_velocity / b.engine.velocity) - 1.0).abs() >= 0.1
            } else {
                0.0 < b.root_velocity || 0.0 > b.root_velocity
            };
        }
        let engine_inertia = self.base.engine.inertia;
        let r = self.base.ratio;
        let inertia_from_engine = self.get_inertia_from_engine();
        let inertia_from_wheels = self.get_inertia_from_wheels();
        let mut clutch_torque = 0.0f64;
        let in_gear = ordered_nonzero(r);

        // the engine side: its part of the block's speed goes to the axles by the front share
        if self.base.clutch_open_state {
            let engine_speed = self.base.engine.velocity;
            let root = self.base.root_velocity;
            let slip = engine_speed - root;
            let capacity = lc as f64 * self.base.clutch_max_torque;
            clutch_torque = -((slip / (slip.abs() + 4.0)) * capacity);
            self.base.current_clutch_torque = clutch_torque as f32;
            if in_gear {
                let gearbox_inertia = inertia_from_engine - engine_inertia;
                self.base.engine.velocity = (((clutch_torque + torque) / engine_inertia) * dtd) + engine_speed;
                let d = dtd * ((-clutch_torque) / gearbox_inertia);
                self.base.root_velocity = d + root;
                self.accelerate_drivetrain_block(d / r, true);
            } else {
                let speed = ((torque / engine_inertia) * dtd) + engine_speed;
                self.base.engine.velocity = speed;
                self.base.root_velocity = speed;
            }
        } else if in_gear {
            let d = dtd * (torque / inertia_from_engine);
            self.base.root_velocity = d + self.base.root_velocity;
            self.accelerate_drivetrain_block(d / r, true);
        } else {
            self.base.root_velocity = ((torque / inertia_from_engine) * dtd) + self.base.root_velocity;
        }

        // the wheel side: the sum of the four tyres' torques turns the whole block
        let fb = [0, 1, 2, 3].map(|i| car.tyres[i].status.feedback_torque);
        {
            let b = &mut self.base;
            let feedback: f32 = (fb[3] + fb[2]) + (fb[1] + fb[0]);
            let a = if in_gear {
                let a = dtd * (feedback as f64 / inertia_from_wheels);
                b.root_velocity = (a * b.ratio) + b.root_velocity;
                a
            } else {
                (feedback as f64 / inertia_from_wheels) * dtd
            };
            b.drive.velocity = a + b.drive.velocity;
            let a2 = (a * 0.5) * 2.0;
            b.out_shaft_lf.velocity = a2 + b.out_shaft_lf.velocity;
            b.out_shaft_rf.velocity = a2 + b.out_shaft_rf.velocity;
            b.out_shaft_r.velocity = a2 + b.out_shaft_r.velocity;
            b.out_shaft_l.velocity = a2 + b.out_shaft_l.velocity;
        }

        // the three differentials. The input torque has no clutch-pedal factor here, and the
        // slip's softening constant is 0.1, not the 0.01 of the two-wheel-drive differential
        {
            let b = &mut self.base;
            let abs_ratio = b.ratio.abs();
            let front_share: f32 = b.awd_front_share;
            let rear_share: f32 = 1.0 - front_share;
            let lock_of = |diff: &DifferentialSetting, input: f64| {
                if input > 0.0 {
                    (diff.power as f64 * input) * abs_ratio
                } else {
                    ((diff.coast as f64 * input) * abs_ratio).abs()
                }
            };

            // rear
            let input = if ordered_nonzero(clutch_torque) { -(rear_share as f64 * clutch_torque) } else { rear_share as f64 * torque };
            let lock = lock_of(&b.awd_rear_diff, input);
            let mut left = b.out_shaft_l.velocity;
            let mut right = b.out_shaft_r.velocity;
            let dv = left - right;
            let friction = -((dv / (dv.abs() + F64C_0_1F)) * (b.awd_rear_diff.preload as f64 + lock));
            let x = dtd * ((friction / b.out_shaft_l.inertia) * 0.5);
            left += x;
            right -= x;
            let y = dtd * ((((fb[3] - fb[2]) as f64) / b.out_shaft_r.inertia) * 0.5);
            b.out_shaft_l.velocity = left - y;
            b.out_shaft_r.velocity = y + right;

            // front
            let input = if ordered_nonzero(clutch_torque) { -(front_share as f64 * clutch_torque) } else { front_share as f64 * torque };
            let lock = lock_of(&b.awd_front_diff, input);
            let dv = b.out_shaft_lf.velocity - b.out_shaft_rf.velocity;
            let friction = -((dv / (dv.abs() + F64C_0_1F)) * (b.awd_front_diff.preload as f64 + lock));
            let x = dtd * ((friction / b.out_shaft_lf.inertia) * 0.5);
            let mut front_right = b.out_shaft_rf.velocity - x;
            let mut front_left = x + b.out_shaft_lf.velocity;
            let y = dtd * ((((fb[1] - fb[0]) as f64) / b.out_shaft_rf.inertia) * 0.5);
            front_left -= y;
            front_right = y + front_right;
            b.out_shaft_lf.velocity = front_left;
            b.out_shaft_rf.velocity = front_right;

            // centre: the whole torque; the rear right's inertia is in the sum twice and the
            // front right's not at all
            let input = if ordered_nonzero(clutch_torque) { -clutch_torque } else { torque };
            let lock = lock_of(&b.awd_center_diff, input);
            let inv_inertia = 1.0 / (((b.out_shaft_r.inertia + b.out_shaft_l.inertia) + b.out_shaft_lf.inertia) + b.out_shaft_r.inertia);
            let front_mean = (front_left + front_right) * 0.5;
            let rear_mean = (b.out_shaft_l.velocity + b.out_shaft_r.velocity) * 0.5;
            let dv = rear_mean - front_mean;
            let friction = -((dv / (dv.abs() + F64C_0_1F)) * (b.awd_center_diff.preload as f64 + lock));
            let x = dtd * (friction * inv_inertia);
            front_left -= x;
            front_right -= x;
            let right = x + b.out_shaft_r.velocity;
            let left = b.out_shaft_l.velocity + x;
            let y = dtd * ((((fb[3] + fb[2]) - (fb[1] + fb[0])) as f64) * inv_inertia);
            b.out_shaft_l.velocity = y + left;
            b.out_shaft_r.velocity = y + right;
            b.out_shaft_lf.velocity = front_left - y;
            b.out_shaft_rf.velocity = front_right - y;
        }

        if !self.base.clutch_open_state {
            self.base.engine.velocity = self.base.root_velocity;
        }
        self.write_back_four(car, dt);
        self.total_torque_four(car, torque);
    }

    /// `Drivetrain::step4WD_new` @ 0x14026ad80 (`AWD2`): a complete rear-wheel drive, then one
    /// torque between the axles, proportional to how much faster the rear one turns and limited
    /// to `awd2.maxTorque`; the front wheels are otherwise free.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_4wd_new(&mut self, car: &mut RollingChassis, dt: f32) {
        let dtd = dt as f64;
        self.step_2wd(car, dt);
        let torque = self.ac_engine.base().status.out_torque;

        let b = &self.base;
        let demand =
            (((b.out_shaft_l.velocity + b.out_shaft_r.velocity) * 0.5) - ((b.out_shaft_lf.velocity + b.out_shaft_rf.velocity) * 0.5)) * b.awd2.ramp;
        if self.awd2_ctrl.is_some() {
            // evaluated here, after the rear axle's step: the gear of a shift just finished,
            // the engine speed `step2WD` left
            let signals = self.signals(car);
            self.base.awd2.max_torque = self.awd2_ctrl.as_mut().unwrap().eval(&signals) as f64;
        }
        let fb_front_left = car.tyres[0].status.feedback_torque;
        let fb_front_right = car.tyres[1].status.feedback_torque;
        let b = &mut self.base;
        let most = b.awd2.max_torque;
        let coupling = if demand > most {
            most
        } else if !(demand >= -most) {
            -most
        } else {
            demand
        };
        let inv_front = 1.0 / (b.out_shaft_rf.inertia + b.out_shaft_lf.inertia);
        b.awd2.current_lock_torque = coupling as f32;
        let df = (inv_front * coupling) * dtd;
        let mut front_right = df + b.out_shaft_rf.velocity;
        let mut front_left = df + b.out_shaft_lf.velocity;
        // each front tyre's torque: to both shafts together, then as an open differential.
        // `(x + e) - e` is not `x` in floating point: kept as the machine does it
        let e = (fb_front_left as f64 * inv_front) * dtd;
        front_left = (front_left + e) + e;
        front_right = (front_right + e) - e;
        let e = (fb_front_right as f64 * inv_front) * dtd;
        front_left = (e + front_left) - e;
        front_right = (e + front_right) + e;
        b.out_shaft_lf.velocity = front_left;
        b.out_shaft_rf.velocity = front_right;
        // the rear block loses what the front shafts got
        let inertia_from_wheels = self.get_inertia_from_wheels();
        let acc = -((coupling / inertia_from_wheels) * dtd);
        if ordered_nonzero(self.base.ratio) {
            self.base.root_velocity = (self.base.ratio * acc) + self.base.root_velocity;
        }
        self.accelerate_drivetrain_block(acc, false);

        if !self.base.clutch_open_state {
            self.base.engine.velocity = self.base.root_velocity;
        }
        self.write_back_four(car, dt);
        self.total_torque_four(car, torque);
    }
}

impl DrivetrainModel for VanillaDrivetrain {
    fn step(&mut self, car: &mut RollingChassis, dt: f32) {
        self.base.out_shaft_lf.old_velocity = self.base.out_shaft_lf.velocity;
        self.base.out_shaft_rf.old_velocity = self.base.out_shaft_rf.velocity;
        self.base.out_shaft_l.old_velocity = self.base.out_shaft_l.velocity;
        self.base.out_shaft_r.old_velocity = self.base.out_shaft_r.velocity;
        let loc_clutch = powf(car.controls.clutch, 1.5);
        self.base.current_clutch_torque = 0.0;
        self.base.loc_clutch = loc_clutch;
        self.step_controllers(car);
        match self.base.traction_type {
            TractionType::Rwd | TractionType::Fwd => self.step_2wd(car, dt),
            TractionType::Awd => self.step_4wd(car, dt),
            TractionType::AwdNew => self.step_4wd_new(car, dt),
        }
    }

    fn gear_up(&mut self, car: &mut RollingChassis) -> bool {
        if is_gearbox_locked(car) {
            return false;
        }
        let b = &mut self.base;
        // an unsigned compare: a negative gear is refused too
        if b.current_gear < 0 || b.current_gear as usize >= b.gears.len().wrapping_sub(1) {
            return false;
        }
        if b.gear_request.request != GearChangeRequest::None {
            return false;
        }
        b.gear_request.timeout = b.gear_up_time;
        b.gear_request.request = GearChangeRequest::Up;
        b.gear_request.time_accumulator = 0.0;
        b.gear_request.requested_gear = b.current_gear + 1;
        let event = OnGearRequestEvent { request: GearChangeRequest::Up, next_gear: b.current_gear + 1 };
        car.on_gear_request(&event);
        let b = &mut self.base;
        if ordered_nonzero(b.auto_cut_off_time) {
            b.cut_off = b.auto_cut_off_time;
        }
        // neutral for the duration of the shift
        b.current_gear = 1;
        true
    }

    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn gear_down(&mut self, car: &mut RollingChassis) -> bool {
        if is_gearbox_locked(car) {
            return false;
        }
        let b = &mut self.base;
        if b.downshift_protection.is_active && b.current_gear > 0 {
            let target = b.gears[(b.current_gear - 1) as usize].ratio;
            if ordered_nonzero(target) && ordered_nonzero(b.gears[b.current_gear as usize].ratio) {
                // where the engine would be after the shift, with the wheels slowing as they
                // did in the last step
                let (left, right) = (b.out_shaft_l.velocity, b.out_shaft_r.velocity);
                let mut left_acc = (left - b.out_shaft_l.old_velocity) * INV_DT;
                if left_acc > 0.0 {
                    left_acc = 0.0;
                }
                let left_then = (b.gear_dn_time * left_acc) + left;
                let mut right_acc = (right - b.out_shaft_r.old_velocity) * INV_DT;
                if right_acc > 0.0 {
                    right_acc = 0.0;
                }
                let wheels = (((b.gear_dn_time * right_acc) + right) + left_then) * 0.5;
                let rpm = (((target * wheels) * b.final_ratio as f64) * K2PI) * 60.0;
                let limit = self.ac_engine.get_limiter_rpm().wrapping_add(b.downshift_protection.overrev);
                if rpm > limit as f64 {
                    return false;
                }
            } else if b.downshift_protection.lock_n && !ordered_nonzero(target) && car.speed * 3.6 > 2.0 {
                return false;
            }
        }
        if b.current_gear <= 0 {
            return false;
        }
        if b.gear_request.request != GearChangeRequest::None {
            return false;
        }
        b.gear_request.request = GearChangeRequest::Down;
        b.gear_request.timeout = b.gear_dn_time;
        b.gear_request.time_accumulator = 0.0;
        b.gear_request.requested_gear = b.current_gear - 1;
        let event = OnGearRequestEvent { request: GearChangeRequest::Down, next_gear: b.current_gear - 1 };
        car.on_gear_request(&event);
        self.base.current_gear = 1;
        true
    }

    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn set_current_gear(&mut self, index: i32, force: bool, car: &RollingChassis) {
        let b = &mut self.base;
        if index != 1 && is_gearbox_locked(car) {
            b.current_gear = 1;
            return;
        }
        b.is_gear_grinding = false;
        if index < 0 || index as usize >= b.gears.len() || index == b.current_gear {
            return;
        }
        if index == 1 || force {
            b.current_gear = index;
            return;
        }
        // the H-shifter: the gear only goes in when engine and shaft speeds are close enough,
        // or the clutch is pressed
        let difference =
            (b.engine.velocity - ((b.gears[index as usize].ratio * b.drive.velocity) * b.final_ratio as f64)).abs();
        let clutch = b.loc_clutch as f64;
        let closed = clutch * difference;
        let mismatch_rpm =
            ((((((car.controls.gas as f64 * clutch) * difference) - closed) * b.controls_window_gain) + closed) * K2PI) * 60.0;
        if !(mismatch_rpm >= b.valid_shift_rpm_window) {
            b.current_gear = index;
            return;
        }
        b.is_gear_grinding = true;
        if !(b.valid_shift_rpm_window > 0.0) {
            return;
        }
        let damage_rate = car.env.mechanical_damage_rate;
        if !(damage_rate > 0.0) {
            return;
        }
        b.valid_shift_rpm_window -= (b.damage_rpm_window * F64C_0_003F) * damage_rate as f64;
    }

    fn set_gear_ratio(&mut self, index: i32, ratio: f32) {
        if index < 0 || index as usize >= self.base.gears.len() {
            return;
        }
        self.base.gears[index as usize].ratio = ratio as f64;
    }

    fn reset(&mut self) {
        let b = &mut self.base;
        b.clutch_open_state = true;
        b.root_velocity = 0.0;
        b.engine.velocity = 0.0;
        b.out_shaft_l.velocity = 0.0;
        b.out_shaft_r.velocity = 0.0;
        b.out_shaft_lf.velocity = 0.0;
        b.out_shaft_rf.velocity = 0.0;
        b.drive.velocity = 0.0;
        b.gear_request.request = GearChangeRequest::None;
        // the raw ini value: 0 when the key is missing
        b.valid_shift_rpm_window = b.org_rpm_window;
        self.ac_engine.reset();
    }

    fn get_engine_rpm(&self) -> f32 {
        ((self.base.engine.velocity as f32) * 0.159_155_07) * 60.0
    }

    fn is_changing_gear(&self) -> bool {
        self.base.gear_request.request != GearChangeRequest::None
    }

    fn has_dynamic_controllers(&self) -> bool {
        self.awd2_ctrl.is_some() || self.awd_center_lock.is_some() || self.awd_front_share_ctrl.is_some() || self.single_diff_lock.is_some()
    }

    fn project_rpm_at_downshift(&self) -> f32 {
        let b = &self.base;
        if b.current_gear == 0 {
            return -1.0;
        }
        let target = b.gears[(b.current_gear - 1) as usize].ratio;
        if !ordered_nonzero(target) {
            return -1.0;
        }
        let wheels = ((b.out_shaft_r.velocity + b.out_shaft_l.velocity) * 0.5) as f32;
        ((((wheels as f64 * target) * b.final_ratio as f64) * K2PI) * 60.0) as f32
    }

    fn engine(&self) -> &dyn EngineModel {
        self.ac_engine.as_ref()
    }

    fn engine_mut(&mut self) -> &mut dyn EngineModel {
        self.ac_engine.as_mut()
    }

    fn base(&self) -> &DrivetrainBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut DrivetrainBase {
        &mut self.base
    }

    fn trace(&self, out: &mut Vec<TraceValue>) {
        let b = &self.base;
        out.push(TraceValue::i("drivetrain.currentGear", b.current_gear));
        out.push(TraceValue::d("drivetrain.engine.velocity", b.engine.velocity));
        out.push(TraceValue::f("drivetrain.engineRPM", self.get_engine_rpm()));
        out.push(TraceValue::d("drivetrain.drive.velocity", b.drive.velocity));
        out.push(TraceValue::d("drivetrain.outShaftL.velocity", b.out_shaft_l.velocity));
        out.push(TraceValue::d("drivetrain.outShaftR.velocity", b.out_shaft_r.velocity));
        out.push(TraceValue::d("drivetrain.rootVelocity", b.root_velocity));
        out.push(TraceValue::i("drivetrain.clutchOpenState", b.clutch_open_state as i32));
        out.push(TraceValue::d("drivetrain.ratio", b.ratio));
        out.push(TraceValue::d("drivetrain.cutOff", b.cut_off));
        out.push(TraceValue::f("drivetrain.totalTorque", b.total_torque));
        out.push(TraceValue::f("drivetrain.currentClutchTorque", b.current_clutch_torque));
        out.push(TraceValue::f("drivetrain.locClutch", b.loc_clutch));
        out.push(TraceValue::i("drivetrain.isGearGrinding", b.is_gear_grinding as i32));
        out.push(TraceValue::f("drivetrain.diffPowerRamp", b.diff_power_ramp));
        out.push(TraceValue::f("drivetrain.diffCoastRamp", b.diff_coast_ramp));
        out.push(TraceValue::f("drivetrain.diffPreLoad", b.diff_pre_load));
        out.push(TraceValue::i("drivetrain.gearRequest.request", b.gear_request.request as i32));
        out.push(TraceValue::i("drivetrain.gearRequest.requestedGear", b.gear_request.requested_gear));
        out.push(TraceValue::d("drivetrain.gearRequest.timeAccumulator", b.gear_request.time_accumulator).extra());
        out.push(TraceValue::d("drivetrain.gearRequest.timeout", b.gear_request.timeout).extra());
        out.push(TraceValue::d("drivetrain.validShiftRPMWindow", b.valid_shift_rpm_window).extra());
        out.push(TraceValue::d("drivetrain.lastRatio", b.last_ratio).extra());
        out.push(TraceValue::d("drivetrain.outShaftL.oldVelocity", b.out_shaft_l.old_velocity).extra());
        out.push(TraceValue::d("drivetrain.outShaftR.oldVelocity", b.out_shaft_r.old_velocity).extra());
        if matches!(b.traction_type, TractionType::Awd | TractionType::AwdNew) {
            out.push(TraceValue::i("drivetrain.tractionType", b.traction_type as i32).extra());
            out.push(TraceValue::d("drivetrain.outShaftLF.velocity", b.out_shaft_lf.velocity).extra());
            out.push(TraceValue::d("drivetrain.outShaftRF.velocity", b.out_shaft_rf.velocity).extra());
            out.push(TraceValue::d("drivetrain.outShaftLF.oldVelocity", b.out_shaft_lf.old_velocity).extra());
            out.push(TraceValue::d("drivetrain.outShaftRF.oldVelocity", b.out_shaft_rf.old_velocity).extra());
            out.push(TraceValue::d("drivetrain.outShaftL.inertia", b.out_shaft_l.inertia).extra());
            out.push(TraceValue::d("drivetrain.outShaftR.inertia", b.out_shaft_r.inertia).extra());
            out.push(TraceValue::d("drivetrain.outShaftLF.inertia", b.out_shaft_lf.inertia).extra());
            out.push(TraceValue::d("drivetrain.outShaftRF.inertia", b.out_shaft_rf.inertia).extra());
            out.push(TraceValue::f("drivetrain.awdFrontShare", b.awd_front_share).extra());
            for (name, diff) in [("awdFrontDiff", &b.awd_front_diff), ("awdRearDiff", &b.awd_rear_diff), ("awdCenterDiff", &b.awd_center_diff)] {
                out.push(TraceValue::f(&format!("drivetrain.{name}.power"), diff.power).extra());
                out.push(TraceValue::f(&format!("drivetrain.{name}.coast"), diff.coast).extra());
                out.push(TraceValue::f(&format!("drivetrain.{name}.preload"), diff.preload).extra());
            }
            out.push(TraceValue::d("drivetrain.awd2.ramp", b.awd2.ramp).extra());
            out.push(TraceValue::d("drivetrain.awd2.maxTorque", b.awd2.max_torque).extra());
            out.push(TraceValue::f("drivetrain.awd2.currentLockTorque", b.awd2.current_lock_torque).extra());
            for (name, controller) in self.awd_controllers() {
                for (k, stage) in controller.stages.iter().enumerate() {
                    out.push(TraceValue::f(&format!("drivetrain.ctrl.{name}.stage{k}.currentValue"), stage.current_value).extra());
                }
            }
        }
        self.ac_engine.trace(out);
    }

    fn save_state(&self, out: &mut Vec<u32>) {
        let b = &self.base;
        let mut doubles = vec![
            b.engine.velocity,
            b.engine.inertia,
            b.drive.velocity,
            b.out_shaft_l.velocity,
            b.out_shaft_l.old_velocity,
            b.out_shaft_r.velocity,
            b.out_shaft_r.old_velocity,
            b.root_velocity,
            b.ratio,
            b.cut_off,
            b.last_ratio,
            b.gear_request.time_accumulator,
            b.gear_request.timeout,
            b.valid_shift_rpm_window,
        ];
        doubles.extend(b.gears.iter().map(|gear| gear.ratio));
        for value in doubles {
            out.push(value.to_bits() as u32);
            out.push((value.to_bits() >> 32) as u32);
        }
        out.extend(
            [b.final_ratio, b.diff_power_ramp, b.diff_coast_ramp, b.diff_pre_load, b.total_torque, b.current_clutch_torque, b.loc_clutch]
                .map(f32::to_bits),
        );
        out.extend([
            b.is_gear_grinding as u32,
            b.clutch_open_state as u32,
            b.current_gear as u32,
            b.gear_request.request as u32,
            b.gear_request.requested_gear as u32,
        ]);
        if let Some(controller) = &self.single_diff_lock {
            out.extend(controller.stages.iter().map(|stage| stage.current_value.to_bits()));
        }
        // four-wheel drive (the saved states of the other cars keep their form)
        if matches!(b.traction_type, TractionType::Awd | TractionType::AwdNew) {
            for value in [b.out_shaft_lf.velocity, b.out_shaft_lf.old_velocity, b.out_shaft_rf.velocity, b.out_shaft_rf.old_velocity, b.awd2.max_torque] {
                out.push(value.to_bits() as u32);
                out.push((value.to_bits() >> 32) as u32);
            }
            out.push(b.awd_front_share.to_bits());
            for diff in [&b.awd_front_diff, &b.awd_rear_diff, &b.awd_center_diff] {
                out.extend([diff.power, diff.coast, diff.preload].map(f32::to_bits));
            }
            out.push(b.awd2.current_lock_torque.to_bits());
            for (_, controller) in self.awd_controllers() {
                out.extend(controller.stages.iter().map(|stage| stage.current_value.to_bits()));
            }
        }
        self.ac_engine.save_state(out);
    }

    fn load_state(&mut self, words: &mut dyn Iterator<Item = u32>) -> Result<(), String> {
        fn word(words: &mut dyn Iterator<Item = u32>) -> Result<u32, String> {
            words.next().ok_or("the saved drivetrain state is too short".to_string())
        }
        fn double(words: &mut dyn Iterator<Item = u32>) -> Result<f64, String> {
            let low = word(words)? as u64;
            let high = word(words)? as u64;
            Ok(f64::from_bits(low | high << 32))
        }
        let b = &mut self.base;
        b.engine.velocity = double(words)?;
        b.engine.inertia = double(words)?;
        b.drive.velocity = double(words)?;
        b.out_shaft_l.velocity = double(words)?;
        b.out_shaft_l.old_velocity = double(words)?;
        b.out_shaft_r.velocity = double(words)?;
        b.out_shaft_r.old_velocity = double(words)?;
        b.root_velocity = double(words)?;
        b.ratio = double(words)?;
        b.cut_off = double(words)?;
        b.last_ratio = double(words)?;
        b.gear_request.time_accumulator = double(words)?;
        b.gear_request.timeout = double(words)?;
        b.valid_shift_rpm_window = double(words)?;
        for gear in &mut b.gears {
            gear.ratio = double(words)?;
        }
        b.final_ratio = f32::from_bits(word(words)?);
        b.diff_power_ramp = f32::from_bits(word(words)?);
        b.diff_coast_ramp = f32::from_bits(word(words)?);
        b.diff_pre_load = f32::from_bits(word(words)?);
        b.total_torque = f32::from_bits(word(words)?);
        b.current_clutch_torque = f32::from_bits(word(words)?);
        b.loc_clutch = f32::from_bits(word(words)?);
        b.is_gear_grinding = word(words)? != 0;
        b.clutch_open_state = word(words)? != 0;
        b.current_gear = word(words)? as i32;
        b.gear_request.request = match word(words)? {
            1 => GearChangeRequest::Up,
            2 => GearChangeRequest::Down,
            _ => GearChangeRequest::None,
        };
        b.gear_request.requested_gear = word(words)? as i32;
        if let Some(controller) = &mut self.single_diff_lock {
            for stage in &mut controller.stages {
                stage.current_value = f32::from_bits(word(words)?);
            }
        }
        if matches!(self.base.traction_type, TractionType::Awd | TractionType::AwdNew) {
            let b = &mut self.base;
            b.out_shaft_lf.velocity = double(words)?;
            b.out_shaft_lf.old_velocity = double(words)?;
            b.out_shaft_rf.velocity = double(words)?;
            b.out_shaft_rf.old_velocity = double(words)?;
            b.awd2.max_torque = double(words)?;
            b.awd_front_share = f32::from_bits(word(words)?);
            for diff in [&mut b.awd_front_diff, &mut b.awd_rear_diff, &mut b.awd_center_diff] {
                diff.power = f32::from_bits(word(words)?);
                diff.coast = f32::from_bits(word(words)?);
                diff.preload = f32::from_bits(word(words)?);
            }
            b.awd2.current_lock_torque = f32::from_bits(word(words)?);
            for controller in [&mut self.awd_front_share_ctrl, &mut self.awd_center_lock, &mut self.awd2_ctrl].into_iter().flatten() {
                for stage in &mut controller.stages {
                    stage.current_value = f32::from_bits(word(words)?);
                }
            }
        }
        self.ac_engine.load_state(words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_like_the_machine() {
        assert_eq!(cvttss2si(18_423.9), 18_423);
        assert_eq!(cvttss2si(-0.9), 0);
        assert_eq!(cvttss2si(f32::NAN), i32::MIN);
        assert_eq!(cvttss2si(3.0e9), i32::MIN);
        assert_eq!(cvttss2si(-2_147_483_648.0), i32::MIN);
    }

    #[test]
    fn widened_float_constants() {
        assert_eq!(F64C_0_1F, 0.1f32 as f64);
        assert_eq!(F64C_0_15F, 0.15f32 as f64);
        assert_eq!(F64C_0_01F, 0.01f32 as f64);
        assert_eq!(F64C_0_003F, 0.003f32 as f64);
        assert_eq!(INV_DT, 1.0 / (0.003f32 as f64));
        // the gear logic's own literal is not the widened float of `getEngineRPM`
        assert_ne!(K2PI, 0.159_155_07f32 as f64);
        assert_eq!(K2PI as f32, 0.159_155_07f32);
    }
}
