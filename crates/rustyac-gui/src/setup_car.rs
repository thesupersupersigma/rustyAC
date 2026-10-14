// SPDX-License-Identifier: GPL-3.0-or-later

//! What the setup editor reads of the player's car and of the session, and what it asks of
//! them. The game's editor holds pointers into the physics car and hands it jobs through
//! `PhysicsAvatar::executeOnPhysicsThread`; here whoever runs the GUI gives it a [`SetupCar`].

use std::path::PathBuf;

pub use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::vecmath::Mat44f;

/// A `SetupItem` of the car's `SetupManager`, as far as the editor reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupItemInfo {
    pub name: String,
    pub units: String,
    pub label_multiplier: f32,
}

/// `TyreThermalState`, what the car's status panel shows of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreInfo {
    pub last_grain: f32,
    pub last_blister: f32,
    pub dynamic_pressure: f32,
    pub static_pressure: f32,
    /// inner, middle, outer
    pub last_set_imo: [f32; 3],
}

/// What the car's status panel reads of `CarAvatar::physicsState` every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetupCarState {
    /// `camberRAD`
    pub camber_rad: [f32; 4],
    /// `caster[0]`
    pub caster: f32,
    /// `suspensionMatrix`; the world matrix of `CarAvatar::bodyTransform`
    pub suspension_matrix: [Mat44f; 4],
    pub body_matrix: Mat44f,
    /// `tyreThermalStates`
    pub tyres: [TyreInfo; 4],
    /// `rideHeight`; `physicsInfo.minHeightM`
    pub ride_height: [f32; 2],
    pub min_height_m: f32,
}

impl Default for SetupCarState {
    fn default() -> SetupCarState {
        SetupCarState { camber_rad: [0.0; 4], caster: 0.0, suspension_matrix: [Mat44f { m: [[0.0; 4]; 4] }; 4], body_matrix: Mat44f::IDENTITY, tyres: [TyreInfo::default(); 4], ride_height: [0.0; 2], min_height_m: 0.0 }
    }
}

/// The player's car as the setup editor sees it.
pub trait SetupCar {
    /// `Car::unixName`
    fn unix_name(&self) -> String;
    /// `content/cars/<car>/data`
    fn data_path(&self) -> PathBuf;
    /// `SetupManager::items`, in their order
    fn items(&self) -> Vec<SetupItemInfo>;
    fn item(&self, index: usize) -> SetupItemInfo {
        self.items().into_iter().nth(index).unwrap_or_default()
    }
    /// `SetupItem::newValue`
    fn item_new_value(&self, index: usize) -> f32;
    /// `CarAvatar::physicsInfo.maxGear`
    fn max_gear(&self) -> i32;
    /// `PhysicsAvatar::executeOnPhysicsThread`
    fn job(&mut self, job: SetupJob);
    /// `Drivetrain::gears[index].ratio` (0 reverse, 1 neutral, 2 the first gear), `finalRatio`
    fn gear_ratio(&self, index: usize) -> f64;
    fn final_ratio(&self) -> f32;
    /// `Drivetrain::tractionType` is FWD
    fn is_front_wheel_drive(&self) -> bool;
    /// `Car::tyres[index].data.radius`
    fn tyre_radius(&self, index: usize) -> f32;
    /// `Engine::getLimiterRPM`
    fn limiter_rpm(&self) -> i32;
    /// `CarAvatar::physicsInfo.tyreCompounds`
    fn tyre_compound_names(&self) -> Vec<String>;
    /// `CarAvatar::physicsInfo.maxFuel`, `Car::requestedFuel`
    fn max_fuel(&self) -> f64;
    fn requested_fuel(&self) -> f32;
    /// `FuelLapEvaluator::getFuelPerLap`, `CarAvatar::physicsState.normalizedSplinePosition`
    fn fuel_per_lap(&self) -> f32;
    fn normalized_spline_position(&self) -> f32;
    /// `CarAvatar::isTcAvailable`, `isAbsAvailable`; `getTCMode`, `getABSMode` (the mode shown,
    /// 0 for off, and how many there are)
    fn tc_available(&self) -> bool;
    fn abs_available(&self) -> bool;
    fn tc_mode(&self) -> (u32, u32);
    fn abs_mode(&self) -> (u32, u32);
    /// `physicsInfo.hasAdjustableTurbo`, `Engine::getTurboBoostLevel`
    fn has_adjustable_turbo(&self) -> bool;
    fn turbo_boost_level(&self) -> f32;
    /// `physicsInfo.hasERS`, `hasCockpitERSDeliveryProfile`, `hasCockpitERSRecovery`,
    /// `hasCockpitERSMguHMode`; the names of `ERS::ersPowerControllers`
    fn has_ers(&self) -> bool;
    fn has_cockpit_ers_delivery_profile(&self) -> bool;
    fn has_cockpit_ers_recovery(&self) -> bool;
    fn has_cockpit_ers_mgu_h_mode(&self) -> bool;
    fn ers_power_controller_names(&self) -> Vec<String>;
    /// `CarAvatar::currentERSPowerIndex`, `currentERSRecovery`, `isHeatChargingBatteries`
    fn ers_power_index(&self) -> i32;
    fn ers_recovery(&self) -> i32;
    fn ers_heat_charging(&self) -> bool;
    /// `physicsInfo.engineBrakeSettingsCount`, `CarAvatar::currentEngineBrakeSetting`
    fn engine_brake_settings_count(&self) -> i32;
    fn engine_brake_setting(&self) -> i32;
    /// `CarAvatar::physicsState` as of now
    fn state(&self) -> SetupCarState;
}

/// What the editor reads of the session besides the car.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupLink {
    /// the name of `Sim::track`
    pub track_name: String,
    /// the folders of `content/tracks`, in the file system's order (by name)
    pub tracks: Vec<String>,
    /// rustyAC's own `setups` folder: everything the editor writes goes here
    pub setups_save: PathBuf,
    /// the game's `Documents\Assetto Corsa\setups`: only read (none: not looked at)
    pub setups_read: Option<PathBuf>,
    /// `Sim::useMousePitstop`
    pub use_mouse_pitstop: bool,
    /// `cfg/assists.ini [ASSISTS] TRACTION_CONTROL` and `ABS` (on line the server's): 0 off,
    /// 1 as the car has it, 2 on whatever the car has
    pub tc_assist: i32,
    pub abs_assist: i32,
    /// `Speed::useMPH`
    pub use_mph: bool,
    /// `RaceManager::getSessionTotalTime(0)`, ms
    pub session_total_time: i32,
    /// `QuickMenuPitstop::buildControls` was asked of these pit menus (the runner does it)
    pub quick_menu_build_controls: Vec<crate::control::Id>,
}

/// What [`SetupCar`] says of a car, read from the physics car at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupSnapshot {
    /// how many of the editor's jobs the car had done when the picture was taken (the
    /// physics thread's count; 0 where jobs are done at once)
    pub applied_jobs: u64,
    pub unix_name: String,
    pub data_path: PathBuf,
    pub items: Vec<SetupItemInfo>,
    pub new_values: Vec<f32>,
    pub max_gear: i32,
    pub gear_ratios: Vec<f64>,
    pub final_ratio: f32,
    pub front_wheel_drive: bool,
    pub tyre_radius: [f32; 4],
    pub limiter_rpm: i32,
    pub tyre_compounds: Vec<String>,
    pub max_fuel: f64,
    pub requested_fuel: f32,
    pub fuel_per_lap: f32,
    pub normalized_spline_position: f32,
    pub tc_available: bool,
    pub abs_available: bool,
    pub tc_mode: (u32, u32),
    pub abs_mode: (u32, u32),
    pub adjustable_turbo: bool,
    pub turbo_boost_level: f32,
    pub has_ers: bool,
    pub cockpit_ers_delivery_profile: bool,
    pub cockpit_ers_recovery: bool,
    pub cockpit_ers_mgu_h_mode: bool,
    pub ers_power_controller_names: Vec<String>,
    pub ers_power_index: i32,
    pub ers_recovery: i32,
    pub ers_heat_charging: bool,
    pub engine_brake_settings_count: i32,
    pub engine_brake_setting: i32,
    /// `SetupManager::minimumHeight_m`, `setupState`
    pub minimum_height_m: f32,
    pub setup_state: i32,
    /// what the status panel shows of the car's state: `Car::getTyreThermalState`, the
    /// caster (`RaceEngineer::getCasterRAD` of the left front wheel, in degrees) and the two
    /// ride heights
    pub tyres: [TyreInfo; 4],
    pub caster: f32,
    pub ride_height: [f32; 2],
}

impl SetupSnapshot {
    /// The car as it is now. `unix_name` is the name of its folder.
    pub fn of(chassis: &rustyac_physics::car::RollingChassis, unix_name: &str) -> SetupSnapshot {
        let drivetrain = chassis.drivetrain.as_deref();
        let aids = chassis.aids.as_deref();
        let ers = chassis.ers.as_ref();
        let manager = &chassis.setup_manager;
        let mut tyre_radius = [0.0; 4];
        for (radius, tyre) in tyre_radius.iter_mut().zip(&chassis.tyres) {
            *radius = tyre.data.radius;
        }
        SetupSnapshot {
            applied_jobs: 0,
            unix_name: unix_name.to_string(),
            data_path: chassis.data_path.clone(),
            items: manager.items.iter().map(|item| SetupItemInfo { name: item.name.clone(), units: item.units.clone(), label_multiplier: item.label_multiplier }).collect(),
            new_values: manager.items.iter().map(|item| item.new_value).collect(),
            max_gear: drivetrain.map_or(0, |d| d.base().gears.len() as i32 - 2),
            gear_ratios: drivetrain.map(|d| d.base().gears.iter().map(|gear| gear.ratio).collect()).unwrap_or_default(),
            final_ratio: drivetrain.map_or(0.0, |d| d.base().final_ratio),
            front_wheel_drive: drivetrain.is_some_and(|d| d.base().traction_type == rustyac_physics::car::drivetrain::TractionType::Fwd),
            tyre_radius,
            limiter_rpm: drivetrain.map_or(0, |d| d.engine().get_limiter_rpm()),
            // (`Car::Car` copies the names only when the four tyres have as many compounds)
            tyre_compounds: chassis.tyres.first().filter(|first| chassis.tyres.iter().all(|tyre| tyre.compound_defs.len() == first.compound_defs.len())).map(|tyre| tyre.compound_defs.iter().map(|def| def.name.clone()).collect()).unwrap_or_default(),
            max_fuel: chassis.max_fuel,
            requested_fuel: chassis.requested_fuel,
            fuel_per_lap: chassis.fuel_lap_evaluator.fuel_per_lap(),
            normalized_spline_position: chassis.spline_locator_data.npos,
            tc_available: aids.is_some_and(|a| a.base().traction_control.is_present),
            abs_available: aids.is_some_and(|a| a.base().abs.is_present),
            tc_mode: aids.map_or((0, 0), |a| a.base().traction_control.get_current_mode()),
            abs_mode: aids.map_or((0, 0), |a| a.base().abs.get_current_mode()),
            adjustable_turbo: drivetrain.is_some_and(|d| d.engine().turbo_adjustable_from_cockpit()),
            turbo_boost_level: drivetrain.map_or(0.0, |d| d.engine().get_turbo_boost_level()),
            has_ers: ers.is_some(),
            cockpit_ers_delivery_profile: ers.is_some_and(|e| e.cockpit_delivery_profile),
            cockpit_ers_recovery: ers.is_some_and(|e| e.cockpit_recovery),
            cockpit_ers_mgu_h_mode: ers.is_some_and(|e| e.cockpit_mgu_h_mode),
            ers_power_controller_names: ers.map(|e| e.power_controllers.iter().map(|c| c.name.clone()).collect()).unwrap_or_default(),
            ers_power_index: chassis.cockpit.ers_power_index,
            ers_recovery: chassis.cockpit.ers_recovery,
            ers_heat_charging: chassis.cockpit.ers_heat_charging,
            engine_brake_settings_count: chassis.engine_brake_settings(),
            engine_brake_setting: chassis.cockpit.engine_brake,
            minimum_height_m: manager.minimum_height_m,
            setup_state: manager.setup_state,
            tyres: std::array::from_fn(|w| {
                chassis.tyres.get(w).map_or(TyreInfo::default(), |tyre| {
                    let s = &tyre.status;
                    TyreInfo { last_grain: s.last_grain, last_blister: s.last_blister, dynamic_pressure: s.pressure_dynamic, static_pressure: s.pressure_static, last_set_imo: s.last_temp_imo }
                })
            }),
            caster: chassis.suspensions.first().map_or(0.0, |suspension| suspension.caster_rad() * f32::from_bits(0x4265_2ee1)),
            ride_height: chassis.telemetry.as_ref().map_or([0.0; 2], |writer| writer.ride_height),
        }
    }

    /// What a job changes of what the editor reads back, before the physics has done it (as
    /// the `CarAvatar` of the game keeps its own copy of the cockpit settings).
    pub fn foresee(&mut self, job: &SetupJob) {
        let cycle = |value: i32, dir: i32, count: i32| {
            let next = value + if dir > 0 { 1 } else { -1 };
            if next >= count {
                0
            } else if next < 0 {
                count - 1
            } else {
                next
            }
        };
        // a mode shown (0 off, 1 to n) a step on
        let mode = |(shown, count): (u32, u32), dir: i32| if count == 0 { (shown, count) } else { ((shown + if dir > 0 { 1 } else { count }) % (count + 1), count) };
        match *job {
            SetupJob::SetNewValue { item, value } => {
                if let Some(v) = self.new_values.get_mut(item) {
                    *v = value;
                }
            }
            SetupJob::SetRequestedFuel { value, .. } => self.requested_fuel = value,
            SetupJob::CycleTc { dir } => self.tc_mode = mode(self.tc_mode, dir),
            SetupJob::CycleAbs { dir } => self.abs_mode = mode(self.abs_mode, dir),
            SetupJob::SetTurboBoostLevel { level } => self.turbo_boost_level = level,
            SetupJob::CycleErsPower { dir } => self.ers_power_index = cycle(self.ers_power_index, dir, self.ers_power_controller_names.len() as i32),
            SetupJob::CycleErsRecovery { dir } => self.ers_recovery = cycle(self.ers_recovery, dir, 11),
            SetupJob::CycleErsHeatCharging => self.ers_heat_charging = !self.ers_heat_charging,
            SetupJob::CycleEngineBrake { dir } => self.engine_brake_setting = cycle(self.engine_brake_setting, dir, self.engine_brake_settings_count),
            SetupJob::Attach { .. } | SetupJob::SetTyreCompound { .. } | SetupJob::SetUserFfGain { .. } | SetupJob::SetFfMult { .. } | SetupJob::SetFfUpgrade { .. } | SetupJob::SetBallastKg { .. } | SetupJob::SetSteerAssist { .. } => {}
        }
    }
}

/// What does a job at once and says how the car is after it (the physics thread of the oracle).
pub type JobDoer = Box<dyn FnMut(&SetupJob) -> SetupSnapshot>;

/// What a [`SnapshotCar`] holds. Its jobs are done at once by `apply`, or (none) kept in
/// `jobs` for whoever runs the GUI to hand to the physics thread.
#[derive(Default)]
pub struct SnapshotData {
    pub snapshot: SetupSnapshot,
    pub state: SetupCarState,
    pub jobs: Vec<SetupJob>,
    /// jobs are dropped (while the editor is made: the start of the session has done them)
    pub muted: bool,
    pub apply: Option<JobDoer>,
}

/// A [`SetupCar`] that answers from a [`SetupSnapshot`].
#[derive(Clone, Default)]
pub struct SnapshotCar {
    pub data: std::rc::Rc<std::cell::RefCell<SnapshotData>>,
}

impl SetupCar for SnapshotCar {
    fn unix_name(&self) -> String {
        self.data.borrow().snapshot.unix_name.clone()
    }

    fn data_path(&self) -> PathBuf {
        self.data.borrow().snapshot.data_path.clone()
    }

    fn items(&self) -> Vec<SetupItemInfo> {
        self.data.borrow().snapshot.items.clone()
    }

    fn item_new_value(&self, index: usize) -> f32 {
        self.data.borrow().snapshot.new_values.get(index).copied().unwrap_or(0.0)
    }

    fn max_gear(&self) -> i32 {
        self.data.borrow().snapshot.max_gear
    }

    fn job(&mut self, job: SetupJob) {
        let mut data = self.data.borrow_mut();
        if data.muted {
            return;
        }
        match &mut data.apply {
            Some(apply) => {
                let snapshot = apply(&job);
                data.snapshot = snapshot;
            }
            None => {
                data.snapshot.foresee(&job);
                data.jobs.push(job);
            }
        }
    }

    fn gear_ratio(&self, index: usize) -> f64 {
        self.data.borrow().snapshot.gear_ratios.get(index).copied().unwrap_or(0.0)
    }

    fn final_ratio(&self) -> f32 {
        self.data.borrow().snapshot.final_ratio
    }

    fn is_front_wheel_drive(&self) -> bool {
        self.data.borrow().snapshot.front_wheel_drive
    }

    fn tyre_radius(&self, index: usize) -> f32 {
        self.data.borrow().snapshot.tyre_radius[index]
    }

    fn limiter_rpm(&self) -> i32 {
        self.data.borrow().snapshot.limiter_rpm
    }

    fn tyre_compound_names(&self) -> Vec<String> {
        self.data.borrow().snapshot.tyre_compounds.clone()
    }

    fn max_fuel(&self) -> f64 {
        self.data.borrow().snapshot.max_fuel
    }

    fn requested_fuel(&self) -> f32 {
        self.data.borrow().snapshot.requested_fuel
    }

    fn fuel_per_lap(&self) -> f32 {
        self.data.borrow().snapshot.fuel_per_lap
    }

    fn normalized_spline_position(&self) -> f32 {
        self.data.borrow().snapshot.normalized_spline_position
    }

    fn tc_available(&self) -> bool {
        self.data.borrow().snapshot.tc_available
    }

    fn abs_available(&self) -> bool {
        self.data.borrow().snapshot.abs_available
    }

    fn tc_mode(&self) -> (u32, u32) {
        self.data.borrow().snapshot.tc_mode
    }

    fn abs_mode(&self) -> (u32, u32) {
        self.data.borrow().snapshot.abs_mode
    }

    fn has_adjustable_turbo(&self) -> bool {
        self.data.borrow().snapshot.adjustable_turbo
    }

    fn turbo_boost_level(&self) -> f32 {
        self.data.borrow().snapshot.turbo_boost_level
    }

    fn has_ers(&self) -> bool {
        self.data.borrow().snapshot.has_ers
    }

    fn has_cockpit_ers_delivery_profile(&self) -> bool {
        self.data.borrow().snapshot.cockpit_ers_delivery_profile
    }

    fn has_cockpit_ers_recovery(&self) -> bool {
        self.data.borrow().snapshot.cockpit_ers_recovery
    }

    fn has_cockpit_ers_mgu_h_mode(&self) -> bool {
        self.data.borrow().snapshot.cockpit_ers_mgu_h_mode
    }

    fn ers_power_controller_names(&self) -> Vec<String> {
        self.data.borrow().snapshot.ers_power_controller_names.clone()
    }

    fn ers_power_index(&self) -> i32 {
        self.data.borrow().snapshot.ers_power_index
    }

    fn ers_recovery(&self) -> i32 {
        self.data.borrow().snapshot.ers_recovery
    }

    fn ers_heat_charging(&self) -> bool {
        self.data.borrow().snapshot.ers_heat_charging
    }

    fn engine_brake_settings_count(&self) -> i32 {
        self.data.borrow().snapshot.engine_brake_settings_count
    }

    fn engine_brake_setting(&self) -> i32 {
        self.data.borrow().snapshot.engine_brake_setting
    }

    fn state(&self) -> SetupCarState {
        self.data.borrow().state
    }
}
