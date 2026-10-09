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

const SIZE_GRAPHICS_MANAGER: usize = 0x4e0;

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
        rustyac_render::gpulog::install(*device, *context).expect("the command log");
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
