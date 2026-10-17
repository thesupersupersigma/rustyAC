// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 37: the keyboard's steering look-ahead. The game's own `KeyboardCarControl::stepSteer`
//! 0x140111650 and `getKeyboardSteering` 0x14010faa0 (with `getPointAtDistance`,
//! `getCarPositionOnSpline`, `steeringMovement` under them) on a real game ODE body that is
//! put somewhere new every call, with a track's AI line loaded by the game's `AISpline::load`
//! or with none, written to a tape: what went in and what came out. The keys come from a
//! stand-in `GetAsyncKeyState`. The port lives in `rustyac-game`, which this crate does not
//! build; its example `keyboard_steering_check` plays the tape through the port.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicI32, Ordering};

use super::*;

const VA_STEP_STEER: usize = 0x1_4011_1650;
const VA_GET_KEYBOARD_STEERING: usize = 0x1_4010_faa0;
const VA_AI_SPLINE_CTOR: usize = 0x1_402a_4d70; // AISpline::AISpline()
const VA_AI_SPLINE_LOAD: usize = 0x1_402a_8390; // AISpline::load(const std::wstring&)
const VA_CORE_CREATE_RIGID_BODY: usize = 0x1_402c_c3b0; // IRigidBody* PhysicsCore::createRigidBody()
const VA_RB_SET_POSITION: usize = 0x1_402c_e9e0;
const VA_RB_SET_ROTATION: usize = 0x1_402c_ea00;
const VA_RB_GET_WORLD_MATRIX: usize = 0x1_402c_e6c0;
const VA_SPLINE_POINTS_COUNT: usize = 0x1_401e_e880;
const VA_SPLINE_POINT_AT: usize = 0x1_401e_e820; // vec3f Spline::pointAt(unsigned) const

// KeyboardCarControl
const K_SIZE: usize = 0x120;
const K_ENABLED: usize = 0x13;
const K_MIN_STEERING: usize = 0x18;
const K_STEER_SPEED: usize = 0x1c;
const K_OPPOSITE: usize = 0x20;
const K_STEER_GAIN: usize = 0x24;
const K_RESET: usize = 0x28;
const K_LOOK_AHEAD: usize = 0x2c;
const K_DECELERATOR: usize = 0x34;
const K_SPEED_DECEL: usize = 0x38;
const K_TURN_DECEL: usize = 0x3c;
const K_INT_STEER: usize = 0x44;
const K_GAS_PEDAL_SPEED: usize = 0x60;
const K_UNDERSTEER: usize = 0x64;
const K_SPLINE_POS: usize = 0x68;
const K_OLD_STEER: usize = 0x6c;
const K_KEY_RIGHT: usize = 0x94;
const K_KEY_LEFT: usize = 0x98;
const K_AI_SPLINE: usize = 0x110;
const K_CAR: usize = 0x118;
// AISplineRecorder: bestLapSpline; AISpline: its InterpolatingSpline
const REC_BEST_LAP: usize = 0xf0;
const AI_SPLINE: usize = 0x50;

/// `Car::getSpeed` 0x140272160: the car's speed in m/s.
const CAR_SPEED: usize = 0x3e90;

const KEY_LEFT: i32 = 0x25;
const KEY_RIGHT: i32 = 0x27;

static KEYS: AtomicI32 = AtomicI32::new(0);

/// `GetAsyncKeyState`: bit 0 of `KEYS` is the left arrow, bit 1 the right one.
pub extern "system" fn get_async_key_state(key: i32) -> i16 {
    let keys = KEYS.load(Ordering::Relaxed);
    let down = (key == KEY_LEFT && keys & 1 != 0) || (key == KEY_RIGHT && keys & 2 != 0);
    if down {
        0x8000u16 as i16
    } else {
        0
    }
}

/// The imports the oracle answers itself.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize)]
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
}

/// The settings a run starts from: STEERING_SPEED, STEERING_OPPOSITE_DIRECTION_SPEED,
/// STEER_RESET_SPEED, STEER_GAIN, LOOKAHEAD_POINTS (AC's own values first).
const SETTINGS: [[f32; 5]; 6] = [
    [1.1, 2.0, 1.5, 0.18, 8.0],
    [1.75, 2.5, 1.8, 0.18, 8.0],
    [1.0, 1.0, 1.0, 0.5, 20.0],
    [2.5, 3.0, 2.0, 0.05, 2.0],
    [1.75, 2.5, 1.8, 0.0, 0.0],
    [0.8, 1.6, 1.2, 1.0, 50.0],
];

/// `car_oracle steer-micro`: the tape. `line`: a `fast_lane.ai` in the oracle's own folder,
/// or none. One line per call; floats as hex bits.
pub unsafe fn micro(acs: &Acs, seed: u32, steps: usize, line: Option<&str>) -> Result<String, String> {
    let mut tape = String::new();
    let mut rng = Rng(seed as u64 ^ 0x5157_3737);
    // a body of the game's own physics core
    let engine = acs.alloc(PE_SIZE);
    let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PHYSICS_ENGINE_CTOR));
    ctor(engine);
    let core: *mut u8 = rd(engine, PE_CORE);
    let create: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CORE_CREATE_RIGID_BODY));
    let body = create(core);
    let set_rotation: extern "C" fn(*mut u8, *const [f32; 16]) = std::mem::transmute(acs.va(VA_RB_SET_ROTATION));
    let set_position: extern "C" fn(*mut u8, *const [f32; 3]) = std::mem::transmute(acs.va(VA_RB_SET_POSITION));
    let get_world_matrix: extern "C" fn(*mut u8, *mut [f32; 16], f32) -> *mut [f32; 16] = std::mem::transmute(acs.va(VA_RB_GET_WORLD_MATRIX));
    // the car: its body and four tyres (stepSteer reads the front tyres' axes, and uses nothing of them)
    let car = acs.alloc((CAR_TYRES + 4 * TYRE_SIZE).max(CAR_SPEED + 4) + 0x100);
    wr(car, CAR_BODY, body);
    // the track's AI spline recorder: only its best lap's line is read
    let recorder = acs.alloc(REC_BEST_LAP + 0x200);
    let spline_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_AI_SPLINE_CTOR));
    spline_ctor(recorder.add(REC_BEST_LAP));
    if let Some(file) = line {
        let load: extern "C" fn(*mut u8, *mut u8) -> u8 = std::mem::transmute(acs.va(VA_AI_SPLINE_LOAD));
        load(recorder.add(REC_BEST_LAP), wstring(acs, file));
    }
    let spline = recorder.add(REC_BEST_LAP + AI_SPLINE);
    let points_count: extern "C" fn(*mut u8) -> u32 = std::mem::transmute(acs.va(VA_SPLINE_POINTS_COUNT));
    let point_at: extern "C" fn(*mut u8, *mut [f32; 3], u32) -> *mut [f32; 3] = std::mem::transmute(acs.va(VA_SPLINE_POINT_AT));
    let count = points_count(spline);
    writeln!(tape, "line {count}").unwrap();
    let mut points = Vec::new();
    for i in 0..count {
        let mut p = [0.0f32; 3];
        point_at(spline, &mut p, i);
        points.push(p);
        writeln!(tape, "point {:08x} {:08x} {:08x}", p[0].to_bits(), p[1].to_bits(), p[2].to_bits()).unwrap();
    }
    let step_steer: extern "C" fn(*mut u8, f32) -> f32 = std::mem::transmute(acs.va(VA_STEP_STEER));
    let get_keyboard_steering: extern "C" fn(*mut u8, f32, *const f32) -> f32 = std::mem::transmute(acs.va(VA_GET_KEYBOARD_STEERING));
    let mut k = std::ptr::null_mut();
    let mut place = 0usize;
    let mut heading = 0.0f32;
    for step in 0..steps {
        // every 500 calls a new keyboard: the constructor's values and one of the settings
        if step % 500 == 0 {
            let s = SETTINGS[(step / 500) % SETTINGS.len()];
            k = acs.alloc(K_SIZE);
            wr(k, K_ENABLED, 1u8);
            wr(k, K_MIN_STEERING, 0.0f32);
            wr(k, K_STEER_SPEED, s[0]);
            wr(k, K_OPPOSITE, s[1]);
            wr(k, K_RESET, s[2]);
            wr(k, K_STEER_GAIN, s[3]);
            wr(k, K_LOOK_AHEAD, s[4]);
            wr(k, K_DECELERATOR, 1.0f32);
            wr(k, K_SPEED_DECEL, 1.0f32);
            wr(k, K_TURN_DECEL, 0.0f32);
            wr(k, K_GAS_PEDAL_SPEED, 4.0f32);
            wr(k, K_UNDERSTEER, 0.0f32);
            wr(k, K_SPLINE_POS, -1i32);
            wr(k, K_OLD_STEER, 0.0f32);
            wr(k, K_KEY_RIGHT, KEY_RIGHT);
            wr(k, K_KEY_LEFT, KEY_LEFT);
            wr(k, K_AI_SPLINE, recorder);
            wr(k, K_CAR, car);
            writeln!(tape, "keyboard {:08x} {:08x} {:08x} {:08x} {:08x}", s[0].to_bits(), s[1].to_bits(), s[2].to_bits(), s[3].to_bits(), s[4].to_bits()).unwrap();
        }
        // where the car is: near the line (or the origin), now and then somewhere else
        if !points.is_empty() && !rng.next().is_multiple_of(300) {
            place = (place + (rng.next() % 4) as usize) % points.len();
            let p = points[place];
            heading += rng.float(-0.2, 0.2);
            let pos = [p[0] + rng.float(-12.0, 12.0), p[1] + rng.float(-1.0, 3.0), p[2] + rng.float(-12.0, 12.0)];
            pose(set_rotation, set_position, body, pos, heading, rng.float(-0.1, 0.1));
        } else {
            let far = if rng.next().is_multiple_of(3) { 3000.0 } else { 80.0 };
            heading = rng.float(-4.0, 4.0);
            let pos = [rng.float(-far, far), rng.float(-5.0, 50.0), rng.float(-far, far)];
            pose(set_rotation, set_position, body, pos, heading, rng.float(-0.4, 0.4));
        }
        let speed = match rng.next() % 6 {
            0 => 0.0,
            1 => rng.float(0.0, 5.0),
            _ => rng.float(0.0, 90.0),
        };
        let dt = if rng.next().is_multiple_of(50) { 0.0166 } else { 0.003 };
        // the arrows: held for a while, both now and then
        if step % 37 == 0 {
            KEYS.store((rng.next() % 4) as i32, Ordering::Relaxed);
        }
        let keys = KEYS.load(Ordering::Relaxed);
        if rng.next().is_multiple_of(64) {
            wr(k, K_OLD_STEER, rng.float(-1.0, 1.0));
        }
        let mut m = [0.0f32; 16];
        get_world_matrix(body, &mut m, 0.0);
        let int_steer: f32 = rd(k, K_INT_STEER);
        let old_steer: f32 = rd(k, K_OLD_STEER);
        let wish = step_steer(k, dt);
        // CarControlsInput: steerLock, speed
        let input = [rng.float(180.0, 900.0), speed];
        // steeringMovement reads the car's own speed: the game's input has that speed too
        wr(car, CAR_SPEED, speed);
        let r = get_keyboard_steering(k, dt, input.as_ptr());
        write!(tape, "step").unwrap();
        for v in m.iter().chain(&[speed, dt, int_steer, old_steer]) {
            write!(tape, " {:08x}", v.to_bits()).unwrap();
        }
        let after = [rd::<f32>(k, K_INT_STEER), rd(k, K_MIN_STEERING), rd(k, K_DECELERATOR), rd(k, K_SPEED_DECEL), rd(k, K_TURN_DECEL)];
        write!(tape, " {keys} -> {:08x} {:08x}", wish.to_bits(), r.to_bits()).unwrap();
        for v in after {
            write!(tape, " {:08x}", v.to_bits()).unwrap();
        }
        writeln!(tape, " {}", rd::<i32>(k, K_SPLINE_POS)).unwrap();
    }
    Ok(tape)
}

/// `RigidBodyODE::setRotation` (yawed by `heading`, rolled by `roll`), then `setPosition`.
unsafe fn pose(set_rotation: extern "C" fn(*mut u8, *const [f32; 16]), set_position: extern "C" fn(*mut u8, *const [f32; 3]), body: *mut u8, pos: [f32; 3], heading: f32, roll: f32) {
    let (s, c) = heading.sin_cos();
    let (rs, rc) = roll.sin_cos();
    // rows: the body's left, up and forward axes
    let left = [c * rc, rs, -s * rc];
    let up = [-c * rs, rc, s * rs];
    let fwd = [s, 0.0, c];
    let m = [left[0], left[1], left[2], 0.0, up[0], up[1], up[2], 0.0, fwd[0], fwd[1], fwd[2], 0.0, 0.0, 0.0, 0.0, 1.0];
    set_rotation(body, &m);
    set_position(body, &pos);
}
