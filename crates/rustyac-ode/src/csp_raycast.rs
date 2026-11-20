// SPDX-License-Identifier: BSD-3-Clause

//! ODE's ray against the track as Custom Shaders Patch 0.3.2 (build 4305) casts it when Embree
//! does not answer (+0x926270): before its scene is built, a ray straight down that met a
//! triangle's back, or with Embree off. `dGeomRaySet` (+0x169dc30), the ray's box (+0x169db10),
//! `dSpaceCollide2` (+0x168a5b0) of the ray against the track's hash space, whose pairs come
//! back as (ray, trimesh) to the callback (+0x932f70): `dCollide` (+0x1687280) for one contact,
//! the swapped entry of `dCollideRTL` (+0x16b2490), the nearest contact kept. Double precision
//! like the rest of CSP's ODE, but OPCODE's walk inside a mesh is single precision and the
//! same code as the game's ([`crate::collision::stab`]).

use crate::collision::stab;
use crate::csp_collide::Unhandled;
use crate::csp_collision::{enabled, Collision, Shape};
use crate::csp_opcode::aabb_test;
use crate::csp_space::overlap;
use crate::odemath::safe_normalize3;

/// The ray geom's own flags (+0xc): OPCODE's first contact, back-face culling, closest hit.
pub const RAY_FIRST_CONTACT: u32 = 0x1_0000;
pub const RAY_BACKFACE_CULL: u32 = 0x2_0000;
pub const RAY_CLOSEST_HIT: u32 = 0x4_0000;

/// The ray geom: flags (+0xc), category and collide bits (+0x90, +0x94), length (+0xa0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub flags: u32,
    pub category: u32,
    pub collide: u32,
    pub length: f64,
}

/// What +0x926270 writes: the contact's position and normal in single precision, and the
/// trimesh (`c.geoms` index) whose user data CSP hands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OdeHit {
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub geom: usize,
}

/// The ray from `org` along `dir` against the track of `c`; none when nothing is nearer than
/// `f32::MAX`.
pub fn cast(c: &Collision, org: [f32; 3], dir: [f32; 3], ray: &Ray) -> Result<Option<OdeHit>, Unhandled> {
    if ray.flags & (RAY_FIRST_CONTACT | RAY_BACKFACE_CULL | RAY_CLOSEST_HIT) != RAY_FIRST_CONTACT | RAY_BACKFACE_CULL {
        return Err(Unhandled("a ray not in first-contact mode with culling"));
    }
    // dGeomRaySet: the position, the direction normalised into the rotation's third column
    let pos = org.map(f64::from);
    let mut d = dir.map(f64::from);
    if !safe_normalize3::<f64>(&mut d) {
        d = [1.0, 0.0, 0.0];
    }
    // +0x169db10
    let mut aabb = [0.0; 6];
    for k in 0..3 {
        let e = ray.length * d[k] + pos[k];
        (aabb[2 * k], aabb[2 * k + 1]) = if e <= pos[k] { (e, pos[k]) } else { (pos[k], e) };
    }
    let mut found = Vec::new();
    c.table.collide2(&aabb, |t| found.push(c.track[t]));
    // the callback's accumulator: depth, contact
    let mut best = (f32::MAX as f64, None);
    for t in found {
        let g = &c.geoms[t];
        // the pair test (+0x1689bf0's): enabled, bodies, categories, boxes, the trimesh's AABBTest
        let Shape::Mesh { mesh, tree_test, .. } = g.shape else { return Err(Unhandled("a track geom not a trimesh")) };
        if !enabled(g) || g.side.body.is_some() || (g.category & ray.collide == 0 && g.collide & ray.category == 0) || !overlap(&g.aabb, &aabb) {
            continue;
        }
        let mesh = mesh.ok_or(Unhandled("a trimesh tree kind not ported"))?;
        let tm = &c.meshes[mesh];
        if tree_test && c.aabb_limit != 0 && ray.flags & 0x8_0000 == 0 && !aabb_test(&tm.model.nodes, &aabb, c.aabb_limit) {
            continue;
        }
        if !tm.model.built {
            continue;
        }
        // dCollideRTL: OPCODE gets the ray relative to the mesh and the mesh's rotation alone
        let r = &g.r;
        let w = [[r[0], r[4], r[8]], [r[1], r[5], r[9]], [r[2], r[6], r[10]], [0.0, 0.0, 0.0]].map(|row| row.map(|x| x as f32));
        let o = [(pos[0] - g.pos[0]) as f32, (pos[1] - g.pos[1]) as f32, (pos[2] - g.pos[2]) as f32];
        let Some(face) = stab(&tm.mesh, &tm.model, &w, &o, &d.map(|x| x as f32), ray.length as f32) else { continue };
        // the triangle in the world (+0x1694600), its normal, the contact along the ray
        let v = tm.mesh.triangle(face.face_id).map(|p| {
            let p = p.map(f64::from);
            [0, 1, 2].map(|i| ((p[1] * r[4 * i + 1] + p[0] * r[4 * i]) + p[2] * r[4 * i + 2]) + g.pos[i])
        });
        let (a, b, cc) = (v[0], v[1], v[2]);
        let mut n = [
            (cc[1] - a[1]) * (b[2] - a[2]) - (cc[2] - a[2]) * (b[1] - a[1]),
            (cc[2] - a[2]) * (b[0] - a[0]) - (cc[0] - a[0]) * (b[2] - a[2]),
            (cc[0] - a[0]) * (b[1] - a[1]) - (cc[1] - a[1]) * (b[0] - a[0]),
        ];
        if !safe_normalize3::<f64>(&mut n) {
            continue;
        }
        let depth = face.distance as f64;
        if depth < best.0 {
            // dCollide's swapped entry turns the normal round
            let p = [0, 1, 2].map(|i| depth * d[i] + pos[i]);
            best = (depth, Some(OdeHit { point: p.map(|x| x as f32), normal: n.map(|x| -x as f32), geom: t }));
        }
    }
    Ok(if best.0 >= 0.0 { best.1 } else { None })
}
