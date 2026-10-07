// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The keyboard: AC's `KeyboardCarControl` (KeyboardCarControl.obj), ported from the
//! disassembly (`re/scratch/task11/spec_pad_keyboard.md` section 5): the steering that moves
//! towards a speed-dependent limit, the throttle that ramps and backs off when the driven
//! tyres slip, the brake that goes straight to the pedal the tyres can take.
//!
//! Not AC's: the look-ahead on the AI line (there is no track; its term is zero here, see
//! [`KeyboardCarControl::step_steer`]), mouse steering (not ported), a second key per
//! action and a clutch key.

use rustyac_physics::car::{CarControls, CarControlsInput};

use super::ini::ControlsIni;
use super::{CarProbe, Extra};
use crate::crt::expf;

/// The key codes (Windows virtual keys; 0 or -1 = none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keys {
    pub gas: i32,
    pub brake: i32,
    pub left: i32,
    pub right: i32,
    pub gear_up: i32,
    pub gear_down: i32,
    pub handbrake: i32,
    pub kers: i32,
    pub drs: i32,
    pub headlights: i32,
    pub brake_balance_up: i32,
    pub brake_balance_dn: i32,
    pub abs_up: i32,
    pub abs_dn: i32,
    pub tc_up: i32,
    pub tc_dn: i32,
    /// Not AC's: a clutch key (pedal to the floor while held).
    pub clutch: i32,
}

impl Keys {
    /// No key at all.
    pub const NONE: Keys = Keys {
        gas: 0,
        brake: 0,
        left: 0,
        right: 0,
        gear_up: 0,
        gear_down: 0,
        handbrake: 0,
        kers: 0,
        drs: 0,
        headlights: 0,
        brake_balance_up: 0,
        brake_balance_dn: 0,
        abs_up: 0,
        abs_dn: 0,
        tc_up: 0,
        tc_dn: 0,
        clutch: 0,
    };

    /// The keys of `KeyboardCarControl`'s constructor: nothing but headlights on Backspace
    /// and the handbrake on F9 (its horn on Tab has no use here).
    pub const AC_DEFAULT: Keys = Keys { headlights: 8, handbrake: 0x78, ..Keys::NONE };

    /// `KeyboardCarControl::readFromIni` @ 0x14010ff60 on top of the constructor's keys.
    pub fn from_ini(ini: &ControlsIni) -> Keys {
        let mut keys = Keys::AC_DEFAULT;
        // stored as read, -1 when missing
        keys.gas = ini.get_hex("KEYBOARD", "GAS");
        keys.brake = ini.get_hex("KEYBOARD", "BRAKE");
        keys.left = ini.get_hex("KEYBOARD", "LEFT");
        keys.right = ini.get_hex("KEYBOARD", "RIGHT");
        keys.gear_up = ini.get_hex("GEARUP", "KEY");
        keys.gear_down = ini.get_hex("GEARDN", "KEY");
        // the handbrake keeps F9 for a missing key and for -1
        let handbrake = ini.get_hex("HANDBRAKE", "KEY");
        if handbrake >= 0 {
            keys.handbrake = handbrake;
        }
        // the others keep the constructor's value unless the file names a key
        let optional = |slot: &mut i32, section: &str| {
            let value = ini.get_hex(section, "KEY");
            if value != -1 {
                *slot = value;
            }
        };
        optional(&mut keys.kers, "KERS");
        optional(&mut keys.drs, "DRS");
        optional(&mut keys.headlights, "ACTION_HEADLIGHTS");
        optional(&mut keys.brake_balance_up, "BALANCEUP");
        optional(&mut keys.brake_balance_dn, "BALANCEDN");
        optional(&mut keys.abs_up, "ABSUP");
        optional(&mut keys.abs_dn, "ABSDN");
        optional(&mut keys.tc_up, "TCUP");
        optional(&mut keys.tc_dn, "TCDN");
        optional(&mut keys.clutch, "__EXT_KEYBOARD_CLUTCH");
        keys
    }

    /// The same set of keys, read from the sections of rustyAC's second keys (`[RUSTYAC_KEYS_2]`).
    pub fn second_from_ini(ini: &ControlsIni) -> Keys {
        let key = |name: &str| ini.get_hex("RUSTYAC_KEYS_2", name);
        Keys {
            gas: key("GAS"),
            brake: key("BRAKE"),
            left: key("LEFT"),
            right: key("RIGHT"),
            gear_up: key("GEARUP"),
            gear_down: key("GEARDN"),
            handbrake: key("HANDBRAKE"),
            kers: key("KERS"),
            drs: key("DRS"),
            headlights: key("ACTION_HEADLIGHTS"),
            brake_balance_up: key("BALANCEUP"),
            brake_balance_dn: key("BALANCEDN"),
            abs_up: key("ABSUP"),
            abs_dn: key("ABSDN"),
            tc_up: key("TCUP"),
            tc_dn: key("TCDN"),
            clutch: key("CLUTCH"),
        }
    }

    /// Every key with the name of what it does.
    pub fn named(&self) -> [(&'static str, i32); 17] {
        [
            ("GAS", self.gas),
            ("BRAKE", self.brake),
            ("LEFT", self.left),
            ("RIGHT", self.right),
            ("GEARUP", self.gear_up),
            ("GEARDN", self.gear_down),
            ("HANDBRAKE", self.handbrake),
            ("KERS", self.kers),
            ("DRS", self.drs),
            ("ACTION_HEADLIGHTS", self.headlights),
            ("BALANCEUP", self.brake_balance_up),
            ("BALANCEDN", self.brake_balance_dn),
            ("ABSUP", self.abs_up),
            ("ABSDN", self.abs_dn),
            ("TCUP", self.tc_up),
            ("TCDN", self.tc_dn),
            ("CLUTCH", self.clutch),
        ]
    }
}

/// AC's `KeyboardCarControl`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyboardCarControl {
    pub min_steering: f32,
    /// `STEERING_SPEED`: steering fraction per second at a standstill.
    pub steer_speed: f32,
    pub steer_opposite_direction_factor: f32,
    pub steer_gain: f32,
    pub steer_reset_factor: f32,
    pub look_ahead: f32,
    pub steer_decelerator: f32,
    pub speed_decelering_factor: f32,
    pub turn_decelering_factor: f32,
    pub int_gas: f32,
    pub int_steer: f32,
    /// Pedal travel per second (4.0; not in the ini).
    pub gas_pedal_speed: f32,
    pub old_steer: f32,
    pub keys: Keys,
    /// Not AC's: a second key for each action (WASD beside the arrows).
    pub keys2: Keys,
}

fn sat(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// The machine code's sign: NaN gives -1.
fn sgn(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

impl KeyboardCarControl {
    /// `KeyboardCarControl::KeyboardCarControl` @ 0x14010f0e0: the values without a file.
    pub fn defaults() -> KeyboardCarControl {
        KeyboardCarControl {
            min_steering: 0.0,
            steer_speed: 1.1,
            steer_opposite_direction_factor: 2.0,
            steer_gain: 0.18,
            steer_reset_factor: 1.5,
            look_ahead: 8.0,
            steer_decelerator: 1.0,
            speed_decelering_factor: 1.0,
            turn_decelering_factor: 0.0,
            int_gas: 0.0,
            int_steer: 0.0,
            gas_pedal_speed: 4.0,
            old_steer: 0.0,
            keys: Keys::AC_DEFAULT,
            keys2: Keys::NONE,
        }
    }

    /// The constructor followed by `readFromIni` @ 0x14010ff60 on a file that exists: a
    /// missing number reads as 0 (a missing `STEERING_SPEED` means no steering, as in the game).
    pub fn from_ini(ini: &ControlsIni) -> KeyboardCarControl {
        KeyboardCarControl {
            steer_speed: ini.get_float("KEYBOARD", "STEERING_SPEED"),
            steer_opposite_direction_factor: ini.get_float("KEYBOARD", "STEERING_OPPOSITE_DIRECTION_SPEED"),
            steer_gain: ini.get_float("KEYBOARD", "STEER_GAIN"),
            steer_reset_factor: ini.get_float("KEYBOARD", "STEER_RESET_SPEED"),
            look_ahead: ini.get_float("KEYBOARD", "LOOKAHEAD_POINTS"),
            keys: Keys::from_ini(ini),
            keys2: Keys::second_from_ini(ini),
            ..KeyboardCarControl::defaults()
        }
    }

    /// `KeyboardCarControl::stepSteer` @ 0x140111650: in the game, a steering wish from a
    /// point ahead on the AI line; only its size is used, and only to raise the steering
    /// limit. Without a track there is no line. (On a track without one the game uses the
    /// world's origin as that point, and on a track with one a frozen index makes it follow
    /// the line's first points: neither is a behaviour worth having.) rustyAC's term is 0,
    /// so the limit is the speed rule alone. **Approximated.**
    pub fn step_steer(&self) -> f32 {
        0.0
    }

    /// `KeyboardCarControl::steeringMovement` @ 0x140111360.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn steering_movement(&mut self, target: f32, dt: f32, speed: f32) -> f32 {
        let cur = self.int_steer;
        let diff = target - cur;
        let step_max = dt * self.steer_speed;
        if !(diff.abs() >= step_max) {
            // close enough: there
            self.int_steer = target;
        } else if !(target < 0.0 || target > 0.0) {
            // no key: back to the centre, and stop there
            let new = (sgn(diff) * step_max) * self.steer_reset_factor + cur;
            self.int_steer = new;
            if sgn(cur) != sgn(new) {
                self.int_steer = 0.0;
            }
        } else if sgn(cur) == sgn(target) || cur == 0.0 {
            // further the same way: slower at speed, and slower towards a small limit
            let v = speed;
            let e = expf(v * -0.05);
            let a = 2.0 / (e + 1.0) - 1.0;
            let sp = 1.0 - if a >= 0.0 { a } else { 0.0 };
            self.speed_decelering_factor = if sp > 1.0 {
                1.0
            } else if sp >= 0.1 {
                sp
            } else {
                0.1
            };
            if v * 3.6 > 100.0 {
                self.steer_decelerator = self.speed_decelering_factor;
            } else {
                let t = target.abs() * 0.636_619_75;
                self.turn_decelering_factor = if t > 1.0 {
                    1.0
                } else if t >= 0.1 {
                    t
                } else {
                    0.1
                };
                self.steer_decelerator = (self.turn_decelering_factor + self.speed_decelering_factor) * 0.5;
            }
            self.int_steer = (sgn(target - cur) * step_max) * self.steer_decelerator + cur;
        } else {
            // against the current steering
            self.int_steer = (sgn(diff) * step_max) * self.steer_opposite_direction_factor + cur;
        }
        let r = self.int_steer;
        self.int_steer = if r > 1.0 {
            1.0
        } else if !(r >= -1.0) {
            -1.0
        } else {
            r
        };
        self.int_steer
    }

    /// `KeyboardCarControl::getKeyboardSteering` @ 0x14010faa0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_keyboard_steering(&mut self, dt: f32, input: &CarControlsInput, left: bool, right: bool) -> f32 {
        self.min_steering = (1.0 / (input.speed * 0.5 + 1.0)) * 3.141_592_7;
        let mut m = self.step_steer().abs();
        if !(m >= self.min_steering) {
            m = self.min_steering;
        }
        let target = if left {
            m * -1.0
        } else if right {
            m
        } else {
            0.0
        };
        self.steering_movement(target, dt, input.speed)
    }

    /// `KeyboardCarControl::computeGasCoefficient` @ 0x14010f6f0: the pedal ramps up, and
    /// comes back (not below 0.65) while a driven tyre is past its grip.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn compute_gas_coefficient(&mut self, probe: &CarProbe, speed: f32, dt: f32) -> f32 {
        let step = dt * self.gas_pedal_speed;
        // `comiss L, R` + `cmovb`: the right tyre's unless the left one's is at least as large (a NaN: the right)
        let slip = if !(probe.driven_left_slip >= probe.driven_right_slip) { probe.driven_right_slip } else { probe.driven_left_slip };
        let kmh = speed * 3.6;
        let slipping = if !(kmh >= 100.0) { slip as f64 > 0.99 } else { slip > 2.0 };
        if slipping {
            let g = self.int_gas - step;
            self.int_gas = if g > 1.0 {
                1.0
            } else if g >= 0.65 {
                g
            } else {
                0.65
            };
        } else {
            self.int_gas = sat(step + self.int_gas);
        }
        sat(self.int_gas)
    }

    /// `KeyboardCarControl::acquireControls` @ 0x14010f2e0. `key_down` says whether a key is
    /// held; `probe` is what the game's class reads off its car.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn acquire_controls(
        &mut self,
        controls: &mut CarControls,
        extra: &mut Extra,
        dt: f32,
        input: &CarControlsInput,
        probe: &CarProbe,
        key_down: &dyn Fn(i32) -> bool,
    ) {
        let (k1, k2) = (self.keys, self.keys2);
        let key = |pick: fn(&Keys) -> i32| key_down(pick(&k1)) || key_down(pick(&k2));
        self.old_steer = controls.steer;
        let s = self.get_keyboard_steering(dt, input, key(|k| k.left), key(|k| k.right));
        controls.steer = if s > 1.0 {
            1.0
        } else if s >= -1.0 {
            s
        } else {
            -1.0
        };
        if key(|k| k.gas) {
            controls.gas = sat(self.compute_gas_coefficient(probe, input.speed, dt));
        } else {
            controls.gas = 0.0;
            self.int_gas = 0.0;
        }
        controls.brake = if key(|k| k.brake) { sat(probe.optimal_brake) } else { 0.0 };
        controls.kers = key(|k| k.kers);
        controls.drs = key(|k| k.drs);
        controls.hand_brake = key(|k| k.handbrake) as i32 as f32;
        controls.gear_up = key(|k| k.gear_up);
        controls.gear_dn = key(|k| k.gear_down);
        extra.brake_balance_up = key(|k| k.brake_balance_up);
        extra.brake_balance_dn = key(|k| k.brake_balance_dn);
        extra.abs_dn = key(|k| k.abs_dn);
        extra.abs_up = key(|k| k.abs_up);
        extra.tc_dn = key(|k| k.tc_dn);
        extra.tc_up = key(|k| k.tc_up);
        // AC's keyboard never writes the clutch. With a clutch key (not AC's) it is the pedal.
        if k1.clutch > 0 || k2.clutch > 0 {
            extra.clutch_pressed = key(|k| k.clutch);
            controls.clutch = if extra.clutch_pressed { 0.0 } else { 1.0 };
        }
    }

    /// `KeyboardCarControl::getAction` @ 0x14010f820, as far as the car asks (4: headlights).
    pub fn get_action(&self, action: i32, key_down: &dyn Fn(i32) -> bool) -> bool {
        action == 4 && (key_down(self.keys.headlights) || key_down(self.keys2.headlights))
    }

    /// Is a driving key held (for the choice of the driving device)? Shift, Ctrl and Alt do
    /// not count even where they are bound (AC's own files put gear-down on Left Ctrl): a
    /// modifier pressed for a command must not take the car away from the pad.
    pub fn in_use(&self, key_down: &dyn Fn(i32) -> bool) -> bool {
        let modifier = |key: i32| (0x10..=0x12).contains(&key) || (0xa0..=0xa5).contains(&key);
        [self.keys, self.keys2]
            .iter()
            .any(|k| [k.gas, k.brake, k.left, k.right, k.gear_up, k.gear_down].into_iter().any(|key| !modifier(key) && key_down(key)))
    }

    /// Is this key one the keyboard drives with (steering, pedals, gears, clutch)?
    pub fn drives_with(&self, key: i32) -> bool {
        key > 0 && [self.keys, self.keys2].iter().any(|k| [k.gas, k.brake, k.left, k.right, k.gear_up, k.gear_down, k.clutch, k.handbrake].contains(&key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    const DT: f32 = 0.003;

    /// The user's `[KEYBOARD]` numbers.
    fn user() -> KeyboardCarControl {
        KeyboardCarControl::from_ini(&ControlsIni::parse(
            "[KEYBOARD]\nSTEERING_SPEED=1.75\nSTEERING_OPPOSITE_DIRECTION_SPEED=2.5\nSTEER_GAIN=0.18\nSTEER_RESET_SPEED=1.8\nLOOKAHEAD_POINTS=6\n\
             GAS=0x26\nBRAKE=0x28\nLEFT=0x25\nRIGHT=0x27\n[GEARUP]\nKEY=0x20\n[GEARDN]\nKEY=0xA2\n[HANDBRAKE]\nKEY=-1\n[ACTION_HEADLIGHTS]\nKEY=0x4C\n[DRS]\nKEY=-1\n",
        ))
    }

    fn input(speed: f32) -> CarControlsInput {
        CarControlsInput { steer_lock: 180.0, speed }
    }

    /// `expf` here is the C runtime's when MSVCR120.dll is on the machine, else Rust's: the
    /// numbers that go through it are held to the last bit only with the game's own.
    fn same(got: f32, expected: u32) {
        if crate::crt::is_msvcr120() {
            assert_eq!(bits(got), expected, "{got}");
        } else {
            let expected = f32::from_bits(expected);
            assert!((got - expected).abs() <= expected.abs() * 1e-6, "{got} / {expected}");
        }
    }

    #[test]
    fn keys_of_the_user_s_file() {
        let k = user().keys;
        assert_eq!((k.gas, k.brake, k.left, k.right), (0x26, 0x28, 0x25, 0x27));
        assert_eq!((k.gear_up, k.gear_down), (0x20, 0xa2));
        // KEY=-1 keeps F9 for the handbrake and leaves DRS without a key
        assert_eq!((k.handbrake, k.drs, k.headlights), (0x78, 0, 0x4c));
        // without a file: the constructor's values
        let d = KeyboardCarControl::defaults();
        assert_eq!((d.steer_speed, d.steer_opposite_direction_factor, d.steer_reset_factor, d.gas_pedal_speed), (1.1, 2.0, 1.5, 4.0));
        // a file without the numbers: zero, the car cannot be steered
        assert_eq!(KeyboardCarControl::from_ini(&ControlsIni::parse("[KEYBOARD]\n")).steer_speed, 0.0);
    }

    #[test]
    fn a_modifier_key_alone_does_not_take_the_car() {
        // the user's file: gear down is Left Ctrl, which is also what Ctrl+T is typed with
        let k = user();
        assert!(k.drives_with(0xa2) && k.drives_with(0x26) && !k.drives_with(0xa3) && !k.drives_with(0x54));
        assert!(!k.in_use(&|key| key == 0xa2), "Left Ctrl alone");
        assert!(k.in_use(&|key| key == 0x26) && k.in_use(&|key| key == 0x20), "the throttle key, the gear-up key");
        // the tyres' slip: the right one's unless the left one's is at least as large
        let mut k = user();
        k.int_gas = 1.0;
        let nan_left = CarProbe { driven_left_slip: f32::NAN, driven_right_slip: 1.5, optimal_brake: 0.0 };
        assert!(k.compute_gas_coefficient(&nan_left, 10.0, DT) < 1.0, "a NaN on the left: the right tyre's slip counts");
    }

    #[test]
    fn the_steering_limit_falls_with_speed() {
        // spec G.25
        let mut k = user();
        for (speed, expected) in [(0.0, 0x40490fdb), (10.0, 0x3f060a92), (20.0, 0x3e923a14), (30.0, 0x3e490fdb), (50.0, 0x3df775fa)] {
            k.get_keyboard_steering(DT, &input(speed), false, false);
            assert_eq!(bits(k.min_steering), expected, "{speed} m/s");
        }
    }

    #[test]
    fn steering_towards_a_key() {
        // spec G.26, G.27: at a standstill every factor is 1
        let mut k = user();
        assert_eq!(bits(DT * k.steer_speed), 0x3bac0831);
        assert_eq!(bits(k.get_keyboard_steering(DT, &input(0.0), false, true)), 0x3bac0831);
        assert_eq!(bits(k.get_keyboard_steering(DT, &input(0.0), false, true)), 0x3c2c0831);
        let mut steps = 2;
        while k.int_steer < 1.0 {
            k.get_keyboard_steering(DT, &input(0.0), false, true);
            steps += 1;
        }
        assert_eq!(steps, 191);
        // G.28: 20 m/s
        let mut k = user();
        let first = k.get_keyboard_steering(DT, &input(20.0), false, true);
        same(k.speed_decelering_factor, 0x3f09b2b0);
        assert_eq!(bits(k.turn_decelering_factor), 0x3e3a2e8c);
        same(k.steer_decelerator, 0x3eb83e53);
        same(first, 0x3af79f8a);
        let mut steps = 1;
        while k.int_steer < k.min_steering {
            k.get_keyboard_steering(DT, &input(20.0), false, true);
            steps += 1;
        }
        assert!((150..=152).contains(&steps), "{steps} steps to the limit");
        assert_eq!(k.int_steer, k.min_steering, "it stops at the limit");
        // G.29: above 100 km/h only the speed factor counts; left is the mirror image
        let mut k = user();
        let right = k.get_keyboard_steering(DT, &input(30.0), false, true);
        same(k.steer_decelerator, 0x3ebacdc4);
        same(right, 0x3afb1074);
        let mut k = user();
        assert_eq!(k.get_keyboard_steering(DT, &input(30.0), true, false), -right);
        // left wins when both are held
        let mut k = user();
        assert!(k.get_keyboard_steering(DT, &input(0.0), true, true) < 0.0);
    }

    #[test]
    fn steering_back_and_against() {
        // spec G.30: against the steering
        let mut k = user();
        k.int_steer = 0.5;
        assert_eq!(bits(k.get_keyboard_steering(DT, &input(20.0), true, false)), 0x3ef947ae);
        // G.31: no key
        for (from, expected) in [(0.5f32, 0x3efb295fu32), (-0.5, 0xbefb295f), (0.006, 0), (0.004, 0)] {
            let mut k = user();
            k.int_steer = from;
            assert_eq!(bits(k.get_keyboard_steering(DT, &input(20.0), false, false)), expected, "from {from}");
        }
        // G.32: no file
        let mut k = KeyboardCarControl::defaults();
        assert_eq!(bits(k.get_keyboard_steering(DT, &input(0.0), false, true)), 0x3b5844d1);
    }

    #[test]
    fn the_throttle_ramps_and_backs_off() {
        let grip = CarProbe { driven_left_slip: 0.5, driven_right_slip: 0.6, optimal_brake: 0.7 };
        // spec H.34
        let mut k = user();
        assert_eq!(bits(DT * k.gas_pedal_speed), 0x3c449ba6);
        let first: Vec<u32> = (0..3).map(|_| bits(k.compute_gas_coefficient(&grip, 10.0, DT))).collect();
        assert_eq!(first, [0x3c449ba6, 0x3cc49ba6, 0x3d1374bc]);
        let mut steps = 3;
        while k.int_gas < 1.0 {
            k.compute_gas_coefficient(&grip, 10.0, DT);
            steps += 1;
        }
        assert_eq!(steps, 84);
        // H.35: slipping
        let spin = CarProbe { driven_left_slip: 1.3, driven_right_slip: 0.2, optimal_brake: 0.7 };
        let down: Vec<u32> = (0..3).map(|_| bits(k.compute_gas_coefficient(&spin, 10.0, DT))).collect();
        assert_eq!(down, [0x3f7ced91, 0x3f79db22, 0x3f76c8b3]);
        for _ in 0..200 {
            k.compute_gas_coefficient(&spin, 10.0, DT);
        }
        assert_eq!(bits(k.int_gas), 0x3f266666);
        k.int_gas = 0.3;
        assert_eq!(bits(k.compute_gas_coefficient(&spin, 10.0, DT)), 0x3f266666, "from a low pedal it jumps up to 0.65");
        // H.36: the thresholds
        let slipping = |slip: f32, kmh: f32| {
            let mut k = user();
            k.int_gas = 1.0;
            k.compute_gas_coefficient(&CarProbe { driven_left_slip: slip, driven_right_slip: 0.0, optimal_brake: 0.0 }, kmh / 3.6, DT) < 1.0
        };
        assert!(slipping(0.99, 50.0), "0.99f is above the double 0.99");
        assert!(!slipping(0.98, 50.0));
        assert!(!slipping(1.5, 101.0) && !slipping(2.0, 101.0) && slipping(2.000_000_2, 101.0));
    }

    #[test]
    fn pedals_and_keys() {
        let mut k = user();
        let mut controls = CarControls { clutch: 0.37, ..CarControls::default() };
        let mut extra = Extra::default();
        let probe = CarProbe { driven_left_slip: 0.0, driven_right_slip: 0.0, optimal_brake: 0.675 };
        // gas (Up), brake (Down), gear up (Space), headlights (L)
        let held = [0x26, 0x28, 0x20, 0x4c];
        let key_down: &dyn Fn(i32) -> bool = &|key| held.contains(&key);
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &probe, key_down);
        assert_eq!(bits(controls.gas), 0x3c449ba6);
        // spec H.37: the brake is the optimal pedal, at once
        assert_eq!(controls.brake, 0.675);
        assert!(controls.gear_up && !controls.gear_dn && !controls.drs);
        assert!(k.get_action(4, key_down) && !k.get_action(5, key_down));
        // the clutch is not the keyboard's to write
        assert_eq!(controls.clutch, 0.37);
        assert_eq!(controls.requested_gear_index, -1);
        // released: no throttle, the ramp starts again; a pedal beyond 1 is 1
        let none: &dyn Fn(i32) -> bool = &|_| false;
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &probe, none);
        assert_eq!((controls.gas, controls.brake, k.int_gas), (0.0, 0.0, 0.0));
        let strong = CarProbe { optimal_brake: 1.53, ..probe };
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &strong, &|key| key == 0x28);
        assert_eq!(controls.brake, 1.0);
        let nan = CarProbe { optimal_brake: f32::NAN, ..probe };
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &nan, &|key| key == 0x28);
        assert_eq!(controls.brake, 0.0);
        // rustyAC's second keys and clutch key
        k.keys2 = Keys { gas: 0x57, clutch: 0xa0, ..Keys::NONE };
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &probe, &|key| key == 0x57 || key == 0xa0);
        assert!(controls.gas > 0.0);
        assert_eq!(controls.clutch, 0.0);
        k.acquire_controls(&mut controls, &mut extra, DT, &input(10.0), &probe, none);
        assert_eq!(controls.clutch, 1.0);
    }
}
