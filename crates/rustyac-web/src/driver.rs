// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's driver: the page's keys and the Gamepad API's pad behind AC's own
//! `KeyboardCarControl` and `JoypadCarControl`, the classes the desktop game uses, so the
//! steering speed, the throttle ramp and the pad's response curve are AC's.
//!
//! The page hands in what it sees ([`Input`]): which keys are down, as Windows virtual-key
//! codes (the page translates `KeyboardEvent.code`), and the pad in XInput's units. As on
//! the desktop every device is alive and the one touched last drives.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use rustyac_game::autodrive::AutoDriver;
use rustyac_game::input::bindings::{default_ini, Bindings};
use rustyac_game::input::keyboard::KeyboardCarControl;
use rustyac_game::input::pad::{JoypadCarControl, PadState};
use rustyac_game::input::{Extra, DEVICE_KEYBOARD, DEVICE_PAD};
use rustyac_game::input_file::event;
use rustyac_game::sim::{CarProbe, DriverSource};
use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};
use rustyac_physics::track::Track;

/// What the page knows about the devices, written by the page and read every physics step.
#[derive(Default)]
pub struct Input {
    /// Windows virtual-key codes of the keys held down.
    pub keys: HashSet<i32>,
    /// The first connected pad, `None` without one.
    pub pad: Option<PadState>,
    /// Event bits (`rustyac_game::input_file::event`) asked for from outside: R, Shift+R, N.
    pub requests: u32,
    /// The line follower drives instead of the devices.
    pub autodrive: bool,
    /// The pad's two motors, 0..1, as AC's class last set them (the page plays them).
    pub rumble: [f32; 2],
    /// Presses of the pad's camera button since the page last looked.
    pub camera_toggles: u32,
}

pub type SharedInput = Arc<Mutex<Input>>;

/// The page's devices as the car's driver.
pub struct WebSource {
    input: SharedInput,
    pub bindings: Bindings,
    pad: JoypadCarControl,
    keyboard: KeyboardCarControl,
    auto: AutoDriver,
    /// The device that drives: `DEVICE_KEYBOARD` or `DEVICE_PAD`.
    active: u32,
    extra: Extra,
    pending_events: u32,
    pending_bias: i32,
    probe: CarProbe,
    /// The pad's own buttons (reset, camera) as they were in the last look.
    meta_down: [bool; 2],
    /// Steps since the reset button went down, and whether its hold has already acted.
    reset_down: Option<(u32, bool)>,
}

impl WebSource {
    /// With rustyAC's built-in bindings: AC's default keys plus WASD, and the pad layout
    /// RT gas, LT brake, left stick, Y up, X down, A clutch, LB DRS, B KERS.
    pub fn new(input: SharedInput) -> WebSource {
        let bindings = Bindings::from_ini(&default_ini(), "the built-in layout".to_string());
        let pad = JoypadCarControl::from_ini(&bindings.ini, bindings.use_legacy_gamepad_code);
        let keyboard = KeyboardCarControl::from_ini(&bindings.ini);
        WebSource {
            input,
            bindings,
            pad,
            keyboard,
            auto: AutoDriver::new(),
            active: DEVICE_KEYBOARD,
            extra: Extra::default(),
            pending_events: 0,
            pending_bias: 0,
            probe: CarProbe::default(),
            meta_down: [false; 2],
            reset_down: None,
        }
    }

    /// The pad buttons that are rustyAC's own: reset (a tap: back to the pits; held 0.6 s:
    /// back onto the track where the car is) and the camera.
    fn meta_buttons(&mut self, state: Option<&PadState>, camera_toggles: &mut u32) {
        /// 0.6 s of physics steps.
        const HOLD_STEPS: u32 = 200;
        let mask = state.map(PadState::button_mask).unwrap_or(0);
        let never: &dyn Fn(i32) -> bool = &|_| false;
        let down = [self.bindings.pad_reset.is_pressed(mask, never), self.pad.get_action(5, never) && mask != 0];
        if down[0] && !self.meta_down[0] {
            self.reset_down = Some((0, false));
        }
        match (down[0], self.reset_down) {
            (true, Some((steps, false))) if steps >= HOLD_STEPS => {
                self.pending_events |= event::TO_TRACK;
                self.reset_down = Some((steps, true));
            }
            (true, Some((steps, held))) => self.reset_down = Some((steps + 1, held)),
            (false, Some((_, held))) => {
                if !held {
                    self.pending_events |= event::RESET;
                }
                self.reset_down = None;
            }
            _ => {}
        }
        if down[1] && !self.meta_down[1] {
            *camera_toggles += 1;
        }
        self.meta_down = down;
    }
}

impl DriverSource for WebSource {
    fn take_events(&mut self) -> (u32, i32) {
        let requests = std::mem::take(&mut self.input.lock().unwrap().requests);
        (std::mem::take(&mut self.pending_events) | requests, std::mem::take(&mut self.pending_bias))
    }

    fn request(&mut self, events: u32) {
        self.pending_events |= events;
    }

    fn wants_probe(&self) -> bool {
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
        self.auto.set_probe(probe);
    }

    fn set_track(&mut self, track: &Arc<Track>) {
        self.auto.set_track(track);
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        let (keys, pad_state, autodrive) = {
            let shared = self.input.lock().unwrap();
            (shared.keys.clone(), shared.pad, shared.autodrive)
        };
        let mut camera_toggles = 0;
        self.meta_buttons(pad_state.as_ref(), &mut camera_toggles);
        if camera_toggles != 0 {
            self.input.lock().unwrap().camera_toggles += camera_toggles;
        }
        if autodrive {
            self.auto.acquire(controls, dt, input);
            self.extra = Extra::default();
            return;
        }
        let key_down: &dyn Fn(i32) -> bool = &|key| keys.contains(&key);
        // which device drives this step: the one in use; the keyboard when the pad is gone
        let pad_in_use = pad_state.is_some_and(|s| s.in_use());
        let keys_in_use = self.keyboard.in_use(key_down);
        let before = self.active;
        let idle = if self.active == DEVICE_PAD { !pad_in_use } else { !keys_in_use };
        if idle {
            if pad_in_use {
                self.active = DEVICE_PAD;
            } else if keys_in_use {
                self.active = DEVICE_KEYBOARD;
            }
        }
        if self.active == DEVICE_PAD && pad_state.is_none() {
            self.active = DEVICE_KEYBOARD;
        }
        if self.active != before {
            // as on the desktop: the keyboard's steering goes on from where the wheels point
            // and its throttle ramps up from nothing
            self.keyboard.int_steer = controls.steer;
            self.keyboard.int_gas = 0.0;
        }
        let mut extra = Extra::default();
        if self.active == DEVICE_PAD {
            self.pad.acquire_controls(&pad_state.unwrap_or_default(), controls, &mut extra, input, key_down);
        } else {
            self.keyboard.acquire_controls(controls, &mut extra, dt, input, &self.probe, key_down);
        }
        // the cockpit keys of the hybrid system, as on the desktop
        {
            let keyboard = self.active == DEVICE_KEYBOARD;
            let down = |index: usize| {
                let [ac, second] = self.bindings.hybrid_keys[index];
                key_down(second) || (keyboard && key_down(ac))
            };
            extra.engine_brake_up |= down(0);
            extra.engine_brake_dn |= down(1);
            extra.mguk_delivery_up |= down(2);
            extra.mguk_delivery_dn |= down(3);
            extra.mguk_recovery_up |= down(4);
            extra.mguk_recovery_dn |= down(5);
            extra.mguh_mode |= down(6);
        }
        let last = self.extra;
        for (up, dn, last_up, last_dn, up_bit, dn_bit) in [
            (extra.engine_brake_up, extra.engine_brake_dn, last.engine_brake_up, last.engine_brake_dn, event::ENGINE_BRAKE_UP, event::ENGINE_BRAKE_DN),
            (extra.mguk_delivery_up, extra.mguk_delivery_dn, last.mguk_delivery_up, last.mguk_delivery_dn, event::MGUK_DELIVERY_UP, event::MGUK_DELIVERY_DN),
            (extra.mguk_recovery_up, extra.mguk_recovery_dn, last.mguk_recovery_up, last.mguk_recovery_dn, event::MGUK_RECOVERY_UP, event::MGUK_RECOVERY_DN),
        ] {
            if dn && !last_dn {
                self.pending_events |= dn_bit;
            } else if up && !last_up {
                self.pending_events |= up_bit;
            }
        }
        if extra.mguh_mode && !last.mguh_mode {
            self.pending_events |= event::MGUH_MODE;
        }
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
            self.pending_events |= event::MANUAL_CLUTCH;
        }
        self.extra = extra;
    }

    fn headlights(&mut self) -> bool {
        let keys = self.input.lock().unwrap().keys.clone();
        let key_down: &dyn Fn(i32) -> bool = &|key| keys.contains(&key);
        if self.active == DEVICE_PAD {
            self.pad.get_action(4, key_down)
        } else {
            self.keyboard.get_action(4, key_down)
        }
    }

    fn send_ff(&mut self, _ff: f32, _damper: f32, _user_gain: f32) {
        // the pad's class counts its calls whoever drives
        if let Some(motors) = self.pad.send_ff() {
            let on = self.active == DEVICE_PAD;
            self.input.lock().unwrap().rumble = if on { motors } else { [0.0; 2] };
        }
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.pad.set_vibrations(def);
    }

    fn device_id(&self) -> u32 {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_keys_reach_the_car_through_the_keyboard_class() {
        let input: SharedInput = Arc::default();
        let mut source = WebSource::new(input.clone());
        let mut controls = CarControls::default();
        let car = CarControlsInput { steer_lock: 180.0, speed: 20.0 };
        // W (0x57) and D (0x44): throttle ramps up, the wheel goes right
        input.lock().unwrap().keys.extend([0x57, 0x44]);
        for _ in 0..100 {
            source.acquire(&mut controls, 0.003, &car);
        }
        assert!(controls.gas > 0.9 && controls.steer > 0.05, "{controls:?}");
        assert_eq!(source.device_id(), DEVICE_KEYBOARD);
        // the pad takes over when its trigger is pulled
        input.lock().unwrap().keys.clear();
        input.lock().unwrap().pad = Some(PadState { right_trigger: 255, thumb_lx: -32768, ..PadState::default() });
        for _ in 0..100 {
            source.acquire(&mut controls, 0.003, &car);
        }
        assert_eq!(source.device_id(), DEVICE_PAD);
        assert!(controls.gas == 1.0 && controls.steer < -0.05, "{controls:?}");
        // R asks for the pits
        input.lock().unwrap().requests |= event::RESET;
        assert_eq!(source.take_events().0 & event::RESET, event::RESET);
        assert_eq!(source.take_events().0, 0);
    }
}
