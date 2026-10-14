// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's HUD: acs.exe's own HUD objects built on a `Sim` laid out
//! by hand.
//!
//! What stands in for the game around them: a zeroed `Game` with only its graphics and its
//! GUI, a zeroed `Sim` with the members the HUD objects read (the game screen, two plain
//! controls for the ESC and the pause menu, a replay manager's "in replay" byte, the cars), a
//! zeroed `CarAvatar` a car with its names, steering and penalty members written from the
//! script, and five `RaceManager` functions and `acTranslate` replaced by ones that answer
//! with the script's values (the referee itself is the proof of Task 28).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rustyac_gui::hud::HudInput;

use crate::ac::{rd, wr, write_wstring, wstring, Game};
use crate::acs::Acs;
use crate::gui_ac::read_wstring;

const VA_RACE_MANAGER_GET_CURRENT_SESSION: usize = 0x1_4013_8220;
const VA_RACE_MANAGER_GET_PRACTICE_QUALIFY_TEXT: usize = 0x1_4013_8d80;
const VA_RACE_MANAGER_GET_RACE_TEXT: usize = 0x1_4013_9150;
const VA_RACE_MANAGER_GET_TIME_TO_SESSION_START: usize = 0x1_4013_a0a0;
const VA_RACE_MANAGER_GET_CAR_LEADERBOARD_POSITION: usize = 0x1_4013_7ee0;
const VA_RACE_MANAGER_GET_LAST_LAP: usize = 0x1_4013_87c0;
const VA_AC_TRANSLATE: usize = 0x1_4024_0cf0;
const VA_SYSTEM_MESSAGE_CTOR: usize = 0x1_401b_5610;
const VA_SYSTEM_MESSAGE_ADD_MESSAGE: usize = 0x1_401b_61c0;
const VA_SYSTEM_MESSAGE_ADD_CAR_MESSAGE: usize = 0x1_401b_60b0;
const VA_SYSTEM_MESSAGE_CLEAR: usize = 0x1_401b_6400;
const VA_SYSTEM_MESSAGE_LOAD_DRIVERS_INFO: usize = 0x1_401b_64f0;
const VA_SYSTEM_MESSAGE_UPDATE: usize = 0x1_401b_66b0;
const VA_SYSTEM_MESSAGE_RENDER_HUD: usize = 0x1_401b_6520;
const VA_SYSTEM_NOTIFICATION_CTOR: usize = 0x1_401b_68a0;
const VA_SYSTEM_NOTIFICATION_SHOW: usize = 0x1_401b_7910;
const VA_SYSTEM_NOTIFICATION_UPDATE: usize = 0x1_401b_79c0;

pub struct HudGame {
    pub sim: *mut u8,
    pub message: *mut u8,
    pub notification: *mut u8,
    pub cars: Vec<*mut u8>,
    pub esc: *mut u8,
    pub pause: *mut u8,
    pub more: crate::gui_ac_hud2::More,
}

static INPUT: Mutex<Option<HudInput>> = Mutex::new(None);
static CARS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static ACS: AtomicUsize = AtomicUsize::new(0);
static HUD: AtomicUsize = AtomicUsize::new(0);

pub fn input<T>(f: impl FnOnce(&mut HudInput) -> T) -> T {
    f(INPUT.lock().unwrap().get_or_insert_with(HudInput::default))
}

pub fn set_acs(acs: &Acs) {
    ACS.store(acs as *const Acs as usize, Ordering::SeqCst);
}

pub unsafe fn acs() -> &'static Acs {
    &*(ACS.load(Ordering::SeqCst) as *const Acs)
}

pub unsafe fn hud() -> Option<&'static mut HudGame> {
    (HUD.load(Ordering::SeqCst) as *mut HudGame).as_mut()
}

fn car_index(car: *const u8) -> usize {
    CARS.lock().unwrap().iter().position(|c| *c == car as usize).unwrap_or(0)
}

unsafe fn empty_wstring(at: *mut u8) {
    std::ptr::write_bytes(at, 0, 0x20);
    wr(at, 0x18, 7usize);
}

/// In place of `RaceManager::getCurrentSession`: a `Session` (0x70 bytes) of the script's kind.
unsafe extern "C" fn get_current_session(_this: *mut u8, out: *mut u8) -> *mut u8 {
    std::ptr::write_bytes(out, 0, 0x70);
    wr(out, 0, input(|i| i.session_type));
    wr(out, 4, crate::gui_seq_menu::script().pit.timed as u8);
    empty_wstring(out.add(0x18));
    empty_wstring(out.add(0x38));
    out
}

unsafe extern "C" fn get_practice_qualify_text(_this: *mut u8, out: *mut u8) -> *mut u8 {
    write_wstring(acs(), out, &input(|i| i.practice_qualify_text.clone()));
    out
}

unsafe extern "C" fn get_race_text(_this: *mut u8, out: *mut u8) -> *mut u8 {
    write_wstring(acs(), out, &input(|i| i.race_text.clone()));
    out
}

unsafe extern "C" fn get_time_to_session_start(_this: *mut u8) -> f64 {
    input(|i| i.time_to_session_start)
}

unsafe extern "C" fn get_car_leaderboard_position(_this: *mut u8, car: *mut u8) -> i32 {
    let index = car_index(car);
    input(|i| i.cars.get(index).map(|c| c.leaderboard_position).unwrap_or(-1))
}

/// In place of `RaceManager::getLastLap`: a `Lap` (0x48 bytes) with only its time.
unsafe extern "C" fn get_last_lap(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    let index = car_index(car);
    // (the standings' scripts say the whole lap; the HUD's only its time)
    if let Some(car) = crate::gui_ac_race::race().cars.get(index) {
        crate::gui_ac_race::write_lap(out, &car.last_lap);
        return out;
    }
    std::ptr::write_bytes(out, 0, 0x48);
    wr(out, 0, input(|i| i.cars.get(index).map(|c| c.last_lap_ms).unwrap_or(0)));
    empty_wstring(out.add(0x28));
    out
}

/// In place of `acTranslate`: the string as it is (what the game gives for English).
pub unsafe extern "C" fn ac_translate(out: *mut u8, text: *const u8) -> *mut u8 {
    write_wstring(acs(), out, &read_wstring(text));
    out
}

pub unsafe fn jump(game: &Game, va: usize, to: usize) {
    // mov rax, <to>; jmp rax
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&to.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xe0]);
    game.acs.patch(game.acs.va(va), &code);
}

/// The script's values written into the stand-in blocks the game's code reads itself.
unsafe fn sync(h: &HudGame) {
    let state = input(|i| i.clone());
    wr(h.esc, 0x150, state.esc_menu_visible as u8);
    wr(h.esc, 0x28, [0.0f32, state.esc_menu_width, 0.0, 720.0]);
    wr(h.pause, 0x150, state.pause_menu_visible as u8);
    let replay: *mut u8 = rd(h.sim, 0x1b0);
    wr(replay, 0x148, state.in_replay as u8);
    for (avatar, car) in h.cars.iter().zip(&state.cars) {
        wr(*avatar, 0xdb4, car.lock_controls_time);
        wr(*avatar, 0x4c4, car.steer);
        wr(*avatar, 0xe40, car.steer_lock);
        // Car::getPenaltyTime 0x140270d50 and TimeTransponder::getCuts 0x1402906a0 read these
        let physics: *mut u8 = rd(*avatar, 0x1168);
        wr(physics, 0x3d30, if car.penalty_time > 0.0 { 1.0f64 } else { 0.0 });
        wr(physics, 0x3d28, car.penalty_time);
        wr(physics, 0x1f8 + 0x84, !car.current_lap_valid as u32);
    }
    crate::gui_ac_hud2::sync(h, &state);
}

unsafe fn write_names(h: &HudGame) {
    let state = input(|i| i.clone());
    for (avatar, car) in h.cars.iter().zip(&state.cars) {
        write_wstring(acs(), avatar.add(0x10b8), &car.driver_name);
        write_wstring(acs(), avatar.add(0x178), &car.gui_name);
        write_wstring(acs(), avatar.add(0x138), &car.unix_name);
    }
}

/// A script's HUD call on the game's objects (see `gui_seq::hud_input` for the ones that only
/// change the session's state).
pub unsafe fn call(game: &Game, gui: *mut u8, screen: *mut u8, what: &str, number: f32, text: &str) -> Result<(), String> {
    let acs_ref = &game.acs;
    ACS.store(acs_ref as *const Acs as usize, Ordering::SeqCst);
    if what.starts_with("app.") {
        if crate::gui_seq_apps::app_input(what, number, text)? {
            crate::gui_ac_apps::update();
            crate::gui_ac_apps::announce_laps(game);
            return Ok(());
        }
        return crate::gui_ac_apps::call(game, gui, screen, what, number, text);
    }
    if what.starts_with("menu.") || what.starts_with("pit.") || what.starts_with("race.") {
        if crate::gui_seq_menu::menu_input(what, number, text) {
            return Ok(());
        }
        if let Some(h) = hud() {
            sync(h);
            crate::gui_ac_race::sync(h);
        }
        if what.starts_with("menu.pit") {
            return crate::gui_ac_pit::call(game, gui, what, number, text);
        }
        return crate::gui_ac_menu::call(game, gui, what, number, text);
    }
    if crate::gui_seq::hud_input(&mut *INPUT.lock().unwrap().get_or_insert_with(HudInput::default), what, number, text) {
        if let Some(h) = hud() {
            if what.starts_with("car.") {
                write_names(h);
            }
        }
        return Ok(());
    }
    if what == "create" {
        jump(game, VA_RACE_MANAGER_GET_CURRENT_SESSION, get_current_session as *const () as usize);
        jump(game, VA_RACE_MANAGER_GET_PRACTICE_QUALIFY_TEXT, get_practice_qualify_text as *const () as usize);
        jump(game, VA_RACE_MANAGER_GET_RACE_TEXT, get_race_text as *const () as usize);
        jump(game, VA_RACE_MANAGER_GET_TIME_TO_SESSION_START, get_time_to_session_start as *const () as usize);
        jump(game, VA_RACE_MANAGER_GET_CAR_LEADERBOARD_POSITION, get_car_leaderboard_position as *const () as usize);
        jump(game, VA_RACE_MANAGER_GET_LAST_LAP, get_last_lap as *const () as usize);
        if crate::gui_seq::language().is_none() {
            jump(game, VA_AC_TRANSLATE, ac_translate as *const () as usize);
        }

        let game_block = acs_ref.alloc(0x300);
        wr(game_block, 0x138, game.graphics);
        wr(game_block, 0x180, gui);
        let sim = acs_ref.alloc(0x2d0);
        wr(sim, 0x8, game_block);
        wr(sim, 0x108, screen);
        let control = |name: &str| {
            let object = acs_ref.alloc(0x178);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs_ref.va(0x1_4024_4150));
            ctor(object, wstring(acs_ref, name), gui);
            wr(object, 0x150, 0u8);
            object
        };
        let (pause, esc) = (control("PAUSE_MENU"), control("ESC_MENU"));
        wr(sim, 0x110, pause);
        wr(sim, 0x118, esc);
        wr(sim, 0x1a8, acs_ref.alloc(0x400));
        wr(sim, 0x1b0, acs_ref.alloc(0x200));
        let count = number as usize;
        let list = acs_ref.alloc(count * 8 + 8);
        let mut cars = Vec::new();
        for i in 0..count {
            let avatar = acs_ref.alloc(0x2000);
            wr(avatar, 0x130, sim);
            wr(avatar, 0x1168, acs_ref.alloc(0x4000));
            for at in [0x10b8, 0x178, 0x138] {
                empty_wstring(avatar.add(at));
            }
            wr(list, i * 8, avatar);
            cars.push(avatar);
        }
        wr(sim, 0x208, list);
        wr(sim, 0x210, list.add(count * 8));
        wr(sim, 0x218, list.add(count * 8));
        *CARS.lock().unwrap() = cars.iter().map(|c| *c as usize).collect();
        input(|i| i.cars.resize(count, crate::gui_seq::default_car()));

        let h = Box::leak(Box::new(HudGame { sim, message: std::ptr::null_mut(), notification: std::ptr::null_mut(), cars, esc, pause, more: Default::default() }));
        HUD.store(h as *mut HudGame as usize, Ordering::SeqCst);
        write_names(h);
        sync(h);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs_ref.va(VA_SYSTEM_MESSAGE_CTOR));
        h.message = acs_ref.alloc(0x98);
        ctor(h.message, sim);
        wr(sim, 0x198, h.message);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs_ref.va(VA_SYSTEM_NOTIFICATION_CTOR));
        h.notification = acs_ref.alloc(0xa8);
        ctor(h.notification, sim);
        wr(sim, 0x1a0, h.notification);
        let load: extern "C" fn(*mut u8) = std::mem::transmute(acs_ref.va(VA_SYSTEM_MESSAGE_LOAD_DRIVERS_INFO));
        load(h.message);
        crate::gui_ac_hud2::create(game, gui, screen, h)?;
        return Ok(());
    }
    let h = hud().ok_or("the HUD was not made (Hud(\"create\", cars, \"\") first)")?;
    sync(h);
    match what {
        "message" => {
            let (title, description) = text.split_once('|').unwrap_or((text, ""));
            let add: extern "C" fn(*mut u8, *mut u8, *mut u8, i32) = std::mem::transmute(acs_ref.va(VA_SYSTEM_MESSAGE_ADD_MESSAGE));
            add(h.message, wstring(acs_ref, title), wstring(acs_ref, description), number as i32);
        }
        "carMessage" => {
            let add: extern "C" fn(*mut u8, u32, i32) = std::mem::transmute(acs_ref.va(VA_SYSTEM_MESSAGE_ADD_CAR_MESSAGE));
            add(h.message, number as u32, text.parse().unwrap_or(0));
        }
        "clear" => {
            let clear: extern "C" fn(*mut u8) = std::mem::transmute(acs_ref.va(VA_SYSTEM_MESSAGE_CLEAR));
            clear(h.message);
        }
        "notify" => {
            let show: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs_ref.va(VA_SYSTEM_NOTIFICATION_SHOW));
            show(h.notification, number as i32);
        }
        other => return crate::gui_ac_hud2::call(game, gui, h, other, number, text),
    }
    Ok(())
}

/// `Game::update` over the HUD objects, in the order `Sim::Sim` made them.
pub unsafe fn update(game: &Game, dt: f32) {
    let Some(h) = hud() else { return };
    sync(h);
    crate::gui_ac_race::sync(h);
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_SYSTEM_MESSAGE_UPDATE));
    update(h.message, dt);
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_SYSTEM_NOTIFICATION_UPDATE));
    update(h.notification, dt);
    crate::gui_ac_hud2::update(game, h, dt);
    crate::gui_ac_menu::update(game, h, dt);
}

/// `Game::renderHUD` over the HUD objects.
pub unsafe fn render_hud(game: &Game, dt: f32) {
    let Some(h) = hud() else { return };
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_SYSTEM_MESSAGE_RENDER_HUD));
    render(h.message, dt);
    crate::gui_ac_pit::render_menu(game, h, dt);
    crate::gui_ac_hud2::render_hud(game, h, dt);
}

pub unsafe fn dump(game: &Game, out: &mut String) {
    let Some(h) = hud() else { return };
    let m = h.message;
    let session: *const u8 = rd(m, 0x90);
    out.push_str(&crate::gui_seq::message_line(rd(m, 0x58), rd(m, 0x5c), &read_wstring(session.add(0x10)), rd::<u8>(session, 0x30) != 0, rd(session, 0x34), rd(rd::<*const u8>(m, 0x88), 0x178)));
    crate::gui_ac::dump_control(game, rd(m, 0x80), 1, out);
    crate::gui_ac::dump_control(game, rd(m, 0x88), 1, out);
    let n = h.notification;
    out.push_str(&crate::gui_seq::notification_line(rd(n, 0x80), rd(n, 0x84), rd(n, 0x88), rd::<u8>(n, 0x8c) != 0));
    crate::gui_ac_hud2::dump(game, h, out);
    crate::gui_ac_pit::dump(game, out);
    crate::gui_ac_menu::dump(game, out);
    crate::gui_seq_menu::dump_actions(out);
    crate::gui_ac_setup::items(out);
}
