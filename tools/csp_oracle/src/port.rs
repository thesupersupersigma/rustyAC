// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5g: `CSP_ORACLE_PORT=1` (with `CSP_ORACLE_BREAK=<every wrapper>p` and
//! `CSP_ORACLE_BREAK_SETUP=1`): rustyac-ode's `csp_core` fed every outermost call the car makes
//! into CSP's cphys:: wrappers as it happens, nothing copied from CSP's world. Each getter's answer
//! is checked at its return, and the port's world against CSP's at each world step's entry.

use rustyac_ode::csp_core::Core;
use rustyac_ode::csp_step::JointKind;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

/// What a call's return is checked against or tells the port.
pub(crate) enum Expect {
    Nothing,
    /// the creator returned this body / joint of the port's
    Body(usize),
    Joint(usize),
    /// the f32s the call writes at this address (and whether in the set-up)
    Out(usize, Vec<f32>, bool),
    /// an f32 returned in xmm0
    Float(f32, bool),
}

struct Port {
    core: Core,
    bodies: HashMap<usize, usize>,
    joints: HashMap<usize, usize>,
    /// rva: (calls, wrong answers)
    calls: BTreeMap<usize, (usize, usize)>,
    ignored: BTreeMap<usize, usize>,
    /// set-up and per step
    answers: [usize; 2],
    wrong: [usize; 2],
    worlds: usize,
    differ: usize,
    log: String,
}

static PORT: Mutex<Option<Port>> = Mutex::new(None);

pub(crate) fn on() -> bool {
    std::env::var_os("CSP_ORACLE_PORT").is_some()
}

unsafe fn f32s(p: usize, n: usize) -> Vec<f32> {
    (0..n).map(|i| std::ptr::read_unaligned((p + 4 * i) as *const f32)).collect()
}

unsafe fn v3(p: usize) -> [f32; 3] {
    let v = f32s(p, 3);
    [v[0], v[1], v[2]]
}

/// A body by its game interface pointer, or by the cphys one 8 bytes in.
fn body_of(bodies: &HashMap<usize, usize>, x: usize) -> Option<usize> {
    bodies.get(&x).or_else(|| bodies.get(&x.wrapping_sub(8))).copied()
}

/// A wrapper's entry: the call done on the port; what its return must show.
pub(crate) unsafe fn enter(context: usize, rva: usize, step: usize) -> Expect {
    let r = |o: usize| std::ptr::read_unaligned((context + o) as *const usize);
    let xmm = |i: usize| std::ptr::read_unaligned((context + 0x1a0 + 0x10 * i) as *const f32);
    let [rcx, rdx, r8, r9] = [0x80, 0x88, 0xb8, 0xc0].map(r);
    let stack = |i: usize| std::ptr::read_unaligned((r(0x98) + 0x28 + 8 * i) as *const usize);
    let mut guard = PORT.lock().unwrap();
    let port = guard.get_or_insert_with(|| Port {
        core: Core::new(),
        bodies: HashMap::new(),
        joints: HashMap::new(),
        calls: BTreeMap::new(),
        ignored: BTreeMap::new(),
        answers: [0; 2],
        wrong: [0; 2],
        worlds: 0,
        differ: 0,
        log: String::new(),
    });
    let b = body_of(&port.bodies, rcx);
    let expect = match (rva, b) {
        (0x92ed80, _) => {
            assert_eq!(std::ptr::read_unaligned((rcx + 0x3298) as *const usize), 0, "a split-along body pending: not ported");
            let no_finite = *((crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed) + 0x280_320f) as *const u8) != 0;
            Expect::Body(port.core.create_body(no_finite))
        }
        (0x92f050, _) => {
            let (b1, b2) = (body_of(&port.bodies, rdx).expect("joint body 1"), body_of(&port.bodies, r8).expect("joint body 2"));
            Expect::Joint(port.core.create_distance_joint(b1, b2, v3(r9), v3(stack(0))))
        }
        (0x92fd20, _) => {
            let (b1, b2) = (body_of(&port.bodies, rdx).expect("joint body 1"), body_of(&port.bodies, r8).expect("joint body 2"));
            Expect::Joint(port.core.create_fixed_joint(b1, b2))
        }
        (0x92fbb0, _) => {
            port.core.reseat(port.joints[&rdx], v3(r8), v3(r9));
            Expect::Nothing
        }
        (0x928310, _) => {
            port.core.set_erp_cfm(port.joints[&rcx], xmm(1), xmm(2));
            Expect::Nothing
        }
        (0x92e4a0, _) => {
            port.worlds += 1;
            compare(port, rcx, step);
            Expect::Nothing
        }
        (rva, Some(b)) => {
            let c = &mut port.core;
            match rva {
                0x937780 => c.set_mass_box(b, xmm(1), xmm(2), xmm(3), f32::from_bits(stack(0) as u32)),
                0x937460 => c.set_position(b, v3(rdx)),
                0x937050 => c.set_rotation(b, &f32s(rdx, 16).try_into().unwrap()),
                0x937910 => c.stop(b),
                0x936ea0 => c.add_local_force_at_local_pos(b, v3(rdx), v3(r8)),
                0x936f00 => c.add_force_at_local_pos(b, v3(rdx), v3(r8)),
                0x936fc0 => c.add_force_at_pos(b, v3(rdx), v3(r8)),
                0x934100 => c.add_torque(b, v3(rdx)),
                _ => {}
            }
            let setup = step == crate::SETUP;
            let out = |v: &[f32]| Expect::Out(rdx, v.to_vec(), setup);
            match rva {
                0x9348b0 => out(&c.position(b)),
                0x934e80 => out(&c.velocity(b)),
                0x934870 => out(&c.angular_velocity(b)),
                0x9346d0 => out(&c.local_velocity(b)),
                0x9347a0 => out(&c.local_angular_velocity(b)),
                0x937620 => {
                    // the wrapper's offset (byte +156) is never set by the calls seen
                    assert_eq!(*((rcx + 156) as *const u8), 0, "a body with an offset");
                    out(&c.world_matrix(b))
                }
                0x937700 => out(&c.local_inertia(b)),
                0x934ec0 => out(&c.world_to_local_normal(b, v3(r8))),
                0x937520 => out(&c.world_to_local(b, v3(r8))),
                0x9375a0 => out(&c.local_to_world(b, v3(r8))),
                0x933e30 => out(&c.point_velocity(b, v3(r8))),
                0x936db0 => out(&c.local_point_velocity(b, v3(r8))),
                0x9378d0 => Expect::Float(c.mass(b), setup),
                0x937780 | 0x937460 | 0x937050 | 0x937910 | 0x936ea0 | 0x936f00 | 0x936fc0 | 0x934100 => Expect::Nothing,
                _ => {
                    *port.ignored.entry(rva).or_default() += 1;
                    return Expect::Nothing;
                }
            }
        }
        _ => {
            *port.ignored.entry(rva).or_default() += 1;
            return Expect::Nothing;
        }
    };
    port.calls.entry(rva).or_default().0 += 1;
    expect
}

/// The call's return: a creator's pointer mapped, an answer checked.
pub(crate) unsafe fn leave(rva: usize, rax: usize, xmm0: usize, expect: Expect) {
    let mut guard = PORT.lock().unwrap();
    let Some(port) = guard.as_mut() else { return };
    let (ok, setup) = match expect {
        Expect::Nothing => return,
        Expect::Body(b) => {
            port.bodies.insert(rax, b);
            return;
        }
        Expect::Joint(j) => {
            port.joints.insert(rax, j);
            return;
        }
        Expect::Out(p, want, setup) => {
            let got = f32s(p, want.len());
            let ok = got.iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits());
            if !ok && port.wrong[0] + port.wrong[1] < 8 {
                port.log.push_str(&format!("  {rva:#x}: csp {got:?} port {want:?}\n"));
            }
            (ok, setup)
        }
        Expect::Float(want, setup) => (xmm0 as u32 == want.to_bits(), setup),
    };
    let k = usize::from(!setup);
    port.answers[k] += 1;
    if !ok {
        port.wrong[k] += 1;
        port.calls.entry(rva).or_default().1 += 1;
    }
}

/// The port's world against CSP's (`dxWorld` at core +16) at a world step's entry: every body and
/// joint in list order and each body's joint nodes.
unsafe fn compare(port: &mut Port, core: usize, step: usize) {
    let r = |a: usize| std::ptr::read_unaligned(a as *const usize);
    let world = r(core + 16);
    let list = |mut p: usize| {
        let mut v = Vec::new();
        while p != 0 {
            v.push(p);
            p = r(p + 0x10);
        }
        v
    };
    let words = |p: usize, n: usize| (0..n / 8).map(|i| r(p + 8 * i) as u64).collect::<Vec<u64>>();
    let (bodies, joints) = (list(r(world + 0x28)), list(r(world + 0x30)));
    let index = |p: u64| bodies.iter().position(|&b| b as u64 == p);
    let w = &port.core.world;
    let mut bad = Vec::new();
    if bodies.len() != w.bodies.len() || joints.len() != w.joints.len() {
        bad.push(format!("{} bodies {} joints, port {} {}", bodies.len(), joints.len(), w.bodies.len(), w.joints.len()));
    }
    for (i, (&b, pb)) in bodies.iter().zip(&w.bodies).enumerate() {
        let mut cb = crate::island::body(&words(b, 0x300));
        if cb.flags & 16 != 0 {
            let samples = cb.adis.average_samples as usize;
            let buffer = |p: usize| (0..samples).map(|s| std::array::from_fn(|k| f64::from_bits(r(p + 32 * s + 8 * k) as u64))).collect();
            (cb.average_lvel_buffer, cb.average_avel_buffer) = (buffer(r(b + 0x2a8)), buffer(r(b + 0x2b0)));
        } else {
            // never read with auto-disable off (CSP leaves them as malloc gave them)
            cb.average_lvel_buffer.clone_from(&pb.average_lvel_buffer);
            cb.average_avel_buffer.clone_from(&pb.average_avel_buffer);
        }
        if format!("{cb:?}") != format!("{pb:?}") {
            bad.push(format!("body {i}:\n    csp  {cb:?}\n    port {pb:?}"));
        }
        let mut nodes = Vec::new();
        let mut n = r(b + 0x30);
        while n != 0 {
            nodes.push((joints.iter().position(|&j| j == r(n)).expect("a node's joint in the list"), index(r(n + 8) as u64)));
            n = r(n + 0x10);
        }
        if w.nodes.get(i) != Some(&nodes) {
            bad.push(format!("body {i} nodes: csp {nodes:?} port {:?}", w.nodes.get(i)));
        }
    }
    for (i, (&j, pj)) in joints.iter().zip(&w.joints).enumerate() {
        let words = words(j, crate::island::JOINT);
        let mut cj = crate::island::joint_of(&words, &index);
        // a vector's 4th double is padding CSP never writes
        match &mut cj.kind {
            JointKind::DBall { anchor1, anchor2, .. } => (anchor1[3], anchor2[3]) = (0.0, 0.0),
            JointKind::Fixed { offset, .. } => offset[3] = 0.0,
            _ => {}
        }
        let (flags, tag) = (words[0x30 / 8] as u32, words[0x20 / 8] as u32 as i32);
        let a = format!("{:?} {:?} {:?} {flags} {tag}", cj.kind, cj.b0, cj.b1);
        let b = format!("{:?} {:?} {:?} {} {}", pj.joint.kind, pj.joint.b0, pj.joint.b1, pj.flags, pj.tag);
        if a != b {
            bad.push(format!("joint {i}:\n    csp  {a}\n    port {b}"));
        }
    }
    port.differ += bad.len();
    if !bad.is_empty() && port.log.len() < 20000 {
        port.log.push_str(&format!("  world step {} (step {step}): {} differ\n", port.worlds, bad.len()));
        bad.iter().take(6).for_each(|x| port.log.push_str(&format!("  {x}\n")));
    }
}

/// "port: <n> world steps compared, <d> differ; <a> answers, <w> wrong; calls (n/wrong) ...;
/// not ported ...", then the first differences.
pub(crate) fn finish() -> String {
    let guard = PORT.lock().unwrap();
    let Some(p) = guard.as_ref() else { return String::new() };
    let calls: Vec<String> = p.calls.iter().map(|(rva, (n, bad))| format!("{rva:x} {n}/{bad}")).collect();
    let ignored: Vec<String> = p.ignored.iter().map(|(rva, n)| format!("{rva:x} {n}")).collect();
    format!("port: {} world steps compared, {} differ; answers set-up {} ({} wrong), steps {} ({} wrong); calls (n/wrong) {}; not ported {}\n{}", p.worlds, p.differ, p.answers[0], p.wrong[0], p.answers[1], p.wrong[1], calls.join(" "), ignored.join(" "), p.log)
}
