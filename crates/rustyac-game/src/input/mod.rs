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
pub mod dinput;
pub mod ini;
pub mod keyboard;
pub mod pad;
pub mod wheel;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::input_file::event;
use crate::physics_thread::Shared;
pub use crate::sim::CarProbe;
use crate::sim::DriverSource;
use bindings::Bindings;
use keyboard::KeyboardCarControl;
use pad::{JoypadCarControl, PadButton, PadState, XInput};
use wheel::WheelDevice;

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

/// Is the key held right now? (AC also counts "was pressed since the last look", bit 0 of
/// `GetAsyncKeyState`, which other programs can eat; only "down now" is used here.)
pub fn key_is_down(key: i32) -> bool {
    // SAFETY: a plain query of the keyboard state.
    key > 0 && key < 256 && unsafe { GetAsyncKeyState(key) } < 0
}

/// The live devices as the car's driver.
pub struct LiveSource {
    pub bindings: Bindings,
    pub pad: JoypadCarControl,
    pub keyboard: KeyboardCarControl,
    pub wheel: Option<WheelDevice>,
    xinput: XInput,
    shared: Arc<Shared>,
    /// The device that drives: one of the `DEVICE_` values.
    pub active: u32,
    extra: Extra,
    pending_events: u32,
    pending_bias: i32,
    probe: CarProbe,
    rumble: bool,
    /// The pad's state of this step, `None` without a pad.
    pad_state: Option<PadState>,
    /// The pad's own buttons (reset, pause, camera) as they were in the last look.
    meta_down: [bool; 3],
    /// When the reset button went down, and whether its hold has already acted.
    reset_down: Option<(std::time::Instant, bool)>,
    rumbling: bool,
}

impl LiveSource {
    pub fn new(bindings: Bindings, shared: Arc<Shared>, rumble: bool, wheel: Option<WheelDevice>) -> LiveSource {
        let pad = JoypadCarControl::from_ini(&bindings.ini, bindings.use_legacy_gamepad_code);
        let keyboard = KeyboardCarControl::from_ini(&bindings.ini);
        let xinput = XInput::new();
        let active = if wheel.is_some() && bindings.input_method == "WHEEL" {
            DEVICE_WHEEL
        } else if xinput.index.is_some() && bindings.input_method != "KEYBOARD" {
            DEVICE_PAD
        } else {
            DEVICE_KEYBOARD
        };
        LiveSource {
            bindings,
            pad,
            keyboard,
            wheel,
            xinput,
            shared,
            active,
            extra: Extra::default(),
            pending_events: 0,
            pending_bias: 0,
            probe: CarProbe::default(),
            rumble,
            pad_state: None,
            meta_down: [false; 3],
            reset_down: None,
            rumbling: false,
        }
    }

    pub fn pad_connected(&self) -> bool {
        self.xinput.index.is_some()
    }

    fn focused(&self) -> bool {
        self.shared.focused.load(Ordering::Relaxed)
    }

    /// The pad buttons that are rustyAC's own: reset, pause, camera. They work on the press.
    fn meta_buttons(&mut self, state: Option<&PadState>, paused: bool) {
        let mask = state.map(PadState::button_mask).unwrap_or(0);
        let never: &dyn Fn(i32) -> bool = &|_| false;
        let down = [
            self.bindings.pad_reset.is_pressed(mask, never),
            self.bindings.pad_pause.is_pressed(mask, never),
            self.pad.get_action_on(mask, 5),
        ];
        let pressed = |k: usize| down[k] && !self.meta_down[k];
        if pressed(1) {
            self.shared.paused.fetch_xor(true, Ordering::Relaxed);
        }
        // the reset button: a tap puts the car back at its spawn point (on release), holding
        // it for 0.6 s puts it back on the track where it is
        const HOLD: std::time::Duration = std::time::Duration::from_millis(600);
        if pressed(0) {
            self.reset_down = Some((std::time::Instant::now(), false));
        }
        match (down[0], self.reset_down) {
            (true, Some((since, false))) if since.elapsed() >= HOLD => {
                if !paused {
                    self.pending_events |= event::TO_TRACK;
                }
                self.reset_down = Some((since, true));
            }
            (false, Some((_, held))) => {
                if !held && !paused {
                    self.pending_events |= event::RESET;
                }
                self.reset_down = None;
            }
            _ => {}
        }
        if !paused && pressed(2) {
            self.shared.camera_toggles.fetch_add(1, Ordering::Relaxed);
        }
        self.meta_down = down;
    }

    /// Which device drives this step: the one in use; the keyboard when the pad is gone.
    fn choose(&mut self, controls: &CarControls, keys_in_use: bool) {
        let pad_in_use = self.pad_state.is_some_and(|s| s.in_use());
        let wheel_in_use = self.wheel.as_ref().is_some_and(WheelDevice::in_use);
        let before = self.active;
        let idle = match self.active {
            DEVICE_PAD => !pad_in_use,
            DEVICE_WHEEL => !wheel_in_use,
            _ => !keys_in_use,
        };
        if idle {
            if pad_in_use {
                self.active = DEVICE_PAD;
            } else if wheel_in_use {
                self.active = DEVICE_WHEEL;
            } else if keys_in_use {
                self.active = DEVICE_KEYBOARD;
            }
        }
        if self.active == DEVICE_PAD && self.pad_state.is_none() {
            self.active = DEVICE_KEYBOARD;
        }
        if self.active == DEVICE_WHEEL && self.wheel.as_ref().is_none_or(|w| !w.connected()) {
            self.active = DEVICE_KEYBOARD;
        }
        if self.active != before {
            // a class that was not asked for a while starts afresh: the keyboard's steering
            // goes on from where the wheels point and its throttle ramps up from nothing,
            // a paddle's debouncing window is closed
            self.keyboard.int_steer = controls.steer;
            self.keyboard.int_gas = 0.0;
            if let Some(wheel) = &mut self.wheel {
                wheel.control.shift_up_trigger.accumulator = 0.0;
                wheel.control.shift_dn_trigger.accumulator = 0.0;
            }
        }
    }
}

impl DriverSource for LiveSource {
    fn take_events(&mut self) -> (u32, i32) {
        (std::mem::take(&mut self.pending_events), std::mem::take(&mut self.pending_bias))
    }

    fn wants_probe(&self) -> bool {
        // only the keyboard's class looks at the car
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        let focused = self.focused();
        let key_down: &dyn Fn(i32) -> bool = &|key| focused && key_is_down(key);
        self.pad_state = self.xinput.poll();
        if let Some(wheel) = &mut self.wheel {
            wheel.poll();
        }
        let state = self.pad_state;
        self.meta_buttons(state.as_ref(), false);
        self.choose(controls, self.keyboard.in_use(key_down));
        let mut extra = Extra::default();
        match self.active {
            DEVICE_PAD => {
                let state = self.pad_state.unwrap_or_default();
                self.pad.acquire_controls(&state, controls, &mut extra, input, key_down);
            }
            DEVICE_WHEEL => {
                if let Some(wheel) = &mut self.wheel {
                    wheel.acquire_controls(controls, &mut extra, dt, input, key_down);
                }
            }
            _ => self.keyboard.acquire_controls(controls, &mut extra, dt, input, &self.probe, key_down),
        }
        // what the game's main thread does with the extra buttons, on the press
        let last = self.extra;
        for (now, before, bit) in [
            (extra.tc_up, last.tc_up, event::TC_UP),
            (extra.tc_dn, last.tc_dn, event::TC_DN),
            (extra.abs_up, last.abs_up, event::ABS_UP),
            (extra.abs_dn, last.abs_dn, event::ABS_DN),
        ] {
            if now && !before {
                self.pending_events |= bit;
            }
        }
        if extra.brake_balance_up && !last.brake_balance_up {
            self.pending_bias += 1;
        }
        if extra.brake_balance_dn && !last.brake_balance_dn {
            self.pending_bias -= 1;
        }
        if extra.clutch_pressed {
            // for the next step, and every step the clutch stays pressed
            self.pending_events |= event::MANUAL_CLUTCH;
        }
        self.extra = extra;
    }

    fn headlights(&mut self) -> bool {
        let focused = self.focused();
        let key_down: &dyn Fn(i32) -> bool = &|key| focused && key_is_down(key);
        match self.active {
            DEVICE_PAD => self.pad.get_action(4, key_down),
            DEVICE_WHEEL => self.wheel.as_ref().is_some_and(|w| w.get_action(4, key_down)),
            _ => self.keyboard.get_action(4, key_down),
        }
    }

    fn send_ff(&mut self, ff: f32, damper: f32, user_gain: f32) {
        // the pad's class counts its calls whoever drives
        if let Some([left, right]) = self.pad.send_ff() {
            if self.rumble && self.active == DEVICE_PAD {
                self.xinput.rumble(left, right);
                self.rumbling = left > 0.0 || right > 0.0;
            } else if self.rumbling {
                self.xinput.rumble(0.0, 0.0);
                self.rumbling = false;
            }
        }
        if let Some(wheel) = &mut self.wheel {
            wheel.send_ff(ff, damper, user_gain, self.active == DEVICE_WHEEL);
        }
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.pad.set_vibrations(def);
        if let Some(wheel) = &mut self.wheel {
            wheel.set_vibrations(def);
        }
    }

    fn device_id(&self) -> u32 {
        self.active
    }

    fn idle(&mut self) {
        // paused: the motors stop, the pad's pause button still works
        if self.rumbling {
            self.xinput.rumble(0.0, 0.0);
            self.rumbling = false;
        }
        if let Some(wheel) = &mut self.wheel {
            wheel.stop_ff();
        }
        let state = self.xinput.poll();
        self.meta_buttons(state.as_ref(), true);
    }
}

impl Drop for LiveSource {
    fn drop(&mut self) {
        if self.rumbling {
            self.xinput.rumble(0.0, 0.0);
        }
    }
}

impl JoypadCarControl {
    /// One of the pad's `getAction` buttons on a given button mask.
    fn get_action_on(&self, mask: u32, action: i32) -> bool {
        let section = match action {
            5 => "ACTION_CHANGE_CAMERA",
            _ => return false,
        };
        let index = pad::PAD_ACTIONS.iter().position(|a| *a == section).unwrap();
        let button: &PadButton = &self.buttons[index];
        button.is_pressed(mask, &|_| false)
    }
}
