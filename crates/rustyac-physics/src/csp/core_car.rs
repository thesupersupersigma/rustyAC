// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5i-a part 1: a car's colliders as CSP 0.3.2-preview2's core takes them (the floor boxes,
//! the box floor of `EXTEND_COLLIDER_BOXES`, the collider mesh's built OPCODE data), from rustyAC's
//! own car loaders (docs/csp/core.md, C5i-a).

use std::path::Path;

use crate::car::colliders::{self, BoxColliderDef};
use crate::data::ini::{append_path, IniReader};
use rustyac_ode::csp_collision::{Shape, TrackMesh};
use rustyac_ode::opcode::{MeshInterface, Model};

/// The collider mesh: its built data, its shape, and where it sits on the body (row by row).
pub struct CspMesh {
    pub mesh: TrackMesh,
    pub shape: Shape,
    pub matrix: [f32; 16],
}

pub struct CspCar {
    pub boxes: Vec<BoxColliderDef>,
    /// `+0x27f9950` while the boxes are made: `max(INERTIA.z * 0.2, INERTIA.y * 0.5)` when
    /// [`extend_boxes`], else 0 (`+0x413c20` around `CarColliderManager::loadINI`)
    pub min_height: f32,
    pub mesh: Option<CspMesh>,
}

/// The car whose data is in `data_folder` (`car.ini`, `colliders.ini`), its `collider.kn5` under
/// `game_root`.
pub fn build(data_folder: &Path, game_root: &Path, name: &str) -> Result<CspCar, String> {
    let loaded = colliders::load(data_folder, Some(game_root), name)?;
    let min_height = box_floor(data_folder)?;
    let mesh = loaded.mesh.map(|m| -> Result<CspMesh, String> {
        let matrix: [f32; 16] = std::array::from_fn(|i| m.matrix.m[i / 4][i % 4]);
        let vertices = clamp_mesh(data_folder, &m.vertices, &matrix)?.unwrap_or_else(|| m.vertices.clone());
        Ok(mesh(vertices, m.indices.clone(), matrix))
    });
    let mesh = mesh.transpose()?;
    Ok(CspCar { boxes: loaded.boxes, min_height, mesh })
}

/// The box floor (`+0x27f9950`) while the car's boxes are made.
pub fn box_floor(data_folder: &Path) -> Result<f32, String> {
    let inertia = IniReader::load(&append_path(data_folder, "car.ini"))?.get_float3("BASIC", "INERTIA")?;
    Ok(if extend_boxes(data_folder)? { (inertia[2] * 0.2).max(inertia[1] * 0.5) } else { 0.0 })
}

/// A mesh collider's built data and shape (vertices already clamped).
pub fn mesh(vertices: Vec<[f32; 3]>, indices: Vec<u16>, matrix: [f32; 16]) -> CspMesh {
    let bounds = super::core_track::bounds(&vertices);
    let mesh = MeshInterface::new(vertices, indices);
    let model = Model::build(&mesh);
    CspMesh { mesh: TrackMesh { mesh, model }, shape: Shape::Mesh { mesh: Some(0), tc_on: true, key: 0, tree_test: true, bounds }, matrix }
}

/// `+0x413850`: whether `EXTEND_COLLIDER_BOXES` (on by default) acts on the car: no `script.lua`
/// in its data, and a `colliders.ini` without `[_EXTENSION]` and without `MAX_WEAR` in any
/// `COLLIDER…` section.
fn extend_boxes(data_folder: &Path) -> Result<bool, String> {
    if crate::data::exists(&append_path(data_folder, "script.lua")) {
        return Ok(false);
    }
    let path = append_path(data_folder, "colliders.ini");
    if !crate::data::exists(&path) {
        return Ok(true);
    }
    let ini = IniReader::load(&path)?;
    Ok(!ini.has_section("_EXTENSION") && !ini.sections().any(|s| s.starts_with("COLLIDER") && ini.has_key(s, "MAX_WEAR")))
}

/// `CLAMP_MESH_COLLIDERS` (on by default, as `ENABLE_MESH_COLLIDER_SOONER`), CSP's
/// `Car::initColliderMesh` detour `+0x58e720`: in the body's frame no vertex may be lower than the
/// lowest box collider (or `suspensions.ini [FRONT] BASEY`); `None` when the clamped mesh would be
/// less than 0.3 m high (CSP then keeps the mesh as it is).
// shortcut: `[_EXTENSION] MESH_OFFSET` of colliders.ini is added to the vertices (CSP: only while
// its car-config flag +0xe0 is set) and not moved into the matrix when unclamped; upgrade when a car
// with that key is proven
pub fn clamp_mesh(data_folder: &Path, vertices: &[[f32; 3]], a: &[f32; 16]) -> Result<Option<Vec<[f32; 3]>>, String> {
    if vertices.is_empty() {
        return Ok(None);
    }
    let read = |name: &str| -> Result<Option<IniReader>, String> {
        let path = append_path(data_folder, name);
        if crate::data::exists(&path) { IniReader::load(&path).map(Some) } else { Ok(None) }
    };
    let colliders = read("colliders.ini")?;
    let offset = match &colliders {
        Some(ini) if ini.has_key("_EXTENSION", "MESH_OFFSET") => ini.get_float3("_EXTENSION", "MESH_OFFSET")?,
        _ => [0.0; 3],
    };
    let mut lowest = match read("suspensions.ini")? {
        Some(ini) if ini.has_key("FRONT", "BASEY") => ini.get_float("FRONT", "BASEY")?,
        _ => 1.0,
    };
    if let Some(ini) = &colliders {
        for section in ini.sections().filter(|s| s.starts_with("COLLIDER")) {
            let get = |key: &str| if ini.has_key(section, key) { ini.get_float3(section, key) } else { Ok([0.0; 3]) };
            let (size, centre) = (get("SIZE")?, get("CENTRE")?);
            lowest = (centre[1] - size[1] * 0.5).min(lowest);
        }
    }
    let i = inverse(a);
    let mut top = -f32::MAX;
    let out: Vec<[f32; 3]> = vertices
        .iter()
        .map(|v| {
            let (x, y, z) = (offset[0] + v[0], offset[1] + v[1], offset[2] + v[2]);
            let bx = ((y * a[4] + x * a[0]) + z * a[8]) + a[12];
            let by = ((x * a[1] + y * a[5]) + z * a[9]) + a[13];
            let bz = ((y * a[6] + x * a[2]) + z * a[10]) + a[14];
            let by = by.max(lowest);
            top = by.max(top);
            [((i[4] * by + i[0] * bx) + i[8] * bz) + i[12], ((i[5] * by + i[1] * bx) + i[9] * bz) + i[13], ((by * i[6] + bx * i[2]) + bz * i[10]) + i[14]]
        })
        .collect();
    Ok((top - lowest >= 0.3).then_some(out))
}

/// CSP's 4x4 inverse `+0x304a40`: the first row's cofactors for the determinant, then the adjugate
/// from 3x3 minors, times `1 / det`.
fn inverse(a: &[f32; 16]) -> [f32; 16] {
    let sign = |k: usize| if k & 1 == 0 { 1.0f32 } else { -1.0 };
    let mut c = [0.0f32; 4];
    for (k, ck) in c.iter_mut().enumerate() {
        let pick = |lo: usize, at: usize| if k <= lo { at + 1 } else { at };
        let (v6, v8, v10, v14, v16, v20) = (a[pick(0, 4)], a[pick(1, 5)], a[pick(0, 8)], a[pick(1, 9)], a[pick(0, 12)], a[pick(1, 13)]);
        let (v13, v18, v19) = (v10 * v8, v16 * v14, v16 * v8);
        let v22 = v19 - v20 * v6;
        let v23 = v6 * v14 - v13;
        let v24 = (v10 * v20 - v18) * a[pick(2, 6)];
        *ck = ((v24 + v22 * a[pick(2, 10)]) + v23 * a[pick(2, 14)]) * sign(k);
    }
    let p: [f32; 4] = std::array::from_fn(|k| c[k] * a[k]);
    let inv_det = 1.0 / ((p[2] + p[0]) + (p[3] + p[1]));
    let m = |r: usize, col: usize| a[r * 4 + col];
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        let (c0, c1, c2) = (if row == 0 { 1 } else { 0 }, if row <= 1 { 2 } else { 1 }, if row <= 2 { 3 } else { 2 });
        for col in 0..4 {
            let (r0, r1, r2) = (if col == 0 { 1 } else { 0 }, if col <= 1 { 2 } else { 1 }, if col <= 2 { 3 } else { 2 });
            let t1 = (m(r1, c0) * m(r2, c1) - m(r2, c0) * m(r1, c1)) * m(r0, c2);
            let t2 = (m(r2, c0) * m(r0, c1) - m(r2, c1) * m(r0, c0)) * m(r1, c2);
            let t3 = (m(r0, c0) * m(r1, c1) - m(r1, c0) * m(r0, c1)) * m(r2, c2);
            out[row * 4 + col] = ((t1 + t2) + t3) * sign(row + col) * inv_det;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn inverse_of_a_body_matrix() {
        // pitch -0.5 degrees, moved: the inverse times the matrix is the identity to f32 rounding
        let (s, c) = (0.008726389f32, 0.9999619f32);
        let a = [1.0, 0.0, 0.0, 0.0, 0.0, c, -s, 0.0, 0.0, s, c, 0.0, 0.0, -0.23, 0.35, 1.0];
        let i = super::inverse(&a);
        for r in 0..4 {
            for k in 0..4 {
                let v: f32 = (0..4).map(|j| a[r * 4 + j] * i[j * 4 + k]).sum();
                assert!((v - if r == k { 1.0 } else { 0.0 }).abs() < 1e-6, "{r} {k} {v}");
            }
        }
    }
}
