// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Mirrors: `MirrorTextureRenderer` (0x68 bytes; constructor 0x140113c40, `render`
//! 0x140114410), `CameraMirror` (`renderOpaque` 0x14021bd80, `renderTransparent`
//! 0x14021bf30), `CarMirrorManager` (0x78 bytes; constructor 0x1400e5f20, `update`
//! 0x1400e6fc0) and `VirtualMirrorRenderer` (0x78 bytes; `renderVirtualMirror` 0x1401d1ea0).
//!
//! One texture for the whole game: the focused car's view backwards, drawn before the main
//! picture without shadows, with the car itself hidden. Each car's mirror glass gets two
//! materials that show it (one for the cockpit, one for outside), and the "virtual mirror"
//! is a quad at the top of the screen. With `[MIRROR] HQ=1` the picture is drawn into a
//! multisampled target (as many samples as the screen's `AASAMPLES`) with the whole
//! transparent pass and a far plane of 800 m, and resolved into the texture.

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;

use crate::camera::Camera;
use crate::gl::{GlRenderer, GL_QUADS};
use crate::graphics::{Graphics, RenderTarget};
use crate::material::{Material, MaterialFilter, MaterialId, PASS_OPAQUE, PASS_TRANSPARENT};
use crate::scene::{NodeId, NodeKind, RenderContext, Scene};
use crate::sky::SkyBox;
use crate::texture::Texture;

/// `ACCameraManager::isCameraOnBoard` 0x1400336f0 for the focused car: the cockpit camera,
/// or the dashboard one of the drivable cameras.
pub fn is_camera_on_board(camera_mode: i32, drivable_mode: i32) -> bool {
    camera_mode == 0 || (camera_mode == 2 && drivable_mode == 4)
}

/// `ACCameraManager::isVirtualMirrorOn` 0x140033760: cockpit or drivable cameras.
pub fn is_virtual_mirror_on(camera_mode: i32) -> bool {
    (camera_mode & !2) == 0
}

/// The nodes of the scene and of the focused car that a mirror frame needs.
pub struct MirrorScene<'a> {
    pub root: NodeId,
    pub particles: NodeId,
    pub before_cars: NodeId,
    /// `TrackAvatar::idealLine`
    pub ideal_line: Option<NodeId>,
    /// the focused car's `bodyTransform`
    pub body_transform: NodeId,
    /// what `CarAvatar::setVisible` 0x1400dade0 switches besides the body: the four wheel
    /// nodes and the four suspension nodes
    pub car_nodes: &'a [NodeId],
    /// `car.ini [GRAPHICS] MIRROR_POSITION`
    pub mirror_position: [f32; 3],
}

pub struct MirrorTextureRenderer {
    pub texture: Texture,
    pub render_smoke: bool,
    pub has_been_rendered: bool,
    pub camera: Camera,
    render_target: RenderTarget,
    null_shadow: RenderTarget,
    /// `highQuality` and `renderTargetMS`: `[MIRROR] HQ`
    pub high_quality: bool,
    render_target_ms: Option<RenderTarget>,
}

impl MirrorTextureRenderer {
    /// `MirrorTextureRenderer::MirrorTextureRenderer` 0x140113c40. `size`: `[MIRROR] SIZE` of
    /// `video.ini` (the game makes no renderer when it is 0).
    /// `high_quality`: `[MIRROR] HQ` > 0, with `GraphicsManager::videoSettings.aaSamples`.
    pub fn new(graphics: &mut Graphics, size: i32, render_smoke: bool, high_quality: Option<i32>) -> Result<MirrorTextureRenderer, String> {
        let height = size / 4;
        // the multisampled target first
        let render_target_ms = match high_quality {
            Some(samples) => {
                let color = graphics.kgl.create_render_target(DXGI_FORMAT_R16G16B16A16_FLOAT, size, height, samples, 1);
                let depth = graphics.kgl.create_render_target_depth(size, height, samples)?;
                Some(RenderTarget { kid_color: Some(color), kid_depth: Some(depth), width: size, height })
            }
            None => None,
        };
        let color = graphics.kgl.create_render_target(DXGI_FORMAT_R16G16B16A16_FLOAT, size, height, 1, 1);
        let depth = graphics.kgl.create_render_target_depth(size, height, 1)?;
        let texture = graphics.resources.texture_from_render_target(&color);
        let render_target = RenderTarget { kid_color: Some(color), kid_depth: Some(depth), width: size, height };
        let null_depth = graphics.kgl.create_render_target_depth(1, 1, 1)?;
        let null_shadow = RenderTarget { kid_color: None, kid_depth: Some(null_depth), width: 1, height: 1 };
        let mut camera = Camera::new();
        camera.max_layer = 0.0;
        camera.fov = 10.0;
        camera.far_plane = 400.0;
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/assetto_corsa.ini")) {
            if ini.has_section("MIRRORS") {
                camera.fov = ini.get_float("MIRRORS", "FOV").unwrap_or(0.0);
                camera.far_plane = ini.get_float("MIRRORS", "FAR_PLANE").unwrap_or(0.0);
            }
        }
        camera.aspect_ratio = size as f32 / height as f32;
        if high_quality.is_some() {
            camera.far_plane = 800.0;
        }
        Ok(MirrorTextureRenderer { texture, render_smoke, has_been_rendered: false, camera, render_target, null_shadow, high_quality: high_quality.is_some(), render_target_ms })
    }

    /// The test of `Sim::renderScene` 0x14019e570: the mirror is drawn for an on-board
    /// camera, for the first frame, or while the virtual mirror shows.
    pub fn wants_render(&self, camera_mode: i32, drivable_mode: i32, virtual_mirror_active: bool) -> bool {
        is_camera_on_board(camera_mode, drivable_mode) || !self.has_been_rendered || (virtual_mirror_active && is_virtual_mirror_on(camera_mode))
    }

    /// `MirrorTextureRenderer::render` 0x140114410.
    pub fn render(&mut self, graphics: &mut Graphics, scene: &mut Scene, sky: Option<&mut SkyBox>, s: &MirrorScene) {
        let saved_ideal = s.ideal_line.map(|n| std::mem::replace(&mut scene.nodes[n].is_active, false));
        if let Some(depth) = &self.null_shadow.kid_depth {
            graphics.kgl.render_target_clear(depth, &[0.0, 0.0, 0.0, 1.0], 1.0);
        }
        for i in 0..4 {
            graphics.set_shadow_map_texture(i, Some(&self.null_shadow));
        }
        for i in 0..3 {
            graphics.set_shadow_map_matrix(i, &Mat44f::IDENTITY);
        }
        match &self.render_target_ms {
            None => {
                graphics.kgl.set_sampler_ps(&graphics.samplers.linear_simple, 0);
                graphics.set_render_target(Some(&self.render_target));
            }
            Some(ms) => {
                graphics.kgl.set_sampler_ps(&graphics.samplers.aniso, 0);
                graphics.set_render_target(Some(ms));
            }
        }
        graphics.set_viewport(0, 0, self.render_target.width, self.render_target.height);
        let body = scene.nodes[s.body_transform].matrix;
        let set_visible = |scene: &mut Scene, visible: bool| {
            scene.nodes[s.body_transform].is_active = visible;
            for &n in s.car_nodes {
                scene.nodes[n].is_active = visible;
            }
        };
        set_visible(scene, false);
        let mut t = Mat44f::IDENTITY;
        t.m[3][0] = s.mirror_position[0];
        t.m[3][1] = s.mirror_position[1];
        t.m[3][2] = s.mirror_position[2];
        self.camera.matrix = xm_matrix_multiply(&t, &body);
        self.render_opaque(graphics, scene, sky, s.root);
        if self.high_quality {
            // the whole scene's transparent pass, the smoke only when asked for
            let old = std::mem::replace(&mut scene.nodes[s.particles].is_active, self.render_smoke);
            self.render_transparent(graphics, scene, s.root);
            scene.nodes[s.particles].is_active = old;
        } else if self.render_smoke {
            self.render_transparent(graphics, scene, s.particles);
        }
        self.render_transparent(graphics, scene, s.before_cars);
        set_visible(scene, true);
        if let Some(ms) = &self.render_target_ms {
            if let (Some(from), Some(to)) = (&ms.kid_color, &self.render_target.kid_color) {
                graphics.kgl.resolve_render_target(from, to, 1);
            }
        }
        graphics.kgl.set_sampler_ps(&graphics.samplers.aniso, 0);
        if let (Some(n), Some(saved)) = (s.ideal_line, saved_ideal) {
            scene.nodes[n].is_active = saved;
        }
        self.has_been_rendered = true;
    }

    /// `CameraMirror::renderOpaque` 0x14021bd80: the depth is cleared again after the sky.
    fn render_opaque(&mut self, graphics: &mut Graphics, scene: &mut Scene, sky: Option<&mut SkyBox>, node: NodeId) {
        let camera = &mut self.camera;
        let max_layer = camera.max_layer as i32;
        let mut filter = MaterialFilter::plain();
        camera.render_camera(graphics);
        camera.clear_buffers(graphics);
        if let Some(sky) = sky {
            graphics.set_world_matrix(&Mat44f::IDENTITY);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
            sky.render(graphics, &mut rc, false);
            graphics.clear_render_target_depth(camera.clear_depth);
        }
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
        scene.render(graphics, node, &mut rc);
    }

    /// `CameraMirror::renderTransparent` 0x14021bf30.
    fn render_transparent(&mut self, graphics: &mut Graphics, scene: &mut Scene, node: NodeId) {
        let camera = &self.camera;
        let mut filter = MaterialFilter::plain();
        graphics.set_depth_mode(1);
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_TRANSPARENT, max_layer: camera.max_layer as i32, camera: camera.cull_camera() };
        scene.render(graphics, node, &mut rc);
        graphics.set_depth_mode(0);
    }
}

/// `Node::getNodeChild<Mesh>` 0x1400e5d50.
fn mesh_node(scene: &Scene, node: NodeId, name: &str) -> Option<NodeId> {
    for &child in &scene.nodes[node].children {
        if scene.nodes[child].name == name && matches!(scene.nodes[child].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_)) {
            return Some(child);
        }
        if let Some(found) = mesh_node(scene, child, name) {
            return Some(found);
        }
    }
    None
}

/// `CarMirrorManager`: the meshes `data/mirrors.ini` names, each with its two materials.
#[derive(Default)]
pub struct CarMirrorManager {
    /// (the mesh, the material for the cockpit, the material for everywhere else)
    pub objects: Vec<(NodeId, MaterialId, MaterialId)>,
}

impl CarMirrorManager {
    /// `CarMirrorManager::CarMirrorManager` 0x1400e5f20.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &std::path::Path, body_transform: NodeId, texture: &Texture) -> Result<CarMirrorManager, String> {
        let mut manager = CarMirrorManager::default();
        let Ok(ini) = IniReader::load(&folder.join("data/mirrors.ini")) else { return Ok(manager) };
        let mut i = 0;
        loop {
            let section = format!("MIRROR_{i}");
            if !ini.has_section(&section) {
                break;
            }
            i += 1;
            let name = ini.get_string(&section, "NAME");
            let Some(mesh) = mesh_node(scene, body_transform, &name) else { continue };
            let has_material = match &scene.nodes[mesh].kind {
                NodeKind::Mesh(m) => m.material.is_some(),
                NodeKind::SkinnedMesh(m) => m.material.is_some(),
                _ => false,
            };
            if !has_material {
                continue;
            }
            let mut make = |shader: &str, specular_exp: f32, fresnel: bool| -> Result<MaterialId, String> {
                let mut m = Material::new("MIRROR_IN");
                m.set_shader(graphics, shader)?;
                m.set_texture("txDiffuse", texture.clone());
                m.set_float("ksDiffuse", 0.0);
                m.set_float("ksAmbient", 0.0);
                if let Some(var) = m.get_var("ksEmissive") {
                    m.update_var(var, |v| v.f_value3 = [1.0; 3]);
                }
                m.set_float("ksSpecular", 0.0);
                m.set_float("ksSpecularEXP", specular_exp);
                if fresnel {
                    m.set_float("fresnelMaxLevel", 1.0);
                }
                scene.materials.push(m);
                Ok(MaterialId(scene.materials.len() as u32 - 1))
            };
            let inside = make("ksPerPixel", 1.0, false)?;
            let outside = make("ksPerPixelReflection", 255.0, true)?;
            manager.objects.push((mesh, inside, outside));
        }
        Ok(manager)
    }

    /// `CarMirrorManager::update` 0x1400e6fc0.
    pub fn update(&self, scene: &mut Scene, on_board: bool) {
        for &(mesh, inside, outside) in &self.objects {
            let id = if on_board { inside } else { outside };
            match &mut scene.nodes[mesh].kind {
                NodeKind::Mesh(m) => m.material = Some(id),
                NodeKind::SkinnedMesh(m) => m.material = Some(id),
                _ => {}
            }
        }
    }
}

/// `VirtualMirrorRenderer`: the mirror texture on a quad at the top of the screen.
pub struct VirtualMirrorRenderer {
    /// `nodeVirtualMirror->isActive`: F11 turns it
    pub active: bool,
    gl: GlRenderer,
}

impl VirtualMirrorRenderer {
    /// `VirtualMirrorRenderer::VirtualMirrorRenderer` 0x1401d1ab0; `active` is
    /// `gameplay.ini [VIRTUAL_MIRROR] ACTIVE`.
    pub fn new(graphics: &Graphics, active: bool) -> VirtualMirrorRenderer {
        VirtualMirrorRenderer { active, gl: GlRenderer::new(graphics, 6) }
    }

    /// The handler of `Game::evOnPreGUI` with `renderVirtualMirror` 0x1401d1ea0.
    pub fn render(&mut self, graphics: &mut Graphics, texture: &Texture, camera_mode: i32) {
        if !self.active || !is_virtual_mirror_on(camera_mode) {
            return;
        }
        graphics.set_depth_mode(2);
        graphics.set_screen_space_mode();
        let height = graphics.video.height as f32;
        let width = graphics.video.width as f32;
        let h = (height * f32::from_bits(0x3a72_b9d6)) * 128.0;
        let w = (h * f32::from_bits(0x3c00_0000)) * 512.0;
        let x0 = (width - w) * 0.5;
        graphics.set_blend_mode(0);
        graphics.set_cull_mode(2);
        graphics.clear_texture_slot(0);
        graphics.set_texture(0, texture);
        let c = if graphics.video.pp_hdr_enabled { f32::from_bits(0x3e99_999a) } else { 1.0 };
        let gl = &mut self.gl;
        gl.color3f(c, c, c);
        gl.begin(GL_QUADS, None);
        gl.tex_coord2f(1.0, 0.0);
        gl.vertex3f(x0, 85.0, 0.0);
        gl.tex_coord2f(1.0, 1.0);
        let y1 = h + 85.0;
        gl.vertex3f(x0, y1, 0.0);
        gl.tex_coord2f(0.0, 1.0);
        let x1 = x0 + w;
        gl.vertex3f(x1, y1, 0.0);
        gl.tex_coord2f(0.0, 0.0);
        gl.vertex3f(x1, 85.0, 0.0);
        gl.end(graphics);
        graphics.clear_texture_slot(0);
    }
}
