// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormOpenVR` (0x1e8), "OpenVR": SteamVR's supersampling (asked for at Save, used after a
//! restart) and the pupillary distance (applied at once, written to `cfg\openvr.ini` at Save).
//! Made only with the HTC Vive camera (`Sim::Sim`).
//!
//! Ctor 0x1402fd030; the handlers 0x1402fdd70 (supersampling), 0x1402fde50 (distance), Save's
//! click 0x1402fdb80.

use std::any::Any;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};
use crate::widgets::WidgetEvent;

/// What the form reads from `StereoCameraVive` when it is made.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenVrValues {
    /// `getRenderMultiplier`: SteamVR's `[steamvr] renderTargetMultiplier`
    pub render_multiplier: f32,
    pub eyes_distance_ref: f32,
    pub eyes_distance: f32,
    pub max_distance_offset: f32,
}

pub struct OpenVrForm;

impl Gui {
    /// `FormOpenVR::FormOpenVR(Sim*)`.
    pub fn new_open_vr_app(&mut self, painter: &mut dyn Painter, v: OpenVrValues) -> Id {
        let form = self.new_form(painter, "FORM_OPENVR", false);
        self.form_data_mut(form).dev_app = false;
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 18.0, false, false));
        let font18 = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 15.0, false, false));
        let font15 = self.fonts.len() - 1;
        self.form_set_title(form, "OpenVR");

        let super_sampling = self.new_spinner(painter, "SUPERSAMPLING *", true, false);
        self.set_size(super_sampling, 310.0, 23.0);
        self.spinner_set_position(super_sampling, 30.0, 70.0);
        for i in 8..25 {
            self.spinner_add_item(super_sampling, &format!("{:.2}", i as f32 * 0.1));
        }
        self.add_control(form, super_sampling);
        let m = v.render_multiplier;
        // (a NaN fails both tests: 2.4)
        let m = if !(m <= 2.4) { 2.4 } else if !(0.8 <= m) { 0.8 } else { m };
        self.spinner_set_value_by_item(super_sampling, &format!("{m:.2}"));

        let eyes = self.new_spinner(painter, "PUPILLARY DISTANCE(mm)", true, false);
        self.set_size(eyes, 310.0, 23.0);
        self.spinner_set_position(eyes, 30.0, 140.0);
        let max = (v.max_distance_offset * 1000.0) as i32;
        let reference = (v.eyes_distance_ref * 1000.0) as i32;
        for i in -max..=max {
            self.spinner_add_item(eyes, &(i + reference).to_string());
        }
        self.add_control(form, eyes);
        let d = (v.eyes_distance * 1000.0) as i32 - reference;
        let d = if d > max { max } else if d < -max { -max } else { d };
        self.spinner_set_value_by_item(eyes, &(d + reference).to_string());

        let save = self.new_control(painter, "SAVE_BUTTON");
        self.controls[save].font = Some(font18);
        self.set_text(save, &translate("Save"));
        self.set_size(save, 60.0, 23.0);
        self.set_repeat_interval(save, f32::from_bits(0x3d4c_cccd));
        self.set_position(save, 275.0, 200.0);
        self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let value = gui.spinner(super_sampling).value;
            gui.apps.actions.push(AppAction::SetVrRenderMultiplier((value + 8) as f32 * 0.1));
            let value = gui.spinner(eyes).value;
            let value = (reference + (value - max)).to_string();
            gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/openvr.ini".into(), section: "SETTINGS".into(), key: "PUPILLARY_DISTANCE".into(), value });
            gui.set_text(save, &translate("Saved"));
        }));
        self.add_control(form, save);

        let warning = self.new_label(painter, "LAB_WARNING");
        self.controls[warning].font = Some(font15);
        self.controls[warning].font_align = ALIGN_LEFT;
        self.set_text(warning, &translate("* supersampling changes requires restart"));
        self.set_size(warning, 100.0, 23.0);
        self.set_position(warning, 20.0, 250.0);
        self.set_visible(warning, true);
        self.add_control(form, warning);
        self.form_set_size(form, 370.0, 280.0);

        // (made after the values are set: the game's handlers wait for `loaded`)
        self.spinner_mut(super_sampling).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if matches!(ev, WidgetEvent::SpinnerValueChanged { .. }) {
                gui.set_text(save, &translate("Save"));
            }
        }));
        self.spinner_mut(eyes).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { value, .. } = *ev else { return };
            gui.apps.actions.push(AppAction::SetVrEyesDistance((value - max) as f32 * 0.001));
            gui.set_text(save, &translate("Save"));
        }));
        self.set_app(form, Box::new(OpenVrForm));
        form
    }
}

impl App for OpenVrForm {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn supersampling_items() {
        let items: Vec<String> = (8..25).map(|i| format!("{:.2}", i as f32 * 0.1)).collect();
        assert_eq!(items.first().map(String::as_str), Some("0.80"));
        assert_eq!(items[5], "1.30");
        assert_eq!(items.last().map(String::as_str), Some("2.40"));
    }
}
