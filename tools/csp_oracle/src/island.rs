// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5d part 2: the island recorder. `CSP_ORACLE_BREAK=16c24b0i,16c6d00w` with
//! `CSP_ORACLE_ISLANDS=<file>`: at each `dxStepIsland` (+0x16c24b0) entry the island's world, bodies
//! and joints are copied raw (record 1); at the return of the world step that called it
//! (`dWorldStep` +0x16c6d00, whose return site gets a break of its own) the same bodies and joints
//! again (record 2). Every value is a little-endian u64 word:
//! record 1: 1, step, h bits, nb, nj, world ptr, world 0x200 bytes, nb × (ptr, 0x300 bytes),
//! nj × (ptr, 0x200 bytes, feedback 0x80 bytes or zeros); record 2: 2, step, nb, nj, nb × 0x300
//! bytes, nj × (0x200 bytes, feedback 0x80 bytes or zeros).

use std::sync::Mutex;

pub(crate) const WORLD: usize = 0x200;
pub(crate) const BODY: usize = 0x300;
pub(crate) const JOINT: usize = 0x200;
pub(crate) const FEEDBACK: usize = 0x80;

static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// Islands entered since the world step began: (bodies, joints).
static PENDING: Mutex<Vec<(Vec<usize>, Vec<usize>)>> = Mutex::new(Vec::new());

unsafe fn raw(out: &mut Vec<u8>, address: usize, n: usize) {
    out.extend_from_slice(std::slice::from_raw_parts(address as *const u8, n));
}

unsafe fn joint(out: &mut Vec<u8>, j: usize) {
    raw(out, j, JOINT);
    let fb = *((j + 0x68) as *const usize);
    if fb != 0 {
        raw(out, fb, FEEDBACK);
    } else {
        out.extend_from_slice(&[0; FEEDBACK]);
    }
}

fn word(out: &mut Vec<u8>, x: u64) {
    out.extend_from_slice(&x.to_le_bytes());
}

/// `dxStepIsland(callContext)`: rcx is the context (world +0, h +8, bodies +0x20, joints +0x28,
/// nb +0x30, nj +0x34).
pub(crate) unsafe fn enter(ctx: usize, step: usize) {
    let r = |o: usize| *((ctx + o) as *const usize);
    let (world, nb, nj) = (r(0), r(0x30) as u32 as usize, r(0x34) as u32 as usize);
    let bodies: Vec<usize> = (0..nb).map(|i| *((r(0x20) + 8 * i) as *const usize)).collect();
    let joints: Vec<usize> = (0..nj).map(|i| *((r(0x28) + 8 * i) as *const usize)).collect();
    let mut out = OUT.lock().unwrap();
    for x in [1, step as u64, r(8) as u64, nb as u64, nj as u64, world as u64] {
        word(&mut out, x);
    }
    raw(&mut out, world, WORLD);
    for &b in &bodies {
        word(&mut out, b as u64);
        raw(&mut out, b, BODY);
    }
    for &j in &joints {
        word(&mut out, j as u64);
        joint(&mut out, j);
    }
    PENDING.lock().unwrap().push((bodies, joints));
}

/// The world step returned: every island it stepped, after.
pub(crate) unsafe fn leave(step: usize) {
    let mut out = OUT.lock().unwrap();
    for (bodies, joints) in PENDING.lock().unwrap().drain(..) {
        for x in [2, step as u64, bodies.len() as u64, joints.len() as u64] {
            word(&mut out, x);
        }
        bodies.iter().for_each(|&b| raw(&mut out, b, BODY));
        joints.iter().for_each(|&j| joint(&mut out, j));
    }
}

pub(crate) fn save() {
    if let Ok(path) = std::env::var("CSP_ORACLE_ISLANDS") {
        std::fs::write(&path, &*OUT.lock().unwrap()).expect("the island file can be written");
    }
}
