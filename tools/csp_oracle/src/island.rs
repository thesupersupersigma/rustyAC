// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5d part 2: the island recorder. `CSP_ORACLE_BREAK=16c24b0i,16c6d00w` with
//! `CSP_ORACLE_ISLANDS=<file>`: at each `dxStepIsland` (+0x16c24b0) entry the island's world, bodies
//! and joints are copied raw (record 1); at the return of the world step that called it
//! (`dWorldStep` +0x16c6d00, whose return site gets a break of its own) the same bodies and joints
//! again (record 2). Every value is a little-endian u64 word:
//! record 1: 1, step, h bits, nb, nj, world ptr, world 0x200 bytes, nb × (ptr, 0x300 bytes),
//! nj × (ptr, 0x200 bytes, feedback 0x80 bytes or zeros); record 2: 2, step, nb, nj, nb × 0x300
//! bytes, nj × (0x200 bytes, feedback 0x80 bytes or zeros).

use rustyac_ode::csp_contact::Contact;
use rustyac_ode::csp_step::{step_island_probed, Joint, JointKind, WorldParams};
use rustyac_ode::mass::Mass;
use rustyac_ode::world::{AutoDisable, Body, Damping};
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
        // CSP_ORACLE_FEEDBACK=1: a zeroed dJointFeedback for every joint without one (CSP's cars set none)
        let fb = (j + 0x68) as *mut usize;
        if *fb == 0 && std::env::var_os("CSP_ORACLE_FEEDBACK").is_some() {
            *fb = Box::leak(Box::new([0u8; FEEDBACK])).as_mut_ptr() as usize;
        }
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

/// `dxSolveLCP(memarena, m, A, pairsbx, outer_w, nub, pairslh, findex)` entry (+0x16ffe10, kind
/// 'l'): record 3 = 3, m, nub, A (m × pad(m)), pairsbx (2m), pairslh (2m), findex (m words), then
/// the newest island's bodies (0x300 bytes each).
pub(crate) unsafe fn lcp(m: usize, a: usize, bx: usize, rsp: usize) {
    let m = m as u32 as usize;
    let st = |k: usize| *((rsp + 8 * k) as *const usize);
    let (nub, lh, findex) = (st(6) as u32 as u64, st(7), st(8));
    let nskip = if m <= 1 { m } else { (m + 3) & !3 };
    let mut out = OUT.lock().unwrap();
    for x in [3, m as u64, nub] {
        word(&mut out, x);
    }
    raw(&mut out, a, 8 * m * nskip);
    raw(&mut out, bx, 16 * m);
    raw(&mut out, lh, 16 * m);
    for i in 0..m {
        word(&mut out, if findex == 0 { u64::MAX } else { *((findex + 4 * i) as *const i32) as i64 as u64 });
    }
    if let Some((bodies, _)) = PENDING.lock().unwrap().last() {
        bodies.iter().for_each(|&b| raw(&mut out, b, BODY));
    }
}

pub(crate) fn save() {
    if let Ok(path) = std::env::var("CSP_ORACLE_ISLANDS") {
        std::fs::write(&path, &*OUT.lock().unwrap()).expect("the island file can be written");
    }
}

fn f(w: &[u64], o: usize) -> f64 {
    f64::from_bits(w[o / 8])
}

fn v4(w: &[u64], o: usize) -> [f64; 4] {
    std::array::from_fn(|i| f(w, o + 8 * i))
}

fn m12(w: &[u64], o: usize) -> [f64; 12] {
    std::array::from_fn(|i| f(w, o + 8 * i))
}

fn u32_at(w: &[u64], o: usize) -> u32 {
    (w[o / 8] >> (8 * (o % 8))) as u32
}

pub(crate) fn body(b: &[u64]) -> Body<f64> {
    Body::<f64> {
        alive: true,
        next: None,
        tag: u32_at(b, 0x20) as i32,
        first_joint: None,
        first_geom: None,
        flags: b[0x38 / 8] as u32,
        mass: Mass { mass: f(b, 0x48), c: v4(b, 0x50), i: m12(b, 0x70) },
        inv_i: m12(b, 0xd0),
        inv_mass: f(b, 0x130),
        pos: v4(b, 0x138),
        r: m12(b, 0x158),
        q: v4(b, 0x1b8),
        lvel: v4(b, 0x1d8),
        avel: v4(b, 0x1f8),
        facc: v4(b, 0x218),
        tacc: v4(b, 0x238),
        finite_rot_axis: v4(b, 0x258),
        adis: AutoDisable { idle_time: f(b, 0x278), idle_steps: u32_at(b, 0x280) as i32, linear_average_threshold: f(b, 0x288), angular_average_threshold: f(b, 0x290), average_samples: u32_at(b, 0x284) },
        adis_timeleft: f(b, 0x298),
        adis_stepsleft: u32_at(b, 0x2a0) as i32,
        average_lvel_buffer: Vec::new(),
        average_avel_buffer: Vec::new(),
        average_counter: u32_at(b, 0x2b8),
        average_ready: u32_at(b, 0x2bc) != 0,
        dampingp: Damping { linear_scale: f(b, 0x2c8), angular_scale: f(b, 0x2d0), linear_threshold: f(b, 0x2d8), angular_threshold: f(b, 0x2e0) },
        max_angular_speed: f(b, 0x2e8),
    }
}

/// A recorded joint (0x200 bytes) by its vtable's low 16 bits; `index` maps a body pointer to its
/// place in the island.
fn joint_of(j: &[u64], index: &dyn Fn(u64) -> Option<usize>) -> Joint {
    let kind = match j[0] & 0xffff {
        0x2928 => JointKind::Fixed { qrel: v4(j, 0xa0), offset: v4(j, 0xc0), erp: f(j, 0xe0), cfm: f(j, 0xf8) },
        0x29e8 => JointKind::DBall { anchor1: v4(j, 0xa0), anchor2: v4(j, 0xc0), erp: f(j, 0xe0), cfm: f(j, 0xe8), target_distance: f(j, 0xf0) },
        0x3170 => {
            let contact = Contact {
                mode: j[0xa8 / 8] as u32,
                mu: f(j, 0xb0),
                mu2: f(j, 0xb8),
                rho: f(j, 0xc0),
                rho2: f(j, 0xc8),
                rho_n: f(j, 0xd0),
                bounce: f(j, 0xd8),
                bounce_vel: f(j, 0xe0),
                soft_erp: f(j, 0xe8),
                soft_cfm: f(j, 0xf0),
                motion1: f(j, 0xf8),
                motion2: f(j, 0x100),
                motion_n: f(j, 0x108),
                slip1: f(j, 0x110),
                slip2: f(j, 0x118),
                pos: v4(j, 0x120),
                normal: v4(j, 0x140),
                depth: f(j, 0x160),
                fdir1: v4(j, 0x180),
            };
            JointKind::Contact { contact, reverse: j[0x30 / 8] & 2 != 0, the_m: 0 }
        }
        vt => panic!("joint vtable ..{vt:x} not handled"),
    };
    let feedback = (j[0x68 / 8] != 0).then(|| std::array::from_fn(|i| f(j, JOINT + 8 * i)));
    Joint { kind, b0: index(j[0x40 / 8]).expect("body 0 in the island"), b1: index(j[0x58 / 8]), feedback }
}

/// A record 1 waiting for its record 2: h, world, (pointer, body) words, joint words, record 3's parts.
type Pending = (f64, Vec<u64>, Vec<(u64, Vec<u64>)>, Vec<Vec<u64>>, Option<Vec<Vec<u64>>>);

/// `--islands <file>`: every recorded island stepped by rustyac-ode's `csp_step::step_island` from
/// its record-1 state, the bodies against record 2 (pos, q, R, lvel, avel, facc, tacc bit for
/// bit). Prints "island <steps> steps, <islands> islands (<c> with contacts): <d> differ".
pub(crate) fn check(path: &str) {
    let data = std::fs::read(path).expect("the island file can be read");
    let words: Vec<u64> = data.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    let at = std::cell::Cell::new(0);
    let take = |n: usize| {
        at.set(at.get() + n);
        &words[at.get() - n..at.get()]
    };
    let mut pending: std::collections::VecDeque<Pending> = std::collections::VecDeque::new();
    let mut kinds = std::collections::BTreeMap::new();
    let mut with_feedback = 0;
    let (mut islands, mut with_contacts, mut differ, mut steps, mut lcp_differ) = (0, 0, 0, std::collections::BTreeSet::new(), 0);
    while at.get() < words.len() {
        match take(1)[0] {
            1 => {
                let head = take(5).to_vec();
                let (h, nb, nj) = (f64::from_bits(head[1]), head[2] as usize, head[3] as usize);
                let world = take(WORLD / 8).to_vec();
                let bodies: Vec<(u64, Vec<u64>)> = (0..nb).map(|_| (take(1)[0], take(BODY / 8).to_vec())).collect();
                let joints: Vec<Vec<u64>> = (0..nj).map(|_| take(1 + (JOINT + FEEDBACK) / 8)[1..].to_vec()).collect();
                pending.push_back((h, world, bodies, joints, None));
            }
            3 => {
                let head = take(2).to_vec();
                let (m, nub) = (head[0] as usize, head[1] as usize);
                let nskip = if m <= 1 { m } else { (m + 3) & !3 };
                let a = take(m * nskip).to_vec();
                let (bx, lh, fi) = (take(2 * m).to_vec(), take(2 * m).to_vec(), take(m).to_vec());
                let last: &mut Pending = pending.back_mut().expect("a record 3 follows its record 1");
                let bodies = (0..last.2.len()).map(|_| take(BODY / 8).to_vec()).collect();
                let mut v = vec![vec![m as u64, nub as u64], a, bx, lh, fi];
                v.extend::<Vec<Vec<u64>>>(bodies);
                last.4 = Some(v);
            }
            2 => {
                let head = take(3).to_vec();
                let (step, nb, nj) = (head[0], head[1] as usize, head[2] as usize);
                let after: Vec<Vec<u64>> = (0..nb).map(|_| take(BODY / 8).to_vec()).collect();
                let joints_after: Vec<Vec<u64>> = (0..nj).map(|_| take((JOINT + FEEDBACK) / 8).to_vec()).collect();
                let (h, w, recorded, recorded_joints, lcp) = pending.pop_front().expect("a record 2 follows its record 1");
                steps.insert(step);
                islands += 1;
                let world = WorldParams { gravity: [f(&w, 0x40), f(&w, 0x48), f(&w, 0x50)], erp: f(&w, 0x60), cfm: f(&w, 0x68), max_vel: f(&w, 0xf8), min_depth: f(&w, 0x100) };
                let mut bodies: Vec<Body<f64>> = recorded.iter().map(|(_, b)| body(b)).collect();
                let index = |p: u64| recorded.iter().position(|(q, _)| *q == p);
                let mut joints: Vec<Joint> = recorded_joints.iter().map(|j| joint_of(j, &index)).collect();
                with_contacts += joints.iter().any(|j| matches!(j.kind, JointKind::Contact { .. })) as usize;
                let mut probe = |p: &rustyac_ode::csp_step::LcpInputs| {
                    let Some(l) = &lcp else { return };
                    let mut bad = |what: &str, i: usize, csp: u64, port: u64| {
                        if csp != port && lcp_differ < 12 {
                            println!("step {step} lcp {what}[{i}] csp {:e} port {:e} ({csp:x} {port:x})", f64::from_bits(csp), f64::from_bits(port));
                        }
                        lcp_differ += (csp != port) as usize;
                        if csp != port {
                            *kinds.entry(what.split(' ').next_back().unwrap().to_string()).or_insert(0usize) += 1;
                        }
                    };
                    bad("m", 0, l[0][0], p.m as u64);
                    bad("nub", 0, l[0][1], p.nub as u64);
                    // only A's lower triangle is written (the rest is old arena memory)
                    let nskip = if p.m <= 1 { p.m } else { (p.m + 3) & !3 };
                    for (i, (c, x)) in l[1].iter().zip(p.a).enumerate().filter(|(i, _)| i % nskip <= i / nskip) {
                        bad("A", i, *c, x.to_bits());
                    }
                    for (i, (c, x)) in l[2].iter().zip(p.bx).enumerate() {
                        bad("bx", i, *c, x.to_bits());
                    }
                    for (i, (c, x)) in l[3].iter().zip(p.lh).enumerate() {
                        bad("lh", i, *c, x.to_bits());
                    }
                    for (i, (c, x)) in l[4].iter().zip(p.findex).enumerate() {
                        bad("findex", i, *c, *x as i64 as u64);
                    }
                    for (bi, b) in p.bodies.iter().enumerate() {
                        let w = &l[5 + bi];
                        for k in 0..3 {
                            bad(&format!("b{bi} facc"), k, w[0x218 / 8 + k], b.facc[k].to_bits());
                            bad(&format!("b{bi} tacc"), k, w[0x238 / 8 + k], b.tacc[k].to_bits());
                        }
                    }
                };
                step_island_probed(&world, &mut bodies, &mut joints, h, &mut probe);
                for (ji, (j, a)) in joints.iter().zip(&joints_after).enumerate() {
                    let Some(fb) = &j.feedback else { continue };
                    with_feedback += 1;
                    for k in (0..16).filter(|k| k % 4 != 3) {
                        let csp = a[JOINT / 8 + k];
                        if csp != fb[k].to_bits() {
                            if differ < 8 {
                                println!("step {step} joint {ji} feedback[{k}]: csp {:e} port {:e}", f64::from_bits(csp), fb[k]);
                            }
                            differ += 1;
                        }
                    }
                }
                for (bi, (b, a)) in bodies.iter().zip(&after).enumerate() {
                    let fields = [(0x138, &b.pos[..3]), (0x1b8, &b.q[..]), (0x158, &b.r[..]), (0x1d8, &b.lvel[..3]), (0x1f8, &b.avel[..3]), (0x218, &b.facc[..3]), (0x238, &b.tacc[..3])];
                    for (o, port) in fields {
                        for (k, x) in port.iter().enumerate() {
                            let csp = a[o / 8 + k];
                            if csp != x.to_bits() && !(x.is_nan() && f64::from_bits(csp).is_nan()) {
                                if differ < 8 {
                                    println!("step {step} body {bi} +0x{:x}: csp {:e} port {x:e}", o + 8 * k, f64::from_bits(csp));
                                }
                                differ += 1;
                            }
                        }
                    }
                }
            }
            r => panic!("record kind {r} at word {}", at.get() - 1),
        }
    }
    println!("island {} steps, {islands} islands ({with_contacts} with contacts, {with_feedback} joint feedbacks): {differ} differ, solver inputs {lcp_differ} differ {kinds:?}", steps.len());
}

// ---- Task C5e: the world recorder -------------------------------------------------------------
// `CSP_ORACLE_BREAK=16c7650a,92ea7dz` with `CSP_ORACLE_ISLANDS=<file>`: at the entry of the
// world-process set-up (+0x16c7650: both worker paths call it before the auto-disable and the island
// build; rcx the ODE world, h in xmm2) the whole world (record 4), at the join after the islands
// were stepped (+0x92ea7d) the whole world again (record 5). Both: kind, step, h bits (0 in 5), world
// ptr, world 0x200 bytes, nb, nb × (ptr, 0x300 bytes, the auto-disable sample buffers (samples × 4
// words each, lvel then avel), node count, nodes × (joint ptr, other body ptr)), nj, nj × (ptr,
// 0x200 bytes, feedback 0x80 bytes or zeros); bodies and joints in the world's list order.

static WORLD_PTR: Mutex<usize> = Mutex::new(0);

unsafe fn world_dump(kind: u64, world: usize, h: f64, step: usize) {
    let r = |a: usize| *(a as *const usize);
    let list = |mut p: usize| {
        let mut v = Vec::new();
        while p != 0 {
            v.push(p);
            p = r(p + 0x10);
        }
        v
    };
    let (bodies, joints) = (list(r(world + 0x28)), list(r(world + 0x30)));
    let mut out = OUT.lock().unwrap();
    for x in [kind, step as u64, h.to_bits(), world as u64] {
        word(&mut out, x);
    }
    raw(&mut out, world, WORLD);
    word(&mut out, bodies.len() as u64);
    for &b in &bodies {
        word(&mut out, b as u64);
        raw(&mut out, b, BODY);
        let samples = *((b + 0x284) as *const u32) as usize;
        for buffer in [r(b + 0x2a8), r(b + 0x2b0)] {
            if buffer != 0 {
                raw(&mut out, buffer, 0x20 * samples);
            } else {
                out.extend(std::iter::repeat_n(0, 0x20 * samples));
            }
        }
        let mut nodes = Vec::new();
        let mut n = r(b + 0x30);
        while n != 0 {
            nodes.push((r(n), r(n + 8)));
            n = r(n + 0x10);
        }
        word(&mut out, nodes.len() as u64);
        for (j, other) in nodes {
            word(&mut out, j as u64);
            word(&mut out, other as u64);
        }
    }
    word(&mut out, joints.len() as u64);
    for &j in &joints {
        // CSP_ORACLE_FEEDBACK=1: a zeroed dJointFeedback for every joint without one
        let fb = (j + 0x68) as *mut usize;
        if *fb == 0 && kind == 4 && std::env::var_os("CSP_ORACLE_FEEDBACK").is_some() {
            *fb = Box::leak(Box::new([0u8; FEEDBACK])).as_mut_ptr() as usize;
        }
        word(&mut out, j as u64);
        joint(&mut out, j);
    }
}

pub(crate) unsafe fn world_enter(world: usize, h: f64, step: usize) {
    *WORLD_PTR.lock().unwrap() = world;
    world_dump(4, world, h, step);
}

pub(crate) unsafe fn world_leave(step: usize) {
    let world = std::mem::take(&mut *WORLD_PTR.lock().unwrap());
    if world != 0 {
        world_dump(5, world, 0.0, step);
    }
}

/// A recorded body: pointer, 0x300 bytes, lvel and avel sample buffers, nodes (joint, other body).
type WorldBody = (u64, Vec<u64>, Vec<u64>, Vec<u64>, Vec<(u64, u64)>);

/// A parsed record 4 or 5.
struct WorldRecord {
    kind: u64,
    step: u64,
    h: f64,
    world: Vec<u64>,
    bodies: Vec<WorldBody>,
    joints: Vec<(u64, Vec<u64>)>,
}

/// `--world <file>`: every recorded world step done again by rustyac-ode's `CspWorld::step` from
/// its record-4 state, every body (pos, q, R, lvel, avel, facc, tacc, flags, tag, the auto-disable
/// state and sample buffers) and joint feedback against record 5 bit for bit. Prints "world <steps>
/// steps, <islands> islands, <bodies> bodies, <contacts> contacts: <d> differ, after <hash>", the
/// hash over record 5's bodies (pos .. tacc, time left, flags) and feedback, to compare recordings.
pub(crate) fn check_world(path: &str) {
    use rustyac_ode::csp_world::{CspWorld, WorldJoint};
    let data = std::fs::read(path).expect("the world file can be read");
    let words: Vec<u64> = data.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    let at = std::cell::Cell::new(0);
    let take = |n: usize| {
        at.set(at.get() + n);
        words[at.get() - n..at.get()].to_vec()
    };
    let mut records = Vec::new();
    while at.get() < words.len() {
        let head = take(4);
        assert!(head[0] == 4 || head[0] == 5, "record kind {} at word {}", head[0], at.get() - 4);
        let world = take(WORLD / 8);
        let nb = take(1)[0] as usize;
        let bodies = (0..nb)
            .map(|_| {
                let (p, b) = (take(1)[0], take(BODY / 8));
                let samples = u32_at(&b, 0x284) as usize;
                let (l, a) = (take(4 * samples), take(4 * samples));
                let n = take(1)[0] as usize;
                let nodes = take(2 * n).chunks(2).map(|c| (c[0], c[1])).collect();
                (p, b, l, a, nodes)
            })
            .collect();
        let nj = take(1)[0] as usize;
        let joints = (0..nj).map(|_| (take(1)[0], take((JOINT + FEEDBACK) / 8))).collect();
        records.push(WorldRecord { kind: head[0], step: head[1], h: f64::from_bits(head[2]), world, bodies, joints });
    }
    let (mut steps, mut islands, mut max_bodies, mut contacts, mut differ) = (0, 0, 0, 0, 0usize);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let buffer = |w: &[u64]| w.chunks(4).map(|c| [f64::from_bits(c[0]), f64::from_bits(c[1]), f64::from_bits(c[2])]).collect::<Vec<_>>();
    for pair in records.chunks(2) {
        let [before, after] = pair else { panic!("a record 4 without its record 5") };
        assert!(before.kind == 4 && after.kind == 5, "record 4 then 5 expected at step {}", before.step);
        assert_eq!(before.bodies.iter().map(|b| b.0).collect::<Vec<_>>(), after.bodies.iter().map(|b| b.0).collect::<Vec<_>>(), "the same bodies before and after");
        let index = |p: u64| before.bodies.iter().position(|b| b.0 == p);
        let w = &before.world;
        let mut world = CspWorld {
            params: WorldParams { gravity: [f(w, 0x40), f(w, 0x48), f(w, 0x50)], erp: f(w, 0x60), cfm: f(w, 0x68), max_vel: f(w, 0xf8), min_depth: f(w, 0x100) },
            bodies: before
                .bodies
                .iter()
                .map(|(_, b, l, a, _)| {
                    let mut body = body(b);
                    (body.average_lvel_buffer, body.average_avel_buffer) = (buffer(l), buffer(a));
                    body
                })
                .collect(),
            joints: before.joints.iter().map(|(_, j)| WorldJoint { joint: joint_of(j, &index), flags: j[0x30 / 8] as u32, tag: u32_at(j, 0x20) as i32 }).collect(),
            nodes: before
                .bodies
                .iter()
                .map(|b| b.4.iter().map(|&(j, o)| (before.joints.iter().position(|x| x.0 == j).expect("a node's joint in the list"), index(o))).collect())
                .collect(),
        };
        contacts += world.joints.iter().filter(|j| matches!(j.joint.kind, JointKind::Contact { .. })).count();
        max_bodies = max_bodies.max(world.bodies.len());
        islands += world.step(before.h).len();
        steps += 1;
        let step = before.step;
        let mut bad = |what: String, csp: u64, port: u64| {
            if csp != port && !(f64::from_bits(csp).is_nan() && f64::from_bits(port).is_nan()) {
                if differ < 12 {
                    println!("step {step} {what}: csp {:e} port {:e} ({csp:x} {port:x})", f64::from_bits(csp), f64::from_bits(port));
                }
                differ += 1;
            }
        };
        let mut eat = |x: u64| hash = (hash ^ x).wrapping_mul(0x100_0000_01b3);
        for (bi, (b, (_, a, l, av, _))) in world.bodies.iter().zip(&after.bodies).enumerate() {
            let fields = [(0x138, &b.pos[..3]), (0x1b8, &b.q[..]), (0x158, &b.r[..]), (0x1d8, &b.lvel[..3]), (0x1f8, &b.avel[..3]), (0x218, &b.facc[..3]), (0x238, &b.tacc[..3]), (0x298, &[b.adis_timeleft][..])];
            for (o, port) in fields {
                for (k, x) in port.iter().enumerate() {
                    bad(format!("body {bi} +0x{:x}", o + 8 * k), a[o / 8 + k], x.to_bits());
                    eat(a[o / 8 + k]);
                }
            }
            for (o, port) in [(0x38, b.flags), (0x20, b.tag as u32), (0x2a0, b.adis_stepsleft as u32), (0x2b8, b.average_counter), (0x2bc, b.average_ready as u32)] {
                bad(format!("body {bi} +0x{o:x}"), u32_at(a, o) as u64, port as u64);
            }
            eat(u32_at(a, 0x38) as u64);
            for (s, (pl, pa)) in b.average_lvel_buffer.iter().zip(&b.average_avel_buffer).enumerate() {
                for c in 0..3 {
                    bad(format!("body {bi} lvel sample {s}[{c}]"), l[4 * s + c], pl[c].to_bits());
                    bad(format!("body {bi} avel sample {s}[{c}]"), av[4 * s + c], pa[c].to_bits());
                }
            }
        }
        for (ji, (j, (_, a))) in world.joints.iter().zip(&after.joints).enumerate() {
            let Some(fb) = &j.joint.feedback else { continue };
            for k in (0..16).filter(|k| k % 4 != 3) {
                bad(format!("joint {ji} feedback[{k}]"), a[JOINT / 8 + k], fb[k].to_bits());
                eat(a[JOINT / 8 + k]);
            }
        }
    }
    println!("world {steps} steps, {islands} islands, {max_bodies} bodies, {contacts} contacts: {differ} differ, after {hash:016x}");
}
