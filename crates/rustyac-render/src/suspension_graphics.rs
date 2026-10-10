// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SuspensionGraphics` (0xf0 bytes): a rod of the car's model that aims from a point on one
//! node at a point on another (`data/suspension_graphics.ini`, `[CONNECTION_n]`).
//! `SuspensionGraphicsGenerator::generateConnections` 0x1401b4280, `getNodeFromName`
//! 0x1401b4da0, `SuspensionGraphics::SuspensionGraphics` 0x1401b39d0, `update` 0x1401b3ca0.
//!
//! No car of the game as it is installed gets a rod from it: four of the six files number
//! their sections from 10 (the loop stops at the first missing number) and the other two name
//! a node their models do not have.

use rustyac_math::{acosf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::scene::{NodeId, Scene};

pub struct SuspensionGraphics {
    object_node: NodeId,
    dst_node: NodeId,
    /// `dstPivot`: the point on the destination node, and the same point in the rod's frame
    dst_pivot: [Vec3f; 2],
    /// `srcPivot[1]`: the point on the source node, in the rod's frame
    src_pivot_local: Vec3f,
    original_matrix: Mat44f,
}

/// `Node::worldToLocal` 0x14020e730: a point through the inverse of the node's world matrix.
fn world_to_local(scene: &Scene, node: NodeId, v: &Vec3f) -> Vec3f {
    let w = xm_matrix_inverse(&scene.get_world_matrix(node));
    let m = &w.m;
    Vec3f::new(
        (((v.y * m[1][0]) + (v.x * m[0][0])) + (v.z * m[2][0])) + m[3][0],
        (((v.x * m[0][1]) + (v.y * m[1][1])) + (v.z * m[2][1])) + m[3][1],
        (((v.x * m[0][2]) + (v.y * m[1][2])) + (v.z * m[2][2])) + m[3][2],
    )
}

/// Divides by the length unless that is zero (or not a number).
fn normalize(v: &mut [f32; 3]) {
    let len = sqrtf((v[1] * v[1] + v[0] * v[0]) + v[2] * v[2]);
    if len != 0.0 && !len.is_nan() {
        let inv = 1.0 / len;
        v[0] *= inv;
        v[1] *= inv;
        v[2] *= inv;
    }
}

impl SuspensionGraphics {
    /// `SuspensionGraphicsGenerator::generateConnections` 0x1401b4280. `sus_transform(i)`:
    /// `ISuspensionAvatar::getSusTransform`, the suspension node of wheel `i` (an animated
    /// car: the hub of the first level of detail).
    pub fn generate(scene: &Scene, ini: Option<&IniReader>, body_transform: NodeId, sus_transform: &dyn Fn(usize) -> Option<NodeId>) -> Vec<SuspensionGraphics> {
        let mut out = Vec::new();
        let Some(ini) = ini else { return out };
        // getNodeFromName 0x1401b4da0: the four wheel names, and the body for anything else
        let node_from_name = |name: &str| match name {
            "WHEEL_LF" => sus_transform(0),
            "WHEEL_RF" => sus_transform(1),
            "WHEEL_LR" => sus_transform(2),
            "WHEEL_RR" => sus_transform(3),
            _ => Some(body_transform),
        };
        let mut index = 0;
        loop {
            let section = format!("CONNECTION_{index}");
            if !ini.has_section(&section) {
                break;
            }
            index += 1;
            let names = [ini.get_string(&section, "DST_OBJECT"), ini.get_string(&section, "SRC_OBJECT"), ini.get_string(&section, "TARGET_OBJECT")];
            let dst_node = node_from_name(&names[0]);
            let src_node = node_from_name(&names[1]);
            let object_node = scene.find_child_by_name(body_transform, &names[2], true);
            let pivot_on_dst = ini.get_float3(&section, "DST_POINT").unwrap_or([0.0; 3]);
            let pivot_on_src = ini.get_float3(&section, "SRC_POINT").unwrap_or([0.0; 3]);
            let (Some(dst_node), Some(src_node), Some(object_node)) = (dst_node, src_node, object_node) else {
                for (node, name) in [dst_node, src_node, object_node].iter().zip(&names) {
                    if node.is_none() {
                        println!("[ERROR]: SUS GRAPHICS cannot find: {name}?");
                    }
                }
                continue;
            };
            // SuspensionGraphics::SuspensionGraphics 0x1401b39d0
            let src = Vec3f::new(pivot_on_src[0], pivot_on_src[1], pivot_on_src[2]);
            let dst = Vec3f::new(pivot_on_dst[0], pivot_on_dst[1], pivot_on_dst[2]);
            let world = scene.local_to_world(src_node, &src);
            let src_pivot_local = world_to_local(scene, object_node, &world);
            let world = scene.local_to_world(dst_node, &dst);
            let dst_local = world_to_local(scene, object_node, &world);
            out.push(SuspensionGraphics { object_node, dst_node, dst_pivot: [dst, dst_local], src_pivot_local, original_matrix: scene.nodes[object_node].matrix });
        }
        out
    }

    /// `SuspensionGraphics::update` 0x1401b3ca0 (the time step is not read).
    pub fn update(&self, scene: &mut Scene) {
        scene.nodes[self.object_node].matrix = self.original_matrix;
        let world = scene.local_to_world(self.dst_node, &self.dst_pivot[0]);
        let cur = world_to_local(scene, self.object_node, &world);
        let (dl, sl) = (self.dst_pivot[1], self.src_pivot_local);
        let mut v1 = [dl.x - sl.x, dl.y - sl.y, dl.z - sl.z];
        let mut v2 = [cur.x - sl.x, cur.y - sl.y, cur.z - sl.z];
        normalize(&mut v1);
        normalize(&mut v2);
        let dot = (v1[1] * v2[1] + v1[0] * v2[0]) + v1[2] * v2[2];
        // not a clamp: outside the open range (and for a NaN) the angle is 0
        let angle = if !(dot > -1.0) || dot >= 1.0 { 0.0 } else { acosf(dot) };
        let mut axis = [v1[1] * v2[2] - v1[2] * v2[1], v1[2] * v2[0] - v1[0] * v2[2], v1[0] * v2[1] - v1[1] * v2[0]];
        normalize(&mut axis);
        let neg = [sl.x * -1.0, sl.y * -1.0, sl.z * -1.0];
        let r = Mat44f::create_from_axis_angle(&Vec3f::new(axis[0], axis[1], axis[2]), angle);
        let t1 = Mat44f { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [neg[0], neg[1], neg[2], 1.0]] };
        let a = xm_matrix_multiply(&t1, &r);
        let t2 = xm_matrix_inverse(&t1);
        let b = xm_matrix_multiply(&a, &t2);
        scene.nodes[self.object_node].matrix = xm_matrix_multiply(&b, &self.original_matrix);
    }
}
