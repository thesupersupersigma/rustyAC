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
    /// `WritePrivateProfileStringW(section, key, value, <Documents>\Assetto Corsa\<file>)`
    /// (see [`write_profile_string`])
    WriteProfileString { file: String, section: String, key: String, value: String },
    /// `PostProcessEffectsUpdater::loadConfigurationFile` 0x1400abb70: a post-processing filter
    /// (YEBIS')
    LoadPostProcessFilter(String),
    /// `AudioEngine::setVolume` 0x1401fbdf0 (the master level, 0..1)
    SetMasterVolume(f32),
    /// one level of every car's `CarAudioFMOD::mixVolumes`; for the transmission -1 means
    /// "the engine's level now", any other value is stored times 3
    SetMixLevel { level: MixLevel, value: f32 },
    /// `CameraDrivableManager::setDrivableCameraDefAt`: a car's bonnet (0) or bumper (1) view
    /// hears the car from outside
    SetCameraSound { car: usize, which: usize, external: bool },
    /// the player's car's `Car::userFFGain` (`CarAvatar::setUserFFGain`'s job)
    SetUserFfGain(f32),
    /// the player's car's `Car::ffMult` (`CarAvatar::setFFMult`'s job)
    SetFfMult(f32),
    /// a gain of the wheel's `DICarControl::ffUpgrades` (0 curbs, 1 road, 2 slips, 3 ABS)
    SetFfUpgrade { which: usize, value: f32 },
    /// `SystemMessage::addMessage(title, text, eDefault)` 0x1401b61c0
    AddMessage { title: String, text: String },
    /// the player's car's `Car::ballastKG` (`CarAvatar::setBallastKG`'s job)
    SetBallastKg(f32),
    /// the player's car's `CarAvatar::driverEyesPosition` (the 3D model's frame)
    SetDriverEyes([f32; 3]),
    /// `CameraOnBoard::onBoardFov` (degrees), `customCameraSettings[0].pitch` (radians)
    SetOnboardFov(f32),
    SetOnboardPitch(f32),
    /// `KeyboardControlsDebug`'s spinners: one of the keyboard's steering parameters
    /// (`KeyboardTuning`'s order)
    SetKeyboardTuning { index: usize, value: f32 },
    /// `X360ControllerDebug`'s buttons: one of the pad's steering parameters (`PadTuning`'s order)
    SetPadTuning { index: usize, value: f32 },
    /// `FormCamera::selectCamera`: `ACCameraManager::setMode(mode)` then the drivable view
    /// (mode 0, `setDrivableCarIndex`) or the car camera (mode 1, `setCameraCarIndex`) `index`
    SelectCamera { mode: i32, index: i32 },
    /// `ACCameraManager::setMode(mode, false, false)` (the Photo mode's Start)
    SetCameraMode(i32),
    /// `PhysicsAvatar::pausePhysics(paused)`
    PausePhysics(bool),
    /// `ScreenCapturer::takeScreen(L"")`: a screenshot where F8 puts them
    TakeScreenshot,
    /// `DeleteFileW(<Documents>\Assetto Corsa\<path>)`
    DeleteFile { path: String },
    /// Task 44c: FormAI's recorder buttons and AI driver values
    Ai(crate::app_ai::AiJob),
}

/// A level of `MixVolumes` (`CarAudioFMOD+0x118`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixLevel {
    Wind,
    Tyres,
    Surfaces,
    Dirt,
    Engine,
    Transmission,
    Opponents,
}

/// What Windows' `WritePrivateProfileStringW` makes of a file (`None`: there is none) when it
/// sets `key` of `section` to `value`, as the game's settings apps write: the first section of
/// that name (no case, blanks around it ignored) gets it; a line of that key (no case, blanks
/// ignored, an `=` in it) keeps everything up to its `=` and takes the value up to its line's
/// end; a new key goes after the section's last line that is not empty, or at the file's end
/// when the section is the file's last; a new section goes at the end. New lines end in CRLF.
pub fn write_profile_string(file: Option<&[u8]>, section: &str, key: &str, value: &str) -> Vec<u8> {
    let text = file.unwrap_or_default();
    // the lines with their ends
    let mut lines: Vec<(usize, usize, usize)> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let end = text[at..].iter().position(|b| *b == b'\n').map_or(text.len(), |p| at + p + 1);
        let mut body = end;
        while body > at && (text[body - 1] == b'\n' || text[body - 1] == b'\r') {
            body -= 1;
        }
        lines.push((at, body, end));
        at = end;
    }
    let trim = |b: &[u8]| -> String { String::from_utf8_lossy(b).trim().to_ascii_lowercase() };
    let header = |l: &(usize, usize, usize)| -> Option<String> {
        let t = String::from_utf8_lossy(&text[l.0..l.1]).trim_start().to_string();
        let t = t.strip_prefix('[')?;
        let close = t.find(']')?;
        Some(t[..close].trim().to_ascii_lowercase())
    };
    let new_line = format!("{key}={value}\r\n").into_bytes();
    let terminated = |out: &mut Vec<u8>| {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.extend_from_slice(b"\r\n");
        }
    };
    let Some(start) = lines.iter().position(|l| header(l).as_deref() == Some(&section.trim().to_ascii_lowercase())) else {
        let mut out = text.to_vec();
        terminated(&mut out);
        out.extend_from_slice(format!("[{section}]\r\n").as_bytes());
        out.extend_from_slice(&new_line);
        return out;
    };
    let next = lines[start + 1..].iter().position(|l| header(l).is_some()).map(|p| start + 1 + p);
    for l in &lines[start + 1..next.unwrap_or(lines.len())] {
        let body = &text[l.0..l.1];
        if let Some(eq) = body.iter().position(|b| *b == b'=') {
            if trim(&body[..eq]) == key.trim().to_ascii_lowercase() {
                let mut out = text[..l.0 + eq + 1].to_vec();
                out.extend_from_slice(value.as_bytes());
                out.extend_from_slice(&text[l.1..]);
                return out;
            }
        }
    }
    let mut out;
    match next {
        None => {
            out = text.to_vec();
            terminated(&mut out);
            out.extend_from_slice(&new_line);
        }
        Some(next) => {
            let last = (start..next).rev().find(|i| !trim(&text[lines[*i].0..lines[*i].1]).is_empty()).unwrap_or(start);
            out = text[..lines[last].2].to_vec();
            terminated(&mut out);
            out.extend_from_slice(&new_line);
            out.extend_from_slice(&text[lines[last].2..]);
        }
    }
    out
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
    /// the car has physics (`CarAvatar+0x1168`); `CarAvatar::userFFGain` (the avatar's copy of
    /// the user's gain), `Car::ffMult`, `Car::steerAssist`; `CarAvatar::configName`
    pub has_physics: bool,
    pub avatar_ff_gain: f32,
    pub ff_mult: f32,
    pub steer_assist: f32,
    pub config_name: String,
    /// `CarAvatar::getERSPower`'s name, `currentERSRecovery`, `currentEngineBrakeSetting`,
    /// `isHeatChargingBatteries`
    pub ers_power_name: String,
    pub ers_recovery: i32,
    pub engine_brake_setting: i32,
    pub ers_heat_charging: bool,
    /// `CarAvatar::getCurrentCompoundShortName`, `getCurrentCompoundIdealPressure(tyre)`
    pub compound_short_name: String,
    pub ideal_pressure: [i32; 4],
    /// `CarAvatar::isConnected`
    pub connected: bool,
    /// `CarAvatar::pitPosition`'s place (M41, M42, M43): `isInPit` measures from it
    pub pit_position: [f32; 3],
    /// `CarAvatar::getAILapsToComplete` (none: -1, the car has no physics), `isRequestingPitStop`
    pub ai_laps_to_complete: Option<i32>,
    pub requesting_pit_stop: bool,
    /// `CarAvatar::getPackerRange` of each wheel (none: the car has no physics: 0)
    pub packer_range: Option<[f32; 4]>,
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
    /// a car's `DrivableCameraDef` `bonnetExternalSound` (+0x35), `bumperExternalSound` (+0x34)
    pub camera_sounds: Vec<[bool; 2]>,
    /// `AudioEngine::getDriverNum` / `getDriverInfo`: the sound cards' names
    pub audio_drivers: Vec<String>,
    /// Documents `cfg/audio.ini` as it is now (`None`: there is none)
    pub audio_ini: Option<String>,
    /// `Game::gameTime.deltaT` (s); the wheel's `DICarControl::ffUpgrades` (curbs, road, slips,
    /// ABS; none: the player drives without a wheel)
    pub game_dt: f32,
    pub ff_upgrades: Option<[f32; 4]>,
    /// the player's car's `driverEyesPosition`, `CameraOnBoard::onBoardFov`,
    /// `customCameraSettings[0].pitch`; `DriverModel::getEyesPosition` in the body node's frame
    /// (none: no driver model); Ctrl held (`GetAsyncKeyState(VK_CONTROL)`)
    pub driver_eyes: [f32; 3],
    pub onboard_fov: f32,
    pub onboard_pitch: f32,
    pub auto_eyes: Option<[f32; 3]>,
    /// `CameraDrivableManager::chaseCamData[0]` (distance, height, pitch in radians: the
    /// ChaseCam Editor changes it), and whether the first chase camera is on screen
    /// (`ACCameraManager::mode == eDrivable`, `currentMode == eChase`, `chaseIndex == 0`)
    pub chase_cam: (f32, f32, f32),
    pub chase_cam_active: bool,
    /// online: `ACClient::getPing(car 0)` (the client's own ping), the second car's
    /// `NetCarStateProvider::errorVector` (the Net monitor reads them)
    pub ping: i32,
    pub net_error_vector: [f32; 3],
    /// the player's controls if they are the keyboard's (`KeyboardCarControl`): its steering
    /// parameters, for "Keyboard debug"
    pub keyboard: Option<crate::app_keyboard_debug::KeyboardTuning>,
    /// the player's controls if they are the Xbox pad's (`JoypadCarControl`), for "X360
    /// Controller debug"
    pub pad: Option<crate::app_pad_debug::PadTuning>,
    /// What the Camera editor reads and changes (None: no editor; the game makes it only with
    /// post-processing on)
    pub camera_editor: Option<crate::app_camera_editor::CameraEditorLink>,
    /// Task 44: FormDriverModel's car 0 driver
    pub driver_model: Option<crate::app_driver_model::DriverModelLink>,
    /// What the Photo mode reads and changes (the scene camera, the exposure, the sun)
    pub photo: crate::app_photo_mode::PhotoLink,
    pub ctrl_down: bool,
    /// Task 44c: what FormAI reads of the focused car and the track's line
    pub ai: crate::app_ai::AiAppView,
    /// `RaceManager::penaltiesEnabled`
    pub penalties_enabled: bool,
    /// Task 33: what the statistics apps read
    pub stats: crate::app_stats::AppStats,
    /// Task 33b: `FormTelemetry`'s channels of the player's car
    pub telemetry: crate::app_telemetry::TelemetryFeed,
    /// `TyreWearingDebug`'s four tyres of the player's car (None: no car)
    pub tyre_debug: Option<[crate::app_tyre_debug::TyreDebugRow; 4]>,
    /// The player's speed, wheel loads and compounds for the Tyre tester (None: no car).
    pub tyre_tester: Option<crate::app_tyre_tester::TyreTesterCar>,
    /// Task 43c: ACSuspension's player car facts and wheel nodes
    pub suspension: Option<crate::app_suspension::SuspensionLink>,
    /// `SetupDebug`'s members of the focused car's `Car` (None: no car)
    pub setup_debug: Option<crate::app_setup_debug::SetupDebugCar>,
    /// `CarPhysicsForm`'s numbers of the player's car beside its physics state
    pub car_physics: crate::app_car_physics::CarPhysicsExtra,
    /// `ACDCarEngineer`'s inputs of the player's car
    pub car_engineer: crate::app_car_engineer::CarEngineer,
    /// `FormWings`' rows of the player's car
    pub wings: Vec<crate::app_wings::WingRow>,
    /// `GPUProfiler::getResults`: the last measured frame's rows
    pub gpu_profile: Vec<crate::app_gpu_profiler::GpuQueryRow>,
    /// `GPUProfiler::setActive(true)`, as `GPUProfilerApp::render` calls it (never cleared)
    pub gpu_profiler_active: bool,
    /// `Sim::track`'s folder name (`TrackAvatar` GameObject name) and layout (`getConfig`)
    pub track_name: String,
    pub track_config: String,
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

    /// `Form::onTitleClicked` (vtable +0xa8): the form is caught by the mouse.
    fn on_title_clicked(&mut self, gui: &mut Gui, id: Id, ev: &crate::control::Clicked) {
        gui.form_on_title_clicked(id, ev);
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

    /// `MicroSectors::update` 0x14009fe20 of a frame, on the cars as `AppLink` has them.
    pub fn micro_sectors_update(&mut self) {
        let cars: Vec<crate::micro_sectors::MsCar> = self
            .apps
            .cars
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let m = &c.state.world_matrix.m;
                crate::micro_sectors::MsCar { guid: i as i32, in_pit: crate::micro_sectors::is_in_pit([m[3][0], m[3][1], m[3][2]], c.pit_position), npos: c.state.normalized_spline_position, lap_time: c.state.lap_time }
            })
            .collect();
        let tts = self.apps.time_to_session_start;
        self.micro_sectors.update(tts, &cars);
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

#[cfg(test)]
mod tests {
    use super::write_profile_string;

    /// What Windows 10's `WritePrivateProfileStringW(L"LEVELS", L"MASTER", L"0.700000", file)`
    /// made of each file. (Not followed: a key line without `=` in the last section, before
    /// which Windows puts the new key.)
    #[test]
    fn as_windows_writes() {
        let cases: [(Option<&[u8]>, &[u8]); 16] = [
            (None, b"[LEVELS]\r\nMASTER=0.700000\r\n"),
            (Some(b"[A]\r\nX=1\r\n"), b"[A]\r\nX=1\r\n[LEVELS]\r\nMASTER=0.700000\r\n"),
            (Some(b"[A]\r\nX=1"), b"[A]\r\nX=1\r\n[LEVELS]\r\nMASTER=0.700000\r\n"),
            (Some(b"[A]\r\nX=1\r\n\r\n"), b"[A]\r\nX=1\r\n\r\n[LEVELS]\r\nMASTER=0.700000\r\n"),
            (Some(b"[LEVELS]\r\nmaster = 1\r\nWIND=0.5\r\n"), b"[LEVELS]\r\nmaster =0.700000\r\nWIND=0.5\r\n"),
            (Some(b"[LEVELS]\nMASTER=1\nWIND=0.5\n"), b"[LEVELS]\nMASTER=0.700000\nWIND=0.5\n"),
            (Some(b"[levels]\r\nWIND=0.5\r\n"), b"[levels]\r\nWIND=0.5\r\nMASTER=0.700000\r\n"),
            (Some(b"[ LEVELS ]\r\nWIND=0.5\r\n"), b"[ LEVELS ]\r\nWIND=0.5\r\nMASTER=0.700000\r\n"),
            (Some(b"[LEVELS]\r\n;c\r\nWIND=0.5\r\n;d\r\n\r\n"), b"[LEVELS]\r\n;c\r\nWIND=0.5\r\n;d\r\n\r\nMASTER=0.700000\r\n"),
            (Some(b"[LEVELS]\r\nWIND=1\r\n\r\n[B]\r\nY=2\r\n"), b"[LEVELS]\r\nWIND=1\r\nMASTER=0.700000\r\n\r\n[B]\r\nY=2\r\n"),
            (Some(b"[LEVELS]\r\nWIND=1"), b"[LEVELS]\r\nWIND=1\r\nMASTER=0.700000\r\n"),
            (Some(b"[LEVELS]\r\nWIND=1\r\n\r\n\r\n[B]\r\nY=2\r\n"), b"[LEVELS]\r\nWIND=1\r\nMASTER=0.700000\r\n\r\n\r\n[B]\r\nY=2\r\n"),
            (Some(b"[LEVELS]\nWIND=1\n\n[B]\nY=2\n"), b"[LEVELS]\nWIND=1\nMASTER=0.700000\r\n\n[B]\nY=2\n"),
            (Some(b"x\r\n[LEVELS]\r\n  MASTER  =  9 ;c\r\n"), b"x\r\n[LEVELS]\r\n  MASTER  =0.700000\r\n"),
            (Some(b"[LEVELS]\r\nWIND=1\r\n[LEVELS]\r\nMASTER=2\r\n"), b"[LEVELS]\r\nWIND=1\r\nMASTER=0.700000\r\n[LEVELS]\r\nMASTER=2\r\n"),
            (Some(b"[LEVELS]\r\n\r\n"), b"[LEVELS]\r\n\r\nMASTER=0.700000\r\n"),
        ];
        for (i, (file, want)) in cases.iter().enumerate() {
            let got = write_profile_string(*file, "LEVELS", "MASTER", "0.700000");
            assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(want), "case {i}");
        }
    }
}
