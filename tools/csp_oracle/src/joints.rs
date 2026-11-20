// SPDX-License-Identifier: GPL-3.0-or-later

//! Break kind `j` on CSP's contact joint maker (+0x92c9f0, (world, contacts, n, g1, g2)): at the
//! entry the oracle keeps the contacts, both geoms' sides, the switches, the bodies and the head of
//! the world's joint list, and puts a break (kind `k`) on the return site. There it walks the
//! joints CSP added and compares them with `rustyac_ode::csp_collide::contact_joints` on the same
//! inputs: count, kind, contact (surface, geometry, fdir1) bit for bit, bodies, reverse flag.
//! Each entry also runs CSP's deep-contact softening (+0x91c520) and the port's on made-up
//! inputs under both MXCSR settings.

use crate::read;
use rustyac_ode::csp_btl::ContactGeom;
use rustyac_ode::csp_collide::{contact_joints, soften, NewJoint, Side, Switches};
use std::collections::HashMap;
use std::sync::Mutex;

struct Call {
    hit: u64,
    world: usize,
    head: usize,
    contacts: Vec<ContactGeom>,
    sides: [Side; 2],
    bodies: Vec<usize>,
    words: Vec<Vec<u64>>,
    switches: Switches,
    batched: bool,
}

#[derive(Default)]
struct State {
    hits: u64,
    compared: u64,
    joints: u64,
    contacts: u64,
    skipped: u64,
    differ: u64,
    unhandled: HashMap<&'static str, u64>,
    paths: HashMap<String, u64>,
    notes: Vec<String>,
    rng: u64,
    soft: [(u64, u64); 2],
    open: HashMap<u32, Vec<Call>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

unsafe fn words(at: usize, n: usize) -> Vec<u64> {
    (0..n).map(|i| read::<u64>(at + 8 * i)).collect()
}

pub(crate) unsafe fn side(geom: usize) -> (Side, usize) {
    let data = read::<usize>(geom + 0x10);
    let body = read::<usize>(geom + 0x18);
    let user = if body != 0 { read::<usize>(body + 0x28) } else { 0 };
    let special = user != 0 && read::<u8>(user + 152) != 0 && read::<u8>(user + 153) == 0;
    let grip = user != 0 && read::<u8>(user + 159) & 1 != 0;
    (Side { class: read::<i32>(geom + 8), no_data: data == 0 || data & 1 != 0, body: None, grip, special }, body)
}

/// The maker's entry: `rsp` is the stack pointer at the break (the fifth argument at +0x28).
pub(crate) unsafe fn enter(world: usize, buf: usize, n: i32, g1: usize, rsp: usize, dll: usize, thread: u32) {
    let g2 = read::<usize>(rsp + 0x28);
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(Default::default);
    st.hits += 1;
    let contacts = (0..n.max(0) as usize)
        .map(|i| {
            let c = buf + i * 96;
            ContactGeom { pos: read(c), normal: read(c + 32), depth: read(c + 64), g1: read(c + 72), g2: read(c + 80), side1: read(c + 88), side2: read(c + 92) }
        })
        .collect();
    let (mut s1, p1) = side(g1);
    let (mut s2, p2) = side(g2);
    let mut bodies = Vec::new();
    for (s, p) in [(&mut s1, p1), (&mut s2, p2)] {
        if p != 0 {
            s.body = Some(bodies.len());
            bodies.push(p);
        }
    }
    let switches = switches(dll, &bodies);
    let path = format!("classes {} {} bodies {} {} data {} {} sw {:?}", s1.class, s2.class, p1 != 0, p2 != 0, !s1.no_data, !s2.no_data, switches);
    *st.paths.entry(path).or_default() += 1;
    let call = Call {
        hit: st.hits,
        world,
        head: read::<usize>(read::<usize>(world + 0x10) + 0x30),
        contacts,
        sides: [s1, s2],
        words: bodies.iter().map(|&b| words(b, 0x60)).collect(),
        bodies,
        switches,
        batched: read::<u8>(world + 0x3279) != 0,
    };
    st.open.entry(thread).or_default().push(call);
    soft_check(st, dll);
}

/// CSP's softening (+0x91c520) and the port's on made-up inputs, both MXCSR settings.
unsafe fn soft_check(st: &mut State, dll: usize) {
    type Soft = extern "C" fn(*mut f64, *mut f64, f32, f32);
    let csp: Soft = std::mem::transmute(dll + 0x91_c520);
    if st.rng == 0 {
        st.rng = 0x9e37_79b9_7f4a_7c15;
    }
    let mut next = || {
        st.rng ^= st.rng << 13;
        st.rng ^= st.rng >> 7;
        st.rng ^= st.rng << 17;
        st.rng
    };
    let u = |r: u64| (r >> 11) as f64 / (1u64 << 53) as f64;
    let mut cases = Vec::new();
    for i in 0..24 {
        let limit = [0.1f32, 0.07, 0.3, u(next()) as f32, 1e-40, 0.0][i % 6];
        let depth = match i % 4 {
            0 => limit + (u(next()) * 0.5) as f32,
            1 => (u(next()) * 2.0) as f32,
            2 => [f32::NAN, f32::INFINITY, 1e-40, limit, limit * 2.0, -0.0][(next() % 6) as usize],
            _ => f32::from_bits(next() as u32),
        };
        let erp = match i % 3 {
            0 => 0.3,
            1 => u(next()),
            _ => [0.0, 1.0, f64::NAN, 1e-310, f64::from_bits(next())][(next() % 5) as usize],
        };
        let cfm = match i % 5 {
            0 => 0.0001,
            1 => u(next()) * 1e-2,
            _ => [0.0, 1e-310, f64::INFINITY, f64::from_bits(next()), 0.0009523809523809524][(next() % 5) as usize],
        };
        cases.push((erp, cfm, depth, limit));
    }
    let mut csr_old = 0u32;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr_old, options(nostack, preserves_flags));
    for (k, csr) in [0x1f80u32, 0x9fc0].into_iter().enumerate() {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        for &(erp, cfm, depth, limit) in &cases {
            let (mut e, mut c) = (erp, cfm);
            csp(&mut e, &mut c, depth, limit);
            let (pe, pc) = soften(erp, cfm, depth, limit);
            st.soft[k].0 += 1;
            if (e.to_bits(), c.to_bits()) != (pe.to_bits(), pc.to_bits()) {
                st.soft[k].1 += 1;
                if st.notes.len() < 20 {
                    st.notes.push(format!("soften mxcsr {csr:#x} ({erp:e}, {cfm:e}, {depth:e}, {limit:e}): csp ({e:e}, {c:e}) port ({pe:e}, {pc:e})"));
                }
            }
        }
    }
    std::arch::asm!("ldmxcsr [{}]", in(reg) &csr_old, options(nostack, preserves_flags, readonly));
}

/// The maker's return site.
pub(crate) unsafe fn leave(thread: u32) {
    let mut guard = STATE.lock().unwrap();
    let Some(st) = guard.as_mut() else { return };
    let Some(call) = st.open.get_mut(&thread).and_then(|v| v.pop()) else { return };
    // the joints CSP added, oldest first
    let mut added = Vec::new();
    let mut j = read::<usize>(read::<usize>(call.world + 0x10) + 0x30);
    while j != 0 && j != call.head {
        added.push(j);
        j = read::<usize>(j + 0x10);
    }
    added.reverse();
    if call.batched {
        st.skipped += 1;
        *st.unhandled.entry("batched").or_default() += 1;
        return;
    }
    let bodies: Vec<_> = call.words.iter().map(|w| crate::island::body(w)).collect();
    let port = match contact_joints(&call.contacts, &call.sides[0], &call.sides[1], &bodies, &call.switches) {
        Ok(p) => p,
        Err(e) => {
            st.skipped += 1;
            *st.unhandled.entry(e.0).or_default() += 1;
            return;
        }
    };
    st.compared += 1;
    st.joints += added.len() as u64;
    st.contacts += call.contacts.len() as u64;
    let mut bad = Vec::new();
    if port.len() != added.len() {
        bad.push(format!("{} joints, port {}", added.len(), port.len()));
    }
    for (k, (&j, p)) in added.iter().zip(&port).enumerate() {
        if let Some(d) = differs(k, j, p, &call.bodies) {
            bad.push(d);
        }
    }
    if !bad.is_empty() {
        st.differ += 1;
        if st.notes.len() < 20 {
            st.notes.push(format!("hit {}: {} contacts: {}", call.hit, call.contacts.len(), bad.join("; ")));
        }
    }
}

/// A port joint's words from +0xa8 as CSP lays a contact joint out (mode .. fdir1).
pub(crate) fn contact_words(p: &NewJoint) -> Vec<u64> {
    let c = &p.contact;
    let g = &p.geom;
    let mut want: Vec<u64> = vec![c.mode as u64];
    want.extend([c.mu, c.mu2, c.rho, c.rho2, c.rho_n, c.bounce, c.bounce_vel, c.soft_erp, c.soft_cfm, c.motion1, c.motion2, c.motion_n, c.slip1, c.slip2].map(f64::to_bits));
    want.extend(c.pos.map(f64::to_bits));
    want.extend(c.normal.map(f64::to_bits));
    want.extend([c.depth.to_bits(), g.g1 as u64, g.g2 as u64, (g.side1 as u32 as u64) | ((g.side2 as u32 as u64) << 32)]);
    want.extend(c.fdir1.map(f64::to_bits));
    want
}

/// CSP's contact joint `j` against the port's `p` (`bodies` maps the port's body indices to
/// pointers): the first difference.
pub(crate) unsafe fn differs(k: usize, j: usize, p: &NewJoint, bodies: &[usize]) -> Option<String> {
    if read::<u64>(j) & 0xffff != 0x3170 {
        return Some(format!("#{k} not a contact joint"));
    }
    let want = contact_words(p);
    let got = words(j + 0xa8, want.len());
    // the mode word's high half and the vector pads are what CSP left there
    let mask = |i: usize| if i == 0 { 0xffff_ffff } else if i == 18 || i == 22 || i == 30 { 0 } else { u64::MAX };
    if let Some(i) = (0..want.len()).find(|&i| (got[i] ^ want[i]) & mask(i) != 0) {
        return Some(format!("#{k} word {i} csp {:#x} port {:#x}", got[i], want[i]));
    }
    let b0 = read::<usize>(j + 0x40);
    let b1 = read::<usize>(j + 0x58);
    let pb0 = bodies[p.b0];
    let pb1 = p.b1.map_or(0, |b| bodies[b]);
    let reverse = read::<u32>(j + 0x30) & 2 != 0;
    if (b0, b1, reverse) != (pb0, pb1, p.reverse) {
        return Some(format!("#{k} bodies {b0:#x} {b1:#x} rev {reverse}, port {pb0:#x} {pb1:#x} rev {}", p.reverse));
    }
    None
}

/// The joint maker's switches as CSP has them now; `bodies` the pair's body pointers.
pub(crate) unsafe fn switches(dll: usize, bodies: &[usize]) -> Switches {
    let flag200 = bodies.iter().any(|&b| read::<u32>(b + 0x38) & 0x200 != 0);
    Switches {
        depth_from_speed: read::<u8>(dll + 0x27f_9197) != 0,
        surface_mix: read::<u32>(dll + 0x26b_9e88) != 0,
        soft_deep: read::<u8>(dll + 0x280_3208 + 6) != 0,
        car_pairs: read::<u8>(dll + 0x280_3220 + 1) != 0 || flag200,
    }
}

pub(crate) fn finish() -> String {
    let guard = STATE.lock().unwrap();
    let Some(st) = guard.as_ref() else { return String::new() };
    let mut s = format!(
        "joints: {} calls, {} compared ({} contacts, {} joints), {} differ, {} not ported; soften mxcsr 0x1f80 {} calls {} differ, 0x9fc0 {} calls {} differ\n",
        st.hits, st.compared, st.contacts, st.joints, st.differ, st.skipped, st.soft[0].0, st.soft[0].1, st.soft[1].0, st.soft[1].1
    );
    st.paths.iter().for_each(|(p, n)| s.push_str(&format!("  path {n}: {p}\n")));
    st.unhandled.iter().for_each(|(p, n)| s.push_str(&format!("  not ported {n}: {p}\n")));
    st.notes.iter().for_each(|n| s.push_str(&format!("  {n}\n")));
    s
}
