// SPDX-License-Identifier: GPL-3.0-or-later

//! `dinput.rs` off Windows: no game controllers yet (step L4 reads wheels through SDL3), so
//! [`DirectInput::open`] refuses and the wheel class above it is never made.

use super::wheel::WheelDevice;

/// `InputDeviceState`, as in `dinput.rs`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiState {
    pub axes: [f32; 8],
    pub povs: [u32; 4],
    pub buttons: [u8; 128],
}

impl Default for DiState {
    fn default() -> DiState {
        DiState { axes: [0.0; 8], povs: [0; 4], buttons: [0; 128] }
    }
}

pub const FF_CAP: f32 = 0.3;

pub struct DiDevice {
    pub index: usize,
    pub name: String,
    pub is_xinput: bool,
    pub has_ff: bool,
    pub is_wheel: bool,
}

pub struct DirectInput {
    pub devices: Vec<DiDevice>,
}

impl DirectInput {
    pub fn open(_window: isize) -> Result<DirectInput, String> {
        Err("no wheels off Windows yet (step L4)".to_string())
    }

    pub fn poll(&mut self) {}

    pub fn state(&self, _joy: i32) -> Option<&DiState> {
        None
    }

    pub fn enable_ff(&mut self, _joy: i32) -> Result<(), String> {
        Err("no force feedback off Windows yet".to_string())
    }

    pub fn send_ff(&mut self, _joy: i32, _ff: f32) {}
}

pub fn describe_devices(_wheel: &Option<WheelDevice>) -> String {
    "  no wheels off Windows yet (step L4)\n".to_string()
}
