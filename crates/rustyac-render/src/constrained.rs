// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ConstrainedObjectsManager` (0x78 bytes): push rods, steering arms and drive shafts that
//! aim at another node. A node of a car's model whose name starts with `DIR_` is a target; the
//! node named by the rest of the name is turned every frame so that its own X axis points at
//! the target. `addModel` 0x140077040 / `addNodes` 0x140077050, `updateConstraints`
//! 0x140077320 (a handler of `evOnPostUpdate`, between the animated suspension's and
//! `CarLodManager::updateLodVisibility`).

use rustyac_math::{acosf, cosf, sinf, sqrtf};
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f};

use crate::scene::{NodeId, Scene};

/// `ConstrainedObjectDef` (0x50 bytes).
pub struct ConstrainedObjectDef {
    pub node: NodeId,
    pub org_matrix: Mat44f,
    pub target: NodeId,
}

#[derive(Default)]
pub struct ConstrainedObjectsManager {
    pub objects: Vec<ConstrainedObjectDef>,
}

impl ConstrainedObjectsManager {
    /// `ConstrainedObjectsManager::addModel` 0x140077040: once per level of detail, on its
    /// model's top node.
    pub fn add_model(&mut self, scene: &Scene, root: NodeId) {
        self.add_nodes(scene, root, root);
    }

    /// `addNodes` 0x140077050.
    fn add_nodes(&mut self, scene: &Scene, root: NodeId, node: NodeId) {
        let name = &scene.nodes[node].name;
        if let Some(suffix) = name.strip_prefix("DIR_") {
            println!("Found target object: {name} looking for constrained node:{suffix}");
            match scene.find_child_by_name(root, suffix, true) {
                Some(t) => self.objects.push(ConstrainedObjectDef { node: t, org_matrix: scene.nodes[t].matrix, target: node }),
                None => println!("WARNING: Costrained node {suffix} not found"),
            }
        }
        for &child in &scene.nodes[node].children {
            self.add_nodes(scene, root, child);
        }
    }

    /// `ConstrainedObjectsManager::updateConstraints` 0x140077320.
    pub fn update_constraints(&self, scene: &mut Scene) {
        for def in &self.objects {
            let Some(parent) = scene.nodes[def.node].parent else {
                continue;
            };
            let inv = xm_matrix_inverse(&scene.get_world_matrix(parent)).m;
            let tw = scene.get_world_matrix(def.target).m;
            let (tx, ty, tz) = (tw[3][0], tw[3][1], tw[3][2]);
            let org = &def.org_matrix.m;
            // the target in the parent's axes, from where the node stands
            let dx = ((((ty * inv[1][0]) + (tx * inv[0][0])) + (tz * inv[2][0])) + inv[3][0]) - org[3][0];
            let dy = ((((ty * inv[1][1]) + (tx * inv[0][1])) + (tz * inv[2][1])) + inv[3][1]) - org[3][1];
            let dz = ((((ty * inv[1][2]) + (tx * inv[0][2])) + (tz * inv[2][2])) + inv[3][2]) - org[3][2];
            let len = sqrtf(((dy * dy) + (dx * dx)) + (dz * dz));
            let (mut nx, mut ny, mut nz) = (dx, dy, dz);
            if len != 0.0 && !len.is_nan() {
                let inv_len = 1.0 / len;
                nx = dx * inv_len;
                ny = dy * inv_len;
                nz = dz * inv_len;
            }
            let (ox, oy, oz) = (org[0][0], org[0][1], org[0][2]);
            let dot = ((ox * nx) + (oy * ny)) + (oz * nz);
            let mut angle = 0.0f32;
            if !(dot.abs() >= 1.0) {
                angle = acosf(dot);
            }
            let mut ax = (oy * nz) - (oz * ny);
            let mut ay = (oz * nx) - (ox * nz);
            let mut az = (ox * ny) - (oy * nx);
            let alen = sqrtf(((ay * ay) + (ax * ax)) + (az * az));
            if alen != 0.0 && !alen.is_nan() {
                let inv_len = 1.0 / alen;
                ax *= inv_len;
                ay *= inv_len;
                az *= inv_len;
            }
            let c = cosf(angle);
            let s = sinf(angle);
            let omc = 1.0 - c;
            let r = Mat44f {
                m: [
                    [((ax * ax) * omc) + c, (az * s) + ((ax * ay) * omc), ((ax * az) * omc) - (ay * s), 0.0],
                    [((ax * ay) * omc) - (az * s), ((ay * ay) * omc) + c, (ax * s) + ((ay * az) * omc), 0.0],
                    [(ay * s) + ((ax * az) * omc), ((ay * az) * omc) - (ax * s), ((az * az) * omc) + c, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            };
            let mut result = xm_matrix_multiply(&def.org_matrix, &r);
            result.m[3][0] = org[3][0];
            result.m[3][1] = org[3][1];
            result.m[3][2] = org[3][2];
            scene.nodes[def.node].matrix = result;
        }
    }
}
