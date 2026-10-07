// SPDX-License-Identifier: BSD-3-Clause

//! Box against triangle mesh: `ode/src/collision_trimesh_box.cpp` (`dCollideBTL`
//! @ 0x14038a3d0) as compiled into `acs.exe`.
//!
//! OPCODE's box query ([`crate::opcode_obb`]) names the triangles that may touch the box, in
//! the order of its tree walk. Each of them goes through a separating-axis test with
//! thirteen axes (the triangle's normal, the box's three face normals, the nine cross
//! products of a box axis and a triangle edge); the axis of least overlap decides how the
//! contact points are made: by clipping the triangle against the box's face (or the box's
//! face against the triangle), or as the closest points of two edges. Contact points close
//! to one that is already there are merged into it.
//!
//! The game asks for 4 or 32 contacts and never sets `CONTACTS_UNIMPORTANT`: every triangle
//! the query names is processed even when the array is full (the depths of contacts it
//! already has can still grow).

use crate::contact::ContactGeom;
use crate::geom::{BoxPose, GeomRef, MeshPose, NUMC_MASK};
use crate::odemath::safe_normalize3;
use crate::opcode_obb::obb_query;
use rustyac_math::sqrtf;

/// `dEpsilon` (`FLT_EPSILON`)
const EPS: f32 = f32::EPSILON;

#[inline]
fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

#[inline]
fn cross(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `sTrimeshBoxColliderData`
struct BoxCollider<'a> {
    /// `m_mHullBoxRot`: the box's rotation (ODE layout); its columns are the box's axes
    r: [f32; 12],
    /// `m_vHullBoxPos`
    p: [f32; 3],
    /// `m_vBoxHalfSize`
    h: [f32; 3],
    /// `m_vBestNormal`, `m_fBestDepth`, `m_iBestAxis`
    best_normal: [f32; 3],
    best_depth: f32,
    best_axis: i32,
    /// `m_vE0`, `m_vE1`, `m_vE2`, `m_vN`
    e0: [f32; 3],
    e1: [f32; 3],
    e2: [f32; 3],
    n: [f32; 3],
    /// `m_iFlags`
    flags: u32,
    g1: GeomRef,
    g2: GeomRef,
    /// The contacts of this call (`m_ContactGeoms`, `m_ctContacts`).
    contacts: &'a mut Vec<ContactGeom>,
    start: usize,
}

impl BoxCollider<'_> {
    /// Column `i` of the box's rotation: the box's axis `i` in the world.
    #[inline]
    fn axis(&self, i: usize) -> [f32; 3] {
        [self.r[i], self.r[4 + i], self.r[8 + i]]
    }

    /// `_cldTestFace` @ 0x140388e30: a box face normal as separating axis.
    fn test_face(&mut self, fp0: f32, fp1: f32, fp2: f32, fr: f32, normal: &mut [f32; 3], axis: i32) -> bool {
        // find min of triangle interval
        let fmin = if !(fp0 >= fp1) {
            if !(fp0 >= fp2) {
                fp0
            } else {
                fp2
            }
        } else if !(fp1 >= fp2) {
            fp1
        } else {
            fp2
        };
        // find max of triangle interval
        let fmax = if fp0 > fp1 {
            if fp0 > fp2 {
                fp0
            } else {
                fp2
            }
        } else if fp1 > fp2 {
            fp1
        } else {
            fp2
        };
        // calculate minimum and maximum depth
        let depth_max = fmax + fr;
        let depth_min = fr - fmin;
        // if we dont't have overlapping interval
        if !(depth_min >= 0.0) {
            return false;
        }
        if !(depth_max >= 0.0) {
            return false;
        }
        let depth;
        // if greater depth is on negative side
        if depth_min > depth_max {
            // use smaller depth (one from positive side), flip normal direction
            depth = depth_max;
            normal[0] = -normal[0];
            normal[1] = -normal[1];
            normal[2] = -normal[2];
        } else {
            // use smaller depth (one from negative side)
            depth = depth_min;
        }
        // if lower depth than best found so far
        if !(depth >= self.best_depth) {
            self.best_normal = *normal;
            self.best_axis = axis;
            self.best_depth = depth;
        }
        true
    }

    /// `_cldTestEdge` @ 0x140388cd0: the cross product of a box axis and a triangle edge as
    /// separating axis.
    fn test_edge(&mut self, fp0: f32, fp1: f32, fr: f32, normal: &mut [f32; 3], axis: i32) -> bool {
        // a null normal would be dangerous: this axis does not separate
        let sq = (normal[0] * normal[0] + normal[1] * normal[1]) + normal[2] * normal[2];
        if !(sq > EPS) {
            return true;
        }
        // calculate min and max interval values
        let (fmin, fmax) = if !(fp0 >= fp1) { (fp0, fp1) } else { (fp1, fp0) };
        // check if we overlapp
        let depth_max = fmax + fr;
        let depth_min = fr - fmin;
        if !(depth_min >= 0.0) {
            return false;
        }
        if !(depth_max >= 0.0) {
            return false;
        }
        let mut depth;
        // if greater depth is on negative side
        if depth_min > depth_max {
            depth = depth_max;
            normal[0] = -normal[0];
            normal[1] = -normal[1];
            normal[2] = -normal[2];
        } else {
            depth = depth_min;
        }
        // calculate normal's length
        let length = sqrtf((normal[0] * normal[0] + normal[1] * normal[1]) + normal[2] * normal[2]);
        // if long enough
        if length > 0.0 {
            // normalize depth
            let one_over_length = 1.0f32 / length;
            depth = depth * one_over_length;
            // if lower depth than best found so far (favor face over edges)
            if !(depth * 1.5f32 >= self.best_depth) {
                self.best_normal = [normal[0] * one_over_length, one_over_length * normal[1], one_over_length * normal[2]];
                self.best_depth = depth;
                self.best_axis = axis;
            }
        }
        true
    }

    /// `_cldTestSeparatingAxes` @ 0x140388ef0: false when the triangle and the box do not
    /// meet; otherwise the best axis, its normal and its depth are left in `self`.
    fn test_separating_axes(&mut self, v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3]) -> bool {
        // reset best axis
        self.best_axis = 0;
        self.best_depth = f32::MAX;

        // calculate edges
        self.e0 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        self.e1 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        self.e2 = [self.e1[0] - self.e0[0], self.e1[1] - self.e0[1], self.e1[2] - self.e0[2]];
        let (e0, e1, e2) = (self.e0, self.e1, self.e2);

        // calculate poly normal
        let n = cross(&e0, &e1);
        self.n = n;

        // calculate length of face normal; a triangle without an area (or a NaN) is left out
        let n_len = sqrtf((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]);
        if !(n_len < 0.0 || n_len > 0.0) {
            return false;
        }

        // extract box axes as vectors
        let a = [self.axis(0), self.axis(1), self.axis(2)];
        // box halfsizes
        let fa = self.h;
        // calculate relative position between box and triangle
        let d = [v0[0] - self.p[0], v0[1] - self.p[1], v0[2] - self.p[2]];

        // Axis 1 - Triangle Normal (_cldTestNormal, inlined)
        {
            let fr = (fa[0] * dot(&n, &a[0]).abs() + fa[1] * dot(&n, &a[1]).abs()) + fa[2] * dot(&n, &a[2]).abs();
            let fp0 = dot(&n, &d);
            // calculate overlapping interval of box and triangle
            let mut depth = fr + fp0;
            // if we do not overlap (comiss + jb: also for a NaN)
            if !(depth >= 0.0) {
                return false;
            }
            // calculate normal's length
            let length = sqrtf((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]);
            // if long enough
            if length > 0.0 {
                let one_over_length = 1.0f32 / length;
                // normalize depth
                depth = depth * one_over_length;
                // get minimum depth
                if !(depth >= self.best_depth) {
                    self.best_normal = [-(one_over_length * n[0]), -(one_over_length * n[1]), -(one_over_length * n[2])];
                    self.best_depth = depth;
                    self.best_axis = 1;
                }
            }
        }

        // Axes 2, 3, 4 - Box X, Y, Z-Axis
        for i in 0..3 {
            let mut l = a[i];
            let fp0 = dot(&l, &d);
            let fp1 = dot(&l, &e0) + fp0;
            let fp2 = dot(&l, &e1) + fp0;
            if !self.test_face(fp0, fp1, fp2, fa[i], &mut l, 2 + i as i32) {
                return false;
            }
        }

        // Axes 5 … 13 - box axis x triangle edge
        let edges = [e0, e1, e2];
        for i in 0..3 {
            for j in 0..3 {
                let e = &edges[j];
                let mut l = cross(&a[i], e);
                let fp0 = dot(&l, &d);
                let q = dot(&a[i], &n);
                let (pa, pb) = if j == 0 { (fp0, q + fp0) } else { (fp0, fp0 - q) };
                let fr = match i {
                    0 => fa[1] * dot(&a[2], e).abs() + fa[2] * dot(&a[1], e).abs(),
                    1 => fa[0] * dot(&a[2], e).abs() + fa[2] * dot(&a[0], e).abs(),
                    _ => fa[0] * dot(&a[1], e).abs() + fa[1] * dot(&a[0], e).abs(),
                };
                if !self.test_edge(pa, pb, fr, &mut l, 5 + 3 * i as i32 + j as i32) {
                    return false;
                }
            }
        }
        true
    }
}

/// `FetchTriangle` @ 0x140346770: the three corners of a triangle in the world.
#[inline]
pub(crate) fn fetch_triangle(mesh: &MeshPose, index: u32) -> [[f32; 3]; 3] {
    let tri = mesh.data.mesh.triangle(index);
    let (rot, pos) = (&mesh.r, &mesh.pos);
    let mut out = [[0.0f32; 3]; 3];
    for i in 0..3 {
        let (x, y, z) = (tri[i][0], tri[i][1], tri[i][2]);
        let o0 = (rot[0] * x + rot[1] * y) + rot[2] * z;
        let o1 = (rot[4] * x + rot[5] * y) + rot[6] * z;
        let o2 = (rot[8] * x + rot[9] * y) + rot[10] * z;
        out[i] = [o0 + pos[0], o1 + pos[1], o2 + pos[2]];
    }
    out
}

/// `dCollideBTL` @ 0x14038a3d0: the contacts of the box with the mesh, appended to `out`
/// (`g1` is the mesh, `g2` the box). The low 16 bits of `flags` are the most contacts.
pub fn collide_btl(mesh: &MeshPose, box_: &BoxPose, flags: u32, g1: GeomRef, g2: GeomRef, out: &mut Vec<ContactGeom>) {
    let start = out.len();
    // SetupInitialContext @ 0x1403876e0
    let mut c = BoxCollider {
        r: box_.r,
        p: box_.pos,
        h: [box_.side[0] * 0.5, box_.side[1] * 0.5, box_.side[2] * 0.5],
        best_normal: [0.0; 3],
        best_depth: f32::MAX,
        best_axis: 0,
        e0: [0.0; 3],
        e1: [0.0; 3],
        e2: [0.0; 3],
        n: [0.0; 3],
        flags,
        g1,
        g2,
        contacts: out,
        start,
    };
    // dQueryBTLPotentialCollisionTriangles @ 0x14038a630
    let (contact, triangles) = obb_query(&c.p, &c.h, &c.r, &mesh.data.model, &mesh.data.mesh, &mesh.pos, &mesh.r);
    if !contact {
        return;
    }
    // in the order OPCODE returned them
    for &tri in &triangles {
        let dv = fetch_triangle(mesh, tri);
        if c.test_separating_axes(&dv[0], &dv[1], &dv[2]) && c.best_axis != 0 {
            c.clipping(&dv[0], &dv[1], &dv[2], tri as i32);
        }
        // (side1 = the triangle, side2 = -1 for the new contacts: GenerateContact wrote them)
        // The "array is full" exit needs CONTACTS_UNIMPORTANT, which the game never sets.
        if flags & 0x8000_0000 != 0 && (c.contacts.len() - c.start) as u32 == flags & NUMC_MASK {
            break;
        }
    }
}

/// `_cldClipPolyToPlane` @ 0x140387880: the part of a polygon on the positive side of the
/// plane `pl` (normal, distance). For every edge (previous vertex -> this vertex, starting
/// with last -> first): the previous vertex if it is not behind the plane, then the crossing
/// point if the edge crosses. The crossing is computed with one reciprocal for the three
/// components (the source divides three times; the game's compiler did not).
fn clip(input: &[[f32; 3]], out: &mut Vec<[f32; 3]>, pl: &[f32; 4]) {
    out.clear();
    let n = input.len();
    if n == 0 {
        return;
    }
    let mut i0 = n - 1;
    for i1 in 0..n {
        let (a, b) = (&input[i0], &input[i1]);
        let d0 = ((pl[0] * a[0] + pl[1] * a[1]) + pl[2] * a[2]) + pl[3];
        let d1 = ((pl[0] * b[0] + pl[1] * b[1]) + pl[2] * b[2]) + pl[3];
        let cross_over;
        if d0 >= 0.0 {
            // emit point
            out.push(*a);
            cross_over = d0 > 0.0 && !(d1 >= 0.0);
        } else {
            cross_over = d1 > 0.0;
        }
        // if points are on different sides: the intersection point of edge and plane
        if cross_over {
            let inv = 1.0f32 / (d0 - d1);
            out.push([a[0] - ((a[0] - b[0]) * d0) * inv, a[1] - ((a[1] - b[1]) * d0) * inv, a[2] - ((a[2] - b[2]) * d0) * inv]);
        }
        i0 = i1;
    }
    // (the game's arrays hold 9 points and are not checked; a convex polygon never needs more)
    debug_assert!(out.len() <= 9, "a clipped polygon of more than 9 points");
}

impl BoxCollider<'_> {
    /// `GenerateContact` @ 0x140387560: a new contact, unless one of this call's contacts is
    /// at (nearly) the same place with (nearly) the same normal: then that one only gets the
    /// larger depth. When the array is full the new contact is dropped.
    fn generate_contact(&mut self, tri: i32, pos: &[f32; 3], normal: &[f32; 3], depth: f32) {
        let count = self.contacts.len() - self.start;
        if self.flags & 0x8000_0000 == 0 {
            let mut duplicate = false;
            for c in self.contacts[self.start..].iter_mut() {
                let d = [pos[0] - c.pos[0], pos[1] - c.pos[1], pos[2] - c.pos[2]];
                let dd = (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2];
                if !(dd >= EPS) {
                    let nn = (normal[0] * c.normal[0] + normal[1] * c.normal[1]) + normal[2] * c.normal[2];
                    if !((1.0f32 - nn.abs()) >= EPS) {
                        // if new contact is deeper: replace the depth (no break: every match)
                        if depth > c.depth {
                            c.depth = depth;
                        }
                        duplicate = true;
                    }
                }
            }
            if duplicate {
                return;
            }
            if count as u32 == self.flags & NUMC_MASK {
                return;
            }
        }
        self.contacts.push(ContactGeom { pos: *pos, normal: *normal, depth, g1: self.g1, g2: self.g2, side1: tri, side2: -1 });
    }

    /// `_cldClipping` @ 0x140387a40: the contact points for the best axis.
    fn clipping(&mut self, v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3], tri: i32) {
        let (r, p, h, bn) = (self.r, self.p, self.h, self.best_normal);
        let mut arr1: Vec<[f32; 3]> = Vec::with_capacity(16);
        let mut arr2: Vec<[f32; 3]> = Vec::with_capacity(16);
        // if we have edge/edge intersection
        if self.best_axis > 4 {
            // calculate point on box edge
            let mut pa = p;
            for i in 0..3 {
                let col = self.axis(i);
                let sign = if dot(&col, &bn) > 0.0 { 1.0f32 } else { -1.0f32 };
                let t = sign * h[i];
                pa[0] = pa[0] + t * col[0];
                pa[1] = pa[1] + t * col[1];
                pa[2] = pa[2] + t * col[2];
            }
            // decide which edge is on triangle
            let edge = (self.best_axis - 5) % 3;
            let (mut pb, mut ub) = match edge {
                0 => (*v0, self.e0),
                1 => (*v2, self.e1),
                _ => (*v1, self.e2),
            };
            // setup direction parameter for face edge
            safe_normalize3(&mut ub);
            // setup direction parameter for box edge
            let ua = self.axis(((self.best_axis - 5) / 3) as usize);
            // find two closest points on both edges (_cldClosestPointOnTwoLines)
            let vp = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let uaub = dot(&ua, &ub);
            let q1 = dot(&ua, &vp);
            let q2 = -dot(&ub, &vp);
            let fd = 1.0f32 - uaub * uaub;
            let (param1, param2) = if fd > 0.0 {
                let inv = 1.0f32 / fd;
                ((q2 * uaub + q1) * inv, (q1 * uaub + q2) * inv)
            } else {
                // lines are parallel
                (0.0f32, 0.0f32)
            };
            for k in 0..3 {
                pa[k] = pa[k] + ua[k] * param1;
            }
            for k in 0..3 {
                pb[k] = pb[k] + ub[k] * param2;
            }
            // the contact point is between the two closest points
            let point = [(pb[0] + pa[0]) * 0.5, (pb[1] + pa[1]) * 0.5, (pb[2] + pa[2]) * 0.5];
            let depth = self.best_depth;
            self.generate_contact(tri, &point, &bn, depth);
        } else if self.best_axis == 1 {
            // the triangle is the referent face: clip the box to the triangle face
            let n2 = [-bn[0], -bn[1], -bn[2]];
            // vNr is the normal in the box frame, pointing from triangle to box
            let nr = [
                (r[0] * n2[0] + r[4] * n2[1]) + r[8] * n2[2],
                (r[1] * n2[0] + r[5] * n2[1]) + r[9] * n2[2],
                (r[2] * n2[0] + r[6] * n2[1]) + r[10] * n2[2],
            ];
            let a = [nr[0].abs(), nr[1].abs(), nr[2].abs()];
            // get closest face from box
            let (ib0, ib1, ib2) = if a[1] > a[0] {
                if a[1] > a[2] {
                    (1, 0, 2)
                } else {
                    (2, 0, 1)
                }
            } else if a[0] > a[2] {
                (0, 1, 2)
            } else {
                (2, 0, 1)
            };
            // the centre of the box face we are going to project, relative to v0
            let c0 = self.axis(ib0);
            let mut center = [p[0] - v0[0], p[1] - v0[1], p[2] - v0[2]];
            if !(0.0 >= nr[ib0]) {
                for k in 0..3 {
                    center[k] = center[k] - h[ib0] * c0[k];
                }
            } else {
                for k in 0..3 {
                    center[k] = center[k] + h[ib0] * c0[k];
                }
            }
            // the 4 corner points of the box face
            let (c1, c2) = (self.axis(ib1), self.axis(ib2));
            let mut points = [[0.0f32; 3]; 4];
            for k in 0..3 {
                let a1 = h[ib1] * c1[k];
                let a2 = h[ib2] * c2[k];
                points[0][k] = (center[k] + a1) - a2;
                points[1][k] = (center[k] - a1) - a2;
                points[2][k] = (center[k] - a1) + a2;
                points[3][k] = (center[k] + a1) + a2;
            }
            // clip the box face with the 4 planes of the triangle (1 face plane, 3 edge planes)
            let n = self.n;
            // Normal plane
            let mut temp = [-n[0], -n[1], -n[2]];
            safe_normalize3(&mut temp);
            clip(&points, &mut arr1, &[temp[0], temp[1], temp[2], 0.0]);
            // Plane p0
            let t = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            temp = cross(&n, &t);
            safe_normalize3(&mut temp);
            clip(&arr1, &mut arr2, &[temp[0], temp[1], temp[2], 0.0]);
            // Plane p1
            let t = [v2[0] - v1[0], v2[1] - v1[1], v2[2] - v1[2]];
            temp = cross(&n, &t);
            safe_normalize3(&mut temp);
            let u = [v0[0] - v2[0], v0[1] - v2[1], v0[2] - v2[2]];
            clip(&arr2, &mut arr1, &[temp[0], temp[1], temp[2], dot(&u, &temp)]);
            // Plane p2
            let t = [v0[0] - v2[0], v0[1] - v2[1], v0[2] - v2[2]];
            temp = cross(&n, &t);
            safe_normalize3(&mut temp);
            clip(&arr1, &mut arr2, &[temp[0], temp[1], temp[2], 0.0]);
            // for each generated contact point
            for i in 0..arr2.len() {
                let q = arr2[i];
                // calculate depth
                let mut depth = (q[0] * n2[0] + q[1] * n2[1]) + q[2] * n2[2];
                // clamp depth to zero
                if depth > 0.0 {
                    depth = 0.0;
                }
                let point = [q[0] + v0[0], q[1] + v0[1], q[2] + v0[2]];
                self.generate_contact(tri, &point, &bn, -depth);
                if self.flags & 0x8000_0000 != 0 && (self.contacts.len() - self.start) as u32 == self.flags & NUMC_MASK {
                    break;
                }
            }
        } else {
            // a box face is the referent face: clip the triangle on the box
            let ia0 = (self.best_axis - 2) as usize;
            let (ia1, ia2) = match ia0 {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            // the triangle relative to the box centre
            let points = [
                [v0[0] - p[0], v0[1] - p[1], v0[2] - p[2]],
                [v1[0] - p[0], v1[1] - p[1], v1[2] - p[2]],
                [v2[0] - p[0], v2[1] - p[1], v2[2] - p[2]],
            ];
            let (c1, c2) = (self.axis(ia1), self.axis(ia2));
            // Normal plane, then the four planes of the face's sides
            clip(&points, &mut arr1, &[-bn[0], -bn[1], -bn[2], h[ia0]]);
            clip(&arr1, &mut arr2, &[c1[0], c1[1], c1[2], h[ia1]]);
            clip(&arr2, &mut arr1, &[-c1[0], -c1[1], -c1[2], h[ia1]]);
            clip(&arr1, &mut arr2, &[c2[0], c2[1], c2[2], h[ia2]]);
            clip(&arr2, &mut arr1, &[-c2[0], -c2[1], -c2[2], h[ia2]]);
            // for each generated contact point
            for i in 0..arr1.len() {
                let q = arr1[i];
                // calculate depth
                let mut depth = ((q[0] * bn[0] + q[1] * bn[1]) + q[2] * bn[2]) - h[ia0];
                // clamp depth to zero
                if depth > 0.0 {
                    depth = 0.0;
                }
                let point = [q[0] + p[0], q[1] + p[1], q[2] + p[2]];
                self.generate_contact(tri, &point, &bn, -depth);
                if self.flags & 0x8000_0000 != 0 && (self.contacts.len() - self.start) as u32 == self.flags & NUMC_MASK {
                    break;
                }
            }
        }
    }
}
