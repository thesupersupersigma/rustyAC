// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's menus: acs.exe's own `PauseMenu` on the stand-in `Sim` of
//! the HUD scripts.
//!
//! The menu's buttons call into the session. `Sim::activateGameGui`, `activateEscMenu` and
//! `activatePauseMenu` are the game's own (they only show and hide controls); what would need
//! a whole game behind it is replaced by a stand-in that writes the call down: `Sim::
//! setPauseMode`, `RaceManager::skipCurrentSession` / `restartCurrentSession`, `AudioEngine::
//! start` / `stop`, `ACCameraManager::registerToFadeInEvent` / `fadeIn` (the job is kept and
//! run when the script says the fade is over), `CarAvatar::lockControlsUntilTime` /
//! `goToSpawnPosition`, `ReplayManager::startReplayMode`, `Game::shutdown`.

use std::sync::Mutex;

use crate::ac::{rd, wr, wstring, Game};
use crate::gui_ac::{children, read_wstring};
use crate::gui_ac_hud::{acs, hud, input, jump, HudGame};
use crate::gui_seq_menu::{action, script};

const VA_PAUSE_MENU_CTOR: usize = 0x1_4011_d9b0;
const VA_PAUSE_MENU_BLANK_BUTTON: usize = 0x1_4011_f400;
pub const VA_PAUSE_MENU_VTABLE: usize = 0x1_404c_7300;
const VA_GET_GAME_OBJECT_BY_NAME: usize = 0x1_4011_f770;
const VA_RACE_MANAGER_GET_SESSION_COUNT: usize = 0x1_4013_9ca0;
const VA_RACE_MANAGER_SKIP_CURRENT_SESSION: usize = 0x1_4014_1350;
const VA_RACE_MANAGER_RESTART_CURRENT_SESSION: usize = 0x1_4014_0d00;
const VA_SIM_SET_PAUSE_MODE: usize = 0x1_4019_e6d0;
const VA_SIM_ACTIVATE_GAME_GUI: usize = 0x1_4019_77e0;
const VA_SIM_ACTIVATE_PAUSE_MENU: usize = 0x1_4019_7840;
const VA_AUDIO_ENGINE_STOP: usize = 0x1_401f_bfc0;
const VA_AUDIO_ENGINE_START: usize = 0x1_401f_bec0;
const VA_CAMERA_REGISTER_TO_FADE_IN: usize = 0x1_4003_3ce0;
const VA_CAMERA_FADE_IN: usize = 0x1_4003_3690;
const VA_CAR_LOCK_CONTROLS_UNTIL_TIME: usize = 0x1_400d_8e20;
const VA_CAR_GO_TO_SPAWN_POSITION: usize = 0x1_400d_3ae0;
const VA_REPLAY_START_REPLAY_MODE: usize = 0x1_4015_b480;
const VA_CONTROL_SET_VISIBLE: usize = 0x1_4024_51a0;
const VA_GUI_ADD_CONTROL: usize = 0x1_4025_2e50;
const VA_GUI_ON_MOUSE_DOWN: usize = 0x1_4025_4290;
const VA_GUI_ON_MOUSE_MOVE: usize = 0x1_4025_4320;
const VA_GUI_ON_MOUSE_UP: usize = 0x1_4025_45c0;

/// class, size, constructor, table (see `gui_ac::CLASSES`)
pub const CLASSES: [(&str, usize, usize, usize); 3] = [("PauseMenu", 0x200, VA_PAUSE_MENU_CTOR, VA_PAUSE_MENU_VTABLE), ("Leaderboard", 0x2e8, VA_LEADERBOARD_CTOR, VA_LEADERBOARD_VTABLE), ("EndCommon", 0x1c0, 0x1_4009_1ed0, 0x1_404b_4998)];
const VA_END_SESSION_CTOR: usize = 0x1_4010_a650;
const VA_END_SESSION_UPDATE: usize = 0x1_4010_bf90;
const VA_END_SESSION_DISPLAY_RESULTS: usize = 0x1_4010_b260;
const VA_END_SESSION_HIDE: usize = 0x1_4010_b380;
const VA_END_SESSION_RENDER_HUD: usize = 0x1_4010_b520;
const VA_CAMERA_SET_MODE: usize = 0x1_4003_40d0;
const VA_CAMERA_SET_INTRO_MODE: usize = 0x1_4011_5c80;
const VA_CAR_SET_CONTROLS_LOCK: usize = 0x1_400d_9f00;
const VA_LEADERBOARD_CTOR: usize = 0x1_402d_10f0;
const VA_LEADERBOARD_VTABLE: usize = 0x1_4050_1288;
const VA_LEADERBOARD_SET_LAST_SESSION: usize = 0x1_402d_6990;
const VA_CONTROL_SET_POSITION: usize = 0x1_4024_50a0;

struct Menus {
    pause: usize,
    /// `ACCameraManager::evOnFadeInFinished`: owner, the `std::function`'s implementation
    fade: Vec<(usize, usize)>,
    fade_sender: usize,
    /// every `ACStandardLeaderboard` the script made
    boards: Vec<usize>,
    /// the `EndSessionDisplayer`
    end: usize,
}

static MENUS: Mutex<Menus> = Mutex::new(Menus { pause: 0, fade: Vec::new(), fade_sender: 0, boards: Vec::new(), end: 0 });

unsafe extern "C" fn set_mode(_this: *mut u8, mode: i32, _fade: bool, _random: bool) {
    action(format!("setMode {mode}"));
}

unsafe extern "C" fn set_intro_mode(_this: *mut u8, on: bool) {
    action(format!("setIntroMode {}", on as u8));
}

unsafe extern "C" fn set_controls_lock(_this: *mut u8, on: bool) {
    action(format!("setControlsLock {}", on as u8));
}

/// What the menus call of the session, replaced by stand-ins that write the call down, and a
/// table for `Game` whose only function is `shutdown`.
unsafe fn install(game: &Game, sim: *mut u8) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let a = &game.acs;
    for (va, to) in [
        (VA_RACE_MANAGER_GET_SESSION_COUNT, get_session_count as *const () as usize),
        (VA_RACE_MANAGER_SKIP_CURRENT_SESSION, skip_current_session as *const () as usize),
        (VA_RACE_MANAGER_RESTART_CURRENT_SESSION, restart_current_session as *const () as usize),
        (VA_SIM_SET_PAUSE_MODE, set_pause_mode as *const () as usize),
        (VA_AUDIO_ENGINE_STOP, audio_stop as *const () as usize),
        (VA_AUDIO_ENGINE_START, audio_start as *const () as usize),
        (VA_CAMERA_REGISTER_TO_FADE_IN, register_to_fade_in_event as *const () as usize),
        (VA_CAMERA_FADE_IN, fade_in as *const () as usize),
        (VA_CAR_LOCK_CONTROLS_UNTIL_TIME, lock_controls_until_time as *const () as usize),
        (VA_CAR_GO_TO_SPAWN_POSITION, go_to_spawn_position as *const () as usize),
        (VA_REPLAY_START_REPLAY_MODE, start_replay_mode as *const () as usize),
        (VA_GET_GAME_OBJECT_BY_NAME, get_game_object as *const () as usize),
        (VA_CAMERA_SET_MODE, set_mode as *const () as usize),
        (VA_CAMERA_SET_INTRO_MODE, set_intro_mode as *const () as usize),
        (VA_CAR_SET_CONTROLS_LOCK, set_controls_lock as *const () as usize),
    ] {
        jump(game, va, to);
    }
    let table = a.alloc(16 * 8);
    for slot in 0..16 {
        wr(table, slot * 8, no_such_slot as *const () as usize);
    }
    wr(table, 8, shutdown as *const () as usize);
    let game_block: *mut u8 = rd(sim, 0x8);
    wr(game_block, 0, table);
}

/// The window's mouse events (`RenderWindow::evOnMouseDown` … at `Game` + 0x58, 0x88, 0xa0,
/// 0x70) as whoever hung a handler on them hears them.
pub unsafe fn window_mouse(gui_handler: usize, event: *mut u8) {
    let Some(h) = hud() else { return };
    let at = match gui_handler {
        VA_GUI_ON_MOUSE_DOWN => 0x58,
        VA_GUI_ON_MOUSE_UP => 0x88,
        VA_GUI_ON_MOUSE_MOVE => 0xa0,
        _ => 0x70,
    };
    let game_block: *const u8 = rd(h.sim, 0x8);
    let (begin, end): (*const u8, *const u8) = (rd(game_block, at), rd(game_block, at + 8));
    if begin.is_null() {
        return;
    }
    for i in 0..end.offset_from(begin) as usize / 0x28 {
        let implementation: *mut u8 = rd(begin.add(i * 0x28), 0x20);
        let table: *const usize = rd(implementation, 0);
        let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*table.add(2));
        call(implementation, event);
    }
}

/// `EndSessionDisplayer::update`, in the update pass (only while the object is active).
pub unsafe fn update(game: &Game, _h: &HudGame, dt: f32) {
    let end = MENUS.lock().unwrap().end as *mut u8;
    if !end.is_null() && rd::<u8>(end, 0x30) != 0 {
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_END_SESSION_UPDATE));
        update(end, dt);
    }
}

/// `EndSessionDisplayer::renderHUD` (after the damage picture).
pub unsafe fn render_end(game: &Game, h: &HudGame, dt: f32) {
    let end = MENUS.lock().unwrap().end as *mut u8;
    if !end.is_null() && rd::<u8>(end, 0x30) != 0 {
        client_present(true);
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_END_SESSION_RENDER_HUD));
        render(end, dt);
        client_present(false);
    }
    let _ = h;
}

pub unsafe fn dump(game: &Game, out: &mut String) {
    let end = MENUS.lock().unwrap().end as *const u8;
    if !end.is_null() {
        let gui: *const u8 = rd(rd::<*const u8>(end, 0x8), 0x180);
        out.push_str(&crate::gui_seq_menu::end_line(rd::<u8>(end, 0x30) != 0, rd::<u8>(end, 0x128) != 0, rd(end, 0x100), rd::<u8>(gui, 0x8) != 0));
        crate::gui_ac::dump_control(game, rd(end, 0x70), 1, out);
    }
}

/// `Sim::client` is there or not, around the calls of the game that ask (the tables of an
/// online session read it while they are drawn; the HUD's proofs of Task 29 ran without it).
pub unsafe fn client_present(present: bool) {
    let Some(h) = hud() else { return };
    if MENUS.lock().unwrap().boards.is_empty() {
        return;
    }
    wr(h.sim, 0x1c8, if present && input(|i| i.online) { h.more.cars.client as usize } else { 0 });
}

unsafe extern "C" fn get_session_count(_this: *mut u8) -> u32 {
    script().session_count
}

unsafe extern "C" fn skip_current_session(_this: *mut u8) -> bool {
    action("skipCurrentSession".into());
    true
}

unsafe extern "C" fn restart_current_session(_this: *mut u8) {
    action("restartCurrentSession".into());
}

unsafe extern "C" fn set_pause_mode(_this: *mut u8, paused: bool) {
    action(format!("setPauseMode {}", paused as u8));
}

unsafe extern "C" fn audio_stop(_this: *mut u8) {
    action("audio stop".into());
}

unsafe extern "C" fn audio_start(_this: *mut u8) {
    action("audio start".into());
}

unsafe fn run_job(implementation: usize, interrupted: bool) {
    let table: *const usize = rd(implementation as *const u8, 0);
    let call: extern "C" fn(*mut u8, *const bool) = std::mem::transmute(*table.add(2));
    call(implementation as *mut u8, &interrupted);
}

/// In place of `ACCameraManager::registerToFadeInEvent` 0x140033ce0, with what it does: jobs
/// that still wait are run, those of the fade's sender taken off, the new one kept (a copy the
/// `std::function` makes of itself).
unsafe extern "C" fn register_to_fade_in_event(_this: *mut u8, function: *mut u8, owner: *mut u8) {
    let waiting = MENUS.lock().unwrap().fade.clone();
    if !waiting.is_empty() {
        for (_, implementation) in &waiting {
            run_job(*implementation, true);
        }
        let mut m = MENUS.lock().unwrap();
        let sender = m.fade_sender;
        m.fade.retain(|(owner, _)| *owner != sender);
    }
    let implementation: *mut u8 = rd(function, 0x18);
    let table: *const usize = rd(implementation, 0);
    let copy: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(*table);
    // (a small function object lives inside the `std::function`: its copy is told where to go)
    let kept = copy(implementation, if implementation == function { acs().alloc(0x40) } else { std::ptr::null_mut() });
    MENUS.lock().unwrap().fade.push((owner as usize, kept as usize));
}

/// In place of `ACCameraManager::fadeIn` 0x140033690.
unsafe extern "C" fn fade_in(_this: *mut u8, mode: *const u8) {
    MENUS.lock().unwrap().fade_sender = rd(mode, 0x10);
    action(format!("fadeIn {:08x}", rd::<u32>(mode, 8)));
}

unsafe extern "C" fn lock_controls_until_time(_this: *mut u8, ms: f64, _now: f64, _force: bool) {
    action(format!("lockControlsUntilTime {:016x}", ms.to_bits()));
}

unsafe extern "C" fn go_to_spawn_position(_this: *mut u8, name: *const u8) {
    action(format!("goToSpawnPosition {}", read_wstring(name)));
}

unsafe extern "C" fn start_replay_mode(_this: *mut u8, frame: u32) {
    action(format!("startReplayMode {frame}"));
}

unsafe extern "C" fn shutdown(_this: *mut u8) {
    action("shutdown".into());
}

unsafe extern "C" fn no_such_slot() {
    eprintln!("oracle: the game called a function of Game the oracle has no stand-in for");
    std::process::abort();
}

/// In place of `GameObject::getGameObject(name)` 0x14011f770 (the pause menu asks for the
/// replay manager by name).
unsafe extern "C" fn get_game_object(this: *mut u8, name: *const u8) -> *mut u8 {
    if read_wstring(name) == "REPLAY_MANAGER" && script().replay_manager {
        return rd(this, 0x1b0);
    }
    std::ptr::null_mut()
}

unsafe fn button(menu: *mut u8, name: &str) -> Option<[f32; 4]> {
    let parent: [f32; 4] = rd(menu, 0x28);
    children(menu).into_iter().find(|c| read_wstring(c.add(8)) == name).map(|c| {
        let r: [f32; 4] = rd(c, 0x28);
        [parent[0] + r[0], parent[0] + r[1], parent[2] + r[2], parent[2] + r[3]]
    })
}

unsafe fn mouse(game: &Game, gui: *mut u8, va: usize, x: i32, y: i32, button: i32) {
    let event = game.acs.alloc(16);
    wr(event, 0, x);
    wr(event, 4, y);
    wr(event, 8, button);
    wr(event, 12, 0.0f32);
    let handler: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(game.acs.va(va));
    handler(gui, event);
}

pub unsafe fn call(game: &Game, gui: *mut u8, what: &str, number: f32, text: &str) -> Result<(), String> {
    if crate::gui_seq_menu::menu_input(what, number, text) {
        return Ok(());
    }
    let a = &game.acs;
    let h = hud().ok_or("the HUD was not made (Hud(\"create\", cars, \"\") first)")?;
    let sim = h.sim;
    let menu = MENUS.lock().unwrap().pause as *mut u8;
    match what {
        "menu.pause" => {
            install(game, sim);
            // Sim::Sim 0x1401931b5: the menu, hidden, a top-level control of the GUI
            let client: usize = rd(sim, 0x1c8);
            if input(|i| i.online) {
                wr(sim, 0x1c8, h.more.cars.client);
            }
            let menu = a.alloc(0x200);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_PAUSE_MENU_CTOR));
            ctor(menu, sim, gui);
            wr(sim, 0x1c8, client);
            wr(sim, 0x110, menu);
            let set_visible: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(VA_CONTROL_SET_VISIBLE));
            set_visible(menu, false);
            let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(a.va(VA_GUI_ADD_CONTROL));
            add(gui, menu);
            MENUS.lock().unwrap().pause = menu as usize;
        }
        // Sim::onKeyDown's ESC: the pause menu, and the pause
        "menu.esc" => {
            let activate: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_SIM_ACTIVATE_PAUSE_MENU));
            activate(sim);
            set_pause_mode(sim, true);
        }
        "menu.resume" => {
            let activate: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_SIM_ACTIVATE_GAME_GUI));
            activate(sim);
        }
        "menu.blank" => {
            let blank: extern "C" fn(*mut u8, i32, bool) = std::mem::transmute(a.va(VA_PAUSE_MENU_BLANK_BUTTON));
            // (the game's function takes a missing button for granted, but for "SKIP")
            let names = ["RESUME", "PITS", "RESTART", "REPLAY", "EXIT", "SKIP"];
            if names.get(number as usize).is_some_and(|name| button(menu, name).is_some()) {
                blank(menu, number as i32, text == "1");
            }
        }
        "menu.hover" | "menu.click" => {
            if let Some(r) = button(menu, text) {
                let (x, y) = (((r[0] + r[1]) * 0.5) as i32, ((r[2] + r[3]) * 0.5) as i32);
                mouse(game, gui, VA_GUI_ON_MOUSE_MOVE, x, y, -1);
                if what == "menu.click" {
                    mouse(game, gui, VA_GUI_ON_MOUSE_DOWN, x, y, 0);
                    mouse(game, gui, VA_GUI_ON_MOUSE_UP, x, y, 0);
                }
            }
        }
        "menu.lb" => {
            crate::gui_ac_race::install(game);
            crate::gui_ac_race::sync(h);
            install(game, sim);
            let client: usize = rd(sim, 0x1c8);
            if input(|i| i.online) {
                wr(sim, 0x1c8, h.more.cars.client);
            }
            let board = a.alloc(0x2e8);
            let ctor: extern "C" fn(*mut u8, *mut u8, i32, bool) -> *mut u8 = std::mem::transmute(a.va(VA_LEADERBOARD_CTOR));
            ctor(board, sim, number as i32, text == "focus");
            wr(sim, 0x1c8, client);
            let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(a.va(VA_GUI_ADD_CONTROL));
            add(gui, board);
            MENUS.lock().unwrap().boards.push(board as usize);
        }
        "menu.lbPos" | "menu.lbSize" | "menu.lbSet" | "menu.lbLast" | "menu.lbVisible" => {
            let board = *MENUS.lock().unwrap().boards.last().ok_or("no leaderboard")? as *mut u8;
            let pair = || -> (f32, f32) {
                let (x, y) = text.split_once(',').unwrap_or(("0", "0"));
                (x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0))
            };
            match what {
                "menu.lbPos" => {
                    let set: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(a.va(VA_CONTROL_SET_POSITION));
                    set(board, pair().0, pair().1);
                }
                "menu.lbSize" => {
                    let table: *const usize = rd(board, 0);
                    let set: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(*table.add(0x38 / 8));
                    set(board, pair().0, pair().1);
                }
                "menu.lbSet" => match text {
                    "ui" => wr(board, 0x280, number as i32),
                    "show" => wr(board, 0x18c, (number != 0.0) as u8),
                    "loaded" => wr(board, 0x284, (number != 0.0) as u8),
                    other => return Err(format!("no leaderboard member {other:?}")),
                },
                "menu.lbLast" => {
                    let set: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_LEADERBOARD_SET_LAST_SESSION));
                    set(board);
                }
                _ => {
                    let set: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(VA_CONTROL_SET_VISIBLE));
                    set(board, number != 0.0);
                }
            }
        }
        // Sim::evOnNewSession, as the tables hear it
        "menu.newSession" => {
            let (begin, end): (*const u8, *const u8) = (rd(sim, 0xc0), rd(sim, 0xc8));
            if !begin.is_null() {
                let event = a.alloc(0x40);
                let boards = MENUS.lock().unwrap().boards.clone();
                for i in 0..end.offset_from(begin) as usize / 0x28 {
                    // (only the tables' handlers: an entry's first word is who hung it up)
                    if !boards.contains(&rd::<usize>(begin.add(i * 0x28), 0)) {
                        continue;
                    }
                    let implementation: *mut u8 = rd(begin.add(i * 0x28), 0x20);
                    let table: *const usize = rd(implementation, 0);
                    let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*table.add(2));
                    call(implementation, event);
                }
            }
        }
        "menu.end" => {
            crate::gui_ac_race::install(game);
            crate::gui_ac_race::sync(h);
            install(game, sim);
            let end = a.alloc(0x130);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_END_SESSION_CTOR));
            let client: usize = rd(sim, 0x1c8);
            if input(|i| i.online) {
                wr(sim, 0x1c8, h.more.cars.client);
            }
            ctor(end, sim);
            wr(sim, 0x1c8, client);
            wr(sim, 0x260, end);
            let mut m = MENUS.lock().unwrap();
            m.end = end as usize;
            m.boards.push(rd::<usize>(end, 0x78));
        }
        "menu.endShow" | "menu.endHide" => {
            let end = MENUS.lock().unwrap().end as *mut u8;
            wr(sim, 0x188, h.more.camera);
            let call: extern "C" fn(*mut u8) = std::mem::transmute(a.va(if what == "menu.endShow" { VA_END_SESSION_DISPLAY_RESULTS } else { VA_END_SESSION_HIDE }));
            call(end);
            wr(sim, 0x188, 0usize);
        }
        "menu.fadeDone" => {
            let waiting = std::mem::take(&mut MENUS.lock().unwrap().fade);
            for (_, implementation) in waiting {
                run_job(implementation, false);
            }
        }
        other => return Err(format!("the game side knows no menu call {other:?}")),
    }
    let _ = wstring;
    Ok(())
}

pub unsafe fn extra(control: *const u8, class: &str) -> Option<String> {
    match class {
        "PauseMenu" => Some(format!(" pause={} fade={:08x}", rd::<i32>(control, 0x1f0), rd::<u32>(control, 0x1f4))),
        "EndCommon" => Some(String::new()),
        "Leaderboard" => {
            let (begin, end): (*const *const u8, *const *const u8) = (rd(control, 0x190), rd(control, 0x198));
            let printed: Vec<String> = if begin.is_null() { Vec::new() } else { (0..end.offset_from(begin) as usize).map(|i| rd::<i32>(*begin.add(i), 0x1158).to_string()).collect() };
            let picked: *const u8 = rd(control, 0x240);
            let picked = if picked.is_null() { -1 } else { rd::<i32>(picked, 0x1158) };
            Some(crate::gui_seq_menu::board_extra(rd(control, 0x178), rd(control, 0x184), rd(control, 0x188), rd(control, 0x200), rd(control, 0x204), rd::<u8>(control, 0x18c) != 0, rd::<u8>(control, 0x284) != 0, &printed.join(","), picked))
        }
        _ => None,
    }
}

/// The controls no GUI holds: the panel of the end-of-session displayer.
pub unsafe fn extra_roots() -> Vec<*mut u8> {
    let end = MENUS.lock().unwrap().end as *const u8;
    if end.is_null() {
        return Vec::new();
    }
    vec![rd(end, 0x70)]
}
