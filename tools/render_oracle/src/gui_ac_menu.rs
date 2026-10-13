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
use crate::gui_ac_hud::{acs, hud, input, jump};
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
pub const CLASSES: [(&str, usize, usize, usize); 1] = [("PauseMenu", 0x200, VA_PAUSE_MENU_CTOR, VA_PAUSE_MENU_VTABLE)];

struct Menus {
    pause: usize,
    /// `ACCameraManager::evOnFadeInFinished`: owner, the `std::function`'s implementation
    fade: Vec<(usize, usize)>,
    fade_sender: usize,
}

static MENUS: Mutex<Menus> = Mutex::new(Menus { pause: 0, fade: Vec::new(), fade_sender: 0 });

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
            jump(game, VA_RACE_MANAGER_GET_SESSION_COUNT, get_session_count as *const () as usize);
            jump(game, VA_RACE_MANAGER_SKIP_CURRENT_SESSION, skip_current_session as *const () as usize);
            jump(game, VA_RACE_MANAGER_RESTART_CURRENT_SESSION, restart_current_session as *const () as usize);
            jump(game, VA_SIM_SET_PAUSE_MODE, set_pause_mode as *const () as usize);
            jump(game, VA_AUDIO_ENGINE_STOP, audio_stop as *const () as usize);
            jump(game, VA_AUDIO_ENGINE_START, audio_start as *const () as usize);
            jump(game, VA_CAMERA_REGISTER_TO_FADE_IN, register_to_fade_in_event as *const () as usize);
            jump(game, VA_CAMERA_FADE_IN, fade_in as *const () as usize);
            jump(game, VA_CAR_LOCK_CONTROLS_UNTIL_TIME, lock_controls_until_time as *const () as usize);
            jump(game, VA_CAR_GO_TO_SPAWN_POSITION, go_to_spawn_position as *const () as usize);
            jump(game, VA_REPLAY_START_REPLAY_MODE, start_replay_mode as *const () as usize);
            jump(game, VA_GET_GAME_OBJECT_BY_NAME, get_game_object as *const () as usize);
            // Game's table: only `shutdown` (slot 1) is ever called
            let table = a.alloc(16 * 8);
            for slot in 0..16 {
                wr(table, slot * 8, no_such_slot as *const () as usize);
            }
            wr(table, 8, shutdown as *const () as usize);
            let game_block: *mut u8 = rd(sim, 0x8);
            wr(game_block, 0, table);
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
        _ => None,
    }
}
