// SPDX-License-Identifier: BSD-3-Clause

//! Contact joints from one pair's contacts, as Custom Shaders Patch 0.3.2 (build 4305) makes them
//! (+0x92c9f0) for a car's box against the track's trimesh: fixed surface parameters (a box without
//! user data, as a car's collider, gets its own and keeps only contacts near the body's up axis),
//! CSP's softer erp/cfm for deep contacts; two cars' meshes get the default surface and a joint
//! between both bodies. Only the paths CSP takes on a Spa drive are here; the others (a disabled
//! body, a body-less trimesh against a trimesh, surface mixing, depth from contact speed, moving
//! surfaces, the deferred batches, batched car-car joints) give [`Unhandled`].

use crate::csp_btl::ContactGeom;
use crate::csp_contact::Contact;
use crate::world::{Body, BODY_DISABLED};

/// One geom of the pair as the joint maker sees it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Side {
    /// `dGeomGetClass` (1 box, 8 trimesh; CSP's 10 counts as a box)
    pub class: i32,
    /// `geom->data` is null (a tagged pointer, low bit set, counts as null)
    pub no_data: bool,
    pub body: Option<usize>,
    /// the body's user data byte +159, bit 0: friction 0.4 instead of 0.1 for a car's collider
    pub grip: bool,
    /// user data bytes +152 set and +153 clear: CSP's moving-surface joint (not ported)
    pub special: bool,
    /// the reduction weight of +0x92b9d0: user data byte +159 bit 1 gives 1.0, else bit 2 0.3
    pub weight: f32,
}

/// CSP switches the joint maker reads; the port handles them all off.
#[derive(Clone, Copy, Debug, Default)]
pub struct Switches {
    /// `byte 0x27f9197`: depth from the contact speed
    pub depth_from_speed: bool,
    /// dword `0x26b9e88`: surface mixing from both geoms' materials
    pub surface_mix: bool,
    /// `qword 0x2803208` byte 6: softer erp/cfm for deep contacts
    pub soft_deep: bool,
    /// `qword 0x2803220` byte 1: car-against-car joints
    pub car_pairs: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unhandled(pub &'static str);

/// A contact joint as CSP adds it: surface, the contact as given, bodies (`b1` none when one
/// side has no body; `reverse` when only the second geom has one).
#[derive(Clone, Copy, Debug)]
pub struct NewJoint {
    pub contact: Contact,
    pub geom: ContactGeom,
    pub b0: usize,
    pub b1: Option<usize>,
    pub reverse: bool,
}

fn is_box(class: i32) -> bool {
    class == 1 || class == 10
}

/// CSP's softer erp/cfm for a contact deeper than `limit` (+0x91c520): the spring and damper
/// behind erp/cfm (per unit step) scaled by stiffness factors that grow with depth past `limit`
/// and twice `limit`, all f32 but the last part.
pub fn soften(erp: f64, cfm: f64, depth: f32, limit: f32) -> (f64, f64) {
    if !(depth > limit) {
        return (erp, cfm);
    }
    let over = depth - (limit + limit);
    let over = if over > 0.0 { over } else { 0.0 }; // maxss
    let past = depth - limit;
    let past = if past < limit { past } else { limit }; // minss
    let k = over * 400.0 + past * 20.0 + limit;
    let d = (over * 20.0 + past) * 20.0 + limit;
    let inv_cfm = 1.0 / cfm;
    let inv_depth = 1.0 / depth as f64;
    let kp = k as f64 * (inv_cfm * erp) * inv_depth;
    let kd = d as f64 * ((1.0 - erp) * inv_cfm) * inv_depth;
    let cfm = 1.0 / (kd + kp);
    (cfm * kp, cfm)
}

/// The joints for `contacts` of the pair (`s1`, `s2`), in the order CSP adds them.
pub fn contact_joints(contacts: &[ContactGeom], s1: &Side, s2: &Side, bodies: &[Body<f64>], sw: &Switches) -> Result<Vec<NewJoint>, Unhandled> {
    if s1.class == 8 && s2.class == 8 && s1.body.is_some() != s2.body.is_some() {
        return Err(Unhandled("trimesh against trimesh"));
    }
    let two = s1.body.is_some() && s2.body.is_some();
    if two && sw.car_pairs {
        return Err(Unhandled("batched car-car joints"));
    }
    if s1.special || s2.special {
        return Err(Unhandled("moving surface"));
    }
    if sw.surface_mix || sw.depth_from_speed {
        return Err(Unhandled("switch on"));
    }
    // a box without user data (a car's collider) against a trimesh: CSP's own surface, and only
    // contacts whose normal is within about 25 degrees of the body's up axis
    let up = ((is_box(s1.class) && s1.no_data) || (is_box(s2.class) && s2.no_data)) && (s1.class == 8 || s2.class == 8);
    let side = if s1.body.is_some() { s1 } else { s2 };
    let Some(body) = side.body else { return Err(Unhandled("no body")) };
    // CSP wakes a body (dBodyEnable) for a deep or fast contact; a disabled car body was never
    // seen; a body flagged 0x200 takes the batched path
    for b in [s1.body, s2.body].into_iter().flatten() {
        if bodies[b].flags & BODY_DISABLED != 0 {
            return Err(Unhandled("disabled body"));
        }
        if bodies[b].flags & 0x200 != 0 {
            return Err(Unhandled("a body on the batched path"));
        }
    }
    let b1 = if two { s2.body } else { None };
    let r = &bodies[body].r;
    let mut out = Vec::new();
    for c in contacts {
        let mut contact = Contact {
            mode: 0x701c,
            mu: 0.25,
            bounce: 0.01,
            soft_erp: 0.3,
            soft_cfm: 0.0001,
            pos: [c.pos[0], c.pos[1], c.pos[2], 0.0],
            normal: [c.normal[0], c.normal[1], c.normal[2], 0.0],
            depth: c.depth,
            ..Default::default()
        };
        if up {
            let n = c.normal.map(|v| v as f32 as f64);
            if n[1] * r[5] + n[0] * r[1] + n[2] * r[9] < 0.9 {
                continue;
            }
            (contact.mu, contact.bounce, contact.soft_erp, contact.soft_cfm) = (if side.grip { 0.4 } else { 0.1 }, 0.0, 0.7142857142857143, 0.0009523809523809524);
        }
        let limit = if up { 0.07f32 } else { 0.1 };
        if sw.soft_deep && c.depth > limit as f64 {
            (contact.soft_erp, contact.soft_cfm) = soften(contact.soft_erp, contact.soft_cfm, c.depth as f32, limit);
        }
        out.push(NewJoint { contact, geom: *c, b0: body, b1, reverse: s1.body.is_none() });
    }
    Ok(out)
}
