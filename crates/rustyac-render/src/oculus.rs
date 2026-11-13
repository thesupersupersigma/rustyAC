// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `StereoCameraForward`, the camera of video.ini `[CAMERA] MODE=OCULUS`: the scene drawn once
//! per eye into the Oculus runtime's swap chains, handed to it with `ovr_SubmitFrame` after the
//! window's swap, the window showing the left eye's texture. `StereoCameraForward::
//! StereoCameraForward` 0x140211370, `initOculus` 0x140212fd0, `render` 0x1402135c0,
//! `renderNoPP` 0x1402139a0, `oculusRenderPass` 0x140213230, `finishRendering` 0x140212d10.
//!
//! The runtime is reached as the game's LibOVR shim reaches it: the runtime's `ovr_*` functions,
//! with the shim's own parts (`ovr_Initialize`'s defaults, `ovr_GetEyePoses`,
//! `ovr_CalcEyePoses`, `ovrMatrix4f_Projection`) ported here. Whoever makes the camera hands
//! over the functions ([`OvrApi`]): from the Oculus runtime's `LibOVRRT64_1.dll` (only when it is
//! signed by Oculus, as the shim checks), or a stand-in runtime in a proof.
//!
//! Post-processing (Task Y3e): one YEBIS for both eyes, `StereoCameraForward::renderYebis` 0x140213b10.

use std::ffi::c_void;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use windows::core::{Interface as _, GUID};
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

use crate::camera::Camera;
use crate::forward::CameraForward;
use crate::graphics::{Graphics, RenderTarget, DEPTH_OFF};
use crate::kgl::{KglRenderTarget, FORMAT_R32F, FORMAT_RGBA16F, FORMAT_RGBA8};
use crate::scene::{NodeId, Scene};
use crate::vr::{normal_rendering, place, YebisFrom};

/// `ovrSession`
pub type Session = *mut c_void;
/// `ovrTextureSwapChain`
pub type Chain = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FovPort {
    pub up: f32,
    pub down: f32,
    pub left: f32,
    pub right: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sizei {
    pub w: i32,
    pub h: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Recti {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// `ovrPosef`: a quaternion (x, y, z, w) and a place.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Posef {
    pub orientation: [f32; 4],
    pub position: [f32; 3],
}

/// `ovrHmdDesc` (0x108 bytes)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HmdDesc {
    pub kind: i32,
    pub pad0: u32,
    pub product_name: [u8; 64],
    pub manufacturer: [u8; 64],
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: [u8; 24],
    pub firmware: [u16; 2],
    pub caps: [u32; 4],
    pub default_eye_fov: [FovPort; 2],
    pub max_eye_fov: [FovPort; 2],
    pub resolution: Sizei,
    pub display_refresh_rate: f32,
    pub pad1: u32,
}

/// `ovrEyeRenderDesc` (0x38 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EyeRenderDesc {
    pub eye: i32,
    pub fov: FovPort,
    pub distorted_viewport: Recti,
    pub pixels_per_tan_angle_at_center: [f32; 2],
    pub hmd_to_eye_offset: [f32; 3],
}

/// `ovrTrackingState` (0x138 bytes): only the head's pose is read.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct TrackingState {
    pub head_pose: Posef,
    pub rest: [u8; 0x138 - 0x1c],
}

/// `ovrTextureSwapChainDesc` (0x28 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SwapChainDesc {
    pub kind: i32,
    pub format: i32,
    pub array_size: i32,
    pub width: i32,
    pub height: i32,
    pub mip_levels: i32,
    pub sample_count: i32,
    pub static_image: u32,
    pub misc_flags: u32,
    pub bind_flags: u32,
}

/// `ovrLayerEyeFov` (0x98 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LayerEyeFov {
    pub kind: i32,
    pub flags: u32,
    pub color_texture: [Chain; 2],
    pub viewport: [Recti; 2],
    pub fov: [FovPort; 2],
    pub render_pose: [Posef; 2],
    pub sensor_sample_time: f64,
}

/// `ovrInitParams` (0x20 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InitParams {
    pub flags: u32,
    pub requested_minor_version: u32,
    pub log_callback: usize,
    pub user_data: usize,
    pub connection_timeout_ms: u32,
}

/// `ovrSessionStatus`: IsVisible, HmdPresent, HmdMounted, DisplayLost, ShouldQuit, ShouldRecenter.
pub type SessionStatus = [u8; 6];

/// The runtime's functions the game's shim calls, and `GetAsyncKeyState` (the recentring keys).
#[derive(Clone, Copy)]
pub struct OvrApi {
    pub initialize: unsafe extern "C" fn(*const InitParams) -> i32,
    pub shutdown: unsafe extern "C" fn(),
    pub create: unsafe extern "C" fn(*mut Session, *mut [u8; 8]) -> i32,
    pub get_hmd_desc: unsafe extern "C" fn(*mut HmdDesc, Session) -> *mut HmdDesc,
    pub get_fov_texture_size: unsafe extern "C" fn(Session, i32, *const FovPort, f32) -> Sizei,
    pub get_render_desc: unsafe extern "C" fn(*mut EyeRenderDesc, Session, i32, *const FovPort) -> *mut EyeRenderDesc,
    pub get_predicted_display_time: unsafe extern "C" fn(Session, i64) -> f64,
    pub get_tracking_state: unsafe extern "C" fn(*mut TrackingState, Session, f64, u8) -> *mut TrackingState,
    pub get_time_in_seconds: unsafe extern "C" fn() -> f64,
    pub create_texture_swap_chain_dx: unsafe extern "C" fn(Session, *mut c_void, *const SwapChainDesc, *mut Chain) -> i32,
    pub get_texture_swap_chain_length: unsafe extern "C" fn(Session, Chain, *mut i32) -> i32,
    pub get_texture_swap_chain_current_index: unsafe extern "C" fn(Session, Chain, *mut i32) -> i32,
    pub get_texture_swap_chain_buffer_dx: unsafe extern "C" fn(Session, Chain, i32, *const GUID, *mut *mut c_void) -> i32,
    pub commit_texture_swap_chain: unsafe extern "C" fn(Session, Chain) -> i32,
    pub submit_frame: unsafe extern "C" fn(Session, i64, *const c_void, *const *const LayerEyeFov, u32) -> i32,
    pub get_session_status: unsafe extern "C" fn(Session, *mut SessionStatus) -> i32,
    pub recenter_tracking_origin: unsafe extern "C" fn(Session) -> i32,
    pub report_client_info: Option<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32) -> i32>,
    /// `ovr_GetAudioDeviceOutWaveId` (none: the shim answers `ovrError_MissingDLL`)
    pub get_audio_device_out_wave_id: Option<unsafe extern "C" fn(*mut u32) -> i32>,
    /// `ovr_GetAudioDeviceOutGuidStr`: 128 wide characters
    pub get_audio_device_out_guid_str: Option<unsafe extern "C" fn(*mut u16) -> i32>,
    pub get_async_key_state: unsafe extern "system" fn(i32) -> i16,
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn SearchPathW(path: *const u16, file: *const u16, ext: *const u16, size: u32, buffer: *mut u16, part: *mut *mut u16) -> u32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: *mut c_void) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
}
#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(Some(0)).collect()
}

/// The Oculus runtime's `LibOVRRT64_1.dll` (in `LIBOVR_DLL_DIR`, else where Windows finds it),
/// loaded only when it is signed by "Oculus VR, LLC" through DigiCert, as the shim's
/// `OVR_LoadSharedLibrary` 0x1401dc710 checks it (`OVR_Win32_SignCheck` 0x1401dd320).
// (each transmute's target is the OvrApi field it fills)
#[allow(clippy::missing_transmute_annotations)]
pub fn load() -> Result<OvrApi, String> {
    const NAME: &str = "LibOVRRT64_1.dll";
    let path = match std::env::var_os("LIBOVR_DLL_DIR") {
        Some(dir) => std::path::Path::new(&dir).join(NAME),
        None => {
            let mut buffer = [0u16; 520];
            // SAFETY: a terminated name and a buffer of the size given
            let n = unsafe { SearchPathW(std::ptr::null(), wide(NAME.as_ref()).as_ptr(), std::ptr::null(), buffer.len() as u32, buffer.as_mut_ptr(), std::ptr::null_mut()) } as usize;
            if n == 0 || n >= buffer.len() {
                return Err(format!("no {NAME}: the Oculus runtime is not installed"));
            }
            std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..n]))
        }
    };
    let path = wide(path.as_os_str());
    // SAFETY: a terminated path; the file stays open (so it cannot be swapped) until it is loaded
    unsafe {
        let file = sign_check(&path).ok_or_else(|| format!("{NAME} is not signed by Oculus VR, LLC"))?;
        let module = LoadLibraryW(path.as_ptr());
        CloseHandle(file);
        if module.is_null() {
            return Err(format!("{NAME} does not load"));
        }
        let symbol = |name: &str| -> Result<usize, String> {
            let c = format!("{name}\0");
            let p = GetProcAddress(module, c.as_ptr());
            if p.is_null() {
                Err(format!("{NAME} has no {name}"))
            } else {
                Ok(p as usize)
            }
        };
        macro_rules! f {
            ($name:literal) => {
                std::mem::transmute::<usize, _>(symbol($name)?)
            };
        }
        Ok(OvrApi {
            initialize: f!("ovr_Initialize"),
            shutdown: f!("ovr_Shutdown"),
            create: f!("ovr_Create"),
            get_hmd_desc: f!("ovr_GetHmdDesc"),
            get_fov_texture_size: f!("ovr_GetFovTextureSize"),
            get_render_desc: f!("ovr_GetRenderDesc"),
            get_predicted_display_time: f!("ovr_GetPredictedDisplayTime"),
            get_tracking_state: f!("ovr_GetTrackingState"),
            get_time_in_seconds: f!("ovr_GetTimeInSeconds"),
            create_texture_swap_chain_dx: f!("ovr_CreateTextureSwapChainDX"),
            get_texture_swap_chain_length: f!("ovr_GetTextureSwapChainLength"),
            get_texture_swap_chain_current_index: f!("ovr_GetTextureSwapChainCurrentIndex"),
            get_texture_swap_chain_buffer_dx: f!("ovr_GetTextureSwapChainBufferDX"),
            commit_texture_swap_chain: f!("ovr_CommitTextureSwapChain"),
            submit_frame: f!("ovr_SubmitFrame"),
            get_session_status: f!("ovr_GetSessionStatus"),
            recenter_tracking_origin: f!("ovr_RecenterTrackingOrigin"),
            report_client_info: symbol("ovr_ReportClientInfo").ok().map(|p| std::mem::transmute::<usize, _>(p)),
            get_audio_device_out_wave_id: symbol("ovr_GetAudioDeviceOutWaveId").ok().map(|p| std::mem::transmute::<usize, _>(p)),
            get_audio_device_out_guid_str: symbol("ovr_GetAudioDeviceOutGuidStr").ok().map(|p| std::mem::transmute::<usize, _>(p)),
            get_async_key_state: GetAsyncKeyState,
        })
    }
}

/// `AllowedCertificateChains`: (issuer, subject) of the signer, its CA and the root.
const ALLOWED_CHAIN: [(&str, &str); 3] = [
    ("DigiCert SHA2 Assured ID Code Signing CA", "Oculus VR, LLC"),
    ("DigiCert Assured ID Root CA", "DigiCert SHA2 Assured ID Code Signing CA"),
    ("DigiCert Assured ID Root CA", "DigiCert Assured ID Root CA"),
];

/// `OVR_Win32_SignCheck` 0x1401dd320: the file opened for reading, its Authenticode signature
/// trusted by Windows (no revocation check), one signer whose chain is [`ALLOWED_CHAIN`]
/// (`ValidateCertificateContents` 0x1401dd5b0). The open file, or `None`.
unsafe fn sign_check(path: &[u16]) -> Option<*mut c_void> {
    type VerifyTrust = unsafe extern "system" fn(*mut c_void, *const GUID, *mut c_void) -> i32;
    type ProvData = unsafe extern "system" fn(*mut c_void) -> *mut u8;
    type Signer = unsafe extern "system" fn(*mut u8, u32, i32, u32) -> *mut u8;
    type NameString = unsafe extern "system" fn(*const c_void, u32, u32, *const c_void, *mut u16, u32) -> u32;
    let wintrust = LoadLibraryW(wide("wintrust".as_ref()).as_ptr());
    let crypt32 = LoadLibraryW(wide("crypt32".as_ref()).as_ptr());
    if wintrust.is_null() || crypt32.is_null() {
        return None;
    }
    let get = |module, name: &str| {
        let p = GetProcAddress(module, format!("{name}\0").as_ptr());
        (!p.is_null()).then_some(p as usize)
    };
    let name_string: NameString = std::mem::transmute(get(crypt32, "CertGetNameStringW")?);
    let verify: VerifyTrust = std::mem::transmute(get(wintrust, "WinVerifyTrust")?);
    let prov_data: ProvData = std::mem::transmute(get(wintrust, "WTHelperProvDataFromStateData")?);
    let signer: Signer = std::mem::transmute(get(wintrust, "WTHelperGetProvSignerFromChain")?);
    let file = CreateFileW(path.as_ptr(), 0x8000_0000, 1, std::ptr::null_mut(), 3, 0, std::ptr::null_mut());
    if file as isize == -1 {
        return None;
    }
    // WINTRUST_ACTION_GENERIC_VERIFY_V2
    let action = GUID::from_u128(0x00aac56b_cd44_11d0_8cc2_00c04fc295ee);
    // WINTRUST_FILE_INFO (0x20) and WINTRUST_DATA (0x58)
    let file_info: [usize; 4] = [0x20, path.as_ptr() as usize, file as usize, 0];
    let mut data = [0u8; 0x58];
    let put = |data: &mut [u8; 0x58], at: usize, v: u64| data[at..at + 8].copy_from_slice(&v.to_le_bytes());
    data[0..4].copy_from_slice(&0x58u32.to_le_bytes());
    put(&mut data, 0x18, 2); // dwUIChoice WTD_UI_NONE, no revocation check
    data[0x20..0x24].copy_from_slice(&1u32.to_le_bytes()); // WTD_CHOICE_FILE
    put(&mut data, 0x28, file_info.as_ptr() as u64);
    data[0x30..0x34].copy_from_slice(&1u32.to_le_bytes()); // WTD_STATEACTION_VERIFY
    put(&mut data, 0x48, 0x10); // WTD_REVOCATION_CHECK_NONE
    let window = -1isize as *mut c_void;
    let mut ok = false;
    if verify(window, &action, data.as_mut_ptr().cast()) == 0 {
        let state = u64::from_le_bytes(data[0x38..0x40].try_into().unwrap_or_default());
        if state.wrapping_sub(1) <= u64::MAX - 3 {
            let prov = prov_data(state as *mut c_void);
            if !prov.is_null() && *prov.add(0x78).cast::<u32>() == 1 {
                let sgnr = signer(prov, 0, 0, 0);
                ok = chain_allowed(sgnr, name_string);
            }
        }
    }
    data[0x30..0x34].copy_from_slice(&2u32.to_le_bytes()); // WTD_STATEACTION_CLOSE
    verify(window, &action, data.as_mut_ptr().cast());
    if ok {
        Some(file)
    } else {
        CloseHandle(file);
        None
    }
}

/// `ValidateCertificateContents` 0x1401dd5b0: three certificates, none a test one nor
/// self-signed without being a trusted root, their names those of [`ALLOWED_CHAIN`].
unsafe fn chain_allowed(sgnr: *mut u8, name_string: unsafe extern "system" fn(*const c_void, u32, u32, *const c_void, *mut u16, u32) -> u32) -> bool {
    if sgnr.is_null() || (*sgnr.add(0x10).cast::<*mut u8>()).is_null() || *sgnr.add(0xc).cast::<u32>() != 3 {
        return false;
    }
    let certs = *sgnr.add(0x10).cast::<*mut u8>();
    for (i, (issuer, subject)) in ALLOWED_CHAIN.iter().enumerate() {
        let cert = certs.add(i * 0x58);
        let (trusted_root, self_signed, test) = (*cert.add(0x14).cast::<i32>(), *cert.add(0x18).cast::<i32>(), *cert.add(0x1c).cast::<i32>());
        if (self_signed != 0 && trusted_root == 0) || test != 0 {
            return false;
        }
        let context = *cert.add(8).cast::<*const c_void>();
        let name = |flags: u32, kind: *const c_void| {
            let mut buffer = [0u16; 400];
            name_string(context, 3, flags, kind, buffer.as_mut_ptr(), 400);
            String::from_utf16_lossy(&buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(400)])
        };
        if name(0, c"2.5.4.3".as_ptr().cast()) != *subject || name(1, std::ptr::null()) != *issuer {
            return false;
        }
    }
    true
}

/// The game's globals of `StereoCameraForward` (`initialized`, `session`, `luid`) and the
/// runtime's functions.
/// Task 47c: `OculusManager::OculusManager` 0x1400a01c0, its sound part: with oculus.ini
/// `[SETTINGS] AUTOSELECT_RIFT_AUDIO_DEVICE` the Rift's waveOut number, which the game hands to
/// `AudioEngine::setDriver` as FMOD's driver number.
pub fn rift_audio_driver(api: &OvrApi, documents: &std::path::Path) -> Option<u32> {
    let ini = IniReader::load(&documents.join("cfg/oculus.ini")).ok()?;
    if !ini.ready || ini.get_int("SETTINGS", "AUTOSELECT_RIFT_AUDIO_DEVICE").unwrap_or(0) == 0 {
        return None;
    }
    println!("Autoselecting Oculus Rift Audio Device");
    let mut id = 0u32;
    // (the shim's answer without the runtime's function: ovrError_MissingDLL)
    let r = api.get_audio_device_out_wave_id.map_or(-1004, |f| unsafe { f(&mut id) });
    if r != 0 {
        println!("CANNOT FIND OCULUS AUDIO DEVICE");
        return None;
    }
    println!("Selecting audio device: {id}");
    let mut name = [0u16; 128];
    if let Some(f) = api.get_audio_device_out_guid_str {
        unsafe { f(name.as_mut_ptr()) };
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    println!("Device name: {}", String::from_utf16_lossy(&name[..end]));
    Some(id)
}

pub struct Ovr {
    pub api: OvrApi,
    pub initialized: bool,
    pub session: Session,
    pub luid: [u8; 8],
}

impl Ovr {
    pub fn new(api: OvrApi) -> Ovr {
        Ovr { api, initialized: false, session: std::ptr::null_mut(), luid: [0; 8] }
    }

    /// The shim's `ovr_Initialize` 0x1401ddc40 with no parameters: its defaults (flags 4,
    /// minor version 3), then `ovr_ReportClientInfo` when the runtime has it.
    unsafe fn initialize(&self) -> i32 {
        let params = InitParams { flags: 4, requested_minor_version: 3, ..InitParams::default() };
        let result = (self.api.initialize)(&params);
        if let (0, Some(report)) = (result, self.api.report_client_info) {
            report(0xabb33b5, 1, 1, 3, 0, 0);
        }
        result
    }

    /// `StereoCameraForward::initOculus` 0x140212fd0.
    pub fn init(&mut self) -> bool {
        // SAFETY: the runtime's functions, as the shim calls them
        unsafe {
            if self.initialize() < 0 {
                eprintln!("ERROR: ovr_Initialize failed");
                return false;
            }
            if (self.api.create)(&mut self.session, &mut self.luid) < 0 {
                (self.api.shutdown)();
                return false;
            }
        }
        self.initialized = true;
        true
    }

    fn hmd_desc(&self) -> HmdDesc {
        // SAFETY: a plain struct the runtime fills
        unsafe {
            let mut desc: HmdDesc = std::mem::zeroed();
            (self.api.get_hmd_desc)(&mut desc, self.session);
            desc
        }
    }

    fn current_index(&self, chain: Chain) -> i32 {
        let mut index = 0;
        unsafe { (self.api.get_texture_swap_chain_current_index)(self.session, chain, &mut index) };
        index
    }

    /// `ovr_RecenterTrackingOrigin`
    pub fn recenter(&self) {
        unsafe { (self.api.recenter_tracking_origin)(self.session) };
    }
}

/// `ovr_CalcEyePoses` 0x140440b00: each eye's offset turned by the head and added to its place.
pub fn calc_eye_poses(head: &Posef, offsets: &[[f32; 3]; 2]) -> [Posef; 2] {
    let [x, y, z, w] = head.orientation;
    let p = head.position;
    offsets.map(|v| {
        let tx = (y * v[2] - z * v[1]) * 2.0;
        let ty = (z * v[0] - x * v[2]) * 2.0;
        let tz = (x * v[1] - y * v[0]) * 2.0;
        let rx = ((w * tx + v[0]) + y * tz) - z * ty;
        let ry = ((w * ty + v[1]) + z * tx) - x * tz;
        let rz = ((w * tz + v[2]) + x * ty) - y * tx;
        Posef { orientation: head.orientation, position: [p[0] + rx, p[1] + ry, p[2] + rz] }
    })
}

/// `ovrMatrix4f_Projection` 0x140440a40 (`OVR::CreateProjection` 0x140440f30, no flags): the
/// runtime's (column-vector) projection of a field of view.
pub fn projection(fov: &FovPort, znear: f32, zfar: f32) -> [[f32; 4]; 4] {
    let scale_x = 2.0 / (fov.right + fov.left);
    let scale_y = 2.0 / (fov.up + fov.down);
    let offset_x = ((fov.left - fov.right) * scale_x) * 0.5;
    let offset_y = ((fov.up - fov.down) * scale_y) * 0.5;
    let handedness = -1.0f32;
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = scale_x;
    m[0][2] = offset_x * handedness;
    m[1][1] = scale_y;
    m[1][2] = -(offset_y * handedness);
    m[2][2] = -((zfar * handedness) / (znear - zfar));
    m[2][3] = (zfar * znear) / (znear - zfar);
    m[3][2] = handedness;
    m
}

/// `OculusTexture`: one eye's swap chain and a render target view of each of its textures.
pub struct OculusTexture {
    pub chain: Chain,
    rtv: Vec<ID3D11RenderTargetView>,
    textures: Vec<ID3D11Texture2D>,
}

impl OculusTexture {
    /// `OculusTexture::Init` 0x140212310.
    unsafe fn init(ovr: &Ovr, graphics: &Graphics, width: i32, height: i32) -> Option<OculusTexture> {
        // ovrTexture_2D, OVR_FORMAT_R8G8B8A8_UNORM_SRGB, typeless, render target
        let desc = SwapChainDesc { kind: 0, format: 5, array_size: 1, width, height, mip_levels: 1, sample_count: 1, static_image: 0, misc_flags: 1, bind_flags: 1 };
        let mut texture = OculusTexture { chain: std::ptr::null_mut(), rtv: Vec::new(), textures: Vec::new() };
        if (ovr.api.create_texture_swap_chain_dx)(ovr.session, graphics.kgl.device.as_raw(), &desc, &mut texture.chain) < 0 {
            return None;
        }
        let mut length = 0;
        (ovr.api.get_texture_swap_chain_length)(ovr.session, texture.chain, &mut length);
        for i in 0..length {
            let Some(buffer) = buffer(ovr, texture.chain, i) else { continue };
            let view = D3D11_RENDER_TARGET_VIEW_DESC { Format: DXGI_FORMAT(0x1c), ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D, Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } } };
            let mut rtv = None;
            let _ = graphics.kgl.device.CreateRenderTargetView(&buffer, Some(&view), Some(&mut rtv));
            texture.rtv.extend(rtv);
            texture.textures.push(buffer);
        }
        Some(texture)
    }
}

/// `ovr_GetTextureSwapChainBufferDX` as an `ID3D11Texture2D` (its reference handed over).
unsafe fn buffer(ovr: &Ovr, chain: Chain, index: i32) -> Option<ID3D11Texture2D> {
    let mut raw = std::ptr::null_mut();
    (ovr.api.get_texture_swap_chain_buffer_dx)(ovr.session, chain, index, &ID3D11Texture2D::IID, &mut raw);
    (!raw.is_null()).then(|| ID3D11Texture2D::from_raw(raw))
}

/// What `StereoCameraForward` (0xa48 bytes) adds to its `CameraForward`.
pub struct StereoCameraForward {
    pub ovr: Ovr,
    pub render_scene: bool,
    /// oculus.ini `[MIRROR_TEXTURE] ENABLED`: the window shows the left eye
    pub mirror_enabled: bool,
    /// oculus.ini `[SETTINGS] PIXEL_PER_DISPLAY`
    pub pixel_per_display: f32,
    pub default_position: [f32; 3],
    pub proj_matrix: Mat44f,
    pub view_matrix: Mat44f,
    pub frame_index: i64,
    pub eye_render_viewport: [Recti; 2],
    pub eye_render_poses: [Posef; 2],
    pub eye_render_texture: [OculusTexture; 2],
    eye_depth_buffer: [KglRenderTarget; 2],
    pub eye_render_desc: [EyeRenderDesc; 2],
    pub sensor_sample_time: f64,
    rt_temp_aa: Option<RenderTarget>,
    /// `rtFXAA_Temp`: YEBIS's destinations when FXAA is on
    fxaa_temp: Vec<RenderTarget>,
    fxaa_shader: Option<crate::shader::ShaderId>,
    depth_resolve_shader: Option<crate::shader::ShaderId>,
    /// `yebisPP` (video.ini ppHDREnabled): one YEBIS for both eyes
    yebis: Option<crate::yebis::Yebis>,
    /// `rtYebisTarget` (multisampled with AA), `rtYebisResolved`, `rtYebisResolvedDepth`
    rt_yebis_target: Option<RenderTarget>,
    rt_yebis_resolved: Option<RenderTarget>,
    rt_yebis_resolved_depth: Option<RenderTarget>,
    /// `mirrorTextureBuffers`: a view of each texture of the left eye's chain
    mirror_texture_buffers: Vec<Option<ID3D11ShaderResourceView>>,
}

impl StereoCameraForward {
    /// `StereoCameraForward::StereoCameraForward` 0x140211370, after `forward`'s own
    /// constructor. `yebis`: where YEBIS comes from when post-processing is on.
    pub fn new(graphics: &mut Graphics, forward: &mut CameraForward, mut ovr: Ovr, yebis: Option<YebisFrom>) -> Result<StereoCameraForward, String> {
        forward.base.camera.lod_multiplier = 0.5;
        if !ovr.initialized && !ovr.init() {
            return Err("Could not initialize oculus".into());
        }
        let desc = ovr.hmd_desc();
        if desc.product_name[0] == 0 {
            return Err("FAILED Rift detected, display not enabled.".into());
        }
        let depth_resolve_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksMSDepthResolve")?);
        let mut mirror_enabled = true;
        let mut ppd = 1.0;
        if let Ok(ini) = IniReader::load(&graphics.documents_folder.join("cfg/oculus.ini")) {
            ppd = ini.get_float("SETTINGS", "PIXEL_PER_DISPLAY").unwrap_or(0.0);
            if ini.has_section("MIRROR_TEXTURE") {
                mirror_enabled = ini.get_int("MIRROR_TEXTURE", "ENABLED").unwrap_or(0) != 0;
            }
        }
        let mut textures = Vec::new();
        let mut depth = Vec::new();
        let mut viewport = [Recti::default(); 2];
        for (eye, fov) in desc.default_eye_fov.iter().enumerate() {
            // SAFETY: the runtime's functions, as the game calls them
            let size = unsafe { (ovr.api.get_fov_texture_size)(ovr.session, eye as i32, fov, ppd) };
            let texture = unsafe { OculusTexture::init(&ovr, graphics, size.w, size.h) }.ok_or("OculusTexture->Init failed")?;
            depth.push(graphics.kgl.create_render_target_depth(size.w, size.h, 1)?);
            viewport[eye] = Recti { x: 0, y: 0, w: size.w, h: size.h };
            if texture.chain.is_null() {
                return Err("OculusTexture->Init failed to create swap chain".into());
            }
            textures.push(texture);
        }
        let (w, h, aa) = (viewport[0].w, viewport[0].h, graphics.video.aa_samples);
        let target = |graphics: &Graphics, format, samples: i32, depth: bool| -> Result<RenderTarget, String> {
            let color = graphics.kgl.create_render_target(format, w, h, samples, 1);
            let depth = if depth { Some(graphics.kgl.create_render_target_depth(w, h, samples)?) } else { None };
            Ok(RenderTarget { kid_color: Some(color), kid_depth: depth, width: w, height: h })
        };
        let (mut rt_temp_aa, mut yebis_pp, mut rt_yebis_target, mut rt_yebis_resolved, mut rt_yebis_resolved_depth) = (None, None, None, None, None);
        if !graphics.video.pp_hdr_enabled {
            if aa > 1 {
                rt_temp_aa = Some(target(graphics, FORMAT_RGBA8, aa, true)?);
            }
        } else {
            // (the constructor turns videoSettings.ppHeatShimmer off: no shimmer in VR)
            let YebisFrom { root, data, video } = yebis.ok_or("VR with post-processing needs YEBIS")?;
            // YebisPP(graphics, w, h, the left eye's current texture) 0x14002ba30, loadPPSet("default")
            // 0x14002cc80: initPP 0x140030540 takes the eye's width as the base width
            let filter = IniReader::load(&data.join("system/cfg/ppfilters/default.ini"))?;
            let index = ovr.current_index(textures[0].chain) as usize;
            let screen = textures[0].rtv.get(index).cloned();
            yebis_pp = Some(crate::yebis::Yebis::new(&graphics.kgl.device, &graphics.kgl.context, root, filter, video, w, h, w, screen.clone(), 256)?);
            // YEBIS's initialisation leaves its destination bound without depth (and kgl's cache unaware)
            unsafe { graphics.kgl.context.OMSetRenderTargets(Some(&[screen]), None) };
            rt_yebis_target = Some(target(graphics, FORMAT_RGBA16F, aa, true)?);
            if aa != 1 {
                rt_yebis_resolved = Some(target(graphics, FORMAT_RGBA16F, 1, false)?);
                rt_yebis_resolved_depth = Some(target(graphics, FORMAT_R32F, 1, false)?);
            }
        }
        let mut fxaa_temp = Vec::new();
        let mut fxaa_shader = None;
        if graphics.video.pp_fxaa && aa == 1 {
            for _ in 0..2 {
                fxaa_temp.push(target(graphics, FORMAT_RGBA8, 1, false)?);
            }
            fxaa_shader = Some(graphics.shaders.get_shader(&graphics.kgl, "ksFXAA")?);
        }
        graphics.video.v_sync = false;
        graphics.kgl.video.v_sync = false;
        let mut mirror = Vec::new();
        let chain = textures[0].chain;
        let mut length = 0;
        unsafe { (ovr.api.get_texture_swap_chain_length)(ovr.session, chain, &mut length) };
        for i in 0..length {
            let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT(0x1c),
                ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
            };
            let mut view = None;
            // (the texture's reference released when it drops, as the game releases it)
            if let Some(texture) = unsafe { buffer(&ovr, chain, i) } {
                unsafe { graphics.kgl.device.CreateShaderResourceView(&texture, Some(&desc), Some(&mut view)) }.map_err(|e| e.to_string())?;
            }
            mirror.push(view);
        }
        let mut textures = textures.into_iter();
        let mut depth = depth.into_iter();
        let (Some(t0), Some(t1), Some(d0), Some(d1)) = (textures.next(), textures.next(), depth.next(), depth.next()) else {
            return Err("no eye".into());
        };
        Ok(StereoCameraForward {
            ovr,
            render_scene: true,
            mirror_enabled,
            pixel_per_display: ppd,
            default_position: [0.0; 3],
            proj_matrix: Mat44f::default(),
            view_matrix: Mat44f::default(),
            frame_index: 0,
            eye_render_viewport: viewport,
            eye_render_poses: [Posef::default(); 2],
            eye_render_texture: [t0, t1],
            eye_depth_buffer: [d0, d1],
            eye_render_desc: [EyeRenderDesc::default(); 2],
            sensor_sample_time: 0.0,
            rt_temp_aa,
            fxaa_temp,
            fxaa_shader,
            depth_resolve_shader,
            yebis: yebis_pp,
            rt_yebis_target,
            rt_yebis_resolved,
            rt_yebis_resolved_depth,
            mirror_texture_buffers: mirror,
        })
    }

    /// `StereoCameraForward::computeCameraMatrix` 0x140212900: an eye's pose seen from the
    /// camera `c`.
    pub fn compute_camera_matrix(&self, c: &Mat44f, pose: &Posef) -> Mat44f {
        let r = crate::animator::xm_matrix_rotation_quaternion(&pose.orientation);
        let (p, d) = (pose.position, self.default_position);
        place(c, &r, [p[0] - d[0], p[1] - d[1], p[2] - d[2]])
    }

    /// `StereoCameraForward::computeEyePoses` 0x140212b30 (with the shim's `ovr_GetEyePoses`
    /// 0x140440e20).
    pub fn compute_eye_poses(&mut self) {
        let desc = self.ovr.hmd_desc();
        let (api, session) = (self.ovr.api, self.ovr.session);
        // SAFETY: the runtime's functions, as the game calls them
        unsafe {
            for eye in 0..2 {
                (api.get_render_desc)(&mut self.eye_render_desc[eye], session, eye as i32, &desc.default_eye_fov[eye]);
            }
            let offsets = [self.eye_render_desc[0].hmd_to_eye_offset, self.eye_render_desc[1].hmd_to_eye_offset];
            let time = (api.get_predicted_display_time)(session, self.frame_index);
            let mut state: TrackingState = std::mem::zeroed();
            (api.get_tracking_state)(&mut state, session, time, 1);
            self.eye_render_poses = calc_eye_poses(&state.head_pose, &offsets);
            self.sensor_sample_time = (api.get_time_in_seconds)();
        }
    }

    /// `StereoCameraForward::resetHeadPosition` 0x140213f40.
    pub fn reset_head_position(&mut self) {
        self.ovr.recenter();
    }

    /// `StereoCameraForward::render` 0x1402135c0: the window's picture, Ctrl+Space (recentre),
    /// the poses, the shadows and the cube map from the left eye, then both eyes.
    pub fn render(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, cube_node: Option<NodeId>, root: NodeId, dt: f32) -> Result<(), String> {
        self.render_mirror_texture(graphics);
        unsafe {
            if (self.ovr.api.get_async_key_state)(0x11) != 0 && (self.ovr.api.get_async_key_state)(0x20) != 0 {
                self.ovr.recenter();
            }
        }
        let exposure = forward.base.camera.exposure;
        self.compute_eye_poses();
        let saved = forward.base.camera.matrix;
        forward.base.camera.matrix = self.compute_camera_matrix(&saved, &self.eye_render_poses[0]);
        forward.base.camera.fov = 90.0;
        forward.base.shadow_map_pass(graphics, scene, root)?;
        if let Some(node) = cube_node {
            let m = &forward.base.camera.matrix.m;
            let position = [m[3][0], m[3][1], m[3][2]];
            crate::cubemap::render(&mut forward.cube_map_renderer, graphics, scene, &forward.cube_map, node, position, forward.base.sky_box.as_mut());
        }
        forward.base.camera.matrix = saved;
        forward.base.camera.fov = 45.0;
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

    /// `StereoCameraForward::renderNoPP` 0x1402139a0: each eye into its chain's current texture
    /// (with anti-aliasing through the multisampled target, then resolved), then committed.
    fn render_no_pp(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId) {
        let aa = graphics.video.aa_samples;
        for eye in 0..2 {
            let chain = self.eye_render_texture[eye].chain;
            if aa == 1 {
                let index = self.ovr.current_index(chain) as usize;
                if let Some(view) = self.eye_render_texture[eye].rtv.get(index) {
                    graphics.kgl.set_render_target_native(view, &self.eye_depth_buffer[eye]);
                }
            } else {
                graphics.set_render_target(self.rt_temp_aa.as_ref());
            }
            self.render_pass(forward, graphics, scene, eye, root);
            if aa > 1 {
                let index = self.ovr.current_index(chain) as usize;
                let from = self.rt_temp_aa.as_ref().and_then(|rt| rt.kid_color.as_ref()).and_then(|c| c.texture.clone());
                if let (Some(to), Some(from)) = (self.eye_render_texture[eye].textures.get(index), from) {
                    unsafe { graphics.kgl.context.ResolveSubresource(to, 0, &from, 0, DXGI_FORMAT(0x1c)) };
                }
            }
            unsafe { (self.ovr.api.commit_texture_swap_chain)(self.ovr.session, chain) };
        }
    }

    /// `StereoCameraForward::renderYebis` 0x140213b10: each eye into the HDR target, then the one
    /// YEBIS into the chain's current texture (through FXAA's target when FXAA is on), committed.
    fn render_yebis(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, root: NodeId, dt: f32) {
        let Some(target) = self.rt_yebis_target.take() else { return };
        self.ovr.hmd_desc(); // (the game asks for it and does not use it)
        for eye in 0..2 {
            graphics.kgl.set_render_targets(target.kid_color.as_ref(), target.kid_depth.as_ref());
            self.render_pass(forward, graphics, scene, eye, root);
            graphics.kgl.set_render_targets(None, None);
            // updatePPCameraValues 0x14002bcc0 and updatePP 0x14002c270 (no depth of field, no shimmer in VR)
            let c = &forward.base.camera;
            let v = self.eye_render_viewport[eye];
            let camera = crate::yebis_cb::Camera { near: c.near_plane, far: c.far_plane, fov_deg: c.fov, width: v.w as u32, height: v.h as u32, exposure: c.exposure, exposure_multiplier: graphics.exposure_multiplier, dt, chromatic_aberration: false, new_exposure: eye == 0, sun: None };
            let chain = self.eye_render_texture[eye].chain;
            let fxaa = self.fxaa_temp.get(eye);
            let (scene_srv, depth_srv) = match (&self.rt_yebis_resolved, &self.rt_yebis_resolved_depth, fxaa) {
                (Some(color), Some(depth), None) => {
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
            let destination = match fxaa {
                Some(f) => f.kid_color.as_ref().and_then(|c| c.render_target_view.clone()),
                None => self.eye_render_texture[eye].rtv.get(self.ovr.current_index(chain) as usize).cloned(),
            };
            if let (Some(yebis), Some(scene_srv), Some(depth_srv), Some(destination)) = (&mut self.yebis, scene_srv, depth_srv, destination) {
                if let Err(e) = yebis.render(&depth_srv, &scene_srv, &destination, &camera, None) {
                    eprintln!("{e}");
                }
            }
            if let Some(fxaa) = fxaa {
                graphics.kgl.set_default_state();
                graphics.set_sampler_state();
                graphics.reset_render_states();
                let index = self.ovr.current_index(chain) as usize;
                if let Some(view) = self.eye_render_texture[eye].rtv.get(index) {
                    graphics.kgl.set_render_target_native(view, &self.eye_depth_buffer[eye]);
                }
                graphics.set_viewport(0, 0, v.w, v.h);
                graphics.set_depth_mode(DEPTH_OFF);
                graphics.kgl.set_texture(5, &fxaa.kid_color.as_ref().and_then(|c| c.shader_resource_view.clone()));
                crate::gl::full_screen_quad(graphics, self.fxaa_shader);
            }
            unsafe { (self.ovr.api.commit_texture_swap_chain)(self.ovr.session, chain) };
            graphics.kgl.set_default_state();
            graphics.set_sampler_state();
            graphics.reset_render_states();
            graphics.kgl.set_texture_cube_map(Some(&forward.cube_map), 10);
            for i in 0..3 {
                graphics.set_shadow_map_texture(i as i32, Some(&forward.base.shadow_rt[i]));
                graphics.set_shadow_map_matrix(i as i32, &forward.base.shadow_matrices[i]);
            }
        }
        self.rt_yebis_target = Some(target);
    }

    /// `StereoCameraForward::oculusRenderPass` 0x140213230: one eye from its pose, with the
    /// runtime's projection of its field of view.
    fn render_pass(&mut self, forward: &mut CameraForward, graphics: &mut Graphics, scene: &mut Scene, eye: usize, root: NodeId) {
        let camera: &mut Camera = &mut forward.base.camera;
        let saved = camera.matrix;
        camera.matrix = self.compute_camera_matrix(&saved, &self.eye_render_poses[eye]);
        self.view_matrix = camera.get_view_matrix();
        let m = projection(&self.eye_render_desc[eye].fov, camera.near_plane, camera.far_plane);
        self.proj_matrix = Mat44f { m: std::array::from_fn(|r| std::array::from_fn(|c| m[c][r])) };
        let v = self.eye_render_viewport[eye];
        graphics.set_viewport(v.x, v.y, v.w, v.h);
        normal_rendering(forward, graphics, scene, root, self.render_scene, self.proj_matrix, self.view_matrix);
        forward.base.camera.matrix = saved;
    }

    /// `StereoCameraForward::renderMirrorTexture` 0x1402137b0: the window, with the left eye's
    /// current texture filling it (cropped to keep its shape) when oculus.ini asks for it.
    fn render_mirror_texture(&mut self, graphics: &mut Graphics) {
        let index = self.ovr.current_index(self.eye_render_texture[0].chain) as usize;
        graphics.set_screen_render_targets();
        graphics.clear_render_target(&[0.5, 0.1, 0.1, 1.0]);
        if !self.mirror_enabled {
            return;
        }
        graphics.clear_render_target_depth(1.0);
        graphics.reset_render_states();
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_depth_mode(DEPTH_OFF);
        graphics.kgl.set_texture(0, self.mirror_texture_buffers.get(index).unwrap_or(&None));
        graphics.set_screen_space_mode();
        let Some(mut gl) = graphics.gl.take() else { return };
        gl.color3f(1.0, 1.0, 1.0);
        let (mut ew, mut eh) = (self.eye_render_viewport[0].w as f32, self.eye_render_viewport[0].h as f32);
        let (w, h) = (graphics.video.width as f32, graphics.video.height as f32);
        let (sx, sy) = (w / ew, h / eh);
        let s = if sx <= sy { sy } else { sx };
        eh *= s;
        ew *= s;
        gl.quad(graphics, (ew - w) * -0.5, (eh - h) * -0.5, ew, eh, true, None);
        graphics.gl = Some(gl);
    }

    /// `StereoCameraForward::finishRendering` 0x140212d10 (`Game::evOnRenderFinished`, in
    /// place of `GraphicsManager::endScene`): the frame to the runtime, the window's swap, then
    /// the runtime's wish to recentre.
    pub fn finish_rendering(&mut self, graphics: &Graphics) {
        self.submit();
        graphics.kgl.swap_buffers();
        self.after_swap();
    }

    /// `finishRendering`'s part before the swap: one `ovrLayerEyeFov` of both eyes.
    pub fn submit(&mut self) {
        let desc = self.ovr.hmd_desc();
        let layer = LayerEyeFov {
            kind: 1,
            flags: 0,
            color_texture: [self.eye_render_texture[0].chain, self.eye_render_texture[1].chain],
            viewport: self.eye_render_viewport,
            fov: desc.default_eye_fov,
            render_pose: self.eye_render_poses,
            sensor_sample_time: self.sensor_sample_time,
        };
        let layers = [&layer as *const LayerEyeFov];
        unsafe { (self.ovr.api.submit_frame)(self.ovr.session, self.frame_index, std::ptr::null(), layers.as_ptr(), 1) };
        self.frame_index += 1;
    }

    /// `finishRendering`'s part after the swap: `ovrSessionStatus::ShouldRecenter`.
    pub fn after_swap(&mut self) {
        let mut status: SessionStatus = [0; 6];
        unsafe { (self.ovr.api.get_session_status)(self.ovr.session, &mut status) };
        if status[5] != 0 {
            self.ovr.recenter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_the_game_s() {
        assert_eq!(std::mem::size_of::<HmdDesc>(), 0x108);
        assert_eq!(std::mem::offset_of!(HmdDesc, default_eye_fov), 0xb8);
        assert_eq!(std::mem::size_of::<EyeRenderDesc>(), 0x38);
        assert_eq!(std::mem::offset_of!(EyeRenderDesc, hmd_to_eye_offset), 0x2c);
        assert_eq!(std::mem::size_of::<TrackingState>(), 0x138);
        assert_eq!(std::mem::size_of::<SwapChainDesc>(), 0x28);
        assert_eq!(std::mem::size_of::<LayerEyeFov>(), 0x98);
        assert_eq!(std::mem::offset_of!(LayerEyeFov, render_pose), 0x58);
        assert_eq!(std::mem::size_of::<InitParams>(), 0x20);
    }
}
