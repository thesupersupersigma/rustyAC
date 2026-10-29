// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `KeyboardControlsDebug` (0x1f8), the developers' "Keyboard debug": the keyboard's steering
//! parameters (`KeyboardCarControl`) in red text, five of them with spinners that change them
//! while driving.
//!
//! `KeyboardControlsDebug::KeyboardControlsDebug(CarAvatar*)` 0x14030dbe0, `render` 0x14030ef10,
//! `onSpinnerValueChanged` 0x14030e8c0, `printStringList` 0x14030ec40.

use std::any::Any;
use std::rc::Rc;

use crate::acos::format_g6;
use crate::app_setup_debug::floor4;
use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};
use crate::widgets::WidgetEvent;

/// `KeyboardCarControl`'s parameters in the order the app prints them: steerSpeed,
/// steerOppositeDirectionFactor, steerGain, steerResetFactor, lookAhead, speedDeceleringFactor,
/// turnDeceleringFactor, steerDecelerator, intGas, intSteer, minSteering. The spinners change
/// the first five.
pub type KeyboardTuning = [f32; 11];

const LINES: [&str; 11] = [
    "Steering Speed",
    "Opposite Direction Speed",
    "Steering Gain",
    "Steering Reset",
    "LookAhead Points",
    "Speed Decelering Factor",
    "Turn Decelering Factor",
    "Steer Decelerator Factor",
    "Int Gas",
    "Int Steer",
    "Min Steer",
];

pub struct KeyboardDebug {
    y: f32,
}

impl Gui {
    /// `KeyboardControlsDebug::KeyboardControlsDebug` 0x14030dbe0: 600 x 600, a red Consolas 10
    /// drawn at 20; the spinners only when the car's controls are the keyboard's
    /// (`gui.apps.keyboard`).
    pub fn new_keyboard_debug_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "KEY_DEBUG", false);
        self.form_set_title(form, &translate("Keyboard debug"));
        self.form_set_size(form, 600.0, 600.0);
        // (made at 10, its scale then set to 20)
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        if let Some(k) = self.apps.keyboard {
            // (name, y, the value shown)
            let rows = [
                ("Steering Speed", 175.0, (k[0] * 0.2 * 100.0) as i32),
                ("SteerChangeDirectionSpeed", 250.0, (k[1] * 10.0) as i32),
                ("SteerGain from spline", 325.0, (k[2] * 100.0) as i32),
                ("SteerReset Speed", 400.0, (k[3] * 10.0) as i32),
                ("LookAhead Points", 475.0, k[4] as i32),
            ];
            let spinners: Vec<Id> = rows
                .iter()
                .map(|(name, y, value)| {
                    let sp = self.new_spinner(painter, &translate(name), false, false);
                    self.controls[sp].back_color = [1.0, 0.0, 0.0, 0.5];
                    let s = self.spinner_mut(sp);
                    s.min_value = 0;
                    s.max_value = 100;
                    self.spinner_set_position(sp, 400.0, *y);
                    self.spinner_set_size(sp, 150.0, 35.0);
                    // (before the handlers are hooked: nobody hears of it)
                    self.spinner_set_value(sp, *value);
                    self.control_add_control(form, sp);
                    sp
                })
                .collect();
            // onSpinnerValueChanged 0x14030e8c0: (float)value * 0.01f * 5.0f, * 0.1f, * 0.01f, * 0.1f, as is
            for (i, sp) in spinners.into_iter().enumerate() {
                self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                    let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                    let v = match i {
                        0 => *value as f32 * 0.01 * 5.0,
                        1 | 3 => *value as f32 * 0.1,
                        2 => *value as f32 * 0.01,
                        _ => *value as f32,
                    };
                    if let Some(k) = gui.apps.keyboard.as_mut() {
                        k[i] = v;
                    }
                    gui.apps.actions.push(AppAction::SetKeyboardTuning { index: i, value: v });
                }));
            }
        }
        self.set_app(form, Box::new(KeyboardDebug { y: 15.0 }));
        form
    }
}

impl KeyboardDebug {
    /// `printStringList(name, value, 4.0)` 0x14030ec40: "name : value" at (0, y) of the form.
    fn print(&mut self, gui: &mut Gui, id: Id, painter: &mut dyn Painter, name: &str, value: f32) {
        let p = gui.local_to_world(id, [0.0, self.y]);
        let font = gui.controls[id].font.unwrap_or(gui.font);
        gui.fonts[font].blit_string(painter, p[0], p[1], &format!("{name} : {}", format_g6(floor4(value))), 1.0, ALIGN_LEFT);
        self.y += 20.0;
    }
}

impl App for KeyboardDebug {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `KeyboardControlsDebug::render` 0x14030ef10: the form, then the parameters.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        self.y = 15.0;
        match gui.apps.keyboard {
            None => self.print(gui, id, painter, &translate("KEYBOARD CONTROL NOT SELECTED"), 0.0),
            Some(k) => {
                for (name, v) in LINES.iter().zip(k) {
                    self.print(gui, id, painter, &translate(name), v);
                }
            }
        }
    }
}
