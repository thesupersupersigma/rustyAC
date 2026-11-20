// SPDX-License-Identifier: BSD-3-Clause

//! The collision part of Custom Shaders Patch 0.3.2's (build 4305) world step (+0x92c2b0, its "v2"
//! path) in f64: geoms and spaces as CSP's world holds them for cars on a track, and the pipeline
//! CSP runs every step to make contact joints from them. CSP alternates two kinds of step
//! (world +0x4c): a static step asks the track's hash space for what each car space's box touches
//! and collides every car geom against what it found (`dSpaceCollide2` +0x168a5b0, the simple
//! space's `collide2` +0x1688800, the callback +0x92bef0), keeping the pairs that gave contacts; the
//! dynamic step after it collides those pairs again (+0x9324a0) and looks for cars touching each
//! other (+0x91bd40). A pair's contacts go through `dCollide` (+0x1687280, box against trimesh by
//! [`crate::csp_btl`]), the reduction ([`crate::csp_reduce`]) and the joint maker
//! ([`crate::csp_collide`]). Only what a Spa drive runs is here: car spaces (simple spaces of boxes)
//! in one container, the track's hash space of trimeshes; anything else gives [`Unhandled`].

use crate::csp_btl::{collide_btl, ContactGeom, Mesh};
use crate::csp_collide::{contact_joints, NewJoint, Side, Switches, Unhandled};
use crate::csp_opcode::{aabb_test, BoxTc};
use crate::csp_reduce::{reduce, Reduce};
use crate::csp_space::{overlap, HashSpace};
use crate::csp_step::{Joint, JointKind};
use crate::csp_world::{CspWorld, WorldJoint};
use crate::opcode::{MeshInterface, Model};
use crate::world::{Body, BODY_DISABLED};

/// dxGeom flags (+0xc).
pub const GEOM_DIRTY: u32 = 1;
pub const GEOM_POSR_BAD: u32 = 2;
pub const GEOM_AABB_BAD: u32 = 4;
pub const GEOM_ENABLED: u32 = 16;
pub const GEOM_ZERO_SIZED: u32 = 32;

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // one per geom, a few dozen per world
pub enum Shape {
    /// a box: sides (+0xa0) and its OPCODE temporal-coherence block (+0x98)
    Box { sides: [f64; 3], tc: BoxTc },
    /// a trimesh: index into [`Collision::meshes`] (none for a tree kind not ported), `doBoxTC`
    /// (+0xc9), its TC key (data +0xf0) and
    /// whether its `AABBTest` (+0x1693970) walks the tree (the switch on, a tree, an identity pose)
    Mesh { mesh: Option<usize>, tc_on: bool, key: u32, tree_test: bool },
    /// a simple space: children in list order
    Space { children: Vec<usize> },
}

/// A geom as CSP's collision reads it.
#[derive(Clone, Debug)]
pub struct Geom {
    /// what contacts carry as g1 / g2 (CSP's pointer, to compare)
    pub id: usize,
    /// class, user-data and body facts the joint maker reads; `side.body` indexes the world's bodies
    pub side: Side,
    /// +0xc, see the `GEOM_` flags
    pub flags: u32,
    /// +0x90 and +0x94
    pub category: u32,
    pub collide: u32,
    /// the offset from the body (+0x30): position and rotation
    pub offset: Option<([f64; 3], [f64; 12])>,
    /// the final pose (+0x28), valid unless `GEOM_POSR_BAD`
    pub pos: [f64; 3],
    pub r: [f64; 12],
    /// +0x60, valid unless `GEOM_AABB_BAD`
    pub aabb: [f64; 6],
    pub shape: Shape,
}

pub struct TrackMesh {
    pub mesh: MeshInterface,
    pub model: Model,
}

/// The world's collision state at a step's start.
pub struct Collision {
    pub geoms: Vec<Geom>,
    pub meshes: Vec<TrackMesh>,
    /// the car container's children (car spaces), list order, with the container's sublevel
    /// (+0xb4) and child count (+0xa0)
    pub cars: Vec<usize>,
    pub cars_level: i32,
    pub cars_count: i32,
    /// the track's hash space: children in list order, sublevel, count and its table
    /// (shortcut: built by the caller from the children's boxes; a track geom that moves is not handled)
    pub track: Vec<usize>,
    pub track_level: i32,
    pub track_count: i32,
    pub table: HashSpace,
    /// world +0x50: the pairs the last static step got contacts from, as (geom, geom)
    pub pairs: Vec<(usize, usize)>,
    /// world +0x4c set
    pub static_step: bool,
    /// `TRIMESH_AABB_TEST` (+0x27f995c) and OPCODE's full box-box test (collider +0x144)
    pub aabb_limit: i32,
    pub full_box_box: bool,
    pub switches: Switches,
}

const REDUCE: Reduce = Reduce { per_group: 4, groups: 1, total: 4, same_normal: 0.95 };

fn is_space(g: &Geom) -> bool {
    matches!(g.shape, Shape::Space { .. })
}

fn enabled(g: &Geom) -> bool {
    g.flags & (GEOM_ENABLED | GEOM_ZERO_SIZED) == GEOM_ENABLED
}

/// The pose from the body and the offset (+0x1686600) when `GEOM_POSR_BAD`.
fn posr(g: &mut Geom, bodies: &[Body<f64>]) -> Result<(), Unhandled> {
    if g.flags & GEOM_POSR_BAD == 0 {
        return Ok(());
    }
    let (Some(b), Some((o, orr))) = (g.side.body, g.offset) else { return Err(Unhandled("pose without body or offset")) };
    let (bp, a) = (&bodies[b].pos, &bodies[b].r);
    // operands in the machine code's order (a NaN's sign and payload follow the first operand)
    for i in 0..3 {
        let r = 4 * i;
        g.pos[i] = bp[i] + ((o[1] * a[r + 1] + o[0] * a[r]) + o[2] * a[r + 2]);
        g.r[r] = (a[r] * orr[0] + a[r + 1] * orr[4]) + a[r + 2] * orr[8];
        // clippy sees a + b and b + a as the same block; the order is CSP's (NaN sign follows the first operand)
        #[allow(clippy::if_same_then_else)]
        let m = if i < 2 { a[r + 1] * orr[5] + a[r] * orr[1] } else { a[r] * orr[1] + a[r + 1] * orr[5] };
        g.r[r + 1] = m + a[r + 2] * orr[9];
        g.r[r + 2] = (a[r + 1] * orr[6] + a[r] * orr[2]) + a[r + 2] * orr[10];
    }
    g.flags &= !GEOM_POSR_BAD;
    Ok(())
}

/// `recomputeAABB` of `geoms[i]`: the pose, then the class's `computeAABB` when `GEOM_AABB_BAD`.
pub fn refresh(geoms: &mut [Geom], i: usize, bodies: &[Body<f64>]) -> Result<(), Unhandled> {
    if geoms[i].flags & GEOM_AABB_BAD == 0 {
        return Ok(());
    }
    posr(&mut geoms[i], bodies)?;
    let g = &geoms[i];
    let aabb = match &g.shape {
        // +0x1699680
        Shape::Box { sides: s, .. } => {
            let (p, r) = (g.pos, g.r);
            let dx = (((s[1] * r[1]).abs() + (s[0] * r[0]).abs()) + (s[2] * r[2]).abs()) * 0.5;
            let d = |row: usize| (((s[0] * r[row]).abs() + (s[1] * r[row + 1]).abs()) + (s[2] * r[row + 2]).abs()) * 0.5;
            let (dy, dz) = (d(4), d(8));
            [p[0] - dx, dx + p[0], p[1] - dy, dy + p[1], p[2] - dz, dz + p[2]]
        }
        // +0x1688460: the children's union, each refreshed first
        Shape::Space { children } => {
            let kids = children.clone();
            let mut a = [1e300, -1e300, 1e300, -1e300, 1e300, -1e300];
            for &k in &kids {
                refresh(geoms, k, bodies)?;
                let b = geoms[k].aabb;
                for e in [0, 2, 4] {
                    // comisd + jae: a NaN low side is taken, a NaN high side is not
                    if !(b[e] >= a[e]) {
                        a[e] = b[e];
                    }
                    if b[e + 1] > a[e + 1] {
                        a[e + 1] = b[e + 1];
                    }
                }
            }
            if kids.is_empty() {
                a = [0.0; 6];
            }
            a
        }
        Shape::Mesh { .. } => return Err(Unhandled("a trimesh's box")),
    };
    let g = &mut geoms[i];
    g.aabb = aabb;
    g.flags &= !GEOM_AABB_BAD;
    Ok(())
}

/// `AABBTest` of `g` against `other`'s box (only trimeshes test anything).
fn aabb_ok(c: &Collision, g: usize, other: usize) -> bool {
    match c.geoms[g].shape {
        Shape::Mesh { mesh: Some(mesh), tree_test: true, .. } if c.aabb_limit != 0 && c.geoms[other].flags & 0x8_0000 == 0 => aabb_test(&c.meshes[mesh].model.nodes, &c.geoms[other].aabb, c.aabb_limit),
        _ => true,
    }
}

/// The pair test before a callback (+0x16880b0 and the hash space's): bodies, categories either
/// way, boxes, `AABBTest` both ways.
fn pair_ok(c: &Collision, a: usize, b: usize) -> bool {
    let (ga, gb) = (&c.geoms[a], &c.geoms[b]);
    (ga.side.body != gb.side.body || ga.side.body.is_none())
        && (ga.category & gb.collide != 0 || ga.collide & gb.category != 0)
        && overlap(&ga.aabb, &gb.aabb)
        && aabb_ok(c, a, b)
        && aabb_ok(c, b, a)
}

fn disabled(c: &Collision, bodies: &[Body<f64>], a: usize, b: usize) -> Result<bool, Unhandled> {
    let g = if c.geoms[a].side.body.is_some() { a } else { b };
    let body = c.geoms[g].side.body.ok_or(Unhandled("a pair without a body"))?;
    Ok(bodies[body].flags & BODY_DISABLED != 0)
}

/// `dCollide` (+0x1687280) with room for `max` contacts.
fn dcollide(c: &mut Collision, bodies: &[Body<f64>], a: usize, b: usize, max: u32) -> Result<Vec<ContactGeom>, Unhandled> {
    if a == b || (c.geoms[a].side.body.is_some() && c.geoms[a].side.body == c.geoms[b].side.body) {
        return Ok(Vec::new());
    }
    posr(&mut c.geoms[a], bodies)?;
    posr(&mut c.geoms[b], bodies)?;
    // box against trimesh is registered as (trimesh, box): called swapped, the contacts turned back
    let (bx, m) = match (&c.geoms[a].shape, &c.geoms[b].shape) {
        (Shape::Box { .. }, Shape::Mesh { .. }) => (a, b),
        _ => return Err(Unhandled("a pair other than box against trimesh")),
    };
    let Shape::Mesh { mesh, tc_on, key, .. } = c.geoms[m].shape else { unreachable!() };
    let mesh = mesh.ok_or(Unhandled("a trimesh tree kind not ported"))?;
    let (mid, bid, mpos, mr) = (c.geoms[m].id, c.geoms[bx].id, c.geoms[m].pos, c.geoms[m].r);
    let (full_box_box, tm) = (c.full_box_box, &c.meshes[mesh]);
    let g = &mut c.geoms[bx];
    let Shape::Box { sides, tc } = &mut g.shape else { unreachable!() };
    let slot = tc_on.then(|| tc.slot(mid, key, 1.1));
    let pose = Mesh { mesh: &tm.mesh, model: &tm.model, pos: mpos, r: mr };
    let mut out = collide_btl(&pose, &g.pos, &g.r, sides, max, mid, bid, slot, full_box_box);
    for k in &mut out {
        k.normal = k.normal.map(|x| -x);
        (k.g1, k.g2, k.side1, k.side2) = (k.g2, k.g1, k.side2, k.side1);
    }
    Ok(out)
}

/// One pair's contacts to joints: the reduction for a box, then the joint maker.
fn pair_joints(c: &mut Collision, bodies: &[Body<f64>], a: usize, b: usize, out: &mut Vec<NewJoint>) -> Result<bool, Unhandled> {
    let g = if c.geoms[a].side.body.is_some() { a } else { b };
    if !matches!(c.geoms[g].side.class, 1 | 10) {
        return Err(Unhandled("a pair whose body geom is not a box"));
    }
    let mut contacts = dcollide(c, bodies, a, b, 16)?;
    if contacts.is_empty() {
        return Ok(false);
    }
    if reduce(&mut contacts, &REDUCE) == 0 {
        return Err(Unhandled("a reduction keeping no contact"));
    }
    out.extend(contact_joints(&contacts, &c.geoms[a].side, &c.geoms[b].side, bodies, &c.switches)?);
    Ok(true)
}

impl CspWorld {
    /// The step's collision: contact joints in the order CSP makes them; `c` keeps its state
    /// (poses, boxes, caches, the pair list) for the next step.
    pub fn collide(&self, c: &mut Collision) -> Result<Vec<NewJoint>, Unhandled> {
        let bodies = &self.bodies;
        let mut out = Vec::new();
        if !c.static_step {
            // +0x9324a0 on each kept pair, untagged: no category or box test
            for (a, b) in std::mem::take(&mut c.pairs) {
                if !disabled(c, bodies, a, b)? {
                    pair_joints(c, bodies, a, b, &mut out)?;
                }
            }
            // +0x91bd40: cars against each other (shortcut: any two car spaces passing the pair
            // test is unhandled, port the car-car callback +0x92bcb0 when a drive needs it)
            for k in 0..c.cars.len() {
                refresh(&mut c.geoms, c.cars[k], bodies)?;
            }
            for (i, &a) in c.cars.iter().enumerate() {
                for &b in &c.cars[i + 1..] {
                    if enabled(&c.geoms[a]) && enabled(&c.geoms[b]) && pair_ok(c, a, b) {
                        return Err(Unhandled("cars touching"));
                    }
                }
            }
            return Ok(out);
        }
        // dSpaceCollide2(container, track): both spaces of one sublevel, the track has more children
        if c.cars_level != c.track_level || c.cars_count >= c.track_count {
            return Err(Unhandled("container and track spaces of another shape"));
        }
        let mut pairs = Vec::new();
        for k in 0..c.cars.len() {
            let s = c.cars[k];
            // the hash space's collide2 (+0x1689bf0) with the car space as the query
            refresh(&mut c.geoms, s, bodies)?;
            if !enabled(&c.geoms[s]) {
                continue;
            }
            let mut found = Vec::new();
            c.table.collide2(&c.geoms[s].aabb, |t| found.push(c.track[t]));
            for t in found {
                if !enabled(&c.geoms[t]) || !pair_ok(c, t, s) {
                    continue;
                }
                // the callback (+0x92bef0) with (car space, trimesh): a space, so dSpaceCollide2,
                // the car space's collide2 (+0x1688800) with the trimesh as the query
                let Shape::Space { children } = &c.geoms[s].shape else { unreachable!() };
                let children = children.clone();
                // cleanGeoms (+0x16886d0): the dirty geoms at the list's head
                for &g in &children {
                    if c.geoms[g].flags & GEOM_DIRTY == 0 {
                        break;
                    }
                    if is_space(&c.geoms[g]) {
                        return Err(Unhandled("a space in a car space"));
                    }
                    refresh(&mut c.geoms, g, bodies)?;
                    c.geoms[g].flags &= !GEOM_DIRTY;
                }
                refresh(&mut c.geoms, t, bodies)?;
                for &g in &children {
                    if !enabled(&c.geoms[g]) || !pair_ok(c, g, t) {
                        continue;
                    }
                    // the callback with (car geom, trimesh)
                    let (gg, gt) = (&c.geoms[g], &c.geoms[t]);
                    if gt.collide & gg.category == 0 || gt.category & gg.collide == 0 || disabled(c, bodies, g, t)? {
                        continue;
                    }
                    if pair_joints(c, bodies, g, t, &mut out)? {
                        pairs.push((g, t));
                    }
                }
            }
        }
        c.pairs = pairs;
        Ok(out)
    }

    /// Adds contact joints (oldest first) as CSP's `dJointCreateContact` + `dJointAttach` do: each
    /// at the head of the world's joint list and of its bodies' node lists.
    pub fn add_contacts(&mut self, joints: Vec<Joint>) {
        for joint in joints {
            let JointKind::Contact { reverse, .. } = joint.kind else { panic!("a contact joint") };
            let (b0, b1) = (joint.b0, joint.b1);
            self.nodes.iter_mut().flatten().for_each(|n| n.0 += 1);
            self.joints.insert(0, WorldJoint { joint, flags: if reverse { 2 } else { 0 }, tag: 0 });
            self.nodes[b0].insert(0, (0, b1));
            if let Some(b1) = b1 {
                self.nodes[b1].insert(0, (0, Some(b0)));
            }
        }
    }
}

/// A new joint as the world's [`Joint`].
pub fn joint_of(j: &NewJoint, feedback: bool) -> Joint {
    Joint { kind: JointKind::Contact { contact: j.contact, reverse: j.reverse, the_m: 0 }, b0: j.b0, b1: j.b1, feedback: feedback.then_some([0.0; 16]) }
}
