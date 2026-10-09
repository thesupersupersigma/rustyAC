// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The sky dome: placeholder until the port of `SkyBox`.

use crate::graphics::Graphics;
use crate::scene::RenderContext;

pub struct SkyBox {}

impl SkyBox {
    pub fn render(&mut self, _graphics: &mut Graphics, _rc: &mut RenderContext, _cube_map_camera: bool) {}
}
