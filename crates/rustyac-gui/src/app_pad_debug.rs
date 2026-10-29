// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `X360ControllerDebug` (0x1d8), the developers' "X360 Controller debug": the Xbox pad's
//! steering (`JoypadCarControl`) in coloured text, five of its parameters with - and + buttons
//! (`LRControl`) and a Save button that writes them into `cfg/controls.ini [X360]`.
//!
//! `X360ControllerDebug::X360ControllerDebug(Sim*)` 0x1403276d0, `render` 0x140328f40,
//! `printStringList` 0x140328c50, `LRControl::LRControl` 0x140327460, the Save button's lambda
//! 0x140327eb0.

use std::any::Any;
use std::rc::Rc;

use crate::acos::format_g6;
use crate::app_setup_debug::floor4;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};

/// `JoypadCarControl`'s fields from +0x18 on: speedSensitivity, steerGamma, steerFilter,
/// deadZone, steerSpeed, lastMaxSteer, lastRaw, lastNormalized. The buttons change the first five.
pub type PadTuning = [f32; 8];

/// `float increment` 0.001f (Steer Gamma's buttons step `increment * 10.0f`).
const INCREMENT: f32 = 0.001;

pub struct PadDebug {
    y: f32,
}

impl Gui {
    /// `X360ControllerDebug::X360ControllerDebug` 0x1403276d0: 400 x 300; the buttons are made
    /// whatever the controls are (the game's then write through a null pad: the port's do
    /// nothing).
    pub fn new_pad_debug_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "X360_CONTROL_DEBUG", false);
        self.form_set_title(form, &translate("X360 Controller debug"));
        self.form_set_size(form, 400.0, 300.0);
        // (the label, y, the field, the step)
        let rows = [("Speed Sens", 50.0, 0, INCREMENT), ("Steer Gamma", 100.0, 1, INCREMENT * 10.0), ("Steer Filter", 150.0, 2, INCREMENT), ("Deadzone", 200.0, 3, INCREMENT), ("Speed", 250.0, 4, INCREMENT)];
        for (name, y, field, step) in rows {
            // LRControl::LRControl 0x140327460: two 50 x 50 controls, the second 150 to the right
            let name = translate(name);
            for (control, x, sign, suffix) in [("DEBUG_CONTROL_L", 200.0, -1.0f32, "\n -"), ("DEBUG_CONTROL_R", 350.0, 1.0, "\n +")] {
                let id = self.new_control(painter, control);
                self.set_position(id, x, y);
                self.set_size(id, 50.0, 50.0);
                self.control_add_control(form, id);
                self.set_text(id, &format!("{name}{suffix}"));
                self.controls[id].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    let Some(p) = gui.apps.pad.as_mut() else { return };
                    // (x - increment, increment + x: the same sum)
                    p[field] += sign * step;
                    let value = p[field];
                    gui.apps.actions.push(AppAction::SetPadTuning { index: field, value });
                }));
            }
        }
        let save = self.new_control(painter, "SAVE_CFG");
        self.set_size(save, 100.0, 50.0);
        let (f, b) = (self.controls[form].rect, self.controls[save].rect);
        self.set_position(save, 20.0, (f.bottom - f.top) - (b.bottom - b.top));
        self.control_add_control(form, save);
        // lambda 0x140327eb0: five keys of Documents' cfg/controls.ini (a stream: %g), a message
        self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let Some(p) = gui.apps.pad else { return };
            for (key, v) in [("STEER_GAMMA", p[1]), ("STEER_FILTER", p[2]), ("SPEED_SENSITIVITY", p[0]), ("STEER_DEADZONE", p[3]), ("STEER_SPEED", p[4])] {
                gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/controls.ini".into(), section: "X360".into(), key: key.into(), value: format_g6(v) });
            }
            gui.apps.actions.push(AppAction::AddMessage { title: "X360 Parameters".into(), text: translate("Saved") });
        }));
        self.set_text(save, &translate("Save"));
        self.set_app(form, Box::new(PadDebug { y: 0.0 }));
        form
    }
}

impl PadDebug {
    /// `printStringList(name, value, 4.0, color)` 0x140328c50: "name : value" at (0, y) of the
    /// form in the GUI's own font, its colour changed for good.
    fn print(&mut self, gui: &mut Gui, id: Id, painter: &mut dyn Painter, name: &str, value: f32, color: [f32; 4]) {
        let p = gui.local_to_world(id, [0.0, self.y]);
        let font = gui.font;
        gui.fonts[font].set_color4(&color);
        gui.fonts[font].blit_string(painter, p[0], p[1], &format!("{} : {}", translate(name), format_g6(floor4(value))), 1.0, ALIGN_LEFT);
        self.y += 20.0;
    }
}

impl App for PadDebug {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `X360ControllerDebug::render` 0x140328f40: the form, then with a pad its values.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        self.y = 35.0;
        let Some(p) = gui.apps.pad else { return };
        let (yellow, magenta, white) = ([1.0, 1.0, 0.0, 1.0], [1.0, 0.0, 1.0, 1.0], [1.0; 4]);
        for (name, v, color) in [
            ("RAW", p[6], yellow),
            ("Filtered", p[7], yellow),
            ("% of steer", p[5], yellow),
            ("Steering Speed", p[4], magenta),
            ("DeadZone", p[3], magenta),
            ("Steer Filter", p[2], magenta),
            ("Steer Gamma", p[1], magenta),
            ("Speed Sens.", p[0], white),
        ] {
            self.print(gui, id, painter, name, v, color);
        }
    }
}
