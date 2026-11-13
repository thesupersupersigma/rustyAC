// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `StereoCameraVive`, the camera of video.ini `[CAMERA] MODE=OPENVR`: the scene drawn once per
//! eye into the headset's two textures, handed to the OpenVR compositor after the frame, and
//! the window showing the left eye of the frame before.
//! `StereoCameraVive::StereoCameraVive` 0x140220b80, `initOpenVR` 0x140223440, `render`
//! 0x140223720, `renderNoPP` 0x140223aa0, `finishRendering` 0x140223210.
//!
//! The runtime is reached as the game reaches it: the six flat functions of `openvr_api.dll`
//! and the interfaces `IVRSystem_015`, `IVRCompositor_020` and `IVRSettings_002` by their
//! vtable slots. Whoever makes the camera hands over the functions ([`OpenVrApi`]): the
//! player's own `openvr_api.dll`, or a stand-in runtime in a proof.
//!
//! Post-processing (Task Y3e): one YEBIS for both eyes, `StereoCameraVive::renderYebis` 0x140223f60.

use std::ffi::{c_char, c_void, CStr};

use rustyac_math::{cosf, sinf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f};
use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

use crate::camera::Camera;
use crate::forward::CameraForward;
use crate::graphics::{Graphics, RenderTarget, CULL_FRONT, DEPTH_NORMAL, DEPTH_NO_WRITE, DEPTH_OFF};
use crate::kgl::{FORMAT_R32F, FORMAT_RGBA16F, FORMAT_RGBA8};
use crate::shader::ShaderId;
use crate::material::{MaterialFilter, PASS_OPAQUE, PASS_TRANSPARENT};
use crate::scene::{NodeId, RenderContext, Scene};

/// An OpenVR interface: a pointer to its vtable.
pub type Interface = *mut *const usize;

/// The flat functions of `openvr_api.dll` the game imports (delay-loaded), and `GetAsyncKeyState`
/// (the recentring keys).
#[derive(Clone, Copy)]
pub struct OpenVrApi {
    pub init_internal: unsafe extern "C" fn(*mut i32, i32) -> u32,
    pub is_interface_version_valid: unsafe extern "C" fn(*const c_char) -> bool,
    pub get_generic_interface: unsafe extern "C" fn(*const c_char, *mut i32) -> Interface,
    pub get_init_token: unsafe extern "C" fn() -> u32,
    pub shutdown_internal: unsafe extern "C" fn(),
    pub error_description: unsafe extern "C" fn(i32) -> *const c_char,
    pub get_async_key_state: unsafe extern "system" fn(i32) -> i16,
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}
#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

/// The game's own `system/x64/openvr_api.dll` of the Assetto Corsa folder (the game delay-loads
/// it from there), or `None`.
pub fn load(ac_root: &std::path::Path) -> Option<OpenVrApi> {
    use std::os::windows::ffi::OsStrExt;
    let path = ac_root.join("system/x64/openvr_api.dll");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a terminated path; the exports are OpenVR's flat functions, the game's imports
    unsafe {
        let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x8);
        if module.is_null() {
            return None;
        }
        let symbol = |name: &CStr| -> Option<usize> {
            let p = GetProcAddress(module, name.as_ptr().cast());
            (!p.is_null()).then_some(p as usize)
        };
        Some(OpenVrApi {
            init_internal: std::mem::transmute::<usize, unsafe extern "C" fn(*mut i32, i32) -> u32>(symbol(c"VR_InitInternal")?),
            is_interface_version_valid: std::mem::transmute::<usize, unsafe extern "C" fn(*const c_char) -> bool>(symbol(c"VR_IsInterfaceVersionValid")?),
            get_generic_interface: std::mem::transmute::<usize, unsafe extern "C" fn(*const c_char, *mut i32) -> Interface>(symbol(c"VR_GetGenericInterface")?),
            get_init_token: std::mem::transmute::<usize, unsafe extern "C" fn() -> u32>(symbol(c"VR_GetInitToken")?),
            shutdown_internal: std::mem::transmute::<usize, unsafe extern "C" fn()>(symbol(c"VR_ShutdownInternal")?),
            error_description: std::mem::transmute::<usize, unsafe extern "C" fn(i32) -> *const c_char>(symbol(c"VR_GetVRInitErrorAsEnglishDescription")?),
            get_async_key_state: GetAsyncKeyState,
        })
    }
}

/// A vtable slot of an interface, as the function type it is called with.
unsafe fn slot<F: Copy>(instance: Interface, index: usize) -> F {
    std::mem::transmute_copy(&*(*instance).add(index))
}

/// The runtime as the game holds it: `vr::VRToken`, `OpenVRInternal_ModuleContext`, `pHMD` and
/// `initializedV`.
pub struct OpenVr {
    pub api: OpenVrApi,
    token: u32,
    system: Interface,
    compositor: Interface,
    settings: Interface,
    /// `pHMD`
    pub hmd: Interface,
    /// `initializedV`
    pub initialized: bool,
}

impl OpenVr {
    pub fn new(api: OpenVrApi) -> OpenVr {
        OpenVr { api, token: 0, system: std::ptr::null_mut(), compositor: std::ptr::null_mut(), settings: std::ptr::null_mut(), hmd: std::ptr::null_mut(), initialized: false }
    }

    /// The interface getters' start (`vr::VRSystem` 0x1402227c0 and the others): a new token
    /// forgets every interface.
    fn check_token(&mut self) {
        unsafe {
            if self.token != (self.api.get_init_token)() {
                (self.system, self.compositor, self.settings) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                self.token = (self.api.get_init_token)();
            }
        }
    }

    fn interface(&mut self, which: usize, name: &CStr) -> Interface {
        self.check_token();
        let cached = match which {
            0 => &mut self.system,
            1 => &mut self.compositor,
            _ => &mut self.settings,
        };
        if cached.is_null() {
            let mut error = 0;
            *cached = unsafe { (self.api.get_generic_interface)(name.as_ptr(), &mut error) };
        }
        *cached
    }

    /// `vr::VRSystem` 0x1402227c0.
    pub fn system(&mut self) -> Interface {
        self.interface(0, c"IVRSystem_015")
    }

    /// `vr::VRCompositor` 0x1402226a0.
    pub fn compositor(&mut self) -> Interface {
        self.interface(1, c"IVRCompositor_020")
    }

    /// `vr::VRSettings` 0x140222730.
    pub fn settings(&mut self) -> Interface {
        self.interface(2, c"IVRSettings_002")
    }

    /// `StereoCameraVive::initOpenVR` 0x140223440 (`vr::VR_Init` as a scene application).
    pub fn init(&mut self) -> bool {
        let mut error = 0;
        unsafe {
            self.token = (self.api.init_internal)(&mut error, 1);
            (self.system, self.compositor, self.settings) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            let mut hmd = std::ptr::null_mut();
            if error == 0 {
                if (self.api.is_interface_version_valid)(c"IVRSystem_015".as_ptr()) {
                    hmd = self.system();
                } else {
                    (self.api.shutdown_internal)();
                    error = 0x69;
                }
            }
            if error != 0 {
                self.hmd = std::ptr::null_mut();
                let text = (self.api.error_description)(error);
                let text = if text.is_null() { String::new() } else { CStr::from_ptr(text).to_string_lossy().into_owned() };
                eprintln!("Unable to init VR runtime: {text}");
                return false;
            }
            self.hmd = hmd;
        }
        self.initialized = true;
        true
    }

    fn get_float(&mut self, section: &CStr, key: &CStr) -> f32 {
        let settings = self.settings();
        unsafe { slot::<unsafe extern "C" fn(Interface, *const c_char, *const c_char, *mut i32) -> f32>(settings, 8)(settings, section.as_ptr(), key.as_ptr(), std::ptr::null_mut()) }
    }

    /// `IVRSettings::GetFloat` with an error to read back.
    fn get_float_error(&mut self, section: &CStr, key: &CStr) -> (f32, i32) {
        let settings = self.settings();
        let mut error = 0;
        let value = unsafe { slot::<unsafe extern "C" fn(Interface, *const c_char, *const c_char, *mut i32) -> f32>(settings, 8)(settings, section.as_ptr(), key.as_ptr(), &mut error) };
        (value, error)
    }

    /// `IVRSettings::SetFloat` with an error, then `Sync(false)`: whether it synced.
    fn set_float_error_and_sync(&mut self, section: &CStr, key: &CStr, value: f32) -> bool {
        let settings = self.settings();
        let mut error = 0;
        unsafe { slot::<unsafe extern "C" fn(Interface, *const c_char, *const c_char, f32, *mut i32)>(settings, 4)(settings, section.as_ptr(), key.as_ptr(), value, &mut error) };
        let settings = self.settings();
        unsafe { slot::<unsafe extern "C" fn(Interface, bool, *mut i32) -> bool>(settings, 1)(settings, false, std::ptr::null_mut()) }
    }

    /// `IVRSettings::SetFloat` then `Sync(false)`.
    fn set_float_and_sync(&mut self, section: &CStr, key: &CStr, value: f32) {
        let settings = self.settings();
        unsafe { slot::<unsafe extern "C" fn(Interface, *const c_char, *const c_char, f32, *mut i32)>(settings, 4)(settings, section.as_ptr(), key.as_ptr(), value, std::ptr::null_mut()) };
        let settings = self.settings();
        unsafe { slot::<unsafe extern "C" fn(Interface, bool, *mut i32) -> bool>(settings, 1)(settings, false, std::ptr::null_mut()) };
    }
}

/// `vr::TrackedDevicePose_t` (0x50 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrackedDevicePose {
    pub device_to_absolute_tracking: [[f32; 4]; 3],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub tracking_result: i32,
    pub pose_is_valid: bool,
    pub device_is_connected: bool,
}

const _: () = assert!(std::mem::size_of::<TrackedDevicePose>() == 0x50);

/// `vr::Texture_t`.
#[repr(C)]
pub struct Texture {
    pub handle: *mut c_void,
    /// `TextureType_DirectX` 0
    pub kind: i32,
    /// `ColorSpace_Gamma` 1
    pub color_space: i32,
}

/// An OpenVR `HmdMatrix34_t` as the game turns it into a matrix (`ConvertSteamVRMatrixToMatrix4`).
fn matrix34(m: &[[f32; 4]; 3]) -> Mat44f {
    Mat44f { m: [[m[0][0], m[1][0], m[2][0], 0.0], [m[0][1], m[1][1], m[2][1], 0.0], [m[0][2], m[1][2], m[2][2], 0.0], [m[0][3], m[1][3], m[2][3], 1.0]] }
}

/// `StereoCameraVive::computeCameraMatrix` 0x140222850: `matrix` (an eye's place in the head's
/// space) seen from the camera `c`: its rotation made clean through a quaternion, its place
/// moved by the camera's.
pub fn compute_camera_matrix(c: &Mat44f, matrix: &Mat44f) -> Mat44f {
    let r = crate::animator::xm_matrix_rotation_quaternion(&crate::animator::xm_quaternion_rotation_matrix(matrix));
    let p = matrix.m[3];
    place(c, &r, [p[0], p[1], p[2]])
}

/// The common tail of both VR cameras' `computeCameraMatrix`: the rotation `r` after the
/// camera's, the place `p` moved into the camera's.
pub(crate) fn place(c: &Mat44f, r: &Mat44f, p: [f32; 3]) -> Mat44f {
    let mut camera = *c;
    camera.m[3] = [0.0, 0.0, 0.0, c.m[3][3]];
    let mut out = xm_matrix_multiply(r, &camera);
    let c = &c.m;
    for k in 0..3 {
        out.m[3][k] = (((p[1] * c[1][k]) + (p[0] * c[0][k])) + (p[2] * c[2][k])) + c[3][k];
    }
    out
}

/// `StereoCameraVive::normalRendering` 0x140223520 (`StereoCameraForward::normalRendering`
/// 0x140213030 is the same): one eye, cleared magenta.
pub(crate) fn normal_rendering(forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId, render_scene: bool, proj: Mat44f, view: Mat44f) {
    let camera = &mut forward.base.camera;
    graphics.clear_render_target(&[1.0, 0.0, 1.0, 1.0]);
    graphics.clear_render_target_depth(1.0);
    graphics.set_depth_mode(DEPTH_NORMAL);
    let max_layer = camera.max_layer as i32;
    let mut filter = MaterialFilter::plain();
    graphics.set_cull_mode(CULL_FRONT);
    camera.render_with(graphics, proj, view);
    if !render_scene {
        return;
    }
    if let Some(sky) = &mut forward.base.sky_box {
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
        sky.render(graphics, &mut rc, camera.is_cube_map_camera);
        graphics.clear_render_target_depth(camera.clear_depth);
    }
    let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: camera.cull_camera() };
    scene.render(graphics, root, &mut rc);
    if camera.axis_rendering_mode != 0 {
        camera.render_axis(graphics, camera.axis_rendering_mode == 1);
    }
    let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_TRANSPARENT, max_layer, camera: camera.cull_camera() };
    graphics.set_depth_mode(DEPTH_NO_WRITE);
    scene.render(graphics, root, &mut rc);
    graphics.set_depth_mode(DEPTH_NORMAL);
}

/// Where a headset's YEBIS comes from: `root` holds the player's acs.exe, `data` the game's
/// working folder (its `system/cfg/ppfilters/default.ini`), `video` video.ini.
pub struct YebisFrom<'a> {
    pub root: &'a std::path::Path,
    pub data: &'a std::path::Path,
    pub video: IniReader,
}

/// What `StereoCameraVive` (0x1418 bytes) adds to its `CameraForward`.
pub struct StereoCameraVive {
    pub vr: OpenVr,
    /// `renderScene`
    pub render_scene: bool,
    /// openvr.ini `[MIRROR_TEXTURE] ENABLED`: the window shows the left eye
    pub mirror_enabled: bool,
    /// `viveData[i].renderEye`: what is handed to the compositor
    pub render_eye: [RenderTarget; 2],
    /// `viveData[i].eyePose`: the eye's place in the head (`GetEyeToHeadTransform`)
    pub eye_pose: [Mat44f; 2],
    /// `viveData[i].eyeProjection`
    pub eye_projection: [Mat44f; 2],
    /// the head's pose of the frame (`m_rTrackedDevicePose[0]`)
    pub hmd_matrix: Mat44f,
    /// SteamVR's `steamvr/ipd`
    pub eyes_distance_ref: f32,
    /// SteamVR's `ipd` plus `ipdOffset` at the start
    pub eyes_distance_default: f32,
    pub eyes_distance: f32,
    /// how far `ipdOffset` may go (0.015)
    pub max_distance_offset: f32,
    pub proj_matrix: Mat44f,
    pub view_matrix: Mat44f,
    pub poses: [TrackedDevicePose; 16],
    pub device_pose: [Mat44f; 16],
    pub render_width: u32,
    pub render_height: u32,
    rt_temp: RenderTarget,
    /// `rtFXAA_Temp`: YEBIS's destinations when FXAA is on
    fxaa_temp: Vec<RenderTarget>,
    fxaa_shader: Option<ShaderId>,
    depth_resolve_shader: Option<ShaderId>,
    /// `yebisPP` (video.ini ppHDREnabled): one YEBIS for both eyes
    yebis: Option<crate::yebis::Yebis>,
    /// `rtYebisTarget` (multisampled with AA), `rtYebisResolved`, `rtYebisResolvedDepth`
    rt_yebis_target: RenderTarget,
    rt_yebis_resolved: Option<RenderTarget>,
    rt_yebis_resolved_depth: Option<RenderTarget>,
    /// `mirrorTextureBuffers[0]`: a view of the left eye's texture
    mirror_texture: Option<ID3D11ShaderResourceView>,
    /// video.ini `[VR_ROOM] ROTATION` (radians), room scale only
    pub rotation_offset: f32,
    /// openvr.ini `[SETTINGS] MOTION_TRACKER`: 0 none, 1 `LEFT_HAND`, 2 `RIGHT_HAND`, 3 `TRACKER`
    pub motion_tracker: i32,
    /// `hmdMatrixTracker`: the motion tracker's pose, its place dropped
    pub hmd_matrix_tracker: Mat44f,
}

impl StereoCameraVive {
    /// `StereoCameraVive::StereoCameraVive` 0x140220b80, after `forward`'s own constructor.
    /// `room_scale`: the constructor's last flag, the compositor's tracking space (false:
    /// seated) and whether video.ini `[VR_ROOM] ROTATION` turns the room. `yebis`: where YEBIS
    /// comes from when post-processing is on.
    pub fn new(graphics: &mut Graphics, forward: &mut CameraForward, mut vr: OpenVr, room_scale: bool, yebis: Option<YebisFrom>) -> Result<StereoCameraVive, String> {
        forward.base.camera.lod_multiplier = 0.5;
        if !vr.initialized && !vr.init() {
            return Err("Could not initialize the vive".into());
        }
        let empty = || RenderTarget { kid_color: None, kid_depth: None, width: 0, height: 0 };
        let mut camera = StereoCameraVive {
            vr,
            render_scene: true,
            mirror_enabled: true,
            render_eye: [empty(), empty()],
            eye_pose: [Mat44f::default(); 2],
            eye_projection: [Mat44f::default(); 2],
            hmd_matrix: Mat44f::default(),
            eyes_distance_ref: 0.063,
            eyes_distance_default: 0.0,
            eyes_distance: 0.0,
            max_distance_offset: 0.015,
            proj_matrix: Mat44f::default(),
            view_matrix: Mat44f::default(),
            poses: [TrackedDevicePose::default(); 16],
            device_pose: [Mat44f::default(); 16],
            render_width: 0,
            render_height: 0,
            rt_temp: empty(),
            fxaa_temp: Vec::new(),
            fxaa_shader: None,
            depth_resolve_shader: None,
            yebis: None,
            rt_yebis_target: empty(),
            rt_yebis_resolved: None,
            rt_yebis_resolved_depth: None,
            mirror_texture: None,
            rotation_offset: 0.0,
            motion_tracker: 0,
            hmd_matrix_tracker: Mat44f::default(),
        };
        camera.configure(graphics);
        let hmd = camera.vr.hmd;
        let (mut width, mut height) = (0u32, 0u32);
        unsafe { slot::<unsafe extern "C" fn(Interface, *mut u32, *mut u32)>(hmd, 0)(hmd, &mut width, &mut height) };
        (camera.render_width, camera.render_height) = (width, height);
        let compositor = camera.vr.compositor();
        unsafe { slot::<unsafe extern "C" fn(Interface, i32)>(compositor, 0)(compositor, room_scale as i32) };
        camera.depth_resolve_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksMSDepthResolve")?);
        let (w, h, aa) = (width as i32, height as i32, graphics.video.aa_samples);
        for eye in &mut camera.render_eye {
            *eye = RenderTarget { kid_color: Some(graphics.kgl.create_render_target(FORMAT_RGBA8, w, h, 1, 1)), kid_depth: None, width: w, height: h };
        }
        let target = |graphics: &Graphics, format, samples: i32, depth: bool| -> Result<RenderTarget, String> {
            let color = graphics.kgl.create_render_target(format, w, h, samples, 1);
            let depth = if depth { Some(graphics.kgl.create_render_target_depth(w, h, samples)?) } else { None };
            Ok(RenderTarget { kid_color: Some(color), kid_depth: depth, width: w, height: h })
        };
        if !graphics.video.pp_hdr_enabled {
            camera.rt_temp = target(graphics, FORMAT_RGBA8, aa, true)?;
        } else {
            // (the constructor turns videoSettings.ppHeatShimmer off: no shimmer in VR)
            let YebisFrom { root, data, video } = yebis.ok_or("VR with post-processing needs YEBIS")?;
            // YebisPP(graphics, w, h, renderEye[0]'s view) 0x14002ba30, loadPPSet("default") 0x14002cc80:
            // initPP 0x140030540 takes the eye's width as the base width, whatever [POST_PROCESS] QUALITY
            let filter = IniReader::load(&data.join("system/cfg/ppfilters/default.ini"))?;
            let screen = camera.render_eye[0].kid_color.as_ref().and_then(|c| c.render_target_view.clone());
            camera.yebis = Some(crate::yebis::Yebis::new(&graphics.kgl.device, &graphics.kgl.context, root, filter, video, w, h, w, screen.clone(), 256)?);
            // YEBIS's initialisation leaves its destination bound without depth (and kgl's cache unaware)
            unsafe { graphics.kgl.context.OMSetRenderTargets(Some(&[screen]), None) };
            camera.rt_yebis_target = target(graphics, FORMAT_RGBA16F, aa, true)?;
            if aa != 1 {
                camera.rt_yebis_resolved = Some(target(graphics, FORMAT_RGBA16F, 1, false)?);
                camera.rt_yebis_resolved_depth = Some(target(graphics, FORMAT_R32F, 1, false)?);
            }
        }
        if graphics.video.pp_fxaa && aa == 1 {
            for _ in 0..2 {
                camera.fxaa_temp.push(target(graphics, FORMAT_RGBA8, 1, false)?);
            }
            camera.fxaa_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksFXAA")?);
        }
        for eye in 0..2 {
            camera.eye_projection[eye] = camera.projection_eye(&forward.base.camera, eye as i32);
            camera.eye_pose[eye] = camera.pose_eye(eye as i32);
        }
        graphics.video.v_sync = false;
        graphics.kgl.video.v_sync = false;
        let texture = camera.render_eye[0].kid_color.as_ref().and_then(|c| c.texture.clone()).ok_or("no eye texture")?;
        let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT(0x1c),
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
        };
        unsafe { graphics.kgl.device.CreateShaderResourceView(&texture, Some(&desc), Some(&mut camera.mirror_texture)) }.map_err(|e| e.to_string())?;
        if room_scale {
            if let Ok(ini) = IniReader::load(&graphics.documents_folder.join("cfg/video.ini")) {
                camera.rotation_offset = ini.get_int("VR_ROOM", "ROTATION").unwrap_or(0) as f32;
            }
        }
        if camera.vr.compositor().is_null() {
            eprintln!("Compositor initialization failed.");
        }
        Ok(camera)
    }

    /// `StereoCameraVive::configureOpenVR` 0x140222c20: Documents cfg/openvr.ini and SteamVR's
    /// distance of the eyes.
    fn configure(&mut self, graphics: &Graphics) {
        self.eyes_distance = 0.0;
        self.motion_tracker = 0;
        if let Ok(ini) = IniReader::load(&graphics.documents_folder.join("cfg/openvr.ini")) {
            if ini.has_section("MIRROR_TEXTURE") {
                self.mirror_enabled = ini.get_int("MIRROR_TEXTURE", "ENABLED").unwrap_or(0) != 0;
            }
            if ini.has_section("SETTINGS") {
                self.eyes_distance = ini.get_float("SETTINGS", "PUPILLARY_DISTANCE").unwrap_or(0.0) * 0.001;
                if ini.has_key("SETTINGS", "MOTION_TRACKER") {
                    match ini.get_string("SETTINGS", "MOTION_TRACKER").as_str() {
                        "LEFT_HAND" => self.motion_tracker = 1,
                        "RIGHT_HAND" => self.motion_tracker = 2,
                        "TRACKER" => self.motion_tracker = 3,
                        _ => {}
                    }
                }
            }
        }
        self.eyes_distance_ref = self.vr.get_float(c"steamvr", c"ipd");
        let offset = self.vr.get_float(c"steamvr", c"ipdOffset");
        self.eyes_distance_default = self.eyes_distance_ref + offset;
        if self.eyes_distance <= 0.0 {
            self.eyes_distance = self.eyes_distance_default;
        }
        // (`ucomiss`: an unordered pair counts as equal, so not `!=`)
        if self.eyes_distance.partial_cmp(&self.eyes_distance_default).is_some_and(|o| o.is_ne()) {
            self.set_eyes_distance(self.eyes_distance - self.eyes_distance_ref);
        }
    }

    /// `StereoCameraVive::setEyesDistance` 0x140224310: SteamVR's `ipdOffset`, kept within
    /// ±`max_distance_offset`.
    pub fn set_eyes_distance(&mut self, offset: f32) {
        let max = self.max_distance_offset;
        let offset = if offset > max {
            max
        } else if offset >= -max {
            offset
        } else {
            -max
        };
        self.eyes_distance = offset + self.eyes_distance_ref;
        self.vr.set_float_and_sync(c"steamvr", c"ipdOffset", offset);
    }

    /// `StereoCameraVive::getRenderMultiplier` 0x1402233e0: SteamVR's supersampling.
    pub fn render_multiplier(&mut self) -> f32 {
        self.vr.get_float(c"steamvr", c"renderTargetMultiplier")
    }

    /// `StereoCameraVive::setRenderMultiplier(float)` 0x1402244d0: `steamvr`'s (with its
    /// messages), then the compositor's; a setting SteamVR cannot read goes back to 1.
    pub fn set_render_multiplier(&mut self, value: f32) {
        self.set_section_render_multiplier(c"steamvr", value);
        let (old, error) = self.vr.get_float_error(c"compositor", c"renderTargetMultiplier");
        let value = if error != 0 {
            1.0
        } else if value == old {
            return;
        } else {
            value
        };
        self.vr.set_float_error_and_sync(c"compositor", c"renderTargetMultiplier", value);
    }

    /// `StereoCameraVive::setRenderMultiplier(section, value, verbose)` 0x1402243a0 (verbose).
    fn set_section_render_multiplier(&mut self, section: &CStr, value: f32) {
        let (old, error) = self.vr.get_float_error(section, c"renderTargetMultiplier");
        if error == 0 {
            if value != old && self.vr.set_float_error_and_sync(section, c"renderTargetMultiplier", value) {
                println!("Changed render multiplier value to {:.6}", value as f64);
            }
        } else {
            let settings = self.vr.settings();
            // IVRSettings::GetSettingsErrorNameFromEnum
            let name = unsafe { slot::<unsafe extern "C" fn(Interface, i32) -> *const c_char>(settings, 0)(settings, error) };
            let name = if name.is_null() { String::new() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
            println!("Error reading render multiplier default setting: {name}");
            self.vr.set_float_error_and_sync(section, c"renderTargetMultiplier", 1.0);
        }
    }

    /// What `FormOpenVR` reads when it is made.
    pub fn open_vr_values(&mut self) -> (f32, f32, f32, f32) {
        (self.render_multiplier(), self.eyes_distance_ref, self.eyes_distance, self.max_distance_offset)
    }

    /// `StereoCameraVive::GetHMDMatrixProjectionEye` 0x140221ac0.
    fn projection_eye(&self, camera: &Camera, eye: i32) -> Mat44f {
        let hmd = self.vr.hmd;
        if hmd.is_null() {
            return Mat44f::default();
        }
        let mut m = [[0f32; 4]; 4];
        unsafe { slot::<unsafe extern "C" fn(Interface, *mut [[f32; 4]; 4], i32, f32, f32) -> *mut [[f32; 4]; 4]>(hmd, 1)(hmd, &mut m, eye, camera.near_plane, camera.far_plane) };
        Mat44f { m: std::array::from_fn(|r| std::array::from_fn(|c| m[c][r])) }
    }

    /// `StereoCameraVive::GetHMDMatrixPoseEye` 0x140221980.
    fn pose_eye(&self, eye: i32) -> Mat44f {
        let hmd = self.vr.hmd;
        if hmd.is_null() {
            return Mat44f::default();
        }
        let mut m = [[0f32; 4]; 3];
        unsafe { slot::<unsafe extern "C" fn(Interface, *mut [[f32; 4]; 3], i32) -> *mut [[f32; 4]; 3]>(hmd, 4)(hmd, &mut m, eye) };
        matrix34(&m)
    }

    /// `StereoCameraVive::computeEyePoses` 0x140222ab0.
    pub fn compute_eye_poses(&mut self) {
        for eye in 0..2 {
            self.eye_pose[eye] = self.pose_eye(eye as i32);
        }
    }

    /// `StereoCameraVive::resetHeadPosition` 0x140224300: `IVRSystem::ResetSeatedZeroPose`.
    pub fn reset_head_position(&mut self) {
        let hmd = self.vr.hmd;
        unsafe { slot::<unsafe extern "C" fn(Interface)>(hmd, 11)(hmd) };
    }

    /// `StereoCameraVive::UpdateHMDMatrixPose` 0x140221c00 (no motion tracker): the poses of
    /// the frame, the head's into `hmd_matrix`, turned by the room's rotation.
    pub fn update_hmd_matrix_pose(&mut self) {
        if self.vr.hmd.is_null() {
            return;
        }
        let compositor = self.vr.compositor();
        unsafe { slot::<unsafe extern "C" fn(Interface, *mut TrackedDevicePose, u32, *mut TrackedDevicePose, u32) -> i32>(compositor, 2)(compositor, self.poses.as_mut_ptr(), 16, std::ptr::null_mut(), 0) };
        for (pose, matrix) in self.poses.iter().zip(&mut self.device_pose) {
            if pose.pose_is_valid {
                *matrix = matrix34(&pose.device_to_absolute_tracking);
            }
        }
        if self.motion_tracker != 0 {
            // the hand's controller (IVRSystem::GetTrackedDeviceIndexForControllerRole) or the
            // last generic tracker (GetTrackedDeviceClass 3), device 0 without one
            let index = match self.motion_tracker {
                role @ (1 | 2) => {
                    let system = self.vr.system();
                    unsafe { slot::<unsafe extern "C" fn(Interface, i32) -> u32>(system, 17)(system, role) as i32 }
                }
                3 => { let hmd = self.vr.hmd; (0..16u32).fold(0, |found, i| if unsafe { slot::<unsafe extern "C" fn(Interface, u32) -> i32>(hmd, 19)(hmd, i) } == 3 { i as i32 } else { found }) }
                _ => 0,
            };
            self.hmd_matrix_tracker = self.device_matrix(index);
        }
        if self.poses[0].pose_is_valid {
            self.hmd_matrix = self.device_pose[0];
        }
        if self.motion_tracker != 0 {
            // the head seen from the tracker: its turn taken out, its place kept
            self.hmd_matrix_tracker.m[3] = [0.0, 0.0, 0.0, self.hmd_matrix_tracker.m[3][3]];
            let inverse = xm_matrix_inverse(&self.hmd_matrix_tracker);
            self.hmd_matrix = xm_matrix_multiply(&self.hmd_matrix, &inverse);
        }
        let a = self.rotation_offset;
        if a != 0.0 {
            let (c, s) = (cosf(a), sinf(a));
            let (k, s0) = ((1.0 - c) * 0.0, s * 0.0);
            let r = Mat44f { m: [[k + c, s0 + k, k - s, 0.0], [k - s0, (1.0 - c) + c, s0 + k, 0.0], [s + k, k - s0, k + c, 0.0], [0.0, 0.0, 0.0, 1.0]] };
            self.hmd_matrix = xm_matrix_multiply(&self.hmd_matrix, &r);
        }
    }

    /// `m_rmat4DevicePose[index]` as the game reads it: a hand with no controller
    /// (`k_unTrackedDeviceIndexInvalid`, -1) reads the 64 bytes before the matrices, the end
    /// of the last device's pose and the eyes' size.
    fn device_matrix(&self, index: i32) -> Mat44f {
        match usize::try_from(index) {
            Ok(i) if i < 16 => self.device_pose[i],
            _ => {
                // shortcut: only -1 is laid out (SteamVR's one invalid index); any other index past the array reads that too
                // (the pose's two padding bytes taken as zero)
                let p = &self.poses[15];
                let m = &p.device_to_absolute_tracking;
                let (v, w) = (p.velocity, p.angular_velocity);
                let flags = f32::from_bits(p.pose_is_valid as u32 | (p.device_is_connected as u32) << 8);
                let size = (f32::from_bits(self.render_width), f32::from_bits(self.render_height));
                let f = [m[1][2], m[1][3], m[2][0], m[2][1], m[2][2], m[2][3], v[0], v[1], v[2], w[0], w[1], w[2], f32::from_bits(p.tracking_result as u32), flags, size.0, size.1];
                Mat44f { m: [[f[0], f[1], f[2], f[3]], [f[4], f[5], f[6], f[7]], [f[8], f[9], f[10], f[11]], [f[12], f[13], f[14], f[15]]] }
            }
        }
    }

    /// `StereoCameraVive::render` 0x140223720: the poses, the window's picture, Ctrl+Space
    /// (recentre), the shadows and the cube map from the left eye, then both eyes.
    pub fn render(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, cube_node: Option<NodeId>, root: NodeId, dt: f32) -> Result<(), String> {
        self.update_hmd_matrix_pose();
        self.render_mirror_texture(graphics);
        unsafe {
            if (self.vr.api.get_async_key_state)(0x11) != 0 && (self.vr.api.get_async_key_state)(0x20) != 0 {
                self.reset_head_position();
            }
        }
        let exposure = forward.base.camera.exposure;
        self.compute_eye_poses();
        let saved = forward.base.camera.matrix;
        forward.base.camera.matrix = compute_camera_matrix(&saved, &self.eye_pose[0]);
        forward.base.camera.fov = 90.0;
        forward.base.shadow_map_pass(graphics, scene, root)?;
        if let Some(node) = cube_node {
            let m = &forward.base.camera.matrix.m;
            let position = [m[3][0], m[3][1], m[3][2]];
            crate::cubemap::render(&mut forward.cube_map_renderer, graphics, scene, &forward.cube_map, node, position, forward.base.sky_box.as_mut());
        }
        forward.base.camera.matrix = saved;
        forward.base.camera.fov = 55.0;
        graphics.stats.is_in_main_render_pass = true;
        if self.yebis.is_some() {
            self.render_yebis(forward, graphics, scene, root, dt);
        } else {
            self.render_no_pp(forward, graphics, scene, root);
        }
        graphics.stats.is_in_main_render_pass = false;
        graphics.set_viewport(0, 0, graphics.video.width, graphics.video.height);
        forward.base.camera.exposure = exposure;
        Ok(())
    }

    /// `StereoCameraVive::viveRenderPass` 0x140224620: the eye from the head's pose, into the
    /// bound target.
    fn vive_render_pass(&mut self, eye: usize, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId) {
        let m = xm_matrix_multiply(&self.eye_pose[eye], &self.hmd_matrix);
        let saved = forward.base.camera.matrix;
        forward.base.camera.matrix = compute_camera_matrix(&saved, &m);
        self.view_matrix = forward.base.camera.get_view_matrix();
        self.eye_projection[eye] = self.projection_eye(&forward.base.camera, eye as i32);
        self.proj_matrix = self.eye_projection[eye];
        graphics.set_viewport(0, 0, self.render_width as i32, self.render_height as i32);
        normal_rendering(forward, graphics, scene, root, self.render_scene, self.proj_matrix, self.view_matrix);
        forward.base.camera.matrix = saved;
    }

    /// `StereoCameraVive::renderYebis` 0x140223f60: each eye into the HDR target, then the one
    /// YEBIS into the eye's texture (through FXAA's target when FXAA is on).
    fn render_yebis(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId, dt: f32) {
        let (w, h) = (self.render_width as i32, self.render_height as i32);
        for eye in 0..2 {
            let target = &self.rt_yebis_target;
            graphics.kgl.set_render_targets(target.kid_color.as_ref(), target.kid_depth.as_ref());
            self.vive_render_pass(eye, forward, graphics, scene, root);
            graphics.kgl.set_render_targets(None, None);
            // updatePPCameraValues 0x14002bcc0 and updatePP 0x14002c270 (no depth of field, no shimmer in VR)
            let c = &forward.base.camera;
            let camera = crate::yebis_cb::Camera { near: c.near_plane, far: c.far_plane, fov_deg: c.fov, width: w as u32, height: h as u32, exposure: c.exposure, exposure_multiplier: graphics.exposure_multiplier, dt, chromatic_aberration: false, dof_focus: None, new_exposure: eye == 0, sun: crate::post::godrays(&c.matrix, &self.proj_matrix, graphics) };
            let target = &self.rt_yebis_target;
            let (scene_srv, depth_srv) = match (&self.rt_yebis_resolved, &self.rt_yebis_resolved_depth) {
                (Some(color), Some(depth)) => {
                    if let (Some(from), Some(to)) = (&target.kid_color, &color.kid_color) {
                        graphics.kgl.resolve_render_target(from, to, 1);
                    }
                    graphics.set_blend_mode(crate::graphics::BLEND_OPAQUE);
                    graphics.kgl.set_render_targets(depth.kid_color.as_ref(), None);
                    graphics.kgl.set_texture(1, &target.kid_depth.as_ref().and_then(|t| t.shader_resource_view.clone()));
                    graphics.set_viewport(0, 0, target.width, target.height);
                    crate::gl::full_screen_quad(graphics, self.depth_resolve_shader);
                    graphics.kgl.set_render_targets(None, None);
                    (color.kid_color.as_ref(), depth.kid_color.as_ref())
                }
                _ => (target.kid_color.as_ref(), target.kid_depth.as_ref()),
            };
            let (scene_srv, depth_srv) = (scene_srv.and_then(|t| t.shader_resource_view.clone()), depth_srv.and_then(|t| t.shader_resource_view.clone()));
            let fxaa = self.fxaa_temp.get(eye);
            let destination = fxaa.unwrap_or(&self.render_eye[eye]).kid_color.as_ref().and_then(|c| c.render_target_view.clone());
            if let (Some(yebis), Some(scene_srv), Some(depth_srv), Some(destination)) = (&mut self.yebis, scene_srv, depth_srv, destination) {
                if let Err(e) = yebis.render(&depth_srv, &scene_srv, &destination, &camera, None) {
                    eprintln!("{e}");
                }
            }
            if let Some(fxaa) = fxaa {
                graphics.kgl.set_default_state();
                graphics.set_sampler_state();
                graphics.reset_render_states();
                graphics.set_render_target(Some(&self.render_eye[eye]));
                graphics.set_viewport(0, 0, w, h);
                graphics.set_depth_mode(DEPTH_OFF);
                graphics.kgl.set_texture(5, &fxaa.kid_color.as_ref().and_then(|c| c.shader_resource_view.clone()));
                crate::gl::full_screen_quad(graphics, self.fxaa_shader);
            }
            graphics.kgl.set_default_state();
            graphics.set_sampler_state();
            graphics.reset_render_states();
            graphics.kgl.set_texture_cube_map(Some(&forward.cube_map), 10);
            for i in 0..3 {
                graphics.set_shadow_map_texture(i as i32, Some(&forward.base.shadow_rt[i]));
                graphics.set_shadow_map_matrix(i as i32, &forward.base.shadow_matrices[i]);
            }
        }
    }

    /// `StereoCameraVive::renderNoPP` 0x140223aa0: each eye from the head's pose into the
    /// multisampled target, then copied (resolved) into the eye's texture.
    fn render_no_pp(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId) {
        for eye in 0..2 {
            graphics.set_render_target(Some(&self.rt_temp));
            self.vive_render_pass(eye, forward, graphics, scene, root);
            let texture = |rt: &RenderTarget| rt.kid_color.as_ref().and_then(|c| c.texture.clone());
            if let (Some(to), Some(from)) = (texture(&self.render_eye[eye]), texture(&self.rt_temp)) {
                unsafe {
                    if graphics.video.aa_samples < 2 {
                        graphics.kgl.context.CopyResource(&to, &from);
                    } else {
                        graphics.kgl.context.ResolveSubresource(&to, 0, &from, 0, DXGI_FORMAT(0x1c));
                    }
                }
            }
        }
    }

    /// `StereoCameraVive::renderMirrorTexture` 0x140223910: the window, with the left eye of the
    /// frame before when openvr.ini asks for it.
    fn render_mirror_texture(&mut self, graphics: &mut Graphics) {
        graphics.set_screen_render_targets();
        graphics.clear_render_target(&[0.5, 0.1, 0.1, 1.0]);
        if !self.mirror_enabled {
            return;
        }
        graphics.clear_render_target_depth(1.0);
        graphics.reset_render_states();
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_depth_mode(DEPTH_OFF);
        graphics.kgl.set_texture(0, &self.mirror_texture);
        graphics.set_screen_space_mode();
        let Some(mut gl) = graphics.gl.take() else { return };
        gl.color3f(1.0, 1.0, 1.0);
        let (w, h) = (graphics.video.width, graphics.video.height);
        gl.quad(graphics, (w - w) as f32 * 0.5, (h - h) as f32 * 0.5, w as f32, h as f32, true, None);
        graphics.gl = Some(gl);
    }

    /// `StereoCameraVive::finishRendering` 0x140223210 (`Game::evOnRenderFinished`, in place of
    /// `GraphicsManager::endScene`): the window's swap, then both eyes to the compositor.
    pub fn finish_rendering(&mut self, graphics: &Graphics) {
        graphics.kgl.swap_buffers();
        self.submit();
    }

    /// `finishRendering`'s part after the swap (a window that swaps on its own calls only this).
    pub fn submit(&mut self) {
        for eye in 0..2 {
            let handle = self.render_eye[eye].kid_color.as_ref().and_then(|c| c.texture.as_ref()).map_or(std::ptr::null_mut(), |t| t.as_raw());
            let texture = Texture { handle, kind: 0, color_space: 1 };
            let compositor = self.vr.compositor();
            unsafe { slot::<unsafe extern "C" fn(Interface, i32, *const Texture, *const c_void, i32) -> i32>(compositor, 5)(compositor, eye as i32, &texture, std::ptr::null(), 0) };
        }
    }

    /// `StereoCameraVive::shutdown` 0x140224580: SteamVR's `ipdOffset` back as it was, the
    /// runtime closed.
    pub fn shutdown(&mut self) {
        if !self.vr.initialized {
            return;
        }
        if self.eyes_distance_default != self.eyes_distance {
            self.vr.set_float_and_sync(c"steamvr", c"ipdOffset", self.eyes_distance_default - self.eyes_distance_ref);
        }
        self.vr.hmd = std::ptr::null_mut();
        unsafe { (self.vr.api.shutdown_internal)() };
    }
}
