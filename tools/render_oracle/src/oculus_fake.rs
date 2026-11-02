// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47b: a stand-in Oculus runtime. The same functions answer the game's
//! `StereoCameraForward` (put into its LibOVR shim's function pointers, the shim's library
//! marked loaded, so its signature check never runs) and the port's (handed over as
//! `rustyac_render::oculus::OvrApi`): a scripted head, offset eyes, an asymmetric field of view,
//! swap chains of three textures. Every call is written down with its arguments (numbers as
//! bits), and the textures a submitted frame names are read back as each eye's picture.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use rustyac_render::oculus::{Chain, EyeRenderDesc, FovPort, HmdDesc, InitParams, LayerEyeFov, OvrApi, Session, SessionStatus, Sizei, SwapChainDesc, TrackingState};
use windows::core::{Interface as _, GUID};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::openvr_fake::Eye;

static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EYES: Mutex<Vec<Eye>> = Mutex::new(Vec::new());
/// The size of an eye's texture (`ovr_GetFovTextureSize`).
static SIZE: Mutex<(i32, i32)> = Mutex::new((320, 360));
static FRAME: AtomicU32 = AtomicU32::new(0);
const SESSION: usize = 0x5e55;
const LENGTH: i32 = 3;

/// A swap chain: its textures (raw, owned), the current one, the last committed one.
struct SwapChain {
    textures: Vec<usize>,
    index: i32,
    committed: i32,
}
static CHAINS: Mutex<Vec<SwapChain>> = Mutex::new(Vec::new());

fn log(line: String) {
    CALLS.lock().unwrap().push(line);
}

fn bits(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

fn fov(f: &FovPort) -> String {
    [f.up, f.down, f.left, f.right].map(bits).join(",")
}

/// A chain as the log names it (its number, not its address).
fn chain_id(chain: Chain) -> usize {
    (chain as usize).wrapping_sub(0x100)
}

pub fn set_size(width: u32, height: u32) {
    *SIZE.lock().unwrap() = (width as i32, height as i32);
}

pub fn take_calls() -> Vec<String> {
    std::mem::take(&mut CALLS.lock().unwrap())
}

pub fn take_eyes() -> Vec<Eye> {
    std::mem::take(&mut EYES.lock().unwrap())
}

/// The headset's fields of view: the inner side narrower, mirrored for the right eye.
fn eye_fov(eye: usize) -> FovPort {
    let (inner, outer) = (0.97f32, 1.06f32);
    if eye == 0 {
        FovPort { up: 1.33, down: 1.47, left: outer, right: inner }
    } else {
        FovPort { up: 1.33, down: 1.47, left: inner, right: outer }
    }
}

extern "C" fn initialize(params: *const InitParams) -> i32 {
    let p = unsafe { &*params };
    log(format!("ovr_Initialize {} {} {} {} {}", p.flags, p.requested_minor_version, p.log_callback, p.user_data, p.connection_timeout_ms));
    0
}
extern "C" fn shutdown() {
    log("ovr_Shutdown".into());
}
extern "C" fn create(session: *mut Session, luid: *mut [u8; 8]) -> i32 {
    log("ovr_Create".into());
    unsafe {
        *session = SESSION as Session;
        *luid = [1, 2, 3, 4, 0, 0, 0, 0];
    }
    0
}
extern "C" fn get_hmd_desc(out: *mut HmdDesc, session: Session) -> *mut HmdDesc {
    log(format!("ovr_GetHmdDesc {}", session as usize == SESSION));
    unsafe {
        let mut desc: HmdDesc = std::mem::zeroed();
        desc.kind = 14;
        desc.product_name[..9].copy_from_slice(b"Stand-in\0");
        desc.manufacturer[..7].copy_from_slice(b"rustyAC");
        desc.default_eye_fov = [eye_fov(0), eye_fov(1)];
        desc.max_eye_fov = desc.default_eye_fov;
        let (w, h) = *SIZE.lock().unwrap();
        desc.resolution = Sizei { w: w * 2, h };
        desc.display_refresh_rate = 90.0;
        *out = desc;
    }
    out
}
extern "C" fn get_fov_texture_size(_session: Session, eye: i32, f: *const FovPort, pixels: f32) -> Sizei {
    log(format!("ovr_GetFovTextureSize {eye} {} {}", fov(unsafe { &*f }), bits(pixels)));
    let (w, h) = *SIZE.lock().unwrap();
    Sizei { w, h }
}
extern "C" fn get_render_desc(out: *mut EyeRenderDesc, _session: Session, eye: i32, f: *const FovPort) -> *mut EyeRenderDesc {
    let f = unsafe { *f };
    log(format!("ovr_GetRenderDesc {eye} {}", fov(&f)));
    let (w, h) = *SIZE.lock().unwrap();
    // 32 mm to each side, a little up and ahead
    let x = if eye == 0 { -0.032 } else { 0.032 };
    unsafe { *out = EyeRenderDesc { eye, fov: f, distorted_viewport: rustyac_render::oculus::Recti { x: eye * w, y: 0, w, h }, pixels_per_tan_angle_at_center: [500.0, 500.0], hmd_to_eye_offset: [x, 0.001, 0.0005] } };
    out
}
extern "C" fn get_predicted_display_time(_session: Session, frame: i64) -> f64 {
    log(format!("ovr_GetPredictedDisplayTime {frame}"));
    1000.0 + (frame + 1) as f64 / 90.0
}
extern "C" fn get_tracking_state(out: *mut TrackingState, _session: Session, time: f64, marker: u8) -> *mut TrackingState {
    log(format!("ovr_GetTrackingState {:016x} {marker}", time.to_bits()));
    let n = FRAME.fetch_add(1, Ordering::Relaxed) as f32;
    // the head turning left and leaning: a yaw, a pitch and a small move each frame
    let (yaw, pitch) = (0.12 + 0.05 * n, -0.06 + 0.02 * n);
    let (cy, sy, cp, sp) = ((yaw * 0.5).cos(), (yaw * 0.5).sin(), (pitch * 0.5).cos(), (pitch * 0.5).sin());
    unsafe {
        let mut state: TrackingState = std::mem::zeroed();
        state.head_pose.orientation = [cy * sp, sy * cp, -sy * sp, cy * cp];
        state.head_pose.position = [0.01 * n - 0.02, 0.03, 0.04 - 0.015 * n];
        *out = state;
    }
    out
}
extern "C" fn get_time_in_seconds() -> f64 {
    log("ovr_GetTimeInSeconds".into());
    1000.0 + FRAME.load(Ordering::Relaxed) as f64 / 90.0 + 0.002
}
extern "C" fn create_texture_swap_chain_dx(_session: Session, device: *mut c_void, desc: *const SwapChainDesc, out: *mut Chain) -> i32 {
    let d = unsafe { *desc };
    log(format!(
        "ovr_CreateTextureSwapChainDX {} {} {} {}x{} {} {} {} {} {}",
        d.kind, d.format, d.array_size, d.width, d.height, d.mip_levels, d.sample_count, d.static_image, d.misc_flags, d.bind_flags
    ));
    let Some(device) = (unsafe { ID3D11Device::from_raw_borrowed(&device) }) else { return -1 };
    // OVR_FORMAT_R8G8B8A8_UNORM_SRGB with the typeless flag: R8G8B8A8_TYPELESS, drawn as UNORM
    let texture = D3D11_TEXTURE2D_DESC {
        Width: d.width as u32,
        Height: d.height as u32,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_TYPELESS,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut textures = Vec::new();
    for _ in 0..LENGTH {
        let mut t = None;
        if unsafe { device.CreateTexture2D(&texture, None, Some(&mut t)) }.is_err() {
            return -1;
        }
        textures.extend(t.map(|t| t.into_raw() as usize));
    }
    let mut chains = CHAINS.lock().unwrap();
    chains.push(SwapChain { textures, index: 0, committed: 0 });
    unsafe { *out = (0x100 + chains.len() - 1) as Chain };
    0
}
extern "C" fn get_texture_swap_chain_length(_session: Session, chain: Chain, out: *mut i32) -> i32 {
    log(format!("ovr_GetTextureSwapChainLength {}", chain_id(chain)));
    unsafe { *out = LENGTH };
    0
}
extern "C" fn get_texture_swap_chain_current_index(_session: Session, chain: Chain, out: *mut i32) -> i32 {
    let index = CHAINS.lock().unwrap().get(chain_id(chain)).map_or(0, |c| c.index);
    log(format!("ovr_GetTextureSwapChainCurrentIndex {} -> {index}", chain_id(chain)));
    unsafe { *out = index };
    0
}
extern "C" fn get_texture_swap_chain_buffer_dx(_session: Session, chain: Chain, index: i32, iid: *const GUID, out: *mut *mut c_void) -> i32 {
    let is_texture = unsafe { *iid } == ID3D11Texture2D::IID;
    log(format!("ovr_GetTextureSwapChainBufferDX {} {index} {is_texture}", chain_id(chain)));
    let chains = CHAINS.lock().unwrap();
    let Some(&raw) = chains.get(chain_id(chain)).and_then(|c| c.textures.get(index as usize)) else { return -1 };
    let raw = raw as *mut c_void;
    // (a reference of its own for the caller)
    unsafe { *out = ID3D11Texture2D::from_raw_borrowed(&raw).map_or(std::ptr::null_mut(), |t| t.clone().into_raw()) };
    0
}
extern "C" fn commit_texture_swap_chain(_session: Session, chain: Chain) -> i32 {
    log(format!("ovr_CommitTextureSwapChain {}", chain_id(chain)));
    if let Some(c) = CHAINS.lock().unwrap().get_mut(chain_id(chain)) {
        c.committed = c.index;
        c.index = (c.index + 1) % LENGTH;
    }
    0
}
extern "C" fn submit_frame(_session: Session, frame: i64, scale: *const c_void, layers: *const *const LayerEyeFov, count: u32) -> i32 {
    let l = unsafe { &**layers };
    let pose = |i: usize| [l.render_pose[i].orientation.to_vec(), l.render_pose[i].position.to_vec()].concat().into_iter().map(bits).collect::<Vec<_>>().join(",");
    let view = |i: usize| format!("{},{},{},{}", l.viewport[i].x, l.viewport[i].y, l.viewport[i].w, l.viewport[i].h);
    log(format!(
        "ovr_SubmitFrame {frame} {} {count} {} {} chains {} {} viewports {} {} fov {} {} poses {} {} time {:016x}",
        scale.is_null(),
        l.kind,
        l.flags,
        chain_id(l.color_texture[0]),
        chain_id(l.color_texture[1]),
        view(0),
        view(1),
        fov(&l.fov[0]),
        fov(&l.fov[1]),
        pose(0),
        pose(1),
        l.sensor_sample_time.to_bits()
    ));
    let chains = CHAINS.lock().unwrap();
    for eye in 0..2 {
        let Some(c) = chains.get(chain_id(l.color_texture[eye])) else { continue };
        match unsafe { crate::openvr_fake::read_texture(c.textures[c.committed as usize] as *mut c_void) } {
            Ok((w, h, pixels)) => EYES.lock().unwrap().push((eye as i32, w, h, pixels)),
            Err(e) => log(format!("SubmitFrame: the texture could not be read: {e}")),
        }
    }
    0
}
extern "C" fn get_session_status(_session: Session, out: *mut SessionStatus) -> i32 {
    log("ovr_GetSessionStatus".into());
    // the runtime asks for a recentring once (as after its own "reset view")
    let recenter = FRAME.load(Ordering::Relaxed) == 5;
    unsafe { *out = [1, 1, 1, 0, 0, recenter as u8] };
    0
}
extern "C" fn recenter_tracking_origin(_session: Session) -> i32 {
    log("ovr_RecenterTrackingOrigin".into());
    0
}
extern "system" fn get_async_key_state(key: i32) -> i16 {
    log(format!("GetAsyncKeyState {key:#x}"));
    // Ctrl+Space held in the third frame: the recentring keys of `StereoCameraForward::render`
    if FRAME.load(Ordering::Relaxed) == 3 && (key == 0x11 || key == 0x20) { i16::MIN } else { 0 }
}

/// The game's LibOVR shim (`OVR_LoadSharedLibrary` 0x1401dc710): its library handle and the
/// pointers it fills from the runtime, at their places in acs.exe.
const SHIM_LIBRARY: usize = 0x1_4155_9e90;

/// The stand-ins at the shim's pointers, and the library marked loaded (so the shim neither
/// looks for `LibOVRRT64_1.dll` nor checks its signature).
pub fn shim() -> Vec<(usize, usize)> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> usize;
    }
    vec![
        // (this program: it has no ovr_ReportClientInfo, as the stand-in has none)
        (SHIM_LIBRARY, unsafe { GetModuleHandleW(std::ptr::null()) }),
        (0x1_4155_9ea0, initialize as *const () as usize),
        (0x1_4155_9ea8, shutdown as *const () as usize),
        (0x1_4155_9ec0, get_hmd_desc as *const () as usize),
        (0x1_4155_9ce0, create as *const () as usize),
        (0x1_4155_9cf0, get_session_status as *const () as usize),
        (0x1_4155_9d08, recenter_tracking_origin as *const () as usize),
        (0x1_4155_9d18, get_tracking_state as *const () as usize),
        (0x1_4155_9d40, get_fov_texture_size as *const () as usize),
        (0x1_4155_9d48, submit_frame as *const () as usize),
        (0x1_4155_9d50, get_render_desc as *const () as usize),
        (0x1_4155_9d58, get_predicted_display_time as *const () as usize),
        (0x1_4155_9d60, get_time_in_seconds as *const () as usize),
        (0x1_4155_9dc0, create_texture_swap_chain_dx as *const () as usize),
        (0x1_4155_9dd0, get_texture_swap_chain_buffer_dx as *const () as usize),
        (0x1_4155_9e30, get_texture_swap_chain_length as *const () as usize),
        (0x1_4155_9e38, get_texture_swap_chain_current_index as *const () as usize),
        (0x1_4155_9e48, commit_texture_swap_chain as *const () as usize),
    ]
}

/// The key stand-in as the game's loader binds it.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize)]
}

/// The stand-ins as the port takes them.
pub fn api() -> OvrApi {
    OvrApi {
        initialize,
        shutdown,
        create,
        get_hmd_desc,
        get_fov_texture_size,
        get_render_desc,
        get_predicted_display_time,
        get_tracking_state,
        get_time_in_seconds,
        create_texture_swap_chain_dx,
        get_texture_swap_chain_length,
        get_texture_swap_chain_current_index,
        get_texture_swap_chain_buffer_dx,
        commit_texture_swap_chain,
        submit_frame,
        get_session_status,
        recenter_tracking_origin,
        report_client_info: None,
        get_async_key_state,
    }
}
