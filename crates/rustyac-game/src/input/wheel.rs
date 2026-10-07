//! DirectInput wheels and pads: placeholder until `dinput.rs` is filled in.

use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};

use super::Extra;

pub struct WheelDevice;

impl WheelDevice {
    pub fn in_use(&self) -> bool {
        false
    }
    pub fn connected(&self) -> bool {
        false
    }
    pub fn poll(&mut self) {}
    pub fn acquire_controls(&mut self, _c: &mut CarControls, _e: &mut Extra, _dt: f32, _i: &CarControlsInput, _k: &dyn Fn(i32) -> bool) {}
    pub fn get_action(&self, _action: i32, _k: &dyn Fn(i32) -> bool) -> bool {
        false
    }
    pub fn send_ff(&mut self, _ff: f32, _damper: f32, _user_gain: f32, _active: bool) {}
    pub fn set_vibrations(&mut self, _def: &VibrationDef) {}
    pub fn stop_ff(&mut self) {}
}
