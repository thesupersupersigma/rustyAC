// SPDX-License-Identifier: GPL-3.0-or-later

//! [`super::pad`]'s `XInput` on Linux: the first SDL3 gamepad (Xbox layout, as XInput's),
//! turned back into the `XINPUT_GAMEPAD` that Windows would read, so AC's pad class above it
//! computes the same numbers. SDL's values are XInput's as SDL's own XInput backend reports
//! them (`SDL_xinputjoystick.c`: the sticks' y as `~y`, a trigger `b` as `b * 257 - 32768`,
//! then the gamepad's 0..32767); [`pad_state`] undoes exactly that.

use std::ffi::c_int;

use sdl3::sys::gamepad::*;
use sdl3::sys::init::{SDL_InitSubSystem, SDL_QuitSubSystem, SDL_INIT_GAMEPAD};
use sdl3::sys::joystick::{SDL_GetJoystickTypeForID, SDL_JoystickID, SDL_JOYSTICK_TYPE_WHEEL};
use sdl3::sys::stdinc::SDL_free;

use super::pad::PadState;

/// XInput's `wButtons` bit of each SDL button (positions: south = A, east = B ...). The guide
/// button is not in `XInputGetState`'s state.
const BUTTONS: [(SDL_GamepadButton, u16); 14] = [
    (SDL_GAMEPAD_BUTTON_DPAD_UP, 0x0001),
    (SDL_GAMEPAD_BUTTON_DPAD_DOWN, 0x0002),
    (SDL_GAMEPAD_BUTTON_DPAD_LEFT, 0x0004),
    (SDL_GAMEPAD_BUTTON_DPAD_RIGHT, 0x0008),
    (SDL_GAMEPAD_BUTTON_START, 0x0010),
    (SDL_GAMEPAD_BUTTON_BACK, 0x0020),
    (SDL_GAMEPAD_BUTTON_LEFT_STICK, 0x0040),
    (SDL_GAMEPAD_BUTTON_RIGHT_STICK, 0x0080),
    (SDL_GAMEPAD_BUTTON_LEFT_SHOULDER, 0x0100),
    (SDL_GAMEPAD_BUTTON_RIGHT_SHOULDER, 0x0200),
    (SDL_GAMEPAD_BUTTON_SOUTH, 0x1000),
    (SDL_GAMEPAD_BUTTON_EAST, 0x2000),
    (SDL_GAMEPAD_BUTTON_WEST, 0x4000),
    (SDL_GAMEPAD_BUTTON_NORTH, 0x8000),
];

/// SDL's gamepad trigger (0..32767) as XInput's byte: the nearest, which is the byte SDL made
/// it from (the test tries all 256).
pub fn trigger_byte(value: i16) -> u8 {
    ((value.max(0) as i32 * 255 + 16383) / 32767) as u8
}

/// The `XINPUT_GAMEPAD` behind SDL's buttons (as XInput bits) and axes (left x, left y, right
/// x, right y, left trigger, right trigger, SDL's order).
pub fn pad_state(buttons: u16, axes: [i16; 6]) -> PadState {
    PadState {
        buttons,
        left_trigger: trigger_byte(axes[4]),
        right_trigger: trigger_byte(axes[5]),
        thumb_lx: axes[0],
        thumb_ly: !axes[1],
        thumb_rx: axes[2],
        thumb_ry: !axes[3],
    }
}

/// The pads SDL3 offers. AC only ever reads pad 0; rustyAC takes the first gamepad that is
/// not a wheel.
pub struct XInput {
    /// SDL's id of the pad in use.
    pub index: Option<u32>,
    pad: *mut SDL_Gamepad,
    since_scan: u32,
    sdl: bool,
}

// SAFETY: made, used and dropped on the physics thread (or the main thread for the device
// list); SDL's gamepad calls lock its joystick list themselves.
unsafe impl Send for XInput {}

impl XInput {
    pub fn new() -> XInput {
        // SAFETY: SDL counts its subsystems; the window may have started SDL already
        let sdl = unsafe { SDL_InitSubSystem(SDL_INIT_GAMEPAD) };
        if !sdl {
            eprintln!("WARNING: SDL's gamepads: {}", sdl3::get_error());
        }
        let mut xinput = XInput { index: None, pad: std::ptr::null_mut(), since_scan: 0, sdl };
        xinput.scan();
        xinput
    }

    fn close(&mut self) {
        if !self.pad.is_null() {
            // SAFETY: a gamepad this struct opened
            unsafe { SDL_CloseGamepad(self.pad) };
        }
        self.pad = std::ptr::null_mut();
        self.index = None;
    }

    /// Looks for gamepads (wheels that SDL also maps as one are left to the wheel's class)
    /// and opens the first.
    pub fn scan(&mut self) -> Vec<u32> {
        self.since_scan = 0;
        if !self.sdl {
            return Vec::new();
        }
        let mut found = Vec::new();
        // SAFETY: SDL's list is read within its count and freed
        unsafe {
            SDL_UpdateGamepads();
            let mut count: c_int = 0;
            let ids = SDL_GetGamepads(&mut count);
            if !ids.is_null() {
                for k in 0..count.max(0) as usize {
                    let id = *ids.add(k);
                    if SDL_GetJoystickTypeForID(id) != SDL_JOYSTICK_TYPE_WHEEL {
                        found.push(id.0);
                    }
                }
                SDL_free(ids.cast());
            }
        }
        if self.index != found.first().copied() {
            self.close();
            if let Some(&id) = found.first() {
                // SAFETY: an id SDL just listed
                let pad = unsafe { SDL_OpenGamepad(SDL_JoystickID(id)) };
                if !pad.is_null() {
                    self.pad = pad;
                    self.index = Some(id);
                }
            }
        }
        found
    }

    /// The pad's state now; `None` while no pad is connected (it is looked for again about
    /// once a second).
    pub fn poll(&mut self) -> Option<PadState> {
        if self.pad.is_null() {
            self.since_scan += 1;
            if self.since_scan >= 333 {
                self.scan();
            }
            return None;
        }
        // SAFETY: the gamepad is open
        unsafe {
            SDL_UpdateGamepads();
            if !SDL_GamepadConnected(self.pad) {
                self.close();
                return None;
            }
            let buttons = BUTTONS.iter().filter(|(b, _)| SDL_GetGamepadButton(self.pad, *b)).fold(0, |m, (_, bit)| m | bit);
            let axes = [
                SDL_GAMEPAD_AXIS_LEFTX,
                SDL_GAMEPAD_AXIS_LEFTY,
                SDL_GAMEPAD_AXIS_RIGHTX,
                SDL_GAMEPAD_AXIS_RIGHTY,
                SDL_GAMEPAD_AXIS_LEFT_TRIGGER,
                SDL_GAMEPAD_AXIS_RIGHT_TRIGGER,
            ]
            .map(|a| SDL_GetGamepadAxis(self.pad, a));
            Some(pad_state(buttons, axes))
        }
    }

    /// `XInputSetState`'s two motor words: left = the heavy low-frequency motor, right = the
    /// light one. XInput keeps a level until the next call; SDL's lasts its duration, which
    /// every call starts again (the pad's class sends often).
    pub fn rumble(&mut self, left: f32, right: f32) {
        if !self.pad.is_null() {
            // SAFETY: the gamepad is open
            unsafe { SDL_RumbleGamepad(self.pad, super::pad::motor_word(left), super::pad::motor_word(right), 0xffff) };
        }
    }
}

impl Default for XInput {
    fn default() -> XInput {
        XInput::new()
    }
}

impl Drop for XInput {
    fn drop(&mut self) {
        self.close();
        if self.sdl {
            // SAFETY: balances this struct's own `SDL_InitSubSystem`
            unsafe { SDL_QuitSubSystem(SDL_INIT_GAMEPAD) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SDL's gamepad trigger of XInput's byte `b` (`SDL_xinputjoystick.c`, then
    /// `SDL_gamepad.c`'s full-range axis to 0..32767, in its float arithmetic). Linux's xpad
    /// gives the same: its 0..255 axis scaled by SDL's evdev code is `b * 257 - 32768` too.
    fn sdl_trigger(b: u8) -> i16 {
        let value = b as i32 * 257 - 32768;
        let normalized = (value - -32768) as f32 / (32767 - -32768) as f32;
        (normalized * 32767.0) as i32 as i16
    }

    #[test]
    fn every_xinput_value_comes_back() {
        for b in 0..=255u8 {
            assert_eq!(trigger_byte(sdl_trigger(b)), b, "trigger {b}");
        }
        // sticks: SDL's y is `~y` (Linux's xpad reports `~y` too), x as it is
        for v in i16::MIN..=i16::MAX {
            let state = pad_state(0, [v, !v, v, !v, 0, 0]);
            assert_eq!((state.thumb_lx, state.thumb_ly, state.thumb_rx, state.thumb_ry), (v, v, v, v));
        }
        let raw = PadState { buttons: 0xf3ff, left_trigger: 200, right_trigger: 7, thumb_lx: -32768, thumb_ly: 32767, thumb_rx: 7849, thumb_ry: -1 };
        let sdl = pad_state(raw.buttons, [raw.thumb_lx, !raw.thumb_ly, raw.thumb_rx, !raw.thumb_ry, sdl_trigger(200), sdl_trigger(7)]);
        assert_eq!(sdl, raw);
        assert_eq!(BUTTONS.iter().fold(0, |m, (_, bit)| m | bit), 0xf3ff);
    }
}
