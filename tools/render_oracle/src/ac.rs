// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side: acs.exe's own `GraphicsManager` (and with it kgl, the state objects, the
//! `ShaderManager`), built by address on a WARP device with a window that is never shown.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP};

use crate::acs::Acs;
use crate::Args;

const VA_GRAPHICS_MANAGER_CTOR: usize = 0x1_4020_1620; // GraphicsManager::GraphicsManager(const VideoSettings&)
const VA_KGL_INIT_FONTS: usize = 0x1_4001_8210; // kglInitFonts (FW1FontWrapper + DirectWrite: not part of the 3D frame)
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized
const VA_ATEXIT: usize = 0x1_4039_9754; // atexit (the program's own, statically linked)

const VA_GRAPHICS_BEGIN_SCENE: usize = 0x1_4020_2580; // GraphicsManager::beginScene()
const VA_GRAPHICS_END_SCENE: usize = 0x1_4020_2850; // GraphicsManager::endScene()
const VA_NODE_CTOR: usize = 0x1_4020_db10; // Node::Node(const std::wstring&)
const VA_KN5IO_CTOR: usize = 0x1_4021_45a0; // KN5IO::KN5IO(GraphicsManager*)
const VA_KN5IO_ADD_DLC_KEY: usize = 0x1_4021_4de0; // static void KN5IO::addDLCKey(int)
const VA_KN5IO_LOAD: usize = 0x1_4021_51a0; // Node* KN5IO::load(const std::wstring&)
const VA_CAMERA_FORWARD_CTOR: usize = 0x1_4021_f230; // CameraForward::CameraForward(const std::wstring&, GraphicsManager*, bool)
const VA_SKYBOX_CTOR: usize = 0x1_4021_c790; // SkyBox::SkyBox(GraphicsManager*)
const VA_WORLD_MATRIX_TRAVERSE: usize = 0x1_4021_abf0; // WorldMatrixTraverser::traverse(Node*)

/// Start-up initialisers of static objects the renderer uses (the program's entry point, which
/// would run them all, is never run): kgl's map of input layouts, `Curve::openedFiles`,
/// `KN5IO::dlc_keys`.
const VA_STATIC_INITIALISERS: [usize; 3] = [0x1_4000_1040, 0x1_4001_0f90, 0x1_4001_0fa0];

const SIZE_GRAPHICS_MANAGER: usize = 0x4e0;
const SIZE_NODE: usize = 0xe0;
const SIZE_KN5IO: usize = 0xc0;
const SIZE_CAMERA_FORWARD: usize = 0x788;
const SIZE_SKYBOX: usize = 0x80;

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

pub unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    std::ptr::write_unaligned(base.add(offset) as *mut T, value);
}

#[allow(dead_code)]
pub unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    std::ptr::read_unaligned(base.add(offset) as *const T)
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
) -> i32 {
    const D3D_DRIVER_TYPE_WARP: i32 = 5;
    let module = LoadLibraryW(w!("d3d11.dll")).expect("d3d11.dll");
    let real: CreateDeviceAndSwapChain = std::mem::transmute(GetProcAddress(module, windows::core::s!("D3D11CreateDeviceAndSwapChain")).expect("D3D11CreateDeviceAndSwapChain"));
    let wanted: Vec<i32> = if levels.is_null() { Vec::new() } else { std::slice::from_raw_parts(levels, level_count as usize).to_vec() };
    eprintln!("oracle: the game asks for a device: driver type {driver_type}, flags {flags:#x}, feature levels {wanted:x?}, sdk {sdk}");
    if !swap_chain_desc.is_null() {
        let words = std::slice::from_raw_parts(swap_chain_desc as *const u32, 18);
        eprintln!("oracle: its DXGI_SWAP_CHAIN_DESC: {words:x?}");
    }
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

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

/// A window that is never shown: the swap chain needs one.
unsafe fn hidden_window(width: u32, height: u32) -> Result<HWND, String> {
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
}

impl Game {
    pub fn start(args: &Args) -> Result<Game, String> {
        crate::acs::set_extra_bound(vec!["d3d11.dll", "dxgi.dll", "d3dcompiler_43.dll", "d3dx11_43.dll"]);
        crate::acs::set_extra_overrides(vec![("d3d11.dll", "D3D11CreateDeviceAndSwapChain", create_device_and_swap_chain as *const () as usize)]);
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
            // VideoSettings (0x50 bytes), as Game::Game fills it from cfg/video.ini
            let settings = acs.alloc(0x50);
            wr(settings, 0x00, 1i32); // aaSamples
            wr(settings, 0x04, args.width as i32);
            wr(settings, 0x08, args.height as i32);
            wr(settings, 0x10, window.0 as usize);
            wr(settings, 0x18, 0u8); // isFullscreen
            wr(settings, 0x19, 0u8); // vSync
            wr(settings, 0x1c, 8i32); // anisotropic
            wr(settings, 0x20, 0i32); // aaQuality
            wr(settings, 0x24, 2048i32); // shadowMapSize
            wr(settings, 0x28, 0f64); // fpsCapMS
            wr(settings, 0x30, 0i32); // dxgiModeIndex
            wr(settings, 0x34, 5i32); // worldDetail
            wr(settings, 0x3c, 0i32); // ppQuality
            wr(settings, 0x40, 0i32); // ppGlare
            wr(settings, 0x44, 0i32); // ppDof
            wr(settings, 0x48, 0u8); // tripleBuffer
            wr(settings, 0x4c, 60f32); // refresh

            let graphics = acs.alloc(SIZE_GRAPHICS_MANAGER);
            let ctor: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GRAPHICS_MANAGER_CTOR));
            ctor(graphics, settings);
            Ok(Game {
                acs,
                graphics,
                device: DEVICE.load(Ordering::SeqCst) as *mut c_void,
                context: CONTEXT.load(Ordering::SeqCst) as *mut c_void,
                swap_chain: SWAP_CHAIN.load(Ordering::SeqCst) as *mut c_void,
            })
        }
    }
}

impl Game {
    pub unsafe fn node(&self, name: &str) -> *mut u8 {
        let node = self.acs.alloc(SIZE_NODE);
        let ctor: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_NODE_CTOR));
        ctor(node, wstring(&self.acs, name))
    }

    /// `parent->addChild(child)` (virtual, slot +8).
    pub unsafe fn add_child(&self, parent: *mut u8, child: *mut u8) {
        let vtable = rd::<*const usize>(parent, 0);
        let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(1));
        add(parent, child);
    }

    /// The game's own `KN5IO::load` of a model file (an absolute path into the game's folder).
    pub unsafe fn load_kn5(&self, path: &std::path::Path) -> *mut u8 {
        // a version 6 file carries a number that must be one the game was told at start-up (by
        // SteamInit, per owned pack): the oracle tells it the file's own
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.len() >= 14 && i32::from_le_bytes(bytes[6..10].try_into().unwrap()) >= 6 {
                let key = i32::from_le_bytes(bytes[10..14].try_into().unwrap());
                if key != 0 {
                    let add: extern "C" fn(i32) = std::mem::transmute(self.acs.va(VA_KN5IO_ADD_DLC_KEY));
                    add(key);
                }
            }
        }
        let io = self.acs.alloc(SIZE_KN5IO);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_KN5IO_CTOR));
        ctor(io, self.graphics);
        let load: extern "C" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_KN5IO_LOAD));
        load(io, wstring(&self.acs, &path.to_string_lossy()))
    }

    /// First experiment: one model, the game's camera, one frame.
    pub unsafe fn experiment(&self, model: &std::path::Path, out: &std::path::Path) -> Result<(), String> {
        let acs = &self.acs;
        let root = self.node("ROOT");
        let blurred = self.node("BLURRED");
        let unblurred = self.node("UNBLURRED");
        self.add_child(root, blurred);
        self.add_child(root, unblurred);
        let model_node = self.load_kn5(model);
        eprintln!("oracle: model root {model_node:p}");
        self.add_child(blurred, model_node);

        let camera = acs.alloc(SIZE_CAMERA_FORWARD);
        let ctor: extern "C" fn(*mut u8, *const u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_FORWARD_CTOR));
        ctor(camera, wstring(acs, "MAIN_CAMERA"), self.graphics, false);
        let sky = acs.alloc(SIZE_SKYBOX);
        let sky_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SKYBOX_CTOR));
        sky_ctor(sky, self.graphics);
        wr(camera, 0x98, sky);
        // what Sim::Sim sets after Sim::createCamera: the clear colour and Camera::maxLayer = WORLD_DETAIL
        wr(camera, 0x80, [0.3f32, 0.25, 0.25, 1.0]);
        wr(camera, 0x1ac, 5f32);
        // Camera::matrix: at (0, 1.2, -6), looking along +z
        let matrix: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.2, -6.0, 1.0];
        wr(camera, 0xc, matrix);
        eprintln!("oracle: camera fov {} near {} far {} aspect {}", rd::<f32>(camera, 8), rd::<f32>(camera, 0x70), rd::<f32>(camera, 0x74), rd::<f32>(camera, 0x1a8));

        let begin: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_BEGIN_SCENE));
        let end: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GRAPHICS_END_SCENE));
        let traverse: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_WORLD_MATRIX_TRAVERSE));
        let traverser = acs.alloc(0x40);
        for frame in 0..3 {
            rustyac_render::gpulog::begin_capture("frame");
            begin(self.graphics);
            traverse(traverser, root);
            let vtable = rd::<*const usize>(camera, 0);
            let render: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8, f32) = std::mem::transmute(*vtable.add(7));
            render(camera, blurred, unblurred, root, 1.0 / 60.0);
            let capture = rustyac_render::gpulog::end_capture();
            eprintln!("oracle: frame {frame}: {} lines, {} draws", capture.lines, capture.draws);
            if frame == 1 {
                std::fs::write(out.with_extension("gpulog"), &capture.text).map_err(|e| e.to_string())?;
                let (width, height, pixels) = self.read_back()?;
                write_png(&out.with_extension("png"), width, height, &pixels)?;
            }
            end(self.graphics);
        }
        Ok(())
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

pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}
