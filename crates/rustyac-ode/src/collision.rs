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
}

impl TriMeshGeom {
    /// `dxTriMesh::computeAABB` @ 0x14034ae10
    fn compute_aabb(&mut self) {
        let (c, e, r, p) = (&self.data.aabb_center, &self.data.aabb_extents, &self.r, &self.pos);
        let xc = (c[0] * r[0] + r[1] * c[1]) + r[2] * c[2];
        let yc = (c[1] * r[5] + c[0] * r[4]) + c[2] * r[6];
        let zc = (c[1] * r[9] + c[0] * r[8]) + c[2] * r[10];
        let xr = ((e[0] * r[0]).abs() + (e[1] * r[1]).abs()) + (e[2] * r[2]).abs();
        let yr = ((e[1] * r[5]).abs() + (e[0] * r[4]).abs()) + (e[2] * r[6]).abs();
        let zr = ((e[1] * r[9]).abs() + (e[0] * r[8]).abs()) + (e[2] * r[10]).abs();
        self.aabb = [(xc + p[0]) - xr, (xc + p[0]) + xr, (yc + p[1]) - yr, (yc + p[1]) + yr, (zc + p[2]) - zr, (zc + p[2]) + zr];
    }
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
    /// Meshes were added since the boxes were last computed.
    dirty: bool,
}

/// `collideAABBs` @ 0x140342c60, the six comparisons: does a member's box `b1` meet the ray's `b2`?
#[inline]
fn boxes_meet(b1: &[f32; 6], b2: &[f32; 6]) -> bool {
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
        if distance.to_bits() >= self.max_dist.to_bits() {
            return None;
        }
        Some(Face { face_id: prim, distance })
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
        });
        match self.spaces.iter_mut().find(|s| s.id == space_id) {
            Some(space) => space.members.push(index),
            None => self.spaces.push(SubSpace { id: space_id, members: vec![index], aabb: [0.0; 6] }),
        }
        self.dirty = true;
        index
    }

    /// What the first `dxSimpleSpace::cleanGeoms` @ 0x1403429d0 after loading does: every
    /// mesh's box, then every sub-space's (`dxSpace::computeAABB` @ 0x140342d40).
    pub fn clean(&mut self) {
        for mesh in &mut self.meshes {
            mesh.compute_aabb();
        }
        for space in &mut self.spaces {
            let mut a = [f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY];
            // the list's order: the newest member first
            for &member in space.members.iter().rev() {
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
            space.aabb = a;
        }
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
            collider.triangle(0)
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
        debug_assert!(length.to_bits() != 0x7f7f_ffff, "a ray of length f32::MAX is OPCODE's unbounded ray, which is not ported");
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
        // a ray's bits are all ones, a sub-space's too: only the boxes decide there
        for space in self.spaces.iter().rev() {
            if !boxes_meet(&space.aabb, &ray_box) {
                continue;
            }
            for &member in space.members.iter().rev() {
                let geom = &self.meshes[member];
                if geom.category_bits == 0 && geom.collide_bits == 0 {
                    continue;
                }
                if !boxes_meet(&geom.aabb, &ray_box) {
                    continue;
                }
                if let Some(contact) = self.collide_ray_mesh(member, org, &n, length) {
                    // comiss acc, 0 ; jb take | comiss acc, c.depth ; jbe skip
                    if !(depth >= 0.0) || depth > contact.depth {
                        depth = contact.depth;
                        best = Some(contact);
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
