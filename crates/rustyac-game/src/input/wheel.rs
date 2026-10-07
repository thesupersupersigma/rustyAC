//! Wheels and other DirectInput controllers: AC's `DICarControl` (DICarControl.obj) with its
//! axis and button bindings, ported from the disassembly
//! (`re/scratch/task11/spec_wheel_ffb.md` sections 3 and 4), on top of [`super::dinput`].
//!
//! Not ported: the H-pattern shifter (the Rust car does not say whether it supports one),
//! the damper effect, the force post-processor, the soft lock, the rev LEDs. Nobody had a
//! wheel on the desk when this was written: the arithmetic is tested against AC's numbers,
//! the hardware path is not.

use rustyac_math::powf;
use rustyac_physics::car::{CarControls, CarControlsInput, VibrationDef};

use super::bindings::Bindings;
use super::dinput::{DiState, DirectInput};
use super::ini::ControlsIni;
use super::Extra;

/// The pedals' dead zone (`0x3ca3d70a`).
const DEAD_ZONE: f32 = 0.02;

/// AC's `DIControlAxis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub joy: i32,
    /// `AXLE`: 0..7, -1 = none.
    pub index: i32,
    pub minv: f32,
    pub maxv: f32,
    /// The axis has shown a value other than 0 (before that it reports 0: a pedal at rest
    /// must not read as half pressed before the device's first real sample).
    pub has_moved: bool,
}

impl Axis {
    /// `DIControlAxis::load` @ 0x140081990. A missing key is 0, not -1.
    pub fn load(ini: &ControlsIni, section: &str, min_max: bool) -> Axis {
        Axis {
            joy: ini.get_int(section, "JOY"),
            index: ini.get_int(section, "AXLE"),
            minv: if min_max { ini.get_float(section, "MIN") } else { 0.0 },
            maxv: if min_max { ini.get_float(section, "MAX") } else { 0.0 },
            has_moved: false,
        }
    }

    pub const NONE: Axis = Axis { joy: -1, index: -1, minv: 0.0, maxv: 0.0, has_moved: false };

    /// `DIControlAxis::getValue` @ 0x140081910 on the device's state (`None`: no such device).
    pub fn get_value(&mut self, state: Option<&DiState>, normalized: bool) -> f32 {
        let (Some(state), 0..=7) = (state, self.index) else { return 0.0 };
        let v = state.axes[self.index as usize];
        if !self.has_moved {
            self.has_moved = v < 0.0 || v > 0.0;
            if !self.has_moved {
                return 0.0;
            }
        }
        if !normalized {
            return v;
        }
        let range = self.maxv - self.minv;
        if !(range < 0.0 || range > 0.0) {
            return 0.0;
        }
        (v - self.minv) / range
    }
}

/// AC's `DIControlButton`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub joy: i32,
    /// `BUTTON`: 0..127, -1 = none.
    pub index: i32,
    pub key: i32,
    pub combine_with_keyboard: bool,
}

impl Button {
    pub const NONE: Button = Button { joy: -1, index: -1, key: -1, combine_with_keyboard: false };

    /// `DIControlButton::load` @ 0x140081cc0.
    pub fn load(ini: &ControlsIni, section: &str) -> Button {
        Button {
            joy: ini.get_int(section, "JOY"),
            index: ini.get_int(section, "BUTTON"),
            key: ini.get_hex(section, "KEY"),
            combine_with_keyboard: ini.has_section("ADVANCED") && ini.get_int("ADVANCED", "COMBINE_WITH_KEYBOARD_CONTROL") > 0,
        }
    }

    /// Only if the file has the section (the constructor's optional buttons).
    pub fn load_optional(ini: &ControlsIni, section: &str) -> Button {
        if ini.has_section(section) {
            Button::load(ini, section)
        } else {
            Button::NONE
        }
    }

    /// The button is down on its device, or its key is down (keys only count with
    /// `COMBINE_WITH_KEYBOARD_CONTROL`).
    pub fn pressed(&self, di: &dyn Fn(i32) -> Option<DiState>, key_down: &dyn Fn(i32) -> bool) -> bool {
        let device = (0..128).contains(&self.index) && di(self.joy).is_some_and(|state| state.buttons[self.index as usize] != 0);
        device || (self.combine_with_keyboard && self.key != -1 && key_down(self.key))
    }
}

/// AC's `Trigger::keepSteady` @ 0x14023b3c0: a press stays true for `limit` seconds whatever
/// the button does meanwhile (`[STEER] DEBOUNCING_MS`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger {
    pub accumulator: f32,
    pub limit: f32,
}

impl Trigger {
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn keep_steady(&mut self, dt: f32, input: bool) -> bool {
        let idle = !(self.accumulator < 0.0 || self.accumulator > 0.0);
        if input && idle {
            self.accumulator = dt;
            return true;
        }
        if idle {
            return false;
        }
        self.accumulator += dt;
        if self.accumulator >= self.limit {
            self.accumulator = 0.0;
        }
        true
    }
}

fn sat(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x < 0.0 {
        0.0
    } else if x.is_nan() {
        0.0
    } else {
        x
    }
}

/// AC's `DICarControl`.
#[derive(Clone, Debug, PartialEq)]
pub struct DiCarControl {
    pub steer: Axis,
    pub gas: Axis,
    pub brake: Axis,
    pub clutch: Axis,
    pub hand_brake_axis: Axis,
    pub gear_up: Button,
    pub gear_dn: Button,
    pub hand_brake: Button,
    pub headlights: Button,
    pub brake_balance_up: Button,
    pub brake_balance_dn: Button,
    pub kers: Button,
    pub drs: Button,
    pub abs_up: Button,
    pub abs_dn: Button,
    pub tc_up: Button,
    pub tc_dn: Button,
    pub shift_up_trigger: Trigger,
    pub shift_dn_trigger: Trigger,
    pub steer_scale: f32,
    /// `[STEER] STEER_GAMMA`
    pub linearity: f32,
    /// Half of `[STEER] LOCK`, degrees.
    pub steer_lock: f32,
    pub speed_sensitivity: f32,
    /// `STEER_FILTER / 0.003`: times `dt` it is the share of the way taken per step.
    pub steer_filter: f32,
    pub brake_gamma: f32,
    pub ff_gain: f32,
    /// `[STEER] FILTER_FF`, for the physics.
    pub ff_filter: f32,
    /// `[FF_ENHANCEMENT] CURBS, ROAD, SLIPS, ABS`
    pub curbs_gain: f32,
    pub gforce_gain: f32,
    pub slips_gain: f32,
    pub abs_gain: f32,
    /// `[FF_ENHANCEMENT_2] UNDERSTEER`, for the physics.
    pub use_fake_understeer_ff: bool,
    pub min_ff: f32,
    pub center_boost_gain: f32,
    pub center_boost_range: f32,
    pub ff_counter: i32,
    /// `FF_SKIP_STEPS`: the force goes out every `ff_interval + 1` steps.
    pub ff_interval: i32,
    pub current_vibration: f32,
    /// The steering before its clamp to -1..1.
    pub current_lock: f32,
    pub last_speed: f32,
}

impl DiCarControl {
    /// `DICarControl::DICarControl` @ 0x14007ddd0, the `controls.ini` part.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn from_ini(ini: &ControlsIni) -> DiCarControl {
        let zero_means = |value: f32, default: f32| if value < 0.0 || value > 0.0 { value } else { default };
        let limit = ini.get_float("STEER", "DEBOUNCING_MS") * 0.001;
        let enhancement = ini.has_section("FF_ENHANCEMENT");
        let tweaks = ini.has_section("FF_TWEAKS");
        DiCarControl {
            steer: Axis::load(ini, "STEER", false),
            gas: Axis::load(ini, "THROTTLE", true),
            brake: Axis::load(ini, "BRAKES", true),
            clutch: Axis::load(ini, "CLUTCH", true),
            hand_brake_axis: Axis::load(ini, "HANDBRAKE", true),
            gear_up: Button::load(ini, "GEARUP"),
            gear_dn: Button::load(ini, "GEARDN"),
            hand_brake: Button::load_optional(ini, "HANDBRAKE"),
            headlights: Button::load_optional(ini, "ACTION_HEADLIGHTS"),
            brake_balance_up: Button::load_optional(ini, "BALANCEUP"),
            brake_balance_dn: Button::load_optional(ini, "BALANCEDN"),
            kers: Button::load_optional(ini, "KERS"),
            drs: Button::load_optional(ini, "DRS"),
            abs_up: Button::load_optional(ini, "ABSUP"),
            abs_dn: Button::load_optional(ini, "ABSDN"),
            tc_up: Button::load_optional(ini, "TCUP"),
            tc_dn: Button::load_optional(ini, "TCDN"),
            shift_up_trigger: Trigger { accumulator: 0.0, limit },
            shift_dn_trigger: Trigger { accumulator: 0.0, limit },
            steer_scale: zero_means(ini.get_float("STEER", "SCALE"), 1.0),
            linearity: zero_means(ini.get_float("STEER", "STEER_GAMMA"), 1.0),
            steer_lock: zero_means(ini.get_float("STEER", "LOCK") * 0.5, 200.0),
            speed_sensitivity: ini.get_float("STEER", "SPEED_SENSITIVITY"),
            steer_filter: ini.get_float("STEER", "STEER_FILTER") / 0.003,
            brake_gamma: zero_means(ini.get_float("BRAKES", "GAMMA"), 1.0),
            ff_gain: ini.get_float("STEER", "FF_GAIN"),
            ff_filter: {
                let v = ini.get_float("STEER", "FILTER_FF");
                if v > 1.0 {
                    1.0
                } else if v < 0.0 {
                    0.0
                } else {
                    v
                }
            },
            curbs_gain: if enhancement { ini.get_float("FF_ENHANCEMENT", "CURBS") } else { 0.0 },
            gforce_gain: if enhancement { ini.get_float("FF_ENHANCEMENT", "ROAD") } else { 0.0 },
            slips_gain: if enhancement { ini.get_float("FF_ENHANCEMENT", "SLIPS") } else { 0.0 },
            abs_gain: if ini.has_key("FF_ENHANCEMENT", "ABS") { ini.get_float("FF_ENHANCEMENT", "ABS") } else { 0.0 },
            use_fake_understeer_ff: ini.has_section("FF_ENHANCEMENT_2") && ini.get_int("FF_ENHANCEMENT_2", "UNDERSTEER") != 0,
            min_ff: if tweaks { ini.get_float("FF_TWEAKS", "MIN_FF") } else { 0.0 },
            center_boost_gain: if tweaks { ini.get_float("FF_TWEAKS", "CENTER_BOOST_GAIN") } else { 0.0 },
            center_boost_range: if tweaks { ini.get_float("FF_TWEAKS", "CENTER_BOOST_RANGE") } else { 0.1 },
            ff_counter: 0,
            // `system/cfg/assetto_corsa.ini [FORCE_FEEDBACK] FF_SKIP_STEPS` ships as 0
            ff_interval: if ini.has_section("FF_SKIP_STEPS") { ini.get_int("FF_SKIP_STEPS", "VALUE") } else { 0 },
            current_vibration: 0.0,
            current_lock: 0.0,
            last_speed: 0.0,
        }
    }

    /// `DICarControl::acquireControls` @ 0x14007fe70. `di` gives the state of a `JOY`
    /// number. The H-pattern branch is not ported: the paddles always count.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn acquire_controls(
        &mut self,
        di: &dyn Fn(i32) -> Option<DiState>,
        controls: &mut CarControls,
        extra: &mut Extra,
        dt: f32,
        input: &CarControlsInput,
        key_down: &dyn Fn(i32) -> bool,
    ) {
        self.last_speed = input.speed;
        // steering
        let mut x = self.steer.get_value(di(self.steer.joy).as_ref(), false) * self.steer_scale;
        if self.linearity < 1.0 || self.linearity > 1.0 {
            let p = powf(x.abs(), self.linearity);
            let s = if x > 0.0 {
                1.0
            } else if x >= 0.0 {
                0.0
            } else {
                -1.0
            };
            x = s * p;
        }
        if self.steer_lock > input.steer_lock {
            x *= self.steer_lock / input.steer_lock;
        }
        if self.speed_sensitivity < 0.0 || self.speed_sensitivity > 0.0 {
            x /= self.speed_sensitivity * input.speed + 1.0;
        }
        if self.steer_filter > 0.0 {
            let prev = controls.steer;
            let k = sat(self.steer_filter * dt);
            x = (x - prev) * k + prev;
        }
        self.current_lock = x;
        controls.steer = if x > 1.0 {
            1.0
        } else if !(x >= -1.0) {
            -1.0
        } else {
            x
        };
        // pedals
        let pedal = |v: f32| if !(v >= DEAD_ZONE) { 0.0 } else { sat(v) };
        controls.gas = pedal(self.gas.get_value(di(self.gas.joy).as_ref(), true));
        controls.brake = sat(powf(pedal(self.brake.get_value(di(self.brake.joy).as_ref(), true)), self.brake_gamma));
        let mut v = self.clutch.get_value(di(self.clutch.joy).as_ref(), true);
        if !(v >= DEAD_ZONE) {
            v = 0.0;
        }
        controls.clutch = sat(1.0 - v);
        // not AC's: a pressed pedal tells the automatic clutch aid to stand back
        extra.clutch_pressed = v > 0.0;
        let b = self.hand_brake.pressed(di, key_down) as i32 as f32;
        controls.hand_brake = sat(self.hand_brake_axis.get_value(di(self.hand_brake_axis.joy).as_ref(), true) + b);
        // gears: the paddles, each press held for the debouncing time
        controls.requested_gear_index = -1;
        let none: &dyn Fn(i32) -> bool = &|_| false;
        let up = self.shift_up_trigger.keep_steady(dt, self.gear_up.pressed(di, none));
        controls.gear_up = up || (self.gear_up.combine_with_keyboard && self.gear_up.key != -1 && key_down(self.gear_up.key));
        let dn = self.shift_dn_trigger.keep_steady(dt, self.gear_dn.pressed(di, none));
        controls.gear_dn = dn || (self.gear_dn.combine_with_keyboard && self.gear_dn.key != -1 && key_down(self.gear_dn.key));
        // level buttons
        extra.brake_balance_dn = self.brake_balance_dn.pressed(di, key_down);
        extra.brake_balance_up = self.brake_balance_up.pressed(di, key_down);
        controls.kers = self.kers.pressed(di, key_down);
        controls.drs = self.drs.pressed(di, key_down);
        extra.tc_dn = self.tc_dn.pressed(di, key_down);
        extra.tc_up = self.tc_up.pressed(di, key_down);
        extra.abs_dn = self.abs_dn.pressed(di, key_down);
        extra.abs_up = self.abs_up.pressed(di, key_down);
    }

    /// `DICarControl::setVibrations` @ 0x140081010: the mix that is added to the steering force.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn set_vibrations(&mut self, d: &VibrationDef) {
        let mut x = self.gforce_gain * d.gforce;
        x += self.curbs_gain * d.curbs;
        x += self.slips_gain * d.slips;
        x += self.abs_gain * d.abs;
        self.current_vibration = if x > 1.0 {
            1.0
        } else if x >= -1.0 {
            x
        } else {
            -1.0
        };
    }

    /// `DICarControl::sendFF` @ 0x140080e40 up to the device: centre boost, minimum force,
    /// skipped steps, gains. `raw_steer` is the steering axis as the device reports it.
    /// Returns the force for the device when one is due (-1..1 before the device's clamp).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn send_ff(&mut self, mut ff: f32, raw_steer: f32, user_gain: f32) -> Option<f32> {
        let a = raw_steer.abs();
        if self.center_boost_gain < 0.0 || self.center_boost_gain > 0.0 {
            let r = self.center_boost_range;
            if a < r {
                // inside the range the force is replaced, not raised: gain x (1 - a / range)
                ff *= (1.0 - a / r) * self.center_boost_gain;
            }
        }
        let m = sat((self.last_speed * 3.6 - 1.0) * 0.2) * self.min_ff;
        if ff.abs() < m {
            let s = if ff > 0.0 {
                1.0
            } else if ff >= 0.0 {
                0.0
            } else {
                -1.0
            };
            ff = s * m;
        }
        if self.ff_counter < self.ff_interval {
            self.ff_counter += 1;
            return None;
        }
        self.ff_counter = 0;
        Some(((ff + self.current_vibration) * self.ff_gain) * user_gain)
    }
}

/// The DirectInput devices with AC's wheel class on top.
pub struct WheelDevice {
    pub di: DirectInput,
    pub control: DiCarControl,
    /// Force feedback is on (`--ffb`, a wheel that has it, and it could be set up).
    pub ffb: bool,
    /// What happened while the devices were set up, for the console.
    pub notes: Vec<String>,
    /// The steering axis as it was at the last look (to see whether somebody turns it).
    last_axes: [f32; 3],
    moved: bool,
    force_on: bool,
}

// SAFETY: the DirectInput objects are created, used and dropped on the physics thread only;
// the struct is moved there inside the driver before any call is made.
unsafe impl Send for WheelDevice {}

impl WheelDevice {
    /// Opens DirectInput and binds AC's wheel class to it. `None` only if DirectInput itself
    /// cannot be opened.
    pub fn open(bindings: &Bindings, window: isize, ffb: bool) -> Option<WheelDevice> {
        let di = match DirectInput::open(window) {
            Ok(di) => di,
            Err(message) => {
                eprintln!("WARNING: {message}");
                return None;
            }
        };
        let mut control = DiCarControl::from_ini(&bindings.ini);
        let mut notes = Vec::new();
        let usable = |joy: i32| di.state(joy).is_some();
        if !usable(control.steer.joy) {
            // the bindings do not name a device that is there (and is not an Xbox pad)
            control.steer = Axis::NONE;
            if let Some(wheel) = di.devices.iter().find(|d| d.is_wheel && !d.is_xinput) {
                // a guess at a wheel nobody has bound: wheel on X, throttle on Y, brake on Rz
                // (the commonest layout), pedals pressed = -1
                let joy = wheel.index as i32;
                control.steer = Axis { joy, index: 0, ..Axis::NONE };
                control.gas = Axis { joy, index: 1, minv: 1.0, maxv: -1.0, has_moved: false };
                control.brake = Axis { joy, index: 5, minv: 1.0, maxv: -1.0, has_moved: false };
                control.clutch = Axis::NONE;
                control.hand_brake_axis = Axis::NONE;
                if !(control.ff_gain > 0.0) {
                    control.ff_gain = 1.0;
                }
                notes.push(format!(
                    "\"{}\" has no bindings: guessing wheel = X axis, throttle = Y, brake = Rz. Set [STEER] / [THROTTLE] / [BRAKES] JOY={joy} and AXLE in rustyac_controls.ini to bind it properly",
                    wheel.name
                ));
            } else if di.devices.iter().any(|d| !d.is_xinput) {
                notes.push("a DirectInput controller is attached but [STEER] JOY in the bindings does not point at it: it is not used".to_string());
            }
        }
        let mut device = WheelDevice { di, control, ffb: false, notes, last_axes: [0.0; 3], moved: false, force_on: false };
        if ffb {
            match device.steer_joy() {
                Some(joy) => match device.di.enable_ff(joy) {
                    Ok(()) => {
                        device.ffb = true;
                        device.notes.push(format!(
                            "force feedback ON, capped at {:.0} % of the wheel's strength",
                            super::dinput::FF_CAP * 100.0
                        ));
                    }
                    Err(message) => device.notes.push(format!("no force feedback: {message}")),
                },
                None => device.notes.push("--ffb: no wheel is bound, no force feedback".to_string()),
            }
        }
        Some(device)
    }

    /// The `JOY` number that steers, if a device is bound and open.
    pub fn steer_joy(&self) -> Option<i32> {
        (self.control.steer.index >= 0 && self.di.state(self.control.steer.joy).is_some()).then_some(self.control.steer.joy)
    }

    pub fn connected(&self) -> bool {
        self.steer_joy().is_some()
    }

    pub fn poll(&mut self) {
        self.di.poll();
        let axes = [self.control.steer, self.control.gas, self.control.brake].map(|axis| {
            self.di.state(axis.joy).filter(|_| (0..8).contains(&axis.index)).map(|s| s.axes[axis.index as usize]).unwrap_or(0.0)
        });
        self.moved = (0..3).any(|k| (axes[k] - self.last_axes[k]).abs() > 0.02);
        if self.moved {
            self.last_axes = axes;
        }
    }

    /// Is somebody at the wheel (an axis moving, a paddle held)?
    pub fn in_use(&self) -> bool {
        let di = |joy: i32| self.di.state(joy).copied();
        self.connected() && (self.moved || self.control.gear_up.pressed(&di, &|_| false) || self.control.gear_dn.pressed(&di, &|_| false))
    }

    pub fn acquire_controls(&mut self, controls: &mut CarControls, extra: &mut Extra, dt: f32, input: &CarControlsInput, key_down: &dyn Fn(i32) -> bool) {
        let di = &self.di;
        self.control.acquire_controls(&|joy| di.state(joy).copied(), controls, extra, dt, input, key_down);
    }

    pub fn get_action(&self, action: i32, key_down: &dyn Fn(i32) -> bool) -> bool {
        action == 4 && self.control.headlights.pressed(&|joy| self.di.state(joy).copied(), key_down)
    }

    pub fn set_vibrations(&mut self, def: &VibrationDef) {
        self.control.set_vibrations(def);
    }

    /// The steering force of this step. Only a wheel that is driving gets any.
    pub fn send_ff(&mut self, ff: f32, _damper: f32, user_gain: f32, active: bool) {
        let Some(joy) = self.steer_joy() else { return };
        let raw = self.di.state(joy).map(|s| s.axes[self.control.steer.index.clamp(0, 7) as usize]).unwrap_or(0.0);
        let Some(out) = self.control.send_ff(ff, raw, user_gain) else { return };
        if self.ffb {
            self.di.send_ff(joy, if active { out } else { 0.0 });
            self.force_on = active;
        }
    }

    /// No force (paused, or the wheel is not driving).
    pub fn stop_ff(&mut self) {
        if self.ffb && self.force_on {
            if let Some(joy) = self.steer_joy() {
                self.di.send_ff(joy, 0.0);
            }
            self.force_on = false;
        }
    }
}

impl Drop for WheelDevice {
    fn drop(&mut self) {
        self.force_on = true;
        self.stop_ff();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    fn state(axis0: f32) -> DiState {
        let mut s = DiState::default();
        s.axes[0] = axis0;
        s
    }

    const NO_KEYS: &dyn Fn(i32) -> bool = &|_| false;

    #[test]
    fn an_axis_rescales_and_waits_for_its_first_move() {
        // spec 9.B.5, 6
        let value = |minv: f32, maxv: f32, raw: f32| Axis { joy: 0, index: 0, minv, maxv, has_moved: true }.get_value(Some(&state(raw)), true);
        assert_eq!((value(-1.0, 1.0, -1.0), value(-1.0, 1.0, 1.0)), (0.0, 1.0));
        assert_eq!(bits(value(-1.0, 1.0, 0.5)), 0x3f400000);
        assert_eq!(bits(value(-1.0, 1.0, 3333.0 * 0.0001)), 0x3f2aa993);
        assert_eq!(value(1.0, -1.0, -1.0), 1.0);
        assert_eq!(bits(value(1.0, -1.0, 0.2)), 0x3ecccccd);
        assert_eq!(bits(value(1.0, -1.0, 1.0)), 0x80000000);
        // B.7: the latch
        let mut axis = Axis { joy: 0, index: 0, minv: -1.0, maxv: 1.0, has_moved: false };
        let got: Vec<f32> = [0.0, 0.0, -1.0, 0.0].iter().map(|raw| axis.get_value(Some(&state(*raw)), true)).collect();
        assert_eq!(got, [0.0, 0.0, 0.0, 0.5]);
        // B.8
        assert_eq!(value(0.3, 0.3, 0.7), 0.0);
        assert_eq!(Axis { index: -1, ..axis }.get_value(Some(&state(0.7)), true), 0.0);
        assert_eq!(axis.get_value(None, true), 0.0);
    }

    /// A wheel on JOY 0: steering on axis 0, throttle 1, brake 2, clutch 3.
    fn wheel(extra: &str) -> DiCarControl {
        DiCarControl::from_ini(&ControlsIni::parse(&format!(
            "[STEER]\nJOY=0\nAXLE=0\nLOCK=900\nSCALE=1\nSTEER_GAMMA=1\nFF_GAIN=1\nDEBOUNCING_MS=50\n[THROTTLE]\nJOY=0\nAXLE=1\nMIN=-1\nMAX=1\n\
             [BRAKES]\nJOY=0\nAXLE=2\nMIN=-1\nMAX=1\nGAMMA=2\n[CLUTCH]\nJOY=0\nAXLE=3\nMIN=-1\nMAX=1\n[HANDBRAKE]\nJOY=-1\nAXLE=-1\nBUTTON=-1\n\
             [GEARUP]\nJOY=0\nBUTTON=4\nKEY=-1\n[GEARDN]\nJOY=0\nBUTTON=5\nKEY=-1\n{extra}"
        )))
    }

    fn drive(control: &mut DiCarControl, s: DiState, from: f32, speed: f32) -> CarControls {
        let mut controls = CarControls { steer: from, ..CarControls::default() };
        let mut extra = Extra::default();
        control.acquire_controls(&|joy| (joy == 0).then_some(s), &mut controls, &mut extra, 0.003, &CarControlsInput { steer_lock: 180.0, speed }, NO_KEYS);
        controls
    }

    #[test]
    fn steering_is_matched_to_the_car_s_lock() {
        // spec 9.D.14: a 900 degree wheel on a 180 degree car
        let mut w = wheel("");
        assert_eq!(bits(w.steer_lock), 0x43e10000);
        assert_eq!(drive(&mut w, state(0.25), 0.0, 0.0).steer, 0.625);
        assert_eq!((drive(&mut w, state(0.5), 0.0, 0.0).steer, bits(w.current_lock)), (1.0, 0x3fa00000));
        // D.17: speed sensitivity
        w.speed_sensitivity = 0.1;
        assert_eq!(bits(drive(&mut w, state(0.25), 0.0, 50.0).steer), 0x3dd55555);
        // D.18: gamma 2
        let mut w = wheel("");
        w.linearity = 2.0;
        assert_eq!(bits(drive(&mut w, state(-0.5), 0.0, 0.0).steer), 0xbf200000);
        // D.19: STEER_FILTER 0.5 takes half the way per step
        let mut w = wheel("");
        w.steer_filter = 0.5 / 0.003;
        assert_eq!(bits(w.steer_filter), 0x4326aaab);
        let first = drive(&mut w, state(0.25), 0.0, 0.0).steer;
        assert_eq!((bits(first), bits(drive(&mut w, state(0.25), first, 0.0).steer)), (0x3ea00000, 0x3ef00000));
        // D.15, D.20: a wheel with less rotation than the car is not scaled; the defaults
        let mut narrow = wheel("");
        narrow.steer_lock = 135.0;
        assert_eq!(drive(&mut narrow, state(0.5), 0.0, 0.0).steer, 0.5);
        let bare = DiCarControl::from_ini(&ControlsIni::parse("[STEER]\nJOY=0\n"));
        assert_eq!((bare.steer_lock, bare.steer_scale, bare.linearity, bare.brake_gamma, bare.ff_gain), (200.0, 1.0, 1.0, 1.0, 0.0));
    }

    #[test]
    fn pedals() {
        let mut w = wheel("");
        // throttle half way, brake half way (gamma 2), clutch pedal up
        let mut s = DiState { axes: [0.0, 0.5, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0], ..DiState::default() };
        // the brake axis has not moved yet: it reads 0, not a half
        let c = drive(&mut w, s, 0.0, 0.0);
        assert_eq!((c.gas, c.brake, c.clutch), (0.75, 0.0, 1.0));
        s.axes[2] = 1e-4;
        let c = drive(&mut w, s, 0.0, 0.0);
        assert!((c.brake - 0.25).abs() < 1e-4, "{}", c.brake);
        // spec 9.C.9: just under the dead zone is nothing; 9.C.12: the clutch
        s.axes[1] = -1.0 + 2.0 * 0.0199;
        s.axes[3] = -1.0 + 2.0 * 0.3;
        let c = drive(&mut w, s, 0.0, 0.0);
        assert_eq!(c.gas, 0.0);
        assert!((c.clutch - 0.7).abs() < 1e-6);
        assert_eq!(c.requested_gear_index, -1);
    }

    #[test]
    fn a_paddle_tap_is_held_for_the_debouncing_time() {
        // spec 9.E.22 to 24
        let run = |limit: f32, presses: usize, steps: usize| {
            let mut t = Trigger { accumulator: 0.0, limit };
            (0..steps).filter(|k| t.keep_steady(0.003, *k < presses)).count()
        };
        assert_eq!(run(0.05, 1, 60), 17);
        assert_eq!(run(0.05, 5, 60), 17);
        assert_eq!(run(0.05, 20, 60), 34);
        assert_eq!(run(0.0, 1, 10), 2);
        // through the wheel: one step of the paddle gives 17 steps of gear-up
        let mut w = wheel("");
        let mut s = DiState::default();
        s.buttons[4] = 128;
        let mut count = drive(&mut w, s, 0.0, 0.0).gear_up as usize;
        for _ in 0..40 {
            count += drive(&mut w, DiState::default(), 0.0, 0.0).gear_up as usize;
        }
        assert_eq!(count, 17);
    }

    #[test]
    fn the_force_chain() {
        // spec 9.G.26
        let mut w = wheel("[FF_ENHANCEMENT]\nCURBS=0.4\nROAD=0.5\nSLIPS=0\nABS=0.21\n[FF_TWEAKS]\nMIN_FF=0.05\nCENTER_BOOST_GAIN=0\nCENTER_BOOST_RANGE=0.1\n");
        w.set_vibrations(&VibrationDef { curbs: 0.5, gforce: -0.2, slips: 0.9, engine: 0.3, abs: 1.0 });
        assert_eq!(bits(w.current_vibration), 0x3e9eb852);
        // H.27
        w.last_speed = 30.0;
        assert_eq!(bits(w.send_ff(0.3, 0.0, 1.0).unwrap()), 0x3f1c28f6);
        // H.28: the minimum force
        w.current_vibration = 0.0;
        w.last_speed = 2.0;
        assert_eq!(bits(w.send_ff(0.01, 0.0, 1.0).unwrap()), 0x3d4ccccd);
        assert_eq!(w.send_ff(-0.001, 0.0, 1.0).unwrap(), -0.05);
        assert_eq!(w.send_ff(0.0, 0.0, 1.0).unwrap(), 0.0);
        w.last_speed = 0.2;
        assert_eq!(w.send_ff(0.01, 0.0, 1.0).unwrap(), 0.01);
        // H.29: the centre boost replaces the force inside its range
        w.min_ff = 0.0;
        w.center_boost_gain = 2.0;
        assert_eq!(w.send_ff(0.4, 0.0, 1.0).unwrap(), 0.8);
        assert_eq!(w.send_ff(0.4, 0.05, 1.0).unwrap(), 0.4);
        assert_eq!(w.send_ff(0.4, 0.1, 1.0).unwrap(), 0.4);
        // H.30: the gains
        w.center_boost_gain = 0.0;
        w.ff_gain = 0.7;
        assert_eq!(bits(w.send_ff(0.8, 0.0, 1.5).unwrap()), 0x3f570a3e);
        // H.31: skipped steps
        w.ff_interval = 1;
        let sent: Vec<usize> = (1..=6).filter(|_| w.send_ff(0.1, 0.0, 1.0).is_some()).collect();
        assert_eq!(sent.len(), 3);
    }
}
