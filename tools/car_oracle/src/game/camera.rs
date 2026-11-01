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

const VA_ONBOARD_UPDATE: usize = 0x1_400c_9ea0;
const VA_DRIVABLE_UPDATE: usize = 0x1_400c_6a30;
const VA_DRIVABLE_NEXT_MODE: usize = 0x1_400c_6790;
const VA_CAR_CAMERAS_UPDATE: usize = 0x1_400c_47d0;
const VA_CAR_CAMERAS_NEXT: usize = 0x1_400c_4710;
const VA_SIGNAL_GENERATOR_3D: usize = 0x1_400c_8400;
const VA_MAKE_BODY_MATRIX: usize = 0x1_400d_8ec0;

/// A `std::vector` at `at` over `count` zeroed elements of `size` bytes; the elements.
unsafe fn vector(acs: &Acs, at: *mut u8, size: usize, count: usize) -> *mut u8 {
    let data = acs.alloc(size * count);
    wr(at, 0, data as usize);
    wr(at, 8, data as usize + size * count);
    wr(at, 16, data as usize + size * count);
    data
}

/// A rotation (rows: side, up, forward) turned by yaw, then pitch, then roll.
fn axes(yaw: f32, pitch: f32, roll: f32) -> [f32; 9] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let forward = [sy * cp, -sp, cy * cp];
    let side0 = [cy, 0.0, -sy];
    let up0 = [sy * sp, cp, cy * sp];
    let side: Vec<f32> = (0..3).map(|i| side0[i] * cr + up0[i] * sr).collect();
    let up: Vec<f32> = (0..3).map(|i| up0[i] * cr - side0[i] * sr).collect();
    [side[0], side[1], side[2], up[0], up[1], up[2], forward[0], forward[1], forward[2]]
}

/// Writes `values` at `at + offset` and onto the tape's `cfg`.
unsafe fn put(at: *mut u8, offset: usize, values: &[f32], cfg: &mut Vec<f32>) {
    for (i, v) in values.iter().enumerate() {
        wr(at, offset + 4 * i, *v);
    }
    cfg.extend_from_slice(values);
}

/// Task 46: the cameras that drive with the car, on a tape of their own: the game's
/// `CameraOnBoard::update` (F1's cockpit), `CameraDrivableManager::update` (chase, chase 2,
/// bonnet, bumper, dash) and `CameraCarManager::update` (F6, the car's `cameras.ini`) on a
/// made-up car 0 whose 3D matrix is the game's own `CarAvatar::makeBodyMatrix`; F1 and F6 at
/// frames 100 and 200 of each 300 (`CameraDrivableManager::nextMode`, back to the cockpit when
/// it says so, `CameraCarManager::nextCamera`). Each 300 frames a new car and new cameras, with
/// their features on or off: 1 the g forces (`gForceMix`, `accGXMul` ...), 2 the head shake,
/// 4 the look keys (left, right, back, the views' own yaw), 8 `IS_WORLD_ALIGNED`.
///
/// Lines: `seg <mode 0 cockpit, 1 drivable, 2 car> <drivable view 0-4> <car camera> <features>
/// <cfg ...>` (in the order `put` writes them), then per frame `f <switch 0|1> <actions hex>
/// <body 16> <rear left hub 3> <rear right hub 3> <speed> <accG 3> <velocity 3> <dt> ->
/// <matrix 16> <fov> <near> <far> <exposure>`. `rand()` runs from seed 1 at the tape's start.
pub unsafe fn driving_micro(acs: &Acs, seed: u32, steps: usize) -> Result<String, String> {
    acs.patch(acs.va(VA_SET_SHADOW_MAPS_SPLITS), &[0xc3]);
    acs.patch(acs.va(VA_SET_LISTENER), &[0xc3]);
    let onboard_update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_ONBOARD_UPDATE));
    let drivable_update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_DRIVABLE_UPDATE));
    let next_mode: extern "C" fn(*mut u8) -> bool = std::mem::transmute(acs.va(VA_DRIVABLE_NEXT_MODE));
    let cars_update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_CAMERAS_UPDATE));
    let next_camera: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_CAMERAS_NEXT));
    let signal_3d: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SIGNAL_GENERATOR_3D));
    let make_body_matrix: extern "C" fn(*mut u8, *const u8, *mut u8) = std::mem::transmute(acs.va(VA_MAKE_BODY_MATRIX));
    SEED.store(1, Ordering::Relaxed);
    let mut tape = String::new();
    let mut rng = Rng(seed as u64 ^ 0xca3e);
    // every slot of the scene camera's vtable returns at once (only its listener matrix is asked for)
    let vtable = acs.alloc(0x200);
    for slot in 0..64 {
        wr(vtable, slot * 8, acs.va(VA_SET_LISTENER));
    }
    let game = acs.alloc(0x400);
    let null = std::ptr::null_mut::<u8>();
    let (mut onboard, mut drivable, mut cars, mut camera, mut car) = (null, null, null, null, null);
    let mut mode = 0u32;
    let mut features = 0u32;
    let (mut yaw, mut pitch, mut roll, mut pos) = (0.0f32, 0.0f32, 0.0f32, [0.0f32; 3]);
    let mut actions = 0u32;
    for step in 0..steps {
        let frame = step % 300;
        if frame == 0 {
            mode = (rng.next() % 3) as u32;
            features = if rng.next().is_multiple_of(4) { 0 } else { (rng.next() % 16) as u32 };
            let on = |bit: u32| features & bit != 0;
            let view = (rng.next() % 5) as u32;
            let sim = acs.alloc(0x400);
            car = acs.alloc(0x1200);
            camera = acs.alloc(0x800);
            wr(camera, 0, vtable as usize);
            for i in 0..4 {
                wr(camera, 0x0c + 20 * i, 1.0f32); // Camera's ctor: an identity matrix
            }
            let car_list = acs.alloc(8);
            wr(car_list, 0, car as usize);
            wr(sim, 0x208, car_list as usize);
            wr(sim, 0x210, car_list as usize + 8);
            wr(sim, 0xd8, rng.float(0.0, 1.0));
            wr(sim, 0xdc, rng.float(1.0, 3.0));
            wr(sim, 0x1b0, acs.alloc(0x200) as usize);
            let manager = acs.alloc(0x200);
            wr(sim, 0x188, manager as usize);
            wr(car, 0x130, sim as usize);
            wr(car, 0x1b8, acs.alloc(0xe0) as usize);
            let mut cfg: Vec<f32> = Vec::new();
            // the car: GRAPHICS_OFFSET, GRAPHICS_PITCH_ROTATION (often 0), DRIVEREYES
            put(car, 0x104c, &[rng.float(-0.1, 0.1), rng.float(-0.3, 0.0), rng.float(-0.2, 0.2)], &mut cfg);
            let pitch0 = if rng.next().is_multiple_of(3) { 0.0 } else { rng.float(-0.05, 0.05) };
            put(car, 0x1058, &[pitch0], &mut cfg);
            put(car, 0x218, &[rng.float(-0.4, 0.4), rng.float(0.5, 1.0), rng.float(-0.6, 0.2)], &mut cfg);
            // the drivable views' places (`DrivableCameraDef` of car 0): bumper pos, pitch;
            // bonnet pos, pitch; dash pos; the views' own yaw
            let drivable_def = acs.alloc(0x38);
            put(drivable_def, 0, &[rng.float(-0.2, 0.2), rng.float(0.3, 0.8), rng.float(1.5, 2.5), rng.float(-0.1, 0.1)], &mut cfg);
            put(drivable_def, 0x10, &[rng.float(-0.2, 0.2), rng.float(0.8, 1.3), rng.float(0.0, 1.0), rng.float(-0.1, 0.1)], &mut cfg);
            wr(drivable_def, 0x20, rng.float(-0.1, 0.1));
            put(drivable_def, 0x24, &[rng.float(-0.3, 0.3), rng.float(0.5, 0.9), rng.float(-0.2, 0.4)], &mut cfg);
            put(drivable_def, 0x30, &[if on(4) { rng.float(-0.5, 0.5) } else { 0.0 }], &mut cfg);
            // CameraOnBoard: onBoardFov, farPlane; gForceMix; gForceLagMix; headRotationDegrees,
            // headRotationSpeed; shakeMul; shakeFreq; shakeScale; stillCamera; isWorldAligned;
            // the car's custom pitch and yaw; its look-back point
            onboard = acs.alloc(0x200);
            wr(manager, 0x88, onboard as usize);
            wr(onboard, 8, game as usize);
            wr(onboard, 0x58, camera as usize);
            wr(onboard, 0x70, sim as usize);
            wr(onboard, 0x78, car as usize);
            signal_3d(onboard.add(0x108));
            let fov = [54.0, 56.0, rng.float(20.0, 120.0)][(rng.next() % 3) as usize];
            let far = [1000.0, 15000.0, rng.float(100.0, 20000.0)][(rng.next() % 3) as usize];
            put(onboard, 0x90, &[fov], &mut cfg);
            put(onboard, 0xa8, &[far], &mut cfg);
            let g = if on(1) { [rng.float(0.0, 2.0), rng.float(0.0, 2.0), rng.float(0.0, 2.0)] } else { [0.0; 3] };
            put(onboard, 0x84, &g, &mut cfg);
            put(onboard, 0x94, &[rng.float(0.0, 0.99)], &mut cfg);
            put(onboard, 0x98, &[rng.float(30.0, 90.0)], &mut cfg);
            put(onboard, 0xa0, &[rng.float(1.0, 10.0)], &mut cfg);
            let shake = on(2);
            put(onboard, 0xa4, &[if shake { rng.float(0.0, 2.0) } else { 0.0 }], &mut cfg);
            put(onboard, 0xec, &[rng.float(0.0, 30.0), rng.float(0.0, 30.0), rng.float(0.0, 30.0)], &mut cfg);
            let scale = if shake { [rng.float(0.0, 0.01), rng.float(0.0, 0.01), rng.float(0.0, 0.01)] } else { [0.0; 3] };
            put(onboard, 0xf8, &scale, &mut cfg);
            let still = rng.next().is_multiple_of(4);
            wr(onboard, 0x80, still as u8);
            cfg.push(still as u8 as f32);
            let aligned = on(8) && rng.next().is_multiple_of(2);
            wr(onboard, 0x160, aligned as u8);
            cfg.push(aligned as u8 as f32);
            let custom = vector(acs, onboard.add(0xc8), 8, 1);
            put(custom, 0, &[rng.float(-0.2, 0.2), if on(4) { rng.float(-0.5, 0.5) } else { 0.0 }], &mut cfg);
            let back = vector(acs, onboard.add(0xb0), 12, 1);
            put(back, 0, &[rng.float(-0.3, 0.3), rng.float(0.6, 1.2), rng.float(-1.0, 0.5)], &mut cfg);
            // CameraDrivableManager: chaseCamData (distance, height, pitch) x2; accGXMul,
            // accGZMul, chaseCamFilter; rotationDegrees, rotationSpeed; isWorldAligned; the
            // look-back point
            drivable = acs.alloc(0x200);
            wr(drivable, 8, game as usize);
            wr(drivable, 0x58, view);
            wr(drivable, 0x78, sim as usize);
            wr(drivable, 0x80, camera as usize);
            put(drivable, 0x5c, &[rng.float(2.0, 5.0), rng.float(0.8, 2.5), rng.float(-0.1, 0.1), rng.float(2.0, 6.0), rng.float(0.8, 3.0), rng.float(-0.1, 0.1)], &mut cfg);
            let chase = if on(1) { [0.4 * rng.float(0.0, 2.0), -0.2 * rng.float(0.0, 2.0), rng.float(0.5, 10.0)] } else { [0.4, -0.2, 3.0] };
            put(drivable, 0x11c, &chase, &mut cfg);
            put(drivable, 0x110, &if on(4) { [rng.float(20.0, 90.0), rng.float(0.5, 5.0)] } else { [60.0, 1.0] }, &mut cfg);
            let aligned = on(8) && rng.next().is_multiple_of(2);
            wr(drivable, 0x108, aligned as u8);
            cfg.push(aligned as u8 as f32);
            let back = vector(acs, drivable.add(0xf0), 12, 1);
            put(back, 0, &[rng.float(-0.3, 0.3), rng.float(0.6, 1.2), rng.float(-1.0, 0.5)], &mut cfg);
            let positions = vector(acs, drivable.add(0x88), 0x38, 1);
            std::ptr::copy_nonoverlapping(drivable_def, positions, 0x38);
            // exposures: on board, out board, dash (0: the on board one)
            put(car, 0xfb8, &[rng.float(10.0, 40.0), rng.float(10.0, 40.0), if rng.next().is_multiple_of(2) { 0.0 } else { rng.float(10.0, 40.0) }], &mut cfg);
            // the car's own cameras: count, then matrix, fov, exposure each
            let count = (rng.next() % 4) as usize + 1;
            let car_cameras = vector(acs, car.add(0xfc8), 0x4c, count);
            cfg.push(count as f32);
            for k in 0..count {
                let r = axes(rng.float(-3.2, 3.2), rng.float(-0.5, 0.5), rng.float(-0.3, 0.3));
                let m = [r[0], r[1], r[2], 0.0, r[3], r[4], r[5], 0.0, r[6], r[7], r[8], 0.0, rng.float(-2.0, 2.0), rng.float(-0.5, 1.5), rng.float(-3.0, 3.0), 1.0];
                put(car_cameras, k * 0x4c, &m, &mut cfg);
                put(car_cameras, k * 0x4c + 0x40, &[rng.float(10.0, 90.0), rng.float(10.0, 40.0)], &mut cfg);
            }
            // the shake's randomBlend (SignalGenerator3D +0x20; the ctor's 0.5 from camera_onboard.ini [SHAKE] RANDOM), last on the line
            put(onboard, 0x128, &[[0.0, 0.5, 1.0, rng.float(0.0, 1.0)][(rng.next() % 4) as usize]], &mut cfg);
            // CameraCarManager (an index past the last one goes back to 0)
            cars = acs.alloc(0x100);
            wr(cars, 8, game as usize);
            wr(cars, 0x58, sim as usize);
            wr(cars, 0x60, camera as usize);
            let index = (rng.next() % (count as u64 + 1)) as u32;
            wr(cars, 0x68, index);
            write!(tape, "seg {mode} {view} {index} {features}").unwrap();
            bits(&mut tape, &cfg);
            tape.push('\n');
            yaw = rng.float(-3.2, 3.2);
            pitch = rng.float(-0.3, 0.3);
            roll = if rng.next().is_multiple_of(5) { rng.float(2.0, 3.6) } else { rng.float(-0.3, 0.3) };
            pos = [rng.float(-2000.0, 2000.0), rng.float(-50.0, 300.0), rng.float(-2000.0, 2000.0)];
            actions = 0;
        }
        // the switches: F1 (the drivable views in turn, then the cockpit), F6 (the next car camera)
        let switch = frame == 100 || frame == 200;
        if switch {
            match mode {
                0 => mode = 1,
                1 => {
                    if next_mode(drivable) {
                        mode = 0;
                    }
                }
                _ => next_camera(cars),
            }
        }
        // the car moves on
        yaw += rng.float(-0.05, 0.05);
        pitch = (pitch + rng.float(-0.01, 0.01)).clamp(-0.4, 0.4);
        roll += rng.float(-0.01, 0.01);
        let r = axes(yaw, pitch, roll);
        let speed = if rng.next().is_multiple_of(6) { rng.float(0.0, 0.6) } else { rng.float(0.0, 80.0) };
        for i in 0..3 {
            pos[i] += r[6 + i] * speed * 0.02;
        }
        let body = [r[0], r[1], r[2], 0.0, r[3], r[4], r[5], 0.0, r[6], r[7], r[8], 0.0, pos[0], pos[1], pos[2], 1.0];
        for (i, v) in body.iter().enumerate() {
            wr(car, 0x26c + 4 * i, *v);
        }
        // CarAvatar::update: the 3D matrix, then the body node's matrix is that one
        make_body_matrix(car, car.add(0x26c), car.add(0x224));
        std::ptr::copy_nonoverlapping(car.add(0x224), (rd::<usize>(car, 0x1b8) as *mut u8).add(8), 64);
        // the rear hubs (suspensionMatrix[2], [3]: only their place is read)
        let mut hubs = [0.0f32; 6];
        for (w, side) in [0.75f32, -0.75].iter().enumerate() {
            let back = rng.float(1.2, 1.4);
            let down = rng.float(0.0, 0.05);
            for i in 0..3 {
                hubs[3 * w + i] = pos[i] + r[i] * side - r[6 + i] * back - r[3 + i] * down;
                wr(car, 0x2ac + 64 * (2 + w) + 0x30 + 4 * i, hubs[3 * w + i]);
            }
        }
        let acc = if features & 1 != 0 || rng.next().is_multiple_of(2) { [rng.float(-3.0, 3.0), rng.float(-2.0, 2.0), rng.float(-3.0, 3.0)] } else { [0.0; 3] };
        let velocity = [r[6] * speed, r[7] * speed, r[8] * speed];
        if features & 4 != 0 && rng.next().is_multiple_of(40) {
            actions = [0, 1, 2, 3, 0x200, 0x201][(rng.next() % 6) as usize];
        }
        let dt = match rng.next() % 12 {
            0 => 0.0,
            1 => rng.float(0.05, 0.5),
            _ => rng.float(0.004, 0.034),
        };
        wr(car, 0x268 + 0x270, speed);
        for i in 0..3 {
            wr(car, 0x268 + 0x64c + 4 * i, acc[i]);
            wr(car, 0x268 + 0x274 + 4 * i, velocity[i]);
        }
        wr(car, 0x268 + 0x7d4, actions);
        match mode {
            0 => onboard_update(onboard, dt),
            1 => drivable_update(drivable, dt),
            _ => cars_update(cars, dt),
        }
        write!(tape, "f {} {actions:x}", switch as u8).unwrap();
        bits(&mut tape, &body);
        bits(&mut tape, &hubs);
        bits(&mut tape, &[speed]);
        bits(&mut tape, &acc);
        bits(&mut tape, &velocity);
        bits(&mut tape, &[dt]);
        tape.push_str(" ->");
        let matrix: Vec<f32> = (0..16).map(|i| rd(camera, 0x0c + 4 * i)).collect();
        bits(&mut tape, &matrix);
        bits(&mut tape, &[rd(camera, 0x08), rd(camera, 0x70), rd(camera, 0x74), rd(camera, 0x19c)]);
        tape.push('\n');
    }
    Ok(tape)
}

const VA_CAMERA_MANAGER: usize = 0x1_401f_45f0;
const VA_DISTANCE_RANGES: usize = 0x1_401f_48b0;
const VA_SPLINE: usize = 0x1_401e_d180;
const VA_SPLINE_LOAD_CSV: usize = 0x1_401e_e3c0;
const VA_TRACK_UPDATE: usize = 0x1_400c_be30;
const VA_TRACK_NEXT_SET: usize = 0x1_400c_bc90;
const VA_FROM_AXIS_ANGLE: usize = 0x1_4005_71a0;
const VA_SPLINE_APPLY_MATRIX: usize = 0x1_401e_d730;

/// The body node's `getWorldMatrix` (vtable +0x40): the matrix the tape put at node + 8.
extern "C" fn world_matrix(node: *const u8, out: *mut u8) -> *mut u8 {
    unsafe { std::ptr::copy_nonoverlapping(node.add(8), out, 64) };
    out
}

/// A `std::wstring` of the game's, as text.
unsafe fn read_wstring(string: *const u8) -> String {
    let len: usize = rd(string, 0x10);
    let at = if rd::<usize>(string, 0x18) < 8 { string.cast::<u16>() } else { rd::<usize>(string, 0) as *const u16 };
    String::from_utf16_lossy(std::slice::from_raw_parts(at, len))
}

/// The folders (relative to `ac`) whose `data/cameras.ini` the game reads as a track's TV
/// cameras: every track and every layout.
fn camera_folders(ac: &std::path::Path) -> Vec<String> {
    let names = |dir: &std::path::Path| {
        let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    };
    let mut folders = Vec::new();
    for track in names(&ac.join("content/tracks")) {
        let base = format!("content/tracks/{track}");
        if ac.join(&base).join("data/cameras.ini").is_file() {
            folders.push(base.clone());
        }
        for layout in names(&ac.join(&base)) {
            if layout != "data" && ac.join(&base).join(&layout).join("data/cameras.ini").is_file() {
                folders.push(format!("{base}/{layout}"));
            }
        }
    }
    folders
}

/// Task 46: F3, the track's TV cameras. For every track and layout of the AC folder `ac`, the
/// game's own `CameraManager` on each of its `cameras*.ini` (as `CameraTrack::loadSet` does:
/// `cameras.ini`, `cameras_1.ini` ... to the first missing one, the cameras' paths through
/// `Spline::loadFromCSV`) and `initCamerasDistanceRanges` on a made-up AI line around the
/// cameras; then the game's `CameraTrack::update` on a made-up car going along that line,
/// with F3 again (`nextSet`) now and then, the pause's still camera and a replay's speed.
///
/// Lines: `group <folder> <sets>`, `line <points xyz ...>`, `cfg <upward offset> <wobbling
/// speed> <wobbling strength>`, per set `set <cameras> <SET_NAME>`, per camera `cam <name
/// length> <fields ...> <dof manual> <fixed> <path name length> | <path points xyz ...>`, per
/// frame `f <next set 0|1> <still 0|1> <in replay 0|1> <replay speed> <body 16> <steer 3>
/// <lap place> <speed> <dt> -> <matrix 16> <fov> <near> <far> <min exposure> <max exposure>
/// <dof factor> <dof focus> <dof range> <+0x19c>`.
pub unsafe fn track_micro(acs: &Acs, ac: &std::path::Path, seed: u32, steps: usize) -> Result<String, String> {
    acs.patch(acs.va(VA_SET_SHADOW_MAPS_SPLITS), &[0xc3]);
    acs.patch(acs.va(VA_SET_LISTENER), &[0xc3]);
    let manager_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAMERA_MANAGER));
    let ranges: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_DISTANCE_RANGES));
    let spline_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SPLINE));
    let load_csv: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_SPLINE_LOAD_CSV));
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_TRACK_UPDATE));
    let next_set: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_NEXT_SET));
    let from_axis_angle: extern "C" fn(*mut u8, *const f32, f32) -> *mut u8 = std::mem::transmute(acs.va(VA_FROM_AXIS_ANGLE));
    let apply_matrix: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_SPLINE_APPLY_MATRIX));
    let folders = camera_folders(ac);
    if folders.is_empty() {
        return Err(format!("{}: no track with data/cameras.ini", ac.display()));
    }
    let frames = (steps / folders.len()).max(50);
    let mut tape = String::new();
    let mut rng = Rng(seed as u64 ^ 0x7ac3);
    let camera_vtable = acs.alloc(0x200);
    for slot in 0..64 {
        wr(camera_vtable, slot * 8, acs.va(VA_SET_LISTENER));
    }
    let node_vtable = acs.alloc(0x100);
    wr(node_vtable, 0x40, world_matrix as *const () as usize);
    let cams = |m: *mut u8| (rd::<usize>(m, 8), (rd::<usize>(m, 0x10) - rd::<usize>(m, 8)) / 0xf8);
    for folder in &folders {
        // the sets, as `loadSet` finds them
        let data = ac.join(folder).join("data");
        let mut managers = Vec::new();
        for n in 0.. {
            let file = data.join(if n == 0 { "cameras.ini".to_string() } else { format!("cameras_{n}.ini") });
            if !file.is_file() {
                break;
            }
            let manager = acs.alloc(0x48);
            manager_ctor(manager, wstring(acs, &file.to_string_lossy()));
            managers.push(manager);
        }
        writeln!(tape, "group {folder} {}", managers.len()).unwrap();
        // a made-up AI line around the first set's cameras
        let (first, count) = cams(managers[0]);
        let n = 100 + (rng.next() % 3000) as usize;
        let line = acs.alloc(0x28);
        spline_ctor(line);
        let points = vector(acs, line.add(8), 0x14, n);
        let mut xyz = Vec::with_capacity(3 * n);
        for i in 0..n {
            let p = if count == 0 {
                [0.0; 3]
            } else {
                let c = (first + 0xf8 * (rng.next() as usize % count)) as *const u8;
                [rd::<f32>(c, 0x58), rd::<f32>(c, 0x5c), rd::<f32>(c, 0x60)]
            };
            let q = [p[0] + rng.float(-150.0, 150.0), p[1] + rng.float(-8.0, 2.0), p[2] + rng.float(-150.0, 150.0)];
            put(points, 0x14 * i, &q, &mut xyz);
        }
        tape.push_str("line");
        bits(&mut tape, &xyz);
        tape.push('\n');
        // the settings of system/cfg/camera_track.ini: AC's own, or made up
        let cfg = if rng.next().is_multiple_of(2) { [0.0, 0.001, 0.08] } else { [rng.float(-1.0, 2.0), rng.float(0.0, 0.01), rng.float(0.0, 0.5)] };
        tape.push_str("cfg");
        bits(&mut tape, &cfg);
        tape.push('\n');
        for &manager in &managers {
            ranges(manager, line);
            let (first, count) = cams(manager);
            writeln!(tape, "set {count} {}", read_wstring(manager.add(0x20))).unwrap();
            for k in 0..count {
                let c = (first + 0xf8 * k) as *mut u8;
                let name = read_wstring(c.add(8));
                let path_name = read_wstring(c.add(0xb8));
                if !path_name.is_empty() {
                    let file = data.join(&path_name);
                    if file.is_file() {
                        let spline = acs.alloc(0x28);
                        spline_ctor(spline);
                        load_csv(spline, wstring(acs, &file.to_string_lossy()));
                        wr(c, 0xe0, spline as usize);
                        // `ucomiss; je`: a turn that is not a number is no turn either
                        let turn: f32 = rd(c, 0xd8);
                        if turn != 0.0 && !turn.is_nan() {
                            let m = acs.alloc(64);
                            let axis = [0.0f32, 1.0, 0.0];
                            from_axis_angle(m, axis.as_ptr(), turn * f32::from_bits(0x3c8e_f998));
                            apply_matrix(spline, m);
                        }
                    }
                }
                write!(tape, "cam {}", name.encode_utf16().count()).unwrap();
                let fields = [0x68, 0x6c, 0x70, 0x74, 0x7c, 0x80, 0x84, 0x8c, 0x90, 0x94, 0x98, 0x9c, 0xa0, 0xa4, 0xac, 0xb0, 0xd8, 0xe8, 0xf0];
                let floats: Vec<f32> = (0..16).map(|i| rd(c, 0x28 + 4 * i)).chain(fields.map(|o| rd::<f32>(c, o))).collect();
                bits(&mut tape, &floats);
                write!(tape, " {} {} {} |", rd::<u8>(c, 0xa8), rd::<u8>(c, 0xf4), path_name.encode_utf16().count()).unwrap();
                let spline: usize = rd(c, 0xe0);
                if spline != 0 {
                    let (at, end) = (rd::<usize>(spline as *const u8, 8), rd::<usize>(spline as *const u8, 0x10));
                    let path: Vec<f32> = (0..(end - at) / 0x14).flat_map(|i| (0..3).map(move |j| *((at + 0x14 * i + 4 * j) as *const f32))).collect();
                    bits(&mut tape, &path);
                }
                tape.push('\n');
            }
        }
        // CameraTrack, its sim, the focused car and the scene camera
        let sim = acs.alloc(0x400);
        let car = acs.alloc(0x1200);
        let car_list = acs.alloc(8);
        wr(car_list, 0, car as usize);
        wr(sim, 0x208, car_list as usize);
        wr(sim, 0x210, car_list as usize + 8);
        let replay = acs.alloc(0x200);
        wr(sim, 0x1b0, replay as usize);
        let node = acs.alloc(0x100);
        wr(node, 0, node_vtable as usize);
        wr(car, 0x1b8, node as usize);
        let camera = acs.alloc(0x800);
        wr(camera, 0, camera_vtable as usize);
        for i in 0..4 {
            wr(camera, 0x0c + 20 * i, 1.0f32);
        }
        wr(camera, 0x50 + 0x18, 7usize);
        let track = acs.alloc(0x200);
        wr(track, 8, acs.alloc(0x400) as usize);
        wr(track, 0x90, sim as usize);
        wr(track, 0x98, camera as usize);
        let sets = vector(acs, track.add(0x78), 8, managers.len());
        for (i, m) in managers.iter().enumerate() {
            wr(sets, 8 * i, *m as usize);
        }
        wr(track, 0xb4, cfg[0]);
        wr(track, 0xb8, cfg[1]);
        wr(track, 0xbc, cfg[2]);
        let (mut yaw, mut k) = (rng.float(-3.2, 3.2), rng.next() as usize % n);
        let (mut still, mut in_replay, mut replay_speed) = (false, false, 1.0f32);
        for frame in 0..frames {
            if frame % 100 == 0 {
                still = rng.next().is_multiple_of(6);
                in_replay = rng.next().is_multiple_of(5);
                replay_speed = [1.0, 0.5, 2.0, 0.25, 4.0, 0.0, rng.float(0.0, 4.0)][(rng.next() % 7) as usize];
            }
            let next = frame != 0 && rng.next().is_multiple_of(150);
            if next {
                next_set(track, acs.alloc(32));
            }
            wr(track, 0x70, still as u8);
            wr(replay, 0x148, in_replay as u8);
            wr(replay, 0x130, replay_speed);
            k = (k + (rng.next() % (n as u64 / 100 + 2)) as usize) % n;
            let place = match rng.next() % 40 {
                0 => rng.float(-0.2, 1.2),
                1 => rng.float(0.0, 1.0),
                _ => k as f32 / n as f32 + rng.float(-0.002, 0.002),
            };
            yaw += rng.float(-0.05, 0.05);
            let r = axes(yaw, rng.float(-0.1, 0.1), rng.float(-0.1, 0.1));
            let p = [xyz[3 * k] + rng.float(-3.0, 3.0), xyz[3 * k + 1] + rng.float(0.0, 2.0), xyz[3 * k + 2] + rng.float(-3.0, 3.0)];
            let body = [r[0], r[1], r[2], 0.0, r[3], r[4], r[5], 0.0, r[6], r[7], r[8], 0.0, p[0], p[1], p[2], 1.0];
            for (i, v) in body.iter().enumerate() {
                wr(node, 8 + 4 * i, *v);
            }
            let steer = [rng.float(-0.4, 0.4), rng.float(0.5, 1.0), rng.float(-0.6, 0.6)];
            for (i, v) in steer.iter().enumerate() {
                wr(car, 0x200 + 4 * i, *v);
            }
            let speed = if rng.next().is_multiple_of(6) { rng.float(0.0, 0.6) } else { rng.float(0.0, 80.0) };
            wr(car, 0x984, place);
            wr(car, 0x4d8, speed);
            let dt = match rng.next() % 12 {
                0 => 0.0,
                1 => rng.float(0.05, 0.5),
                _ => rng.float(0.004, 0.034),
            };
            update(track, dt);
            write!(tape, "f {} {} {}", next as u8, still as u8, in_replay as u8).unwrap();
            bits(&mut tape, &[replay_speed]);
            bits(&mut tape, &body);
            bits(&mut tape, &steer);
            bits(&mut tape, &[place, speed, dt]);
            tape.push_str(" ->");
            let matrix: Vec<f32> = (0..16).map(|i| rd(camera, 0x0c + 4 * i)).collect();
            bits(&mut tape, &matrix);
            bits(&mut tape, &[0x08, 0x70, 0x74, 0x1a0, 0x1a4, 0x1b0, 0x1b4, 0x1b8, 0x19c].map(|o| rd::<f32>(camera, o)));
            tape.push('\n');
        }
    }
    Ok(tape)
}
