// SPDX-License-Identifier: BSD-3-Clause

//! Custom Shaders Patch 0.3.2's (build 4305) `cphys::` wrappers (`ode_impl_core` +0x2304e10,
//! `rigidbody_spec<Base>` +0x2305480, the distance and fixed joints) over its f64 ODE world: what
//! the car's calls into the physics core do to the world. Bodies and joints are named by their
//! creation order; the world keeps them newest first, so a body's world index is
//! `bodies - 1 - id` (non-contact joints likewise, counted from the list's tail). Every value the
//! car hands over is an f32 and every answer goes back as an f32, as the wrappers do.

use crate::csp_collide::{NewJoint, Side, Unhandled};
#[cfg(feature = "embree")]
use crate::csp_ray::{Cast, Scene, TraceMesh};
use crate::csp_raycast::{self, Ray, RAY_BACKFACE_CULL, RAY_FIRST_CONTACT};
use crate::csp_collision::{joint_of, posr, product, refresh, Collision, Geom, Shape, TrackMesh, GEOM_AABB_BAD, GEOM_DIRTY, GEOM_ENABLED, GEOM_POSR_BAD};
use crate::csp_space::HashSpace;
use crate::csp_step::{Joint, JointKind, WorldParams};
use crate::csp_world::{CspWorld, WorldJoint};
use crate::mass::Mass;
use crate::world::{AutoDisable, Body, Damping};

/// A distance joint wrapper (+0x928000, 104 bytes): its two bodies, the distance it was made with
/// (+48, put back after every reseat) and the anchor offset (+88, never set by the calls seen).
#[derive(Clone)]
struct DistanceJoint {
    b1: usize,
    b2: usize,
    target: f64,
}

#[derive(Clone)]
pub struct Core {
    pub world: CspWorld,
    /// `dWorldCreate` +0x168cf90 defaults every new body takes
    adis: AutoDisable<f64>,
    dampingp: Damping<f64>,
    max_angular_speed: f64,
    distance: Vec<Option<DistanceJoint>>,
    /// joints made, contact joints aside (tail ids)
    made: usize,
    /// the collision state: the car geoms made here, the track's handed over by [`Core::set_track`]
    pub col: Collision,
    track_set: bool,
    /// per geom: its body id and its space (`CARS` the car container)
    owner: Vec<Option<usize>>,
    parent: Vec<Option<usize>>,
    /// per body id: its geoms in the body's list order (newest first), the wrapper's bytes
    /// +152..+159, its mesh colliders in made order (wrapper vector +48)
    body_geoms: Vec<Vec<usize>>,
    user: Vec<[u8; 8]>,
    meshes: Vec<Vec<usize>>,
    /// +0x91bdb0's map (+72): key → car space
    spaces: Vec<(u32, usize)>,
    /// the track space's hash levels, for its table built again once its geoms are cleaned
    track_levels: (i32, i32),
    /// a tyre ray (+0x92b970) cast: the set-up is over
    tyre_seen: bool,
    /// Task C5h: per CSP mesh list index (+0x2885370) the track geom the rays name; the Embree
    /// scene of those meshes when Embree is on; +0x2812c7f
    ray_geoms: Vec<usize>,
    #[cfg(feature = "embree")]
    scene: Option<std::sync::Arc<Scene>>,
    below: bool,
}

/// Task C5h: a track mesh as the rays see it (its object's block +48 and flag +40), in CSP's
/// mesh list order; `geom` its index in [`Core::set_track`]'s list.
pub struct RayMesh {
    pub geom: usize,
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u16>,
    pub two_sided: bool,
}

/// What a track ray (+0x932900) writes for the tyre: point (+0), normal (+12) and the mesh hit
/// (+24, as its CSP mesh list index; +32 the hit byte).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackRay {
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub mesh: Option<usize>,
}

/// The car container (`spaces + 8`, a hash space) as a parent.
const CARS: usize = usize::MAX;
/// `dxGeom`'s constructor flags: dirty, box out of date, enabled
const NEW_GEOM: u32 = GEOM_DIRTY | GEOM_AABB_BAD | GEOM_ENABLED;
const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

impl Core {
    /// The world as +0x91c630 sets it up: gravity (0, -9.806, 0), ERP 0.3, CFM 1e-7, contact max
    /// correcting velocity 3, surface layer 0.001; the rest `dWorldCreate`'s defaults.
    pub fn new() -> Self {
        Core {
            world: CspWorld {
                params: WorldParams { gravity: [0.0, -9.806, 0.0], erp: 0.3, cfm: 1e-7, max_vel: 3.0, min_depth: 0.001 },
                bodies: Vec::new(),
                joints: Vec::new(),
                nodes: Vec::new(),
            },
            adis: AutoDisable { idle_time: 0.0, idle_steps: 10, linear_average_threshold: 0.01 * 0.01, angular_average_threshold: 0.01 * 0.01, average_samples: 1 },
            dampingp: Damping { linear_scale: 0.0, angular_scale: 0.0, linear_threshold: 0.01 * 0.01, angular_threshold: 0.01 * 0.01 },
            max_angular_speed: f64::from_bits(0x7e37_e43c_8800_759c),
            distance: Vec::new(),
            made: 0,
            col: Collision {
                geoms: Vec::new(),
                meshes: Vec::new(),
                cars: Vec::new(),
                cars_level: 0,
                cars_count: 0,
                track: Vec::new(),
                track_level: 0,
                track_count: 0,
                ..Default::default()
            },
            track_set: false,
            owner: Vec::new(),
            parent: Vec::new(),
            body_geoms: Vec::new(),
            user: Vec::new(),
            meshes: Vec::new(),
            spaces: Vec::new(),
            track_levels: (0, 0),
            tyre_seen: false,
            ray_geoms: Vec::new(),
            #[cfg(feature = "embree")]
            scene: None,
            below: false,
        }
    }

    pub fn body(&self, id: usize) -> &Body<f64> {
        &self.world.bodies[self.world.bodies.len() - 1 - id]
    }

    pub fn body_mut(&mut self, id: usize) -> &mut Body<f64> {
        let n = self.world.bodies.len();
        &mut self.world.bodies[n - 1 - id]
    }

    /// The body id a geom is on.
    pub fn geom_body(&self, g: usize) -> Option<usize> {
        self.owner.get(g).copied().flatten()
    }

    /// Task C5i-a: body `id`'s state as `from` (another copy of this core) has it, its geoms
    /// moved when it was put somewhere else; true then.
    pub fn copy_body_from(&mut self, from: &Core, id: usize) -> bool {
        let src = from.body(id);
        let dst = self.body_mut(id);
        let moved = dst.pos != src.pos || dst.r != src.r;
        let (next, first_joint, first_geom) = (dst.next, dst.first_joint, dst.first_geom);
        *dst = src.clone();
        (dst.next, dst.first_joint, dst.first_geom) = (next, first_joint, first_geom);
        if moved {
            self.geoms_moved(id);
        }
        moved
    }

    /// Task C5i-a: joint `id`'s parameters (anchors, erp, cfm, distance) as `from` has them.
    pub fn copy_joint_from(&mut self, from: &Core, id: usize) {
        let (i, j) = (self.joint_index(id), from.joint_index(id));
        self.world.joints[i].joint.kind = from.world.joints[j].joint.kind.clone();
        if let Some(d) = from.distance.get(id) {
            if self.distance.len() <= id {
                self.distance.resize_with(id + 1, || None);
            }
            self.distance[id] = d.clone();
        }
    }

    /// Joints made so far (contact joints aside): the next joint's id.
    pub fn joints_made(&self) -> usize {
        self.made
    }

    /// A joint's world index by its tail id.
    pub fn joint_index(&self, id: usize) -> usize {
        self.world.joints.len() - 1 - id
    }

    /// `createRigidBody` +0x92ed80 with no split-along spec pending: `rigidbody_spec<Base>`
    /// (+0x937f90) over a new `dBodyCreate` (+0x168c0f0). `no_finite` is the byte +0x280320f
    /// (infinitesimal rotation kept); else the finite rotation mode is set. Returns the body's id.
    pub fn create_body(&mut self, no_finite: bool) -> usize {
        let samples = self.adis.average_samples as usize;
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let body = Body::<f64> {
            alive: true,
            next: None,
            tag: 0,
            first_joint: None,
            first_geom: None,
            // the gyroscopic term; the world's auto-disable and damping flags are clear
            flags: 0x100 | if no_finite { 0 } else { 1 },
            mass: Mass { mass: 1.0, c: [0.0; 4], i: identity },
            inv_i: identity,
            inv_mass: 1.0,
            pos: [0.0; 4],
            r: identity,
            q: [1.0, 0.0, 0.0, 0.0],
            lvel: [0.0; 4],
            avel: [0.0; 4],
            facc: [0.0; 4],
            tacc: [0.0; 4],
            finite_rot_axis: [0.0; 4],
            adis: self.adis,
            adis_timeleft: self.adis.idle_time,
            adis_stepsleft: self.adis.idle_steps,
            average_lvel_buffer: vec![[0.0; 3]; samples],
            average_avel_buffer: vec![[0.0; 3]; samples],
            average_counter: 0,
            average_ready: false,
            dampingp: self.dampingp,
            max_angular_speed: self.max_angular_speed,
        };
        self.world.bodies.insert(0, body);
        self.world.nodes.insert(0, Vec::new());
        // the joints' body indices: every older body moved one along
        for j in &mut self.world.joints {
            j.joint.b0 += 1;
            j.joint.b1 = j.joint.b1.map(|b| b + 1);
        }
        self.world.nodes.iter_mut().flatten().for_each(|n| n.1 = n.1.map(|b| b + 1));
        self.body_geoms.push(Vec::new());
        // +152 = 0x10000: byte +154 (mesh colliders go in the car space) set
        self.user.push([0, 0, 1, 0, 0, 0, 0, 0]);
        self.meshes.push(Vec::new());
        self.world.bodies.len() - 1
    }

    /// `setMassBox` (+0x937780): `dMassSetBoxTotal`'s inertia from f32 mass and sides, then
    /// `dBodySetMass` (+0x91ac70).
    pub fn set_mass_box(&mut self, id: usize, m: f32, x: f32, y: f32, z: f32) {
        let (m, x, y, z) = (m as f64, x as f64, y as f64, z as f64);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let k = m * 0.08333333333333333;
        let mut i = [0.0; 12];
        (i[0], i[5], i[10]) = ((yy + zz) * k, (xx + zz) * k, (xx + yy) * k);
        let body = self.body_mut(id);
        body.mass = Mass { mass: m, c: [0.0; 4], i };
        body.inv_i = invert_pd3(&i).unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        body.inv_mass = 1.0 / m;
    }

    /// `setPosition` (+0x937460, `dBodySetPosition` +0x52c6d0).
    pub fn set_position(&mut self, id: usize, p: [f32; 3]) {
        self.body_set_position(id, p.map(f64::from));
    }

    /// `dBodySetPosition` (+0x168c720): the body's geoms moved.
    fn body_set_position(&mut self, id: usize, p: [f64; 3]) {
        self.body_mut(id).pos[..3].copy_from_slice(&p);
        self.geoms_moved(id);
    }

    /// `dBodySetRotation` (+0x168c7c0): `r` orthogonalised, the quaternion from `r` as given,
    /// the body's geoms moved.
    fn body_set_rotation(&mut self, id: usize, r: [f64; 12]) {
        let body = self.body_mut(id);
        body.r = r;
        orthogonalize(&mut body.r);
        body.q = normalize4(q_from_r(&r));
        self.geoms_moved(id);
    }

    /// `setRotation` (+0x937050): the f32 4x4's rows 1 (up) and 2 (forward) made an orthonormal
    /// basis (columns side, up, forward), then `dBodySetRotation` (+0x91b9b0).
    pub fn set_rotation(&mut self, id: usize, m: &[f32; 16]) {
        let [ux, uy, uz, fx, fy, fz] = [m[4], m[5], m[6], m[8], m[9], m[10]].map(f64::from);
        let c = [uy * fz + uz * -fy, uz * fx + ux * -fz, ux * fy + uy * -fx];
        let d = [fy * c[2] + fz * -c[1], fz * c[0] + fx * -c[2], fx * c[1] + fy * -c[0]];
        let nf = 1.0 / ((fy * fy + fx * fx) + fz * fz).sqrt();
        let nd = 1.0 / ((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]).sqrt();
        let nc = 1.0 / ((c[0] * c[0] + c[1] * c[1]) + c[2] * c[2]).sqrt();
        let r = [nc * c[0], nd * d[0], nf * fx, 0.0, nc * c[1], nd * d[1], nf * fy, 0.0, nc * c[2], nd * d[2], nf * fz, 0.0];
        self.body_set_rotation(id, r);
    }

    /// `stop` (+0x937910): velocities and accumulators zeroed.
    pub fn stop(&mut self, id: usize) {
        let body = self.body_mut(id);
        for v in [&mut body.lvel, &mut body.avel, &mut body.facc, &mut body.tacc] {
            v[..3].fill(0.0);
        }
    }

    /// `addLocalForceAtLocalPos` (+0x936ea0, `dBodyAddRelForceAtRelPos` +0x91b2d0).
    pub fn add_local_force_at_local_pos(&mut self, id: usize, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(id);
        let (f, p, r) = (f.map(f64::from), p.map(f64::from), body.r);
        let rf = rot(&r, f);
        let rp = rot(&r, p);
        add_at(body, rf, rp);
    }

    /// `addForceAtLocalPos` (+0x936f00, `dBodyAddForceAtRelPos` +0x91b000).
    pub fn add_force_at_local_pos(&mut self, id: usize, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(id);
        let (f, p, r) = (f.map(f64::from), p.map(f64::from), body.r);
        let rp = [(p[1] * r[1] + p[0] * r[0]) + p[2] * r[2], (p[1] * r[5] + p[0] * r[4]) + p[2] * r[6], (p[1] * r[9] + p[0] * r[8]) + p[2] * r[10]];
        add_at(body, f, rp);
    }

    /// `addForceAtPos` (+0x936fc0, `dBodyAddForceAtPos` +0x91af10).
    pub fn add_force_at_pos(&mut self, id: usize, f: [f32; 3], p: [f32; 3]) {
        let body = self.body_mut(id);
        let (f, p) = (f.map(f64::from), p.map(f64::from));
        let rp = [p[0] - body.pos[0], p[1] - body.pos[1], p[2] - body.pos[2]];
        add_at(body, f, rp);
    }

    /// `addTorque` (+0x934100).
    pub fn add_torque(&mut self, id: usize, t: [f32; 3]) {
        let body = self.body_mut(id);
        (0..3).for_each(|k| body.tacc[k] = t[k] as f64 + body.tacc[k]);
    }

    /// `getPosition` (+0x9348b0).
    pub fn position(&self, id: usize) -> [f32; 3] {
        let p = self.body(id).pos;
        [p[0] as f32, p[1] as f32, p[2] as f32]
    }

    /// `getVelocity` (+0x934e80).
    pub fn velocity(&self, id: usize) -> [f32; 3] {
        let v = self.body(id).lvel;
        [v[0] as f32, v[1] as f32, v[2] as f32]
    }

    /// `getAngularVelocity` (+0x934870).
    pub fn angular_velocity(&self, id: usize) -> [f32; 3] {
        let v = self.body(id).avel;
        [v[0] as f32, v[1] as f32, v[2] as f32]
    }

    /// `getLocalVelocity` (+0x9346d0): R^T lvel.
    pub fn local_velocity(&self, id: usize) -> [f32; 3] {
        let b = self.body(id);
        f32s(rot_t(&b.r, [b.lvel[0], b.lvel[1], b.lvel[2]]))
    }

    /// `getLocalAngularVelocity` (+0x9347a0): R^T avel.
    pub fn local_angular_velocity(&self, id: usize) -> [f32; 3] {
        let b = self.body(id);
        f32s(rot_t(&b.r, [b.avel[0], b.avel[1], b.avel[2]]))
    }

    /// `worldToLocalNormal` (+0x934ec0): R^T v.
    pub fn world_to_local_normal(&self, id: usize, v: [f32; 3]) -> [f32; 3] {
        f32s(rot_t(&self.body(id).r, v.map(f64::from)))
    }

    /// `worldToLocal` (+0x937520, `dBodyGetPosRelPoint` +0x91b840).
    pub fn world_to_local(&self, id: usize, p: [f32; 3]) -> [f32; 3] {
        let b = self.body(id);
        let p = p.map(f64::from);
        f32s(rot_t(&b.r, [p[0] - b.pos[0], p[1] - b.pos[1], p[2] - b.pos[2]]))
    }

    /// `localToWorld` (+0x9375a0, `dBodyGetRelPointPos` +0x91b510).
    pub fn local_to_world(&self, id: usize, p: [f32; 3]) -> [f32; 3] {
        let b = self.body(id);
        let ([x, y, z], r, o) = (p.map(f64::from), &b.r, &b.pos);
        f32s([(z * r[2] + x * r[0]) + (y * r[1] + o[0]), (y * r[5] + o[1]) + (x * r[4] + z * r[6]), (y * r[9] + o[2]) + (x * r[8] + z * r[10])])
    }

    /// `getPointVelocity` (+0x933e30, `dBodyGetPointVel` +0x91b740).
    pub fn point_velocity(&self, id: usize, p: [f32; 3]) -> [f32; 3] {
        let b = self.body(id);
        let p = p.map(f64::from);
        let d = [p[0] - b.pos[0], p[1] - b.pos[1], p[2] - b.pos[2]];
        let (v, w) = (&b.lvel, &b.avel);
        f32s([w[1] * d[2] - w[2] * d[1] + v[0], w[2] * d[0] - w[0] * d[2] + v[1], w[0] * d[1] - w[1] * d[0] + v[2]])
    }

    /// `getLocalPointVelocity` (+0x936db0, `dBodyGetRelPointVel` +0x91b5e0).
    pub fn local_point_velocity(&self, id: usize, p: [f32; 3]) -> [f32; 3] {
        f32s(rel_point_vel(self.body(id), p.map(f64::from)))
    }

    /// `getWorldMatrix` (+0x937620, +0x91c2c0): R transposed into an f32 4x4 with the position.
    pub fn world_matrix(&self, id: usize) -> [f32; 16] {
        let b = self.body(id);
        let (r, p) = (&b.r, &b.pos);
        [r[0], r[4], r[8], 0.0, r[1], r[5], r[9], 0.0, r[2], r[6], r[10], 0.0, p[0], p[1], p[2], 1.0].map(|x| x as f32)
    }

    /// `getMass` (+0x9378d0).
    pub fn mass(&self, id: usize) -> f32 {
        self.body(id).mass.mass as f32
    }

    /// `getLocalInertia` (+0x937700): the inertia's diagonal.
    pub fn local_inertia(&self, id: usize) -> [f32; 3] {
        let i = &self.body(id).mass.i;
        [i[0] as f32, i[5] as f32, i[10] as f32]
    }

    /// A new non-contact joint at the list's head, attached as `dJointAttach` does (body 0's node
    /// names body 1 and the other way round, each at the head of its body's list).
    fn add_joint(&mut self, kind: JointKind, b1: usize, b2: Option<usize>) -> usize {
        let n = self.world.bodies.len();
        let (w1, w2) = (n - 1 - b1, b2.map(|b| n - 1 - b));
        self.world.nodes.iter_mut().flatten().for_each(|x| x.0 += 1);
        self.world.joints.insert(0, WorldJoint { joint: Joint { kind, b0: w1, b1: w2, feedback: None }, flags: 0, tag: 0 });
        self.world.nodes[w1].insert(0, (0, w2));
        if let Some(w2) = w2 {
            self.world.nodes[w2].insert(0, (0, Some(w1)));
        }
        self.made += 1;
        self.made - 1
    }

    /// `createDistanceJoint` (+0x92f050 → +0x928000): a `dJointCreateDBall` between the bodies,
    /// anchors at the f32 world points, its distance from them. Returns the joint's id.
    pub fn create_distance_joint(&mut self, b1: usize, b2: usize, p1: [f32; 3], p2: [f32; 3]) -> usize {
        let world = &self.world.params;
        let kind = JointKind::DBall { anchor1: [0.0; 4], anchor2: [0.0; 4], erp: world.erp, cfm: world.cfm, target_distance: 0.0 };
        let id = self.add_joint(kind, b1, Some(b2));
        self.update_distance(id);
        self.set_anchors(id, p1.map(f64::from), p2.map(f64::from));
        let target = *self.dball(id).4;
        if self.distance.len() <= id {
            self.distance.resize_with(id + 1, || None);
        }
        self.distance[id] = Some(DistanceJoint { b1, b2, target });
        id
    }

    fn dball(&mut self, id: usize) -> (&mut [f64; 4], &mut [f64; 4], &mut f64, &mut f64, &mut f64) {
        let i = self.joint_index(id);
        let JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } = &mut self.world.joints[i].joint.kind else { panic!("a distance joint") };
        (anchor1, anchor2, erp, cfm, target_distance)
    }

    /// `dJointSetDBallAnchor1/2` (+0x16a37d0 / +0x16a3860), each followed by the distance update.
    fn set_anchors(&mut self, id: usize, p1: [f64; 3], p2: [f64; 3]) {
        let i = self.joint_index(id);
        let (b0, b1) = (self.world.joints[i].joint.b0, self.world.joints[i].joint.b1);
        let a1 = pos_rel_point(&self.world.bodies[b0], p1);
        self.dball(id).0[..3].copy_from_slice(&a1);
        self.update_distance(id);
        let a2 = b1.map_or(p2, |b| pos_rel_point(&self.world.bodies[b], p2));
        self.dball(id).1[..3].copy_from_slice(&a2);
        self.update_distance(id);
    }

    /// +0x16a35e0: the target distance from the anchors in world space.
    fn update_distance(&mut self, id: usize) {
        let i = self.joint_index(id);
        let (b0, b1) = (self.world.joints[i].joint.b0, self.world.joints[i].joint.b1);
        let JointKind::DBall { anchor1, anchor2, .. } = self.world.joints[i].joint.kind else { unreachable!() };
        let world = |b: Option<usize>, a: [f64; 4]| match b {
            Some(b) => {
                let (r, p) = (&self.world.bodies[b].r, &self.world.bodies[b].pos);
                [((a[2] * r[2] + a[0] * r[0]) + a[1] * r[1]) + p[0], ((a[2] * r[6] + a[0] * r[4]) + a[1] * r[5]) + p[1], ((a[2] * r[10] + a[0] * r[8]) + a[1] * r[9]) + p[2]]
            }
            None => [a[0], a[1], a[2]],
        };
        let (p1, p2) = (world(Some(b0), anchor1), world(b1, anchor2));
        let d = ((p1[1] - p2[1]) * (p1[1] - p2[1]) + (p1[0] - p2[0]) * (p1[0] - p2[0])) + (p1[2] - p2[2]) * (p1[2] - p2[2]);
        *self.dball(id).4 = d.sqrt();
    }

    /// The world step (+0x92e4a0 when it steps): once the track is in, the collision (+0x92c2b0)
    /// and its contact joints; the ODE step of `h` (an f32 widened); the contact joints emptied
    /// (+0x168ccc0); each stepped body's geoms moved; the static and dynamic steps alternate.
    /// Returns the contact joints made, in CSP's order.
    pub fn step(&mut self, h: f32) -> Result<Vec<NewJoint>, Unhandled> {
        let joints = self.collide()?;
        self.step_world(h);
        Ok(joints)
    }

    /// The first half of [`Core::step`]: the collision and its contact joints (none before the
    /// track is in).
    pub fn collide(&mut self) -> Result<Vec<NewJoint>, Unhandled> {
        let mut joints = Vec::new();
        if self.track_set {
            self.sync_sides();
            joints = self.world.collide(&mut self.col)?;
            self.world.add_contacts(joints.iter().map(|j| joint_of(j, false)).collect());
        }
        Ok(joints)
    }

    /// The second half of [`Core::step`]: the ODE step and what follows it.
    pub fn step_world(&mut self, h: f32) {
        let islands = self.world.step(h as f64);
        let w = &mut self.world;
        let n = w.joints.iter().take_while(|j| matches!(j.joint.kind, JointKind::Contact { .. })).count();
        w.joints.drain(..n);
        for nodes in &mut w.nodes {
            nodes.retain(|x| x.0 >= n);
            nodes.iter_mut().for_each(|x| x.0 -= n);
        }
        let count = self.world.bodies.len();
        for (bodies, _) in islands {
            for b in bodies {
                self.geoms_moved(count - 1 - b);
            }
        }
        if self.track_set {
            self.col.static_step = !self.col.static_step;
        }
    }

    /// Each car geom's body index and the user-data bits the joint maker reads.
    fn sync_sides(&mut self) {
        let n = self.world.bodies.len();
        for (g, owner) in self.owner.iter().enumerate() {
            if let Some(id) = *owner {
                let u = self.user[id];
                let side = &mut self.col.geoms[g].side;
                side.body = Some(n - 1 - id);
                side.grip = u[7] & 1 != 0;
                side.special = u[0] != 0 && u[1] == 0;
                side.weight = if u[7] & 2 != 0 {
                    1.0
                } else if u[7] & 4 != 0 {
                    f32::from_bits(0x3e99_999a)
                } else {
                    0.0
                };
            }
        }
        self.col.cars_count = self.col.cars.len() as i32;
    }

    /// The car step's "Prepare spaces" (+0x67e2e0 → +0x1688ab0 on the car container): its
    /// `cleanGeoms` (+0x1689360) over every child, each dirty car space cleaned in turn
    /// (+0x16886d0, the dirty children at its list's head), poses and boxes out of date refreshed.
    pub fn prepare(&mut self) -> Result<(), Unhandled> {
        self.sync_sides();
        let bodies = &self.world.bodies;
        let geoms = &mut self.col.geoms;
        for &s in &self.col.cars {
            if geoms[s].flags & GEOM_DIRTY == 0 {
                continue;
            }
            let Shape::Space { children } = &geoms[s].shape else { return Err(Unhandled("a car container child not a space")) };
            for g in children.clone() {
                if geoms[g].flags & GEOM_DIRTY == 0 {
                    break;
                }
                refresh(geoms, g, bodies)?;
                geoms[g].flags &= !GEOM_DIRTY;
            }
            refresh(geoms, s, bodies)?;
            geoms[s].flags &= !GEOM_DIRTY;
        }
        if self.col.track.iter().any(|&t| self.col.geoms[t].flags & GEOM_DIRTY != 0) {
            return Err(Unhandled("a track geom moved"));
        }
        // the container's table, built again once a car space moved (+216 clear or +217 set)
        let c = &mut self.col;
        if self.track_set && (c.cars_dirty || !c.cars_built) {
            let aabbs: Vec<[f64; 6]> = c.cars.iter().map(|&s| c.geoms[s].aabb).collect();
            c.cars_table = HashSpace::build(&aabbs, c.cars_levels.0, c.cars_levels.1);
            (c.cars_built, c.cars_dirty) = (true, false);
        }
        Ok(())
    }

    /// Task C5h: the track's ray meshes (after [`Core::set_track`]); `embree`: CSP's Embree on
    /// (`ODE_BATCH_RAYCAST=0`, the default) with its device string and robust flag (+0x2803200),
    /// the scene built here when this build has Embree; `below`: +0x2812c7f.
    pub fn set_rays(&mut self, meshes: &[RayMesh], embree: Option<(&std::ffi::CStr, bool)>, below: bool) -> Result<(), String> {
        self.ray_geoms = meshes.iter().map(|m| self.col.track[m.geom]).collect();
        self.below = below;
        #[cfg(feature = "embree")]
        if let Some((config, robust)) = embree {
            let trace: Vec<Option<TraceMesh>> = meshes.iter().map(|m| Some(TraceMesh { vertices: &m.vertices, indices: &m.indices, two_sided: m.two_sided })).collect();
            self.scene = Some(std::sync::Arc::new(Scene::new(config, robust, &trace)?));
        }
        #[cfg(not(feature = "embree"))]
        let _ = embree;
        Ok(())
    }

    /// Whether the rays use Embree's scene now: from the first tyre ray on, the set-up's rays all
    /// ODE's. CSP builds the scene on a pool thread from its first ray, so when it is ready is a
    /// race; in every run seen it was ready before the first tyre ray (16,986 set-up rays on Spa,
    /// the switch after 9,319 to all of them). The oracle pins CSP to this.
    pub fn embree_ready(&self) -> bool {
        #[cfg(feature = "embree")]
        return self.scene.is_some() && self.tyre_seen;
        #[cfg(not(feature = "embree"))]
        false
    }

    /// Task C5h: a track ray (+0x932900) from `org` along `dir` for `length`; `tyre` with the
    /// tyre raycaster's ray geom (+0x92b970), else none (the set-up's, +0x92fac0). Embree's scene
    /// once ready ([`Core::embree_ready`]), ODE's ray (+0x926270, flags first contact and back-face cull as +0x92fe80 and
    /// +0x924ff0 make them) before, without Embree, or for a downward tyre ray Embree hits from
    /// below. `None`: Embree missed (hit byte and mesh cleared, point and normal left as they were).
    pub fn cast(&mut self, org: [f32; 3], dir: [f32; 3], length: f32, tyre: bool) -> Result<Option<TrackRay>, Unhandled> {
        self.tyre_seen |= tyre;
        #[cfg(feature = "embree")]
        if self.embree_ready() {
            match self.scene.as_ref().unwrap().cast(org, dir, length, tyre, self.below) {
                Cast::Miss => return Ok(None),
                Cast::Hit { point, normal, mesh } => return Ok(Some(TrackRay { point, normal, mesh: Some(mesh as usize) })),
                Cast::Ode => {}
            }
        }
        self.clean_track()?;
        let ray = Ray { flags: RAY_FIRST_CONTACT | RAY_BACKFACE_CULL, category: !0, collide: !0, length: length as f64 };
        Ok(Some(match csp_raycast::cast(&self.col, org, dir, &ray)? {
            None => TrackRay { point: [0.0; 3], normal: [0.0, 1.0, 0.0], mesh: None },
            Some(h) => TrackRay { point: h.point, normal: h.normal, mesh: Some(self.ray_geoms.iter().position(|&g| g == h.geom).ok_or(Unhandled("a ray hit off the mesh list"))?) },
        }))
    }

    /// The track space's `cleanGeoms` (+0x1689360) as ODE's ray (`dSpaceCollide2`) does it: stale
    /// boxes made, dirty marks cleared, its table built again.
    pub fn clean_track(&mut self) -> Result<(), Unhandled> {
        let c = &mut self.col;
        if !c.track.iter().any(|&t| c.geoms[t].flags & GEOM_DIRTY != 0) {
            return Ok(());
        }
        for &t in &c.track {
            refresh(&mut c.geoms, t, &[])?;
            c.geoms[t].flags &= !GEOM_DIRTY;
        }
        let aabbs: Vec<[f64; 6]> = c.track.iter().map(|&t| c.geoms[t].aabb).collect();
        c.table = HashSpace::build(&aabbs, self.track_levels.0, self.track_levels.1);
        Ok(())
    }

    /// The track as CSP loaded it (its trimeshes, their models and hash table) and the world's
    /// collision settings: collision on from here.
    #[allow(clippy::too_many_arguments)]
    pub fn set_track(&mut self, mut geoms: Vec<Geom>, meshes: Vec<TrackMesh>, track_level: i32, track_count: i32, levels: (i32, i32), cars_level: i32, static_step: bool, aabb_limit: i32, full_box_box: bool) {
        let (g0, m0) = (self.col.geoms.len(), self.col.meshes.len());
        for g in &mut geoms {
            if let Shape::Mesh { mesh: Some(m), .. } = &mut g.shape {
                *m += m0;
            }
        }
        let aabbs: Vec<[f64; 6]> = geoms.iter().map(|g| g.aabb).collect();
        let c = &mut self.col;
        c.table = HashSpace::build(&aabbs, levels.0, levels.1);
        self.track_levels = levels;
        c.track = (g0..g0 + geoms.len()).collect();
        self.owner.extend(geoms.iter().map(|_| None));
        self.parent.extend(geoms.iter().map(|_| None));
        c.geoms.extend(geoms);
        c.meshes.extend(meshes.into_iter().map(std::sync::Arc::new));
        (c.track_level, c.track_count, c.cars_level, c.static_step, c.aabb_limit, c.full_box_box) = (track_level, track_count, cars_level, static_step, aabb_limit, full_box_box);
        self.track_set = true;
    }

    /// A new geom (`dxGeom`'s constructor, no space): flags dirty, box out of date, enabled,
    /// placeable (8) but for a space; category and collide bits all set.
    fn new_geom(&mut self, id: usize, class: i32, shape: Shape) -> usize {
        let side = Side { class, no_data: true, ..Default::default() };
        let flags = if class == 11 { NEW_GEOM } else { NEW_GEOM | 8 };
        self.col.geoms.push(Geom { id, side, flags, category: !0, collide: !0, offset: None, pos: [0.0; 3], r: [0.0; 12], aabb: [0.0; 6], shape });
        self.owner.push(None);
        self.parent.push(None);
        self.col.geoms.len() - 1
    }

    /// `dGeomMoved` (+0x16881b0): an offset geom's pose out of date; up the spaces while the geom
    /// was clean: marked dirty with its box, moved to its space's head; the rest marked.
    fn moved(&mut self, g: usize) {
        let geoms = &mut self.col.geoms;
        if geoms[g].offset.is_some() {
            geoms[g].flags |= GEOM_POSR_BAD;
        }
        let mut a = Some(g);
        while let Some(x) = a {
            let Some(p) = self.parent[x] else { break };
            if geoms[x].flags & GEOM_DIRTY != 0 {
                break;
            }
            geoms[x].flags |= GEOM_DIRTY | GEOM_AABB_BAD;
            if p == CARS {
                // the container is a hash space: its `dirty` (+0x1688aa0) only asks for a table
                // rebuild (+217) and keeps the list; its own flags are never read
                self.col.cars_dirty = true;
                break;
            }
            let Shape::Space { children } = &mut geoms[p].shape else { unreachable!("a parent not a space") };
            children.retain(|&k| k != x);
            children.insert(0, x);
            a = Some(p);
        }
        while let Some(x) = a {
            geoms[x].flags |= GEOM_DIRTY | GEOM_AABB_BAD;
            a = self.parent[x].filter(|&p| p != CARS);
        }
    }

    /// Each geom of body `id`, in the body's list order.
    fn geoms_moved(&mut self, id: usize) {
        for g in self.body_geoms[id].clone() {
            self.moved(g);
        }
    }

    /// `dSpaceAdd` (vtable +40): at the list's head, the space moved.
    fn space_add(&mut self, space: usize, g: usize) {
        self.parent[g] = Some(space);
        let list = match space {
            CARS => &mut self.col.cars,
            _ => match &mut self.col.geoms[space].shape {
                Shape::Space { children } => children,
                _ => unreachable!("not a space"),
            },
        };
        list.insert(0, g);
        if space != CARS {
            self.moved(space);
        }
    }

    /// +0x91bdb0 on the map (+72) path: the car space of `key`, made (`dSimpleSpaceCreate` in
    /// the car container, +0x168a3f0) the first time.
    fn car_space(&mut self, key: u32) -> usize {
        if let Some(&(_, s)) = self.spaces.iter().find(|x| x.0 == key) {
            return s;
        }
        let s = self.new_geom(0, 11, Shape::Space { children: Vec::new() });
        // the space's data is its key (a set low bit counts as no data)
        self.col.geoms[s].side.no_data = key == 0 || key & 1 != 0;
        self.space_add(CARS, s);
        self.spaces.push((key, s));
        s
    }

    /// `dGeomSetBody` (+0x1687a40): the body's pose shared, at the head of its geom list, moved.
    fn geom_set_body(&mut self, g: usize, id: usize) {
        self.owner[g] = Some(id);
        self.body_geoms[id].insert(0, g);
        let b = self.body(id);
        let (pos, r) = ([b.pos[0], b.pos[1], b.pos[2]], b.r);
        let geom = &mut self.col.geoms[g];
        (geom.pos, geom.r) = (pos, r);
        self.moved(g);
    }

    /// `dGeomCreateOffset` (+0x1687f40) when the geom has none: zero, identity, pose out of date.
    fn create_offset(&mut self, g: usize) {
        let geom = &mut self.col.geoms[g];
        if geom.offset.is_none() {
            geom.offset = Some(([0.0; 3], IDENTITY));
            geom.flags |= GEOM_POSR_BAD;
        }
    }

    /// `dGeomSetOffsetPosition` (+0x1688030).
    fn set_offset_position(&mut self, g: usize, p: [f64; 3]) {
        self.create_offset(g);
        self.col.geoms[g].offset.as_mut().unwrap().0 = p;
        self.moved(g);
    }

    /// `dGeomSetRotation` (+0x1687da0) of an offset geom: the body turned so the geom's final
    /// rotation is `r` at its current final position (`getBodyPosr` +0x1687900).
    fn geom_set_rotation(&mut self, g: usize, r: [f64; 12]) -> Result<(), Unhandled> {
        let id = self.owner[g].expect("a geom on a body");
        self.sync_sides();
        let geom = &mut self.col.geoms[g];
        posr(geom, &self.world.bodies)?;
        let (o, or) = geom.offset.expect("an offset");
        let t = [or[0], or[4], or[8], 0.0, or[1], or[5], or[9], 0.0, or[2], or[6], or[10], 0.0];
        let br = product(&r, &t);
        let p = geom.pos;
        let bp = [
            p[0] - ((o[1] * br[1] + o[0] * br[0]) + o[2] * br[2]),
            p[1] - ((o[1] * br[5] + o[0] * br[4]) + o[2] * br[6]),
            p[2] - ((o[1] * br[9] + o[0] * br[8]) + o[2] * br[10]),
        ];
        self.body_set_rotation(id, br);
        self.body_set_position(id, bp);
        Ok(())
    }

    /// `addBoxCollider` (+0x933cc0): a box of f32 sides `size` at `pos` in body `id`'s frame, in
    /// the car space of `key`; a box lower than `min_height` (+0x27f9950, raised at load by
    /// EXTEND_COLLIDER_BOXES) is made that high, its centre raised by half the difference. `gid` is
    /// CSP's geom (what contacts name it by).
    #[allow(clippy::too_many_arguments)]
    pub fn add_box_collider(&mut self, id: usize, pos: [f32; 3], size: [f32; 3], category: u32, collide: u32, key: u32, min_height: f32, gid: usize) -> Result<usize, Unhandled> {
        let (mut y, mut h) = (pos[1], size[1]);
        if !(h >= min_height) {
            y += (min_height - h) * 0.5;
            h = min_height;
        }
        let sides = [size[0] as f64, h as f64, size[2] as f64];
        if sides.contains(&0.0) {
            return Err(Unhandled("a box with a zero side"));
        }
        let space = self.car_space(key);
        let g = self.new_geom(gid, 1, Shape::Box { sides, tc: Default::default() });
        self.space_add(space, g);
        self.geom_set_body(g, id);
        self.set_offset_position(g, [pos[0] as f64, y as f64, pos[2] as f64]);
        let r = self.body(id).r;
        self.geom_set_rotation(g, r)?;
        (self.col.geoms[g].collide, self.col.geoms[g].category) = (collide, category);
        Ok(g)
    }

    /// `addMeshCollider` (+0x9337b0, the trimesh path): a trimesh (`mesh` and the shape's model
    /// facts taken as CSP built them) on body `id`, its offset from the f32 4x4 `m` (rows: side,
    /// up, forward, position), then into the car space of `key` (+0x939880).
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh_collider(&mut self, id: usize, m: &[f32; 16], category: u32, collide: u32, key: u32, mesh: Option<TrackMesh>, shape: Shape, gid: usize) -> usize {
        let Shape::Mesh { tc_on, key: tc_key, bounds, .. } = shape else { panic!("a trimesh") };
        let mesh = mesh.map(|m| {
            self.col.meshes.push(std::sync::Arc::new(m));
            self.col.meshes.len() - 1
        });
        // shortcut: AABBTest walks the tree only at an identity pose, which a car's mesh never
        // has once placed; make tree_test follow the pose if a car ever sits at one.
        let g = self.new_geom(gid, 8, Shape::Mesh { mesh, tc_on, key: tc_key, tree_test: false, bounds });
        // geom +16 is the collision mesh object
        self.col.geoms[g].side.no_data = false;
        self.geom_set_body(g, id);
        self.create_offset(g);
        let mf = m.map(f64::from);
        self.col.geoms[g].offset.as_mut().unwrap().1 = [mf[0], mf[4], mf[8], 0.0, mf[1], mf[5], mf[9], 0.0, mf[2], mf[6], mf[10], 0.0];
        self.moved(g);
        // the mesh's centre (object +44, zero on the trimesh path) through `m`, in f32
        let p: [f64; 3] = std::array::from_fn(|j| (((0.0f32 * m[j] + 0.0 * m[4 + j]) + 0.0 * m[8 + j]) + m[12 + j]) as f64);
        self.set_offset_position(g, p);
        (self.col.geoms[g].collide, self.col.geoms[g].category) = (collide, category);
        self.meshes[id].push(g);
        if key as i32 != -569_524_499 && self.user[id][2] != 0 {
            let space = self.car_space(key);
            self.space_add(space, g);
        }
        g
    }

    /// +0x933710 (cphys slot): the wrapper's byte +159 bits `mask` set or cleared.
    pub fn set_user_bits(&mut self, id: usize, mask: u8, on: bool) {
        let u = &mut self.user[id][7];
        *u = if on { *u | mask } else { *u & !mask };
    }

    /// +0x933750: mesh collider `i`'s collide bits.
    pub fn set_mesh_collide(&mut self, id: usize, i: usize, collide: u32) {
        if let Some(&g) = self.meshes[id].get(i) {
            self.col.geoms[g].collide = collide;
        }
    }

    /// `setERPCFM` on a distance joint (+0x928310): values <= 0 are ignored.
    pub fn set_erp_cfm(&mut self, id: usize, erp: f32, cfm: f32) {
        let (_, _, e, c, _) = self.dball(id);
        if erp > 0.0 {
            *e = erp as f64;
        }
        if cfm > 0.0 {
            *c = cfm as f64;
        }
    }

    /// `reseatDistanceJointLocal` (+0x92fbb0 → +0x928350): anchor 1 at the f32 point `p` in body
    /// 1's frame, anchor 2 at `q` in body 2's, the distance put back to the one made with.
    /// shortcut: q = (FLT_MAX, 0, 1) (anchor 1 moved by its point velocity) not ported, no car
    /// seen uses it; port it when one does.
    pub fn reseat(&mut self, id: usize, p: [f32; 3], q: [f32; 3]) {
        assert!(q != [f32::MAX, 0.0, 1.0], "reseat by point velocity not ported");
        let d = self.distance[id].as_ref().expect("a distance joint");
        let (b1, b2, target) = (d.b1, d.b2, d.target);
        let [x, y, z] = p.map(f64::from);
        let (a, b) = (self.body(b1), self.body(b2));
        let (ra, pa, rb, pb) = (&a.r, &a.pos, &b.r, &b.pos);
        let p1 = [(z * ra[2] + x * ra[0]) + (y * ra[1] + pa[0]), (z * ra[6] + x * ra[4]) + (y * ra[5] + pa[1]), (z * ra[10] + x * ra[8]) + (y * ra[9] + pa[2])];
        let [x, y, z] = q.map(f64::from);
        let p2 = [(z * rb[2] + y * rb[1]) + (x * rb[0] + pb[0]), (z * rb[6] + x * rb[4]) + (y * rb[5] + pb[1]), (z * rb[10] + x * rb[8]) + (y * rb[9] + pb[2])];
        self.set_anchors(id, p1, p2);
        *self.dball(id).4 = target;
    }

    /// `createFixedJoint` (+0x92fd20): `dJointCreateFixed` attached to both bodies, then
    /// `dJointSetFixed` (+0x16a12f0) keeps their relative pose.
    pub fn create_fixed_joint(&mut self, b1: usize, b2: usize) -> usize {
        let erp = self.world.params.erp;
        let cfm = self.world.params.cfm;
        let id = self.add_joint(JointKind::Fixed { qrel: [0.0; 4], offset: [0.0; 4], erp, cfm }, b1, Some(b2));
        let (a, b) = (self.body(b1), self.body(b2));
        let d = [a.pos[0] - b.pos[0], a.pos[1] - b.pos[1], a.pos[2] - b.pos[2]];
        let r = &a.r;
        let offset = [(d[1] * r[4] + d[0] * r[0]) + d[2] * r[8], (d[1] * r[5] + d[0] * r[1]) + d[2] * r[9], (d[1] * r[6] + d[0] * r[2]) + d[2] * r[10], 0.0];
        let (qa, qb) = (a.q, b.q);
        let qrel = [
            ((qb[2] * qa[2] + qb[3] * qa[3]) + qa[0] * qb[0]) + qb[1] * qa[1],
            ((qb[1] * qa[0] - qb[0] * qa[1]) - qb[3] * qa[2]) + qb[2] * qa[3],
            ((qb[2] * qa[0] - qa[2] * qb[0]) - qa[3] * qb[1]) + qb[3] * qa[1],
            ((qb[3] * qa[0] - qa[3] * qb[0]) - qb[2] * qa[1]) + qa[2] * qb[1],
        ];
        let i = self.joint_index(id);
        self.world.joints[i].joint.kind = JointKind::Fixed { qrel, offset, erp, cfm };
        id
    }
}

fn f32s(v: [f64; 3]) -> [f32; 3] {
    v.map(|x| x as f32)
}

/// R v as `dBodyAddRelForceAtRelPos` turns its vectors.
fn rot(r: &[f64; 12], v: [f64; 3]) -> [f64; 3] {
    [(r[0] * v[0] + r[1] * v[1]) + r[2] * v[2], (r[4] * v[0] + r[5] * v[1]) + r[6] * v[2], (r[8] * v[0] + r[9] * v[1]) + r[10] * v[2]]
}

/// R^T v as the getters turn theirs.
fn rot_t(r: &[f64; 12], v: [f64; 3]) -> [f64; 3] {
    [(v[1] * r[4] + v[0] * r[0]) + v[2] * r[8], (v[1] * r[5] + v[0] * r[1]) + v[2] * r[9], (v[1] * r[6] + v[0] * r[2]) + v[2] * r[10]]
}

/// `dBodyGetPosRelPoint` (+0x168c980).
fn pos_rel_point(b: &Body<f64>, p: [f64; 3]) -> [f64; 3] {
    rot_t(&b.r, [p[0] - b.pos[0], p[1] - b.pos[1], p[2] - b.pos[2]])
}

/// `dBodyGetRelPointVel` (+0x91b5e0).
fn rel_point_vel(b: &Body<f64>, p: [f64; 3]) -> [f64; 3] {
    let r = &b.r;
    let rp = [(p[1] * r[1] + p[0] * r[0]) + p[2] * r[2], (p[1] * r[5] + p[0] * r[4]) + p[2] * r[6], (p[1] * r[9] + p[0] * r[8]) + p[2] * r[10]];
    let (v, w) = (&b.lvel, &b.avel);
    [rp[2] * w[1] - rp[1] * w[2] + v[0], rp[0] * w[2] - rp[2] * w[0] + v[1], rp[1] * w[0] - rp[0] * w[1] + v[2]]
}

/// facc += f, tacc += rp x f, in +0x91b2d0's order.
fn add_at(body: &mut Body<f64>, f: [f64; 3], rp: [f64; 3]) {
    (0..3).for_each(|k| body.facc[k] = f[k] + body.facc[k]);
    let t = &mut body.tacc;
    t[0] = rp[1] * f[2] - rp[2] * f[1] + t[0];
    t[1] = rp[2] * f[0] - rp[0] * f[2] + t[1];
    t[2] = rp[0] * f[1] - rp[1] * f[0] + t[2];
}

/// `dInvertPDMatrix` for 3x3 (+0x168dd00): Cholesky (+0x168d930), then each column solved
/// (+0x168db60). None when not positive definite.
fn invert_pd3(a: &[f64; 12]) -> Option<[f64; 12]> {
    let mut l = *a;
    let mut recip = [0.0; 3];
    for i in 0..3 {
        for j in 0..i {
            let mut sum = l[4 * i + j];
            for k in 0..j {
                sum -= l[4 * j + k] * l[4 * i + k];
            }
            l[4 * i + j] = sum * recip[j];
        }
        let mut sum = l[4 * i + i];
        for k in 0..i {
            sum -= l[4 * i + k] * l[4 * i + k];
        }
        if sum <= 0.0 {
            return None;
        }
        let s = sum.sqrt();
        l[4 * i + i] = s;
        recip[i] = 1.0 / s;
    }
    let mut inv = [0.0; 12];
    for c in 0..3 {
        let mut x = [0.0; 3];
        x[c] = 1.0;
        let mut y = [0.0; 3];
        for i in 0..3 {
            let mut sum = 0.0;
            for k in 0..i {
                sum += l[4 * i + k] * y[k];
            }
            y[i] = (x[i] - sum) / l[4 * i + i];
        }
        for i in (0..3).rev() {
            let mut sum = 0.0;
            for k in i + 1..3 {
                sum += x[k] * l[4 * k + i];
            }
            x[i] = (y[i] - sum) / l[4 * i + i];
        }
        (0..3).for_each(|k| inv[4 * k + c] = x[k]);
    }
    Some(inv)
}

/// `dxOrthogonalizeR` (+0x168b060) as CSP's ODE has it: row 1 made orthogonal in a copy only (the
/// matrix keeps its own row 1), row 2 their cross product.
fn orthogonalize(m: &mut [f64; 12]) -> bool {
    if m[..3].iter().all(|&x| x == 0.0) {
        return false;
    }
    let n0 = (m[0] * m[0] + m[1] * m[1]) + m[2] * m[2];
    let dot = (m[1] * m[5] + m[4] * m[0]) + m[2] * m[6];
    let mut row1 = [m[4], m[5], m[6]];
    let copy = dot != 0.0;
    if copy {
        let k = dot / n0;
        row1 = [m[4] - k * m[0], m[5] - k * m[1], m[6] - k * m[2]];
    }
    if row1.iter().all(|&x| x == 0.0) {
        return false;
    }
    if n0 != 1.0 {
        let mut r0 = [m[0], m[1], m[2]];
        safe_normalize3(&mut r0);
        m[..3].copy_from_slice(&r0);
    }
    if (row1[0] * row1[0] + row1[1] * row1[1]) + row1[2] * row1[2] != 1.0 {
        safe_normalize3(&mut row1);
    }
    if !copy {
        m[4..7].copy_from_slice(&row1);
    }
    let [a, b, c] = row1;
    (m[8], m[9], m[10]) = (m[1] * c - b * m[2], a * m[2] - m[0] * c, b * m[0] - m[1] * a);
    (m[3], m[7], m[11]) = (0.0, 0.0, 0.0);
    true
}

/// `dxSafeNormalize3` (+0x168aed0).
fn safe_normalize3(v: &mut [f64; 3]) -> bool {
    let a = v.map(f64::abs);
    let big = if a[1] > a[0] {
        if a[2] > a[1] { 2 } else { 1 }
    } else if a[2] > a[0] {
        2
    } else if a[0] > 0.0 {
        0
    } else {
        return false;
    };
    let s = 1.0 / a[big];
    let (i, j) = match big {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let (x, y) = (s * v[i], s * v[j]);
    let l = 1.0 / ((x * x + 1.0) + y * y).sqrt();
    v[big] = l.copysign(v[big]);
    v[i] = x * l;
    v[j] = y * l;
    true
}

/// `dQfromR` (+0x168bae0).
fn q_from_r(r: &[f64; 12]) -> [f64; 4] {
    let tr = (r[0] + r[5]) + r[10];
    if tr >= 0.0 {
        let s = (tr + 1.0).sqrt();
        let h = 0.5 / s;
        return [s * 0.5, (r[9] - r[6]) * h, (r[2] - r[8]) * h, (r[4] - r[1]) * h];
    }
    let branch = if r[5] > r[0] {
        if r[10] > r[5] { 2 } else { 1 }
    } else if r[10] > r[0] {
        2
    } else {
        0
    };
    match branch {
        1 => {
            let s = ((r[5] - (r[10] + r[0])) + 1.0).sqrt();
            let h = 0.5 / s;
            [(r[2] - r[8]) * h, (r[4] + r[1]) * h, s * 0.5, (r[9] + r[6]) * h]
        }
        2 => {
            let s = ((r[10] - (r[0] + r[5])) + 1.0).sqrt();
            let h = 0.5 / s;
            [(r[4] - r[1]) * h, (r[8] + r[2]) * h, (r[9] + r[6]) * h, s * 0.5]
        }
        _ => {
            let s = ((r[0] - (r[10] + r[5])) + 1.0).sqrt();
            let h = 0.5 / s;
            [(r[9] - r[6]) * h, s * 0.5, (r[4] + r[1]) * h, (r[8] + r[2]) * h]
        }
    }
}

/// `dNormalize4` (+0x168ae40).
fn normalize4(q: [f64; 4]) -> [f64; 4] {
    let l = ((q[0] * q[0] + q[1] * q[1]) + q[3] * q[3]) + q[2] * q[2];
    if l <= 0.0 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let s = 1.0 / l.sqrt();
    q.map(|x| x * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_mass_inverse_is_diagonal_reciprocal() {
        let mut core = Core::new();
        let b = core.create_body(false);
        core.set_mass_box(b, 12.0, 1.0, 2.0, 3.0);
        let body = core.body(b);
        assert!((body.mass.i[0] - 13.0).abs() < 1e-12 && (body.inv_i[0] * body.mass.i[0] - 1.0).abs() < 1e-15 && body.inv_i[1] == 0.0);
    }

    #[test]
    fn rotation_identity_stays() {
        let mut core = Core::new();
        let b = core.create_body(false);
        let mut m = [0.0f32; 16];
        (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
        core.set_rotation(b, &m);
        assert_eq!(core.body(b).q, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(core.world_matrix(b), m);
    }

    #[test]
    fn box_collider_placed_by_prepare_after_a_move() {
        let mut core = Core::new();
        let b = core.create_body(false);
        let g = core.add_box_collider(b, [0.0, 0.1, 0.0], [2.0, 0.2, 4.0], 4, 1, 1, 0.5, 7).unwrap();
        core.set_position(b, [1.0, 2.0, 3.0]);
        core.prepare().unwrap();
        let geom = &core.col.geoms[g];
        // the 0.2 high box made 0.5 high, its centre raised by 0.15
        assert_eq!(geom.pos[0], 1.0);
        assert!((geom.pos[1] - 2.25).abs() < 1e-6 && (geom.aabb[3] - geom.aabb[2] - 0.5).abs() < 1e-6);
        assert_eq!(geom.flags & (GEOM_DIRTY | GEOM_POSR_BAD | GEOM_AABB_BAD), 0);
        // the space was moved to the container's head and cleaned with it
        assert_eq!(core.col.geoms[core.col.cars[0]].flags & GEOM_DIRTY, 0);
    }
}
