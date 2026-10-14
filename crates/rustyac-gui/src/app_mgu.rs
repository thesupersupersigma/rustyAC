// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Two small apps of the player's car (`Sim::cars[0]`): `FormMGU` (0x1e0 bytes, "ERS
//! Management": the ERS's recovery, delivery mode, MGU-H mode and the engine brake, shown only)
//! and `FormBallast` (0x1c0 bytes, "Ballast": a spinner of 0 to 150 kg, made offline only).
//!
//! `FormMGU::FormMGU` 0x1402eb080, `FormMGU::render` 0x1402ec440; `FormBallast::FormBallast`
//! 0x1402f0ef0, its spinner's handler 0x1402f1330 (`CarAvatar::setBallastKG` 0x1400d9de0, a
//! job for the physics thread).

use std::any::Any;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER};
use crate::widgets::WidgetEvent;

pub struct Mgu {
    pub lbl_delivery: Id,
    pub lbl_recovery: Id,
    pub lbl_engine_brake: Id,
    pub lbl_mode: Id,
}

impl Gui {
    /// `FormMGU::FormMGU(CarAvatar*)` 0x1402eb080 for the player's car.
    pub fn new_mgu_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "FORM_MGU", true);
        self.form_set_title(form, &translate("ERS Management"));
        self.form_data_mut(form).dev_app = false;
        self.set_auto_hide_mode(form, true);
        // three bold Consolas, shadowed by a pixel: 12, 14, 40
        let mut fonts = [0usize; 3];
        for (slot, size) in fonts.iter_mut().zip([12.0, 14.0, 40.0]) {
            let mut font = Font::new(painter, "", FontType::Monospaced, size, false, true);
            font.shadowed = true;
            font.shadow_pixel_distance = 1.0;
            self.fonts.push(font);
            *slot = self.fonts.len() - 1;
        }
        let [a, b, c] = fonts;
        // (name, font, text, white, w, h, x, y)
        let rows: [(&str, usize, &str, bool, f32, f32, f32, f32); 12] = [
            ("Recovery_title", a, "MGU-K", true, 80.0, 20.0, 5.0, 40.0),
            ("Delivery_title", a, "MGU-K", true, 120.0, 20.0, 90.0, 40.0),
            ("Mode_title", a, "MGU-H", true, 80.0, 20.0, 215.0, 40.0),
            ("EngineBrake_title", a, "ENGINE", true, 80.0, 20.0, 300.0, 40.0),
            ("Recovery_title2", a, "RECOVERY", true, 80.0, 20.0, 5.0, 53.0),
            ("Delivery_title2", a, "DELIVERY", true, 120.0, 20.0, 90.0, 53.0),
            ("Mode_title2", a, "MODE", true, 80.0, 20.0, 215.0, 53.0),
            ("EngineBrake_title2", a, "BRAKE", true, 80.0, 20.0, 300.0, 53.0),
            ("Recovery_value", c, "", false, 80.0, 40.0, 5.0, 60.0),
            ("Delivery_value", b, "", true, 120.0, 30.0, 90.0, 75.0),
            ("Mode_value", b, "", true, 80.0, 30.0, 215.0, 75.0),
            ("EngineBrake_value", c, "", true, 80.0, 40.0, 300.0, 60.0),
        ];
        let mut ids = [0usize; 12];
        for (slot, (name, font, text, white, w, h, x, y)) in ids.iter_mut().zip(rows) {
            let l = self.new_label(painter, name);
            self.controls[l].font_scale = self.controls[form].font_scale;
            self.controls[l].font = Some(font);
            self.controls[l].font_align = ALIGN_CENTER;
            self.set_text(l, text);
            if white {
                self.controls[l].fore_color = [1.0; 4];
            }
            self.set_size(l, w, h);
            self.set_position(l, x, y);
            self.add_control(form, l);
            *slot = l;
        }
        self.set_size(form, 385.0, 100.0);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Mgu { lbl_recovery: ids[8], lbl_delivery: ids[9], lbl_mode: ids[10], lbl_engine_brake: ids[11] }));
        form
    }

    /// `FormBallast::FormBallast(CarAvatar*)` 0x1402f0ef0 for the player's car (offline only).
    pub fn new_ballast_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "FORM_BALLAST", true);
        self.form_data_mut(form).dev_app = false;
        self.set_auto_hide_mode(form, true);
        self.form_set_title(form, "Ballast");
        // a first spinner the form keeps and never shows
        let _dummy = self.new_spinner(painter, "spinnerBallast", true, false);
        let sp = self.new_spinner(painter, &translate("Ballast"), true, false);
        let s = self.spinner_mut(sp);
        s.min_value = 0;
        s.step = 1;
        s.max_value = 150;
        // Spinner::setValue(0) before the handler is hooked: nobody hears of it
        s.value = 0;
        self.spinner_mut(sp).handlers.push(Rc::new(|gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
            // CarAvatar::setBallastKG: a job for the physics thread, if the car has physics
            if gui.apps.cars.first().is_some_and(|c| c.has_physics) {
                gui.apps.actions.push(AppAction::SetBallastKg(*value as f32));
            }
        }));
        self.spinner_set_draw_arrows(painter, sp, true);
        self.set_size(sp, 184.0, 21.0);
        let mut font = Font::new(painter, "", FontType::Proportional, 12.0, false, false);
        font.shadow_pixel_distance = 2.0;
        self.fonts.push(font);
        self.controls[sp].font = Some(self.fonts.len() - 1);
        self.spinner_set_position(sp, 58.0, 70.0);
        self.add_control(form, sp);
        self.set_size(form, 300.0, 110.0);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        form
    }
}

impl App for Mgu {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormMGU::render` 0x1402ec440: the four values (with an ERS), then the form.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        if let Some(car) = gui.apps.cars.first().filter(|c| c.has_ers).cloned() {
            gui.set_text(self.lbl_delivery, if car.has_physics { &car.ers_power_name } else { "" });
            gui.set_text(self.lbl_recovery, &car.ers_recovery.to_string());
            gui.set_text(self.lbl_engine_brake, &(car.engine_brake_setting + 1).to_string());
            gui.set_text(self.lbl_mode, &translate(if car.ers_heat_charging { "BATTERY" } else { "MOTOR" }));
        }
        gui.control_render(id, dt, painter);
    }
}
