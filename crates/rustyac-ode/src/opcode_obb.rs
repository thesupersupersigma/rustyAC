// SPDX-License-Identifier: BSD-3-Clause

//! OPCODE's box-against-tree query (`Opcode::OBBCollider`, `OPC_OBBCollider.cpp`) as ODE's
//! box-trimesh collider runs it in `acs.exe`: which triangles of a mesh may touch an
//! oriented box, **in the order the tree walk meets them** (the contact generator consumes
//! them in that order and may stop early).
//!
//! The game's settings: first contact off, temporal coherence off (ODE clears the bit before
//! every query), primitive tests on, full box-box test on. So the query is a pure function of
//! the box, the mesh's place and the mesh: a depth-first walk, positive child first, with
//! the 15-axis box-box test per node; a node whose box lies completely inside the box hands
//! over every triangle below it untested; otherwise each leaf's triangle is tested against
//! the box.
//!
//! Everything is scalar single precision in the machine code; the sums are written here in
//! its grouping, the comparisons with its predicates (they decide what a NaN does).

use crate::opcode::{AabbNoLeafNode, Child, MeshInterface, Model};

/// IceMaths' row-major 4x4 matrix; a point is a row vector (`p' = p * M`), row 3 is the
/// translation.
pub(crate) type Matrix4x4 = [[f32; 4]; 4];

/// `MakeMatrix(pos, R, out)` of ODE's trimesh code: ODE's rotation transposed into rows, the
/// position below.
pub(crate) fn make_matrix(pos: &[f32; 3], r: &[f32; 12]) -> Matrix4x4 {
    [[r[0], r[4], r[8], 0.0], [r[1], r[5], r[9], 0.0], [r[2], r[6], r[10], 0.0], [pos[0], pos[1], pos[2], 1.0]]
}

/// `InvertPRMatrix` @ 0x1403954a0: the inverse of a rotation + translation matrix. The
/// translation's sign is flipped as a bit (a sum of +0 gives -0).
pub(crate) fn invert_pr_matrix(src: &Matrix4x4) -> Matrix4x4 {
    let mut dest = [[0.0f32; 4]; 4];
    for j in 0..3 {
        dest[0][j] = src[j][0];
        dest[1][j] = src[j][1];
        dest[2][j] = src[j][2];
        dest[3][j] = -((src[3][0] * src[j][0] + src[3][1] * src[j][1]) + src[3][2] * src[j][2]);
    }
    dest[3][3] = 1.0;
    dest
}

/// `Matrix4x4::operator*` @ 0x1403542a0: all four terms of every element are computed and
/// added, the ones with the fourth column included.
pub(crate) fn mul4(a: &Matrix4x4, b: &Matrix4x4) -> Matrix4x4 {
    let mut r = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = ((a[i][0] * b[0][j] + a[i][1] * b[1][j]) + a[i][2] * b[2][j]) + a[i][3] * b[3][j];
        }
    }
    r
}

/// The state of one query (`Opcode::OBBCollider` after `InitQuery`).
struct ObbCollider<'a> {
    mesh: &'a MeshInterface,
    /// `mAR`
    ar: [[f32; 3]; 3],
    /// `mRModelToBox`, `mRBoxToModel`
    r_model_to_box: [[f32; 3]; 3],
    r_box_to_model: [[f32; 3]; 3],
    /// `mTModelToBox`, `mTBoxToModel`
    t_model_to_box: [f32; 3],
    t_box_to_model: [f32; 3],
    /// `mBoxExtents`
    box_extents: [f32; 3],
    /// `mB0`, `mB1`
    b0: [f32; 3],
    b1: [f32; 3],
    /// `mBBx1`, `mBBy1`, `mBBz1`
    bb1: [f32; 3],
    /// `mBB_1` … `mBB_9`
    bb: [f32; 9],
    /// `mTouchedPrimitives`
    touched: Vec<u32>,
    /// `OPC_CONTACT` in `mFlags`
    contact: bool,
}

#[inline]
fn min3(a: f32, b: f32, c: f32) -> f32 {
    if !(a >= b) {
        if !(a >= c) {
            a
        } else {
            c
        }
    } else if !(b >= c) {
        b
    } else {
        c
    }
}

#[inline]
fn max3(a: f32, b: f32, c: f32) -> f32 {
    if a > b {
        if a > c {
            a
        } else {
            c
        }
    } else if b > c {
        b
    } else {
        c
    }
}

impl ObbCollider<'_> {
    /// `BoxBoxOverlap` as inlined in `_Collide` @ 0x140373190: all 15 axes. Only "strictly
    /// greater" rejects, so a NaN never does.
    fn box_box_overlap(&self, node: &AabbNoLeafNode) -> bool {
        let (c, e) = (&node.center, &node.extents);
        let (r, ar, t, be) = (&self.r_box_to_model, &self.ar, &self.t_box_to_model, &self.box_extents);

        // Class I : A's basis vectors
        let tx = t[0] - c[0];
        if tx.abs() > e[0] + self.bb1[0] {
            return false;
        }
        let ty = t[1] - c[1];
        if ty.abs() > e[1] + self.bb1[1] {
            return false;
        }
        let tz = t[2] - c[2];
        if tz.abs() > e[2] + self.bb1[2] {
            return false;
        }

        // Class II : B's basis vectors
        for k in 0..3 {
            let tt = (tx * r[k][0] + ty * r[k][1]) + tz * r[k][2];
            let t2 = ((e[0] * ar[k][0] + e[1] * ar[k][1]) + e[2] * ar[k][2]) + be[k];
            if tt.abs() > t2 {
                return false;
            }
        }

        // Class III : 9 cross products (the full test is always on)
        for k in 0..3 {
            let tt = tz * r[k][1] - ty * r[k][2];
            let t2 = (e[1] * ar[k][2] + e[2] * ar[k][1]) + self.bb[k];
            if tt.abs() > t2 {
                return false;
            }
        }
        for k in 0..3 {
            let tt = tx * r[k][2] - tz * r[k][0];
            let t2 = (e[0] * ar[k][2] + e[2] * ar[k][0]) + self.bb[3 + k];
            if tt.abs() > t2 {
                return false;
            }
        }
        for k in 0..3 {
            let tt = ty * r[k][0] - tx * r[k][1];
            let t2 = (e[0] * ar[k][1] + e[1] * ar[k][0]) + self.bb[6 + k];
            if tt.abs() > t2 {
                return false;
            }
        }
        true
    }

    /// `OBBContainsBox` as inlined: is the node's box completely inside the box? (A NaN says
    /// no at the lower bounds.)
    fn obb_contains_box(&self, node: &AabbNoLeafNode) -> bool {
        let (c, e, m) = (&node.center, &node.extents, &self.r_model_to_box);
        for j in 0..3 {
            let nc = (c[0] * m[0][j] + c[1] * m[1][j]) + c[2] * m[2][j];
            let ne = ((m[0][j] * e[0]).abs() + (m[1][j] * e[1]).abs()) + (m[2][j] * e[2]).abs();
            if nc + ne > self.b0[j] {
                return false;
            }
            if !(nc - ne >= self.b1[j]) {
                return false;
            }
        }
        true
    }

    /// `OBB_PRIM`: the triangle into the box's space, `TriBoxOverlap`, and on overlap the
    /// triangle's index is appended.
    fn prim(&mut self, index: u32) {
        let tri = self.mesh.triangle(index);
        let (m, t) = (&self.r_model_to_box, &self.t_model_to_box);
        let mut v = [[0.0f32; 3]; 3];
        for k in 0..3 {
            let p = tri[k];
            for j in 0..3 {
                // the translation is added right after the first product
                v[k][j] = ((p[0] * m[0][j] + t[j]) + p[1] * m[1][j]) + p[2] * m[2][j];
            }
        }
        if self.tri_box_overlap(&v) {
            self.contact = true;
            self.touched.push(index);
        }
    }

    /// `TriBoxOverlap` (`OPC_TriBoxOverlap.h`): the triangle `v` (in the box's space) against
    /// the box around the origin.
    fn tri_box_overlap(&self, v: &[[f32; 3]; 3]) -> bool {
        let e = &self.box_extents;
        let (v0, v1, v2) = (&v[0], &v[1], &v[2]);

        // 1) the three box axes
        for q in 0..3 {
            if min3(v0[q], v1[q], v2[q]) > e[q] {
                return false;
            }
            if !(max3(v0[q], v1[q], v2[q]) >= -e[q]) {
                return false;
            }
        }

        // 2) the triangle's plane against the box
        let e0 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e1 = [v2[0] - v1[0], v2[1] - v1[1], v2[2] - v1[2]];
        let n = [e0[1] * e1[2] - e0[2] * e1[1], e0[2] * e1[0] - e0[0] * e1[2], e0[0] * e1[1] - e0[1] * e1[0]];
        let nn = [-n[0], -n[1], -n[2]];
        let d = (nn[0] * v0[0] + nn[1] * v0[1]) + nn[2] * v0[2];
        let mut vmin = [0.0f32; 3];
        let mut vmax = [0.0f32; 3];
        for q in 0..3 {
            if n[q] > 0.0 {
                vmin[q] = -e[q];
                vmax[q] = e[q];
            } else {
                vmin[q] = e[q];
                vmax[q] = -e[q];
            }
        }
        if ((n[0] * vmin[0] + n[1] * vmin[1]) + n[2] * vmin[2]) + d > 0.0 {
            return false;
        }
        if !(((n[0] * vmax[0] + n[1] * vmax[1]) + n[2] * vmax[2]) + d >= 0.0) {
            return false;
        }

        // 3) the nine edge axes
        #[inline]
        fn axis(mut min: f32, mut max: f32, rad: f32) -> bool {
            if min > max {
                std::mem::swap(&mut min, &mut max);
            }
            if min > rad {
                return false;
            }
            if !(max >= -rad) {
                return false;
            }
            true
        }
        // edge 0
        let (fex, fey, fez) = (e0[0].abs(), e0[1].abs(), e0[2].abs());
        // AXISTEST_X01
        if !axis(e0[2] * v0[1] - e0[1] * v0[2], e0[2] * v2[1] - e0[1] * v2[2], fez * e[1] + fey * e[2]) {
            return false;
        }
        // AXISTEST_Y02
        if !axis(e0[0] * v0[2] - e0[2] * v0[0], e0[0] * v2[2] - e0[2] * v2[0], fez * e[0] + fex * e[2]) {
            return false;
        }
        // AXISTEST_Z12
        if !axis(e0[1] * v1[0] - e0[0] * v1[1], e0[1] * v2[0] - e0[0] * v2[1], fey * e[0] + fex * e[1]) {
            return false;
        }
        // edge 1
        let (fex, fey, fez) = (e1[0].abs(), e1[1].abs(), e1[2].abs());
        // AXISTEST_X01
        if !axis(e1[2] * v0[1] - e1[1] * v0[2], e1[2] * v2[1] - e1[1] * v2[2], fez * e[1] + fey * e[2]) {
            return false;
        }
        // AXISTEST_Y02
        if !axis(e1[0] * v0[2] - e1[2] * v0[0], e1[0] * v2[2] - e1[2] * v2[0], fez * e[0] + fex * e[2]) {
            return false;
        }
        // AXISTEST_Z0
        if !axis(e1[1] * v0[0] - e1[0] * v0[1], e1[1] * v1[0] - e1[0] * v1[1], fey * e[0] + fex * e[1]) {
            return false;
        }
        // edge 2
        let e2 = [v0[0] - v2[0], v0[1] - v2[1], v0[2] - v2[2]];
        let (fex, fey, fez) = (e2[0].abs(), e2[1].abs(), e2[2].abs());
        // AXISTEST_X2
        if !axis(e2[2] * v0[1] - e2[1] * v0[2], e2[2] * v1[1] - e2[1] * v1[2], fez * e[1] + fey * e[2]) {
            return false;
        }
        // AXISTEST_Y1
        if !axis(e2[0] * v0[2] - e2[2] * v0[0], e2[0] * v1[2] - e2[2] * v1[0], fez * e[0] + fex * e[2]) {
            return false;
        }
        // AXISTEST_Z12
        if !axis(e2[1] * v1[0] - e2[0] * v1[1], e2[1] * v2[0] - e2[0] * v2[1], fey * e[0] + fex * e[1]) {
            return false;
        }
        true
    }

    /// `VolumeCollider::_Dump` @ 0x1403956c0: every triangle below the node, depth first, the
    /// positive side first, untested.
    fn dump(&mut self, model: &Model, node: u32) {
        let mut pending = vec![Child::Node(node)];
        while let Some(item) = pending.pop() {
            match item {
                Child::Triangle(prim) => self.touched.push(prim),
                Child::Node(id) => {
                    let n = &model.nodes[id as usize];
                    pending.push(n.neg);
                    pending.push(n.pos);
                }
            }
        }
    }

    /// `OBBCollider::_Collide(const AABBNoLeafNode*)` @ 0x140373190.
    fn collide(&mut self, model: &Model) {
        let mut pending = vec![Child::Node(0)];
        while let Some(item) = pending.pop() {
            match item {
                // a leaf has no box of its own: its triangle is tested as soon as its parent passed
                Child::Triangle(prim) => self.prim(prim),
                Child::Node(id) => {
                    let node = &model.nodes[id as usize];
                    if !self.box_box_overlap(node) {
                        continue;
                    }
                    if self.obb_contains_box(node) {
                        self.contact = true;
                        self.dump(model, id);
                        continue;
                    }
                    pending.push(node.neg);
                    pending.push(node.pos);
                }
            }
        }
    }
}

/// `OBBCollider::Collide` @ 0x140370500 with `InitQuery` @ 0x140370680, as
/// `dQueryBTLPotentialCollisionTriangles` @ 0x14038a630 calls it: the box (`center`, half
/// sizes `extents`, ODE rotation `box_r`) against the mesh at (`mesh_pos`, `mesh_r`).
/// Returns OPCODE's contact flag and the touched triangles in walk order.
pub(crate) fn obb_query(
    center: &[f32; 3],
    extents: &[f32; 3],
    box_r: &[f32; 12],
    model: &Model,
    mesh: &MeshInterface,
    mesh_pos: &[f32; 3],
    mesh_r: &[f32; 12],
) -> (bool, Vec<u32>) {
    if !model.built {
        // Collider::Setup: a model without a mesh. (The game then reads the flag and the
        // list of the query before; no mesh of a track or car is like that.)
        return (false, Vec::new());
    }
    // WorldB: the box's rotation (mRot.m[i][j] = R[4*j + i]) with its centre as translation
    let world_b = make_matrix(center, box_r);
    let world_m = make_matrix(mesh_pos, mesh_r);
    let inv_world_b = invert_pr_matrix(&world_b);
    let inv_world_m = invert_pr_matrix(&world_m);
    let world_b_to_m = mul4(&world_b, &inv_world_m);
    let world_m_to_b = mul4(&world_m, &inv_world_b);
    let rot = |m: &Matrix4x4| [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let mut collider = ObbCollider {
        mesh,
        ar: [[0.0; 3]; 3],
        r_model_to_box: rot(&world_m_to_b),
        r_box_to_model: rot(&world_b_to_m),
        t_model_to_box: [world_m_to_b[3][0], world_m_to_b[3][1], world_m_to_b[3][2]],
        t_box_to_model: [world_b_to_m[3][0], world_b_to_m[3][1], world_b_to_m[3][2]],
        box_extents: *extents,
        b0: [0.0; 3],
        b1: [0.0; 3],
        bb1: [0.0; 3],
        bb: [0.0; 9],
        touched: Vec::new(),
        contact: false,
    };

    // the mesh of one triangle has no tree
    if model.single_node {
        collider.prim(0);
        return (collider.contact, collider.touched);
    }

    // precomputed data of the box-box test
    for i in 0..3 {
        for j in 0..3 {
            collider.ar[i][j] = collider.r_box_to_model[i][j].abs() + f32::from_bits(0x3586_37bd); // 1e-6
        }
    }
    let (e, t, ar) = (collider.box_extents, collider.t_model_to_box, collider.ar);
    collider.b0 = [e[0] - t[0], e[1] - t[1], e[2] - t[2]];
    collider.b1 = [(-e[0]) - t[0], (-e[1]) - t[1], (-e[2]) - t[2]];
    for j in 0..3 {
        collider.bb1[j] = (e[0] * ar[0][j] + e[1] * ar[1][j]) + e[2] * ar[2][j];
        collider.bb[3 * j] = e[1] * ar[2][j] + e[2] * ar[1][j];
        collider.bb[3 * j + 1] = e[0] * ar[2][j] + e[2] * ar[0][j];
        collider.bb[3 * j + 2] = e[0] * ar[1][j] + e[1] * ar[0][j];
    }
    collider.collide(model);
    (collider.contact, collider.touched)
}
