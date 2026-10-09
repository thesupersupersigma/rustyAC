// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GraphicsManager`: the current matrices, the cached render state, the seven samplers and the
//! four system constant buffers, on top of [`crate::kgl`].

use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use windows::Win32::Graphics::Direct3D11::*;

use crate::kgl::{DeviceOptions, Kgl, KglIndexBuffer, KglRenderTarget, KglVertexBuffer, KglVideoSettings};
use crate::lighting::LightingSettings;
use crate::shader::{CBuffer, ShaderId, ShaderManager};
use crate::texture::{ResourceStore, Texture};

/// `VideoSettings` (0x50 bytes), as `loadVideoSettings` 0x1400c18d0 fills it from
/// `cfg/video.ini`.
#[derive(Clone, Copy, Debug)]
pub struct VideoSettings {
    pub aa_samples: i32,
    pub width: i32,
    pub height: i32,
    pub is_fullscreen: bool,
    pub v_sync: bool,
    pub anisotropic: i32,
    pub aa_quality: i32,
    pub shadow_map_size: i32,
    pub fps_cap_ms: f64,
    pub world_detail: i32,
    pub pp_hdr_enabled: bool,
    pub triple_buffer: bool,
}

impl Default for VideoSettings {
    /// The defaults of the `GraphicsManager` constructor.
    fn default() -> VideoSettings {
        VideoSettings { aa_samples: 1, width: 1680, height: 1050, is_fullscreen: true, v_sync: false, anisotropic: 4, aa_quality: 0, shadow_map_size: 2048, fps_cap_ms: 0.0, world_detail: 5, pp_hdr_enabled: false, triple_buffer: false }
    }
}

/// `BlendMode`: also the index of kgl's blend state.
pub const BLEND_OPAQUE: i32 = 0;
pub const BLEND_ALPHA: i32 = 1;
pub const BLEND_ALPHA_TO_COVERAGE: i32 = 2;
/// `CullMode`: also the index of kgl's rasterizer state.
pub const CULL_FRONT: i32 = 0;
pub const CULL_BACK: i32 = 1;
pub const CULL_NONE: i32 = 2;
/// `DepthMode`: also the index of kgl's depth-stencil state.
pub const DEPTH_NORMAL: i32 = 0;
pub const DEPTH_NO_WRITE: i32 = 1;
pub const DEPTH_OFF: i32 = 2;
pub const DEPTH_LESS_EQUAL: i32 = 3;

/// `RenderTarget` (the engine's): a colour target and/or a depth target.
pub struct RenderTarget {
    pub kid_color: Option<KglRenderTarget>,
    pub kid_depth: Option<KglRenderTarget>,
    pub width: i32,
    pub height: i32,
}

/// `RenderState` (0x1f8 bytes): what the manager believes is bound.
pub struct RenderState {
    /// the texture last bound per pixel-shader slot through [`Graphics::set_texture`]
    pub textures: [Option<crate::texture::TextureId>; 32],
    pub cull_mode: i32,
    pub blend_mode: i32,
    pub depth_state: i32,
    pub projection_matrix: Mat44f,
    pub view_matrix: Mat44f,
    pub world_matrix: Mat44f,
    pub shader: Option<ShaderId>,
    pub override_no_ms: bool,
}

/// `RenderStats`.
#[derive(Default, Clone, Copy, Debug)]
pub struct RenderStats {
    pub dip_calls: i32,
    pub scene_dip_calls: i32,
    pub triangles: i32,
    pub scene_triangles: i32,
    pub is_in_main_render_pass: bool,
}

pub struct Samplers {
    pub aniso: Option<ID3D11SamplerState>,
    pub shadow: Option<ID3D11SamplerState>,
    pub point: Option<ID3D11SamplerState>,
    pub point_clamp: Option<ID3D11SamplerState>,
    pub linear_clamp: Option<ID3D11SamplerState>,
    pub linear_shadow: Option<ID3D11SamplerState>,
    pub linear_simple: Option<ID3D11SamplerState>,
}

/// `GraphicsManager` (0x4e0 bytes).
pub struct Graphics {
    pub kgl: Kgl,
    pub game_folder: PathBuf,
    pub use_custom_sun_direction: bool,
    pub exposure_multiplier: f32,
    pub video: VideoSettings,
    pub stats: RenderStats,
    pub lighting: LightingSettings,
    pub max_frame_latency: i32,
    pub mip_lod_bias: f32,
    pub suspend_viewport_update_on_set_render_target: bool,
    pub state: RenderState,
    pub samplers: Samplers,
    pub cb_camera: CBuffer,
    pub cb_per_object: CBuffer,
    pub cb_lighting: CBuffer,
    pub cb_shadow_map: CBuffer,
    pub shaders: ShaderManager,
    pub resources: ResourceStore,
    pub custom_sun_direction: rustyac_physics::vecmath::Vec3f,
}

fn ini_float(ini: &Option<IniReader>, section: &str, key: &str) -> f32 {
    ini.as_ref().and_then(|i| i.get_float(section, key).ok()).unwrap_or(0.0)
}

fn ini_int(ini: &Option<IniReader>, section: &str, key: &str) -> i32 {
    ini.as_ref().and_then(|i| i.get_int(section, key).ok()).unwrap_or(0)
}

impl Graphics {
    /// `GraphicsManager::GraphicsManager` 0x140201620.
    pub fn new(video: VideoSettings, options: DeviceOptions, game_folder: &Path) -> Result<Graphics, String> {
        // initRenderFlags 0x140202f60
        let graphics_ini = IniReader::load(&game_folder.join("system/cfg/graphics.ini")).ok();
        let max_frame_latency = ini_int(&graphics_ini, "DX11", "MAXIMUM_FRAME_LATENCY");
        let mip_lod_bias = ini_float(&graphics_ini, "DX11", "MIP_LOD_BIAS");

        let kgl_video = KglVideoSettings { aa_samples: video.aa_samples, width: video.width, height: video.height, v_sync: video.v_sync, hdr_post_processing: video.pp_hdr_enabled, triple_buffer: video.triple_buffer };
        let kgl = Kgl::init(kgl_video, options)?;
        let mut video = video;
        video.aa_samples = kgl.video.aa_samples;

        let mut lighting = LightingSettings::new();
        // the constructor: lightDirection = (1, -1, 1), the light colour "normalised", angle 40
        lighting.light_direction = rustyac_physics::vecmath::Vec3f::new(1.0, -1.0, 1.0);
        lighting.light_color.normalize();
        lighting.angle = 40.0;

        // initCBuffers 0x140202d20
        let cb_camera = CBuffer::init(&kgl, 0, 0xe0);
        let cb_per_object = CBuffer::init(&kgl, 1, 0x40);
        let mut cb_lighting = CBuffer::init(&kgl, 2, 0xa0);
        cb_lighting.set_f32s(&[lighting.light_color.x, lighting.light_color.y, lighting.light_color.z], 0x20);
        {
            // normalize(1, -1, 0)
            let s = rustyac_math::sqrtf(2.0);
            if s != 0.0 {
                let inv = 1.0 / s;
                cb_lighting.set_f32s(&[inv, inv * -1.0, inv * 0.0], 0x00);
            } else {
                cb_lighting.set_f32s(&[1.0, -1.0, 0.0], 0x00);
            }
        }
        cb_lighting.set_f32s(&[0.4, 0.4, 0.4, 1.0], 0x10);
        cb_lighting.set_f32s(&[f32::from_bits(0x3e90_9091), f32::from_bits(0x3edc_dcdd), f32::from_bits(0x3f34_b4b5), 1.0], 0x40);
        cb_lighting.set_f32s(&[f32::from_bits(0x3f40_c0c1), f32::from_bits(0x3f48_c8c9), f32::from_bits(0x3f73_f3f4), 1.0], 0x30);
        cb_lighting.set_f32(2.0, 0x50);
        cb_lighting.set_f32(0.0, 0x80);
        cb_lighting.set_f32(10000.0, 0x84);
        cb_lighting.set_f32(400.0, 0x88);
        cb_lighting.set_f32(500.0, 0x8c);
        cb_lighting.set_f32(1.0, 0x90);
        let cb_shadow_map = CBuffer::init(&kgl, 3, 0xd0);
        let mut cb_camera = cb_camera;
        let mut cb_per_object = cb_per_object;
        let mut cb_shadow_map = cb_shadow_map;
        cb_camera.is_system = true;
        cb_per_object.is_system = true;
        cb_lighting.is_system = true;
        cb_shadow_map.is_system = true;

        // initSamplerStates 0x140203180
        let samplers = {
            let aniso = kgl.create_sampler(0, false, video.anisotropic, mip_lod_bias);
            let shadow = kgl.create_sampler(3, true, 0, 0.0);
            let point = kgl.create_sampler(2, false, 0, 0.0);
            let point_clamp = kgl.create_sampler(2, true, 0, 0.0);
            let linear_clamp = kgl.create_sampler(1, true, 0, 0.0);
            let linear_shadow = kgl.create_sampler(1, false, 0, 0.0);
            let linear_simple = kgl.create_sampler(1, false, 0, 0.0);
            Samplers { aniso, shadow, point, point_clamp, linear_clamp, linear_shadow, linear_simple }
        };

        let mut graphics = Graphics {
            kgl,
            game_folder: game_folder.to_path_buf(),
            use_custom_sun_direction: false,
            exposure_multiplier: 1.0,
            video,
            stats: RenderStats::default(),
            lighting,
            max_frame_latency,
            mip_lod_bias,
            suspend_viewport_update_on_set_render_target: false,
            state: RenderState {
                textures: [None; 32],
                cull_mode: 0,
                blend_mode: 0,
                depth_state: 0,
                projection_matrix: Mat44f::default(),
                view_matrix: Mat44f::default(),
                world_matrix: Mat44f::default(),
                shader: None,
                override_no_ms: false,
            },
            samplers,
            cb_camera,
            cb_per_object,
            cb_lighting,
            cb_shadow_map,
            shaders: ShaderManager::new(game_folder),
            resources: ResourceStore::new(),
            custom_sun_direction: rustyac_physics::vecmath::Vec3f::default(),
        };
        graphics.set_sampler_state();
        graphics.set_viewport(0, 0, video.width, video.height);
        graphics.load_lighting_settings(&game_folder.join("system/cfg/colorCurves.ini"));

        // the shadow biases
        let (b0, b1, b2) = match &graphics_ini {
            Some(ini) => (ini.get_float("DX11", "SHADOW_MAP_BIAS_0").unwrap_or(0.0), ini.get_float("DX11", "SHADOW_MAP_BIAS_1").unwrap_or(0.0), ini.get_float("DX11", "SHADOW_MAP_BIAS_2").unwrap_or(0.0)),
            None => (f32::from_bits(0x3586_37bd), f32::from_bits(0x38d1_b717), f32::from_bits(0x3a83_126f)),
        };
        graphics.set_shadow_map_bias(b0, b1, b2);
        Ok(graphics)
    }

    /// `GraphicsManager::setShadowMapBias` 0x140204a40.
    pub fn set_shadow_map_bias(&mut self, b0: f32, b1: f32, b2: f32) {
        let mut v = [b0, b1, b2];
        if self.video.shadow_map_size < 2040 {
            for x in &mut v {
                *x *= 2.0;
            }
        }
        if self.video.shadow_map_size < 1020 {
            for x in &mut v {
                *x *= 2.0;
            }
        }
        self.cb_shadow_map.set_f32s(&v, 0xc0);
    }

    /// `GraphicsManager::setSamplerState` 0x140204800.
    pub fn set_sampler_state(&self) {
        self.kgl.set_sampler_ps(&self.samplers.aniso, 0);
        self.kgl.set_sampler_ps(&self.samplers.shadow, 1);
        self.kgl.set_sampler_ps(&self.samplers.point, 2);
        self.kgl.set_sampler_ps(&self.samplers.point_clamp, 4);
        self.kgl.set_sampler_ps(&self.samplers.linear_shadow, 3);
        self.kgl.set_sampler_ps(&self.samplers.linear_simple, 5);
    }

    /// `GraphicsManager::beginScene` 0x140202580.
    pub fn begin_scene(&mut self) {
        self.kgl.set_default_state();
        self.state.cull_mode = CULL_NONE;
        self.state.depth_state = DEPTH_NORMAL;
        if self.state.blend_mode != BLEND_OPAQUE {
            self.kgl.set_blend_state(0);
            self.state.blend_mode = BLEND_OPAQUE;
        }
        if self.video.aa_samples == 1 || !self.video.pp_hdr_enabled {
            self.kgl.set_screen_render_targets(true);
        }
        self.set_sampler_state();
        self.set_viewport(0, 0, self.video.width, self.video.height);
        self.stats = RenderStats::default();
        self.update_lighting_settings();
    }

    /// `GraphicsManager::endScene` 0x140202850.
    pub fn end_scene(&mut self) {
        self.kgl.swap_buffers();
    }

    /// `GraphicsManager::setBlendMode` 0x1402044e0.
    pub fn set_blend_mode(&mut self, mode: i32) {
        if self.state.blend_mode != mode {
            self.kgl.set_blend_state(mode);
            self.state.blend_mode = mode;
        }
    }

    /// `GraphicsManager::setCullMode` 0x140204510.
    pub fn set_cull_mode(&mut self, mode: i32) {
        let mut mode = mode;
        if self.state.override_no_ms && mode == 0 {
            mode = 5;
        }
        if mode != self.state.cull_mode {
            self.kgl.set_cull_state(mode);
            self.state.cull_mode = mode;
        }
    }

    /// `GraphicsManager::setDepthMode` 0x140204580.
    pub fn set_depth_mode(&mut self, mode: i32) {
        if (0..=3).contains(&mode) && self.state.depth_state != mode {
            self.kgl.set_depth_state(mode);
            self.state.depth_state = mode;
        }
    }

    /// `GraphicsManager::setShader` 0x140204a00.
    pub fn set_shader(&mut self, shader: ShaderId) {
        if self.state.shader != Some(shader) {
            let s = &self.shaders.shaders[shader.0];
            self.kgl.set_shader(&s.kid.input_layout, &s.kid.vs, &s.kid.ps);
            self.state.shader = Some(shader);
        }
    }

    /// `GraphicsManager::setTexture` 0x140204c70.
    pub fn set_texture(&mut self, slot: i32, texture: &Texture) {
        let current = self.state.textures[slot as usize];
        match texture.kid {
            Some(kid) => {
                if Some(kid) != current {
                    self.kgl.set_texture(slot as u32, &self.resources.view(kid));
                    self.state.textures[slot as usize] = Some(kid);
                }
            }
            None => {
                if current.is_some() {
                    self.kgl.set_texture(slot as u32, &None);
                    self.state.textures[slot as usize] = None;
                }
            }
        }
    }

    /// `GraphicsManager::clearTextureSlot` 0x140202660.
    pub fn clear_texture_slot(&mut self, slot: i32) {
        if self.state.textures[slot as usize].is_some() {
            self.kgl.set_texture(slot as u32, &None);
        }
        self.state.textures[slot as usize] = None;
    }

    /// `GraphicsManager::setShadowMapTexture` 0x140204bf0: the cascades live in slots 6, 7, 8.
    pub fn set_shadow_map_texture(&mut self, index: i32, target: Option<&RenderTarget>) {
        let slot = (index + 6) as u32;
        match target {
            Some(rt) => {
                let view = rt.kid_depth.as_ref().and_then(|d| d.shader_resource_view.clone());
                self.kgl.set_texture(slot, &view);
                self.cb_shadow_map.set_f32(1.0 / rt.width as f32, 0xcc);
            }
            None => self.kgl.set_texture(slot, &None),
        }
    }

    /// `GraphicsManager::setVB` 0x140204cf0.
    pub fn set_vb(&self, vb: &KglVertexBuffer) {
        self.kgl.set_vertex_buffer(vb);
    }

    /// `GraphicsManager::setIB` 0x140204650.
    pub fn set_ib(&self, ib: &KglIndexBuffer) {
        self.kgl.set_index_buffer(ib);
    }

    /// `GraphicsManager::drawPrimitive` 0x1402027f0.
    pub fn draw_primitive(&mut self, index_count: i32, start_index: i32, base_vertex: i32) {
        self.kgl.draw_indexed(index_count, start_index, base_vertex);
        self.stats.dip_calls += 1;
        let triangles = (index_count as u32 / 3) as i32;
        self.stats.triangles += triangles;
        if self.stats.is_in_main_render_pass {
            self.stats.scene_triangles += triangles;
            self.stats.scene_dip_calls += 1;
        }
    }

    /// `GraphicsManager::commitShaderChanges` 0x1402026a0: camera, lighting, per object, shadow.
    pub fn commit_shader_changes(&mut self) {
        self.cb_camera.commit(&self.kgl);
        self.cb_lighting.commit(&self.kgl);
        self.cb_per_object.commit(&self.kgl);
        self.cb_shadow_map.commit(&self.kgl);
    }

    /// `GraphicsManager::setViewport` 0x140204ff0.
    pub fn set_viewport(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.kgl.set_viewport(x as f32, y as f32, width as f32, height as f32);
        self.cb_lighting.set_f32(1.0 / width as f32, 0x54);
        self.cb_lighting.set_f32(1.0 / height as f32, 0x58);
        self.cb_lighting.touch();
    }

    /// `GraphicsManager::setRenderTarget` 0x140204760.
    pub fn set_render_target(&mut self, target: Option<&RenderTarget>) {
        match target {
            Some(rt) => {
                match (&rt.kid_color, &rt.kid_depth) {
                    (Some(color), Some(depth)) => self.kgl.set_render_targets(Some(color), Some(depth)),
                    (Some(color), None) => self.kgl.set_render_target(Some(color)),
                    (None, depth) => self.kgl.set_render_targets(None, depth.as_ref()),
                }
                if !self.suspend_viewport_update_on_set_render_target {
                    self.set_viewport(0, 0, rt.width, rt.height);
                }
            }
            None => {
                self.kgl.set_screen_render_targets(true);
                if !self.suspend_viewport_update_on_set_render_target {
                    self.set_viewport(0, 0, self.video.width, self.video.height);
                }
            }
        }
    }

    /// `GraphicsManager::clearRenderTarget` 0x140202640.
    pub fn clear_render_target(&self, colour: &[f32; 4]) {
        self.kgl.clear_color(colour);
    }

    /// `GraphicsManager::clearRenderTargetDepth` 0x140202650.
    pub fn clear_render_target_depth(&self, depth: f32) {
        self.kgl.clear_depth(depth);
    }
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

impl Graphics {
    /// `GraphicsManager::setProjectionMatrix` 0x140204660.
    pub fn set_projection_matrix(&mut self, m: &Mat44f) {
        self.cb_camera.set_f32s(&transposed(m), 0x40);
        self.state.projection_matrix = *m;
    }

    /// `GraphicsManager::setViewMatrix` 0x140204d00. It works with the projection stored last.
    pub fn set_view_matrix(&mut self, v: &Mat44f, camera: Option<&crate::camera::Camera>) {
        self.state.view_matrix = *v;
        self.cb_camera.set_f32s(&transposed(v), 0x00);
        if let Some(cam) = camera {
            self.cb_camera.set_f32s(&[cam.matrix.m[3][0], cam.matrix.m[3][1], cam.matrix.m[3][2]], 0xc0);
            self.cb_camera.set_f32(cam.near_plane, 0xd0);
            self.cb_camera.set_f32(cam.far_plane, 0xd4);
            self.cb_camera.set_f32(cam.fov, 0xd8);
        }
        let vp = rustyac_physics::vecmath::xm_matrix_multiply(&self.state.view_matrix, &self.state.projection_matrix);
        let inverse = rustyac_physics::vecmath::xm_matrix_inverse(&vp);
        self.cb_camera.set_f32s(&transposed(&inverse), 0x80);
    }

    /// `GraphicsManager::setWorldMatrix` 0x1402050a0: no compare, every call marks the buffer.
    pub fn set_world_matrix(&mut self, w: &Mat44f) {
        self.state.world_matrix = *w;
        self.cb_per_object.set_f32s(&transposed(w), 0x00);
    }

    /// `GraphicsManager::setShadowMapMatrix` 0x140204af0.
    pub fn set_shadow_map_matrix(&mut self, index: i32, m: &Mat44f) {
        let offset = match index {
            0 => 0x00,
            1 => 0x40,
            2 => 0x80,
            _ => return,
        };
        self.cb_shadow_map.set_f32s(&transposed(m), offset);
    }
}

impl Graphics {
    /// `GraphicsManager::setScreenRenderTargets` 0x140204880.
    pub fn set_screen_render_targets(&mut self) {
        let with_depth = !(self.video.aa_samples != 1 && self.video.pp_hdr_enabled);
        self.kgl.set_screen_render_targets(with_depth);
        self.set_viewport(0, 0, self.video.width, self.video.height);
    }

    /// `GraphicsManager::setScreenSpaceMode` 0x1402048e0: what the frame ends with before the
    /// 2D drawing. No Direct3D call, but the camera and per-object buffers are written.
    pub fn set_screen_space_mode(&mut self) {
        let w = self.video.width as f32;
        let h = self.video.height as f32;
        let mut proj = Mat44f::default();
        proj.m[0][0] = 2.0 / w;
        proj.m[1][1] = 2.0 / (-h);
        proj.m[2][2] = -0.5;
        proj.m[3][0] = w / (-w);
        proj.m[3][1] = h / h;
        proj.m[3][2] = 0.5;
        proj.m[3][3] = 1.0;
        self.set_projection_matrix(&proj);
        self.set_view_matrix(&Mat44f::IDENTITY, None);
        self.set_world_matrix(&Mat44f::IDENTITY);
    }
}

impl Graphics {
    /// `GraphicsManager::resetRenderStates` 0x140204440: what the game's 2D drawing calls
    /// before it draws. The states are set for real and everything the manager believes about
    /// the shader and the textures is forgotten (the textures stay bound).
    pub fn reset_render_states(&mut self) {
        self.state.blend_mode = 0;
        self.kgl.set_blend_state(0);
        self.state.cull_mode = 0;
        self.kgl.set_cull_state(0);
        self.state.depth_state = 0;
        self.kgl.set_depth_state(0);
        self.state.override_no_ms = false;
        self.state.shader = None;
        self.state.textures = [None; 32];
    }

    /// `GraphicsManager::onResize` 0x140204330 for a screen that is a texture (no swap chain):
    /// the screen's colour and depth targets are made again at the new size.
    pub fn on_resize(&mut self, width: i32, height: i32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.video.width = width;
        self.video.height = height;
        self.kgl.resize_screen(width, height)
    }
}
