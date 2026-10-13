// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's HUD, part two: the objects besides the messages.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;

use crate::gui_port_hud::HudPort;

#[derive(Default)]
pub struct More {}

pub fn create(_hud: &mut HudPort, _gui: &mut Gui, _painter: &mut dyn Painter, _game_folder: &std::path::Path, _screen: Id) -> Result<(), String> {
    Ok(())
}

pub fn call(_hud: &mut HudPort, _gui: &mut Gui, _painter: &mut dyn Painter, what: &str, _number: f32, _text: &str) -> Result<(), String> {
    Err(format!("the port side knows no HUD call {what:?}"))
}

pub fn update(_hud: &mut HudPort, _gui: &mut Gui, _dt: f32) {}

pub fn render_hud(_hud: &mut HudPort, _gui: &mut Gui, _painter: &mut dyn Painter, _dt: f32) {}

pub fn dump(_hud: &HudPort, _gui: &Gui, _out: &mut String) {}
