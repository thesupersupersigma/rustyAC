// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's HUD, part three: the objects that follow the cars.

use rustyac_gui::hud::HudInput;

use crate::ac::Game;
use crate::gui_ac_hud::HudGame;

#[derive(Default)]
pub struct Cars {}

pub unsafe fn sync(_h: &HudGame, _state: &HudInput) {}

pub unsafe fn create(_game: &Game, _gui: *mut u8, _screen: *mut u8, _h: &mut HudGame) -> Result<(), String> {
    Ok(())
}

pub unsafe fn call(_game: &Game, _gui: *mut u8, _h: &mut HudGame, what: &str, _number: f32, _text: &str) -> Result<(), String> {
    Err(format!("the game side knows no HUD call {what:?}"))
}

pub unsafe fn update(_game: &Game, _h: &mut HudGame, _dt: f32) {}

pub unsafe fn render_hud(_game: &Game, _h: &mut HudGame, _dt: f32) {}

pub unsafe fn dump(_game: &Game, _h: &HudGame, _out: &mut String) {}
