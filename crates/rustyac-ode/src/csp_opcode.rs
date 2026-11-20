// SPDX-License-Identifier: BSD-3-Clause

//! OPCODE's box-against-tree query as Custom Shaders Patch 0.3.2 (build 4305) runs it for ODE's
//! box-trimesh collider: `OBBCollider::Collide` (+0x16cb830), `InitQuery` (+0x16cc420) and the walk
//! of a no-leaf, not quantised tree with primitive tests (+0x16d1890). Single precision like the
//! vanilla port in `opcode_obb.rs`, but CSP's build adds in its own grouping, and CSP queries the
//! box relative to the mesh (centre `box - mesh`, the mesh matrix a rotation only) and keeps
//! OPCODE's temporal coherence on: a box remembers, per mesh, a box 1.1 times its size and the
//! triangles that touched it, and while it stays inside that box the old list is the answer.
//! [`BoxTc`] is that memory: CSP's four-slot block hung on the box geom (+0x16a8110, +0x16a8210).

use crate::opcode::{AabbNoLeafNode, Child, MeshInterface, Model};

/// `OBBCache` (0x70 bytes): `TouchedPrimitives`, `Model` (whether it is the current one),
/// `FatBox`, `FatCoeff`.
#[derive(Clone, Debug, PartialEq)]
pub struct ObbCache {
    pub touched: Vec<u32>,
    /// `cache.Model == the queried model`
    pub valid: bool,
    pub fat_center: [f32; 3],
    pub fat_extents: [f32; 3],
    pub fat_rot: [[f32; 3]; 3],
    pub fat_coeff: f32,
}

impl Default for ObbCache {
    fn default() -> Self {
        ObbCache {
            touched: Vec::new(),
            valid: false,
            fat_center: [0.0; 3],
            fat_extents: [0.0; 3],
            fat_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            fat_coeff: 1.1,
        }
    }
}

/// One slot of CSP's block: the cache, the mesh geom it is for (+0x60, 0 = free) and the mesh
/// data's key (+0x68).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TcSlot {
    pub cache: ObbCache,
    pub trimesh: usize,
    pub key: u32,
}

/// CSP's per-box block (0x1e0 bytes): four slots, the next one to reuse at +0x1d8.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxTc {
    pub slots: [TcSlot; 4],
    pub next: u32,
}

impl BoxTc {
    /// +0x16a8210: the slot of a mesh; a new mesh takes the next slot round robin, a changed key
    /// drops the cached list (the fat coefficient stays).
    pub fn slot(&mut self, trimesh: usize, key: u32, fat: f32) -> &mut ObbCache {
        if let Some(i) = self.slots.iter().position(|s| s.trimesh == trimesh) {
            let s = &mut self.slots[i];
            if s.key != key {
                s.key = key;
                s.cache.valid = false;
            }
            return &mut self.slots[i].cache;
        }
        let i = self.next as usize;
        self.next = (self.next + 1) % 4;
        let s = &mut self.slots[i];
        s.trimesh = trimesh;
        s.key = key;
        s.cache.valid = false;
        s.cache.fat_coeff = fat;
        &mut s.cache
    }
}

type M3 = [[f32; 3]; 3];
type M4 = [[f32; 4]; 4];

/// `InvertPRMatrix` (+0x1704140).
fn invert_pr(src: &M4) -> M4 {
    let mut d = [[0.0f32; 4]; 4];
    for j in 0..3 {
        d[0][j] = src[j][0];
        d[1][j] = src[j][1];
        d[2][j] = src[j][2];
        d[3][j] = -((src[3][0] * src[j][0] + src[j][1] * src[3][1]) + src[3][2] * src[j][2]);
    }
    d[3][3] = 1.0;
    d
}

/// `Matrix4x4::operator*` as CSP's build groups it.
fn mul4(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (a[i][3] * b[3][j] + a[i][2] * b[2][j]) + (a[i][0] * b[0][j] + a[i][1] * b[1][j]);
        }
    }
    r
}

fn rot(m: &M4) -> M3 {
    [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]]
}

/// `OBB::IsInside` (+0x1704220): the box (`c`, `e`, `r`) inside the fat box.
fn is_inside(c: &[f32; 3], e: &[f32; 3], r: &M3, fat: &ObbCache) -> bool {
    let b = &fat.fat_rot;
    for j in 0..3 {
        let cj = ((c[1] * b[j][1] + c[0] * b[j][0]) + c[2] * b[j][2])
            - ((fat.fat_center[1] * b[j][1] + fat.fat_center[0] * b[j][0]) + fat.fat_center[2] * b[j][2]);
        let rr = |i: usize| (r[i][1] * b[j][1] + r[i][0] * b[j][0]) + r[i][2] * b[j][2];
        let f = (((rr(1) * e[1]).abs() + (rr(0) * e[0]).abs()) + (rr(2) * e[2]).abs()) - fat.fat_extents[j];
        if f > cj || !(-f >= cj) {
            return false;
        }
    }
    true
}

/// `minss a, b`
#[inline]
fn minss(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

/// `maxss a, b`
#[inline]
fn maxss(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

struct Query<'a> {
    mesh: &'a MeshInterface,
    model: &'a Model,
    full_box_box: bool,
    nb_bv: u32,
    ar: M3,
    m: M3,
    r: M3,
    t_mb: [f32; 3],
    t_bm: [f32; 3],
    e: [f32; 3],
    b0: [f32; 3],
    b1: [f32; 3],
    bb1: [f32; 3],
    bb: [f32; 9],
    contact: bool,
    touched: Vec<u32>,
}

impl Query<'_> {
    /// One leaf: the triangle into box space (the positive child's grouping, or the negative
    /// child's, whose x row differs) and the triangle-box overlap test.
    fn prim(&mut self, tri: u32, neg: bool) {
        let [p0, p1, p2] = self.mesh.triangle(tri);
        let (m, t) = (&self.m, &self.t_mb);
        let tr = |p: &[f32; 3], j: usize| -> f32 {
            if neg && j == 0 {
                (p[1] * m[1][0] + m[0][0] * p[0]) + (m[2][0] * p[2] + t[0])
            } else {
                (m[2][j] * p[2] + m[1][j] * p[1]) + (m[0][j] * p[0] + t[j])
            }
        };
        let v = [
            [tr(p0, 0), tr(p0, 1), tr(p0, 2)],
            [tr(p1, 0), tr(p1, 1), tr(p1, 2)],
            [tr(p2, 0), tr(p2, 1), tr(p2, 2)],
        ];
        if self.tri_box(&v) {
            self.contact = true;
            self.touched.push(tri);
        }
    }

    fn tri_box(&self, v: &[[f32; 3]; 3]) -> bool {
        let e = &self.e;
        let (v0, v1, v2) = (&v[0], &v[1], &v[2]);
        for q in 0..3 {
            let mn = if v0[q] >= v1[q] { minss(v2[q], v1[q]) } else { minss(v2[q], v0[q]) };
            if !(mn <= e[q]) {
                return false;
            }
            let mx = if v0[q] <= v1[q] { maxss(v2[q], v1[q]) } else { maxss(v2[q], v0[q]) };
            if !(mx >= -e[q]) {
                return false;
            }
        }
        let e0 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e1 = [v2[0] - v1[0], v2[1] - v1[1], v2[2] - v1[2]];
        let n = [e1[2] * e0[1] - e0[2] * e1[1], e0[2] * e1[0] - e1[2] * e0[0], e1[1] * e0[0] - e1[0] * e0[1]];
        let d = (v0[0] * n[0] + v0[1] * n[1]) + n[2] * v0[2];
        let mut vmin = [0.0f32; 3];
        let mut vmax = [0.0f32; 3];
        for q in 0..3 {
            if n[q] <= 0.0 {
                vmin[q] = e[q];
                vmax[q] = -e[q];
            } else {
                vmin[q] = -e[q];
                vmax[q] = e[q];
            }
        }
        if !((((n[1] * vmin[1] + n[0] * vmin[0]) + n[2] * vmin[2]) - d) <= 0.0) {
            return false;
        }
        if !((((vmax[1] * n[1] + vmax[0] * n[0]) + vmax[2] * n[2]) - d) >= 0.0) {
            return false;
        }
        #[inline]
        fn axis(mut min: f32, mut max: f32, rad: f32) -> bool {
            if min > max {
                std::mem::swap(&mut min, &mut max);
            }
            min <= rad && max >= -rad
        }
        let (fx, fy, fz) = (e0[0].abs(), e0[1].abs(), e0[2].abs());
        if !axis(e0[2] * v0[1] - e0[1] * v0[2], e0[2] * v2[1] - e0[1] * v2[2], e[1] * fz + e[2] * fy)
            || !axis(e0[0] * v0[2] - e0[2] * v0[0], e0[0] * v2[2] - e0[2] * v2[0], e[2] * fx + fz * e[0])
            || !axis(e0[1] * v1[0] - e0[0] * v1[1], e0[1] * v2[0] - e0[0] * v2[1], e[1] * fx + e[0] * fy)
        {
            return false;
        }
        let (fx, fy, fz) = (e1[0].abs(), e1[1].abs(), e1[2].abs());
        if !axis(e1[2] * v0[1] - e1[1] * v0[2], e1[2] * v2[1] - e1[1] * v2[2], e[1] * fz + e[2] * fy)
            || !axis(e1[0] * v0[2] - e1[2] * v0[0], e1[0] * v2[2] - e1[2] * v2[0], fx * e[2] + fz * e[0])
            || !axis(e1[1] * v0[0] - e1[0] * v0[1], e1[1] * v1[0] - e1[0] * v1[1], fx * e[1] + e[0] * fy)
        {
            return false;
        }
        let e2 = [v0[0] - v2[0], v0[1] - v2[1], v0[2] - v2[2]];
        let (fx, fy, fz) = (e2[0].abs(), e2[1].abs(), e2[2].abs());
        axis(e2[2] * v0[1] - e2[1] * v0[2], e2[2] * v1[1] - e2[1] * v1[2], e[1] * fz + e[2] * fy)
            && axis(e2[0] * v0[2] - e2[2] * v0[0], e2[0] * v1[2] - e2[2] * v1[0], e[2] * fx + fz * e[0])
            && axis(e2[1] * v1[0] - e2[0] * v1[1], e2[1] * v2[0] - e2[0] * v2[1], e[1] * fx + e[0] * fy)
    }

    /// Box-box test of a node (classes I, II, and III when the full test is on or on the first
    /// node of the query); "strictly greater" rejects, so a NaN never does.
    fn overlap(&mut self, n: &AabbNoLeafNode) -> bool {
        self.nb_bv += 1;
        let (c, e, r, ar, be) = (&n.center, &n.extents, &self.r, &self.ar, &self.e);
        let tx = self.t_bm[0] - c[0];
        if tx.abs() > e[0] + self.bb1[0] {
            return false;
        }
        let ty = self.t_bm[1] - c[1];
        if ty.abs() > e[1] + self.bb1[1] {
            return false;
        }
        let tz = self.t_bm[2] - c[2];
        if tz.abs() > e[2] + self.bb1[2] {
            return false;
        }
        for k in 0..3 {
            let t = (r[k][0] * tx + r[k][1] * ty) + r[k][2] * tz;
            if t.abs() > (e[0] * ar[k][0] + e[1] * ar[k][1]) + (e[2] * ar[k][2] + be[k]) {
                return false;
            }
        }
        if self.full_box_box || self.nb_bv == 1 {
            for k in 0..3 {
                if (r[k][1] * tz - r[k][2] * ty).abs() > (e[1] * ar[k][2] + e[2] * ar[k][1]) + self.bb[k] {
                    return false;
                }
            }
            for k in 0..3 {
                if (r[k][2] * tx - r[k][0] * tz).abs() > (e[0] * ar[k][2] + e[2] * ar[k][0]) + self.bb[3 + k] {
                    return false;
                }
            }
            for k in 0..3 {
                if (r[k][0] * ty - r[k][1] * tx).abs() > (e[0] * ar[k][1] + e[1] * ar[k][0]) + self.bb[6 + k] {
                    return false;
                }
            }
        }
        true
    }

    fn contains(&self, n: &AabbNoLeafNode) -> bool {
        let (c, e, m) = (&n.center, &n.extents, &self.m);
        for j in 0..3 {
            let nc = (m[1][j] * c[1] + c[0] * m[0][j]) + c[2] * m[2][j];
            let ne = ((e[0] * m[0][j]).abs() + (e[1] * m[1][j]).abs()) + (m[2][j] * e[2]).abs();
            if !((ne + nc) <= self.b0[j] && (nc - ne) >= self.b1[j]) {
                return false;
            }
        }
        true
    }

    /// `_Dump` (+0x1703ea0): every triangle below, positive side first.
    fn dump(&mut self, node: u32) {
        let n = self.model.nodes[node as usize];
        for c in [n.pos, n.neg] {
            match c {
                Child::Triangle(t) => self.touched.push(t),
                Child::Node(i) => self.dump(i),
            }
        }
    }

    /// The walk (+0x16d1890).
    fn walk(&mut self, node: u32) {
        let n = self.model.nodes[node as usize];
        if !self.overlap(&n) {
            return;
        }
        if self.contains(&n) {
            self.contact = true;
            self.dump(node);
            return;
        }
        match n.pos {
            Child::Triangle(t) => self.prim(t, false),
            Child::Node(i) => self.walk(i),
        }
        match n.neg {
            Child::Triangle(t) => self.prim(t, true),
            Child::Node(i) => self.walk(i),
        }
    }
}

/// The query of CSP's box-trimesh collider (+0x16b18c0 then `OBBCollider::Collide`): the box at
/// `center` (already `box - mesh` in f32) with half sizes `extents` and ODE rotation `box_r`,
/// against the mesh rotated by `mesh_r` (both rotations already f32). `tc` is the box's cache slot
/// for this mesh when temporal coherence is on (the mesh's `doBoxTC`). `full_box_box` is the
/// collider's +0x144. Returns OPCODE's contact flag and the touched triangles in order.
#[allow(clippy::too_many_arguments)]
pub fn obb_query(
    center: &[f32; 3],
    extents: &[f32; 3],
    box_r: &[f32; 12],
    mesh_r: &[f32; 12],
    model: &Model,
    mesh: &MeshInterface,
    tc: Option<&mut ObbCache>,
    full_box_box: bool,
) -> (bool, Vec<u32>) {
    let mk = |p: &[f32; 3], r: &[f32; 12]| -> M4 {
        [[r[0], r[4], r[8], 0.0], [r[1], r[5], r[9], 0.0], [r[2], r[6], r[10], 0.0], [p[0], p[1], p[2], 1.0]]
    };
    let world_b = mk(center, box_r);
    let world_m = mk(&[0.0; 3], mesh_r);
    let b_to_m = mul4(&world_b, &invert_pr(&world_m));
    let m_to_b = mul4(&world_m, &invert_pr(&world_b));
    let mut q = Query {
        mesh,
        model,
        full_box_box,
        nb_bv: 0,
        ar: [[0.0; 3]; 3],
        m: rot(&m_to_b),
        r: rot(&b_to_m),
        t_mb: [m_to_b[3][0], m_to_b[3][1], m_to_b[3][2]],
        t_bm: [b_to_m[3][0], b_to_m[3][1], b_to_m[3][2]],
        e: *extents,
        b0: [0.0; 3],
        b1: [0.0; 3],
        bb1: [0.0; 3],
        bb: [0.0; 9],
        contact: false,
        touched: Vec::new(),
    };
    // shortcut: a one-triangle mesh (no tree) is tested with the positive leaf's grouping, unchecked
    // against CSP (no such mesh on Spa); check it when a track has one.
    if model.single_node {
        q.prim(0, false);
        if let Some(c) = tc {
            c.touched = q.touched.clone();
        }
        return (q.contact, q.touched);
    }
    let mut tc = tc;
    if let Some(c) = tc.as_deref_mut() {
        let was_valid = c.valid;
        c.valid = true;
        if was_valid && is_inside(&q.t_bm, &q.e, &q.r, c) {
            return (!c.touched.is_empty(), c.touched.clone());
        }
        let f = c.fat_coeff;
        q.e = [q.e[0] * f, q.e[1] * f, q.e[2] * f];
        c.fat_center = q.t_bm;
        c.fat_extents = q.e;
        c.fat_rot = q.r;
    }
    for i in 0..3 {
        for j in 0..3 {
            q.ar[i][j] = q.r[i][j].abs() + f32::from_bits(0x3586_37bd); // 1e-6
        }
    }
    let (e, t, ar) = (q.e, q.t_mb, q.ar);
    q.b0 = [e[0] - t[0], e[1] - t[1], e[2] - t[2]];
    q.b1 = [(-e[0]) - t[0], (-e[1]) - t[1], (-e[2]) - t[2]];
    for j in 0..3 {
        q.bb1[j] = (e[0] * ar[0][j] + e[1] * ar[1][j]) + e[2] * ar[2][j];
        q.bb[3 * j] = e[1] * ar[2][j] + e[2] * ar[1][j];
        q.bb[3 * j + 1] = e[0] * ar[2][j] + e[2] * ar[0][j];
        q.bb[3 * j + 2] = e[0] * ar[1][j] + e[1] * ar[0][j];
    }
    if !model.nodes.is_empty() {
        q.walk(0);
    }
    if let Some(c) = tc {
        c.touched = q.touched.clone();
    }
    (q.contact, q.touched)
}

/// CSP's trimesh `AABBTest` (+0x1693970) once its gates passed (`TRIMESH_AABB_TEST` = `limit` not
/// 0, the other geom without flag 0x80000, a no-leaf tree, the mesh at the origin unrotated): does
/// the box `aabb` (min x, max x, min y, ...) reach the tree's root box, or a node `limit` levels
/// down, or a node with a triangle child? Centres and half sizes in f64, the tree's f32 widened;
/// the comparisons as CSP compiled them (a NaN mostly passes).
pub fn aabb_test(nodes: &[AabbNoLeafNode], aabb: &[f64; 6], limit: i32) -> bool {
    let Some(root) = nodes.first() else { return true };
    let c = [(aabb[0] + aabb[1]) * 0.5, (aabb[2] + aabb[3]) * 0.5, (aabb[4] + aabb[5]) * 0.5];
    let e = [(aabb[1] - aabb[0]) * 0.5, (aabb[3] - aabb[2]) * 0.5, (aabb[5] - aabb[4]) * 0.5];
    if (0..3).all(|k| {
        !(c[k] - e[k] > (root.center[k] - root.extents[k]) as f64) && e[k] + c[k] >= (root.center[k] + root.extents[k]) as f64
    }) {
        return true;
    }
    let mut stack = vec![(0u32, 0)];
    while let Some((index, depth)) = stack.pop() {
        let node = &nodes[index as usize];
        if (0..3).all(|k| !((c[k] - node.center[k] as f64).abs() > node.extents[k] as f64 + e[k])) {
            if depth >= limit {
                return true;
            }
            match (node.pos, node.neg) {
                (Child::Node(pos), Child::Node(neg)) => {
                    stack.push((pos, depth + 1));
                    stack.push((neg, depth + 1));
                }
                _ => return true,
            }
        }
    }
    false
}
