// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PerformanceMeterApp` (0x200 bytes), the app "Performance Delta": the time against the
//! best lap at this point of the track, the speed against it as two bars, the lap time it
//! comes to.
//!
//! `PerformanceMeterApp::PerformanceMeterApp` 0x140302570, `render` 0x140303090, with the
//! strip of micro-sectors under the form (`MicroSectors::drawSectors` 0x14009f290, drawn when
//! `gameplay.ini [GUI] SHOW_SECTORS` is set).

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::app_essentials::cut_f;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::leaderboard::time_to_diff_string;
use crate::painter::{Painter, ALIGN_LEFT, ALIGN_RIGHT};

pub struct PerformanceDelta {
    pub delta_label: Id,
    pub estimated_label: Id,
    pub lab_text: Id,
    pub lab_proj_time: Id,
    pub pb_green: Id,
    pub pb_red: Id,
}

impl Gui {
    /// `PerformanceMeterApp::PerformanceMeterApp(CarAvatar*)` 0x140302570.
    pub fn new_performance_delta_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "PERF_METER", true);
        self.form_set_title(form, &translate("Performance Delta"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 300.0, 105.0);
        self.form_data_mut(form).autohide = true;
        let width = |gui: &Gui, id: Id| {
            let r = gui.controls[id].rect;
            r.right - r.left
        };
        let third = f32::from_bits(0x3eaa_aaab);
        let sixth = f32::from_bits(0x3e2a_aaab);

        let pb_green = self.new_progress_bar(painter, "GREEN");
        let w = width(self, form);
        self.set_size(pb_green, w * third, 20.0);
        let w = width(self, form);
        self.set_position(pb_green, w * 0.5, 30.0);
        self.controls[pb_green].fore_color = [0.0, 1.0, 0.0, 1.0];
        self.progress_bar_mut(pb_green).is_vertical = false;
        self.controls[pb_green].border_color = [1.0; 4];
        self.control_add_control(form, pb_green);

        let pb_red = self.new_progress_bar(painter, "RED");
        let w = width(self, form);
        self.set_size(pb_red, w * third, 20.0);
        let w = width(self, form);
        let x = w * 0.5 - width(self, pb_red);
        self.set_position(pb_red, x, 30.0);
        self.controls[pb_red].fore_color = [1.0, 0.0, 0.0, 1.0];
        self.controls[pb_red].border_color = [1.0; 4];
        self.progress_bar_mut(pb_red).is_vertical = false;
        self.progress_bar_mut(pb_red).is_inverted = true;
        self.control_add_control(form, pb_red);

        let label = |gui: &mut Gui, painter: &mut dyn Painter, name: &str| gui.new_label(painter, name);
        let new_font = |gui: &mut Gui, painter: &mut dyn Painter, kind: FontType, size: f32, bold: bool| {
            gui.fonts.push(Font::new(painter, "", kind, size, false, bold));
            gui.fonts.len() - 1
        };

        let delta_label = label(self, painter, "DELTA_LABEL");
        self.set_text(delta_label, &translate("DELTA"));
        self.controls[delta_label].font_align = ALIGN_LEFT;
        let w = width(self, form);
        self.set_size(delta_label, w * 0.5, 40.0);
        let w = width(self, form);
        self.set_position(delta_label, w * sixth, 54.0);
        let f = new_font(self, painter, FontType::Proportional, 17.0, true);
        self.controls[delta_label].font = Some(f);
        self.controls[delta_label].fore_color = [1.0; 4];
        self.fonts[f].shadowed = false;
        self.control_add_control(form, delta_label);

        let lab_text = label(self, painter, "LAB_TEXT");
        let w = width(self, form);
        self.set_size(lab_text, w * third - 2.0, 40.0);
        let w = width(self, form);
        self.set_position(lab_text, w * 0.5, 53.0);
        let f = new_font(self, painter, FontType::Monospaced, 22.0, false);
        self.controls[lab_text].font = Some(f);
        self.controls[lab_text].font_align = ALIGN_RIGHT;
        self.controls[lab_text].draw_background = false;
        self.fonts[f].shadowed = false;
        self.control_add_control(form, lab_text);

        let estimated_label = label(self, painter, "ESTIMATED_LABEL");
        self.set_text(estimated_label, &translate("ESTIMATED"));
        self.controls[estimated_label].font_align = ALIGN_LEFT;
        let w = width(self, form);
        self.set_size(estimated_label, w * 0.5, 40.0);
        let w = width(self, form);
        self.set_position(estimated_label, w * sixth, 78.0);
        let f = new_font(self, painter, FontType::Proportional, 15.0, false);
        self.controls[estimated_label].font = Some(f);
        self.controls[estimated_label].fore_color = [1.0; 4];
        self.fonts[f].shadowed = false;
        self.control_add_control(form, estimated_label);

        let lab_proj_time = label(self, painter, "LAB_PRJ");
        let w = width(self, form);
        self.set_size(lab_proj_time, w * third - 2.0, 40.0);
        let w = width(self, form);
        self.set_position(lab_proj_time, w * 0.5, 78.0);
        let f = new_font(self, painter, FontType::Monospaced, 17.0, false);
        self.controls[lab_proj_time].font = Some(f);
        self.controls[lab_proj_time].font_align = ALIGN_RIGHT;
        self.controls[lab_proj_time].draw_background = false;
        self.controls[lab_proj_time].fore_color = [1.0; 4];
        self.fonts[f].shadowed = false;
        self.control_add_control(form, lab_proj_time);

        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(PerformanceDelta { delta_label, estimated_label, lab_text, lab_proj_time, pb_green, pb_red }));
        form
    }
}

fn clamp01(v: f32) -> f32 {
    if v > 1.0 {
        1.0
    } else if v < 0.0 {
        0.0
    } else if v >= 0.0 {
        v
    } else {
        0.0
    }
}

impl App for PerformanceDelta {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `PerformanceMeterApp::render` 0x140303090.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let car = gui.apps.focused();
        let st = &car.state;
        gui.progress_bar_mut(self.pb_green).value = clamp01(st.performance_meter_speed_diff_ms * f32::from_bits(0x3e38_51eb));
        gui.progress_bar_mut(self.pb_red).value = clamp01(st.performance_meter_speed_diff_ms * f32::from_bits(0xbe38_51eb));
        // the micro-sectors of the player's car (guid 0) under the form
        if gui.micro_sectors.enabled {
            let mut r = gui.controls[id].rect;
            gui.get_world_rect(id, &mut r);
            let p = gui.local_to_world(id, [0.0, r.bottom - r.top]);
            let strip_h = ((r.bottom - r.top) * f32::from_bits(0x3c6a_0ea1)) * 10.0;
            let color = gui.controls[id].back_color;
            gui.micro_sectors.draw_sectors(painter, crate::painter::GRAPHICS_GL, 0, p[0], p[1], r.right - r.left, strip_h, color);
        }
        gui.control_render(id, dt, painter);
        let d = st.performance_meter;
        if d == 0.0 || d.is_nan() {
            gui.controls[self.lab_text].fore_color = [1.0; 4];
            gui.set_text(self.lab_text, "0.000");
        } else {
            gui.controls[self.lab_text].fore_color = if d > 0.0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0, 1.0] };
            gui.set_text(self.lab_text, &time_to_diff_string(cut_f(d * 1000.0), 3));
        }
        let best = st.best_lap;
        if best == 0 {
            gui.set_text(self.lab_proj_time, "---");
        } else {
            let est = (d * 1000.0) + best as f32;
            let est = if est > -9.223372e18 && est < 9.223372e18 { est as i64 } else { i64::MIN };
            gui.set_text(self.lab_proj_time, &time_to_string3(est as i32));
        }
        let _ = (self.delta_label, self.estimated_label);
    }
}
