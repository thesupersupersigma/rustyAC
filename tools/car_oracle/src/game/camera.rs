// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 34: the camera of a session that begins in the ESC menu. The game's own
//! `CameraModeStart::updateCar` 0x140060470 on a made-up car 0 (`Sim::getCar` and
//! `CarAvatar::isInPit` are the game's; `rand()` is the C library's generator from seed 1,
//! answered here; `setShadowMapsSplits` and `AudioEngine::setListener` return at once), written
//! to a tape: what went in and the camera that came out. The port lives in `rustyac-game`,
//! which this crate does not build; its example `start_camera_check` plays the tape.
//!
//! And F7's free camera: the game's `CameraMouseControlBase::update` 0x14020ff30 (the arrows
//! and the mouse look) on made-up keys and cursors (`GetAsyncKeyState` and `GetCursorPos` are
//! answered here), on a tape of its own for the same example.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use super::*;

const VA_UPDATE_CAR: usize = 0x1_4006_0470;
const VA_SET_SHADOW_MAPS_SPLITS: usize = 0x1_4020_d5f0;
const VA_SET_LISTENER: usize = 0x1_401f_bb30;

static SEED: AtomicU32 = AtomicU32::new(1);
static KEYS: AtomicU32 = AtomicU32::new(0);
static CURSOR: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

/// The keys the free camera reads, as bits of [`KEYS`]: Shift, Ctrl, up, down, left, right,
/// the right mouse button, Page Up, Page Down.
const FREE_KEYS: [i32; 9] = [0x10, 0x11, 0x26, 0x28, 0x25, 0x27, 0x02, 0x21, 0x22];

pub extern "system" fn get_async_key_state(key: i32) -> i16 {
    let held = FREE_KEYS.iter().position(|k| *k == key).is_some_and(|bit| KEYS.load(Ordering::Relaxed) & (1 << bit) != 0);
    if held {
        -32768
    } else {
        0
    }
}

pub extern "system" fn get_cursor_pos(point: *mut i32) -> i32 {
    unsafe {
        *point = CURSOR[0].load(Ordering::Relaxed);
        *point.add(1) = CURSOR[1].load(Ordering::Relaxed);
    }
    1
}

/// MSVCR120's `rand`
pub extern "C" fn rand() -> i32 {
    let seed = SEED.load(Ordering::Relaxed).wrapping_mul(214013).wrapping_add(2531011);
    SEED.store(seed, Ordering::Relaxed);
    ((seed >> 16) & 0x7fff) as i32
}

/// The imports the oracle answers itself.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("msvcr120.dll", "rand", rand as *const () as usize),
        ("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize),
        ("user32.dll", "GetCursorPos", get_cursor_pos as *const () as usize),
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

    fn float(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() % 1_000_000) as f32 / 1_000_000.0 * (hi - lo)
    }
}

fn bits(tape: &mut String, values: &[f32]) {
    for v in values {
        write!(tape, " {:08x}", v.to_bits()).unwrap();
    }
}

/// `car_oracle start-camera-micro`: the tape. Line `crew <side>` per camera, then per frame
/// `frame <eye_x> <body 16> <contact 12> <pit 3> <dt> -> <matrix 16> <fov> <near> <far> <exposure>`.
pub unsafe fn micro(acs: &Acs, seed: u32, steps: usize) -> Result<String, String> {
    acs.patch(acs.va(VA_SET_SHADOW_MAPS_SPLITS), &[0xc3]);
    acs.patch(acs.va(VA_SET_LISTENER), &[0xc3]);
    let update_car: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE_CAR));
    let mut tape = String::new();
    let mut rng = Rng(seed as u64);
    let game = acs.alloc(0x400);
    let camera = acs.alloc(0x800);
    let car = acs.alloc(0x1200);
    let cars = acs.alloc(8);
    wr(cars, 0, car as usize);
    let sim = acs.alloc(0x400);
    wr(sim, 0x208, cars as usize);
    wr(sim, 0x210, cars as usize + 8);
    let mut this = std::ptr::null_mut();
    let mut body = [0.0f32; 16];
    for step in 0..steps {
        if step % 400 == 0 {
            // a new CameraModeStart (the ctor's values; rand goes on)
            this = acs.alloc(0x100);
            wr(this, 8, game as usize);
            wr(this, 0x70, camera as usize);
            wr(this, 0x78, 1000.0f32);
            for (o, v) in [(0x80, 1.0f32), (0x84, 5.0), (0x88, 1.0), (0x8c, 7.0), (0x9c, 1.0), (0xa0, 0.5)] {
                wr(this, o, v);
            }
            wr(this, 0xa8, sim as usize);
            wr(this, 0xc8, 2.0f32);
            wr(this, 0xcc, 0.4f32);
            wr(this, 0xe8, 0.1f32);
            wr(this, 0xec, -2.0f32);
            let crew = match rng.next() % 4 {
                0 => -1.0f32,
                1 => 1.0,
                2 => -0.0,
                _ => rng.float(-3.0, 3.0),
            };
            wr(this, 0xfc, crew);
            writeln!(tape, "crew {:08x}", crew.to_bits()).unwrap();
            // a car somewhere, turned some way
            let yaw = rng.float(-3.2, 3.2);
            let pitch = rng.float(-0.3, 0.3);
            let (s, c) = (yaw.sin(), yaw.cos());
            let (sp, cp) = (pitch.sin(), pitch.cos());
            body = [c, 0.0, -s, 0.0, s * sp, cp, c * sp, 0.0, s * cp, -sp, c * cp, 0.0, rng.float(-2000.0, 2000.0), rng.float(-50.0, 300.0), rng.float(-2000.0, 2000.0), 1.0];
        }
        // the car moves on
        let speed = rng.float(0.0, 2.0);
        body[12] += body[8] * speed;
        body[13] += body[9] * speed;
        body[14] += body[10] * speed;
        let eye_x = match rng.next() % 5 {
            0 => 0.0,
            1 => -0.0,
            2 => rng.float(-0.5, 0.0),
            _ => rng.float(0.0, 0.5),
        };
        let mut contact = [0.0f32; 12];
        for (w, chunk) in contact.chunks_mut(3).enumerate() {
            let (dx, dz) = ([0.8f32, -0.8, 0.8, -0.8][w], [1.3f32, 1.3, -1.2, -1.2][w]);
            for (i, v) in chunk.iter_mut().enumerate() {
                *v = body[12 + i] + body[i] * dx + body[8 + i] * dz - body[4 + i] * rng.float(0.28, 0.34);
            }
        }
        let pit = match rng.next() % 3 {
            0 => [body[12] + rng.float(-1.0, 1.0), body[13] + rng.float(-0.5, 0.5), body[14] + rng.float(-1.0, 1.0)],
            1 => [body[12], body[13], body[14]],
            _ => [rng.float(-100.0, 100.0), 0.0, rng.float(-100.0, 100.0)],
        };
        let dt = match rng.next() % 12 {
            0 => 0.5,
            1 => rng.float(0.5, 3.0),
            2 => 0.0,
            _ => rng.float(0.001, 0.05),
        };
        wr(car, 0x218, eye_x);
        for (i, v) in body.iter().enumerate() {
            wr(car, 0x26c + 4 * i, *v);
        }
        for (i, v) in contact.iter().enumerate() {
            wr(car, 0x8e4 + 4 * i, *v);
        }
        for (i, v) in pit.iter().enumerate() {
            wr(car, 0x10a8 + 4 * i, *v);
        }
        update_car(this, dt);
        tape.push_str("frame");
        bits(&mut tape, &[eye_x]);
        bits(&mut tape, &body);
        bits(&mut tape, &contact);
        bits(&mut tape, &pit);
        bits(&mut tape, &[dt]);
        tape.push_str(" ->");
        let matrix: Vec<f32> = (0..16).map(|i| rd(camera, 0x0c + 4 * i)).collect();
        bits(&mut tape, &matrix);
        bits(&mut tape, &[rd(camera, 0x08), rd(camera, 0x70), rd(camera, 0x74), rd(camera, 0x19c)]);
        tape.push('\n');
    }
    Ok(tape)
}

const VA_MOUSE_CONTROL_UPDATE: usize = 0x1_4020_ff30;

/// The free camera's tape: `free <matrix 16> <fov>` when a camera starts (as the last mode
/// left it), then per frame `step <keys> <x> <y> <dt> -> <matrix 16>`.
pub unsafe fn free_micro(acs: &Acs, seed: u32, steps: usize) -> Result<String, String> {
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_MOUSE_CONTROL_UPDATE));
    let mut tape = String::new();
    let mut rng = Rng(seed as u64 ^ 0x5eed);
    let camera = acs.alloc(0x800);
    let mut base = std::ptr::null_mut();
    let (mut x, mut y) = (0i32, 0i32);
    for step in 0..steps {
        if step % 300 == 0 {
            // CameraMouseControlBase ctor 0x14020fc20, then CameraMouseControl's moveSpeed 25
            base = acs.alloc(0x60);
            for (o, v) in [(0x8, 5.0f32), (0xc, 0.05), (0x10, 50.0), (0x14, 0.01), (0x18, 25.0), (0x20, 0.05), (0x24, 50.0)] {
                wr(base, o, v);
            }
            wr(base, 0x1c, 0u8);
            wr(base, 0x1d, 1u8);
            wr(base, 0x28, camera as usize);
            let yaw = rng.float(-3.2, 3.2);
            let pitch = rng.float(-0.6, 0.6);
            let (s, c) = (yaw.sin(), yaw.cos());
            let (sp, cp) = (pitch.sin(), pitch.cos());
            let matrix = [c, 0.0, -s, 0.0, s * sp, cp, c * sp, 0.0, s * cp, -sp, c * cp, 0.0, rng.float(-2000.0, 2000.0), rng.float(-50.0, 300.0), rng.float(-2000.0, 2000.0), 1.0];
            let fov = [40.0f32, 56.0, 60.0, rng.float(10.0, 120.0)][(rng.next() % 4) as usize];
            for (i, v) in matrix.iter().enumerate() {
                wr(camera, 0x0c + 4 * i, *v);
            }
            wr(camera, 0x08, fov);
            tape.push_str("free");
            bits(&mut tape, &matrix);
            bits(&mut tape, &[fov]);
            tape.push('\n');
        }
        let keys = (rng.next() & (rng.next() | rng.next())) as u32 & 0x1ff;
        KEYS.store(keys, Ordering::Relaxed);
        if !rng.next().is_multiple_of(3) {
            x = x.wrapping_add((rng.next() % 61) as i32 - 30);
            y = y.wrapping_add((rng.next() % 61) as i32 - 30);
        } else if rng.next().is_multiple_of(50) {
            x = (rng.next() % 4000) as i32 - 1000;
            y = (rng.next() % 3000) as i32 - 500;
        }
        CURSOR[0].store(x, Ordering::Relaxed);
        CURSOR[1].store(y, Ordering::Relaxed);
        let dt = if rng.next().is_multiple_of(20) { rng.float(0.0, 0.5) } else { rng.float(0.004, 0.034) };
        update(base, dt);
        write!(tape, "step {keys:x} {x} {y}").unwrap();
        bits(&mut tape, &[dt]);
        tape.push_str(" ->");
        let matrix: Vec<f32> = (0..16).map(|i| rd(camera, 0x0c + 4 * i)).collect();
        bits(&mut tape, &matrix);
        tape.push('\n');
    }
    Ok(tape)
}
