// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's HUD, part three: the objects that follow the cars.
//!
//! `ProximityIndicator`, `DriverNameDisplayer` (with its `CarLabel`s) and `DamageDisplayer`
//! are built by the game's own constructors. A car gets a `Node` for its body (the game's own
//! class, with the script's matrix) and a block for the node whose "shown" byte says it is
//! connected; the scene camera is a block with only its matrix, and the two matrices
//! `DriverNameDisplayer::update` would take from it are written into the object.

use rustyac_gui::hud::HudInput;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::ac::{rd, wr, write_wstring, Game};
use crate::gui_ac_hud::{acs, input, jump, HudGame};

const VA_RACE_MANAGER_GET_LAP_COUNT: usize = 0x1_4013_8570;
const VA_RACE_MANAGER_GET_CAR_REAL_TIME_POSITION: usize = 0x1_4013_7f90;
const VA_AC_CLIENT_GET_PING: usize = 0x1_4004_6f30;
const VA_NODE_VTABLE: usize = 0x1_404e_b558;
const VA_PROXIMITY_CTOR: usize = 0x1_4012_3880;
const VA_PROXIMITY_RENDER_HUD: usize = 0x1_4012_3a80;
const VA_NAMES_CTOR: usize = 0x1_4008_c7e0;
const VA_NAMES_RENDER_HUD: usize = 0x1_4008_d900;
const VA_DAMAGE_CTOR: usize = 0x1_4007_8c10;
const VA_DAMAGE_RENDER_HUD: usize = 0x1_4007_ae80;
const VA_DAMAGE_SHOW: usize = 0x1_4007_b7f0;

pub struct Cars {
    pub proximity: *mut u8,
    pub names: *mut u8,
    pub damage: *mut u8,
    pub scene_camera: *mut u8,
    pub client: *mut u8,
}

impl Default for Cars {
    fn default() -> Cars {
        let n = std::ptr::null_mut();
        Cars { proximity: n, names: n, damage: n, scene_camera: n, client: n }
    }
}

fn car_of(car: *const u8) -> usize {
    unsafe { rd::<i32>(car, 0x1158) as usize }
}

unsafe extern "C" fn get_lap_count(_this: *mut u8, car: *mut u8) -> i32 {
    let index = car_of(car);
    input(|i| i.cars.get(index).map(|c| c.lap_count).unwrap_or(0))
}

unsafe extern "C" fn get_car_real_time_position(_this: *mut u8, car: *mut u8) -> i32 {
    let index = car_of(car);
    input(|i| i.cars.get(index).map(|c| c.real_time_position).unwrap_or(0))
}

unsafe extern "C" fn get_ping(_this: *mut u8, car: *mut u8) -> i32 {
    if car.is_null() {
        // (no car: the player's own ping to the server)
        return crate::gui_seq_menu::script().own_ping;
    }
    let index = car_of(car);
    input(|i| i.cars.get(index).map(|c| c.ping).unwrap_or(-1))
}

pub unsafe fn write_matrix(at: *mut u8, m: &Mat44f) {
    for (i, row) in m.m.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            wr(at, (i * 4 + j) * 4, *v);
        }
    }
}

pub unsafe fn sync(h: &HudGame, state: &HudInput) {
    let c = &h.more.cars;
    if c.names.is_null() {
        return;
    }
    write_matrix(c.scene_camera.add(0xc), &state.camera_matrix);
    let (view, perspective) = crate::gui_seq::hud_view(&state.camera_matrix);
    write_matrix(c.names.add(0xb8), &state.camera_matrix);
    write_matrix(c.names.add(0xf8), &xm_matrix_multiply(&view, &perspective));
    wr(rd::<*mut u8>(h.sim, 0x8), 0x18, state.game_time_now);
    for (avatar, car) in h.cars.iter().zip(&state.cars) {
        let avatar = *avatar;
        for k in 0..3 {
            wr(avatar, 0x29c + k * 4, car.world_position[k]);
        }
        write_matrix(rd::<*mut u8>(avatar, 0x1b8).add(0x8), &car.body_matrix);
        wr(rd::<*mut u8>(avatar, 0x210), 0xd8, car.connected as u8);
        if crate::gui_ac::read_wstring(avatar.add(0x1118)) != car.nation_code {
            write_wstring(acs(), avatar.add(0x1118), &car.nation_code);
        }
        for k in 0..4 {
            wr(avatar, 0x9c4 + k * 4, car.damage_zone_level[k]);
            wr(avatar, 0xa44 + k * 4, car.tyre_inflation[k]);
            wr(avatar, 0xa60 + k * 4, car.sus_damage[k]);
            wr(avatar, 0xa70 + k * 4, car.tyre_flat_spot[k]);
        }
        wr(avatar, 0xa10, car.engine_life_left);
        wr(avatar, 0xa5c, car.gearbox);
    }
}

unsafe fn construct(game: &Game, size: usize, ctor: usize, argument: *mut u8) -> *mut u8 {
    let object = game.acs.alloc(size);
    let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(game.acs.va(ctor));
    ctor(object, argument);
    object
}

pub unsafe fn create(game: &Game, _gui: *mut u8, _screen: *mut u8, h: &mut HudGame) -> Result<(), String> {
    let a = &game.acs;
    let sim = h.sim;
    jump(game, VA_RACE_MANAGER_GET_LAP_COUNT, get_lap_count as *const () as usize);
    jump(game, VA_RACE_MANAGER_GET_CAR_REAL_TIME_POSITION, get_car_real_time_position as *const () as usize);
    jump(game, VA_AC_CLIENT_GET_PING, get_ping as *const () as usize);
    // Sim::flagMapCache: an empty std::map (its head node points at itself)
    let head = a.alloc(0x80);
    for at in [0, 8, 16] {
        wr(head, at, head);
    }
    wr(head, 24, 1u8);
    wr(head, 25, 1u8);
    wr(sim, 0x2c0, head);
    wr(sim, 0x2c8, 0usize);
    for (i, avatar) in h.cars.iter().enumerate() {
        let avatar = *avatar;
        wr(avatar, 0x1158, i as i32);
        std::ptr::write_bytes(avatar.add(0x1118), 0, 0x20);
        wr(avatar, 0x1118 + 0x18, 7usize);
        let body = a.alloc(0x200);
        wr(body, 0, a.va(VA_NODE_VTABLE));
        write_matrix(body.add(0x8), &Mat44f::IDENTITY);
        wr(avatar, 0x1b8, body);
        wr(avatar, 0x210, a.alloc(0x100));
    }
    input(|i| {
        if i.camera_matrix.m[3][3] == 0.0 {
            i.camera_matrix = Mat44f::IDENTITY;
        }
    });
    let c = &mut h.more.cars;
    c.proximity = construct(game, 0x60, VA_PROXIMITY_CTOR, sim);
    c.names = construct(game, 0x138, VA_NAMES_CTOR, sim);
    c.damage = construct(game, 0xb0, VA_DAMAGE_CTOR, sim);
    c.scene_camera = a.alloc(0x100);
    wr(sim, 0x238, c.scene_camera);
    c.client = a.alloc(0x100);
    let state = input(|i| i.clone());
    sync(h, &state);
    Ok(())
}

pub unsafe fn call(game: &Game, _gui: *mut u8, h: &mut HudGame, what: &str, number: f32, _text: &str) -> Result<(), String> {
    let c = &h.more.cars;
    match what {
        "namesActive" => wr(c.names, 0x30, (number != 0.0) as u8),
        "proximityActive" => wr(c.proximity, 0x30, (number != 0.0) as u8),
        "damageEnabled" => wr(c.damage, 0x58, (number != 0.0) as u8),
        "damageShow" => {
            let show: extern "C" fn(*mut u8) = std::mem::transmute(game.acs.va(VA_DAMAGE_SHOW));
            show(c.damage);
        }
        other => return Err(format!("the game side knows no HUD call {other:?}")),
    }
    Ok(())
}

pub unsafe fn update(_game: &Game, _h: &mut HudGame, _dt: f32) {}

unsafe fn render(game: &Game, object: *mut u8, va: usize, dt: f32) {
    if rd::<u8>(object, 0x30) != 0 {
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(va));
        render(object, dt);
    }
}

/// The three objects in the order `Sim::Sim` made them (between the banners and the flags).
pub unsafe fn render_hud(game: &Game, h: &mut HudGame, dt: f32) {
    let c = &h.more.cars;
    if c.names.is_null() {
        return;
    }
    // (the camera manager and the client are only there for these calls)
    wr(h.sim, 0x188, h.more.camera);
    if input(|i| i.online) {
        wr(h.sim, 0x1c8, c.client);
    }
    render(game, c.proximity, VA_PROXIMITY_RENDER_HUD, dt);
    wr(h.sim, 0x188, 0usize);
    wr(h.sim, 0x1c8, 0usize);
    crate::gui_ac_pit::render_stop(game, h, dt);
    wr(h.sim, 0x188, h.more.camera);
    if input(|i| i.online) {
        wr(h.sim, 0x1c8, c.client);
    }
    render(game, c.names, VA_NAMES_RENDER_HUD, dt);
    render(game, c.damage, VA_DAMAGE_RENDER_HUD, dt);
    wr(h.sim, 0x188, 0usize);
    wr(h.sim, 0x1c8, 0usize);
    crate::gui_ac_menu::render_end(game, h, dt);
}

pub unsafe fn dump(game: &Game, h: &HudGame, out: &mut String) {
    let c = &h.more.cars;
    if c.names.is_null() {
        return;
    }
    let (begin, end): (*const *const u8, *const *const u8) = (rd(c.names, 0x98), rd(c.names, 0xa0));
    if !begin.is_null() {
        for i in 0..end.offset_from(begin) as usize {
            let mut block = String::new();
            crate::gui_ac::dump_control(game, *begin.add(i), 1, &mut block);
            out.push_str(&crate::gui_seq_menu::tag_names(i, block));
        }
    }
    let (begin, end): (*const u8, *const u8) = (rd(c.damage, 0x68), rd(c.damage, 0x70));
    let mut items = Vec::new();
    if !begin.is_null() {
        for i in 0..end.offset_from(begin) as usize / 0x50 {
            items.push((rd::<f32>(begin.add(i * 0x50), 0x48), rd::<f32>(begin.add(i * 0x50), 0x4c)));
        }
    }
    out.push_str(&crate::gui_seq::damage_line(rd(c.damage, 0x88), rd::<u8>(c.damage, 0x84) != 0, rd::<u8>(c.damage, 0x58) != 0, &items));
}
