// SPDX-License-Identifier: BSD-3-Clause

//! The collision part of Custom Shaders Patch 0.3.2's (build 4305) world step (+0x92c2b0, its "v2"
//! path) in f64: geoms and spaces as CSP's world holds them for cars on a track, and the pipeline
//! CSP runs every step to make contact joints from them. CSP alternates two kinds of step
//! (world +0x4c): a static step asks the track's hash space for what each car space's box touches
//! and collides every car geom against what it found (`dSpaceCollide2` +0x168a5b0, the simple
//! space's `collide2` +0x1688800, the callback +0x92bef0), keeping the pairs that gave contacts; the
//! dynamic step after it collides those pairs again (+0x9324a0) and looks for cars touching each
//! other (+0x91bd40: the car container's hash table, then the two car spaces' geoms, +0x92bab0),
//! whose pairs the next static step collides again. A pair's contacts go through `dCollide`
//! (+0x1687280: box against trimesh by [`crate::csp_btl`], trimesh against trimesh by
//! [`crate::csp_ttl`]), the reduction ([`crate::csp_reduce`], for trimeshes with limits from the
//! step's contact load, +0x169fd90) and the joint maker ([`crate::csp_collide`]). Only what a Spa
//! drive runs is here: car spaces (simple spaces of boxes and a trimesh) in one container, the
//! track's hash space of trimeshes; anything else gives [`Unhandled`].

use crate::csp_btl::{collide_btl, ContactGeom, Mesh};
use crate::csp_collide::{contact_joints, NewJoint, Side, Switches, Unhandled};
use crate::csp_opcode::{aabb_test, BoxTc};
use crate::csp_reduce::{reduce, Reduce};
use crate::csp_space::{overlap, HashSpace};
use crate::csp_step::{Joint, JointKind};
use crate::csp_ttl::collide_ttl;
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
    /// (+0xc9), its TC key (data +0xf0),
    /// whether its `AABBTest` (+0x1693970) walks the tree (the switch on, a tree, an identity pose)
    /// and the data's box: centre (+0xa0) and half sides (+0xc0)
    Mesh { mesh: Option<usize>, tc_on: bool, key: u32, tree_test: bool, bounds: [f64; 6] },
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

/// The contact load (+0x31f8 on): contacts kept this step, the limits between which the
/// reduction gets stricter, the running average and its rise and fall rates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load {
    pub counter: i32,
    pub lo: i32,
    pub hi: i32,
    pub avg: f64,
    pub up: f64,
    pub down: f64,
}

impl Default for Load {
    /// as the core's constructor (+0x6149c0) sets it
    fn default() -> Load {
        Load { counter: 0, lo: 50, hi: 150, avg: 0.0, up: 1.0, down: 0.05 }
    }
}

/// The world's collision state at a step's start.
#[derive(Default)]
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
    /// the car container's +217: a car space moved since its table was built
    pub cars_dirty: bool,
    /// the car container's table (built when `cars_built` is clear or `cars_dirty` set) and its
    /// lowest and highest level
    pub cars_table: HashSpace,
    pub cars_built: bool,
    pub cars_levels: (i32, i32),
    /// world +0x60: the car-car pairs the last dynamic step got contacts from
    pub car_pairs: Vec<(usize, usize)>,
    pub load: Load,
    /// `TRIMESH_OBB_PREFILTER` (byte +0x27f91a4) and the tree collider's edge axes (byte +0x27f91a6)
    pub prefilter: bool,
    pub edges: bool,
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
pub fn posr(g: &mut Geom, bodies: &[Body<f64>]) -> Result<(), Unhandled> {
    if g.flags & GEOM_POSR_BAD == 0 {
        return Ok(());
    }
    let (Some(b), Some((o, orr))) = (g.side.body, g.offset) else { return Err(Unhandled("pose without body or offset")) };
    let (bp, a) = (&bodies[b].pos, &bodies[b].r);
    // operands in the machine code's order (a NaN's sign and payload follow the first operand)
    for i in 0..3 {
        let r = 4 * i;
        g.pos[i] = bp[i] + ((o[1] * a[r + 1] + o[0] * a[r]) + o[2] * a[r + 2]);
    }
    g.r = product(a, &orr);
    g.flags &= !GEOM_POSR_BAD;
    Ok(())
}

/// The rotation product `a b` of +0x1686440, in its operand order.
pub fn product(a: &[f64; 12], b: &[f64; 12]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for i in 0..3 {
        let r = 4 * i;
        out[r] = (a[r] * b[0] + a[r + 1] * b[4]) + a[r + 2] * b[8];
        // clippy sees a + b and b + a as the same block; the order is CSP's (NaN sign follows the first operand)
        #[allow(clippy::if_same_then_else)]
        let m = if i < 2 { a[r + 1] * b[5] + a[r] * b[1] } else { a[r] * b[1] + a[r + 1] * b[5] };
        out[r + 1] = m + a[r + 2] * b[9];
        out[r + 2] = (a[r + 1] * b[6] + a[r] * b[2]) + a[r + 2] * b[10];
    }
    out
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
        // +0x1694130: the data's box turned by the pose
        Shape::Mesh { bounds: [c0, c1, c2, e0, e1, e2], .. } => {
            let (p, r) = (g.pos, g.r);
            let t0 = (c0 * r[0] + r[1] * c1) + r[2] * c2;
            let t1 = (c1 * r[5] + r[4] * c0) + c2 * r[6];
            let t2 = (c1 * r[9] + r[8] * c0) + c2 * r[10];
            let d0 = ((e0 * r[0]).abs() + (e1 * r[1]).abs()) + (e2 * r[2]).abs();
            let d1 = ((e1 * r[5]).abs() + (r[4] * e0).abs()) + (e2 * r[6]).abs();
            let d2 = ((e1 * r[9]).abs() + (r[8] * e0).abs()) + (e2 * r[10]).abs();
            [(t0 + p[0]) - d0, (t0 + p[0]) + d0, (t1 + p[1]) - d1, (t1 + p[1]) + d1, (t2 + p[2]) - d2, (t2 + p[2]) + d2]
        }
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
        (Shape::Mesh { mesh: m1, bounds: b1, .. }, Shape::Mesh { mesh: m2, bounds: b2, .. }) => {
            let (Some(m1), Some(m2)) = (*m1, *m2) else { return Err(Unhandled("a trimesh tree kind not ported")) };
            let (ga, gb) = (&c.geoms[a], &c.geoms[b]);
            let p1 = Mesh { mesh: &c.meshes[m1].mesh, model: &c.meshes[m1].model, pos: ga.pos, r: ga.r };
            let p2 = Mesh { mesh: &c.meshes[m2].mesh, model: &c.meshes[m2].model, pos: gb.pos, r: gb.r };
            return Ok(collide_ttl(&p1, b1, &p2, b2, max, ga.id, gb.id, c.prefilter, c.edges));
        }
        _ => return Err(Unhandled("a pair other than box against trimesh or trimesh against trimesh")),
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

/// +0x92b9d0: a geom's reduction weight (none without a body).
fn weight(g: &Geom) -> f32 {
    if g.side.body.is_some() {
        g.side.weight
    } else {
        0.0
    }
}

/// +0x92ba10: the pair moves apart or together faster than 1 m/s.
fn fast(c: &Collision, bodies: &[Body<f64>], a: usize, b: usize) -> bool {
    let l = |g: usize| c.geoms[g].side.body.map(|b| bodies[b].lvel);
    match (l(a), l(b)) {
        (Some(x), Some(y)) => {
            let d = [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
            (d[1] * d[1] + d[0] * d[0]) + d[2] * d[2] > 1.0
        }
        (Some(x), None) | (None, Some(x)) => (x[0] * x[0] + x[1] * x[1]) + x[2] * x[2] > 1.0,
        (None, None) => false,
    }
}

/// +0x169fd90: the reduction with limits from the contact load and the pair's `weight`; `few`
/// keeps at least three. Kept contacts add to the load.
fn reduce_by_load(contacts: &mut Vec<ContactGeom>, l: &mut Load, weight: f64, few: bool) -> usize {
    let lo = l.lo as f64;
    let v = if l.avg > l.counter as f64 { l.avg } else { l.counter as f64 };
    let free = if v > lo {
        if !(v >= l.hi as f64) {
            1.0 - (v - lo) / l.hi.wrapping_sub(l.lo) as f64
        } else {
            0.0
        }
    } else {
        1.0
    };
    let w = if weight >= 0.0 {
        if weight < 1.0 {
            weight
        } else {
            1.0
        }
    } else {
        0.0
    };
    let strict = (1.0 - free) * (1.0 - w);
    let mut p = Reduce { per_group: 4, groups: 2, total: 8, same_normal: 0.95 };
    if strict > 0.25 {
        p.groups = 1;
        (p.per_group, p.total) = if strict > 0.5 {
            if strict > 0.75 {
                (1, 1)
            } else {
                (3, 3)
            }
        } else {
            (4, 4)
        };
    }
    if few {
        (p.per_group, p.total) = (p.per_group.max(3), p.total.max(3));
    }
    let n = reduce(contacts, &p);
    if n > 0 {
        l.counter += n as i32;
    }
    n
}

/// A kept pair again (+0x9324a0, untagged): a box by the fixed reduction, else by the load with
/// the body geom's weight; then the joint maker.
fn pair_joints(c: &mut Collision, bodies: &[Body<f64>], a: usize, b: usize, out: &mut Vec<NewJoint>) -> Result<bool, Unhandled> {
    let g = if c.geoms[a].side.body.is_some() { a } else { b };
    let is_box = matches!(c.geoms[g].side.class, 1 | 10);
    let mut contacts = dcollide(c, bodies, a, b, if is_box { 16 } else { 32 })?;
    if contacts.is_empty() {
        return Ok(false);
    }
    let n = if is_box {
        reduce(&mut contacts, &REDUCE)
    } else {
        let l = c.geoms[g].side.body.map_or([0.0; 4], |x| bodies[x].lvel);
        let w = weight(&c.geoms[g]) as f64;
        reduce_by_load(&mut contacts, &mut c.load, w, l[0] * l[0] + l[1] * l[1] + l[2] * l[2] <= 1.0)
    };
    if n == 0 {
        return Err(Unhandled("a reduction keeping no contact"));
    }
    out.extend(contact_joints(&contacts, &c.geoms[a].side, &c.geoms[b].side, bodies, &c.switches)?);
    Ok(true)
}

/// The simple space's `cleanGeoms` (+0x16886d0): the dirty geoms at the list's head.
fn clean(c: &mut Collision, s: usize, bodies: &[Body<f64>]) -> Result<Vec<usize>, Unhandled> {
    let Shape::Space { children } = &c.geoms[s].shape else { return Err(Unhandled("a car container child not a space")) };
    let children = children.clone();
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
    Ok(children)
}

/// +0x92bab0 with two geoms of two cars: categories both ways, a body awake, then `dCollide`, the
/// reduction by the load (the bigger weight, at least three unless fast) and the joint maker;
/// a pair with contacts is kept for the next static step.
fn car_geoms(c: &mut Collision, bodies: &[Body<f64>], a: usize, b: usize, out: &mut Vec<NewJoint>) -> Result<(), Unhandled> {
    let (ga, gb) = (&c.geoms[a], &c.geoms[b]);
    if gb.collide & ga.category == 0 || gb.category & ga.collide == 0 {
        return Ok(());
    }
    let awake = |g: &Geom| g.side.body.is_some_and(|x| bodies[x].flags & BODY_DISABLED == 0);
    if !awake(ga) && !awake(gb) {
        return Ok(());
    }
    let mut contacts = dcollide(c, bodies, a, b, 32)?;
    if contacts.is_empty() {
        return Ok(());
    }
    let (wa, wb) = (weight(&c.geoms[a]), weight(&c.geoms[b]));
    let w = if wa > wb { wa } else { wb };
    let few = !fast(c, bodies, a, b);
    if reduce_by_load(&mut contacts, &mut c.load, w as f64, few) == 0 {
        return Err(Unhandled("a reduction keeping no contact"));
    }
    out.extend(contact_joints(&contacts, &c.geoms[a].side, &c.geoms[b].side, bodies, &c.switches)?);
    c.car_pairs.push((a, b));
    Ok(())
}

/// `dSpaceCollide2` (+0x168a5b0) of two car spaces: the space with more geoms (the first on a tie)
/// takes each of the other's as a query (its `collide2`, +0x1688800); pairs reach the callback as
/// (geom of `a`, geom of `b`).
fn car_spaces(c: &mut Collision, bodies: &[Body<f64>], a: usize, b: usize, out: &mut Vec<NewJoint>) -> Result<(), Unhandled> {
    let count = |s: usize| match &c.geoms[s].shape {
        Shape::Space { children } => children.len(),
        _ => 0,
    };
    let (big, small) = if count(a) >= count(b) { (a, b) } else { (b, a) };
    let Shape::Space { children } = &c.geoms[small].shape else { return Err(Unhandled("a car container child not a space")) };
    for q in children.clone() {
        let kids = clean(c, big, bodies)?;
        refresh(&mut c.geoms, q, bodies)?;
        for k in kids {
            if enabled(&c.geoms[k]) && pair_ok(c, k, q) {
                if big == a {
                    car_geoms(c, bodies, k, q, out)?;
                } else {
                    car_geoms(c, bodies, q, k, out)?;
                }
            }
        }
    }
    Ok(())
}

impl CspWorld {
    /// The step's collision: contact joints in the order CSP makes them; `c` keeps its state
    /// (poses, boxes, caches, the pair list) for the next step.
    pub fn collide(&self, c: &mut Collision) -> Result<Vec<NewJoint>, Unhandled> {
        let bodies = &self.bodies;
        let mut out = Vec::new();
        let l = &mut c.load;
        // comisd + cmovbe: a NaN takes the falling rate
        let rate = if !(l.counter as f64 > l.avg) { l.down } else { l.up };
        l.avg = (l.counter as f64 - l.avg) * rate + l.avg;
        l.counter = 0;
        if !c.static_step {
            // +0x9324a0 on each kept pair, untagged: no category or box test
            for (a, b) in std::mem::take(&mut c.pairs) {
                if !disabled(c, bodies, a, b)? {
                    pair_joints(c, bodies, a, b, &mut out)?;
                }
            }
            // +0x91bd40: the car container's self-collide (+0x16894f0) on its table, each pair of
            // car spaces through +0x92bcb0
            if c.cars.len() >= 2 {
                // the table as the step's prepare left it: rebuilt only when never built or with
                // +218 set (the oracle checks it clear), even though the cars moved since
                if !c.cars_built {
                    return Err(Unhandled("the car container's table never built"));
                }
                let mut found = Vec::new();
                c.cars_table.self_pairs(|e, o| found.push((c.cars[e], c.cars[o])));
                for (a, b) in found {
                    if enabled(&c.geoms[a]) && enabled(&c.geoms[b]) && pair_ok(c, a, b) {
                        car_spaces(c, bodies, a, b, &mut out)?;
                    }
                }
            }
            return Ok(out);
        }
        // the car pairs the dynamic step kept, again through +0x9324a0
        for (a, b) in std::mem::take(&mut c.car_pairs) {
            if !disabled(c, bodies, a, b)? {
                pair_joints(c, bodies, a, b, &mut out)?;
            }
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
                let children = clean(c, s, bodies)?;
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
