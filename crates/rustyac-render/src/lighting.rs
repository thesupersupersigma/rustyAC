// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Lighting: placeholder until the port of `loadLightingSettings` / `updateLightingSetttings`.

use rustyac_physics::vecmath::Vec3f;

use crate::graphics::Graphics;

#[derive(Clone, Debug, Default)]
pub struct LightingSettings {
    pub light_direction: Vec3f,
    pub light_color: Vec3f,
    pub angle: f32,
}

impl LightingSettings {
    pub fn new() -> LightingSettings {
        LightingSettings::default()
    }
}

#[derive(Default)]
pub struct LightingCurves {}

impl Graphics {
    pub fn load_lighting_settings(&mut self, _path: &std::path::Path) {}

    pub fn update_lighting_settings(&mut self) {}
}
