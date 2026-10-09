// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The sky dome: `SkyBox::SkyBox` 0x14021c790, `SkyBox::render` 0x14021d0d0,
//! `ShapeBuilder::buildHemiSphere` 0x1402267d0, `ShapeBuilder::getSpherePos` 0x140226e90.
//!
//! The dome is not a file: 91 quads of a hemisphere of 10 m, generated at start-up. It is drawn
//! first in the main pass with the shader `ksSky`, whose vertex shader moves it to the camera.
//! Clouds (the billboards of the other weathers) are Task 21; the clear weather has none.

use rustyac_math::{asinf, atan2f, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::camera::DEG_TO_RAD;
use crate::graphics::{Graphics, DEPTH_OFF};
use crate::kgl::{KglIndexBuffer, KglVertexBuffer};
use crate::material::{Material, MaterialId};
use crate::scene::RenderContext;
use crate::shader::ShaderId;

/// The sky dome's material is not one of a model: it gets a name of its own for the filter.
const SKY_MATERIAL: MaterialId = MaterialId(u32::MAX);

/// The game's inline `floorf`: toward minus infinity, a whole number is kept as it is.
fn floor_inline(t: f32) -> f32 {
    let i = t as i32; // cvttss2si (the cases that overflow keep `t`)
    if !(-2147483648.0..2147483648.0).contains(&t) || t.is_nan() {
        return t;
    }
    if i as f32 != t {
        let i = i - if t.is_sign_negative() { 1 } else { 0 };
        return i as f32;
    }
    t
}

/// `ShapeBuilder::getSpherePos` 0x140226e90: a point of the unit sphere from two angles in
/// degrees, rounded down to thousandths.
fn get_sphere_pos(a_deg: f32, b_deg: f32) -> Vec3f {
    let m = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), a_deg * DEG_TO_RAD);
    let mut v = [0.0f32; 3];
    for (c, slot) in v.iter_mut().enumerate() {
        *slot = (((m.m[1][c] * 0.0) + (m.m[0][c] * 0.0)) + (m.m[2][c] * -1.0)) + m.m[3][c];
    }
    let axis = Vec3f::new((v[1] * 0.0) - (v[2] * -1.0), (v[2] * 0.0) - (v[0] * 0.0), (v[0] * -1.0) - (v[1] * 0.0));
    let n = Mat44f::create_from_axis_angle(&axis, -(b_deg * DEG_TO_RAD));
    let mut w = [0.0f32; 3];
    for (c, slot) in w.iter_mut().enumerate() {
        *slot = (((v[0] * n.m[0][c]) + (v[1] * n.m[1][c])) + (v[2] * n.m[2][c])) + n.m[3][c];
    }
    let len = sqrtf(((w[1] * w[1]) + (w[0] * w[0])) + (w[2] * w[2]));
    if len != 0.0 && !len.is_nan() {
        let inv = 1.0 / len;
        w[1] *= inv;
        w[0] = inv * w[0];
        w[2] *= inv;
    }
    for c in &mut w {
        let t = floor_inline(*c * 1000.0);
        *c = t * f32::from_bits(0x3a83_126f);
    }
    Vec3f::new(w[0], w[1], w[2])
}

/// `ShapeBuilder::buildHemiSphere` 0x1402267d0: the vertex records (44 bytes each) and the
/// indices.
pub fn build_hemi_sphere(radius: f32, n: i32) -> (Vec<u8>, Vec<u16>) {
    const INV_PI: f32 = f32::from_bits(0x3ea2_f982);
    // position, normal, texture coordinate; the tangent stays zero
    let mut vertices: Vec<([f32; 3], [f32; 3], [f32; 2])> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let inv = 1.0 / n as f32;
    let step_b = inv * 180.0;
    let step_a = inv * 360.0;
    let mut quad = 0u32;
    for i in -1..n {
        let a1 = (i + 1) as f32 * step_a;
        let a0 = i as f32 * step_a;
        for j in -1..n / 2 {
            let b0 = j as f32 * step_b;
            let b1 = (j + 1) as f32 * step_b;
            let points = [get_sphere_pos(a0, b0), get_sphere_pos(a1, b0), get_sphere_pos(a1, b1), get_sphere_pos(a0, b1)];
            for p in points {
                vertices.push(([p.x * radius, p.y * radius, p.z * radius], [p.x, p.y, p.z], [0.0, 0.0]));
            }
            let count = vertices.len();
            let (v0, v1, v2, v3) = (count - 4, count - 3, count - 2, count - 1);
            for v in [v3, v2, v1, v0] {
                let normal = vertices[v].1;
                vertices[v].2[0] = (asinf(normal[1]) * INV_PI) + 0.5;
                vertices[v].2[1] = ((atan2f(normal[2], normal[0]) * INV_PI) + 1.0) * 0.5;
            }
            // the seam
            if (vertices[v1].2[1] - vertices[v0].2[1]).abs() > 0.1 || (vertices[v3].2[1] - vertices[v2].2[1]).abs() > 0.1 {
                vertices[v2].2[1] -= 1.0;
                vertices[v1].2[1] -= 1.0;
            }
            // the pole (`comiss; jae` skips: a NaN passes)
            if !((vertices[v3].2[0] - 1.0).abs() >= f32::from_bits(0x3a83_126f)) && !((vertices[v2].2[0] - 1.0).abs() >= f32::from_bits(0x3a83_126f)) {
                vertices[v3].2[1] = (vertices[v0].2[1] + vertices[v1].2[1]) * 0.5;
                vertices[v2].2[1] = (vertices[v0].2[1] + vertices[v1].2[1]) * 0.5;
            }
            // ShapeBuilder::addQuadIndicesToMesh 0x1402264d0
            let base = (4 * quad) as u16;
            indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            quad += 1;
        }
    }
    let mut bytes = Vec::with_capacity(vertices.len() * 44);
    for (position, normal, uv) in &vertices {
        for v in position.iter().chain(normal).chain(uv).chain(&[0.0f32; 3]) {
            bytes.extend(v.to_le_bytes());
        }
    }
    (bytes, indices)
}

/// `SkyBox` (0x80 bytes), without its clouds.
pub struct SkyBox {
    vb: KglVertexBuffer,
    ib: KglIndexBuffer,
    index_count: i32,
    pub material: Material,
    pub clouds_shader: ShaderId,
    pub cubemap_sky_shader: ShaderId,
    pub skybox_gain: f32,
}

impl SkyBox {
    /// `SkyBox::SkyBox` 0x14021c790.
    pub fn new(graphics: &mut Graphics) -> Result<SkyBox, String> {
        let (vertices, indices) = build_hemi_sphere(10.0, 12);
        let mut material = Material::new("SKYBOX MATERIAL");
        material.set_shader(graphics, "ksSky")?;
        material.depth_mode = DEPTH_OFF;
        material.blend_mode = 0;
        // graphics->compile(mesh)
        let vb = graphics.kgl.create_vertex_buffer(&vertices, vertices.len(), 44, false);
        let ib = graphics.kgl.create_index_buffer(&indices);
        let clouds_shader = graphics.shaders.get_shader(&graphics.kgl, "ksClouds")?;
        let cubemap_sky_shader = graphics.shaders.get_shader(&graphics.kgl, "ksSkyCubemap")?;
        let mut skybox_gain = 1.0;
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/graphics.ini")) {
            skybox_gain = ini.get_float("DX11", "SKYBOX_REFLECTION_GAIN").unwrap_or(0.0);
        }
        Ok(SkyBox { vb, ib, index_count: indices.len() as i32, material, clouds_shader, cubemap_sky_shader, skybox_gain })
    }

    /// `SkyBox::render` 0x14021d0d0 for the plain camera: the dome (`Mesh::render` of a mesh
    /// without a parent: no visibility test), then the clouds, of which there are none.
    pub fn render(&mut self, graphics: &mut Graphics, rc: &mut RenderContext, cube_map_camera: bool) {
        if cube_map_camera {
            crate::cubemap::render_sky(self, graphics, rc);
            return;
        }
        self.render_mesh(graphics, rc);
    }

    /// `Mesh::render` 0x1402261a0 of the dome.
    pub fn render_mesh(&mut self, graphics: &mut Graphics, rc: &mut RenderContext) {
        rc.material_filter.apply(SKY_MATERIAL, &mut self.material, graphics, rc.pass_id);
        graphics.set_vb(&self.vb);
        graphics.set_ib(&self.ib);
        graphics.commit_shader_changes();
        graphics.draw_primitive(self.index_count, 0, 0);
    }
}
