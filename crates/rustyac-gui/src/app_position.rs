// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `RacePosition` (0x200 bytes), the app "Race Position": the place among the connected cars
//! and the lap of the race (or the time left).
//!
//! `RacePosition::RacePosition` 0x1402fe0f0, `render` 0x1402feb80.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::esc_menu::time_to_wstring;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT};

pub struct RacePosition {
    pub lab_position: Id,
    pub lab_total_cars: Id,
    pub lab_pos_text: Id,
    pub lab_lap: Id,
    pub lab_total_laps: Id,
    pub lab_lap_text: Id,
    pub position_accumulator: f32,
}

impl Gui {
    /// `RacePosition::RacePosition(Sim*)` 0x1402fe0f0.
    pub fn new_race_position_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "RACE_POSITION", true);
        self.form_set_title(form, &translate("Race Position"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 450.0, 150.0);
        self.set_auto_hide_mode(form, true);
        let mut font = Font::new(painter, "", FontType::Proportional, 98.0, false, false);
        font.shadowed = true;
        self.fonts.push(font);
        let form_font = self.fonts.len() - 1;
        self.controls[form].font = Some(form_font);
        let font = |gui: &mut Gui, painter: &mut dyn Painter, size: f32| {
            let mut font = Font::new(painter, "", FontType::Proportional, size, false, false);
            font.shadowed = true;
            gui.fonts.push(font);
            gui.fonts.len() - 1
        };
        let label = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, x: f32, y: f32| {
            let id = gui.new_label(painter, name);
            gui.set_position(id, x, y);
            gui.set_size(id, 50.0, 32.0);
            id
        };

        let lab_position = label(self, painter, "-", 80.0, 26.0);
        self.controls[lab_position].font = Some(form_font);
        self.controls[lab_position].fore_color = [1.0; 4];
        self.controls[lab_position].font_align = ALIGN_RIGHT;
        self.add_control(form, lab_position);

        let lab_total_cars = label(self, painter, "/-", 130.0, 70.0);
        let f = font(self, painter, 48.0);
        self.controls[lab_total_cars].font = Some(f);
        self.controls[lab_total_cars].font_align = ALIGN_LEFT;
        self.controls[lab_total_cars].fore_color = [1.0; 4];
        self.add_control(form, lab_total_cars);

        let lab_pos_text = self.new_label(painter, "LABEL_POSITION");
        self.set_text(lab_pos_text, &translate("Pos"));
        self.set_position(lab_pos_text, 60.0, 30.0);
        self.set_size(lab_pos_text, 50.0, 32.0);
        let f = font(self, painter, 24.0);
        self.controls[lab_pos_text].font = Some(f);
        self.controls[lab_pos_text].fore_color = [1.0; 4];
        self.add_control(form, lab_pos_text);

        let lab_lap = label(self, painter, "-", 255.0, 26.0);
        self.controls[lab_lap].font = Some(form_font);
        self.controls[lab_lap].fore_color = [1.0; 4];
        self.add_control(form, lab_lap);

        let lab_total_laps = label(self, painter, "/-", 310.0, 70.0);
        let f = font(self, painter, 48.0);
        self.controls[lab_total_laps].font = Some(f);
        self.controls[lab_total_laps].fore_color = [1.0; 4];
        self.add_control(form, lab_total_laps);

        let lab_lap_text = self.new_label(painter, "LABEL_LAP");
        self.set_text(lab_lap_text, &translate("Lap"));
        self.set_position(lab_lap_text, 310.0, 30.0);
        self.set_size(lab_lap_text, 50.0, 32.0);
        let f = font(self, painter, 24.0);
        self.controls[lab_lap_text].font = Some(f);
        self.controls[lab_lap_text].fore_color = [1.0; 4];
        self.add_control(form, lab_lap_text);

        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(RacePosition { lab_position, lab_total_cars, lab_pos_text, lab_lap, lab_total_laps, lab_lap_text, position_accumulator: -1.0 }));
        form
    }
}

/// `(float)(text.size() - 1)` as the game converts an unsigned 64-bit length less one.
fn length_less_one(text: &str) -> f32 {
    let n = (text.encode_utf16().count() as u64).wrapping_sub(1) as i64;
    let mut f = n as f32;
    if n < 0 {
        f += f32::from_bits(0x5f80_0000);
    }
    f
}

impl App for RacePosition {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `RacePosition::render` 0x1402feb80.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let link = gui.apps.clone();
        let car = link.focused();
        let saved_position_x = gui.controls[self.lab_position].rect.left;
        let saved_lap_x = gui.controls[self.lab_lap].rect.left;
        let count = link.connected_cars_count;
        if count < 1 {
            gui.set_text(self.lab_total_cars, "-/-");
            gui.set_text(self.lab_position, "-");
        } else {
            gui.set_text(self.lab_total_cars, &format!("/{count}"));
            let kind = link.session_type;
            if kind >= 0 {
                if kind < 3 {
                    let place = car.leaderboard_position;
                    gui.set_text(self.lab_position, &if place < 1 { "-".to_string() } else { place.to_string() });
                } else if kind == 3 {
                    gui.set_text(self.lab_position, &if link.in_replay { "-".to_string() } else { car.realtime_position.wrapping_add(1).to_string() });
                } else if kind == 4 {
                    gui.set_text(self.lab_position, "1");
                }
            }
        }
        gui.set_text(self.lab_lap, "");
        gui.controls[self.lab_total_laps].fore_color = [1.0; 4];
        let kind = link.session_type;
        let time_left = |gui: &mut Gui, me: &RacePosition| {
            gui.set_text(me.lab_lap_text, &translate("Time"));
            let left = crate::app_laps::cut(link.session_time_left);
            if left < 1 {
                gui.controls[me.lab_total_laps].fore_color = [1.0, 1.0, 0.0, 1.0];
                gui.set_text(me.lab_total_laps, "0:00");
            } else {
                gui.set_text(me.lab_total_laps, &time_to_wstring(left, false, false));
            }
        };
        if kind < 1 {
            gui.set_text(self.lab_lap_text, "");
        } else if kind <= 2 {
            time_left(gui, self);
        } else if kind == 3 {
            if link.is_timed_race {
                if !(link.time_to_session_start >= 0.0) {
                    gui.set_text(self.lab_lap_text, &translate("Race Time"));
                    let left = crate::app_laps::cut(link.session_time_left);
                    gui.set_text(self.lab_total_laps, &time_to_wstring(left, false, false));
                    gui.controls[self.lab_total_laps].font_align = ALIGN_CENTER;
                }
            } else {
                gui.set_text(self.lab_lap_text, &translate("Lap"));
                gui.controls[self.lab_total_laps].font_align = ALIGN_LEFT;
                let laps = link.session_laps as u32;
                let lap = car.state.lap_count.wrapping_add(1);
                let _shown = if laps >= lap { lap } else { laps };
                if laps == 0 {
                    gui.set_text(self.lab_total_laps, "");
                } else {
                    gui.set_text(self.lab_total_laps, &format!("{_shown}/{laps}"));
                    let k = length_less_one(&gui.controls[self.lab_position].text) * 50.0;
                    gui.controls[self.lab_position].rect.left -= k;
                    let k = length_less_one(&gui.controls[self.lab_lap].text) * 50.0;
                    gui.controls[self.lab_lap].rect.left -= k;
                }
            }
        } else if kind - 4 <= 1 {
            time_left(gui, self);
        } else {
            gui.set_text(self.lab_lap_text, "");
        }
        gui.controls[self.lab_lap].rect.left = saved_lap_x;
        gui.controls[self.lab_position].rect.left = saved_position_x;
        gui.control_render(id, dt, painter);
        let _ = self.position_accumulator;
    }
}
