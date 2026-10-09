// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Skid marks: `SkidMarkBuffer` (0x198 bytes; `SkidMarkBuffer::SkidMarkBuffer` 0x14018f2d0,
//! `addSegment` 0x1401900c0, `render` 0x1401905e0, `reset` 0x140190720, `split` 0x140190740),
//! `DynamicBuffer` (`DynamicBuffer::DynamicBuffer` 0x14021c360, `commit` 0x14021c4e0, `render`
//! 0x14021c540) and `CarAvatar::updateSkidMarks` 0x1400dd780.
//!
//! One buffer per wheel, a ring of vertices (two triangles per segment) under the scene's
//! `SKIDMARKS` node. A tyre that slides (slip above 1.5 of its peak) on a grippy surface with
//! load on it lays a segment every 20 cm across its width.

use std::cell::RefCell;
use std::rc::Rc;

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use crate::graphics::{Graphics, BLEND_ALPHA, CULL_BACK, DEPTH_NORMAL, DEPTH_NO_WRITE};
use crate::kgl::KglVertexBuffer;
use crate::material::Material;
use crate::scene::{NodeId, OnNodeRenderEvent, RenderableObject, Scene};
use crate::state::CarPhysicsState;

const STRIDE: usize = 44;

/// `DynamicBuffer` (0x28 bytes): `MeshVertex` records kept on the CPU and a dynamic vertex
/// buffer they are copied into.
pub struct DynamicBuffer {
    pub size: u32,
    pub vertices: Vec<u8>,
    kid: KglVertexBuffer,
    old_number_of_vertices_to_commit: u32,
}

impl DynamicBuffer {
    /// `DynamicBuffer::DynamicBuffer` 0x14021c360.
    pub fn new(graphics: &Graphics, size: u32) -> DynamicBuffer {
        let vertices = vec![0u8; size as usize * STRIDE];
        let kid = graphics.kgl.create_vertex_buffer(&vertices, vertices.len(), STRIDE as u32, true);
        graphics.kgl.vertex_buffer_map(&kid, &vertices);
        DynamicBuffer { size, vertices, kid, old_number_of_vertices_to_commit: 0 }
    }

    /// `DynamicBuffer::commit` 0x14021c4e0: only what is new while the ring has not gone round.
    pub fn commit(&mut self, count: u32, graphics: &Graphics) {
        if count < self.size {
            let offset = self.old_number_of_vertices_to_commit as usize * STRIDE;
            let length = (count.wrapping_sub(self.old_number_of_vertices_to_commit)) as usize * STRIDE;
            graphics.kgl.vertex_buffer_map_no_overwrite(&self.kid, &self.vertices, offset, length);
            self.old_number_of_vertices_to_commit = count;
        } else {
            graphics.kgl.vertex_buffer_map(&self.kid, &self.vertices);
        }
    }

    /// `DynamicBuffer::render` 0x14021c540.
    pub fn render(&self, graphics: &Graphics, start: u32, count: u32) {
        graphics.kgl.set_vertex_buffer(&self.kid);
        graphics.kgl.draw(count as i32, start as i32);
    }
}

pub struct SkidMarkBuffer {
    pub texture_step: f32,
    pub height_from_ground: f32,
    pub cursor: u32,
    pub valid_vertex_count: u32,
    last_v1: [f32; 3],
    last_v2: [f32; 3],
    last_normal: [f32; 3],
    last_alpha: f32,
    current_texture_v: f32,
    dynamic_buffer: DynamicBuffer,
    is_started: bool,
    material: Material,
    is_dirty: bool,
    /// the buffer's node, for its world matrix and its `Renderable` members
    node: NodeId,
}

impl SkidMarkBuffer {
    /// `SkidMarkBuffer::SkidMarkBuffer` 0x14018f2d0; the node goes under `skid_mark_node`.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, skid_mark_node: NodeId, size: u32) -> Result<Rc<RefCell<SkidMarkBuffer>>, String> {
        let dynamic_buffer = DynamicBuffer::new(graphics, size);
        let mut material = Material::new("SKID_MARK_MATERIAL");
        material.set_shader(graphics, "ksSkidMark")?;
        let game = crate::model::path_text(&graphics.game_folder);
        let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/skids.dds"));
        material.set_texture("txDiffuse", texture);
        let mut texture_step = f32::from_bits(0x3e4c_cccd); // 0.2
        let mut height_from_ground = f32::from_bits(0x3ca3_d70a); // 0.02
        material.set_float("ksSpecular", 1.0);
        material.set_float("ksSpecularEXP", 30.0);
        material.set_float("ksAmbient", 0.5);
        material.set_float("ksDiffuse", 1.0);
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/skidmarks.ini")) {
            let get = |key: &str| ini.get_float("GRAPHICS", key).unwrap_or(0.0);
            texture_step = get("TEXTURE_STEP");
            height_from_ground = get("HEIGHT_FROM_GROUND");
            material.set_float("ksSpecular", get("SPECULAR"));
            material.set_float("ksSpecularEXP", get("SPECULAR_EXP"));
            material.set_float("ksAmbient", get("AMBIENT"));
            material.set_float("ksDiffuse", get("DIFFUSE"));
        }
        if let Some(var) = material.get_var("ksEmissive") {
            material.update_var(var, |v| v.f_value3 = [0.0; 3]);
        }
        material.set_float("ksAlphaRef", 0.0);
        material.blend_mode = BLEND_ALPHA;
        material.cull_mode = CULL_BACK;
        let node = scene.renderable_object("SKID_BUFFER");
        if let Some(r) = &mut scene.nodes[node].renderable {
            r.cast_shadows = false;
            r.is_transparent = false;
            r.no_cull = true;
        }
        let buffer = Rc::new(RefCell::new(SkidMarkBuffer {
            texture_step,
            height_from_ground,
            cursor: 0,
            valid_vertex_count: 0,
            last_v1: [0.0; 3],
            last_v2: [0.0; 3],
            last_normal: [0.0; 3],
            last_alpha: 0.0,
            current_texture_v: 0.0,
            dynamic_buffer,
            is_started: false,
            material,
            is_dirty: false,
            node,
        }));
        scene.set_renderable_object(node, buffer.clone());
        scene.add_child(skid_mark_node, node);
        Ok(buffer)
    }

    fn put(&mut self, pos: &[f32; 3], normal: &[f32; 3], u: f32, v: f32, alpha: f32) {
        let at = self.cursor as usize * STRIDE;
        let record = &mut self.dynamic_buffer.vertices[at..at + STRIDE];
        for (k, value) in pos.iter().chain(normal).chain([&u, &v, &alpha]).enumerate() {
            record[k * 4..k * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        self.valid_vertex_count = self.valid_vertex_count.wrapping_add(1);
        self.cursor += 1;
        if self.cursor >= self.dynamic_buffer.size {
            self.cursor = 0;
        }
    }

    /// `SkidMarkBuffer::addSegment` 0x1401900c0: two triangles from the last pair of points to
    /// this one; the first call after a split only remembers its pair.
    pub fn add_segment(&mut self, v1: &[f32; 3], v2: &[f32; 3], normal: &[f32; 3], alpha: f32) {
        if self.is_started {
            let old_v = self.current_texture_v;
            let new_v = old_v + self.texture_step;
            self.current_texture_v = new_v;
            let (l1, l2, ln, la) = (self.last_v1, self.last_v2, self.last_normal, self.last_alpha);
            self.put(&l1, &ln, 0.0, old_v, la);
            self.put(&l2, &ln, 1.0, old_v, la);
            self.put(v2, normal, 1.0, new_v, alpha);
            self.put(&l1, &ln, 0.0, old_v, la);
            self.put(v2, normal, 1.0, new_v, alpha);
            self.put(v1, normal, 0.0, new_v, alpha);
            self.is_dirty = true;
        } else {
            self.is_started = true;
        }
        self.last_v1 = *v1;
        self.last_v2 = *v2;
        self.last_normal = *normal;
        self.last_alpha = alpha;
    }

    /// `SkidMarkBuffer::split` 0x140190740: the next segment starts a new mark.
    pub fn split(&mut self) {
        self.is_started = false;
    }

    /// `SkidMarkBuffer::reset` 0x140190720.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.valid_vertex_count = 0;
        self.is_started = false;
        self.dynamic_buffer.old_number_of_vertices_to_commit = 0;
    }
}

impl RenderableObject for SkidMarkBuffer {
    /// `SkidMarkBuffer::render` 0x1401905e0.
    fn render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) -> bool {
        if event.camera.is_cube_map_camera {
            return false;
        }
        if self.cursor > 3 {
            let node = &scene.nodes[self.node];
            let visible = node.renderable.as_ref().is_some_and(|r| crate::scene::is_visible_in(event, r, &node.matrix_ws));
            if visible {
                let count = self.dynamic_buffer.size.min(self.valid_vertex_count);
                if self.is_dirty {
                    self.dynamic_buffer.commit(count, graphics);
                    self.is_dirty = false;
                }
                crate::material::apply(&mut self.material, graphics, event.pass_id);
                graphics.set_world_matrix(&Mat44f::IDENTITY);
                graphics.commit_shader_changes();
                graphics.set_depth_mode(DEPTH_NO_WRITE);
                self.dynamic_buffer.render(graphics, 0, count);
                graphics.set_depth_mode(DEPTH_NORMAL);
            }
        }
        true
    }
}

/// The skid marks of one car: `CarAvatar::skidMarkBuffers` and `lastSkidPosition`.
#[derive(Default)]
pub struct CarSkidMarks {
    pub buffers: [Option<Rc<RefCell<SkidMarkBuffer>>>; 4],
    pub last_skid_position: [[f32; 3]; 4],
    /// `CarPhysicsInfo::tyreWidth`
    pub tyre_width: [f32; 4],
}

impl CarSkidMarks {
    /// The part of `CarAvatar::initCommonPostPhysics` 0x1400d6190 that makes wheel `i`'s
    /// buffer: none with `WORLD_DETAIL` 0 or `QUANTITY_MULT` 0; 6000 vertices times the
    /// multiplier, 12000 from world detail 3 on.
    pub fn make_buffer(&mut self, graphics: &mut Graphics, scene: &mut Scene, skid_mark_node: NodeId, i: usize) -> Result<(), String> {
        let mut mult = 1.0f32;
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/skidmarks.ini")) {
            if ini.has_key("GRAPHICS", "QUANTITY_MULT") {
                mult = ini.get_float("GRAPHICS", "QUANTITY_MULT").unwrap_or(0.0);
            }
        }
        let detail = graphics.video.world_detail;
        self.buffers[i] = None;
        if detail > 0 && mult != 0.0 && !mult.is_nan() {
            let size = if detail >= 3 { (mult * 12000.0) as i32 } else { (mult * 6000.0) as i32 };
            self.buffers[i] = Some(SkidMarkBuffer::new(graphics, scene, skid_mark_node, size as u32)?);
        }
        Ok(())
    }

    /// `CarAvatar::updateSkidMarks` 0x1400dd780. `wheel_world(i)` is the world matrix of the
    /// node `ISuspensionAvatar::getWheelTransform(i)` gives, from the nodes' matrices as they
    /// are when the car's own update runs (the wheels' are last frame's).
    pub fn update(&mut self, state: &CarPhysicsState, car_node_active: bool, wheel_world: &dyn Fn(usize) -> Mat44f) {
        if !car_node_active {
            return;
        }
        let threshold = (((state.load[1] + state.load[0]) + state.load[2]) + state.load[3]) * 0.125;
        for i in 0..4 {
            let Some(buffer) = &self.buffers[i] else {
                continue;
            };
            let mut buffer = buffer.borrow_mut();
            let nd = state.nd_slip[i];
            if 1.5 > nd || 1.0 >= state.speed || f32::from_bits(0x3f66_6666) >= state.tyre_surface_def[i].grip_mod || threshold >= state.load[i] {
                buffer.split();
                continue;
            }
            let w = wheel_world(i);
            let (x, z) = (w.m[3][0], w.m[3][2]);
            let last = self.last_skid_position[i];
            let fy = -last[1];
            let dx = x - last[0];
            let dz = z - last[2];
            if !((fy * fy + dx * dx) + dz * dz > f32::from_bits(0x3d23_d70b)) {
                continue;
            }
            // vec3f::normalize 0x140024000
            let (mut vx, mut vy, mut vz) = (dx, fy, dz);
            let len = sqrtf((vx * vx + vy * vy) + vz * vz);
            if len != 0.0 {
                vx /= len;
                vy /= len;
                vz /= len;
            }
            let [nx, ny, nz] = state.tyre_contact_normal[i];
            let [px, py, pz] = state.tyre_contact_point[i];
            let height = buffer.height_from_ground;
            let width = self.tyre_width[i];
            let cx = ((nz * vy - ny * vz) * width) * 0.5;
            let cy = ((nx * vz - nz * vx) * width) * 0.5;
            let cz = ((ny * vx - nx * vy) * width) * 0.5;
            let v1 = [cx + px, (cy + py) + height, cz + pz];
            let v2 = [px - cx, (py - cy) + height, pz - cz];
            self.last_skid_position[i] = [x, 0.0, z];
            let a = (nd - 1.5) * f32::from_bits(0x3e99_999a);
            let a = if a > 1.0 {
                1.0
            } else if a >= 0.0 {
                a
            } else {
                0.0
            };
            buffer.add_segment(&v1, &v2, &state.tyre_contact_normal[i], a * f32::from_bits(0x3f19_999a));
        }
    }
}
