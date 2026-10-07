// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The Xbox pad: AC's XInput wrapper (`X360Joypad`, JoypadManager.obj) and its gamepad
//! driver `JoypadCarControl` (X360CarControl.obj), ported from the disassembly
//! (`re/scratch/task11/spec_pad_keyboard.md`). Sums and comparisons are in the machine code's
//! order; the unit tests hold the numbers computed from AC's listings.

use rustyac_math::powf;
use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};
use windows::Win32::UI::Input::XboxController::{XInputGetState, XInputSetState, XINPUT_STATE, XINPUT_VIBRATION};

use super::ini::ControlsIni;
use super::Extra;

/// `XINPUT_GAMEPAD` as read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    /// `wButtons`
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

/// AC's `JoypadButton`.
pub mod button {
    pub const NONE: u32 = 0;
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const UP: u32 = 3;
    pub const DOWN: u32 = 4;
    pub const A: u32 = 5;
    pub const B: u32 = 6;
    pub const X: u32 = 7;
    pub const Y: u32 = 8;
    pub const LEFT_SHOULDER: u32 = 9;
    pub const RIGHT_SHOULDER: u32 = 10;
    pub const LEFT_THUMB: u32 = 11;
    pub const RIGHT_THUMB: u32 = 12;
    pub const START: u32 = 13;
    pub const BACK: u32 = 14;
}

/// `X360Joypad::buttonMappings` @ 0x14151d1b0: XInput's `wButtons` bit, `JoypadButton`.
const BUTTON_MAPPINGS: [(u16, u32); 14] = [
    (0x0200, button::RIGHT_SHOULDER),
    (0x0100, button::LEFT_SHOULDER),
    (0x1000, button::A),
    (0x2000, button::B),
    (0x4000, button::X),
    (0x8000, button::Y),
    (0x0004, button::LEFT),
    (0x0008, button::RIGHT),
    (0x0001, button::UP),
    (0x0002, button::DOWN),
    (0x0040, button::LEFT_THUMB),
    (0x0080, button::RIGHT_THUMB),
    (0x0010, button::START),
    (0x0020, button::BACK),
];

/// The names `controls.ini` uses for `XBOXBUTTON` (`JoypadCarControl`'s map; case matters,
/// anything else is no button).
pub const BUTTON_NAMES: [(&str, u32); 17] = [
    ("-1", button::NONE),
    ("RSHOULDER", button::RIGHT_SHOULDER),
    ("LSHOULDER", button::LEFT_SHOULDER),
    ("A", button::A),
    ("B", button::B),
    ("X", button::X),
    ("Y", button::Y),
    ("DPAD_LEFT", button::LEFT),
    ("DPAD_RIGHT", button::RIGHT),
    ("DPAD_UP", button::UP),
    ("DPAD_DOWN", button::DOWN),
    ("LEFT_THUMB", button::LEFT_THUMB),
    ("RIGHT_THUMB", button::RIGHT_THUMB),
    ("START", button::START),
    ("BACK", button::BACK),
    ("RTHUMB_PRESS", button::RIGHT_THUMB),
    ("LTHUMB_PRESS", button::LEFT_THUMB),
];

pub fn button_from_name(name: &str) -> u32 {
    BUTTON_NAMES.iter().find(|(n, _)| *n == name).map(|(_, b)| *b).unwrap_or(button::NONE)
}

/// The name written to a bindings file.
pub fn button_name(button: u32) -> &'static str {
    match button {
        button::LEFT_THUMB => "LTHUMB_PRESS",
        button::RIGHT_THUMB => "RTHUMB_PRESS",
        other => BUTTON_NAMES.iter().find(|(_, b)| *b == other).map(|(n, _)| *n).unwrap_or("-1"),
    }
}

/// How a person calls the button.
pub fn button_label(button: u32) -> &'static str {
    match button {
        button::LEFT => "D-pad left",
        button::RIGHT => "D-pad right",
        button::UP => "D-pad up",
        button::DOWN => "D-pad down",
        button::A => "A",
        button::B => "B",
        button::X => "X",
        button::Y => "Y",
        button::LEFT_SHOULDER => "LB",
        button::RIGHT_SHOULDER => "RB",
        button::LEFT_THUMB => "left stick press",
        button::RIGHT_THUMB => "right stick press",
        button::START => "Start (Menu)",
        button::BACK => "Back (View)",
        _ => "-",
    }
}

impl PadState {
    /// `X360Joypad::getLeftStick` @ 0x140244050 (x only): no dead zone, no clamp.
    pub fn left_stick_x(&self) -> f32 {
        self.thumb_lx as i32 as f32 * 3.051_850_9e-5
    }

    /// `X360Joypad::getRightStick` @ 0x1402440b0 (x only).
    pub fn right_stick_x(&self) -> f32 {
        self.thumb_rx as i32 as f32 * 3.051_850_9e-5
    }

    /// `X360Joypad::getLeftTrigger` @ 0x140244090.
    pub fn left_trigger(&self) -> f32 {
        self.left_trigger as f32 * 0.003_921_569
    }

    /// `X360Joypad::getRightTrigger` @ 0x1402440f0.
    pub fn right_trigger(&self) -> f32 {
        self.right_trigger as f32 * 0.003_921_569
    }

    /// `X360Joypad::getButtonMask` @ 0x140244010: one bit per `JoypadButton`.
    pub fn button_mask(&self) -> u32 {
        let mut mask = 0;
        for (xinput, button) in BUTTON_MAPPINGS {
            if xinput & self.buttons != 0 {
                mask |= 1 << button;
            }
        }
        mask
    }

    /// Is anybody touching the pad (for the choice of the driving device)?
    pub fn in_use(&self) -> bool {
        self.buttons != 0
            || self.left_trigger > 30
            || self.right_trigger > 30
            || self.thumb_lx.unsigned_abs() > 12_000
            || self.thumb_rx.unsigned_abs() > 12_000
    }
}

/// `X360Joypad::setVibrations` @ 0x140244110: the motor word of a level. The game truncates
/// and keeps the low 16 bits, so a level above 1 wraps (1.5 gives 32766).
pub fn motor_word_ac(level: f32) -> u16 {
    (level * 65535.0) as i64 as u16
}

/// The same with the level kept within 0..1 first (what rustyAC sends: a wrapped word would
/// be a wrong strength, and a NaN must not reach the motor).
pub fn motor_word(level: f32) -> u16 {
    motor_word_ac(if level > 1.0 {
        1.0
    } else if level >= 0.0 {
        level
    } else {
        0.0
    })
}

/// The pads Windows offers through XInput. AC only ever reads pad 0; rustyAC takes the first
/// one that is connected.
pub struct XInput {
    /// The pad in use.
    pub index: Option<u32>,
    /// Calls since the last look for a pad (a look at empty slots is slow, so it is rare).
    since_scan: u32,
}

impl XInput {
    pub fn new() -> XInput {
        let mut xinput = XInput { index: None, since_scan: 0 };
        xinput.scan();
        xinput
    }

    fn read(index: u32) -> Option<PadState> {
        let mut state = XINPUT_STATE::default();
        // SAFETY: `state` is a valid out pointer; the index is 0..3.
        let result = unsafe { XInputGetState(index, &mut state) };
        (result == 0).then(|| {
            let g = state.Gamepad;
            PadState {
                buttons: g.wButtons.0,
                left_trigger: g.bLeftTrigger,
                right_trigger: g.bRightTrigger,
                thumb_lx: g.sThumbLX,
                thumb_ly: g.sThumbLY,
                thumb_rx: g.sThumbRX,
                thumb_ry: g.sThumbRY,
            }
        })
    }

    /// Looks at all four slots for a connected pad.
    pub fn scan(&mut self) -> Vec<u32> {
        let found: Vec<u32> = (0..4).filter(|index| XInput::read(*index).is_some()).collect();
        self.index = found.first().copied();
        self.since_scan = 0;
        found
    }

    /// The pad's state now; `None` while no pad is connected (it is looked for again about
    /// once a second).
    pub fn poll(&mut self) -> Option<PadState> {
        match self.index {
            Some(index) => {
                let state = XInput::read(index);
                if state.is_none() {
                    self.index = None;
                    self.since_scan = 0;
                }
                state
            }
            None => {
                self.since_scan += 1;
                if self.since_scan >= 333 {
                    self.scan();
                }
                None
            }
        }
    }

    /// `XInputSetState`: left = the heavy low-frequency motor, right = the light one.
    pub fn rumble(&mut self, left: f32, right: f32) {
        if let Some(index) = self.index {
            let vibration = XINPUT_VIBRATION { wLeftMotorSpeed: motor_word(left), wRightMotorSpeed: motor_word(right) };
            // SAFETY: a valid pointer to a vibration struct.
            unsafe { XInputSetState(index, &vibration) };
        }
    }
}

impl Default for XInput {
    fn default() -> XInput {
        XInput::new()
    }
}

/// AC's `SecondaryJoypadButton`: a pad button and/or a keyboard key for one action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadButton {
    /// `JoypadButton` (0 = none).
    pub button: u32,
    /// Virtual-key code, -1 = none. Only used with `[ADVANCED] COMBINE_WITH_KEYBOARD_CONTROL`.
    pub key: i32,
    pub allow_keyboard_input: bool,
}

impl PadButton {
    pub const NONE: PadButton = PadButton { button: button::NONE, key: -1, allow_keyboard_input: false };

    /// `SecondaryJoypadButton::isPressed` @ 0x1401dc300.
    pub fn is_pressed(&self, mask: u32, key_down: &dyn Fn(i32) -> bool) -> bool {
        let button_mask = 1u32 << self.button;
        if self.allow_keyboard_input && key_down(self.key) {
            return true;
        }
        mask & button_mask != 0
    }
}

/// The 27 actions of `JoypadCarControl`, in the order its constructor reads their sections,
/// plus the two rustyAC adds at the end.
pub const PAD_ACTIONS: [&str; 27] = [
    "GEARUP",
    "GEARDN",
    "HANDBRAKE",
    "GLANCELEFT",
    "GLANCERIGHT",
    "GLANCEBACK",
    "ACTION_CHANGE_CAMERA",
    "ACTION_HEADLIGHTS",
    "ACTION_HORN",
    "BALANCEDN",
    "BALANCEUP",
    "KERS",
    "DRS",
    "ACTION_HEADLIGHTS_FLASH",
    "ABSDN",
    "ABSUP",
    "TCDN",
    "TCUP",
    "TURBODN",
    "TURBOUP",
    "ENGINE_BRAKE_UP",
    "ENGINE_BRAKE_DN",
    "MGUK_DELIVERY_UP",
    "MGUK_DELIVERY_DN",
    "MGUK_RECOVERY_UP",
    "MGUK_RECOVERY_DN",
    "MGUH_MODE",
];

/// AC's `JoypadCarControl`.
#[derive(Clone, Debug, PartialEq)]
pub struct JoypadCarControl {
    pub speed_sensitivity: f32,
    pub steer_gamma: f32,
    pub steer_filter: f32,
    pub dead_zone: f32,
    /// The most the steering moves per physics step (`dt` is not used).
    pub steer_speed: f32,
    pub last_max_steer: f32,
    pub last_raw: f32,
    pub last_normalized: f32,
    /// `system/cfg/assetto_corsa.ini [GAMEPAD] USE_LEGACY_CODE`
    pub use_legacy_code: bool,
    pub is_steer_with_right: bool,
    pub rumble_intensity: f32,
    /// The buttons of [`PAD_ACTIONS`], in that order.
    pub buttons: [PadButton; 27],
    pub ff_counter: i32,
    /// Left and right motor.
    pub current_vibrations: [f32; 2],
    /// Not AC's: a clutch button (Custom Shaders Patch's `[__EXT_KEYBOARD_CLUTCH]`; the
    /// original pad driver has no clutch input and always reports the pedal up).
    pub clutch: PadButton,
    /// The pad state of the last `acquire_controls` (`getAction` reads it).
    pub state: PadState,
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

/// `JoypadCarControl::getAxisValue` @ 0x1401dc250: dead zone, then the gamma curve.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn get_axis_value(value: f32, gamma: f32, dead_zone: f32) -> f32 {
    if !(value.abs() >= dead_zone) {
        return 0.0;
    }
    let v = if value > 0.0 { value - dead_zone } else { value + dead_zone };
    let v = v / (1.0 - dead_zone);
    let p = powf(v.abs(), gamma);
    let s = if v > 0.0 {
        1.0
    } else if v >= 0.0 {
        0.0
    } else {
        -1.0
    };
    p * s
}

impl JoypadCarControl {
    /// `JoypadCarControl::JoypadCarControl` @ 0x1401d9280: the settings of `[X360]` and the
    /// buttons of the action sections. A key that is missing reads as 0 / no button, not as
    /// a default (so a missing `STEER_THUMB` means the right stick, as in the game).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn from_ini(ini: &ControlsIni, use_legacy_code: bool) -> JoypadCarControl {
        let steer_speed = {
            let x = ini.get_float("X360", "STEER_SPEED") * 0.1;
            if x > 0.1 {
                0.1
            } else if x < 0.01 {
                0.01
            } else if x.is_nan() {
                0.01
            } else {
                x
            }
        };
        let mut steer_gamma = ini.get_float("X360", "STEER_GAMMA");
        if !(1.0 < steer_gamma) {
            steer_gamma = 1.0;
        }
        let combine = ini.has_section("ADVANCED") && ini.get_int("ADVANCED", "COMBINE_WITH_KEYBOARD_CONTROL") > 0;
        let read = |section: &str| PadButton {
            button: button_from_name(ini.get_string(section, "XBOXBUTTON")),
            key: ini.get_hex(section, "KEY"),
            allow_keyboard_input: combine,
        };
        let mut buttons = [PadButton::NONE; 27];
        for (slot, section) in buttons.iter_mut().zip(PAD_ACTIONS) {
            *slot = read(section);
        }
        JoypadCarControl {
            speed_sensitivity: ini.get_float("X360", "SPEED_SENSITIVITY"),
            steer_gamma,
            steer_filter: ((ini.get_float("X360", "STEER_FILTER") * 0.99) * 3.0) * 0.333_333_34,
            dead_zone: ini.get_float("X360", "STEER_DEADZONE"),
            steer_speed,
            last_max_steer: 0.0,
            last_raw: 0.0,
            last_normalized: 0.0,
            use_legacy_code,
            is_steer_with_right: ini.get_string("X360", "STEER_THUMB") != "LEFT",
            rumble_intensity: ini.get_float("X360", "RUMBLE_INTENSITY"),
            buttons,
            ff_counter: 0,
            current_vibrations: [0.0; 2],
            clutch: read("__EXT_KEYBOARD_CLUTCH"),
            state: PadState::default(),
        }
    }

    fn button(&self, action: &str) -> &PadButton {
        &self.buttons[PAD_ACTIONS.iter().position(|a| *a == action).expect("a pad action")]
    }

    /// The speed-dependent steering limit of `acquireControls`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn max_steer(&self, input: &CarControlsInput) -> f32 {
        let kmh = input.speed * 3.6;
        if self.use_legacy_code {
            let x = sat(kmh * 10.0);
            let x2 = 1.0 * (x * x);
            let m = powf(10.0, -x2);
            if m > 1.0 {
                1.0
            } else if !(m >= self.speed_sensitivity) {
                self.speed_sensitivity
            } else {
                m
            }
        } else {
            let l = input.steer_lock;
            let t = kmh * 0.005;
            let a = (90.0 - l) * self.speed_sensitivity + l;
            let b = (20.0 - a) * self.speed_sensitivity + a;
            let t = sat(t);
            ((b - l) * t + l) / l
        }
    }

    /// `JoypadCarControl::acquireControls` @ 0x1401dbce0 with the pad's state of this step.
    /// `dt` is not used by the game: every rate is per call. `controls.steer` on entry is
    /// the car's value of the last step, which is the state of the steering filter.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn acquire_controls(
        &mut self,
        state: &PadState,
        controls: &mut CarControls,
        extra: &mut Extra,
        input: &CarControlsInput,
        key_down: &dyn Fn(i32) -> bool,
    ) {
        self.state = *state;
        let m = self.max_steer(input);
        self.last_max_steer = m;

        let lx = state.left_stick_x();
        let rx = state.right_stick_x();
        self.last_raw = lx;
        let mut a = get_axis_value(lx, self.steer_gamma, self.dead_zone);
        self.last_normalized = a;
        if self.is_steer_with_right {
            a = get_axis_value(rx, self.steer_gamma, self.dead_zone);
        }

        let prev = controls.steer;
        let delta = a * m - prev;
        let d = if delta > self.steer_speed {
            self.steer_speed
        } else if delta >= -self.steer_speed {
            delta
        } else {
            -self.steer_speed
        };
        let x = (prev + d) - prev;
        let r = (1.0 - self.steer_filter) * x + prev;
        controls.steer = if r > 1.0 {
            1.0
        } else if !(r >= -1.0) {
            -1.0
        } else {
            r
        };

        controls.gas = sat(state.right_trigger());
        controls.brake = sat(state.left_trigger());
        controls.clutch = 1.0;

        let mask = state.button_mask();
        let pressed = |action: &str| self.button(action).is_pressed(mask, key_down);
        controls.gear_up = pressed("GEARUP");
        controls.gear_dn = pressed("GEARDN");
        controls.hand_brake = pressed("HANDBRAKE") as i32 as f32;
        extra.brake_balance_up = pressed("BALANCEUP");
        extra.brake_balance_dn = pressed("BALANCEDN");
        controls.kers = pressed("KERS");
        controls.drs = pressed("DRS");
        extra.tc_dn = pressed("TCDN");
        extra.tc_up = pressed("TCUP");
        extra.abs_dn = pressed("ABSDN");
        extra.abs_up = pressed("ABSUP");
        // turbo, engine brake and the MGU buttons belong to systems that are not ported

        // not AC's: the clutch button presses the pedal to the floor
        if self.clutch.button != button::NONE && self.clutch.is_pressed(mask, key_down) {
            controls.clutch = 0.0;
            extra.clutch_pressed = true;
        }
    }

    /// `JoypadCarControl::getAction` @ 0x1401dc130 (0 glance left, 1 glance right, 4
    /// headlights, 5 camera, 6 horn, 9 glance back, 10 flash), on the state of the last
    /// `acquire_controls`.
    pub fn get_action(&self, action: i32, key_down: &dyn Fn(i32) -> bool) -> bool {
        let section = match action {
            0 => "GLANCELEFT",
            1 => "GLANCERIGHT",
            4 => "ACTION_HEADLIGHTS",
            5 => "ACTION_CHANGE_CAMERA",
            6 => "ACTION_HORN",
            9 => "GLANCEBACK",
            10 => "ACTION_HEADLIGHTS_FLASH",
            _ => return false,
        };
        self.button(section).is_pressed(self.state.button_mask(), key_down)
    }

    /// `JoypadCarControl::setVibrations` @ 0x1401dc3c0: the kerb level switches the left
    /// motor fully on, the tyre slip drives the right one. Road and ABS levels are not used.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn set_vibrations(&mut self, v: &VibrationDef) {
        self.current_vibrations[0] = if !(0.05 >= v.curbs) {
            self.rumble_intensity
        } else if !self.use_legacy_code {
            0.0
        } else {
            (self.rumble_intensity * v.engine) * 0.15
        };
        let s = v.slips.abs();
        let x = if s > 1.0 {
            1.0
        } else if s >= 0.0 {
            s
        } else {
            0.0
        };
        self.current_vibrations[1] = x * self.rumble_intensity;
    }

    /// `JoypadCarControl::sendFF` @ 0x1401dc370: the force is ignored; every 11th call the
    /// motors get their levels. Returns the levels when it is time to send them.
    pub fn send_ff(&mut self) -> Option<[f32; 2]> {
        if self.ff_counter >= 10 {
            self.ff_counter = 0;
            Some(self.current_vibrations)
        } else {
            self.ff_counter += 1;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    /// The user's settings: STEER_SPEED 0.95, STEER_GAMMA 1.4, STEER_DEADZONE 0.05, left stick.
    fn user_ini(extra: &str) -> ControlsIni {
        ControlsIni::parse(&format!(
            "[X360]\nSTEER_THUMB=LEFT\nRUMBLE_INTENSITY=1\nSTEER_SPEED=0.95\nSTEER_GAMMA=1.4\nSTEER_FILTER=0\nSTEER_DEADZONE=0.05\nSPEED_SENSITIVITY=0\n\
             [GEARUP]\nKEY=0x20\nXBOXBUTTON=Y\n[GEARDN]\nKEY=0xA2\nXBOXBUTTON=X\n[DRS]\nXBOXBUTTON=LSHOULDER\n[KERS]\nXBOXBUTTON=B\n\
             [HANDBRAKE]\nXBOXBUTTON=B\n[ACTION_HEADLIGHTS]\nXBOXBUTTON=LTHUMB_PRESS\n[BALANCEUP]\nXBOXBUTTON=DPAD_RIGHT\n{extra}"
        ))
    }

    const NO_KEYS: &dyn Fn(i32) -> bool = &|_| false;

    #[test]
    fn the_wrapper_scales_like_x360joypad() {
        // spec A.1, A.2
        let stick = |v: i16| PadState { thumb_lx: v, ..PadState::default() }.left_stick_x();
        assert_eq!(bits(stick(32767)), 0x3f800000);
        assert_eq!(bits(stick(16384)), 0x3f000100);
        assert_eq!(bits(stick(8000)), 0x3e7a01f4);
        assert_eq!(bits(stick(-32768)), 0xbf800100);
        let trigger = |v: u8| PadState { right_trigger: v, ..PadState::default() }.right_trigger();
        assert_eq!(bits(trigger(255)), 0x3f800000);
        assert_eq!(bits(trigger(128)), 0x3f008081);
        assert_eq!(bits(trigger(64)), 0x3e808081);
        assert_eq!(bits(trigger(1)), 0x3b808081);
        // A.3: Y + LB + D-pad left
        let mask = PadState { buttons: 0x8104, ..PadState::default() }.button_mask();
        assert_eq!(mask, 0x0302);
        let press = |name: &str| PadButton { button: button_from_name(name), key: -1, allow_keyboard_input: false }.is_pressed(mask, NO_KEYS);
        assert!(press("Y") && press("LSHOULDER") && !press("X") && !press("-1") && !press("whatever"));
        // A.4
        assert_eq!(motor_word_ac(1.0), 65535);
        assert_eq!(motor_word_ac(0.3), 19660);
        assert_eq!(motor_word_ac(0.8), 52428);
        assert_eq!(motor_word_ac(0.075), 4915);
        assert_eq!(motor_word_ac(1.5), 32766, "the game's word wraps");
        assert_eq!(motor_word(1.5), 65535, "rustyAC keeps the level within 0..1");
        assert_eq!(motor_word(f32::NAN), 0);
    }

    #[test]
    fn the_ini_s_numbers_are_converted_like_the_constructor_s() {
        // spec B.5, B.6, B.7
        let with = |key: &str, value: &str| JoypadCarControl::from_ini(&ControlsIni::parse(&format!("[X360]\n{key}={value}\n")), false);
        for (value, expected) in [("0.95", 0x3dc28f5c), ("0.05", 0x3c23d70a), ("2.0", 0x3dcccccd), ("0.2", 0x3ca3d70b), ("0.1", 0x3c23d70b)] {
            assert_eq!(bits(with("STEER_SPEED", value).steer_speed), expected, "STEER_SPEED={value}");
        }
        for (value, expected) in [("0", 0), ("0.5", 0x3efd70a4), ("0.9", 0x3f641893), ("1.0", 0x3f7d70a4)] {
            assert_eq!(bits(with("STEER_FILTER", value).steer_filter), expected, "STEER_FILTER={value}");
        }
        assert_eq!(with("STEER_GAMMA", "0.7").steer_gamma, 1.0);
        assert_eq!(with("STEER_GAMMA", "1.4").steer_gamma, 1.4);
        // a file without the keys: no rumble, right stick, slowest steering
        let bare = JoypadCarControl::from_ini(&ControlsIni::default(), false);
        assert_eq!((bare.rumble_intensity, bare.speed_sensitivity, bare.is_steer_with_right, bare.dead_zone), (0.0, 0.0, true, 0.0));
        assert_eq!(bits(bare.steer_speed), 0x3c23d70a);
        // the user's file
        let user = JoypadCarControl::from_ini(&user_ini(""), false);
        assert!(!user.is_steer_with_right);
        assert_eq!((bits(user.steer_speed), bits(user.steer_gamma), bits(user.dead_zone)), (0x3dc28f5c, 0x3fb33333, 0x3d4ccccd));
        assert_eq!(user.button("GEARUP").button, button::Y);
        assert_eq!(user.button("GEARUP").key, 32);
        assert!(!user.button("GEARUP").allow_keyboard_input, "COMBINE_WITH_KEYBOARD_CONTROL is off");
    }

    #[test]
    fn dead_zone_and_gamma() {
        // spec C.8 to C.11
        assert_eq!(bits(get_axis_value(0.5, 1.4, 0.05)), 0x3eb3de43);
        assert_eq!(bits(get_axis_value(-0.5, 1.4, 0.05)), 0xbeb3de43);
        assert_eq!(get_axis_value(1.0, 1.4, 0.05), 1.0);
        assert_eq!(get_axis_value(-1.0, 1.4, 0.05), -1.0);
        assert_eq!(get_axis_value(0.04, 1.4, 0.05), 0.0);
        assert_eq!(get_axis_value(0.05, 1.4, 0.05), 0.0);
        assert_eq!(get_axis_value(0.25, 1.0, 0.0), 0.25);
        assert_eq!(bits(get_axis_value(0.25, 2.0, 0.1)), 0x3ce38e3a);
        let raw = |v: i16| get_axis_value(PadState { thumb_lx: v, ..PadState::default() }.left_stick_x(), 1.4, 0.05);
        assert_eq!(bits(raw(16384)), 0x3eb3e073);
        assert_eq!(bits(raw(8000)), 0x3dddc52e);
        assert_eq!(bits(raw(-32768)), 0xbf800179);
        assert_eq!(get_axis_value(f32::NAN, 1.4, 0.05), 0.0);
    }

    #[test]
    fn the_steering_limit_at_speed() {
        // spec D.12, D.13 (speed in m/s, lock in degrees)
        let m = |speed: f32, steer_lock: f32, sensitivity: f32, legacy: bool| {
            let mut pad = JoypadCarControl::from_ini(&user_ini(""), legacy);
            pad.speed_sensitivity = sensitivity;
            pad.max_steer(&CarControlsInput { steer_lock, speed })
        };
        assert_eq!(m(0.0, 180.0, 0.0, false), 1.0);
        assert_eq!(m(50.0, 180.0, 0.0, false), 1.0);
        assert_eq!(bits(m(50.0, 180.0, 0.5, false)), 0x3ef9999a);
        assert_eq!(bits(m(50.0, 180.0, 1.0, false)), 0x3e4ccccd);
        assert_eq!(bits(m(20.0, 180.0, 0.5, false)), 0x3f4b851f);
        assert_eq!(bits(m(100.0, 180.0, 1.0, false)), 0x3de38e39);
        assert_eq!(bits(m(27.777779, 450.0, 0.3, false)), 0x3f45cd7c);
        assert_eq!(bits(m(50.0, 180.0, 0.005, false)), 0x3f7e6724);
        // D.14: the legacy law
        assert_eq!(m(0.0, 180.0, 0.005, true), 1.0);
        assert_eq!(bits(m(0.01, 180.0, 0.005, true)), 0x3f3df347);
        assert_eq!(bits(m(0.02, 180.0, 0.005, true)), 0x3e9b3137);
        assert_eq!(bits(m(1.0, 180.0, 0.005, true)), 0x3dcccccd);
        assert_eq!(bits(m(50.0, 180.0, 0.3, true)), 0x3e99999a);
    }

    fn steer_steps(pad: &mut JoypadCarControl, stick: i16, from: f32, count: usize) -> Vec<u32> {
        let state = PadState { thumb_lx: stick, ..PadState::default() };
        let input = CarControlsInput { steer_lock: 180.0, speed: 0.0 };
        let mut controls = CarControls { steer: from, ..CarControls::default() };
        let mut extra = Extra::default();
        (0..count)
            .map(|_| {
                pad.acquire_controls(&state, &mut controls, &mut extra, &input, NO_KEYS);
                controls.steer.to_bits()
            })
            .collect()
    }

    #[test]
    fn steering_steps() {
        // spec E.15: full right from the centre, 0.095 per step
        let mut pad = JoypadCarControl::from_ini(&user_ini(""), false);
        assert_eq!(
            steer_steps(&mut pad, 32767, 0.0, 12),
            [
                0x3dc28f5c, 0x3e428f5c, 0x3e91eb85, 0x3ec28f5c, 0x3ef33333, 0x3f11eb85, 0x3f2a3d70, 0x3f428f5c, 0x3f5ae148, 0x3f733334,
                0x3f800000, 0x3f800000
            ]
        );
        // E.17: with STEER_FILTER=0.5
        let mut filtered = JoypadCarControl::from_ini(&user_ini("[X360_]\n"), false);
        filtered.steer_filter = ((0.5f32 * 0.99) * 3.0) * 0.333_333_34;
        assert_eq!(steer_steps(&mut filtered, 32767, 0.0, 4), [0x3d44816f, 0x3dc4816f, 0x3e136114, 0x3e448170]);
        // E.18
        assert_eq!(steer_steps(&mut pad, -32767, 1.0, 1), [0x3f67ae14]);
        assert_eq!(steer_steps(&mut pad, 0, 0.3, 1), [0x3e51eb86]);
        assert_eq!(steer_steps(&mut pad, 0, 0.05, 1), [0]);
        // E.16: half stick stops at its axis value (the stick value 0.5 is not a whole raw
        // count; 16384 gives 0.35132179)
        let steps = steer_steps(&mut pad, 16384, 0.0, 6);
        assert_eq!(steps[..3], [0x3dc28f5c, 0x3e428f5c, 0x3e91eb85]);
        assert_eq!(steps[3..], [0x3eb3e073; 3]);
    }

    #[test]
    fn pedals_and_buttons() {
        let mut pad = JoypadCarControl::from_ini(&user_ini("[__EXT_KEYBOARD_CLUTCH]\nXBOXBUTTON=A\n"), false);
        let input = CarControlsInput { steer_lock: 180.0, speed: 10.0 };
        let mut controls = CarControls::default();
        let mut extra = Extra::default();
        // spec E.19: RT 255, LT 128; Y (gear up) + LB (DRS) + B (handbrake and KERS) + D-pad right
        let state = PadState { right_trigger: 255, left_trigger: 128, buttons: 0x8000 | 0x0100 | 0x2000 | 0x0008, ..PadState::default() };
        pad.acquire_controls(&state, &mut controls, &mut extra, &input, NO_KEYS);
        assert_eq!((controls.gas, bits(controls.brake), controls.clutch), (1.0, 0x3f008081, 1.0));
        assert!(controls.gear_up && !controls.gear_dn && controls.drs && controls.kers);
        assert_eq!(controls.hand_brake, 1.0);
        assert!(extra.brake_balance_up && !extra.brake_balance_dn);
        // the H-shifter request is not the pad's to write
        assert_eq!(controls.requested_gear_index, -1);
        // the left stick's press is the headlight switch
        assert!(!pad.get_action(4, NO_KEYS));
        pad.acquire_controls(&PadState { buttons: 0x0040, ..PadState::default() }, &mut controls, &mut extra, &input, NO_KEYS);
        assert!(pad.get_action(4, NO_KEYS) && !pad.get_action(5, NO_KEYS) && !pad.get_action(7, NO_KEYS));
        assert!(!controls.gear_up && controls.hand_brake == 0.0);
        // rustyAC's clutch button: A
        pad.acquire_controls(&PadState { buttons: 0x1000, ..PadState::default() }, &mut controls, &mut extra, &input, NO_KEYS);
        assert_eq!(controls.clutch, 0.0);
        // a key only counts with COMBINE_WITH_KEYBOARD_CONTROL
        let space: &dyn Fn(i32) -> bool = &|key| key == 0x20;
        pad.acquire_controls(&PadState::default(), &mut controls, &mut extra, &input, space);
        assert!(!controls.gear_up);
        let mut combined = JoypadCarControl::from_ini(&user_ini("[ADVANCED]\nCOMBINE_WITH_KEYBOARD_CONTROL=1\n"), false);
        combined.acquire_controls(&PadState::default(), &mut controls, &mut extra, &input, space);
        assert!(controls.gear_up);
    }

    #[test]
    fn rumble() {
        // spec F.20 to F.24
        let mut pad = JoypadCarControl::from_ini(&user_ini(""), false);
        pad.set_vibrations(&VibrationDef { curbs: 0.06, slips: -0.3, ..VibrationDef::default() });
        assert_eq!((pad.current_vibrations[0], bits(pad.current_vibrations[1])), (1.0, 0x3e99999a));
        assert_eq!((motor_word(pad.current_vibrations[0]), motor_word(pad.current_vibrations[1])), (65535, 19660));
        pad.set_vibrations(&VibrationDef { curbs: 0.05, slips: 1.7, ..VibrationDef::default() });
        assert_eq!(pad.current_vibrations, [0.0, 1.0]);
        let mut legacy = JoypadCarControl::from_ini(&user_ini(""), true);
        legacy.set_vibrations(&VibrationDef { engine: 0.5, ..VibrationDef::default() });
        assert_eq!((bits(legacy.current_vibrations[0]), legacy.current_vibrations[1]), (0x3d99999a, 0.0));
        assert_eq!(motor_word(legacy.current_vibrations[0]), 4915);
        pad.rumble_intensity = 0.8;
        pad.set_vibrations(&VibrationDef { curbs: 0.2, slips: 0.25, ..VibrationDef::default() });
        assert_eq!((bits(pad.current_vibrations[0]), bits(pad.current_vibrations[1])), (0x3f4ccccd, 0x3e4ccccd));
        assert_eq!((motor_word(pad.current_vibrations[0]), motor_word(pad.current_vibrations[1])), (52428, 13107));
        // the motors are written on the 11th call, then on the 22nd
        let mut pad = JoypadCarControl::from_ini(&user_ini(""), false);
        let sent: Vec<usize> = (1..=30).filter(|_| pad.send_ff().is_some()).collect();
        assert_eq!(sent.len(), 2);
        let mut pad = JoypadCarControl::from_ini(&user_ini(""), false);
        let calls: Vec<usize> = (1..=30).filter(|_| pad.send_ff().is_some()).collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        let mut pad = JoypadCarControl::from_ini(&user_ini(""), false);
        let mut when = Vec::new();
        for call in 1..=30 {
            if pad.send_ff().is_some() {
                when.push(call);
            }
        }
        assert_eq!(when, [11, 22]);
    }
}
