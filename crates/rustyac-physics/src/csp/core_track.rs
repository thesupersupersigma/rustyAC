// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5i-a part 1: CSP's collision track (its trimeshes in their hash space, the ray meshes,
//! the world's collision settings) built from rustyAC's own loaded [`Track`], as CSP 0.3.2-preview2
//! builds it from the game's (docs/csp/core.md, C5i-a).

use crate::track::Track;
use rustyac_ode::csp_collide::Side;
use rustyac_ode::csp_collision::{Geom, Load, Shape, TrackMesh, GEOM_AABB_BAD, GEOM_DIRTY, GEOM_ENABLED};
use rustyac_ode::csp_core::RayMesh;

/// What `Core::set_track` and `Core::set_rays` take, and the cars' space settings.
pub struct CspTrack {
    /// the track space's members in its list order (newest first)
    pub geoms: Vec<Geom>,
    pub meshes: Vec<TrackMesh>,
    /// CSP's ray mesh list (newest first), each naming its geom in `geoms`
    pub rays: Vec<RayMesh>,
    pub level: i32,
    pub count: i32,
    pub levels: (i32, i32),
    pub cars_level: i32,
    pub static_step: bool,
    pub aabb_limit: i32,
    pub full_box_box: bool,
    pub cars_levels: (i32, i32),
    pub prefilter: bool,
    pub edges: bool,
    pub load: Load,
}

/// `dxTriMeshData::Build` in CSP's double ODE: the box of all vertices, in f64.
pub(crate) fn bounds(vertices: &[[f32; 3]]) -> [f64; 6] {
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in vertices {
        for k in 0..3 {
            let x = v[k] as f64;
            if x > max[k] {
                max[k] = x;
            }
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN vertex sets min like CSP's compare
            if !(x >= min[k]) {
                min[k] = x;
            }
        }
    }
    let c: [f64; 3] = std::array::from_fn(|k| (min[k] + max[k]) * 0.5);
    [c[0], c[1], c[2], max[0] - c[0], max[1] - c[1], max[2] - c[2]]
}

/// The track as CSP's collision holds it once loaded.
pub fn build(track: &Track) -> CspTrack {
    let n = track.world.meshes.len();
    let (mut geoms, mut meshes, mut rays) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for (i, m) in track.world.meshes.iter().enumerate().rev() {
        let b = bounds(&m.data.mesh.vertices);
        // a one-triangle mesh has no tree (OPCODE's single node): its data is not kept (as the
        // oracle reads CSP: a tree kind the port does not collide with) and AABBTest cannot walk it
        let tree = !m.data.model.single_node;
        let shape = Shape::Mesh { mesh: tree.then_some(meshes.len()), tc_on: true, key: 0, tree_test: tree && !m.data.model.nodes.is_empty(), bounds: b };
        let r = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let side = Side { class: 8, ..Default::default() };
        geoms.push(Geom { id: i + 1, side, flags: GEOM_DIRTY | GEOM_AABB_BAD | 8 | GEOM_ENABLED, category: 1, collide: m.collide_bits, offset: None, pos: [0.0; 3], r, aabb: [0.0; 6], shape });
        if tree {
            meshes.push(TrackMesh { mesh: m.data.mesh.clone(), model: m.data.model.clone() });
        }
        rays.push(RayMesh { geom: n - 1 - i, vertices: m.data.mesh.vertices.clone(), indices: m.data.mesh.indices.clone(), two_sided: true });
    }
    rays.reverse();
    CspTrack { geoms, meshes, rays, level: 0, count: n as i32, levels: (2, 11), cars_level: 0, static_step: false, aabb_limit: 8, full_box_box: true, cars_levels: (0, 5), prefilter: true, edges: false, load: Load::default() }
}
