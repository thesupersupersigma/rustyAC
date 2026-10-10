// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The gates of the time-attack mode: one textured quad across the road at the next time line
//! (`content/texture/checkpoint.png`), drawn when the transparent pass reaches
//! `BEFORE_CARS_NODE`. `TimeAttack` (0x2e0 bytes): `loadOverlaysAndSectors` 0x1401bbc20, the
//! node-render handler 0x1401b8dd0, `mat44f::createTarget` 0x14005ff50; which gate shows is
//! written by `renderGuiStatus` 0x1401bd120 in the HUD pass (one frame late).
//!
//! The gates' places come from the track's `data/overlays.ini`. The game writes that file
//! into the track's folder the first time the mode is used there
//! (`createAndSaveOverlays` 0x1401b9b00); this port only reads it: a track that has none shows
//! no gate.

use std::path::Path;

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use crate::fake_shadow::set_from_heading_up;
use crate::gl::GL_QUADS;
use crate::graphics::{Graphics, BLEND_ALPHA, CULL_NONE};
use crate::material::PASS_TRANSPARENT;
use crate::scene::{NodeEventHandler, OnNodeRenderEvent, Scene};
use crate::texture::Texture;

/// `TimeAttack::Overlay` (0x30 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay {
    pub is_active: bool,
    pub world_position: [f32; 3],
    pub offset: [f32; 3],
    pub height: f32,
    pub width: f32,
    pub orientation: [f32; 3],
}

/// `TimeAttack::loadOverlaysAndSectors` 0x1401bbc20: `[CHECKPOINT_i]` for each of the track's
/// `sectors` time lines (a missing section gives zeros). Empty without the file.
pub fn read_overlays(data_folder: &Path, sectors: usize) -> Vec<Overlay> {
    let file = data_folder.join("data/overlays.ini");
    if !file.is_file() {
        return Vec::new();
    }
    let Ok(ini) = IniReader::load(&file) else { return Vec::new() };
    (0..sectors)
        .map(|i| {
            let section = format!("CHECKPOINT_{i}");
            let get3 = |key: &str| ini.get_float3(&section, key).unwrap_or([0.0; 3]);
            let world_position = get3("WORLD_POSITION");
            let offset = get3("OFFSET");
            let height = ini.get_float(&section, "HEIGHT").unwrap_or(0.0);
            let width = ini.get_float(&section, "WIDTH").unwrap_or(0.0);
            Overlay { is_active: false, world_position, offset, height, width, orientation: get3("ORIENTATION") }
        })
        .collect()
}

/// `mat44f::createTarget` 0x14005ff50: a matrix at `eye` whose -z looks at `target` and whose
/// up is on the sky's side.
pub fn create_target(eye: &[f32; 3], target: &[f32; 3]) -> Mat44f {
    let mut m = Mat44f::IDENTITY;
    let (mut dx, mut dy, mut dz) = (target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]);
    let l = sqrtf((dx * dx + dy * dy) + dz * dz);
    if l != 0.0 && !l.is_nan() {
        let inv = 1.0 / l;
        dx *= inv;
        dy *= inv;
        dz *= inv;
    }
    let mut vx = dy * 0.0 - dz;
    let mut vy = dz * 0.0 - dx * 0.0;
    let mut vz = dx - dy * 0.0;
    let l2 = sqrtf((vy * vy + vx * vx) + vz * vz);
    if l2 != 0.0 && !l2.is_nan() {
        let inv = 1.0 / l2;
        vx *= inv;
        vy *= inv;
        vz *= inv;
    }
    let mut wx = vz * dy - dz * vy;
    let mut wy = vx * dz - vz * dx;
    let mut wz = dx * vy - vx * dy;
    let l3 = sqrtf((wy * wy + wx * wx) + wz * wz);
    if l3 != 0.0 && !l3.is_nan() {
        let inv = 1.0 / l3;
        wx *= inv;
        wy *= inv;
        wz *= inv;
    }
    if !(wy >= 0.0) {
        wx *= -1.0;
        wy *= -1.0;
        wz *= -1.0;
    }
    set_from_heading_up(&mut m, &[dx, dy, dz], &[wx, wy, wz]);
    m.m[3][0] = eye[0];
    m.m[3][1] = eye[1];
    m.m[3][2] = eye[2];
    m
}

/// The gates and their texture: the handler of `BEFORE_CARS_NODE`'s render event.
pub struct TimeAttackGates {
    pub overlays: Vec<Overlay>,
    pub checkpoint_texture: Texture,
    /// `ReplayManager::isInReplaymode`: no gate is drawn
    pub in_replay: bool,
}

impl TimeAttackGates {
    /// The gates' part of `TimeAttack::TimeAttack` 0x1401b7e20.
    pub fn new(graphics: &mut Graphics, overlays: Vec<Overlay>) -> TimeAttackGates {
        let game = crate::model::path_text(&graphics.game_folder);
        let checkpoint_texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/checkpoint.png"));
        TimeAttackGates { overlays, checkpoint_texture, in_replay: false }
    }

    /// The head of `TimeAttack::renderGuiStatus` 0x1401bd120 (the HUD pass, after the scene):
    /// while the mode runs, the gate of the next time line shows and no other.
    pub fn set_active(&mut self, running: bool, sector_index: usize, sectors: usize) {
        for (i, overlay) in self.overlays.iter_mut().enumerate() {
            overlay.is_active = running && sectors != 0 && i == (sector_index + 1) % sectors;
        }
    }
}

impl NodeEventHandler for TimeAttackGates {
    /// The handler 0x1401b8dd0. Depth mode and shader are the pass's; nothing is put back.
    fn on_node_render(&mut self, _scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        if self.in_replay || event.pass_id != PASS_TRANSPARENT {
            return;
        }
        for i in 0..self.overlays.len() {
            let ov = self.overlays[i];
            if !ov.is_active {
                continue;
            }
            let p = ov.world_position;
            let target = [p[0] + ov.orientation[0], p[1] + ov.orientation[1], p[2] + ov.orientation[2]];
            let matrix = create_target(&p, &target);
            graphics.set_world_matrix(&matrix);
            graphics.set_cull_mode(CULL_NONE);
            graphics.set_blend_mode(BLEND_ALPHA);
            graphics.set_texture(0, &self.checkpoint_texture);
            let Some(mut gl) = graphics.gl.take() else { return };
            gl.color4f(1.5, 1.5, 1.5, 1.0);
            gl.begin(GL_QUADS, None);
            let (hw, hh) = (ov.width * 0.5, ov.height * 0.5);
            let [ox, oy, oz] = ov.offset;
            gl.tex_coord2f(1.0, 1.0);
            gl.vertex3f(ox - hw, oy - hh, oz);
            gl.tex_coord2f(0.0, 1.0);
            gl.vertex3f(hw + ox, oy - hh, oz);
            gl.tex_coord2f(0.0, 0.0);
            gl.vertex3f(hw + ox, hh + oy, oz);
            gl.tex_coord2f(1.0, 0.0);
            gl.vertex3f(ox - hw, hh + oy, oz);
            gl.end(graphics);
            graphics.gl = Some(gl);
        }
    }
}
