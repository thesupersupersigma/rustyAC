// SPDX-License-Identifier: BSD-3-Clause

//! Golden test: the game's own F2004, replayed from small excerpts of the whole-car oracle
//! recordings (`tools/car_oracle`, report `docs/oracle/car_oracle.md`).
//!
//! Each excerpt holds the state of the six bodies when an excerpt starts and then, per step,
//! what the car did to them from outside (the force and torque accumulators just before
//! `dWorldStep`, joint parameter changes, mass changes, the "stop" of the sleeping rule) and
//! what the game's ODE made of it. The test starts from the first state and runs free: the
//! Rust state is never reset. Every position, quaternion and velocity of every body, and
//! every joint's constraint force, must come out with the same bits in every step.
//!
//! The files are written by `tools/ode_oracle excerpt`. Format (little endian, 32-bit words):
//!
//! ```text
//! "ODEGOLD1"
//! u32 bodies, u32 joints, u32 steps, u32 first step in the recording
//! f32 gravity x, y, z, ERP, CFM, step size
//! per joint: u32 ODE type (7 fixed, 15 DBall), u32 body 1, u32 body 2
//! per body:  pos[3], q[4] (w, x, y, z), R[9] (row-major 3x3), lvel[3], avel[3]
//! per step:
//!     u32 stop mask (bit b: body b had velocities and accumulators zeroed)
//!     u32 n, then n x (u32 body, mass, inertia x, y, z)          masses that changed
//!     u32 n, then n x (u32 joint, 9 parameters)                  joint parameters that changed
//!                DBall: anchor1[3], anchor2[3], erp, cfm, distance; fixed: qrel[4], offset[3], erp, cfm
//!     per body: facc[3], tacc[3]                                 accumulators before the step
//!     per body: pos[3], q[4], lvel[3], avel[3]                   expected state after the step
//!     u64 FNV-1a hash of the bits of every joint's f1, t1, f2, t2 (3 values each), in joint order
//! ```

use rustyac_ode::{BodyId, JointId, JointKind, Mass, World};

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.data[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn f(&mut self) -> f32 {
        f32::from_bits(self.u())
    }
    fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.data[self.at..self.at + 8].try_into().unwrap());
        self.at += 8;
        v
    }
}

fn replay(data: &[u8]) {
    assert_eq!(&data[..8], b"ODEGOLD1");
    let mut r = Reader { data, at: 8 };
    let (nb, nj, steps, first) = (r.u() as usize, r.u() as usize, r.u() as usize, r.u() as usize);
    let gravity = [r.f(), r.f(), r.f()];
    let (erp, cfm, h) = (r.f(), r.f(), r.f());

    // the world as PhysicsCore::PhysicsCore sets it up, the bodies as RigidBodyODE::RigidBodyODE
    // does, in the car's creation order (it decides the island and row order)
    let mut world = World::new();
    world.set_gravity(gravity[0], gravity[1], gravity[2]);
    world.set_erp(erp);
    world.set_cfm(cfm);
    world.set_damping(0.0, 0.0);
    let bodies: Vec<BodyId> = (0..nb)
        .map(|_| {
            let b = world.body_create();
            world.body_set_finite_rotation_mode(b, true);
            world.body_set_finite_rotation_axis(b, 0.0, 0.0, 0.0);
            world.body_set_linear_damping(b, 0.0);
            world.body_set_angular_damping(b, 0.0);
            b
        })
        .collect();
    let joints: Vec<JointId> = (0..nj)
        .map(|_| {
            let (kind, b1, b2) = (r.u(), r.u() as usize, r.u() as usize);
            let j = match kind {
                7 => world.joint_create_fixed(),
                15 => world.joint_create_dball(),
                other => panic!("joint type {other}"),
            };
            world.joint_attach(j, Some(bodies[b1]), Some(bodies[b2]));
            world.joint_set_feedback(j, true);
            j
        })
        .collect();
    for &b in &bodies {
        let body = world.body_mut(b);
        for k in 0..3 {
            body.pos[k] = r.f();
        }
        for k in 0..4 {
            body.q[k] = r.f();
        }
        for row in 0..3 {
            for col in 0..3 {
                body.r[4 * row + col] = r.f();
            }
        }
        for k in 0..3 {
            body.lvel[k] = r.f();
        }
        for k in 0..3 {
            body.avel[k] = r.f();
        }
    }

    for step in 0..steps {
        let stop_mask = r.u();
        for (b, &id) in bodies.iter().enumerate() {
            if stop_mask >> b & 1 == 1 {
                world.body_set_linear_vel(id, 0.0, 0.0, 0.0);
                world.body_set_angular_vel(id, 0.0, 0.0, 0.0);
            }
        }
        for _ in 0..r.u() {
            let b = r.u() as usize;
            let mut m = Mass::zero();
            m.mass = r.f();
            m.i[0] = r.f();
            m.i[5] = r.f();
            m.i[10] = r.f();
            world.body_set_mass(bodies[b], &m);
        }
        for _ in 0..r.u() {
            let j = r.u() as usize;
            let p: Vec<f32> = (0..9).map(|_| r.f()).collect();
            match &mut world.joint_mut(joints[j]).kind {
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                    anchor1[..3].copy_from_slice(&p[0..3]);
                    anchor2[..3].copy_from_slice(&p[3..6]);
                    *erp = p[6];
                    *cfm = p[7];
                    *target_distance = p[8];
                }
                JointKind::Fixed { qrel, offset, erp, cfm } => {
                    qrel.copy_from_slice(&p[0..4]);
                    offset[..3].copy_from_slice(&p[4..7]);
                    *erp = p[7];
                    *cfm = p[8];
                }
                _ => unreachable!(),
            }
        }
        for &b in &bodies {
            let body = world.body_mut(b);
            for k in 0..3 {
                body.facc[k] = r.f();
            }
            for k in 0..3 {
                body.tacc[k] = r.f();
            }
        }

        world.step(h);

        for (b, &id) in bodies.iter().enumerate() {
            let body = world.body(id);
            let got: Vec<f32> =
                body.pos[..3].iter().chain(&body.q).chain(&body.lvel[..3]).chain(&body.avel[..3]).copied().collect();
            for (k, &value) in got.iter().enumerate() {
                let expected = r.f();
                assert!(
                    value.to_bits() == expected.to_bits(),
                    "recording step {}: body {b}, value {k} (pos 0..2, q 3..6, lvel 7..9, avel 10..12): game {expected:?} \
                     ({:#010x}) / Rust {value:?} ({:#010x})",
                    first + step,
                    expected.to_bits(),
                    value.to_bits()
                );
            }
        }
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &j in &joints {
            let fb = world.joint(j).feedback.unwrap();
            for v in fb.f1[..3].iter().chain(&fb.t1[..3]).chain(&fb.f2[..3]).chain(&fb.t2[..3]) {
                for byte in v.to_bits().to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        assert_eq!(hash, r.u64(), "recording step {}: the joints' constraint forces differ", first + step);
    }
    assert_eq!(r.at, data.len(), "the excerpt has bytes left over");
}

/// The spawn: the car is dropped on the road, sinks into its suspension, and is frozen by
/// the game's sleeping rule from step 155 on.
#[test]
fn settle_first_300_steps() {
    replay(include_bytes!("data/settle_0_300.odegold"));
}

/// The slalom at 100 km/h: both steering rods move every step.
#[test]
fn slalom_300_steps() {
    replay(include_bytes!("data/slalom_3000_300.odegold"));
}
