// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CameraTripleScreen`, the camera of video.ini `[CAMERA] MODE=TRIPLE`: one window, three
//! views side by side, each turned to its screen and drawn with its own off-centre projection.
//! `CameraTripleScreen::CameraTripleScreen` 0x1402248e0, `beginVirtualScreenPass` 0x140224e30,
//! `renderPass` 0x140225180.
//!
//! The screens come from Documents `cfg/triple_screen.ini` (`[LEFT_SCREEN]`, `[CENTER_SCREEN]`,
//! `[RIGHT_SCREEN]`, each `LEFT_POINT` and `RIGHT_POINT`: where the screen's edges are seen from
//! the driver's eyes, x across and y ahead, in the units of the centre screen's width).
//! The game's camera is a `CameraForwardYebis`; its one other change, a handler that sets two
//! YEBIS values (godrays length 1, vignetting 0), has nothing to act on here (no YEBIS).

use rustyac_math::{asinf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::camera::{Camera, CameraShadowMapped};
use crate::graphics::{Graphics, RenderTarget, CULL_FRONT, DEPTH_NORMAL, DEPTH_NO_WRITE};
use crate::material::{MaterialFilter, PASS_OPAQUE, PASS_TRANSPARENT};
use crate::scene::{NodeId, RenderContext, Scene};

/// The ini sections of the three screens, left to right.
pub const SECTIONS: [&str; 3] = ["LEFT_SCREEN", "CENTER_SCREEN", "RIGHT_SCREEN"];

/// One screen of `TripleScreenData` (0x14 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VirtualScreen {
    pub left: [f32; 2],
    pub right: [f32; 2],
    pub height: f32,
}

/// `TripleScreenData` and the camera's own triple members.
#[derive(Clone, Debug)]
pub struct TripleScreen {
    /// (+0x698) set by the constructor: off, `renderPass` is the plain one
    pub enabled: bool,
    pub screens: [VirtualScreen; 3],
    /// view × projection of each screen (the name tags read them)
    pub current_view_proj: [Mat44f; 3],
    pub current_camera_matrix: [Mat44f; 3],
    pub current_screen: i32,
    /// (+0x1d0) the camera's matrix before a screen turned it
    saved_matrix: Mat44f,
}

impl TripleScreen {
    /// The constructor's part: the screens of triple_screen.ini, each one's height from its
    /// width and a third of the window's shape.
    pub fn new(ini: &IniReader, video_width: i32, video_height: i32) -> Result<TripleScreen, String> {
        let factor = video_height as f32 / (video_width as f32 * f32::from_bits(0x3eaa_aaab));
        let mut screens = [VirtualScreen::default(); 3];
        for (screen, section) in screens.iter_mut().zip(SECTIONS) {
            screen.left = ini.get_float2(section, "LEFT_POINT")?;
            screen.right = ini.get_float2(section, "RIGHT_POINT")?;
            let dx = screen.left[0] - screen.right[0];
            let dy = screen.left[1] - screen.right[1];
            screen.height = sqrtf(dx * dx + dy * dy) * factor;
        }
        Ok(TripleScreen { enabled: true, screens, current_view_proj: [Mat44f::default(); 3], current_camera_matrix: [Mat44f::default(); 3], current_screen: 0, saved_matrix: Mat44f::default() })
    }

    /// `CameraTripleScreen::beginVirtualScreenPass` 0x140224e30: turn the camera to face screen
    /// `index` and set the projection through that screen's edges.
    pub fn begin_virtual_screen_pass(&mut self, camera: &mut Camera, graphics: &mut Graphics, index: usize) {
        self.current_screen = index as i32;
        self.saved_matrix = camera.matrix;
        let screen = self.screens[index];
        let dx = screen.right[0] - screen.left[0];
        let dy = screen.right[1] - screen.left[1];
        let len = sqrtf(dx * dx + dy * dy);
        let angle = asinf(if len != 0.0 { dy / len } else { dy });
        let (l, r) = if angle == 0.0 {
            (screen.left, screen.right)
        } else {
            let rot = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), angle);
            camera.matrix = xm_matrix_multiply(&rot, &camera.matrix);
            let m = &rot.m;
            let turn = |p: [f32; 2]| [((m[0][0] * p[0] + m[1][0] * 0.0) + m[2][0] * p[1]) + m[3][0], ((m[0][2] * p[0] + m[1][2] * 0.0) + m[2][2] * p[1]) + m[3][2]];
            (turn(screen.left), turn(screen.right))
        };
        let dx = r[0] - l[0];
        let dy = r[1] - l[1];
        let distance = (l[1] * dx - l[0] * dy).abs() / sqrtf(dy * dy + dx * dx);
        let inv = 1.0 / distance;
        let near = camera.near_plane;
        let half = ((screen.height * 0.5) * near) * inv;
        camera.render_camera_custom_perspective(graphics, (near * l[0]) * inv, (near * r[0]) * inv, half, -half);
    }
}

/// `CameraTripleScreen::renderPass` 0x140225180: the sky of each screen, the depth cleared, the
/// opaque meshes of each screen, then the transparent ones; while a screen draws, the window
/// is a third as wide (`videoSettings.width`). No GPU queries, unlike the plain pass.
pub fn render_pass(base: &mut CameraShadowMapped, graphics: &mut Graphics, scene: &mut Scene, node: NodeId, render_target: Option<&RenderTarget>) {
    let CameraShadowMapped { camera, sky_box, triple, .. } = base;
    let Some(triple) = triple.as_deref_mut() else { return };
    graphics.set_depth_mode(DEPTH_NORMAL);
    let max_layer = camera.max_layer as i32;
    let mut filter = MaterialFilter::plain();
    if render_target.is_some() {
        graphics.set_render_target(render_target);
    }
    graphics.set_cull_mode(CULL_FRONT);
    camera.clear_buffers(graphics);
    let (width, height) = (graphics.video.width, graphics.video.height);
    let third = width / 3;
    let screen = |triple: &mut TripleScreen, camera: &mut Camera, graphics: &mut Graphics, index: usize| {
        graphics.set_viewport(index as i32 * third, 0, third, graphics.video.height);
        graphics.video.width = third;
        triple.begin_virtual_screen_pass(camera, graphics, index);
    };
    for index in 0..3 {
        screen(triple, camera, graphics, index);
        if !camera.is_mirror {
            let view = camera.get_view_matrix();
            triple.current_view_proj[index] = xm_matrix_multiply(&view, &graphics.state.projection_matrix);
            triple.current_camera_matrix[index] = camera.matrix;
        }
        if let Some(sky) = sky_box.as_mut() {
            graphics.set_world_matrix(&Mat44f::IDENTITY);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
            sky.render(graphics, &mut rc, camera.is_cube_map_camera);
        }
        camera.matrix = triple.saved_matrix;
        filter.reset_material_cache();
    }
    graphics.clear_render_target_depth(camera.clear_depth);
    graphics.video.width = width;
    graphics.video.height = height;
    graphics.set_viewport(0, 0, width, graphics.video.height);
    for index in 0..3 {
        screen(triple, camera, graphics, index);
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
        scene.render(graphics, node, &mut rc);
        if camera.axis_rendering_mode != 0 {
            camera.render_axis(graphics, camera.axis_rendering_mode == 1);
        }
        camera.matrix = triple.saved_matrix;
        filter.reset_material_cache();
    }
    graphics.set_depth_mode(DEPTH_NO_WRITE);
    for index in 0..3 {
        screen(triple, camera, graphics, index);
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_TRANSPARENT, max_layer, camera: camera.cull_camera() };
        scene.render(graphics, node, &mut rc);
        camera.matrix = triple.saved_matrix;
        filter.reset_material_cache();
    }
    graphics.video.width = width;
    graphics.video.height = height;
    graphics.set_viewport(0, 0, width, graphics.video.height);
    graphics.set_depth_mode(DEPTH_NORMAL);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user's default file: flat centre screen, side screens turned in a little.
    #[test]
    fn screens_from_ini() {
        let ini = IniReader::from_text(std::path::Path::new("triple_screen.ini"), "[LEFT_SCREEN]\nLEFT_POINT=-1.49863,0.947665\nRIGHT_POINT=-0.5,1\n[CENTER_SCREEN]\nLEFT_POINT=-0.5,1\nRIGHT_POINT=0.5,1\n[RIGHT_SCREEN]\nLEFT_POINT=0.5,1\nRIGHT_POINT=1.49863,0.947665\n");
        let t = TripleScreen::new(&ini, 5760, 1080).unwrap();
        assert_eq!(t.screens[1], VirtualScreen { left: [-0.5, 1.0], right: [0.5, 1.0], height: 1.0 * (1080.0 / (5760.0 * f32::from_bits(0x3eaa_aaab))) });
        assert!(TripleScreen::new(&IniReader::from_text(std::path::Path::new("x.ini"), ""), 5760, 1080).is_err());
    }
}
