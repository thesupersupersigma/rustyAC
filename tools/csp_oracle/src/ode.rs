// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5c: rustyac-ode's f64 functions against CSP's own ODE (master 66615b8a, double) in the
//! process. `--ode <breaks.txt>`: the inputs are the values CSP's functions got on a Spa drive
//! (breakpoint dumps), then edge values and random bits. Prints one line per function:
//! "ode <name>: <n> inputs, <d> differ, <nan> NaN against NaN". NaN against NaN counts as equal
//! (docs/port/ode_stage1.md: a NaN's payload depends on operand order the compiler may swap).

use rustyac_ode::csp_contact::{self as contact, Contact};
use rustyac_ode::joint::{ball_get_info2, fixed_get_info2, Info2};
use rustyac_ode::mass::Mass;
use rustyac_ode::odemath;
use rustyac_ode::rotation as rot;
use rustyac_ode::step::step_body;
use rustyac_ode::world::{AutoDisable, Body, Damping};

type Q = [f64; 4];
type M3 = [f64; 12];

struct Csp {
    q_multiply0: extern "C" fn(*mut f64, *const f64, *const f64),
    q_multiply1: extern "C" fn(*mut f64, *const f64, *const f64),
    q_multiply3: extern "C" fn(*mut f64, *const f64, *const f64),
    r_from_q: extern "C" fn(*mut f64, *const f64),
    q_from_r: extern "C" fn(*mut f64, *const f64),
    dq_from_w: extern "C" fn(*mut f64, *const f64, *const f64),
    box_total: extern "C" fn(*mut f64, f64, f64, f64, f64),
    safe_normalize3: extern "C" fn(*mut f64) -> bool,
    step_body: extern "C" fn(*mut u64, f64),
    ball: GetInfo2,
    fixed: GetInfo2,
    contact1: extern "C" fn(*mut u64, *mut u8),
    contact2: GetInfo2,
}

/// `dxJoint::getInfo2(this, worldFPS, worldERP, rowskip, J1, J2, pairskip, pairRhsCfm, pairLoHi, findex)`
type GetInfo2 = extern "C" fn(*mut u64, f64, f64, i32, *mut f64, *mut f64, i32, *mut f64, *mut f64, *mut i32);

/// The dxBody doubles `dxStepBody` reads (offset, count): pos, q, lvel, avel, finite_rot_axis,
/// damping (linear/angular scale, linear/angular threshold), max_angular_speed. 21 values.
pub const BODY_FIELDS: [(usize, usize); 7] = [(0x138, 3), (0x1b8, 4), (0x1d8, 3), (0x1f8, 3), (0x258, 3), (0x2c8, 4), (0x2e8, 1)];

/// The flags `dxStepBody` tests: finite rotation, its axis, linear / angular damping, max speed.
const STEP_FLAGS: [u32; 5] = [1, 2, 0x20, 0x40, 0x80];

/// One `dxStepBody` call on CSP's side and the port's: `v` = h then the 21 BODY_FIELDS values.
/// Returns pos, q, R, lvel, avel after the step (CSP's, port's).
unsafe fn step_both(f: extern "C" fn(*mut u64, f64), flags: u32, v: &[f64]) -> (Vec<f64>, Vec<f64>) {
    // the body's world chain ends at a byte that says "no geom moved callbacks" (+0x28 != 0)
    let mut chain = [0u64; 32];
    let p = chain.as_mut_ptr();
    let at = p as u64;
    (*p.add(20), *p.add(1), *p.add(5)) = (at, at, 1);
    let mut b = [0u64; 0x300 / 8];
    b[1] = at;
    b[0x38 / 8] = flags as u64;
    let mut k = 1;
    for &(offset, n) in &BODY_FIELDS {
        for i in 0..n {
            b[offset / 8 + i] = v[k].to_bits();
            k += 1;
        }
    }
    f(b.as_mut_ptr(), v[0]);
    let get = |offset: usize, n: usize| (0..n).map(move |i| f64::from_bits(b[offset / 8 + i]));
    let csp = get(0x138, 3).chain(get(0x1b8, 4)).chain(get(0x158, 12)).chain(get(0x1d8, 3)).chain(get(0x1f8, 3)).collect();
    let w = |i: usize| [v[i], v[i + 1], v[i + 2], 0.0];
    let mut p = Body::<f64> {
        flags,
        lvel: w(8),
        avel: w(11),
        finite_rot_axis: w(14),
        dampingp: Damping { linear_scale: v[17], angular_scale: v[18], linear_threshold: v[19], angular_threshold: v[20] },
        max_angular_speed: v[21],
        ..body(w(1), [v[4], v[5], v[6], v[7]], [0.0; 12])
    };
    step_body(&mut p, v[0]);
    let port = p.pos[..3].iter().chain(&p.q).chain(&p.r).chain(&p.lvel[..3]).chain(&p.avel[..3]).copied().collect();
    (csp, port)
}

fn body(pos: [f64; 4], q: Q, r: M3) -> Body<f64> {
    Body::<f64> {
        alive: true,
        next: None,
        tag: 0,
        first_joint: None,
        first_geom: None,
        flags: 0,
        mass: Mass::zero(),
        inv_i: [0.0; 12],
        inv_mass: 0.0,
        pos,
        r,
        q,
        lvel: [0.0; 4],
        avel: [0.0; 4],
        facc: [0.0; 4],
        tacc: [0.0; 4],
        finite_rot_axis: [0.0; 4],
        adis: AutoDisable { idle_time: 0.0, idle_steps: 0, linear_average_threshold: 0.0, angular_average_threshold: 0.0, average_samples: 0 },
        adis_timeleft: 0.0,
        adis_stepsleft: 0,
        average_lvel_buffer: Vec::new(),
        average_avel_buffer: Vec::new(),
        average_counter: 0,
        average_ready: false,
        dampingp: Damping { linear_scale: 0.0, angular_scale: 0.0, linear_threshold: 0.0, angular_threshold: 0.0 },
        max_angular_speed: 0.0,
    }
}

/// One ball (`fixed` false) or fixed joint's getInfo2 on CSP's side and the port's. `v` = pos0,
/// q0, pos1, q1 (7 each), anchor1 (the fixed joint's offset), anchor2, qrel, then fps, world
/// erp, the joint's erp and cfm: 28 values; `r` the two rotations. Returns per row the Jacobian
/// (J1 then J2: linear part, angular part, two padding values; the port's angular part sits in
/// column 4, CSP's in column 3), rhs and cfm (CSP's, port's).
unsafe fn joint_both(f: GetInfo2, fixed: bool, has_b1: bool, v: &[f64], r: [M3; 2]) -> (Vec<f64>, Vec<f64>) {
    let mut bodies = [[0u64; 0x300 / 8]; 2];
    let put = |m: &mut [u64], offset: usize, x: &[f64]| x.iter().enumerate().for_each(|(i, x)| m[offset / 8 + i] = x.to_bits());
    for (k, b) in bodies.iter_mut().enumerate() {
        put(b, 0x138, &v[7 * k..7 * k + 3]);
        put(b, 0x158, &r[k]);
        put(b, 0x1b8, &v[7 * k + 3..7 * k + 7]);
    }
    let mut j = [0u64; 0x140 / 8];
    j[0x40 / 8] = bodies[0].as_mut_ptr() as u64;
    j[0x58 / 8] = if has_b1 { bodies[1].as_mut_ptr() as u64 } else { 0 };
    let (a1, a2, qrel) = (&v[14..17], &v[17..20], &v[20..24]);
    let (fps, world_erp, erp, cfm) = (v[24], v[25], v[26], v[27]);
    if fixed {
        put(&mut j, 0xa0, qrel);
        put(&mut j, 0xc0, a1);
        put(&mut j, 0xe0, &[erp]);
        put(&mut j, 0xf8, &[cfm]);
        // -1: the joint as ODE's (CSP's own other path is part 4)
        put(&mut j, 0x110, &[-1.0]);
    } else {
        put(&mut j, 0xa0, a1);
        put(&mut j, 0xc0, a2);
        put(&mut j, 0xe0, &[erp, cfm]);
    }
    let rows = if fixed { 6 } else { 3 };
    let (mut j1, mut j2, mut rc, mut lohi, mut findex) = ([0f64; 48], [0f64; 48], [0f64; 12], [0f64; 12], [-1i32; 6]);
    f(j.as_mut_ptr(), fps, world_erp, 8, j1.as_mut_ptr(), j2.as_mut_ptr(), 2, rc.as_mut_ptr(), lohi.as_mut_ptr(), findex.as_mut_ptr());
    let mut csp = Vec::new();
    for row in 0..rows {
        csp.extend_from_slice(&j1[8 * row..8 * row + 8]);
        csp.extend_from_slice(&j2[8 * row..8 * row + 8]);
        csp.extend_from_slice(&rc[2 * row..2 * row + 2]);
    }

    let w = |i: usize| [v[i], v[i + 1], v[i + 2], 0.0];
    let q = |i: usize| [v[i], v[i + 1], v[i + 2], v[i + 3]];
    let (b0, b1) = (body(w(0), q(3), r[0]), body(w(7), q(10), r[1]));
    let b1 = has_b1.then_some(&b1);
    let (mut p1, mut p2, mut c, mut pc, mut lo, mut hi, mut pf) = ([0f64; 48], [0f64; 48], [0f64; 6], [0f64; 6], [0f64; 6], [0f64; 6], [-1i32; 6]);
    let mut info = Info2 { j1: &mut p1, j2: &mut p2, c: &mut c, cfm: &mut pc, lo: &mut lo, hi: &mut hi, findex: &mut pf };
    if fixed {
        fixed_get_info2(&b0, b1, fps, world_erp, &q(20), &w(14), erp, cfm, &mut info);
    } else {
        ball_get_info2(&b0, b1, fps, erp, cfm, &mut info, &w(14), &w(17));
    }
    let mut port = Vec::new();
    for row in 0..rows {
        for m in [&p1, &p2] {
            let x = &m[8 * row..8 * row + 8];
            port.extend_from_slice(&[x[0], x[1], x[2], x[4], x[5], x[6], x[3], x[7]]);
        }
        port.extend_from_slice(&[c[row], pc[row]]);
    }
    (csp, port)
}

/// One contact joint's getInfo1 + getInfo2 on CSP's side and the port's. `v` = pos, lvel, avel of
/// body 0 and body 1, contact pos, normal, depth, fdir1, mu, mu2, rho, rho2, rhoN, bounce,
/// bounce_vel, soft_erp, soft_cfm, motion1, motion2, motionN, slip1, slip2, then fps, world erp,
/// max_vel, min_depth: 46 values. Returns m, nub, the five coefficients after getInfo1, then per
/// row J1, J2 (as `joint_both`), rhs, cfm, lo, hi, findex.
unsafe fn contact_both(csp: &Csp, mode: u32, reverse: bool, has_b1: bool, v: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut bodies = [[0u64; 0x300 / 8]; 2];
    let put = |m: &mut [u64], offset: usize, x: &[f64]| x.iter().enumerate().for_each(|(i, x)| m[offset / 8 + i] = x.to_bits());
    for (k, b) in bodies.iter_mut().enumerate() {
        put(b, 0x138, &v[9 * k..9 * k + 3]);
        put(b, 0x1d8, &v[9 * k + 3..9 * k + 6]);
        put(b, 0x1f8, &v[9 * k + 6..9 * k + 9]);
    }
    let mut world = [0u64; 0x110 / 8];
    put(&mut world, 0xf8, &v[44..46]);
    let mut j = [0u64; 0x1a0 / 8];
    j[1] = world.as_mut_ptr() as u64;
    j[0x30 / 8] = if reverse { 2 } else { 0 };
    j[0x40 / 8] = bodies[0].as_mut_ptr() as u64;
    j[0x58 / 8] = if has_b1 { bodies[1].as_mut_ptr() as u64 } else { 0 };
    j[0xa8 / 8] = mode as u64;
    put(&mut j, 0xb0, &v[28..42]);
    put(&mut j, 0x120, &v[18..21]);
    put(&mut j, 0x140, &v[21..24]);
    put(&mut j, 0x160, &v[24..25]);
    put(&mut j, 0x180, &v[25..28]);
    let mut i1 = [0u8; 2];
    (csp.contact1)(j.as_mut_ptr(), i1.as_mut_ptr());
    let mut csp_out = vec![i1[0] as f64, i1[1] as f64];
    csp_out.extend((0..5).map(|k| f64::from_bits(j[0xb0 / 8 + k])));
    let (mut j1, mut j2, mut rc, mut lohi, mut findex) = ([0f64; 48], [0f64; 48], [1e-5f64; 12], [0f64; 12], [-1i32; 6]);
    for r in 0..6 {
        (rc[2 * r], lohi[2 * r], lohi[2 * r + 1]) = (0.0, -1e300, 1e300);
    }
    (csp.contact2)(j.as_mut_ptr(), v[42], v[43], 8, j1.as_mut_ptr(), j2.as_mut_ptr(), 2, rc.as_mut_ptr(), lohi.as_mut_ptr(), findex.as_mut_ptr());
    let rows = (i1[0] as usize).min(6);
    for row in 0..rows {
        csp_out.extend_from_slice(&j1[8 * row..8 * row + 8]);
        csp_out.extend_from_slice(&j2[8 * row..8 * row + 8]);
        csp_out.extend_from_slice(&[rc[2 * row], rc[2 * row + 1], lohi[2 * row], lohi[2 * row + 1], findex[row] as f64]);
    }

    let w = |i: usize| [v[i], v[i + 1], v[i + 2], 0.0];
    let mk = |k: usize| Body::<f64> { lvel: w(9 * k + 3), avel: w(9 * k + 6), ..body(w(9 * k), [1.0, 0.0, 0.0, 0.0], [0.0; 12]) };
    let (b0, b1) = (mk(0), mk(1));
    let mut c = Contact {
        mode,
        mu: v[28],
        mu2: v[29],
        rho: v[30],
        rho2: v[31],
        rho_n: v[32],
        bounce: v[33],
        bounce_vel: v[34],
        soft_erp: v[35],
        soft_cfm: v[36],
        motion1: v[37],
        motion2: v[38],
        motion_n: v[39],
        slip1: v[40],
        slip2: v[41],
        pos: w(18),
        normal: w(21),
        depth: v[24],
        fdir1: w(25),
    };
    let (m, nub) = contact::get_info1(&mut c);
    let mut port = vec![m as f64, nub as f64, c.mu, c.mu2, c.rho, c.rho2, c.rho_n];
    let (mut p1, mut p2, mut pc, mut pcfm, mut lo, mut hi, mut pf) = ([0f64; 48], [0f64; 48], [0f64; 6], [1e-5f64; 6], [-1e300f64; 6], [1e300f64; 6], [-1i32; 6]);
    let mut info = Info2 { j1: &mut p1, j2: &mut p2, c: &mut pc, cfm: &mut pcfm, lo: &mut lo, hi: &mut hi, findex: &mut pf };
    contact::get_info2(&c, m, reverse, &b0, has_b1.then_some(&b1), v[42], v[43], v[44], v[45], &mut info);
    for row in 0..(m as usize).min(6) {
        for m in [&p1, &p2] {
            let x = &m[8 * row..8 * row + 8];
            port.extend_from_slice(&[x[0], x[1], x[2], x[4], x[5], x[6], x[3], x[7]]);
        }
        port.extend_from_slice(&[pc[row], pcfm[row], lo[row], hi[row], pf[row] as f64]);
    }
    (csp_out, port)
}

#[derive(Default)]
pub(crate) struct Count {
    pub(crate) n: u64,
    pub(crate) differ: u64,
    pub(crate) nan: u64,
    pub(crate) first: Vec<String>,
}

impl Count {
    pub(crate) fn check(&mut self, what: &dyn Fn() -> String, csp: &[f64], port: &[f64]) {
        self.n += 1;
        let mut nan = false;
        for (a, b) in csp.iter().zip(port) {
            if a.is_nan() && b.is_nan() {
                nan |= a.to_bits() != b.to_bits();
            } else if a.to_bits() != b.to_bits() {
                self.differ += 1;
                if self.first.len() < 3 {
                    self.first.push(format!("{} csp {csp:?} port {port:?}", what()));
                }
                return;
            }
        }
        self.nan += nan as u64;
    }
}

pub(crate) fn edge() -> Vec<f64> {
    vec![
        0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, 3.0, 1e-300, -1e-300, 1e300, -1e300, 1e-160, 1e160,
        f64::from_bits(1), -f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX, f64::MIN, f64::INFINITY,
        f64::NEG_INFINITY, f64::NAN, -f64::NAN, f64::EPSILON, std::f64::consts::FRAC_1_SQRT_2,
    ]
}

/// The doubles a breakpoint dump logged for `rva` (`CSP_ORACLE_BREAK=<rva>d`): 12 from field 8 on.
fn dumps(log: &str, rva: &str) -> Vec<Vec<f64>> {
    log.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 20 && fields[2] == rva).then(|| fields[8..].iter().map(|v| v.parse::<f64>().expect("a dumped double")).collect())
        })
        .collect()
}

pub unsafe fn run(dll: usize, breaks: &str) {
    type F3 = extern "C" fn(*mut f64, *const f64, *const f64);
    type F2 = extern "C" fn(*mut f64, *const f64);
    let f3 = |rva: usize| std::mem::transmute::<usize, F3>(dll + rva);
    let f2 = |rva: usize| std::mem::transmute::<usize, F2>(dll + rva);
    let csp = Csp {
        q_multiply0: f3(0x168_b680),
        q_multiply1: f3(0x168_b7b0),
        q_multiply3: f3(0x168_b8a0),
        r_from_q: f2(0x168_b990),
        q_from_r: f2(0x168_bae0),
        dq_from_w: f3(0x168_bca0),
        box_total: std::mem::transmute::<usize, extern "C" fn(*mut f64, f64, f64, f64, f64)>(dll + 0x168_d670),
        safe_normalize3: std::mem::transmute::<usize, extern "C" fn(*mut f64) -> bool>(dll + 0x168_aed0),
        step_body: std::mem::transmute::<usize, extern "C" fn(*mut u64, f64)>(dll + 0x16c_61b0),
        ball: std::mem::transmute::<usize, GetInfo2>(dll + 0x16a_3ba0),
        fixed: std::mem::transmute::<usize, GetInfo2>(dll + 0x16a_0fd0),
        contact1: std::mem::transmute::<usize, extern "C" fn(*mut u64, *mut u8)>(dll + 0x16c_79d0),
        contact2: std::mem::transmute::<usize, GetInfo2>(dll + 0x16c_7b30),
    };
    let log = std::fs::read_to_string(breaks).expect("--ode takes the breaks.txt of a drive");
    let q4 = |v: &Vec<f64>| -> Q { [v[0], v[1], v[2], v[3]] };
    let quats: Vec<Q> = dumps(&log, "0x168b990").iter().chain(&dumps(&log, "0x168b7b0")).map(q4).collect();
    let omegas: Vec<Q> = dumps(&log, "0x168bca0").iter().map(q4).collect();
    assert!(quats.len() >= 600 && omegas.len() >= 600, "breaks.txt has {} quaternions, {} angular velocities", quats.len(), omegas.len());
    // the drive's bodies (`CSP_ORACLE_BREAK=16c61b0b`): flags, then h and the 21 BODY_FIELDS
    let bodies: Vec<(u32, Vec<f64>)> = log
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            (f.len() == 31 && f[2] == "0x16c61b0").then(|| (f[9].parse().expect("flags"), std::iter::once(f[8]).chain(f[10..].iter().copied()).map(|v| v.parse::<f64>().expect("a dumped double")).collect()))
        })
        .collect();
    assert!(bodies.len() >= 600, "breaks.txt has {} dxStepBody calls", bodies.len());
    println!("ode: {} quaternions, {} angular velocities, {} bodies from {breaks}", quats.len(), omegas.len(), bodies.len());

    // the inputs: the drive's, every combination slot by slot from the edge values, random bits,
    // random values in [-1, 1]
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let e = edge();
    let mut sets: Vec<(&str, Vec<[f64; 8]>)> = Vec::new();
    let drive: Vec<[f64; 8]> = (0..quats.len())
        .map(|i| {
            let (a, b, w) = (quats[i], quats[(i * 7 + 1) % quats.len()], omegas[i % omegas.len()]);
            let pick = if i < omegas.len() { [w[0], w[1], w[2], w[3]] } else { b };
            [a[0], a[1], a[2], a[3], pick[0], pick[1], pick[2], pick[3]]
        })
        .collect();
    sets.push(("drive", drive));
    sets.push(("edge", (0..400_000).map(|_| std::array::from_fn(|_| e[(next() % e.len() as u64) as usize])).collect()));
    sets.push(("bits", (0..400_000).map(|_| std::array::from_fn(|_| f64::from_bits(next()))).collect()));
    sets.push(("unit", (0..400_000).map(|_| std::array::from_fn(|_| (next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0)).collect()));

    for csr in [0x1f80u32, 0x9fc0] {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        let mut counts: Vec<(&str, Count)> = ["q_multiply0", "q_multiply1", "q_multiply3", "r_from_q", "q_from_r", "dq_from_w", "box_total", "safe_normalize3", "step_body", "ball", "fixed", "contact"].map(|n| (n, Count::default())).into();
        for (set, inputs) in &sets {
            for (i, x) in inputs.iter().enumerate() {
                let what = || format!("{set} #{i} {x:?}");
                let (a, b): (Q, Q) = ([x[0], x[1], x[2], x[3]], [x[4], x[5], x[6], x[7]]);
                let mut out = [0f64; 12];
                for (k, (csp_fn, port)) in [(csp.q_multiply0, rot::q_multiply0::<f64> as fn(&Q, &Q) -> Q), (csp.q_multiply1, rot::q_multiply1), (csp.q_multiply3, rot::q_multiply3)].into_iter().enumerate() {
                    csp_fn(out.as_mut_ptr(), a.as_ptr(), b.as_ptr());
                    counts[k].1.check(&what, &out[..4], &port(&a, &b));
                }
                (csp.r_from_q)(out.as_mut_ptr(), a.as_ptr());
                let mut r: M3 = [0.0; 12];
                rot::r_from_q(&mut r, &a);
                counts[3].1.check(&what, &out, &r);
                // q_from_r on the drive's rotations and on the input's eight values spread over a
                // matrix (diagonal from the first three, so every branch is reached)
                let spread: M3 = [x[0], x[3], x[4], 0.0, x[5], x[1], x[6], 0.0, x[7], x[4] - x[3], x[2], 0.0];
                for m in [r, spread] {
                    (csp.q_from_r)(out.as_mut_ptr(), m.as_ptr());
                    counts[4].1.check(&what, &out[..4], &rot::q_from_r(&m));
                }
                (csp.dq_from_w)(out.as_mut_ptr(), b.as_ptr(), a.as_ptr());
                counts[5].1.check(&what, &out[..4], &rot::dq_from_w(&b, &a));
                let mut mass = [f64::from_bits(0x7ff4_dead_beef_0001); 17];
                (csp.box_total)(mass.as_mut_ptr(), x[0], x[1], x[2], x[3]);
                let m = Mass::<f64>::box_total(x[0], x[1], x[2], x[3]);
                let mut port = [m.mass; 17];
                port[1..5].copy_from_slice(&m.c);
                port[5..].copy_from_slice(&m.i);
                counts[6].1.check(&what, &mass, &port);
                // both halves of the input, the return value as a fourth value
                for v in [[x[0], x[1], x[2]], [x[4], x[5], x[6]]] {
                    let (mut c, mut p) = ([v[0], v[1], v[2], 0.0], [v[0], v[1], v[2], 0.0]);
                    c[3] = (csp.safe_normalize3)(c.as_mut_ptr()) as u8 as f64;
                    p[3] = odemath::safe_normalize3(&mut p) as u8 as f64;
                    counts[7].1.check(&what, &c, &p);
                }
            }
        }
        // step_body: the drive's bodies with every flag combination, then each set's inputs
        // three at a time (22 values) with flag combination i % 32
        let combo = |i: usize| STEP_FLAGS.iter().enumerate().filter(|(bit, _)| i >> bit & 1 != 0).fold(0, |a, (_, f)| a | f);
        for (i, (flags, v)) in bodies.iter().enumerate() {
            for c in 0..32 {
                let (csp, port) = step_both(csp.step_body, if c == 0 { *flags } else { combo(c) }, v);
                counts[8].1.check(&|| format!("drive body #{i} flags {c} {v:?}"), &csp, &port);
            }
        }
        for (set, inputs) in &sets {
            for (i, x) in inputs.windows(3).enumerate() {
                let v: Vec<f64> = x.concat()[..22].to_vec();
                let (csp, port) = step_both(csp.step_body, combo(i), &v);
                counts[8].1.check(&|| format!("{set} body #{i} {v:?}"), &csp, &port);
            }
        }
        // ball and fixed joints: two of the drive's bodies (rotation from their quaternion), a
        // third one's angular velocity and quaternion as anchors and qrel, at 333 Hz; then each
        // set's inputs four at a time, rotations from the quaternions and as raw values in turn.
        // Every fourth joint has no second body.
        let joints = |counts: &mut Vec<(&str, Count)>, i: usize, v: &[f64], raw: bool| {
            let r = |k: usize| -> M3 {
                if raw {
                    std::array::from_fn(|n| v[(12 * k + n) % 28])
                } else {
                    let mut m = [0.0; 12];
                    rot::r_from_q(&mut m, &[v[7 * k + 3], v[7 * k + 4], v[7 * k + 5], v[7 * k + 6]]);
                    m
                }
            };
            for (n, fixed) in [(9, false), (10, true)] {
                let (csp_fn, what) = (if fixed { csp.fixed } else { csp.ball }, || format!("joint #{i} raw {raw} {v:?}"));
                let (c, p) = joint_both(csp_fn, fixed, i % 4 != 3, v, [r(0), r(1)]);
                counts[n].1.check(&what, &c, &p);
            }
        };
        for i in 0..bodies.len() {
            let (a, b, c) = (&bodies[i].1, &bodies[(i * 7 + 1) % bodies.len()].1, &bodies[(i * 13 + 2) % bodies.len()].1);
            let v: Vec<f64> = a[1..8].iter().chain(&b[1..8]).chain(&c[11..14]).chain(&b[8..11]).chain(&c[4..8]).chain(&[1.0 / a[0], 0.2, 0.3, 1e-6]).copied().collect();
            joints(&mut counts, i, &v, false);
        }
        for (_, inputs) in &sets {
            for (i, x) in inputs.windows(4).enumerate() {
                joints(&mut counts, i, &x.concat()[..28], i % 2 == 1);
            }
        }
        // contact joints: two of the drive's bodies, a third one's position and rotation columns as
        // contact point, normal and fdir1, set coefficients; then each set's inputs six at a time.
        // Modes from every surface bit (CSP's car contacts | 0xe2 on every other one), every third
        // joint reversed, every fourth without a second body.
        let mode = |i: usize| {
            let m = ((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) as u32 & 0x77ff;
            if i.is_multiple_of(2) { m | 0xe2 } else { m }
        };
        for i in 0..bodies.len() {
            let (a, b, c) = (&bodies[i].1, &bodies[(i * 7 + 1) % bodies.len()].1, &bodies[(i * 13 + 2) % bodies.len()].1);
            let mut r = [0.0; 12];
            rot::r_from_q(&mut r, &[c[4], c[5], c[6], c[7]]);
            let coeffs = [1.2, 0.9, 0.01, 0.02, 0.005, 0.3, 0.1, 0.2, 1e-5, 0.5, -0.5, 0.1, 1e-4, 2e-4, 1.0 / a[0], 0.2, 1e300, 0.0];
            let v: Vec<f64> = a[1..4].iter().chain(&a[8..14]).chain(&b[1..4]).chain(&b[8..14]).chain(&c[1..4]).chain(&[r[0], r[4], r[8], (c[8] * 0.01).abs(), r[1], r[5], r[9]]).chain(&coeffs).copied().collect();
            let (cs, p) = contact_both(&csp, mode(i), i % 3 == 2, i % 4 != 3, &v);
            counts[11].1.check(&|| format!("drive contact #{i} mode {:#x} {v:?}", mode(i)), &cs, &p);
        }
        for (set, inputs) in &sets {
            for (i, x) in inputs.windows(6).enumerate() {
                let v = &x.concat()[..46];
                let (cs, p) = contact_both(&csp, mode(i), i % 3 == 2, i % 4 != 3, v);
                counts[11].1.check(&|| format!("{set} contact #{i} mode {:#x} {v:?}", mode(i)), &cs, &p);
            }
        }
        for (name, c) in &counts {
            println!("ode {name} mxcsr {csr:#06x}: {} inputs, {} differ, {} NaN against NaN {:?}", c.n, c.differ, c.nan, c.first);
        }
    }
}
