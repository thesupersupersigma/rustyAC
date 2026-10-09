// SPDX-License-Identifier: GPL-3.0-or-later

//! The driver's devices: keyboard, Xbox pad (XInput), DirectInput wheels and pads, with AC's
//! own input classes behind them ([`pad`], [`keyboard`], [`wheel`]) and AC's `controls.ini`
//! as the bindings file ([`bindings`]).
//!
//! AC has exactly one device per session (`[HEADER] INPUT_METHOD`). rustyAC keeps all of them
//! alive and lets the one that was touched last drive, so the keyboard always works and a
//! pad can be picked up at any time. Whichever drives, it is AC's class for that device
//! that fills `Car::controls`.

// The device classes are transcriptions of machine code: sums and comparisons are written
// in its order and with its tests (they decide what a NaN does), constants with its digits.
#![allow(
    clippy::double_comparisons,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::approx_constant,
    clippy::excessive_precision,
    clippy::neg_multiply,
    clippy::if_same_then_else,
    clippy::manual_clamp,
    clippy::nonminimal_bool,
    clippy::too_many_arguments
)]

pub mod bindings;
#[cfg(windows)]
pub mod dinput;
pub mod ini;
pub mod keyboard;
// the devices themselves (`GetAsyncKeyState`, XInput, DirectInput) and the source that reads
// them every step: Windows only
#[cfg(windows)]
mod live;
pub mod pad;
#[cfg(windows)]
pub mod wheel;

pub use crate::sim::CarProbe;
#[cfg(windows)]
pub use live::*;

/// The buttons of AC's `CarControls` that the physics car itself does not read: the game's
/// main thread acts on them (a click of the brake bias, a level of an aid).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extra {
    pub brake_balance_up: bool,
    pub brake_balance_dn: bool,
    pub abs_up: bool,
    pub abs_dn: bool,
    pub tc_up: bool,
    pub tc_dn: bool,
    /// The cockpit buttons of the engine brake and the hybrid system (`CarControls`
    /// `engineBrakeUp` ... `MGUHMode`): levels; the game's main thread acts on the press.
    pub engine_brake_up: bool,
    pub engine_brake_dn: bool,
    pub mguk_delivery_up: bool,
    pub mguk_delivery_dn: bool,
    pub mguk_recovery_up: bool,
    pub mguk_recovery_dn: bool,
    pub mguh_mode: bool,
    /// Not AC's: the driver presses the clutch himself (a button, a key, a pedal), so the
    /// automatic clutch aid must stand back for this step.
    pub clutch_pressed: bool,
}

/// `StepInput::device` values.
pub const DEVICE_KEYBOARD: u32 = 1;
pub const DEVICE_PAD: u32 = 2;
pub const DEVICE_WHEEL: u32 = 3;

pub fn device_name(device: u32) -> &'static str {
    match device {
        DEVICE_KEYBOARD => "keyboard",
        DEVICE_PAD => "Xbox pad",
        DEVICE_WHEEL => "DirectInput device",
        crate::sim::DEVICE_SPAWN => "spawn sequence",
        _ => "recorded / scripted",
    }
}
