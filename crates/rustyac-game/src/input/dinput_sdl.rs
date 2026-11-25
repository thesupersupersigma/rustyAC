// SPDX-License-Identifier: GPL-3.0-or-later

//! `dinput.rs` on Linux: the game controllers SDL3 lists (wheels, pedals, other pads), in the
//! shape Windows' DirectInput gives them to AC's wheel class ([`super::wheel`]), and one
//! constant force through SDL's haptic API for force feedback.
//!
//! What maps how (docs/platforms.md, section 12):
//! - **`JOY`:** DirectInput's enumeration order is not SDL's, so `JOY=n` is the device whose
//!   product GUID is `[CONTROLLERS] PGUIDn` (as CSP's `fix_devices_order` does on Windows); a
//!   listed device that is missing keeps its number, empty. Devices not listed follow, in
//!   SDL's order. Without `[CONTROLLERS]` the numbers are SDL's order.
//! - **`AXLE`:** Linux gives HID's X, Y, Z, Rx, Ry, Rz, Slider and Dial the evdev codes 0 to
//!   7, `DIJOYSTATE2`'s order, so an axis keeps its Windows number; SDL numbers only the axes
//!   a device has, so the codes are read from the device itself. Other codes have no
//!   DirectInput number.
//! - **Values:** the raw value in the device's range, found again from SDL's -32768..32767,
//!   to DirectInput's -10000..10000 as `MulDiv` rounds.
//! - **Force:** DirectInput's magnitude (`dinput.rs`, capped the same) as SDL's level, the
//!   smallest that SDL's own DirectInput backend turns back into that magnitude.
//!
//! As on Windows: an Xbox-layout pad is listed but read through [`super::pad`]'s `XInput`.
//! Not as on Windows: no exclusive access (evdev has none for force feedback), and a device
//! unplugged and plugged in again is not found until the next start.

use std::ffi::{c_int, CStr};

use sdl3::sys::gamepad::SDL_IsGamepad;
use sdl3::sys::haptic::*;
use sdl3::sys::init::{SDL_InitSubSystem, SDL_QuitSubSystem, SDL_INIT_HAPTIC, SDL_INIT_JOYSTICK};
use sdl3::sys::joystick::*;
use sdl3::sys::stdinc::SDL_free;

use super::ini::ControlsIni;
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

/// As `dinput.rs`.
pub const FF_CAP: f32 = 0.3;

/// `InputDevice::poll`'s scaling, as `dinput.rs`.
pub fn axis_value(raw: i32) -> f32 {
    raw as f32 * 0.0001
}

/// `InputDevice::sendFF`'s magnitude, as `dinput.rs`.
pub fn magnitude(ff: f32) -> i32 {
    if ff.is_nan() {
        return 0;
    }
    ((ff * 10000.0) as i32).clamp(-10000, 10000)
}

/// SDL's value of a raw evdev value in `min..=max` (`SDL_sysjoystick.c`, `AxisCorrect`, in
/// its float arithmetic).
pub fn sdl_axis(raw: i32, min: i32, max: i32) -> i32 {
    let scale = 65535.0f32 / (max - min) as f32;
    (((raw - min) as f32 * scale + -32768.0) + 0.5).floor().clamp(-32768.0, 32767.0) as i32
}

/// DirectInput's -10000..10000 of an SDL axis whose device range is `min..=max`: the raw value
/// SDL scaled (found again: SDL's scaling keeps every value of a range up to 65536 apart), then
/// the range mapped as `MulDiv` (half rounds up) does it.
// shortcut: DirectInput's own rounding is Wine's model, not measured on Windows; a wheel read
// on both systems (step L8) settles it
pub fn di_axis(value: i16, min: i32, max: i32) -> i32 {
    if max <= min {
        return 0;
    }
    let range = (max - min) as i64;
    let guess = min + ((value as i64 + 32768) * range / 65535) as i32;
    let raw = (guess - 2..=guess + 2).map(|r| r.clamp(min, max)).find(|r| sdl_axis(*r, min, max) == value as i32).unwrap_or(guess.clamp(min, max));
    (((raw - min) as i64 * 20000 + range / 2) / range - 10000) as i32
}

/// SDL's haptic level of a DirectInput magnitude: the smallest that SDL's DirectInput backend
/// (`level * 10000 / 0x7fff`) turns back into it; Linux gets it as it is.
pub fn haptic_level(magnitude: i32) -> i16 {
    let m = magnitude.clamp(-10000, 10000);
    ((m.abs() * 32767 + 9999) / 10000 * m.signum()) as i16
}

/// DirectInput's POV of an SDL hat: hundredths of a degree clockwise from up, or centred.
pub fn pov(hat: u8) -> u32 {
    match hat {
        0x01 => 0,
        0x03 => 4500,
        0x02 => 9000,
        0x06 => 13500,
        0x04 => 18000,
        0x0c => 22500,
        0x08 => 27000,
        0x09 => 31500,
        _ => u32::MAX,
    }
}

/// DirectInput's product GUID as `controls.ini` writes it (`PGUID0=C24F046D-0000-0000-0000-
/// 504944564944`: the product and vendor ids, then "PIDVID").
pub fn product_guid(vendor: u16, product: u16) -> String {
    format!("{product:04X}{vendor:04X}-0000-0000-0000-504944564944")
}

/// The DirectInput number of each SDL axis of a device with these evdev axis codes (`EV_ABS`
/// bits): SDL counts the codes it has in order, digital hats (0x10..0x17) left out.
pub fn axis_slots(abs: u64) -> Vec<Option<usize>> {
    (0..0x3f).filter(|code| abs >> code & 1 != 0 && !(0x10..=0x17).contains(code)).map(|code| (code < 8).then_some(code)).collect()
}

/// Each SDL axis of an evdev node: its DirectInput number and its range (`EVIOCGBIT(EV_ABS)`,
/// `EVIOCGABS`).
fn evdev_axes(path: &CStr) -> Option<Vec<Option<(usize, i32, i32)>>> {
    use sdl3::libc;
    // SAFETY: a read-only open, ioctls into locals of the sizes given, a close
    unsafe {
        let fd = libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let mut bits = 0u64;
        // _IOC(_IOC_READ, 'E', 0x20 + EV_ABS, 8)
        let axes = (libc::ioctl(fd, 0x8008_4523, &mut bits as *mut u64) >= 0).then(|| {
            axis_slots(bits)
                .into_iter()
                .map(|slot| {
                    let slot = slot?;
                    // struct input_absinfo: value, minimum, maximum, fuzz, flat, resolution
                    let mut info = [0i32; 6];
                    // _IOC(_IOC_READ, 'E', 0x40 + code, 24)
                    (libc::ioctl(fd, 0x8018_4540 + slot as libc::c_ulong, info.as_mut_ptr()) >= 0).then_some((slot, info[1], info[2]))
                })
                .collect()
        });
        libc::close(fd);
        axes
    }
}

pub struct DiDevice {
    /// AC's `JOY` number.
    pub index: usize,
    pub name: String,
    /// An Xbox-layout pad: listed, read through `XInput` instead.
    pub is_xinput: bool,
    pub has_ff: bool,
    pub is_wheel: bool,
    /// The product GUID, as `[CONTROLLERS] PGUIDn` holds it.
    pub pguid: String,
    joystick: *mut SDL_Joystick,
    /// The DirectInput number and the device's range of each of SDL's axes.
    slots: Vec<Option<(usize, i32, i32)>>,
    pub state: DiState,
    haptic: *mut SDL_Haptic,
    effect: SDL_HapticEffectID,
    ff_started: bool,
    last_magnitude: i32,
}

pub struct DirectInput {
    pub devices: Vec<DiDevice>,
    sdl: bool,
}

/// One constant force on the steering axis, endless.
fn constant(level: i16) -> SDL_HapticEffect {
    // SAFETY: a plain C union of integers, all zero is valid
    let mut effect: SDL_HapticEffect = unsafe { std::mem::zeroed() };
    effect.constant = SDL_HapticConstant {
        r#type: SDL_HAPTIC_CONSTANT,
        direction: SDL_HapticDirection { r#type: SDL_HAPTIC_STEERING_AXIS, dir: [0; 3] },
        length: SDL_HAPTIC_INFINITY,
        level,
        // SAFETY: as above
        ..unsafe { std::mem::zeroed() }
    };
    effect
}

impl DirectInput {
    /// Lists the attached game controllers in SDL's order and opens those that are not Xbox
    /// pads. `window` is not needed off Windows.
    pub fn open(_window: isize) -> Result<DirectInput, String> {
        // SAFETY: SDL counts its subsystems; its list is read within its count and freed;
        // names are copied before the next SDL call
        unsafe {
            if !SDL_InitSubSystem(SDL_INIT_JOYSTICK | SDL_INIT_HAPTIC) {
                return Err(format!("SDL's joysticks: {}", sdl3::get_error()));
            }
            SDL_UpdateJoysticks();
            let mut count: c_int = 0;
            let ids = SDL_GetJoysticks(&mut count);
            let mut devices = Vec::new();
            for k in 0..if ids.is_null() { 0 } else { count.max(0) as usize } {
                let id = *ids.add(k);
                let name = SDL_GetJoystickNameForID(id);
                let name = if name.is_null() { String::new() } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
                let is_wheel = SDL_GetJoystickTypeForID(id) == SDL_JOYSTICK_TYPE_WHEEL;
                let mut entry = DiDevice {
                    index: k,
                    name,
                    is_xinput: SDL_IsGamepad(id) && !is_wheel,
                    has_ff: false,
                    is_wheel,
                    pguid: product_guid(SDL_GetJoystickVendorForID(id), SDL_GetJoystickProductForID(id)),
                    joystick: std::ptr::null_mut(),
                    slots: Vec::new(),
                    state: DiState::default(),
                    haptic: std::ptr::null_mut(),
                    effect: SDL_HapticEffectID(-1),
                    ff_started: false,
                    last_magnitude: 0,
                };
                if !entry.is_xinput {
                    let joystick = SDL_OpenJoystick(id);
                    if !joystick.is_null() {
                        let path = SDL_GetJoystickPath(joystick);
                        entry.slots = match (!path.is_null()).then(|| evdev_axes(CStr::from_ptr(path))).flatten() {
                            Some(axes) => axes,
                            // not an evdev node (SDL's own drivers): SDL's numbers, a 16-bit range
                            None => (0..SDL_GetNumJoystickAxes(joystick).max(0) as usize).map(|k| (k < 8).then_some((k, -32768, 32767))).collect(),
                        };
                        entry.has_ff = SDL_IsJoystickHaptic(joystick);
                        entry.joystick = joystick;
                    }
                }
                devices.push(entry);
            }
            if !ids.is_null() {
                SDL_free(ids.cast());
            }
            Ok(DirectInput { devices, sdl: true })
        }
    }

    /// The `JOY` numbers of a Windows `controls.ini`: `[CONTROLLERS] PGUIDn` names device n.
    pub fn ordered(mut self, ini: &ControlsIni) -> DirectInput {
        let mut rest = std::mem::take(&mut self.devices);
        let mut listed = Vec::new();
        let mut n = 0;
        while ini.has_key("CONTROLLERS", &format!("PGUID{n}")) {
            let pguid = ini.get_string("CONTROLLERS", &format!("PGUID{n}")).trim().trim_matches(['{', '}']).to_ascii_uppercase();
            match rest.iter().position(|d| d.pguid == pguid) {
                Some(k) => listed.push(rest.remove(k)),
                None => listed.push(DiDevice { name: format!("{} (not connected)", ini.get_string("CONTROLLERS", &format!("CON{n}"))), ..placeholder(pguid) }),
            }
            n += 1;
        }
        listed.extend(rest);
        for (k, d) in listed.iter_mut().enumerate() {
            d.index = k;
        }
        self.devices = listed;
        self
    }

    /// `InputDevice::poll` for every open device: a device that went away keeps its last
    /// state.
    pub fn poll(&mut self) {
        // SAFETY: SDL calls on joysticks this struct opened
        unsafe {
            SDL_UpdateJoysticks();
            for entry in &mut self.devices {
                if entry.joystick.is_null() {
                    continue;
                }
                if !SDL_JoystickConnected(entry.joystick) {
                    entry.ff_started = false;
                    continue;
                }
                let mut state = DiState::default();
                for (k, slot) in entry.slots.iter().enumerate() {
                    if let Some((slot, min, max)) = *slot {
                        state.axes[slot] = axis_value(di_axis(SDL_GetJoystickAxis(entry.joystick, k as c_int), min, max));
                    }
                }
                for k in 0..4 {
                    state.povs[k] = if (k as c_int) < SDL_GetNumJoystickHats(entry.joystick) { pov(SDL_GetJoystickHat(entry.joystick, k as c_int)) } else { u32::MAX };
                }
                for k in 0..SDL_GetNumJoystickButtons(entry.joystick).clamp(0, 128) {
                    state.buttons[k as usize] = if SDL_GetJoystickButton(entry.joystick, k) { 0x80 } else { 0 };
                }
                entry.state = state;
            }
        }
    }

    /// The state of `JOY` number `joy`, if that device is open.
    pub fn state(&self, joy: i32) -> Option<&DiState> {
        let entry = self.devices.get(usize::try_from(joy).ok()?)?;
        (!entry.joystick.is_null()).then_some(&entry.state)
    }

    /// Prepares force feedback on a device: the wheel's own centring spring off, one constant
    /// force as AC makes it.
    pub fn enable_ff(&mut self, joy: i32) -> Result<(), String> {
        let entry = usize::try_from(joy).ok().and_then(|k| self.devices.get_mut(k)).ok_or("no such device")?;
        if entry.joystick.is_null() {
            return Err("the device is not open".to_string());
        }
        if !entry.has_ff {
            return Err(format!("{} has no force feedback", entry.name));
        }
        // SAFETY: SDL calls on a joystick this struct opened; the effect is a local
        unsafe {
            let haptic = SDL_OpenHapticFromJoystick(entry.joystick);
            if haptic.is_null() {
                return Err(format!("{}'s force feedback: {}", entry.name, sdl3::get_error()));
            }
            let features = SDL_GetHapticFeatures(haptic);
            if features & SDL_HAPTIC_CONSTANT.0 as u32 == 0 {
                SDL_CloseHaptic(haptic);
                return Err(format!("{} has no constant force", entry.name));
            }
            if features & SDL_HAPTIC_AUTOCENTER.0 as u32 != 0 {
                SDL_SetHapticAutocenter(haptic, 0);
            }
            let effect = SDL_CreateHapticEffect(haptic, &constant(0));
            if effect.0 < 0 {
                SDL_CloseHaptic(haptic);
                return Err(format!("the force effect: {}", sdl3::get_error()));
            }
            entry.haptic = haptic;
            entry.effect = effect;
            entry.ff_started = false;
        }
        Ok(())
    }

    /// `InputDevice::sendFF` (the constant force only), as `dinput.rs`.
    pub fn send_ff(&mut self, joy: i32, ff: f32) {
        let Some(entry) = usize::try_from(joy).ok().and_then(|k| self.devices.get_mut(k)) else { return };
        if entry.haptic.is_null() {
            return;
        }
        let magnitude = magnitude(ff.clamp(-1.0, 1.0) * FF_CAP);
        if entry.ff_started && magnitude == entry.last_magnitude {
            return;
        }
        // SAFETY: the haptic and its effect are alive
        let sent = unsafe {
            SDL_UpdateHapticEffect(entry.haptic, entry.effect, &constant(haptic_level(magnitude)))
                && (entry.ff_started || SDL_RunHapticEffect(entry.haptic, entry.effect, 1))
        };
        if sent {
            entry.ff_started = true;
            entry.last_magnitude = magnitude;
        }
    }
}

/// A `JOY` number with no device behind it.
fn placeholder(pguid: String) -> DiDevice {
    DiDevice {
        index: 0,
        name: String::new(),
        is_xinput: false,
        has_ff: false,
        is_wheel: false,
        pguid,
        joystick: std::ptr::null_mut(),
        slots: Vec::new(),
        state: DiState::default(),
        haptic: std::ptr::null_mut(),
        effect: SDL_HapticEffectID(-1),
        ff_started: false,
        last_magnitude: 0,
    }
}

impl Drop for DirectInput {
    fn drop(&mut self) {
        // SAFETY: SDL objects this struct owns; balances its `SDL_InitSubSystem`
        unsafe {
            for entry in &mut self.devices {
                if !entry.haptic.is_null() {
                    SDL_StopHapticEffects(entry.haptic);
                    SDL_CloseHaptic(entry.haptic);
                }
                if !entry.joystick.is_null() {
                    SDL_CloseJoystick(entry.joystick);
                }
            }
            if self.sdl {
                SDL_QuitSubSystem(SDL_INIT_JOYSTICK | SDL_INIT_HAPTIC);
            }
        }
    }
}

/// The controller part of the device list, for the console.
pub fn describe_devices(wheel: &Option<WheelDevice>) -> String {
    let Some(wheel) = wheel else {
        return "  SDL's joysticks: could not be opened\n".to_string();
    };
    let mut out = String::new();
    if wheel.di.devices.is_empty() {
        out.push_str("  no game controller attached (SDL)\n");
    }
    for d in &wheel.di.devices {
        let what = if d.is_xinput {
            "an Xbox-layout pad: read as the pad instead".to_string()
        } else if d.joystick.is_null() {
            "in [CONTROLLERS], not connected".to_string()
        } else {
            format!(
                "{}{}{}",
                if d.is_wheel { "steering wheel" } else { "game controller" },
                if d.has_ff { ", force feedback" } else { "" },
                if wheel.steer_joy() == Some(d.index as i32) { ", steers" } else { "" }
            )
        };
        out.push_str(&format!("  JOY={} \"{}\" PGUID {} ({what})\n", d.index, d.name, d.pguid));
    }
    for note in &wheel.notes {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_map_as_directinput_gives_them() {
        // every raw value of the common ranges, through SDL's scaling, is DirectInput's
        for (min, max) in [(0, 255), (0, 1023), (0, 4095), (0, 16383), (0, 65535), (-32768, 32767), (-511, 512)] {
            for raw in min..=max {
                let want = ((raw - min) as i64 * 20000 + (max - min) as i64 / 2) / (max - min) as i64 - 10000;
                assert_eq!(di_axis(sdl_axis(raw, min, max) as i16, min, max) as i64, want, "{raw} in {min}..{max}");
            }
        }
        assert_eq!((di_axis(-32768, -32768, 32767), di_axis(0, -32768, 32767), di_axis(32767, -32768, 32767)), (-10000, 0, 10000));
        // every magnitude comes back out of SDL's DirectInput backend; Linux's level is SDL's
        for m in -10000..=10000 {
            let level = haptic_level(m) as i32;
            assert_eq!(level * 10000 / 0x7fff, m, "{m}");
            assert!(level.abs() <= 32767);
        }
        assert_eq!((haptic_level(0), haptic_level(3000), haptic_level(-10000)), (0, 9831, -32767));
        // a wheel with X, Z, Rz, a slider and a hat (no Y): DirectInput's numbers 0, 2, 5, 6
        let bits = 1u64 | 1 << 2 | 1 << 5 | 1 << 6 | 1 << 0x10 | 1 << 0x11 | 1 << 0x28;
        assert_eq!(axis_slots(bits), vec![Some(0), Some(2), Some(5), Some(6), None]);
        assert_eq!(product_guid(0x046d, 0xc24f), "C24F046D-0000-0000-0000-504944564944");
        assert_eq!((pov(0), pov(0x01), pov(0x06), pov(0x09)), (u32::MAX, 0, 13500, 31500));
    }

    #[test]
    fn joy_numbers_follow_the_controllers_section() {
        let device = |name: &str, pguid: &str| DiDevice { name: name.to_string(), ..placeholder(pguid.to_string()) };
        let di = DirectInput {
            devices: vec![device("pedals", "B678346E-0000-0000-0000-504944564944"), device("G29", "C24F046D-0000-0000-0000-504944564944"), device("other", "00010002-0000-0000-0000-504944564944")],
            sdl: false,
        };
        let ini = ControlsIni::parse(
            "[CONTROLLERS]\r\nCON0=Xbox One For Windows\r\nPGUID0=02FF045E-0000-0000-0000-504944564944\r\nCON1=Logitech G29\r\nPGUID1={c24f046d-0000-0000-0000-504944564944}\r\nCON2=Pedals\r\nPGUID2=B678346E-0000-0000-0000-504944564944\r\n",
        );
        let di = di.ordered(&ini);
        let names: Vec<_> = di.devices.iter().map(|d| (d.index, d.name.as_str())).collect();
        assert_eq!(names, [(0, "Xbox One For Windows (not connected)"), (1, "G29"), (2, "pedals"), (3, "other")]);
    }
}
