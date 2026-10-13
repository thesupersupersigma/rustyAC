// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's HUD, part two: the objects besides the messages.
//!
//! `FlagManager`, the three banners and `WrongWayIndicator` are built by the game's own
//! constructors on the stand-in `Sim`; `FlagManager`'s three checks (blue, yellow, penalty:
//! the referee's, proven in Task 28) are cut out so that a script sets the flag itself.
//! `FuelLight` is laid out by hand (its constructor opens the car's data through a whole
//! `CarAvatar`) with the game's own handler hung on the flag event.

use rustyac_gui::hud::HudInput;

use crate::ac::{rd, wr, write_wstring, wstring, Game};
use crate::gui_ac_hud::{acs, input, jump, HudGame};

const VA_RACE_MANAGER_GET_CURRENT_SESSION_TYPE: usize = 0x1_4013_8300;
const VA_RACE_MANAGER_IS_RACE_OVER: usize = 0x1_4013_eae0;
const VA_RACE_MANAGER_GET_SESSION_TIME_LEFT: usize = 0x1_4013_9e60;
const VA_FLAG_MANAGER_CTOR: usize = 0x1_4009_ab60;
const VA_FLAG_MANAGER_CHECKS: [usize; 3] = [0x1_4009_c390, 0x1_4009_c550, 0x1_4009_c9b0];
const VA_FLAG_MANAGER_SET_FLAG: usize = 0x1_4009_cf00;
const VA_FLAG_MANAGER_CLEAR_FLAG: usize = 0x1_4009_cb60;
const VA_FLAG_MANAGER_SET_BLUE_FLAG: usize = 0x1_4009_ce30;
const VA_FLAG_MANAGER_UPDATE: usize = 0x1_4009_d140;
const VA_TRACK_AVATAR_VTABLE: usize = 0x1_404d_e068;
const VA_LAP_INVALIDATOR_CTOR: usize = 0x1_4011_1990;
const VA_LAP_INVALIDATOR_RENDER_HUD: usize = 0x1_4011_1c00;
const VA_DRIVE_THROUGH_CTOR: usize = 0x1_4009_07b0;
const VA_DRIVE_THROUGH_RENDER_HUD: usize = 0x1_4009_0b50;
const VA_LOCK_CONTROLS_CTOR: usize = 0x1_4009_d8c0;
const VA_LOCK_CONTROLS_RENDER_HUD: usize = 0x1_4009_dc50;
const VA_WRONG_WAY_CTOR: usize = 0x1_401d_83d0;
const VA_WRONG_WAY_RENDER_HUD: usize = 0x1_401d_87a0;
const VA_FUEL_LIGHT_RENDER_HUD: usize = 0x1_400b_c030;
const VA_FUEL_LIGHT_FLAG_HANDLER: usize = 0x1_400b_bad0;
const VA_RESOURCE_STORE_GET_TEXTURE: usize = 0x1_4020_00d0;
const VA_SPEED_USE_MPH: usize = 0x1_4155_a5ae;

pub struct More {
    pub flags: *mut u8,
    pub lap: *mut u8,
    pub drive: *mut u8,
    pub lock: *mut u8,
    pub wrong: *mut u8,
    pub fuel: *mut u8,
    pub camera: *mut u8,
    pub ended: *mut u8,
    pub cars: crate::gui_ac_hud3::Cars,
}

impl Default for More {
    fn default() -> More {
        let n = std::ptr::null_mut();
        More { flags: n, lap: n, drive: n, lock: n, wrong: n, fuel: n, camera: n, ended: n, cars: Default::default() }
    }
}

unsafe extern "C" fn get_current_session_type(_this: *mut u8) -> i32 {
    input(|i| i.session_type)
}

unsafe extern "C" fn is_race_over(_this: *mut u8, _car: u32) -> bool {
    input(|i| i.race_over)
}

unsafe extern "C" fn get_session_time_left(_this: *mut u8) -> f64 {
    input(|i| i.session_time_left)
}

pub unsafe fn sync(h: &HudGame, state: &HudInput) {
    wr(h.sim, 0x220, state.focused_car_index);
    wr(h.sim, 0x201, state.triple_screen as u8);
    wr(acs().va(VA_SPEED_USE_MPH) as *mut u8, 0, state.use_mph as u8);
    if !h.more.ended.is_null() {
        wr(h.more.ended, 0x30, state.end_session_active as u8);
        wr(h.more.camera, 0x108, state.camera.map(|c| c.1).unwrap_or(false) as u8);
        wr(h.more.camera, 0x120, state.camera_mode);
    }
    for (avatar, car) in h.cars.iter().zip(&state.cars) {
        let physics: *mut u8 = rd(*avatar, 0x1168);
        wr(physics, 0x3e2c, car.min_speed_penalty_clear_disabled as u8);
        wr(physics, 0x3a5e, car.pit_penalty_laps);
        wr(*avatar, 0x9fc, car.fuel);
    }
    crate::gui_ac_hud3::sync(h, state);
}

unsafe fn construct(game: &Game, size: usize, ctor: usize, argument: *mut u8) -> *mut u8 {
    let object = game.acs.alloc(size);
    let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(game.acs.va(ctor));
    ctor(object, argument);
    object
}

pub unsafe fn create(game: &Game, gui: *mut u8, screen: *mut u8, h: &mut HudGame) -> Result<(), String> {
    let a = &game.acs;
    jump(game, VA_RACE_MANAGER_GET_CURRENT_SESSION_TYPE, get_current_session_type as *const () as usize);
    jump(game, VA_RACE_MANAGER_IS_RACE_OVER, is_race_over as *const () as usize);
    jump(game, VA_RACE_MANAGER_GET_SESSION_TIME_LEFT, get_session_time_left as *const () as usize);
    for va in VA_FLAG_MANAGER_CHECKS {
        a.patch(a.va(va), &[0xc3]);
    }
    let sim = h.sim;
    let game_block: *mut u8 = rd(sim, 0x8);
    for car in &h.cars {
        wr(*car, 0x8, game_block);
    }
    // a track that is only a name, and the list of the Sim's children the constructors search
    let track = a.alloc(0x400);
    wr(track, 0, a.va(VA_TRACK_AVATAR_VTABLE));
    for at in [0x10, 0x80, 0xa0] {
        std::ptr::write_bytes(track.add(at), 0, 0x20);
        wr(track, at + 0x18, 7usize);
    }
    write_wstring(a, track.add(0x10), "nowhere");
    wr(sim, 0x150, track);
    let children = a.alloc(4 * 8);
    wr(children, 0, track);
    wr(sim, 0x40, children);
    wr(sim, 0x48, children.add(8));
    wr(sim, 0x50, children.add(4 * 8));
    wr(sim, 0x1b8, a.alloc(0x400));
    h.more.ended = a.alloc(0x60);
    wr(sim, 0x260, h.more.ended);
    h.more.camera = a.alloc(0x200);
    let state = input(|i| i.clone());
    sync(h, &state);

    h.more.wrong = construct(game, 0xb8, VA_WRONG_WAY_CTOR, h.cars[0]);
    h.more.lap = construct(game, 0xb8, VA_LAP_INVALIDATOR_CTOR, sim);
    h.more.drive = construct(game, 0x78, VA_DRIVE_THROUGH_CTOR, sim);
    h.more.lock = construct(game, 0x90, VA_LOCK_CONTROLS_CTOR, sim);
    h.more.flags = construct(game, 0x108, VA_FLAG_MANAGER_CTOR, sim);
    wr(children, 8, h.more.flags);
    wr(sim, 0x48, children.add(16));

    let fuel = a.alloc(0xd8);
    wr(fuel, 0x8, game_block);
    wr(fuel, 0x30, 1u8);
    wr(fuel, 0x58, sim);
    let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(a.va(VA_RESOURCE_STORE_GET_TEXTURE));
    get_texture(rd(game.graphics, 0x180), fuel.add(0x60), wstring(a, "content//gui//fuelIndicator//fuelIndicator.png"), false);
    get_texture(rd(game.graphics, 0x180), fuel.add(0x88), wstring(a, "content//gui//fuelIndicator//outOfFuel.png"), false);
    let limits = a.alloc(4);
    wr(limits, 0, 3.0f32);
    wr(fuel, 0xb0, limits);
    wr(fuel, 0xb8, limits.add(4));
    wr(fuel, 0xc0, limits.add(4));
    wr(fuel, 0xc8, h.more.flags);
    wr(fuel, 0xd0, 25.0f32);
    // the game's own handler on FlagManager::evOnFlagChange: a function object whose target is
    // FuelLight's lambda, which finds the light in the word after the table
    let table = a.alloc(6 * 8);
    wr(table, 2 * 8, a.va(VA_FUEL_LIGHT_FLAG_HANDLER));
    let function = a.alloc(16);
    wr(function, 0, table);
    wr(function, 8, fuel);
    let entry = a.alloc(0x28);
    wr(entry, 0, fuel);
    wr(entry, 0x20, function);
    wr(h.more.flags, 0x58, entry);
    wr(h.more.flags, 0x60, entry.add(0x28));
    wr(h.more.flags, 0x68, entry.add(0x28));
    h.more.fuel = fuel;
    crate::gui_ac_hud3::create(game, gui, screen, h)
}

pub unsafe fn call(game: &Game, gui: *mut u8, h: &mut HudGame, what: &str, number: f32, text: &str) -> Result<(), String> {
    let a = &game.acs;
    let flags = h.more.flags;
    match what {
        "setFlag" => {
            let set: extern "C" fn(*mut u8, i32) = std::mem::transmute(a.va(VA_FLAG_MANAGER_SET_FLAG));
            set(flags, number as i32);
        }
        "clearFlag" => {
            let clear: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_FLAG_MANAGER_CLEAR_FLAG));
            clear(flags);
        }
        "setBlueFlag" => {
            let set: extern "C" fn(*mut u8, i32) = std::mem::transmute(a.va(VA_FLAG_MANAGER_SET_BLUE_FLAG));
            set(flags, 0);
        }
        "flagTimer" => wr(flags, 0x70, number),
        // what the lambda on the physics' flag event does (0x14009b830)
        "blackFlag" => {
            if number != 0.0 {
                let set: extern "C" fn(*mut u8, i32) = std::mem::transmute(a.va(VA_FLAG_MANAGER_SET_FLAG));
                set(flags, 2);
            } else {
                let clear: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_FLAG_MANAGER_CLEAR_FLAG));
                clear(flags);
            }
            wr(h.cars[0], 0x1000, (number != 0.0) as u8);
            let caption: *mut u8 = rd(flags, 0x100);
            let table: *const usize = rd(caption, 0);
            let set_text: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*table.add(0x88 / 8));
            set_text(caption, wstring(a, text));
        }
        "wrongWay" => wr(h.more.wrong, 0x78, (number != 0.0) as u8),
        "fuelLimit" => wr(rd::<*mut u8>(h.more.fuel, 0xb0), 0, number),
        "replayActive" => {
            wr(h.more.wrong, 0x30, (number != 0.0) as u8);
            wr(h.more.fuel, 0x30, (number != 0.0) as u8);
        }
        other => return crate::gui_ac_hud3::call(game, gui, h, other, number, text),
    }
    Ok(())
}

pub unsafe fn update(game: &Game, h: &mut HudGame, dt: f32) {
    if h.more.flags.is_null() {
        return;
    }
    crate::gui_ac_hud3::update(game, h, dt);
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_FLAG_MANAGER_UPDATE));
    update(h.more.flags, dt);
}

unsafe fn render(game: &Game, object: *mut u8, va: usize, dt: f32) {
    if rd::<u8>(object, 0x30) != 0 {
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(va));
        render(object, dt);
    }
}

/// `Game::renderHUD` over the objects, in the order `Sim::Sim` made them.
pub unsafe fn render_hud(game: &Game, h: &mut HudGame, dt: f32) {
    let m = &h.more;
    if m.flags.is_null() {
        return;
    }
    // (the camera manager is only there for this call: the messages were proven without one)
    wr(h.sim, 0x188, m.camera);
    render(game, m.wrong, VA_WRONG_WAY_RENDER_HUD, dt);
    wr(h.sim, 0x188, 0usize);
    render(game, m.lap, VA_LAP_INVALIDATOR_RENDER_HUD, dt);
    render(game, m.drive, VA_DRIVE_THROUGH_RENDER_HUD, dt);
    render(game, m.lock, VA_LOCK_CONTROLS_RENDER_HUD, dt);
    crate::gui_ac_hud3::render_hud(game, h, dt);
    render(game, h.more.fuel, VA_FUEL_LIGHT_RENDER_HUD, dt);
}

pub unsafe fn dump(game: &Game, h: &HudGame, out: &mut String) {
    let m = &h.more;
    if m.flags.is_null() {
        return;
    }
    out.push_str(&crate::gui_seq::flags_line(rd(m.flags, 0xb0), rd(m.flags, 0x70), rd(m.lap, 0xb0), rd::<u8>(m.lap, 0xb4) != 0, rd(m.fuel, 0xd0), rd(m.drive, 0x70)));
    crate::gui_ac_hud3::dump(game, h, out);
}
