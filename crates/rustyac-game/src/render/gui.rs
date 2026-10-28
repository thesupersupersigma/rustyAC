// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's GUI and HUD in rustyAC's frame: the widget library's `GUI` with its game screen and app
//! bar, and the HUD objects `Sim::Sim` makes, fed from what the window knows of the session.
//!
//! The objects themselves are the ports of `rustyac-gui` (proven against the game one by one);
//! this file is rustyAC's own: it stands where the game's `Sim` hands them its state, reads
//! the files they are made from, and keeps the two files the game writes back at its end
//! (`acos.ini`, `gameplay.ini`) in rustyAC's own cfg folder, never in the game's.

use std::path::{Path, PathBuf};

use rustyac_gui::control::{Id, MouseEvent};
use rustyac_gui::gui::{Gui, GuiFiles};
use rustyac_gui::hud::{HudCar, HudInput, SystemMessage, SystemNotification, MESSAGE_ABS, MESSAGE_BRAKE_BIAS, MESSAGE_DEFAULT, MESSAGE_TC};
use rustyac_gui::hud_cars::{DamageDisplayer, DriverNames, ProximityIndicator};
use rustyac_gui::end_session::EndSession;
use rustyac_gui::leaderboard::{RaceCar, RaceSession, SessionLeaderboard};
use rustyac_gui::overlay_leaderboard::OverlayLeaderboard;
use rustyac_gui::menus::SimAction;
use rustyac_gui::pit_menu::{PitMenu, PitMenuLooks, PitMenuScene, PitStopHud};
use rustyac_gui::hud_flags::{DriveThroughPenalty, FlagDisplay, FuelLight, LapInvalidator, LockControlsTime, WrongWayIndicator};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::graphics::Graphics;
use rustyac_render::gui_paint::GuiRenderer;

use super::hud::HudInfo;
use crate::view::CarView;

/// What the GUI is made from.
#[derive(Clone, Debug, Default)]
pub struct GuiSetup {
    /// Assetto Corsa's folder (read only)
    pub game: PathBuf,
    /// rustyAC's own cfg folder: `acos.ini` and `gameplay.ini` are written here
    pub cfg_dir: PathBuf,
    /// the player's car's compounds (`physicsInfo.tyreCompounds`: the mouse menu's buttons)
    pub pit_compounds: Vec<String>,
    /// `Documents\Assetto Corsa\cfg` (read only: the start of the two files above)
    pub documents_cfg: Option<PathBuf>,
    /// every car: the driver's name, the car's name on the screen, its folder, the nation code
    pub cars: Vec<[String; 4]>,
    /// every car's `car.ini [CONTROLS] STEER_LOCK`
    pub steer_locks: Vec<f32>,
    /// the player's `car.ini [GRAPHICS] FUEL_LIGHT_MIN_LITERS` (0: not there)
    pub fuel_light_min_liters: f32,
    /// the session's type as the game starts (6, a drift session, has no wrong-way sign)
    pub session_type: i32,
    /// `race.ini [RACE] PENALTIES`
    pub penalties: bool,
    /// how many sessions the event has (`RaceManager::getSessionCount`)
    pub session_count: u32,
    /// the game is online (`Sim::client`)
    pub online: bool,
    /// AC's replay is recorded (the game object "REPLAY_MANAGER")
    pub replay: bool,
    /// the player's car's data folder (its `setup.ini` names the pit menu's rows)
    pub player_data: PathBuf,
    /// every car: its skin's folder, and its short name (`car.ini [INFO] SHORT_NAME`)
    pub skins: Vec<String>,
    pub short_names: Vec<String>,
    /// Task 33b: what the Telemetry app reads of the player's car once
    pub telemetry_car: rustyac_gui::app_telemetry::TelemetryCar,
    /// Task 33b: what the Car physics app reads of the player's car once
    pub car_physics: rustyac_gui::app_car_physics::CarPhysicsExtra,
    pub engineer_masses: rustyac_gui::app_car_engineer::CarEngineer,
    /// Task 31: the player's car as the setup editor sees it (none: no editor)
    pub setup_snapshot: Option<rustyac_gui::setup_car::SetupSnapshot>,
    /// Task 34: the player's pit menu for the page "PITSTOP STRATEGY" (none: no pit stop)
    pub pit_page: Option<rustyac_gui::setup_car::SetupPit>,
    /// rustyAC's own `setups` folder (everything the editor writes), and the game's
    /// `Documents\Assetto Corsa\setups` (read only: its setups can be loaded)
    pub setups_dir: PathBuf,
    pub documents_setups: Option<PathBuf>,
    /// the track's folder name ("" for none), and its layout
    pub track_name: String,
    pub track_layout: String,
    /// rustyAC's own `out` folder (the page "Time" prints there)
    pub out_dir: PathBuf,
    /// the session begins in the ESC menu (and so does every session after it)
    pub esc_start: bool,
    /// race.ini `[RACE] FIXED_SETUP` (`RaceManager::fixedSetup`): the menu's Setup button is
    /// dimmed and answers "Blocked"
    pub fixed_setup: bool,
    /// Task 32: every car's data folder (the apps read `engine.ini`, `drs.ini`)
    pub data_paths: Vec<PathBuf>,
    /// Task 33: the developers' apps get icons (`Sim::Sim`'s `devEnabled`: `system/cfg/
    /// assetto_corsa.ini [AC_APPS] ENABLE_DEV_APPS`, or `--dev-apps`)
    pub dev_apps: bool,
    /// Task 40: apps this front end cannot run (the page's): no icon, never shown
    pub hidden_apps: &'static [&'static str],
    /// the GUI's language (`system/locales/<it>.ini`)
    pub language: String,
    /// the comparative telemetry's folder (its `opponents` folder holds others' laps; none:
    /// there is no recorder), the player's car's `drivetrain.ini [GEARS] COUNT`
    pub telemetry_folder: Option<PathBuf>,
    pub max_gear: i32,
    /// Task 42c: what the Python apps read of each car, and online the server
    /// (`ACClient::serverInfo`: its name, IP and HTTP port; none offline)
    pub python_cars: Vec<PythonCar>,
    pub server: Option<(String, String, i32)>,
}

/// Task 42c: a car as the Python apps read it.
#[derive(Clone, Debug, Default)]
pub struct PythonCar {
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// `physicsInfo.minHeightM` (none: no race engineer, every car but the player's)
    pub min_height: Option<f32>,
    pub ai_controlled: bool,
    pub camera_count: i32,
}

/// `Sim::Sim` 0x140192070's `devEnabled`: true unless the game's `system/cfg/assetto_corsa.ini`
/// has an `[AC_APPS]` section whose `ENABLE_DEV_APPS` is not above 0 (`--dev-apps`: always true).
pub fn dev_apps_enabled(game: &Path, forced: bool) -> bool {
    if forced {
        return true;
    }
    match IniReader::load(&game.join("system/cfg/assetto_corsa.ini")) {
        Ok(ini) if ini.ready && ini.has_section("AC_APPS") => ini.get_int("AC_APPS", "ENABLE_DEV_APPS").unwrap_or(0) > 0,
        _ => true,
    }
}

/// The state of the player's car a message is made from when it changes.
#[derive(Clone, Copy, PartialEq)]
struct Aids {
    tc: (u32, u32),
    abs: (u32, u32),
    brake_bias: f32,
    auto_shifter: bool,
}

pub struct GameGui {
    renderer: GuiRenderer,
    pub gui: Gui,
    pub screen: Id,
    pub message: SystemMessage,
    pub notification: SystemNotification,
    pub wrong: Option<WrongWayIndicator>,
    pub lap: LapInvalidator,
    pub drive: DriveThroughPenalty,
    pub lock: LockControlsTime,
    pub proximity: ProximityIndicator,
    pub names: DriverNames,
    pub damage: DamageDisplayer,
    pub flags: FlagDisplay,
    pub fuel: FuelLight,
    pub input: HudInput,
    pub setup: GuiSetup,
    black_flagged: bool,
    last_focus: usize,
    aids: Option<Aids>,
    /// the caption of the black flag as it was last set
    caption: String,
    was_replay: bool,
    /// `--gui-do flag:<name>`: flags shown whatever the referee says (for pictures and checks)
    forced_flags: [bool; 5],
    /// `Sim::pauseMenu`
    pub pause_menu: Id,
    /// `ACCameraManager::fadeMode` of a fade a menu asked for: `fadeTime`, `fadeTotalTime`
    menu_fade: Option<(f32, f32)>,
    /// `QuickMenuPitstop`'s labels (made when the referee first shows its menu), their looks,
    /// and the panel of `PitStop::renderHUD`
    pit_menu: Option<PitMenu>,
    pit_looks: PitMenuLooks,
    pit_stop: PitStopHud,
    /// the menu and the stop as the referee last showed them
    pit: Option<crate::referee::PitGui>,
    /// the mouse menu's clicks since the physics thread last took them
    pit_clicks: Vec<rustyac_physics::race::pitstop::MousePitClick>,
    /// `Sim::endSessionDisplayer`
    pub end: EndSession,
    /// an online session's own overlays (`ACClient::renderHUD`, `QOSOverlay`), what they
    /// show, the welcome message a click closed, and how long the countdown has run
    client_hud: Option<(rustyac_gui::online_hud::ClientHud, rustyac_gui::online_hud::QosOverlay)>,
    online: Option<crate::online::OnlineGui>,
    welcome_closed: String,
    disconnect_time: f32,
    /// `ReplayManager::replayInterface`: the replay's bar (when the session has a replay)
    pub replay_bar: Option<Id>,
    /// `Sim::escMenu` (online too: Task 34)
    pub esc_menu: Option<Id>,
    /// Task 34: `ACClientVotingManager` (online), what the GUI asks of the client
    voting: rustyac_gui::race_control_online::VotingManager,
    online_requests: Vec<crate::online::OnlineRequest>,
    /// the player's car as the setup editor sees it, and which of the physics thread's
    /// pictures of it that is
    setup_data: Option<std::rc::Rc<std::cell::RefCell<rustyac_gui::setup_car::SnapshotData>>>,
    setup_generation: u64,
    /// how many jobs the editor has handed over (a picture of the car that holds fewer of
    /// them is not taken: it would put a spinner back for a moment)
    setup_jobs_sent: u64,
    /// Task 34: the page "PITSTOP STRATEGY": how many of its changes went to the physics
    /// thread, and the pit menu's presets and items as the referee last showed them
    pit_edits_sent: u64,
    pit_seen: Option<(Vec<Vec<f32>>, Vec<rustyac_physics::race::pitstop::QuickMenuItem>)>,
    /// a new session began since the window last asked (the car is its own again: a lock of
    /// the controls the ESC menu wants has to be asked for anew)
    pub pits_lock_stale: bool,
    /// the referee's count of new sessions when last looked at
    new_sessions_seen: Option<u32>,
    /// the `OverlayLeaderboard` (with more than one car, outside a drag race)
    pub overlay: Option<OverlayLeaderboard>,
    /// `Sim::leaderboard`: the table held on TAB (with more than one car, or online)
    pub tab: Option<SessionLeaderboard>,
    /// the app "Leaderboard"
    pub leaderboard_app: Id,
    /// the other apps (made at the first frame), `Speed::useMPH`
    pub apps: super::gui_apps::GameApps,
    /// Task 33: AC's console (Home), and its `exit`
    pub console: rustyac_gui::console::Console,
    pub exit_requested: bool,
    /// Task 33: `Game::gameTime`, the last frame's start; `Game::stats.cpuTime` and
    /// `renderHUDTime` of the last frame (they are stored after the HUD)
    pub game_time: rustyac_gui::app_stats::GameTime,
    #[cfg_attr(not(windows), allow(dead_code))]
    last_frame: Option<std::time::Instant>,
    last_cpu_hud: (f64, f64),
    use_mph: bool,
    /// what the tables read of every car that never changes (names, nation, folder, skin)
    race_static: Vec<RaceCar>,
    /// `RaceManager::getCurrentSessionIndex` as of the last frame
    last_session_index: i32,
    /// `ScreenCapturer`: `readyToCapture`, `frameSkip`, `flashTime`
    shot_ready: bool,
    shot_frame_skip: i32,
    screen_flash: f32,
    /// Task 42: `gameplay.ini [PYTHON] ENABLE_PYTHON` (`Sim::Sim` makes `PythonInterface` when
    /// it is above 0), and the apps once they run
    #[cfg_attr(not(windows), allow(dead_code))]
    python_on: bool,
    #[cfg(windows)]
    python: Option<rustyac_python::Python>,
    /// the Python apps' asks of the cameras, for the window's loop (it owns the cameras)
    #[cfg(windows)]
    pub python_cameras: Vec<rustyac_python::Action>,
}

fn read(path: PathBuf) -> Option<IniReader> {
    IniReader::load(&path).ok().filter(|ini| ini.ready)
}

/// rustyAC's own copy of a cfg file, else the game's (read only).
fn own_or_documents(setup: &GuiSetup, name: &str) -> Option<IniReader> {
    read(setup.cfg_dir.join(name)).or_else(|| setup.documents_cfg.as_ref().and_then(|folder| read(folder.join(name))))
}

impl GameGui {
    /// The comparative telemetry recorder's best and last laps for the ESC menu's page
    /// "Telemetry" (`getBestTime` / `getBestTimeData`, `getLastTime` / `getLastTimeData`), taken
    /// when they changed.
    pub fn telemetry_laps(&mut self, recorder: &rustyac_physics::replay::telemetry::ComparativeTelemetryRecorder) {
        let link = &mut self.gui.sim.telemetry;
        if link.best_time != recorder.best_time {
            link.best_time = recorder.best_time;
            link.best = std::rc::Rc::new(recorder.best.clone());
        }
        if link.last_time != recorder.last_time {
            link.last_time = recorder.last_time;
            link.last = std::rc::Rc::new(recorder.last.clone());
        }
    }

    /// The GUI and the HUD objects, in the order `Sim::Sim` 0x140192070 makes them.
    pub fn new(graphics: &mut Graphics, setup: GuiSetup) -> Result<GameGui, String> {
        let mut renderer = GuiRenderer::new(graphics);
        // initLocalization 0x1400c15c0 (wWinMain, before anything is made): the language's
        // strings, its font locale
        if let Some(font_locale) = rustyac_gui::locale::font_locale(&setup.language) {
            renderer.fonts.set_locale(font_locale);
        }
        let locale = rustyac_gui::locale::Locale::load(&setup.game, &setup.language);
        let system = |name: &str| read(setup.game.join("system/cfg").join(name));
        let gameplay = own_or_documents(&setup, "gameplay.ini");
        let python_on = gameplay.as_ref().is_some_and(|ini| ini.get_int("PYTHON", "ENABLE_PYTHON").unwrap_or(0) > 0);
        let files = GuiFiles {
            skin: IniReader::load(&setup.game.join("content/gui/skin.ini")).map_err(|e| e.to_string())?,
            acos: own_or_documents(&setup, "acos.ini"),
            gameplay: own_or_documents(&setup, "gameplay.ini"),
            assetto_corsa: system("assetto_corsa.ini"),
        };
        let mut painter = renderer.painter(graphics);
        let mut gui = Gui::new(&mut painter, files);
        gui.locale = locale;
        // Console::Console (Sim::Sim makes it first), its tables as the session registers them
        let mut console = rustyac_gui::console::Console::new(&mut painter);
        super::console::register(&mut console, &setup);
        // MicroSectors::MicroSectors 0x14009e9d0: Documents cfg/gameplay.ini [GUI] SHOW_SECTORS
        gui.micro_sectors = rustyac_gui::micro_sectors::MicroSectors::new(gameplay.as_ref().filter(|ini| ini.has_key("GUI", "SHOW_SECTORS")).map(|ini| ini.get_int("GUI", "SHOW_SECTORS").unwrap_or(0)));
        let screen = gui.new_game_screen(&mut painter, "SIM_GAME_SCREEN");
        gui.add_root(&mut painter, screen);

        let mut input = HudInput::default();
        for (i, names) in setup.cars.iter().enumerate() {
            input.cars.push(HudCar {
                driver_name: names[0].clone(),
                gui_name: names[1].clone(),
                unix_name: names[2].clone(),
                nation_code: names[3].clone(),
                steer_lock: setup.steer_locks.get(i).copied().unwrap_or(0.0),
                current_lap_valid: true,
                leaderboard_position: -1,
                ping: -1,
                connected: true,
                body_matrix: Mat44f::IDENTITY,
                engine_life_left: 1000.0,
                gearbox: 1.0,
                tyre_inflation: [1.0; 4],
                ..HudCar::default()
            });
        }
        input.session_type = setup.session_type;
        input.camera_matrix = Mat44f::IDENTITY;

        let mut message = SystemMessage::new(&mut gui, &mut painter, system("messages.ini").as_ref(), system("session_info.ini").as_ref());
        let notification = SystemNotification::new(&mut gui, &mut painter, screen, gameplay.as_ref());
        // what the tables read of the cars and the sessions from the start (Sim::Sim loads the
        // default flag; the liveries are loaded when a table is made)
        let race_static: Vec<RaceCar> = setup
            .cars
            .iter()
            .enumerate()
            .map(|(i, names)| RaceCar {
                driver_name: names[0].clone(),
                unix_name: names[2].clone(),
                nation_code: names[3].clone(),
                skin: setup.skins.get(i).cloned().unwrap_or_default(),
                gui_short_name: setup.short_names.get(i).cloned().unwrap_or_else(|| names[1].clone()),
                connected: true,
                ..RaceCar::default()
            })
            .collect();
        gui.sim.race.cars = race_static.clone();
        gui.sim.race.sessions = vec![RaceSession { session_type: setup.session_type, ..RaceSession::default() }; setup.session_count.max(1) as usize];
        let default_flag = {
            use rustyac_gui::painter::Painter;
            painter.get_texture("content/gui/NationFlags/AC.png")
        };
        gui.sim.flag_cache.insert("AC".to_string(), default_flag);
        // PauseMenu, hidden, a top-level control (Sim::Sim 0x1401931b5)
        gui.sim.game_screen = Some(screen);
        gui.sim.session_type = setup.session_type;
        gui.sim.session_count = setup.session_count;
        gui.sim.fixed_setup = setup.fixed_setup;
        gui.sim.online = setup.online;
        gui.sim.has_replay_manager = setup.replay;
        // ReplayInterface, hidden, the top-level control after the game screen
        // (ReplayManager::ReplayManager 0x140153130)
        gui.sim.replay.status = 2;
        gui.sim.replay.slow_mo_level = 2.0;
        gui.sim.replay.record_interval_ms = 30.0;
        gui.sim.replay.post_processing = own_or_documents(&setup, "video.ini").filter(|ini| ini.ready && ini.has_section("POST_PROCESS")).map(|ini| ini.get_int("POST_PROCESS", "ENABLED").unwrap_or(0) != 0);
        let replay_bar = setup.replay.then(|| {
            let id = gui.new_replay_interface(&mut painter);
            gui.add_root(&mut painter, id);
            gui.set_visible(id, false);
            id
        });
        let pause_menu = gui.new_pause_menu(&mut painter, system("fades.ini").as_ref());
        gui.sim.pause_menu = Some(pause_menu);
        gui.set_visible(pause_menu, false);
        gui.add_root(&mut painter, pause_menu);
        // ESCMenu, hidden, the next top-level control (Sim::Sim 0x1401942ae). Its pages
        // "Setup", "Telemetry" and "Time table" are not made: their buttons cannot be used.
        gui.sim.setup_state = 1;
        // the page "Setup": the editor on the player's car. What its making would ask of the
        // car (every item attached and sent once, the default compound) the session's start
        // has done already (`RollingChassis::session_start`): those jobs are not sent again.
        let mut setup_data = None;
        let mut pages = rustyac_gui::esc_menu::EscPages::default();
        // (online too: Task 34; the server's setup locks what it names)
        if let Some(snapshot) = setup.setup_snapshot.clone() {
            // (the editor's own `QuickMenuPitstop` and its labels: rustyAC's pit menu is the
            // referee's, drawn by the HUD; the page keeps a control that stands for it)
            fn quick_menu(gui: &mut Gui, painter: &mut dyn rustyac_gui::painter::Painter) -> Id {
                let id = gui.new_control(painter, "QUICK_MENU_PITSTOP");
                gui.controls[id].visible = false;
                id
            }
            fn editor(gui: &mut Gui, painter: &mut dyn rustyac_gui::painter::Painter) -> Id {
                gui.new_setup_screen(painter, Some(quick_menu))
            }
            let assists = own_or_documents(&setup, "assists.ini");
            let assist = |key: &str| assists.as_ref().map_or(0, |ini| ini.get_int("ASSISTS", key).unwrap_or(0));
            let mut tracks: Vec<String> = rustyac_content::vfs::read_dir(&setup.game.join("content/tracks")).map(|d| d.into_iter().filter(|e| e.1).map(|e| e.0).collect()).unwrap_or_default();
            tracks.sort_by_key(|name| name.to_uppercase());
            gui.setup = rustyac_gui::setup_car::SetupLink {
                track_name: setup.track_name.clone(),
                tracks,
                setups_save: setup.setups_dir.clone(),
                setups_read: setup.documents_setups.clone(),
                // Sim::useMousePitstop: pitstop.ini [SETTINGS] USE_MOUSE_PITSTOP (without a
                // pit menu to edit, no page either)
                use_mouse_pitstop: setup.pit_page.is_none() || system("pitstop.ini").is_some_and(|ini| ini.get_int("SETTINGS", "USE_MOUSE_PITSTOP").unwrap_or(0) != 0),
                pit: setup.pit_page.clone(),
                tc_assist: assist("TRACTION_CONTROL"),
                abs_assist: assist("ABS"),
                ..Default::default()
            };
            let car = rustyac_gui::setup_car::SnapshotCar::default();
            {
                let mut data = car.data.borrow_mut();
                data.snapshot = snapshot;
                data.muted = true;
            }
            setup_data = Some(car.data.clone());
            gui.setup_car = Some(Box::new(car));
            pages.setup_screen = Some(editor);
        }
        // the page "Telemetry" (ComparativeTelemetry): what it reads of the recorder and the car
        {
            fn telemetry(gui: &mut Gui, painter: &mut dyn rustyac_gui::painter::Painter) -> Id {
                gui.new_comparative_telemetry(painter)
            }
            let folder = setup.game.join("content/tracks").join(&setup.track_name);
            let folder = if setup.track_layout.is_empty() { folder } else { folder.join(&setup.track_layout) };
            gui.sim.telemetry = rustyac_gui::comparative_telemetry::TelemetryLink {
                has_recorder: setup.telemetry_folder.is_some(),
                best_time: u32::MAX,
                last_time: u32::MAX,
                opponents_folder: setup.telemetry_folder.as_ref().map(|f| f.join("opponents")),
                track: setup.track_name.clone(),
                track_config: setup.track_layout.clone(),
                car: setup.cars.first().map(|c| c[2].clone()).unwrap_or_default(),
                player_name: setup.cars.first().map(|c| c[0].clone()).unwrap_or_default(),
                max_gear: setup.max_gear,
                ai_spline_length: rustyac_physics::track::spline::AiSpline::load(&folder.join("ai/fast_lane.ai")).map(|ai| ai.spline.length()).unwrap_or(0.0),
                use_mph: super::gui_apps::use_mph(&setup.cfg_dir, setup.documents_cfg.as_ref()),
                ..Default::default()
            };
            // (ComparativeTelemetryRecorder::ComparativeTelemetryRecorder loads the best lap's
            // file before the menu is made)
            if let Some(folder) = &setup.telemetry_folder {
                use rustyac_physics::replay::telemetry::{ComparativeTelemetryRecorder, TelemetryNames};
                let driver = setup.cars.first().map(|c| c[0].clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Player".to_string());
                let names = TelemetryNames { driver, track: setup.track_name.clone(), car: gui.sim.telemetry.car.clone(), config: setup.track_layout.clone() };
                let mut recorder = ComparativeTelemetryRecorder::new(names);
                if let Ok(bytes) = std::fs::read(folder.join(ComparativeTelemetryRecorder::file_name(&recorder.names))) {
                    if recorder.load_bytes(&bytes) {
                        gui.sim.telemetry.best_time = recorder.best_time;
                        gui.sim.telemetry.best = std::rc::Rc::new(recorder.best.clone());
                    }
                }
            }
            pages.comparative_telemetry = Some(telemetry);
        }
        // the page "Time" (the session's lap lists are the referee's: with it only)
        if setup.session_count > 0 {
            fn time_table(gui: &mut Gui, painter: &mut dyn rustyac_gui::painter::Painter) -> Id {
                gui.new_time_table(painter)
            }
            pages.time_table = Some(time_table);
            gui.sim.time.out_dir = setup.out_dir.clone();
            gui.sim.time.track_config = setup.track_layout.clone();
        }
        // (online the page "Race control" has the server's rules, the votes and the chat)
        let esc_menu = {
            let id = gui.new_esc_menu(&mut painter, system("fades.ini").as_ref(), pages);
            gui.add_root(&mut painter, id);
            gui.sim.esc_menu = Some(id);
            Some(id)
        };
        if let Some(data) = &setup_data {
            data.borrow_mut().muted = false;
        }
        // Sim::Sim 0x140192070: the session begins in the ESC menu (no replay, and
        // `options.ini [OPTIONS] DISABLE_SHOW_MODE` is 0)
        if setup.esc_start && esc_menu.is_some() {
            gui.set_visible(esc_menu.unwrap_or(0), true);
        }
        gui.sim.actions.clear();
        // the apps (Sim::addForm): the Leaderboard's form now, every app put on the screen in
        // the game's order at the first frame (GameApps::make)
        let leaderboard_app = gui.new_leaderboard_app(&mut painter);
        let use_mph = super::gui_apps::use_mph(&setup.cfg_dir, setup.documents_cfg.as_ref());
        let wrong = (setup.session_type != 6).then(|| WrongWayIndicator::new(&mut painter, setup.penalties));
        let lap = LapInvalidator::new(&mut painter);
        let drive = DriveThroughPenalty::new(&mut painter);
        let lock = LockControlsTime::new(&mut painter, system("messages.ini").as_ref());
        let proximity = ProximityIndicator::new(system("proximity_indicator.ini").as_ref());
        let section = |name: &str| gameplay.as_ref().filter(|ini| ini.has_section(name)).map(|ini| ini.get_int(name, "IS_ACTIVE").unwrap_or(0));
        let names = DriverNames::new(&mut gui, &mut painter, system("name_displayer.ini").as_ref(), section("DRIVER_NAME_DISPLAYER").map(|v| v != 0), setup.cars.len());
        let damage = DamageDisplayer::new(&mut painter, system("damage_displayer.ini").as_ref(), section("DAMAGE_DISPLAYER"), 0);
        // EndSessionDisplayer (Sim::Sim 0x140194ed7)
        let end = EndSession::new(&mut gui, &mut painter, system("endSessionDisplayer.ini").as_ref());
        // OverlayLeaderboard (Sim::Sim 0x140194f0d)
        let overlay = (setup.cars.len() > 1 && setup.session_type != 7).then(|| OverlayLeaderboard::new(&mut gui, &mut painter, gameplay.as_ref()));
        // SessionLeaderboard (Sim::Sim 0x140195191)
        let tab = (setup.cars.len() > 1 || setup.online).then(|| SessionLeaderboard::new(&mut gui, &mut painter));
        let flags = FlagDisplay::new(&mut gui, &mut painter, screen, false);
        let fuel = FuelLight::new(&mut painter, setup.fuel_light_min_liters);
        message.load_drivers_info(&mut painter, &input.cars, None);
        let pit_looks = PitMenuLooks::from_ini(system("pitstop.ini").as_ref());
        let compounds: Vec<String> = setup.pit_compounds.clone();
        let pit_stop = PitStopHud::new(&mut gui, &mut painter, &compounds);
        // ACClient's own font and its QOSOverlay (ACClient::ACClient 0x14003a2c0)
        let client_hud = setup.online.then(|| (rustyac_gui::online_hud::ClientHud::new(&mut gui, &mut painter), rustyac_gui::online_hud::QosOverlay::new(&mut gui, &mut painter)));
        Ok(GameGui { renderer, gui, screen, message, notification, wrong, lap, drive, lock, proximity, names, damage, flags, fuel, input, setup, black_flagged: false, last_focus: 0, aids: None, caption: String::new(), was_replay: false, forced_flags: [false; 5], pause_menu, menu_fade: None, pit_menu: None, pit_looks, pit_stop, pit: None, pit_clicks: Vec::new(), end, client_hud, online: None, welcome_closed: String::new(), disconnect_time: 0.0, replay_bar, esc_menu, voting: Default::default(), online_requests: Vec::new(), setup_data, setup_generation: 0, setup_jobs_sent: 0, pit_edits_sent: 0, pit_seen: None, pits_lock_stale: false, new_sessions_seen: None, overlay, tab, leaderboard_app, apps: Default::default(), console, exit_requested: false, game_time: Default::default(), last_frame: None, last_cpu_hud: (0.0, 0.0), use_mph, race_static, last_session_index: 0, shot_ready: false, shot_frame_skip: 0, screen_flash: 0.0, python_on, #[cfg(windows)] python: None, #[cfg(windows)] python_cameras: Vec::new() })
    }

    /// The update pass: the session's state is read, the objects that have an `update` run.
    /// `camera` is the scene camera's matrix, its view and its perspective; `camera_mode`
    /// `ACCameraManager::mode`; `time_ms` the game's clock.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, world: &[CarView], focus: usize, info: &HudInfo, camera: (&Mat44f, &Mat44f, &Mat44f), camera_mode: i32, time_ms: f64, dt: f32) {
        // Task 42c: the session's facts the Python apps read that change (the track's sectors,
        // `Sim::connectedCarsCount`)
        #[cfg(windows)]
        if let Some(python) = &mut self.python {
            let connected = self.gui.apps.connected_cars_count;
            python.world_mut(|w| {
                w.sector_count = world.first().map_or(w.sector_count, |car| car.lap.sector_count as usize);
                if let Some(server) = &mut w.server {
                    server.3 = connected;
                }
            });
        }
        let input = &mut self.input;
        input.cars.resize(world.len().max(input.cars.len()), HudCar { current_lap_valid: true, leaderboard_position: -1, ping: -1, body_matrix: Mat44f::IDENTITY, engine_life_left: 1000.0, gearbox: 1.0, tyre_inflation: [1.0; 4], ..HudCar::default() });
        for (car, view) in input.cars.iter_mut().zip(world) {
            car.fuel = view.fuel;
            car.steer = view.steer_deg;
            car.connected = !view.disconnected;
            car.body_matrix = Mat44f { m: view.body };
            car.world_position = [view.body[3][0], view.body[3][1], view.body[3][2]];
            car.damage_zone_level = [view.damage[0], view.damage[1], view.damage[2], view.damage[3]];
            car.sus_damage = view.suspension_damage;
            car.engine_life_left = view.engine_life;
        }
        // the car's state for the editor's status panel, while the ESC menu shows
        if let (Some(data), Some(view), true) = (&self.setup_data, world.first(), self.esc_menu_visible()) {
            let state = view.physics_state();
            let mut data = data.borrow_mut();
            let s = &data.snapshot;
            let state = rustyac_gui::setup_car::SetupCarState {
                camber_rad: state.camber_rad,
                caster: s.caster,
                suspension_matrix: state.suspension_matrix,
                body_matrix: Mat44f { m: view.body },
                tyres: s.tyres,
                ride_height: s.ride_height,
                min_height_m: s.minimum_height_m,
            };
            data.state = state;
        }
        let input = &mut self.input;
        input.pause_menu_visible = self.gui.controls[self.pause_menu].visible;
        input.in_replay = info.replay;
        input.focused_car_index = focus as i32;
        input.camera_mode = camera_mode;
        input.camera_matrix = *camera.0;
        input.game_time_now = time_ms;
        self.pit = info.referee.as_ref().and_then(|referee| referee.pit_gui.clone());
        // Task 34: what the arrow keys changed of the pit menu, into the page "PITSTOP
        // STRATEGY"'s copy (its spinners follow at the page's next showing, as the game's)
        if let (Some(pit), Some(page)) = (&self.pit, &mut self.gui.setup.pit) {
            if self.pit_seen.as_ref().is_none_or(|(presets, items)| *presets != pit.menu.presets || *items != pit.menu.items) {
                page.menu.presets = pit.menu.presets.clone();
                page.menu.items = pit.menu.items.clone();
                self.pit_seen = Some((pit.menu.presets.clone(), pit.menu.items.clone()));
            }
        }
        // the standings: the referee's, with what the picture's side knows of every car
        self.gui.sim.in_replay = info.replay;
        self.gui.sim.camera_mode = camera_mode;
        if let Some(mut race) = info.referee.as_ref().and_then(|referee| referee.race.clone()) {
            for (i, car) in race.cars.iter_mut().enumerate() {
                if let Some(fixed) = self.race_static.get(i) {
                    car.driver_name = fixed.driver_name.clone();
                    car.unix_name = fixed.unix_name.clone();
                    car.nation_code = fixed.nation_code.clone();
                    car.skin = fixed.skin.clone();
                    car.gui_short_name = fixed.gui_short_name.clone();
                }
                car.connected = true;
                if let Some(view) = world.get(i) {
                    car.connected = !view.disconnected;
                    car.in_pitlane = view.lap.in_pit_lane;
                    car.compound_short_name = view.compound_short.as_str().to_string();
                }
            }
            let index = race.current_session_index;
            self.gui.sim.race = race;
            if let Some(referee) = &info.referee {
                self.gui.setup.session_total_time = referee.session_total_time;
                self.gui.sim.fixed_setup = referee.fixed_setup;
            }
            // the page "Time": the laps, the car the camera follows, where PRINT writes
            if let Some(time) = info.referee.as_ref().and_then(|referee| referee.time.clone()) {
                let (config, out_dir) = (std::mem::take(&mut self.gui.sim.time.track_config), std::mem::take(&mut self.gui.sim.time.out_dir));
                self.gui.sim.time = rustyac_gui::time_table::TimeLink { focused_car: focus, track_config: config, out_dir, time_stamp: time_stamp(), ..time };
            }
            // Sim::evOnNewSession, as the tables hear it
            if index != self.last_session_index {
                self.last_session_index = index;
                self.gui.sim_on_new_session();
                self.pits_lock_stale = true;
            }
            // the handler of `Sim::evOnNewSession` (0x140195c60): the ESC menu comes up, at
            // another session and at a restart alike (the referee has locked the controls)
            if let Some(referee) = &info.referee {
                if self.new_sessions_seen.is_some_and(|seen| seen != referee.new_sessions) {
                    self.pits_lock_stale = true;
                    if self.setup.esc_start && self.esc_menu.is_some() {
                        self.gui.sim_activate_esc_menu();
                    }
                }
                self.new_sessions_seen = Some(referee.new_sessions);
            }
        }
        // an online session's overlays; the countdown of a driver who was put out
        self.online = info.online.clone();
        if self.online.as_ref().is_some_and(|online| online.disconnect.is_some()) {
            self.disconnect_time += dt;
        }
        // what the page "Race control" reads of the client and of the server's rules
        if let Some(online) = &self.online {
            let link = &mut self.gui.sim.online_link;
            link.server_name.clone_from(&online.server_name);
            if let Some(rules) = &online.rules {
                (link.tc, link.abs, link.stability, link.auto_clutch) = (rules.tc, rules.abs, rules.stability, rules.auto_clutch);
                (link.damage_rate, link.fuel_rate, link.tyre_wear_rate) = (rules.damage_rate, rules.fuel_rate, rules.tyre_wear_rate);
                (link.tyre_blankets, link.jump_start_mode) = (rules.tyre_blankets, rules.jump_start_mode);
            }
            self.gui.sim.race.inverted_grid_positions = online.inverted_grid_positions;
            if info.referee.is_none() {
                self.gui.sim.session_type = online.session_type;
                self.gui.sim.time_to_session_start = online.time_to_session_start;
                self.gui.sim.player_in_pit = online.player_in_pit;
            }
        }
        // what the ESC menu and its page read
        self.gui.sim.game_time = time_ms;
        if let Some(referee) = &info.referee {
            self.gui.sim.session_time_left = referee.gui.session_time_left;
            [self.gui.sim.track_grip, self.gui.sim.ambient_temperature, self.gui.sim.road_temperature, self.gui.sim.wind_speed, self.gui.sim.wind_direction_deg] = referee.env;
        }
        if let Some(esc) = self.esc_menu {
            input.esc_menu_visible = self.gui.controls[esc].visible;
            input.esc_menu_width = self.gui.controls[esc].rect.right;
        }
        // the results: RaceManager shows them (EndSessionDisplayer::displayResults), a new
        // session takes them down (hide)
        let results = info.referee.as_ref().is_some_and(|referee| referee.gui.results_displayed);
        if results && !self.end.is_active {
            self.end.display_results(&mut self.gui);
            // (RaceManager's job at the end of a race, 0x140133c90, takes the TAB table down)
            if let Some(tab) = &mut self.tab {
                tab.display(&mut self.gui, false);
            }
        } else if !results && self.end.is_active {
            self.end.hide(&mut self.gui);
        }
        self.end.update(&mut self.gui);
        let mut shown = [false; 5];
        match info.referee.as_ref().map(|referee| &referee.gui) {
            Some(r) => {
                self.gui.sim.session_type = r.session_type;
                self.gui.sim.time_to_session_start = r.time_to_session_start;
                input.session_type = r.session_type;
                input.practice_qualify_text = r.practice_qualify_text.clone();
                input.race_text = r.race_text.clone();
                input.time_to_session_start = r.time_to_session_start;
                input.race_over = r.race_over;
                input.session_time_left = r.session_time_left;
                input.end_session_active = r.results_displayed;
                shown = r.flags;
                if r.caption != self.caption {
                    self.caption = r.caption.clone();
                    self.gui.set_text(self.flags.black_flag_caption, &rustyac_gui::hud::translate(&self.caption));
                }
                self.black_flagged = r.is_black_flagged;
                // WrongWayIndicator::update 0x1401d8940, its first lines (the count of metres
                // and its penalty are not made here)
                if let Some(wrong) = &mut self.wrong {
                    wrong.is_wrong_way = r.against_road > 0.4;
                }
                if let Some(car) = input.cars.first_mut() {
                    car.penalty_time = r.penalty_time;
                    car.min_speed_penalty_clear_disabled = r.min_speed_penalty_clear_disabled;
                    car.lock_controls_time = r.lock_controls_time;
                    car.pit_penalty_laps = r.pit_penalty_laps;
                    car.current_lap_valid = r.current_lap_valid;
                }
                for (car, place) in input.cars.iter_mut().zip(&r.cars) {
                    car.leaderboard_position = place.0;
                    car.real_time_position = place.1;
                    car.lap_count = place.2;
                    car.last_lap_ms = place.3;
                }
            }
            None => {
                input.practice_qualify_text.clear();
                input.race_text.clear();
            }
        }

        // the messages the game's notifiers make when the driver changes an aid (CarAvatar's
        // AbsNotifier 0x1400daf90, TractionControlNotifier 0x1400dca70, BrakeBiasNotifier
        // 0x1400db510; Sim::onKeyDown's "Auto Shifter")
        if let Some(view) = world.first() {
            let now = Aids { tc: view.tc_mode, abs: view.abs_mode, brake_bias: view.brake_bias, auto_shifter: view.auto_shifter };
            if let Some(before) = self.aids {
                let level = |mode: (u32, u32)| {
                    if mode.0 == 0 {
                        "OFF".to_string()
                    } else if mode.1 > 1 {
                        format!("Set on level : {}/{}", mode.0, mode.1)
                    } else {
                        format!("Set on level : {}", mode.0)
                    }
                };
                if now.abs != before.abs && view.abs_present {
                    self.message.add_message(&mut self.gui, "ABS", &level(now.abs), MESSAGE_ABS);
                }
                if now.tc != before.tc && view.tc_present {
                    self.message.add_message(&mut self.gui, "TC", &level(now.tc), MESSAGE_TC);
                }
                if now.brake_bias != before.brake_bias {
                    let text = format!("{} %", rustyac_gui::widgets2::format_fixed((now.brake_bias * 100.0) as f64, 1));
                    self.message.add_message(&mut self.gui, "Brake Bias", &text, MESSAGE_BRAKE_BIAS);
                }
                if now.auto_shifter != before.auto_shifter {
                    self.message.add_message(&mut self.gui, "Auto Shifter", if now.auto_shifter { "ON" } else { "OFF" }, MESSAGE_DEFAULT);
                }
            }
            self.aids = Some(now);
        }
        // the camera went to another car: its bar (the lambdas of Sim::onKeyDown)
        if focus != self.last_focus {
            self.last_focus = focus;
            self.message.add_car_message(focus as u32, MESSAGE_DEFAULT);
        }

        self.gui.time_ms = time_ms;
        self.message.update(&mut self.gui, dt, &self.input);
        self.notification.update(&mut self.gui, dt, &self.input);
        self.names.update(camera.0, camera.1, camera.2);
        for (flag, forced) in shown.iter_mut().zip(self.forced_flags) {
            *flag |= forced;
        }
        // FlagManager::update: the referee has decided (Task 28); the pictures follow it
        self.flags.show(&mut self.gui, shown[0], shown[1], shown[2], shown[3], shown[4]);
        // (the fuel light stands beside a flag's column; the game moves it with its flag events)
        self.fuel.x_pos = if shown[0] || shown[2] || shown[3] || shown[4] { 30.0 } else { 25.0 };
        if let Some(wrong) = &mut self.wrong {
            wrong.is_active = !info.replay;
        }
        self.fuel.is_active = !info.replay;
        // DamageDisplayer's handler of the replay's status (0x14007a4e0): off going in, on
        // coming out
        if info.replay != self.was_replay {
            self.was_replay = info.replay;
            self.damage.enabled = !info.replay;
        }
        // the apps' view of the session
        self.apps.update(&mut self.gui, world, focus, info, &self.setup, self.use_mph, time_ms);
        // MicroSectors::update, a game object updated every frame (Task 33)
        self.gui.micro_sectors_update();
        // Sim::update's statistics block (Render Stats)
        let dt = self.gui.apps.stats.delta_t;
        self.gui.stats_sim_update(dt);
        // what the console was asked since the last frame
        if super::console::carry_out(self) {
            self.exit_requested = true;
        }
    }

    /// `ksgui::GUI::render` 0x140254c10: the 2D pass's states, the game screen with its forms
    /// (and the flags), the app bar.
    pub fn render(&mut self, graphics: &mut Graphics, dt: f32) {
        let mut painter = self.renderer.painter(graphics);
        if self.apps.made.is_empty() {
            self.apps.make(&mut self.gui, &mut painter, self.screen, &self.setup, self.leaderboard_app);
            #[cfg(windows)]
            if self.python_on {
                self.python = start_python(&mut self.gui, &mut painter, self.screen, &self.setup);
            }
        }
        // Game::gameTime.deltaT (the force feedback app's arrows)
        self.apps.game_dt = dt;
        self.gui.apps.game_dt = dt;
        // Game::update: PyPlugin::update of every app (acUpdate), what they asked carried out
        #[cfg(windows)]
        if let Some(python) = &mut self.python {
            let paused = self.gui.controls[self.pause_menu].visible;
            python.world_mut(|w| w.paused = paused);
            python.update(&mut self.gui, &mut painter, dt);
            for line in python.take_console() {
                self.console.print(&line);
                self.console.print("
");
            }
            for action in python.take_actions() {
                match action {
                    rustyac_python::Action::Shutdown | rustyac_python::Action::Restart => self.exit_requested = true,
                    rustyac_python::Action::SetUserFfGain(value) => self.gui.apps.actions.push(rustyac_gui::apps::AppAction::SetUserFfGain(value)),
                    // Sim::setFocusedCarIndex, ACClient::sendChat: as the tables and the chat ask
                    rustyac_python::Action::FocusCar(car) => self.gui.sim.actions.push(SimAction::SetFocusedCarIndex(car as u32)),
                    rustyac_python::Action::SendChat(text) => self.gui.sim.actions.push(SimAction::SendChat(text)),
                    camera => self.python_cameras.push(camera),
                }
            }
        }
        self.gui.render_all(dt, &mut painter);
        self.apps.write_files(&mut self.gui, &self.setup);
        for (title, text) in std::mem::take(&mut self.apps.messages) {
            self.message.add_message(&mut self.gui, &title, &text, MESSAGE_DEFAULT);
        }
    }

    /// `GameScreen::shutdown` 0x1402510c0 (from `Sim::shutdown`): every app's `shutdown` (the
    /// force feedback app saves the car's car.ini), its files written.
    pub fn shutdown_apps(&mut self) {
        self.gui.shutdown_apps();
        // (PythonInterface is a child of Sim: acShutdown, then Py_Finalize)
        #[cfg(windows)]
        if let Some(python) = self.python.take() {
            python.shutdown(&mut self.gui);
        }
        self.apps.write_files(&mut self.gui, &self.setup);
    }

    /// `Game::renderHUD` 0x140242e30 over the HUD objects, in the order `Sim::Sim` made them.
    pub fn render_hud(&mut self, graphics: &mut Graphics, dt: f32) {
        let mut painter = self.renderer.painter(graphics);
        // ScreenCapturer::renderHUD 0x1401631a0: the white flash of a screenshot, a second long
        if self.screen_flash > 0.0 {
            use rustyac_gui::painter::{Painter, BLEND_ALPHA, DEPTH_OFF, GRAPHICS_GL};
            painter.set_depth_mode(DEPTH_OFF);
            painter.set_blend_mode(BLEND_ALPHA);
            painter.gl_color4f(GRAPHICS_GL, 3.0, 3.0, 3.0, self.screen_flash);
            let (width, height) = painter.screen();
            painter.gl_quad(GRAPHICS_GL, 0.0, 0.0, width as f32, height as f32, false);
            self.screen_flash -= dt;
            painter.set_depth_mode(0);
        }
        let (gui, input) = (&mut self.gui, &self.input);
        self.message.render_hud(gui, &mut painter, dt, input);
        // QuickMenuPitstop::renderHUD 0x1400b1230 (the menu's coming and going is the
        // referee's: Task 28b; here what the picture's side knows hides it too)
        let scene = PitMenuScene {
            esc_menu_visible: input.esc_menu_visible,
            pause_menu_visible: input.pause_menu_visible,
            in_replay: input.in_replay,
            end_session_active: input.end_session_active,
            camera_mode: input.camera_mode,
            app_interaction_enabled: true,
            focused_car_index: input.focused_car_index,
            in_pitlane: self.pit.as_ref().is_some_and(|pit| pit.in_pitlane),
        };
        if let Some(pit) = &self.pit {
            if self.pit_menu.is_none() {
                if let Ok(setup) = IniReader::load(&self.setup.player_data.join("setup.ini")) {
                    self.pit_menu = Some(PitMenu::build(gui, &mut painter, self.pit_looks, &pit.menu, &setup, &pit.car));
                }
            }
            if let Some(labels) = self.pit_menu.as_ref().filter(|labels| labels.items.len() == pit.menu.items.len()) {
                let hidden = scene.esc_menu_visible || scene.pause_menu_visible || scene.in_replay || scene.end_session_active || (scene.camera_mode as u32 & 0xffff_fffd) != 0 || scene.focused_car_index != 0;
                if pit.menu.is_visible && !hidden {
                    labels.update_ui(gui, &pit.menu, &pit.car);
                    labels.draw(gui, &mut painter, dt, &pit.menu, &pit.car);
                }
            }
        }
        if let Some(wrong) = self.wrong.as_mut().filter(|w| w.is_active) {
            wrong.render_hud(gui, &mut painter, input);
        }
        self.lap.render_hud(gui, &mut painter, dt, input, &mut self.message);
        self.drive.render_hud(gui, &mut painter, input, self.black_flagged);
        self.lock.render_hud(gui, &mut painter, input, &mut self.message);
        if self.proximity.is_active {
            self.proximity.render_hud(gui, &mut painter, input);
        }
        // PitStop::renderHUD 0x1400aa300
        if let Some(pit) = &self.pit {
            let mut state = pit.state;
            if let Some(mouse) = &pit.mouse {
                self.pit_stop.sync(gui, mouse);
            }
            self.pit_stop.render_hud(gui, &mut painter, dt, &scene, input.race_over, input.cars.first().map_or(0.0, |car| car.lock_controls_time), &mut state, &pit.times, &pit.mouse_values);
        }
        self.names.render_hud(gui, &mut painter, dt, input);
        self.damage.render_hud(gui, &mut painter, dt, input);
        self.end.render_hud(gui, &mut painter, dt);
        if let Some(overlay) = &mut self.overlay {
            let position = input.cars.first().map_or(-1, |car| car.leaderboard_position);
            overlay.render_hud(gui, &mut painter, dt, self.end.is_active, position);
        }
        if let Some(tab) = &self.tab {
            tab.render_hud(gui, &mut painter, dt);
        }
        if self.fuel.is_active {
            self.fuel.render_hud(gui, &mut painter, input);
        }
        // ACClient::renderHUD, then its child the QOSOverlay
        if let (Some((client, qos)), Some(online)) = (&self.client_hud, &self.online) {
            let welcome = if online.welcome == self.welcome_closed { "" } else { online.welcome.as_str() };
            client.render_hud(gui, &mut painter, welcome, online.disconnect.as_deref(), input.game_time_now);
            qos.render_hud(gui, &mut painter, online.ping);
        }
        // Console::renderHUD 0x14018c550, the last of the HUD objects
        let (width, _) = rustyac_gui::painter::Painter::screen(&painter);
        let now = self.gui.time_ms;
        self.console.render_hud(&mut painter, width, now);
    }

    // ----- the window's input (`ksgui::GUI`'s listeners of the mouse and the keyboard) -----

    // (the GUI hears of the mouse first, then the game objects that hung themselves on the
    // window's mouse events: the end-of-session displayer)

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        let e = MouseEvent { x, y, ..MouseEvent::default() };
        self.gui.mouse_move(&e);
        self.pit_mouse(2, &e);
        self.end.on_mouse_move(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_move(&mut self.gui, &e);
        }
    }

    pub fn mouse_down(&mut self, x: i32, y: i32, button: i32) {
        let e = MouseEvent { x, y, button, delta: 0.0 };
        // (ACClient's handler of the window's mouse buttons: the welcome message goes)
        if let Some(online) = &self.online {
            self.welcome_closed = online.welcome.clone();
        }
        self.gui.mouse_down(&e);
        self.pit_mouse(0, &e);
        self.end.on_mouse_down(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_down(&mut self.gui, &e);
        }
    }

    pub fn mouse_up(&mut self, x: i32, y: i32, button: i32) {
        let e = MouseEvent { x, y, button, delta: 0.0 };
        self.gui.mouse_up(&e);
        self.pit_mouse(1, &e);
        self.end.on_mouse_up(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_up(&mut self.gui, &e);
        }
    }

    /// `PitStop`'s handlers of the window's mouse (`isActive`, the menu shown, the state
    /// "choosing"); `kind` 0 down, 1 up, 2 move.
    fn pit_mouse(&mut self, kind: u32, e: &MouseEvent) {
        let Some(pit) = &self.pit else { return };
        if let Some(mouse) = &pit.mouse {
            self.pit_stop.sync(&mut self.gui, mouse);
            let open = pit.state == rustyac_physics::race::pitstop::STATE_CHOOSING;
            let clicks = self.pit_stop.mouse(&mut self.gui, open, kind, e);
            self.pit_clicks.extend(clicks);
        }
    }

    /// The mouse menu's clicks, for the physics thread.
    pub fn take_pit_clicks(&mut self) -> Vec<rustyac_physics::race::pitstop::MousePitClick> {
        std::mem::take(&mut self.pit_clicks)
    }

    pub fn mouse_wheel(&mut self, x: i32, y: i32, delta: f32) {
        let e = MouseEvent { x, y, button: 0, delta };
        self.gui.mouse_wheel(&e);
        self.end.on_mouse_wheel(&mut self.gui, &e);
    }

    /// What the statistics apps read this frame (`GameTime::update`, `Game::stats`,
    /// `GraphicsManager::stats`, the physics thread's `occupancy` and `physicsLateLoops`).
    #[cfg(windows)]
    pub fn frame_stats(&mut self, update_ms: f64, render_ms: f64, counters: &rustyac_render::graphics::RenderStats, timing: &crate::physics_thread::Timing) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_frame {
            self.game_time.update(now.duration_since(last).as_secs_f64() * 1000.0);
        }
        self.last_frame = Some(now);
        // PhysicsDriveThread::run: the step's milliseconds as a share of its 3 ms, cut, at most 300
        let occ = (timing.last_step_ms as f32 * f32::from_bits(0x3eaa_aaab)) * 100.0;
        let occ = if !(-2_147_483_648.0..2_147_483_648.0).contains(&occ) { i32::MIN } else { occ as i32 };
        self.apps.stats = rustyac_gui::app_stats::AppStats {
            fps: self.game_time.fps,
            delta_t: self.game_time.delta_t,
            dip_calls: counters.dip_calls,
            triangles: counters.triangles,
            scene_triangles: counters.scene_triangles,
            cpu_time: self.last_cpu_hud.0,
            update_time: update_ms,
            render_hud_time: self.last_cpu_hud.1,
            render_time: render_ms,
            render_audio_time: 0.0,
            occupancy: if occ >= 300 { 300 } else { occ },
            physics_late_loops: timing.late_loops,
            fmod_update_usage: None,
        };
    }

    /// The frame's `Game::stats.cpuTime` and `renderHUDTime`, for the next frame's apps.
    pub fn frame_end(&mut self, cpu_ms: f64, hud_ms: f64) {
        self.last_cpu_hud = (cpu_ms, hud_ms);
    }

    /// KeyboardManager: the focused listener first (the console, while it is shown), then
    /// the others (the GUI hears every key whatever the focus).
    pub fn key_down(&mut self, key: u32) {
        self.console.on_key_down(key);
        self.gui.key_down(key);
    }

    pub fn key_char(&mut self, key: u32) {
        self.console.on_key_char(key);
        self.gui.key_char(key);
    }

    /// `Sim::onKeyDown`, Home: `Console::show(!isVisible)`.
    pub fn console_toggle(&mut self) {
        let shown = self.console.shown;
        self.console.show(!shown);
    }

    // ----- the table on TAB -----

    /// `Sim::onKeyDown`, TAB: the table comes (not over a menu or the results).
    pub fn tab_down(&mut self) {
        let (pause, results) = (self.pause_menu_visible(), self.end.is_active);
        let esc = self.gui.sim.esc_menu.is_some_and(|esc| self.gui.controls[esc].visible);
        if let Some(tab) = &mut self.tab {
            tab.on_tab_down(&mut self.gui, esc, pause, results);
        }
    }

    /// `Sim::renderHUD`, once a frame: the table goes when TAB is let go.
    pub fn tab_poll(&mut self, held: bool) {
        let results = self.end.is_active;
        let esc = self.gui.sim.esc_menu.is_some_and(|esc| self.gui.controls[esc].visible);
        if let Some(tab) = &mut self.tab {
            tab.poll(&mut self.gui, held, esc, results);
        }
    }

    /// `OverlayLeaderboard::update`, once a frame: F9 steps through its faces, each with a
    /// message.
    pub fn overlay_poll(&mut self, f9_down: bool) {
        if let Some(words) = self.overlay.as_mut().and_then(|overlay| overlay.update(f9_down)) {
            self.message.add_message(&mut self.gui, "Overlay Leaderboard", words, MESSAGE_DEFAULT);
        }
    }

    // ----- ScreenCapturer -----

    /// `ScreenCapturer::takeScreen` 0x140163470 (F8): a picture is asked for, unless the flash
    /// of the last one still shows.
    pub fn take_screen(&mut self) {
        if self.screen_flash <= 0.0 {
            self.shot_ready = true;
        }
    }

    /// The handler `ScreenCapturer` hangs on `Game::evOnPreGUI` (0x140162b60): the third frame
    /// after the key is the picture (true: this frame, as it is before the GUI and the HUD
    /// are drawn), and the flash begins.
    pub fn pre_gui(&mut self) -> bool {
        if self.shot_ready {
            self.shot_frame_skip += 1;
            if self.shot_frame_skip > 2 {
                self.shot_frame_skip = 0;
                self.screen_flash = 1.0;
                self.shot_ready = false;
                return true;
            }
        }
        false
    }

    // ----- the menus -----

    /// `Sim::onKeyDown` 0x14019a940, ESC while driving: the pause menu and the pause, unless a
    /// fade still runs. Online the car must stand first (`online_moving`: it is faster than
    /// 1 km/h), and a message says so.
    pub fn esc_key(&mut self, online_moving: bool) {
        // (Sim::isDisplayingResults: the results have their own buttons)
        if self.end.is_active {
            return;
        }
        if online_moving {
            self.message.add_message(&mut self.gui, "Stop for Esc Menu", "Stop your car to enter the pause menu", MESSAGE_DEFAULT);
        } else if self.gui.sim.esc_menu.is_none_or(|esc| !self.gui.controls[esc].visible) && self.gui.sim.fade_listeners.is_empty() {
            self.gui.sim_activate_pause_menu();
            self.gui.sim_set_pause_mode(true);
        }
    }

    /// `ReplayManager::startReplayMode` / `stopReplayMode` as far as the bar goes: it shows
    /// (its cut markers back at the ends) or goes.
    pub fn replay_bar_show(&mut self, on: bool) {
        let Some(bar) = self.replay_bar else { return };
        if self.gui.controls[bar].visible != on {
            self.gui.set_visible(bar, on);
            if on {
                self.gui.replay_reset_cuts(bar);
            }
        }
    }

    /// `ACCameraManager::evOnCameraSelectedIndex`: another camera is the one in use.
    pub fn replay_camera_index(&mut self, index: i32) {
        self.gui.sim.replay.camera_index = index;
        if let Some(bar) = self.replay_bar {
            self.gui.replay_on_camera_selected_index(bar, index);
        }
    }

    /// `ReplayManager::evDidFinishedSavingReplay`.
    pub fn replay_saved(&mut self, ok: bool, file: &str) {
        if let Some(bar) = self.replay_bar {
            self.gui.replay_did_finished_saving(bar, ok, file);
        }
    }

    /// The countdown of a driver who was put out of an online session is over (ten seconds).
    pub fn disconnect_over(&self) -> bool {
        self.disconnect_time > 10.0
    }

    /// There is an ESC menu.
    pub fn has_esc_menu(&self) -> bool {
        self.esc_menu.is_some()
    }

    /// The ESC menu (the menu of the pits) shows.
    pub fn esc_menu_visible(&self) -> bool {
        self.esc_menu.is_some_and(|esc| self.gui.controls[esc].visible)
    }

    /// The pause menu shows.
    pub fn pause_menu_visible(&self) -> bool {
        self.gui.controls[self.pause_menu].visible
    }

    /// What the menus asked of the session since the last call, in order. The message banner
    /// is cleared and a fade is started here; the rest is the window's to carry out.
    pub fn take_actions(&mut self) -> Vec<SimAction> {
        let mut out = Vec::new();
        for action in std::mem::take(&mut self.gui.sim.actions) {
            match action {
                SimAction::ClearMessage => self.message.clear_message(),
                SimAction::AddMessage { title, text } => self.message.add_message(&mut self.gui, &title, &text, MESSAGE_DEFAULT),
                SimAction::FadeIn(total) => self.menu_fade = Some((0.0, total)),
                // ACClient::sendChat 0x140052a90 (it sets `lastChatMessage`), ACClient::sendVote
                // 0x140052ba0 (it sets `hasVoted`, whatever the vote); off line no client
                SimAction::SendChat(text) if self.setup.online => {
                    self.gui.sim.online_link.last_chat_message = self.gui.sim.game_time;
                    self.online_requests.push(crate::online::OnlineRequest::Chat { text, now: self.gui.sim.game_time });
                }
                SimAction::SendVote { kind, value, guid } if self.setup.online => {
                    self.gui.sim.online_link.vote.has_voted = true;
                    self.online_requests.push(crate::online::OnlineRequest::Vote { kind, yes: value, guid });
                }
                other => out.push(other),
            }
        }
        out
    }

    /// Task 34: what the client heard since the last frame, in order: the chat lines
    /// (`ACClient::evOnChatMessage`, every `ACChat`), the votes (`ACClientVotingManager`'s
    /// handlers) and the client's system messages.
    pub fn online_heard(&mut self, events: Vec<crate::online::OnlineGuiEvent>) {
        use crate::online::OnlineGuiEvent;
        use rustyac_gui::race_control_online::{ReceivedVote, VotingManager};
        for event in events {
            match event {
                OnlineGuiEvent::Chat { session, text } => {
                    let net = self.online.as_ref().and_then(|online| online.net_cars.iter().find(|car| car.0 == session).map(|car| (car.2.clone(), car.1)));
                    // (the Chat app's handler was hung up before the page's)
                    self.gui.chat_apps_on_chat_message(session as u32, &text, net.as_ref().map(|(name, guid)| (name.as_str(), *guid)));
                    // the Python apps' listeners (hung up in Sim::Sim before the ESC menu's)
                    #[cfg(windows)]
                    if let Some(python) = &mut self.python {
                        let own = self.online.as_ref().is_some_and(|online| online.session_id == session);
                        let name = match (session, &net) {
                            (0xff, _) => "SERVER".to_string(),
                            (_, Some((name, _))) => name.clone(),
                            _ if own => self.gui.apps.cars.first().map(|car| car.driver_name.clone()).unwrap_or_default(),
                            _ => String::new(),
                        };
                        python.on_chat(&mut self.gui, &text, &name);
                    }
                    self.gui.chats_on_chat_message(session as u32, &text, net.as_ref().map(|(name, _)| name.as_str()));
                }
                OnlineGuiEvent::Vote { kind, target, time_left_ms, is_me } => {
                    VotingManager::on_vote_received(&mut self.gui.sim.online_link.vote, &ReceivedVote { vote_type: kind, time_left_ms, is_me, target });
                }
                OnlineGuiEvent::VoteNotPassed => {
                    let out = VotingManager::on_vote_not_passed(&self.gui.sim.online_link.vote);
                    self.vote_output(vec![out]);
                }
                OnlineGuiEvent::Message { title, text, kind } => self.message.add_message(&mut self.gui, &title, &text, kind),
                // Task 42b: the end of the race (lambda @ 0x14003dde0) and the next session
                OnlineGuiEvent::SentToPits | OnlineGuiEvent::NewSession => {
                    if let Some(menu) = self.esc_menu {
                        let sent = event == OnlineGuiEvent::SentToPits;
                        if sent {
                            self.gui.sim_activate_esc_menu();
                        } else {
                            // (the pit stop's handler of the new session freed the controls;
                            // the ESC menu that still shows locks them again)
                            self.pits_lock_stale = true;
                        }
                        self.gui.esc_menu_set_drive_button_enabled(menu, !sent);
                    }
                }
                // (the game's Documents\Assetto Corsa\setups is always there; rustyAC's own
                // setups folder is made for it)
                OnlineGuiEvent::Setup { values } => {
                    if let Some(screen) = self.esc_menu.and_then(|menu| self.gui.esc_menu(menu).setup_screen) {
                        let _ = rustyac_content::guard::create_dir_all(&self.setup.setups_dir);
                        self.gui.setup_screen_load_server_setup(screen, &values, &self.setup.setups_dir);
                    }
                }
            }
        }
    }

    /// Task 34: `ACClientVotingManager::update` 0x140055600, once a frame online: the keys Y and
    /// N (`GetAsyncKeyState`), the vote's time, the prompt.
    pub fn voting_update(&mut self, y_down: bool, n_down: bool, dt: f32) {
        if !self.setup.online {
            return;
        }
        let esc = self.esc_menu_visible();
        let out = self.voting.update(&mut self.gui.sim.online_link.vote, dt, y_down, n_down, esc);
        self.vote_output(out);
    }

    fn vote_output(&mut self, out: Vec<rustyac_gui::race_control_online::VoteOutput>) {
        use rustyac_gui::race_control_online::VoteOutput;
        for out in out {
            match out {
                VoteOutput::ClearMessage => self.message.clear_message(),
                VoteOutput::Message { title, text, kind } => self.message.add_message(&mut self.gui, &title, &text, kind),
                VoteOutput::SendVote { kind, yes, guid } => {
                    self.gui.sim.online_link.vote.has_voted = true;
                    self.online_requests.push(crate::online::OnlineRequest::Vote { kind, yes, guid });
                }
            }
        }
    }

    /// The console's `vote_next`, `vote_restart`, `vote_kick`: `ACClient::sendVote(kind, true, guid)`.
    pub fn console_vote(&mut self, kind: i32, guid: i32) {
        self.vote_output(vec![rustyac_gui::race_control_online::VoteOutput::SendVote { kind, yes: true, guid }]);
    }

    /// Task 34: what the GUI asked of the client since the last call.
    pub fn take_online_requests(&mut self) -> Vec<crate::online::OnlineRequest> {
        std::mem::take(&mut self.online_requests)
    }

    /// `ACCameraManager`'s handler of `Game::evOnPostGui` (the lambda 0x140032770) for a fade a
    /// menu asked for: the picture goes black over `fadeTotalTime`, the jobs hung on the fade
    /// are done at the first frame after that, and the picture comes back over the same time.
    pub fn post_gui(&mut self, graphics: &mut Graphics, dt: f32) {
        let Some((time, total)) = self.menu_fade else { return };
        if total * 2.0 <= time {
            if !self.gui.sim.fade_listeners.is_empty() {
                self.gui.sim_fade_in_finished();
            }
            self.menu_fade = None;
        } else {
            if total <= time && !self.gui.sim.fade_listeners.is_empty() {
                self.gui.sim_fade_in_finished();
            }
            rustyac_render::gl::fade_overlay(graphics, time, total);
            self.menu_fade = Some((dt + time, total));
        }
    }

    // ----- the commands of `Sim::onKeyDown` 0x14019a940 that act on the GUI and the HUD -----

    /// "Hide Show Apps" (Ctrl+H): `GameScreen::showDesktop`.
    pub fn hide_show_apps(&mut self) {
        self.gui.show_desktop(self.screen);
    }

    /// "Cycle Virtual Desktop" (Ctrl+U): `Taskbar::cycleDekstop`.
    pub fn cycle_desktop(&mut self) -> i16 {
        self.gui.cycle_desktop()
    }

    /// "Driver Names" (Ctrl+L): the name tags on or off, with the game's message.
    pub fn toggle_driver_names(&mut self) -> bool {
        self.names.is_active = !self.names.is_active;
        self.message.add_message(&mut self.gui, "Driver Names", if self.names.is_active { "ON" } else { "OFF" }, MESSAGE_DEFAULT);
        self.names.is_active
    }

    /// "Show Damage" (Ctrl+J): `DamageDisplayer::show`.
    pub fn show_damage(&mut self) {
        self.damage.show();
    }

    /// "Hide Damage" (Ctrl+Q): the damage picture switched on or off, with the game's message.
    pub fn toggle_damage(&mut self) -> bool {
        self.damage.enabled = !self.damage.enabled;
        self.message.add_message(&mut self.gui, "Damage Displayer", if self.damage.enabled { "Enabled" } else { "Disabled" }, MESSAGE_DEFAULT);
        self.damage.enabled
    }

    /// `--gui-do`: what a user would do with the mouse and the keys, and a few things only a
    /// session brings about, for pictures and unattended checks. Commands, `;` between them:
    /// `mouse:X:Y`, `click:X:Y`, `wheel:X:Y:N`, `key:VK`, `apps` (Ctrl+H), `desktop` (Ctrl+U),
    /// `names` (Ctrl+L), `damage` (Ctrl+J), `hidedamage` (Ctrl+Q), `message:Title|Text`,
    /// `car:N` (the bar of car N), `notify:N` (1 the down-shift icon, 2 the wheel warning),
    /// `flag:blue|black|yellow|penalty|chequered|none`, `esc` (the pause menu), `screenshot` (F8), `overlay` (F9: the overlay leaderboard's next face), `pits` (the ESC menu, as "Back to pits" ends), `replay` (the replay's bar, for pictures), `tab` (the table of TAB, until a frame finds the key up), `app` (the Leaderboard app shown or hidden), `console` (Home), `type:<text>` (typed into the console, then Enter).
    pub fn script(&mut self, commands: &str) -> Result<(), String> {
        for command in commands.split(';').map(str::trim).filter(|c| !c.is_empty()) {
            let mut parts = command.splitn(2, ':');
            let (name, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let numbers = || -> Result<Vec<i32>, String> { rest.split(':').map(|n| n.trim().parse::<i32>().map_err(|e| format!("--gui-do {command}: {e}"))).collect() };
            let need = |n: usize| -> Result<Vec<i32>, String> { numbers().and_then(|v| if v.len() == n { Ok(v) } else { Err(format!("--gui-do {command}: {n} numbers")) }) };
            match name {
                "mouse" => {
                    let v = need(2)?;
                    self.mouse_move(v[0], v[1]);
                }
                "click" => {
                    let v = need(2)?;
                    self.mouse_move(v[0], v[1]);
                    self.mouse_down(v[0], v[1], 0);
                    self.mouse_up(v[0], v[1], 0);
                }
                "wheel" => {
                    let v = need(3)?;
                    self.mouse_wheel(v[0], v[1], v[2] as f32);
                }
                "key" => self.key_down(need(1)?[0] as u32),
                // Task 33: Home (the console), a line typed into it with Enter
                "console" => self.console_toggle(),
                "type" => {
                    for unit in rest.encode_utf16() {
                        self.key_char(unit as u32);
                    }
                    self.key_char(13);
                }
                "apps" => self.hide_show_apps(),
                "desktop" => {
                    self.cycle_desktop();
                }
                "names" => {
                    self.toggle_driver_names();
                }
                "damage" => self.show_damage(),
                "hidedamage" => {
                    self.toggle_damage();
                }
                "message" => {
                    let (title, text) = rest.split_once('|').unwrap_or((rest, ""));
                    self.add_message(title, text);
                }
                "car" => self.message.add_car_message(need(1)?[0] as u32, MESSAGE_DEFAULT),
                "notify" => self.notification.show_notification(&mut self.gui, need(1)?[0]),
                "esc" => self.esc_key(false),
                "screenshot" => self.take_screen(),
                "tab" => self.tab_down(),
                "replay" => {
                    // (for pictures: the bar as a replay of 100 s, half way, would show it)
                    let link = &mut self.gui.sim.replay;
                    (link.status, link.time_mult, link.current_frame, link.recorded_frames, link.camera_manager, link.lap_marks) = (0, 1.0, 1666, 3333, true, vec![0.31, 0.68]);
                    self.replay_bar_show(true);
                }
                "pits" => {
                    if self.esc_menu.is_some() {
                        self.gui.sim_activate_esc_menu();
                    }
                }
                "overlay" => {
                    self.overlay_poll(true);
                    self.overlay_poll(false);
                }
                "app" | "show" => {
                    // app (the Leaderboard) or app:<name> / show:<name> (essentials, laps, delta,
                    // realtime, position, tyres, gears, pedals, electronics, trackmap, help,
                    // timeflow, trackdescription, hideall, runningtime): shown or hidden, or shown
                    if rest.is_empty() {
                        let app = self.leaderboard_app;
                        let shown = self.gui.controls[app].visible;
                        self.gui.set_visible(app, !shown);
                    } else if self.apps.made.is_empty() {
                        self.apps.pending.push((rest.to_string(), name == "show"));
                    } else {
                        let Some(app) = self.apps.app(rest) else { return Err(format!("--gui-do {name}:{rest}: no such app")) };
                        let visible = name == "show" || !self.gui.controls[app].visible;
                        self.gui.set_visible(app, visible);
                    }
                }
                "flag" => {
                    let index = ["blue", "black", "yellow", "penalty", "chequered"].iter().position(|f| *f == rest);
                    self.forced_flags = [false; 5];
                    match (index, rest) {
                        (Some(i), _) => self.forced_flags[i] = true,
                        (None, "none") => {}
                        _ => return Err(format!("--gui-do {command}: blue, black, yellow, penalty, chequered or none")),
                    }
                }
                _ => return Err(format!("--gui-do: no command {name:?} (mouse, click, wheel, key, apps, desktop, names, damage, hidedamage, message, car, notify, flag, esc, screenshot, overlay, pits, replay, tab, app)")),
            }
        }
        Ok(())
    }

    /// A message of rustyAC's own on AC's banner.
    pub fn add_message(&mut self, title: &str, description: &str) {
        self.message.add_message(&mut self.gui, title, description, MESSAGE_DEFAULT);
    }

    /// The Telemetry app's physics steps since the last frame (the newest step's values are
    /// also what it reads in the frame); true while it is shown, the physics thread then
    /// records the steps.
    pub fn telemetry_exchange(&mut self, steps: Vec<(f64, Vec<f32>)>) -> bool {
        if let Some(last) = steps.last() {
            self.apps.telemetry.graphics = last.1.clone();
        }
        self.apps.telemetry.steps = steps;
        self.apps.app("telemetry").is_some_and(|id| self.gui.controls[id].visible)
    }

    /// The Telemetry app's skid channels (read in its `render`, like every channel not read on the
    /// physics thread): the player's car's skid sounds as the sound left them.
    pub fn telemetry_skids(&mut self, skids: [[f32; 4]; 2]) {
        for (i, def) in rustyac_gui::app_telemetry::channels(&self.setup.telemetry_car).iter().enumerate() {
            let wheel = ["_LF", "_RF", "_LR", "_RR"].iter().position(|end| def.name.ends_with(end));
            let kind = if def.name.starts_with("SKID_VOL_") { Some(0) } else if def.name.starts_with("SKID_PITCH_") { Some(1) } else { None };
            if let (Some(kind), Some(wheel), Some(value)) = (kind, wheel, self.apps.telemetry.graphics.get_mut(i)) {
                *value = skids[kind][wheel];
            }
        }
    }

    /// The Setup debug app's newest car from the physics thread; true while it is shown.
    pub fn setup_debug_exchange(&mut self, car: Option<rustyac_gui::app_setup_debug::SetupDebugCar>) -> bool {
        if car.is_some() {
            self.apps.setup_debug = car;
        }
        self.apps.app("setupdebug").is_some_and(|id| self.gui.controls[id].visible)
    }

    /// The setup editor's jobs since the last call, for the physics thread; and the physics
    /// thread's newest picture of the car (with its count) for the editor.
    pub fn setup_exchange(&mut self, newest: &(u64, Option<rustyac_gui::setup_car::SetupSnapshot>)) -> Vec<rustyac_physics::car::setup::SetupJob> {
        let Some(data) = &self.setup_data else { return Vec::new() };
        let mut data = data.borrow_mut();
        let jobs = std::mem::take(&mut data.jobs);
        self.setup_jobs_sent += jobs.len() as u64;
        let sent = self.setup_jobs_sent;
        if let (true, true, Some(snapshot)) = (jobs.is_empty(), newest.0 != self.setup_generation, newest.1.as_ref().filter(|snapshot| snapshot.applied_jobs >= sent)) {
            self.setup_generation = newest.0;
            data.snapshot = snapshot.clone();
            self.gui.sim.setup_state = snapshot.setup_state;
        }
        jobs
    }

    /// Task 34: the pit menu as the page "PITSTOP STRATEGY" changed it since the last call,
    /// for the physics thread.
    pub fn pit_page_exchange(&mut self) -> Option<rustyac_physics::race::QuickMenu> {
        let page = self.gui.setup.pit.as_ref().filter(|page| page.edits != self.pit_edits_sent)?;
        self.pit_edits_sent = page.edits;
        Some(page.menu.clone())
    }

    /// What the game does at its end (`GameScreen::shutdown` 0x1402510c0, `GUI::serializeForms`
    /// 0x1402551e0, `DriverNameDisplayer::shutdown` 0x14008e080, `DamageDisplayer::shutdown`
    /// 0x14007b800, `OverlayLeaderboard::shutdown` 0x1401c1a30), into rustyAC's own cfg folder: the forms' layout, and the two switches in
    /// a `gameplay.ini` that starts as a copy of the game's.
    pub fn shutdown(&mut self) -> Result<Vec<PathBuf>, String> {
        let folder = self.setup.cfg_dir.clone();
        rustyac_content::guard::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let mut written = Vec::new();
        self.gui.game_screen_shutdown(self.screen);
        if let Some(bytes) = self.gui.serialize_forms() {
            let path = folder.join("acos.ini");
            rustyac_content::guard::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            written.push(path);
        }
        let path = folder.join("gameplay.ini");
        let start = std::fs::read_to_string(&path).ok().or_else(|| self.setup.documents_cfg.as_ref().and_then(|d| std::fs::read_to_string(d.join("gameplay.ini")).ok())).unwrap_or_default();
        let text = set_ini_key(&start, "DRIVER_NAME_DISPLAYER", "IS_ACTIVE", if self.names.is_active { "1" } else { "0" });
        let mut text = set_ini_key(&text, "DAMAGE_DISPLAYER", "IS_ACTIVE", if self.damage.enabled { "1" } else { "0" });
        // OverlayLeaderboard::shutdown 0x1401c1a30
        if let Some(overlay) = &self.overlay {
            let (active, vertical) = overlay.shutdown();
            text = set_ini_key(&text, "OVERLAY_LEADERBOARD", "ACTIVE", &active);
            text = set_ini_key(&text, "OVERLAY_LEADERBOARD", "VERTICAL", &vertical);
        }
        rustyac_content::guard::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
        // ESCMenu::onShutdown 0x1400fdf80: the setup as it is, as `last.ini` (rustyAC's own folder)
        if let (Some(menu), Some(car)) = (self.esc_menu, &self.gui.setup_car) {
            let last = self.setup.setups_dir.join(car.unix_name()).join("generic").join("last.ini");
            let root = self.setup.setups_dir.clone();
            self.gui.esc_menu_on_shutdown(menu, &root);
            if last.is_file() {
                written.push(last);
            }
        }
        Ok(written)
    }
}

/// "DDMMYY-HHMMSS" of now (the name of the file the page "Time" prints).
fn time_stamp() -> String {
    let seconds = crate::ac_replay::unix_seconds() as i64;
    // days since 1970 to a date (the civil calendar)
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!("{day:02}{month:02}{:02}-{:02}{:02}{:02}", year.rem_euclid(100), rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `WritePrivateProfileStringW`'s effect on an ini text: the key of the section is set (the
/// section is added at the end when it is not there, the key at the section's end).
pub fn set_ini_key(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let header = format!("[{section}]");
    let Some(start) = lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&header)) else {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(format!("{key}={value}"));
        return lines.join("\r\n") + "\r\n";
    };
    let end = lines.iter().enumerate().skip(start + 1).find(|(_, l)| l.trim_start().starts_with('[')).map_or(lines.len(), |(i, _)| i);
    let found = (start + 1..end).find(|i| lines[*i].split('=').next().is_some_and(|k| k.trim().eq_ignore_ascii_case(key)));
    match found {
        Some(i) => lines[i] = format!("{key}={value}"),
        None => {
            // after the section's last line that is not blank
            let at = (start + 1..end).rev().find(|i| !lines[*i].trim().is_empty()).map_or(start + 1, |i| i + 1);
            lines.insert(at, format!("{key}={value}"));
        }
    }
    lines.join("\r\n") + "\r\n"
}

/// The cars' names for the HUD: the drivers and nations of a session file (`race.ini`'s
/// `[CAR_n] DRIVER_NAME` / `NATION_CODE`), the cars' names from their `ui/ui_car.json`.
pub fn car_names(game: &Path, race_ini: Option<&IniReader>, folders: &[String]) -> Vec<[String; 4]> {
    folders
        .iter()
        .enumerate()
        .map(|(i, folder)| {
            let section = format!("CAR_{i}");
            let key = |name: &str| race_ini.filter(|ini| ini.has_key(&section, name)).map(|ini| ini.get_string(&section, name)).unwrap_or_default();
            let mut driver = key("DRIVER_NAME");
            if driver.is_empty() {
                driver = if i == 0 { "Player".to_string() } else { format!("Driver {i}") };
            }
            let screen = rustyac_content::vfs::read_to_string(&game.join("content/cars").join(folder).join("ui/ui_car.json")).ok().and_then(|json| json_string(&json, "name")).unwrap_or_else(|| folder.clone());
            [driver, screen, folder.clone(), key("NATION_CODE")]
        })
        .collect()
}

/// The first `"key": "value"` of a JSON text (the cars' `ui_car.json` are flat).
fn json_string(json: &str, key: &str) -> Option<String> {
    let at = json.find(&format!("\"{key}\""))?;
    let rest = &json[at + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

/// `PythonInterface::PythonInterface` 0x140124920 in rustyAC: the player's `python33.dll`, the
/// apps of `python.ini` (rustyAC's cfg folder, else the game's Documents), `py_log.txt` in
/// rustyAC's own `logs`. The working directory becomes the game's folder, as the apps expect.
/// No DLL: the game exits ("Failed to load Python, exiting"); rustyAC runs on without them.
#[cfg(windows)]
fn start_python(gui: &mut Gui, painter: &mut dyn rustyac_gui::painter::Painter, screen: Id, setup: &GuiSetup) -> Option<rustyac_python::Python> {
    let own = setup.cfg_dir.parent().map(Path::to_path_buf).unwrap_or_default();
    let ini = Some(setup.cfg_dir.join("python.ini")).filter(|p| p.is_file()).or_else(|| setup.documents_cfg.as_ref().map(|cfg| cfg.join("python.ini"))).unwrap_or_default();
    // (every path option is absolute since parsing, and every write goes through
    // rustyac_content::guard, which refuses the game's folder)
    if let Err(e) = std::env::set_current_dir(&setup.game) {
        println!("Python: no apps ({}: {e})", setup.game.display());
        return None;
    }
    let world = rustyac_python::World {
        cars: (0..gui.apps.cars.len()).map(|i| rustyac_python::CarInfo {
            skin: setup.skins.get(i).cloned().unwrap_or_default(),
            nation_code: setup.cars.get(i).map(|c| c[3].clone()).unwrap_or_default(),
            ballast_kg: setup.python_cars.get(i).map_or(0.0, |c| c.ballast_kg),
            restrictor: setup.python_cars.get(i).map_or(0.0, |c| c.restrictor),
            min_height: setup.python_cars.get(i).map_or((i == 0).then_some(-1.0), |c| c.min_height),
            ai_controlled: setup.python_cars.get(i).is_some_and(|c| c.ai_controlled),
            camera_count: setup.python_cars.get(i).map_or(0, |c| c.camera_count),
            ..Default::default()
        }).collect(),
        server: setup.server.clone().map(|(name, ip, http_port)| (name, ip, http_port, gui.apps.connected_cars_count)),
        ..Default::default()
    };
    match rustyac_python::start(&setup.game, &own, &ini, gui, painter, screen, world) {
        Ok(python) => {
            println!("Python: {} app(s): {}", python.apps().len(), python.apps().join(", "));
            Some(python)
        }
        Err(e) => {
            println!("Python: no apps ({e}; the game exits here: \"Failed to load Python, exiting\")");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_keys_are_set_in_place() {
        let text = "[A]\r\nX=1\r\n\r\n[DAMAGE_DISPLAYER]\r\nIS_ACTIVE=1\r\n";
        assert_eq!(set_ini_key(text, "DAMAGE_DISPLAYER", "IS_ACTIVE", "0"), "[A]\r\nX=1\r\n\r\n[DAMAGE_DISPLAYER]\r\nIS_ACTIVE=0\r\n");
        assert_eq!(set_ini_key(text, "A", "Y", "2"), "[A]\r\nX=1\r\nY=2\r\n\r\n[DAMAGE_DISPLAYER]\r\nIS_ACTIVE=1\r\n");
        assert_eq!(set_ini_key("", "B", "K", "v"), "[B]\r\nK=v\r\n");
        assert_eq!(set_ini_key("[A]\nX=1", "B", "K", "v"), "[A]\r\nX=1\r\n\r\n[B]\r\nK=v\r\n");
    }

    #[test]
    fn json_names() {
        assert_eq!(json_string("{\n \"name\": \"Ferrari \\\"F2004\\\"\", \"brand\": \"x\"}", "name").as_deref(), Some("Ferrari \"F2004\""));
        assert_eq!(json_string("{}", "name"), None);
    }
}
