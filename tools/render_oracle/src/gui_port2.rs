// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle, part two: the classes and HUD objects added after the
//! base.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::widgets::WidgetEvent;

use crate::gui_seq::Val;

#[derive(Default)]
pub struct Hud {}

impl Hud {
    pub fn render(&mut self, _gui: &mut Gui, _painter: &mut dyn Painter, _dt: f32) {}
}

pub fn class_name(_gui: &Gui, _id: Id) -> &'static str {
    "?"
}

pub fn extra(_gui: &Gui, _id: Id) -> String {
    String::new()
}

pub fn new_object(_gui: &mut Gui, _painter: &mut dyn Painter, class: &str, _name: &str, _flag: bool) -> Result<Id, String> {
    Err(format!("the port side knows no class {class:?}"))
}

pub fn set(_gui: &mut Gui, _id: Id, name: &str, _value: &Val) -> Result<(), String> {
    Err(format!("the port side knows no member {name:?}"))
}

pub fn call(_gui: &mut Gui, _painter: &mut dyn Painter, _id: Id, what: &str, _number: f32, _text: &str) -> Result<(), String> {
    Err(format!("the port side knows no call {what:?}"))
}

pub fn event_text(gui: &Gui, event: &WidgetEvent) -> String {
    match event {
        WidgetEvent::CheckBoxChanged { check_box, value } => format!("check \"{}\" {}", gui.controls[*check_box].name, *value as u8),
        WidgetEvent::SliderInteraction { slider, value } => format!("slider \"{}\" {:08x}", gui.controls[*slider].name, value.to_bits()),
        WidgetEvent::CutExtremesChanged { slider, cut_in, cut_out } => format!("cut \"{}\" {:08x} {:08x}", gui.controls[*slider].name, cut_in.to_bits(), cut_out.to_bits()),
        other => format!("{other:?}"),
    }
}
