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
    gui.sim.time_to_session_start = hud.input.time_to_session_start;
    gui.sim.has_replay_manager = s.replay_manager;
    if let Some(esc) = hud.more.menus.esc {
        gui.controls[esc].visible = hud.input.esc_menu_visible;
    }
    if let Some(pause) = hud.more.menus.pause {
        hud.input.pause_menu_visible = gui.controls[pause].visible;
    } else if let Some(stand_in) = hud.more.menus.pause_stand_in {
        gui.controls[stand_in].visible = hud.input.pause_menu_visible;
    }
}

/// The calls the menus made since the last time, written down; the one on the message banner
/// is carried out.
pub fn drain(hud: &mut HudPort, gui: &mut Gui) {
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
            SimAction::SetFocusedCarIndex(car) => action(format!("setFocusedCarIndex {car}")),
            SimAction::SetCameraMode(mode) => action(format!("setMode {mode}")),
            SimAction::SetIntroMode(on) => action(format!("setIntroMode {}", on as u8)),
            SimAction::SetControlsLock(on) => action(format!("setControlsLock {}", on as u8)),
        }
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
    if hud.message.is_none() {
        return Err("the HUD was not made".into());
    }
    sync(hud, gui);
    match what {
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
        other => return Err(format!("the port side knows no menu call {other:?}")),
    }
    drain(hud, gui);
    Ok(())
}

pub fn class_name(gui: &Gui, id: Id) -> &'static str {
    match &gui.controls[id].kind {
        rustyac_gui::control::Kind::Menu(menu) => match &**menu {
            rustyac_gui::menus::Menu::Pause(_) => "PauseMenu",
            rustyac_gui::menus::Menu::Leaderboard(_) => "Leaderboard",
            rustyac_gui::menus::Menu::EndCommon(_) => "EndCommon",
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
    if let Some(tab) = &hud.more.menus.tab {
        tab.render_hud(gui, painter, dt);
    }
}

/// The window's mouse events as the game objects that hang on them hear them (after the GUI):
/// 0 down, 1 up, 2 move, 3 the wheel.
pub fn window_mouse(hud: &mut HudPort, gui: &mut Gui, kind: u32, e: &MouseEvent) {
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
    if let Some(end) = &hud.more.menus.end {
        out.push_str(&crate::gui_seq_menu::end_line(end.is_active, end.is_loaded, end.old_mode, gui.is_active));
        crate::gui_port::dump_control(gui, end.parent_control, 1, out);
    }
    if let Some(tab) = &hud.more.menus.tab {
        out.push_str(&format!("tab active={}\n", tab.is_active as u8));
        crate::gui_port::dump_control(gui, tab.parent_control, 1, out);
    }
}
