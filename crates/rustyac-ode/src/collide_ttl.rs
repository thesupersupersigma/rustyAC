// SPDX-License-Identifier: BSD-3-Clause

//! Triangle mesh against triangle mesh: `ode/src/collision_trimesh_trimesh_new.cpp`
//! (`dCollideTTL` @ 0x14038c0d0) as compiled into `acs.exe`.
//!
//! OPCODE's tree-against-tree query ([`crate::opcode_tree`]) names the pairs of triangles
//! that meet, in the order of its walk. For each pair the collider looks at both triangles'
//! planes: the other triangle is clipped to the prism over the triangle and its deepest
//! points behind the plane are taken; the triangle with the smaller penetration gives the
//! normal, the depth and the points. Points that fall into the same 0.1 mm cell as an
//! earlier contact (and within 0.17 mm of it) are merged into it: a clearly deeper pair
//! takes the contact over, an equally deep one has its normal averaged in.
//!
//! The order of the pairs decides which points get the room in the contact array (4 between
//! two bodies, 32 against the track) and whose position a merged contact keeps, so the
//! pairs are consumed exactly in the query's order.

// `x * -1.0` is ODE's source and the game's machine code (a multiply, not a sign flip)
#![allow(clippy::neg_multiply)]

use crate::collide_btl::fetch_triangle;
use crate::contact::ContactGeom;
use crate::geom::{GeomRef, MeshPose, NUMC_MASK};
use crate::odemath::safe_normalize3;
use crate::opcode_obb::make_matrix;
use rustyac_math::sqrtf;

/// `MAX_POINTS` of a `LineContactSet`.
const MAX_POINTS: usize = 8;
/// `MAXCONTACT_X_NODE`, `CONTACTS_HASHSIZE`
const KEYS_PER_NODE: usize = 4;
const HASH_SIZE: usize = 256;
/// `CONTACT_DIFF_EPSILON` (and, in single precision, `CONTACT_NORMAL_ZERO`): 1e-5
const CONTACT_EPSILON: f32 = f32::from_bits(0x3727_c5ac);
/// The distance under which two points of one cell are one contact: 1.00001 * sqrt(3) / 10000.
const SAME_CONTACT_DISTANCE: f32 = f32::from_bits(0x3935_9ed9);
/// `CONTACT_POS_HASH_QUOTIENT`
const POS_HASH_QUOTIENT: f32 = 10000.0;

type Point = [f32; 3];

/// Which contact a hash-set entry (or the collider) means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// A contact of this call, by its index.
    Index(usize),
    /// The stand-in for "the array is full" (`dLocalContact`).
    Dummy,
}

/// `CONTACT_KEY_HASH_NODE`
#[derive(Clone, Copy)]
struct Node {
    keys: [(Slot, u32); KEYS_PER_NODE],
    count: i32,
}

/// `BuildPlane` @ 0x14038b060: the unit normal of a triangle and its plane's distance.
fn build_plane(s: &[Point; 3]) -> Option<[f32; 4]> {
    let e0 = [s[1][0] - s[0][0], s[1][1] - s[0][1], s[1][2] - s[0][2]];
    let e1 = [s[2][0] - s[0][0], s[2][1] - s[0][1], s[2][2] - s[0][2]];
    let mut n = [e1[2] * e0[1] - e1[1] * e0[2], e1[0] * e0[2] - e1[2] * e0[0], e1[1] * e0[0] - e1[0] * e0[1]];
    if !safe_normalize3(&mut n) {
        return None;
    }
    let dist = (s[0][0] * n[0] + s[0][1] * n[1]) + n[2] * s[0][2];
    Some([n[0], n[1], n[2], dist])
}

/// `PlaneClipSegment` @ 0x14038baf0: where the segment s1 -> s2 crosses the plane.
fn plane_clip_segment(s1: &Point, s2: &Point, n: &[f32], c: f32) -> Point {
    let dis1 = ((s1[0] * n[0] + s1[1] * n[1]) + s1[2] * n[2]) - c;
    let d = [s2[0] - s1[0], s2[1] - s1[1], s2[2] - s1[2]];
    let dis2 = (n[0] * d[0] + d[1] * n[1]) + d[2] * n[2];
    let t = -(dis1 / dis2);
    [t * d[0] + s1[0], d[1] * t + s1[1], d[2] * t + s1[2]]
}

/// `ClipConvexPolygonAgainstPlane` @ 0x14038b150: keeps the part of the polygon behind the
/// plane (`n`, `c`); at most eight points.
fn clip_convex_polygon_against_plane(n: &[f32], c: f32, contacts: &mut Vec<Point>) {
    let count = contacts.len();
    if count == 0 {
        return;
    }
    let mut clipped: Vec<Point> = Vec::with_capacity(MAX_POINTS);
    let mut prevclassif = 32000;
    for i in 0..=count {
        let vi = i % count;
        let p = contacts[vi];
        let d = ((n[1] * p[1] + n[0] * p[0]) + n[2] * p[2]) - c;
        let classif = if d > f32::from_bits(0x322b_cc77) { 1 } else { 0 };
        if classif == 0 {
            // back
            if i > 0 && prevclassif == 1 {
                // if prev was at front then the clip point comes first
                if clipped.len() >= MAX_POINTS {
                    prevclassif = classif;
                    continue;
                }
                clipped.push(plane_clip_segment(&contacts[i - 1], &contacts[vi], n, c));
            }
            if clipped.len() < MAX_POINTS && i < count {
                clipped.push(p);
            }
        } else if i > 0 && prevclassif == 0 && clipped.len() < MAX_POINTS {
            // front, and the previous one was behind
            clipped.push(plane_clip_segment(&contacts[i - 1], &contacts[vi], n, c));
        }
        prevclassif = classif;
    }
    *contacts = clipped;
}

/// `ClipPointsByTri` (inlined, without the triangle's own plane): `points` clipped by the
/// three planes through the edges of `tri`, along its normal.
fn clip_points_by_tri(points: &[Point; 3], tri: &[Point; 3], triplane: &[f32; 4]) -> Vec<Point> {
    let mut clipped: Vec<Point> = points.to_vec();
    for i in 0..3 {
        let j = (i + 1) % 3;
        // BuildEdgePlane
        let n = triplane;
        let e0 = [tri[j][0] - tri[i][0], tri[j][1] - tri[i][1], tri[j][2] - tri[i][2]];
        let mut plane = [n[2] * e0[1] - n[1] * e0[2], n[0] * e0[2] - n[2] * e0[0], n[1] * e0[0] - n[0] * e0[1]];
        if safe_normalize3(&mut plane) {
            let dist = (plane[1] * tri[i][1] + plane[0] * tri[i][0]) + plane[2] * tri[i][2];
            clip_convex_polygon_against_plane(&plane, dist, &mut clipped);
        }
        // (an edge without a length just leaves that clip out)
    }
    clipped
}

/// `MostDeepPoints` (inlined): the points furthest behind the plane (within 1e-6 of the
/// deepest so far, in the polygon's order) and how deep they are.
fn most_deep_points(points: &[Point], plane: &[f32; 4]) -> (f32, Vec<Point>) {
    let mut maxdeep = f32::NEG_INFINITY;
    let mut candidates: Vec<usize> = Vec::with_capacity(MAX_POINTS);
    for (i, p) in points.iter().enumerate() {
        let mut dist = ((plane[0] * p[0] + plane[1] * p[1]) + plane[2] * p[2]) - plane[3];
        dist = dist * -1.0f32;
        if dist > maxdeep {
            maxdeep = dist;
            candidates.clear();
            candidates.push(i);
        } else if dist + f32::from_bits(0x3586_37bd) >= maxdeep {
            candidates.push(i);
        }
    }
    (maxdeep, candidates.iter().map(|&i| points[i]).collect())
}

/// `FindTriangleTriangleCollision` @ 0x14038b3e0: the penetration of two triangles (below
/// zero: none), the normal to part them along and the deepest points.
fn find_triangle_triangle_collision(tri1: &[Point; 3], tri2: &[Point; 3]) -> (f32, [f32; 3], Vec<Point>) {
    let mut maxdeep = f32::INFINITY;
    // (when neither triangle has a plane the game's normal is whatever its stack held; no
    // point comes with it, so it is never used)
    let mut separating_normal = [0.0f32; 3];
    let mut deep_points1: Vec<Point> = Vec::new();

    // the face of triangle 1: triangle 2 clipped by the three edge planes of triangle 1
    if let Some(tri1plane) = build_plane(tri1) {
        let clipped = clip_points_by_tri(tri2, tri1, &tri1plane);
        let (deep, points) = most_deep_points(&clipped, &tri1plane);
        maxdeep = deep;
        deep_points1 = points;
        separating_normal = [tri1plane[0], tri1plane[1], tri1plane[2]];
    }

    // the face of triangle 2: triangle 1 clipped by the three edge planes of triangle 2
    if let Some(tri2plane) = build_plane(tri2) {
        let clipped = clip_points_by_tri(tri1, tri2, &tri2plane);
        let (dist, deep_points2) = most_deep_points(&clipped, &tri2plane);
        // comiss dist, maxdeep / jae keeps the first face
        if !(dist >= maxdeep) {
            return (dist, [tri2plane[0], tri2plane[1], tri2plane[2]], deep_points2);
        }
    }
    let normal = [separating_normal[0] * -1.0f32, separating_normal[1] * -1.0f32, separating_normal[2] * -1.0f32];
    (maxdeep, normal, deep_points1)
}

/// `UpdateContactKey` @ 0x14038c000: the hash of the 0.1 mm cell a point lies in.
fn contact_key(pos: &Point) -> u32 {
    let mut hash: u32 = 0;
    for i in 0..3 {
        let coord = (pos[i] * POS_HASH_QUOTIENT).floor();
        let h = coord.to_bits();
        hash = (hash << 4).wrapping_add(h >> 24) ^ (hash >> 28);
        hash = (hash << 4).wrapping_add((h >> 16) & 0xff) ^ (hash >> 28);
        hash = (hash << 4).wrapping_add((h >> 8) & 0xff) ^ (hash >> 28);
        hash = (hash << 4).wrapping_add(h & 0xff) ^ (hash >> 28);
        if i == 2 {
            break;
        }
        hash = hash.rotate_left(11);
    }
    hash
}

/// `MakeContactIndex`
fn contact_index(key: u32) -> usize {
    let i = key ^ (key >> 16);
    ((i ^ (i >> 8)) & 0xff) as usize
}

struct TriTri<'a> {
    flags: u32,
    g1: GeomRef,
    g2: GeomRef,
    out: &'a mut Vec<ContactGeom>,
    start: usize,
    /// `normal[3]` of each contact of this call: the length of its normal before it was
    /// normalised (1 for a contact that was not merged by averaging).
    lens: Vec<f32>,
    /// `_hashcontactset`, cleared for every call.
    set: Vec<Node>,
}

impl TriTri<'_> {
    fn count(&self) -> usize {
        self.out.len() - self.start
    }

    fn pos_of(&self, slot: Slot, pending: &Point) -> Point {
        match slot {
            Slot::Index(i) if i < self.count() => self.out[self.start + i].pos,
            // the slot being allocated (or the stand-in) holds the new point
            _ => *pending,
        }
    }

    /// `AddContactToNode` @ 0x14038ae90: the contact of the same cell that is close enough,
    /// or the new one (which is then filed in the node, if there is room).
    fn add_contact_to_node(&mut self, slot: Slot, key: u32, pos: &Point, index: usize) -> Slot {
        let keycount = self.set[index].count;
        for i in 0..keycount.max(0) as usize {
            let (found, found_key) = self.set[index].keys[i];
            if found_key == key {
                let f = self.pos_of(found, pos);
                let (dy, dz, dx) = (f[1] - pos[1], f[2] - pos[2], f[0] - pos[0]);
                let dist = sqrtf((dy * dy + dx * dx) + dz * dz);
                // comiss dist, threshold / jb: nearer, or a NaN
                if !(dist >= SAME_CONTACT_DISTANCE) {
                    return found;
                }
            }
        }
        if keycount < KEYS_PER_NODE as i32 {
            self.set[index].keys[keycount.max(0) as usize] = (slot, key);
            self.set[index].count += 1;
        }
        // (a full node does not take the key: that contact can never be merged into)
        slot
    }

    /// `AllocNewContact` @ 0x14038af40. `(contact, is_new)`: a new slot for the point, the
    /// contact it falls on, or nothing (the array is full and no contact is near).
    fn alloc_new_contact(&mut self, point: &Point) -> (Option<usize>, bool) {
        let max = (self.flags & NUMC_MASK) as usize;
        let count = self.count();
        let pending = if count != max { Slot::Index(count) } else { Slot::Dummy };
        let key = contact_key(point);
        let index = contact_index(key);
        let found = self.add_contact_to_node(pending, key, point, index);
        if found == pending {
            if let Slot::Index(_) = pending {
                // the next free slot: its position is the point's and never changes again
                self.out.push(ContactGeom { pos: *point, normal: [0.0; 3], depth: 0.0, g1: self.g1, g2: self.g2, side1: -1, side2: -1 });
                self.lens.push(1.0);
                return (Some(count), true);
            }
            // the array is full: the stand-in comes out of the set again
            let node = &mut self.set[index];
            let kc = node.count;
            if kc > 0 && node.keys[kc as usize - 1].0 == Slot::Dummy {
                node.count = kc - 1;
            }
            return (None, true);
        }
        match found {
            Slot::Index(i) => (Some(i), false),
            Slot::Dummy => (None, true),
        }
    }

    /// `FreeExistingContact` @ 0x14038b9a0: the contact leaves the set and the array; the
    /// last contact takes its place.
    fn free_existing_contact(&mut self, slot: usize) {
        // 1. out of its node
        let pos = self.out[self.start + slot].pos;
        let node = &mut self.set[contact_index(contact_key(&pos))];
        let last = node.count - 1;
        let mut k = 0;
        while k < last {
            if node.keys[k as usize].0 == Slot::Index(slot) {
                node.keys[k as usize] = node.keys[last as usize];
                break;
            }
            k += 1;
        }
        // (unconditional: if the contact was not among the others, the last entry is dropped)
        node.count = last.max(0);

        // 2. the array stays dense
        let last_index = self.count() - 1;
        if slot != last_index {
            self.out[self.start + slot] = self.out[self.start + last_index];
            self.lens[slot] = self.lens[last_index];
            let pos = self.out[self.start + last_index].pos;
            let node = &mut self.set[contact_index(contact_key(&pos))];
            let last2 = node.count - 1;
            let mut k = 0;
            while k < last2 {
                if node.keys[k as usize].0 == Slot::Index(last_index) {
                    break;
                }
                k += 1;
            }
            // (no check that entry k really was the moved contact; with an empty node this
            // writes entry 0, which nothing reads)
            node.keys[k.max(0) as usize].0 = Slot::Index(slot);
        }
        self.out.truncate(self.start + last_index);
        self.lens.truncate(last_index);
    }

    /// `PushNewContact` @ 0x14038bbe0.
    fn push_new_contact(&mut self, tri1: i32, tri2: i32, point: &Point, normal: &[f32; 3], depth: f32) {
        let (contact, is_new) = self.alloc_new_contact(point);
        let Some(slot) = contact else {
            // the array is full and no existing contact is near: the point is dropped
            return;
        };
        let at = self.start + slot;
        if is_new {
            let c = &mut self.out[at];
            c.normal = *normal;
            c.depth = depth;
            c.side1 = tri1;
            c.side2 = tri2;
            self.lens[slot] = 1.0;
            return;
        }
        // the point fell on an existing contact
        let depth_difference = depth - self.out[at].depth;
        if depth_difference > CONTACT_EPSILON {
            // the deeper pair takes the contact over; its place stays
            let c = &mut self.out[at];
            c.normal = *normal;
            c.depth = depth;
            c.side1 = tri1;
            c.side2 = tri2;
            self.lens[slot] = 1.0;
        } else if depth_difference >= -CONTACT_EPSILON {
            // as deep: the normals are averaged. (The game turns the new normal round first
            // when the contact's first geom is this call's second; within one call it never is.)
            let old_len = self.lens[slot];
            let c = &mut self.out[at];
            let n0 = old_len * c.normal[0] + normal[0];
            let n1 = old_len * c.normal[1] + normal[1];
            let n2 = old_len * c.normal[2] + normal[2];
            c.normal = [n0, n1, n2];
            let len = sqrtf((n0 * n0 + n1 * n1) + n2 * n2);
            if len > CONTACT_EPSILON {
                let inv = 1.0f32 / len;
                c.normal = [inv * n0, inv * n1, inv * n2];
                self.lens[slot] = len;
                // no merge callback in the game: the sides are not triangles any more
                c.side1 = -1;
                c.side2 = -1;
            } else {
                // the two normals cancelled
                self.free_existing_contact(slot);
            }
        }
        // else: the new point is shallower and is left out
    }

    /// `TriTriContacts` @ 0x14038be80.
    fn tri_tri_contacts(&mut self, tr1: &[Point; 3], tr2: &[Point; 3], tri1: i32, tri2: i32) {
        let (depth, normal, points) = find_triangle_triangle_collision(tr1, tr2);
        // comiss depth, 0 / jae go on
        if !(depth >= 0.0) {
            return;
        }
        let mask = self.flags & 0x8000_ffff;
        for point in &points {
            self.push_new_contact(tri1, tri2, point, &normal, depth);
            // only with CONTACTS_UNIMPORTANT, which the game never sets
            if self.count() as u32 | 0x8000_0000 == mask {
                break;
            }
        }
    }
}

/// `dCollideTTL` @ 0x14038c0d0: the contacts of two meshes, appended to `out` (`g1` is the
/// first mesh, `g2` the second). The low 16 bits of `flags` are the most contacts.
pub fn collide_ttl(m1: &MeshPose, m2: &MeshPose, flags: u32, g1: GeomRef, g2: GeomRef, out: &mut Vec<ContactGeom>) {
    // the two places as OPCODE's matrices; the pairs of triangles that meet
    let world0 = make_matrix(&m1.pos, &m1.r);
    let world1 = make_matrix(&m2.pos, &m2.r);
    let Some(pairs) = crate::opcode_tree::tree_query(&m1.data.model, &m1.data.mesh, &world0, &m2.data.model, &m2.data.mesh, &world1) else {
        return;
    };
    let start = out.len();
    let mut state = TriTri {
        flags,
        g1,
        g2,
        out,
        start,
        lens: Vec::new(),
        set: vec![Node { keys: [(Slot::Dummy, 0); KEYS_PER_NODE], count: 0 }; HASH_SIZE],
    };
    let mask = flags & 0x8000_ffff;
    for &(id0, id1) in &pairs {
        let v1 = fetch_triangle(m1, id0);
        let v2 = fetch_triangle(m2, id1);
        state.tri_tri_contacts(&v1, &v2, id0 as i32, id1 as i32);
        if state.count() as u32 | 0x8000_0000 == mask {
            break;
        }
    }
}
