// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47b: a stand-in OpenVR runtime. The same functions answer the game's
//! `StereoCameraVive` (bound into its delay-load table of `openvr_api.dll`) and the port's
//! (handed over as `rustyac_render::vr::OpenVrApi`): a scripted head, canted eyes, an
//! asymmetric projection. Every call is written down with its arguments (numbers as bits), and
//! what is handed to `Submit` is read back as each eye's picture.

use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use rustyac_render::vr::{Interface, OpenVrApi, TrackedDevicePose};

pub static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// An eye handed to `Submit`: eye, width, height, its bytes.
pub type Eye = (i32, u32, u32, Vec<u8>);
/// The eyes handed to `Submit` since the last [`take_eyes`].
static EYES: Mutex<Vec<Eye>> = Mutex::new(Vec::new());
/// The size the stand-in asks for (`GetRecommendedRenderTargetSize`).
static SIZE: Mutex<(u32, u32)> = Mutex::new((320, 360));
static FRAME: AtomicU32 = AtomicU32::new(0);
static IPD_OFFSET: Mutex<f32> = Mutex::new(0.0);
const TOKEN: u32 = 0x2a;

fn log(line: String) {
    CALLS.lock().unwrap().push(line);
}

fn bits(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() { "null".into() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

pub fn set_size(width: u32, height: u32) {
    *SIZE.lock().unwrap() = (width, height);
}

pub fn take_calls() -> Vec<String> {
    std::mem::take(&mut CALLS.lock().unwrap())
}

pub fn take_eyes() -> Vec<Eye> {
    std::mem::take(&mut EYES.lock().unwrap())
}

extern "C" fn init_internal(error: *mut i32, kind: i32) -> u32 {
    log(format!("VR_InitInternal {kind}"));
    unsafe { *error = 0 };
    TOKEN
}
extern "C" fn is_interface_version_valid(name: *const c_char) -> bool {
    log(format!("VR_IsInterfaceVersionValid {}", unsafe { text(name) }));
    true
}
extern "C" fn get_generic_interface(name: *const c_char, error: *mut i32) -> Interface {
    let name = unsafe { text(name) };
    log(format!("VR_GetGenericInterface {name}"));
    if !error.is_null() {
        unsafe { *error = 0 };
    }
    match name.as_str() {
        "IVRSystem_015" => system(),
        "IVRCompositor_020" => compositor(),
        "IVRSettings_002" => settings(),
        _ => std::ptr::null_mut(),
    }
}
extern "C" fn get_init_token() -> u32 {
    log("VR_GetInitToken".into());
    TOKEN
}
extern "C" fn shutdown_internal() {
    log("VR_ShutdownInternal".into());
}
extern "C" fn error_description(error: i32) -> *const c_char {
    log(format!("VR_GetVRInitErrorAsEnglishDescription {error}"));
    c"stand-in error".as_ptr()
}
extern "system" fn get_async_key_state(key: i32) -> i16 {
    log(format!("GetAsyncKeyState {key:#x}"));
    // Ctrl+Space held in the third frame: the recentring keys of `StereoCameraVive::render`
    if FRAME.load(Ordering::Relaxed) == 3 && (key == 0x11 || key == 0x20) { i16::MIN } else { 0 }
}

// IVRSystem_015
extern "C" fn get_recommended_render_target_size(_this: Interface, width: *mut u32, height: *mut u32) {
    log("IVRSystem::GetRecommendedRenderTargetSize".into());
    let (w, h) = *SIZE.lock().unwrap();
    unsafe { (*width, *height) = (w, h) };
}
extern "C" fn get_projection_matrix(_this: Interface, out: *mut [[f32; 4]; 4], eye: i32, near: f32, far: f32) -> *mut [[f32; 4]; 4] {
    log(format!("IVRSystem::GetProjectionMatrix {eye} {} {}", bits(near), bits(far)));
    // (tangents of a headset's lens, the inner side narrower; OpenVR's `ComposeProjection`)
    let (mut left, mut right, top, bottom) = (-1.39f32, 1.24f32, -1.47f32, 1.45f32);
    if eye == 1 {
        (left, right) = (-right, -left);
    }
    let (idx, idy, idz) = (1.0 / (right - left), 1.0 / (bottom - top), 1.0 / (far - near));
    let m = [
        [2.0 * idx, 0.0, (right + left) * idx, 0.0],
        [0.0, 2.0 * idy, (bottom + top) * idy, 0.0],
        [0.0, 0.0, -far * idz, -far * near * idz],
        [0.0, 0.0, -1.0, 0.0],
    ];
    unsafe { *out = m };
    out
}
extern "C" fn get_eye_to_head_transform(_this: Interface, out: *mut [[f32; 4]; 3], eye: i32) -> *mut [[f32; 4]; 3] {
    log(format!("IVRSystem::GetEyeToHeadTransform {eye}"));
    // eyes turned out by 2 degrees, 32 mm to each side, 15 mm ahead of the head's centre
    let (c, s) = (0.999_390_8f32, if eye == 0 { 0.034_899_5f32 } else { -0.034_899_5 });
    let x = if eye == 0 { -0.032f32 } else { 0.032 };
    unsafe { *out = [[c, 0.0, s, x], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.015]] };
    out
}
extern "C" fn reset_seated_zero_pose(_this: Interface) {
    log("IVRSystem::ResetSeatedZeroPose".into());
}

/// Task 47c: `GetTrackedDeviceIndexForControllerRole`: the left hand device 3, no right hand.
extern "C" fn controller_role_index(_this: Interface, role: i32) -> u32 {
    log(format!("IVRSystem::GetTrackedDeviceIndexForControllerRole {role}"));
    if role == 1 { 3 } else { u32::MAX }
}
/// Task 47c: `GetTrackedDeviceClass`: the head 1, controllers 2 (3, 4), generic trackers 3 (5, 6).
extern "C" fn device_class(_this: Interface, index: u32) -> i32 {
    log(format!("IVRSystem::GetTrackedDeviceClass {index}"));
    match index {
        0 => 1,
        3 | 4 => 2,
        5 | 6 => 3,
        _ => 0,
    }
}

// IVRCompositor_020
extern "C" fn set_tracking_space(_this: Interface, space: i32) {
    log(format!("IVRCompositor::SetTrackingSpace {space}"));
}
extern "C" fn wait_get_poses(_this: Interface, poses: *mut TrackedDevicePose, count: u32, game: *mut TrackedDevicePose, game_count: u32) -> i32 {
    log(format!("IVRCompositor::WaitGetPoses {count} {} {game_count}", game.is_null()));
    let n = FRAME.fetch_add(1, Ordering::Relaxed) as f32;
    // the head turning left and leaning: a yaw, a pitch and a small move each frame
    let (yaw, pitch) = (0.12 + 0.05 * n, -0.06 + 0.02 * n);
    let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    let m = [[cy, sy * sp, sy * cp, 0.01 * n - 0.02], [0.0, cp, -sp, 0.03], [-sy, cy * sp, cy * cp, 0.04 - 0.015 * n]];
    for i in 0..count as usize {
        // (zeroed first: the padding too, which a missing hand's pose reads in the game)
        let p = unsafe { poses.add(i) };
        unsafe { std::ptr::write_bytes(p, 0, 1) };
        let pose = |m: [[f32; 4]; 3]| unsafe {
            (*p).device_to_absolute_tracking = m;
            (*p).velocity = [0.1, -0.2, 0.3];
            (*p).angular_velocity = [0.01, 0.02, -0.03];
            (*p).tracking_result = 200;
            (*p).pose_is_valid = true;
            (*p).device_is_connected = true;
        };
        match i {
            0 => pose(m),
            // Task 47c: the left hand's controller (3) and two generic trackers (5, 6), each turned
            // about another axis; no right hand (role 2 answers k_unTrackedDeviceIndexInvalid)
            3 | 5 | 6 => {
                let a = 0.2 * i as f32 + 0.03 * n;
                let (c, s) = (a.cos(), a.sin());
                pose(match i {
                    3 => [[c, -s, 0.0, 0.2], [s, c, 0.0, -0.1], [0.0, 0.0, 1.0, 0.3]],
                    5 => [[c, 0.0, s, -0.2], [0.0, 1.0, 0.0, 0.5], [-s, 0.0, c, 0.1]],
                    _ => [[1.0, 0.0, 0.0, 0.0], [0.0, c, -s, 0.9], [0.0, s, c, -0.4]],
                })
            }
            _ => {}
        }
    }
    0
}
#[repr(C)]
struct Texture {
    handle: *mut std::ffi::c_void,
    kind: i32,
    color_space: i32,
}
extern "C" fn submit(_this: Interface, eye: i32, texture: *const Texture, bounds: *const u8, flags: i32) -> i32 {
    let t = unsafe { &*texture };
    log(format!("IVRCompositor::Submit {eye} {} {} {} {flags}", t.kind, t.color_space, bounds.is_null()));
    match unsafe { read_texture(t.handle) } {
        Ok((w, h, pixels)) => EYES.lock().unwrap().push((eye, w, h, pixels)),
        Err(e) => log(format!("Submit: the texture could not be read: {e}")),
    }
    0
}

/// A 2D texture's pixels through a staging copy on its own device.
pub(crate) unsafe fn read_texture(handle: *mut std::ffi::c_void) -> Result<(u32, u32, Vec<u8>), String> {
    use windows::core::Interface as _;
    use windows::Win32::Graphics::Direct3D11::*;
    let texture = ID3D11Texture2D::from_raw_borrowed(&handle).ok_or("no texture")?;
    let device = texture.GetDevice().map_err(|e| e.to_string())?;
    let context = device.GetImmediateContext().map_err(|e| e.to_string())?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut desc);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    let mut staging = None;
    device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(|e| e.to_string())?;
    let staging = staging.ok_or("no staging texture")?;
    context.CopyResource(&staging, texture);
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| e.to_string())?;
    let mut pixels = Vec::with_capacity((desc.Width * desc.Height * 4) as usize);
    for row in 0..desc.Height as usize {
        let p = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
        pixels.extend_from_slice(std::slice::from_raw_parts(p, desc.Width as usize * 4));
    }
    context.Unmap(&staging, 0);
    Ok((desc.Width, desc.Height, pixels))
}

// IVRSettings_002
extern "C" fn sync(_this: Interface, force: bool, error: *mut i32) -> bool {
    log(format!("IVRSettings::Sync {force} {}", error.is_null()));
    true
}
extern "C" fn set_float(_this: Interface, section: *const c_char, key: *const c_char, value: f32, error: *mut i32) {
    let (section, key) = unsafe { (text(section), text(key)) };
    log(format!("IVRSettings::SetFloat {section} {key} {} {}", bits(value), error.is_null()));
    if key == "ipdOffset" {
        *IPD_OFFSET.lock().unwrap() = value;
    }
}
extern "C" fn get_float(_this: Interface, section: *const c_char, key: *const c_char, error: *mut i32) -> f32 {
    let (section, key) = unsafe { (text(section), text(key)) };
    log(format!("IVRSettings::GetFloat {section} {key} {}", error.is_null()));
    match key.as_str() {
        "ipd" => 0.0635,
        "ipdOffset" => *IPD_OFFSET.lock().unwrap(),
        _ => 0.0,
    }
}

extern "C" fn unknown_slot(_this: Interface) -> usize {
    log("an interface slot the stand-in does not know".into());
    0
}

/// One interface: a vtable of `unknown_slot` with the given slots filled in.
fn interface(slots: &[(usize, usize)]) -> usize {
    let mut vtable = vec![unknown_slot as *const () as usize; 64];
    for &(i, f) in slots {
        vtable[i] = f;
    }
    let vtable = Box::leak(vtable.into_boxed_slice());
    Box::leak(Box::new(vtable.as_ptr() as usize)) as *mut usize as usize
}

static SYSTEM: OnceLock<usize> = OnceLock::new();
static COMPOSITOR: OnceLock<usize> = OnceLock::new();
static SETTINGS: OnceLock<usize> = OnceLock::new();

fn system() -> Interface {
    *SYSTEM.get_or_init(|| {
        interface(&[
            (0, get_recommended_render_target_size as *const () as usize),
            (1, get_projection_matrix as *const () as usize),
            (4, get_eye_to_head_transform as *const () as usize),
            (11, reset_seated_zero_pose as *const () as usize),
            (17, controller_role_index as *const () as usize),
            (19, device_class as *const () as usize),
        ])
    }) as Interface
}
fn compositor() -> Interface {
    *COMPOSITOR.get_or_init(|| interface(&[(0, set_tracking_space as *const () as usize), (2, wait_get_poses as *const () as usize), (5, submit as *const () as usize)])) as Interface
}
fn settings() -> Interface {
    *SETTINGS.get_or_init(|| interface(&[(1, sync as *const () as usize), (4, set_float as *const () as usize), (8, get_float as *const () as usize)])) as Interface
}

/// The stand-ins as the game's loader binds them.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("openvr_api.dll", "VR_InitInternal", init_internal as *const () as usize),
        ("openvr_api.dll", "VR_IsInterfaceVersionValid", is_interface_version_valid as *const () as usize),
        ("openvr_api.dll", "VR_GetGenericInterface", get_generic_interface as *const () as usize),
        ("openvr_api.dll", "VR_GetInitToken", get_init_token as *const () as usize),
        ("openvr_api.dll", "VR_ShutdownInternal", shutdown_internal as *const () as usize),
        ("openvr_api.dll", "VR_GetVRInitErrorAsEnglishDescription", error_description as *const () as usize),
        ("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize),
    ]
}

/// The stand-ins as the port takes them.
pub fn api() -> OpenVrApi {
    OpenVrApi { init_internal, is_interface_version_valid, get_generic_interface, get_init_token, shutdown_internal, error_description, get_async_key_state }
}

/// The window's picture with the two eyes side by side under it (`width` x `height` window,
/// each eye `width / 2` x `height`), so one picture holds all three.
pub fn stack(window: Vec<u8>, width: u32, height: u32, eyes: &[Eye]) -> Vec<u8> {
    let mut out = window;
    let half = (width / 2) as usize;
    for row in 0..height as usize {
        for eye in 0..2 {
            match eyes.iter().find(|e| e.0 == eye && e.1 as usize == half && e.2 == height) {
                Some((_, w, _, p)) => out.extend_from_slice(&p[row * *w as usize * 4..][..half * 4]),
                None => out.extend(std::iter::repeat_n(0u8, half * 4)),
            }
        }
    }
    out
}
