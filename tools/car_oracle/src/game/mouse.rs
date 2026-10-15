// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 34: the keyboard's mouse steering (Ctrl+M). The game's own
//! `KeyboardCarControl::readFromIni` 0x14010ff60 (its `MOUSE_*` keys), `setMouseSteering`
//! 0x140111350 and `getMouseSteering` 0x14010fb70 on a made-up cursor and window (the
//! game's `GetCursorPos`, `GetWindowRect` and `SetCursorPos` are stand-ins here), written to a
//! tape: what went in and what came out. The port lives in `rustyac-game`, which this crate
//! does not build; its example `mouse_steering_check` plays the tape through the port.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicI32, Ordering};

use super::*;

const VA_READ_FROM_INI: usize = 0x1_4010_ff60;
const VA_SET_MOUSE_STEERING: usize = 0x1_4011_1350;
const VA_GET_MOUSE_STEERING: usize = 0x1_4010_fb70;

static CURSOR_X: AtomicI32 = AtomicI32::new(0);
static CURSOR_Y: AtomicI32 = AtomicI32::new(0);
static RECT: [AtomicI32; 4] = [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];
static SET: [AtomicI32; 3] = [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];

pub extern "system" fn get_cursor_pos(point: *mut i32) -> i32 {
    unsafe {
        *point = CURSOR_X.load(Ordering::Relaxed);
        *point.add(1) = CURSOR_Y.load(Ordering::Relaxed);
    }
    1
}

pub extern "system" fn get_window_rect(_window: usize, rect: *mut i32) -> i32 {
    for (i, side) in RECT.iter().enumerate() {
        unsafe { *rect.add(i) = side.load(Ordering::Relaxed) };
    }
    1
}

pub extern "system" fn set_cursor_pos(x: i32, y: i32) -> i32 {
    SET[0].store(x, Ordering::Relaxed);
    SET[1].store(y, Ordering::Relaxed);
    SET[2].fetch_add(1, Ordering::Relaxed);
    1
}

pub extern "system" fn get_active_window() -> usize {
    0x1234
}

/// The imports the oracle answers itself.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("shell32.dll", "SHGetFolderPathW", multi::sh_get_folder_path_w as *const () as usize),
        ("user32.dll", "GetCursorPos", get_cursor_pos as *const () as usize),
        ("user32.dll", "GetWindowRect", get_window_rect as *const () as usize),
        ("user32.dll", "SetCursorPos", set_cursor_pos as *const () as usize),
        ("user32.dll", "GetActiveWindow", get_active_window as *const () as usize),
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

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

/// The controls.ini files the reader is given (`|` for a line break).
const INIS: [&str; 9] = [
    "[KEYBOARD]|STEERING_SPEED=1.75|MOUSE_STEER=1|MOUSE_SPEED=0.1|MOUSE_ACCELERATOR_BRAKE=1",
    "[KEYBOARD]|MOUSE_STEER=0|MOUSE_SPEED=0.05|MOUSE_ACCELERATOR_BRAKE=0",
    "[KEYBOARD]|MOUSE_STEER=2|MOUSE_SPEED=0",
    "[KEYBOARD]|MOUSE_STEER=-1|MOUSE_SPEED=-0.02|MOUSE_ACCELERATOR_BRAKE=3",
    "[KEYBOARD]|STEERING_SPEED=1",
    "[KEYBOARD]|MOUSE_STEER=1|MOUSE_SPEED=0.0001",
    "[KEYBOARD]|MOUSE_STEER=1|MOUSE_SPEED=1e30",
    "[KEYBOARD]|MOUSE_STEER=1|MOUSE_SPEED=0.033333",
    "[OTHER]|X=1",
];

/// `car_oracle mouse-micro`: the tape (one line per call; numbers as hex bits where floats).
pub unsafe fn micro(acs: &Acs, seed: u32, steps: usize) -> Result<String, String> {
    let mut tape = String::new();
    let cfg = std::path::Path::new("oracle_no_documents/Assetto Corsa/cfg");
    std::fs::create_dir_all(cfg).map_err(|e| e.to_string())?;
    // INIReaderDocuments' first making: Documents (the stand-in "oracle_no_documents") + "/" +
    // system/cfg/inireaderdocuments.ini [FOLDER] PROGRAM_NAME + "/"
    std::fs::create_dir_all("system/cfg").map_err(|e| e.to_string())?;
    std::fs::write("system/cfg/inireaderdocuments.ini", "[FOLDER]
PROGRAM_NAME=Assetto Corsa
").map_err(|e| e.to_string())?;
    let read_from_ini: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_READ_FROM_INI));
    let set_mouse_steering: extern "C" fn(*mut u8, bool) = std::mem::transmute(acs.va(VA_SET_MOUSE_STEERING));
    let get_mouse_steering: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(acs.va(VA_GET_MOUSE_STEERING));
    // the reader on each file, after the constructor's values (0x14010f0e0, the parts it reads)
    for ini in INIS {
        std::fs::write(cfg.join("controls.ini"), ini.replace('|', "\r\n")).map_err(|e| e.to_string())?;
        let k = acs.alloc(0x120);
        wr(k, 0x71, 0u8);
        wr(k, 0x72, 0u8);
        wr(k, 0x74, 0.0f32);
        wr(k, 0x78, 1000.0f32);
        read_from_ini(k);
        let (steer, brake, speed): (u8, u8, f32) = (rd(k, 0x71), rd(k, 0x72), rd(k, 0x78));
        writeln!(tape, "ini {ini} -> {steer} {brake} {:08x}", speed.to_bits()).unwrap();
    }
    // the toggle and the integrator
    let mut rng = Rng(seed as u64);
    let k = acs.alloc(0x120);
    wr(k, 0x80, 0x1234usize);
    let speeds = [1000.0f32, 500.0, 1.0, 333.33, 0.5, 1e-30, 1e30, -200.0];
    for step in 0..steps {
        if step % 50 == 0 {
            let on = !rng.next().is_multiple_of(4);
            set_mouse_steering(k, on);
            let speed = speeds[(rng.next() % speeds.len() as u64) as usize];
            wr(k, 0x78, speed);
            writeln!(tape, "set {} -> {} {:08x}", on as u8, rd::<u8>(k, 0x71), speed.to_bits()).unwrap();
        }
        let rect = match rng.next() % 6 {
            0 => [0, 0, 1920, 1080],
            1 => [-1921, 0, -1, 1080],
            2 => [100, 50, 1381, 770],
            3 => [i32::MIN + 5, 0, i32::MAX - 3, 99],
            _ => {
                let l = rng.range(-3000, 3000);
                let t = rng.range(-2000, 2000);
                [l, t, l + rng.range(-5, 4000), t + rng.range(-5, 3000)]
            }
        };
        let cx = (rect[2].wrapping_sub(rect[0])) / 2 + rect[0];
        let x = match rng.next() % 5 {
            0 => cx,
            1 => rng.range(-40000, 40000),
            _ => cx.wrapping_add(rng.range(-300, 300)),
        };
        CURSOR_X.store(x, Ordering::Relaxed);
        CURSOR_Y.store(rng.range(-500, 2000), Ordering::Relaxed);
        for (side, value) in RECT.iter().zip(rect) {
            side.store(value, Ordering::Relaxed);
        }
        if rng.next().is_multiple_of(97) {
            wr(k, 0x74, f32::from_bits(0x7fc0_0000 | (rng.next() as u32 & 0xff)));
        }
        let old: f32 = rd(k, 0x74);
        let speed: f32 = rd(k, 0x78);
        SET[2].store(0, Ordering::Relaxed);
        let r = get_mouse_steering(k);
        let after: f32 = rd(k, 0x74);
        writeln!(
            tape,
            "step {:08x} {:08x} {x} {} {} {} {} -> {:08x} {:08x} {} {} {}",
            old.to_bits(),
            speed.to_bits(),
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            r.to_bits(),
            after.to_bits(),
            SET[0].load(Ordering::Relaxed),
            SET[1].load(Ordering::Relaxed),
            SET[2].load(Ordering::Relaxed)
        )
        .unwrap();
    }
    Ok(tape)
}
