// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! FW1FontWrapper's interface under kgl on wgpu, drawing no text: the glyphs come from
//! DirectWrite, which only Windows has. shortcut: the HUD's text is missing in Full mode, port
//! the glyph sheets onto kgl when the HUD is drawn in the browser.

use std::path::Path;

use crate::kgl::Kgl;

pub const FW1_CENTER: u32 = 0x1;
pub const FW1_RIGHT: u32 = 0x2;
pub const FW1_NOWORDWRAP: u32 = 0x10;
pub const FW1_ALIASED: u32 = 0x20;
pub const FW1_RESTORESTATE: u32 = 0x800;

#[derive(Default)]
pub struct Fonts {}

impl Fonts {
    pub fn new(_game_folder: &Path) -> Fonts {
        Fonts {}
    }

    pub fn set_locale(&mut self, _locale: &str) {}

    pub fn get_font_wrapper(&mut self, _kgl: &Kgl, _family: &str, _italic: bool, _bold: bool) -> Option<usize> {
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn blit_string(&mut self, _kgl: &Kgl, _wrapper: usize, _x: f32, _y: f32, _text: &[u16], _size: f32, _align: u32, _color: u32) {}
}
