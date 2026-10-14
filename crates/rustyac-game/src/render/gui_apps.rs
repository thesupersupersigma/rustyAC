// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's apps in rustyAC's frame: made in the order `Sim::Sim` makes them (the developers' apps
//! are not made: Task 33), fed every frame what the game's apps read of `Sim` (the cars as
//! the picture has them, the referee's standings and laps), and what they write kept in
//! rustyAC's own folders.
//!
//! The apps are made at the first frame rather than with the GUI: some read the cars (their
//! KERS / ERS, their engine's turbos) as the session has them.

use std::path::{Path, PathBuf};

use rustyac_gui::apps::{AppAction, AppCar, AppLink};
use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::leaderboard::RaceLap;
use rustyac_gui::painter::Painter;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use super::gui::GuiSetup;
use super::hud::HudInfo;
use crate::view::CarView;

/// The apps of the session.
#[derive(Default)]
pub struct GameApps {
    /// every app made, by the name the oracle and `--gui-do app:<name>` use
    pub made: Vec<(&'static str, Id)>,
    /// every car's lap count as of the last frame (a lap is over when it grows)
    laps: Vec<u32>,
    /// the referee's count of new sessions as of the last frame
    sessions: Option<u32>,
    /// in a replay as of the last frame
    replay: bool,
    /// `--gui-do app:<name>` / `show:<name>` given before the apps were made: (name, show)
    pub pending: Vec<(String, bool)>,
    /// the AI line's length (Realtime's gaps); every car has DRS (`physicsInfo.hasDRS`: its
    /// `drs.ini`); the player's controls' force feedback gain (`getFFGlobalGain`: a wheel's
    /// `controls.ini [STEER] FF_GAIN`, 1 for the others)
    ai_spline_length: f32,
    has_drs: Vec<bool>,
    ff_gain: f32,
    /// the Audio app's calls on the sound (master level, mix levels, views), for the sound to
    /// carry out
    pub sound_actions: Vec<AppAction>,
    /// the sound as the Audio app reads it: the sound cards' names, the views' "external" flags
    pub sound: AppSound,
    /// `cfg/audio.ini` as it is (rustyAC's own, else Documents'), kept as the app writes it
    audio_ini: Option<String>,
    /// the force feedback app's jobs for the physics thread (`PhysicsAvatar::executeOnPhysicsThread`)
    pub ffb_jobs: Vec<rustyac_physics::car::setup::SetupJob>,
    /// the wheel's `ffUpgrades` (controls.ini `[FF_ENHANCEMENT]` with INPUT_METHOD=WHEEL, then the
    /// app's changes); the messages the apps asked for (title, text)
    ff_upgrades: Option<[f32; 4]>,
    pub messages: Vec<(String, String)>,
    /// `Game::gameTime.deltaT` of this frame
    pub game_dt: f32,
    /// the player's car's eye point (model frame), the cockpit view's field of view and pitch
    /// (the camera's, given at the start; then as Onboard settings changes them), changed in
    /// this frame; Ctrl held
    pub onboard: ([f32; 3], f32, f32),
    pub onboard_changed: bool,
    pub ctrl_down: bool,
}

fn read(path: PathBuf) -> Option<IniReader> {
    IniReader::load(&path).ok().filter(|ini| ini.ready)
}

impl GameApps {
    pub fn app(&self, name: &str) -> Option<Id> {
        self.made.iter().find(|a| a.0 == name).map(|a| a.1)
    }

    /// The apps `Sim::Sim` 0x140192070 makes, in its order, each put on the game screen as
    /// `Sim::addForm` 0x140197ae0 does. `gui.apps` holds the session as it begins.
    pub fn make(&mut self, gui: &mut Gui, painter: &mut dyn Painter, screen: Id, setup: &GuiSetup, leaderboard: Id) {
        let system = |name: &str| read(setup.game.join("system/cfg").join(name));
        let own_or_documents = |name: &str| read(setup.cfg_dir.join(name)).or_else(|| setup.documents_cfg.as_ref().and_then(|folder| read(folder.join(name))));
        let mut add = |gui: &mut Gui, painter: &mut dyn Painter, name: &'static str, id: Id| {
            if name != "leaderboard" {
                gui.game_screen_add_control(painter, screen, id, true);
            }
            self.made.push((name, id));
        };
        let folder = setup.game.join("content/tracks").join(&setup.track_name);
        let folder = if setup.track_layout.is_empty() { folder } else { folder.join(&setup.track_layout) };
        self.ai_spline_length = rustyac_physics::track::spline::AiSpline::load(&folder.join("ai/fast_lane.ai")).map(|ai| ai.spline.length()).unwrap_or(0.0);
        self.has_drs = setup.data_paths.iter().map(|data| rustyac_content::vfs::is_file(&data.join("drs.ini"))).collect();
        let controls = own_or_documents("controls.ini");
        self.ff_gain = match &controls {
            Some(ini) if ini.get_string("HEADER", "INPUT_METHOD") == "WHEEL" => ini.get_float("STEER", "FF_GAIN").unwrap_or(0.0),
            _ => 1.0,
        };
        // ACHideAll: made before every other app, its icon the first of the bar
        let id = gui.new_hide_all_app(painter, screen);
        add(gui, painter, "hideall", id);
        let id = gui.new_essentials_app(painter);
        add(gui, painter, "essentials", id);
        let id = gui.new_tyres_app(painter, system("tyres_app.ini").as_ref());
        add(gui, painter, "tyres", id);
        add(gui, painter, "leaderboard", leaderboard);
        gui.game_screen_add_control(painter, screen, leaderboard, true);
        let id = gui.new_laps_app(painter);
        add(gui, painter, "laps", id);
        // FormVolumes (after FormHDR, made only with a Yebis camera)
        self.audio_ini = own_or_documents_text(setup, "audio.ini").map(|b| String::from_utf8_lossy(&b).into_owned());
        let id = gui.new_volumes_app(painter);
        add(gui, painter, "volumes", id);
        // ACHelp reads help/<acLocaleGetCurrentLocale>.tag
        let help = std::fs::read(setup.game.join(format!("system/locales/help/{}.tag", setup.language))).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let id = gui.new_help_app(painter, &help);
        add(gui, painter, "help", id);
        let id = gui.new_time_flow_app(painter);
        add(gui, painter, "timeflow", id);
        // FFBForm (after DriverEyesPositioner, not ported): the player's car; its car.ini a
        // loose file of the game's folder or not, its wheel's effects
        self.ff_upgrades = match &controls {
            Some(ini) if ini.get_string("HEADER", "INPUT_METHOD") == "WHEEL" => {
                let enhancement = ini.has_section("FF_ENHANCEMENT");
                let get = |key: &str| if enhancement { ini.get_float("FF_ENHANCEMENT", key).unwrap_or(0.0) } else { 0.0 };
                Some([get("CURBS"), get("ROAD"), get("SLIPS"), if ini.has_key("FF_ENHANCEMENT", "ABS") { ini.get_float("FF_ENHANCEMENT", "ABS").unwrap_or(0.0) } else { 0.0 }])
            }
            _ => None,
        };
        gui.apps.ff_upgrades = self.ff_upgrades;
        let unix = setup.cars.first().map(|c| c[2].clone()).unwrap_or_default();
        let loose = setup.game.join("content/cars").join(&unix).join("data/car.ini").is_file();
        // DriverEyesPositioner (after FormOpenVR and PostProcessFilterSelector, not made here):
        // the car.ini's eye point and pitch, the game folder's camera_onboard.ini field of view
        let car_ini = crate::input::ini::ControlsIni::load(&setup.cfg_dir.join("content/cars").join(&unix).join("data/car.ini")).ok().or_else(|| setup.data_paths.first().and_then(|data| crate::input::ini::ControlsIni::load_car_data(&data.join("car.ini")).ok()));
        let onboard_ini = read(setup.game.join("cfg/camera_onboard.ini"));
        let defaults = rustyac_gui::app_eyes::EyesDefaults {
            eyes: car_ini.as_ref().map(|ini| {
                let v: Vec<f32> = ini.get_string("GRAPHICS", "DRIVEREYES").split(',').filter_map(|p| p.trim().parse().ok()).collect();
                if v.len() == 3 { [v[0], v[1], v[2]] } else { [0.0; 3] }
            }),
            pitch_degrees: car_ini.as_ref().map(|ini| ini.get_float("GRAPHICS", "ON_BOARD_PITCH_ANGLE")),
            fov: onboard_ini.as_ref().map(|ini| ini.get_float("MODE", "FOV").unwrap_or(0.0)),
        };
        gui.apps.driver_eyes = self.onboard.0;
        gui.apps.onboard_fov = self.onboard.1;
        gui.apps.onboard_pitch = self.onboard.2;
        let id = gui.new_eyes_app(painter, defaults, loose);
        add(gui, painter, "eyes", id);
        let id = gui.new_ffb_app(painter, loose, self.ff_upgrades);
        add(gui, painter, "ffb", id);
        let id = gui.new_pedals_app(painter, own_or_documents("controls.ini").as_ref());
        add(gui, painter, "pedals", id);
        let id = gui.new_realtime_app(painter);
        add(gui, painter, "realtime", id);
        let id = gui.new_performance_delta_app(painter);
        add(gui, painter, "delta", id);
        let engines: Vec<Option<IniReader>> = setup.data_paths.iter().map(|data| read(data.join("engine.ini"))).collect();
        let id = gui.new_gears_app(painter, &engines);
        add(gui, painter, "gears", id);
        let id = Self::track_map(gui, painter, setup);
        add(gui, painter, "trackmap", id);
        let id = gui.new_electronics_app(painter);
        add(gui, painter, "electronics", id);
        let id = gui.new_mgu_app(painter);
        add(gui, painter, "mgu", id);
        // FormBallast: offline only (no ACClient)
        if !setup.online {
            let id = gui.new_ballast_app(painter);
            add(gui, painter, "ballast", id);
        }
        let id = gui.new_race_position_app(painter);
        add(gui, painter, "position", id);
        let sections = rustyac_gui::app_small::read_sections(read(folder.join("data/sections.ini")).as_ref());
        let id = gui.new_track_description_app(painter, sections);
        add(gui, painter, "trackdescription", id);
        // FormRunningTime: only with a time limited test (Documents cfg/time_limited_test.ini)
        if let Some(test) = own_or_documents("time_limited_test.ini") {
            let max = test.get_int("SETTINGS", "MAX_RUNNING_TIME").unwrap_or(0);
            if max != 0 {
                let id = gui.new_running_time_app(painter, max);
                add(gui, painter, "runningtime", id);
            }
        }
        // FriendsLeaderboardDisplayer: made after SteamInterface, the last of the apps here
        let id = gui.new_friends_app(painter);
        add(gui, painter, "friends", id);
        for (name, show) in std::mem::take(&mut self.pending) {
            if let Some(id) = self.app(&name) {
                let visible = if show { true } else { !gui.controls[id].visible };
                gui.set_visible(id, visible);
            }
        }
        self.write_files(gui, setup);
    }

    /// `TrackMap::TrackMap` 0x1402ee8e0 with the track's files; a track without `map.png` has
    /// its map drawn from its AI line (the game writes it into the track's folder; rustyAC
    /// keeps it in its own `cfg\maps`).
    fn track_map(gui: &mut Gui, painter: &mut dyn Painter, setup: &GuiSetup) -> Id {
        let map_ini = read(setup.game.join("system/cfg/map.ini"));
        let folder = setup.game.join("content/tracks").join(&setup.track_name);
        let folder = if setup.track_layout.is_empty() { folder } else { folder.join(&setup.track_layout) };
        let track_map_ini = read(folder.join("data/map.ini"));
        let points: Vec<[f32; 3]> = rustyac_physics::track::spline::AiSpline::load(&folder.join("ai/fast_lane.ai")).map(|ai| ai.spline.points.iter().map(|p| p.point).collect()).unwrap_or_default();
        let files = rustyac_gui::app_track_map::TrackMapFiles { map_ini: map_ini.as_ref(), track_map_ini: track_map_ini.as_ref(), track: &setup.track_name, config: &setup.track_layout, ai_points: &points };
        gui.new_track_map_app(painter, &files)
    }

    /// What the apps asked to write: into rustyAC's own cfg folder (a track's map under
    /// `maps\`, a settings file under its own name), never into the game's folders.
    /// A settings line the game writes with `WritePrivateProfileStringW` goes into rustyAC's own
    /// copy of the file (made from Documents' the first time). The sound's calls wait in
    /// [`GameApps::sound_actions`].
    pub fn write_files(&mut self, gui: &mut Gui, setup: &GuiSetup) {
        for action in std::mem::take(&mut gui.apps.actions) {
            let (path, bytes) = match action {
                AppAction::WriteFile { path, bytes } => (path, bytes),
                AppAction::WriteProfileString { file, section, key, value } => {
                    let name = file.strip_prefix("cfg/").unwrap_or(&file).to_string();
                    // (a car's car.ini: rustyAC's copy, else the game's loose file)
                    let old = if file.starts_with("content/") { std::fs::read(setup.cfg_dir.join(&file)).ok().or_else(|| std::fs::read(setup.game.join(&file)).ok()) } else { own_or_documents_text(setup, &name) };
                    let bytes = rustyac_gui::apps::write_profile_string(old.as_deref(), &section, &key, &value);
                    if name == "audio.ini" {
                        self.audio_ini = Some(String::from_utf8_lossy(&bytes).into_owned());
                    }
                    (file, bytes)
                }
                AppAction::SetUserFfGain(value) => {
                    self.ffb_jobs.push(rustyac_physics::car::setup::SetupJob::SetUserFfGain { value });
                    continue;
                }
                AppAction::SetFfMult(value) => {
                    self.ffb_jobs.push(rustyac_physics::car::setup::SetupJob::SetFfMult { value });
                    continue;
                }
                AppAction::SetFfUpgrade { which, value } => {
                    if let Some(gains) = self.ff_upgrades.as_mut() {
                        gains[which.min(3)] = value;
                    }
                    self.ffb_jobs.push(rustyac_physics::car::setup::SetupJob::SetFfUpgrade { which, value });
                    continue;
                }
                AppAction::SetDriverEyes(e) => {
                    self.onboard.0 = e;
                    self.onboard_changed = true;
                    continue;
                }
                AppAction::SetOnboardFov(f) => {
                    self.onboard.1 = f;
                    self.onboard_changed = true;
                    continue;
                }
                AppAction::SetOnboardPitch(p) => {
                    self.onboard.2 = p;
                    self.onboard_changed = true;
                    continue;
                }
                AppAction::DeleteFile { path } => {
                    let _ = std::fs::remove_file(setup.cfg_dir.join(path.strip_prefix("cfg/").unwrap_or(&path)));
                    continue;
                }
                AppAction::SetBallastKg(kg) => {
                    self.ffb_jobs.push(rustyac_physics::car::setup::SetupJob::SetBallastKg { kg });
                    continue;
                }
                AppAction::AddMessage { title, text } => {
                    self.messages.push((title, text));
                    continue;
                }
                other => {
                    self.sound_actions.push(other);
                    continue;
                }
            };
            let relative = path.replace('\\', "/");
            let target = match relative.strip_prefix("content/tracks/") {
                Some(track) => setup.cfg_dir.join("maps").join(track),
                None => setup.cfg_dir.join(relative.strip_prefix("cfg/").unwrap_or(&relative)),
            };
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&target, bytes) {
                eprintln!("WARNING: {}: {e}", target.display());
            }
        }
    }

    /// What the apps read of `Sim` this frame, and the events they hear (a lap finished, a new
    /// session).
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, gui: &mut Gui, world: &[CarView], focus: usize, info: &HudInfo, setup: &GuiSetup, use_mph: bool, time_ms: f64) {
        let referee = info.referee.as_ref();
        let race = &gui.sim.race;
        let r = referee.map(|r| &r.gui);
        let session = race.session_info(race.current_session_index);
        let cars = world
            .iter()
            .enumerate()
            .map(|(i, view)| {
                let rc = race.cars.get(i).cloned().unwrap_or_default();
                let place = r.and_then(|r| r.cars.get(i)).copied();
                let state = view.physics_state();
                AppCar {
                    state,
                    body_matrix: Mat44f { m: view.body },
                    current_lap: RaceLap { time: view.lap.current_ms, splits: rc.current_splits.clone(), is_valid: false },
                    best_lap: if rc.best.time != 0 { rc.best.clone() } else { RaceLap { time: view.lap.best_ms, splits: Vec::new(), is_valid: true } },
                    last_lap: if rc.last_lap.time != 0 { rc.last_lap.clone() } else { RaceLap { time: view.lap.last_ms, splits: Vec::new(), is_valid: view.lap.last_valid } },
                    split: if referee.is_some() { rc.best_lap_split } else { -1 },
                    current_lap_valid: if i == 0 { r.map_or(view.lap.valid, |r| r.current_lap_valid) } else { view.lap.valid },
                    lap_count: place.map_or(view.lap.laps, |p| p.2.max(0) as u32),
                    leaderboard_position: place.map_or(-1, |p| p.0),
                    realtime_position: place.map_or(i as i32, |p| p.1),
                    max_fuel: view.max_fuel,
                    // (the engine's damage revs are not in the picture: the limiter decides)
                    engine_damage_rpm: i32::MAX,
                    abs_enabled: view.abs_present && view.abs_mode.0 > 0,
                    abs_in_action: view.abs_in_action,
                    tc_enabled: view.tc_present && view.tc_mode.0 > 0,
                    tc_in_action: view.tc_in_action,
                    has_kers: view.hybrid.has_kers,
                    has_ers: view.hybrid.has_ers,
                    kers_max_j: view.kers_max_j,
                    ers_max_j: view.ers_max_j,
                    edl_enabled: false,
                    edl_out_level: 0.0,
                    has_drs: self.has_drs.get(i).copied().unwrap_or(false),
                    // the player's car reads the controls' gain (the FFB app's), the others none
                    ff_global_gain: (i == 0).then_some(self.ff_gain),
                    user_ff_gain: (i == 0).then_some(1.0),
                    compound_short_name: view.compound_short.as_str().to_string(),
                    ideal_pressure: if i == 0 { view.render.ideal_pressure } else { [0; 4] },
                    connected: !view.disconnected,
                    in_pit: view.lap.in_pit_lane,
                    in_pitlane: view.lap.in_pit_lane,
                    driver_name: setup.cars.get(i).map(|c| c[0].clone()).unwrap_or_default(),
                    unix_name: setup.cars.get(i).map(|c| c[2].clone()).unwrap_or_default(),
                    gui_name: setup.cars.get(i).map(|c| c[1].clone()).unwrap_or_default(),
                    gui_short_name: setup.short_names.get(i).cloned().unwrap_or_default(),
                    has_physics: true,
                    avatar_ff_gain: view.user_ff_gain,
                    ff_mult: view.ff_mult,
                    steer_assist: view.steer_assist,
                    config_name: String::new(),
                    ers_power_name: view.hybrid.power_name.as_str().to_string(),
                    ers_recovery: view.hybrid.recovery,
                    engine_brake_setting: view.hybrid.engine_brake,
                    ers_heat_charging: view.hybrid.heat_charging,
                }
            })
            .collect::<Vec<AppCar>>();
        let first = world.first();
        let actions = std::mem::take(&mut gui.apps.actions);
        gui.apps = AppLink {
            connected_cars_count: cars.iter().filter(|c| c.connected).count() as i32,
            cars,
            focused_car: focus,
            session_type: r.map_or(setup.session_type, |r| r.session_type),
            is_timed_race: session.is_timed_race,
            session_laps: session.laps,
            session_count: setup.session_count,
            time_to_wait: 0.0,
            has_p2p: false,
            session_time_left: r.map_or(0.0, |r| r.session_time_left),
            time_to_session_start: r.map_or(0.0, |r| r.time_to_session_start),
            in_replay: info.replay,
            ai_spline_length: self.ai_spline_length,
            ambient_temperature: first.map_or(0.0, |v| v.air),
            road_temperature: first.map_or(0.0, |v| v.road),
            grip: first.map_or(1.0, |v| v.grip),
            wind_speed: first.map_or(0.0, |v| v.wind_kmh / 3.6),
            wind_direction_deg: first.map_or(0.0, |v| v.wind_deg),
            game_time: time_ms,
            game_start_time: 0.0,
            sun_angle: gui.sim.sun_angle,
            time_mult: None,
            use_mph,
            game_dt: self.game_dt,
            ff_upgrades: self.ff_upgrades,
            driver_eyes: self.onboard.0,
            onboard_fov: self.onboard.1,
            onboard_pitch: self.onboard.2,
            // (rustyAC draws no driver model: no head to put the eyes in)
            auto_eyes: None,
            ctrl_down: self.ctrl_down,
            penalties_enabled: setup.penalties,
            camera_sounds: vec![self.sound.camera; world.len()],
            audio_drivers: self.sound.drivers.clone(),
            audio_ini: self.audio_ini.clone(),
            actions,
        };
        // Sim::evOnReplayStatusChanged (going in, coming out)
        if info.replay != self.replay {
            self.replay = info.replay;
            gui.apps_on_replay_status(info.replay);
        }
        // Sim::evOnNewSession, RaceManager::evOnLapCompleted (the referee's laps)
        if let Some(referee) = referee {
            if self.sessions.is_some_and(|seen| seen != referee.new_sessions) {
                gui.apps_on_new_session();
                self.laps.clear();
            }
            self.sessions = Some(referee.new_sessions);
        }
        let counts: Vec<(u32, u32)> = gui.apps.cars.iter().map(|c| (c.lap_count, c.last_lap.time)).collect();
        if !self.made.is_empty() {
            for (car, (count, time)) in counts.iter().enumerate() {
                if self.laps.get(car).is_some_and(|before| count > before) {
                    gui.apps_on_lap_completed(car as u32, *time);
                }
            }
        }
        self.laps = counts.iter().map(|c| c.0).collect();
    }
}

/// The jobs that give the player's car what the force feedback app saved in rustyAC's own
/// `cfg\content\cars\<car>\data\car.ini` (`[CONTROLS] FFMULT` / 1000, `STEER_ASSIST`; none:
/// no such file), as the game's `Car` reads the loose car.ini the app wrote over.
pub fn saved_car_jobs(cfg_dir: &Path, unix_name: &str) -> Vec<rustyac_physics::car::setup::SetupJob> {
    use rustyac_physics::car::setup::SetupJob;
    let Some(ini) = read(cfg_dir.join("content/cars").join(unix_name).join("data/car.ini")) else { return Vec::new() };
    let mut jobs = Vec::new();
    if ini.has_key("CONTROLS", "FFMULT") {
        if let Ok(mult) = ini.get_float("CONTROLS", "FFMULT") {
            jobs.push(SetupJob::SetFfMult { value: mult * 0.001 });
        }
    }
    if ini.has_key("CONTROLS", "STEER_ASSIST") {
        if let Ok(mut assist) = ini.get_float("CONTROLS", "STEER_ASSIST") {
            // (Car::Car: 0 or not a number is 1)
            if assist == 0.0 || assist.is_nan() {
                assist = 1.0;
            }
            jobs.push(SetupJob::SetSteerAssist { value: assist });
        }
    }
    jobs
}

/// `cfg/gameplay.ini [OPTIONS] USE_MPH` (`Sim::Sim` sets `Speed::useMPH` from it).
pub fn use_mph(cfg_dir: &Path, documents_cfg: Option<&PathBuf>) -> bool {
    let ini = read(cfg_dir.join("gameplay.ini")).or_else(|| documents_cfg.and_then(|d| read(d.join("gameplay.ini"))));
    ini.is_some_and(|ini| ini.get_int("OPTIONS", "USE_MPH").unwrap_or(0) != 0)
}

/// The sound as the Audio app reads it.
#[derive(Clone, Debug, Default)]
pub struct AppSound {
    pub drivers: Vec<String>,
    /// the bonnet's and the bumper's views heard from outside
    pub camera: [bool; 2],
}

/// A settings file's bytes: rustyAC's own copy, else Documents'.
fn own_or_documents_text(setup: &GuiSetup, name: &str) -> Option<Vec<u8>> {
    std::fs::read(setup.cfg_dir.join(name)).ok().or_else(|| setup.documents_cfg.as_ref().and_then(|folder| std::fs::read(folder.join(name)).ok()))
}
