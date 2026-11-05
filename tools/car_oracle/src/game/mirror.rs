// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47d: the game's `VirtualMirrorRenderer` (constructor 0x1401d1ab0, `update` 0x1401d2190)
//! with the server's FORCE_VIRTUAL_MIRROR (Sim+0x1e3) and a headset (Sim+0x1e1) on and off: F11
//! from a stand-in `GetAsyncKeyState`, every system message written down.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::race_ini::{text, write_wstring};
use super::*;

const VA_CTOR: usize = 0x1_401d_1ab0;
const VA_UPDATE: usize = 0x1_401d_2190;
const VA_ADD_MESSAGE: usize = 0x1_401b_61c0;
const VA_AC_TRANSLATE: usize = 0x1_4024_0cf0;

static F11: AtomicBool = AtomicBool::new(false);
static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());

extern "system" fn get_async_key_state(key: i32) -> i16 {
    if key == 0x7a && F11.load(Ordering::Relaxed) {
        0x8000u16 as i16
    } else {
        0
    }
}

extern "C" fn add_message(_messages: *mut u8, message: *const u8, description: *const u8, kind: i32) {
    let line = unsafe { format!("Message({} | {} | {kind})", text(message), text(description)) };
    SAID.lock().unwrap().push(line);
}

/// `acTranslate` with no locale table loaded (the port's `ac_translate` likewise): the text itself.
extern "C" fn ac_translate(out: *mut u8, text_in: *const u8) -> *mut u8 {
    unsafe { write_wstring(out, &text(text_in)) };
    out
}

extern "C" fn add_child(_root: *mut u8, _node: *mut u8) {}

pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![("user32.dll", "GetAsyncKeyState", get_async_key_state as *const () as usize)]
}

/// The F11 pattern of a case: a key held for a few frames, let go, pressed again.
pub fn keys(case: u32, seed: u32) -> Vec<bool> {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ case.wrapping_mul(0x85eb_ca6b);
    (0..48)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.is_multiple_of(3)
        })
        .collect()
}

/// Lines: `case <n> vr <0|1> forced <0|1> start <0|1>`, then per frame `<i> key <0|1> active
/// <0|1>` and the messages of that frame, `end`. The renderer is built by the game's
/// constructor on a forced server without a headset (so it reads no gameplay.ini); the case's
/// flags and start state are then set.
pub unsafe fn micro(acs: &Acs, seed: u32) -> Result<String, String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    stub(VA_AC_TRANSLATE, ac_translate as *const () as usize);
    let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CTOR));
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE));
    let mut tape = String::new();
    for case in 0..8u32 {
        let (vr, forced, start) = (case & 1, case >> 1 & 1, case >> 2 & 1);
        let game = acs.alloc(0x400);
        let sim = acs.alloc(0x2d0);
        let root = acs.alloc(0x10);
        let vtable = Box::leak(vec![add_child as *const () as usize; 4].into_boxed_slice());
        wr(root, 0, vtable.as_ptr() as usize);
        wr(sim, 8, game as usize);
        wr(sim, 0x120, root as usize);
        wr(sim, 0x1e3, 1u8);
        let mirror = ctor(acs.alloc(0x78), sim);
        let node: *mut u8 = rd(mirror, 0x58);
        if rd::<u8>(node, 0xd8) != 1 {
            return Err("the constructor did not turn a forced mirror on".into());
        }
        wr(sim, 0x1e1, vr as u8);
        wr(sim, 0x1e3, forced as u8);
        wr(node, 0xd8, start as u8);
        writeln!(tape, "case {case} vr {vr} forced {forced} start {start}").unwrap();
        for (i, key) in keys(case, seed).into_iter().enumerate() {
            F11.store(key, Ordering::Relaxed);
            update(mirror, 0.016);
            writeln!(tape, "{i} key {} active {}", key as u8, rd::<u8>(node, 0xd8)).unwrap();
            for line in std::mem::take(&mut *SAID.lock().unwrap()) {
                writeln!(tape, "{line}").unwrap();
            }
        }
        tape.push_str("end\n");
    }
    Ok(tape)
}
