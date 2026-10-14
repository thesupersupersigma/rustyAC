// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The apps of `SystemApps.lib`: each is a `ksgui::Form` with a `render(dt)` of its own (its
//! per-frame update and its drawing at once) and sometimes mouse handlers.
//!
//! In the game an app reads `Sim` (the cars' `CarAvatar::physicsState`, the `RaceManager`,
//! the track); here it reads an [`AppLink`] that whoever runs the GUI fills every frame, and
//! what it asks of the session (a setter of the game, a file to write) is left in
//! [`AppLink::actions`] in the order the game makes the calls.
//!
//! An app's own members live beside its form ([`crate::form::FormData::app`]); the form's
//! virtual functions go to the app's [`App`] methods, whose defaults are `ksgui::Form`'s.

use std::any::Any;

use rustyac_physics::car_physics_state::CarPhysicsState;

use crate::control::{Id, Kind, MouseEvent};
use crate::leaderboard::RaceLap;
use crate::gui::Gui;
use crate::painter::Painter;

/// A call an app makes on the session, for whoever runs the GUI to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    /// a file the app writes: its name as the game names it (under `Documents\Assetto Corsa\`
    /// for a settings file, under the game's folder for a track's map) and its bytes
    WriteFile { path: String, bytes: Vec<u8> },
}

/// One car as the apps read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppCar {
    /// `CarAvatar::physicsState`
    pub state: CarPhysicsState,
    /// `CarAvatar::bodyTransform`'s matrix (the car as it is drawn)
    pub body_matrix: rustyac_physics::vecmath::Mat44f,
    /// `RaceManager::getCurrentLap(car)` (the running lap's time and finished sector times),
    /// `getBestLap(car)`, `getLastLap(car)`
    pub current_lap: RaceLap,
    pub best_lap: RaceLap,
    pub last_lap: RaceLap,
    /// `RaceManager::getSplit(car)`: the last lap against the best before it (ms; -1: none)
    pub split: i32,
    /// `CarAvatar::isCurrentLapValid`
    pub current_lap_valid: bool,
    /// `RaceManager::getLapCount(car)`
    pub lap_count: u32,
    /// `RaceManager::getCarLeaderboardPosition(car)` (from 1; 0 or less: none),
    /// `getCarRealTimePosition(car)` (from 0)
    pub leaderboard_position: i32,
    pub realtime_position: i32,
    /// `CarAvatar::physicsInfo`: `maxFuel` (litres), `engineDamageRPM`
    pub max_fuel: f64,
    pub engine_damage_rpm: i32,
    /// `CarAvatar::isAbsEnabled`, `isAbsInAction`, `isTcEnabled`, `isTcInAction`
    pub abs_enabled: bool,
    pub abs_in_action: bool,
    pub tc_enabled: bool,
    pub tc_in_action: bool,
    /// `physicsInfo`: `hasKERS`, `hasERS`, `kersMaxJ`, `ersMaxJ`
    pub has_kers: bool,
    pub has_ers: bool,
    pub kers_max_j: f32,
    pub ers_max_j: f32,
    /// `CarAvatar::isEdlEnabled`, `getEdlOutLevel`; `physicsInfo.hasDRS`
    pub edl_enabled: bool,
    pub edl_out_level: f32,
    pub has_drs: bool,
    /// the controls' `getFFGlobalGain` (none: the car has no controls provider, no physics),
    /// `raceEngineer->car->userFFGain` (none: no race engineer)
    pub ff_global_gain: Option<f32>,
    pub user_ff_gain: Option<f32>,
    /// `CarAvatar::getCurrentCompoundShortName`, `getCurrentCompoundIdealPressure(tyre)`
    pub compound_short_name: String,
    pub ideal_pressure: [i32; 4],
    /// `CarAvatar::isConnected`
    pub connected: bool,
    /// `CarAvatar::isInPit`, `isInPitlane`
    pub in_pit: bool,
    pub in_pitlane: bool,
    /// `CarAvatar::getDriverName`, `unixName`, `guiName`, `guiShortName`
    pub driver_name: String,
    pub unix_name: String,
    pub gui_name: String,
    pub gui_short_name: String,
}

/// What the apps read of `Sim`, and what they ask of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppLink {
    /// `Sim::cars`
    pub cars: Vec<AppCar>,
    /// `Sim::focusedCarIndex`
    pub focused_car: usize,
    /// `Sim::connectedCarsCount`
    pub connected_cars_count: i32,
    /// `RaceManager::getCurrentSession`: `sessionType`, `isTimedRace`, `laps`
    pub session_type: i32,
    pub is_timed_race: bool,
    pub session_laps: i32,
    /// `RaceManager::getSessionCount`, `getTimetoWait` (ms)
    pub session_count: u32,
    pub time_to_wait: f64,
    /// `Sim` made a `PushToPassManager` (every car has push-to-pass)
    pub has_p2p: bool,
    /// `RaceManager::getSessionTimeLeft` (ms), `getTimeToSessionStart` (ms)
    pub session_time_left: f64,
    pub time_to_session_start: f64,
    /// `ReplayManager::isInReplaymode`
    pub in_replay: bool,
    /// `Spline::length` of the track's AI spline (m)
    pub ai_spline_length: f32,
    /// `PhysicsAvatar::getTemperatureInfo` (ambient, road), `getDynamicTrackStatus().currentGripLevel`,
    /// `windData` (`speed.value` m/s, `directionDEG`)
    pub ambient_temperature: f32,
    pub road_temperature: f32,
    pub grip: f32,
    pub wind_speed: f32,
    pub wind_direction_deg: f32,
    /// `Game::gameTime.now` (ms), `startTime` (the clock when the game started)
    pub game_time: f64,
    pub game_start_time: f64,
    /// `GraphicsManager::lightingSettings.angle` (the sun's), `SunAnimator::getVelocityMultiplier`
    /// (none: there is no sun animator)
    pub sun_angle: f32,
    pub time_mult: Option<f32>,
    /// `GameplaySettings` / `ksIsMPH`: speeds in miles an hour
    pub use_mph: bool,
    pub actions: Vec<AppAction>,
}

impl AppLink {
    /// `Sim::getCar(focusedCarIndex)`: the car the apps show (a car at the origin when there
    /// is none).
    pub fn focused(&self) -> AppCar {
        self.cars.get(self.focused_car).cloned().unwrap_or_default()
    }
}

/// An app's own members and its overrides of `ksgui::Form`'s virtual functions.
pub trait App: Any {
    fn as_any(&mut self) -> &mut dyn Any;

    /// vtable +0x18.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
    }

    /// vtable +0x50.
    fn on_mouse_down(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) -> bool {
        gui.control_on_mouse_down(id, e)
    }

    /// vtable +0x58.
    fn on_mouse_up(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        gui.control_on_mouse_up(id, e)
    }

    /// vtable +0x60.
    fn on_mouse_move(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        gui.form_on_mouse_move(id, e)
    }

    /// vtable +0x68.
    fn on_mouse_wheel(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        gui.control_on_mouse_wheel(id, e)
    }

    /// vtable +0x40.
    fn on_visible_changed(&mut self, gui: &mut Gui, id: Id, _visible: bool) {
        gui.form_on_visible_changed(id)
    }

    /// vtable +0x98.
    fn scale_by_mult(&mut self, gui: &mut Gui, id: Id, m: f32) {
        gui.form_scale_by_mult(id, m)
    }

    /// `Form::shutdown` (vtable +0xb0): the game is closing.
    fn shutdown(&mut self, _gui: &mut Gui, _id: Id) {}

    /// A handler of `RaceManager::evOnLapCompleted` (`OnLapCompletedEvent`: the car's index,
    /// the lap's time).
    fn on_lap_completed(&mut self, _gui: &mut Gui, _id: Id, _car: u32, _time: u32) {}

    /// A handler of `Sim::evOnNewSession`.
    fn on_new_session(&mut self, _gui: &mut Gui, _id: Id) {}

    /// A handler of `Sim::evOnReplayStatusChanged`: the replay began (true) or ended.
    fn on_replay_status(&mut self, _gui: &mut Gui, _id: Id, _entered: bool) {}
}

impl Gui {
    /// The app on a form, if the form is one.
    pub fn is_app(&self, id: Id) -> bool {
        matches!(&self.controls[id].kind, Kind::Form(data) if data.app.is_some())
    }

    /// Runs `f` on the form's app (taken out of the form while it runs, so that it may change
    /// the GUI); none: the form is a plain one.
    pub fn with_app<R>(&mut self, id: Id, f: impl FnOnce(&mut dyn App, &mut Gui) -> R) -> Option<R> {
        let mut app = match &mut self.controls[id].kind {
            Kind::Form(data) => data.app.take()?,
            _ => return None,
        };
        let r = f(&mut *app, self);
        if let Kind::Form(data) = &mut self.controls[id].kind {
            data.app = Some(app);
        }
        Some(r)
    }

    /// The app of a form as its own type.
    pub fn app_mut<T: App>(&mut self, id: Id) -> Option<&mut T> {
        match &mut self.controls[id].kind {
            Kind::Form(data) => data.app.as_mut()?.as_any().downcast_mut::<T>(),
            _ => None,
        }
    }

    /// Hangs an app's members on its form.
    pub fn set_app(&mut self, id: Id, app: Box<dyn App>) {
        if let Kind::Form(data) = &mut self.controls[id].kind {
            data.app = Some(app);
        }
    }

    /// `RaceManager::evOnLapCompleted` as the apps hear it, in the order they were made.
    pub fn apps_on_lap_completed(&mut self, car: u32, time: u32) {
        for id in 0..self.controls.len() {
            if self.is_app(id) {
                self.with_app(id, |app, gui| app.on_lap_completed(gui, id, car, time));
            }
        }
    }

    /// `Sim::evOnNewSession` as the apps hear it.
    pub fn apps_on_new_session(&mut self) {
        for id in 0..self.controls.len() {
            if self.is_app(id) {
                self.with_app(id, |app, gui| app.on_new_session(gui, id));
            }
        }
    }

    /// `Sim::evOnReplayStatusChanged` as the apps hear it.
    pub fn apps_on_replay_status(&mut self, entered: bool) {
        for id in 0..self.controls.len() {
            if self.is_app(id) {
                self.with_app(id, |app, gui| app.on_replay_status(gui, id, entered));
            }
        }
    }

    /// `Form::shutdown` of every app (the game's end: the apps that write their settings).
    pub fn shutdown_apps(&mut self) {
        for id in 0..self.controls.len() {
            if self.is_app(id) {
                self.with_app(id, |app, gui| app.shutdown(gui, id));
            }
        }
    }
}

/// `ksgui::Form::setSize`'s callers in the apps call it through the vtable (+0x38).
pub fn form_size(gui: &mut Gui, id: Id, w: f32, h: f32) {
    gui.set_size(id, w, h);
}
