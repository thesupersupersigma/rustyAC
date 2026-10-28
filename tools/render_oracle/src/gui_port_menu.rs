// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's menus: `rustyac_gui::menus` fed the script's session
//! state; what the menus ask of the session is written down as the game's side writes it.

use rustyac_gui::control::{Id, MouseEvent};
use rustyac_gui::gui::Gui;
use rustyac_gui::menus::SimAction;
use rustyac_gui::painter::Painter;
use rustyac_physics::data::ini::IniReader;

use crate::gui_port_hud::HudPort;
use crate::gui_seq_menu::{action, script};

#[derive(Default)]
pub struct Menus {
    pub pause: Option<Id>,
    pub esc: Option<Id>,
    pub boards: Vec<Id>,
    pub end: Option<rustyac_gui::end_session::EndSession>,
    /// a plain control where the game's stand-in `Sim` has one for the pause menu (its
    /// "shows" flag is the script's), until the script makes the real menu
    pub pause_stand_in: Option<Id>,
    pub tab: Option<rustyac_gui::leaderboard::SessionLeaderboard>,
    pub app: Option<Id>,
    pub overlay: Option<rustyac_gui::overlay_leaderboard::OverlayLeaderboard>,
    /// the ESC menu itself (`esc` is the stand-in the pause menu's scripts have)
    pub pits: Option<Id>,
    /// the replay's bar; `CameraDrivableManager`'s camera; `ReplayManager::lastFilename`
    pub replay: Option<Id>,
    pub drivable_camera: i32,
    pub last_replay_name: String,
    pub client: Option<(rustyac_gui::online_hud::ClientHud, rustyac_gui::online_hud::QosOverlay)>,
}

fn replay_model(gui: &Gui) -> crate::gui_seq_menu::ReplayModel {
    let r = &gui.sim.replay;
    crate::gui_seq_menu::ReplayModel { status: r.status, time_mult: r.time_mult, slow_mo_level: r.slow_mo_level, current_frame: r.current_frame, recorded_frames: r.recorded_frames, cut_in: r.cut_in, cut_out: r.cut_out, has_loaded: r.has_loaded_replay }
}

fn replay_store(gui: &mut Gui, m: &crate::gui_seq_menu::ReplayModel) {
    let r = &mut gui.sim.replay;
    (r.status, r.time_mult, r.slow_mo_level, r.current_frame, r.recorded_frames, r.cut_in, r.cut_out, r.has_loaded_replay) = (m.status, m.time_mult, m.slow_mo_level, m.current_frame, m.recorded_frames, m.cut_in, m.cut_out, m.has_loaded);
}

/// What the game's side writes into its stand-in `Sim` before every call: the script's state.
pub fn sync(hud: &mut HudPort, gui: &mut Gui) {
    let s = script();
    gui.sim.session_type = hud.input.session_type;
    gui.sim.online = hud.input.online;
    gui.sim.race = crate::gui_seq_menu::race_link(&hud.input);
    gui.sim.in_replay = hud.input.in_replay;
    gui.sim.camera_mode = hud.input.camera_mode;
    if let Some(end) = &hud.more.menus.end {
        hud.input.end_session_active = end.is_active;
    }
    gui.sim.session_count = s.session_count;
    if crate::gui_seq_setup::time_seed() != 0.0 {
        gui.sim.time = crate::gui_seq_setup::time_link(gui.sim.race.cars.len(), gui.sim.race.current_session_index, hud.input.focused_car_index.max(0) as usize);
    }
    if let Some(car) = hud.input.cars.first() {
        crate::gui_port_setup::set_state(&car.body_matrix);
    }
    gui.sim.time_to_session_start = hud.input.time_to_session_start;
    gui.sim.has_replay_manager = s.replay_manager;
    if let Some(pits) = hud.more.menus.pits {
        hud.input.esc_menu_visible = gui.controls[pits].visible;
        hud.input.esc_menu_width = gui.controls[pits].rect.right;
    }
    if let Some(esc) = hud.more.menus.esc {
        gui.controls[esc].visible = hud.input.esc_menu_visible;
    }
    gui.sim.replay.lap_marks = s.replay.lap_marks.clone();
    gui.sim.replay.camera_manager = s.replay.camera_manager;
    gui.sim.replay.track_camera_sets = s.replay.camera_sets.clone();
    gui.sim.replay.camera_index = s.replay.camera_index;
    gui.sim.replay.auto_file_name = crate::gui_seq_menu::REPLAY_AUTO_NAME.to_string();
    gui.sim.replay.record_interval_ms = 30.0;
    // (the game's side reads the scratch root's video.ini at every press of "Photo mode")
    gui.sim.replay.post_processing = IniReader::load(&crate::root::documents().join("Assetto Corsa/cfg/video.ini")).ok().filter(|ini| ini.ready && ini.has_section("POST_PROCESS")).map(|ini| ini.get_int("POST_PROCESS", "ENABLED").unwrap_or(0) != 0);
    let e = s.esc;
    gui.sim.setup_state = e.setup_state;
    gui.sim.fixed_setup = e.fixed_setup;
    gui.sim.time_to_wait = e.time_to_wait;
    gui.sim.game_time = e.game_time;
    gui.sim.player_in_pit = e.in_pit;
    gui.sim.session_time_left = hud.input.session_time_left;
    let o = &s.online;
    let link = &mut gui.sim.online_link;
    link.server_name = o.server_name.clone();
    (link.tc, link.abs, link.stability, link.auto_clutch) = (o.tc, o.abs, o.stability, o.auto_clutch);
    (link.damage_rate, link.fuel_rate, link.tyre_wear_rate, link.tyre_blankets, link.jump_start_mode) = (o.damage, o.fuel, o.tyre, o.blankets, o.jump_start);
    let target = hud.input.cars.get(o.target as usize).filter(|_| o.target >= 0);
    link.vote = rustyac_gui::race_control_online::VoteLink { vote_type: o.vote_type, has_voted: o.has_voted, time_left: o.time_left, max_time: o.max_time, target_name: target.map_or(String::new(), |c| c.driver_name.clone()), target_guid: if target.is_some() { o.target } else { -1 } };
    gui.sim.track_grip = e.grip;
    gui.sim.sun_angle = e.sun_angle;
    gui.sim.ambient_temperature = e.ambient;
    gui.sim.road_temperature = e.road;
    gui.sim.wind_speed = e.wind_speed;
    gui.sim.wind_direction_deg = e.wind_direction;
    if let Some(pause) = hud.more.menus.pause {
        hud.input.pause_menu_visible = gui.controls[pause].visible;
    } else if let Some(stand_in) = hud.more.menus.pause_stand_in {
        gui.controls[stand_in].visible = hud.input.pause_menu_visible;
    }
}

/// The calls the menus made since the last time, written down; the one on the message banner
/// is carried out.
pub fn drain(hud: &mut HudPort, gui: &mut Gui) {
    let mut focus = false;
    for a in std::mem::take(&mut gui.sim.actions) {
        match a {
            SimAction::SetPauseMode(paused) => action(format!("setPauseMode {}", paused as u8)),
            SimAction::ClearMessage => {
                if let Some(message) = &mut hud.message {
                    message.clear_message();
                }
            }
            SimAction::AudioStop => action("audio stop".into()),
            SimAction::AudioStart => action("audio start".into()),
            SimAction::SkipCurrentSession => action("skipCurrentSession".into()),
            SimAction::RestartCurrentSession => action("restartCurrentSession".into()),
            SimAction::FadeIn(seconds) => action(format!("fadeIn {:08x}", seconds.to_bits())),
            SimAction::LockControlsUntilTime(ms) => action(format!("lockControlsUntilTime {:016x}", ms.to_bits())),
            SimAction::GoToSpawnPosition(name) => action(format!("goToSpawnPosition {name}")),
            SimAction::StartReplayMode => action("startReplayMode 0".into()),
            SimAction::Shutdown => action("shutdown".into()),
            SimAction::SendVote { kind, value, guid } => action(format!("sendVote {kind} {} {guid}", value as u8)),
            SimAction::SendChat(text) => {
                action(format!("sendChat {text}"));
                gui.sim.online_link.last_chat_message = gui.sim.game_time;
            }
            SimAction::SetFocusedCarIndex(car) => action(format!("setFocusedCarIndex {car}")),
            SimAction::SetCameraMode(mode) => action(format!("setMode {mode}")),
            SimAction::SetIntroMode(on) => action(format!("setIntroMode {}", on as u8)),
            SimAction::SetControlsLock(on) => action(format!("setControlsLock {}", on as u8)),
            SimAction::AddMessage { title, text } => {
                if let Some(message) = &mut hud.message {
                    message.add_message(gui, &title, &text, 0);
                }
            }
            SimAction::StartRace(wait) => action(format!("startRace {:08x}", wait.to_bits())),
            // (a member the game writes without a call: its side sees it when the job is over)
            SimAction::FocusPlayerCar => focus = true,
            SimAction::DriveMenuPressed => {}
            SimAction::LoadPreviousSessionUserCamera => action("loadPreviousSessionUserCamera".into()),
            SimAction::Replay(asked) => {
                use rustyac_gui::replay_interface::ReplayAction;
                let mut m = replay_model(gui);
                let mut apply = |what: &str, value: f32| crate::gui_seq_menu::replay_apply(&mut m, what, value);
                match asked {
                    ReplayAction::Rewind => action(apply("rewind", 0.0)),
                    ReplayAction::FastForward => action(apply("fastForward", 0.0)),
                    ReplayAction::Play => action(apply("play", 0.0)),
                    ReplayAction::Pause => action(apply("pause", 0.0)),
                    ReplayAction::Stop => action(apply("stop", 0.0)),
                    ReplayAction::NextFrame => action(apply("nextFrame", 0.0)),
                    ReplayAction::SlowMotion(level) => action(apply("slowMotion", level)),
                    ReplayAction::StopReplayMode => action(apply("stopReplayMode", 0.0)),
                    ReplayAction::SetNormalizedPosition(p) => action(apply("setNormalizedPosition", p)),
                    // (nobody listens in the scripts)
                    ReplayAction::SliderInteraction => {}
                    // (the game's own code on its side: no stand-in writes these down)
                    ReplayAction::SetCutInPerc(p) => drop(apply("cutIn", p)),
                    ReplayAction::SetCutOutPerc(p) => drop(apply("cutOut", p)),
                    ReplayAction::Save(name) => {
                        action(format!("replay save {name:?} 1"));
                        hud.more.menus.last_replay_name = name;
                    }
                }
                replay_store(gui, &m);
            }
            // CameraDrivableManager::setCurrentCamera: five cameras, the others are not taken
            SimAction::SetDrivableCamera(camera) => {
                if camera as u32 <= 4 {
                    hud.more.menus.drivable_camera = camera;
                }
            }
            SimAction::SetTrackCameraSet(set) => action(format!("setCurrentCameraSet {set}")),
            SimAction::SetCameraModeKept(mode) => action(format!("setMode {mode}")),
        }
    }
    if focus {
        action("focusPlayerCar".into());
    }
    if let Some(pits) = hud.more.menus.pits {
        hud.input.esc_menu_visible = gui.controls[pits].visible;
    }
    if let Some(pause) = hud.more.menus.pause {
        hud.input.pause_menu_visible = gui.controls[pause].visible;
    }
}

fn button(gui: &Gui, menu: Id, name: &str) -> Option<[f32; 4]> {
    let parent = gui.controls[menu].rect;
    gui.controls[menu].controls.iter().find(|c| gui.controls[**c].name == name).map(|c| {
        let r = gui.controls[*c].rect;
        [parent.left + r.left, parent.left + r.right, parent.top + r.top, parent.top + r.bottom]
    })
}

pub fn call(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, root: &std::path::Path, screen: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    if crate::gui_seq_menu::menu_input(what, number, text) {
        return Ok(());
    }
    if hud.message.is_none() && what != "menu.chat" {
        return Err("the HUD was not made".into());
    }
    sync(hud, gui);
    match what {
        "menu.setupCar" => {
            gui.locale = rustyac_gui::locale::Locale::load(root, &crate::gui_seq::language_or_en());
            gui.setup.track_name = "nowhere".to_string();
            (gui.setup.tc_assist, gui.setup.abs_assist) = (1, 1);
            gui.setup.tracks = crate::gui_seq_setup::tracks(root);
            gui.setup.setups_save = root.join("port_documents/Assetto Corsa/setups");
            gui.setup.use_mouse_pitstop = true;
            gui.setup_car = Some(Box::new(crate::gui_port_setup::make_car(root, text, number as i32)?));
        }
        "menu.setupPit" => crate::gui_port_pit::setup_pit(gui, root)?,
        "menu.setupState" => crate::gui_seq_setup::set_car_state(number),
        "menu.setupServer" => {
            let menu = gui.sim.esc_menu.ok_or("no ESC menu")?;
            let screen = gui.esc_menu(menu).setup_screen.ok_or("no setup screen")?;
            let values = crate::gui_seq_setup::server_values(text);
            gui.setup_screen_load_server_setup(screen, &values, &root.join("port_documents/Assetto Corsa/setups"));
        }
        "menu.timeLaps" => crate::gui_seq_setup::set_time_seed(number),
        "menu.lb" => {
            let board = gui.new_leaderboard(painter, number as i32, text == "focus");
            gui.add_root(painter, board);
            hud.more.menus.boards.push(board);
        }
        "menu.lbPos" | "menu.lbSize" | "menu.lbSet" | "menu.lbLast" | "menu.lbVisible" => {
            let board = *hud.more.menus.boards.last().ok_or("no leaderboard")?;
            let pair = || -> (f32, f32) {
                let (x, y) = text.split_once(',').unwrap_or(("0", "0"));
                (x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0))
            };
            match what {
                "menu.lbPos" => gui.set_position(board, pair().0, pair().1),
                "menu.lbSize" => gui.set_size(board, pair().0, pair().1),
                "menu.lbSet" => match text {
                    "ui" => gui.leaderboard_mut(board).ui = number as i32,
                    "show" => gui.leaderboard_mut(board).show_mode_buttons = number != 0.0,
                    "loaded" => gui.leaderboard_mut(board).is_loaded = number != 0.0,
                    other => return Err(format!("no leaderboard member {other:?}")),
                },
                "menu.lbLast" => gui.leaderboard_set_last_session(board),
                _ => gui.set_visible(board, number != 0.0),
            }
        }
        "menu.newSession" => gui.sim_on_new_session(),
        "menu.end" => {
            if gui.sim.pause_menu.is_none() {
                let stand_in = gui.new_control(painter, "PAUSE_MENU");
                gui.controls[stand_in].visible = hud.input.pause_menu_visible;
                gui.sim.pause_menu = Some(stand_in);
                hud.more.menus.pause_stand_in = Some(stand_in);
            }
            let ini = IniReader::load(&root.join("system/cfg/endSessionDisplayer.ini")).ok();
            hud.more.menus.end = Some(rustyac_gui::end_session::EndSession::new(gui, painter, ini.as_ref()));
        }
        "menu.tab" => hud.more.menus.tab = Some(rustyac_gui::leaderboard::SessionLeaderboard::new(gui, painter)),
        "menu.tabShow" => {
            let Some(tab) = &mut hud.more.menus.tab else { return Err("no TAB leaderboard".into()) };
            tab.display(gui, number != 0.0);
        }
        "menu.overlay" => {
            let mut overlay = rustyac_gui::overlay_leaderboard::OverlayLeaderboard::new(gui, painter, None);
            overlay.gap_display_mode = number as i32;
            overlay.vertical_layout = text == "v";
            hud.more.menus.overlay = Some(overlay);
        }
        "menu.overlayMode" => {
            let Some(overlay) = &mut hud.more.menus.overlay else { return Err("no overlay leaderboard".into()) };
            overlay.gap_display_mode = number as i32;
            overlay.vertical_layout = text == "v";
        }
        "menu.overlayF9" => {
            let Some(overlay) = &mut hud.more.menus.overlay else { return Err("no overlay leaderboard".into()) };
            if let (Some(words), Some(message)) = (overlay.update(true), &mut hud.message) {
                message.add_message(gui, "Overlay Leaderboard", words, 0);
            }
            overlay.update(false);
        }
        "menu.app" => {
            let app = gui.new_leaderboard_app(painter);
            gui.game_screen_add_control(painter, screen, app, true);
            hud.more.menus.app = Some(app);
        }
        "menu.appVisible" => {
            let app = hud.more.menus.app.ok_or("no leaderboard app")?;
            gui.set_visible(app, number != 0.0);
        }
        "menu.endShow" | "menu.endHide" => {
            let Some(end) = &mut hud.more.menus.end else { return Err("no end of session".into()) };
            if what == "menu.endShow" {
                end.display_results(gui);
            } else {
                end.hide(gui);
            }
        }
        "menu.pause" => {
            gui.sim.online = hud.input.online;
            gui.sim.game_screen = Some(screen);
            let esc = gui.new_control(painter, "ESC_MENU");
            gui.controls[esc].visible = false;
            gui.sim.esc_menu = Some(esc);
            hud.more.menus.esc = Some(esc);
            let fades = IniReader::load(&root.join("system/cfg/fades.ini")).ok();
            let menu = gui.new_pause_menu(painter, fades.as_ref());
            gui.sim.pause_menu = Some(menu);
            gui.set_visible(menu, false);
            gui.add_root(painter, menu);
            hud.more.menus.pause = Some(menu);
        }
        "menu.esc" => {
            gui.sim_activate_pause_menu();
            gui.sim_set_pause_mode(true);
        }
        "menu.resume" => gui.sim_activate_game_gui(),
        "menu.blank" => {
            let menu = hud.more.menus.pause.ok_or("no pause menu")?;
            gui.pause_menu_blank_button(menu, number as i32, text == "1");
        }
        "menu.hover" | "menu.click" => {
            let menu = hud.more.menus.pause.ok_or("no pause menu")?;
            if let Some(r) = button(gui, menu, text) {
                let (x, y) = (((r[0] + r[1]) * 0.5) as i32, ((r[2] + r[3]) * 0.5) as i32);
                gui.mouse_move(&MouseEvent { x, y, button: -1, delta: 0.0 });
                if what == "menu.click" {
                    gui.mouse_down(&MouseEvent { x, y, button: 0, delta: 0.0 });
                    gui.mouse_up(&MouseEvent { x, y, button: 0, delta: 0.0 });
                }
            }
        }
        "menu.fadeDone" => gui.sim_fade_in_finished(),
        "menu.qos" => {
            let client = rustyac_gui::online_hud::ClientHud::new(gui, painter);
            let qos = rustyac_gui::online_hud::QosOverlay::new(gui, painter);
            hud.more.menus.client = Some((client, qos));
        }
        "menu.replay" => {
            replay_store(gui, &crate::gui_seq_menu::ReplayModel::default());
            sync(hud, gui);
            let id = gui.new_replay_interface(painter);
            gui.add_root(painter, id);
            gui.set_visible(id, false);
            // (what the constructor's own calls made the spinners say is not listened to yet)
            gui.widget_events.clear();
            hud.more.menus.replay = Some(id);
        }
        "menu.replayShow" => {
            sync(hud, gui);
            let id = hud.more.menus.replay.ok_or("no replay bar")?;
            gui.set_visible(id, number != 0.0);
            if number != 0.0 {
                gui.replay_reset_cuts(id);
            }
        }
        "menu.replaySet" => {
            let mut m = replay_model(gui);
            crate::gui_seq_menu::replay_set(&mut m, text, number);
            replay_store(gui, &m);
        }
        "menu.cameraIndex" => {
            sync(hud, gui);
            let id = hud.more.menus.replay.ok_or("no replay bar")?;
            gui.replay_on_camera_selected_index(id, number as i32);
        }
        "menu.replaySaved" => {
            let id = hud.more.menus.replay.ok_or("no replay bar")?;
            let name = hud.more.menus.last_replay_name.clone();
            gui.replay_did_finished_saving(id, number != 0.0, &name);
        }
        "menu.escMenu" => {
            use rustyac_gui::esc_menu::EscPages;
            gui.sim.online = hud.input.online;
            gui.sim.game_screen = Some(screen);
            sync(hud, gui);
            let fades = IniReader::load(&root.join("system/cfg/fades.ini")).ok();
            fn setup(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                let id = gui.new_control(painter, "SETUP_SCREEN");
                gui.controls[id].back_color = [0.0; 4];
                gui.set_position(id, 100.0, 0.0);
                gui.controls[id].visible = false;
                gui.controls[id].draw_border = false;
                gui.controls[id].draw_background = false;
                id
            }
            fn telemetry(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                gui.new_form(painter, "COMPARATIVE_TELEMETRY", false)
            }
            fn comparative_telemetry(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                gui.new_comparative_telemetry(painter)
            }
            let telemetry = if crate::gui_seq_tele::active() { comparative_telemetry } else { telemetry };
            fn table(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                gui.new_control(painter, "ESC_MENU_TIME_TABLE")
            }
            fn setup_editor(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                gui.new_setup_screen(painter, Some(crate::gui_port_pit::setup_pit_menu))
            }
            let setup = if gui.setup_car.is_some() { setup_editor } else { setup };
            fn time_table(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
                gui.new_time_table(painter)
            }
            let table = if crate::gui_seq_setup::time_seed() != 0.0 { time_table } else { table };
            // (the game's side hears the widgets only once they are made)
            // (nor the click on "Race control" an online menu makes in its constructor)
            let heard = gui.widget_events.len();
            let fired = gui.fired.len();
            let id = gui.new_esc_menu(painter, fades.as_ref(), EscPages { setup_screen: Some(setup), comparative_telemetry: Some(telemetry), time_table: Some(table) });
            gui.widget_events.truncate(heard);
            gui.fired.truncate(fired);
            gui.add_root(painter, id);
            gui.sim.esc_menu = Some(id);
            hud.more.menus.pits = Some(id);
        }
        "menu.escShow" => {
            sync(hud, gui);
            gui.sim_activate_esc_menu();
        }
        "menu.driveEnabled" => {
            let id = hud.more.menus.pits.ok_or("no ESC menu")?;
            gui.esc_menu_set_drive_button_enabled(id, number != 0.0);
        }
        "menu.chat" => {
            let o = crate::gui_seq_menu::script().online;
            // (the Chat app's sequences make no HUD: the apps' drivers)
            let driver = o.net_cars.iter().find(|(sid, _)| *sid == number as i32 as u8).map(|(_, car)| hud.input.cars.get(*car).map_or_else(|| crate::gui_seq_apps::driver_name(*car), |c| c.driver_name.clone()));
            // (the Chat app's handler was hung up before the page's)
            let guid = o.net_cars.iter().find(|(sid, _)| *sid == number as i32 as u8).map(|(_, car)| *car as i32);
            gui.chat_apps_on_chat_message(number as i32 as u32, text, driver.as_deref().zip(guid));
            gui.chats_on_chat_message(number as i32 as u32, text, driver.as_deref());
        }
        "menu.client" => {}
        "menu.raceControl" => {
            let id = hud.more.menus.pits.ok_or("no ESC menu")?;
            gui.esc_menu_set_race_control_screen(id);
        }
        other => return Err(format!("the port side knows no menu call {other:?}")),
    }
    drain(hud, gui);
    Ok(())
}

pub fn class_name(gui: &Gui, id: Id) -> &'static str {
    match &gui.controls[id].kind {
        rustyac_gui::control::Kind::Menu(menu) => match &**menu {
            rustyac_gui::menus::Menu::Pause(_) => "PauseMenu",
            rustyac_gui::menus::Menu::SetupScreen(_) => "SetupScreen",
            rustyac_gui::menus::Menu::SetupInfo(_) => "SetupInformation",
            rustyac_gui::menus::Menu::TimeTable(_) => "EscMenuTimeTable",
            rustyac_gui::menus::Menu::SetupTab(tab) => match tab.kind {
                rustyac_gui::setup_tab::TabKind::Generic => "SetupTab",
                rustyac_gui::setup_tab::TabKind::Io(_) => "SetupIOManager",
                rustyac_gui::setup_tab::TabKind::Gears(_) => "SetupGearManager",
                rustyac_gui::setup_tab::TabKind::Tyres(_) => "SetupTyresManager",
                rustyac_gui::setup_tab::TabKind::Fuel(_) => "SetupFuelManager",
                rustyac_gui::setup_tab::TabKind::Electronics(_) => "SetupElectronics",
                rustyac_gui::setup_tab::TabKind::Presets(_) => "SetupPitstopPresets",
            },
            rustyac_gui::menus::Menu::Leaderboard(_) => "Leaderboard",
            rustyac_gui::menus::Menu::EndCommon(_) => "EndCommon",
            rustyac_gui::menus::Menu::Esc(_) => "ESCMenu",
            rustyac_gui::menus::Menu::Replay(_) => "ReplayInterface",
            rustyac_gui::menus::Menu::RaceControl(_) => "RaceControlScreen",
            rustyac_gui::menus::Menu::SpSession(_) => "RaceControlSPSession",
            rustyac_gui::menus::Menu::SessionInfo(_) => "RaceControlSessionInfo",
            rustyac_gui::menus::Menu::SessionServer(_) => "RaceControlSessionServer",
            rustyac_gui::menus::Menu::Voting(_) => "RaceControlVoting",
            rustyac_gui::menus::Menu::AcChat(_) => "ACChat",
        },
        _ => "?",
    }
}

pub fn extra(gui: &Gui, id: Id) -> String {
    match class_name(gui, id) {
        "PauseMenu" => {
            let p = gui.pause_menu(id);
            format!(" pause={} fade={:08x}", p.currently_selected_button, p.fade_seconds.to_bits())
        }
        "ReplayInterface" => {
            let r = gui.replay_interface(id);
            let index = r.last_selected.and_then(|last| gui.controls[id].controls.iter().position(|c| *c == last)).map_or(-1, |i| i as i32);
            crate::gui_seq_menu::replay_extra(r.force_camera, r.target_y, r.stay_up_counter, index, r.camera_names.len())
        }
        "ESCMenu" => {
            let m = gui.esc_menu(id);
            let index = m.last_selected.and_then(|last| gui.controls[id].controls.iter().position(|c| *c == last)).map_or(-1, |i| i as i32);
            crate::gui_seq_menu::esc_extra(index, m.drive_menu_pressed, m.fade_time)
        }
        "Leaderboard" => {
            let l = gui.leaderboard(id);
            let printed: Vec<String> = l.printed_cars.iter().map(|car| car.to_string()).collect();
            crate::gui_seq_menu::board_extra(l.session_id, l.current_page, l.selected_car_index, l.mode, l.last_mode, l.show_mode_buttons, l.is_loaded, &printed.join(","), l.avatar_to_be_kicked.map_or(-1, |car| car as i32))
        }
        _ => String::new(),
    }
}

/// `EndSessionDisplayer::update`, in the update pass.
pub fn update(hud: &mut HudPort, gui: &mut Gui) {
    if let Some(end) = &mut hud.more.menus.end {
        end.update(gui);
    }
    drain(hud, gui);
}

/// `EndSessionDisplayer::renderHUD` (after the damage picture).
pub fn render_end(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    if let Some(end) = &mut hud.more.menus.end {
        end.render_hud(gui, painter, dt);
    }
    if let Some(overlay) = &mut hud.more.menus.overlay {
        let hidden = hud.input.end_session_active || hud.input.esc_menu_visible || hud.input.pause_menu_visible;
        let position = hud.input.cars.first().map_or(-1, |car| car.leaderboard_position);
        overlay.render_hud(gui, painter, dt, hidden, position);
    }
    if let Some(tab) = &hud.more.menus.tab {
        tab.render_hud(gui, painter, dt);
    }
    if let Some((client, qos)) = &hud.more.menus.client {
        let s = script();
        client.render_hud(gui, painter, &s.welcome, Some(s.disconnect.as_str()).filter(|message| !message.is_empty()), s.esc.game_time);
        qos.render_hud(gui, painter, s.own_ping);
    }
}

/// The window's mouse events as the game objects that hang on them hear them (after the GUI):
/// 0 down, 1 up, 2 move, 3 the wheel.
pub fn window_mouse(hud: &mut HudPort, gui: &mut Gui, kind: u32, e: &MouseEvent) {
    crate::gui_port_pit::window_mouse(hud, gui, kind, e);
    if let Some(end) = &hud.more.menus.end {
        match kind {
            0 => end.on_mouse_down(gui, e),
            1 => end.on_mouse_up(gui, e),
            2 => end.on_mouse_move(gui, e),
            _ => end.on_mouse_wheel(gui, e),
        }
    }
    if let Some(tab) = &hud.more.menus.tab {
        match kind {
            0 => tab.on_mouse_down(gui, e),
            1 => tab.on_mouse_up(gui, e),
            2 => tab.on_mouse_move(gui, e),
            _ => {}
        }
    }
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    if hud.more.menus.replay.is_some() {
        out.push_str(&crate::gui_seq_menu::replay_line(&replay_model(gui), hud.more.menus.drivable_camera));
    }
    if let Some(end) = &hud.more.menus.end {
        out.push_str(&crate::gui_seq_menu::end_line(end.is_active, end.is_loaded, end.old_mode, gui.is_active));
        crate::gui_port::dump_control(gui, end.parent_control, 1, out);
    }
    if let Some(overlay) = &hud.more.menus.overlay {
        let names: Vec<String> = overlay.driver_names.iter().map(|(name, short)| format!("{name}={short}")).collect();
        out.push_str(&crate::gui_seq_menu::overlay_line(overlay.gap_display_mode, overlay.vertical_layout, overlay.timer, &names));
    }
    if let Some(tab) = &hud.more.menus.tab {
        out.push_str(&format!("tab active={}\n", tab.is_active as u8));
        crate::gui_port::dump_control(gui, tab.parent_control, 1, out);
    }
}
