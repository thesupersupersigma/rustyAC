// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CameraForward`: the frame of the plain path, `CameraForward::CameraForward` 0x14021f230,
//! `CameraForward::render` 0x14021fc40, `CubeMapRenderer::render` 0x14021edb0,
//! `CameraForward::setCubemapSize` 0x1402201b0, and the motion blur the post-processing camera
//! draws with: `CameraForward::renderBlurred` 0x14021fde0, `CameraForward::solveBlur`
//! 0x140220280.
//!
//! In `acs.exe` the blurred branch only ever runs under `CameraForwardYebis` ([`crate::post`]):
//! `Sim::createCamera` 0x1401982e0 asks for blur only when `[POST_PROCESS] ENABLED` is on, and
//! the plain class's `setSolveBlurRenderTarget` 0x140220250 reads an `HDR` object nothing ever
//! makes.

use rustyac_physics::vecmath::{xm_matrix_inverse, Mat44f, Vec3f};

use crate::camera::{Camera, CameraShadowMapped};
use crate::graphics::{Graphics, RenderTarget, BLEND_OPAQUE, CULL_FRONT, DEPTH_LESS_EQUAL, DEPTH_NORMAL, DEPTH_NO_WRITE, DEPTH_OFF};
use crate::kgl::KglCubeMap;
use crate::material::{MaterialFilter, PASS_OPAQUE, PASS_TRANSPARENT};
use crate::scene::{NodeId, RenderContext, Scene};
use crate::shader::ShaderId;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;

/// `CubeMapRenderer` (0x3e0 bytes): the camera that looks along the six axes.
pub struct CubeMapRenderer {
    pub faces_per_frame: i32,
    pub camera_matrix: [Mat44f; 6],
    pub camera: Camera,
    pub current_face: i32,
    /// NOT Assetto Corsa, diagnosis only (`RUSTYAC_DIAG_NO_CUBE_MIPS=1`): the mip levels are not
    /// made again every frame. The game always makes them.
    pub diagnosis_no_mips: bool,
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
        CubeMapRenderer { faces_per_frame: 6, camera_matrix: crate::cubemap::face_matrices(), camera, current_face: 0, diagnosis_no_mips: false }
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

/// The blur shader's variables (`svProjInv`, `svProj`, `svCameraVelocity`,
/// `svCameraAngularVelocity`, `svQuality`; `fpsCorrection` is asked for and no shader has it).
#[derive(Clone, Copy, Default)]
struct BlurVars {
    proj_inv: Option<usize>,
    proj: Option<usize>,
    camera_velocity: Option<usize>,
    camera_angular_velocity: Option<usize>,
    quality: Option<usize>,
}

/// `CameraForward` (0x788 bytes).
pub struct CameraForward {
    pub base: CameraShadowMapped,
    pub cube_map_renderer: CubeMapRenderer,
    pub cube_map: KglCubeMap,
    /// `useBlur`: the constructor's flag (`[EFFECTS] MOTION_BLUR` > 0 and post-processing on)
    pub use_blur: bool,
    /// `blurQuality` (8): the solve's `quality`
    pub blur_quality: f32,
    /// `rtBlur`: what the `BLURRED` branch is drawn into
    pub rt_blur: Option<RenderTarget>,
    blur_shader: Option<ShaderId>,
    /// `blurShaderRadial` (`ksPostBlurRadial` / `ksPostBlurRadialMS`): loaded, never drawn with
    pub blur_shader_radial: Option<ShaderId>,
    blur_vars: BlurVars,
    /// `lastWorldMatrix`: the camera's matrix at the last solve (all zero before the first)
    pub last_world_matrix: Mat44f,
    pub current_blur_angle: f32,
}

impl CameraForward {
    /// `CameraForward::CameraForward` 0x14021f230: the shadow targets, the cube-map camera and
    /// a first cube map of 512; with `use_blur` and post-processing on, the blur target and the
    /// two blur shaders (the `_MS` pair when the screen is multisampled).
    pub fn new(graphics: &mut Graphics, use_blur: bool) -> Result<CameraForward, String> {
        let base = CameraShadowMapped::new(graphics)?;
        let mut camera = CameraForward {
            base,
            cube_map_renderer: CubeMapRenderer::new(),
            // (made below, after the blur's objects, as in the game)
            cube_map: KglCubeMap { texture: None, srv_cube: None, rtv_face: Vec::new(), depth_texture: None, dsv_depth: None, size: 0, mips: 0 },
            use_blur,
            blur_quality: 8.0,
            rt_blur: None,
            blur_shader: None,
            blur_shader_radial: None,
            blur_vars: BlurVars::default(),
            last_world_matrix: Mat44f::default(),
            current_blur_angle: 0.0,
        };
        if use_blur && graphics.video.pp_hdr_enabled {
            camera.rt_blur = Some(blur_target(graphics)?);
            let multisampled = graphics.video.aa_samples != 1;
            let blur = graphics.shaders.get_shader(&graphics.kgl, if multisampled { "ksPostBlur_MS" } else { "ksPostBlur" })?;
            let radial = graphics.shaders.get_shader(&graphics.kgl, if multisampled { "ksPostBlurRadialMS" } else { "ksPostBlurRadial" })?;
            let shader = graphics.shaders.get(blur);
            camera.blur_vars = BlurVars {
                proj_inv: shader.get_var("matProjectionInv"),
                proj: shader.get_var("matProjection"),
                camera_velocity: shader.get_var("cameraVelocity"),
                camera_angular_velocity: shader.get_var("matAngularVelocity"),
                quality: shader.get_var("quality"),
            };
            camera.blur_shader = Some(blur);
            camera.blur_shader_radial = Some(radial);
        }
        camera.cube_map = graphics.kgl.create_cube_map(512, true, 7);
        Ok(camera)
    }

    /// `CameraForward::onResize` 0x14021f990, the blur target's part: made again at the
    /// screen's size.
    pub fn on_resize(&mut self, graphics: &mut Graphics) -> Result<(), String> {
        if self.rt_blur.is_some() {
            self.rt_blur = Some(blur_target(graphics)?);
        }
        Ok(())
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

    /// `CameraForward::renderBlurred` 0x14021fde0: the `BLURRED` branch (with the sky) into the
    /// blur target, the solve into `solve_target`, then the `UNBLURRED` branch and every
    /// transparent thing on top of it, unblurred.
    pub fn render_blurred(&mut self, graphics: &mut Graphics, scene: &mut Scene, blurred: NodeId, unblurred: NodeId, root: NodeId, dt: f32, solve_target: &RenderTarget) {
        graphics.set_depth_mode(DEPTH_NORMAL);
        let max_layer = self.base.camera.max_layer as i32;
        let mut filter = MaterialFilter::plain();
        graphics.set_render_target(self.rt_blur.as_ref());
        graphics.set_cull_mode(CULL_FRONT);
        self.base.camera.render_camera(graphics);
        self.base.camera.clear_buffers(graphics);
        if let Some(sky) = &mut self.base.sky_box {
            graphics.set_depth_mode(DEPTH_OFF);
            graphics.set_world_matrix(&Mat44f::IDENTITY);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: self.base.camera.cull_camera() };
            sky.render(graphics, &mut rc, self.base.camera.is_cube_map_camera);
            graphics.set_depth_mode(DEPTH_NORMAL);
        }
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: self.base.camera.cull_camera() };
            scene.render(graphics, blurred, &mut rc);
        }
        self.base.camera.render_camera(graphics);
        self.solve_blur(graphics, dt, solve_target);
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: self.base.camera.cull_camera() };
            scene.render(graphics, unblurred, &mut rc);
        }
        graphics.set_depth_mode(DEPTH_NO_WRITE);
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_TRANSPARENT, max_layer, camera: self.base.camera.cull_camera() };
            scene.render(graphics, root, &mut rc);
        }
        graphics.set_depth_mode(DEPTH_NORMAL);
    }

    /// `CameraForward::solveBlur` 0x140220280: one full-screen quad that reads the blur target's
    /// colour (slot 2) and depth (slot 1) and smears each pixel along the camera's movement
    /// since the last solve. `solve_target` is what the camera's `setSolveBlurRenderTarget`
    /// binds (`CameraForwardYebis::setSolveBlurRenderTarget` 0x14002a710: its scene target).
    pub fn solve_blur(&mut self, graphics: &mut Graphics, dt: f32, solve_target: &RenderTarget) {
        graphics.kgl.set_render_targets(solve_target.kid_color.as_ref(), solve_target.kid_depth.as_ref());
        let Some(shader) = self.blur_shader else { return };
        if let Some(rt) = &self.rt_blur {
            // GraphicsManager::setTextureRT 0x140204cd0, setDepthTextureRT 0x140204630
            graphics.kgl.set_texture(2, &rt.kid_color.as_ref().and_then(|c| c.shader_resource_view.clone()));
            graphics.kgl.set_texture(1, &rt.kid_depth.as_ref().and_then(|d| d.shader_resource_view.clone()));
        }
        let proj = graphics.state.projection_matrix;
        let inverse = xm_matrix_inverse(&proj);
        let vars = self.blur_vars;
        let set = |graphics: &mut Graphics, var: Option<usize>, values: &[f32]| {
            if let Some(var) = var {
                let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
                graphics.shaders.get_mut(shader).set_var(var, &bytes);
            }
        };
        set(graphics, vars.proj_inv, &transposed(&inverse));
        set(graphics, vars.proj, &transposed(&proj));

        let v = graphics.state.view_matrix.m;
        let m = self.base.camera.matrix.m;
        let last = self.last_world_matrix.m;
        // the camera's place now and at the last solve, both through this frame's view matrix
        let through = |x: f32, y: f32, z: f32, c: usize| ((v[1][c] * y + v[0][c] * x) + v[2][c] * z) + v[3][c];
        let now = [through(m[3][0], m[3][1], m[3][2], 0), through(m[3][0], m[3][1], m[3][2], 1), through(m[3][0], m[3][1], m[3][2], 2)];
        let was = [through(last[3][0], last[3][1], last[3][2], 0), through(last[3][0], last[3][1], last[3][2], 1), through(last[3][0], last[3][1], last[3][2], 2)];
        let k = f32::from_bits(0x3ac4_9ba6); // 0.0015
        let inv_dt = 1.0 / dt;
        let velocity = [((now[0] - was[0]) * k) * inv_dt, ((now[1] - was[1]) * k) * inv_dt, ((now[2] - was[2]) * k) * inv_dt];
        set(graphics, vars.camera_velocity, &velocity);

        // the two looking directions (the matrix's third row, negated) in view space
        let direction = |row: &[f32; 4]| {
            let (x, y, z) = (-row[0], -row[1], -row[2]);
            let mut d = [(y * v[1][0] + x * v[0][0]) + z * v[2][0], (y * v[1][1] + x * v[0][1]) + z * v[2][1], (y * v[1][2] + x * v[0][2]) + z * v[2][2]];
            let length = ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt();
            if length != 0.0 {
                let inv = 1.0 / length;
                d[1] *= inv;
                d[2] *= inv;
                d[0] = inv * d[0];
            }
            d
        };
        let a = direction(&m[2]);
        let b = direction(&last[2]);
        let mut axis = Vec3f::new(b[2] * a[1] - b[1] * a[2], b[0] * a[2] - b[2] * a[0], b[1] * a[0] - b[0] * a[1]);
        let length = ((axis.y * axis.y + axis.x * axis.x) + axis.z * axis.z).sqrt();
        if length != 0.0 {
            let inv = 1.0 / length;
            axis.z *= inv;
            axis.y *= inv;
            axis.x *= inv;
        }
        let dot = (b[0] * a[0] + b[1] * a[1]) + b[2] * a[2];
        let mut angle = 0.0f32;
        if dot < 1.0 {
            angle = (rustyac_math::acosf(dot) * inv_dt) * k;
        }
        if dot < f32::from_bits(0x3f66_6666) {
            angle = (angle - self.current_blur_angle) * f32::from_bits(0x3dcc_cccd) + self.current_blur_angle;
        }
        self.current_blur_angle = angle;
        let rotation = Mat44f::create_from_axis_angle(&axis, angle);
        set(graphics, vars.camera_angular_velocity, &transposed(&rotation));
        set(graphics, vars.quality, &[self.blur_quality]);
        self.last_world_matrix = self.base.camera.matrix;
        // CBuffer::commit of the buffer the velocity lives in
        if let Some(var) = vars.camera_velocity {
            let s = graphics.shaders.get_mut(shader);
            if let Some(buffer) = s.vars[var].buffer {
                let mut cb = std::mem::replace(&mut s.cbuffers[buffer], crate::shader::CBuffer::empty());
                cb.commit(&graphics.kgl);
                graphics.shaders.get_mut(shader).cbuffers[buffer] = cb;
            }
        }
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.clear_render_target_depth(1.0);
        graphics.set_blend_mode(BLEND_OPAQUE);
        graphics.set_depth_mode(DEPTH_LESS_EQUAL);
        crate::gl::full_screen_quad(graphics, Some(shader));
        graphics.set_depth_mode(DEPTH_NORMAL);
        graphics.clear_texture_slot(1);
        graphics.clear_texture_slot(2);
    }
}

/// `RenderTarget(graphics, eR16G16B16A16 or eR16G16B16A16_MS, width, height, colour, depth, 1)`:
/// the blur target and the post-processing camera's scene target.
pub(crate) fn blur_target(graphics: &Graphics) -> Result<RenderTarget, String> {
    let (width, height, samples) = (graphics.video.width, graphics.video.height, graphics.video.aa_samples);
    let color = graphics.kgl.create_render_target(DXGI_FORMAT_R16G16B16A16_FLOAT, width, height, samples, 1);
    let depth = graphics.kgl.create_render_target_depth(width, height, samples)?;
    Ok(RenderTarget { kid_color: Some(color), kid_depth: Some(depth), width, height })
}

fn transposed(m: &Mat44f) -> [f32; 16] {
    let mut t = [0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            t[4 * i + j] = m.m[j][i];
        }
    }
    t
}
