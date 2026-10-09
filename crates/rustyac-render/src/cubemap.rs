// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The reflection cube map: `CubeMapRenderer::render` 0x14021edb0.

use rustyac_physics::vecmath::Mat44f;

use crate::camera::CameraShadowMapped;
use crate::forward::CubeMapRenderer;
use crate::graphics::Graphics;
use crate::kgl::KglCubeMap;
use crate::scene::{NodeId, Scene};

/// The six world matrices of the face camera.
pub fn face_matrices() -> [Mat44f; 6] {
    [Mat44f::IDENTITY; 6]
}

/// `CubeMapRenderer::render` 0x14021edb0. It runs every frame, also when no face is drawn:
/// the mip levels are made again and the cube is bound to pixel-shader slot 10.
pub fn render(renderer: &mut CubeMapRenderer, graphics: &mut Graphics, _scene: &mut Scene, cube: &KglCubeMap, _node: NodeId, _main: &CameraShadowMapped) {
    graphics.state.override_no_ms = true;
    graphics.kgl.set_sampler_ps(&graphics.samplers.linear_simple, 0);
    graphics.clear_texture_slot(10);
    for _ in 0..renderer.faces_per_frame {
        // the faces: see `render_face`
    }
    graphics.kgl.set_render_targets(None, None);
    graphics.kgl.cube_map_generate_mips(cube, -1);
    graphics.kgl.set_texture_cube_map(Some(cube), 10);
    graphics.kgl.set_sampler_ps(&graphics.samplers.aniso, 0);
    graphics.state.override_no_ms = false;
}

/// `SkyBox::render` 0x14021d0d0 for the cube-map camera.
pub fn render_sky(sky: &mut crate::sky::SkyBox, graphics: &mut Graphics, rc: &mut crate::scene::RenderContext) {
    sky.render_mesh(graphics, rc);
}
