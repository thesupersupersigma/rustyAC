// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `OverlayLeaderboard` (0xf0 bytes): the strip of places along the bottom of the screen (or
//! the column at its left), with the leader's time and the gaps of the cars around the player.
//! F9 turns it on and steps through its faces.
//!
//! `OverlayLeaderboard::OverlayLeaderboard` 0x1401bf440, `update` 0x1401c1f20, `renderHUD`
//! 0x1401c1350, `drawCell` 0x1401c0050, `drawVerticalCell` 0x1401c06b0, `filterName`
//! 0x1401c0d50, `filterRemovingFromSequence` 0x1401c1190, `timeToDiffShort` 0x1401c1cb0,
//! `timeToDiff_Secs_3Millis` 0x1401c1df0, `shutdown` 0x1401c1a30; `Trigger::
//! ignoreSubsequentTrue` 0x14023b3a0.
//!
//! Not ported: the row of micro-sector boxes under a cell (`MicroSectors::drawSectors`, with
//! `gameplay.ini [GUI] SHOW_SECTORS`): `maxSectorsSize` stays 0.

use rustyac_physics::data::ini::IniReader;

use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::leaderboard::RaceEntry;
use crate::painter::{Painter, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GRAPHICS_GL};
use crate::widgets2::format_fixed;

/// `OverlayLeaderboard::GapMode`.
pub const GAP_OFF: i32 = 0;
pub const GAP_FROM_FIRST: i32 = 1;
pub const GAP_FROM_PREVIOUS: i32 = 2;
pub const GAP_ALTERNATE: i32 = 3;

pub struct OverlayLeaderboard {
    pub gap_display_mode: i32,
    /// `keySwitchMode`: the `Trigger`'s `lastState`
    key_was_down: bool,
    pub vertical_layout: bool,
    pub max_sectors_size: f32,
    pub max_timer: f32,
    pub back_color: [f32; 4],
    pub timer: f32,
    position_font: usize,
    name_font: usize,
    time_font: usize,
    /// `driverNames`: every car's name, and the three letters it is shown by
    pub driver_names: Vec<(String, String)>,
}

/// `cvttsd2si`.
fn cut(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `filterRemovingFromSequence` 0x1401c1190: what follows the first of `separators` in `s`,
/// when that is more than two characters.
fn filter_removing_from_sequence(s: &[u16], separators: &str) -> Vec<u16> {
    let separators: Vec<u16> = separators.encode_utf16().collect();
    if let Some(pos) = s.iter().position(|c| separators.contains(c)) {
        let rest = &s[pos + 1..];
        if rest.len() > 2 {
            return rest.to_vec();
        }
    }
    s.to_vec()
}

/// `timeToDiffShort` 0x1401c1cb0: seconds with one decimal.
pub fn time_to_diff_short(ms: i32) -> String {
    if ms > 0 {
        format_fixed((ms as f32 * f32::from_bits(0x3a83_126f)) as f64, 1)
    } else if ms == 0 {
        "---".to_string()
    } else {
        // of the magnitude: "-%0d.%0d"
        let a = ms.wrapping_abs();
        format!("-{}.{}", a / 1000, ((a - (a / 1000) * 1000) / 100).wrapping_abs())
    }
}

/// `timeToDiff_Secs_3Millis` 0x1401c1df0: seconds with three decimals.
pub fn time_to_diff_secs_3_millis(ms: i32) -> String {
    if ms > 0 {
        format_fixed((ms as f32 * f32::from_bits(0x3a83_126f)) as f64, 3)
    } else if ms == 0 {
        "---".to_string()
    } else {
        format!("{}.{}", ms / 1000, ((ms - (ms / 1000) * 1000) / 100).abs())
    }
}

impl OverlayLeaderboard {
    /// The constructor (the game makes the object with more than one car, outside a drag
    /// race). `gameplay` is `cfg/gameplay.ini`: `[OVERLAY_LEADERBOARD] ACTIVE`, `VERTICAL`.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, gameplay: Option<&IniReader>) -> OverlayLeaderboard {
        let mut font = |scale: f32, bold: bool| {
            gui.fonts.push(Font::new(painter, "", FontType::Monospaced, scale, false, bold));
            gui.fonts.len() - 1
        };
        let position_font = font(30.0, true);
        let name_font = font(19.0, true);
        let time_font = font(14.0, false);
        let mut overlay = OverlayLeaderboard { gap_display_mode: GAP_OFF, key_was_down: false, vertical_layout: false, max_sectors_size: 0.0, max_timer: 10.0, back_color: [0.08, 0.08, 0.08, 0.8], timer: 0.0, position_font, name_font, time_font, driver_names: Vec::new() };
        if let Some(ini) = gameplay.filter(|ini| ini.ready) {
            if ini.has_key("OVERLAY_LEADERBOARD", "ACTIVE") {
                overlay.gap_display_mode = ini.get_int("OVERLAY_LEADERBOARD", "ACTIVE").unwrap_or(0);
            }
            if ini.has_key("OVERLAY_LEADERBOARD", "VERTICAL") {
                overlay.vertical_layout = ini.get_int("OVERLAY_LEADERBOARD", "VERTICAL").unwrap_or(0) != 0;
            }
        }
        for name in gui.sim.race.cars.iter().map(|car| car.driver_name.clone()).collect::<Vec<_>>() {
            let short = overlay.filter_name(&name);
            overlay.driver_names.push((name, short));
        }
        overlay
    }

    /// `OverlayLeaderboard::filterName` 0x1401c0d50: three capital letters of the surname,
    /// changed until no car before has the same.
    pub fn filter_name(&self, name: &str) -> String {
        let mut a: Vec<u16> = name.encode_utf16().collect();
        for separators in [" ", ".", "'"] {
            a = filter_removing_from_sequence(&a, separators);
        }
        let mut r = a.clone();
        let mut index = 3;
        if r.len() > 3 {
            r.truncate(3);
        }
        for c in &mut r {
            if (b'a' as u16..=b'z' as u16).contains(c) {
                *c -= 32;
            }
        }
        let text = String::from_utf16_lossy(&r);
        if let Some((_, short)) = self.driver_names.iter().find(|(_, short)| *short == text) {
            let short: Vec<u16> = short.encode_utf16().collect();
            loop {
                if short.get(2) != r.get(2) || index >= a.len() {
                    break;
                }
                r[2] = a[index];
                index += 1;
            }
            if short == r && r.len() > 2 {
                r[2] = b'2' as u16;
            }
        }
        String::from_utf16_lossy(&r)
    }

    /// What F9 does (the body of `update` 0x1401c1f20 once the key's edge is there): the next
    /// face, and the words of the message "Overlay Leaderboard".
    pub fn switch_mode(&mut self) -> &'static str {
        match self.gap_display_mode {
            GAP_OFF => {
                self.gap_display_mode = GAP_FROM_FIRST;
                self.vertical_layout = false;
                "Showing difference from first"
            }
            GAP_FROM_FIRST => {
                self.gap_display_mode = GAP_FROM_PREVIOUS;
                "Showing difference from previous"
            }
            GAP_FROM_PREVIOUS => {
                self.gap_display_mode = GAP_ALTERNATE;
                "Alternate mode"
            }
            GAP_ALTERNATE => {
                if !self.vertical_layout {
                    self.gap_display_mode = GAP_FROM_FIRST;
                    self.vertical_layout = true;
                    "Showing difference from first"
                } else {
                    self.gap_display_mode = GAP_OFF;
                    self.vertical_layout = false;
                    "Off"
                }
            }
            _ => {
                self.gap_display_mode = GAP_ALTERNATE;
                "Alternate mode"
            }
        }
    }

    /// `OverlayLeaderboard::update` 0x1401c1f20, once a frame: `f9_down` is what
    /// `GetAsyncKeyState(VK_F9)` says. The words of a message to show, when the face changed.
    pub fn update(&mut self, f9_down: bool) -> Option<&'static str> {
        // Trigger::ignoreSubsequentTrue 0x14023b3a0
        let edge = f9_down && !self.key_was_down;
        if edge {
            self.key_was_down = true;
        }
        if !f9_down {
            self.key_was_down = false;
        }
        edge.then(|| self.switch_mode())
    }

    /// The gap text's colour: yellow while the gaps are those to the car ahead.
    fn time_color(&self, session: i32) -> [f32; 4] {
        if session == 3 && (self.gap_display_mode == GAP_FROM_PREVIOUS || (self.gap_display_mode == GAP_ALTERNATE && self.timer < 0.0)) {
            [1.0, 0.8, 0.0, 1.0]
        } else {
            [1.0; 4]
        }
    }

    /// A cell's driver: the three letters, made again when the name of the car's driver changed.
    fn short_name(&mut self, gui: &Gui, car: usize) -> String {
        let name = gui.sim.race.cars.get(car).map(|c| c.driver_name.clone()).unwrap_or_default();
        if self.driver_names.get(car).is_some_and(|(complete, _)| *complete != name) {
            let short = self.filter_name(&name);
            self.driver_names[car] = (name, short);
        }
        self.driver_names.get(car).map(|(_, short)| short.clone()).unwrap_or_default()
    }

    /// `OverlayLeaderboard::drawCell` 0x1401c0050: a cell of the strip, 170 wide and 40 high.
    #[allow(clippy::too_many_arguments)]
    fn draw_cell(&mut self, gui: &mut Gui, painter: &mut dyn Painter, entry: &RaceEntry, pos: i32, text: &str, x: f32, y: f32, session: i32) {
        let gl = GRAPHICS_GL;
        let x_a = x + 20.0;
        let x_b = (x + 40.0) + 10.0;
        let x_c = (x + 170.0) - 5.0;
        let y_b = y + 20.0;
        let is_player = entry.car == 0;
        let c = if is_player { [1.0; 4] } else { [0.6, 0.6, 0.6, 1.0] };
        painter.gl_color4f(gl, c[0], c[1], c[2], c[3]);
        painter.gl_quad(gl, x, y, 40.0, 40.0, false);
        gui.fonts[self.position_font].set_color(0.0, 0.0, 0.0, 1.0);
        gui.fonts[self.position_font].blit_string(painter, x_a, y, &pos.to_string(), 1.0, ALIGN_CENTER);
        let short = self.short_name(gui, entry.car);
        gui.fonts[self.name_font].set_color(1.0, if is_player { 0.8 } else { 1.0 }, if is_player { 0.0 } else { 1.0 }, 1.0);
        gui.fonts[self.name_font].blit_string(painter, x_b, y, &short, 1.0, ALIGN_LEFT);
        let color = self.time_color(session);
        gui.fonts[self.time_font].set_color(color[0], color[1], color[2], color[3]);
        let car = gui.sim.race.cars.get(entry.car).cloned().unwrap_or_default();
        if car.is_black_flagged {
            gui.fonts[self.time_font].blit_string(painter, x_b, y_b, &translate("Retired"), 1.0, ALIGN_LEFT);
            return;
        }
        let state = if car.in_pitlane { " PIT".to_string() } else { car.compound_short_name.clone() };
        gui.fonts[self.time_font].blit_string(painter, x_c, y_b, &state, 0.8, ALIGN_RIGHT);
        gui.fonts[self.time_font].blit_string(painter, x_b, y_b, text, 1.0, ALIGN_LEFT);
    }

    /// `OverlayLeaderboard::drawVerticalCell` 0x1401c06b0: a row of the column, 180 wide and
    /// 25 high.
    #[allow(clippy::too_many_arguments)]
    fn draw_vertical_cell(&mut self, gui: &mut Gui, painter: &mut dyn Painter, entry: &RaceEntry, pos: i32, text: &str, x: f32, y: f32, session: i32) {
        let gl = GRAPHICS_GL;
        let x_p = (x + 7.0) + 5.0;
        let y_t = y + 3.0;
        let x_n = (x + 25.0) + 7.0;
        let x_time = (x + 170.0) - 22.0;
        let x_cmp = (x + 170.0) + 5.0;
        let y_s = y + 5.0;
        let is_player = entry.car == 0;
        if pos & 1 == 0 {
            painter.gl_color4f(gl, 0.1, 0.1, 0.1, 0.6);
        } else {
            painter.gl_color4f(gl, 0.05, 0.05, 0.05, 0.8);
        }
        painter.gl_quad(gl, x, y, 25.0, 25.0, false);
        painter.gl_quad(gl, x + 26.0, y, 43.0, 25.0, false);
        painter.gl_quad(gl, x + 70.0, y, 110.0, 25.0, false);
        let short = self.short_name(gui, entry.car);
        gui.fonts[self.name_font].set_color(1.0, if is_player { 0.8 } else { 1.0 }, if is_player { 0.0 } else { 1.0 }, 1.0);
        gui.fonts[self.name_font].blit_string(painter, x_p, y_t, &pos.to_string(), 1.0, ALIGN_CENTER);
        gui.fonts[self.name_font].blit_string(painter, x_n, y_t, &short, 1.0, ALIGN_LEFT);
        let color = self.time_color(session);
        gui.fonts[self.time_font].set_color(color[0], color[1], color[2], color[3]);
        let car = gui.sim.race.cars.get(entry.car).cloned().unwrap_or_default();
        if !car.is_black_flagged {
            let state = if car.in_pitlane { " PIT".to_string() } else { car.compound_short_name.clone() };
            gui.fonts[self.time_font].blit_string(painter, x_cmp, y_s, &state, 0.8, ALIGN_RIGHT);
            gui.fonts[self.time_font].blit_string(painter, x_time, y_s, text, 1.0, ALIGN_RIGHT);
        } else {
            gui.fonts[self.time_font].blit_string(painter, x_time, y_s, "Retired", 1.0, ALIGN_RIGHT);
        }
    }

    /// `OverlayLeaderboard::renderHUD` 0x1401c1350. `results_displayed`: the end-of-session
    /// displayer is active; `player_position`: `RaceManager::getCarLeaderboardPosition` of
    /// the player's car (1 for the leader).
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, results_displayed: bool, player_position: i32) {
        let visible = |id: Option<usize>| id.is_some_and(|id| gui.controls[id].visible);
        if self.gap_display_mode == GAP_OFF || visible(gui.sim.esc_menu) || visible(gui.sim.pause_menu) || gui.sim.in_replay || results_displayed {
            return;
        }
        self.timer = self.timer - dt;
        if !(self.timer >= -self.max_timer) {
            self.timer = self.max_timer;
        }
        let entries: Vec<RaceEntry> = gui.sim.race.leaderboard.iter().filter(|e| gui.sim.race.cars.get(e.car).is_some_and(|car| car.connected)).copied().collect();
        if entries.is_empty() {
            // (the game reads the first entry whether it is there or not)
            return;
        }
        let gl = GRAPHICS_GL;
        let vertical = self.vertical_layout;
        let height = painter.screen().1;
        let cell_h = (if vertical { 25.0 } else { 40.0 }) + self.max_sectors_size;
        let avail_w = gui.controls[gui.taskbar].rect.left - 1.0;
        let avail_v = (height as f32 - 150.0) - 200.0;
        painter.gl_color4f(gl, self.back_color[0], self.back_color[1], self.back_color[2], self.back_color[3]);
        if !vertical {
            painter.gl_quad(gl, 0.0, height as f32 - cell_h, avail_w, cell_h - self.max_sectors_size, false);
        }
        let session = gui.sim.session_type;
        let mut reference = 0i32;
        if session >= 1 {
            if session <= 2 {
                reference = cut(entries[0].best_lap);
            } else if session == 3 {
                reference = cut(entries[0].total_time);
            }
        }
        let text = time_to_string3(reference);
        if !vertical {
            self.draw_cell(gui, painter, &entries[0], 1, &text, 0.0, height as f32 - cell_h, session);
        } else {
            self.draw_vertical_cell(gui, painter, &entries[0], 1, &text, 1.0, 150.0, session);
        }
        let fit = if !vertical { (avail_w - 170.0) * f32::from_bits(0x3bc0_c0c1) } else { (avail_v - cell_h) / cell_h };
        let nfit = (fit - 1.0) as i32;
        let last = entries.len() as i32 - 1;
        let mut hi = (player_position - 1) + nfit / 2;
        if hi > last {
            hi = last;
        } else if hi < 1 {
            hi = 1;
        }
        let mut lo = hi - nfit;
        if lo <= hi {
            if lo < 1 {
                lo = 1;
            }
        } else {
            lo = hi;
        }
        let mut i = lo;
        while i <= last {
            let (k, p) = (i - lo, i + 1);
            let e = &entries[i as usize];
            let mut g = 0i32;
            let mut clamp = false;
            if session == 1 || session == 2 {
                g = cut(e.best_lap).wrapping_sub(reference);
                clamp = true;
            } else if session == 3 {
                let m = self.gap_display_mode;
                if m == GAP_FROM_FIRST || (m == GAP_ALTERNATE && !(self.timer < 0.0)) {
                    if e.laps == entries[0].laps {
                        g = cut(e.total_time - reference as f64);
                        clamp = true;
                    }
                } else if m == GAP_FROM_PREVIOUS || (m == GAP_ALTERNATE && self.timer < 0.0) {
                    let before = &entries[i as usize - 1];
                    if e.laps == before.laps {
                        g = cut(e.total_time - before.total_time);
                        clamp = true;
                    }
                }
            }
            if clamp {
                if g > 99_999_999 {
                    g = 99_999_999;
                } else if g < 0 {
                    g = 0;
                }
            }
            let (x, y);
            if !vertical {
                x = (k as f32 + 1.0) * 170.0;
                y = height as f32 - cell_h;
                if x + 170.0 > avail_w {
                    break;
                }
            } else {
                let step = cell_h + 1.0;
                let base = cell_h + 150.0;
                y = ((k as f32 * step) + base) + 1.0;
                x = 1.0;
                if y + cell_h > avail_v + 150.0 {
                    break;
                }
            }
            let text = if session == 1 || session == 2 { time_to_diff_secs_3_millis(g) } else { time_to_diff_short(g) };
            if !vertical {
                self.draw_cell(gui, painter, e, p, &text, x, y, session);
            } else {
                self.draw_vertical_cell(gui, painter, e, p, &text, x, y, session);
            }
            i += 1;
        }
    }

    /// `OverlayLeaderboard::shutdown` 0x1401c1a30: the two values the game writes back into
    /// `gameplay.ini [OVERLAY_LEADERBOARD]` (`ACTIVE`, `VERTICAL`).
    pub fn shutdown(&self) -> (String, String) {
        (self.gap_display_mode.to_string(), (self.vertical_layout as i32).to_string())
    }
}
