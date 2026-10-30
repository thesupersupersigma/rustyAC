// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's game modes (Task 45): `TimeAttack`'s HUD.
//!
//! The mode is a hand-made block with the game's vtable (so `getGameObject<TimeAttack>` of the
//! end screen finds it among the `Sim`'s children), its own font and textures as its
//! constructor makes them, and the members the script writes. The track the HUD asks for its
//! sectors is the HUD's stand-in, given a sector list for the mode's calls only.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ac::{rd, wr, wstring, Game};
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
