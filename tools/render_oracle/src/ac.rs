// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side: acs.exe's own `GraphicsManager` (and with it kgl, the state objects, the
//! `ShaderManager`), `KN5IO`, `SkyBox` and `CameraForward`, built by address on a WARP device
//! with a window that is never shown, and one frame rendered by the game's own
//! `CameraForward::render`.
//!
//! What the harness itself does in place of game code it cannot run (the whole `Sim`, `Game`,
//! `TrackAvatar` and `CarAvatar`) is listed in `docs/port/renderer_core.md`: the scene graph of
//! `Sim::initSceneGraph`, the loop over `models.ini` of `TrackAvatar::init3D`, what `Sim::Sim`
//! and `Sim::initCubemaps` set on the camera, what the game camera of the moment sets every
//! frame, and the order of calls of `Game::onIdle`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP};

use crate::acs::Acs;
use crate::frames::Frame;
use crate::Args;

const VA_GRAPHICS_MANAGER_CTOR: usize = 0x1_4020_1620; // GraphicsManager::GraphicsManager(const VideoSettings&)
const VA_KGL_INIT_FONTS: usize = 0x1_4001_8210; // kglInitFonts (FW1FontWrapper + DirectWrite: not part of the 3D frame)
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized
const VA_ATEXIT: usize = 0x1_4039_9754; // atexit (the program's own, statically linked)
const VA_GRAPHICS_BEGIN_SCENE: usize = 0x1_4020_2580; // GraphicsManager::beginScene()
const VA_GRAPHICS_END_SCENE: usize = 0x1_4020_2850; // GraphicsManager::endScene()
const VA_GRAPHICS_COMPILE: usize = 0x1_4020_26e0; // GraphicsManager::compile(Node*)
const VA_GRAPHICS_SET_SCREEN_SPACE_MODE: usize = 0x1_4020_48e0; // GraphicsManager::setScreenSpaceMode()
const VA_NODE_CTOR: usize = 0x1_4020_db10; // Node::Node(const std::wstring&)
const VA_NODE_EVENT_CTOR: usize = 0x1_4021_e4c0; // NodeEvent::NodeEvent(const std::wstring&)
const VA_KN5IO_CTOR: usize = 0x1_4021_45a0; // KN5IO::KN5IO(GraphicsManager*)
const VA_KN5IO_ADD_DLC_KEY: usize = 0x1_4021_4de0; // static void KN5IO::addDLCKey(unsigned int)
const VA_KN5IO_LOAD: usize = 0x1_4021_51a0; // Node* KN5IO::load(const std::wstring&)
const VA_CAMERA_FORWARD_CTOR: usize = 0x1_4021_f230; // CameraForward::CameraForward(const std::wstring&, GraphicsManager*, bool)
const VA_CAMERA_FORWARD_SET_CUBEMAP_SIZE: usize = 0x1_4022_01b0; // CameraForward::setCubemapSize(int)
const VA_CAMERA_SET_SHADOW_MAPS_SPLITS: usize = 0x1_4020_d5f0; // CameraShadowMapped::setShadowMapsSplits(float, float, float, float)
const VA_SKYBOX_CTOR: usize = 0x1_4021_c790; // SkyBox::SkyBox(GraphicsManager*)
const VA_WORLD_MATRIX_TRAVERSE: usize = 0x1_4021_abf0; // WorldMatrixTraverser::traverse(Node*)
const VA_KN5IO_ADD_TEXTURE_FOLDER: usize = 0x1_4021_4e90; // KN5IO::addTextureFolder(const std::wstring&)
const VA_GRAPHICS_UPDATE_LIGHTING: usize = 0x1_4020_5190; // GraphicsManager::updateLightingSetttings()
const VA_GRAPHICS_LOAD_LIGHTING: usize = 0x1_4020_3250; // GraphicsManager::loadLightingSettings(const std::wstring&)
const VA_WEATHER_LOAD_PRESET: usize = 0x1_4022_7260; // static bool WeatherGenerator::loadPreset(const std::wstring&, GraphicsManager*, float)
const VA_CUBE_MAP_RENDERER_RENDER: usize = 0x1_4021_edb0; // CubeMapRenderer::render(CubeMap*, Node*, Camera*)
const VA_CUBE_MAP_RENDERER_SET_PLANES: usize = 0x1_4021_f110; // CubeMapRenderer::setCameraNearFarPlanes(float, float)
const VA_SKYBOX_UPDATE_CLOUDS: usize = 0x1_4021_db00; // SkyBox::updateCloudsGeneration(const std::wstring&)
const VA_DYNAMIC_TRACK_MANAGER_CTOR: usize = 0x1_401c_cc20; // DynamicTrackManager::DynamicTrackManager(Sim*, const std::wstring&)
const VA_DYNAMIC_TRACK_MANAGER_UPDATE: usize = 0x1_401c_dba0; // DynamicTrackManager::update(float)
const VA_CAMERA_FACING_CTOR: usize = 0x1_4005_e640; // CameraFacing::CameraFacing(TrackAvatar*, Sim*)
const VA_GROOVE_HIDE_HANDLER: usize = 0x1_4019_69f0; // Sim::initCubemaps' handler of evOnRenderBegin
const VA_GROOVE_SHOW_HANDLER: usize = 0x1_4019_6bc0; // … and of evOnRenderEnd
const VA_INIT_DYNAMIC_OBJECTS: usize = 0x1_401c_9840; // TrackAvatar::initDynamicObjects()
const VA_UPDATE_DYNAMIC_OBJECTS: usize = 0x1_401c_cb30; // TrackAvatar::updateDynamicObjects(float)
const VA_MESH_VTABLE: usize = 0x1_404e_da28; // const Mesh::`vftable'
const VA_KS_RANDOMIZE: usize = 0x1_4004_b290; // ksRandomize(unsigned int): srand
const VA_SKYBOX_UPDATE_CLOUDS_ANIMATION: usize = 0x1_4021_dad0; // SkyBox::updateCloudsAnimation(float)
const VA_AI_SPLINE_CTOR: usize = 0x1_402a_4d70; // AISpline::AISpline()
const VA_AI_SPLINE_LOAD_FAST: usize = 0x1_402a_83c0; // AISpline::loadFast(const std::wstring&)
const VA_RESOURCE_STORE_GET_TEXTURE: usize = 0x1_4020_00d0; // ResourceStore::getTexture(Texture*, const std::wstring&, bool)
const VA_TIME_ATTACK_RENDER_HANDLER_VTABLE: usize = 0x1_404d_cef8; // the std::function body of TimeAttack's node-render handler (0x1401b8dd0)
const VA_INIT_RESPAWN_POSITION_SET: usize = 0x1_401c_b8c0; // TrackAvatar::initRespawnPositionSet(const std::wstring&)
const VA_SPAWN_POSITIONS_HEAD: usize = 0x1_401c_6d80; // the head node of TrackAvatar::spawnPositions (std::map's _Buyheadnode)
const VA_INIT_PIT_CREW: usize = 0x1_401c_a630; // TrackAvatar::initPitCrew()
const VA_LOLLIPOP_CREW_UPDATE: usize = 0x1_4011_3ab0; // LollipopCrew::update(float)
const VA_TRACK_NEW_SESSION_HANDLER: usize = 0x1_401c_6b70; // the handler TrackAvatar::TrackAvatar hangs on Sim::evOnNewSession
const VA_FUNCTION_QUEUE_VTABLE: usize = 0x1_404c_7b30; // Concurrency::concurrent_queue<std::function<void(void)>>::`vftable'
const VA_STARTING_LIGHTS_CTOR: usize = 0x1_401a_1a20; // StartingLights::StartingLights(Sim*)
const VA_STARTING_LIGHTS_RENDER_HUD: usize = 0x1_401a_2e80; // StartingLights::renderHUD(float)
const VA_GRAPHICS_RESET_RENDER_STATES: usize = 0x1_4020_4440; // GraphicsManager::resetRenderStates()
const VA_SEMAPHORE_CTOR: usize = 0x1_401a_37a0; // StartingTrackSemaphore::StartingTrackSemaphore(Sim*)
const VA_SEMAPHORE_UPDATE: usize = 0x1_401a_4cc0; // StartingTrackSemaphore::update(float)
const VA_IDEAL_LINE_CTOR: usize = 0x1_4010_9d10; // IdealLine::IdealLine(AISpline*, GraphicsManager*, IRayTrackCollisionProvider*)

/// In place of `Track::rayCast` 0x140278bb0 (slot 1 of the track's table): the port's ray on
/// the port's track. A hit fills the game's `RayCastResult` (position +8, normal +0x14, the hit
/// flag +0x20); a miss only clears the flag, as the game's does.
unsafe extern "C" fn track_ray_cast(_track: *mut u8, from: *const [f32; 3], dir: *const [f32; 3], out: *mut u8, length: f32) -> bool {
    match crate::frames::ray_cast(&*from, &*dir, length) {
        Some((pos, normal)) => {
            wr(out, 0x8, pos);
            wr(out, 0x14, normal);
            wr(out, 0x20, 1u8);
            true
        }
        None => {
            wr(out, 0x20, 0u8);
            false
        }
    }
}

/// A stand-in `Track` (0x148 bytes) of which only `rayCast` is ever called.
pub unsafe fn ray_track(acs: &Acs) -> *mut u8 {
    let track = acs.alloc(0x148);
    wr(track, 0, ray_table(acs));
    track
}

/// The table of a stand-in `Track`: `rayCast` in slot 1.
unsafe fn ray_table(acs: &Acs) -> *mut u8 {
    let table = acs.alloc(0x40);
    wr(table, 8, track_ray_cast as *const () as usize);
    table
}

/// Files of the game the pit crew opens by paths under the current folder (the scratch root).
fn copy_into_root(game: &std::path::Path, relative: &str) -> Result<(), String> {
    let from = game.join(relative);
    if from.is_dir() {
        std::fs::create_dir_all(relative).map_err(|e| format!("{relative}: {e}"))?;
        for entry in std::fs::read_dir(&from).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
            let to = std::path::Path::new(relative).join(entry.file_name());
            if !to.is_file() {
                std::fs::copy(entry.path(), &to).map_err(|e| format!("{}: {e}", to.display()))?;
            }
        }
    } else if from.is_file() && !std::path::Path::new(relative).is_file() {
        if let Some(parent) = std::path::Path::new(relative).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&from, relative).map_err(|e| format!("{relative}: {e}"))?;
    }
    Ok(())
}

/// Start-up initialisers of static objects the renderer uses (the program's entry point, which
/// would run them all, is never run): kgl's map of input layouts, `Curve::openedFiles`,
/// `KN5IO::dlc_keys`.
const VA_STATIC_INITIALISERS: [usize; 3] = [0x1_4000_1040, 0x1_4001_0f90, 0x1_4001_0fa0];

const SIZE_GRAPHICS_MANAGER: usize = 0x4e0;
const SIZE_NODE: usize = 0xe0;
const SIZE_KN5IO: usize = 0xc0;
const SIZE_CAMERA_FORWARD: usize = 0x788;
const SIZE_CAMERA_FORWARD_YEBIS: usize = 0xe80;
const VA_CAMERA_FORWARD_YEBIS_CTOR: usize = 0x1_4002_5db0; // CameraForwardYebis::CameraForwardYebis(const std::wstring&, GraphicsManager*, bool, bool, int)
const VA_YEBIS_INIT_POST_PROCESSING: usize = 0x1_4002_4ca0; // CameraForwardYebis::initPostProcessing()
const VA_YEBIS_APPLY_POST_PROCESSING: usize = 0x1_4002_5c30; // CameraForwardYebis::applyPostProcessing()
const VA_GRAPHICS_GET_SHADER: usize = 0x1_4020_2c60; // GraphicsManager::getShader(const std::wstring&)
const VA_GRAPHICS_SET_BLEND_MODE: usize = 0x1_4020_44e0; // GraphicsManager::setBlendMode(BlendMode)
const VA_GRAPHICS_SET_DEPTH_MODE: usize = 0x1_4020_4580; // GraphicsManager::setDepthMode(DepthMode)
const VA_KGL_SET_RENDER_TARGETS: usize = 0x1_4001_8e40; // kglSetRenderTargets(void*, void*)
const VA_KGL_SET_VIEWPORT: usize = 0x1_4001_8ee0; // kglSetViewport(float, float, float, float)
const VA_KGL_SET_TEXTURE_RT: usize = 0x1_4001_8ec0; // kglSetTextureRT(int, void*)
const VA_GL_FULL_SCREEN_QUAD: usize = 0x1_401f_eaa0; // GLRenderer::fullScreenQuad(Shader*)

/// What the stand-in for YEBIS needs of the game (see `rustyac_render::post`).
struct StandIn {
    graphics: *mut u8,
    shader: *mut u8,
    /// a KGLRenderTarget with no views: binds "no texture"
    no_texture: *mut u8,
    set_blend_mode: usize,
    set_depth_mode: usize,
    set_render_targets: usize,
    set_viewport: usize,
    set_texture_rt: usize,
    full_screen_quad: usize,
}

static STAND_IN: std::sync::atomic::AtomicPtr<StandIn> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// NOT Assetto Corsa: runs where the game would call YEBIS
/// (`CameraForwardYebis::applyPostProcessing` 0x140025c30 is patched to jump here). The same
/// plain copy as `rustyac_render::post::stand_in_apply_effects`, made of the game's own calls:
/// `pp.sourceRenderTarget` into `pp.destinationRenderTarget` with `ksPostCopy`.
unsafe extern "C" fn stand_in_apply_post_processing(camera: *mut u8) {
    let s = &*STAND_IN.load(std::sync::atomic::Ordering::SeqCst);
    let source: *mut u8 = rd(camera, 0x828);
    let destination: *mut u8 = rd(camera, 0x838);
    let (width, height): (i32, i32) = (rd(camera, 0xe70), rd(camera, 0xe74));
    let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(s.set_blend_mode);
    set_mode(s.graphics, 0);
    let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(s.set_depth_mode);
    set_mode(s.graphics, 2);
    let set_render_targets: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(s.set_render_targets);
    set_render_targets(destination, std::ptr::null_mut());
    let set_viewport: extern "C" fn(f32, f32, f32, f32) = std::mem::transmute(s.set_viewport);
    set_viewport(0.0, 0.0, width as f32, height as f32);
    let set_texture_rt: extern "C" fn(i32, *mut u8) = std::mem::transmute(s.set_texture_rt);
    set_texture_rt(0, source);
    let quad: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(s.full_screen_quad);
    quad(rd(s.graphics, 0x380), s.shader);
    set_texture_rt(0, s.no_texture);
}
const SIZE_SKYBOX: usize = 0x80;

pub unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    std::ptr::write_unaligned(base.add(offset) as *mut T, value);
}

pub unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    std::ptr::read_unaligned(base.add(offset) as *const T)
}

/// Writes an MSVC `std::wstring` (0x20 bytes: buffer or pointer, size, capacity).
pub unsafe fn write_wstring(acs: &Acs, at: *mut u8, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() < 8 {
        std::ptr::write_bytes(at, 0, 0x10);
        std::ptr::copy_nonoverlapping(units.as_ptr(), at as *mut u16, units.len());
        wr(at, 0x10, units.len());
        wr(at, 0x18, 7usize);
    } else {
        let buffer = acs.alloc((units.len() + 1) * 2);
        std::ptr::copy_nonoverlapping(units.as_ptr(), buffer as *mut u16, units.len());
        wr(at, 0, buffer);
        wr(at, 0x10, units.len());
        wr(at, 0x18, units.len());
    }
}

/// A `std::wstring` of its own, for a `const std::wstring&` argument.
pub unsafe fn wstring(acs: &Acs, text: &str) -> *mut u8 {
    let s = acs.alloc(0x20);
    write_wstring(acs, s, text);
    s
}

static DEVICE: AtomicUsize = AtomicUsize::new(0);
static CONTEXT: AtomicUsize = AtomicUsize::new(0);
static SWAP_CHAIN: AtomicUsize = AtomicUsize::new(0);

type CreateDeviceAndSwapChain = unsafe extern "system" fn(
    adapter: *mut c_void,
    driver_type: i32,
    software: *mut c_void,
    flags: u32,
    levels: *const i32,
    level_count: u32,
    sdk: u32,
    swap_chain_desc: *const c_void,
    swap_chain: *mut *mut c_void,
    device: *mut *mut c_void,
    level: *mut i32,
    context: *mut *mut c_void,
) -> i32;

/// In place of `D3D11CreateDeviceAndSwapChain`: the same call, on WARP (Microsoft's software
/// rasteriser: the same pixels on every machine, no GPU needed), whatever adapter was asked for.
unsafe extern "system" fn create_device_and_swap_chain(
    _adapter: *mut c_void,
    _driver_type: i32,
    software: *mut c_void,
    flags: u32,
    levels: *const i32,
    level_count: u32,
    sdk: u32,
    swap_chain_desc: *const c_void,
    swap_chain: *mut *mut c_void,
    device: *mut *mut c_void,
    level: *mut i32,
    context: *mut *mut c_void,
) -> i32 {
    const D3D_DRIVER_TYPE_WARP: i32 = 5;
    let module = LoadLibraryW(w!("d3d11.dll")).expect("d3d11.dll");
    let real: CreateDeviceAndSwapChain = std::mem::transmute(GetProcAddress(module, windows::core::s!("D3D11CreateDeviceAndSwapChain")).expect("D3D11CreateDeviceAndSwapChain"));
    let result = real(std::ptr::null_mut(), driver_type(D3D_DRIVER_TYPE_WARP), software, flags, levels, level_count, sdk, swap_chain_desc, swap_chain, device, level, context);
    if result >= 0 {
        DEVICE.store(*device as usize, Ordering::SeqCst);
        CONTEXT.store(*context as usize, Ordering::SeqCst);
        if !swap_chain.is_null() {
            SWAP_CHAIN.store(*swap_chain as usize, Ordering::SeqCst);
        }
        // the game gets the logging stand-in of the context; the oracle keeps the real one
        // (a timing run draws on the real context: the stand-in costs far more than a frame)
        if !crate::timing() {
            *context = rustyac_render::gpulog::install(*device, *context).expect("the command log");
        }
    } else {
        eprintln!("oracle: D3D11CreateDeviceAndSwapChain on WARP failed: {result:#x}");
    }
    result
}

/// `RENDER_ORACLE_HARDWARE=1` (Task 22b, the white sparkles): both sides draw on the graphics
/// card instead of WARP, to see what the game's own renderer does on this PC's driver. The
/// proofs never set it.
pub fn hardware() -> bool {
    std::env::var_os("RENDER_ORACLE_HARDWARE").is_some_and(|v| v == "1")
}

fn driver_type(warp: i32) -> i32 {
    const D3D_DRIVER_TYPE_HARDWARE: i32 = 1;
    if hardware() {
        D3D_DRIVER_TYPE_HARDWARE
    } else {
        warp
    }
}

type CreateDevice = unsafe extern "system" fn(*mut c_void, i32, *mut c_void, u32, *const i32, u32, u32, *mut *mut c_void, *mut i32, *mut *mut c_void) -> i32;

/// In place of `D3D11CreateDevice` (the game's throw-away first device, made to learn the
/// feature level): on WARP as well, so that no graphics card is touched.
unsafe extern "system" fn create_device(_adapter: *mut c_void, _driver_type: i32, software: *mut c_void, flags: u32, levels: *const i32, level_count: u32, sdk: u32, device: *mut *mut c_void, level: *mut i32, context: *mut *mut c_void) -> i32 {
    const D3D_DRIVER_TYPE_WARP: i32 = 5;
    let module = LoadLibraryW(w!("d3d11.dll")).expect("d3d11.dll");
    let real: CreateDevice = std::mem::transmute(GetProcAddress(module, windows::core::s!("D3D11CreateDevice")).expect("D3D11CreateDevice"));
    real(std::ptr::null_mut(), driver_type(D3D_DRIVER_TYPE_WARP), software, flags, levels, level_count, sdk, device, level, context)
}

/// In place of `SHGetFolderPathW` (the game asks for the Documents folder): a folder of the
/// oracle's scratch root, so that nothing of the user's own settings is read.
unsafe extern "system" fn sh_get_folder_path(_window: *mut c_void, _folder: i32, _token: *mut c_void, _flags: u32, path: *mut u16) -> i32 {
    let documents = crate::root::documents();
    let units: Vec<u16> = documents.to_string_lossy().encode_utf16().chain([0]).collect();
    std::ptr::copy_nonoverlapping(units.as_ptr(), path, units.len().min(260));
    0
}

/// In place of the game's `operator new` and `operator new[]` in the GUI oracle: the C
/// runtime's `calloc`. The game leaves some of what it allocates unwritten (the last index of
/// FW1's index buffer, which is one longer than its quads need), and the command log hashes
/// it; zeroed, the log does not depend on what the heap held before.
unsafe extern "C" fn operator_new_zeroed(size: usize) -> *mut c_void {
    static CALLOC: AtomicUsize = AtomicUsize::new(0);
    let mut calloc = CALLOC.load(Ordering::SeqCst);
    if calloc == 0 {
        let module = LoadLibraryW(w!("msvcr120.dll")).expect("msvcr120.dll");
        calloc = GetProcAddress(module, windows::core::s!("calloc")).expect("calloc") as usize;
        CALLOC.store(calloc, Ordering::SeqCst);
    }
    let calloc: extern "C" fn(usize, usize) -> *mut c_void = std::mem::transmute(calloc);
    calloc(1, size.max(1))
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

/// A window that is never shown: the swap chain needs one.
pub unsafe fn hidden_window(width: u32, height: u32) -> Result<HWND, String> {
    let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance.into(), lpszClassName: w!("rustyac_render_oracle"), ..Default::default() };
    RegisterClassW(&class);
    CreateWindowExW(WINDOW_EX_STYLE(0), w!("rustyac_render_oracle"), w!("render oracle"), WS_POPUP, 0, 0, width as i32, height as i32, None, None, Some(instance.into()), None)
        .map_err(|e| format!("CreateWindowExW: {e}"))
}

pub struct Game {
    pub acs: Acs,
    /// the game's `GraphicsManager`
    pub graphics: *mut u8,
    pub device: *mut c_void,
    pub context: *mut c_void,
    pub swap_chain: *mut c_void,
    /// the calls of the start-up (state objects, constant buffers, samplers)
    pub init_log: Vec<u8>,
    /// the game's folder (files the oracle copies into its scratch root)
    pub game_folder: std::path::PathBuf,
}

/// One captured frame: its hashes, and the whole of it when it was asked for.
pub struct Captured {
    pub index: usize,
    pub calls: usize,
    pub draws: u64,
    pub log_hash: u64,
    pub pixels_hash: u64,
    pub log: Option<Vec<u8>>,
    pub pixels: Option<Vec<u8>>,
}

/// FNV-1a.
pub fn hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl Captured {
    pub fn new(index: usize, log: Vec<u8>, draws: u64, pixels: Vec<u8>, keep: bool) -> Captured {
        Captured { index, calls: log.iter().filter(|b| **b == 10).count(), draws, log_hash: hash(&log), pixels_hash: hash(&pixels), log: keep.then_some(log), pixels: keep.then_some(pixels) }
    }
}

/// What one run of a renderer gave.
pub struct Rendered {
    /// the calls of the one-time render of the reflection cube map (`Sim::initStaticCubemap`)
    pub cube_log: Vec<u8>,
    pub frames: Vec<Captured>,
    pub width: u32,
    pub height: u32,
}

impl Game {
    pub fn start(args: &Args) -> Result<Game, String> {
        let gui = args.gui.is_some();
        if gui {
            // FW1FontWrapper: DirectWrite (Windows' own, from System32) and the GDI calls of its glyph rasteriser
            crate::acs::set_extra_bound(vec!["d3d11.dll", "dxgi.dll", "d3dcompiler_43.dll", "d3dx11_43.dll", "dwrite.dll", "gdi32.dll", "user32.dll"]);
        } else {
            crate::acs::set_extra_bound(vec!["d3d11.dll", "dxgi.dll", "d3dcompiler_43.dll", "d3dx11_43.dll"]);
        }
        let mut overrides = vec![
            ("d3d11.dll", "D3D11CreateDeviceAndSwapChain", create_device_and_swap_chain as *const () as usize),
            ("d3d11.dll", "D3D11CreateDevice", create_device as *const () as usize),
            ("shell32.dll", "SHGetFolderPathW", sh_get_folder_path as *const () as usize),
        ];
        if gui {
            overrides.push(("msvcr120.dll", "??2@YAPEAX_K@Z", operator_new_zeroed as *const () as usize));
            overrides.push(("msvcr120.dll", "??_U@YAPEAX_K@Z", operator_new_zeroed as *const () as usize));
        }
        crate::acs::set_extra_overrides(overrides);
        let acs = Acs::load(&args.acs)?;
        if args.verbose {
            acs.unbuffer_game_stdout();
        } else {
            acs.silence_game_stdout();
        }
        unsafe {
            acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);
            // xor eax, eax; ret: static objects made here are never destroyed
            acs.patch(acs.va(VA_ATEXIT), &[0x31, 0xc0, 0xc3]);
            // ret: no fonts (the 3D frame draws no text)
            if !gui {
                acs.patch(acs.va(VA_KGL_INIT_FONTS), &[0xc3]);
            }
            for va in VA_STATIC_INITIALISERS {
                let init: extern "C" fn() = std::mem::transmute(acs.va(va));
                init();
            }

            let window = hidden_window(args.width, args.height)?;
            // VideoSettings (0x50 bytes), as loadVideoSettings fills it from the profile's video.ini
            let settings = acs.alloc(0x50);
            wr(settings, 0x00, crate::root::profile().aa_samples); // aaSamples
            wr(settings, 0x04, args.width as i32);
            wr(settings, 0x08, args.height as i32);
            wr(settings, 0x10, window.0 as usize);
            wr(settings, 0x18, 0u8); // isFullscreen
            wr(settings, 0x19, 0u8); // vSync
            wr(settings, 0x1c, crate::root::profile().anisotropic); // anisotropic
            wr(settings, 0x20, 0i32); // aaQuality
            wr(settings, 0x24, crate::root::profile().shadow_map_size); // shadowMapSize
            wr(settings, 0x28, 0f64); // fpsCapMS
            wr(settings, 0x30, 0i32); // dxgiModeIndex
            wr(settings, 0x34, crate::root::profile().world_detail); // worldDetail
            wr(settings, 0x38, crate::root::profile().post as u8); // ppHDREnabled
            wr(settings, 0x3a, (crate::root::profile().post && crate::root::profile().fxaa) as u8); // ppFXAA
            wr(settings, 0x3c, 0i32); // ppQuality
            wr(settings, 0x40, 0i32); // ppGlare
            wr(settings, 0x44, 0i32); // ppDof
            wr(settings, 0x48, 0u8); // tripleBuffer
            wr(settings, 0x4c, 60f32); // refresh

            let graphics = acs.alloc(SIZE_GRAPHICS_MANAGER);
            let ctor: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GRAPHICS_MANAGER_CTOR));
            rustyac_render::gpulog::capture_from_install(true);
            ctor(graphics, settings);
            let init = rustyac_render::gpulog::end_capture();
            if crate::root::profile().post && crate::root::profile().ldr_levels {
                wr(graphics, 0x48, 0u8);
            }
            Ok(Game {
                acs,
                graphics,
                device: DEVICE.load(Ordering::SeqCst) as *mut c_void,
                context: CONTEXT.load(Ordering::SeqCst) as *mut c_void,
                swap_chain: SWAP_CHAIN.load(Ordering::SeqCst) as *mut c_void,
                init_log: init.text,
                game_folder: args.game.clone(),
            })
        }
    }

    /// `--ldr-levels`: `GraphicsManager::videoSettings.ppHDREnabled` is on only around the
    /// camera's own calls (`on`), and off for every other reader of the flag.
    unsafe fn hdr_flag(&self, on: bool) {
        let profile = crate::root::profile();
        if profile.post && profile.ldr_levels {
            wr(self.graphics, 0x48, on as u8);
        }
    }

    pub unsafe fn node(&self, name: &str) -> *mut u8 {
        let node = self.acs.alloc(SIZE_NODE);
        let ctor: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_NODE_CTOR));
        ctor(node, wstring(&self.acs, name))
    }

    /// A `NodeEvent` (0xf8 bytes) of the game.
    pub unsafe fn node_event(&self, name: &str) -> *mut u8 {
        let node = self.acs.alloc(0xf8);
        let ctor: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_NODE_EVENT_CTOR));
        ctor(node, wstring(&self.acs, name))
    }

    /// `parent->addChild(child)` (virtual, slot +8).
    pub unsafe fn add_child(&self, parent: *mut u8, child: *mut u8) {
        let vtable = rd::<*const usize>(parent, 0);
        let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(1));
        add(parent, child);
    }

    /// A `KN5IO` of the game.
    pub unsafe fn kn5io(&self) -> *mut u8 {
        let io = self.acs.alloc(SIZE_KN5IO);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_KN5IO_CTOR));
        ctor(io, self.graphics)
    }

    /// The game's own `KN5IO::load` of a model file (an absolute path into the game's folder).
    pub unsafe fn load_kn5(&self, io: *mut u8, path: &str) -> Result<*mut u8, String> {
        // a version 6 file carries a number that must be one the game was told at start-up (by
        // SteamInit, per owned pack): the oracle tells it the file's own
        if let Ok(mut file) = std::fs::File::open(path) {
            let mut head = [0u8; 14];
            if std::io::Read::read_exact(&mut file, &mut head).is_ok() && i32::from_le_bytes(head[6..10].try_into().unwrap()) >= 6 {
                let key = u32::from_le_bytes(head[10..14].try_into().unwrap());
                if key != 0 {
                    let add: extern "C" fn(u32) = std::mem::transmute(self.acs.va(VA_KN5IO_ADD_DLC_KEY));
                    add(key);
                }
            }
        }
        let load: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_KN5IO_LOAD));
        let node = load(io, wstring(&self.acs, path));
        if node.is_null() {
            return Err(format!("the game's KN5IO::load gave nothing for {path}"));
        }
        Ok(node)
    }

    /// The name of a node of the game.
    pub unsafe fn node_name(&self, node: *const u8) -> String {
        let s = node.add(0xb8);
        let (length, capacity): (usize, usize) = (rd(s, 0x10), rd(s, 0x18));
        let text = if capacity >= 8 { rd::<*const u16>(s, 0) } else { s as *const u16 };
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    }

    /// The children of a node of the game.
    pub unsafe fn children(&self, node: *const u8) -> Vec<*mut u8> {
        let (begin, end): (*const *mut u8, *const *mut u8) = (rd(node, 0x90), rd(node, 0x98));
        if begin.is_null() {
            return Vec::new();
        }
        std::slice::from_raw_parts(begin, end.offset_from(begin) as usize).to_vec()
    }

    /// `TrackAvatar::processPhysicsNode` 0x1401cc5e0, the part the picture sees: every node
    /// whose name starts with `AC_` (spawn points, timing gates, loose objects …) is switched off.
    unsafe fn hide_helpers(&self, node: *mut u8) {
        let name = self.node_name(node);
        if name.starts_with("AC_") {
            // … and TrackAvatar::TrackAvatar switches a loose object's node (AC_POBJECT with a
            // mesh as its first child) on again
            let object = name.starts_with("AC_POBJECT") && self.children(node).first().is_some_and(|c| rd::<usize>(*c, 0) == self.acs.va(VA_MESH_VTABLE));
            wr(node, 0xd8, object as u8);
        }
        for child in self.children(node) {
            self.hide_helpers(child);
        }
    }

    /// The scene of a frame built with the game's own objects, and the frame rendered by the
    /// game's own camera.
    pub unsafe fn render(&self, frame: &Frame, dump: &[usize], time_mult: f32) -> Result<Rendered, String> {
        let acs = &self.acs;
        let begin: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_BEGIN_SCENE));
        let end: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_END_SCENE));
        let screen_space: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_SCREEN_SPACE_MODE));
        // the splash screen: in the game frames are drawn before the Sim exists (the loading
        // screen), so the default state of beginScene is there when the Sim loads and draws
        // its static cube map. One frame with nothing in it stands for them.
        self.hdr_flag(true);
        begin(self.graphics);
        screen_space(self.graphics);
        end(self.graphics);
        self.hdr_flag(false);
        // Sim::initSceneGraph 0x140199d70
        let root = self.node("ROOT");
        let blurred = self.node("BLURRED");
        let unblurred = self.node("UNBLURRED");
        self.add_child(root, blurred);
        self.add_child(root, unblurred);
        let track_node = self.node("TRACK");
        self.add_child(blurred, track_node);
        let skid_marks = self.node("SKIDMARKS");
        self.add_child(blurred, skid_marks);
        let car_shadows = self.node_event("CAR_SHADOWS");
        self.add_child(blurred, car_shadows);
        let before_cars = self.node_event("BEFORE_CARS_NODE");
        self.add_child(unblurred, before_cars);
        // (several cars: the game's own CarNodeSorter; its Sim is set when the first car is loaded)
        let cars = if frame.others.is_empty() {
            self.node("CARS")
        } else {
            let sorter = self.acs.alloc(0xf8);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(crate::ac_car::VA_CAR_NODE_SORTER_CTOR));
            ctor(sorter, wstring(&self.acs, "CARS"), std::ptr::null_mut())
        };
        self.add_child(unblurred, cars);
        let particles = self.node("PARTICLES_NODE");
        self.add_child(unblurred, particles);
        let render_finished = self.node_event("RENDER FINISHED");
        self.add_child(unblurred, render_finished);

        let compile: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_COMPILE));
        // TrackAvatar::init3D 0x1401c8740: one importer, the files of models.ini in order
        let mut track_model = std::ptr::null_mut::<u8>();
        if let Some(track) = &frame.track {
            let model = self.node(&format!("TRACK {}", track.name));
            let io = self.kn5io();
            // Model::load 0x140217d30 adds `<folder of the model>/texture` before every load
            let add_folder: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_KN5IO_ADD_TEXTURE_FOLDER));
            for entry in &track.models {
                add_folder(io, wstring(acs, &format!("{}/texture", rustyac_render::model::get_path(&entry.filename))));
                let top = self.load_kn5(io, &entry.filename)?;
                self.add_child(model, top);
                let matrix: [f32; 16] = rd(top, 8);
                wr(top, 8, crate::frames::placed(&matrix, entry));
            }
            compile(self.graphics, model);
            self.add_child(track_node, model);
            self.hide_helpers(model);
            track_model = model;
        }
        // TrackAvatar::TrackAvatar 0x1401c5250: the game's own DynamicTrackManager (grooves) and
        // CameraFacing (crowds), on a Sim and a TrackAvatar that hold what those two read
        let console = self.prepare_cars();
        let track_sim = acs.alloc(0x2d0);
        let mut grooves = std::ptr::null_mut::<u8>();
        let mut dynamic_track = std::ptr::null_mut::<u8>();
        let mut ideal_line = std::ptr::null_mut::<u8>();
        let mut new_session_track = std::ptr::null_mut::<u8>();
        let mut pit_track = std::ptr::null_mut::<u8>();
        let pit_stop = acs.alloc(0x240);
        wr(pit_stop, 0x120, 4i32);
        let mut start_lights = std::ptr::null_mut::<u8>();
        let mut semaphore = std::ptr::null_mut::<u8>();
        let mut track_game = std::ptr::null_mut::<u8>();
        let track_race_manager = acs.alloc(0x400);
        if let Some(track) = &frame.track {
            let game = acs.alloc(0x278);
            wr(game, 0x138, self.graphics);
            track_game = game;
            wr(track_sim, 0x08, game);
            wr(track_sim, 0x120, root);
            wr(track_sim, 0x128, track_node);
            wr(track_sim, 0x158, blurred);
            wr(track_sim, 0x160, unblurred);
            wr(track_sim, 0x228, console);
            // PhysicsAvatar::engine.track: a Track whose grip does not change
            let physics_avatar = acs.alloc(0x600);
            wr(physics_avatar, 0x58 + 0x198, acs.alloc(0x400));
            // PhysicsAvatar::PhysicsAvatar 0x140120a0c: its queue of jobs for the physics thread
            // (the new-session handler puts one in; nothing ever takes it out)
            {
                let ctor: extern "C" fn(*mut u8, usize) -> *mut u8 = std::mem::transmute(acs.msvcp_function(c"??0_Concurrent_queue_base_v4@details@Concurrency@@IEAA@_K@Z"));
                ctor(physics_avatar.add(0x318), 0x20);
                wr(physics_avatar, 0x318, acs.va(VA_FUNCTION_QUEUE_VTABLE));
            }
            wr(track_sim, 0x1b8, physics_avatar);
            let track_avatar = acs.alloc(0x308);
            wr(track_avatar, 0xe8, track_sim);
            wr(track_avatar, 0x08, game);
            new_session_track = track_avatar;
            // TrackAvatar::TrackAvatar: the pit boxes put on the road by the game's own
            // initRespawnPositionSet, with the port's rays
            if frame.pit_crew {
                wr(track_avatar, 0xf0, track_model);
                wr(track_avatar, 0xf8, ray_table(acs));
                let head: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SPAWN_POSITIONS_HEAD));
                wr(track_avatar, 0x240, head(track_avatar.add(0x240)));
                let init: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_INIT_RESPAWN_POSITION_SET));
                init(track_avatar, wstring(acs, "PIT"));
                wr(track_avatar, 0x58, acs.alloc(0xe0));
                pit_track = track_avatar;
            }
            write_wstring(acs, track_avatar.add(0x200), &track.data_folder);
            wr(track_sim, 0x150, track_avatar);
            // TrackAvatar::initDynamicObjects 0x1401c9840: the game's own, on the files of the
            // scratch root (models.ini and the models its [DYNAMIC_OBJECT_n] sections name)
            {
                let ini_name = if track.layout.is_empty() { "models.ini".to_string() } else { format!("models_{}.ini", track.layout) };
                let from = std::path::Path::new(&track.folder);
                let to = std::path::PathBuf::from(format!("content/tracks/{}", track.name));
                if let Ok(text) = std::fs::read_to_string(from.join(&ini_name)) {
                    if text.contains("[DYNAMIC_OBJECT_") {
                        std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
                        std::fs::write(to.join(&ini_name), &text).map_err(|e| e.to_string())?;
                        let mut in_object = false;
                        for line in text.lines() {
                            let line = line.trim();
                            if line.starts_with('[') {
                                in_object = line.starts_with("[DYNAMIC_OBJECT_");
                            } else if let (true, Some(file)) = (in_object, line.strip_prefix("FILE=")) {
                                let file = file.split(';').next().unwrap_or("").trim();
                                if from.join(file).is_file() {
                                    if !to.join(file).is_file() {
                                        std::fs::copy(from.join(file), to.join(file)).map_err(|e| e.to_string())?;
                                    }
                                    // (a version 6 file's number, as load_kn5 tells it)
                                    let mut head = [0u8; 14];
                                    if let Ok(mut f) = std::fs::File::open(from.join(file)) {
                                        if std::io::Read::read_exact(&mut f, &mut head).is_ok() && i32::from_le_bytes(head[6..10].try_into().unwrap()) >= 6 {
                                            let key = u32::from_le_bytes(head[10..14].try_into().unwrap());
                                            if key != 0 {
                                                let add: extern "C" fn(u32) = std::mem::transmute(acs.va(VA_KN5IO_ADD_DLC_KEY));
                                                add(key);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        wr(track_avatar, 0x08, game);
                        write_wstring(acs, track_avatar.add(0x10), &track.name);
                        write_wstring(acs, track_avatar.add(0xa0), &track.layout);
                        wr(track_avatar, 0xf0, track_model);
                        let init: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_INIT_DYNAMIC_OBJECTS));
                        init(track_avatar);
                        dynamic_track = track_avatar;
                    }
                }
            }
            // TrackAvatar::TrackAvatar: the ideal line, the game's own on its own reader of the
            // file; its rays are the port's
            if let Some(file) = &frame.ideal_line {
                let texture = std::path::Path::new("content/texture/ideal_line.png");
                if !texture.is_file() {
                    std::fs::create_dir_all("content/texture").map_err(|e| e.to_string())?;
                    std::fs::copy(std::path::Path::new(&crate::frames::path_text(&self.game_folder)).join(texture), texture).map_err(|e| format!("ideal_line.png: {e}"))?;
                }
                let spline = acs.alloc(0xe0);
                let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_AI_SPLINE_CTOR));
                ctor(spline);
                let load: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_AI_SPLINE_LOAD_FAST));
                load(spline, wstring(acs, file));
                let line = acs.alloc(0xe8);
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_IDEAL_LINE_CTOR));
                ctor(line, spline, self.graphics, ray_track(acs));
                compile(self.graphics, line);
                self.add_child(track_node, line);
                wr(track_avatar, 0x58, line);
                ideal_line = line;
                if !pit_track.is_null() {
                    wr(pit_track, 0x58, line);
                }
            }
            grooves = acs.alloc(0x90);
            let ctor: extern "C" fn(*mut u8, *mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(VA_DYNAMIC_TRACK_MANAGER_CTOR));
            ctor(grooves, track_sim, wstring(acs, &track.data_folder));
            wr(track_avatar, 0x2e8, grooves);
            let facing = acs.alloc(0x58);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_FACING_CTOR));
            ctor(facing, track_avatar, track_sim);
            // (the constructor ends with srand(tick count): a number both sides have)
            let randomize: extern "C" fn(u32) = std::mem::transmute(acs.va(VA_KS_RANDOMIZE));
            randomize(crate::frames::RAND_SEED_TRACK);
            // Sim::Sim: the game's own StartingLights and StartingTrackSemaphore, on a pause menu
            // that does not show and a RaceManager that holds the session's kind and start time
            if frame.start_lights {
                let texture = std::path::Path::new("content/texture/off.png");
                if !texture.is_file() {
                    std::fs::create_dir_all("content/texture").map_err(|e| e.to_string())?;
                    std::fs::copy(self.game_folder.join(texture), texture).map_err(|e| format!("off.png: {e}"))?;
                }
                wr(track_sim, 0x110, acs.alloc(0x200));
                wr(track_race_manager, 0x8, game);
                wr(track_race_manager, 0x168, track_sim);
                write_wstring(acs, track_race_manager.add(0xe8 + 0x18), "");
                write_wstring(acs, track_race_manager.add(0xe8 + 0x38), "");
                wr(track_sim, 0x1a8, track_race_manager);
                start_lights = acs.alloc(0xf0);
                let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_STARTING_LIGHTS_CTOR));
                ctor(start_lights, track_sim);
                semaphore = acs.alloc(0x88);
                let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SEMAPHORE_CTOR));
                ctor(semaphore, track_sim);
            }
        }

        // TimeAttack::TimeAttack 0x1401b7e20, the gates' part: the game's own render handler on
        // BEFORE_CARS_NODE, on a TimeAttack block that holds the gates of the track's overlays.ini
        // (read by the port's reader) and the texture the game's own store loads
        let mut gates: Vec<*mut u8> = Vec::new();
        let gate_replay = acs.alloc(0x198);
        if let (true, Some(track)) = (frame.time_attack, &frame.track) {
            copy_into_root(&self.game_folder, "content/texture/checkpoint.png")?;
            let overlays = crate::frames::time_attack_overlays(&track.data_folder);
            let block = acs.alloc(0x2e0);
            let list = acs.alloc(overlays.len().max(1) * 0x30);
            for (i, ov) in overlays.iter().enumerate() {
                let p = list.add(i * 0x30);
                wr(p, 0x4, ov.world_position);
                wr(p, 0x10, ov.offset);
                wr(p, 0x1c, ov.height);
                wr(p, 0x20, ov.width);
                wr(p, 0x24, ov.orientation);
                gates.push(p);
            }
            wr(block, 0xc8, list);
            wr(block, 0xd0, list.add(overlays.len() * 0x30));
            let get_texture: extern "C" fn(*mut u8, *mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(acs.va(VA_RESOURCE_STORE_GET_TEXTURE));
            let texture = acs.alloc(0x28);
            get_texture(rd(self.graphics, 0x180), texture, wstring(acs, "content/texture/checkpoint.png"), false);
            std::ptr::copy_nonoverlapping(texture, block.add(0x208), 0x28);
            let gate_car = acs.alloc(0x200);
            wr(gate_car, 0x130, track_sim);
            wr(track_sim, 0x1b0, gate_replay);
            wr(block, 0x240, gate_car);
            // the handler's record in the node's evOnRender (0x28 bytes, the function object at +0x20)
            if rd::<usize>(before_cars, 0xe0) != rd::<usize>(before_cars, 0xe8) {
                return Err("BEFORE_CARS_NODE has a render handler already".into());
            }
            let function = acs.alloc(0x20);
            wr(function, 0x0, acs.va(VA_TIME_ATTACK_RENDER_HANDLER_VTABLE));
            wr(function, 0x8, block);
            let record = acs.alloc(0x28);
            wr(record, 0x0, block);
            wr(record, 0x20, function);
            wr(before_cars, 0xe0, record);
            wr(before_cars, 0xe8, record.add(0x28));
            wr(before_cars, 0xf0, record.add(0x28));
        }
        // Sim::createCamera 0x1401982e0 and what Sim::Sim sets on the camera afterwards
        let profile = crate::root::profile();
        let camera = if profile.post {
            // the game's own post-processing camera, without YEBIS: its start-up returns at once
            // (g_pPfxContext stays null, so every YEBIS call of the frame is skipped by the game's
            // own tests) and applyPostProcessing is the stand-in the port draws too
            acs.patch(acs.va(VA_YEBIS_INIT_POST_PROCESSING), &[0xc3]);
            let mut jump = vec![0x48u8, 0xb8];
            jump.extend_from_slice(&(stand_in_apply_post_processing as *const () as usize).to_le_bytes());
            jump.extend_from_slice(&[0xff, 0xe0]);
            acs.patch(acs.va(VA_YEBIS_APPLY_POST_PROCESSING), &jump);
            let get_shader: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GRAPHICS_GET_SHADER));
            STAND_IN.store(Box::into_raw(Box::new(StandIn {
                graphics: self.graphics,
                shader: get_shader(self.graphics, wstring(acs, "ksPostCopy")),
                no_texture: acs.alloc(0x30),
                set_blend_mode: acs.va(VA_GRAPHICS_SET_BLEND_MODE),
                set_depth_mode: acs.va(VA_GRAPHICS_SET_DEPTH_MODE),
                set_render_targets: acs.va(VA_KGL_SET_RENDER_TARGETS),
                set_viewport: acs.va(VA_KGL_SET_VIEWPORT),
                set_texture_rt: acs.va(VA_KGL_SET_TEXTURE_RT),
                full_screen_quad: acs.va(VA_GL_FULL_SCREEN_QUAD),
            })), std::sync::atomic::Ordering::SeqCst);
            let camera = acs.alloc(SIZE_CAMERA_FORWARD_YEBIS);
            let ctor: extern "C" fn(*mut u8, *const u8, *mut u8, bool, bool, i32) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_FORWARD_YEBIS_CTOR));
            self.hdr_flag(true);
            ctor(camera, wstring(acs, "MAIN_CAMERA_YEBIS"), self.graphics, profile.motion_blur > 0, false, 1);
            self.hdr_flag(false);
            wr(camera, 0x2a0, profile.motion_blur as f32 * 2.0); // blurQuality (Sim::Sim)
            camera
        } else {
            let camera = acs.alloc(SIZE_CAMERA_FORWARD);
            let ctor: extern "C" fn(*mut u8, *const u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_FORWARD_CTOR));
            ctor(camera, wstring(acs, "MAIN_CAMERA"), self.graphics, false);
            camera
        };
        wr(camera, 0x80, [0.3f32, 0.25, 0.25, 1.0]); // clearColor
        wr(camera, 0x1ac, crate::root::profile().world_detail as f32); // maxLayer
        let sky = acs.alloc(SIZE_SKYBOX);
        let sky_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SKYBOX_CTOR));
        sky_ctor(sky, self.graphics);
        wr(camera, 0x98, sky);
        // RaceManager::initLighting 0x14013a3d0: race.ini [LIGHTING] SUN_ANGLE (no SunAnimator
        // runs here: the frame is a still, the clock of the lighting stays 0)
        let update_lighting: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_UPDATE_LIGHTING));
        wr(self.graphics, 0xd4, frame.sun_angle);
        update_lighting(self.graphics);
        // TrackAvatar::TrackAvatar 0x1401c5250: the track's data/lighting.ini
        if let Some(track) = &frame.track {
            if let (Some(pitch), Some(heading)) = (track.sun_pitch, track.sun_heading) {
                wr(self.graphics, 0xdc, pitch);
                wr(self.graphics, 0xd8, heading);
                update_lighting(self.graphics);
            }
        }
        // Sim::applyCustomWeather 0x140198010 / WeatherManager::applyCustomWeather 0x1401d8110
        if !frame.weather.is_empty() {
            let update_clouds: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_SKYBOX_UPDATE_CLOUDS));
            update_clouds(sky, wstring(acs, &frame.weather));
            let weather_ini = format!("content/weather/{}/weather.ini", frame.weather);
            let curves_ini = format!("content/weather/{}/colorCurves.ini", frame.weather);
            let mut m = 1.0f32;
            // (WeatherManager::applyCustomWeather reads it only with post-processing off)
            if !profile.post || profile.ldr_levels {
                if let Ok(c) = rustyac_physics::data::ini::IniReader::load(std::path::Path::new(&curves_ini)) {
                    m = c.get_float("HEADER", "HDR_OFF_MULT").unwrap_or(0.0);
                }
            }
            if std::path::Path::new(&weather_ini).is_file() {
                let load_preset: extern "C" fn(*const u8, *mut u8, f32) -> bool = std::mem::transmute(acs.va(VA_WEATHER_LOAD_PRESET));
                load_preset(wstring(acs, &weather_ini), self.graphics, m);
            }
            if std::path::Path::new(&curves_ini).is_file() {
                let load_lighting: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_GRAPHICS_LOAD_LIGHTING));
                load_lighting(self.graphics, wstring(acs, &curves_ini));
            }
        }
        wr(camera, 0x70, 0.05f32); // nearPlane
        wr(camera, 0x74, 40000.0f32); // farPlane
        // Sim::initCubemaps 0x1401997a0 with the profile's [CUBEMAP]
        wr(camera, 0x2b8, crate::root::profile().cubemap_faces_per_frame); // cubeMapRenderer.facesPerFrame
        if crate::root::profile().cubemap_far_plane != 0.0 {
            let set_planes: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(acs.va(VA_CUBE_MAP_RENDERER_SET_PLANES));
            set_planes(camera.add(0x2b0), f32::from_bits(0x3c23_d70a), crate::root::profile().cubemap_far_plane);
        }
        let set_cubemap_size: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_CAMERA_FORWARD_SET_CUBEMAP_SIZE));
        set_cubemap_size(camera, crate::root::profile().cubemap_size);
        // … and its two handlers on the cube map's events (records of 0x28 bytes, the function
        // object at +0x20): the game's own, which hide the grooves while the faces are drawn
        if !grooves.is_null() {
            for (event, handler) in [(0x10usize, VA_GROOVE_HIDE_HANDLER), (0x28, VA_GROOVE_SHOW_HANDLER)] {
                let vtable = acs.alloc(0x40);
                wr(vtable, 0x10, acs.va(handler));
                let object = acs.alloc(0x20);
                wr(object, 0x0, vtable);
                wr(object, 0x8, track_sim);
                let record = acs.alloc(0x28);
                wr(record, 0x20, object);
                wr(camera, 0x2b0 + event, record);
                wr(camera, 0x2b0 + event + 8, record.add(0x28));
                wr(camera, 0x2b0 + event + 0x10, record.add(0x28));
            }
        }
        // Sim::addCar: the car's models and its objects
        let sim_nodes = crate::ac_car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished, blurred, unblurred, ideal_line, track_avatar: pit_track, pit_stop };
        let count = 1 + frame.others.len();
        if count > 1 {
            acs.set_global::<i32>(crate::ac_car::VA_CARS_TO_BE_LOADED, frame.cars_to_be_loaded);
        }
        let car = match &frame.car {
            Some(spec) => Some(self.load_car(spec, &sim_nodes, camera, console, None, 0, count)?),
            None => None,
        };
        // the cars after the first, into the same Sim
        let mut others: Vec<crate::ac_car::Car> = Vec::new();
        if let Some(first) = &car {
            for (i, spec) in frame.others.iter().enumerate() {
                others.push(self.load_car(spec, &sim_nodes, camera, console, Some(first), i + 1, count)?);
            }
        }
        // Task 43c: FormWings::FormWings (Sim::Sim, after the cars)
        if let (Some(c), true) = (&car, frame.steps.iter().any(|s| s.extra.wings_app)) {
            self.make_wings_form(c, render_finished);
        }
        // Sim::cars, for the grooves
        if let Some(car) = &car {
            let list = acs.alloc(8 * count);
            wr(list, 0, car.avatar());
            for (i, other) in others.iter().enumerate() {
                wr(list, 8 * (i + 1), other.avatar());
            }
            wr(track_sim, 0x208, list);
            wr(track_sim, 0x210, list.add(8 * count));
        }
        // Sim::Sim: TrackAvatar::initPitCrew (the lollipop men, the track's crew meshes), the
        // game's own; then the pit box marker of PitStop::PitStop, its model by hand
        let mut lollipops: Vec<*mut u8> = Vec::new();
        let mut pit_marker: Option<(*mut u8, rustyac_render::scene::Scene, rustyac_render::pit_crew::PitIndicator)> = None;
        if let (false, Some(spec)) = (pit_track.is_null(), &frame.car) {
            for file in ["pitcrew.kn5", "pitcrew.ksanim", "pitcrew_idle_up.ksanim", "pitcrew_idle_dw.ksanim", "pitcrew_Left.kn5", "pitcrew_Left.ksanim", "pitcrew_Left_idle_up.ksanim", "pitcrew_Left_idle_dw.ksanim"] {
                copy_into_root(&self.game_folder, &format!("content/objects3D/{file}"))?;
            }
            if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&self.game_folder.join(format!("content/cars/{}/skins/{}/skin.ini", spec.name, spec.skin))) {
                for key in ["SUIT", "HELMET", "BRAND"] {
                    let folder = format!("content/texture/crew_{key}{}", ini.get_string("CREW", key).replace('\\', "/"));
                    copy_into_root(&self.game_folder, &folder)?;
                    if let Some(at) = folder.rfind('/') {
                        copy_into_root(&self.game_folder, &format!("{}/_nm", &folder[..at]))?;
                    }
                }
            }
            wr(track_sim, 0x1d0, pit_stop);
            let init: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_INIT_PIT_CREW));
            init(pit_track);
            let (begin, end): (*const *mut u8, *const *mut u8) = (rd(pit_track, 0x2a0), rd(pit_track, 0x2a8));
            if !begin.is_null() {
                lollipops = std::slice::from_raw_parts(begin, end.offset_from(begin) as usize).to_vec();
            }
            // the marker at the player's pit box (the node as initRespawnPositionSet left it)
            let vtable = rd::<*const usize>(track_model, 0);
            let find: extern "C" fn(*mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(*vtable.add(5));
            let pit_node = find(track_model, wstring(acs, "AC_PIT_0"), true);
            if !pit_node.is_null() {
                let marker = self.node("PIT_INDICATOR");
                let io = self.kn5io();
                let file = format!("{}/content/objects3D/pit_indicator.kn5", crate::frames::path_text(&self.game_folder));
                let add_folder: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_KN5IO_ADD_TEXTURE_FOLDER));
                add_folder(io, wstring(acs, &format!("{}/texture", rustyac_render::model::get_path(&file))));
                let top = self.load_kn5(io, &file)?;
                self.add_child(marker, top);
                compile(self.graphics, marker);
                self.add_child(track_node, marker);
                let matrix: [f32; 16] = rd(pit_node, 8);
                wr(marker, 8, matrix);
                // its rules are the port's (PitStop::update needs the game's whole pit menu)
                let mut rules_scene = rustyac_render::scene::Scene::new();
                let rules_node = rules_scene.node("PIT_INDICATOR");
                let rules = rustyac_render::pit_crew::PitIndicator::rules_only(rules_node, [matrix[12], matrix[13], matrix[14]], true);
                pit_marker = Some((marker, rules_scene, rules));
            }
        }
        // RaceManager::initGhostcar: the ghost car of the player's car, the game's own
        let mut ghost = std::ptr::null_mut::<u8>();
        if let (Some(recorder), Some(car)) = (&frame.ghost, &car) {
            copy_into_root(&self.game_folder, "system/cfg/ghost_car.ini")?;
            ghost = self.make_ghost(car, recorder);
        }
        // PitStop::PitStop: the tyre changers of the player's car, the game's own
        let mut tyre_crew = std::ptr::null_mut::<u8>();
        let mut crew_began = false;
        if let (true, Some(spec), Some(car)) = (frame.tyre_crew, &frame.car, &car) {
            for file in ["pitcrewtyre.kn5", "pitcrewtyre.ksanim", "pitcrewtyre.ini"] {
                copy_into_root(&self.game_folder, &format!("content/objects3D/{file}"))?;
            }
            tyre_crew = self.make_pit_crew(car, &format!("content/cars/{}/skins/{}", spec.name, spec.skin), pit_stop, blurred, crate::frames::tyre_change_time_sec(spec));
        }

        let cube_log;
        // Sim::initStaticCubemap 0x14019a2a0, from Sim::onPostLoad: the small model and the sky
        // into the six faces, once, with the scene camera where its constructor left it
        {
            let io = self.kn5io();
            let model = self.load_kn5(io, &frame.cubemap_model)?;
            compile(self.graphics, model);
            let saved: i32 = rd(camera, 0x2b8);
            wr(camera, 0x2b8, 6i32);
            let render: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) = std::mem::transmute(acs.va(VA_CUBE_MAP_RENDERER_RENDER));
            rustyac_render::gpulog::begin_capture("static cube map");
            render(camera.add(0x2b0), rd::<*mut u8>(camera, 0x690), model, camera);
            cube_log = rustyac_render::gpulog::end_capture().text;
            wr(camera, 0x2b8, saved);
        }
        // Sim::onPostLoad, after the cube map: CarAvatar::onPostLoad
        let mut cube_log = cube_log;
        if let (Some(car), Some(spec)) = (&car, &frame.car) {
            rustyac_render::gpulog::begin_capture("car shadows");
            let lines = self.post_load_car(car, &spec.name);
            cube_log.extend_from_slice(&rustyac_render::gpulog::end_capture().text);
            cube_log.extend_from_slice(&lines?);
            for (other, spec) in others.iter().zip(&frame.others) {
                rustyac_render::gpulog::begin_capture("car shadows");
                let lines = self.post_load_car(other, &spec.name);
                cube_log.extend_from_slice(&rustyac_render::gpulog::end_capture().text);
                cube_log.extend_from_slice(&lines?);
            }
        }

        let traverse: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_WORLD_MATRIX_TRAVERSE));
        let set_splits: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(acs.va(VA_CAMERA_SET_SHADOW_MAPS_SPLITS));
        let traverser = acs.alloc(0x40);
        let mut rendered = Rendered { cube_log, frames: Vec::new(), width: 0, height: 0 };
        let (mut lights_pause, mut lights_status) = (false, -1);
        let mut told_new_session = 0;
        for (index, step) in frame.steps.iter().enumerate() {
            *crate::ac_car::WINGS_FRAME.lock().unwrap() = index;
            // RaceManager::setCurrentSession 0x140140df0: Sim::evOnNewSession, the handlers in the
            // order they were registered: the track's own, then the car's objects'
            if step.extra.new_session != told_new_session {
                told_new_session = step.extra.new_session;
                let event = acs.alloc(0x78);
                if !new_session_track.is_null() {
                    wr(track_game, 0x18, crate::frames::game_time_ms(index));
                    let mut captured = new_session_track;
                    let handler: extern "C" fn(*mut *mut u8, *const u8) = std::mem::transmute(acs.va(VA_TRACK_NEW_SESSION_HANDLER));
                    handler(&mut captured, event);
                }
                if let Some(car) = &car {
                    self.fire_new_session(car, event);
                }
            }
            // the session's clock, and the events the start lights listen to
            if !start_lights.is_null() {
                wr(track_game, 0x18, crate::frames::game_time_ms(index));
                wr(track_race_manager, 0xe8, step.extra.session_type);
                wr(track_race_manager, 0x140, crate::frames::session_start(index, step.extra.tts).0);
                wr(rd::<*mut u8>(track_sim, 0x110), 0x150, step.extra.pause as u8);
                let fire = |event: usize, payload: *mut u8| {
                    let (mut at, end): (*mut u8, *mut u8) = (rd(track_sim, event), rd(track_sim, event + 8));
                    while at < end {
                        let object: *mut u8 = rd(at, 0x20);
                        let vtable: *const usize = rd(object, 0);
                        let call: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(2));
                        call(object, payload);
                        at = at.add(0x28);
                    }
                };
                if step.extra.pause != lights_pause {
                    lights_pause = step.extra.pause;
                    let mut paused = lights_pause as u8;
                    fire(0x90, &mut paused);
                }
                if step.extra.replay_status != lights_status {
                    lights_status = step.extra.replay_status;
                    let mut payload = [0u8; 12];
                    payload[0..4].copy_from_slice(&lights_status.to_le_bytes());
                    payload[4..8].copy_from_slice(&step.extra.replay_scale.to_le_bytes());
                    payload[8..12].copy_from_slice(&2.0f32.to_le_bytes());
                    fire(0x78, payload.as_mut_ptr());
                }
                // Game::update: StartingTrackSemaphore::update
                let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SEMAPHORE_UPDATE));
                update(semaphore, crate::frames::DT);
            }
            if let (Some(car), true) = (&car, frame.fade_at == Some(index)) {
                self.fade_in(car, crate::frames::camera_fade_time());
            }
            // the game camera's update: where it looks from, its lens and the shadow splits
            wr(camera, 0x8, step.camera.fov);
            wr(camera, 0xc, step.camera.matrix);
            wr(camera, 0x70, step.camera.near);
            if let Some(far) = step.camera.far {
                wr(camera, 0x74, far);
            }
            let s = step.camera.splits;
            set_splits(camera, s[0], s[1], s[2], s[3]);
            // the gates are not drawn in a replay
            wr(gate_replay, 0x148, step.extra.replay as u8);
            // DrivingAssistManager::update 0x1400fcb20
            if !ideal_line.is_null() {
                wr(ideal_line, 0xd8, step.extra.ideal as u8);
            }
            // TrackAvatar::update 0x1401cca10: updateDynamicObjects
            if !dynamic_track.is_null() {
                let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE_DYNAMIC_OBJECTS));
                update(dynamic_track, crate::frames::DT);
            }
            // SunAnimator::update 0x1401adc10, the clouds' part
            {
                let animate: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SKYBOX_UPDATE_CLOUDS_ANIMATION));
                animate(sky, crate::frames::DT * time_mult);
            }
            // Game::update: the track's children first: LollipopCrew::update
            wr(pit_stop, 0x120, step.extra.pit_state);
            for &crew in &lollipops {
                let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_LOLLIPOP_CREW_UPDATE));
                update(crew, crate::frames::DT);
            }
            // Game::update: the car's objects, then the handlers of evOnPostUpdate
            if let Some(car) = &car {
                // Sim::update: CarNodeSorter::sort, then the cars in the order they were made
                if count > 1 {
                    self.sort_cars(car, cars, &step.camera);
                }
                self.update_car(car, &step.state, &step.camera, &step.extra, crate::frames::DT, crate::frames::game_time_ms(index));
                // (the car's children after it: GhostCar::update)
                if !ghost.is_null() {
                    self.update_ghost(ghost, crate::frames::DT);
                }
                for (other, state) in others.iter().zip(&step.others) {
                    self.update_car(other, state, &step.camera, &step.extra, crate::frames::DT, crate::frames::game_time_ms(index));
                }
                self.post_update_car(car, crate::frames::DT);
                for other in &others {
                    self.post_update_car(other, crate::frames::DT);
                }
            }
            // the pit stop's handler of evOnPostUpdate: PitCrew::pitCrewUpdate; a stop that was
            // committed begins here (the camera fade that leads to it is not part of the frame)
            if let (Some(car), false) = (&car, tyre_crew.is_null()) {
                wr(pit_stop, 0xa4, step.extra.crew);
                wr(pit_stop, 0xac, step.extra.crew);
                wr(pit_stop, 0xbc, step.extra.crew);
                if step.extra.crew != 0.0 && !crew_began {
                    crew_began = true;
                    self.pit_crew_begin(tyre_crew, step.extra.crew, step.extra.crew);
                }
                let _ = car;
                self.pit_crew_update(tyre_crew, crate::frames::DT);
            }
            // PitStop::update (after the cars): the marker's rules; the number Car::getPitstopTime
            // draws comes from the game's own rand()
            if let (Some((marker, rules_scene, rules)), Some(car)) = (&mut pit_marker, &car) {
                let inputs = crate::frames::pit_inputs(step, car.body_position());
                let rand: extern "C" fn() -> i32 = std::mem::transmute(acs.crt_function(c"rand"));
                rules.update(rules_scene, &mut || {
                    rand();
                }, &inputs);
                wr(*marker, 0xd8, rules_scene.nodes[rules.node].is_active as u8);
            }
            // DynamicTrackManager::update 0x1401cdba0 (Game::update skips an object that is not active)
            if !grooves.is_null() && rd::<u8>(grooves, 0x30) != 0 {
                let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_DYNAMIC_TRACK_MANAGER_UPDATE));
                update(grooves, crate::frames::DT);
            }

            if index >= frame.capture && !crate::timing() {
                rustyac_render::gpulog::begin_capture(&frame.name);
            }
            // Game::onIdle 0x140242730
            let started = std::time::Instant::now();
            self.hdr_flag(true);
            begin(self.graphics);
            self.hdr_flag(false);
            // Sim::renderScene 0x14019e570
            traverse(traverser, root);
            if let Some(car) = &car {
                self.render_mirror(car, &step.camera, 1.0 / 60.0);
            }
            let vtable = rd::<*const usize>(camera, 0);
            let render: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8, f32) = std::mem::transmute(*vtable.add(7));
            self.hdr_flag(true);
            render(camera, blurred, unblurred, root, 1.0 / 60.0);
            screen_space(self.graphics);
            self.hdr_flag(false);
            if let Some(car) = &car {
                self.render_virtual_mirror(car, 1.0 / 60.0);
            }
            // ksgui::GUI::render -> doRender2D 0x1402539d0 (no form shows): the states of the 2D pass
            {
                let reset: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_RESET_RENDER_STATES));
                reset(self.graphics);
                let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_DEPTH_MODE));
                set_mode(self.graphics, 2);
                let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_BLEND_MODE));
                set_mode(self.graphics, 1);
            }
            // Game::renderHUD: TimeAttack::renderGuiStatus, the gates' flags
            for (i, gate) in gates.iter().enumerate() {
                let active = step.extra.gate >= 0 && i == (step.extra.gate as usize + 1) % gates.len();
                wr(*gate, 0, active as u8);
            }
            // Game::renderHUD: PitStop::renderHUD, the marker's part
            if let (Some((marker, rules_scene, rules)), Some(car)) = (&mut pit_marker, &car) {
                let inputs = crate::frames::pit_inputs(step, car.body_position());
                rules.render_hud(rules_scene, &inputs);
                wr(*marker, 0xd8, rules_scene.nodes[rules.node].is_active as u8);
            }
            // Game::renderHUD 0x140242e30, after the scene: StartingLights::renderHUD (an object
            // that is not active is left out)
            if !start_lights.is_null() && rd::<u8>(start_lights, 0x30) != 0 {
                let render_hud: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_STARTING_LIGHTS_RENDER_HUD));
                render_hud(start_lights, crate::frames::DT);
            }
            // Game::evOnPostGui: the camera manager's fade (the game's own handler)
            if let (Some(car), Some(_)) = (&car, frame.fade_at) {
                self.post_gui_fade(car, crate::frames::DT);
            }
            if index >= frame.capture {
                // (a timing run keeps no log: the logger costs far more than the frame)
                let (text, draws) = if crate::timing() {
                    (Vec::new(), 0)
                } else {
                    let capture = rustyac_render::gpulog::end_capture();
                    (capture.text, capture.draws)
                };
                let (width, height, pixels) = self.read_back()?;
                crate::FRAME_MS.lock().unwrap().push(started.elapsed().as_secs_f64() * 1000.0);
                rendered.width = width;
                rendered.height = height;
                rendered.frames.push(Captured::new(index, text, draws, pixels, !frame.sequence || dump.contains(&index)));
            }
            end(self.graphics);
        }
        Ok(rendered)
    }

    /// The swap chain's back buffer, as RGBA rows.
    pub unsafe fn read_back(&self) -> Result<(u32, u32, Vec<u8>), String> {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct3D11::*;
        use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
        let chain = IDXGISwapChain::from_raw_borrowed(&self.swap_chain).ok_or("no swap chain")?;
        let device = ID3D11Device::from_raw_borrowed(&self.device).ok_or("no device")?;
        let context = ID3D11DeviceContext::from_raw_borrowed(&self.context).ok_or("no context")?;
        let back: ID3D11Texture2D = chain.GetBuffer(0).map_err(|e| e.to_string())?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        back.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        // a multisampled back buffer is resolved first, as Present does
        let samples = desc.SampleDesc.Count;
        desc.SampleDesc.Count = 1;
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(|e| e.to_string())?;
        let staging = staging.ok_or("no staging texture")?;
        if samples > 1 {
            let mut plain = desc;
            plain.Usage = D3D11_USAGE_DEFAULT;
            plain.CPUAccessFlags = 0;
            let mut resolved = None;
            device.CreateTexture2D(&plain, None, Some(&mut resolved)).map_err(|e| e.to_string())?;
            let resolved = resolved.ok_or("no resolve texture")?;
            context.ResolveSubresource(&resolved, 0, &back, 0, desc.Format);
            context.CopyResource(&staging, &resolved);
        } else {
            context.CopyResource(&staging, &back);
        }
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
}
