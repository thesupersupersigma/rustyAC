// SPDX-License-Identifier: BSD-3-Clause

//! The ODE part of Custom Shaders Patch 0.3.2's (build 4305) world step (+0x92e4a0) in f64, as it
//! runs with CSP's physics worker count 0: `dInternalHandleAutoDisabling` (+0x16c5e60), the island
//! builder (+0x16c6ab0) and every island through `dxStepIsland` (+0x16c24b0), one after the other in
//! the order found, on one thread (`dxProcessIslands` +0x16c6d00, its stepper fixed to
//! `dxStepIsland`). Its own type rather than a generic `World` because the f32 world holds f32
//! joints, geoms and spaces; collision is not here (the step is fed the contacts as joints).

use crate::csp_step::{step_island, Joint, WorldParams};
use crate::world::{Body, BODY_AUTO_DISABLE, BODY_DISABLED};

/// A joint in the world's list: CSP's joint flags (+0x30: 2 reverse, 8 disabled) and its tag.
#[derive(Clone, Debug)]
pub struct WorldJoint {
    pub joint: Joint,
    pub flags: u32,
    pub tag: i32,
}

pub const JOINT_DISABLED: u32 = 8;

pub struct CspWorld {
    pub params: WorldParams,
    /// in the world's list order (newest first); joints name bodies by index here
    pub bodies: Vec<Body<f64>>,
    pub joints: Vec<WorldJoint>,
    /// each body's joint node list, first node first: (joint, the joint's other body)
    pub nodes: Vec<Vec<(usize, Option<usize>)>>,
}

impl CspWorld {
    /// The world step's ODE part; returns the islands stepped as (bodies, joints) world indices.
    pub fn step(&mut self, h: f64) -> Vec<(Vec<usize>, Vec<usize>)> {
        self.auto_disable(h);
        let islands = self.build_islands();
        for (bodies, joints) in &islands {
            let mut local = vec![usize::MAX; self.bodies.len()];
            bodies.iter().enumerate().for_each(|(i, &b)| local[b] = i);
            let mut island_bodies: Vec<Body<f64>> = bodies.iter().map(|&b| self.bodies[b].clone()).collect();
            let mut island_joints: Vec<Joint> = joints
                .iter()
                .map(|&j| {
                    let mut joint = self.joints[j].joint.clone();
                    joint.b0 = local[joint.b0];
                    joint.b1 = joint.b1.map(|b| local[b]);
                    joint
                })
                .collect();
            step_island(&self.params, &mut island_bodies, &mut island_joints, h);
            for (i, (&b, mut body)) in bodies.iter().zip(island_bodies).enumerate() {
                body.tag = i as i32; // step.cpp: body[i]->tag = i
                self.bodies[b] = body;
            }
            for (&j, joint) in joints.iter().zip(island_joints) {
                self.joints[j].joint.feedback = joint.feedback;
                self.joints[j].joint.kind = joint.kind;
            }
        }
        islands
    }

    /// +0x16c5e60 = master's: no NaN guard on the thresholds (comisd: a NaN speed counts as idle)
    /// and a NaN time left disables.
    fn auto_disable(&mut self, h: f64) {
        for (b, nodes) in self.bodies.iter_mut().zip(&self.nodes) {
            let samples = b.adis.average_samples;
            if nodes.is_empty() || b.flags & (BODY_AUTO_DISABLE | BODY_DISABLED) != BODY_AUTO_DISABLE || samples == 0 {
                continue;
            }
            let c = b.average_counter as usize;
            b.average_lvel_buffer[c] = [b.lvel[0], b.lvel[1], b.lvel[2]];
            b.average_avel_buffer[c] = [b.avel[0], b.avel[1], b.avel[2]];
            b.average_counter += 1;
            if b.average_counter >= samples {
                b.average_counter = 0;
                b.average_ready = true;
            }
            let mut idle = false;
            if b.average_ready {
                let (mut l, mut a) = (b.average_lvel_buffer[0], b.average_avel_buffer[0]);
                if samples > 1 {
                    for i in 1..samples as usize {
                        for k in 0..3 {
                            l[k] += b.average_lvel_buffer[i][k];
                            a[k] += b.average_avel_buffer[i][k];
                        }
                    }
                    let r1 = 1.0 / samples as f64;
                    l = l.map(|x| r1 * x);
                    a = a.map(|x| r1 * x);
                }
                let dot = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2];
                idle = !(dot(l) > b.adis.linear_average_threshold) && !(dot(a) > b.adis.angular_average_threshold);
            }
            if idle {
                b.adis_stepsleft = b.adis_stepsleft.wrapping_sub(1);
                b.adis_timeleft -= h;
            } else {
                b.adis_stepsleft = b.adis.idle_steps;
                b.adis_timeleft = b.adis.idle_time;
            }
            if b.adis_stepsleft <= 0 && !(b.adis_timeleft > 0.0) {
                b.flags |= BODY_DISABLED;
                b.lvel[..3].fill(0.0);
                b.avel[..3].fill(0.0);
            }
        }
    }

    /// +0x16c6ab0 after the auto-disable = master's `BuildIslands...`; a joint counts as enabled
    /// when not disabled and a body has `!(0.0 >= inv_mass)` (so a NaN inverse mass counts).
    fn build_islands(&mut self) -> Vec<(Vec<usize>, Vec<usize>)> {
        self.bodies.iter_mut().for_each(|b| b.tag = 0);
        self.joints.iter_mut().for_each(|j| j.tag = 0);
        let movable = |b: &Body<f64>| !(0.0 >= b.inv_mass);
        let mut islands = Vec::new();
        for bb in 0..self.bodies.len() {
            if self.bodies[bb].tag != 0 {
                continue;
            }
            if self.bodies[bb].flags & BODY_DISABLED != 0 {
                self.bodies[bb].tag = -1;
                continue;
            }
            self.bodies[bb].tag = 1;
            let (mut bodies, mut joints, mut stack) = (vec![bb], Vec::new(), Vec::new());
            let mut b = bb;
            loop {
                for &(j, other) in &self.nodes[b] {
                    if self.joints[j].tag != 0 {
                        continue;
                    }
                    let joint = &self.joints[j];
                    let enabled = joint.flags & JOINT_DISABLED == 0 && (movable(&self.bodies[joint.joint.b0]) || joint.joint.b1.is_some_and(|b1| movable(&self.bodies[b1])));
                    if !enabled {
                        self.joints[j].tag = -1;
                        continue;
                    }
                    self.joints[j].tag = 1;
                    joints.push(j);
                    if let Some(o) = other.filter(|&o| self.bodies[o].tag <= 0) {
                        self.bodies[o].tag = 1;
                        self.bodies[o].flags &= !BODY_DISABLED;
                        stack.push(o);
                    }
                }
                match stack.pop() {
                    Some(popped) => {
                        b = popped;
                        bodies.push(b);
                    }
                    None => break,
                }
            }
            islands.push((bodies, joints));
        }
        islands
    }
}
