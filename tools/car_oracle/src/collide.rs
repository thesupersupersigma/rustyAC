// SPDX-License-Identifier: GPL-3.0-or-later

//! The collision micro-oracle: the game's own ODE, OPCODE and `PhysicsCore` (run in this
//! process, from `acs.exe`) against `rustyac-ode` and the port's `PhysicsCore`, bit for bit.
//!
//! Both sides are driven through one [`Twin`]: every call makes the same thing in the game's
//! core and in the port's (a static mesh, a body, a box or a mesh on a body, a pose, a step).
//! Two kinds of check:
//!
//! * `car_oracle collide --track spa`: the F2004's floor boxes and its collider mesh are put
//!   in random poses over Spa. For every pose (a) `dCollide` is called for every near pair of
//!   a car geom and a track mesh on both sides, and the contacts are compared (count, order,
//!   position, normal, depth, sides); (b) the game's `PhysicsCore::collisionStep` runs and the
//!   contact joints it leaves are compared with the port's (which pairs, in which order, with
//!   which material; the box contacts the game drops).
//! * `car_oracle collide-worlds`: small worlds that are stepped (`PhysicsCore::step`: the
//!   collision pass and `dWorldStep` with contacts): a box resting, sliding and tumbling on
//!   a mesh, a mesh pushed into a mesh, a car floor on bumps. The bodies' states and the
//!   contact joints are compared after every step.

use std::collections::HashMap;
use std::sync::Arc;

use rustyac_physics::car::colliders::CarColliders;
use rustyac_physics::car::{PhysicsCore, RigidBody};
use rustyac_physics::ode::{GeomId, GeomRef, JointKind, StaticWorld};
use rustyac_physics::track::Track;
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::acs::Acs;
use crate::game::{rd, wr};

const VA_CORE_CREATE_RIGID_BODY: usize = 0x1_402c_c3b0; // IRigidBody* PhysicsCore::createRigidBody()
const VA_CORE_CREATE_COLLISION_MESH: usize = 0x1_402c_c0f0; // PhysicsCore::createCollisionMesh(float*, uint, ushort*, int, const mat44f&, IRigidBody*, ulong category, ulong mask, uint space)
const VA_CORE_COLLISION_STEP: usize = 0x1_402c_bf90; // PhysicsCore::collisionStep(float)
const VA_CORE_STEP: usize = 0x1_402c_d690; // PhysicsCore::step(float)
const VA_RB_ADD_BOX_COLLIDER: usize = 0x1_402c_ddb0; // RigidBodyODE::addBoxCollider(const vec3f& pos, const vec3f& size, uint category, ulong mask, uint space)
const VA_RB_ADD_MESH_COLLIDER: usize = 0x1_402c_e080; // RigidBodyODE::addMeshCollider(float*, uint, ushort*, uint, mat44f, ulong category, ulong mask, uint space)
const VA_RB_SET_MESH_COLLIDE_MASK: usize = 0x1_402c_e9c0; // RigidBodyODE::setMeshCollideMask(uint index, ulong mask)
const VA_RB_SET_POSITION: usize = 0x1_402c_e9e0; // RigidBodyODE::setPosition(const vec3f&)
const VA_RB_SET_ROTATION: usize = 0x1_402c_ea00; // RigidBodyODE::setRotation(const mat44f&)
const VA_RB_SET_MASS_BOX: usize = 0x1_402c_e890; // RigidBodyODE::setMassBox(float m, float x, float y, float z)
const VA_RB_SET_VELOCITY: usize = 0x1_402c_eab0; // RigidBodyODE::setVelocity(const vec3f&)
const VA_RB_SET_ANGULAR_VELOCITY: usize = 0x1_402c_e820; // RigidBodyODE::setAngularVelocity(const vec3f&)
const VA_D_COLLIDE: usize = 0x1_4034_4120; // int dCollide(dGeomID, dGeomID, int flags, dContactGeom*, int skip)
const VA_CONTACT_JOINT_VTABLE: usize = 0x1_4051_2d50; // dxJointContact
const PE_CORE: usize = 0x190;
const CORE_WORLD: usize = 0x08;
const CORE_NO_COLLISION_COUNTER: usize = 0x60;
const CORE_CURRENT_FRAME: usize = 0xa0;
const RB_ODE_BODY: usize = 0x08;
const RB_COLLISION_MESHES: usize = 0x30;
const CM_TRIMESH: usize = 0x10;
const WORLD_FIRST_JOINT: usize = 0x28;

type V3 = [f32; 3];

/// ODE's `dContactGeom`.
#[repr(C)]
#[derive(Clone, Copy)]
struct DContactGeom {
    pos: [f32; 4],
    normal: [f32; 4],
    depth: f32,
    _pad: u32,
    g1: usize,
    g2: usize,
    side1: i32,
    side2: i32,
}

/// A contact with the geoms named the port's way and the floats as bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContactBits {
    pub pos: [u32; 3],
    pub normal: [u32; 3],
    pub depth: u32,
    pub g1: Option<GeomRef>,
    pub g2: Option<GeomRef>,
    pub side1: i32,
    pub side2: i32,
}

/// A NaN is a NaN (its sign and payload depend on operand order no compiler promises).
fn canon(x: f32) -> u32 {
    if x.is_nan() {
        0x7fc0_0000
    } else {
        x.to_bits()
    }
}

fn canon3(v: &[f32]) -> [u32; 3] {
    [canon(v[0]), canon(v[1]), canon(v[2])]
}

/// A contact joint: the contact and what the game's material made of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointBits {
    pub contact: ContactBits,
    pub mode: i32,
    pub mu: u32,
    pub bounce: u32,
    /// Only compared for mesh contacts (see `PhysicsCore::mesh_bounce_vel`).
    pub bounce_vel: u32,
    /// Only meaningful with `dContactSoftERP` in the mode.
    pub soft_erp: u32,
    pub soft_cfm: u32,
    pub bodies: [bool; 2],
    pub reverse: bool,
}

/// A body's state as bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyBits {
    pub pos: [u32; 3],
    pub q: [u32; 4],
    pub lvel: [u32; 3],
    pub avel: [u32; 3],
}

struct TwinBody {
    game: *mut u8,
    ode: *mut u8,
    rust: RigidBody,
}

/// The same world in the game's `PhysicsCore` and in the port's.
pub struct Twin<'a> {
    acs: &'a Acs,
    /// The game's `PhysicsCore`.
    pub game_core: *mut u8,
    pub core: PhysicsCore,
    /// The port's static world when the twin built it itself (no track).
    pub statics: StaticWorld,
    track: Option<&'a Track>,
    bodies: Vec<TwinBody>,
    /// The game's geom pointer -> the port's name for it.
    pub geoms: HashMap<usize, GeomRef>,
    /// The other way round.
    pub game_geom: HashMap<GeomRef, usize>,
    /// `bounce_vel` values seen in the game's box contact joints / mesh contact joints.
    pub box_bounce_vels: Vec<u32>,
    pub mesh_bounce_vels: Vec<u32>,
}

fn mat16(m: &Mat44f) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[4 * r + c] = m.m[r][c];
        }
    }
    out
}

impl<'a> Twin<'a> {
    /// `engine`: the game's `PhysicsEngine`. `track`: the game's track was built on that
    /// engine with these meshes (`objects[i]` is the game's `CollisionMeshODE` of the port's
    /// mesh `i`).
    pub fn new(acs: &'a Acs, engine: *mut u8, track: Option<(&'a Track, &[*mut u8])>) -> Twin<'a> {
        let game_core: *mut u8 = unsafe { rd(engine, PE_CORE) };
        let mut twin = Twin {
            acs,
            game_core,
            core: PhysicsCore::new(),
            statics: StaticWorld::new(),
            track: None,
            bodies: Vec::new(),
            geoms: HashMap::new(),
            game_geom: HashMap::new(),
            box_bounce_vels: Vec::new(),
            mesh_bounce_vels: Vec::new(),
        };
        if let Some((track, objects)) = track {
            twin.track = Some(track);
            for (i, &object) in objects.iter().enumerate() {
                let geom: usize = unsafe { rd(object, CM_TRIMESH) };
                twin.name(geom, GeomRef::StaticMesh(i as u32));
            }
        }
        twin
    }

    fn name(&mut self, game: usize, port: GeomRef) {
        self.geoms.insert(game, port);
        self.game_geom.insert(port, game);
    }

    /// The static world the port collides with.
    pub fn static_world(&self) -> &StaticWorld {
        match self.track {
            Some(track) => &track.world,
            None => &self.statics,
        }
    }

    /// A static mesh on both sides (`PhysicsCore::createCollisionMesh`). Only without a track.
    pub fn static_mesh(&mut self, vertices: &[V3], indices: &[u16], category: u32, mask: u32, space: u32) -> GeomRef {
        assert!(self.track.is_none(), "the track owns the static world");
        let identity = mat16(&Mat44f::IDENTITY);
        let object = unsafe {
            let create: extern "C" fn(*mut u8, *const f32, u32, *const u16, i32, *const [f32; 16], *mut u8, u32, u32, u32) -> *mut u8 =
                std::mem::transmute(self.acs.va(VA_CORE_CREATE_COLLISION_MESH));
            create(
                self.game_core,
                vertices.as_ptr().cast(),
                vertices.len() as u32,
                indices.as_ptr(),
                indices.len() as i32,
                &identity,
                std::ptr::null_mut(),
                category,
                mask,
                space,
            )
        };
        let index = self.statics.create_tri_mesh(vertices.to_vec(), indices.to_vec(), category, mask, space);
        self.statics.clean();
        let name = GeomRef::StaticMesh(index as u32);
        let geom: usize = unsafe { rd(object, CM_TRIMESH) };
        self.name(geom, name);
        name
    }

    /// A body on both sides (`PhysicsCore::createRigidBody`); returns its index.
    pub fn body(&mut self) -> usize {
        let game = unsafe {
            let create: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_CORE_CREATE_RIGID_BODY));
            create(self.game_core)
        };
        let ode = unsafe { rd(game, RB_ODE_BODY) };
        let rust = self.core.create_rigid_body();
        self.bodies.push(TwinBody { game, ode, rust });
        self.bodies.len() - 1
    }

    pub fn rust_body(&self, body: usize) -> RigidBody {
        self.bodies[body].rust
    }

    /// `RigidBodyODE::setMassBox`.
    pub fn set_mass_box(&mut self, body: usize, mass: f32, x: f32, y: f32, z: f32) {
        unsafe {
            let f: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(self.acs.va(VA_RB_SET_MASS_BOX));
            f(self.bodies[body].game, mass, x, y, z);
        }
        let b = self.bodies[body].rust;
        self.core.set_mass_box(b, mass, x, y, z);
    }

    /// `RigidBodyODE::addBoxCollider`.
    pub fn add_box(&mut self, body: usize, centre: V3, size: V3, category: u32, mask: u32, space: u32) -> GeomId {
        let game = unsafe {
            let f: extern "C" fn(*mut u8, *const V3, *const V3, u32, u32, u32) -> *mut u8 = std::mem::transmute(self.acs.va(VA_RB_ADD_BOX_COLLIDER));
            f(self.bodies[body].game, &centre, &size, category, mask, space)
        };
        let b = self.bodies[body].rust;
        let geom = self.core.add_box_collider(b, &Vec3f::new(centre[0], centre[1], centre[2]), &Vec3f::new(size[0], size[1], size[2]), category, mask, space);
        self.name(game as usize, GeomRef::Dyn(geom));
        geom
    }

    /// `RigidBodyODE::addMeshCollider`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh(&mut self, body: usize, vertices: &[V3], indices: &[u16], matrix: &Mat44f, category: u32, mask: u32, space: u32) -> GeomId {
        let b = self.bodies[body].rust;
        let index = self.core.mesh_colliders(b).len();
        let m = mat16(matrix);
        let game = unsafe {
            let f: extern "C" fn(*mut u8, *const f32, u32, *const u16, u32, *const [f32; 16], u32, u32, u32) = std::mem::transmute(self.acs.va(VA_RB_ADD_MESH_COLLIDER));
            f(self.bodies[body].game, vertices.as_ptr().cast(), vertices.len() as u32, indices.as_ptr(), indices.len() as u32, &m, category, mask, space);
            // collisionMeshes[index] (a shared_ptr<BodyCollisionMesh>) -> geomID
            let begin: *const *const u8 = rd(self.bodies[body].game, RB_COLLISION_MESHES);
            let mesh = *begin.add(2 * index);
            rd::<usize>(mesh, 0x10)
        };
        let geom = self.core.add_mesh_collider(b, vertices.to_vec(), indices.to_vec(), matrix, category, mask, space);
        self.name(game, GeomRef::Dyn(geom));
        // what the game's contact joints of this mesh get as bounce_vel: the upper half of
        // the geom's address
        self.core.mesh_bounce_vel = f32::from_bits((game >> 32) as u32);
        geom
    }

    /// `RigidBodyODE::setMeshCollideMask`.
    pub fn set_mesh_mask(&mut self, body: usize, index: u32, mask: u32) {
        unsafe {
            let f: extern "C" fn(*mut u8, u32, u32) = std::mem::transmute(self.acs.va(VA_RB_SET_MESH_COLLIDE_MASK));
            f(self.bodies[body].game, index, mask);
        }
        let b = self.bodies[body].rust;
        self.core.set_mesh_collide_mask(b, index as usize, mask);
    }

    /// `RigidBodyODE::setRotation`, then `setPosition`. `rotation`: the body's axes as rows.
    pub fn set_pose(&mut self, body: usize, position: V3, rotation: &Mat44f) {
        let m = mat16(rotation);
        unsafe {
            let set_rotation: extern "C" fn(*mut u8, *const [f32; 16]) = std::mem::transmute(self.acs.va(VA_RB_SET_ROTATION));
            set_rotation(self.bodies[body].game, &m);
            let set_position: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(self.acs.va(VA_RB_SET_POSITION));
            set_position(self.bodies[body].game, &position);
        }
        let b = self.bodies[body].rust;
        self.core.set_rotation(b, rotation);
        self.core.set_position(b, &Vec3f::new(position[0], position[1], position[2]));
    }

    /// `RigidBodyODE::setVelocity`, `setAngularVelocity`.
    pub fn set_velocity(&mut self, body: usize, linear: V3, angular: V3) {
        unsafe {
            let set_velocity: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(self.acs.va(VA_RB_SET_VELOCITY));
            set_velocity(self.bodies[body].game, &linear);
            let set_angular: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(self.acs.va(VA_RB_SET_ANGULAR_VELOCITY));
            set_angular(self.bodies[body].game, &angular);
        }
        let b = self.bodies[body].rust;
        self.core.world.body_set_linear_vel(b.id, linear[0], linear[1], linear[2]);
        self.core.world.body_set_angular_vel(b.id, angular[0], angular[1], angular[2]);
    }

    fn contact_bits(&self, c: &DContactGeom) -> ContactBits {
        ContactBits {
            pos: canon3(&c.pos),
            normal: canon3(&c.normal),
            depth: canon(c.depth),
            g1: self.geoms.get(&c.g1).copied(),
            g2: self.geoms.get(&c.g2).copied(),
            side1: c.side1,
            side2: c.side2,
        }
    }

    /// `dCollide` on both sides for one pair, with room for `flags` contacts.
    pub fn collide(&mut self, o1: GeomRef, o2: GeomRef, flags: u32) -> (Vec<ContactBits>, Vec<ContactBits>) {
        let mut buffer = [DContactGeom { pos: [0.0; 4], normal: [0.0; 4], depth: 0.0, _pad: 0, g1: 0, g2: 0, side1: 0, side2: 0 }; 64];
        let n = unsafe {
            let f: extern "C" fn(usize, usize, i32, *mut DContactGeom, i32) -> i32 = std::mem::transmute(self.acs.va(VA_D_COLLIDE));
            f(self.game_geom[&o1], self.game_geom[&o2], flags as i32, buffer.as_mut_ptr(), 0x40)
        };
        let game: Vec<ContactBits> = buffer[..n.max(0) as usize].iter().map(|c| self.contact_bits(c)).collect();
        let mut out = Vec::new();
        let statics = match self.track {
            Some(track) => &track.world,
            None => &self.statics,
        };
        self.core.world.collide(o1, o2, Some(statics), flags, &mut out);
        let port = out
            .iter()
            .map(|c| ContactBits { pos: canon3(&c.pos), normal: canon3(&c.normal), depth: canon(c.depth), g1: Some(c.g1), g2: Some(c.g2), side1: c.side1, side2: c.side2 })
            .collect();
        (game, port)
    }

    /// The game's contact joints, newest first (the order of the world's joint list).
    pub fn game_joints(&mut self) -> Vec<JointBits> {
        let mut out = Vec::new();
        unsafe {
            let world: *const u8 = rd(self.game_core, CORE_WORLD);
            let vtable = self.acs.va(VA_CONTACT_JOINT_VTABLE);
            let mut joint: *const u8 = rd(world, WORLD_FIRST_JOINT);
            while !joint.is_null() {
                if rd::<usize>(joint, 0) == vtable {
                    let c: DContactGeom = rd(joint, 0x90 + 0x40);
                    let mode: i32 = rd(joint, 0x90);
                    let bounce_vel: u32 = rd(joint, 0xac);
                    let is_box = mode & 8 != 0;
                    let seen = if is_box { &mut self.box_bounce_vels } else { &mut self.mesh_bounce_vels };
                    if !seen.contains(&bounce_vel) && seen.len() < 16 {
                        seen.push(bounce_vel);
                    }
                    out.push(JointBits {
                        contact: self.contact_bits(&c),
                        mode,
                        mu: rd(joint, 0x94),
                        bounce: rd(joint, 0xa8),
                        bounce_vel: if is_box { 0 } else { bounce_vel },
                        soft_erp: if is_box { rd(joint, 0xb0) } else { 0 },
                        soft_cfm: rd(joint, 0xb4),
                        bodies: [!rd::<*const u8>(joint, 0x40).is_null(), !rd::<*const u8>(joint, 0x58).is_null()],
                        reverse: rd::<u32>(joint, 0x30) & 2 != 0,
                    });
                }
                joint = rd(joint, 0x10);
            }
        }
        out
    }

    /// The port's contact joints, newest first.
    pub fn port_joints(&self) -> Vec<JointBits> {
        self.core
            .contact_joints()
            .into_iter()
            .map(|id| {
                let joint = self.core.world.joint(id);
                let JointKind::Contact { contact, .. } = &joint.kind else { unreachable!() };
                let (s, c) = (&contact.surface, &contact.geom);
                let is_box = s.mode & 8 != 0;
                JointBits {
                    contact: ContactBits { pos: canon3(&c.pos), normal: canon3(&c.normal), depth: canon(c.depth), g1: Some(c.g1), g2: Some(c.g2), side1: c.side1, side2: c.side2 },
                    mode: s.mode,
                    mu: s.mu.to_bits(),
                    bounce: s.bounce.to_bits(),
                    bounce_vel: if is_box { 0 } else { s.bounce_vel.to_bits() },
                    soft_erp: if is_box { s.soft_erp.to_bits() } else { 0 },
                    soft_cfm: s.soft_cfm.to_bits(),
                    bodies: [joint.node[0].body.is_some(), joint.node[1].body.is_some()],
                    reverse: joint.flags & 2 != 0,
                }
            })
            .collect()
    }

    /// One collision pass of the given parity on both sides (`PhysicsCore::collisionStep`
    /// with `currentFrame` set), without a world step. `odd`: dynamic against static.
    pub fn collision_pass(&mut self, odd: bool) {
        unsafe {
            wr(self.game_core, CORE_CURRENT_FRAME, odd as u32);
            wr(self.game_core, CORE_NO_COLLISION_COUNTER, 0i32);
            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_CORE_COLLISION_STEP));
            f(self.game_core, 0.003);
        }
        self.core.current_frame = odd as u32;
        self.core.no_collision_counter = 0;
        let statics = match self.track {
            Some(track) => &track.world,
            None => &self.statics,
        };
        self.core.collision_step(Some(statics));
    }

    /// `PhysicsCore::step` on both sides.
    pub fn step(&mut self, dt: f32) -> rustyac_physics::ode::StepStats {
        unsafe {
            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_CORE_STEP));
            f(self.game_core, dt);
        }
        let statics = match self.track {
            Some(track) => &track.world,
            None => &self.statics,
        };
        self.core.collision_step(Some(statics));
        self.core.world_step(dt)
    }

    /// `PhysicsCore::step` in the game; the port then takes the game's contact joints as they
    /// are and only does its own `dWorldStep` with them (the solver without the narrow phase).
    pub fn step_hybrid(&mut self, dt: f32) -> rustyac_physics::ode::StepStats {
        use rustyac_physics::ode::{Contact, ContactGeom, SurfaceParameters};
        let mut contacts = Vec::new();
        unsafe {
            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_CORE_STEP));
            f(self.game_core, dt);
            let world: *const u8 = rd(self.game_core, CORE_WORLD);
            let vtable = self.acs.va(VA_CONTACT_JOINT_VTABLE);
            let mut joint: *const u8 = rd(world, WORLD_FIRST_JOINT);
            while !joint.is_null() {
                if rd::<usize>(joint, 0) == vtable {
                    let s: [f32; 15] = rd(joint, 0x90);
                    let c: DContactGeom = rd(joint, 0x90 + 0x40);
                    let contact = Contact {
                        surface: SurfaceParameters {
                            mode: rd(joint, 0x90),
                            mu: s[1],
                            mu2: s[2],
                            rho: s[3],
                            rho2: s[4],
                            rho_n: s[5],
                            bounce: s[6],
                            bounce_vel: s[7],
                            soft_erp: s[8],
                            soft_cfm: s[9],
                            motion1: s[10],
                            motion2: s[11],
                            motion_n: s[12],
                            slip1: s[13],
                            slip2: s[14],
                        },
                        geom: ContactGeom {
                            pos: [c.pos[0], c.pos[1], c.pos[2]],
                            normal: [c.normal[0], c.normal[1], c.normal[2]],
                            depth: c.depth,
                            g1: self.geoms.get(&c.g1).copied().unwrap_or(GeomRef::StaticSpace),
                            g2: self.geoms.get(&c.g2).copied().unwrap_or(GeomRef::StaticSpace),
                            side1: c.side1,
                            side2: c.side2,
                        },
                        fdir1: rd(joint, 0x110),
                    };
                    let body = |p: *const u8| self.bodies.iter().find(|b| b.ode as *const u8 == p).map(|b| b.rust.id);
                    let (b0, b1) = (body(rd(joint, 0x40)), body(rd(joint, 0x58)));
                    // as it was attached: a reversed joint had no first body
                    let reverse = rd::<u32>(joint, 0x30) & 2 != 0;
                    contacts.push(if reverse { (contact, None, b0) } else { (contact, b0, b1) });
                }
                joint = rd(joint, 0x10);
            }
        }
        // the world's list has the newest joint first
        contacts.reverse();
        self.core.set_contacts(&contacts);
        self.core.world_step(dt)
    }

    pub fn game_body(&self, body: usize) -> BodyBits {
        let b = self.bodies[body].ode;
        unsafe {
            BodyBits {
                pos: canon3(&rd::<[f32; 3]>(b, 0xc0)),
                q: rd::<[f32; 4]>(b, 0x100).map(canon),
                lvel: canon3(&rd::<[f32; 3]>(b, 0x110)),
                avel: canon3(&rd::<[f32; 3]>(b, 0x120)),
            }
        }
    }

    pub fn port_body(&self, body: usize) -> BodyBits {
        let b = self.core.world.body(self.bodies[body].rust.id);
        BodyBits { pos: canon3(&b.pos), q: b.q.map(canon), lvel: canon3(&b.lvel), avel: canon3(&b.avel) }
    }

    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }
}

/// xorshift64*
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The body's axes (rows) from yaw about y, then pitch about the body's x, then roll about
/// the body's z, in radians. Plain `f32` maths: both sides get the same matrix.
fn rotation(yaw: f32, pitch: f32, roll: f32) -> Mat44f {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sr, cr) = roll.sin_cos();
    // R = Ry(yaw) * Rx(pitch) * Rz(roll), columns = body axes in the world
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rx = [[1.0, 0.0, 0.0], [0.0, cp, -sp], [0.0, sp, cp]];
    let rz = [[cr, -sr, 0.0], [sr, cr, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]| {
        let mut out = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            }
        }
        out
    };
    let r = mul(&mul(&ry, &rx), &rz);
    let mut m = Mat44f::IDENTITY;
    for axis in 0..3 {
        for k in 0..3 {
            // row `axis` of the mat44f is the body's axis `axis` in the world = column of R
            m.m[axis][k] = r[k][axis];
        }
    }
    m
}

/// A random rotation (any orientation).
fn any_rotation(rng: &mut Rng) -> Mat44f {
    rotation(rng.range(0.0, std::f32::consts::TAU), rng.range(-3.1, 3.1), rng.range(-3.1, 3.1))
}

const KINDS: [&str; 10] = [
    "floor boxes: at ride height over the road, nearly level",
    "floor boxes: nose or tail down by up to 15 degrees",
    "floor boxes: rolled by up to 25 degrees",
    "floor boxes: sunk into the road by up to 30 cm",
    "floor boxes: over a kerb",
    "floor boxes: any orientation",
    "collider mesh: next to a wall, upright",
    "collider mesh: at a wall, any orientation",
    "collider mesh: upside down on the road",
    "collider mesh: on its side on the road",
];

/// Per kind: poses, pairs, pairs with contacts in the game, contacts in the game, identical
/// pairs, poses whose contact joints are identical, contact joints in the game.
#[derive(Clone, Copy, Default)]
struct Tally {
    poses: u64,
    pairs: u64,
    touching: u64,
    contacts: u64,
    same_pairs: u64,
    same_passes: u64,
    joints: u64,
    dropped: u64,
}

/// A random point of a random triangle of one of `meshes` (indices into the track's world),
/// with the triangle's (unnormalised) normal.
fn random_point(rng: &mut Rng, track: &Track, meshes: &[usize]) -> (V3, V3) {
    let mesh = &track.world.meshes[meshes[rng.below(meshes.len())]].data.mesh;
    let triangle = rng.below(mesh.nb_tris.max(1) as usize) as u32;
    let [a, b, c] = mesh.triangle(triangle);
    let (mut u, mut v) = (rng.unit(), rng.unit());
    if u + v > 1.0 {
        u = 1.0 - u;
        v = 1.0 - v;
    }
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let point = [a[0] + e1[0] * u + e2[0] * v, a[1] + e1[1] * u + e2[1] * v, a[2] + e1[2] * u + e2[2] * v];
    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
    (point, [n[0] / l, n[1] / l, n[2] / l])
}

fn describe(c: &[ContactBits]) -> String {
    let mut out = String::new();
    for (i, c) in c.iter().enumerate() {
        let f = |v: [u32; 3]| v.map(f32::from_bits);
        out.push_str(&format!(
            "    {i}: pos {:?} normal {:?} depth {:?} sides {} {} geoms {:?} {:?}\n       bits pos {:08x?} normal {:08x?} depth {:08x}\n",
            f(c.pos),
            f(c.normal),
            f32::from_bits(c.depth),
            c.side1,
            c.side2,
            c.g1,
            c.g2,
            c.pos,
            c.normal,
            c.depth
        ));
    }
    out
}

/// `car_oracle collide`: random poses of the car's floor boxes and collider mesh over a track.
/// Returns the report (markdown) and whether everything was identical.
#[allow(clippy::too_many_arguments)]
pub fn collide(
    acs: &Acs,
    engine: *mut u8,
    game_track: &crate::track::GameTrack,
    colliders: &CarColliders,
    box_poses: usize,
    mesh_poses: usize,
    seed: u64,
    only_kind: Option<usize>,
    boxes_only: bool,
) -> Result<(String, bool), String> {
    let track = &game_track.rust;
    let mut twin = Twin::new(acs, engine, Some((track, &game_track.objects)));
    let body = twin.body();
    let mut boxes = Vec::new();
    for b in &colliders.boxes {
        boxes.push(twin.add_box(body, [b.centre.x, b.centre.y, b.centre.z], [b.size.x, b.size.y, b.size.z], 4, 1, 1));
    }
    let mesh_def = colliders.mesh.as_ref().ok_or("the car has no collider mesh (collider.kn5 not found)")?;
    let mesh = twin.add_mesh(body, &mesh_def.vertices, &mesh_def.indices, &mesh_def.matrix, 4, 0x1e, 1);

    // the track's meshes by category, and the kerbs
    let surfaces: Vec<usize> = (0..track.world.meshes.len()).filter(|&i| track.world.meshes[i].category_bits & 1 != 0).collect();
    let walls: Vec<usize> = (0..track.world.meshes.len()).filter(|&i| track.world.meshes[i].category_bits & 2 != 0).collect();
    let kerbs: Vec<usize> = surfaces.iter().copied().filter(|&i| track.surfaces[i].key.contains("KERB")).collect();
    if surfaces.is_empty() || walls.is_empty() {
        return Err("the track has no surfaces or no walls".into());
    }
    let kerbs = if kerbs.is_empty() { surfaces.clone() } else { kerbs };

    let mut rng = Rng(seed | 1);
    let mut tally = [Tally::default(); KINDS.len()];
    let mut first: Option<String> = None;
    let mut meshes_touched = vec![false; track.world.meshes.len()];
    let (mut seconds_game, mut seconds_port) = (0.0f64, 0.0f64);
    let total = box_poses + mesh_poses;
    let degrees = std::f32::consts::PI / 180.0;
    for i in 0..total {
        let kind = match only_kind {
            Some(kind) => kind,
            None if i < box_poses => [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5][i % 12],
            None => [6, 6, 7, 7, 8, 9][(i - box_poses) % 6],
        };
        let yaw = rng.range(0.0, std::f32::consts::TAU);
        let (position, rot, mask) = match kind {
            0..=4 => {
                let (point, _) = random_point(&mut rng, track, if kind == 4 { &kerbs } else { &surfaces });
                // the floor's lowest face is 0.185 m under the body's origin
                let (height, pitch, roll) = match kind {
                    0 => (rng.range(0.10, 0.32), rng.range(-2.0, 2.0), rng.range(-2.0, 2.0)),
                    1 => (rng.range(0.05, 0.60), rng.range(-15.0, 15.0), rng.range(-2.0, 2.0)),
                    2 => (rng.range(0.05, 0.60), rng.range(-2.0, 2.0), rng.range(-25.0, 25.0)),
                    3 => (rng.range(-0.12, 0.19), rng.range(-4.0, 4.0), rng.range(-4.0, 4.0)),
                    _ => (rng.range(0.05, 0.30), rng.range(-5.0, 5.0), rng.range(-8.0, 8.0)),
                };
                ([point[0] + rng.range(-1.0, 1.0), point[1] + height, point[2] + rng.range(-1.0, 1.0)], rotation(yaw, pitch * degrees, roll * degrees), 0x1e)
            }
            5 => {
                let (point, _) = random_point(&mut rng, track, &surfaces);
                ([point[0], point[1] + rng.range(-0.2, 1.2), point[2]], any_rotation(&mut rng), 0x1e)
            }
            6 => {
                let (point, n) = random_point(&mut rng, track, &walls);
                let d = rng.range(-0.6, 2.2);
                ([point[0] + n[0] * d, point[1] + n[1] * d + rng.range(-0.3, 0.8), point[2] + n[2] * d], rotation(yaw, rng.range(-4.0, 4.0) * degrees, rng.range(-4.0, 4.0) * degrees), 0x1e)
            }
            7 => {
                let (point, n) = random_point(&mut rng, track, &walls);
                let d = rng.range(-0.8, 2.5);
                ([point[0] + n[0] * d, point[1] + n[1] * d, point[2] + n[2] * d], any_rotation(&mut rng), 0x1e)
            }
            8 => {
                let (point, _) = random_point(&mut rng, track, &surfaces);
                ([point[0], point[1] + rng.range(0.0, 1.1), point[2]], rotation(yaw, rng.range(-25.0, 25.0) * degrees, (180.0 + rng.range(-35.0, 35.0)) * degrees), 0x1f)
            }
            _ => {
                let (point, _) = random_point(&mut rng, track, &surfaces);
                let side = if rng.unit() < 0.5 { 90.0 } else { -90.0 };
                ([point[0], point[1] + rng.range(0.2, 1.2), point[2]], rotation(yaw, rng.range(-20.0, 20.0) * degrees, (side + rng.range(-25.0, 25.0)) * degrees), 0x1f)
            }
        };
        // (`boxes_only`: the mesh collides with nothing, for a look at the boxes alone)
        twin.set_mesh_mask(body, 0, if boxes_only { 0 } else { mask });
        twin.set_pose(body, position, &rot);
        let entry = &mut tally[kind];
        entry.poses += 1;

        // (a) the narrow phase, pair by pair
        let reach = 4.0f32;
        let car_geoms: Vec<GeomId> = if kind <= 5 { boxes.clone() } else { vec![mesh] };
        for (index, static_mesh) in track.world.meshes.iter().enumerate() {
            let wanted = if kind <= 5 { 1 } else { mask & 3 };
            if static_mesh.category_bits & wanted == 0 {
                continue;
            }
            let a = &static_mesh.aabb;
            if position[0] + reach < a[0] || position[0] - reach > a[1] || position[1] + reach < a[2] || position[1] - reach > a[3] || position[2] + reach < a[4] || position[2] - reach > a[5] {
                continue;
            }
            for &geom in &car_geoms {
                let (o1, o2) = (GeomRef::Dyn(geom), GeomRef::StaticMesh(index as u32));
                let t0 = std::time::Instant::now();
                let (theirs, ours) = twin.collide(o1, o2, 0x20);
                let _ = t0;
                entry.pairs += 1;
                if !theirs.is_empty() {
                    entry.touching += 1;
                    entry.contacts += theirs.len() as u64;
                    meshes_touched[index] = true;
                }
                if theirs == ours {
                    entry.same_pairs += 1;
                } else if first.is_none() {
                    first = Some(format!(
                        "pose {i} ({}), dCollide({o1:?}, {o2:?}): position {position:?} bits {:08x?}\n  rotation rows {:?}\n  game: {} contacts\n{}  port: {} contacts\n{}",
                        KINDS[kind],
                        position.map(f32::to_bits),
                        &rot.m[..3],
                        theirs.len(),
                        describe(&theirs),
                        ours.len(),
                        describe(&ours)
                    ));
                }
            }
        }

        // (b) the game's collision pass against the port's: the contact joints
        let t0 = std::time::Instant::now();
        twin.core.contact_log = Some(Vec::new());
        twin.collision_pass(true);
        let t1 = std::time::Instant::now();
        let theirs = twin.game_joints();
        let ours = twin.port_joints();
        seconds_game += (t1 - t0).as_secs_f64();
        seconds_port += t1.elapsed().as_secs_f64();
        entry.joints += theirs.len() as u64;
        entry.dropped += twin.core.contact_log.as_ref().map_or(0, |log| log.iter().filter(|c| !c.kept).count()) as u64;
        if theirs == ours {
            entry.same_passes += 1;
        } else if first.is_none() {
            let list = |joints: &[JointBits]| {
                let contacts: Vec<ContactBits> = joints.iter().map(|j| j.contact).collect();
                let materials: Vec<String> = joints.iter().map(|j| format!("{:x}/{:08x}/{:08x}/{:08x}/{:08x}/{:08x}/{:?}/{}", j.mode, j.mu, j.bounce, j.bounce_vel, j.soft_erp, j.soft_cfm, j.bodies, j.reverse)).collect();
                format!("{}    materials {materials:?}\n", describe(&contacts))
            };
            first = Some(format!(
                "pose {i} ({}), collision pass: position {position:?} bits {:08x?}\n  rotation rows {:?}\n  game: {} joints (newest first)\n{}  port: {} joints\n{}",
                KINDS[kind],
                position.map(f32::to_bits),
                &rot.m[..3],
                theirs.len(),
                list(&theirs),
                ours.len(),
                list(&ours)
            ));
        }
    }

    let mut out = String::new();
    out.push_str(&format!(
        "Track `{}`: {} physics meshes ({} surfaces, {} walls). Car: {} floor boxes, collider mesh of {} vertices and {} triangles.\n\n",
        track.name,
        track.world.meshes.len(),
        surfaces.len(),
        walls.len(),
        colliders.boxes.len(),
        mesh_def.vertices.len(),
        mesh_def.indices.len() / 3
    ));
    out.push_str("| Kind of pose | Poses | `dCollide` pairs | Pairs that touch (game) | Contacts (game) | Identical pairs | Contact joints after the game's pass | Box contacts the game dropped | Identical passes |\n|---|---|---|---|---|---|---|---|---|\n");
    let mut sum = Tally::default();
    for (kind, t) in KINDS.iter().zip(tally) {
        if t.poses == 0 {
            continue;
        }
        out.push_str(&format!(
            "| {kind} | {} | {} | {} | {} | {} ({:.4} %) | {} | {} | {} ({:.4} %) |\n",
            t.poses,
            t.pairs,
            t.touching,
            t.contacts,
            t.same_pairs,
            t.same_pairs as f64 * 100.0 / t.pairs.max(1) as f64,
            t.joints,
            t.dropped,
            t.same_passes,
            t.same_passes as f64 * 100.0 / t.poses.max(1) as f64
        ));
        sum.poses += t.poses;
        sum.pairs += t.pairs;
        sum.touching += t.touching;
        sum.contacts += t.contacts;
        sum.same_pairs += t.same_pairs;
        sum.joints += t.joints;
        sum.dropped += t.dropped;
        sum.same_passes += t.same_passes;
    }
    out.push_str(&format!(
        "| **all** | **{}** | **{}** | **{}** | **{}** | **{} ({:.4} %)** | **{}** | **{}** | **{} ({:.4} %)** |\n\n",
        sum.poses,
        sum.pairs,
        sum.touching,
        sum.contacts,
        sum.same_pairs,
        sum.same_pairs as f64 * 100.0 / sum.pairs.max(1) as f64,
        sum.joints,
        sum.dropped,
        sum.same_passes,
        sum.same_passes as f64 * 100.0 / sum.poses.max(1) as f64
    ));
    out.push_str(&format!(
        "A pair is identical when both sides give the same number of contacts in the same order with the same bits of position, normal and depth, the same geoms and the same triangle numbers. A pass is identical when the contact joints left by `PhysicsCore::collisionStep` are the same in the same order, with the same contact and material (seed {seed}). {} of the {} meshes were touched at least once. Time for the passes: game {seconds_game:.2} s, port {seconds_port:.2} s.\n\n",
        meshes_touched.iter().filter(|t| **t).count(),
        meshes_touched.len()
    ));
    out.push_str(&format!(
        "`bounce_vel` as the game's contact joints had it in this run (never written by the game): box contacts {:08x?} (the upper half of register r12 of this test program; the game has 0 there), mesh contacts {:08x?} (the upper half of the address of the car's mesh geom).\n",
        twin.box_bounce_vels, twin.mesh_bounce_vels
    ));
    match &first {
        Some(first) => out.push_str(&format!("\nFirst difference:\n\n```\n{first}\n```\n")),
        None => out.push_str("\nNo difference.\n"),
    }
    let ok = sum.same_pairs == sum.pairs && sum.same_passes == sum.poses;
    Ok((out, ok))
}

// --- small worlds that are stepped -------------------------------------------------------

/// A bumpy square floor: `n` x `n` cells of `cell` metres around (`x0`, `z0`), heights from
/// the generator within `bump`. Counter-clockwise seen from above.
fn terrain(rng: &mut Rng, x0: f32, z0: f32, n: usize, cell: f32, bump: f32) -> (Vec<V3>, Vec<u16>) {
    let mut vertices = Vec::new();
    let half = n as f32 * cell * 0.5;
    for iz in 0..=n {
        for ix in 0..=n {
            let y = if bump == 0.0 { 0.0 } else { rng.range(-bump, bump) };
            vertices.push([x0 - half + ix as f32 * cell, y, z0 - half + iz as f32 * cell]);
        }
    }
    let mut indices = Vec::new();
    let at = |ix: usize, iz: usize| (iz * (n + 1) + ix) as u16;
    for iz in 0..n {
        for ix in 0..n {
            indices.extend([at(ix, iz), at(ix, iz + 1), at(ix + 1, iz + 1)]);
            indices.extend([at(ix, iz), at(ix + 1, iz + 1), at(ix + 1, iz)]);
        }
    }
    (vertices, indices)
}

/// A closed box mesh (12 triangles, outward normals) with half sizes `h`.
fn box_mesh(h: V3) -> (Vec<V3>, Vec<u16>) {
    let mut vertices = Vec::new();
    for i in 0..8 {
        vertices.push([if i & 1 == 0 { -h[0] } else { h[0] }, if i & 2 == 0 { -h[1] } else { h[1] }, if i & 4 == 0 { -h[2] } else { h[2] }]);
    }
    let indices = vec![0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
    (vertices, indices)
}

/// A wall: a quad in the plane x = `x`, facing -x, from the floor up.
fn wall(x: f32, z0: f32, half: f32, height: f32) -> (Vec<V3>, Vec<u16>) {
    (vec![[x, -1.0, z0 - half], [x, -1.0, z0 + half], [x, height, z0 + half], [x, height, z0 - half]], vec![0, 1, 2, 0, 2, 3])
}

struct WorldDef {
    name: &'static str,
    about: &'static str,
    bodies: Vec<usize>,
}

/// What one world did.
#[derive(Clone, Default)]
struct WorldResult {
    steps: usize,
    same_steps: usize,
    first: Option<String>,
    joints: u64,
    max_joints: usize,
    steps_touching: usize,
    bounded_rows: u64,
    pivots: u64,
    lcp_errors: u32,
}

/// `car_oracle collide-worlds`: small worlds stepped on both sides.
pub fn worlds(acs: &Acs, engine: *mut u8, colliders: &CarColliders, steps: usize, seed: u64, only: &[String], hybrid: bool) -> Result<(String, bool), String> {
    let mut twin = Twin::new(acs, engine, None);
    let mut rng = Rng(seed | 1);
    let mut defs: Vec<WorldDef> = Vec::new();
    let level = Mat44f::IDENTITY;
    let wanted = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    let mut slot = 0u32;
    // every world has its own place (2 km apart), its own static sub-space and its own
    // dynamic sub-space
    let place = |slot: &mut u32| {
        *slot += 1;
        (*slot as f32 * 2000.0, *slot)
    };

    if wanted("box_rest") {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 2, 10.0, 0.0);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let b = twin.body();
        twin.set_mass_box(b, 120.0, 1.0, 0.2, 2.0);
        twin.add_box(b, [0.0, 0.0, 0.0], [1.0, 0.2, 2.0], 4, 1, id);
        twin.set_pose(b, [x + 0.3, 0.102, 0.4], &level);
        defs.push(WorldDef { name: "box_rest", about: "a box put down on a flat mesh, left to rest", bodies: vec![b] });
    }
    if wanted("box_slide") {
        let (x, id) = place(&mut slot);
        let (mut v, i) = terrain(&mut rng, x, 0.0, 4, 10.0, 0.0);
        // a slope of 3 % along z
        for p in v.iter_mut() {
            p[1] = p[2] * 0.03;
        }
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let b = twin.body();
        twin.set_mass_box(b, 80.0, 1.2, 0.3, 1.8);
        twin.add_box(b, [0.0, 0.0, 0.0], [1.2, 0.3, 1.8], 4, 1, id);
        twin.set_pose(b, [x - 6.0, 0.16, -3.0], &rotation(0.3, 0.03, 0.0));
        twin.set_velocity(b, [7.0, 0.0, 3.0], [0.0, 0.4, 0.0]);
        defs.push(WorldDef { name: "box_slide", about: "a box sliding and turning over a sloped mesh until friction stops it", bodies: vec![b] });
    }
    if wanted("box_bounce") {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 16, 1.5, 0.12);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let b = twin.body();
        twin.set_mass_box(b, 60.0, 1.0, 0.4, 1.4);
        twin.add_box(b, [0.0, 0.0, 0.0], [1.0, 0.4, 1.4], 4, 1, id);
        twin.set_pose(b, [x + 0.2, 1.4, -0.3], &rotation(0.5, 0.1, -0.08));
        twin.set_velocity(b, [1.5, 0.0, 2.5], [0.25, 0.2, -0.2]);
        defs.push(WorldDef { name: "box_bounce", about: "a box dropped with spin from 1.4 m onto bumps", bodies: vec![b] });
    }
    if wanted("car_floor") {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 40, 1.0, 0.035);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let b = twin.body();
        twin.set_mass_box(b, 550.0, 1.8, 0.55, 3.6);
        for c in &colliders.boxes {
            twin.add_box(b, [c.centre.x, c.centre.y, c.centre.z], [c.size.x, c.size.y, c.size.z], 4, 1, id);
        }
        twin.set_pose(b, [x - 8.0, 0.30, -6.0], &rotation(0.6, 0.0, 0.0));
        twin.set_velocity(b, [9.0, 0.0, 12.0], [0.0, 0.3, 0.0]);
        defs.push(WorldDef { name: "car_floor", about: "the F2004's six floor boxes on one body, thrown along a bumpy mesh at 54 km/h", bodies: vec![b] });
    }
    if wanted("mesh_push") {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 4, 10.0, 0.0);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let (v, i) = wall(x + 6.0, 0.0, 12.0, 3.0);
        twin.static_mesh(&v, &i, 2, 0x14, id + 10_000);
        let b = twin.body();
        twin.set_mass_box(b, 300.0, 1.0, 1.0, 2.0);
        let (v, i) = box_mesh([0.5, 0.5, 1.0]);
        twin.add_mesh(b, &v, &i, &level, 4, 0x1f, id);
        twin.set_pose(b, [x, 0.52, 0.0], &rotation(0.4, 0.0, 0.0));
        twin.set_velocity(b, [14.0, 0.0, 1.0], [0.0, 0.0, 0.0]);
        defs.push(WorldDef { name: "mesh_push", about: "a box-shaped mesh sliding on a floor mesh into a wall mesh at 50 km/h", bodies: vec![b] });
    }
    if let (true, Some(mesh)) = (wanted("mesh_tumble"), &colliders.mesh) {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 24, 1.5, 0.08);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let b = twin.body();
        twin.set_mass_box(b, 550.0, 1.8, 0.55, 3.6);
        twin.add_mesh(b, &mesh.vertices, &mesh.indices, &mesh.matrix, 4, 0x1f, id);
        twin.set_pose(b, [x, 1.3, 0.0], &rotation(1.0, 0.3, 2.8));
        twin.set_velocity(b, [6.0, 0.0, 4.0], [1.5, 0.3, 2.0]);
        defs.push(WorldDef { name: "mesh_tumble", about: "the F2004's collider mesh dropped upside down and tumbling over bumps", bodies: vec![b] });
    }
    if let (true, Some(mesh)) = (wanted("car_wall"), &colliders.mesh) {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 8, 5.0, 0.0);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let (v, i) = wall(x + 9.0, 0.0, 20.0, 2.0);
        twin.static_mesh(&v, &i, 2, 0x14, id + 10_000);
        let b = twin.body();
        twin.set_mass_box(b, 550.0, 1.8, 0.55, 3.6);
        for c in &colliders.boxes {
            twin.add_box(b, [c.centre.x, c.centre.y, c.centre.z], [c.size.x, c.size.y, c.size.z], 4, 1, id);
        }
        twin.add_mesh(b, &mesh.vertices, &mesh.indices, &mesh.matrix, 4, 0x1e, id);
        twin.set_pose(b, [x, 0.20, 0.0], &rotation(1.1, 0.0, 0.0));
        twin.set_velocity(b, [18.0, 0.0, 9.0], [0.0, 0.0, 0.0]);
        defs.push(WorldDef { name: "car_wall", about: "floor boxes and collider mesh on one body: sliding on its floor into a wall at 72 km/h, at an angle", bodies: vec![b] });
    }
    if wanted("two_meshes") {
        let (x, id) = place(&mut slot);
        let (v, i) = terrain(&mut rng, x, 0.0, 4, 10.0, 0.0);
        twin.static_mesh(&v, &i, 1, 0x14, id);
        let (v, i) = box_mesh([0.6, 0.4, 1.2]);
        let a = twin.body();
        twin.set_mass_box(a, 400.0, 1.2, 0.8, 2.4);
        twin.add_mesh(a, &v, &i, &level, 4, 0x1f, id);
        twin.set_pose(a, [x - 3.0, 0.41, 0.0], &rotation(1.5, 0.0, 0.0));
        twin.set_velocity(a, [10.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let b = twin.body();
        twin.set_mass_box(b, 400.0, 1.2, 0.8, 2.4);
        slot += 1;
        twin.add_mesh(b, &v, &i, &level, 4, 0x1f, slot);
        twin.set_pose(b, [x + 2.0, 0.41, 0.3], &rotation(0.2, 0.0, 0.0));
        defs.push(WorldDef { name: "two_meshes", about: "(extra, beyond the task) two bodies with meshes: one slides into the other (the even pass, at most 4 contacts, two-body contact joints)", bodies: vec![a, b] });
    }
    if defs.is_empty() {
        return Err("no world of that name".into());
    }
    twin.core.statics = Some(Arc::new(twin.statics.clone()));

    let mut results = vec![WorldResult::default(); defs.len()];
    let mut dead = vec![false; defs.len()];
    for step in 0..steps {
        let stats = if hybrid { twin.step_hybrid(0.003) } else { twin.step(0.003) };
        let theirs = twin.game_joints();
        // (with the game's contacts fed in there is nothing of the port's to compare them with)
        let ours = if hybrid { theirs.clone() } else { twin.port_joints() };
        for (w, def) in defs.iter().enumerate() {
            if dead[w] {
                continue;
            }
            let near = |j: &&JointBits| {
                let x = f32::from_bits(j.contact.pos[0]);
                let home = f32::from_bits(twin.game_body(def.bodies[0]).pos[0]);
                (x - home).abs() < 900.0
            };
            let their_joints: Vec<JointBits> = theirs.iter().filter(near).copied().collect();
            let our_joints: Vec<JointBits> = ours.iter().filter(near).copied().collect();
            let their_bodies: Vec<BodyBits> = def.bodies.iter().map(|&b| twin.game_body(b)).collect();
            let our_bodies: Vec<BodyBits> = def.bodies.iter().map(|&b| twin.port_body(b)).collect();
            let r = &mut results[w];
            r.steps += 1;
            r.joints += their_joints.len() as u64;
            r.max_joints = r.max_joints.max(their_joints.len());
            r.steps_touching += !their_joints.is_empty() as usize;
            if their_joints == our_joints && their_bodies == our_bodies {
                r.same_steps += 1;
            } else {
                let what = if their_joints != our_joints { "contact joints" } else { "body state" };
                let f3 = |v: [u32; 3]| v.map(f32::from_bits);
                let mut text = format!("step {step}: {what} differ\n");
                for (k, (t, o)) in their_bodies.iter().zip(&our_bodies).enumerate() {
                    text.push_str(&format!(
                        "  body {k}: game pos {:?} lvel {:?} avel {:?}\n          port pos {:?} lvel {:?} avel {:?}\n          bits game {:08x?} {:08x?} {:08x?} {:08x?}\n          bits port {:08x?} {:08x?} {:08x?} {:08x?}\n",
                        f3(t.pos), f3(t.lvel), f3(t.avel), f3(o.pos), f3(o.lvel), f3(o.avel), t.pos, t.q, t.lvel, t.avel, o.pos, o.q, o.lvel, o.avel
                    ));
                }
                let contacts = |j: &[JointBits]| describe(&j.iter().map(|j| j.contact).collect::<Vec<_>>());
                text.push_str(&format!("  game: {} joints\n{}  port: {} joints\n{}", their_joints.len(), contacts(&their_joints), our_joints.len(), contacts(&our_joints)));
                r.first = Some(text);
                // a world that has left the game's path is not compared any further
                dead[w] = true;
            }
        }
        // (the solver's numbers are the port's and for all worlds together)
        results[0].bounded_rows += stats.bounded_rows as u64;
        results[0].pivots += stats.lcp_pivots as u64;
        results[0].lcp_errors += stats.lcp_errors;
    }

    let mut out = String::new();
    out.push_str("| World | What | Steps | Steps with contacts | Contact joints (sum, most at once) | Bit-exact steps | First difference |\n|---|---|---|---|---|---|---|\n");
    let mut ok = true;
    for (def, r) in defs.iter().zip(&results) {
        let all = r.same_steps == steps;
        ok &= all;
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {}, {} | {} ({:.2} %) | {} |\n",
            def.name,
            def.about,
            steps,
            r.steps_touching,
            r.joints,
            r.max_joints,
            r.same_steps,
            r.same_steps as f64 * 100.0 / steps as f64,
            if all { "none".to_string() } else { format!("step {}", r.same_steps) }
        ));
    }
    if hybrid {
        out.push_str("\n**Hybrid run**: the port did not look for contacts itself; before every `dWorldStep` it was given the game's contact joints of that step. This checks the contact joint, the stepper and the LCP solver alone.\n");
    }
    out.push_str(&format!(
        "\nEvery step is `PhysicsCore::step` (the collision pass of that step's parity, then `dWorldStep`) on both sides; compared are position, quaternion, linear and angular velocity of every body and the contact joints (seed {seed}). Over all worlds the port's solver had {} rows with limits and made {} pivots; it gave up {} times (\"s <= 0\").\n",
        results[0].bounded_rows, results[0].pivots, results[0].lcp_errors
    ));
    out.push_str(&format!("`bounce_vel` of the game's contact joints in this run: box contacts {:08x?}, mesh contacts {:08x?}.\n", twin.box_bounce_vels, twin.mesh_bounce_vels));
    for (def, r) in defs.iter().zip(&results) {
        if let Some(first) = &r.first {
            out.push_str(&format!("\nFirst difference of `{}`:\n\n```\n{first}\n```\n", def.name));
        }
    }
    Ok((out, ok))
}
