// SPDX-License-Identifier: GPL-3.0-or-later

//! The window: a plain Win32 window (borderless over the whole screen, or a normal one with
//! `--windowed`) whose messages become a short list of events for the main loop.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

/// What happened to the window since the last look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The user closed it.
    Close,
    /// A key went down (not a repeat): its virtual-key code, and whether the left Ctrl, the
    /// right Ctrl, an Alt key and a Shift key are held.
    Key { key: u32, left_ctrl: bool, right_ctrl: bool, alt: bool, shift: bool },
    /// The window got or lost the keyboard.
    Focus(bool),
    /// The client area's new size, pixels.
    Resize(u32, u32),
}

/// Windows is running a loop of its own inside the window's messages (the title bar is being
/// dragged, the window resized, a menu is open): no frame is drawn and no event read until
/// it ends. The physics thread pauses while this is set.
pub static MODAL_LOOP: AtomicBool = AtomicBool::new(false);

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

fn push(event: Event) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_CLOSE => {
            push(Event::Close);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            // bit 30: the key was down already (auto-repeat)
            if lparam.0 & (1 << 30) == 0 {
                // SAFETY: plain queries of the keyboard state of this thread's message.
                let (left_ctrl, right_ctrl, alt, shift) = unsafe { (GetKeyState(0xa2) < 0, GetKeyState(0xa3) < 0, GetKeyState(0x12) < 0, GetKeyState(0x10) < 0) };
                push(Event::Key { key: wparam.0 as u32, left_ctrl, right_ctrl, alt, shift });
            }
            // Alt+F4 and the like still reach the default handler
            if message == WM_SYSKEYDOWN {
                // SAFETY: the default handler with the message's own arguments.
                return unsafe { DefWindowProcW(window, message, wparam, lparam) };
            }
            LRESULT(0)
        }
        // Alt+letter is a command of rustyAC's (Alt+T, Alt+A, Alt+G): the default handler would
        // look for a menu entry with that letter, find none and beep
        WM_SYSCHAR => LRESULT(0),
        // a tap on Alt or F10 would open the window's menu, whose loop stops the frames
        WM_SYSCOMMAND if (wparam.0 & 0xfff0) == SC_KEYMENU as usize => LRESULT(0),
        WM_ENTERSIZEMOVE | WM_ENTERMENULOOP => {
            MODAL_LOOP.store(true, Ordering::Relaxed);
            // SAFETY: the default handler with the message's own arguments.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_EXITSIZEMOVE | WM_EXITMENULOOP => {
            MODAL_LOOP.store(false, Ordering::Relaxed);
            // SAFETY: as above.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_SETFOCUS => {
            push(Event::Focus(true));
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            push(Event::Focus(false));
            LRESULT(0)
        }
        WM_SIZE => {
            let (width, height) = ((lparam.0 & 0xffff) as u32, ((lparam.0 >> 16) & 0xffff) as u32);
            // a minimised window has no size worth drawing to
            if wparam.0 != SIZE_MINIMIZED as usize && width > 0 && height > 0 {
                push(Event::Resize(width, height));
            }
            LRESULT(0)
        }
        // Direct3D paints all of it
        WM_ERASEBKGND => LRESULT(1),
        // SAFETY: the default handler with the message's own arguments.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

pub struct Window {
    pub handle: HWND,
}

impl Window {
    /// Opens the window. `windowed`: a normal window with a client area of `width` x
    /// `height`, else borderless over the whole primary screen. `no_focus`: it is shown
    /// without taking the keyboard from whatever the user is doing.
    pub fn create(title: &str, width: u32, height: u32, windowed: bool, no_focus: bool) -> Result<Window, String> {
        // SAFETY: the usual window creation; the class and window live as long as the process
        // needs them and the window is destroyed in `drop`.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.to_string())?.into();
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("rustyAC"),
                ..Default::default()
            };
            // registering twice (a second window in one process) just fails harmlessly
            RegisterClassExW(&class);
            let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
            let ex_style = if no_focus { WS_EX_NOACTIVATE } else { WINDOW_EX_STYLE(0) };
            let (style, x, y, w, h) = if windowed {
                let style = WS_OVERLAPPEDWINDOW;
                let mut rect = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
                let _ = AdjustWindowRectEx(&mut rect, style, false, ex_style);
                (style, CW_USEDEFAULT, CW_USEDEFAULT, rect.right - rect.left, rect.bottom - rect.top)
            } else {
                (WS_POPUP, 0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            };
            let handle = CreateWindowExW(ex_style, w!("rustyAC"), windows::core::PCWSTR(title.as_ptr()), style, x, y, w, h, None, None, Some(instance), None)
                .map_err(|e| format!("the window could not be created: {e}"))?;
            if no_focus {
                let _ = ShowWindow(handle, SW_SHOWNOACTIVATE);
                push(Event::Focus(false));
            } else {
                let _ = ShowWindow(handle, SW_SHOW);
                let _ = SetForegroundWindow(handle);
                // Windows may refuse to bring the window to the front (then no focus message
                // comes either): say what is, so that keys typed elsewhere do not drive
                push(Event::Focus(GetForegroundWindow() == handle));
            }
            Ok(Window { handle })
        }
    }

    /// The client area's size, pixels.
    pub fn client_size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        // SAFETY: a query of this window.
        unsafe {
            let _ = GetClientRect(self.handle, &mut rect);
        }
        ((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32)
    }

    /// Handles the waiting messages; returns what happened.
    pub fn pump(&self) -> Vec<Event> {
        let mut message = MSG::default();
        // SAFETY: the standard message loop of the thread that owns the window.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    push(Event::Close);
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window was created by this struct on this thread.
        unsafe {
            let _ = DestroyWindow(self.handle);
        }
        // let its last messages go
        self.pump();
    }
}
