// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5i-a part 2: `PhysicsCore`'s second backend. In CSP mode a core's bodies, joints and
//! shapes live in CSP 0.3.2-preview2's double-precision core (`rustyac_ode::csp_core`, its
//! wrappers, collision and rays) instead of Vanilla's f32 `World`; `PhysicsCore` hands each call
//! here (docs/csp/core.md, C5i-a part 2).
//!
//! A [`RigidBody`]'s `id` is the CSP body id (creation order, as its `index`), a joint's `JointId`
//! the CSP joint id, a `GeomId` the index into the CSP collision's geoms.

use rustyac_ode::csp_core::Core;
use rustyac_ode::csp_step::JointKind;
use rustyac_ode::{BodyId, GeomId, GeomRef, JointId};

use crate::car::body::{CollisionEvent, RigidBody, Shape};
use crate::vecmath::{Mat44f, Vec3f};

/// What contacts name a car geom by: its index with this bit (the track's geoms are named
/// `1..=n`, their mesh index plus one).
const CAR_GEOM: usize = 1 << 30;

#[derive(Clone)]
pub struct CspBody {
    pub core: Core,
    /// `+0x27f9950`: the floor of the next box colliders ([`super::core_car::box_floor`]).
    pub box_floor: f32,
}

fn a3(v: &Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn v3(a: [f32; 3]) -> Vec3f {
    Vec3f::new(a[0], a[1], a[2])
}

fn id(body: RigidBody) -> usize {
    body.id.0 as usize
}

pub fn rigid_body(id: usize) -> RigidBody {
    RigidBody { id: BodyId(id as u32), index: id as u32 }
}

impl CspBody {
    /// A core with the track of `track` in it (a copy of the track's own, [`super::core_track::core`]).
    pub fn new(track: &crate::track::Track) -> Result<CspBody, String> {
        Ok(CspBody { core: track.csp_core()?.core.clone(), box_floor: 0.0 })
    }

    pub fn create_rigid_body(&mut self) -> usize {
        // shortcut: `no_finite` is CSP's byte +0x280320f, false (finite rotation) with the default
        // settings; read it from the settings when a car changes it
        self.core.create_body(false)
    }

    pub fn world_matrix(&self, body: RigidBody) -> Mat44f {
        let m = self.core.world_matrix(id(body));
        Mat44f { m: std::array::from_fn(|r| std::array::from_fn(|c| m[r * 4 + c])) }
    }

    pub fn local_to_world(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.core.local_to_world(id(body), a3(p)))
    }

    pub fn world_to_local(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.core.world_to_local(id(body), a3(p)))
    }

    /// R v (`dBodyVectorToWorld`; no CSP call of it was recorded).
    pub fn local_to_world_normal(&self, body: RigidBody, v: &Vec3f) -> Vec3f {
        let r = &self.core.body(id(body)).r;
        let (x, y, z) = (v.x as f64, v.y as f64, v.z as f64);
        Vec3f::new(((r[0] * x + r[1] * y) + r[2] * z) as f32, ((r[4] * x + r[5] * y) + r[6] * z) as f32, ((r[8] * x + r[9] * y) + r[10] * z) as f32)
    }

    pub fn world_to_local_normal(&self, body: RigidBody, v: &Vec3f) -> Vec3f {
        v3(self.core.world_to_local_normal(id(body), a3(v)))
    }

    pub fn position(&self, body: RigidBody) -> Vec3f {
        v3(self.core.position(id(body)))
    }

    pub fn velocity(&self, body: RigidBody) -> Vec3f {
        v3(self.core.velocity(id(body)))
    }

    pub fn local_velocity(&self, body: RigidBody) -> Vec3f {
        v3(self.core.local_velocity(id(body)))
    }

    pub fn local_angular_velocity(&self, body: RigidBody) -> Vec3f {
        v3(self.core.local_angular_velocity(id(body)))
    }

    pub fn angular_velocity(&self, body: RigidBody) -> Vec3f {
        v3(self.core.angular_velocity(id(body)))
    }

    pub fn local_point_velocity(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.core.local_point_velocity(id(body), a3(p)))
    }

    pub fn point_velocity(&self, body: RigidBody, p: &Vec3f) -> Vec3f {
        v3(self.core.point_velocity(id(body), a3(p)))
    }

    pub fn mass(&self, body: RigidBody) -> f32 {
        self.core.mass(id(body))
    }

    pub fn set_rotation(&mut self, body: RigidBody, m: &Mat44f) {
        self.core.set_rotation(id(body), &std::array::from_fn(|i| m.m[i / 4][i % 4]));
    }

    /// `dBodySetLinearVel` / `dBodySetAngularVel` (no CSP call of them was recorded).
    pub fn set_velocities(&mut self, body: RigidBody, v: Option<&Vec3f>, w: Option<&Vec3f>) {
        let b = self.core.body_mut(id(body));
        if let Some(v) = v {
            b.lvel[..3].copy_from_slice(&[v.x as f64, v.y as f64, v.z as f64]);
        }
        if let Some(w) = w {
            b.avel[..3].copy_from_slice(&[w.x as f64, w.y as f64, w.z as f64]);
        }
    }

    /// The force calls by tape kind; the local-force kinds no CSP call was recorded for go
    /// through world vectors.
    pub fn force(&mut self, body: RigidBody, kind: u32, a: &Vec3f, b: &Vec3f) {
        use crate::car::body::kind::*;
        let i = id(body);
        match kind {
            ADD_FORCE_AT_POS => self.core.add_force_at_pos(i, a3(a), a3(b)),
            ADD_FORCE_AT_LOCAL_POS => self.core.add_force_at_local_pos(i, a3(a), a3(b)),
            ADD_LOCAL_FORCE => self.core.add_local_force_at_local_pos(i, a3(a), [0.0; 3]),
            ADD_LOCAL_FORCE_AT_POS => {
                let f = self.local_to_world_normal(body, a);
                self.core.add_force_at_pos(i, a3(&f), a3(b));
            }
            ADD_LOCAL_FORCE_AT_LOCAL_POS => self.core.add_local_force_at_local_pos(i, a3(a), a3(b)),
            ADD_TORQUE => self.core.add_torque(i, a3(a)),
            ADD_LOCAL_TORQUE => {
                let t = self.local_to_world_normal(body, a);
                self.core.add_torque(i, a3(&t));
            }
            STOP => self.core.stop(i),
            _ => unreachable!("not a force call"),
        }
    }

    pub fn distance_joint(&mut self, body1: RigidBody, body2: RigidBody, p1: &Vec3f, p2: &Vec3f) -> (JointId, f32) {
        let j = self.core.create_distance_joint(id(body1), id(body2), a3(p1), a3(p2));
        let JointKind::DBall { target_distance, .. } = self.core.world.joints[self.core.joint_index(j)].joint.kind else { unreachable!() };
        (JointId(j as u32), target_distance as f32)
    }

    /// `setERPCFM`: a distance joint through CSP's wrapper (+0x928310), any other joint's fields
    /// written the same way (values <= 0 ignored).
    pub fn joint_set_erp_cfm(&mut self, joint: JointId, erp: f32, cfm: f32) {
        let i = self.core.joint_index(joint.0 as usize);
        match &mut self.core.world.joints[i].joint.kind {
            JointKind::DBall { .. } => self.core.set_erp_cfm(joint.0 as usize, erp, cfm),
            JointKind::Fixed { erp: e, cfm: c, .. } | JointKind::Ball { erp: e, cfm: c, .. } => {
                if erp > 0.0 {
                    *e = erp as f64;
                }
                if cfm > 0.0 {
                    *c = cfm as f64;
                }
            }
            JointKind::Contact { .. } => unreachable!("a contact joint's erp"),
        }
    }

    pub fn add_box_collider(&mut self, body: RigidBody, centre: &Vec3f, size: &Vec3f, category: u32, mask: u32, space: u32) -> GeomId {
        let gid = CAR_GEOM | self.core.col.geoms.len();
        let g = self.core.add_box_collider(id(body), a3(centre), a3(size), category, mask, space, self.box_floor, gid).unwrap_or_else(|e| panic!("CSP box collider: {}", e.0));
        GeomId(g as u32)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh_collider(&mut self, body: RigidBody, vertices: Vec<[f32; 3]>, indices: Vec<u16>, matrix: &Mat44f, category: u32, mask: u32, space: u32) -> GeomId {
        let m = super::core_car::mesh(vertices, indices, std::array::from_fn(|i| matrix.m[i / 4][i % 4]));
        let gid = CAR_GEOM | self.core.col.geoms.len();
        GeomId(self.core.add_mesh_collider(id(body), &m.matrix, category, mask, space, Some(m.mesh), m.shape, gid) as u32)
    }

    /// The contact joints in the world now (none between two steps: CSP drops them in the step).
    pub fn contact_joints(&self) -> Vec<JointId> {
        let n = self.core.world.joints.len();
        (0..n).filter(|&i| matches!(self.core.world.joints[i].joint.kind, JointKind::Contact { .. })).map(|i| JointId((n - 1 - i) as u32)).collect()
    }

    /// The collision (CSP's "Prepare spaces", then the collision and its contact joints) and the
    /// game's callback for each contact joint, in CSP's order.
    pub fn collision_step(&mut self) -> Vec<CollisionEvent> {
        let joints = self.core.prepare().and_then(|()| self.core.collide()).unwrap_or_else(|e| panic!("CSP collision: {}", e.0));
        joints
            .iter()
            .map(|j| {
                let c = &j.geom;
                let (body_a, shape_a) = self.geom_of(c.g1);
                let (body_b, shape_b) = self.geom_of(c.g2);
                let f = |v: [f64; 3]| Vec3f::new(v[0] as f32, v[1] as f32, v[2] as f32);
                CollisionEvent { body_a, shape_a, body_b, shape_b, normal: f(c.normal), pos: f(c.pos), depth: c.depth as f32 }
            })
            .collect()
    }

    /// A contact's geom (by its name): its body and, for a mesh, its shape (a track mesh's group
    /// its category bits, as `CollisionMeshODE::getGroup`).
    fn geom_of(&self, name: usize) -> (Option<RigidBody>, Option<Shape>) {
        let col = &self.core.col;
        if name & CAR_GEOM == 0 {
            let mesh = name - 1;
            let g = col.track[col.track.len() - 1 - mesh];
            return (None, Some(Shape { geom: GeomRef::StaticMesh(mesh as u32), group: col.geoms[g].category }));
        }
        let g = name & !CAR_GEOM;
        let shape = matches!(col.geoms[g].shape, rustyac_ode::csp_collision::Shape::Mesh { .. }).then(|| Shape { geom: GeomRef::Dyn(GeomId(g as u32)), group: col.geoms[g].category });
        (self.core.geom_body(g).map(rigid_body), shape)
    }
}
