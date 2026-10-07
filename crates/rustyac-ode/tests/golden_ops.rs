// SPDX-License-Identifier: BSD-3-Clause

//! Golden test: small synthetic worlds of every kind the micro-oracle makes (`tools/ode_oracle`,
//! report `docs/port/ode_stage1.md`), replayed without the game.
//!
//! Each file in `tests/data/ops` holds one world as the list of calls that built it and the
//! calls made before every step, plus a hash of the state of Assetto Corsa's own ODE after
//! every step (taken when the file was written, with the game's code doing the stepping).
//! The test makes the same calls on the Rust port and must arrive at the same hash in every
//! step. Together the files cover what the car excerpts of `golden_car.rs` do not: ball and
//! slider joints, joints to the world, rods of length zero, auto-disable, destroyed and
//! late-created bodies, re-attached and disabled joints, other gravity, damping and step sizes,
//! and worlds whose joints have no feedback buffer (which is how the game runs).
//!
//! The files are written by `ode_oracle golden`. Format (little endian, 32-bit words):
//!
//! ```text
//! "ODEOPS01"
//! f32 gravity x, y, z, ERP, CFM, linear damping, angular damping, step size
//! u32 joints get a feedback buffer (0 / 1)
//! u32 n, then n words: the calls that build the world
//! u32 steps
//! per step: u32 n, then n words: the calls before the step; u64 hash of the state after it
//! ```
//!
//! A call is a code followed by its integers and then its floats:
//!
//! ```text
//!  1 create body                              15 body, auto-disable sample count
//!  2 body, kind; mass + 6 values              16 body; inverse mass (written directly)
//!    (0 box: lx ly lz, 1 dMassSetParameters:  17 body, idle steps; idle time (written directly)
//!    I11 I22 I33 I12 I13 I23, 2 the game's    18 kind: create joint (0 ball, 1 DBall, 2 fixed,
//!    setMassExplicitInertia: three values)       3 slider)
//!  3 body; position                           19 joint, body 1 + 1, body 2 + 1 (0 = world): attach
//!  4 body; rotation (12 values)               20 joint; ball anchor
//!  5 body; linear velocity                    21 joint, which (0 / 1); DBall anchor
//!  6 body; angular velocity                   22 joint; DBall distance
//!  7 body, mode; finite-rotation axis         23 joint: dJointSetFixed
//!  8 body; linear, angular damping            24 joint; slider axis
//!  9 body, auto-disable flag                  25 joint, parameter number; value
//! 10 body, enable (1) / disable (0)           26 joint, flags to set, flags to clear (direct)
//! 11 body, kind; vector, point: the seven     27 joint, body 1, body 2; local point on each: what
//!    dBodyAdd… functions (order below)           the game's reseatDistanceJointLocal does
//! 12 body: velocities and accumulators zeroed
//! 13 body: destroy
//! 14 body, flags to set, flags to clear; max angular speed (written directly)
//! ```
//!
//! The hash is FNV-1a 64 over these words: per live body pos[3], q[4], R (3x3), lvel, avel,
//! facc, tacc, tag, flags and the six frame getters at a point that changes every step; then
//! per joint tag, flags, the two bodies (index or -1), its parameters (for a slider also its
//! position and whether it is at a stop) and the constraint force (f1, t1, f2, t2; zeros
//! without a feedback buffer). Before every step the feedback values
//! are set to -12345.678, so that a force the step does not write shows as such.

use rustyac_ode::{BodyId, JointId, JointKind, Mass, World};

const SENTINEL: f32 = -12345.678;

struct Words<'a> {
    data: &'a [u8],
    at: usize,
}

impl Words<'_> {
    fn u(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.data[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn f(&mut self) -> f32 {
        f32::from_bits(self.u())
    }
    fn v3(&mut self) -> [f32; 3] {
        [self.f(), self.f(), self.f()]
    }
    fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.data[self.at..self.at + 8].try_into().unwrap());
        self.at += 8;
        v
    }
}

struct Replay {
    world: World,
    bodies: Vec<BodyId>,
    dead: Vec<bool>,
    joints: Vec<JointId>,
    /// rod lengths as the game's `DistanceJointODE` remembers them
    rod_length: Vec<Option<f32>>,
    feedback: bool,
}

impl Replay {
    /// Applies the calls in `r` up to byte `end`.
    fn apply(&mut self, r: &mut Words, end: usize) {
        while r.at < end {
            let code = r.u();
            let w = &mut self.world;
            match code {
                1 => {
                    self.bodies.push(w.body_create());
                    self.dead.push(false);
                }
                2 => {
                    let (b, kind) = (self.bodies[r.u() as usize], r.u());
                    let (mass, v) = (r.f(), [r.f(), r.f(), r.f(), r.f(), r.f(), r.f()]);
                    let m = match kind {
                        0 => Mass::box_total(mass, v[0], v[1], v[2]),
                        1 => Mass::parameters(mass, 0.0, 0.0, 0.0, v[0], v[1], v[2], v[3], v[4], v[5]),
                        _ => {
                            let mut m = Mass::zero();
                            m.mass = mass;
                            m.i[0] = v[0];
                            m.i[4] = v[1];
                            m.i[8] = v[2];
                            m
                        }
                    };
                    w.body_set_mass(b, &m);
                }
                3 => {
                    let (b, p) = (self.bodies[r.u() as usize], r.v3());
                    w.body_set_position(b, p[0], p[1], p[2]);
                }
                4 => {
                    let b = self.bodies[r.u() as usize];
                    let mut m = [0.0f32; 12];
                    for v in m.iter_mut() {
                        *v = r.f();
                    }
                    w.body_set_rotation(b, &m);
                }
                5 => {
                    let (b, v) = (self.bodies[r.u() as usize], r.v3());
                    w.body_set_linear_vel(b, v[0], v[1], v[2]);
                }
                6 => {
                    let (b, v) = (self.bodies[r.u() as usize], r.v3());
                    w.body_set_angular_vel(b, v[0], v[1], v[2]);
                }
                7 => {
                    let (b, mode, axis) = (self.bodies[r.u() as usize], r.u() != 0, r.v3());
                    w.body_set_finite_rotation_mode(b, mode);
                    w.body_set_finite_rotation_axis(b, axis[0], axis[1], axis[2]);
                }
                8 => {
                    let (b, linear, angular) = (self.bodies[r.u() as usize], r.f(), r.f());
                    w.body_set_linear_damping(b, linear);
                    w.body_set_angular_damping(b, angular);
                }
                9 => {
                    let (b, on) = (self.bodies[r.u() as usize], r.u() != 0);
                    w.body_set_auto_disable_flag(b, on);
                }
                10 => {
                    let (b, on) = (self.bodies[r.u() as usize], r.u() != 0);
                    if on {
                        w.body_enable(b);
                    } else {
                        w.body_disable(b);
                    }
                }
                11 => {
                    let (b, kind, a, p) = (self.bodies[r.u() as usize], r.u(), r.v3(), r.v3());
                    match kind {
                        0 => w.body_add_force(b, a),
                        1 => w.body_add_torque(b, a),
                        2 => w.body_add_rel_torque(b, a),
                        3 => w.body_add_force_at_pos(b, a, p),
                        4 => w.body_add_force_at_rel_pos(b, a, p),
                        5 => w.body_add_rel_force_at_pos(b, a, p),
                        _ => w.body_add_rel_force_at_rel_pos(b, a, p),
                    }
                }
                12 => {
                    let b = self.bodies[r.u() as usize];
                    w.body_set_linear_vel(b, 0.0, 0.0, 0.0);
                    w.body_set_angular_vel(b, 0.0, 0.0, 0.0);
                    w.body_set_force(b, 0.0, 0.0, 0.0);
                    w.body_set_torque(b, 0.0, 0.0, 0.0);
                }
                13 => {
                    let b = r.u() as usize;
                    w.body_destroy(self.bodies[b]);
                    self.dead[b] = true;
                }
                14 => {
                    let (b, set, clear, speed) = (self.bodies[r.u() as usize], r.u(), r.u(), r.f());
                    let body = w.body_mut(b);
                    body.flags = (body.flags | set) & !clear;
                    body.max_angular_speed = speed;
                }
                15 => {
                    let (b, count) = (self.bodies[r.u() as usize], r.u());
                    w.body_set_auto_disable_average_samples_count(b, count);
                }
                16 => {
                    let (b, inv_mass) = (self.bodies[r.u() as usize], r.f());
                    w.body_mut(b).inv_mass = inv_mass;
                }
                17 => {
                    let (b, steps, time) = (self.bodies[r.u() as usize], r.u() as i32, r.f());
                    let body = w.body_mut(b);
                    body.adis.idle_time = time;
                    body.adis.idle_steps = steps;
                }
                18 => {
                    let j = match r.u() {
                        0 => w.joint_create_ball(),
                        1 => w.joint_create_dball(),
                        2 => w.joint_create_fixed(),
                        _ => w.joint_create_slider(),
                    };
                    w.joint_set_feedback(j, self.feedback);
                    self.joints.push(j);
                    self.rod_length.push(None);
                }
                19 => {
                    let j = self.joints[r.u() as usize];
                    let (b1, b2) = (r.u() as usize, r.u() as usize);
                    let body = |n: usize| (n > 0).then(|| self.bodies[n - 1]);
                    w.joint_attach(j, body(b1), body(b2));
                }
                20 => {
                    let (j, p) = (self.joints[r.u() as usize], r.v3());
                    w.joint_set_ball_anchor(j, p[0], p[1], p[2]);
                }
                21 => {
                    let (j, which, p) = (r.u() as usize, r.u(), r.v3());
                    if which == 0 {
                        w.joint_set_dball_anchor1(self.joints[j], p[0], p[1], p[2]);
                    } else {
                        w.joint_set_dball_anchor2(self.joints[j], p[0], p[1], p[2]);
                        self.rod_length[j] = Some(w.joint_get_dball_distance(self.joints[j]));
                    }
                }
                22 => {
                    let (j, d) = (self.joints[r.u() as usize], r.f());
                    w.joint_set_dball_distance(j, d);
                }
                23 => {
                    let j = self.joints[r.u() as usize];
                    w.joint_set_fixed(j);
                }
                24 => {
                    let (j, axis) = (self.joints[r.u() as usize], r.v3());
                    w.joint_set_slider_axis(j, axis[0], axis[1], axis[2]);
                }
                25 => {
                    let (j, parameter, value) = (self.joints[r.u() as usize], r.u() as i32, r.f());
                    w.joint_set_param(j, parameter, value);
                }
                26 => {
                    let (j, set, clear) = (self.joints[r.u() as usize], r.u(), r.u());
                    let joint = w.joint_mut(j);
                    joint.flags = (joint.flags | set) & !clear;
                }
                27 => {
                    let j = r.u() as usize;
                    let (b1, b2) = (self.bodies[r.u() as usize], self.bodies[r.u() as usize]);
                    let (local1, local2) = (r.v3(), r.v3());
                    let p1 = w.body_get_rel_point_pos(b1, local1);
                    let p2 = w.body_get_rel_point_pos(b2, local2);
                    w.joint_set_dball_anchor1(self.joints[j], p1[0], p1[1], p1[2]);
                    w.joint_set_dball_anchor2(self.joints[j], p2[0], p2[1], p2[2]);
                    if let Some(length) = self.rod_length[j] {
                        w.joint_set_dball_distance(self.joints[j], length);
                    }
                }
                other => panic!("call code {other}"),
            }
        }
        assert_eq!(r.at, end, "a call ran past the end of its list");
    }

    /// The words the hash of step `step` is made of (see the top of the file).
    fn state(&self, step: usize) -> Vec<u32> {
        let mut words = Vec::new();
        let w = &self.world;
        for (b, &id) in self.bodies.iter().enumerate() {
            if self.dead[b] {
                continue;
            }
            let body = w.body(id);
            let rows = [0usize, 1, 2, 4, 5, 6, 8, 9, 10].map(|k| body.r[k]);
            let values = body.pos[..3]
                .iter()
                .chain(&body.q)
                .chain(&rows)
                .chain(&body.lvel[..3])
                .chain(&body.avel[..3])
                .chain(&body.facc[..3])
                .chain(&body.tacc[..3]);
            words.extend(values.map(|v| v.to_bits()));
            words.push(body.tag as u32);
            words.push(body.flags);
            let t = (step % 97) as f32 * 0.03 - 1.4 + b as f32 * 0.1;
            let p = [t, 0.5 - 0.3 * t, t * t - 0.7];
            for part in [
                w.body_get_rel_point_pos(id, p),
                w.body_get_pos_rel_point(id, p),
                w.body_vector_to_world(id, p),
                w.body_vector_from_world(id, p),
                w.body_get_point_vel(id, p),
                w.body_get_rel_point_vel(id, p),
            ] {
                words.extend(part.map(f32::to_bits));
            }
        }
        for &id in &self.joints {
            let joint = w.joint(id);
            words.push(joint.tag as u32);
            words.push(joint.flags);
            for index in 0..2 {
                let body = w.joint_get_body(id, index);
                let at = body.and_then(|b| self.bodies.iter().position(|&x| x == b));
                words.push(at.map_or(-1, |i| i as i32) as u32);
            }
            let mut p: Vec<f32> = Vec::new();
            match &joint.kind {
                JointKind::Ball { anchor1, anchor2, erp, cfm } => {
                    p.extend_from_slice(&anchor1[..3]);
                    p.extend_from_slice(&anchor2[..3]);
                    p.extend_from_slice(&[*erp, *cfm]);
                }
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                    p.extend_from_slice(&anchor1[..3]);
                    p.extend_from_slice(&anchor2[..3]);
                    p.extend_from_slice(&[*erp, *cfm, *target_distance]);
                }
                JointKind::Fixed { qrel, offset, erp, cfm } => {
                    p.extend_from_slice(qrel);
                    p.extend_from_slice(&offset[..3]);
                    p.extend_from_slice(&[*erp, *cfm]);
                }
                // (the recorded worlds have no contact joints)
                JointKind::Contact { .. } => {}
                JointKind::Slider { axis1, qrel, offset, limot } => {
                    p.extend_from_slice(&axis1[..3]);
                    p.extend_from_slice(qrel);
                    p.extend_from_slice(&offset[..3]);
                    p.extend_from_slice(&[
                        limot.vel,
                        limot.fmax,
                        limot.lostop,
                        limot.histop,
                        limot.fudge_factor,
                        limot.normal_cfm,
                        limot.stop_erp,
                        limot.stop_cfm,
                        limot.bounce,
                    ]);
                    p.push(w.joint_get_slider_position(id));
                    p.push(limot.limit as f32);
                }
            }
            words.extend(p.iter().map(|v| v.to_bits()));
            let fb = joint.feedback.unwrap_or_default();
            for part in [fb.f1, fb.t1, fb.f2, fb.t2] {
                words.extend(part[..3].iter().map(|v| v.to_bits()));
            }
        }
        words
    }
}

fn fnv(words: &[u32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for w in words {
        for byte in w.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// Replays one file; returns the number of steps.
fn replay(name: &str, data: &[u8]) -> usize {
    assert_eq!(&data[..8], b"ODEOPS01", "{name}");
    let mut r = Words { data, at: 8 };
    let gravity = r.v3();
    let (erp, cfm, linear, angular, h) = (r.f(), r.f(), r.f(), r.f(), r.f());
    let mut world = World::new();
    world.set_gravity(gravity[0], gravity[1], gravity[2]);
    world.set_erp(erp);
    world.set_cfm(cfm);
    world.set_damping(linear, angular);
    let feedback = r.u() != 0;
    let mut replay =
        Replay { world, bodies: Vec::new(), dead: Vec::new(), joints: Vec::new(), rod_length: Vec::new(), feedback };
    let n = r.u() as usize;
    let end = r.at + 4 * n;
    replay.apply(&mut r, end);
    let steps = r.u() as usize;
    for step in 0..steps {
        let n = r.u() as usize;
        let end = r.at + 4 * n;
        replay.apply(&mut r, end);
        for &j in &replay.joints {
            if let Some(fb) = &mut replay.world.joint_mut(j).feedback {
                (fb.f1, fb.t1, fb.f2, fb.t2) = ([SENTINEL; 4], [SENTINEL; 4], [SENTINEL; 4], [SENTINEL; 4]);
            }
        }
        replay.world.step(h);
        let expected = r.u64();
        let got = fnv(&replay.state(step));
        assert_eq!(got, expected, "{name}: the state after step {step} differs from the game's");
    }
    assert_eq!(r.at, data.len(), "{name}: bytes left over");
    steps
}

#[test]
fn synthetic_worlds_match_the_game_in_every_step() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/ops");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/data/ops")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|x| x == "odeops"))
        .collect();
    files.sort();
    assert!(files.len() >= 31, "only {} golden worlds in {}", files.len(), dir.display());
    let mut steps = 0;
    for path in &files {
        let data = std::fs::read(path).unwrap();
        steps += replay(&path.file_name().unwrap().to_string_lossy(), &data);
    }
    assert!(steps >= 6000, "only {steps} steps");
}
