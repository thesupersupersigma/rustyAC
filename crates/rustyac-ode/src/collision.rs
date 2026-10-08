// SPDX-License-Identifier: BSD-3-Clause

//! Stage 3, part 1: static triangle meshes in ODE's simple spaces, and a ray against them.
//!
//! This is the part of ODE's collision the game's tyres use every step: one ray per wheel
//! against the track ([`StaticWorld::ray_cast`]). What is here follows the compiled code of
//! `acs.exe` 1.16.4 instruction by instruction where a float is involved:
//!
//! * `dxTriMeshData::Build` @ 0x14034a780 (the mesh's box, the OPCODE tree of [`crate::opcode`]),
//! * `dxTriMesh::computeAABB` @ 0x14034ae10, `dxSpace::computeAABB` @ 0x140342d40,
//!   `dxRay::computeAABB` @ 0x140344a60, `collideAABBs` @ 0x140342c60,
//! * `dGeomRaySet` @ 0x140345e50, `dSpaceCollide2` @ 0x1403430a0 with
//!   `dxSimpleSpace::collide2` @ 0x140342a60 for a ray against a space of spaces,
//! * `dCollide` @ 0x140344120 for (ray, mesh), `dCollideRTL` @ 0x14038aa90,
//! * the game's own accumulator around them (`PhysicsCore::rayCast` @ 0x1402cd070,
//!   `rayNearCallback` @ 0x1402cd210): the nearest contact over all meshes.
//!
//! Inside one mesh the ray does **not** find the nearest triangle: the game's rays run in
//! OPCODE's first-contact mode with back-face culling, so the answer is the first front-facing
//! triangle the tree walk meets. Between meshes the nearest answer wins, and of two equally
//! near ones the mesh tested first, which is the one created last (a space's list grows at
//! its head).
//!
//! Not here yet: the car body's boxes and collision mesh against the track (stage 3, part 2
//! with stage 2's contacts). Three things the game does that this port does not, none of
//! which a track or a tyre reaches: a ray whose length is exactly `f32::MAX` (the game then
//! walks the tree with OPCODE's `_RayStab`, an unbounded ray, instead of the segment walk);
//! a mesh in sub-space 0, which in the game is a direct member of the static space and here
//! gets a sub-space of its own (a track's physical meshes never have id 0); and meshes added
//! after the first ray (ODE would leave their sub-space's box stale; here
//! [`StaticWorld::clean`] has to be called again and recomputes every box). A triangle that
//! names a vertex the mesh does not have makes the game read past its array; here the caller
//! has to refuse such a mesh ([`TriMeshData::indices_in_range`]).

use crate::odemath::safe_normalize3;
use crate::opcode::{Child, MeshInterface, Model};
use crate::Matrix3;

/// `dxTriMeshData` (0x88 bytes): a mesh, its tree and its box.
#[derive(Clone, Debug, PartialEq)]
pub struct TriMeshData {
    /// `Mesh`
    pub mesh: MeshInterface,
    /// `BVTree`
    pub model: Model,
    /// `AABBCenter`
    pub aabb_center: [f32; 3],
    /// `AABBExtents`
    pub aabb_extents: [f32; 3],
}

impl TriMeshData {
    /// Does every triangle name vertices the mesh has? The game does not check (it reads past
    /// its copy of the vertices); a port cannot follow it there.
    pub fn indices_in_range(vertex_count: usize, indices: &[u16]) -> bool {
        indices.iter().all(|&i| (i as usize) < vertex_count)
    }

    /// `dGeomTriMeshDataBuildSingle` @ 0x14034b170 -> `dxTriMeshData::Build` @ 0x14034a780.
    pub fn build(vertices: Vec<[f32; 3]>, indices: Vec<u16>) -> TriMeshData {
        let mesh = MeshInterface::new(vertices, indices);
        let model = Model::build(&mesh);
        // the box of ALL vertices, used or not; not the tree's formula
        let mut max = [f32::NEG_INFINITY; 3];
        let mut min = [f32::INFINITY; 3];
        for v in &mesh.vertices {
            for k in 0..3 {
                if v[k] > max[k] {
                    max[k] = v[k];
                }
            }
            for k in 0..3 {
                // comiss v, min ; jae skip: a NaN replaces the minimum
                if !(v[k] >= min[k]) {
                    min[k] = v[k];
                }
            }
        }
        let mut aabb_center = [0.0f32; 3];
        let mut aabb_extents = [0.0f32; 3];
        for k in 0..3 {
            aabb_center[k] = (min[k] + max[k]) * 0.5;
            aabb_extents[k] = max[k] - aabb_center[k];
        }
        TriMeshData { mesh, model, aabb_center, aabb_extents }
    }
}

/// A `dxTriMesh` geom of the static world: never moved, no body.
#[derive(Clone, Debug, PartialEq)]
pub struct TriMeshGeom {
    pub data: TriMeshData,
    /// `final_posr->pos`: zero.
    pub pos: [f32; 3],
    /// `final_posr->R`: the identity.
    pub r: Matrix3,
    /// `aabb`: min x, max x, min y, max y, min z, max z
    pub aabb: [f32; 6],
    pub category_bits: u32,
    pub collide_bits: u32,
    /// The sub-space's id (the number a track mesh's name starts with; walls + 10000).
    pub space_id: u32,
    /// ODE's `GEOM_DIRTY | GEOM_AABB_BAD`: the box was not computed since the mesh was made.
    pub(crate) dirty: bool,
}

impl TriMeshGeom {
    /// `dxTriMesh::computeAABB` @ 0x14034ae10
    fn compute_aabb(&mut self) {
        self.aabb = tri_mesh_aabb(&self.data, &self.pos, &self.r);
    }
}

/// `dxTriMesh::computeAABB` @ 0x14034ae10: the box of a mesh at `pos`, turned by `r`.
pub(crate) fn tri_mesh_aabb(data: &TriMeshData, p: &[f32; 3], r: &Matrix3) -> [f32; 6] {
    let (c, e) = (&data.aabb_center, &data.aabb_extents);
    let xc = (c[0] * r[0] + r[1] * c[1]) + r[2] * c[2];
    let yc = (c[1] * r[5] + c[0] * r[4]) + c[2] * r[6];
    let zc = (c[1] * r[9] + c[0] * r[8]) + c[2] * r[10];
    let xr = ((e[0] * r[0]).abs() + (e[1] * r[1]).abs()) + (e[2] * r[2]).abs();
    let yr = ((e[1] * r[5]).abs() + (e[0] * r[4]).abs()) + (e[2] * r[6]).abs();
    let zr = ((e[1] * r[9]).abs() + (e[0] * r[8]).abs()) + (e[2] * r[10]).abs();
    [(xc + p[0]) - xr, (xc + p[0]) + xr, (yc + p[1]) - yr, (yc + p[1]) + yr, (zc + p[2]) - zr, (zc + p[2]) + zr]
}

/// One of the numbered `dxSimpleSpace`s inside the static space.
#[derive(Clone, Debug, PartialEq)]
pub struct SubSpace {
    pub id: u32,
    /// Its meshes (indices into [`StaticWorld::meshes`]) in creation order; ODE walks the
    /// list from the newest.
    pub members: Vec<usize>,
    /// The box around all of them.
    pub aabb: [f32; 6],
    /// ODE's `GEOM_DIRTY | GEOM_AABB_BAD` of the space: a mesh was added since the box was
    /// last computed.
    pub(crate) dirty: bool,
}

/// A direct member of the static space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootMember {
    /// A numbered sub-space (index into [`StaticWorld::spaces`]).
    Space(usize),
    /// A mesh made with sub-space id 0: `PhysicsCore::getStaticSubSpace(0)` is the static
    /// space itself (index into [`StaticWorld::meshes`]).
    Mesh(usize),
}

/// What a ray found (`dContactGeom` after `dCollide`, as far as the game reads it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayContact {
    /// `pos`: origin + direction x depth
    pub pos: [f32; 3],
    /// `normal`: the triangle's, against the ray
    pub normal: [f32; 3],
    /// `depth`: the distance along the ray
    pub depth: f32,
    /// `g2`: the mesh (index into [`StaticWorld::meshes`], the creation order)
    pub mesh: usize,
    /// `side2`: the triangle
    pub triangle: u32,
}

/// `PhysicsCore::spaceStatic` with its numbered sub-spaces and their meshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticWorld {
    /// Every mesh, in creation order.
    pub meshes: Vec<TriMeshGeom>,
    /// The sub-spaces in the order they were first used.
    pub spaces: Vec<SubSpace>,
    /// The members of the static space itself: the sub-spaces, and between them the meshes
    /// of sub-space id 0. The LAST entry is the head of ODE's list (a new member goes to the
    /// head; so does a clean sub-space that gets a new mesh): rays ask from the end.
    pub root: Vec<RootMember>,
    /// Meshes were added since [`StaticWorld::clean`] last ran.
    dirty: bool,
}

/// `collideAABBs` @ 0x140342c60, the six comparisons: does a member's box `b1` meet the ray's `b2`?
#[inline]
pub(crate) fn boxes_meet(b1: &[f32; 6], b2: &[f32; 6]) -> bool {
    // comiss ; jb rejects on "less or unordered", comiss ; ja on "greater"
    b2[1] >= b1[0] && !(b2[0] > b1[1]) && b2[3] >= b1[2] && !(b2[2] > b1[3]) && b2[5] >= b1[4] && !(b2[4] > b1[5])
}

/// `Opcode::RayCollider` for one query: the ray in the mesh's space.
struct RayCollider<'a> {
    mesh: &'a MeshInterface,
    /// `mOrigin`, `mDir`
    origin: [f32; 3],
    dir: [f32; 3],
    /// `mData`, `mData2`, `mFDir`
    data: [f32; 3],
    data2: [f32; 3],
    fdir: [f32; 3],
    /// `mMaxDist`
    max_dist: f32,
}

/// `CollisionFace`: a stabbed triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Face {
    face_id: u32,
    distance: f32,
}

impl RayCollider<'_> {
    /// The ray-triangle test with culling, as inlined in `_SegmentStab` @ 0x14036aef0 and
    /// `InitQuery` @ 0x1403670b0, with the contact handler behind it.
    fn triangle(&self, prim: u32) -> Option<Face> {
        self.triangle_test(prim, true)
    }

    /// `bounded`: the hit has to be nearer than the segment's end. The unbounded walk
    /// (`_RayStab` @ 0x1403685f0) has the same test without that last comparison.
    fn triangle_test(&self, prim: u32, bounded: bool) -> Option<Face> {
        let [v0, v1, v2] = self.mesh.triangle(prim);
        let d = &self.dir;
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let p = [d[1] * e2[2] - d[2] * e2[1], e2[0] * d[2] - d[0] * e2[2], d[0] * e2[1] - e2[0] * d[1]];
        let det = (e1[0] * p[0] + e1[1] * p[1]) + e1[2] * p[2];
        let e1sq = (e1[0] * e1[0] + e1[1] * e1[1]) + e1[2] * e1[2];
        let e2sq = (e2[0] * e2[0] + e2[1] * e2[1]) + e2[2] * e2[2];
        // comiss e1sq, e2sq ; jb keeps e1sq, also for a NaN
        let sqrmin = if !(e1sq >= e2sq) { e1sq } else { e2sq };
        let eps = sqrmin * f32::from_bits(0x3586_37bd);
        // the back of a triangle, or one seen edge on (or a NaN)
        if !(det > eps) {
            return None;
        }
        let t = [self.origin[0] - v0[0], self.origin[1] - v0[1], self.origin[2] - v0[2]];
        let u = (t[0] * p[0] + t[1] * p[1]) + t[2] * p[2];
        // the sign bit itself is tested: -0 is outside
        if u.to_bits() & 0x8000_0000 != 0 || u.to_bits() > det.to_bits() {
            return None;
        }
        let q = [t[1] * e1[2] - t[2] * e1[1], e1[0] * t[2] - t[0] * e1[2], t[0] * e1[1] - e1[0] * t[1]];
        let v = (q[1] * d[1] + q[0] * d[0]) + q[2] * d[2];
        if v.to_bits() & 0x8000_0000 != 0 || v + u > det {
            return None;
        }
        let dist = (q[0] * e2[0] + q[1] * e2[1]) + q[2] * e2[2];
        if dist.to_bits() & 0x8000_0000 != 0 {
            return None;
        }
        let inv = 1.0 / det;
        let distance = dist * inv;
        // the segment's end, compared as integers
        if bounded && distance.to_bits() >= self.max_dist.to_bits() {
            return None;
        }
        Some(Face { face_id: prim, distance })
    }

    /// `RayCollider::_RayStab(const AABBNoLeafNode*)` @ 0x1403685f0 in first-contact mode:
    /// the walk OPCODE takes for a ray whose length is exactly `f32::MAX` (it compares the
    /// bits). A box is left out when the origin is outside one of its slabs and the ray does
    /// not point back at it, or when one of the three cross-axis tests separates them; a
    /// triangle counts at any distance. `fdir` is `|dir|` here.
    fn ray_stab(&self, model: &Model) -> Option<Face> {
        let mut pending: Vec<Child> = Vec::with_capacity(64);
        pending.push(Child::Node(0));
        while let Some(item) = pending.pop() {
            match item {
                Child::Triangle(prim) => {
                    if let Some(face) = self.triangle_test(prim, false) {
                        return Some(face);
                    }
                }
                Child::Node(id) => {
                    let node = &model.nodes[id as usize];
                    let (c, e) = (&node.center, &node.extents);
                    let (dir, fdir) = (&self.dir, &self.fdir);
                    // comiss |D|, e ; jbe next ... comiss D*dir, 0 ; jae out: a NaN never rejects
                    let dx = self.origin[0] - c[0];
                    if dx.abs() > e[0] && dx * dir[0] >= 0.0 {
                        continue;
                    }
                    let dy = self.origin[1] - c[1];
                    if dy.abs() > e[1] && dy * dir[1] >= 0.0 {
                        continue;
                    }
                    let dz = self.origin[2] - c[2];
                    if dz.abs() > e[2] && dz * dir[2] >= 0.0 {
                        continue;
                    }
                    let f = dir[1] * dz - dir[2] * dy;
                    if f.abs() > fdir[1] * e[2] + fdir[2] * e[1] {
                        continue;
                    }
                    let f = dir[2] * dx - dir[0] * dz;
                    if f.abs() > fdir[0] * e[2] + fdir[2] * e[0] {
                        continue;
                    }
                    let f = dir[0] * dy - dir[1] * dx;
                    if f.abs() > fdir[0] * e[1] + fdir[1] * e[0] {
                        continue;
                    }
                    pending.push(node.neg);
                    pending.push(node.pos);
                }
            }
        }
        None
    }

    /// `RayCollider::_SegmentStab(const AABBNoLeafNode*)` @ 0x14036aef0 in first-contact mode:
    /// depth first, the positive side first, over at the first triangle that is hit.
    fn segment_stab(&self, model: &Model) -> Option<Face> {
        let mut pending: Vec<Child> = Vec::with_capacity(64);
        pending.push(Child::Node(0));
        while let Some(item) = pending.pop() {
            match item {
                Child::Triangle(prim) => {
                    if let Some(face) = self.triangle(prim) {
                        return Some(face);
                    }
                }
                Child::Node(id) => {
                    let node = &model.nodes[id as usize];
                    let (c, e) = (&node.center, &node.extents);
                    let (data, fdir) = (&self.data, &self.fdir);
                    // segment against box; only "strictly greater" rejects, a NaN never does
                    let dx = self.data2[0] - c[0];
                    if dx.abs() > e[0] + fdir[0] {
                        continue;
                    }
                    let dy = self.data2[1] - c[1];
                    if dy.abs() > fdir[1] + e[1] {
                        continue;
                    }
                    let dz = self.data2[2] - c[2];
                    if dz.abs() > fdir[2] + e[2] {
                        continue;
                    }
                    let f = data[1] * dz - data[2] * dy;
                    if f.abs() > e[2] * fdir[1] + fdir[2] * e[1] {
                        continue;
                    }
                    let f = data[2] * dx - data[0] * dz;
                    if f.abs() > fdir[2] * e[0] + e[2] * fdir[0] {
                        continue;
                    }
                    let f = data[0] * dy - data[1] * dx;
                    if f.abs() > e[1] * fdir[0] + fdir[1] * e[0] {
                        continue;
                    }
                    pending.push(node.neg);
                    pending.push(node.pos);
                }
            }
        }
        None
    }
}

impl StaticWorld {
    pub fn new() -> StaticWorld {
        StaticWorld::default()
    }

    /// `CollisionMeshODE::CollisionMeshODE` @ 0x1402cebb0: a mesh in the sub-space `space_id`
    /// (`PhysicsCore::getStaticSubSpace` @ 0x1402ccac0 makes the space when the id is new).
    /// Returns the mesh's index. [`StaticWorld::clean`] has to run before the next ray.
    pub fn create_tri_mesh(&mut self, vertices: Vec<[f32; 3]>, indices: Vec<u16>, category_bits: u32, collide_bits: u32, space_id: u32) -> usize {
        let index = self.meshes.len();
        self.meshes.push(TriMeshGeom {
            data: TriMeshData::build(vertices, indices),
            pos: [0.0; 3],
            // dRSetIdentity
            r: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            aabb: [0.0; 6],
            category_bits,
            collide_bits,
            space_id,
            dirty: true,
        });
        if space_id == 0 {
            // getStaticSubSpace(0): the static space itself
            self.root.push(RootMember::Mesh(index));
        } else {
            match self.spaces.iter_mut().find(|s| s.id == space_id) {
                Some(space) => {
                    space.members.push(index);
                    // dxSpace::add @ 0x140342980 ends in dGeomMoved(the SPACE) @
                    // 0x140342f80: a sub-space that was clean (a ray has been cast since
                    // its last mesh) becomes dirty and moves to the head of the static
                    // space's list; one that is still dirty stays where it is
                    if !space.dirty {
                        space.dirty = true;
                        let space = self.spaces.iter().position(|s| s.id == space_id).expect("the space");
                        let at = self.root.iter().position(|m| *m == RootMember::Space(space)).expect("a member of the static space");
                        let member = self.root.remove(at);
                        self.root.push(member);
                    }
                }
                None => {
                    self.root.push(RootMember::Space(self.spaces.len()));
                    self.spaces.push(SubSpace { id: space_id, members: vec![index], aabb: [0.0; 6], dirty: true });
                }
            }
        }
        self.dirty = true;
        index
    }

    /// How many meshes are direct members of the static space (sub-space id 0).
    pub fn direct_meshes(&self) -> usize {
        self.root.iter().filter(|m| matches!(m, RootMember::Mesh(_))).count()
    }

    /// `dxSimpleSpace::cleanGeoms` @ 0x1403429d0 of one sub-space: the boxes of its dirty
    /// meshes (they are at the head of its list, the newest first; the walk ends at the
    /// first clean one).
    fn clean_members(&mut self, space: usize) {
        for k in (0..self.spaces[space].members.len()).rev() {
            let member = self.spaces[space].members[k];
            if !self.meshes[member].dirty {
                break;
            }
            self.meshes[member].compute_aabb();
            self.meshes[member].dirty = false;
        }
    }

    /// What the next ray does before it looks at anything: `dxSimpleSpace::cleanGeoms` @
    /// 0x1403429d0 of the static space. It walks the members from the head of the list and
    /// stops at the first one that is not dirty; a dirty sub-space on the way gets the boxes
    /// of its new meshes and then its own (`dxSpace::computeAABB` @ 0x140342d40, over all
    /// members). Everything dirty is at the head (new members are put there, and a clean
    /// sub-space that gets a mesh is moved there), so the walk reaches all of it.
    ///
    /// The game makes every mesh before its first ray, so there this runs once. A mesh
    /// added after a ray is found as well; what changes then is the ORDER in which the
    /// sub-spaces are asked (the one that got the mesh is asked first from then on), which
    /// decides between two meshes hit at the same distance.
    pub fn clean(&mut self) {
        for k in (0..self.root.len()).rev() {
            match self.root[k] {
                RootMember::Mesh(mesh) => {
                    if !self.meshes[mesh].dirty {
                        break;
                    }
                    self.meshes[mesh].compute_aabb();
                    self.meshes[mesh].dirty = false;
                }
                RootMember::Space(space) => {
                    if !self.spaces[space].dirty {
                        break;
                    }
                    self.clean_members(space);
                    let mut a = [f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY];
                    // the list's order: the newest member first
                    for &member in self.spaces[space].members.iter().rev() {
                        let g = &self.meshes[member].aabb;
                        for i in [0, 2, 4] {
                            if !(g[i] >= a[i]) {
                                a[i] = g[i];
                            }
                        }
                        for i in [1, 3, 5] {
                            if g[i] > a[i] {
                                a[i] = g[i];
                            }
                        }
                    }
                    self.spaces[space].aabb = a;
                    self.spaces[space].dirty = false;
                }
            }
        }
        debug_assert!(self.spaces.iter().all(|s| !s.dirty) && self.meshes.iter().all(|m| !m.dirty), "a dirty geom behind a clean one");
        self.dirty = false;
    }

    /// `dCollide(ray, mesh)` -> `dCollideRTL` @ 0x14038aa90 for one mesh: `n` is the ray's
    /// normalised direction.
    fn collide_ray_mesh(&self, index: usize, org: &[f32; 3], n: &[f32; 3], length: f32) -> Option<RayContact> {
        let geom = &self.meshes[index];
        let model = &geom.data.model;
        if !model.built {
            // RayCollider::Collide: no mesh interface, nothing to hit
            return None;
        }
        let (r, pos) = (&geom.r, &geom.pos);
        // the matrix handed to OPCODE: the rotation transposed into rows, the position below
        let w = [[r[0], r[4], r[8]], [r[1], r[5], r[9]], [r[2], r[6], r[10]], [pos[0], pos[1], pos[2]]];
        // RayCollider::InitQuery @ 0x1403670b0
        let dir = [
            (n[1] * w[0][1] + n[0] * w[0][0]) + n[2] * w[0][2],
            (n[1] * w[1][1] + n[0] * w[1][0]) + n[2] * w[1][2],
            (n[1] * w[2][1] + n[0] * w[2][0]) + n[2] * w[2][2],
        ];
        // InvertPRMatrix @ 0x1403954a0
        let inv = [
            [w[0][0], w[1][0], w[2][0]],
            [w[0][1], w[1][1], w[2][1]],
            [w[0][2], w[1][2], w[2][2]],
            [
                -((w[3][0] * w[0][0] + w[0][1] * w[3][1]) + w[3][2] * w[0][2]),
                -((w[1][0] * w[3][0] + w[3][1] * w[1][1]) + w[3][2] * w[1][2]),
                -((w[2][0] * w[3][0] + w[3][1] * w[2][1]) + w[3][2] * w[2][2]),
            ],
        ];
        let origin = [
            ((org[1] * inv[1][0] + org[0] * inv[0][0]) + org[2] * inv[2][0]) + inv[3][0],
            ((org[0] * inv[0][1] + org[1] * inv[1][1]) + org[2] * inv[2][1]) + inv[3][1],
            ((org[0] * inv[0][2] + org[1] * inv[1][2]) + org[2] * inv[2][2]) + inv[3][2],
        ];
        let mut collider =
            RayCollider { mesh: &geom.data.mesh, origin, dir, data: [0.0; 3], data2: [0.0; 3], fdir: [0.0; 3], max_dist: length };
        let face = if model.single_node {
            // (answered inside InitQuery, with the segment's distance test in either mode)
            collider.triangle(0)
        } else if length.to_bits() == 0x7f7f_ffff {
            // RayCollider::Collide @ 0x140366ed0: a length of exactly f32::MAX is a ray
            // without an end
            for k in 0..3 {
                collider.fdir[k] = dir[k].abs();
            }
            collider.ray_stab(model)
        } else {
            for k in 0..3 {
                collider.data[k] = (0.5 * dir[k]) * length;
                collider.data2[k] = collider.data[k] + origin[k];
                collider.fdir[k] = collider.data[k].abs();
            }
            collider.segment_stab(model)
        }?;

        // FetchTriangle @ 0x140346770: the corners in the world
        let tri = geom.data.mesh.triangle(face.face_id);
        let mut dv = [[0.0f32; 3]; 3];
        for (out, v) in dv.iter_mut().zip(tri) {
            let tx = (v[1] * r[1] + v[0] * r[0]) + v[2] * r[2];
            let ty = (v[1] * r[5] + v[0] * r[4]) + v[2] * r[6];
            let tz = (v[1] * r[9] + v[0] * r[8]) + v[2] * r[10];
            *out = [pos[0] + tx, ty + pos[1], tz + pos[2]];
        }
        let a = [dv[2][0] - dv[0][0], dv[2][1] - dv[0][1], dv[2][2] - dv[0][2]];
        let b = [dv[1][0] - dv[0][0], dv[1][1] - dv[0][1], dv[1][2] - dv[0][2]];
        let mut normal = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        if !safe_normalize3(&mut normal) {
            // a triangle without an area: OPCODE reported it, ODE drops it
            return None;
        }
        let t = face.distance;
        Some(RayContact {
            pos: [n[0] * t + org[0], n[1] * t + org[1], n[2] * t + org[2]],
            // dCollide swaps the pair back and flips the normal's sign bits
            normal: [-normal[0], -normal[1], -normal[2]],
            depth: t,
            mesh: index,
            triangle: face.face_id,
        })
    }

    /// `PhysicsCore::rayCast` @ 0x1402cd070: a ray of `length` from `org` along `dir` (which
    /// is normalised first) against every mesh; the nearest contact, or none.
    pub fn ray_cast(&self, org: &[f32; 3], dir: &[f32; 3], length: f32) -> Option<RayContact> {
        assert!(!self.dirty, "StaticWorld::clean has to run after the last mesh was added");
        // dGeomRaySet @ 0x140345e50
        let mut n = *dir;
        safe_normalize3(&mut n);
        // dxRay::computeAABB @ 0x140344a60
        let mut ray_box = [0.0f32; 6];
        for k in 0..3 {
            let e = length * n[k] + org[k];
            if org[k] >= e {
                ray_box[2 * k] = e;
                ray_box[2 * k + 1] = org[k];
            } else {
                ray_box[2 * k] = org[k];
                ray_box[2 * k + 1] = e;
            }
        }
        // the accumulator of rayNearCallback @ 0x1402cd210
        let mut depth = -1.0f32;
        let mut best: Option<RayContact> = None;
        let mut ask = |member: usize| {
            let geom = &self.meshes[member];
            if geom.category_bits == 0 && geom.collide_bits == 0 {
                return;
            }
            if !boxes_meet(&geom.aabb, &ray_box) {
                return;
            }
            if let Some(contact) = self.collide_ray_mesh(member, org, &n, length) {
                // comiss acc, 0 ; jb take | comiss acc, c.depth ; jbe skip
                if !(depth >= 0.0) || depth > contact.depth {
                    depth = contact.depth;
                    best = Some(contact);
                }
            }
        };
        // the static space's own list from its head: sub-spaces, and meshes of sub-space id
        // 0 between them. A ray's bits are all ones, a sub-space's too: only the boxes
        // decide there
        for member in self.root.iter().rev() {
            match *member {
                RootMember::Mesh(mesh) => ask(mesh),
                RootMember::Space(space) => {
                    let space = &self.spaces[space];
                    if !boxes_meet(&space.aabb, &ray_box) {
                        continue;
                    }
                    for &mesh in space.members.iter().rev() {
                        ask(mesh);
                    }
                }
            }
        }
        if !(depth >= 0.0) {
            return None;
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of side 2 around (cx, cz) at height y, facing up (seen from above the corners
    /// run counter-clockwise, as the game's road meshes do).
    fn square(cx: f32, y: f32, cz: f32) -> (Vec<[f32; 3]>, Vec<u16>) {
        (vec![[cx - 1.0, y, cz - 1.0], [cx - 1.0, y, cz + 1.0], [cx + 1.0, y, cz + 1.0], [cx + 1.0, y, cz - 1.0]], vec![0, 1, 2, 0, 2, 3])
    }

    fn world(layers: &[(f32, u32)]) -> StaticWorld {
        let mut world = StaticWorld::new();
        for &(y, space) in layers {
            let (v, i) = square(0.0, y, 0.0);
            world.create_tri_mesh(v, i, 1, 0x14, space);
        }
        world.clean();
        world
    }

    const DOWN: [f32; 3] = [0.0, -1.0, 0.0];

    #[test]
    fn a_ray_straight_down_hits_the_road_from_above_only() {
        let world = world(&[(0.0, 1)]);
        let hit = world.ray_cast(&[0.25, 2.0, 0.5], &DOWN, 3.0).unwrap();
        assert_eq!(hit.pos, [0.25, 0.0, 0.5]);
        assert_eq!(hit.normal, [0.0, 1.0, 0.0]);
        assert_eq!(hit.depth, 2.0);
        assert_eq!(hit.mesh, 0);
        // from below the triangles are seen from behind: culled
        assert!(world.ray_cast(&[0.25, -2.0, 0.5], &[0.0, 1.0, 0.0], 3.0).is_none());
        // too short, and exactly as long as the distance (the end itself does not count)
        assert!(world.ray_cast(&[0.25, 2.0, 0.5], &DOWN, 1.5).is_none());
        assert!(world.ray_cast(&[0.25, 2.0, 0.5], &DOWN, 2.0).is_none());
        // beside the square
        assert!(world.ray_cast(&[1.5, 2.0, 0.5], &DOWN, 3.0).is_none());
    }

    #[test]
    fn of_two_meshes_the_nearer_wins_and_of_two_equal_ones_the_newer() {
        let stacked = world(&[(0.0, 1), (0.5, 2)]);
        assert_eq!(stacked.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        let stacked = world(&[(0.5, 1), (0.0, 2)]);
        assert_eq!(stacked.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 0);
        // the same height: the mesh of the sub-space made last is asked first and stays
        let level = world(&[(0.0, 1), (0.0, 2)]);
        assert_eq!(level.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        // and inside one sub-space the mesh made last
        let level = world(&[(0.0, 1), (0.0, 1)]);
        assert_eq!(level.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
    }

    #[test]
    fn the_direction_is_normalised_and_a_mesh_without_bits_is_not_asked() {
        let mut world = world(&[(0.0, 1)]);
        let hit = world.ray_cast(&[0.0, 2.0, 0.5], &[0.0, -7.0, 0.0], 3.0).unwrap();
        assert_eq!((hit.depth, hit.pos), (2.0, [0.0, 0.0, 0.5]));
        world.meshes[0].category_bits = 0;
        // collide bits alone are enough for a ray, whose own bits are all set
        assert!(world.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).is_some());
        world.meshes[0].collide_bits = 0;
        assert!(world.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).is_none());
    }

    /// A flat patch of 8 x 8 squares of side 1 around the origin at height y (128 triangles:
    /// a mesh with a tree).
    fn patch(y: f32) -> (Vec<[f32; 3]>, Vec<u16>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for ix in 0..9 {
            for iz in 0..9 {
                vertices.push([ix as f32 - 4.0, y, iz as f32 - 4.0]);
            }
        }
        for ix in 0..8u16 {
            for iz in 0..8u16 {
                let (a, b, c, d) = (ix * 9 + iz, ix * 9 + iz + 1, (ix + 1) * 9 + iz + 1, (ix + 1) * 9 + iz);
                indices.extend([a, b, c, a, c, d]);
            }
        }
        (vertices, indices)
    }

    #[test]
    fn a_ray_of_length_f32_max_has_no_end() {
        let mut world = StaticWorld::new();
        let (v, i) = patch(0.0);
        world.create_tri_mesh(v, i, 1, 0x14, 1);
        world.clean();
        assert!(!world.meshes[0].data.model.single_node);
        // from a kilometre up: far beyond any tyre ray, found by the unbounded walk
        let hit = world.ray_cast(&[0.25, 1000.0, 0.5], &DOWN, f32::MAX).unwrap();
        assert_eq!((hit.depth, hit.pos), (1000.0, [0.25, 0.0, 0.5]));
        assert!(world.ray_cast(&[0.25, 1000.0, 0.5], &DOWN, 999.0).is_none());
        // a slanted one, and the same answer as a long enough segment gives
        let dir = [0.6f32, -0.8, 0.0];
        let endless = world.ray_cast(&[-3.0, 5.0, 0.5], &dir, f32::MAX).unwrap();
        let segment = world.ray_cast(&[-3.0, 5.0, 0.5], &dir, 100.0).unwrap();
        assert_eq!(endless, segment);
        // beside the patch, pointing away, and from below (the triangles are seen from behind)
        assert!(world.ray_cast(&[10.0, 5.0, 0.5], &DOWN, f32::MAX).is_none());
        assert!(world.ray_cast(&[0.25, 5.0, 0.5], &[0.0, 1.0, 0.0], f32::MAX).is_none());
        assert!(world.ray_cast(&[0.25, -5.0, 0.5], &[0.0, 1.0, 0.0], f32::MAX).is_none());
        // a mesh of one triangle is answered with the segment's test in either mode: its
        // distance is compared with f32::MAX and passes
        let mut one = StaticWorld::new();
        one.create_tri_mesh(vec![[-1.0, 0.0, -1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, 1.0]], vec![0, 1, 2], 1, 0x14, 3);
        one.clean();
        assert_eq!(one.ray_cast(&[-0.5, 50.0, 0.5], &DOWN, f32::MAX).unwrap().depth, 50.0);
    }

    #[test]
    fn a_mesh_of_sub_space_0_is_asked_in_its_place_among_the_sub_spaces() {
        // at the same height the member of the static space that was made last answers first
        let direct_last = world(&[(0.0, 1), (0.0, 0)]);
        assert_eq!(direct_last.direct_meshes(), 1);
        assert_eq!(direct_last.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        let space_last = world(&[(0.0, 0), (0.0, 1)]);
        assert_eq!(space_last.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        // a later mesh of the older sub-space does not overtake the direct mesh (no ray was
        // cast in between: the sub-space stays where it is)
        let third = world(&[(0.0, 1), (0.0, 0), (0.0, 1)]);
        assert_eq!(third.root, vec![RootMember::Space(0), RootMember::Mesh(1)]);
        assert_eq!(third.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        // two direct meshes: the newer first
        let two = world(&[(0.0, 0), (0.0, 0)]);
        assert_eq!(two.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        // and the nearer one wins wherever it is
        let lower_direct = world(&[(0.5, 1), (0.0, 0)]);
        assert_eq!(lower_direct.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 0);
    }

    #[test]
    fn a_mesh_added_after_the_first_ray_moves_its_sub_space_to_the_front() {
        // two sub-spaces with a square each at the same place and height, made before any
        // ray: the newer sub-space answers first
        let mut w = StaticWorld::new();
        for space in [1, 2] {
            let (v, i) = square(0.0, 0.0, 0.0);
            w.create_tri_mesh(v, i, 1, 0x14, space);
        }
        // a mesh added to a sub-space that was never cleaned leaves the order alone
        let (v, i) = square(300.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 1);
        assert_eq!(w.root, vec![RootMember::Space(0), RootMember::Space(1)]);
        w.clean();
        assert_eq!(w.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 1);
        // after that ray a new mesh for the OLDER sub-space, somewhere else: the sub-space
        // goes to the head of the list, and from now on its square answers first
        let (v, i) = square(50.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 1);
        assert_eq!(w.root, vec![RootMember::Space(1), RootMember::Space(0)]);
        w.clean();
        assert_eq!(w.spaces[0].aabb, [-1.0, 301.0, 0.0, 0.0, -1.0, 1.0]);
        assert_eq!(w.ray_cast(&[50.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 3);
        assert_eq!(w.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 0);
        // a second mesh before the next cleaning: the sub-space is dirty already, no move
        let (v, i) = square(0.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 2);
        let (v, i) = square(60.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 2);
        assert_eq!(w.root, vec![RootMember::Space(0), RootMember::Space(1)]);
        w.clean();
        // (inside a sub-space the newest mesh answers first)
        assert_eq!(w.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 4);
        // a new sub-space and a mesh of sub-space id 0 go to the head as well
        let (v, i) = square(0.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 3);
        w.clean();
        assert_eq!(w.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 6);
        let (v, i) = square(0.0, 0.0, 0.0);
        w.create_tri_mesh(v, i, 1, 0x14, 0);
        w.clean();
        assert_eq!(w.ray_cast(&[0.0, 2.0, 0.5], &DOWN, 3.0).unwrap().mesh, 7);
    }

    #[test]
    fn a_single_triangle_needs_no_tree() {
        let mut world = StaticWorld::new();
        world.create_tri_mesh(vec![[-1.0, 0.0, -1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, 1.0]], vec![0, 1, 2], 1, 0x14, 3);
        world.clean();
        assert!(world.meshes[0].data.model.single_node);
        assert_eq!(world.ray_cast(&[-0.5, 1.0, 0.5], &DOWN, 3.0).unwrap().triangle, 0);
        assert!(world.ray_cast(&[0.5, 1.0, -0.5], &DOWN, 3.0).is_none());
    }
}
