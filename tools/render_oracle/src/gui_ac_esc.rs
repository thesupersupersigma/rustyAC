// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the ESC menu's proofs: acs.exe's own `ESCMenu`, with its page "Race
//! control" (`RaceControlScreen`, `RaceControlSessionInfo`, `RaceControlSPSession`, the
//! standard leaderboard) on the stand-in `Sim` of the HUD scripts.
//!
//! The three pages the port does not have are made as what they are underneath and nothing
//! more: `SetupScreen` and `EscMenuTimeTable` a bare `ksgui::Control`, `ComparativeTelemetry`
//! a bare `ksgui::Form` (their constructors are replaced; the menu itself makes the memory,
//! adds them, sizes them and shows and hides them as it does the real ones). What `Sim` hangs
//! on the menu's coming and going (the game screen and the app bar go and come) is the game's
//! own handler, hung up here as `Sim::Sim` does.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::ac::{rd, wr, wstring, Game};
use crate::gui_ac::children;
use crate::gui_ac_hud::{acs, input, jump, HudGame};
use crate::gui_seq_menu::{action, script};

pub const VA_ESC_MENU_CTOR: usize = 0x1_400f_cb40;
pub const VA_ESC_MENU_VTABLE: usize = 0x1_404c_3940;
const VA_ESC_MENU_SET_DRIVE_BUTTON_ENABLED: usize = 0x1_400f_f2a0;
const VA_ESC_MENU_SET_RACE_CONTROL_SCREEN: usize = 0x1_400f_f2b0;
const VA_ESC_MENU_TRIGGERED_HANDLER: usize = 0x1_4019_6850;
const VA_SIM_ACTIVATE_ESC_MENU: usize = 0x1_4019_7780;
const VA_SETUP_SCREEN_CTOR: usize = 0x1_4017_c010;
const VA_COMPARATIVE_TELEMETRY_CTOR: usize = 0x1_4006_dff0;
const VA_TIME_TABLE_CTOR: usize = 0x1_4009_4230;
const VA_CONTROL_CTOR: usize = 0x1_4024_4150;
const VA_FORM_CTOR: usize = 0x1_4024_5890;
const VA_CONTROL_SET_POSITION: usize = 0x1_4024_50a0;
const VA_GUI_ADD_CONTROL: usize = 0x1_4025_2e50;
const VA_GET_TIME_TO_WAIT: usize = 0x1_4013_a130;
const VA_GET_INVERTED_GRID_POSITIONS: usize = 0x1_4013_8550;
const VA_GET_PIT_WINDOW_START: usize = 0x1_4013_8d60;
const VA_GET_PIT_WINDOW_END: usize = 0x1_4013_8d40;
const VA_GET_LAP_COUNT: usize = 0x1_4013_8570;
const VA_START_RACE: usize = 0x1_4014_1470;
const VA_CAR_IS_IN_PIT: usize = 0x1_400d_8bd0;
const VA_LOAD_PREVIOUS_SESSION_USER_CAMERA: usize = 0x1_4003_37e0;

static MENU: AtomicUsize = AtomicUsize::new(0);

pub fn menu() -> *mut u8 {
    MENU.load(Ordering::SeqCst) as *mut u8
}

unsafe fn gui_of(sim: *mut u8) -> *mut u8 {
    rd(rd::<*mut u8>(sim, 0x8), 0x180)
}

unsafe fn bare_control(this: *mut u8, sim: *mut u8, name: &str) {
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs().va(VA_CONTROL_CTOR));
    ctor(this, wstring(acs(), name), gui_of(sim));
}

/// In place of `SetupScreen::SetupScreen` 0x14017c010: the control it is, as its constructor
/// leaves it (no colour behind it, at (100, 0), hidden, no frame).
unsafe extern "C" fn setup_screen_ctor(this: *mut u8, sim: *mut u8) -> *mut u8 {
    bare_control(this, sim, "SETUP_SCREEN");
    wr(this, 0x38, [0.0f32; 4]);
    let set: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(acs().va(VA_CONTROL_SET_POSITION));
    set(this, 100.0, 0.0);
    wr(this, 0x150, 0u8);
    wr(this, 0x110, 0u16);
    this
}

/// In place of `ComparativeTelemetry::ComparativeTelemetry` 0x14006dff0: the form it is.
unsafe extern "C" fn comparative_telemetry_ctor(this: *mut u8, sim: *mut u8) -> *mut u8 {
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(acs().va(VA_FORM_CTOR));
    ctor(this, wstring(acs(), "COMPARATIVE_TELEMETRY"), gui_of(sim), false);
    this
}

/// In place of `EscMenuTimeTable::EscMenuTimeTable` 0x140094230: the control it is.
unsafe extern "C" fn time_table_ctor(this: *mut u8, sim: *mut u8) -> *mut u8 {
    bare_control(this, sim, "ESC_MENU_TIME_TABLE");
    this
}

unsafe extern "C" fn get_time_to_wait(_this: *mut u8) -> f64 {
    script().esc.time_to_wait
}

unsafe extern "C" fn get_inverted_grid_positions(_this: *mut u8) -> i32 {
    crate::gui_ac_race::race().inverted_grid_positions
}

unsafe extern "C" fn get_pit_window_start(_this: *mut u8) -> u16 {
    crate::gui_ac_race::race().pit_window_start
}

unsafe extern "C" fn get_pit_window_end(_this: *mut u8) -> u16 {
    crate::gui_ac_race::race().pit_window_end
}

unsafe extern "C" fn get_lap_count(_this: *mut u8, car: *mut u8) -> u32 {
    crate::gui_ac_race::race().cars.get(rd::<i32>(car, 0x1158) as usize).map_or(0, |c| c.lap_count)
}

unsafe extern "C" fn start_race(_this: *mut u8, wait: f32) {
    action(format!("startRace {:08x}", wait.to_bits()));
}

unsafe extern "C" fn is_in_pit(_this: *mut u8) -> bool {
    script().esc.in_pit
}

unsafe extern "C" fn load_previous_session_user_camera(_this: *mut u8) {
    action("loadPreviousSessionUserCamera".into());
}

/// The script's values where the menu's code reads them itself.
pub unsafe fn sync(game: &Game, h: &HudGame) {
    if menu().is_null() {
        return;
    }
    let e = script().esc;
    let sim = h.sim;
    wr(h.cars[0], 0xa40, e.setup_state);
    wr(rd::<*mut u8>(sim, 0x1a8), 0xa2, e.fixed_setup as u8);
    wr(rd::<*mut u8>(sim, 0x8), 0x18, e.game_time);
    let avatar: *mut u8 = rd(sim, 0x1b8);
    wr(rd::<*mut u8>(avatar, 0x1f0), 0x128, e.grip);
    wr(avatar, 0x310, e.wind_speed);
    wr(avatar, 0x314, e.wind_direction);
    wr(avatar, 0x408, e.ambient);
    wr(avatar, 0x40c, e.road);
    let _ = game;
}

static SUN: AtomicUsize = AtomicUsize::new(0);
static SYNTHETIC: AtomicBool = AtomicBool::new(false);

/// The sun's angle is the script's while the GUI is drawn (`on`), and the game's own again
/// after it (the scene's lighting reads it too).
pub unsafe fn sun(game: &Game, on: bool) {
    if menu().is_null() {
        return;
    }
    if on {
        SUN.store(rd::<u32>(game.graphics, 0xd4) as usize, Ordering::SeqCst);
        wr(game.graphics, 0xd4, script().esc.sun_angle);
    } else {
        wr(game.graphics, 0xd4, SUN.load(Ordering::SeqCst) as u32);
    }
}

/// A click the menu makes itself is being heard (it hands its listeners where the mouse was
/// from whatever is on the stack: the place is left out of the comparison).
pub fn synthetic() -> bool {
    SYNTHETIC.load(Ordering::SeqCst)
}

/// `Sim::cameraManager` is there around the calls that read it (the HUD's proofs leave it
/// empty between theirs).
unsafe fn with_camera(h: &HudGame, f: impl FnOnce()) {
    let before: usize = rd(h.sim, 0x188);
    wr(h.sim, 0x188, h.more.camera);
    f();
    wr(h.sim, 0x188, before);
}

/// The jobs of a fade that is over (`run`), with the camera there; and whether the job of
/// "Drive" wrote the focused car's number (a member of `Sim` it writes without a call).
pub unsafe fn fade_done(h: &HudGame, run: impl FnOnce()) {
    let before: i32 = rd(h.sim, 0x220);
    wr(h.sim, 0x220, 7i32);
    SYNTHETIC.store(true, Ordering::SeqCst);
    with_camera(h, run);
    SYNTHETIC.store(false, Ordering::SeqCst);
    if rd::<i32>(h.sim, 0x220) == 0 {
        action("focusPlayerCar".into());
    }
    wr(h.sim, 0x220, before);
}

/// A menu call of the script's that is the ESC menu's (false: it is not).
pub unsafe fn call(game: &Game, gui: *mut u8, h: &HudGame, what: &str, number: f32, _text: &str) -> bool {
    let a = &game.acs;
    let sim = h.sim;
    match what {
        "menu.escMenu" => {
            static DONE: AtomicBool = AtomicBool::new(false);
            if !DONE.swap(true, Ordering::SeqCst) {
                for (va, to) in [
                    (VA_SETUP_SCREEN_CTOR, setup_screen_ctor as *const () as usize),
                    (VA_COMPARATIVE_TELEMETRY_CTOR, comparative_telemetry_ctor as *const () as usize),
                    (VA_TIME_TABLE_CTOR, time_table_ctor as *const () as usize),
                    (VA_GET_TIME_TO_WAIT, get_time_to_wait as *const () as usize),
                    (VA_GET_INVERTED_GRID_POSITIONS, get_inverted_grid_positions as *const () as usize),
                    (VA_GET_PIT_WINDOW_START, get_pit_window_start as *const () as usize),
                    (VA_GET_PIT_WINDOW_END, get_pit_window_end as *const () as usize),
                    (VA_GET_LAP_COUNT, get_lap_count as *const () as usize),
                    (VA_START_RACE, start_race as *const () as usize),
                    (VA_CAR_IS_IN_PIT, is_in_pit as *const () as usize),
                    (VA_LOAD_PREVIOUS_SESSION_USER_CAMERA, load_previous_session_user_camera as *const () as usize),
                ] {
                    // (a script with a car of the game has the game's own setup editor)
                    if va == VA_SETUP_SCREEN_CTOR && !crate::gui_ac_setup::car().is_null() {
                        continue;
                    }
                    // (and a script with laps has the game's own time table)
                    if va == VA_TIME_TABLE_CTOR && crate::gui_seq_setup::time_seed() != 0.0 {
                        continue;
                    }
                    jump(game, va, to);
                }
            }
            // PhysicsAvatar's stand-in: room for the wind and the temperatures, and a track
            let avatar = a.alloc(0x800);
            std::ptr::copy_nonoverlapping(rd::<*const u8>(sim, 0x1b8), avatar, 0x400);
            wr(avatar, 0x1f0, a.alloc(0x200));
            wr(sim, 0x1b8, avatar);
            let menu = a.alloc(0x260);
            MENU.store(menu as usize, Ordering::SeqCst);
            // (the constructor reads the car's setup state and the race manager)
            wr(h.cars[0], 0xa40, script().esc.setup_state);
            wr(rd::<*mut u8>(sim, 0x1a8), 0xa2, script().esc.fixed_setup as u8);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_ESC_MENU_CTOR));
            SYNTHETIC.store(true, Ordering::SeqCst);
            ctor(menu, sim, gui);
            SYNTHETIC.store(false, Ordering::SeqCst);
            sync(game, h);
            wr(sim, 0x118, menu);
            // Sim::Sim: the menu is a top-level control of the GUI, and its coming and going
            // is heard by the game's own handler (a `std::function` of one captured pointer)
            let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(a.va(VA_GUI_ADD_CONTROL));
            add(gui, menu);
            let table = a.alloc(0x40);
            wr(table, 0x10, a.va(VA_ESC_MENU_TRIGGERED_HANDLER) as usize);
            let implementation = a.alloc(0x10);
            wr(implementation, 0, table);
            wr(implementation, 0x8, sim);
            let entry = a.alloc(0x28);
            wr(entry, 0, sim);
            wr(entry, 0x20, implementation);
            wr(menu, 0x178, entry);
            wr(menu, 0x180, entry.add(0x28));
            wr(menu, 0x188, entry.add(0x28));
        }
        "menu.escShow" => {
            sync(game, h);
            let activate: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_SIM_ACTIVATE_ESC_MENU));
            SYNTHETIC.store(true, Ordering::SeqCst);
            with_camera(h, || activate(sim));
            SYNTHETIC.store(false, Ordering::SeqCst);
        }
        "menu.driveEnabled" => {
            let set: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(VA_ESC_MENU_SET_DRIVE_BUTTON_ENABLED));
            set(menu(), number != 0.0);
        }
        "menu.raceControl" => {
            let set: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_ESC_MENU_SET_RACE_CONTROL_SCREEN));
            set(menu());
        }
        _ => return false,
    }
    let _ = input(|i| i.online);
    true
}

/// The menu's own state in the dump.
pub unsafe fn extra(control: *const u8) -> String {
    let last: *mut u8 = rd(control, 0x238);
    let index = children(control).iter().position(|c| *c == last).map_or(-1, |i| i as i32);
    crate::gui_seq_menu::esc_extra(index, rd::<u8>(control, 0x1b1) != 0, rd(control, 0x1b4))
}
