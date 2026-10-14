// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ACEssentials` (0x2a0 bytes), the app "Essentials": fuel and rpm bars, gear, speed, the
//! session's line, the last lap, the place, ABS and TC.
//!
//! `ACEssentials::ACEssentials` 0x14031bc60, `render` 0x14031d670; the static `gears` (its
//! initialiser 0x140015240) and the layout constants `coordFuel` … `coordPos` 0x14151fb80.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::esc_menu::time_to_wstring;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GRAPHICS_GL};

const GEARS: [&str; 10] = ["R", "N", "1", "2", "3", "4", "5", "6", "7", "8"];
const COORD_FUEL: [f32; 2] = [24.0, 66.0];
const COORD_RPM: [f32; 2] = [164.0, 66.0];
const COORD_GEAR: [f32; 2] = [348.0, 15.0];
const COORD_SPEED: [f32; 2] = [426.0, 55.0];
const COORD_SESSION: [f32; 2] = [532.0, 55.0];
const COORD_LAST: [f32; 2] = [659.0, 55.0];
const COORD_POS: [f32; 2] = [775.0, 55.0];

pub struct Essentials {
    pub lbl_gear: Id,
    pub lbl_speed: Id,
    pub lbl_session: Id,
    pub lbl_position: Id,
    pub lbl_position_description: Id,
    pub lbl_last: Id,
    pub abs: Id,
    pub tc: Id,
    pub gui_session_desc: Id,
    pub gui_speed_desc: Id,
    pub has_p2p: bool,
    pub p2p_label: Option<Id>,
    pub p2p_value: Option<Id>,
    pub lbl: [Id; 4],
    pub tex_fuel_bar: Texture,
    pub tex_rpm_bar: Texture,
    pub current_rpm: f32,
}

/// `cvttss2si`: a float cut to an `int`, the "indefinite" value when it does not fit.
pub fn cut_f(x: f32) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

fn texture_size(painter: &dyn Painter, t: &Texture) -> (u32, u32) {
    if t.kid.is_some() {
        painter.texture_size(t)
    } else {
        (0, 0)
    }
}

impl Gui {
    /// `ACEssentials::ACEssentials(Sim*)` 0x14031bc60.
    pub fn new_essentials_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "AC_ESSENTIALS", true);
        self.form_set_title(form, &translate("Essentials"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 850.0, 110.0);
        if self.apps.session_type == 3 {
            self.set_size(form, 875.0, 110.0);
        }
        self.form_data_mut(form).autohide = true;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 15.0, false, false));
        self.controls[form].font = Some(self.fonts.len() - 1);
        let tex_rpm_bar = painter.get_texture("content/gui/essential/rpmBar.png");
        let tex_fuel_bar = painter.get_texture("content/gui/essential/fuelBar.png");
        let new_font = |gui: &mut Gui, painter: &mut dyn Painter, kind: FontType, size: f32| {
            gui.fonts.push(Font::new(painter, "", kind, size, false, false));
            gui.fonts.len() - 1
        };
        // the captions sit on the form's bottom edge
        let bottom = |gui: &Gui, label: Id| {
            let (f, l) = (gui.controls[form].rect, gui.controls[label].rect);
            (f.bottom - f.top) - (l.bottom - l.top)
        };
        let width = |gui: &Gui, label: Id| {
            let r = gui.controls[label].rect;
            r.right - r.left
        };

        let fuel = self.new_label(painter, "FUEL_BAR");
        self.set_text(fuel, &translate("FUELS"));
        self.set_size(fuel, texture_size(painter, &tex_fuel_bar).0 as f32, 21.0);
        self.controls[fuel].font_align = ALIGN_CENTER;
        let y = bottom(self, fuel);
        self.set_position(fuel, COORD_FUEL[0], y);
        self.add_control(form, fuel);

        let rpm = self.new_label(painter, "RPM_BAR");
        self.set_text(rpm, &translate("RPMS"));
        self.set_size(rpm, texture_size(painter, &tex_rpm_bar).0 as f32, 21.0);
        self.controls[rpm].font_align = ALIGN_CENTER;
        let y = bottom(self, rpm);
        self.set_position(rpm, COORD_RPM[0], y);
        self.add_control(form, rpm);

        let lbl_gear = self.new_label(painter, "4");
        self.set_size(lbl_gear, 55.0, 55.0);
        let f = new_font(self, painter, FontType::Proportional, 60.0);
        self.controls[lbl_gear].font = Some(f);
        self.controls[lbl_gear].font_align = ALIGN_CENTER;
        self.set_position(lbl_gear, COORD_GEAR[0], COORD_GEAR[1]);
        self.add_control(form, lbl_gear);

        let gear = self.new_label(painter, "GEAR_BAR");
        self.set_text(gear, &translate("GEAR"));
        self.set_size(gear, 55.0, 21.0);
        self.controls[gear].font_align = ALIGN_CENTER;
        let y = bottom(self, gear);
        self.set_position(gear, COORD_GEAR[0], y);
        self.add_control(form, gear);

        let value = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, at: [f32; 2]| {
            let id = gui.new_label(painter, name);
            gui.set_size(id, 70.0, 32.0);
            let f = new_font(gui, painter, FontType::Proportional, 25.0);
            gui.controls[id].font = Some(f);
            gui.controls[id].font_align = ALIGN_CENTER;
            gui.set_position(id, at[0], at[1]);
            gui.add_control(form, id);
            id
        };
        let caption = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: Option<String>, w: f32, x: f32| {
            let id = gui.new_label(painter, name);
            if let Some(text) = text {
                gui.set_text(id, &text);
            }
            gui.set_size(id, w, 21.0);
            gui.controls[id].font_align = ALIGN_CENTER;
            let y = bottom(gui, id);
            gui.set_position(id, x, y);
            gui.add_control(form, id);
            id
        };

        let lbl_speed = value(self, painter, "354", COORD_SPEED);
        let gui_speed_desc = caption(self, painter, "KMH", None, 70.0, COORD_SPEED[0]);
        let lbl_session = value(self, painter, "1:23:456", COORD_SESSION);
        let w = width(self, lbl_session);
        let gui_session_desc = caption(self, painter, "SESSION_LABEL", Some(translate("SESSION")), w, COORD_SESSION[0]);
        let lbl_last = value(self, painter, "1:23:456", COORD_LAST);
        let w = width(self, lbl_last);
        let last = caption(self, painter, "LAST_LABEL", Some(translate("LAST")), w, COORD_LAST[0]);
        let lbl_position = value(self, painter, "9", COORD_POS);

        let has_p2p = self.apps.has_p2p;
        let (mut p2p_label, mut p2p_value) = (None, None);
        if has_p2p {
            let id = self.new_label(painter, "P2PLBL");
            self.set_position(id, COORD_POS[0], COORD_POS[1] - 22.0);
            self.set_size(id, 30.0, 32.0);
            let f = new_font(self, painter, FontType::Monospaced, 16.0);
            self.controls[id].font = Some(f);
            self.controls[id].font_align = ALIGN_RIGHT;
            self.set_text(id, "P2P");
            self.add_control(form, id);
            p2p_label = Some(id);
            let id = self.new_label(painter, "P2PVAL");
            self.set_position(id, COORD_POS[0] + 40.0, COORD_POS[1] - 25.0);
            self.set_size(id, 30.0, 32.0);
            let f = new_font(self, painter, FontType::Proportional, 20.0);
            self.controls[id].font = Some(f);
            self.controls[id].font_align = ALIGN_LEFT;
            self.add_control(form, id);
            p2p_value = Some(id);
        }

        let w = width(self, lbl_last);
        let lbl_position_description = caption(self, painter, "POSITION_LABEL", Some(translate("POS.")), w, COORD_POS[0]);
        let aid = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, x: f32| {
            let id = gui.new_label(painter, name);
            let w = width(gui, lbl_last);
            gui.set_size(id, w, 21.0);
            gui.controls[id].font_align = ALIGN_CENTER;
            let f = new_font(gui, painter, FontType::Proportional, 16.0);
            gui.controls[id].font = Some(f);
            gui.set_position(id, x, 30.0);
            gui.add_control(form, id);
            id
        };
        let abs = aid(self, painter, "ABS", 650.0);
        let tc = aid(self, painter, "TC", 690.0);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(
            form,
            Box::new(Essentials {
                lbl_gear,
                lbl_speed,
                lbl_session,
                lbl_position,
                lbl_position_description,
                lbl_last,
                abs,
                tc,
                gui_session_desc,
                gui_speed_desc,
                has_p2p,
                p2p_label,
                p2p_value,
                lbl: [fuel, rpm, gear, last],
                tex_fuel_bar,
                tex_rpm_bar,
                current_rpm: 0.0,
            }),
        );
        form
    }
}

impl App for Essentials {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ACEssentials::render` 0x14031d670.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let link = gui.apps.clone();
        let car = link.focused();
        let st = &car.state;
        let gl = GRAPHICS_GL;
        let s = gui.controls[id].scale_mult;
        let r = gui.controls[id].rect;
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);

        // the fuel bar
        let max64 = car.max_fuel;
        let max = max64 as f32;
        let fuel = st.fuel;
        let v = if fuel > max {
            max
        } else if fuel >= 0.0 {
            fuel
        } else {
            0.0
        };
        let ratio = ((v as f64) / max64) as f32;
        if ratio > 0.25 {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 0.5);
        } else {
            let g = (1.0f64 - (0.25f64 - ratio as f64) * 4.0f64) as f32;
            painter.gl_color4f(gl, 1.0, g, 0.0, 0.5);
        }
        let (fw, fh) = texture_size(painter, &self.tex_fuel_bar);
        let h = fh as f32 * s;
        let w = (fw as f32 * ratio) * s;
        painter.gl_quad(gl, s * COORD_FUEL[0] + r.left, s * COORD_FUEL[1] + r.top, w, h, false);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &self.tex_fuel_bar);
        painter.gl_quad(gl, s * COORD_FUEL[0] + r.left, s * COORD_FUEL[1] + r.top, fw as f32 * s, fh as f32 * s, true);

        // the rpm bar
        let limit = if st.limiter_rpm < car.engine_damage_rpm { st.limiter_rpm } else { car.engine_damage_rpm };
        let limit_f = limit as f32;
        let k = (dt * f32::from_bits(0x4279_ffff)) * f32::from_bits(0x3e4c_cccd);
        self.current_rpm = (st.engine_rpm - self.current_rpm) * k + self.current_rpm;
        if self.current_rpm >= limit_f * f32::from_bits(0x3f7d_70a4) {
            painter.gl_color4f(gl, 1.0, 0.0, 0.0, 0.5);
            gui.controls[self.lbl_gear].fore_color = [1.0, 0.0, 0.0, 1.0];
        } else {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 0.5);
            gui.controls[self.lbl_gear].fore_color = [1.0; 4];
        }
        let x = self.current_rpm / limit_f;
        let x = if x > 1.0 {
            1.0
        } else if x >= 0.0 {
            x
        } else {
            0.0
        };
        let (rw, rh) = texture_size(painter, &self.tex_rpm_bar);
        painter.gl_quad(gl, s * COORD_RPM[0] + r.left, s * COORD_RPM[1] + r.top, (rw as f32 * x) * s, rh as f32 * s, false);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &self.tex_rpm_bar);
        painter.gl_quad(gl, s * COORD_RPM[0] + r.left, s * COORD_RPM[1] + r.top, rw as f32 * s, rh as f32 * s, true);

        // the gear, the speed
        gui.set_text(self.lbl_gear, GEARS.get(st.gear as usize).copied().unwrap_or(""));
        let mut v = st.speed * if link.use_mph { f32::from_bits(0x400f_29f7) } else { f32::from_bits(0x4066_6666) };
        v = v + 0.5;
        let mut t = cut_f(v);
        if t != i32::MIN && t as f32 != v {
            t -= v.is_sign_negative() as i32;
            v = t as f32;
        }
        gui.set_text(self.lbl_speed, &cut_f(v).to_string());
        gui.set_text(self.gui_speed_desc, if link.use_mph { "MPH" } else { "KMH" });

        // the session
        gui.set_visible(self.lbl_position, false);
        gui.set_visible(self.lbl_position_description, false);
        gui.controls[self.lbl_session].fore_color = [1.0; 4];
        let mut text = String::new();
        let n = link.connected_cars_count;
        match link.session_type {
            0 => {
                text = time_to_string3(car.best_lap.time as i32);
                gui.set_text(self.gui_session_desc, &translate("BEST"));
            }
            1 if link.session_count == 1 => {
                text = "---".to_string();
                gui.set_text(self.gui_session_desc, &translate("LEFT"));
            }
            1 | 2 => {
                let pos = car.leaderboard_position;
                gui.set_text(self.lbl_position, &if pos > 0 { format!("{pos} / {n}") } else { format!("-- / {n}") });
                let left = link.session_time_left;
                if left < 0.0 {
                    gui.controls[self.lbl_session].fore_color = [1.0, 1.0, 0.0, 1.0];
                    text = "0:00".to_string();
                    gui.set_text(self.gui_session_desc, "OVER");
                } else {
                    gui.controls[self.lbl_session].fore_color = [1.0; 4];
                    text = time_to_wstring(crate::app_laps::cut(left), false, false);
                    gui.set_text(self.gui_session_desc, &translate("LEFT"));
                }
                gui.set_visible(self.lbl_position, true);
                gui.set_visible(self.lbl_position_description, true);
            }
            3 => {
                gui.set_visible(self.lbl_position, true);
                gui.set_visible(self.lbl_position_description, true);
                gui.set_text(self.lbl_position, &if link.in_replay { format!("-- / {n}") } else { format!("{} / {n}", car.realtime_position.wrapping_add(1)) });
                if !link.is_timed_race {
                    let l = st.lap_count.wrapping_add(1);
                    let total = link.session_laps;
                    let shown = if l <= total as u32 {
                        if l == 0 {
                            1
                        } else {
                            l
                        }
                    } else {
                        total as u32
                    };
                    text = format!("{shown} / {total}");
                    gui.set_text(self.gui_session_desc, &translate("LAPS"));
                } else if link.time_to_session_start < 0.0 {
                    gui.controls[self.lbl_session].fore_color = [1.0; 4];
                    text = time_to_wstring(crate::app_laps::cut(link.session_time_left), false, false);
                    gui.set_text(self.gui_session_desc, &translate("LEFT"));
                } else if link.time_to_wait <= 0.0 {
                    gui.set_text(self.gui_session_desc, "");
                } else {
                    gui.controls[self.lbl_session].fore_color = [1.0, 1.0, 0.0, 1.0];
                    text = time_to_wstring(crate::app_laps::cut(link.time_to_wait), false, false);
                    gui.set_text(self.gui_session_desc, "OVER");
                }
            }
            _ => {}
        }
        gui.set_text(self.lbl_session, &text);

        // the last lap
        gui.set_text(self.lbl_last, &time_to_string3(car.last_lap.time as i32));

        // ABS, TC
        gui.set_visible(self.abs, car.abs_enabled);
        if gui.controls[self.abs].visible {
            gui.controls[self.abs].fore_color = if car.abs_in_action { [1.0, 0.0, 0.0, 1.0] } else { [f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), 1.0] };
        }
        gui.set_visible(self.tc, car.tc_enabled);
        if gui.controls[self.tc].visible {
            gui.controls[self.tc].fore_color = if car.tc_in_action { [1.0, 0.0, 0.0, 1.0] } else { [f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), 1.0] };
        }

        // push to pass
        if self.has_p2p {
            if let Some(value) = self.p2p_value {
                match st.p2p_status {
                    1 => gui.controls[value].fore_color = [1.0, 0.0, 0.0, 1.0],
                    2 => gui.controls[value].fore_color = [1.0; 4],
                    3 => gui.controls[value].fore_color = [0.0, 0.0, 1.0, 1.0],
                    _ => {}
                }
                gui.set_text(value, &(st.p2p_activations as u32).to_string());
            }
        }
        let _ = self.p2p_label;
    }
}
