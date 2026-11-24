// SPDX-License-Identifier: GPL-3.0-or-later

//! `window.rs` on Linux (Task P3a): the game's window, keyboard and mouse on SDL3, with the same
//! [`Event`]s, and the wgpu surface AC's renderer presents to ([`Chain`]).
//!
//! The keys are Windows' virtual-key codes (AC's `controls.ini` binds those), made from SDL's
//! scancodes. The physics thread asks for keys and the cursor ([`key_async`], [`cursor_pos`]):
//! the window's thread keeps them in atomics as its events come, so no SDL call leaves it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};

use rustyac_render::wgpu;
use sdl3::event::{Event as SdlEvent, WindowEvent};
use sdl3::mouse::MouseButton;
use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

/// What happened to the window since the last look (as in `window.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Close,
    Key { key: u32, left_ctrl: bool, right_ctrl: bool, alt: bool, shift: bool },
    Focus(bool),
    Resize(u32, u32),
    Mouse { x: i32, y: i32, action: MouseAction },
    Char(u32),
}

/// What the mouse did (as in `window.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Down(i32),
    Up(i32),
    Wheel(i32),
}

/// Never set here: SDL has no modal loop of its own inside the window's events.
pub static MODAL_LOOP: AtomicBool = AtomicBool::new(false);

/// Virtual keys held now (by Windows' code), the mouse buttons at 1, 2 and 4 as Windows has them.
static KEYS: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
/// The cursor in the window, and the window's place and size (screen pixels).
static MOUSE: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
static RECT: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
/// A cursor move asked for by another thread (x << 32 | y, screen pixels), done by `pump`.
static WARP: AtomicI64 = AtomicI64::new(NO_WARP);
const NO_WARP: i64 = i64::MIN;

/// The SDL window, as an address (the game's `activeWindow`).
#[derive(Clone, Copy)]
pub struct Handle(pub *mut std::ffi::c_void);

/// The window's wgpu surface on the device AC's renderer draws with (given to kgl before the
/// renderer is made), taken once by [`take_chain`].
pub struct Chain {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: RefCell<wgpu::SurfaceConfiguration>,
    /// The adapter and its backend, in words.
    pub adapter: String,
}

thread_local! {
    static CHAIN: RefCell<Option<Chain>> = const { RefCell::new(None) };
}

/// The surface `Window::create` made (once).
pub fn take_chain() -> Result<Chain, String> {
    CHAIN.with(|chain| chain.borrow_mut().take()).ok_or_else(|| "the window has no wgpu surface".to_string())
}

pub struct Window {
    pub handle: Handle,
    // (dropped in this order: the events, the window, SDL itself)
    pump: RefCell<sdl3::EventPump>,
    window: sdl3::video::Window,
    _video: sdl3::VideoSubsystem,
    _sdl: sdl3::Sdl,
}

/// The display's handle as wgpu's GL backend wants it at the instance (EGL's display).
#[derive(Debug)]
struct Display(wgpu::rwh::RawDisplayHandle);
// SAFETY: only an address of the display, which lives as long as SDL's video (the window's).
unsafe impl Send for Display {}
// SAFETY: as above.
unsafe impl Sync for Display {}
impl HasDisplayHandle for Display {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        // SAFETY: the handle came from SDL's live display.
        Ok(unsafe { wgpu::rwh::DisplayHandle::borrow_raw(self.0) })
    }
}

impl Window {
    /// Opens the window (see `window.rs`) and its wgpu surface. The backend: `WGPU_BACKEND`
    /// (`vulkan`, `gl`), else Vulkan or GL, whichever adapter is the better one.
    pub fn create(title: &str, width: u32, height: u32, windowed: bool, no_focus: bool) -> Result<Window, String> {
        if no_focus {
            sdl3::hint::set("SDL_WINDOW_ACTIVATE_WHEN_SHOWN", "0");
        }
        let sdl = sdl3::init().map_err(|e| format!("SDL: {e}"))?;
        let video = sdl.video().map_err(|e| format!("SDL video: {e}"))?;
        let mut builder = video.window(title, width, height);
        if windowed {
            builder.position_centered().resizable();
        } else {
            builder.fullscreen();
        }
        let window = builder.build().map_err(|e| format!("the window could not be created: {e}"))?;
        video.text_input().start(&window);
        let pump = sdl.event_pump().map_err(|e| format!("SDL events: {e}"))?;
        let handle = Handle(window.raw() as *mut _);
        let (x, y) = window.position();
        let (w, h) = window.size();
        for (slot, value) in RECT.iter().zip([x, y, w as i32, h as i32]) {
            slot.store(value, Ordering::Relaxed);
        }
        let chain = surface(&window)?;
        println!("window: SDL {} on {}, wgpu: {}", sdl3::version::version(), video.current_video_driver(), chain.adapter);
        CHAIN.with(|c| *c.borrow_mut() = Some(chain));
        Ok(Window { handle, pump: RefCell::new(pump), window, _video: video, _sdl: sdl })
    }

    /// The client area's size, pixels.
    pub fn client_size(&self) -> (u32, u32) {
        let (w, h) = self.window.size_in_pixels();
        (w.max(1), h.max(1))
    }

    /// Handles the waiting events; returns what happened.
    pub fn pump(&self) -> Vec<Event> {
        let warp = WARP.swap(NO_WARP, Ordering::Relaxed);
        if warp != NO_WARP {
            let (x, y) = ((warp >> 32) as i32 - RECT[0].load(Ordering::Relaxed), warp as i32 - RECT[1].load(Ordering::Relaxed));
            self._sdl.mouse().warp_mouse_in_window(&self.window, x as f32, y as f32);
        }
        let mut events = Vec::new();
        for event in self.pump.borrow_mut().poll_iter() {
            translate(event, &mut events);
        }
        events
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // a surface left unused must go before its window
        CHAIN.with(|c| c.borrow_mut().take());
    }
}

fn surface(window: &sdl3::video::Window) -> Result<Chain, String> {
    let display = window.display_handle().map_err(|e| format!("SDL display: {e}"))?.as_raw();
    let raw_window = window.window_handle().map_err(|e| format!("SDL window: {e}"))?.as_raw();
    let mut d = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(Display(display))).with_env();
    // (wgpu's own reading of the variable needs its `std` feature, which the build leaves off)
    d.backends = std::env::var("WGPU_BACKEND").map_or(wgpu::Backends::VULKAN | wgpu::Backends::GL, |list| wgpu::Backends::from_comma_list(&list));
    let instance = wgpu::Instance::new(d);
    // SAFETY: the window outlives the surface (`Window` drops an untaken one first; the game
    // drops the chain before the window).
    let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: raw_window }) }.map_err(|e| format!("wgpu surface: {e}"))?;
    let adapter = pollster(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: false, compatible_surface: Some(&surface), apply_limit_buckets: false }))
        .map_err(|e| format!("wgpu adapter: {e}"))?;
    let info = adapter.get_info();
    let features = crate::render::ac::AcRenderer::wanted_features(adapter.features());
    let (device, queue) = pollster(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("kgl"), required_features: features, required_limits: adapter.limits(), ..Default::default() })).map_err(|e| format!("wgpu device: {e}"))?;
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| eprintln!("wgpu error: {e}")));
    crate::render::ac::AcRenderer::give_device(device.clone(), queue.clone(), adapter.features());
    let caps = surface.get_capabilities(&adapter);
    // (a plain format: the picture is written as it is, as into Direct3D's R8G8B8A8_UNORM)
    let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied()).ok_or("the surface takes no format")?;
    let (w, h) = window.size_in_pixels();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: w.max(1),
        height: h.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
        view_formats: Vec::new(),
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&device, &config);
    Ok(Chain { surface, device, queue, config: RefCell::new(config), adapter: format!("{} ({:?}, {:?})", info.name, info.backend, format) })
}

/// The adapter's and the device's futures: wgpu answers them at once natively.
fn pollster<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

impl Chain {
    /// The window has a new size.
    pub fn resize(&self, width: u32, height: u32) {
        let mut config = self.config.borrow_mut();
        (config.width, config.height) = (width.max(1), height.max(1));
        self.surface.configure(&self.device, &config);
    }

    /// The next picture of the window, drawn by `draw`, then shown. `Ok(false)`: the window
    /// is hidden or the surface had to be made again (no frame shown).
    pub fn present(&self, vsync: bool, draw: impl FnOnce(&wgpu::TextureView, wgpu::TextureFormat)) -> Result<bool, String> {
        let mode = if vsync { wgpu::PresentMode::Fifo } else { wgpu::PresentMode::AutoNoVsync };
        if self.config.borrow().present_mode != mode {
            self.config.borrow_mut().present_mode = mode;
            self.surface.configure(&self.device, &self.config.borrow());
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config.borrow());
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return Ok(false),
            other => return Err(format!("the window gave no picture to draw into ({other:?})")),
        };
        let format = self.config.borrow().format;
        draw(&frame.texture.create_view(&wgpu::TextureViewDescriptor::default()), format);
        self.queue.present(frame);
        Ok(true)
    }
}

/// `GetAsyncKeyState(vk) != 0`: the key (or mouse button) is held.
pub fn key_async(vk: i32) -> bool {
    let held = |vk: usize| KEYS[vk].load(Ordering::Relaxed);
    match vk {
        0x10 => held(0xa0) || held(0xa1),
        0x11 => held(0xa2) || held(0xa3),
        0x12 => held(0xa4) || held(0xa5),
        1..=255 => held(vk as usize),
        _ => false,
    }
}

/// The cursor in screen pixels (the window's place plus the cursor in it: under Wayland the
/// window's place is unknown, so both are the window's own pixels).
pub fn cursor_pos() -> (i32, i32) {
    (RECT[0].load(Ordering::Relaxed) + MOUSE[0].load(Ordering::Relaxed), RECT[1].load(Ordering::Relaxed) + MOUSE[1].load(Ordering::Relaxed))
}

/// Moves the cursor (screen pixels), on the window's thread's next look at its events.
pub fn set_cursor_pos(x: i32, y: i32) {
    WARP.store(((x as i64) << 32) | (y as u32 as i64), Ordering::Relaxed);
}

/// The window's rectangle in screen pixels: left, top, right, bottom.
pub fn window_rect(_window: isize) -> [i32; 4] {
    let [x, y, w, h] = [0, 1, 2, 3].map(|i| RECT[i].load(Ordering::Relaxed));
    [x, y, x + w, y + h]
}

/// The screen's size (right edge, height), as the orbit cameras read it: the window's own.
pub fn screen_size() -> (i32, i32) {
    let [_, _, right, bottom] = window_rect(0);
    (right, bottom)
}

/// `ShowCursor`'s display counter: the cursor shows while it is not negative.
static CURSOR_COUNT: AtomicI32 = AtomicI32::new(0);

/// Win32's `ShowCursor`: one up or down on the display counter, which is returned; SDL shows
/// or hides the cursor to match. On the window's thread only.
pub fn show_cursor(show: bool) -> i32 {
    let count = CURSOR_COUNT.fetch_add(if show { 1 } else { -1 }, Ordering::Relaxed) + if show { 1 } else { -1 };
    // SAFETY: plain SDL calls on the thread that made the window.
    unsafe {
        if count >= 0 {
            sdl3::sys::mouse::SDL_ShowCursor();
        } else {
            sdl3::sys::mouse::SDL_HideCursor();
        }
    }
    count
}

/// Whether the cursor shows (the display counter is not negative).
pub fn cursor_shown() -> bool {
    CURSOR_COUNT.load(Ordering::Relaxed) >= 0
}

fn set_key(vk: u32, down: bool) {
    if let Some(slot) = KEYS.get(vk as usize) {
        slot.store(down, Ordering::Relaxed);
    }
}

fn translate(event: SdlEvent, out: &mut Vec<Event>) {
    match event {
        SdlEvent::Quit { .. } => out.push(Event::Close),
        SdlEvent::Window { win_event, .. } => match win_event {
            WindowEvent::CloseRequested => out.push(Event::Close),
            WindowEvent::FocusGained => out.push(Event::Focus(true)),
            WindowEvent::FocusLost => {
                for key in &KEYS {
                    key.store(false, Ordering::Relaxed);
                }
                out.push(Event::Focus(false));
            }
            WindowEvent::PixelSizeChanged(w, h) if w > 0 && h > 0 => out.push(Event::Resize(w as u32, h as u32)),
            WindowEvent::Moved(x, y) => {
                RECT[0].store(x, Ordering::Relaxed);
                RECT[1].store(y, Ordering::Relaxed);
            }
            WindowEvent::Resized(w, h) => {
                RECT[2].store(w, Ordering::Relaxed);
                RECT[3].store(h, Ordering::Relaxed);
            }
            _ => {}
        },
        SdlEvent::KeyDown { scancode: Some(scancode), repeat, .. } => {
            let vk = virtual_key(scancode as u32);
            // (WM_CHAR's control characters, which typing in AC's text boxes reads; they repeat)
            if let Some(c) = match vk {
                0x08 => Some(8),
                0x09 => Some(9),
                0x0d => Some(13),
                0x1b => Some(27),
                _ => None,
            } {
                out.push(Event::Char(c));
            }
            if vk == 0 || repeat {
                return;
            }
            set_key(vk, true);
            // WM_KEYDOWN names Shift, Ctrl and Alt without their side
            let key = match vk {
                0xa0 | 0xa1 => 0x10,
                0xa2 | 0xa3 => 0x11,
                0xa4 | 0xa5 => 0x12,
                other => other,
            };
            out.push(Event::Key { key, left_ctrl: key_async(0xa2), right_ctrl: key_async(0xa3), alt: key_async(0x12), shift: key_async(0x10) });
        }
        SdlEvent::KeyUp { scancode: Some(scancode), .. } => set_key(virtual_key(scancode as u32), false),
        SdlEvent::TextInput { text, .. } => {
            let mut units = [0u16; 2];
            for c in text.chars().filter(|c| !c.is_control()) {
                for unit in c.encode_utf16(&mut units).iter() {
                    out.push(Event::Char(*unit as u32));
                }
            }
        }
        SdlEvent::MouseMotion { x, y, .. } => out.push(mouse(x, y, MouseAction::Move)),
        SdlEvent::MouseButtonDown { mouse_btn, x, y, .. } | SdlEvent::MouseButtonUp { mouse_btn, x, y, .. } => {
            let down = matches!(event, SdlEvent::MouseButtonDown { .. });
            let (vk, button) = match mouse_btn {
                MouseButton::Left => (1, Some(0)),
                MouseButton::Right => (2, Some(1)),
                MouseButton::Middle => (4, None),
                _ => (0, None),
            };
            set_key(vk, down);
            if let Some(button) = button {
                out.push(mouse(x, y, if down { MouseAction::Down(button) } else { MouseAction::Up(button) }));
            }
        }
        SdlEvent::MouseWheel { y, mouse_x, mouse_y, .. } => out.push(mouse(mouse_x, mouse_y, MouseAction::Wheel((y * 120.0) as i32))),
        _ => {}
    }
}

fn mouse(x: f32, y: f32, action: MouseAction) -> Event {
    let (x, y) = (x as i32, y as i32);
    MOUSE[0].store(x, Ordering::Relaxed);
    MOUSE[1].store(y, Ordering::Relaxed);
    Event::Mouse { x, y, action }
}

/// Windows' virtual-key code of a USB scancode (SDL's); 0: none.
fn virtual_key(scancode: u32) -> u32 {
    match scancode {
        4..=29 => 0x41 + scancode - 4,      // A..Z
        30..=38 => 0x31 + scancode - 30,    // 1..9
        39 => 0x30,                         // 0
        40 => 0x0d,                         // Return
        41 => 0x1b,                         // Escape
        42 => 0x08,                         // Backspace
        43 => 0x09,                         // Tab
        44 => 0x20,                         // Space
        45 => 0xbd,                         // -
        46 => 0xbb,                         // =
        47 => 0xdb,                         // [
        48 => 0xdd,                         // ]
        49 | 50 => 0xdc,                    // backslash, the non-US #
        51 => 0xba,                         // ;
        52 => 0xde,                         // '
        53 => 0xc0,                         // `
        54 => 0xbc,                         // ,
        55 => 0xbe,                         // .
        56 => 0xbf,                         // /
        57 => 0x14,                         // Caps Lock
        58..=69 => 0x70 + scancode - 58,    // F1..F12
        70 => 0x2c,                         // Print Screen
        71 => 0x91,                         // Scroll Lock
        72 => 0x13,                         // Pause
        73 => 0x2d,                         // Insert
        74 => 0x24,                         // Home
        75 => 0x21,                         // Page Up
        76 => 0x2e,                         // Delete
        77 => 0x23,                         // End
        78 => 0x22,                         // Page Down
        79 => 0x27,                         // Right
        80 => 0x25,                         // Left
        81 => 0x28,                         // Down
        82 => 0x26,                         // Up
        83 => 0x90,                         // Num Lock
        84 => 0x6f,                         // keypad /
        85 => 0x6a,                         // keypad *
        86 => 0x6d,                         // keypad -
        87 => 0x6b,                         // keypad +
        88 => 0x0d,                         // keypad Enter
        89..=97 => 0x61 + scancode - 89,    // keypad 1..9
        98 => 0x60,                         // keypad 0
        99 => 0x6e,                         // keypad .
        100 => 0xe2,                        // the non-US backslash
        104..=115 => 0x7c + scancode - 104, // F13..F24
        224 => 0xa2,                        // left Ctrl
        225 => 0xa0,                        // left Shift
        226 => 0xa4,                        // left Alt
        227 => 0x5b,                        // left Windows key
        228 => 0xa3,                        // right Ctrl
        229 => 0xa1,                        // right Shift
        230 => 0xa5,                        // right Alt
        231 => 0x5c,                        // right Windows key
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scancodes_give_windows_virtual_keys() {
        // A, Z, 1, 0, F1, F12, keypad 0 and 9, left Ctrl, right Shift
        let pairs = [(4, 0x41), (29, 0x5a), (30, 0x31), (39, 0x30), (58, 0x70), (69, 0x7b), (98, 0x60), (97, 0x69), (224, 0xa2), (229, 0xa1)];
        for (scancode, vk) in pairs {
            assert_eq!(super::virtual_key(scancode), vk, "{scancode}");
        }
        super::set_key(0xa3, true);
        assert!(super::key_async(0x11) && super::key_async(0xa3) && !super::key_async(0xa2));
        super::set_key(0xa3, false);
    }
}
