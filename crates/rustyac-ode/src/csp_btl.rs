// SPDX-License-Identifier: BSD-3-Clause

//! Box against triangle mesh in f64 as Custom Shaders Patch 0.3.2 (build 4305) runs it: ODE
//! master's `dCollideBTL` (+0x16b1b90, called as (mesh, box)). The vanilla f32 port is
//! `collide_btl.rs`; this one differs in precision, in the grouping CSP's build gives the sums,
//! in master's contact merge (a deeper duplicate keeps the deeper depth, a full array replaces
//! its shallowest contact) and in the OPCODE query ([`crate::csp_opcode`]: mesh-relative, with
//! temporal coherence).

use crate::csp_opcode::{obb_query, ObbCache};
use crate::odemath::safe_normalize3;
use crate::opcode::{MeshInterface, Model};

/// `dContactGeom` in f64 (96 bytes in CSP).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactGeom {
    pub pos: [f64; 3],
    pub normal: [f64; 3],
    pub depth: f64,
    pub g1: usize,
    pub g2: usize,
    pub side1: i32,
    pub side2: i32,
}

/// The mesh side of a query: data, tree and placement.
pub struct Mesh<'a> {
    pub mesh: &'a MeshInterface,
    pub model: &'a Model,
    pub pos: [f64; 3],
    pub r: [f64; 12],
}

type V3 = [f64; 3];

#[inline]
fn dot(a: &V3, b: &V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

#[inline]
fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[inline]
fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn neg(a: &V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}

/// `minsd a, b` / `maxsd a, b`
#[inline]
fn minsd(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

#[inline]
fn maxsd(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `dSafeNormalize3`, (1, 0, 0) when it fails.
fn normalized(mut v: V3) -> V3 {
    if !safe_normalize3::<f64>(&mut v) {
        v = [1.0, 0.0, 0.0];
    }
    v
}

const EPS: f64 = f64::EPSILON;

struct Collider<'a> {
    r: [f64; 12],
    p: V3,
    h: V3,
    best_normal: V3,
    best_depth: f64,
    best_axis: i32,
    e0: V3,
    e1: V3,
    e2: V3,
    n: V3,
    flags: u32,
    g1: usize,
    g2: usize,
    out: &'a mut Vec<ContactGeom>,
}

impl Collider<'_> {
    fn col(&self, i: usize) -> V3 {
        [self.r[i], self.r[i + 4], self.r[i + 8]]
    }

    fn full(&self) -> bool {
        (self.out.len() as u32 | 0x8000_0000) == self.flags & 0x8000_ffff
    }

    /// `_cldTestFace` (+0x16aede0)
    fn test_face(&mut self, fp0: f64, fp1: f64, fp2: f64, fr: f64, mut vn: V3, axis: i32) -> bool {
        let (mn, mx) = if fp0 < fp1 {
            (minsd(fp0, fp2), maxsd(fp1, fp2))
        } else if fp0 > fp1 {
            (minsd(fp1, fp2), maxsd(fp0, fp2))
        } else {
            (minsd(fp1, fp2), maxsd(fp1, fp2))
        };
        let dmin = fr - mn;
        let dmax = mx + fr;
        if !(dmin >= 0.0) || !(dmax >= 0.0) {
            return false;
        }
        let depth = if dmin > dmax {
            vn = neg(&vn);
            dmax
        } else {
            dmin
        };
        if !(depth >= self.best_depth) {
            self.best_normal = vn;
            self.best_depth = depth;
            self.best_axis = axis;
        }
        true
    }

    /// `_cldTestEdge` (+0x16aee90)
    fn test_edge(&mut self, fp0: f64, fp1: f64, fr: f64, mut vn: V3, axis: i32) -> bool {
        let l2 = (vn[0] * vn[0] + vn[1] * vn[1]) + vn[2] * vn[2];
        if !(l2 > 2.220446049250313e-16) {
            return true;
        }
        let mn = minsd(fp0, fp1);
        let mx = maxsd(fp0, fp1);
        let dmin = fr - mn;
        let dmax = mx + fr;
        if !(dmin >= 0.0) || !(dmax >= 0.0) {
            return false;
        }
        let mut depth = if dmin > dmax {
            vn = neg(&vn);
            dmax
        } else {
            dmin
        };
        let len = l2.sqrt();
        if !(len > 0.0) {
            return true;
        }
        let inv = 1.0 / len;
        depth *= inv;
        if !(depth * 1.5 >= self.best_depth) {
            self.best_axis = axis;
            self.best_normal = [vn[0] * inv, vn[1] * inv, vn[2] * inv];
            self.best_depth = depth;
        }
        true
    }

    /// `_cldTestSeparatingAxes` (+0x16af270)
    fn separating_axes(&mut self, a: &V3, b: &V3, c: &V3) -> bool {
        self.best_axis = 0;
        self.best_depth = f64::MAX;
        self.e0 = sub(b, a);
        self.e1 = sub(c, a);
        self.e2 = sub(&self.e1, &self.e0);
        self.n = cross(&self.e0, &self.e1);
        let nlen = dot(&self.n, &self.n).sqrt();
        if nlen == 0.0 || nlen.is_nan() {
            return false;
        }
        let (e0, e1, e2, n, h) = (self.e0, self.e1, self.e2, self.n, self.h);
        let d = sub(a, &self.p);
        let (a0, a1, a2) = (self.col(0), self.col(1), self.col(2));

        // axis 1: the triangle's normal
        let fr = (h[0] * dot(&a0, &n).abs() + h[1] * dot(&a1, &n).abs()) + h[2] * dot(&a2, &n).abs();
        let mut depth = fr + dot(&d, &n);
        if !(depth >= 0.0) {
            return false;
        }
        if nlen > 0.0 {
            let inv = 1.0 / nlen;
            depth *= inv;
            if !(depth >= self.best_depth) {
                self.best_normal = neg(&[n[0] * inv, n[1] * inv, n[2] * inv]);
                self.best_depth = depth;
                self.best_axis = 1;
            }
        }

        // axis 2: box axis 0
        let fp0 = dot(&a0, &d);
        let fp1 = dot(&a0, &e0) + fp0;
        let fp2 = dot(&a0, &e1) + fp0;
        let (mn, mx) = if fp0 >= fp1 {
            (minsd(fp2, fp1), if fp0 > fp1 { maxsd(fp2, fp0) } else { maxsd(fp2, fp1) })
        } else {
            (minsd(fp2, fp0), maxsd(fp2, fp1))
        };
        let dmin = h[0] - mn;
        let dmax = mx + h[0];
        if !(dmin >= 0.0) || !(dmax >= 0.0) {
            return false;
        }
        let (depth, vn) = if dmin > dmax { (dmax, neg(&a0)) } else { (dmin, a0) };
        if !(depth >= self.best_depth) {
            self.best_normal = vn;
            self.best_depth = depth;
            self.best_axis = 2;
        }

        // axes 3, 4: box axes 1, 2 (CSP adds the last product to fp0 first)
        let face = |ai: &V3, fp0: f64, e: &V3| (ai[0] * e[0] + ai[1] * e[1]) + (ai[2] * e[2] + fp0);
        let fp0 = dot(&a1, &d);
        if !self.test_face(fp0, face(&a1, fp0, &e0), face(&a1, fp0, &e1), h[1], a1, 3) {
            return false;
        }
        let fp0 = dot(&a2, &d);
        if !self.test_face(fp0, face(&a2, fp0, &e0), face(&a2, fp0, &e1), h[2], a2, 4) {
            return false;
        }

        // axes 5..13: box axis x triangle edge
        let edges = [e0, e1, e2];
        let axes = [a0, a1, a2];
        for (i, ai) in axes.iter().enumerate() {
            let (j, k) = match i {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            for (m, ek) in edges.iter().enumerate() {
                let vl = cross(ai, ek);
                let fp0 = dot(&vl, &d);
                let fp1 = if m == 0 { face(ai, fp0, &n) } else { fp0 - dot(ai, &n) };
                let fr = h[j] * dot(&axes[k], ek).abs() + h[k] * dot(&axes[j], ek).abs();
                if !self.test_edge(fp0, fp1, fr, vl, 5 + 3 * i as i32 + m as i32) {
                    return false;
                }
            }
        }
        true
    }

    /// `GenerateContact` (+0x16b16c0), ODE master's merge.
    fn generate_contact(&mut self, tri: i32, pos: &V3, n: &V3, depth: f64) {
        let mut min = 1e300;
        let mut minc = None;
        if (self.flags as i32) >= 0 {
            for i in 0..self.out.len() {
                let c = &mut self.out[i];
                let diff = sub(pos, &c.pos);
                if !(dot(&diff, &diff) >= EPS) && dot(&c.normal, n) > 1.0 - EPS {
                    if depth > c.depth {
                        c.depth = depth;
                        c.side1 = tri;
                    }
                    return;
                }
                if !(c.depth >= min) {
                    min = c.depth;
                    minc = Some(i);
                }
            }
        }
        let c = if self.out.len() as u32 == self.flags & 0xffff {
            match minc {
                Some(i) if !(min >= depth) => &mut self.out[i],
                _ => return,
            }
        } else {
            self.out.push(ContactGeom { g1: self.g1, g2: self.g2, side2: -1, ..Default::default() });
            self.out.last_mut().unwrap()
        };
        c.pos = *pos;
        c.normal = *n;
        c.depth = depth;
        c.side1 = tri;
    }

    /// `_cldClipping` (+0x16b04b0)
    fn clipping(&mut self, a: &V3, b: &V3, c: &V3, tri: i32) {
        let (bn, h) = (self.best_normal, self.h);
        if self.best_axis > 4 {
            // edge against edge: the closest points of the two lines
            let mut pa = self.p;
            for i in 0..3 {
                let col = self.col(i);
                let s = (if dot(&bn, &col) > 0.0 { 1.0 } else { -1.0 }) * h[i];
                for k in 0..3 {
                    pa[k] = s * col[k] + pa[k];
                }
            }
            let e = (self.best_axis - 5) % 3;
            let (mut pb, ub) = match e {
                0 => (*a, self.e0),
                1 => (*c, self.e1),
                _ => (*b, self.e2),
            };
            let ub = normalized(ub);
            let ua = self.col(((self.best_axis - 5) / 3) as usize);
            let vp = sub(&pb, &pa);
            let uaub = dot(&ub, &ua);
            let q1 = dot(&ua, &vp);
            let q2 = -dot(&ub, &vp);
            let fd = 1.0 - uaub * uaub;
            let (v1, v2) = if fd > 0.0 {
                let inv = 1.0 / fd;
                ((q2 * uaub + q1) * inv, (q1 * uaub + q2) * inv)
            } else {
                (0.0, 0.0)
            };
            for k in 0..3 {
                pa[k] += ua[k] * v1;
                pb[k] += ub[k] * v2;
            }
            let pt = [(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5, (pa[2] + pb[2]) * 0.5];
            let depth = self.best_depth;
            self.generate_contact(tri, &pt, &bn, depth);
            return;
        }
        let mut t1: Vec<V3> = Vec::with_capacity(9);
        let mut t2: Vec<V3> = Vec::with_capacity(9);
        if self.best_axis == 1 {
            // the box's face against the triangle
            let n2 = neg(&bn);
            let nr = [dot(&self.col(0), &n2), dot(&self.col(1), &n2), dot(&self.col(2), &n2)];
            let an = [nr[0].abs(), nr[1].abs(), nr[2].abs()];
            let (b0, b1, b2) = if an[1] > an[0] {
                if an[1] > an[2] {
                    (1, 0, 2)
                } else {
                    (2, 0, 1)
                }
            } else if an[0] > an[2] {
                (0, 1, 2)
            } else {
                (2, 0, 1)
            };
            let cb0 = self.col(b0);
            let mut center = [0.0; 3];
            for k in 0..3 {
                center[k] = if !(0.0 >= nr[b0]) {
                    (self.p[k] - a[k]) - h[b0] * cb0[k]
                } else {
                    (self.p[k] - a[k]) + h[b0] * cb0[k]
                };
            }
            let (c1, c2) = (self.col(b1), self.col(b2));
            let h1 = [h[b1] * c1[0], h[b1] * c1[1], h[b1] * c1[2]];
            let h2 = [h[b2] * c2[0], h[b2] * c2[1], h[b2] * c2[2]];
            let mut quad = [[0.0; 3]; 4];
            for k in 0..3 {
                quad[0][k] = (center[k] + h1[k]) - h2[k];
                quad[1][k] = (center[k] - h1[k]) - h2[k];
                quad[2][k] = (center[k] - h1[k]) + h2[k];
                quad[3][k] = (center[k] + h1[k]) + h2[k];
            }
            let n = self.n;
            let t = normalized(neg(&n));
            clip(&quad, &mut t1, &[t[0], t[1], t[2], 0.0]);
            let t = normalized(cross(&n, &sub(b, a)));
            clip(&t1, &mut t2, &[t[0], t[1], t[2], 0.0]);
            let t = normalized(cross(&n, &sub(c, b)));
            clip(&t2, &mut t1, &[t[0], t[1], t[2], dot(&sub(a, c), &t)]);
            let t = normalized(cross(&n, &sub(a, c)));
            clip(&t1, &mut t2, &[t[0], t[1], t[2], 0.0]);
            for p in &t2 {
                let dep = minsd(dot(p, &n2), 0.0);
                let pos = [p[0] + a[0], p[1] + a[1], p[2] + a[2]];
                self.generate_contact(tri, &pos, &bn, -dep);
                if self.full() {
                    break;
                }
            }
            return;
        }
        // the triangle against the box's face
        let ia0 = (self.best_axis - 2) as usize;
        let (ia1, ia2) = match ia0 {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let tri_pts = [sub(a, &self.p), sub(b, &self.p), sub(c, &self.p)];
        let nb = neg(&bn);
        clip(&tri_pts, &mut t1, &[nb[0], nb[1], nb[2], h[ia0]]);
        let c1 = self.col(ia1);
        clip(&t1, &mut t2, &[c1[0], c1[1], c1[2], h[ia1]]);
        let c1 = neg(&c1);
        clip(&t2, &mut t1, &[c1[0], c1[1], c1[2], h[ia1]]);
        let c2 = self.col(ia2);
        clip(&t1, &mut t2, &[c2[0], c2[1], c2[2], h[ia2]]);
        let c2 = neg(&c2);
        clip(&t2, &mut t1, &[c2[0], c2[1], c2[2], h[ia2]]);
        for p in &t1 {
            let dep = minsd(dot(&bn, p) - h[ia0], 0.0);
            let pos = [p[0] + self.p[0], p[1] + self.p[1], p[2] + self.p[2]];
            self.generate_contact(tri, &pos, &bn, -dep);
            if self.full() {
                break;
            }
        }
    }
}

/// `_cldClipPolyToPlane` (+0x16af000): the part of a polygon on the plane's positive side. The
/// crossing divides by the difference of the dot products without the plane's distance.
fn clip(input: &[V3], out: &mut Vec<V3>, pl: &[f64; 4]) {
    out.clear();
    let n = input.len();
    if n == 0 {
        return;
    }
    let pn = [pl[0], pl[1], pl[2]];
    let mut i0 = n - 1;
    for i1 in 0..n {
        let (x0, x1) = (&input[i0], &input[i1]);
        let s0 = dot(&pn, x0);
        let d0 = s0 + pl[3];
        let s1 = dot(&pn, x1);
        let d1 = s1 + pl[3];
        let cross = if d0 >= 0.0 {
            out.push(*x0);
            d0 > 0.0 && !(d1 >= 0.0)
        } else {
            d1 > 0.0
        };
        if cross {
            let inv = 1.0 / (s0 - s1);
            out.push([
                x0[0] - ((x0[0] - x1[0]) * d0) * inv,
                x0[1] - ((x0[1] - x1[1]) * d0) * inv,
                x0[2] - ((x0[2] - x1[2]) * d0) * inv,
            ]);
        }
        i0 = i1;
    }
}

/// `FetchTransformedTriangle` (+0x1694600): a triangle's corners in the world.
pub fn fetch_triangle(mesh: &Mesh, tri: u32) -> [V3; 3] {
    let t = mesh.mesh.triangle(tri);
    let (r, p) = (&mesh.r, &mesh.pos);
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        let (x, y, z) = (t[i][0] as f64, t[i][1] as f64, t[i][2] as f64);
        out[i] = [
            ((x * r[0] + y * r[1]) + z * r[2]) + p[0],
            ((x * r[4] + y * r[5]) + z * r[6]) + p[1],
            ((x * r[8] + y * r[9]) + z * r[10]) + p[2],
        ];
    }
    out
}

/// `dCollideBTL` (+0x16b1b90) as (mesh `g1`, box `g2`): the box at `pos`, rotation `r`, sides
/// `side`. `tc` is the box's temporal-coherence slot for this mesh when the mesh has `doBoxTC`
/// on; `full_box_box` is OPCODE's full box-box test switch. The low 16 bits of `flags` are the
/// most contacts.
#[allow(clippy::too_many_arguments)]
pub fn collide_btl(
    mesh: &Mesh,
    pos: &V3,
    r: &[f64; 12],
    side: &V3,
    flags: u32,
    g1: usize,
    g2: usize,
    tc: Option<&mut ObbCache>,
    full_box_box: bool,
) -> Vec<ContactGeom> {
    let mut out = Vec::new();
    let mut c = Collider {
        r: *r,
        p: *pos,
        h: [side[0] * 0.5, side[1] * 0.5, side[2] * 0.5],
        best_normal: [0.0; 3],
        best_depth: f64::MAX,
        best_axis: 0,
        e0: [0.0; 3],
        e1: [0.0; 3],
        e2: [0.0; 3],
        n: [0.0; 3],
        flags,
        g1,
        g2,
        out: &mut out,
    };
    let center = [(pos[0] - mesh.pos[0]) as f32, (pos[1] - mesh.pos[1]) as f32, (pos[2] - mesh.pos[2]) as f32];
    let ext = [c.h[0] as f32, c.h[1] as f32, c.h[2] as f32];
    let f = |m: &[f64; 12]| -> [f32; 12] { std::array::from_fn(|i| m[i] as f32) };
    let (contact, tris) = obb_query(&center, &ext, &f(r), &f(&mesh.r), mesh.model, mesh.mesh, tc, full_box_box);
    if !contact {
        return out;
    }
    for &tri in &tris {
        let [a, b, cc] = fetch_triangle(mesh, tri);
        if c.separating_axes(&a, &b, &cc) && c.best_axis != 0 {
            c.clipping(&a, &b, &cc, tri as i32);
        }
        if c.full() {
            break;
        }
    }
    out
}
