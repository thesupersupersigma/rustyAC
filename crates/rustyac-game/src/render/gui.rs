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
    setup: GuiSetup,
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
    /// `Sim::endSessionDisplayer`
    pub end: EndSession,
    /// `ReplayManager::replayInterface`: the replay's bar (when the session has a replay)
    pub replay_bar: Option<Id>,
    /// `Sim::escMenu` (off line; an online session has none yet)
    pub esc_menu: Option<Id>,
    /// a new session began since the window last asked (the car is its own again: a lock of
    /// the controls the ESC menu wants has to be asked for anew)
    pub pits_lock_stale: bool,
    /// the `OverlayLeaderboard` (with more than one car, outside a drag race)
    pub overlay: Option<OverlayLeaderboard>,
    /// `Sim::leaderboard`: the table held on TAB (with more than one car, or online)
    pub tab: Option<SessionLeaderboard>,
    /// the app "Leaderboard"
    pub leaderboard_app: Id,
    /// what the tables read of every car that never changes (names, nation, folder, skin)
    race_static: Vec<RaceCar>,
    /// `RaceManager::getCurrentSessionIndex` as of the last frame
    last_session_index: i32,
    /// `ScreenCapturer`: `readyToCapture`, `frameSkip`, `flashTime`
    shot_ready: bool,
    shot_frame_skip: i32,
    screen_flash: f32,
}

fn read(path: PathBuf) -> Option<IniReader> {
    IniReader::load(&path).ok().filter(|ini| ini.ready)
}

/// rustyAC's own copy of a cfg file, else the game's (read only).
fn own_or_documents(setup: &GuiSetup, name: &str) -> Option<IniReader> {
    read(setup.cfg_dir.join(name)).or_else(|| setup.documents_cfg.as_ref().and_then(|folder| read(folder.join(name))))
}

impl GameGui {
    /// The GUI and the HUD objects, in the order `Sim::Sim` 0x140192070 makes them.
    pub fn new(graphics: &mut Graphics, setup: GuiSetup) -> Result<GameGui, String> {
        let mut renderer = GuiRenderer::new(graphics);
        let system = |name: &str| read(setup.game.join("system/cfg").join(name));
        let gameplay = own_or_documents(&setup, "gameplay.ini");
        let files = GuiFiles {
            skin: IniReader::load(&setup.game.join("content/gui/skin.ini")).map_err(|e| e.to_string())?,
            acos: own_or_documents(&setup, "acos.ini"),
            gameplay: own_or_documents(&setup, "gameplay.ini"),
            assetto_corsa: system("assetto_corsa.ini"),
        };
        let mut painter = renderer.painter(graphics);
        let mut gui = Gui::new(&mut painter, files);
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
        let esc_menu = (!setup.online).then(|| {
            let id = gui.new_esc_menu(&mut painter, system("fades.ini").as_ref(), rustyac_gui::esc_menu::EscPages::default());
            gui.add_root(&mut painter, id);
            gui.sim.esc_menu = Some(id);
            id
        });
        gui.sim.actions.clear();
        // the apps (Sim::addForm): of the game's 56 only the Leaderboard so far
        let leaderboard_app = gui.new_leaderboard_app(&mut painter);
        gui.game_screen_add_control(&mut painter, screen, leaderboard_app, true);
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
        let pit_stop = PitStopHud::new(&mut painter);
        Ok(GameGui { renderer, gui, screen, message, notification, wrong, lap, drive, lock, proximity, names, damage, flags, fuel, input, setup, black_flagged: false, last_focus: 0, aids: None, caption: String::new(), was_replay: false, forced_flags: [false; 5], pause_menu, menu_fade: None, pit_menu: None, pit_looks, pit_stop, pit: None, end, replay_bar, esc_menu, pits_lock_stale: false, overlay, tab, leaderboard_app, race_static, last_session_index: 0, shot_ready: false, shot_frame_skip: 0, screen_flash: 0.0 })
    }

    /// The update pass: the session's state is read, the objects that have an `update` run.
    /// `camera` is the scene camera's matrix, its view and its perspective; `camera_mode`
    /// `ACCameraManager::mode`; `time_ms` the game's clock.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, world: &[CarView], focus: usize, info: &HudInfo, camera: (&Mat44f, &Mat44f, &Mat44f), camera_mode: i32, time_ms: f64, dt: f32) {
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
        input.pause_menu_visible = self.gui.controls[self.pause_menu].visible;
        input.in_replay = info.replay;
        input.focused_car_index = focus as i32;
        input.camera_mode = camera_mode;
        input.camera_matrix = *camera.0;
        input.game_time_now = time_ms;
        self.pit = info.referee.as_ref().and_then(|referee| referee.pit_gui.clone());
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
            // Sim::evOnNewSession, as the tables hear it
            if index != self.last_session_index {
                self.last_session_index = index;
                self.gui.sim_on_new_session();
                self.pits_lock_stale = true;
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
    }

    /// `ksgui::GUI::render` 0x140254c10: the 2D pass's states, the game screen with its forms
    /// (and the flags), the app bar.
    pub fn render(&mut self, graphics: &mut Graphics, dt: f32) {
        let mut painter = self.renderer.painter(graphics);
        self.gui.render_all(dt, &mut painter);
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
            self.pit_stop.render_hud(&mut painter, &scene, input.race_over, input.cars.first().map_or(0.0, |car| car.lock_controls_time), &mut state, &pit.times);
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
    }

    // ----- the window's input (`ksgui::GUI`'s listeners of the mouse and the keyboard) -----

    // (the GUI hears of the mouse first, then the game objects that hung themselves on the
    // window's mouse events: the end-of-session displayer)

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        let e = MouseEvent { x, y, ..MouseEvent::default() };
        self.gui.mouse_move(&e);
        self.end.on_mouse_move(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_move(&mut self.gui, &e);
        }
    }

    pub fn mouse_down(&mut self, x: i32, y: i32, button: i32) {
        let e = MouseEvent { x, y, button, delta: 0.0 };
        self.gui.mouse_down(&e);
        self.end.on_mouse_down(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_down(&mut self.gui, &e);
        }
    }

    pub fn mouse_up(&mut self, x: i32, y: i32, button: i32) {
        let e = MouseEvent { x, y, button, delta: 0.0 };
        self.gui.mouse_up(&e);
        self.end.on_mouse_up(&mut self.gui, &e);
        if let Some(tab) = &self.tab {
            tab.on_mouse_up(&mut self.gui, &e);
        }
    }

    pub fn mouse_wheel(&mut self, x: i32, y: i32, delta: f32) {
        let e = MouseEvent { x, y, button: 0, delta };
        self.gui.mouse_wheel(&e);
        self.end.on_mouse_wheel(&mut self.gui, &e);
    }

    pub fn key_down(&mut self, key: u32) {
        self.gui.key_down(key);
    }

    pub fn key_char(&mut self, key: u32) {
        self.gui.key_char(key);
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
                other => out.push(other),
            }
        }
        out
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
    /// `flag:blue|black|yellow|penalty|chequered|none`, `esc` (the pause menu), `screenshot` (F8), `overlay` (F9: the overlay leaderboard's next face), `pits` (the ESC menu, as "Back to pits" ends), `replay` (the replay's bar, for pictures), `tab` (the table of TAB, until a frame finds the key up), `app` (the Leaderboard app shown or hidden).
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
                "app" => {
                    let app = self.leaderboard_app;
                    let shown = self.gui.controls[app].visible;
                    self.gui.set_visible(app, !shown);
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

    /// What the game does at its end (`GameScreen::shutdown` 0x1402510c0, `GUI::serializeForms`
    /// 0x1402551e0, `DriverNameDisplayer::shutdown` 0x14008e080, `DamageDisplayer::shutdown`
    /// 0x14007b800, `OverlayLeaderboard::shutdown` 0x1401c1a30), into rustyAC's own cfg folder: the forms' layout, and the two switches in
    /// a `gameplay.ini` that starts as a copy of the game's.
    pub fn shutdown(&mut self) -> Result<Vec<PathBuf>, String> {
        let folder = self.setup.cfg_dir.clone();
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let mut written = Vec::new();
        self.gui.game_screen_shutdown(self.screen);
        if let Some(bytes) = self.gui.serialize_forms() {
            let path = folder.join("acos.ini");
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
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
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
        Ok(written)
    }
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
            let screen = std::fs::read_to_string(game.join("content/cars").join(folder).join("ui/ui_car.json")).ok().and_then(|json| json_string(&json, "name")).unwrap_or_else(|| folder.clone());
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
