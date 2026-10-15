// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `MouseHider` (0x80 bytes): the mouse cursor goes away when the mouse has not moved for a
//! while, and comes back with the next move; Ctrl+F8 hides it for good.
//!
//! `MouseHider::MouseHider` 0x140114850, its handlers of the window's mouse-move and
//! mouse-up events (0x140114c10, 0x140114b60), `showCursor` 0x140114e20, `update` 0x140114ed0,
//! `forceMouseHide` 0x1401992d0, `getForceMouseHide` 0x140199340.
//!
//! The game looks once, when it is made, whether the player's car is driven with the keyboard
//! (`KeyboardCarControl`), and treats the keyboard's mouse steering (Ctrl+M) apart: the cursor
//! is hidden while the mouse steers, and shown while a menu is up.

use rustyac_gui::gui::Gui;
use rustyac_physics::data::ini::IniReader;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, GetCursorPos, GetWindowRect, ShowCursor, CURSORINFO, CURSOR_SHOWING};

pub struct MouseHider {
    /// `keyboardControls` is there: the player's car is driven with the keyboard
    pub keyboard: bool,
    /// `lastPoint` (the game leaves it as the heap had it; here it starts off the screen)
    last_point: (i32, i32),
    /// `lastTime`, ms of the game's clock
    last_time: u32,
    /// `interval`, `hideInterval`, ms
    interval: i32,
    hide_interval: i32,
    /// `forceHide`
    force_hide: bool,
    window: HWND,
    /// `keyboardControls->mouseSteering`, as the physics thread last told it
    pub mouse_steering: bool,
}

/// `GetCursorInfo`: the cursor shows (`None`: the call failed).
fn cursor_showing() -> Option<bool> {
    let mut info = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
    // SAFETY: a plain call with a structure of the size it is told.
    unsafe { GetCursorInfo(&mut info).ok().map(|()| info.flags == CURSOR_SHOWING) }
}

/// `ShowCursor(TRUE)` until the display counter is not negative any more.
fn show_until_shown() {
    // SAFETY: plain calls.
    unsafe { while ShowCursor(true) < 0 {} }
}

impl MouseHider {
    /// `MouseHider::MouseHider` 0x140114850. `ini` is `system/cfg/mouse_hider.ini`.
    pub fn new(ini: Option<&IniReader>, keyboard: bool, window: HWND) -> MouseHider {
        let mut hide_interval = 0x9c4;
        if let Some(ini) = ini.filter(|ini| ini.ready) {
            hide_interval = ini.get_int("SETTINGS", "HIDE_INTERVAL").unwrap_or(0) >> 1;
        }
        MouseHider { keyboard, last_point: (i32::MIN, i32::MIN), last_time: 0, interval: hide_interval, hide_interval, force_hide: false, window, mouse_steering: false }
    }

    /// `MouseHider::forceMouseHide` 0x1401992d0.
    pub fn force_mouse_hide(&mut self, hide: bool) {
        self.force_hide = hide;
    }

    /// `MouseHider::getForceMouseHide` 0x140199340.
    pub fn get_force_mouse_hide(&self) -> bool {
        self.force_hide
    }

    /// What both mouse handlers do when they act.
    fn woke(&mut self, gui: &mut Gui) {
        if !self.force_hide {
            if cursor_showing() != Some(true) {
                show_until_shown();
                self.interval = self.hide_interval;
            }
            gui.is_pointer_visible = true;
        } else {
            gui.is_pointer_visible = false;
            // SAFETY: a plain call.
            unsafe {
                ShowCursor(false);
            }
        }
    }

    /// The handler of the window's mouse-move event (0x140114c10).
    pub fn on_mouse_move(&mut self, gui: &mut Gui, now_ms: f64) {
        if !(self.keyboard && self.mouse_steering) {
            self.woke(gui);
        }
        self.last_time = now_ms as i64 as u32;
    }

    /// The handler of the window's mouse-up event (0x140114b60).
    pub fn on_mouse_up(&mut self, gui: &mut Gui, now_ms: f64) {
        if self.keyboard && !self.mouse_steering {
            self.woke(gui);
        }
        self.last_time = now_ms as i64 as u32;
    }

    /// `MouseHider::showCursor` 0x140114e20.
    fn show_cursor(&mut self, gui: &mut Gui, show: bool) {
        if self.force_hide {
            gui.is_pointer_visible = false;
            // SAFETY: a plain call.
            unsafe {
                ShowCursor(false);
            }
            return;
        }
        let showing = cursor_showing();
        if show {
            if showing != Some(true) {
                show_until_shown();
                self.interval = self.hide_interval;
            }
        } else if showing == Some(true) {
            // SAFETY: plain calls.
            unsafe { while ShowCursor(false) >= 0 {} }
        }
        gui.is_pointer_visible = show;
    }

    /// `MouseHider::update` 0x140114ed0, once a frame: a cursor over the window that showed
    /// and stood still from one look to the next (they are `interval` apart) is hidden. With
    /// mouse steering the cursor shows while the ESC or pause menu is up (`menu_up`), and is
    /// hidden otherwise (not in a replay).
    pub fn update(&mut self, gui: &mut Gui, now_ms: f64, has_focus: bool, menu_up: bool, replay: bool) {
        if self.keyboard && self.mouse_steering {
            if menu_up {
                self.woke(gui);
                return;
            }
            if !replay {
                self.show_cursor(gui, false);
                return;
            }
        }
        if (self.interval as f64) < now_ms - self.last_time as f64 && has_focus {
            let (mut point, mut rect) = (POINT::default(), RECT::default());
            // SAFETY: plain calls on the game's own window.
            let inside = unsafe { GetCursorPos(&mut point).is_ok() && GetWindowRect(self.window, &mut rect).is_ok() } && point.x >= rect.left && point.x <= rect.right && point.y >= rect.top && point.y <= rect.bottom;
            if inside {
                if cursor_showing() == Some(true) && (point.x, point.y) == self.last_point {
                    self.show_cursor(gui, false);
                }
                self.last_point = (point.x, point.y);
            }
            self.last_time = now_ms as i64 as u32;
        }
    }
}

impl Drop for MouseHider {
    /// (rustyAC's own: whoever comes after the game finds the cursor shown)
    fn drop(&mut self) {
        show_until_shown();
    }
}
