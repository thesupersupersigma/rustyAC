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
use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::keyboard::KeyboardCarControl;
use rustyac_game::input::pad::{JoypadCarControl, PadState};
use rustyac_game::input::{Extra, ResetCombo, DEVICE_KEYBOARD, DEVICE_PAD};
use rustyac_game::input_file::event;
use rustyac_game::sim::{CarProbe, DriverSource, DT};
use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};
use rustyac_physics::track::Track;

/// What the page knows about the devices, written by the page and read every physics step.
#[derive(Default)]
pub struct Input {
    /// Windows virtual-key codes of the keys held down.
    pub keys: HashSet<i32>,
    /// The first connected pad, `None` without one.
    pub pad: Option<PadState>,
    /// Event bits (`rustyac_game::input_file::event`) asked for from outside: R, Shift+R, N,
    /// G, and the page's keys of the aids (T, Y with Shift for "down").
    pub requests: u32,
    /// Clicks of the cockpit's brake-bias control asked for from outside: the page's ] and [.
    pub bias_clicks: i32,
    /// The line follower drives instead of the devices.
    pub autodrive: bool,
    /// The pad's two motors, 0..1, as AC's class last set them (the page plays them).
    pub rumble: [f32; 2],
    /// Presses of the pad's camera button since the page last looked.
    pub camera_toggles: u32,
    /// New settings from the page's Controls panel: the classes are built again from them
    /// before the next physics step.
    pub controls: Option<ControlsIni>,
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
    /// The pad's camera button as it was in the last look.
    camera_down: bool,
    /// The reset button and its second layer (held: the D-pad's up / down are the ABS's).
    reset: ResetCombo,
}

impl WebSource {
    /// With rustyAC's built-in bindings: AC's default keys plus WASD, and the pad layout
    /// RT gas, LT brake, left stick, Y up, X down, A clutch, LB DRS, B KERS.
    pub fn new(input: SharedInput) -> WebSource {
        WebSource::with_controls(input, &default_ini(), "the built-in layout")
    }

    /// With the settings of a `controls.ini`, read as the desktop reads it.
    pub fn with_controls(input: SharedInput, ini: &ControlsIni, origin: &str) -> WebSource {
        let bindings = Bindings::from_ini(ini, origin.to_string());
        let pad = JoypadCarControl::from_ini(&bindings.ini, bindings.use_legacy_gamepad_code);
        // (the browser has no mouse steering: MOUSE_STEER would leave the car unsteered)
        let keyboard = KeyboardCarControl { mouse_steering: false, ..KeyboardCarControl::from_ini(&bindings.ini) };
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
            camera_down: false,
            reset: ResetCombo::default(),
        }
    }

    /// Builds AC's two classes again from new settings, while driving. What the classes
    /// carry from one step to the next stays: where the keyboard's steering and throttle are,
    /// the pad's last state and its motors.
    pub fn set_controls(&mut self, ini: &ControlsIni, origin: &str) {
        let bindings = Bindings::from_ini(ini, origin.to_string());
        let pad = JoypadCarControl::from_ini(&bindings.ini, bindings.use_legacy_gamepad_code);
        // (the browser has no mouse steering: MOUSE_STEER would leave the car unsteered)
        let keyboard = KeyboardCarControl { mouse_steering: false, ..KeyboardCarControl::from_ini(&bindings.ini) };
        self.pad = JoypadCarControl {
            last_max_steer: self.pad.last_max_steer,
            last_raw: self.pad.last_raw,
            last_normalized: self.pad.last_normalized,
            ff_counter: self.pad.ff_counter,
            current_vibrations: self.pad.current_vibrations,
            state: self.pad.state,
            ..pad
        };
        self.keyboard = KeyboardCarControl {
            min_steering: self.keyboard.min_steering,
            steer_decelerator: self.keyboard.steer_decelerator,
            speed_decelering_factor: self.keyboard.speed_decelering_factor,
            turn_decelering_factor: self.keyboard.turn_decelering_factor,
            int_gas: self.keyboard.int_gas,
            int_steer: self.keyboard.int_steer,
            old_steer: self.keyboard.old_steer,
            understeer_steer_correction: self.keyboard.understeer_steer_correction,
            current_spline_pos: self.keyboard.current_spline_pos,
            line: self.keyboard.line.take(),
            ..keyboard
        };
        self.bindings = bindings;
    }

    /// Every key the layout uses (Windows virtual-key codes): the keyboard's two tables, the
    /// hybrid cockpit's keys, and the keys that work beside the pad's buttons.
    pub fn keys_used(&self) -> Vec<i32> {
        let beside = self.pad.buttons.iter().chain([&self.pad.clutch]).filter(|b| b.allow_keyboard_input).map(|b| b.key);
        let mut keys: Vec<i32> =
            [self.keyboard.keys, self.keyboard.keys2].iter().flat_map(|k| k.named().map(|(_, code)| code)).chain(self.bindings.hybrid_keys.iter().flatten().copied()).chain(beside).filter(|code| *code > 0).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// The pad buttons that are rustyAC's own: the camera, and reset (Back). What Back does
    /// is decided when it comes up ([`ResetCombo`]): if the D-pad's up or down went down
    /// while it was held, nothing (it was the ABS's second layer); else back to the pits
    /// after less than 0.6 s, back onto the track where the car is after more.
    fn meta_buttons(&mut self, state: Option<&PadState>, camera_toggles: &mut u32) {
        let mask = state.map(PadState::button_mask).unwrap_or(0);
        let never: &dyn Fn(i32) -> bool = &|_| false;
        let reset = self.bindings.pad_reset.is_pressed(mask, never);
        if state.is_none() {
            // a pad that went away let go of nothing
            self.reset.cancel();
        } else {
            // (one look per physics step)
            self.pending_events |= self.reset.update(reset, self.pad.tc_buttons_on(mask), DT as f64);
        }
        let camera = self.pad.get_action(5, never) && mask != 0;
        if camera && !self.camera_down {
            *camera_toggles += 1;
        }
        self.camera_down = camera;
    }
}

impl DriverSource for WebSource {
    fn take_events(&mut self) -> (u32, i32) {
        let (requests, clicks) = {
            let mut input = self.input.lock().unwrap();
            (std::mem::take(&mut input.requests), std::mem::take(&mut input.bias_clicks))
        };
        (std::mem::take(&mut self.pending_events) | requests, std::mem::take(&mut self.pending_bias) + clicks)
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
        self.keyboard.line = rustyac_game::input::keyboard::ai_line(track);
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        let (keys, pad_state, autodrive, new_controls) = {
            let mut shared = self.input.lock().unwrap();
            (shared.keys.clone(), shared.pad, shared.autodrive, shared.controls.take())
        };
        if let Some(ini) = new_controls {
            self.set_controls(&ini, "the page's Controls panel");
        }
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
            self.keyboard.acquire_controls(controls, &mut extra, dt, input, &self.probe, key_down, &mut rustyac_game::input::keyboard::NoMouse);
        }
        // Back held: the D-pad's up / down are the ABS's
        self.reset.apply(&mut extra);
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
    use rustyac_game::input::Extra;

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
        // the keys the page keeps from the browser: the arrows, WASD, Left Ctrl ... but not T
        let used = source.keys_used();
        assert!([0x26, 0x57, 0xa2, 0xa0, 0x20, 0x4d].iter().all(|key| used.contains(key)) && !used.contains(&0x54) && !used.contains(&0x74), "{used:x?}");
        // the page's ] and [ are clicks of the brake bias
        input.lock().unwrap().bias_clicks += 2;
        assert_eq!(source.take_events(), (0, 2));
        assert_eq!(source.take_events(), (0, 0));
    }

    /// One step of a tape: the keys held, the pad (none: unplugged) and the car's speed.
    struct TapeStep {
        keys: Vec<i32>,
        pad: Option<PadState>,
        speed: f32,
    }

    /// A tape of keys and pad that uses both devices in turn: pad sweeps with the triggers
    /// and every button, key presses of every key the layout has, the pad unplugged for a
    /// while, the speed going up and down. The numbers come from a fixed generator.
    fn tape(keys_in_use: &[i32], steps: usize) -> Vec<TapeStep> {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut out = Vec::with_capacity(steps);
        let (mut keys, mut pad) = (Vec::new(), PadState { right_trigger: 255, ..PadState::default() });
        let mut device = 0;
        for step in 0..steps {
            // a new stretch every 40 steps: the pad, the keys, or nothing
            if step % 40 == 0 {
                device = if step == 0 { 0 } else { next() % 3 };
            }
            if step % 7 == 0 && step > 0 {
                keys.clear();
                pad = PadState::default();
                match device {
                    0 => {
                        pad.thumb_lx = (next() % 65536) as i32 as i16;
                        pad.thumb_rx = (next() % 65536) as i32 as i16;
                        pad.left_trigger = (next() % 256) as u8;
                        pad.right_trigger = (next() % 256) as u8;
                        pad.buttons = if next() % 3 == 0 { 1 << (next() % 16) } else { 0 };
                        if next() % 5 == 0 {
                            keys.push(keys_in_use[next() as usize % keys_in_use.len()]);
                        }
                    }
                    1 => {
                        for _ in 0..next() % 4 {
                            keys.push(keys_in_use[next() as usize % keys_in_use.len()]);
                        }
                    }
                    _ => {}
                }
            }
            let unplugged = (2000..2300).contains(&step);
            out.push(TapeStep { keys: keys.clone(), pad: (!unplugged).then_some(pad), speed: 40.0 + 40.0 * ((step as f32) * 0.002).sin() });
        }
        out
    }

    /// The desktop's side: AC's classes built from the file as `LiveSource::new` builds them
    /// and asked as `LiveSource::acquire` asks them (which device drives, the hand-over to the
    /// keyboard). `LiveSource` itself reads the real keyboard and XInput, so it cannot be fed
    /// a tape; the classes and the rule are the same code.
    struct Desktop {
        pad: JoypadCarControl,
        keyboard: KeyboardCarControl,
        active: u32,
    }

    impl Desktop {
        fn new(ini: &ControlsIni, pad_there: bool) -> Desktop {
            let bindings = Bindings::from_ini(ini, "the desktop".to_string());
            let pad = JoypadCarControl::from_ini(&bindings.ini, bindings.use_legacy_gamepad_code);
            // (the browser has no mouse steering: MOUSE_STEER would leave the car unsteered)
            let keyboard = KeyboardCarControl { mouse_steering: false, ..KeyboardCarControl::from_ini(&bindings.ini) };
            let active = if pad_there && bindings.input_method != "KEYBOARD" { DEVICE_PAD } else { DEVICE_KEYBOARD };
            Desktop { pad, keyboard, active }
        }

        fn acquire(&mut self, step: &TapeStep, controls: &mut CarControls, dt: f32, input: &CarControlsInput, probe: &CarProbe) {
            let key_down: &dyn Fn(i32) -> bool = &|key| step.keys.contains(&key);
            let pad_in_use = step.pad.is_some_and(|s| s.in_use());
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
            if self.active == DEVICE_PAD && step.pad.is_none() {
                self.active = DEVICE_KEYBOARD;
            }
            if self.active != before {
                self.keyboard.int_steer = controls.steer;
                self.keyboard.int_gas = 0.0;
            }
            let mut extra = Extra::default();
            if self.active == DEVICE_PAD {
                self.pad.acquire_controls(&step.pad.unwrap_or_default(), controls, &mut extra, input, key_down);
            } else {
                self.keyboard.acquire_controls(controls, &mut extra, dt, input, probe, key_down, &mut rustyac_game::input::keyboard::NoMouse);
            }
        }
    }

    /// Feeds one tape to the desktop's classes and to the browser's driver, both built from
    /// the same `controls.ini`, and wants the same car controls after every step.
    fn same_as_the_desktop(text: &[u8], what: &str) {
        let ini = crate::controls::from_file(text).unwrap_or_else(|why| panic!("{what}: {why}"));
        let input: SharedInput = Arc::default();
        let mut web = WebSource::with_controls(input.clone(), &ini, what);
        let keys_in_use = web.keys_used();
        let steps = tape(&keys_in_use, 6000);
        let mut desktop = Desktop::new(&ini, steps[0].pad.is_some());
        let (mut ours, mut theirs) = (CarControls::default(), CarControls::default());
        let probe = CarProbe { driven_left_slip: 0.4, driven_right_slip: 0.6, optimal_brake: 0.8, ..CarProbe::default() };
        web.set_probe(&probe);
        let (mut pad_steps, mut key_steps, mut moved) = (0, 0, 0);
        for (n, step) in steps.iter().enumerate() {
            let car = CarControlsInput { steer_lock: 240.0, speed: step.speed };
            {
                let mut shared = input.lock().unwrap();
                shared.keys = step.keys.iter().copied().collect();
                shared.pad = step.pad;
            }
            web.acquire(&mut ours, 0.003, &car);
            desktop.acquire(step, &mut theirs, 0.003, &car, &probe);
            assert_eq!(format!("{ours:?}"), format!("{theirs:?}"), "{what}: step {n}, keys {:x?}, pad {:?}", step.keys, step.pad);
            assert_eq!(ours.steer.to_bits(), theirs.steer.to_bits(), "{what}: step {n}");
            assert_eq!(web.device_id(), desktop.active, "{what}: step {n}");
            if desktop.active == DEVICE_PAD {
                pad_steps += 1;
            } else {
                key_steps += 1;
            }
            if ours.steer.abs() > 0.05 && ours.gas > 0.05 {
                moved += 1;
            }
        }
        assert!(pad_steps > 500 && key_steps > 500, "{what}: the tape used the pad for {pad_steps} steps and the keys for {key_steps}");
        assert!(moved > 200, "{what}: the tape steered and accelerated in {moved} steps only");
    }

    #[test]
    fn with_a_controls_ini_the_browser_drives_as_the_desktop_does() {
        // the built-in layout's numbers, then a file that changes every value the two classes
        // read (the pad's with the right stick, the keyboard's with other keys)
        same_as_the_desktop(default_ini().to_text().as_bytes(), "the built-in layout");
        let file = "[HEADER]\nINPUT_METHOD=X360\n\
                    [X360]\nSTEER_THUMB=RIGHT\nSTEER_GAMMA=1.4\nSTEER_FILTER=0.25\nSPEED_SENSITIVITY=0\nSTEER_DEADZONE=0.12\nSTEER_SPEED=0.95\nRUMBLE_INTENSITY=1\n\
                    [KEYBOARD]\nSTEERING_SPEED=3.1\nSTEERING_OPPOSITE_DIRECTION_SPEED=4\nSTEER_GAIN=0.3\nSTEER_RESET_SPEED=0.6\nLOOKAHEAD_POINTS=9\nGAS=0x49\nBRAKE=0x4B\nLEFT=0x4A\nRIGHT=0x4C\n\
                    [ADVANCED]\nCOMBINE_WITH_KEYBOARD_CONTROL=1\n\
                    [GEARUP]\nKEY=0x58\nXBOXBUTTON=B\n[GEARDN]\nKEY=0x5A\nXBOXBUTTON=A\n[KERS]\nKEY=0x4F\nXBOXBUTTON=RSHOULDER\n[DRS]\nKEY=0x50\nXBOXBUTTON=Y\n\
                    [HANDBRAKE]\nKEY=0x48\nXBOXBUTTON=X\n[RUSTYAC]\nUSE_LEGACY_GAMEPAD_CODE=1\n";
        same_as_the_desktop(file.as_bytes(), "a changed file");
        same_as_the_desktop(&file.replace("USE_LEGACY_GAMEPAD_CODE=1", "USE_LEGACY_GAMEPAD_CODE=0").into_bytes(), "a changed file, the newer speed rule");
        // and the player's own file where this machine has one (only ever read)
        // (a file there that is no controls.ini says nothing about this code)
        if let Some(bytes) = rustyac_game::input::bindings::ac_controls_path().and_then(|path| std::fs::read(path).ok()).filter(|bytes| crate::controls::from_file(bytes).is_ok()) {
            same_as_the_desktop(&bytes, "this machine's Documents\\Assetto Corsa\\cfg\\controls.ini");
        }
    }

    #[test]
    fn new_settings_take_over_while_driving() {
        let input: SharedInput = Arc::default();
        let mut source = WebSource::new(input.clone());
        let mut controls = CarControls::default();
        let car = CarControlsInput { steer_lock: 180.0, speed: 60.0 };
        // the stick fully right at 216 km/h: the speed sensitivity holds the steering back
        input.lock().unwrap().pad = Some(PadState { thumb_lx: 32767, ..PadState::default() });
        for _ in 0..2000 {
            source.acquire(&mut controls, 0.003, &car);
        }
        let held_back = controls.steer;
        assert!(held_back > 0.1 && held_back < 0.6, "{held_back}");
        // the panel turns it off: the same classes, built again, go on from where the wheel is
        let mut ini = crate::controls::defaults();
        crate::controls::set(&mut ini, "X360", "SPEED_SENSITIVITY", "0").unwrap();
        input.lock().unwrap().controls = Some(ini);
        source.acquire(&mut controls, 0.003, &car);
        assert!(controls.steer > held_back && controls.steer < held_back + 0.03, "one step on from {held_back}: {}", controls.steer);
        for _ in 0..2000 {
            source.acquire(&mut controls, 0.003, &car);
        }
        assert!(controls.steer > 0.99, "{}", controls.steer);
        assert!(input.lock().unwrap().controls.is_none());
        // the defaults again are the built-in layout again
        input.lock().unwrap().controls = Some(crate::controls::defaults());
        source.acquire(&mut controls, 0.003, &car);
        let fresh = WebSource::new(Arc::default());
        assert_eq!(source.bindings.ini, fresh.bindings.ini);
        assert_eq!((source.pad.speed_sensitivity, source.pad.steer_gamma, source.keyboard.steer_speed), (fresh.pad.speed_sensitivity, fresh.pad.steer_gamma, fresh.keyboard.steer_speed));
    }

    /// XInput's bits: Back, D-pad up, D-pad down, D-pad right.
    const BACK: u16 = 0x0020;
    const UP: u16 = 0x0001;
    const DOWN: u16 = 0x0002;
    const RIGHT: u16 = 0x0008;

    /// Holds `buttons` for `steps` physics steps and returns the events and bias clicks raised.
    fn hold(source: &mut WebSource, input: &SharedInput, buttons: u16, steps: u32) -> (u32, i32) {
        let mut controls = CarControls::default();
        let car = CarControlsInput { steer_lock: 180.0, speed: 20.0 };
        input.lock().unwrap().pad = Some(PadState { buttons, ..PadState::default() });
        let mut out = (0, 0);
        for _ in 0..steps {
            source.acquire(&mut controls, 0.003, &car);
            let (events, clicks) = source.take_events();
            out = (out.0 | events, out.1 + clicks);
        }
        out
    }

    #[test]
    fn the_d_pad_sets_tc_and_bias_and_with_back_held_the_abs() {
        let input: SharedInput = Arc::default();
        let mut source = WebSource::new(input.clone());
        // plain D-pad: up / down traction control, right brake bias forward
        assert_eq!(hold(&mut source, &input, UP, 5), (event::TC_UP, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, DOWN, 5), (event::TC_DN, 0));
        assert_eq!(hold(&mut source, &input, RIGHT, 5), (0, 1));
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
        // Back held, D-pad up, then down: the ABS, once per press, and never the traction control
        assert_eq!(hold(&mut source, &input, BACK, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, BACK | UP, 5), (event::ABS_UP, 0));
        assert_eq!(hold(&mut source, &input, BACK, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, BACK | DOWN, 5), (event::ABS_DN, 0));
        // Back let go first while the D-pad is still down: no traction-control press appears
        assert_eq!(hold(&mut source, &input, DOWN, 5), (0, 0));
        // and Back's own release did nothing: it was the second layer, however long it took
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, BACK, 300), (0, 0));
        assert_eq!(hold(&mut source, &input, BACK | UP, 5), (event::ABS_UP, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
    }

    #[test]
    fn back_alone_is_decided_when_it_comes_up() {
        let input: SharedInput = Arc::default();
        let mut source = WebSource::new(input.clone());
        // a tap: nothing while it is down, the pits when it comes up
        assert_eq!(hold(&mut source, &input, BACK, 50), (0, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (event::RESET, 0));
        // held for 0.6 s (200 steps) or more: nothing while it is down, back onto the track after
        assert_eq!(hold(&mut source, &input, BACK, 260), (0, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (event::TO_TRACK, 0));
        // the D-pad a moment before Back (two thumbs): its traction-control step has happened,
        // no ABS step follows, and Back does nothing when it comes up
        assert_eq!(hold(&mut source, &input, UP, 2), (event::TC_UP, 0));
        assert_eq!(hold(&mut source, &input, UP | BACK, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, UP, 5), (0, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
        // a pad that goes away while Back is held lets go of nothing: no reset
        assert_eq!(hold(&mut source, &input, BACK, 300), (0, 0));
        input.lock().unwrap().pad = None;
        let mut controls = CarControls::default();
        source.acquire(&mut controls, 0.003, &CarControlsInput { steer_lock: 180.0, speed: 20.0 });
        assert_eq!(source.take_events(), (0, 0));
        assert_eq!(hold(&mut source, &input, 0, 5), (0, 0));
    }
}
