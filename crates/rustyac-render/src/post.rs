// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CameraForwardYebis`, the camera of `[POST_PROCESS] ENABLED=1`, without YEBIS: Kunos' own
//! part of that frame. `CameraForwardYebis::CameraForwardYebis` 0x140025db0, `initRenderTargets`
//! 0x140029a90, `render` 0x14002a450, `renderApplyEffect` 0x14002a740, `resetYebisStates`
//! 0x14002a650, `getYebisDestinationRenderTarget` 0x14002a630, `setSolveBlurRenderTarget`
//! 0x14002a710.
//!
//! The frame: the scene (or, with motion blur, the blurred and unblurred branches around the
//! solve) goes into a 16-bit float target with the screen's sample count; a multisampled target
//! is resolved, its depth is resolved by a shader of the game's (`ksMSDepthResolve`); then
//! YEBIS turns that picture into the final one, into the screen or into a target the game's
//! FXAA pass (`ksFXAA`) reads.
//!
//! Where the game calls `CameraForwardYebis::applyPostProcessing` 0x140025c30 this draws YEBIS
//! ([`crate::yebis`], Task Y2b) once [`CameraForwardYebis::start_yebis`] has read it from the
//! player's acs.exe; until then (and on wgpu) [`stand_in_apply_effects`], a plain copy with the
//! game's `ksPostCopy` shader, which the render oracle swaps into the game for the same frame.

use rustyac_content::vfs::PathExt;
use crate::kgl::{FORMAT_R32F as DXGI_FORMAT_R32_FLOAT, FORMAT_RGBA16F as DXGI_FORMAT_R16G16B16A16_FLOAT, FORMAT_RGBA8 as DXGI_FORMAT_R8G8B8A8_UNORM};

use crate::forward::{blur_target, CameraForward};
use crate::graphics::{Graphics, RenderTarget, BLEND_OPAQUE, DEPTH_OFF};
use crate::kgl::KglRenderTarget;
use crate::scene::{NodeId, Scene};
use crate::shader::ShaderId;

/// `CameraForwardYebis` (0xe80 bytes), the members Kunos' side of the frame uses.
pub struct CameraForwardYebis {
    pub forward: CameraForward,
    /// `debug_MotionBlurEnabled` (true): the blurred branch can be switched off at run time
    pub debug_motion_blur_enabled: bool,
    depth_resolve_shader: Option<ShaderId>,
    fxaa_shader: Option<ShaderId>,
    /// `fxaaTempRT`: what YEBIS draws into when FXAA is on (else: the screen)
    fxaa_temp_rt: Option<RenderTarget>,
    /// `yebisRT_MS`: the scene's target (16-bit float, the screen's samples, with depth)
    yebis_rt_ms: Option<RenderTarget>,
    /// `yebisRT_Resolved_Color` and `yebisRT_Resolved_Depth`: only made for a multisampled
    /// scene target (with one sample the scene target is YEBIS' source itself)
    yebis_rt_resolved_color: Option<RenderTarget>,
    yebis_rt_resolved_depth: Option<RenderTarget>,
    width: i32,
    height: i32,
    /// NOT Assetto Corsa: the shader of the copy that stands where YEBIS will be
    stand_in_shader: Option<ShaderId>,
    /// YEBIS (`CPostEffect`), once started
    #[cfg(all(windows, not(feature = "backend-wgpu")))]
    yebis: Option<crate::yebis::Yebis>,
    /// evOnPostYebisParametersUpdate has fired once: on triple screens the vignette strength is zero from then on
    #[cfg(all(windows, not(feature = "backend-wgpu")))]
    parameters_updated: bool,
    /// the heat shimmer's particles, with video.ini's HEAT_SHIMMER on (Task Y3d-3)
    pub heat: Option<crate::heat_shimmer::HeatShimmer>,
}

impl std::ops::Deref for CameraForwardYebis {
    type Target = CameraForward;
    fn deref(&self) -> &CameraForward {
        &self.forward
    }
}

impl std::ops::DerefMut for CameraForwardYebis {
    fn deref_mut(&mut self) -> &mut CameraForward {
        &mut self.forward
    }
}

impl CameraForwardYebis {
    /// `CameraForwardYebis::CameraForwardYebis` 0x140025db0 (and `loadPPSet` 0x140029960 as far
    /// as `initRenderTargets`; the filter file itself is YEBIS' business). `stereo`: the camera
    /// is a headset's (Sim::createCamera makes no CameraForwardYebis then, the stereo camera has
    /// its own YEBIS), so none of the post-processing targets.
    pub fn new(graphics: &mut Graphics, use_blur: bool, stereo: bool) -> Result<CameraForwardYebis, String> {
        let forward = CameraForward::new(graphics, use_blur)?;
        let mut camera = CameraForwardYebis {
            forward,
            debug_motion_blur_enabled: true,
            depth_resolve_shader: None,
            fxaa_shader: None,
            fxaa_temp_rt: None,
            yebis_rt_ms: None,
            yebis_rt_resolved_color: None,
            yebis_rt_resolved_depth: None,
            width: 0,
            height: 0,
            stand_in_shader: None,
            #[cfg(all(windows, not(feature = "backend-wgpu")))]
            yebis: None,
            #[cfg(all(windows, not(feature = "backend-wgpu")))]
            parameters_updated: false,
            heat: None,
        };
        if graphics.video.pp_hdr_enabled && !stereo {
            camera.depth_resolve_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksMSDepthResolve")?);
            camera.width = graphics.video.width;
            camera.height = graphics.video.height;
            if graphics.video.pp_fxaa {
                camera.fxaa_temp_rt = Some(color_target(graphics, DXGI_FORMAT_R8G8B8A8_UNORM, camera.width, camera.height));
                camera.fxaa_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksFXAA")?);
            }
            camera.init_render_targets(graphics)?;
            camera.stand_in_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksPostCopy")?);
        }
        Ok(camera)
    }

    /// `CameraForwardYebis::initRenderTargets` 0x140029a90.
    fn init_render_targets(&mut self, graphics: &mut Graphics) -> Result<(), String> {
        self.yebis_rt_ms = None;
        self.yebis_rt_resolved_color = None;
        self.yebis_rt_resolved_depth = None;
        if graphics.video.aa_samples == 1 {
            // RenderTarget(eR16G16B16A16, colour, depth): one target is scene and source
            self.yebis_rt_ms = Some(blur_target(graphics)?);
        } else {
            // RenderTarget(eR16G16B16A16_MS, colour, depth), then two plain ones without depth
            self.yebis_rt_ms = Some(blur_target(graphics)?);
            self.yebis_rt_resolved_color = Some(color_target(graphics, DXGI_FORMAT_R16G16B16A16_FLOAT, self.width, self.height));
            self.yebis_rt_resolved_depth = Some(color_target(graphics, DXGI_FORMAT_R32_FLOAT, self.width, self.height));
        }
        Ok(())
    }

    /// The old YEBIS's teardown, which the game does before the screen is resized.
    pub fn before_resize(&self, graphics: &Graphics) {
        #[cfg(all(windows, not(feature = "backend-wgpu")))]
        if let Some(yebis) = &self.yebis {
            yebis.teardown(Self::destination(&self.fxaa_temp_rt, graphics));
        }
        #[cfg(not(all(windows, not(feature = "backend-wgpu"))))]
        let _ = graphics;
    }

    /// `CameraForwardYebis::onResize` 0x140029da0. As the game, the FXAA target keeps the size
    /// it was made with; YEBIS is remade at that size and scales the resized scene into it.
    pub fn on_resize(&mut self, graphics: &mut Graphics) -> Result<(), String> {
        self.forward.on_resize(graphics)?;
        self.width = graphics.video.width;
        self.height = graphics.video.height;
        if graphics.video.pp_hdr_enabled {
            self.init_render_targets(graphics)?;
            // initPostProcessing 0x140024ca0: a new particle manager
            #[cfg(all(windows, not(feature = "backend-wgpu")))]
            if let Some(heat) = &mut self.heat {
                heat.remake(&mut graphics.crt_rand);
            }
            #[cfg(all(windows, not(feature = "backend-wgpu")))]
            if let Some(yebis) = self.yebis.take() {
                // YEBIS sizes its buffers from its destination: the FXAA target keeps its first size;
                // the base width comes from the camera's new width (initRenderTargets 0x140029a90)
                let (w, h) = self.fxaa_temp_rt.as_ref().map_or((self.width, self.height), |t| (t.width, t.height));
                // YEBIS's clears restore its destination, which the game hands it without binding it
                let mut yebis = yebis.remade(w, h, self.width, Self::destination(&self.fxaa_temp_rt, graphics))?;
                // CameraTripleScreen's handler 0x140224dc0 zeroes the vignette strength on every
                // evOnPostYebisParametersUpdate, and only YEBIS's init reads it
                if self.forward.base.triple.is_some() && self.parameters_updated {
                    yebis.vignette = 0.0;
                }
                self.yebis = Some(yebis);
            }
        }
        Ok(())
    }

    /// `CameraForwardYebis::render` 0x14002a450.
    pub fn render(&mut self, graphics: &mut Graphics, scene: &mut Scene, blurred: Option<NodeId>, unblurred: NodeId, root: NodeId, dt: f32) -> Result<(), String> {
        // a headset's camera does its own post-processing
        #[cfg(not(feature = "backend-wgpu"))]
        let stereo = self.forward.vr.is_some() || self.forward.oculus.is_some();
        #[cfg(feature = "backend-wgpu")]
        let stereo = false;
        if !graphics.video.pp_hdr_enabled || stereo {
            return self.forward.render(graphics, scene, blurred, root, dt);
        }
        self.forward.base.shadow_map_pass(graphics, scene, root)?;
        graphics.trigger_query("SHADOW_MAP");
        if let Some(blurred) = blurred {
            let m = &self.forward.base.camera.matrix.m;
            let position = [m[3][0], m[3][1], m[3][2]];
            crate::cubemap::render(&mut self.forward.cube_map_renderer, graphics, scene, &self.forward.cube_map, blurred, position, self.forward.base.sky_box.as_mut());
        }
        // updateYebisParameters 0x140029e30: YEBIS' values of the frame (Task 23b), and at its
        // end the scene target is cleared, colour to zero and depth to 1
        graphics.clear_texture_slot(5);
        if let Some(ms) = &self.yebis_rt_ms {
            for target in [&ms.kid_color, &ms.kid_depth].into_iter().flatten() {
                graphics.kgl.render_target_clear(target, &[0.0; 4], 1.0);
            }
        }
        graphics.set_depth_mode(crate::graphics::DEPTH_NORMAL);
        // (evOnPostYebisParametersUpdate 0x14002a57c: the heat shimmer's particles, YEBIS)
        #[cfg(all(windows, not(feature = "backend-wgpu")))]
        {
            self.parameters_updated = true;
        }
        self.render_apply_effect(graphics, scene, blurred, unblurred, root, dt);
        self.reset_yebis_states(graphics);
        if let (Some(rt), Some(shader)) = (&self.fxaa_temp_rt, self.fxaa_shader) {
            // GraphicsManager::setTextureRT(5, fxaaTempRT)
            graphics.kgl.set_texture(5, &rt.kid_color.as_ref().and_then(|c| c.shader_resource_view.clone()));
            crate::gl::full_screen_quad(graphics, Some(shader));
        }
        Ok(())
    }

    #[cfg(all(windows, not(feature = "backend-wgpu")))]
    /// What YEBIS draws into: the FXAA target when FXAA is on, else the screen.
    fn destination(fxaa_temp_rt: &Option<RenderTarget>, graphics: &Graphics) -> Option<windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView> {
        match fxaa_temp_rt {
            Some(t) => t.kid_color.as_ref().and_then(|c| c.render_target_view.clone()),
            None => graphics.kgl.screen_render_target.render_target_view.clone(),
        }
    }

    /// Reads YEBIS from `root`'s acs.exe and makes its objects: call after [`Self::new`], outside
    /// a logged frame. `video` is video.ini; its `[POST_PROCESS] FILTER` names the ppfilter in
    /// `data`'s `system/cfg/ppfilters` (the game's working folder). `heat_sets` is race.ini's
    /// [RACE] CARS (Sim::createCamera 0x1401982e0): the heat shimmer's sets.
    #[cfg(all(windows, not(feature = "backend-wgpu")))]
    pub fn start_yebis(&mut self, graphics: &mut Graphics, root: &std::path::Path, data: &std::path::Path, video: rustyac_physics::data::ini::IniReader, heat_sets: usize) -> Result<(), String> {
        let name = video.get_string("POST_PROCESS", "FILTER");
        let load = |name: &str| rustyac_physics::data::ini::IniReader::load(&data.join("system/cfg/ppfilters").join(format!("{name}.ini")));
        if video.get_int("POST_PROCESS", "HEAT_SHIMMER").unwrap_or(0) != 0 {
            // the ctor's loadPPSet("default"), then PostProcessEffectsUpdater's loadPPSet(FILTER):
            // each reads [HEAT_SHIMMER] and makes a particle manager (initPostProcessing)
            let mut heat = crate::heat_shimmer::HeatShimmer::new(heat_sets);
            for name in if name == "default" { vec!["default"] } else { vec!["default", name.as_str()] } {
                heat.read(&load(name)?)?;
                heat.remake(&mut graphics.crt_rand);
            }
            self.heat = Some(heat);
        }
        let filter = load(&name)?;
        // PFX_INITPARAM's particle count: m_uiMaxParticles with the shimmer, else YEBIS's 256
        let particles = self.heat.as_ref().map_or(256, |h| h.sets.iter().map(Vec::len).sum::<usize>().max(1) as u32);
        let base = crate::yebis::base_width(video.get_int("POST_PROCESS", "QUALITY").unwrap_or(0), self.width);
        self.yebis = Some(crate::yebis::Yebis::new(&graphics.kgl.device, &graphics.kgl.context, root, filter, video, self.width, self.height, base, Self::destination(&self.fxaa_temp_rt, graphics), particles)?);
        // YEBIS's initPostProcessing leaves its destination bound without depth (and kgl's cache unaware)
        unsafe { graphics.kgl.context.OMSetRenderTargets(Some(&[Self::destination(&self.fxaa_temp_rt, graphics)]), None) };
        Ok(())
    }

    /// `CameraForwardYebis::renderApplyEffect` 0x14002a740.
    fn render_apply_effect(&mut self, graphics: &mut Graphics, scene: &mut Scene, blurred: Option<NodeId>, unblurred: NodeId, root: NodeId, dt: f32) {
        let Some(ms) = self.yebis_rt_ms.take() else { return };
        graphics.kgl.set_render_targets(ms.kid_color.as_ref(), ms.kid_depth.as_ref());
        graphics.stats.is_in_main_render_pass = true;
        match blurred {
            Some(blurred) if self.forward.use_blur && self.debug_motion_blur_enabled => self.forward.render_blurred(graphics, scene, blurred, unblurred, root, dt, &ms),
            _ => self.forward.base.render_pass_into(graphics, scene, root, Some(&ms)),
        }
        graphics.stats.is_in_main_render_pass = false;
        if graphics.video.aa_samples != 1 {
            if let (Some(from), Some(to)) = (&ms.kid_color, self.yebis_rt_resolved_color.as_ref().and_then(|t| t.kid_color.as_ref())) {
                // ResolveSubresource(resolved, 0, multisampled, 0, DXGI_FORMAT_R16G16B16A16_FLOAT)
                graphics.kgl.resolve_render_target(from, to, 1);
            }
            graphics.set_blend_mode(BLEND_OPAQUE);
            // the resolved depth's colour view alone, the multisampled depth in slot 1
            graphics.kgl.set_render_targets(self.yebis_rt_resolved_depth.as_ref().and_then(|t| t.kid_color.as_ref()), None);
            graphics.kgl.set_texture(1, &ms.kid_depth.as_ref().and_then(|d| d.shader_resource_view.clone()));
            graphics.set_viewport(0, 0, self.width, self.height);
            crate::gl::full_screen_quad(graphics, self.depth_resolve_shader);
        }
        // CameraForwardYebis::applyPostProcessing 0x140025c30: YEBIS. Its source is the resolved
        // colour (or the scene target itself), its destination the FXAA target or the screen.
        #[cfg(all(windows, not(feature = "backend-wgpu")))]
        if let Some(yebis) = &mut self.yebis {
            let depth = match &self.yebis_rt_resolved_depth {
                Some(resolved) => resolved.kid_color.as_ref(),
                None => ms.kid_depth.as_ref(),
            };
            let scene = match &self.yebis_rt_resolved_color {
                Some(resolved) => resolved.kid_color.as_ref(),
                None => ms.kid_color.as_ref(),
            };
            let destination = Self::destination(&self.fxaa_temp_rt, graphics);
            let camera = &self.forward.base.camera;
            let camera = crate::yebis_cb::Camera {
                near: camera.near_plane,
                far: camera.far_plane,
                fov_deg: camera.fov,
                width: self.width as u32,
                height: self.height as u32,
                exposure: camera.exposure,
                exposure_multiplier: graphics.exposure_multiplier,
                dt,
                chromatic_aberration: true,
                new_exposure: true,
            };
            // setPostProcessing 0x140025080: the particles move, then setupPfxParticles hands them to YEBIS
            let mut shimmer = None;
            if let Some(heat) = &mut self.heat {
                heat.update(dt, &mut graphics.crt_rand);
                if heat.active && heat.volumes.iter().take(heat.sets.len()).any(|v| v.0 != 0) {
                    let m = self.forward.base.camera.get_view_matrix().m;
                    shimmer = Some(crate::yebis_heat::Input { elements: crate::yebis_heat::elements(heat), view: std::array::from_fn(|k| m[k / 4][k % 4]), params: crate::yebis_heat::Params::new(heat) });
                }
            }
            if let (Some(depth), Some(scene), Some(destination)) = (depth.and_then(|t| t.shader_resource_view.as_ref()), scene.and_then(|t| t.shader_resource_view.as_ref()), destination) {
                if let Err(e) = yebis.render(depth, scene, &destination, &camera, shimmer.as_ref()) {
                    eprintln!("{e}");
                }
            }
            graphics.trigger_query("YEBIS");
            self.yebis_rt_ms = Some(ms);
            return;
        }
        {
            let source = match &self.yebis_rt_resolved_color {
                Some(resolved) => resolved.kid_color.as_ref(),
                None => ms.kid_color.as_ref(),
            };
            let destination = self.fxaa_temp_rt.as_ref().and_then(|t| t.kid_color.as_ref());
            if let (Some(source), Some(shader)) = (source, self.stand_in_shader) {
                stand_in_apply_effects(graphics, shader, source, destination, self.width, self.height);
            }
        }
        graphics.trigger_query("YEBIS");
        self.yebis_rt_ms = Some(ms);
    }

    /// `CameraForwardYebis::resetYebisStates` 0x14002a650: YEBIS leaves the device as it
    /// pleases; everything goes back to the start of a frame and the screen is bound, without
    /// a depth buffer.
    fn reset_yebis_states(&mut self, graphics: &mut Graphics) {
        graphics.kgl.set_default_state();
        graphics.set_sampler_state();
        graphics.reset_render_states();
        graphics.kgl.set_screen_render_targets(false);
        graphics.kgl.set_viewport(0.0, 0.0, self.width as f32, self.height as f32);
    }
}

/// `RenderTarget(graphics, format, width, height, colour, no depth, 1)` with one sample.
fn color_target(graphics: &Graphics, format: crate::kgl::Format, width: i32, height: i32) -> RenderTarget {
    let color = graphics.kgl.create_render_target(format, width, height, 1, 1);
    RenderTarget { kid_color: Some(color), kid_depth: None, width, height }
}

/// NOT Assetto Corsa: what stands where YEBIS' `ApplyEffects` is when YEBIS is not started. The source
/// picture is copied into the destination (`None`: the screen) as it is, by one full-screen
/// quad with the game's `ksPostCopy` shader; a target of 8 bits a channel cuts values above 1.
/// The render oracle's game side draws exactly this in place of `applyPostProcessing`.
pub fn stand_in_apply_effects(graphics: &mut Graphics, shader: ShaderId, source: &KglRenderTarget, destination: Option<&KglRenderTarget>, width: i32, height: i32) {
    graphics.set_blend_mode(BLEND_OPAQUE);
    graphics.set_depth_mode(DEPTH_OFF);
    match destination {
        Some(target) => graphics.kgl.set_render_targets(Some(target), None),
        None => graphics.kgl.set_screen_render_targets(false),
    }
    graphics.kgl.set_viewport(0.0, 0.0, width as f32, height as f32);
    graphics.kgl.set_texture(0, &source.shader_resource_view);
    crate::gl::full_screen_quad(graphics, Some(shader));
    graphics.kgl.set_texture(0, &None);
}

/// `HDRLevels` (`Sim::hdrLevels`): `Sim::Sim` 0x140192070 starts them at 0 and 10000.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrLevels {
    pub min_exposure: f32,
    pub max_exposure: f32,
}

/// `Sim::initHDRLevels` 0x140199b00: `system/cfg/lighting.ini [HDR] MIN_EXPOSURE` and
/// `MAX_EXPOSURE` (a missing key reads as 0; a missing file leaves the defaults). The game's
/// camera managers copy the pair into the scene camera every frame
/// ([`crate::camera::Camera::min_exposure`]) and nothing ever reads it there: YEBIS takes its
/// exposure limits from the filter file (`[AUTO_EXPOSURE]`), not from here.
pub fn init_hdr_levels(game_folder: &std::path::Path) -> HdrLevels {
    let mut levels = HdrLevels { min_exposure: 0.0, max_exposure: 10000.0 };
    let path = game_folder.join("system/cfg/lighting.ini");
    if path.vfs_is_file() {
        if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&path) {
            levels.min_exposure = ini.get_float("HDR", "MIN_EXPOSURE").unwrap_or(0.0);
            levels.max_exposure = ini.get_float("HDR", "MAX_EXPOSURE").unwrap_or(0.0);
        }
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_levels_of_a_made_up_file() {
        let folder = std::env::temp_dir().join(format!("rustyac_hdr_levels_{}", std::process::id()));
        // (no file: the constructor's values)
        assert_eq!(init_hdr_levels(&folder), HdrLevels { min_exposure: 0.0, max_exposure: 10000.0 });
    }
}
