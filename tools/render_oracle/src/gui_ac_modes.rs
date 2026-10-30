// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's game modes (Task 45): `TimeAttack`'s HUD.
//!
//! The mode is a hand-made block with the game's vtable (so `getGameObject<TimeAttack>` of the
//! end screen finds it among the `Sim`'s children), its own font and textures as its
//! constructor makes them, and the members the script writes. The track the HUD asks for its
//! sectors is the HUD's stand-in, given a sector list for the mode's calls only.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ac::{rd, wr, write_wstring, wstring, Game};
use crate::gui_ac::read_wstring;
use crate::gui_ac_hud::HudGame;

const VA_TA_VTABLE: usize = 0x1_404d_c990;
const VA_TA_RENDER_HUD: usize = 0x1_401b_da40;
const VA_FONT_CTOR: usize = 0x1_4020_0bb0; // Font::Font(eFontType, float, bool, bool)
const VA_RESOURCE_STORE_GET_TEXTURE: usize = 0x1_4020_00d0;
const TA_SIZE: usize = 0x2e0;
const TA_FONT: usize = 0xe0;
const TA_TEXTURES: [(usize, &str); 7] = [(0xf0, "timeattack_start"), (0x118, "checkpoint"), (0x140, "time_icon"), (0x168, "points_icon"), (0x190, "small_time_icon"), (0x1b8, "small_points_icon"), (0x1e0, "penalty_icon")];
const TA_CAR: usize = 0x240;
const TA_TARGET: usize = 0x248;
const TA_STATUS: usize = 0x278;
const TA_GUI_INFO: usize = 0x2c0;
const TRACK_SECTORS: usize = 0xf8 + 0x130;

static TA: AtomicUsize = AtomicUsize::new(0);
static SECTORS: AtomicUsize = AtomicUsize::new(0);

/// The members a script names: (offset, float?).
pub fn member(name: &str) -> Option<(usize, bool)> {
    Some(match name {
        "mode" => (TA_STATUS + 0x40, false),
        "sectorIndex" => (TA_STATUS + 4, false),
        "timeLeft" => (TA_STATUS + 8, true),
        "points" => (TA_STATUS + 0xc, true),
        "carDamage" => (TA_STATUS + 0x10, true),
        "scoreLabelTimer" => (TA_GUI_INFO + 4, true),
        "penaltyPoints" => (TA_GUI_INFO + 8, true),
        "penaltyTimer" => (TA_GUI_INFO + 0xc, true),
        "gainedPoints" => (TA_GUI_INFO + 0x10, true),
        "gainedSeconds" => (TA_GUI_INFO + 0x14, true),
        "lastCarDamage" => (TA_GUI_INFO + 0x18, true),
        _ => return None,
    })
}

pub fn line(mode: i32, timers: [f32; 6]) -> String {
    let [score, penalty_points, penalty_timer, gained_points, gained_seconds, last_damage] = timers.map(f32::to_bits);
    format!("hud timeattack mode={mode} scoreLabel={score:08x} penaltyPoints={penalty_points:08x} penaltyTimer={penalty_timer:08x} gained={gained_points:08x},{gained_seconds:08x} lastDamage={last_damage:08x}\n")
}

/// `ta.create <sectors>` (0: a track without sectors), `ta.<member> <value>`.
pub unsafe fn call(game: &Game, h: &mut HudGame, what: &str, number: f32) -> Result<(), String> {
    let a = &game.acs;
    if what == "ta.create" {
        let ta = a.alloc(TA_SIZE);
        std::ptr::write_bytes(ta, 0, TA_SIZE);
        wr(ta, 0, a.va(VA_TA_VTABLE));
        wr(ta, 8, rd::<*mut u8>(h.sim, 0x8)); // GameObject::game
        let font = a.alloc(0x40);
        let ctor: extern "C" fn(*mut u8, i32, f32, bool, bool) -> *mut u8 = std::mem::transmute(a.va(VA_FONT_CTOR));
        ctor(font, 1, 45.0, false, false);
        wr(font, 4, 1u8);
        wr(ta, TA_FONT, font);
        let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(a.va(VA_RESOURCE_STORE_GET_TEXTURE));
        for (at, name) in TA_TEXTURES {
            get_texture(rd(game.graphics, 0x180), ta.add(at), wstring(a, &format!("content/gui/timeAttack/{name}.png")), false);
        }
        wr(ta, TA_CAR, h.cars[0]);
        let sectors = number as usize;
        let budget = a.alloc(sectors * 4 + 4);
        for i in 0..sectors {
            wr(budget, i * 4, 30.0f32);
        }
        wr(ta, TA_TARGET, budget);
        wr(ta, TA_TARGET + 8, budget.add(sectors * 4));
        wr(ta, TA_TARGET + 0x10, budget.add(sectors * 4));
        wr(ta, TA_STATUS + 0x40, 2i32);
        wr(ta, TA_GUI_INFO + 4, 1000.0f32);
        // among the Sim's children (the list has room for four)
        let (begin, end): (*mut usize, *mut usize) = (rd(h.sim, 0x40), rd(h.sim, 0x48));
        if end.offset_from(begin) >= 4 {
            return Err("no room for the mode among the Sim's children".into());
        }
        *end = ta as usize;
        wr(h.sim, 0x48, end.add(1));
        let positions = a.alloc(sectors * 4 + 4);
        SECTORS.store(positions as usize | sectors << 48, Ordering::SeqCst);
        TA.store(ta as usize, Ordering::SeqCst);
        return Ok(());
    }
    let ta = TA.load(Ordering::SeqCst) as *mut u8;
    if ta.is_null() {
        return Err("no time attack (ta.create first)".into());
    }
    match member(what.trim_start_matches("ta.")) {
        Some((at, true)) => wr(ta, at, number),
        Some((at, false)) => wr(ta, at, number as i32),
        None => return Err(format!("the game side knows no time attack member {what:?}")),
    }
    Ok(())
}

/// `TimeAttack::renderHUD`, with the track's sectors there for the call.
pub unsafe fn render_hud(game: &Game, h: &HudGame, dt: f32) {
    let ta = TA.load(Ordering::SeqCst) as *mut u8;
    if ta.is_null() {
        return;
    }
    let packed = SECTORS.load(Ordering::SeqCst);
    let (positions, sectors) = ((packed & 0xffff_ffff_ffff) as *mut f32, packed >> 48);
    let track: *mut u8 = rd(h.sim, 0x150);
    wr(track, TRACK_SECTORS, positions);
    wr(track, TRACK_SECTORS + 8, positions.add(sectors));
    wr(track, TRACK_SECTORS + 0x10, positions.add(sectors));
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_TA_RENDER_HUD));
    render(ta, dt);
    std::ptr::write_bytes(track.add(TRACK_SECTORS), 0, 0x18);
}

pub unsafe fn dump(out: &mut String) {
    let ta = TA.load(Ordering::SeqCst) as *const u8;
    if ta.is_null() {
        return;
    }
    let g = TA_GUI_INFO;
    out.push_str(&line(rd(ta, TA_STATUS + 0x40), [rd(ta, g + 4), rd(ta, g + 8), rd(ta, g + 0xc), rd(ta, g + 0x10), rd(ta, g + 0x14), rd(ta, g + 0x18)]));
}

// DriftMode's HUD: the mode's logic runs in its renderHUD, so the dump has every member the
// frame moved; the car's drift numbers are the script's, written into the car's physics copy.

const VA_DRIFT_VTABLE: usize = 0x1_404b_3848;
const VA_DRIFT_RENDER_HUD: usize = 0x1_4008_b7c0;
const VA_SET_GENTLE_STOP: usize = 0x1_400d_a280;
const VA_REGISTER_TO_FADE_IN: usize = 0x1_4003_3ce0;
const VA_FADE_IN: usize = 0x1_4003_3690;
const DRIFT_SIZE: usize = 0x1248;
const DRIFT_TEXTURES: [(usize, &str); 8] = [(0xe8, "star"), (0x110, "time_icon"), (0x138, "points_icon"), (0x160, "instantDrift"), (0x188, "instantDriftExtreme"), (0x1b0, "levelup"), (0x1d8, "combo"), (0x200, "drift_start")];
/// CarAvatar's physics copy: speed, driftPoints, instantDrift, driftComboCounter, driftBonusOn
const CA_SPEED: usize = 0x4d8;
const CA_DRIFT: [usize; 4] = [0x988, 0x98c, 0x994, 0x998];

static DRIFT: AtomicUsize = AtomicUsize::new(0);
/// speed, points, instant, combo, bonus
static DRIFT_CAR: std::sync::Mutex<[f32; 5]> = std::sync::Mutex::new([0.0; 5]);

unsafe extern "C" fn first(a: usize) -> usize {
    a
}

/// The car inputs a script names (index into `DRIFT_CAR`), then the mode's own members.
pub fn drift_input(name: &str) -> Option<usize> {
    ["speed", "points", "instant", "combo", "bonus"].iter().position(|n| *n == name)
}

pub fn drift_member(name: &str) -> Option<(usize, bool)> {
    Some(match name {
        "mode" => (0xd8, false),
        "seconds" => (0x288, true),
        "level" => (0x28c, false),
        "session" => (0x290, true),
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn drift_line(mode: i32, floats: [f32; 6], ints: [i32; 5], bonuses: [(f32, String); 2]) -> String {
    let [combo, level_up, old, seconds, session, per_level] = floats.map(f32::to_bits);
    let [last_time, level, max_points, last_points, last_seconds] = ints;
    let [(ty, tt), (py, pt)] = bonuses;
    format!(
        "hud drift mode={mode} combo={combo:08x} levelUp={level_up:08x} old={old:08x} seconds={seconds:08x} session={session:08x} perLevel={per_level:08x} lastTime={last_time} level={level} maxPoints={max_points} last={last_points},{last_seconds} bonusTime={:08x} {tt:?} bonusPoints={:08x} {pt:?}\n",
        ty.to_bits(),
        py.to_bits()
    )
}

/// `dr.create`, `dr.<car input> <value>`, `dr.<member> <value>`.
pub unsafe fn drift_call(game: &Game, h: &mut HudGame, what: &str, number: f32) -> Result<(), String> {
    let a = &game.acs;
    let name = what.trim_start_matches("dr.");
    if name == "create" {
        let d = a.alloc(DRIFT_SIZE);
        std::ptr::write_bytes(d, 0, DRIFT_SIZE);
        wr(d, 0, a.va(VA_DRIFT_VTABLE));
        wr(d, 8, rd::<*mut u8>(h.sim, 0x8));
        wr(d, 0xc0, 1u8); // GameMode::valid
        wr(d, 0xc4, -1i32); // GameMode::currentTier
        let font = a.alloc(0x40);
        let ctor: extern "C" fn(*mut u8, i32, f32, bool, bool) -> *mut u8 = std::mem::transmute(a.va(VA_FONT_CTOR));
        ctor(font, 1, 35.0, false, false);
        wr(font, 4, 1u8);
        wr(d, 0xc8, font);
        wr(d, 0xd8, 2i32);
        wr(d, 0xe0, h.cars[0]);
        let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(a.va(VA_RESOURCE_STORE_GET_TEXTURE));
        for (at, name) in DRIFT_TEXTURES {
            get_texture(rd(game.graphics, 0x180), d.add(at), wstring(a, &format!("content/gui/drift/{name}.png")), false);
        }
        for at in [0x228 + 8, 0x250 + 8] {
            write_wstring(a, d.add(at), "");
        }
        wr(d, 0x288, 60.0f32);
        for i in 0..1000 {
            wr(d, 0x29c + i * 4, (200 + 100 * i) as f32);
        }
        for va in [VA_SET_GENTLE_STOP, VA_REGISTER_TO_FADE_IN, VA_FADE_IN] {
            crate::gui_ac_hud::jump(game, va, first as *const () as usize);
        }
        // among the Sim's children, for the end screen's getGameObject<DriftMode>
        let (begin, end): (*mut usize, *mut usize) = (rd(h.sim, 0x40), rd(h.sim, 0x48));
        if end.offset_from(begin) >= 4 {
            return Err("no room for the mode among the Sim's children".into());
        }
        *end = d as usize;
        wr(h.sim, 0x48, end.add(1));
        DRIFT.store(d as usize, Ordering::SeqCst);
        return Ok(());
    }
    let d = DRIFT.load(Ordering::SeqCst) as *mut u8;
    if d.is_null() {
        return Err("no drift mode (dr.create first)".into());
    }
    if let Some(i) = drift_input(name) {
        DRIFT_CAR.lock().unwrap()[i] = number;
        return Ok(());
    }
    match drift_member(name) {
        Some((at, true)) => wr(d, at, number),
        Some((at, false)) => wr(d, at, number as i32),
        None => return Err(format!("the game side knows no drift member {what:?}")),
    }
    Ok(())
}

pub unsafe fn drift_render_hud(game: &Game, h: &HudGame, dt: f32) {
    let d = DRIFT.load(Ordering::SeqCst) as *mut u8;
    if d.is_null() {
        return;
    }
    let [speed, points, instant, combo, bonus] = *DRIFT_CAR.lock().unwrap();
    let car = h.cars[0];
    wr(car, CA_SPEED, speed);
    wr(car, CA_DRIFT[0], points);
    wr(car, CA_DRIFT[1], instant);
    wr(car, CA_DRIFT[2], combo as i32);
    wr(car, CA_DRIFT[3], (bonus != 0.0) as u8);
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_DRIFT_RENDER_HUD));
    render(d, dt);
}

pub unsafe fn drift_dump(out: &mut String) {
    let d = DRIFT.load(Ordering::SeqCst) as *const u8;
    if d.is_null() {
        return;
    }
    let bonus = |at: usize| (rd::<f32>(d, at), read_wstring(d.add(at + 8)));
    out.push_str(&drift_line(
        rd(d, 0xd8),
        [rd(d, 0x27c), rd(d, 0x280), rd(d, 0x284), rd(d, 0x288), rd(d, 0x290), rd(d, 0x298)],
        [rd(d, 0x278), rd(d, 0x28c), rd(d, 0x294), rd(d, 0x123c), rd(d, 0x1240)],
        [bonus(0x228), bonus(0x250)],
    ));
}

// DragMode: a hand-made 0x2f0 block, two `DragStatus` (the HUD's two cars), the strip's
// spline built as the constructor builds it, from (0,0,0) to (400,0,0).

const VA_DRAG_VTABLE: usize = 0x1_404b_31e0;
const VA_DRAG_RENDER_HUD: usize = 0x1_4008_8730;
const VA_SPLINE_CTOR: usize = 0x1_401e_ecd0;
const VA_SPLINE_ADD_POINT: usize = 0x1_401e_f700;
const VA_SPLINE_LENGTH: usize = 0x1_401f_1db0;
const DRAG_SIZE: usize = 0x2f0;
const DRAG_TEXTURES: [(usize, &str); 8] = [(0x130, "xmas_tree"), (0x158, "time_icon"), (0x180, "player_car"), (0x1a8, "ai_car"), (0x1d0, "race_straightline"), (0x1f8, "speed_icon"), (0x220, "false_start"), (0x248, "disqualified")];
/// CarAvatar's physics state position, which the HUD places the cars by
const CA_STATE_POSITION: usize = 0x29c;
pub const DRAG_LENGTH: f32 = 400.0;

static DRAG: AtomicUsize = AtomicUsize::new(0);
static DRAG_X: std::sync::Mutex<[f32; 2]> = std::sync::Mutex::new([0.0; 2]);

/// A script's name for a drag member: (car, offset in `DragStatus`, kind) with kind 0 u8,
/// 1 u32/i32, 2 f32; car `None` is the mode's status.
pub fn drag_member(name: &str) -> Option<(Option<usize>, usize, u8)> {
    let (field, car) = match name.strip_suffix(['0', '1']) {
        Some(field) => (field, Some(name.ends_with('1') as usize)),
        None => (name, Some(0)),
    };
    Some(match field {
        "status" => (None, 0xf0, 1),
        "false" => (car, 0, 0),
        "disq" => (car, 1, 0),
        "wins" => (car, 4, 1),
        "reaction" => (Some(0), 0x18, 2),
        "time" => (Some(0), 0x1c, 2),
        "speed" => (Some(0), 0x20, 1),
        _ => return None,
    })
}

pub fn drag_line(status: i32, perc: [f32; 2]) -> String {
    format!("hud drag status={status} perc={:08x},{:08x}\n", perc[0].to_bits(), perc[1].to_bits())
}

/// `dg.create`, `dg.x0`/`dg.x1 <metres along the strip>`, `dg.<member> <value>`.
pub unsafe fn drag_call(game: &Game, h: &mut HudGame, what: &str, number: f32) -> Result<(), String> {
    let a = &game.acs;
    let name = what.trim_start_matches("dg.");
    if name == "create" {
        if h.cars.len() < 2 {
            return Err("a drag race needs two cars".into());
        }
        let d = a.alloc(DRAG_SIZE);
        std::ptr::write_bytes(d, 0, DRAG_SIZE);
        wr(d, 0, a.va(VA_DRAG_VTABLE));
        wr(d, 8, rd::<*mut u8>(h.sim, 0x8));
        wr(d, 0xc0, 1u8);
        wr(d, 0xc4, -1i32);
        let cars = a.alloc(0x90);
        std::ptr::write_bytes(cars, 0, 0x90);
        for i in 0..2 {
            wr(cars, i * 0x48 + 8, h.cars[i]);
            wr(cars, i * 0x48 + 0x2c, f32::MAX);
        }
        wr(d, 0xd8, cars);
        wr(d, 0xe0, cars.add(0x90));
        wr(d, 0xe8, cars.add(0x90));
        wr(d, 0xf0, 3i32);
        wr(d, 0xf8, h.sim);
        let font = a.alloc(0x40);
        let ctor: extern "C" fn(*mut u8, i32, f32, bool, bool) -> *mut u8 = std::mem::transmute(a.va(VA_FONT_CTOR));
        ctor(font, 1, 45.0, false, false);
        wr(d, 0x100, font);
        let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(a.va(VA_RESOURCE_STORE_GET_TEXTURE));
        for (at, name) in DRAG_TEXTURES {
            get_texture(rd(game.graphics, 0x180), d.add(at), wstring(a, &format!("content/gui/drag/{name}.png")), false);
        }
        let spline_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_SPLINE_CTOR));
        let add_point: extern "C" fn(*mut u8, *const [f32; 3], i32) = std::mem::transmute(a.va(VA_SPLINE_ADD_POINT));
        let length: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_SPLINE_LENGTH));
        let spline = d.add(0x270);
        spline_ctor(spline);
        wr(spline, 0x6c, 0i32); // eLinear
        for i in 0..=100u32 {
            let t = i as f32 * f32::from_bits(0x3c23_d70a);
            add_point(spline, &[DRAG_LENGTH * t, 0.0, 0.0], 0);
        }
        length(spline);
        // PauseMenu::blankButton(sim->pauseMenu, ePauseMenuPits, true)
        let pause: *mut u8 = rd(h.sim, 0x110);
        if !pause.is_null() {
            let blank: extern "C" fn(*mut u8, i32, bool) = std::mem::transmute(a.va(0x1_4011_f400));
            blank(pause, 1, true);
        }
        let (begin, end): (*mut usize, *mut usize) = (rd(h.sim, 0x40), rd(h.sim, 0x48));
        if end.offset_from(begin) >= 4 {
            return Err("no room for the mode among the Sim's children".into());
        }
        *end = d as usize;
        wr(h.sim, 0x48, end.add(1));
        DRAG.store(d as usize, Ordering::SeqCst);
        return Ok(());
    }
    let d = DRAG.load(Ordering::SeqCst) as *mut u8;
    if d.is_null() {
        return Err("no drag mode (dg.create first)".into());
    }
    if let Some(i) = ["x0", "x1"].iter().position(|n| *n == name) {
        DRAG_X.lock().unwrap()[i] = number;
        return Ok(());
    }
    let (car, at, kind) = drag_member(name).ok_or_else(|| format!("the game side knows no drag member {what:?}"))?;
    let at = match car {
        Some(i) => rd::<*mut u8>(d, 0xd8).add(i * 0x48 + at),
        None => d.add(at),
    };
    match kind {
        0 => wr(at, 0, (number != 0.0) as u8),
        1 => wr(at, 0, number as i32),
        _ => wr(at, 0, number),
    }
    Ok(())
}

pub unsafe fn drag_render_hud(game: &Game, h: &HudGame, dt: f32) {
    let d = DRAG.load(Ordering::SeqCst) as *mut u8;
    if d.is_null() {
        return;
    }
    // the places only for the drag HUD: the proximity arrows read the same field after it
    let x = *DRAG_X.lock().unwrap();
    let kept: [[f32; 3]; 2] = [rd(h.cars[0], CA_STATE_POSITION), rd(h.cars[1], CA_STATE_POSITION)];
    for (i, x) in x.into_iter().enumerate() {
        wr(h.cars[i], CA_STATE_POSITION, [x, 0.0, 0.0]);
    }
    let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_DRAG_RENDER_HUD));
    render(d, dt);
    for (i, p) in kept.into_iter().enumerate() {
        wr(h.cars[i], CA_STATE_POSITION, p);
    }
}

pub unsafe fn drag_dump(out: &mut String) {
    let d = DRAG.load(Ordering::SeqCst) as *const u8;
    if d.is_null() {
        return;
    }
    out.push_str(&drag_line(rd(d, 0xf0), [rd(d, 0x2e0), rd(d, 0x2e4)]));
}
