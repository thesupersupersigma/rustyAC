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

/// Start-up initialisers of static objects the renderer uses (the program's entry point, which
/// would run them all, is never run): kgl's map of input layouts, `Curve::openedFiles`,
/// `KN5IO::dlc_keys`.
const VA_STATIC_INITIALISERS: [usize; 3] = [0x1_4000_1040, 0x1_4001_0f90, 0x1_4001_0fa0];

const SIZE_GRAPHICS_MANAGER: usize = 0x4e0;
const SIZE_NODE: usize = 0xe0;
const SIZE_KN5IO: usize = 0xc0;
const SIZE_CAMERA_FORWARD: usize = 0x788;
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
    let result = real(std::ptr::null_mut(), D3D_DRIVER_TYPE_WARP, software, flags, levels, level_count, sdk, swap_chain_desc, swap_chain, device, level, context);
    if result >= 0 {
        DEVICE.store(*device as usize, Ordering::SeqCst);
        CONTEXT.store(*context as usize, Ordering::SeqCst);
        if !swap_chain.is_null() {
            SWAP_CHAIN.store(*swap_chain as usize, Ordering::SeqCst);
        }
        // the game gets the logging stand-in of the context; the oracle keeps the real one
        *context = rustyac_render::gpulog::install(*device, *context).expect("the command log");
    } else {
        eprintln!("oracle: D3D11CreateDeviceAndSwapChain on WARP failed: {result:#x}");
    }
    result
}

type CreateDevice = unsafe extern "system" fn(*mut c_void, i32, *mut c_void, u32, *const i32, u32, u32, *mut *mut c_void, *mut i32, *mut *mut c_void) -> i32;

/// In place of `D3D11CreateDevice` (the game's throw-away first device, made to learn the
/// feature level): on WARP as well, so that no graphics card is touched.
unsafe extern "system" fn create_device(_adapter: *mut c_void, _driver_type: i32, software: *mut c_void, flags: u32, levels: *const i32, level_count: u32, sdk: u32, device: *mut *mut c_void, level: *mut i32, context: *mut *mut c_void) -> i32 {
    const D3D_DRIVER_TYPE_WARP: i32 = 5;
    let module = LoadLibraryW(w!("d3d11.dll")).expect("d3d11.dll");
    let real: CreateDevice = std::mem::transmute(GetProcAddress(module, windows::core::s!("D3D11CreateDevice")).expect("D3D11CreateDevice"));
    real(std::ptr::null_mut(), D3D_DRIVER_TYPE_WARP, software, flags, levels, level_count, sdk, device, level, context)
}

/// In place of `SHGetFolderPathW` (the game asks for the Documents folder): a folder of the
/// oracle's scratch root, so that nothing of the user's own settings is read.
unsafe extern "system" fn sh_get_folder_path(_window: *mut c_void, _folder: i32, _token: *mut c_void, _flags: u32, path: *mut u16) -> i32 {
    let documents = crate::root::documents();
    let units: Vec<u16> = documents.to_string_lossy().encode_utf16().chain([0]).collect();
    std::ptr::copy_nonoverlapping(units.as_ptr(), path, units.len().min(260));
    0
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
        crate::acs::set_extra_bound(vec!["d3d11.dll", "dxgi.dll", "d3dcompiler_43.dll", "d3dx11_43.dll"]);
        crate::acs::set_extra_overrides(vec![
            ("d3d11.dll", "D3D11CreateDeviceAndSwapChain", create_device_and_swap_chain as *const () as usize),
            ("d3d11.dll", "D3D11CreateDevice", create_device as *const () as usize),
            ("shell32.dll", "SHGetFolderPathW", sh_get_folder_path as *const () as usize),
        ]);
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
            acs.patch(acs.va(VA_KGL_INIT_FONTS), &[0xc3]);
            for va in VA_STATIC_INITIALISERS {
                let init: extern "C" fn() = std::mem::transmute(acs.va(va));
                init();
            }

            let window = hidden_window(args.width, args.height)?;
            // VideoSettings (0x50 bytes), as loadVideoSettings fills it from the profile's video.ini
            let settings = acs.alloc(0x50);
            wr(settings, 0x00, 1i32); // aaSamples
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
            Ok(Game {
                acs,
                graphics,
                device: DEVICE.load(Ordering::SeqCst) as *mut c_void,
                context: CONTEXT.load(Ordering::SeqCst) as *mut c_void,
                swap_chain: SWAP_CHAIN.load(Ordering::SeqCst) as *mut c_void,
                init_log: init.text,
            })
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
        if self.node_name(node).starts_with("AC_") {
            wr(node, 0xd8, 0u8);
        }
        for child in self.children(node) {
            self.hide_helpers(child);
        }
    }

    /// The scene of a frame built with the game's own objects, and the frame rendered by the
    /// game's own camera.
    pub unsafe fn render(&self, frame: &Frame, dump: &[usize]) -> Result<Rendered, String> {
        let acs = &self.acs;
        let begin: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_BEGIN_SCENE));
        let end: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_END_SCENE));
        let screen_space: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_SCREEN_SPACE_MODE));
        // the splash screen: in the game frames are drawn before the Sim exists (the loading
        // screen), so the default state of beginScene is there when the Sim loads and draws
        // its static cube map. One frame with nothing in it stands for them.
        begin(self.graphics);
        screen_space(self.graphics);
        end(self.graphics);
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
        let cars = self.node("CARS");
        self.add_child(unblurred, cars);
        let particles = self.node("PARTICLES_NODE");
        self.add_child(unblurred, particles);
        let render_finished = self.node_event("RENDER FINISHED");
        self.add_child(unblurred, render_finished);

        let compile: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_COMPILE));
        // TrackAvatar::init3D 0x1401c8740: one importer, the files of models.ini in order
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
        }

        // Sim::createCamera 0x1401982e0 and what Sim::Sim sets on the camera afterwards
        let camera = acs.alloc(SIZE_CAMERA_FORWARD);
        let ctor: extern "C" fn(*mut u8, *const u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_FORWARD_CTOR));
        ctor(camera, wstring(acs, "MAIN_CAMERA"), self.graphics, false);
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
            if let Ok(c) = rustyac_physics::data::ini::IniReader::load(std::path::Path::new(&curves_ini)) {
                m = c.get_float("HEADER", "HDR_OFF_MULT").unwrap_or(0.0);
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
        // Sim::addCar: the car's models and its objects
        let car = match &frame.car {
            Some(spec) => Some(self.load_car(spec, &crate::ac_car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished, blurred, unblurred }, camera)?),
            None => None,
        };

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

        let traverse: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_WORLD_MATRIX_TRAVERSE));
        let set_splits: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(acs.va(VA_CAMERA_SET_SHADOW_MAPS_SPLITS));
        let traverser = acs.alloc(0x40);
        let mut rendered = Rendered { cube_log, frames: Vec::new(), width: 0, height: 0 };
        for (index, step) in frame.steps.iter().enumerate() {
            // the game camera's update: where it looks from, its lens and the shadow splits
            wr(camera, 0x8, step.camera.fov);
            wr(camera, 0xc, step.camera.matrix);
            wr(camera, 0x70, step.camera.near);
            if let Some(far) = step.camera.far {
                wr(camera, 0x74, far);
            }
            let s = step.camera.splits;
            set_splits(camera, s[0], s[1], s[2], s[3]);
            // Game::update: the car's objects, then the handlers of evOnPostUpdate
            if let Some(car) = &car {
                self.update_car(car, &step.state, &step.camera, crate::frames::DT, crate::frames::game_time_ms(index));
            }

            if index >= frame.capture {
                rustyac_render::gpulog::begin_capture(&frame.name);
            }
            // Game::onIdle 0x140242730
            begin(self.graphics);
            // Sim::renderScene 0x14019e570
            traverse(traverser, root);
            let vtable = rd::<*const usize>(camera, 0);
            let render: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8, f32) = std::mem::transmute(*vtable.add(7));
            render(camera, blurred, unblurred, root, 1.0 / 60.0);
            screen_space(self.graphics);
            if index >= frame.capture {
                let capture = rustyac_render::gpulog::end_capture();
                let (width, height, pixels) = self.read_back()?;
                rendered.width = width;
                rendered.height = height;
                rendered.frames.push(Captured::new(index, capture.text, capture.draws, pixels, !frame.sequence || dump.contains(&index)));
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
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(|e| e.to_string())?;
        let staging = staging.ok_or("no staging texture")?;
        context.CopyResource(&staging, &back);
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
