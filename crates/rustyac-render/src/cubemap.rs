// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The reflection cube map: `CubeMapRenderer::render` 0x14021edb0, `Camera::render`
//! 0x14020f160 (the plain camera that draws a face), the cube-map branch of `SkyBox::render`
//! 0x14021d0d0 and `Sim::initStaticCubemap` 0x14019a2a0.
//!
//! With `[CUBEMAP] FACES_PER_FRAME=0` no face is drawn during a frame; what the reflections
//! show is what was drawn once at load: a small model (`content/objects3D/cubemap_model.kn5`,
//! a ground and a ring of trees) and the sky, seen from where the scene camera was then.

use rustyac_physics::vecmath::Mat44f;

use crate::camera::Camera;
use crate::forward::{CameraForward, CubeMapRenderer};
use crate::graphics::{Graphics, DEPTH_NORMAL, DEPTH_OFF};
use crate::kgl::KglCubeMap;
use crate::material::{MaterialFilter, PASS_OPAQUE};
use crate::model::Kn5Io;
use crate::scene::{NodeId, NodeKind, RenderContext, Scene};
use crate::sky::SkyBox;

/// The six world matrices of the face camera, as the constructor of `CubeMapRenderer`
/// 0x14021e650 leaves them (rows right, up, back; it looks at -X, +X, +Y, -Y, +Z, -Z). The
/// zeros with a sign are the game's.
pub fn face_matrices() -> [Mat44f; 6] {
    const N: f32 = -0.0;
    let face = |r0: [f32; 3], r1: [f32; 3], r2: [f32; 3]| Mat44f { m: [[r0[0], r0[1], r0[2], 0.0], [r1[0], r1[1], r1[2], 0.0], [r2[0], r2[1], r2[2], 0.0], [0.0, 0.0, 0.0, 1.0]] };
    [
        face([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, N, N]),
        face([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, N, N]),
        face([-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [N, -1.0, N]),
        face([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [N, 1.0, N]),
        face([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [N, N, -1.0]),
        face([1.0, N, N], [0.0, 1.0, 0.0], [N, N, 1.0]),
    ]
}

/// `CubeMapRenderer::render` 0x14021edb0. It runs every frame, also when no face is drawn:
/// the mip levels are made again and the cube is bound to pixel-shader slot 10.
pub fn render(renderer: &mut CubeMapRenderer, graphics: &mut Graphics, scene: &mut Scene, cube: &KglCubeMap, node: NodeId, main_position: [f32; 3], mut sky: Option<&mut SkyBox>) {
    graphics.state.override_no_ms = true;
    graphics.kgl.set_sampler_ps(&graphics.samplers.linear_simple, 0);
    graphics.clear_texture_slot(10);
    graphics.current_cube_map = false;
    // evOnRenderBegin
    if renderer.faces_per_frame > 0 {
        for &mesh in &graphics.cube_map_hidden {
            scene.nodes[mesh].is_active = false;
        }
    }
    let mut n = 0;
    while n < renderer.faces_per_frame {
        let face = renderer.current_face as usize;
        graphics.kgl.cube_map_begin_face(cube, face);
        renderer.camera.matrix = renderer.camera_matrix[face];
        renderer.camera.matrix.m[3][0] = main_position[0];
        renderer.camera.matrix.m[3][1] = main_position[1];
        renderer.camera.matrix.m[3][2] = main_position[2];
        camera_render(&mut renderer.camera, sky.as_deref_mut(), graphics, scene, node);
        renderer.current_face += 1;
        if renderer.current_face >= 6 {
            renderer.current_face = 0;
        }
        n += 1;
    }
    graphics.kgl.set_render_targets(None, None);
    graphics.kgl.cube_map_generate_mips(cube, -1);
    // evOnRenderEnd
    if renderer.faces_per_frame > 0 {
        for &mesh in &graphics.cube_map_hidden {
            scene.nodes[mesh].is_active = true;
        }
    }
    graphics.kgl.set_texture_cube_map(Some(cube), 10);
    graphics.current_cube_map = true;
    graphics.kgl.set_sampler_ps(&graphics.samplers.aniso, 0);
    graphics.state.override_no_ms = false;
}

/// `Camera::render` 0x14020f160: the camera, the clear, the sky and one walk for the opaque
/// meshes. No transparent pass, and no cull, depth or render-target set-up of its own.
pub fn camera_render(camera: &mut Camera, sky: Option<&mut SkyBox>, graphics: &mut Graphics, scene: &mut Scene, node: NodeId) {
    let max_layer = camera.max_layer as i32;
    let mut filter = MaterialFilter::plain();
    camera.render_camera(graphics);
    camera.clear_buffers(graphics);
    if let Some(sky) = sky {
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_depth_mode(DEPTH_OFF);
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
        sky.render(graphics, &mut rc, camera.is_cube_map_camera);
        graphics.set_depth_mode(DEPTH_NORMAL);
    }
    let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
    scene.render(graphics, node, &mut rc);
}

/// `SkyBox::render` 0x14021d0d0 for the cube-map camera: seven colours of the lighting are
/// multiplied by `SKYBOX_REFLECTION_GAIN` for this one draw and the dome is drawn with
/// `ksSkyCubemap`.
pub fn render_sky(sky: &mut SkyBox, graphics: &mut Graphics, rc: &mut RenderContext) {
    let saved = graphics.lighting.clone();
    let scale = |v: &mut rustyac_physics::vecmath::Vec3f, g: f32| {
        v.x = g * v.x;
        v.y = g * v.y;
        v.z = g * v.z;
    };
    let g = sky.skybox_gain;
    scale(&mut graphics.lighting.ambient_high, g);
    scale(&mut graphics.lighting.ambient_low, g);
    scale(&mut graphics.lighting.horizon_high, g);
    scale(&mut graphics.lighting.horizon_low, g);
    scale(&mut graphics.lighting.light_color, g);
    scale(&mut graphics.lighting.sky_low, g);
    scale(&mut graphics.lighting.sky_high, g);
    let saved_shader = sky.material.shader;
    sky.material.shader = Some(sky.cubemap_sky_shader);
    graphics.update_lighting_settings();
    sky.render_mesh(graphics, rc);
    graphics.lighting = saved;
    graphics.update_lighting_settings();
    sky.material.shader = saved_shader;
}

/// `Sim::initStaticCubemap` 0x14019a2a0: once, after loading. `track_folder` is the game's
/// name for the track's folder (`content/tracks/<track>`).
pub fn init_static_cubemap(camera: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, track_folder: Option<&str>) -> Result<(), String> {
    let model = load_static_cubemap_model(graphics, scene, track_folder)?;
    render_static_cubemap(camera, graphics, scene, model);
    Ok(())
}

/// The first half of [`init_static_cubemap`]: the model, loaded and compiled.
pub fn load_static_cubemap_model(graphics: &mut Graphics, scene: &mut Scene, track_folder: Option<&str>) -> Result<NodeId, String> {
    let game = crate::model::path_text(&graphics.game_folder);
    let mut name = match track_folder {
        Some(folder) => format!("{folder}/cubemap_model.kn5"),
        None => String::new(),
    };
    if name.is_empty() || !std::path::Path::new(&name).is_file() {
        name = format!("{game}/content/objects3D/cubemap_model.kn5");
    }
    let io = Kn5Io::new();
    let model = io.load(graphics, scene, &name, std::path::Path::new(&name))?;
    scene.compile(graphics, model);
    Ok(model)
}

/// The second half of [`init_static_cubemap`]: all six faces, once; then the model goes.
pub fn render_static_cubemap(camera: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, model: NodeId) {
    let saved = camera.cube_map_renderer.faces_per_frame;
    camera.cube_map_renderer.faces_per_frame = 6;
    let position = [camera.base.camera.matrix.m[3][0], camera.base.camera.matrix.m[3][1], camera.base.camera.matrix.m[3][2]];
    render(&mut camera.cube_map_renderer, graphics, scene, &camera.cube_map, model, position, camera.base.sky_box.as_mut());
    camera.cube_map_renderer.faces_per_frame = saved;
    // `delete model`: its buffers go, its textures stay in the store
    release(scene, model);
}

fn release(scene: &mut Scene, node: NodeId) {
    if let NodeKind::Mesh(mesh) = &mut scene.nodes[node].kind {
        mesh.vb = None;
        mesh.ib = None;
        mesh.vertices = Vec::new();
        mesh.indices = Vec::new();
    }
    if let NodeKind::SkinnedMesh(mesh) = &mut scene.nodes[node].kind {
        mesh.vb = None;
        mesh.ib = None;
        mesh.bones_buffer = None;
        mesh.vertices = Vec::new();
        mesh.indices = Vec::new();
    }
    let children = scene.nodes[node].children.clone();
    for child in children {
        release(scene, child);
    }
}
