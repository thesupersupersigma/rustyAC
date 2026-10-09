// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CameraForward`: the frame of the plain path (no post-processing, no motion blur):
//! `CameraForward::CameraForward` 0x14021f230, `CameraForward::render` 0x14021fc40,
//! `CubeMapRenderer::render` 0x14021edb0, `CameraForward::setCubemapSize` 0x1402201b0.

use rustyac_physics::vecmath::Mat44f;

use crate::camera::{Camera, CameraShadowMapped};
use crate::graphics::Graphics;
use crate::kgl::KglCubeMap;
use crate::scene::{NodeId, Scene};

/// `CubeMapRenderer` (0x3e0 bytes): the camera that looks along the six axes.
pub struct CubeMapRenderer {
    pub faces_per_frame: i32,
    pub camera_matrix: [Mat44f; 6],
    pub camera: Camera,
    pub current_face: i32,
}

impl CubeMapRenderer {
    /// `CubeMapRenderer::CubeMapRenderer` 0x14021e650.
    pub fn new() -> CubeMapRenderer {
        let mut camera = Camera::new();
        camera.is_cube_map_camera = true;
        camera.fov = 90.0;
        camera.near_plane = f32::from_bits(0x3c23_d70a); // 0.01
        camera.far_plane = 350.0;
        camera.aspect_ratio = 1.0;
        camera.max_layer = 0.0;
        CubeMapRenderer { faces_per_frame: 6, camera_matrix: crate::cubemap::face_matrices(), camera, current_face: 0 }
    }

    /// `CubeMapRenderer::setCameraNearFarPlanes` 0x14021f110.
    pub fn set_camera_near_far_planes(&mut self, near: f32, far: f32) {
        self.camera.near_plane = near;
        self.camera.far_plane = far;
    }
}

impl Default for CubeMapRenderer {
    fn default() -> CubeMapRenderer {
        CubeMapRenderer::new()
    }
}

/// `CameraForward` (0x788 bytes), the members of the plain path.
pub struct CameraForward {
    pub base: CameraShadowMapped,
    pub cube_map_renderer: CubeMapRenderer,
    pub cube_map: KglCubeMap,
}

impl CameraForward {
    /// `CameraForward::CameraForward` 0x14021f230 (motion blur off): the shadow targets, the
    /// cube-map camera and a first cube map of 512.
    pub fn new(graphics: &mut Graphics) -> Result<CameraForward, String> {
        let base = CameraShadowMapped::new(graphics)?;
        let cube_map = graphics.kgl.create_cube_map(512, true, 7);
        Ok(CameraForward { base, cube_map_renderer: CubeMapRenderer::new(), cube_map })
    }

    /// `CameraForward::setCubemapSize` 0x1402201b0: the old cube map goes, a new one is made.
    pub fn set_cubemap_size(&mut self, graphics: &Graphics, size: i32) {
        self.cube_map = graphics.kgl.create_cube_map(size, true, 7);
    }

    /// `CameraForward::render` 0x14021fc40.
    pub fn render(&mut self, graphics: &mut Graphics, scene: &mut Scene, blurred: Option<NodeId>, root: NodeId) -> Result<(), String> {
        graphics.clear_texture_slot(5);
        self.base.shadow_map_pass(graphics, scene, root)?;
        if let Some(blurred) = blurred {
            let position = [self.base.camera.matrix.m[3][0], self.base.camera.matrix.m[3][1], self.base.camera.matrix.m[3][2]];
            crate::cubemap::render(&mut self.cube_map_renderer, graphics, scene, &self.cube_map, blurred, position, self.base.sky_box.as_mut());
        }
        graphics.set_screen_render_targets();
        graphics.stats.is_in_main_render_pass = true;
        self.base.render_pass(graphics, scene, root);
        graphics.stats.is_in_main_render_pass = false;
        graphics.clear_texture_slot(5);
        graphics.clear_texture_slot(3);
        Ok(())
    }
}
