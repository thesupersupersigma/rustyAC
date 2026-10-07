// SPDX-License-Identifier: BSD-3-Clause

//! Stage 3, part 2: geoms that move with bodies, ODE's simple spaces and the broad phase.
//!
//! `ode/src/collision_kernel.cpp` and `collision_space.cpp` as `acs.exe` uses them: boxes and
//! triangle meshes attached to a body through an offset, kept in nested `dxSimpleSpace`s,
//! tested against each other (`dSpaceCollide`) or against the static world of
//! [`crate::collision`] (`dSpaceCollide2`).
//!
//! **Order is everything here.** The order in which pairs reach the near callback decides
//! the order of the contact joints and with it the rows of the solver's matrix. ODE's simple
//! space is a linked list that
//!
//! * grows at its head (`dxSpace::add`), and
//! * moves a geom to its head whenever the geom is marked dirty (`dxSpace::dirty`, from
//!   `dGeomMoved`), which happens for every geom of a body, newest geom first, each time
//!   the body is moved (`dxStepBody`, `dBodySetPosition` …) and the geom was clean.
//!
//! Both are kept here exactly. The static world never moves, so its lists stay as built
//! ([`StaticWorld`] walks them from the newest member).

use std::sync::Arc;

use crate::collision::{boxes_meet, StaticWorld, TriMeshData};
use crate::odemath::{multiply0_331, multiply0_333};
use crate::contact::ContactGeom;
use crate::world::{Body, BodyId, World};
use crate::Matrix3;

/// Handle of a geom of the moving store (`dGeomID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeomId(pub u32);

/// Any geom the broad phase can meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GeomRef {
    /// A geom of the world's own (moving) store.
    Dyn(GeomId),
    /// The static space itself (`PhysicsCore::spaceStatic`).
    StaticSpace,
    /// One of the static space's numbered sub-spaces: index into [`StaticWorld::spaces`].
    StaticSub(u32),
    /// A static mesh: index into [`StaticWorld::meshes`].
    StaticMesh(u32),
}

/// geom is 'dirty', i.e. position unknown
pub const GEOM_DIRTY: u32 = 1;
/// geom's final posr is not valid
pub const GEOM_POSR_BAD: u32 = 2;
/// geom's AABB is not valid
pub const GEOM_AABB_BAD: u32 = 4;
/// geom is placeable
pub const GEOM_PLACEABLE: u32 = 8;
/// geom is enabled
pub const GEOM_ENABLED: u32 = 16;
/// geom is zero sized
pub const GEOM_ZERO_SIZED: u32 = 32;

/// `dBoxClass`
pub const CLASS_BOX: u32 = 1;
/// `dTriMeshClass`
pub const CLASS_TRIMESH: u32 = 8;
/// `dSimpleSpaceClass`
pub const CLASS_SIMPLE_SPACE: u32 = 10;

const IDENTITY: Matrix3 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// What a geom is.
#[derive(Clone, Debug)]
pub enum GeomKind {
    /// `dxBox`: side lengths.
    Box { side: [f32; 3] },
    /// `dxTriMesh`.
    TriMesh { data: Arc<TriMeshData> },
    /// `dxSimpleSpace`: head of its list and the number of members.
    SimpleSpace { first: Option<GeomId>, count: u32 },
}

/// `dxPosR`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosR {
    pub pos: [f32; 3],
    pub r: Matrix3,
}

/// `dxGeom`.
#[derive(Clone, Debug)]
pub struct Geom {
    pub kind: GeomKind,
    pub gflags: u32,
    /// `data`: whatever the owner wants to find again.
    pub data: u64,
    pub body: Option<BodyId>,
    /// Next geom of the same body (`body_next`).
    pub body_next: Option<GeomId>,
    /// `final_posr` when it is the geom's own (no body, or a body with an offset). A geom on
    /// a body without an offset shares the body's.
    pub posr: PosR,
    /// `offset_posr`: where the geom sits in its body's frame.
    pub offset: Option<PosR>,
    /// Next and previous geom in the parent space's list.
    pub next: Option<GeomId>,
    pub prev: Option<GeomId>,
    pub parent_space: Option<GeomId>,
    /// min x, max x, min y, max y, min z, max z
    pub aabb: [f32; 6],
    pub category_bits: u32,
    pub collide_bits: u32,
}

impl Geom {
    /// `dGeomGetClass`.
    pub fn class(&self) -> u32 {
        match self.kind {
            GeomKind::Box { .. } => CLASS_BOX,
            GeomKind::TriMesh { .. } => CLASS_TRIMESH,
            GeomKind::SimpleSpace { .. } => CLASS_SIMPLE_SPACE,
        }
    }

    pub fn is_space(&self) -> bool {
        matches!(self.kind, GeomKind::SimpleSpace { .. })
    }

    /// `GEOM_ENABLED(g)`: enabled and not zero sized.
    fn enabled(&self) -> bool {
        self.gflags & (GEOM_ENABLED | GEOM_ZERO_SIZED) == GEOM_ENABLED
    }
}

/// The moving geoms of a world.
#[derive(Clone, Debug, Default)]
pub struct Collision {
    pub geoms: Vec<Geom>,
}

/// The game's (or a test's) near callback.
pub trait NearCallback {
    fn near(&mut self, broad: &mut BroadPhase, o1: GeomRef, o2: GeomRef);
}

impl Collision {
    fn new_geom(&mut self, kind: GeomKind, placeable: bool, space: Option<GeomId>) -> GeomId {
        let id = GeomId(self.geoms.len() as u32);
        // dxGeom::dxGeom @ 0x140343280
        let mut gflags = GEOM_DIRTY | GEOM_AABB_BAD | GEOM_ENABLED;
        if placeable {
            gflags |= GEOM_PLACEABLE;
        }
        self.geoms.push(Geom {
            kind,
            gflags,
            data: 0,
            body: None,
            body_next: None,
            posr: PosR { pos: [0.0; 3], r: IDENTITY },
            offset: None,
            next: None,
            prev: None,
            parent_space: None,
            aabb: [0.0; 6],
            category_bits: !0,
            collide_bits: !0,
        });
        if let Some(space) = space {
            self.space_add(space, id);
        }
        id
    }

    pub fn geom(&self, g: GeomId) -> &Geom {
        &self.geoms[g.0 as usize]
    }

    pub fn geom_mut(&mut self, g: GeomId) -> &mut Geom {
        &mut self.geoms[g.0 as usize]
    }

    /// `dSimpleSpaceCreate`: an empty space, inside `parent` if one is given.
    pub fn simple_space_create(&mut self, parent: Option<GeomId>) -> GeomId {
        self.new_geom(GeomKind::SimpleSpace { first: None, count: 0 }, false, parent)
    }

    /// `dCreateBox` @ 0x14034a320.
    pub fn create_box(&mut self, space: Option<GeomId>, lx: f32, ly: f32, lz: f32) -> GeomId {
        let id = self.new_geom(GeomKind::Box { side: [lx, ly, lz] }, true, space);
        // updateZeroSizedFlag(!lx || !ly || !lz); ucomiss + je without a parity test, so a
        // NaN side counts as zero too
        let zero = |x: f32| x == 0.0 || x.is_nan();
        if zero(lx) || zero(ly) || zero(lz) {
            self.geoms[id.0 as usize].gflags |= GEOM_ZERO_SIZED;
        }
        id
    }

    /// `dCreateTriMesh` @ 0x14034b0f0.
    pub fn create_tri_mesh(&mut self, space: Option<GeomId>, data: Arc<TriMeshData>) -> GeomId {
        self.new_geom(GeomKind::TriMesh { data }, true, space)
    }

    /// `dGeomSetCategoryBits` @ 0x1403404b0.
    pub fn set_category_bits(&mut self, g: GeomId, bits: u32) {
        self.geoms[g.0 as usize].category_bits = bits;
    }

    /// `dGeomSetCollideBits`.
    pub fn set_collide_bits(&mut self, g: GeomId, bits: u32) {
        self.geoms[g.0 as usize].collide_bits = bits;
    }

    /// The members of a space from the head of its list.
    pub fn members(&self, space: GeomId) -> Vec<GeomId> {
        let mut out = Vec::new();
        let GeomKind::SimpleSpace { first, .. } = self.geoms[space.0 as usize].kind else {
            return out;
        };
        let mut g = first;
        while let Some(id) = g {
            out.push(id);
            g = self.geoms[id.0 as usize].next;
        }
        out
    }

    fn list_push_front(&mut self, space: GeomId, geom: GeomId) {
        let GeomKind::SimpleSpace { first, .. } = self.geoms[space.0 as usize].kind else {
            panic!("not a space");
        };
        {
            let g = &mut self.geoms[geom.0 as usize];
            g.next = first;
            g.prev = None;
        }
        if let Some(old) = first {
            self.geoms[old.0 as usize].prev = Some(geom);
        }
        if let GeomKind::SimpleSpace { first, .. } = &mut self.geoms[space.0 as usize].kind {
            *first = Some(geom);
        }
    }

    fn list_remove(&mut self, space: GeomId, geom: GeomId) {
        let (prev, next) = {
            let g = &self.geoms[geom.0 as usize];
            (g.prev, g.next)
        };
        match prev {
            Some(p) => self.geoms[p.0 as usize].next = next,
            None => {
                if let GeomKind::SimpleSpace { first, .. } = &mut self.geoms[space.0 as usize].kind {
                    *first = next;
                }
            }
        }
        if let Some(n) = next {
            self.geoms[n.0 as usize].prev = prev;
        }
        let g = &mut self.geoms[geom.0 as usize];
        g.next = None;
        g.prev = None;
    }

    /// `dSpaceAdd` -> `dxSpace::add` @ 0x140342980: new geoms go to the front of the list and
    /// are always dirty; so are, as a consequence, this space and all its parents.
    pub fn space_add(&mut self, space: GeomId, geom: GeomId) {
        debug_assert!(self.geoms[geom.0 as usize].parent_space.is_none(), "geom is already in a space");
        self.geoms[geom.0 as usize].parent_space = Some(space);
        self.list_push_front(space, geom);
        if let GeomKind::SimpleSpace { count, .. } = &mut self.geoms[space.0 as usize].kind {
            *count += 1;
        }
        self.geoms[geom.0 as usize].gflags |= GEOM_DIRTY | GEOM_AABB_BAD;
        self.geom_moved(space);
    }

    /// `dGeomMoved` @ 0x140342f80.
    pub fn geom_moved(&mut self, geom: GeomId) {
        let mut geom = geom;
        // if geom is an offset, mark it as needing a recalculate
        if self.geoms[geom.0 as usize].offset.is_some() {
            self.geoms[geom.0 as usize].gflags |= GEOM_POSR_BAD;
        }
        // from the bottom of the space heirarchy up, process all clean geoms turning them
        // into dirty geoms.
        let mut parent = self.geoms[geom.0 as usize].parent_space;
        while let Some(p) = parent {
            if self.geoms[geom.0 as usize].gflags & GEOM_DIRTY != 0 {
                break;
            }
            self.geoms[geom.0 as usize].gflags |= GEOM_DIRTY | GEOM_AABB_BAD;
            // dxSpace::dirty @ 0x140342e80: to the front of the parent's list
            self.list_remove(p, geom);
            self.list_push_front(p, geom);
            geom = p;
            parent = self.geoms[p.0 as usize].parent_space;
        }
        // all the remaining dirty geoms must have their AABB_BAD flags set, to ensure that
        // their AABBs get recomputed
        let mut g = Some(geom);
        while let Some(id) = g {
            self.geoms[id.0 as usize].gflags |= GEOM_DIRTY | GEOM_AABB_BAD;
            g = self.geoms[id.0 as usize].parent_space;
        }
    }

    /// `dGeomSetBody` @ 0x140344620 with a body (the game never takes a geom off its body).
    pub fn geom_set_body(&mut self, bodies: &mut [Body], g: GeomId, b: BodyId) {
        if self.geoms[g.0 as usize].body != Some(b) {
            self.geoms[g.0 as usize].offset = None;
            // bodyRemove
            if let Some(old) = self.geoms[g.0 as usize].body {
                let mut last: Option<GeomId> = None;
                let mut cur = bodies[old.0 as usize].first_geom;
                while let Some(c) = cur {
                    if c == g {
                        let next = self.geoms[c.0 as usize].body_next;
                        match last {
                            Some(l) => self.geoms[l.0 as usize].body_next = next,
                            None => bodies[old.0 as usize].first_geom = next,
                        }
                        break;
                    }
                    last = Some(c);
                    cur = self.geoms[c.0 as usize].body_next;
                }
            }
            // bodyAdd: the newest geom is the head of the body's list
            let geom = &mut self.geoms[g.0 as usize];
            geom.body = Some(b);
            geom.body_next = bodies[b.0 as usize].first_geom;
            bodies[b.0 as usize].first_geom = Some(g);
        }
        self.geom_moved(g);
    }

    /// `dGeomCreateOffset`.
    fn geom_create_offset(&mut self, g: GeomId) {
        let geom = &mut self.geoms[g.0 as usize];
        debug_assert!(geom.body.is_some(), "geom must be on a body");
        if geom.offset.is_some() {
            return;
        }
        geom.offset = Some(PosR { pos: [0.0; 3], r: IDENTITY });
        geom.gflags |= GEOM_POSR_BAD;
    }

    /// `dGeomSetOffsetPosition` @ 0x140344800.
    pub fn geom_set_offset_position(&mut self, g: GeomId, x: f32, y: f32, z: f32) {
        self.geom_create_offset(g);
        self.geoms[g.0 as usize].offset.as_mut().unwrap().pos = [x, y, z];
        self.geom_moved(g);
    }

    /// `dGeomSetOffsetRotation` @ 0x140344870.
    pub fn geom_set_offset_rotation(&mut self, g: GeomId, r: &Matrix3) {
        self.geom_create_offset(g);
        self.geoms[g.0 as usize].offset.as_mut().unwrap().r = *r;
        self.geom_moved(g);
    }

    /// Every geom of a body was moved (`dxStepBody`, `dBodySetPosition`, `dBodySetRotation`):
    /// `dGeomMoved` for each, from the head of the body's list.
    pub fn body_moved(&mut self, first_geom: Option<GeomId>) {
        let mut g = first_geom;
        while let Some(id) = g {
            self.geom_moved(id);
            g = self.geoms[id.0 as usize].body_next;
        }
    }

    /// `dxGeom::recomputePosr` with `computePosr` @ 0x1403434c0: the place of an offset geom
    /// from its body's.
    pub fn recompute_posr(&mut self, bodies: &[Body], g: GeomId) {
        let geom = &mut self.geoms[g.0 as usize];
        if geom.gflags & GEOM_POSR_BAD != 0 {
            let body = &bodies[geom.body.expect("an offset geom has a body").0 as usize];
            let offset = geom.offset.as_ref().expect("only an offset geom has a bad posr");
            let p = multiply0_331(&body.r, &[offset.pos[0], offset.pos[1], offset.pos[2], 0.0]);
            geom.posr.pos = [p[0] + body.pos[0], p[1] + body.pos[1], p[2] + body.pos[2]];
            multiply0_333(&mut geom.posr.r, &body.r, &offset.r);
            geom.gflags &= !GEOM_POSR_BAD;
        }
    }

    /// The geom's place in the world (`final_posr`), which has to be up to date
    /// ([`Collision::recompute_posr`]).
    pub fn final_posr(&self, bodies: &[Body], g: GeomId) -> PosR {
        let geom = &self.geoms[g.0 as usize];
        match (geom.body, &geom.offset) {
            (Some(b), None) => {
                let body = &bodies[b.0 as usize];
                PosR { pos: [body.pos[0], body.pos[1], body.pos[2]], r: body.r }
            }
            _ => geom.posr,
        }
    }

    /// `dxGeom::recomputeAABB`.
    pub fn recompute_aabb(&mut self, bodies: &[Body], g: GeomId) {
        if self.geoms[g.0 as usize].gflags & GEOM_AABB_BAD == 0 {
            return;
        }
        // our aabb functions assume final_posr is up to date
        self.recompute_posr(bodies, g);
        let posr = self.final_posr(bodies, g);
        let aabb = match &self.geoms[g.0 as usize].kind {
            GeomKind::Box { side } => box_aabb(side, &posr),
            GeomKind::TriMesh { data } => crate::collision::tri_mesh_aabb(data, &posr.pos, &posr.r),
            GeomKind::SimpleSpace { first, .. } => {
                // dxSpace::computeAABB @ 0x140342d40
                let first = *first;
                if first.is_some() {
                    let mut a = [f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY];
                    let mut m = first;
                    while let Some(id) = m {
                        self.recompute_aabb(bodies, id);
                        let gm = &self.geoms[id.0 as usize];
                        let ga = &gm.aabb;
                        for i in [0, 2, 4] {
                            if !(ga[i] >= a[i]) {
                                a[i] = ga[i];
                            }
                        }
                        for i in [1, 3, 5] {
                            if ga[i] > a[i] {
                                a[i] = ga[i];
                            }
                        }
                        m = gm.next;
                    }
                    a
                } else {
                    [0.0; 6]
                }
            }
        };
        let geom = &mut self.geoms[g.0 as usize];
        geom.aabb = aabb;
        geom.gflags &= !GEOM_AABB_BAD;
    }

    /// `dxSimpleSpace::cleanGeoms` @ 0x1403429d0: computes the boxes of all dirty geoms (they
    /// are the head of the list) and clears their dirty flags.
    pub fn clean_geoms(&mut self, bodies: &[Body], space: GeomId) {
        let GeomKind::SimpleSpace { first, .. } = self.geoms[space.0 as usize].kind else {
            return;
        };
        let mut g = first;
        while let Some(id) = g {
            if self.geoms[id.0 as usize].gflags & GEOM_DIRTY == 0 {
                break;
            }
            if self.geoms[id.0 as usize].is_space() {
                self.clean_geoms(bodies, id);
            }
            self.recompute_aabb(bodies, id);
            self.geoms[id.0 as usize].gflags &= !(GEOM_DIRTY | GEOM_AABB_BAD);
            g = self.geoms[id.0 as usize].next;
        }
    }
}

/// `dxBox::computeAABB` @ 0x140346e50.
fn box_aabb(side: &[f32; 3], posr: &PosR) -> [f32; 6] {
    let (r, pos) = (&posr.r, &posr.pos);
    let xrange = 0.5f32 * (((r[0] * side[0]).abs() + (r[1] * side[1]).abs()) + (r[2] * side[2]).abs());
    let yrange = 0.5f32 * (((r[4] * side[0]).abs() + (r[5] * side[1]).abs()) + (r[6] * side[2]).abs());
    let zrange = 0.5f32 * (((r[8] * side[0]).abs() + (r[9] * side[1]).abs()) + (r[10] * side[2]).abs());
    [pos[0] - xrange, pos[0] + xrange, pos[1] - yrange, pos[1] + yrange, pos[2] - zrange, pos[2] + zrange]
}

/// The broad phase over the moving store, the bodies it hangs on and, for
/// `dSpaceCollide2(dynamic, static)`, the static world.
pub struct BroadPhase<'a> {
    pub collision: &'a mut Collision,
    pub bodies: &'a [Body],
    pub statics: Option<&'a StaticWorld>,
}

impl BroadPhase<'_> {
    fn statics(&self) -> &StaticWorld {
        self.statics.expect("a static geom without a static world")
    }

    /// `dGeomIsSpace`.
    pub fn is_space(&self, g: GeomRef) -> bool {
        match g {
            GeomRef::Dyn(id) => self.collision.geoms[id.0 as usize].is_space(),
            GeomRef::StaticSpace | GeomRef::StaticSub(_) => true,
            GeomRef::StaticMesh(_) => false,
        }
    }

    /// `dxSpace::count`.
    fn count(&self, g: GeomRef) -> u32 {
        match g {
            GeomRef::Dyn(id) => match self.collision.geoms[id.0 as usize].kind {
                GeomKind::SimpleSpace { count, .. } => count,
                _ => 0,
            },
            GeomRef::StaticSpace => self.statics().spaces.len() as u32,
            GeomRef::StaticSub(i) => self.statics().spaces[i as usize].members.len() as u32,
            GeomRef::StaticMesh(_) => 0,
        }
    }

    /// The `i`-th member of a space, counted from the head of its list.
    fn member(&self, space: GeomRef, i: u32) -> GeomRef {
        match space {
            GeomRef::Dyn(id) => {
                let GeomKind::SimpleSpace { first, .. } = self.collision.geoms[id.0 as usize].kind else {
                    panic!("not a space");
                };
                let mut g = first.expect("a member");
                for _ in 0..i {
                    g = self.collision.geoms[g.0 as usize].next.expect("a member");
                }
                GeomRef::Dyn(g)
            }
            // the lists grow at their heads: the newest member comes first
            GeomRef::StaticSpace => GeomRef::StaticSub(self.statics().spaces.len() as u32 - 1 - i),
            GeomRef::StaticSub(s) => {
                let members = &self.statics().spaces[s as usize].members;
                GeomRef::StaticMesh(members[members.len() - 1 - i as usize] as u32)
            }
            GeomRef::StaticMesh(_) => panic!("not a space"),
        }
    }

    fn enabled(&self, g: GeomRef) -> bool {
        match g {
            GeomRef::Dyn(id) => self.collision.geoms[id.0 as usize].enabled(),
            _ => true,
        }
    }

    /// `dGeomGetCategoryBits`, `dGeomGetCollideBits`.
    pub fn bits(&self, g: GeomRef) -> (u32, u32) {
        match g {
            GeomRef::Dyn(id) => {
                let geom = &self.collision.geoms[id.0 as usize];
                (geom.category_bits, geom.collide_bits)
            }
            GeomRef::StaticSpace | GeomRef::StaticSub(_) => (!0, !0),
            GeomRef::StaticMesh(i) => {
                let mesh = &self.statics().meshes[i as usize];
                (mesh.category_bits, mesh.collide_bits)
            }
        }
    }

    /// `dGeomGetBody`.
    pub fn body(&self, g: GeomRef) -> Option<BodyId> {
        match g {
            GeomRef::Dyn(id) => self.collision.geoms[id.0 as usize].body,
            _ => None,
        }
    }

    /// `dGeomGetClass`.
    pub fn class(&self, g: GeomRef) -> u32 {
        match g {
            GeomRef::Dyn(id) => self.collision.geoms[id.0 as usize].class(),
            GeomRef::StaticSpace | GeomRef::StaticSub(_) => CLASS_SIMPLE_SPACE,
            GeomRef::StaticMesh(_) => CLASS_TRIMESH,
        }
    }

    fn aabb(&self, g: GeomRef) -> [f32; 6] {
        match g {
            GeomRef::Dyn(id) => self.collision.geoms[id.0 as usize].aabb,
            // (never asked for: the static space is only ever walked)
            GeomRef::StaticSpace => [f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY],
            GeomRef::StaticSub(i) => self.statics().spaces[i as usize].aabb,
            GeomRef::StaticMesh(i) => self.statics().meshes[i as usize].aabb,
        }
    }

    fn clean_geoms(&mut self, space: GeomRef) {
        if let GeomRef::Dyn(id) = space {
            self.collision.clean_geoms(self.bodies, id);
        }
        // the static world was cleaned when it was built and never gets dirty again
    }

    fn recompute_aabb(&mut self, g: GeomRef) {
        if let GeomRef::Dyn(id) = g {
            self.collision.recompute_aabb(self.bodies, id);
        }
    }

    /// `collideAABBs` @ 0x140342c60. `swap`: the callback is `swap_callback`, which hands the
    /// pair over the other way round.
    fn collide_aabbs(&mut self, g1: GeomRef, g2: GeomRef, swap: bool, callback: &mut dyn NearCallback) {
        // no contacts if both geoms on the same body, and the body is not 0
        let b1 = self.body(g1);
        if b1.is_some() && b1 == self.body(g2) {
            return;
        }
        // test if the category and collide bitfields match
        let (cat1, col1) = self.bits(g1);
        let (cat2, col2) = self.bits(g2);
        if cat1 & col2 == 0 && cat2 & col1 == 0 {
            return;
        }
        // if the bounding boxes are disjoint then don't do anything
        if !boxes_meet(&self.aabb(g1), &self.aabb(g2)) {
            return;
        }
        // (AABBTest: a box, a mesh and a space all answer "may intersect")
        // the objects might actually intersect - call the space callback function
        if swap {
            callback.near(self, g2, g1);
        } else {
            callback.near(self, g1, g2);
        }
    }

    /// `dSpaceCollide` @ 0x140343090 -> `dxSimpleSpace::collide` @ 0x140342b10: every pair
    /// of members of one space.
    pub fn space_collide(&mut self, space: GeomRef, callback: &mut dyn NearCallback) {
        self.clean_geoms(space);
        // intersect all bounding boxes
        let count = self.count(space);
        for i in 0..count {
            let g1 = self.member(space, i);
            if !self.enabled(g1) {
                continue;
            }
            for k in i + 1..count {
                let g2 = self.member(space, k);
                if self.enabled(g2) {
                    self.collide_aabbs(g1, g2, false, callback);
                }
            }
        }
    }

    /// `dxSimpleSpace::collide2` @ 0x140342a60: every member of `space` against `geom`.
    fn collide2(&mut self, space: GeomRef, geom: GeomRef, swap: bool, callback: &mut dyn NearCallback) {
        self.clean_geoms(space);
        self.recompute_aabb(geom);
        // intersect bounding boxes
        let count = self.count(space);
        for i in 0..count {
            let g = self.member(space, i);
            if self.enabled(g) {
                self.collide_aabbs(g, geom, swap, callback);
            }
        }
    }

    /// `dSpaceCollide2` @ 0x1403430a0.
    pub fn space_collide2(&mut self, g1: GeomRef, g2: GeomRef, callback: &mut dyn NearCallback) {
        // (every space of the game has sublevel 0, so the sublevel rule changes nothing)
        let s1 = self.is_space(g1);
        let s2 = self.is_space(g2);
        if s1 {
            if s2 {
                // g1 and g2 are spaces.
                if g1 == g2 {
                    // collide a space with itself --> interior collision
                    self.space_collide(g1, callback);
                } else {
                    // iterate through the space that has the fewest geoms, calling
                    // collide2 in the other space for each one.
                    let (c1, c2) = (self.count(g1), self.count(g2));
                    if c1 < c2 {
                        for i in 0..c1 {
                            let g = self.member(g1, i);
                            self.collide2(g2, g, true, callback);
                        }
                    } else {
                        for i in 0..c2 {
                            let g = self.member(g2, i);
                            self.collide2(g1, g, false, callback);
                        }
                    }
                }
            } else {
                // g1 is a space, g2 is a geom
                self.collide2(g1, g2, false, callback);
            }
        } else if s2 {
            // g1 is a geom, g2 is a space
            self.collide2(g2, g1, true, callback);
        } else {
            // g1 and g2 are geoms: make sure they have valid AABBs
            self.recompute_aabb(g1);
            self.recompute_aabb(g2);
            self.collide_aabbs(g1, g2, false, callback);
        }
    }
}

/// `NUMC_MASK`: the low 16 bits of `dCollide`'s flags are the room in the contact array.
pub const NUMC_MASK: u32 = 0xffff;

impl World {
    /// `dGeomSetBody` @ 0x140344620.
    pub fn geom_set_body(&mut self, g: GeomId, b: BodyId) {
        self.collision.geom_set_body(&mut self.bodies, g, b);
    }

    /// `dGeomSetRotation` @ 0x1403448c0 for a geom that sits on a body with an offset: the
    /// BODY is moved so that body + offset gives the rotation `r` (and the geom keeps its
    /// place).
    pub fn geom_set_rotation(&mut self, g: GeomId, r: &Matrix3) {
        let geom = &self.collision.geoms[g.0 as usize];
        let Some(body) = geom.body else {
            self.collision.geoms[g.0 as usize].posr.r = *r;
            self.collision.geom_moved(g);
            return;
        };
        let Some(offset) = geom.offset else {
            // this will call dGeomMoved (g), so we don't have to
            self.body_set_rotation(body, r);
            return;
        };
        self.collision.recompute_posr(&self.bodies, g);
        let final_pos = self.collision.geoms[g.0 as usize].posr.pos;
        // getBodyPosr: move body such that body+offset = rotation
        let o = &offset.r;
        // matrixInvert: the transpose
        let inv_offset: Matrix3 = [o[0], o[4], o[8], 0.0, o[1], o[5], o[9], 0.0, o[2], o[6], o[10], 0.0];
        let mut body_r = [0.0f32; 12];
        multiply0_333(&mut body_r, r, &inv_offset);
        let world_offset = multiply0_331(&body_r, &[offset.pos[0], offset.pos[1], offset.pos[2], 0.0]);
        let body_pos = [final_pos[0] - world_offset[0], final_pos[1] - world_offset[1], final_pos[2] - world_offset[2]];
        self.body_set_rotation(body, &body_r);
        self.body_set_position(body, body_pos[0], body_pos[1], body_pos[2]);
    }

    /// `dSpaceCollide` @ 0x140343090.
    pub fn space_collide(&mut self, space: GeomRef, statics: Option<&StaticWorld>, callback: &mut dyn NearCallback) {
        let mut broad = BroadPhase { collision: &mut self.collision, bodies: &self.bodies, statics };
        broad.space_collide(space, callback);
    }

    /// `dSpaceCollide2` @ 0x1403430a0.
    pub fn space_collide2(&mut self, o1: GeomRef, o2: GeomRef, statics: Option<&StaticWorld>, callback: &mut dyn NearCallback) {
        let mut broad = BroadPhase { collision: &mut self.collision, bodies: &self.bodies, statics };
        broad.space_collide2(o1, o2, callback);
    }

    /// The class, bits and body of a geom, wherever it lives.
    pub fn geom_info(&self, g: GeomRef, statics: Option<&StaticWorld>) -> GeomInfo {
        match g {
            GeomRef::Dyn(id) => {
                let geom = &self.collision.geoms[id.0 as usize];
                GeomInfo { class: geom.class(), category_bits: geom.category_bits, collide_bits: geom.collide_bits, body: geom.body, data: geom.data }
            }
            GeomRef::StaticSpace | GeomRef::StaticSub(_) => {
                GeomInfo { class: CLASS_SIMPLE_SPACE, category_bits: !0, collide_bits: !0, body: None, data: 0 }
            }
            GeomRef::StaticMesh(i) => {
                let mesh = &statics.expect("a static mesh without a static world").meshes[i as usize];
                GeomInfo { class: CLASS_TRIMESH, category_bits: mesh.category_bits, collide_bits: mesh.collide_bits, body: None, data: 0 }
            }
        }
    }

    /// `dCollide` @ 0x140344120: the contacts between two geoms, appended to `out`. The low
    /// 16 bits of `flags` are the most contacts wanted. Returns how many were added.
    pub fn collide(&mut self, o1: GeomRef, o2: GeomRef, statics: Option<&StaticWorld>, flags: u32, out: &mut Vec<ContactGeom>) -> usize {
        // Extra precaution for zero contact count in parameters
        if flags & NUMC_MASK == 0 {
            return 0;
        }
        // no contacts if both geoms are the same
        if o1 == o2 {
            return 0;
        }
        // no contacts if both geoms on the same body, and the body is not 0
        let i1 = self.geom_info(o1, statics);
        let i2 = self.geom_info(o2, statics);
        if i1.body.is_some() && i1.body == i2.body {
            return 0;
        }
        for g in [o1, o2] {
            if let GeomRef::Dyn(id) = g {
                self.collision.recompute_posr(&self.bodies, id);
            }
        }
        let start = out.len();
        // the colliders table: (trimesh, box) is dCollideBTL, (box, trimesh) the same with
        // the pair reversed, (trimesh, trimesh) is dCollideTTL; the masks of the game let
        // no other pair of classes through
        match (i1.class, i2.class) {
            (CLASS_TRIMESH, CLASS_BOX) => {
                let (mesh, box_) = (self.mesh_pose(o1, statics), self.box_pose(o2));
                crate::collide_btl::collide_btl(&mesh, &box_, flags, o1, o2, out);
            }
            (CLASS_BOX, CLASS_TRIMESH) => {
                let (mesh, box_) = (self.mesh_pose(o2, statics), self.box_pose(o1));
                crate::collide_btl::collide_btl(&mesh, &box_, flags, o2, o1, out);
                // the reversed call: the signs of the normals, the geoms and the sides change places
                for c in out[start..].iter_mut() {
                    c.normal = [-c.normal[0], -c.normal[1], -c.normal[2]];
                    std::mem::swap(&mut c.g1, &mut c.g2);
                    std::mem::swap(&mut c.side1, &mut c.side2);
                }
            }
            (CLASS_TRIMESH, CLASS_TRIMESH) => {
                let (m1, m2) = (self.mesh_pose(o1, statics), self.mesh_pose(o2, statics));
                crate::collide_ttl::collide_ttl(&m1, &m2, flags, o1, o2, out);
            }
            _ => {}
        }
        out.len() - start
    }

    fn mesh_pose<'a>(&'a self, g: GeomRef, statics: Option<&'a StaticWorld>) -> MeshPose<'a> {
        match g {
            GeomRef::Dyn(id) => {
                let posr = self.collision.final_posr(&self.bodies, id);
                let GeomKind::TriMesh { data } = &self.collision.geoms[id.0 as usize].kind else {
                    panic!("not a mesh");
                };
                MeshPose { data, pos: posr.pos, r: posr.r }
            }
            GeomRef::StaticMesh(i) => {
                let mesh = &statics.expect("a static mesh without a static world").meshes[i as usize];
                MeshPose { data: &mesh.data, pos: mesh.pos, r: mesh.r }
            }
            _ => panic!("not a mesh"),
        }
    }

    fn box_pose(&self, g: GeomRef) -> BoxPose {
        let GeomRef::Dyn(id) = g else { panic!("not a box") };
        let posr = self.collision.final_posr(&self.bodies, id);
        let GeomKind::Box { side } = &self.collision.geoms[id.0 as usize].kind else {
            panic!("not a box");
        };
        BoxPose { side: *side, pos: posr.pos, r: posr.r }
    }
}

/// What the callbacks of the game ask a geom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeomInfo {
    pub class: u32,
    pub category_bits: u32,
    pub collide_bits: u32,
    pub body: Option<BodyId>,
    pub data: u64,
}

/// A triangle mesh at its place in the world.
#[derive(Clone, Copy, Debug)]
pub struct MeshPose<'a> {
    pub data: &'a TriMeshData,
    pub pos: [f32; 3],
    pub r: Matrix3,
}

/// A box at its place in the world.
#[derive(Clone, Copy, Debug)]
pub struct BoxPose {
    pub side: [f32; 3],
    pub pos: [f32; 3],
    pub r: Matrix3,
}
