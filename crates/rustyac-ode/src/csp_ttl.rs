// SPDX-License-Identifier: BSD-3-Clause

//! Triangle mesh against triangle mesh as Custom Shaders Patch 0.3.2 (build 4305) runs it:
//! `dCollideTTL` (+0x16bc1b0) in f64 and OPCODE's tree-against-tree query (+0x16dfb50) in CSP's
//! build. The vanilla f32 port is `collide_ttl.rs` / `opcode_tree.rs`; this one differs in
//! precision, in CSP's OBB prefilter (`TRIMESH_OBB_PREFILTER`: the smaller mesh's box against the
//! bigger mesh's tree first), in the sums' grouping, in the tree collider's settings (the full
//! box-box test only at the root, the triangle-box test's nine edge axes behind a switch) and in
//! the contact merge (normals cancelling keep the old contact).

// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

use crate::csp_btl::{fetch_triangle, ContactGeom, Mesh};
use crate::csp_opcode::{invert_pr, mul4, obb_query};
use crate::odemath::safe_normalize3;
use crate::opcode::{AabbNoLeafNode, Child, MeshInterface};
use crate::opcode_tree::tri_tri_overlap;

type V3 = [f64; 3];
type P = [f32; 3];
type M3 = [[f32; 3]; 3];

const MAX_POINTS: usize = 8;
/// 1.00001 * sqrt(3) / 10000
const SAME_CONTACT_DISTANCE: f64 = 0.0001732068128076953;
/// `mAR`'s epsilon: 1e-6f
const EPSILON: f32 = f32::from_bits(0x3586_37bd);

/// +0x16bc980: the unit normal and distance of a triangle's plane, `None` for no normal.
fn plane(t: &[V3; 3]) -> Option<(V3, f64)> {
    let (a, b, c) = (&t[0], &t[1], &t[2]);
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let w = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let mut n = [v[2] * w[1] - v[1] * w[2], v[0] * w[2] - v[2] * w[0], v[1] * w[0] - v[0] * w[1]];
    if !safe_normalize3::<f64>(&mut n) {
        return None;
    }
    Some((n, (a[0] * n[0] + n[1] * a[1]) + a[2] * n[2]))
}

/// +0x16bc5f0: where p1 -> p2 crosses the plane.
fn clip_segment(p1: &V3, p2: &V3, n: &V3, d: f64) -> V3 {
    let dir = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let t = (d - ((p1[0] * n[0] + p1[1] * n[1]) + p1[2] * n[2])) / ((dir[1] * n[1] + dir[0] * n[0]) + dir[2] * n[2]);
    [t * dir[0] + p1[0], t * dir[1] + p1[1], t * dir[2] + p1[2]]
}

/// +0x16bc6f0: the part of the polygon behind the plane, at most eight points.
fn clip_polygon(n: &V3, d: f64, pts: &mut Vec<V3>) {
    let count = pts.len();
    if count == 0 {
        return;
    }
    let mut out = Vec::with_capacity(MAX_POINTS);
    let mut prev = 32000;
    for i in 0..=count {
        let p = pts[i % count];
        let front = ((p[1] * n[1] + p[0] * n[0]) + p[2] * n[2]) - d > 1e-8;
        if !front {
            if i > 0 && prev == 1 {
                if out.len() >= MAX_POINTS {
                    prev = 0;
                    continue;
                }
                out.push(clip_segment(&pts[i - 1], &p, n, d));
            }
            if out.len() < MAX_POINTS && i < count {
                out.push(p);
            }
        } else if i > 0 && prev == 0 && out.len() < MAX_POINTS {
            out.push(clip_segment(&pts[i - 1], &p, n, d));
        }
        prev = front as i32;
    }
    *pts = out;
}

/// +0x16bcb80: `pts` clipped by the three planes through `tri`'s edges along its normal `n`.
fn clip_by_tri(pts: &[V3; 3], tri: &[V3; 3], n: &V3) -> Vec<V3> {
    let mut out = pts.to_vec();
    for i in 0..3 {
        let (a, b) = (&tri[i], &tri[(i + 1) % 3]);
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let mut en = [e[1] * n[2] - e[2] * n[1], e[2] * n[0] - e[0] * n[2], e[0] * n[1] - e[1] * n[0]];
        if safe_normalize3::<f64>(&mut en) {
            let d = (en[0] * a[0] + en[1] * a[1]) + en[2] * a[2];
            clip_polygon(&en, d, &mut out);
        }
    }
    out
}

/// The points deepest behind the plane (within 1e-6 of the deepest so far) and their depth.
fn deepest(pts: &[V3], n: &V3, d: f64) -> (f64, Vec<V3>) {
    let mut max = -1e300;
    let mut list = Vec::new();
    for p in pts {
        let dist = d - ((n[1] * p[1] + n[0] * p[0]) + n[2] * p[2]);
        if dist > max {
            max = dist;
            list.clear();
            list.push(*p);
        } else if dist + 1e-6 >= max {
            list.push(*p);
        }
    }
    (max, list)
}

/// +0x16bce00 without a body hint: depth (below zero: none), normal and the deepest points.
fn tri_tri(t1: &[V3; 3], t2: &[V3; 3]) -> (f64, V3, Vec<V3>) {
    let (mut max1, mut n1, mut pts1) = (1e300, [0.0; 3], Vec::new());
    if let Some((n, d)) = plane(t1) {
        (max1, pts1) = deepest(&clip_by_tri(t2, t1, &n), &n, d);
        n1 = n;
    }
    if let Some((n, d)) = plane(t2) {
        let (max2, pts2) = deepest(&clip_by_tri(t1, t2, &n), &n, d);
        if !(max2 >= max1) {
            return (max2, n, pts2);
        }
    }
    (max1, [-n1[0], -n1[1], -n1[2]], pts1)
}

/// +0x16bbbc0's cell hash of a point (0.1 mm cells).
fn key(p: &V3) -> u32 {
    let mut x: u32 = 0;
    for (i, v) in p.iter().enumerate() {
        // shortcut: f64 floor, CSP's own (int truncation) agrees within ±2^31 cells (±214 km)
        let bits = (v * 10000.0).floor().to_bits();
        let h = (bits as u32) ^ ((bits >> 32) as u32);
        for byte in [h >> 24, (h >> 16) & 0xff, (h >> 8) & 0xff, h & 0xff] {
            x = (x >> 28) ^ (x << 4).wrapping_add(byte);
        }
        if i < 2 {
            x = x.rotate_right(21);
        }
    }
    x
}

fn index(k: u32) -> usize {
    let i = k ^ (k >> 16);
    ((i ^ (i >> 8)) & 0xff) as usize
}

/// The call's contacts with CSP's hash set (256 nodes of 4 keys) and each normal's length.
struct Set {
    max: usize,
    out: Vec<ContactGeom>,
    lens: Vec<f64>,
    nodes: Vec<Vec<(usize, u32)>>,
    g1: usize,
    g2: usize,
}

impl Set {
    /// +0x16bbbc0 / +0x16bb960 for one point.
    fn add(&mut self, tri1: i32, tri2: i32, p: &V3, n: &V3, depth: f64) {
        let k = key(p);
        let node = &mut self.nodes[index(k)];
        let found = node.iter().find(|&&(c, ck)| {
            let f = self.out[c].pos;
            let (dy, dx, dz) = (f[1] - p[1], f[0] - p[0], f[2] - p[2]);
            ck == k && ((dy * dy + dx * dx) + dz * dz).sqrt() < SAME_CONTACT_DISTANCE
        });
        let Some(&(c, _)) = found else {
            if self.out.len() == self.max {
                // the full array's stand-in leaves the node again: the point is dropped
                return;
            }
            if node.len() < 4 {
                node.push((self.out.len(), k));
            }
            self.out.push(ContactGeom { pos: *p, normal: *n, depth, g1: self.g1, g2: self.g2, side1: tri1, side2: tri2 });
            self.lens.push(1.0);
            return;
        };
        let (old, len) = (&mut self.out[c], &mut self.lens[c]);
        let dd = depth - old.depth;
        if dd > 1e-5 {
            (old.normal, old.depth, old.side1, old.side2, old.g1, old.g2) = (*n, depth, tri1, tri2, self.g1, self.g2);
            *len = 1.0;
        } else if dd >= -1e-5 {
            // (CSP turns the new normal round when the old contact's g1 is this call's g2: never within one call)
            let m = [old.normal[0] * *len + n[0], old.normal[1] * *len + n[1], old.normal[2] * *len + n[2]];
            let l = ((m[0] * m[0] + m[1] * m[1]) + m[2] * m[2]).sqrt();
            if l > 1e-7 {
                let inv = 1.0 / l;
                old.normal = [inv * m[0], inv * m[1], inv * m[2]];
                *len = l;
                (old.side1, old.side2) = (-1, -1);
            }
        }
    }
}

/// CSP's `AABBTreeCollider` (TLS collider +20016): first contact and temporal coherence off, the
/// full box-box test off (so only the root's), the full triangle-box test on with its nine edge
/// axes only when `edges` (byte +0x27f91a6).
struct Tree<'a> {
    mesh0: &'a MeshInterface,
    mesh1: &'a MeshInterface,
    nodes0: &'a [AabbNoLeafNode],
    nodes1: &'a [AabbNoLeafNode],
    ar: M3,
    r0to1: M3,
    r1to0: M3,
    t0to1: P,
    t1to0: P,
    tests: u32,
    edges: bool,
    /// the fetched leaf (in the other mesh's space), its number, its edges and plane
    v: [P; 3],
    leaf: u32,
    e: [P; 3],
    n: P,
    d: f32,
    pairs: Vec<(u32, u32)>,
}

macro_rules! axis {
    ($a:expr, $b:expr, $rad:expr) => {{
        let (mut min, mut max): (f32, f32) = ($a, $b);
        let rad: f32 = $rad;
        if min > max {
            std::mem::swap(&mut min, &mut max);
        }
        if min > rad || !(max >= -rad) {
            return false;
        }
    }};
}

impl Tree<'_> {
    /// `_Collide`'s inlined box-box test, B (second tree) brought into A's space.
    fn box_box(&mut self, a: &AabbNoLeafNode, b: &AabbNoLeafNode) -> bool {
        self.tests += 1;
        let (ca, ea, cb, eb) = (&a.center, &a.extents, &b.center, &b.extents);
        let (r, ar, t) = (&self.r1to0, &self.ar, &self.t1to0);
        let mut tt = [0.0f32; 3];
        for j in 0..3 {
            tt[j] = ((r[1][j] * cb[1] + r[0][j] * cb[0]) + (r[2][j] * cb[2] + t[j])) - ca[j];
            let reach = (ar[1][j] * eb[1] + ar[2][j] * eb[2]) + (ar[0][j] * eb[0] + ea[j]);
            if tt[j].abs() > reach {
                return false;
            }
        }
        let [tx, ty, tz] = tt;
        for k in 0..3 {
            let along = (r[k][1] * ty + r[k][0] * tx) + tz * r[k][2];
            let reach = (ea[1] * ar[k][1] + ar[k][0] * ea[0]) + (ar[k][2] * ea[2] + eb[k]);
            if along.abs() > reach {
                return false;
            }
        }
        if self.tests != 1 {
            return true;
        }
        let tests = [
            (tz * r[0][1] - r[0][2] * ty, (ar[0][1] * ea[2] + ar[0][2] * ea[1]) + (ar[2][0] * eb[1] + ar[1][0] * eb[2])),
            (tz * r[1][1] - r[1][2] * ty, (ar[1][1] * ea[2] + ar[1][2] * ea[1]) + (ar[2][0] * eb[0] + ar[0][0] * eb[2])),
            (tz * r[2][1] - r[2][2] * ty, (ar[2][1] * ea[2] + ar[2][2] * ea[1]) + (ar[1][0] * eb[0] + ar[0][0] * eb[1])),
            (r[0][2] * tx - tz * r[0][0], (ar[0][0] * ea[2] + ar[0][2] * ea[0]) + (ar[2][1] * eb[1] + ar[1][1] * eb[2])),
            (r[1][2] * tx - tz * r[1][0], (ar[1][0] * ea[2] + ar[1][2] * ea[0]) + (ar[2][1] * eb[0] + ar[0][1] * eb[2])),
            (r[2][2] * tx - tz * r[2][0], (ar[2][0] * ea[2] + ar[2][2] * ea[0]) + (ar[1][1] * eb[0] + ar[0][1] * eb[1])),
            (r[0][0] * ty - r[0][1] * tx, (ea[1] * ar[0][0] + ar[0][1] * ea[0]) + (ar[2][2] * eb[1] + ar[1][2] * eb[2])),
            (r[1][0] * ty - r[1][1] * tx, (ea[1] * ar[1][0] + ar[1][1] * ea[0]) + (ar[2][2] * eb[0] + ar[0][2] * eb[2])),
            (r[2][0] * ty - r[2][1] * tx, (ea[1] * ar[2][0] + ar[2][1] * ea[0]) + (ar[1][2] * eb[0] + ar[0][2] * eb[1])),
        ];
        tests.iter().all(|(along, reach)| !(along.abs() > *reach))
    }

    /// The leaf fetch and its cached edges and plane.
    fn fetch(&mut self, prim: u32, first: bool) {
        self.leaf = prim;
        let (mesh, m, t) = if first { (self.mesh0, &self.r0to1, &self.t0to1) } else { (self.mesh1, &self.r1to0, &self.t1to0) };
        let tri = mesh.triangle(prim);
        for k in 0..3 {
            let p = tri[k];
            self.v[k] = [
                (p[1] * m[1][0] + p[0] * m[0][0]) + (p[2] * m[2][0] + t[0]),
                (p[2] * m[2][1] + p[1] * m[1][1]) + (p[0] * m[0][1] + t[1]),
                (p[2] * m[2][2] + p[1] * m[1][2]) + (p[0] * m[0][2] + t[2]),
            ];
        }
        let v = &self.v;
        let s = |a: &P, b: &P| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        self.e = [s(&v[1], &v[0]), s(&v[2], &v[1]), s(&v[0], &v[2])];
        let (e0, e1) = (&self.e[0], &self.e[1]);
        self.n = [e1[2] * e0[1] - e1[1] * e0[2], e1[0] * e0[2] - e1[2] * e0[0], e1[1] * e0[0] - e1[0] * e0[1]];
        self.d = (self.n[1] * v[0][1] + self.n[0] * v[0][0]) + self.n[2] * v[0][2];
    }

    /// The walkers' inlined triangle-box test: the fetched leaf against a node's box.
    fn tri_box(&self, node: &AabbNoLeafNode) -> bool {
        let (c, e) = (&node.center, &node.extents);
        let mut v = [[0.0f32; 3]; 3];
        for k in 0..3 {
            let (v0, v1, v2) = (self.v[0][k] - c[k], self.v[1][k] - c[k], self.v[2][k] - c[k]);
            let min = if v0 >= v1 { v2.min(v1) } else { v2.min(v0) };
            if min > e[k] {
                return false;
            }
            let max = if v0 <= v1 { v2.max(v1) } else { v2.max(v0) };
            if max < -e[k] {
                return false;
            }
            (v[0][k], v[1][k], v[2][k]) = (v0, v1, v2);
        }
        let n = &self.n;
        if (((c[0] * n[0] + c[1] * n[1]) + c[2] * n[2]) - self.d).abs() > (e[1] * n[1].abs() + e[0] * n[0].abs()) + e[2] * n[2].abs() {
            return false;
        }
        if !self.edges {
            return true;
        }
        let [v0, v1, v2] = &v;
        // each edge's three axes take two of the corners: (a, b) for x and y, (c, d) for z
        for (k, (a, b, cc, dd)) in [(v0, v2, v1, v2), (v0, v2, v0, v1), (v0, v1, v1, v2)].into_iter().enumerate() {
            let ed = &self.e[k];
            let f = [ed[0].abs(), ed[1].abs(), ed[2].abs()];
            axis!(ed[2] * a[1] - ed[1] * a[2], ed[2] * b[1] - ed[1] * b[2], f[2] * e[1] + f[1] * e[2]);
            axis!(ed[0] * a[2] - ed[2] * a[0], ed[0] * b[2] - ed[2] * b[0], f[0] * e[2] + e[0] * f[2]);
            axis!(ed[1] * cc[0] - ed[0] * cc[1], ed[1] * dd[0] - ed[0] * dd[1], e[1] * f[0] + e[0] * f[1]);
        }
        true
    }

    fn prim_tri_index(&mut self, id1: u32) {
        let [u0, u1, u2] = self.mesh1.triangle(id1);
        if tri_tri_overlap(&self.v, &[*u0, *u1, *u2]) {
            self.pairs.push((self.leaf, id1));
        }
    }

    fn prim_index_tri(&mut self, id0: u32) {
        let [u0, u1, u2] = self.mesh0.triangle(id0);
        if tri_tri_overlap(&self.v, &[*u0, *u1, *u2]) {
            self.pairs.push((id0, self.leaf));
        }
    }

    /// +0x16e20c0: the first tree's leaf down the second tree.
    fn tri_down1(&mut self, mut b: u32) {
        loop {
            let node = self.nodes1[b as usize];
            if !self.tri_box(&node) {
                return;
            }
            match node.pos {
                Child::Triangle(id) => self.prim_tri_index(id),
                Child::Node(child) => self.tri_down1(child),
            }
            match node.neg {
                Child::Triangle(id) => return self.prim_tri_index(id),
                Child::Node(child) => b = child,
            }
        }
    }

    /// +0x16e3cf0: the second tree's leaf down the first tree.
    fn tri_down0(&mut self, mut a: u32) {
        loop {
            let node = self.nodes0[a as usize];
            if !self.tri_box(&node) {
                return;
            }
            match node.pos {
                Child::Triangle(id) => self.prim_index_tri(id),
                Child::Node(child) => self.tri_down0(child),
            }
            match node.neg {
                Child::Triangle(id) => return self.prim_index_tri(id),
                Child::Node(child) => a = child,
            }
        }
    }

    fn leaf_against(&mut self, leaf: u32, b: &AabbNoLeafNode) {
        self.fetch(leaf, true);
        for slot in [b.pos, b.neg] {
            match slot {
                Child::Triangle(id) => self.prim_tri_index(id),
                Child::Node(child) => self.tri_down1(child),
            }
        }
    }

    fn node_against_leaf(&mut self, a: u32, leaf: u32) {
        self.fetch(leaf, false);
        self.tri_down0(a);
    }

    /// +0x16e5900: node against node, slots (a.pos, b.pos), (a.pos, b.neg), (a.neg, b.pos), (a.neg, b.neg);
    /// but when both of a's children are nodes and b has a leaf, b's slots come first (+0x16e6032,
    /// +0x16e5fe8): each b leaf fetched once and walked down a.pos then a.neg.
    fn collide(&mut self, mut a: u32, mut b: u32) {
        loop {
            let (na, nb) = (self.nodes0[a as usize], self.nodes1[b as usize]);
            if !self.box_box(&na, &nb) {
                return;
            }
            if let (Child::Node(ap), Child::Node(an)) = (na.pos, na.neg) {
                if !matches!((nb.pos, nb.neg), (Child::Node(_), Child::Node(_))) {
                    for slot in [nb.pos, nb.neg] {
                        match slot {
                            Child::Triangle(leaf) => {
                                self.fetch(leaf, false);
                                self.tri_down0(ap);
                                self.tri_down0(an);
                            }
                            Child::Node(child) => {
                                self.collide(ap, child);
                                self.collide(an, child);
                            }
                        }
                    }
                    return;
                }
            }
            match na.pos {
                Child::Triangle(leaf) => self.leaf_against(leaf, &nb),
                Child::Node(ap) => {
                    for slot in [nb.pos, nb.neg] {
                        match slot {
                            Child::Triangle(leaf) => self.node_against_leaf(ap, leaf),
                            Child::Node(child) => self.collide(ap, child),
                        }
                    }
                }
            }
            match na.neg {
                Child::Triangle(leaf) => return self.leaf_against(leaf, &nb),
                Child::Node(an) => {
                    match nb.pos {
                        Child::Triangle(leaf) => self.node_against_leaf(an, leaf),
                        Child::Node(child) => self.collide(an, child),
                    }
                    match nb.neg {
                        Child::Triangle(leaf) => return self.node_against_leaf(an, leaf),
                        Child::Node(child) => (a, b) = (an, child),
                    }
                }
            }
        }
    }
}

/// OPCODE's matrix of a pose (+0x16bb840): rows are the ODE rotation's columns.
fn matrix(p: &P, r: &[f64; 12]) -> [[f32; 4]; 4] {
    let f = |i: usize| r[i] as f32;
    [[f(0), f(4), f(8), 0.0], [f(1), f(5), f(9), 0.0], [f(2), f(6), f(10), 0.0], [p[0], p[1], p[2], 1.0]]
}

/// The pairs (triangle of the first mesh, triangle of the second) the tree query finds.
fn tree_pairs(m1: &Mesh, m2: &Mesh, edges: bool) -> Vec<(u32, u32)> {
    let (a, b) = (m1.model, m2.model);
    if !a.built || !b.built || a.single_node || b.single_node || a.nodes.is_empty() || b.nodes.is_empty() {
        return Vec::new();
    }
    let d = [(m2.pos[0] - m1.pos[0]) as f32, (m2.pos[1] - m1.pos[1]) as f32, (m2.pos[2] - m1.pos[2]) as f32];
    let (w0, w1) = (matrix(&[0.0; 3], &m1.r), matrix(&d, &m2.r));
    let (to1, to0) = (mul4(&w0, &invert_pr(&w1)), mul4(&w1, &invert_pr(&w0)));
    let rot = |m: &[[f32; 4]; 4]| -> M3 { std::array::from_fn(|i| [m[i][0], m[i][1], m[i][2]]) };
    let r1to0 = rot(&to0);
    let mut t = Tree {
        mesh0: m1.mesh,
        mesh1: m2.mesh,
        nodes0: &a.nodes,
        nodes1: &b.nodes,
        ar: r1to0.map(|row| row.map(|x| x.abs() + EPSILON)),
        r0to1: rot(&to1),
        r1to0,
        t0to1: [to1[3][0], to1[3][1], to1[3][2]],
        t1to0: [to0[3][0], to0[3][1], to0[3][2]],
        tests: 0,
        edges,
        v: [[0.0; 3]; 3],
        leaf: 0,
        e: [[0.0; 3]; 3],
        n: [0.0; 3],
        d: 0.0,
        pairs: Vec::new(),
    };
    t.collide(0, 0);
    t.pairs
}

/// `dCollideTTL` (+0x16bc1b0) of `m1` (geom `g1`) and `m2` (`g2`), each with its data's box
/// (centre, half sides); `prefilter` is `TRIMESH_OBB_PREFILTER` (byte +0x27f91a4), `edges` the
/// tree collider's edge axes (byte +0x27f91a6). No body hint (CSP gives one only when exactly one
/// geom has a body; car against car has two). The low 16 bits of `flags` are the most contacts.
#[allow(clippy::too_many_arguments)]
pub fn collide_ttl(m1: &Mesh, b1: &[f64; 6], m2: &Mesh, b2: &[f64; 6], flags: u32, g1: usize, g2: usize, prefilter: bool, edges: bool) -> Vec<ContactGeom> {
    if prefilter {
        let vol = |b: &[f64; 6]| (b[4] * b[3]) * b[5];
        // the smaller box against the bigger mesh's tree (+0x16bbea0)
        let ((s, sb), big) = if vol(b2) >= vol(b1) { ((m1, b1), m2) } else { ((m2, b2), m1) };
        let (c, r, ps, pb) = (&sb[..3], &s.r, &s.pos, &big.pos);
        let center = [
            ((((c[0] * r[0] + c[2] * r[2]) + c[1] * r[1]) + ps[0]) - pb[0]) as f32,
            ((((c[2] * r[6] + c[0] * r[4]) + c[1] * r[5]) + ps[1]) - pb[1]) as f32,
            ((((c[2] * r[10] + c[0] * r[8]) + c[1] * r[9]) + ps[2]) - pb[2]) as f32,
        ];
        let f = |m: &[f64; 12]| -> [f32; 12] { std::array::from_fn(|i| m[i] as f32) };
        if !obb_query(&center, &[sb[3] as f32, sb[4] as f32, sb[5] as f32], &f(r), &f(&big.r), big.model, big.mesh, None, true).0 {
            return Vec::new();
        }
    }
    let mut set = Set { max: (flags & 0xffff) as usize, out: Vec::new(), lens: Vec::new(), nodes: vec![Vec::new(); 256], g1, g2 };
    let stop = flags & 0x8000_ffff;
    for (t1, t2) in tree_pairs(m1, m2, edges) {
        let (v1, v2) = (fetch_triangle(m1, t1), fetch_triangle(m2, t2));
        let (depth, n, pts) = tri_tri(&v1, &v2);
        if !(depth >= 0.0) {
            continue;
        }
        for p in &pts {
            set.add(t1 as i32, t2 as i32, p, &n, depth);
            if set.out.len() as u32 | 0x8000_0000 == stop {
                return set.out;
            }
        }
    }
    set.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossed_triangles_meet_once_per_point() {
        let t1 = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let t2 = [[0.2, -0.1, 0.2], [0.3, 0.5, 0.2], [0.2, 0.5, 0.3]];
        let (depth, n, pts) = tri_tri(&t1, &t2);
        assert!(depth >= 0.0 && !pts.is_empty(), "{depth} {pts:?}");
        assert!((n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-12);
        let mut set = Set { max: 32, out: Vec::new(), lens: Vec::new(), nodes: vec![Vec::new(); 256], g1: 1, g2: 2 };
        set.add(0, 0, &pts[0], &n, depth);
        set.add(0, 0, &pts[0], &n, depth);
        assert_eq!(set.out.len(), 1);
        assert_eq!(key(&[0.0; 3]), key(&[0.00001; 3]));
    }
}
