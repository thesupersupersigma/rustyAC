// SPDX-License-Identifier: BSD-3-Clause

//! OPCODE's tree-against-tree query (`Opcode::AABBTreeCollider`, `OPC_TreeCollider.cpp`) as
//! ODE's trimesh-trimesh collider runs it in `acs.exe`: which triangles of one mesh meet
//! which triangles of another, **in the order the walk over the two trees finds them**.
//!
//! The settings are the game's and never change: first contact off, temporal coherence off,
//! the full box-box test and the full triangle-box test on. Both trees are "no leaf" trees,
//! not quantised. Nothing here is symmetric in the two meshes: node boxes are compared in
//! the first mesh's space, a leaf of the first tree is tested in the second mesh's space and
//! the other way round, and the triangle-triangle test always gets the transformed leaf
//! first and the raw triangle second.
//!
//! Every comparison is written with the predicate of the machine code (it decides what a
//! NaN does and which of two equal values is kept).

use crate::opcode::{AabbNoLeafNode, Child, MeshInterface, Model};
use crate::opcode_obb::{invert_pr_matrix, mul4, Matrix4x4};

/// `LOCAL_EPSILON` of the triangle test, and the epsilon inside `mAR`: 1e-6
const EPSILON: f32 = f32::from_bits(0x3586_37bd);

type Point = [f32; 3];

#[inline]
fn sub(a: &Point, b: &Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `Point::operator^` @ 0x14034a690
#[inline]
fn cross(a: &Point, b: &Point) -> Point {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `FCMin3` as compiled: `!(a >= b)` where the source has `a < b`.
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

/// `FCMax3`
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

/// `x != 0.0f` as compiled (`ucomiss` + `jne`, no parity test): false for a NaN.
#[inline]
fn not_zero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// `CoplanarTriTri` @ 0x140354d10: two triangles in one plane (`n` is the normal of `v`).
fn coplanar_tri_tri(n: &Point, v: &[Point; 3], u: &[Point; 3]) -> bool {
    let a = [n[0].abs(), n[1].abs(), n[2].abs()];
    // the two axes of the plane the triangles are looked at in
    let (i0, i1) = if a[0] > a[1] {
        if a[0] > a[2] {
            (1, 2)
        } else {
            (0, 1)
        }
    } else if a[2] > a[1] {
        (0, 1)
    } else {
        (0, 2)
    };

    // EDGE_AGAINST_TRI_EDGES: each edge of v against the three edges of u
    for (p, q) in [(&v[0], &v[1]), (&v[1], &v[2]), (&v[2], &v[0])] {
        let ax = q[i0] - p[i0];
        let ay = q[i1] - p[i1];
        for (s, t) in [(&u[0], &u[1]), (&u[1], &u[2]), (&u[2], &u[0])] {
            // EDGE_EDGE_TEST
            let bx = s[i0] - t[i0];
            let by = s[i1] - t[i1];
            let cx = p[i0] - s[i0];
            let cy = p[i1] - s[i1];
            let f = ay * bx - ax * by;
            let d = by * cx - bx * cy;
            if (f > 0.0 && d >= 0.0 && d <= f) || (f < 0.0 && d <= 0.0 && d >= f) {
                let e = ax * cy - ay * cx;
                if f > 0.0 {
                    if e >= 0.0 && e <= f {
                        return true;
                    }
                } else if e <= 0.0 && e >= f {
                    return true;
                }
            }
        }
    }

    // POINT_IN_TRI: the first corner of one triangle inside the other
    let point_in_tri = |p: &Point, s: &[Point; 3]| -> bool {
        let side = |s0: &Point, s1: &Point| -> f32 {
            let a = s1[i1] - s0[i1];
            let b = -(s1[i0] - s0[i0]);
            let c = (-(a * s0[i0])) - b * s0[i1];
            (a * p[i0] + b * p[i1]) + c
        };
        let d0 = side(&s[0], &s[1]);
        let d1 = side(&s[1], &s[2]);
        if d0 * d1 > 0.0 {
            let d2 = side(&s[2], &s[0]);
            if d0 * d2 > 0.0 {
                return true;
            }
        }
        false
    };
    point_in_tri(&v[0], u) || point_in_tri(&u[0], v)
}

/// `NEWCOMPUTE_INTERVALS`: (A, B, C, X0, X1) of a triangle's interval on the line where the
/// two planes meet. `None`: the triangles are in one plane.
#[inline]
#[allow(clippy::too_many_arguments)]
fn intervals(vv0: f32, vv1: f32, vv2: f32, d0: f32, d1: f32, d2: f32, d0d1: f32, d0d2: f32) -> Option<(f32, f32, f32, f32, f32)> {
    let case0 = || Some((vv0, (vv1 - vv0) * d0, (vv2 - vv0) * d0, d0 - d1, d0 - d2));
    let case1 = || Some((vv1, (vv0 - vv1) * d1, (vv2 - vv1) * d1, d1 - d0, d1 - d2));
    let case2 = || Some((vv2, (vv0 - vv2) * d2, (vv1 - vv2) * d2, d2 - d0, d2 - d1));
    if d0d1 > 0.0 {
        // D0 and D1 are on the same side, D2 on the other or on the plane
        case2()
    } else if d0d2 > 0.0 {
        case1()
    } else if d1 * d2 > 0.0 || not_zero(d0) {
        case0()
    } else if not_zero(d1) {
        case1()
    } else if not_zero(d2) {
        case2()
    } else {
        None
    }
}

/// The distances of the other triangle's corners to a plane, with the ones that are as good
/// as zero set to zero (`OPC_TRITRI_EPSILON_TEST` of ODE 0.13.1: relative to the plane's size).
#[inline]
fn plane_distances(n: &Point, d: f32, p: &[Point; 3]) -> [f32; 3] {
    let mut out = [
        ((n[0] * p[0][0] + n[1] * p[0][1]) + n[2] * p[0][2]) + d,
        ((n[0] * p[1][0] + n[1] * p[1][1]) + n[2] * p[1][2]) + d,
        ((n[0] * p[2][0] + n[1] * p[2][1]) + n[2] * p[2][2]) + d,
    ];
    let absd = d.abs();
    let sqmag = (n[0] * n[0] + n[1] * n[1]) + n[2] * n[2];
    if absd >= sqmag {
        let eps = absd * EPSILON;
        for value in &mut out {
            if !(value.abs() > eps) {
                *value = 0.0;
            }
        }
    } else {
        for k in 0..3 {
            let sq = (p[k][0] * p[k][0] + p[k][1] * p[k][1]) + p[k][2] * p[k][2];
            // FCMin2(sqmag, sq), FCMax2(absd, ..) as compiled
            let m = if !(sqmag >= sq) { sqmag } else { sq };
            let big = if absd > m { absd } else { m };
            if !(out[k].abs() > big * EPSILON) {
                out[k] = 0.0;
            }
        }
    }
    out
}

/// `TriTriOverlap` (`OPC_TriTriOverlap.h`, Möller's interval test without divisions), as
/// inlined eight times: do the triangles `v` and `u` meet? Not symmetric in the two.
fn tri_tri_overlap(v: &[Point; 3], u: &[Point; 3]) -> bool {
    // the plane of v, and u's corners against it
    let e1 = sub(&v[1], &v[0]);
    let e2 = sub(&v[2], &v[0]);
    let n1 = cross(&e1, &e2);
    let m1 = [-n1[0], -n1[1], -n1[2]];
    let d1 = (m1[0] * v[0][0] + m1[1] * v[0][1]) + m1[2] * v[0][2];
    let [du0, du1, du2] = plane_distances(&n1, d1, u);
    let du0du1 = du0 * du1;
    let du0du2 = du0 * du2;
    if du0du1 > 0.0 && du0du2 > 0.0 {
        // all of u on one side
        return false;
    }

    // the plane of u, and v's corners against it
    let e1 = sub(&u[1], &u[0]);
    let e2 = sub(&u[2], &u[0]);
    let n2 = cross(&e1, &e2);
    let m2 = [-n2[0], -n2[1], -n2[2]];
    let d2 = (m2[0] * u[0][0] + m2[1] * u[0][1]) + m2[2] * u[0][2];
    let [dv0, dv1, dv2] = plane_distances(&n2, d2, v);
    let dv0dv1 = dv0 * dv1;
    let dv0dv2 = dv0 * dv2;
    if dv0dv1 > 0.0 && dv0dv2 > 0.0 {
        return false;
    }

    // the direction of the line the planes meet in: its largest component
    let d = cross(&n1, &n2);
    let mut max = d[0].abs();
    let mut index = 0;
    let bb = d[1].abs();
    let cc = d[2].abs();
    if bb > max {
        max = bb;
        index = 1;
    }
    if cc > max {
        index = 2;
    }
    let (vp0, vp1, vp2) = (v[0][index], v[1][index], v[2][index]);
    let (up0, up1, up2) = (u[0][index], u[1][index], u[2][index]);

    // the interval of v on the line, then that of u
    let Some((a, b, c, x0, x1)) = intervals(vp0, vp1, vp2, dv0, dv1, dv2, dv0dv1, dv0dv2) else {
        return coplanar_tri_tri(&n1, v, u);
    };
    let Some((d, e, f, y0, y1)) = intervals(up0, up1, up2, du0, du1, du2, du0du1, du0du2) else {
        return coplanar_tri_tri(&n1, v, u);
    };

    let xx = x0 * x1;
    let yy = y0 * y1;
    let xxyy = xx * yy;
    let tmp = a * xxyy;
    let mut i10 = tmp + (b * x1) * yy;
    let mut i11 = tmp + (c * x0) * yy;
    let tmp = d * xxyy;
    let mut i20 = tmp + (e * xx) * y1;
    let mut i21 = tmp + (f * xx) * y0;
    if i10 > i11 {
        std::mem::swap(&mut i10, &mut i11);
    }
    if i20 > i21 {
        std::mem::swap(&mut i20, &mut i21);
    }
    if !(i11 >= i20) {
        return false;
    }
    if !(i21 >= i10) {
        return false;
    }
    true
}

/// The state of one query (`Opcode::AABBTreeCollider` after `InitQuery`).
struct TreeCollider<'a> {
    /// `mIMesh0`, `mIMesh1`
    mesh0: &'a MeshInterface,
    mesh1: &'a MeshInterface,
    nodes0: &'a [AabbNoLeafNode],
    nodes1: &'a [AabbNoLeafNode],
    /// `mAR`: |`mR1to0`| + 1e-6
    ar: [[f32; 3]; 3],
    /// `mR0to1`, `mR1to0`
    r0to1: [[f32; 3]; 3],
    r1to0: [[f32; 3]; 3],
    /// `mT0to1`, `mT1to0`
    t0to1: Point,
    t1to0: Point,
    /// `mLeafVerts`: the fetched leaf triangle, in the OTHER mesh's space
    leaf_verts: [Point; 3],
    /// `mLeafIndex`
    leaf_index: u32,
    /// `mPairs`
    pairs: Vec<(u32, u32)>,
}

/// An axis of `TriBoxOverlap`'s nine: the triangle's extent (two of its corners are enough)
/// against the box's radius.
macro_rules! axis_test {
    ($min:expr, $max:expr, $rad:expr) => {{
        let (mut min, mut max): (f32, f32) = ($min, $max);
        let rad: f32 = $rad;
        if min > max {
            std::mem::swap(&mut min, &mut max);
        }
        if min > rad {
            return false;
        }
        if !(max >= -rad) {
            return false;
        }
    }};
}

impl TreeCollider<'_> {
    /// `BoxBoxOverlap` (`OPC_BoxBoxOverlap.h`), inlined in `_Collide`: box A is the first
    /// tree's, as it is; box B is the second tree's, brought over. All 15 axes. A NaN never
    /// parts two boxes.
    fn box_box_overlap(&self, a: &AabbNoLeafNode, b: &AabbNoLeafNode) -> bool {
        let (ca, ea) = (&a.center, &a.extents);
        let (cb, eb) = (&b.center, &b.extents);
        let (r, ar, t) = (&self.r1to0, &self.ar, &self.t1to0);

        // Class I: A's basis vectors
        let tx = (((r[0][0] * cb[0] + r[1][0] * cb[1]) + r[2][0] * cb[2]) + t[0]) - ca[0];
        let reach = ((ea[0] + eb[0] * ar[0][0]) + eb[1] * ar[1][0]) + eb[2] * ar[2][0];
        if tx.abs() > reach {
            return false;
        }
        let ty = (((r[0][1] * cb[0] + r[1][1] * cb[1]) + r[2][1] * cb[2]) + t[1]) - ca[1];
        let reach = ((ea[1] + eb[0] * ar[0][1]) + eb[1] * ar[1][1]) + eb[2] * ar[2][1];
        if ty.abs() > reach {
            return false;
        }
        let tz = (((r[0][2] * cb[0] + r[1][2] * cb[1]) + r[2][2] * cb[2]) + t[2]) - ca[2];
        let reach = ((ea[2] + eb[0] * ar[0][2]) + eb[1] * ar[1][2]) + eb[2] * ar[2][2];
        if tz.abs() > reach {
            return false;
        }

        // Class II: B's basis vectors
        for k in 0..3 {
            let along = (tx * r[k][0] + ty * r[k][1]) + tz * r[k][2];
            let reach = ((ea[0] * ar[k][0] + ea[1] * ar[k][1]) + ea[2] * ar[k][2]) + eb[k];
            if along.abs() > reach {
                return false;
            }
        }

        // Class III: the nine cross products (mFullBoxBoxTest is on: always)
        let along = tz * r[0][1] - ty * r[0][2];
        let reach = ((ea[1] * ar[0][2] + ea[2] * ar[0][1]) + eb[1] * ar[2][0]) + eb[2] * ar[1][0];
        if along.abs() > reach {
            return false;
        }
        let along = tz * r[1][1] - ty * r[1][2];
        let reach = ((ea[1] * ar[1][2] + ea[2] * ar[1][1]) + eb[0] * ar[2][0]) + eb[2] * ar[0][0];
        if along.abs() > reach {
            return false;
        }
        let along = tz * r[2][1] - ty * r[2][2];
        let reach = ((ea[1] * ar[2][2] + ea[2] * ar[2][1]) + eb[0] * ar[1][0]) + eb[1] * ar[0][0];
        if along.abs() > reach {
            return false;
        }
        let along = tx * r[0][2] - tz * r[0][0];
        let reach = ((ea[0] * ar[0][2] + ea[2] * ar[0][0]) + eb[1] * ar[2][1]) + eb[2] * ar[1][1];
        if along.abs() > reach {
            return false;
        }
        let along = tx * r[1][2] - tz * r[1][0];
        let reach = ((ea[0] * ar[1][2] + ea[2] * ar[1][0]) + eb[0] * ar[2][1]) + eb[2] * ar[0][1];
        if along.abs() > reach {
            return false;
        }
        let along = tx * r[2][2] - tz * r[2][0];
        let reach = ((ea[0] * ar[2][2] + ea[2] * ar[2][0]) + eb[0] * ar[1][1]) + eb[1] * ar[0][1];
        if along.abs() > reach {
            return false;
        }
        let along = ty * r[0][0] - tx * r[0][1];
        let reach = ((ea[0] * ar[0][1] + ea[1] * ar[0][0]) + eb[1] * ar[2][2]) + eb[2] * ar[1][2];
        if along.abs() > reach {
            return false;
        }
        let along = ty * r[1][0] - tx * r[1][1];
        let reach = ((ea[0] * ar[1][1] + ea[1] * ar[1][0]) + eb[0] * ar[2][2]) + eb[2] * ar[0][2];
        if along.abs() > reach {
            return false;
        }
        let along = ty * r[2][0] - tx * r[2][1];
        let reach = ((ea[0] * ar[2][1] + ea[1] * ar[2][0]) + eb[0] * ar[1][2]) + eb[1] * ar[0][2];
        if along.abs() > reach {
            return false;
        }
        true
    }

    /// `FETCH_LEAF`: a leaf's triangle into `mLeafVerts`, in the other mesh's space (the
    /// translation is added right after the first product).
    fn fetch_leaf(&mut self, prim: u32, first: bool) {
        self.leaf_index = prim;
        let (mesh, rot, trans) = if first { (self.mesh0, &self.r0to1, &self.t0to1) } else { (self.mesh1, &self.r1to0, &self.t1to0) };
        let tri = mesh.triangle(prim);
        let mut out = [[0.0f32; 3]; 3];
        for k in 0..3 {
            let p = tri[k];
            out[k] = [
                ((p[0] * rot[0][0] + trans[0]) + p[1] * rot[1][0]) + p[2] * rot[2][0],
                ((p[0] * rot[0][1] + trans[1]) + p[1] * rot[1][1]) + p[2] * rot[2][1],
                ((p[0] * rot[0][2] + trans[2]) + p[1] * rot[1][2]) + p[2] * rot[2][2],
            ];
        }
        self.leaf_verts = out;
    }

    /// `PrimTestTriIndex`: the fetched leaf of the first tree against triangle `id1` of the
    /// second mesh.
    fn prim_test_tri_index(&mut self, id1: u32) {
        let [u0, u1, u2] = self.mesh1.triangle(id1);
        if tri_tri_overlap(&self.leaf_verts, &[*u0, *u1, *u2]) {
            self.pairs.push((self.leaf_index, id1));
        }
    }

    /// `PrimTestIndexTri`: triangle `id0` of the first mesh against the fetched leaf of the
    /// second tree. The leaf is still the triangle test's first triangle.
    fn prim_test_index_tri(&mut self, id0: u32) {
        let [u0, u1, u2] = self.mesh0.triangle(id0);
        if tri_tri_overlap(&self.leaf_verts, &[*u0, *u1, *u2]) {
            self.pairs.push((id0, self.leaf_index));
        }
    }

    /// `TriBoxOverlap` (`OPC_TriBoxOverlap.h`), inlined in the two walkers: the fetched leaf
    /// against a node's box.
    fn tri_box_overlap(&self, node: &AabbNoLeafNode) -> bool {
        let (c, e) = (&node.center, &node.extents);
        let lv = &self.leaf_verts;

        // 1) the box's three axes
        let (v0x, v1x, v2x) = (lv[0][0] - c[0], lv[1][0] - c[0], lv[2][0] - c[0]);
        if min3(v0x, v1x, v2x) > e[0] {
            return false;
        }
        if !(max3(v0x, v1x, v2x) >= -e[0]) {
            return false;
        }
        let (v0y, v1y, v2y) = (lv[0][1] - c[1], lv[1][1] - c[1], lv[2][1] - c[1]);
        if min3(v0y, v1y, v2y) > e[1] {
            return false;
        }
        if !(max3(v0y, v1y, v2y) >= -e[1]) {
            return false;
        }
        let (v0z, v1z, v2z) = (lv[0][2] - c[2], lv[1][2] - c[2], lv[2][2] - c[2]);
        if min3(v0z, v1z, v2z) > e[2] {
            return false;
        }
        if !(max3(v0z, v1z, v2z) >= -e[2]) {
            return false;
        }

        // 2) the triangle's plane
        let e0 = [v1x - v0x, v1y - v0y, v1z - v0z];
        let e1 = [v2x - v1x, v2y - v1y, v2z - v1z];
        let n = cross(&e0, &e1);
        let nn = [-n[0], -n[1], -n[2]];
        let d = (nn[0] * v0x + nn[1] * v0y) + nn[2] * v0z;
        // planeBoxOverlap
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

        // 3) the nine cross products of edges and box axes (mFullPrimBoxTest is on: always)
        let (fex, fey, fez) = (e0[0].abs(), e0[1].abs(), e0[2].abs());
        axis_test!(e0[2] * v0y - e0[1] * v0z, e0[2] * v2y - e0[1] * v2z, fez * e[1] + fey * e[2]); // X01
        axis_test!(e0[0] * v0z - e0[2] * v0x, e0[0] * v2z - e0[2] * v2x, fez * e[0] + fex * e[2]); // Y02
        axis_test!(e0[1] * v1x - e0[0] * v1y, e0[1] * v2x - e0[0] * v2y, fey * e[0] + fex * e[1]); // Z12

        let (fex, fey, fez) = (e1[0].abs(), e1[1].abs(), e1[2].abs());
        axis_test!(e1[2] * v0y - e1[1] * v0z, e1[2] * v2y - e1[1] * v2z, fez * e[1] + fey * e[2]); // X01
        axis_test!(e1[0] * v0z - e1[2] * v0x, e1[0] * v2z - e1[2] * v2x, fez * e[0] + fex * e[2]); // Y02
        axis_test!(e1[1] * v0x - e1[0] * v0y, e1[1] * v1x - e1[0] * v1y, fey * e[0] + fex * e[1]); // Z0

        // the third edge is taken from the corners themselves, not from the centre-relative ones
        let e2 = sub(&lv[0], &lv[2]);
        let (fex, fey, fez) = (e2[0].abs(), e2[1].abs(), e2[2].abs());
        axis_test!(e2[2] * v0y - e2[1] * v0z, e2[2] * v1y - e2[1] * v1z, fez * e[1] + fey * e[2]); // X2
        axis_test!(e2[0] * v0z - e2[2] * v0x, e2[0] * v1z - e2[2] * v1x, fez * e[0] + fex * e[2]); // Y1
        axis_test!(e2[1] * v1x - e2[0] * v1y, e2[1] * v2x - e2[0] * v2y, fey * e[0] + fex * e[1]); // Z12
        true
    }

    /// `_CollideTriBox` @ 0x1403632a0: the fetched leaf of the first tree down the second
    /// tree, positive child first.
    fn collide_tri_box(&mut self, mut b: u32) {
        loop {
            let node = self.nodes1[b as usize];
            if !self.tri_box_overlap(&node) {
                return;
            }
            match node.pos {
                Child::Triangle(id) => self.prim_test_tri_index(id),
                Child::Node(child) => self.collide_tri_box(child),
            }
            match node.neg {
                Child::Triangle(id) => {
                    self.prim_test_tri_index(id);
                    return;
                }
                Child::Node(child) => b = child,
            }
        }
    }

    /// `_CollideBoxTri` @ 0x14035f8f0: the fetched leaf of the second tree down the first tree.
    fn collide_box_tri(&mut self, mut a: u32) {
        loop {
            let node = self.nodes0[a as usize];
            if !self.tri_box_overlap(&node) {
                return;
            }
            match node.pos {
                Child::Triangle(id) => self.prim_test_index_tri(id),
                Child::Node(child) => self.collide_box_tri(child),
            }
            match node.neg {
                Child::Triangle(id) => {
                    self.prim_test_index_tri(id);
                    return;
                }
                Child::Node(child) => a = child,
            }
        }
    }

    /// One slot of `a` that is a leaf, against `b`'s positive then negative slot.
    fn leaf_against(&mut self, leaf: u32, b: &AabbNoLeafNode) {
        self.fetch_leaf(leaf, true);
        for slot in [b.pos, b.neg] {
            match slot {
                Child::Triangle(id) => self.prim_test_tri_index(id),
                Child::Node(child) => self.collide_tri_box(child),
            }
        }
    }

    /// `_Collide(const AABBNoLeafNode* a, const AABBNoLeafNode* b)` @ 0x140356e20: a node of
    /// the first tree against a node of the second. The slots are always taken in the order
    /// (a.pos, b.pos), (a.pos, b.neg), (a.neg, b.pos), (a.neg, b.neg).
    fn collide(&mut self, mut a: u32, mut b: u32) {
        loop {
            let (na, nb) = (self.nodes0[a as usize], self.nodes1[b as usize]);
            if !self.box_box_overlap(&na, &nb) {
                return;
            }

            // a's positive slot
            match na.pos {
                Child::Triangle(leaf) => self.leaf_against(leaf, &nb),
                Child::Node(a_pos) => {
                    for slot in [nb.pos, nb.neg] {
                        match slot {
                            Child::Triangle(leaf) => {
                                self.fetch_leaf(leaf, false);
                                self.collide_box_tri(a_pos);
                            }
                            Child::Node(child) => self.collide(a_pos, child),
                        }
                    }
                }
            }

            // a's negative slot
            match na.neg {
                Child::Triangle(leaf) => {
                    self.leaf_against(leaf, &nb);
                    return;
                }
                Child::Node(a_neg) => {
                    match nb.pos {
                        Child::Triangle(leaf) => {
                            // (fetched again, as the game does)
                            self.fetch_leaf(leaf, false);
                            self.collide_box_tri(a_neg);
                        }
                        Child::Node(child) => self.collide(a_neg, child),
                    }
                    match nb.neg {
                        Child::Triangle(leaf) => {
                            self.fetch_leaf(leaf, false);
                            self.collide_box_tri(a_neg);
                            return;
                        }
                        Child::Node(child) => {
                            a = a_neg;
                            b = child;
                        }
                    }
                }
            }
        }
    }
}

/// `AABBTreeCollider::Collide(BVTCache&, world0, world1)` @ 0x140354770 with the game's two
/// "no leaf" trees (`Collide(AABBNoLeafTree*, ..)` @ 0x140354910): the pairs (triangle of the
/// first mesh, triangle of the second) that overlap, in the collider's order. `world0` and
/// `world1` are `MakeMatrix` of the two meshes' places.
///
/// `None`: the query is refused or finds nothing. A mesh of one triangle has no tree and
/// never collides with another mesh (the game refuses the query before anything is computed;
/// two such meshes would crash it).
pub(crate) fn tree_query(
    model0: &Model,
    mesh0: &MeshInterface,
    world0: &Matrix4x4,
    model1: &Model,
    mesh1: &MeshInterface,
    world1: &Matrix4x4,
) -> Option<Vec<(u32, u32)>> {
    if !model0.built || !model1.built || model0.single_node || model1.single_node {
        return None;
    }
    if model0.nodes.is_empty() || model1.nodes.is_empty() {
        return None;
    }

    // InitQuery @ 0x140355630
    let inv_world0 = invert_pr_matrix(world0);
    let inv_world1 = invert_pr_matrix(world1);
    let world0to1 = mul4(world0, &inv_world1);
    let world1to0 = mul4(world1, &inv_world0);
    let rotation = |m: &Matrix4x4| [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let r1to0 = rotation(&world1to0);
    let mut ar = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            ar[i][j] = r1to0[i][j].abs() + EPSILON;
        }
    }
    let mut collider = TreeCollider {
        mesh0,
        mesh1,
        nodes0: &model0.nodes,
        nodes1: &model1.nodes,
        ar,
        r0to1: rotation(&world0to1),
        r1to0,
        t0to1: [world0to1[3][0], world0to1[3][1], world0to1[3][2]],
        t1to0: [world1to0[3][0], world1to0[3][1], world1to0[3][2]],
        leaf_verts: [[0.0; 3]; 3],
        leaf_index: 0,
        pairs: Vec::new(),
    };
    collider.collide(0, 0);
    if collider.pairs.is_empty() {
        // OPC_CONTACT is not set
        None
    } else {
        Some(collider.pairs)
    }
}
