// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PlayerCurrentTime` (0x2f8 bytes), the app "Laps": the running lap's time, the lap number,
//! the best and the last lap, the split against the best lap.
//!
//! `PlayerCurrentTime::PlayerCurrentTime` 0x1402d9660, `render` 0x1402da350,
//! `renderBackground` 0x1402db5d0, `getSectorSplitsString` 0x1402da220.

use std::any::Any;

use crate::apps::{App, AppCar};
use crate::control::Id;
use crate::esc_menu::time_to_wstring;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::leaderboard::{time_to_diff_string, RaceLap};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GL_LINES, GRAPHICS_GL};

/// `SessionType::Race`.
const RACE: i32 = 3;

pub struct Laps {
    pub time: Id,
    pub last: Id,
    pub best: Id,
    pub tex_last: Texture,
    pub tex_best: Texture,
    pub tex_arrow_down: Texture,
    pub tex_arrow_right: Texture,
    pub tex_sector_up: Texture,
    pub tex_sector_down: Texture,
    pub tex_inf: Texture,
}

impl Gui {
    /// `PlayerCurrentTime::PlayerCurrentTime(Sim*)` 0x1402d9660.
    pub fn new_laps_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "LAP_TIME", true);
        self.form_data_mut(form).dev_app = false;
        self.form_set_title(form, &translate("Laps"));
        self.set_size(form, 295.0, 277.0);
        self.set_auto_hide_mode(form, true);
        let mut font = Font::new(painter, "", FontType::Proportional, 17.0, false, false);
        font.shadowed = true;
        self.fonts.push(font);
        self.controls[form].font = Some(self.fonts.len() - 1);
        let mut tex = |name: &str| painter.get_texture(&format!("content/gui/PlayerCurrentTime/{name}.png"));
        let (tex_last, tex_best, tex_arrow_down, tex_arrow_right, tex_sector_up, tex_sector_down, tex_inf) = (tex("last"), tex("best"), tex("downArrow"), tex("rightArrow"), tex("up"), tex("down"), tex("infinity_icon"));
        let mut font24 = Font::new(painter, "", FontType::Proportional, 24.0, false, false);
        font24.shadowed = true;
        self.fonts.push(font24);
        let font24 = self.fonts.len() - 1;

        let w = {
            let r = self.controls[form].rect;
            r.right - r.left
        };
        let time = self.new_label(painter, "TIME_LABEL");
        self.controls[time].draw_background = false;
        self.set_size(time, w, 40.0);
        self.controls[time].fore_color = [1.0; 4];
        self.controls[time].font_align = ALIGN_CENTER;
        let mut font32 = Font::new(painter, "", FontType::Proportional, 32.0, false, false);
        font32.shadowed = true;
        self.fonts.push(font32);
        self.controls[time].font = Some(self.fonts.len() - 1);
        self.set_position(time, 0.0, 110.0);
        self.add_control(form, time);
        let mut label = |gui: &mut Gui, name: &str, y: f32| {
            let id = gui.new_label(painter, name);
            gui.controls[id].draw_background = false;
            gui.set_size(id, w - 20.0, 40.0);
            gui.controls[id].fore_color = [1.0; 4];
            gui.controls[id].font_align = ALIGN_RIGHT;
            gui.controls[id].font = Some(font24);
            gui.set_position(id, 0.0, y);
            gui.add_control(form, id);
            id
        };
        let best = label(self, "BEST_LABEL", 194.0);
        let last = label(self, "LAST_LABEL", 235.0);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Laps { time, last, best, tex_last, tex_best, tex_arrow_down, tex_arrow_right, tex_sector_up, tex_sector_down, tex_inf }));
        form
    }
}

/// `timeToString(ms, 3)` of a lap's (unsigned) time.
fn lap_text(ms: u32) -> String {
    time_to_string3(ms as i32)
}

impl Laps {
    /// `PlayerCurrentTime::getSectorSplitsString` 0x1402da220.
    fn sector_splits_string(cur: &RaceLap, best: &RaceLap, split: i32, car: &AppCar) -> String {
        if best.time == 0 {
            return String::new();
        }
        if cur.splits.len() > best.splits.len() {
            return String::new();
        }
        if cur.time < 7000 {
            if car.state.lap_count <= 1 {
                return String::new();
            }
            return time_to_diff_string(split, 3);
        }
        let (mut a, mut b) = (0i32, 0i32);
        for i in 0..cur.splits.len() {
            a = a.wrapping_add(cur.splits[i] as i32);
            b = b.wrapping_add(best.splits[i] as i32);
            if (cur.time.wrapping_sub(a as u32)) < 7000 {
                return time_to_diff_string(a.wrapping_sub(b), 3);
            }
        }
        String::new()
    }

    /// `PlayerCurrentTime::renderBackground` 0x1402db5d0.
    fn render_background(&self, gui: &mut Gui, id: Id, painter: &mut dyn Painter) {
        let [ox, oy] = gui.local_to_world(id, [0.0, 0.0]);
        let s = gui.controls[id].scale_mult;
        let r = gui.controls[id].rect;
        let w = r.right - r.left;
        let col_a = [1.0, 1.0, 1.0, 0.5];
        let col_b = [1.0, 1.0, 1.0, f32::from_bits(0x3e4c_cccd)];
        let gl = GRAPHICS_GL;
        let line = |painter: &mut dyn Painter, y: f32| {
            painter.gl_color4f(gl, col_a[0], col_a[1], col_a[2], col_a[3]);
            painter.gl_begin(gl, GL_LINES);
            painter.gl_vertex3f(gl, ox, s * y + oy, 0.0);
            painter.gl_vertex3f(gl, w + ox, s * y + oy, 0.0);
            painter.gl_end(gl);
        };
        let band = |painter: &mut dyn Painter, y: f32| {
            painter.gl_color4f(gl, col_b[0], col_b[1], col_b[2], col_b[3]);
            painter.gl_quad(gl, ox, s * y + oy, w, s * 40.0, false);
        };
        line(painter, 110.0);
        band(painter, 111.0);
        line(painter, 152.0);
        line(painter, 192.0);
        band(painter, 193.0);
        line(painter, 235.0);
        band(painter, 235.0);
        line(painter, 276.0);
        let font = gui.controls[id].font.unwrap_or(gui.font);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &self.tex_best);
        painter.gl_quad(gl, s * 5.0 + ox, s * 196.0 + oy, s * 27.0, s * 31.0, true);
        gui.fonts[font].blit_string(painter, s * 55.0 + ox, s * 201.0 + oy, &(translate("Best") + " :"), s, ALIGN_LEFT);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &self.tex_last);
        painter.gl_quad(gl, s * 5.0 + ox, s * 238.0 + oy, s * 27.0, s * 31.0, true);
        gui.fonts[font].blit_string(painter, s * 55.0 + ox, s * 243.0 + oy, &(translate("Last") + " :"), s, ALIGN_LEFT);
    }
}

impl App for Laps {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `PlayerCurrentTime::render` 0x1402da350.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let link = gui.apps.clone();
        let car = link.focused();
        gui.control_render(id, dt, painter);
        self.render_background(gui, id, painter);
        let [ox, oy] = gui.local_to_world(id, [0.0, 0.0]);
        let s = gui.controls[id].scale_mult;
        let r = gui.controls[id].rect;
        let w = r.right - r.left;
        let font = gui.controls[id].font.unwrap_or(gui.font);
        let gl = GRAPHICS_GL;
        let timed = link.session_type == RACE && link.is_timed_race;
        gui.fonts[font].blit_string(painter, s * 10.0 + ox, s * 60.0 + oy, &translate(if timed { "Total Time" } else { "Current lap" }), s, ALIGN_LEFT);
        painter.set_texture(0, &self.tex_arrow_right);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(gl, s * 100.0 + ox, s * 68.0 + oy, s * 11.0, s * 13.0, true);
        if timed {
            if !(link.time_to_session_start >= 0.0) {
                let t = time_to_wstring(cut(link.session_time_left), false, false);
                gui.fonts[font].blit_string(painter, (w + ox) - s * 10.0, s * 35.0 + oy, &t, s * f32::from_bits(0x4033_3333), ALIGN_RIGHT);
            }
        } else {
            gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
            let n = car.state.lap_count.wrapping_add(1);
            gui.fonts[font].blit_string(painter, ((w * 0.5) + ox) + s * 45.0, s * 35.0 + oy, &n.to_string(), s * 3.0, ALIGN_CENTER);
            gui.fonts[font].blit_string(painter, ((w * 0.5) + ox) + s * 75.0, s * 60.0 + oy, "/", s * f32::from_bits(0x3f8c_cccd), ALIGN_LEFT);
            if link.session_type == RACE {
                gui.fonts[font].blit_string(painter, ((w * 0.5) + ox) + s * 100.0, s * 60.0 + oy, &link.session_laps.to_string(), s * f32::from_bits(0x3f8c_cccd), ALIGN_CENTER);
            } else {
                painter.set_texture(0, &self.tex_inf);
                painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
                painter.gl_quad(gl, ((w * 0.5) + ox) + s * 90.0, s * 68.0 + oy, s * 29.0, s * 17.0, true);
            }
        }
        gui.fonts[font].blit_string(painter, s * 10.0 + ox, s * 80.0 + oy, &translate("Lap Time"), s, ALIGN_LEFT);
        painter.set_texture(0, &self.tex_arrow_down);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(gl, s * 100.0 + ox, s * 87.0 + oy, s * 13.0, s * 13.0, true);
        let (cur, best, last) = (&car.current_lap, &car.best_lap, &car.last_lap);
        gui.controls[self.time].fore_color = if car.current_lap_valid { [1.0; 4] } else { [1.0, 0.0, 0.0, 1.0] };
        gui.controls[self.last].fore_color = if last.time != 0 && !last.is_valid { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
        gui.set_text(self.time, &if cur.time == 0 { "00:00:000".to_string() } else { lap_text(cur.time) });
        gui.set_text(self.last, &if last.time == 0 { "--:--:--".to_string() } else { lap_text(last.time) });
        gui.set_text(self.best, &if best.time == 0 { "--:--:--".to_string() } else { lap_text(best.time) });
        let d = Laps::sector_splits_string(cur, best, car.split, &car);
        if !d.is_empty() {
            gui.fonts[font].blit_string(painter, s * 10.0 + ox, s * 165.0 + oy, &translate("Split from"), s * f32::from_bits(0x3f66_6666), ALIGN_LEFT);
            painter.set_texture(0, &self.tex_arrow_down);
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            painter.gl_quad(gl, s * 95.0 + ox, s * 170.0 + oy, s * 13.0, s * 13.0, true);
            if d.starts_with('+') {
                gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
                painter.set_texture(0, &self.tex_sector_down);
            }
            if d.starts_with('-') {
                gui.fonts[font].set_color(0.0, 1.0, 0.0, 1.0);
                painter.set_texture(0, &self.tex_sector_up);
            }
            let right = gui.controls[id].rect.right;
            painter.gl_quad(gl, right - s * 40.0, s * 167.0 + oy, s * 18.0, s * 17.0, true);
            gui.fonts[font].blit_string(painter, right - s * 50.0, s * 163.0 + oy, &d, s, ALIGN_RIGHT);
        }
    }
}

/// `cvttsd2si`: a double cut to an `int`, the "indefinite" value when it does not fit.
pub fn cut(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}
