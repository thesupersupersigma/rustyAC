// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 37b: the gamepad's car control. The game's own `JoypadCarControl` constructor
//! 0x1401d9280 (it reads `cfg/controls.ini` `[X360]`, `[ADVANCED]` and the action sections,
//! and `system/cfg/assetto_corsa.ini [GAMEPAD] USE_LEGACY_CODE`) and `acquireControls`
//! 0x1401dbce0 on the game's `X360Joypad`, whose `XInputGetState` (`xinput1_3.dll` #2) and
//! the keyboard's `GetAsyncKeyState` are stand-ins here, written to a tape: the settings, what
//! went in and what came out. `[X360] SPEED_SENSITIVITY` takes the controls page's default
//! (0.5), 0, AC's preset value (0.1), 1 and a missing key in turn. The port lives in
//! `rustyac-game`, which this crate does not build; its example `pad_steering_check` plays the
//! tape through the port.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

const VA_JOYPAD_CAR_CONTROL: usize = 0x1_401d_9280; // JoypadCarControl(Car*, Joypad*)
const VA_ACQUIRE_CONTROLS: usize = 0x1_401d_bce0; // acquireControls(CarControls*, float, CarControlsInput*)
const VA_X360_JOYPAD_VFTABLE: usize = 0x1_404f_1160;

// JoypadCarControl
const P_SIZE: usize = 0x1a0;
const P_SPEED_SENSITIVITY: usize = 0x18;
const P_STEER_GAMMA: usize = 0x1c;
const P_STEER_FILTER: usize = 0x20;
const P_DEAD_ZONE: usize = 0x24;
const P_STEER_SPEED: usize = 0x28;
const P_LAST_MAX_STEER: usize = 0x2c;
const P_LAST_RAW: usize = 0x30;
const P_LAST_NORMALIZED: usize = 0x34;
const P_USE_LEGACY: usize = 0x38;
const P_STEER_WITH_RIGHT: usize = 0x49;
const P_RUMBLE: usize = 0x4c;
/// 27 `SecondaryJoypadButton`s of 12 bytes: buttonMask u32, key i32, allowKeyboardInput u8.
const P_BUTTONS: usize = 0x50;

// CarControls (52 bytes)
const C_SIZE: usize = 0x34;
const C_HAND_BRAKE: usize = 0x10;
const C_GAS: usize = 0x24;
const C_BRAKE: usize = 0x28;
const C_STEER: usize = 0x2c;
const C_CLUTCH: usize = 0x30;
/// The flags the pad writes: gearUp, gearDn, drs, kers, brakeBalanceUp, brakeBalanceDn,
/// absUp, absDn, tcUp, tcDn, engineBrakeUp, engineBrakeDn, MGUKDeliveryUp, MGUKDeliveryDn,
/// MGUKRecoveryUp, MGUKRecoveryDn, MGUHMode (turboUp/Dn at 0x18/0x19 belong to a control
/// rustyAC does not port).
const C_FLAGS: [usize; 17] = [0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x14, 0x15, 0x16, 0x17, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20];

/// The game's car: the constructor sets the auto clutch's flags at 0x3090 + 0xc / 0xd.
const CAR_BYTES: usize = 0x4000;

/// The action sections, in the order the constructor reads them.
const ACTIONS: [&str; 27] = [
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

/// `XBOXBUTTON` names (the game's, and one it does not know).
const BUTTONS: [&str; 18] = [
    "-1",
    "RSHOULDER",
    "LSHOULDER",
    "A",
    "B",
    "X",
    "Y",
    "DPAD_LEFT",
    "DPAD_RIGHT",
    "DPAD_UP",
    "DPAD_DOWN",
    "LEFT_THUMB",
    "RIGHT_THUMB",
    "START",
    "BACK",
    "RTHUMB_PRESS",
    "LTHUMB_PRESS",
    "TRIANGLE",
];

/// The XInput state of the next call: dwPacketNumber, then the gamepad (16 bytes).
static PAD: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
/// Keys 0x41..0x58 held: bit (key - 0x41).
static KEYS: AtomicU32 = AtomicU32::new(0);

/// `XInputGetState(user, state)`: the oracle's pad.
pub extern "system" fn xinput_get_state(_user: u32, state: *mut u32) -> u32 {
    for (i, word) in PAD.iter().enumerate() {
        unsafe { state.add(i).write_unaligned(word.load(Ordering::Relaxed)) };
    }
    0
}

/// `GetAsyncKeyState`: the oracle's keys.
pub extern "system" fn get_async_key_state(key: i32) -> i16 {
    let keys = KEYS.load(Ordering::Relaxed);
    if (0x41..=0x58).contains(&key) && keys & (1 << (key - 0x41)) != 0 {
        0x8000u16 as i16
    } else {
        0
    }
}

/// The imports the oracle answers itself.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("shell32.dll", "SHGetFolderPathW", multi::sh_get_folder_path_w as *const () as usize),
        ("xinput1_3.dll", "#2", xinput_get_state as *const () as usize),
        ("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize),
    ]
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// In `lo..hi`.
    fn float(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() >> 40) as f32 / (1u64 << 24) as f32 * (hi - lo)
    }

    fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[(self.next() % of.len() as u64) as usize]
    }
}

/// `SPEED_SENSITIVITY` by group: the controls page's default, 0, AC's preset, 1, missing.
const SPEED_SENSITIVITY: [Option<&str>; 5] = [Some("0.5"), Some("0"), Some("0.1"), Some("1"), None];

/// One group's controls.ini (`|` for a line break) and `USE_LEGACY_CODE`.
fn group(rng: &mut Rng, index: usize) -> (String, bool) {
    let mut ini = String::from("[X360]");
    let mut key = |name: &str, value: Option<&str>| {
        if let Some(v) = value {
            write!(ini, "|{name}={v}").unwrap();
        }
    };
    key("SPEED_SENSITIVITY", SPEED_SENSITIVITY[index % SPEED_SENSITIVITY.len()]);
    key("STEER_THUMB", *rng.pick(&[Some("LEFT"), Some("LEFT"), Some("RIGHT"), None]));
    key("STEER_GAMMA", *rng.pick(&[Some("0.5"), Some("1"), Some("2.0"), Some("4"), Some("1.37")]));
    key("STEER_FILTER", *rng.pick(&[Some("0"), Some("0.7"), Some("1"), Some("0.25")]));
    key("STEER_DEADZONE", *rng.pick(&[Some("0.0"), Some("0.05"), Some("0.5"), Some("0.95")]));
    key("STEER_SPEED", *rng.pick(&[Some("0.05"), Some("0.2"), Some("1"), Some("3"), None]));
    key("RUMBLE_INTENSITY", *rng.pick(&[Some("0.5"), Some("0"), None]));
    let combine = rng.next() % 3;
    if combine > 0 {
        write!(ini, "|[ADVANCED]|COMBINE_WITH_KEYBOARD_CONTROL={}", combine - 1).unwrap();
    }
    for action in ACTIONS {
        if rng.next().is_multiple_of(9) {
            continue; // a section missing
        }
        let button = rng.pick(&BUTTONS);
        let k = rng.next() % 30;
        let key = if k >= 24 { "-1".to_string() } else { format!("0x{:X}", 0x41 + k) };
        write!(ini, "|[{action}]|JOY=-1|BUTTON=-1|KEY={key}|XBOXBUTTON={button}").unwrap();
    }
    ini.push('|');
    (ini, index / SPEED_SENSITIVITY.len() % 2 == 1)
}

fn stick(rng: &mut Rng, dead_zone: f32) -> i16 {
    match rng.next() % 8 {
        0 => 0,
        1 => 32767,
        2 => -32768,
        3 => {
            // the dead zone's edge
            let edge = (dead_zone * 32767.0) as i32 + (rng.next() % 3) as i32 - 1;
            (if rng.next().is_multiple_of(2) { edge } else { -edge }).clamp(-32768, 32767) as i16
        }
        4 => (rng.next() % 41) as i16 - 20,
        _ => rng.next() as i16,
    }
}

/// `car_oracle pad-micro`: the tape. One `pad` line per group of 500 calls (its settings
/// and what the constructor made of them), one `step` line per call; floats as hex bits.
pub unsafe fn micro(acs: &Acs, seed: u32, steps: usize) -> Result<String, String> {
    let mut tape = String::new();
    let cfg = std::path::Path::new("oracle_no_documents/Assetto Corsa/cfg");
    std::fs::create_dir_all(cfg).map_err(|e| e.to_string())?;
    // INIReaderDocuments' first making: Documents (the stand-in "oracle_no_documents") + "/" +
    // system/cfg/inireaderdocuments.ini [FOLDER] PROGRAM_NAME + "/"
    std::fs::create_dir_all("system/cfg").map_err(|e| e.to_string())?;
    std::fs::write("system/cfg/inireaderdocuments.ini", "[FOLDER]\r\nPROGRAM_NAME=Assetto Corsa\r\n").map_err(|e| e.to_string())?;
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_JOYPAD_CAR_CONTROL));
    let acquire: extern "C" fn(*mut u8, *mut u8, f32, *const u8) = std::mem::transmute(acs.va(VA_ACQUIRE_CONTROLS));
    let mut rng = Rng(seed as u64 ^ 0x5057_3338);
    // the game's pad: X360Joypad is a vtable and the XInput state
    let joy = acs.alloc(0x18);
    wr(joy, 0, acs.va(VA_X360_JOYPAD_VFTABLE));
    let controls = acs.alloc(C_SIZE);
    let input = acs.alloc(8);
    let mut pad = std::ptr::null_mut();
    let mut dead_zone = 0.0f32;
    for step in 0..steps {
        if step % 500 == 0 {
            let (ini, legacy) = group(&mut rng, step / 500);
            std::fs::write(cfg.join("controls.ini"), ini.replace('|', "\r\n")).map_err(|e| e.to_string())?;
            std::fs::write("system/cfg/assetto_corsa.ini", format!("[GAMEPAD]\r\nUSE_LEGACY_CODE={}\r\n", legacy as u8)).map_err(|e| e.to_string())?;
            let car = acs.alloc(CAR_BYTES);
            pad = acs.alloc(P_SIZE);
            ctor(pad, car, joy);
            dead_zone = rd(pad, P_DEAD_ZONE);
            let mut buttons = String::new();
            for i in 0..ACTIONS.len() {
                let at = P_BUTTONS + 12 * i;
                write!(buttons, " {:x}/{}/{}", rd::<u32>(pad, at), rd::<i32>(pad, at + 4), rd::<u8>(pad, at + 8)).unwrap();
            }
            writeln!(
                tape,
                "pad {} {ini} -> {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {} {}{buttons}",
                legacy as u8,
                rd::<f32>(pad, P_SPEED_SENSITIVITY).to_bits(),
                rd::<f32>(pad, P_STEER_GAMMA).to_bits(),
                rd::<f32>(pad, P_STEER_FILTER).to_bits(),
                dead_zone.to_bits(),
                rd::<f32>(pad, P_STEER_SPEED).to_bits(),
                rd::<f32>(pad, P_RUMBLE).to_bits(),
                rd::<u8>(pad, P_USE_LEGACY),
                rd::<u8>(pad, P_STEER_WITH_RIGHT),
            )
            .unwrap();
        }
        // the pad, the keys, the car
        let buttons = if rng.next().is_multiple_of(3) { 0u16 } else { rng.next() as u16 };
        let mut trigger = || {
            let some = (rng.next() % 254) as u8 + 1;
            *rng.pick(&[0u8, 255, some])
        };
        let (lt, rt) = (trigger(), trigger());
        let (lx, ly, rx, ry) = (stick(&mut rng, dead_zone), stick(&mut rng, dead_zone), stick(&mut rng, dead_zone), stick(&mut rng, dead_zone));
        PAD[0].store(0, Ordering::Relaxed);
        PAD[1].store(buttons as u32 | (lt as u32) << 16 | (rt as u32) << 24, Ordering::Relaxed);
        PAD[2].store(lx as u16 as u32 | (ly as u16 as u32) << 16, Ordering::Relaxed);
        PAD[3].store(rx as u16 as u32 | (ry as u16 as u32) << 16, Ordering::Relaxed);
        let keys = if rng.next().is_multiple_of(2) { 0 } else { (rng.next() & 0xff_ffff) as u32 };
        KEYS.store(keys, Ordering::Relaxed);
        let lock = match rng.next() % 4 {
            0 => 90.0,
            1 => *rng.pick(&[180.0f32, 360.0, 450.0, 900.0]),
            _ => rng.float(180.0, 1080.0),
        };
        let speed = match rng.next() % 6 {
            0 => 0.0,
            1 => *rng.pick(&[1e-6f32, 1e6, -5.0, 5.555_555_5]),
            _ => rng.float(0.0, 100.0),
        };
        wr(input, 0, lock);
        wr(input, 4, speed);
        if step % 64 == 0 {
            wr(controls, C_STEER, rng.float(-1.0, 1.0));
        }
        // flags the call does not write would show as 5a
        for at in C_FLAGS {
            wr(controls, at, 0x5au8);
        }
        let steer_in: f32 = rd(controls, C_STEER);
        let dt = 0.003f32;
        acquire(pad, controls, dt, input);
        let flags: String = C_FLAGS.iter().map(|at| format!("{:02x}", rd::<u8>(controls, *at))).collect();
        writeln!(
            tape,
            "step {buttons:04x} {lt} {rt} {lx} {ly} {rx} {ry} {keys:x} {:08x} {:08x} {:08x} {:08x} -> {:08x} {:08x} {:08x} {:08x} {:08x} {flags} {:08x} {:08x} {:08x}",
            lock.to_bits(),
            speed.to_bits(),
            dt.to_bits(),
            steer_in.to_bits(),
            rd::<f32>(controls, C_STEER).to_bits(),
            rd::<f32>(controls, C_GAS).to_bits(),
            rd::<f32>(controls, C_BRAKE).to_bits(),
            rd::<f32>(controls, C_CLUTCH).to_bits(),
            rd::<f32>(controls, C_HAND_BRAKE).to_bits(),
            rd::<f32>(pad, P_LAST_MAX_STEER).to_bits(),
            rd::<f32>(pad, P_LAST_RAW).to_bits(),
            rd::<f32>(pad, P_LAST_NORMALIZED).to_bits(),
        )
        .unwrap();
    }
    Ok(tape)
}
