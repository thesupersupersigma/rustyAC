// SPDX-License-Identifier: GPL-3.0-or-later

//! The suspension micro-oracle: the game's own suspension objects of one car (whichever of
//! `Suspension`, `SuspensionStrut`, `SuspensionAxle`, `SuspensionML` its corners are) against
//! the port's, one virtual call at a time on random states.
//!
//! Before every sample both sides are put into the same state, bit for bit: the pose and
//! velocity of every body (written straight into ODE's body on the game's side and into the
//! port's body), the numbers a setup can change (spring, packer, bump stops, rod length, toe,
//! camber, the damper). Then one function of one wheel's suspension is called on both, and
//! everything it can touch is compared: what it returns, the calls it made on the bodies with
//! the force accumulators after each, travel / damper speed / steer torque, every joint's
//! anchors and every body. Every eighth sample one `dWorldStep` follows on both sides, so a
//! wrong joint also shows as motion.

use rustyac_physics::car::replay::Ground;
use rustyac_physics::car::{ChassisEnvironment, RollingChassis};
use rustyac_physics::vecmath::Vec3f;
use rustyac_physics::ode::JointKind;

use super::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// A value of the range, or 0 with probability `p`.
    fn or_zero(&mut self, p: f32, lo: f32, hi: f32) -> f32 {
        let value = self.range(lo, hi);
        if self.chance(p) {
            0.0
        } else {
            value
        }
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn vector(&mut self, size: f32) -> V3 {
        [self.range(-size, size), self.range(-size, size), self.range(-size, size)]
    }
}

type Quat = [f32; 4];

fn quat_mul(a: &Quat, b: &Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn quat_unit(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

fn quat_conjugate(q: &Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

/// ODE's 3x4 rotation matrix of a quaternion (the layout of `dRfromQ`).
fn matrix_of(q: &Quat) -> [f32; 12] {
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - w * z),
        2.0 * (x * z + w * y),
        0.0,
        2.0 * (x * y + w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - w * x),
        0.0,
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
    ]
}

fn rotate(r: &[f32; 12], v: &V3) -> V3 {
    [r[0] * v[0] + r[1] * v[1] + r[2] * v[2], r[4] * v[0] + r[5] * v[1] + r[6] * v[2], r[8] * v[0] + r[9] * v[1] + r[10] * v[2]]
}

fn rotate_back(r: &[f32; 12], v: &V3) -> V3 {
    [r[0] * v[0] + r[4] * v[1] + r[8] * v[2], r[1] * v[0] + r[5] * v[1] + r[9] * v[2], r[2] * v[0] + r[6] * v[1] + r[10] * v[2]]
}

/// A body's state as both sides get it.
#[derive(Clone, Copy)]
struct Pose {
    pos: V3,
    q: Quat,
    r: [f32; 12],
    lvel: V3,
    avel: V3,
}

/// The type-specific numbers of a joint of the port, in the order of the game-side tables
/// (`joint_params`).
fn port_joint_params(kind: &JointKind) -> Vec<f32> {
    match kind {
        JointKind::Fixed { qrel, offset, erp, cfm } => vec![qrel[0], qrel[1], qrel[2], qrel[3], offset[0], offset[1], offset[2], *erp, *cfm],
        JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
            vec![anchor1[0], anchor1[1], anchor1[2], anchor2[0], anchor2[1], anchor2[2], *erp, *cfm, *target_distance]
        }
        JointKind::Ball { anchor1, anchor2, erp, cfm } => vec![anchor1[0], anchor1[1], anchor1[2], anchor2[0], anchor2[1], anchor2[2], *erp, *cfm],
        JointKind::Slider { axis1, qrel, offset, limot } => {
            vec![axis1[0], axis1[1], axis1[2], qrel[0], qrel[1], qrel[2], qrel[3], offset[0], offset[1], offset[2], limot.normal_cfm]
        }
        JointKind::Contact { .. } => Vec::new(),
    }
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn differ(what: &str, game: &[f32], port: &[f32]) -> Option<String> {
    if game.len() != port.len() {
        return Some(format!("{what}: the game has {} values, the port {}", game.len(), port.len()));
    }
    game.iter()
        .zip(port)
        .position(|(g, p)| !same(*g, *p))
        .map(|k| format!("{what}[{k}]: game {:?} ({:#010x}) / port {:?} ({:#010x})", game[k], game[k].to_bits(), port[k], port[k].to_bits()))
}

const OPS: [&str; 21] = [
    "step",
    "step",
    "step",
    "addForceAtPos",
    "addForceAtPos (steer torque)",
    "addTorque",
    "addLocalForceAndTorque",
    "setSteerLengthOffset",
    "setDamage",
    "resetDamage",
    "setERPCFM",
    "attach",
    "stop",
    "getHubWorldMatrix",
    "getPointVelocity",
    "getHubAngularVelocity",
    "getVelocity",
    "getSteerBasis",
    "getBasePosition",
    "getMass / getDamage / getSteerTorque / getK / getPackerRange",
    "dWorldStep after a step of all four",
];

/// What the micro-oracle found.
pub struct MicroReport {
    pub text: String,
    pub ok: bool,
}

impl World<'_> {
    /// Runs `count` samples. `data` is the car's data folder (`cardata/<car>`), `seed` the
    /// C runtime's seed the game's car was built with (it decides the damage directions).
    pub fn sus_micro(&mut self, data: &std::path::Path, car_seed: u32, count: usize, seed: u64) -> Result<MicroReport, String> {
        let st = state();
        let mut chassis = RollingChassis::new(data, ChassisEnvironment::default(), Box::new(Ground::Flat), car_seed, CLOCK_START_MS)?;
        chassis.core.tape = Some(Vec::new());
        let port_bodies: Vec<_> = chassis.core.bodies().collect();
        if port_bodies.len() != st.bodies.len() {
            return Err(format!("the game's car has {} bodies, the port's {}", st.bodies.len(), port_bodies.len()));
        }
        let layout = rustyac_physics::car::replay::CarLayout::of(&chassis);
        let port_joints = rustyac_physics::car::replay::CarLayout::joint_ids(&chassis);
        let game_names: Vec<&str> = st.joints.iter().map(|j| j.name.as_str()).collect();
        let port_names: Vec<&str> = layout.joints.iter().map(|(name, _)| name.as_str()).collect();
        if game_names != port_names {
            return Err(format!("joints: the game has {game_names:?}, the port {port_names:?}"));
        }
        unsafe {
            // the design pose of every body relative to the car body, from the port's car as built
            let body0 = chassis.core.world.body(port_bodies[0].id).clone();
            let design: Vec<(V3, Quat)> = port_bodies
                .iter()
                .map(|b| {
                    let b = chassis.core.world.body(b.id);
                    let d = [b.pos[0] - body0.pos[0], b.pos[1] - body0.pos[1], b.pos[2] - body0.pos[2]];
                    (rotate_back(&body0.r, &d), quat_mul(&quat_conjugate(&body0.q), &b.q))
                })
                .collect();
            let world: *mut u8 = rd(st.bodies[0].ode, 0x8);
            let world_step: extern "C" fn(*mut u8, f32) -> i32 = std::mem::transmute(st.world_step_original);

            let mut rng = Rng(seed);
            let fault = std::env::var_os("SUS_MICRO_FAULT").is_some();
            let mut samples = [0usize; OPS.len()];
            let mut failures = [0usize; OPS.len()];
            let mut first: Vec<Option<String>> = vec![None; OPS.len()];
            let classes: Vec<&SusClass> = (0..4).map(|w| &SUS_CLASSES[st.suspension_classes[w]]).collect();
            for sample in 0..count {
                // --- the same state on both sides ---
                let far = rng.chance(0.1);
                let mut poses = Vec::with_capacity(design.len());
                let q0 = quat_unit([rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)]);
                let car = Pose {
                    pos: [rng.range(-200.0, 200.0), rng.range(0.0, 2.0), rng.range(-200.0, 200.0)],
                    q: q0,
                    r: matrix_of(&q0),
                    lvel: rng.vector(40.0),
                    avel: rng.vector(2.0),
                };
                poses.push(car);
                for (rel, rel_q) in design.iter().skip(1) {
                    let reach = if rng.chance(0.05) { 0.0 } else if far { 0.4 } else { 0.12 };
                    let offset = rng.vector(reach);
                    let local = [rel[0] + offset[0], rel[1] + offset[1], rel[2] + offset[2]];
                    let world_offset = rotate(&car.r, &local);
                    let tilt = if reach == 0.0 { 0.0 } else { 0.05 };
                    let twist = quat_unit([1.0, rng.range(-tilt, tilt), rng.range(-tilt, tilt), rng.range(-tilt, tilt)]);
                    let q = quat_unit(quat_mul(&quat_mul(&car.q, rel_q), &twist));
                    let v = rng.vector(3.0);
                    poses.push(Pose {
                        pos: [car.pos[0] + world_offset[0], car.pos[1] + world_offset[1], car.pos[2] + world_offset[2]],
                        q,
                        r: matrix_of(&q),
                        lvel: [car.lvel[0] + v[0], car.lvel[1] + v[1], car.lvel[2] + v[2]],
                        avel: rng.vector(5.0),
                    });
                }
                for ((pose, game), port) in poses.iter().zip(&st.bodies).zip(&port_bodies) {
                    wr(game.ode, B_POS, pose.pos);
                    wr(game.ode, B_R, pose.r);
                    wr(game.ode, B_Q, pose.q);
                    wr(game.ode, B_LVEL, pose.lvel);
                    wr(game.ode, B_AVEL, pose.avel);
                    wr(game.ode, B_FACC, [0.0f32; 3]);
                    wr(game.ode, B_TACC, [0.0f32; 3]);
                    let b = chassis.core.world.body_mut(port.id);
                    b.pos[..3].copy_from_slice(&pose.pos);
                    b.r = pose.r;
                    b.q = pose.q;
                    b.lvel[..3].copy_from_slice(&pose.lvel);
                    b.avel[..3].copy_from_slice(&pose.avel);
                    b.facc = [0.0; 4];
                    b.tacc = [0.0; 4];
                }
                // --- the numbers a setup can change ---
                for w in 0..4 {
                    let values = [
                        rng.or_zero(0.05, 20_000.0, 300_000.0), // k
                        rng.or_zero(0.5, -1.0e6, 2.0e6),        // progressiveK
                        rng.range(1.0e5, 1.0e6),                // bumpStopRate
                        rng.or_zero(0.6, 0.0, 1.0e7),           // bumpStopProgressive
                        rng.range(-0.1, 0.1),                   // staticCamber
                        rng.or_zero(0.2, 0.005, 0.15),          // bumpStopUp
                        rng.or_zero(0.2, -0.15, -0.005),        // bumpStopDn
                        rng.range(-0.1, 0.1),                   // rodLength
                        rng.range(-0.01, 0.01),                 // toeOUT_Linear
                        rng.or_zero(0.3, 0.005, 0.1),           // packerRange
                    ];
                    let s = st.suspensions[w];
                    for (i, value) in values.iter().enumerate() {
                        wr(s, 0x08 + i * 4, *value);
                    }
                    let base = chassis.suspensions[w].base_mut();
                    base.k = values[0];
                    base.progressive_k = values[1];
                    base.bump_stop_rate = values[2];
                    base.bump_stop_progressive = values[3];
                    base.static_camber = values[4];
                    base.bump_stop_up = values[5];
                    base.bump_stop_dn = values[6];
                    base.rod_length = values[7];
                    base.toe_out_linear = values[8];
                    base.packer_range = values[9];
                    if fault {
                        // the proof that the comparison can fail: the port's spring one bit off
                        base.k = f32::from_bits(base.k.to_bits() ^ 1);
                    }
                    // reboundSlow, reboundFast, bumpSlow, bumpFast, the two thresholds
                    let damper = [
                        rng.range(500.0, 10_000.0),
                        rng.range(200.0, 5_000.0),
                        rng.range(500.0, 10_000.0),
                        rng.range(200.0, 5_000.0),
                        rng.or_zero(0.1, 0.02, 0.5),
                        rng.or_zero(0.1, 0.02, 0.5),
                    ];
                    let get_damper: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(*st.suspension_vtables[w].add(12));
                    let d = get_damper(s);
                    for (i, value) in damper.iter().enumerate() {
                        wr(d, i * 4, *value);
                    }
                    let port = chassis.suspensions[w].damper_mut();
                    port.rebound_slow = damper[0];
                    port.rebound_fast = damper[1];
                    port.bump_slow = damper[2];
                    port.bump_fast = damper[3];
                    port.fast_threshold_bump = damper[4];
                    port.fast_threshold_rebound = damper[5];
                }

                // --- one call ---
                let w = rng.below(4);
                let mut op = if sample % 8 == 7 { OPS.len() - 1 } else { rng.below(OPS.len() - 1) };
                let axle = classes[w].name == "AXLE";
                if op == 17 && axle {
                    op = 0; // an axle's getSteerBasis raises an exception on purpose
                }
                st.tape.clear();
                st.outer_site = 0;
                chassis.core.tape = Some(Vec::new());
                let s = st.suspensions[w];
                let vt = st.suspension_vtables[w];
                let hub_pos = poses[1.min(poses.len() - 1)].pos;
                let mut game_out: Vec<f32> = Vec::new();
                let mut port_out: Vec<f32> = Vec::new();
                let v = |a: V3| Vec3f::new(a[0], a[1], a[2]);
                let mut check_systems = false;
                {
                    let core = &mut chassis.core;
                    let sus = chassis.suspensions[w].as_mut();
                    match op {
                        0..=2 => {
                            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(*vt.add(22));
                            f(s, 0.003);
                            sus.step(core, 0.003);
                            check_systems = true;
                        }
                        3 | 4 => {
                            let (force, at) = (rng.vector(20_000.0), rng.vector(0.4));
                            let pos = [hub_pos[0] + at[0], hub_pos[1] + at[1], hub_pos[2] + at[2]];
                            let (driven, steer) = (rng.chance(0.5), op == 4);
                            let f: extern "C" fn(*mut u8, *const V3, *const V3, usize, usize) = std::mem::transmute(*vt.add(3));
                            f(s, &force, &pos, driven as usize, steer as usize);
                            sus.add_force_at_pos(core, &v(force), &v(pos), driven, steer);
                        }
                        5 => {
                            let torque = rng.vector(3_000.0);
                            let f: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(*vt.add(4));
                            f(s, &torque);
                            sus.add_torque(core, &v(torque));
                        }
                        6 => {
                            let (force, torque) = (rng.vector(20_000.0), rng.vector(3_000.0));
                            let drive = if rng.chance(0.5) { [0.0; 3] } else { rng.vector(2_000.0) };
                            let f: extern "C" fn(*mut u8, *const V3, *const V3, *const V3) = std::mem::transmute(*vt.add(24));
                            f(s, &force, &torque, &drive);
                            sus.add_local_force_and_torque(core, &v(force), &v(torque), &v(drive));
                        }
                        7 => {
                            let offset = if rng.chance(0.1) { 0.0 } else { rng.range(-0.03, 0.03) };
                            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(*vt.add(5));
                            f(s, offset);
                            sus.set_steer_length_offset(core, offset);
                        }
                        8 => {
                            let amount = rng.range(0.0, 250.0);
                            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(*vt.add(15));
                            f(s, amount);
                            sus.set_damage(amount);
                        }
                        9 => {
                            let f: extern "C" fn(*mut u8) = std::mem::transmute(*vt.add(16));
                            f(s);
                            sus.reset_damage();
                        }
                        10 => {
                            let erp = if rng.chance(0.1) { rng.range(-1.0, 0.0) } else { rng.range(0.05, 0.95) };
                            let cfm = if rng.chance(0.1) { -1.0 } else { rng.range(1.0e-9, 1.0e-4) };
                            let f: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(*vt.add(23));
                            f(s, erp, cfm);
                            sus.set_erp_cfm(core, erp, cfm);
                        }
                        11 => {
                            let f: extern "C" fn(*mut u8) = std::mem::transmute(*vt.add(8));
                            f(s);
                            sus.attach(core);
                        }
                        12 => {
                            let f: extern "C" fn(*mut u8) = std::mem::transmute(*vt.add(19));
                            f(s);
                            sus.stop(core);
                        }
                        13 => {
                            let f: extern "C" fn(*mut u8, *mut [f32; 16]) -> *mut [f32; 16] = std::mem::transmute(*vt.add(1));
                            let mut m = [0.0f32; 16];
                            f(s, &mut m);
                            game_out.extend(m);
                            port_out.extend(sus.get_hub_world_matrix(core).m.iter().flatten());
                        }
                        14 => {
                            let at = rng.vector(0.5);
                            let pos = [hub_pos[0] + at[0], hub_pos[1] + at[1], hub_pos[2] + at[2]];
                            let f: extern "C" fn(*mut u8, *mut V3, *const V3) -> *mut V3 = std::mem::transmute(*vt.add(2));
                            let mut out = [0.0f32; 3];
                            f(s, &mut out, &pos);
                            game_out.extend(out);
                            let p = sus.get_point_velocity(core, &v(pos));
                            port_out.extend([p.x, p.y, p.z]);
                        }
                        15 | 16 | 18 => {
                            let slot = match op {
                                15 => 7,
                                16 => 20,
                                _ => 10,
                            };
                            let f: extern "C" fn(*mut u8, *mut V3) -> *mut V3 = std::mem::transmute(*vt.add(slot));
                            let mut out = [0.0f32; 3];
                            f(s, &mut out);
                            game_out.extend(out);
                            let p = match op {
                                15 => sus.get_hub_angular_velocity(core),
                                16 => sus.get_velocity(core),
                                _ => sus.get_base_position(),
                            };
                            port_out.extend([p.x, p.y, p.z]);
                        }
                        17 => {
                            let f: extern "C" fn(*mut u8, *mut V3, *mut V3) = std::mem::transmute(*vt.add(21));
                            let (mut centre, mut axis) = ([0.0f32; 3], [0.0f32; 3]);
                            f(s, &mut centre, &mut axis);
                            game_out.extend(centre);
                            game_out.extend(axis);
                            let (c, a) = sus.get_steer_basis(core);
                            port_out.extend([c.x, c.y, c.z, a.x, a.y, a.z]);
                        }
                        19 => {
                            for slot in [18usize, 17, 6, 11, 13] {
                                let f: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*vt.add(slot));
                                game_out.push(f(s));
                            }
                            port_out.extend([sus.get_mass(core), sus.get_damage(), sus.get_steer_torque(), sus.get_k(), sus.get_packer_range()]);
                        }
                        _ => {}
                    }
                }
                if op == OPS.len() - 1 {
                    // all four suspensions push, then the solver moves the bodies
                    for k in 0..4 {
                        let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(*st.suspension_vtables[k].add(22));
                        f(st.suspensions[k], 0.003);
                        let core = &mut chassis.core;
                        chassis.suspensions[k].step(core, 0.003);
                    }
                    world_step(world, 0.003);
                    chassis.core.world_step(0.003);
                    check_systems = true;
                }

                // --- compare ---
                let mut found: Option<String> = differ("the returned values", &game_out, &port_out);
                let game_tape: Vec<&Call> = st.tape.iter().filter(|call| call.kind <= 8).collect();
                let port_tape = chassis.core.tape.clone().unwrap_or_default();
                if found.is_none() && game_tape.len() != port_tape.len() {
                    found = Some(format!("the game made {} force calls, the port {}", game_tape.len(), port_tape.len()));
                }
                for (n, (g, p)) in game_tape.iter().zip(&port_tape).enumerate() {
                    if found.is_some() {
                        break;
                    }
                    if g.body != p.body || g.kind != p.kind {
                        found = Some(format!("force call {n}: the game made kind {} on body {}, the port kind {} on body {}", g.kind, g.body, p.kind, p.body));
                        break;
                    }
                    let system = crate::sites::system_of(if g.outer_site != 0 { g.outer_site } else { g.site });
                    if check_systems && system != p.source.name() {
                        found = Some(format!("force call {n}: the game's is a {system} call, the port books it under {}", p.source.name()));
                        break;
                    }
                    let vectors = [("a", g.a, p.a), ("b", g.b, p.b), ("facc", g.facc, p.facc), ("tacc", g.tacc, p.tacc)];
                    for (name, gv, pv) in &vectors[if g.kind == 8 { 2 } else { 0 }..] {
                        if found.is_none() {
                            found = differ(&format!("force call {n} (kind {} on body {}) {name}", g.kind, g.body), gv, pv);
                        }
                    }
                }
                for k in 0..4 {
                    if found.is_some() {
                        break;
                    }
                    let class = classes[k];
                    let g = [
                        rd::<f32>(st.suspensions[k], class.status),
                        rd::<f32>(st.suspensions[k], class.status + 4),
                        class.steer_torque.map_or(0.0, |offset| rd(st.suspensions[k], offset)),
                    ];
                    let status = chassis.suspensions[k].get_status();
                    let p = [status.travel, status.damper_speed_ms, chassis.suspensions[k].get_steer_torque()];
                    found = differ(&format!("{} travel / damper speed / steer torque", WHEELS[k]), &g, &p);
                }
                for (j, joint) in st.joints.iter().enumerate() {
                    if found.is_some() {
                        break;
                    }
                    let g: Vec<f32> = joint_params(joint.kind).iter().map(|(_, offset)| rd::<f32>(joint.ode, J_PARAMS + offset)).collect();
                    let p = port_joint_params(&chassis.core.world.joint(port_joints[j]).kind);
                    found = differ(&format!("joint {}", joint.name), &g, &p);
                }
                for (game, port) in st.bodies.iter().zip(&port_bodies) {
                    if found.is_some() {
                        break;
                    }
                    let g = body_state(game.ode);
                    let b = chassis.core.world.body(port.id);
                    let mut gv = Vec::new();
                    gv.extend(g.pos);
                    gv.extend(g.q);
                    gv.extend(g.r);
                    gv.extend(g.lvel);
                    gv.extend(g.avel);
                    gv.extend(g.facc);
                    gv.extend(g.tacc);
                    let mut pv = Vec::new();
                    pv.extend(&b.pos[..3]);
                    pv.extend(b.q);
                    pv.extend([b.r[0], b.r[1], b.r[2], b.r[4], b.r[5], b.r[6], b.r[8], b.r[9], b.r[10]]);
                    pv.extend(&b.lvel[..3]);
                    pv.extend(&b.avel[..3]);
                    pv.extend(&b.facc[..3]);
                    pv.extend(&b.tacc[..3]);
                    found = differ(&format!("body {} (pos 3, q 4, R 9, lvel 3, avel 3, facc 3, tacc 3)", game.name), &gv, &pv);
                }
                samples[op] += 1;
                if let Some(text) = found {
                    failures[op] += 1;
                    if first[op].is_none() {
                        first[op] = Some(format!("sample {sample}, wheel {} ({}): {text}", WHEELS[w], classes[w].name));
                    }
                }
            }

            let kinds = classes.iter().map(|class| class.name).collect::<Vec<_>>().join(" ");
            let mut text = format!("Suspensions (LF RF LR RR): {kinds}. {} bodies, {} joints.\n\n", st.bodies.len(), st.joints.len());
            text.push_str("| Call | Samples | Bit-exact | First difference |\n|---|---|---|---|\n");
            let mut merged: Vec<(&str, usize, usize, Option<String>)> = Vec::new();
            for (i, name) in OPS.iter().enumerate() {
                match merged.last_mut() {
                    Some(last) if last.0 == *name => {
                        last.1 += samples[i];
                        last.2 += failures[i];
                        if last.3.is_none() {
                            last.3 = first[i].clone();
                        }
                    }
                    _ => merged.push((name, samples[i], failures[i], first[i].clone())),
                }
            }
            for (name, n, bad, first) in &merged {
                text.push_str(&format!("| `{name}` | {n} | {} | {} |\n", n - bad, first.clone().unwrap_or_else(|| "none".to_string())));
            }
            let total: usize = samples.iter().sum();
            let bad: usize = failures.iter().sum();
            text.push_str(&format!("| **all** | **{total}** | **{}** | |\n", total - bad));
            Ok(MicroReport { text, ok: bad == 0 })
        }
    }
}
