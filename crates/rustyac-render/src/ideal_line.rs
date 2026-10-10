// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The ideal line: `IdealLine::IdealLine` 0x140109d10 (a `Node` named `IDEAL_LINE` with one
//! mesh), built with `SplineStripBuilder` (`addPoint` 0x14021b6b0, `addVertex` 0x14021b9f0,
//! `buildMesh` 0x14021bb20) from the track's `data/ideal_line.ai` (or its AI line).
//!
//! A ribbon 2 m wide, 9 cm above the road, one sample every 5 m of the line. The colour of a
//! piece is stored in the vertex normal: red where the line brakes, green where it is flat out,
//! yellow between; the shader `ksIdealLine` multiplies the arrow texture by it and fades the
//! ribbon out over the 10 m in front of the camera.
//!
//! Whether it shows is the node's `is_active`, written every frame by
//! `DrivingAssistManager::update` 0x1400fcb20 from `assists.ini [ASSISTS] IDEAL_LINE` (and the
//! Ctrl+I key of `Sim::onKeyDown`).

use rustyac_math::sqrtf;
use rustyac_physics::track::spline::AiSpline;

use crate::graphics::{Graphics, BLEND_ALPHA};
use crate::material::{Material, MaterialId};
use crate::scene::{MeshData, NodeId, Renderable, Scene};

/// `SplineStripBuilder` (0x80 bytes).
struct SplineStripBuilder {
    width: f32,
    texture_step: f32,
    last_point: [f32; 3],
    vertices: Vec<u8>,
    indices: Vec<u16>,
    first_point: bool,
    has_first_vertex: bool,
    /// prev+, prev-, cur+, cur-
    points: [[f32; 3]; 4],
    current_tex_u: f32,
}

impl SplineStripBuilder {
    /// `SplineStripBuilder::addVertex` 0x14021b9f0: a `MeshVertex` of 44 bytes whose normal is
    /// the colour, and its own index.
    fn add_vertex(&mut self, pos: &[f32; 3], normal: &[f32; 3], uv: [f32; 2], tangent_x: f32) {
        for v in pos.iter().chain(normal).chain(&uv).chain(&[tangent_x, 0.0, 0.0]) {
            self.vertices.extend_from_slice(&v.to_le_bytes());
        }
        let count = self.vertices.len() / 44;
        self.indices.push((count - 1) as u16);
    }

    /// `SplineStripBuilder::addPoint` 0x14021b6b0.
    fn add_point(&mut self, p: &[f32; 3], up: &[f32; 3], colour: &[f32; 3], tangent_x: f32) {
        if !self.has_first_vertex {
            self.first_point = true;
            self.has_first_vertex = true;
            self.last_point = *p;
            return;
        }
        let (mut dx, mut dy, mut dz) = (p[0] - self.last_point[0], p[1] - self.last_point[1], p[2] - self.last_point[2]);
        let sum = (dx * dx + dy * dy) + dz * dz;
        let len = if sum == 0.0 { 0.0 } else { sqrtf(sum) };
        let tex_u = len / self.texture_step + self.current_tex_u;
        let len2 = sqrtf(sum);
        if len2 != 0.0 {
            let inv = 1.0 / len2;
            dx *= inv;
            dy *= inv;
            dz *= inv;
        }
        let side = [up[2] * dy - up[1] * dz, up[0] * dz - up[2] * dx, up[1] * dx - up[0] * dy];
        self.points[0] = self.points[2];
        self.points[1] = self.points[3];
        let w = self.width;
        self.points[2] = [(side[0] * w) * 0.5 + p[0], (side[1] * w) * 0.5 + p[1], (side[2] * w) * 0.5 + p[2]];
        self.points[3] = [p[0] - (side[0] * w) * 0.5, p[1] - (side[1] * w) * 0.5, p[2] - (side[2] * w) * 0.5];
        if self.first_point {
            self.points[0] = self.points[2];
            self.points[1] = self.points[3];
            self.first_point = false;
        }
        let last_tex_u = self.current_tex_u;
        let [prev_plus, prev_minus, cur_plus, cur_minus] = self.points;
        self.add_vertex(&prev_plus, colour, [0.0, last_tex_u], tangent_x);
        self.add_vertex(&cur_plus, colour, [0.0, tex_u], tangent_x);
        self.add_vertex(&prev_minus, colour, [1.0, last_tex_u], tangent_x);
        self.add_vertex(&prev_minus, colour, [1.0, last_tex_u], tangent_x);
        self.add_vertex(&cur_plus, colour, [0.0, tex_u], tangent_x);
        self.add_vertex(&cur_minus, colour, [1.0, tex_u], tangent_x);
        self.current_tex_u = tex_u;
        self.last_point = *p;
    }
}

/// `IdealLine::IdealLine` 0x140109d10. `ray_down(from)`: `Track::rayCast` 0x140278bb0 from a
/// point straight down, 100 m: where it hits, or `None`. The game never tests the ray's
/// answer: a miss leaves the point at the world's origin.
///
/// The node is neither compiled nor added to the scene here (`TrackAvatar::TrackAvatar` does
/// `GraphicsManager::compile` and `trackNode->addChild` next).
pub fn build(graphics: &mut Graphics, scene: &mut Scene, spline: &AiSpline, ray_down: &dyn Fn(&[f32; 3]) -> Option<[f32; 3]>) -> Result<NodeId, String> {
    let node = scene.node("IDEAL_LINE");
    let mut builder = SplineStripBuilder { width: 2.0, texture_step: 4.0, last_point: [0.0; 3], vertices: Vec::new(), indices: Vec::new(), first_point: false, has_first_vertex: false, points: [[0.0; 3]; 4], current_tex_u: 0.0 };
    let sample = |builder: &mut SplineStripBuilder, t: f32| {
        let payload = spline.payload_at_position(t);
        let mut c = 0.5f32;
        if payload.brake > f32::from_bits(0x3dcc_cccd) {
            c = 0.0;
        } else if payload.gas > f32::from_bits(0x3f66_6666) {
            c = 1.0;
        }
        let from = spline.spline.spline_to_world(t);
        let hit = ray_down(&from).unwrap_or([0.0; 3]);
        let colour = [(1.0 - c) * 3.0, c * 3.0, 0.0];
        let position = [hit[0], hit[1] + f32::from_bits(0x3db8_51ec), hit[2]];
        builder.add_point(&position, &[0.0, 1.0, 0.0], &colour, 1.0);
    };
    let mut t = 0.0f32;
    loop {
        sample(&mut builder, t);
        t += 5.0 / spline.spline.length();
        if !(t < 1.0) {
            break;
        }
    }
    // the closing point: the second sample of the lap
    sample(&mut builder, 5.0 / spline.spline.length());

    // SplineStripBuilder::buildMesh 0x14021bb20, then the material
    let mut material = Material::new("IDEAL_LINE");
    material.set_shader(graphics, "ksIdealLine")?;
    material.blend_mode = BLEND_ALPHA;
    let game = crate::model::path_text(&graphics.game_folder);
    let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/ideal_line.png"));
    material.set_texture("txDiffuse", texture);
    let id = MaterialId(scene.materials.len() as u32);
    scene.materials.push(material);
    let mut renderable = Renderable::new();
    renderable.is_transparent = true;
    renderable.cast_shadows = false;
    let mesh = scene.mesh("NONAME", renderable, MeshData { vertices: builder.vertices, indices: builder.indices, material: Some(id), is_renderable: true, vb: None, ib: None, compiled_indices_count: 0, compiled_vertices_count: 0 });
    scene.add_child(node, mesh);
    Ok(node)
}
