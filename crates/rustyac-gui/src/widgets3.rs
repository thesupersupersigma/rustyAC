// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The widget set of ksGUI, part three.

use crate::control::{Id, MouseEvent};
use crate::gui::Gui;
use crate::painter::Painter;

pub enum Widget3 {}

impl Gui {
    pub fn widget3_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
    }

    pub fn widget3_set_size(&mut self, id: Id, w: f32, h: f32) {
        self.control_set_size(id, w, h);
    }

    pub fn widget3_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        self.control_on_mouse_down(id, e)
    }

    pub fn widget3_on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        self.control_on_mouse_up(id, e);
    }

    pub fn widget3_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        self.control_on_mouse_move(id, e);
    }

    pub fn widget3_on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        self.control_on_mouse_wheel(id, e);
    }

    pub fn widget3_on_key_char(&mut self, id: Id, key: u32) {
        self.control_on_key_char(id, key);
    }

    pub fn widget3_on_key_down(&mut self, id: Id, key: u32) {
        self.control_on_key_down(id, key);
    }

    pub fn widget3_set_text(&mut self, id: Id, text: &str) {
        self.controls[id].text = text.to_string();
    }

    pub fn widget3_scale_by_mult(&mut self, id: Id, m: f32) {
        self.control_scale_by_mult(id, m);
    }

    pub fn widget3_set_repeat_interval(&mut self, id: Id, v: f32) {
        self.controls[id].repeat_interval = v;
    }
}
