// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's pit stop: acs.exe's own `QuickMenuPitstop` (constructor,
//! `buildControls` on a `setup.ini` of the script's, `sendInput`, `renderHUD`) and
//! `PitStop::renderHUD` on a `PitStop` laid out by hand (its constructor makes the pit crew).
//!
//! What the menu reads of the player's car is written into the stand-in `CarAvatar`, or
//! answered by stand-ins for `CarAvatar::getTyreCompound`, `getWingAngle`,
//! `getCurrentCompoundStaticPressure` and `RaceManager::getSessionTotalTime`.

use std::sync::Mutex;

use crate::ac::{rd, wr, write_wstring, wstring, Game};
use crate::gui_ac_hud::{acs, hud, jump, HudGame};
use crate::gui_seq_menu::script;

const VA_QUICK_MENU_CTOR: usize = 0x1_400a_ca70;
const VA_QUICK_MENU_BUILD_CONTROLS: usize = 0x1_400a_e7b0;
const VA_QUICK_MENU_RENDER_HUD: usize = 0x1_400b_1230;
const VA_QUICK_MENU_SEND_INPUT: usize = 0x1_400b_1460;
const VA_INI_READER_CTOR: usize = 0x1_4023_40a0;
const VA_CAR_GET_TYRE_COMPOUND: usize = 0x1_400d_3980;
const VA_CAR_GET_WING_ANGLE: usize = 0x1_400d_3aa0;
const VA_CAR_GET_STATIC_PRESSURE: usize = 0x1_400d_32b0;
const VA_RACE_MANAGER_GET_SESSION_TOTAL_TIME: usize = 0x1_4013_9ee0;
const VA_PIT_STOP_RENDER_HUD: usize = 0x1_400a_a300;
const VA_RESOURCE_STORE_GET_TEXTURE: usize = 0x1_4020_00d0;

struct Pit {
    menu: usize,
    stop: usize,
}

static PIT: Mutex<Pit> = Mutex::new(Pit { menu: 0, stop: 0 });

unsafe extern "C" fn get_tyre_compound(_this: *mut u8, out: *mut u8, index: u32, _short: bool) -> *mut u8 {
    write_wstring(acs(), out, &script().pit.compounds.get(index as usize).cloned().unwrap_or_default());
    out
}

unsafe extern "C" fn get_wing_angle(_this: *mut u8, wing: i32) -> f32 {
    script().pit.wing.get(wing.max(0) as usize).copied().unwrap_or(0.0)
}

unsafe extern "C" fn get_static_pressure(_this: *mut u8, wheel: u16) -> i32 {
    script().pit.pressure.get(wheel as usize).copied().unwrap_or(0)
}

unsafe extern "C" fn get_session_total_time(_this: *mut u8, _session: u32) -> i32 {
    script().pit.total_time
}

/// `CarAvatar::getTyreCompound`, `getWingAngle` and `getCurrentCompoundStaticPressure`
/// answered from the script.
pub unsafe fn car_stand_ins(game: &Game) {
    jump(game, VA_CAR_GET_TYRE_COMPOUND, get_tyre_compound as *const () as usize);
    jump(game, VA_CAR_GET_WING_ANGLE, get_wing_angle as *const () as usize);
    jump(game, VA_CAR_GET_STATIC_PRESSURE, get_static_pressure as *const () as usize);
}

/// The script's car, written where the game's code reads it.
unsafe fn sync(h: &HudGame) {
    let s = script().pit;
    let avatar = h.cars[0];
    wr(avatar, 0xec8, s.max_fuel as f64);
    wr(avatar, 0x129c, s.in_pitlane as u8);
    wr(avatar, 0x984, s.spline);
    // FuelLapEvaluator::getFuelPerLap 0x14028d290: with no lap counted, its first guess
    let physics: *mut u8 = rd(avatar, 0x1168);
    wr(physics, 0x3af8 + 8, 0i32);
    wr(physics, 0x3af8 + 0x38, s.fuel_per_lap as f64);
    let stop = PIT.lock().unwrap().stop as *mut u8;
    if !stop.is_null() {
        wr(stop, 0xa4, s.times[0]);
        wr(stop, 0xac, s.times[1]);
        wr(stop, 0xbc, s.times[2]);
        wr(stop, 0xb8, s.times[3]);
        wr(stop, 0xb4, s.times[4]);
    }
}

pub unsafe fn call(game: &Game, _gui: *mut u8, what: &str, number: f32, text: &str) -> Result<(), String> {
    let a = &game.acs;
    let h = hud().ok_or("the HUD was not made (Hud(\"create\", cars, \"\") first)")?;
    let sim = h.sim;
    match what {
        "menu.pit" => {
            jump(game, VA_CAR_GET_TYRE_COMPOUND, get_tyre_compound as *const () as usize);
            jump(game, VA_CAR_GET_WING_ANGLE, get_wing_angle as *const () as usize);
            jump(game, VA_CAR_GET_STATIC_PRESSURE, get_static_pressure as *const () as usize);
            jump(game, VA_RACE_MANAGER_GET_SESSION_TOTAL_TIME, get_session_total_time as *const () as usize);
            let avatar = h.cars[0];
            // physicsInfo.tyreCompounds: only its length is read
            let count = script().pit.compounds.len();
            let compounds = a.alloc(count.max(1) * 0x20);
            wr(avatar, 0xf18, compounds);
            wr(avatar, 0xf20, compounds.add(count * 0x20));
            wr(avatar, 0xf28, compounds.add(count * 0x20));
            // raceEngineer: an object whose second word is the physics car
            let engineer = a.alloc(0x40);
            wr(engineer, 8, rd::<*mut u8>(avatar, 0x1168));
            wr(avatar, 0xf80, engineer);
            sync(h);
            let file = "task30_setup.ini";
            std::fs::write(file, if text == "small" { crate::gui_seq_menu::PIT_SETUP_INI_SMALL } else { crate::gui_seq_menu::PIT_SETUP_INI }).map_err(|e| format!("{file}: {e}"))?;
            let ini = a.alloc(0x200);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_INI_READER_CTOR));
            ctor(ini, wstring(a, file));
            let menu = a.alloc(0x190);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_QUICK_MENU_CTOR));
            ctor(menu, sim);
            let build: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(a.va(VA_QUICK_MENU_BUILD_CONTROLS));
            build(menu, ini);
            PIT.lock().unwrap().menu = menu as usize;
        }
        "menu.pitKey" => {
            sync(h);
            let menu = PIT.lock().unwrap().menu as *mut u8;
            let send: extern "C" fn(*mut u8, u32) = std::mem::transmute(a.va(VA_QUICK_MENU_SEND_INPUT));
            send(menu, number as u32);
        }
        "menu.pitStop" => {
            let stop = a.alloc(0x240);
            wr(stop, 0x8, rd::<*mut u8>(sim, 0x8));
            wr(stop, 0x30, 1u8);
            wr(stop, 0xd0, sim);
            wr(stop, 0xc8, h.cars[0]);
            let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(a.va(VA_RESOURCE_STORE_GET_TEXTURE));
            get_texture(rd(game.graphics, 0x180), stop.add(0x188), wstring(a, "content/gui/pitstop/operating_repair.png"), false);
            get_texture(rd(game.graphics, 0x180), stop.add(0x1b0), wstring(a, "content/gui/pitstop/operating_fuel.png"), false);
            get_texture(rd(game.graphics, 0x180), stop.add(0x1d8), wstring(a, "content/gui/pitstop/operating_tyres.png"), false);
            wr(stop, 0x120, script().pit.state);
            PIT.lock().unwrap().stop = stop as usize;
        }
        // (after `Hud("pit.state", …)`: the script's state into the object)
        "menu.pitState" => {
            let stop = PIT.lock().unwrap().stop as *mut u8;
            if !stop.is_null() {
                wr(stop, 0x120, script().pit.state);
            }
        }
        other => return Err(format!("the game side knows no pit call {other:?}")),
    }
    Ok(())
}

/// `QuickMenuPitstop::renderHUD` (the menu is made inside `ESCMenu`'s constructor: after the
/// message banner, before the wrong-way sign).
pub unsafe fn render_menu(game: &Game, h: &HudGame, dt: f32) {
    let menu = PIT.lock().unwrap().menu as *mut u8;
    if menu.is_null() {
        return;
    }
    sync(h);
    wr(h.sim, 0x188, h.more.camera);
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_QUICK_MENU_RENDER_HUD));
    render(menu, dt);
    wr(h.sim, 0x188, 0usize);
}

/// `PitStop::renderHUD` (between the proximity arrows and the name tags).
pub unsafe fn render_stop(game: &Game, h: &HudGame, dt: f32) {
    let stop = PIT.lock().unwrap().stop as *mut u8;
    if stop.is_null() {
        return;
    }
    sync(h);
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_PIT_STOP_RENDER_HUD));
    render(stop, dt);
}

pub unsafe fn dump(game: &Game, out: &mut String) {
    let (menu, stop) = {
        let p = PIT.lock().unwrap();
        (p.menu as *const u8, p.stop as *const u8)
    };
    if !menu.is_null() {
        let (begin, end): (*const u8, *const u8) = (rd(menu, 0x78), rd(menu, 0x80));
        let mut presets = Vec::new();
        if !begin.is_null() {
            for p in 0..end.offset_from(begin) as usize / 0x18 {
                let (from, to): (*const f32, *const f32) = (rd(begin.add(p * 0x18), 0), rd(begin.add(p * 0x18), 8));
                let count = if from.is_null() { 0 } else { to.offset_from(from) as usize };
                presets.push((0..count).map(|i| *from.add(i)).collect::<Vec<f32>>());
            }
        }
        out.push_str(&crate::gui_seq_menu::pit_line(rd::<u8>(menu, 0xe4) != 0, rd(menu, 0xe0), rd(menu, 0xec), rd(menu, 0xf0), rd(menu, 0xe8), &presets));
        let root: *const u8 = rd(menu, 0x98);
        if !root.is_null() {
            crate::gui_ac::dump_control(game, root, 1, out);
        }
    }
    if !stop.is_null() {
        out.push_str(&format!("pitstop state={}\n", rd::<i32>(stop, 0x120)));
    }
}
