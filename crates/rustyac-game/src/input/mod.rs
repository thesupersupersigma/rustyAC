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

/// How long the pad's reset button is held for "back onto the track", seconds.
pub const RESET_HOLD_SECONDS: f64 = 0.6;

/// The pad's reset button (Back), which is also a second layer for the two traction-control
/// buttons (D-pad up / down): held, they change the ABS instead. Not AC's.
///
/// The rule, decided when the reset button comes up:
/// * a traction-control button went down while it was held: nothing (it was the second layer);
/// * else it was held for less than [`RESET_HOLD_SECONDS`]: back to the pits;
/// * else: back onto the track where the car is.
///
/// A traction-control button that went down under the reset button stays an ABS button until
/// it is released itself, whichever of the two is let go first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResetCombo {
    /// Seconds the reset button has been down, and whether it served as the second layer.
    held: Option<(f64, bool)>,
    /// The traction-control buttons (up, down) as they were in the last look.
    was: [bool; 2],
    /// Traction-control buttons that are ABS buttons until released.
    latched: [bool; 2],
}

impl ResetCombo {
    /// One look at the pad, `seconds` after the last one: is the reset button down, are the
    /// pad's traction-control buttons (up, down) down. Returns the [`event`] bit of the reset
    /// button: `RESET` or `TO_TRACK` in the look that sees it released, else 0.
    ///
    /// [`event`]: crate::input_file::event
    pub fn update(&mut self, reset: bool, tc: [bool; 2], seconds: f64) -> u32 {
        use crate::input_file::event;
        let mut events = 0;
        match (reset, self.held) {
            (true, None) => self.held = Some((0.0, false)),
            (true, Some((held, used))) => self.held = Some((held + seconds, used)),
            (false, Some((held, used))) => {
                self.held = None;
                if !used {
                    events = if held >= RESET_HOLD_SECONDS { event::TO_TRACK } else { event::RESET };
                }
            }
            (false, None) => {}
        }
        for (k, down) in tc.into_iter().enumerate() {
            if !down {
                self.latched[k] = false;
            } else if !self.was[k] {
                if let Some((held, _)) = self.held {
                    self.latched[k] = true;
                    self.held = Some((held, true));
                }
            }
        }
        self.was = tc;
        events
    }

    /// The device's buttons with the second layer applied: a latched traction-control button
    /// is the ABS button of the same direction.
    pub fn apply(&self, extra: &mut Extra) {
        if self.latched[0] {
            extra.tc_up = false;
            extra.abs_up = true;
        }
        if self.latched[1] {
            extra.tc_dn = false;
            extra.abs_dn = true;
        }
    }
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
