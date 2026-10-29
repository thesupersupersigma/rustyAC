// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ChaseCamEditor` (0x1f0), the developers' "ChaseCam Editor": three spinners that move the
//! first chase camera (distance and height in tenths of a metre, pitch in degrees) while it is
//! on screen, and two buttons that write it into the game's `system/cfg/chase_cam.ini` as
//! `CHASE_0` or `CHASE_1`.
//!
//! `ChaseCamEditor::ChaseCamEditor(Sim*)` 0x1403372a0, `render` 0x140338190, the spinners'
//! lambdas 0x140338060 / 0x140338090 / 0x140338010, the save lambdas 0x140337d00 (CHASE_0) and
//! 0x140337af0 (CHASE_1).

use std::any::Any;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::painter::Painter;
use crate::widgets::WidgetEvent;

pub struct ChaseCamEditor {
    pub spinners: [Id; 3],
    pub buttons: [Id; 2],
    pub inactive: Id,
}

/// The file both buttons write (the game's own folder; rustyAC keeps its copy in its cfg folder).
pub const CHASE_CAM_INI: &str = "system/cfg/chase_cam.ini";

impl Gui {
    /// `ChaseCamEditor::ChaseCamEditor` 0x1403372a0: 400 x 400.
    pub fn new_chase_cam_editor_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "CHASE_CAM_EDITOR", false);
        self.form_set_title(form, "ChaseCam Editor");
        self.form_set_size(form, 400.0, 400.0);
        let (distance, height, pitch) = self.apps.chase_cam;
        // (name, y, min, max, the value shown, the handler's factor)
        let rows = [
            ("Distance", 40.0, 0, 1000, (distance * 10.0) as i32, f32::from_bits(0x3dcc_cccd)),
            ("Height", 80.0, 0, 1000, (height * 10.0) as i32, f32::from_bits(0x3dcc_cccd)),
            ("Pitch", 120.0, -45, 45, (pitch * f32::from_bits(0x4265_2ee1)) as i32, f32::from_bits(0x3c8e_f998)),
        ];
        let spinners = std::array::from_fn(|i| {
            let (name, y, min, max, value, factor) = rows[i];
            let sp = self.new_spinner(painter, name, false, false);
            self.control_add_control(form, sp);
            self.spinner_set_position(sp, 10.0, y);
            self.spinner_set_size(sp, 150.0, 20.0);
            let s = self.spinner_mut(sp);
            s.min_value = min;
            s.max_value = max;
            // Spinner::setValue before the handler is hooked: nobody hears of it
            self.spinner_set_value(sp, value);
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                let v = *value as f32 * factor;
                let c = &mut gui.apps.chase_cam;
                match i {
                    0 => c.0 = v,
                    1 => c.1 = v,
                    _ => c.2 = v,
                }
            }));
            sp
        });
        let buttons = std::array::from_fn(|i| {
            let section = ["CHASE_0", "CHASE_1"][i];
            let b = self.new_control(painter, ["Save chase 0", "Save chase 1"][i]);
            self.control_add_control(form, b);
            self.set_size(b, 150.0, 30.0);
            self.set_position(b, [10.0, 200.0][i], 350.0);
            self.set_text(b, &format!("Save {section}"));
            // (both write chaseCamData[0], the camera the spinners move)
            self.controls[b].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                let (distance, height, pitch) = gui.apps.chase_cam;
                for (key, v) in [("DISTANCE", distance), ("HEIGHT", height), ("PITCH", pitch)] {
                    gui.apps.actions.push(AppAction::WriteProfileString { file: CHASE_CAM_INI.into(), section: section.into(), key: key.into(), value: to_wstring(v) });
                }
                gui.apps.actions.push(AppAction::AddMessage { title: format!("Global chase_cam.ini saved for {section}"), text: String::new() });
            }));
            b
        });
        let inactive = self.new_label(painter, "INACTIVE");
        self.control_add_control(form, inactive);
        self.set_position(inactive, 50.0, 200.0);
        self.set_text(inactive, "Select Chase Cam 0 to activate");
        self.set_app(form, Box::new(ChaseCamEditor { spinners, buttons, inactive }));
        form
    }
}

/// `std::to_wstring(float)`: `%f`.
fn to_wstring(v: f32) -> String {
    format!("{:.6}", v as f64)
}

impl App for ChaseCamEditor {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ChaseCamEditor::render` 0x140338190: the editor works only on the first chase camera.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let on = gui.apps.chase_cam_active;
        let [distance, height, pitch] = self.spinners;
        for c in [self.buttons[1], self.buttons[0], distance, pitch, height] {
            gui.set_visible(c, on);
        }
        gui.set_visible(self.inactive, !on);
        gui.control_render(id, dt, painter);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn to_wstring_is_percent_f() {
        assert_eq!(super::to_wstring(3.0), "3.000000");
        assert_eq!(super::to_wstring(f32::from_bits(0xbc8e_f998)), "-0.017453");
    }
}
