// SPDX-License-Identifier: GPL-3.0-or-later

//! What the setup editor reads of the player's car and of the session, and what it asks of
//! them. The game's editor holds pointers into the physics car and hands it jobs through
//! `PhysicsAvatar::executeOnPhysicsThread`; here whoever runs the GUI gives it a [`SetupCar`].

use std::path::PathBuf;

pub use rustyac_physics::car::setup::SetupJob;

/// A `SetupItem` of the car's `SetupManager`, as far as the editor reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupItemInfo {
    pub name: String,
    pub units: String,
    pub label_multiplier: f32,
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
