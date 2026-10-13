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
}

/// What the editor reads of the session besides the car.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupLink {
    /// the name of `Sim::track`
    pub track_name: String,
    /// `Sim::useMousePitstop`
    pub use_mouse_pitstop: bool,
    /// `QuickMenuPitstop::buildControls` was asked of these pit menus (the runner does it)
    pub quick_menu_build_controls: Vec<crate::control::Id>,
}
