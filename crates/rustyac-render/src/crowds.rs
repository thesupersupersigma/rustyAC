// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The crowds: `CameraFacing::CameraFacing` 0x14005e640 scatters the people of the track's
//! `data/camera_facing.ini` over the triangles of named, undrawn meshes, into one
//! `StaticParticleSystem` (0x1b8 bytes; constructor 0x14025e8b0, `finalize` 0x14025efb0,
//! `render` 0x14025f550) per section. `Triangle::computeArea` 0x14020bcb0.
//!
//! Also here: the grooves (`DynamicTrackManager`) and the track's drifting objects
//! (`TrackAvatar::initDynamicObjects` 0x1401c9840, `updateDynamicObjects` 0x1401ccb30).
//!
//! The places are drawn from `rand()` after `srand(0)`: the same crowd every time. The
//! billboards stand upright (their up is the world's) and turn to the camera about that axis.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::MsvcRand;
use rustyac_physics::vecmath::Mat44f;

use crate::graphics::{Graphics, BLEND_ALPHA_TO_COVERAGE, DEPTH_NORMAL};
use crate::kgl::{KglIndexBuffer, KglVertexBuffer};
use crate::material::PASS_OPAQUE;
use crate::scene::{NodeId, NodeKind, OnNodeRenderEvent, RenderableObject, Scene};
use crate::shader::ShaderId;
use crate::texture::Texture;

const VERTEX: usize = 36;

/// `StaticParticle` (36 bytes).
#[derive(Clone, Copy, Debug)]
pub struct StaticParticle {
    pub position: [f32; 3],
    pub u_range: [f32; 2],
    pub v_range: [f32; 2],
    pub half_size: [f32; 2],
}

pub struct StaticParticleSystem {
    pub emissive_blend: f32,
    pub min_distance: f32,
    pub blend_mode: i32,
    pub depth_mode: i32,
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub layer: i32,
    pub particles: Vec<StaticParticle>,
    pub max_particles_count: i32,
    pub node: NodeId,
    buffer: KglVertexBuffer,
    ib: KglIndexBuffer,
    staging: Vec<u8>,
    texture: Texture,
    shader: Option<ShaderId>,
    var_emissive_blend: Option<usize>,
    var_min_distance: Option<usize>,
    var_shaded: Option<usize>,
    var_diffuse: Option<usize>,
    var_ambient: Option<usize>,
    is_finalized: bool,
    drew: bool,
}

impl StaticParticleSystem {
    /// `StaticParticleSystem::StaticParticleSystem` 0x14025e8b0; the node is nobody's child yet.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, name: &str, max_particles: i32, texture: &Texture) -> Rc<RefCell<StaticParticleSystem>> {
        let max = max_particles.max(0) as usize;
        // generateIndexBuffer 0x14025f140
        let mut indices = Vec::with_capacity(max * 6);
        for q in 0..max {
            let b = (q * 4) as u16;
            indices.extend_from_slice(&[b, b.wrapping_add(1), b.wrapping_add(2), b, b.wrapping_add(2), b.wrapping_add(3)]);
        }
        let ib = graphics.kgl.create_index_buffer(&indices);
        let staging = vec![0u8; max * 4 * VERTEX];
        let buffer = graphics.kgl.create_vertex_buffer(&staging, staging.len(), VERTEX as u32, true);
        let shader = graphics.shaders.get_shader(&graphics.kgl, "ksParticle").ok();
        let mut vars = [None; 5];
        if let Some(id) = shader {
            let s = graphics.shaders.get_mut(id);
            for (slot, name) in vars.iter_mut().zip(["emissiveBlend", "minDistance", "shaded", "ksDiffuse", "ksAmbient"]) {
                *slot = s.get_var(name);
            }
        }
        let node = scene.object_node(name);
        let system = Rc::new(RefCell::new(StaticParticleSystem {
            emissive_blend: 0.5,
            min_distance: 0.0,
            blend_mode: BLEND_ALPHA_TO_COVERAGE,
            depth_mode: 1,
            diffuse: [f32::from_bits(0x3e4c_cccd); 3],
            ambient: [f32::from_bits(0x3e4c_cccd); 3],
            layer: 0,
            particles: Vec::new(),
            max_particles_count: max_particles,
            node,
            buffer,
            ib,
            staging,
            texture: texture.clone(),
            shader,
            var_emissive_blend: vars[0],
            var_min_distance: vars[1],
            var_shaded: vars[2],
            var_diffuse: vars[3],
            var_ambient: vars[4],
            is_finalized: false,
            drew: false,
        }));
        scene.set_renderable_object(node, system.clone());
        system
    }

    /// `StaticParticleSystem::addParticle` 0x14025ef60.
    pub fn add_particle(&mut self, particle: StaticParticle) {
        if (self.particles.len() as i32) < self.max_particles_count {
            self.particles.push(particle);
        }
    }

    /// `StaticParticleSystem::finalize` 0x14025efb0: the corners' colour and texture cell.
    fn finalize(&mut self) {
        println!("FINALIZING STATIC PARTICLE SYSTEM, particles.size()={} stagingBufferSize={}", self.particles.len(), self.max_particles_count);
        for (j, p) in self.particles.iter().enumerate() {
            let corners = [[p.u_range[0], p.v_range[1]], [p.u_range[1], p.v_range[1]], [p.u_range[1], p.v_range[0]], [p.u_range[0], p.v_range[0]]];
            for (k, uv) in corners.iter().enumerate() {
                let at = (j * 4 + k) * VERTEX;
                for (i, v) in [1.0f32, 1.0, 1.0, 1.0, uv[0], uv[1]].into_iter().enumerate() {
                    self.staging[at + 0xc + i * 4..at + 0x10 + i * 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

impl RenderableObject for StaticParticleSystem {
    /// `StaticParticleSystem::render` 0x14025f550: only in the opaque pass, not above the
    /// camera's layer (`WORLD_DETAIL`), not while a cube map is drawn.
    fn render(&mut self, _scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) -> bool {
        self.drew = false;
        if self.layer > event.max_layer || event.pass_id != PASS_OPAQUE || self.particles.is_empty() || !graphics.current_cube_map {
            return false;
        }
        if !self.is_finalized {
            self.finalize();
            self.is_finalized = true;
        }
        let view = graphics.state.view_matrix;
        let r = [view.m[0][0], view.m[1][0], view.m[2][0]];
        let z = [view.m[0][2], view.m[1][2], view.m[2][2]];
        for (j, p) in self.particles.iter().enumerate() {
            let (hx, hy) = (p.half_size[0], p.half_size[1]);
            for (k, (sx, sy)) in [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)].into_iter().enumerate() {
                let x = (((sx * r[0]) + (sy * 0.0)) + (z[0] * 0.0)) + p.position[0];
                let y = (((sx * r[1]) + sy) + (z[1] * 0.0)) + p.position[1];
                let zz = (((sx * r[2]) + (sy * 0.0)) + (z[2] * 0.0)) + p.position[2];
                let at = (j * 4 + k) * VERTEX;
                for (i, v) in [x, y, zz].into_iter().enumerate() {
                    self.staging[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        let n = self.particles.len();
        graphics.kgl.vertex_buffer_map(&self.buffer, &self.staging[..n * 4 * VERTEX]);
        if let Some(id) = self.shader {
            let s = graphics.shaders.get_mut(id);
            if let Some(v) = self.var_diffuse {
                s.set_var(v, &self.diffuse[0].to_le_bytes());
            }
            if let Some(v) = self.var_ambient {
                s.set_var(v, &self.ambient[0].to_le_bytes());
            }
            if let Some(b) = self.var_diffuse.and_then(|v| s.vars[v].buffer) {
                s.cbuffers[b].commit(&graphics.kgl);
            }
        }
        graphics.set_texture(0, &self.texture);
        graphics.set_blend_mode(self.blend_mode);
        graphics.kgl.set_vertex_buffer(&self.buffer);
        graphics.kgl.set_index_buffer(&self.ib);
        if let Some(id) = self.shader {
            self.min_distance = 1.5;
            let s = graphics.shaders.get_mut(id);
            for (var, value) in [(self.var_shaded, 1.0f32), (self.var_min_distance, self.min_distance), (self.var_emissive_blend, self.emissive_blend)] {
                if let Some(v) = var {
                    s.set_var(v, &value.to_le_bytes());
                }
            }
            if let Some(b) = self.var_emissive_blend.and_then(|v| s.vars[v].buffer) {
                s.cbuffers[b].commit(&graphics.kgl);
            }
            graphics.set_shader(id);
        }
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_cull_mode(0);
        graphics.commit_shader_changes();
        graphics.set_depth_mode(self.depth_mode);
        graphics.kgl.draw_indexed((6 * n) as i32, 0, 0);
        self.drew = true;
        false
    }

    fn resets_material_cache(&self) -> bool {
        self.drew
    }
}

/// `Triangle::computeArea` 0x14020bcb0.
fn compute_area(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let mut t1 = (c[1] - a[1]) * (c[2] - b[2]) - (c[2] - a[2]) * (c[1] - b[1]);
    t1 *= t1;
    let mut t2 = (c[2] - a[2]) * (c[0] - b[0]) - (c[0] - a[0]) * (c[2] - b[2]);
    t2 *= t2;
    let mut t3 = (c[0] - a[0]) * (c[1] - b[1]) - (c[1] - a[1]) * (c[0] - b[0]);
    t3 *= t3;
    let s = (t2 + t1) + t3;
    (if s == 0.0 { 0.0 } else { sqrtf(s) }) * 0.5
}

/// `CameraFacing::CameraFacing` 0x14005e640. `data_folder` is the track's (or its layout's)
/// folder; the systems become children of `blurred`. The main thread's `rand()` is seeded with
/// 0 first and, when the file exists, with `end_seed` at the end (the game: the tick count).
pub fn camera_facing(graphics: &mut Graphics, scene: &mut Scene, data_folder: &Path, track_node: NodeId, blurred: NodeId, end_seed: u32) -> Result<Vec<Rc<RefCell<StaticParticleSystem>>>, String> {
    let mut systems = Vec::new();
    graphics.crt_rand = MsvcRand(0);
    let Ok(ini) = IniReader::load(&data_folder.join("data/camera_facing.ini")) else {
        return Ok(systems);
    };
    let c = f32::from_bits(0x3800_0100);
    let mut i = 0;
    loop {
        let section = format!("CAMERA_FACING_{i}");
        if !ini.has_section(&section) {
            break;
        }
        i += 1;
        let surface = ini.get_string(&section, "SURFACE");
        let n = ini.get_int(&section, "ELEMENTS").unwrap_or(0);
        let size: Vec<f32> = ini.get_string(&section, "SIZE").split(',').map(|p| p.trim().parse::<f64>().unwrap_or(0.0) as f32).collect();
        let size = [size.first().copied().unwrap_or(0.0), size.get(1).copied().unwrap_or(0.0)];
        let texture_name = ini.get_string(&section, "TEXTURE");
        let rows = ini.get_int(&section, "TEXTURE_ROWS").unwrap_or(0);
        let cols = ini.get_int(&section, "TEXTURE_COLUMNS").unwrap_or(0);
        let diffuse = ini.get_float3(&section, "DIFFUSE").unwrap_or([0.0; 3]);
        let ambient = ini.get_float3(&section, "AMBIENT").unwrap_or([0.0; 3]);
        // (the game goes on without a test and stops with a fault)
        let mesh_node = scene.find_child_by_name(track_node, &surface, true).ok_or_else(|| format!("camera_facing.ini [{section}]: the track has no mesh {surface}"))?;
        let (points, layer) = {
            let NodeKind::Mesh(mesh) = &scene.nodes[mesh_node].kind else {
                return Err(format!("camera_facing.ini [{section}]: {surface} is not a mesh"));
            };
            let position = |index: u16| -> [f32; 3] {
                let at = index as usize * 44;
                std::array::from_fn(|k| f32::from_le_bytes(mesh.vertices[at + k * 4..at + k * 4 + 4].try_into().unwrap()))
            };
            let triangles: Vec<[[f32; 3]; 3]> = mesh.indices.chunks_exact(3).map(|t| [position(t[0]), position(t[1]), position(t[2])]).collect();
            (triangles, scene.nodes[mesh_node].renderable.as_ref().map(|r| r.layer).unwrap_or(0))
        };
        if points.is_empty() || rows <= 0 || cols <= 0 {
            return Err(format!("camera_facing.ini [{section}]: nothing to scatter people on"));
        }
        let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{}/{texture_name}", crate::model::path_text(&graphics.game_folder)));
        let system = StaticParticleSystem::new(graphics, scene, &format!("CF_PARTICLES_{section}"), n, &texture);
        scene.add_child(blurred, system.borrow().node);
        {
            let mut s = system.borrow_mut();
            s.blend_mode = BLEND_ALPHA_TO_COVERAGE;
            s.depth_mode = DEPTH_NORMAL;
            s.diffuse = diffuse;
            s.ambient = ambient;
            s.layer = layer;
            let mut cum = Vec::with_capacity(points.len());
            for (k, t) in points.iter().enumerate() {
                let area = compute_area(t[0], t[1], t[2]);
                cum.push(if k == 0 { area } else { area + cum[k - 1] });
            }
            let total = cum[cum.len() - 1];
            let mut rnd: Vec<f32> = (0..n.max(0)).map(|_| graphics.crt_rand.next() as f32 * c).collect();
            rnd.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let inv = 1.0 / total;
            let (col_inv, row_inv) = (1.0 / cols as f32, 1.0 / rows as f32);
            let mut tri = 0usize;
            for r in rnd {
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                if !(inv * cum[tri] > r) {
                    while tri < points.len() - 1 {
                        tri += 1;
                        if inv * cum[tri] > r {
                            break;
                        }
                    }
                }
                let a = graphics.crt_rand.next() as f32 * c;
                let b = graphics.crt_rand.next() as f32 * c;
                let t = &points[tri];
                let mut p = [0.0f32; 3];
                for k in 0..3 {
                    let p1 = t[0][k] + (t[1][k] - t[0][k]) * b;
                    let p2 = t[0][k] + (t[2][k] - t[0][k]) * b;
                    p[k] = (p2 - p1) * a + p1;
                }
                let row = graphics.crt_rand.next() % rows;
                let col = graphics.crt_rand.next() % cols;
                p[1] += size[1] * 0.5;
                s.add_particle(StaticParticle { position: p, u_range: [col as f32 * col_inv, (col + 1) as f32 * col_inv], v_range: [row as f32 * row_inv, (row + 1) as f32 * row_inv], half_size: [size[0] * 0.5, size[1] * 0.5] });
            }
        }
        systems.push(system);
    }
    graphics.crt_rand = MsvcRand(end_seed);
    Ok(systems)
}

/// `GrooveMesh` (0x28 bytes).
struct GrooveMesh {
    min_alpha: f32,
    max_alpha: f32,
    current_alpha: f32,
    target_alpha: f32,
    mult: f32,
    var: Option<(crate::material::MaterialId, usize)>,
    mesh: Option<NodeId>,
}

/// `DynamicTrackManager` (0x90 bytes; constructor 0x1401ccc20, `update` 0x1401cdba0,
/// `setGrooveMeshVisibility` 0x1401cdb60): the rubber line's meshes get more opaque with the
/// laps driven, or with the grip of a dynamic track.
pub struct DynamicTrackManager {
    /// no `groove.ini`: the object never updates
    pub is_active: bool,
    pub reset_to_target: bool,
    grooves: Vec<GrooveMesh>,
    pub max_number_of_laps: f32,
    pub starting_laps_count: f32,
}

impl DynamicTrackManager {
    /// `groove`: race.ini `[GROOVE] MAX_LAPS` and `STARTING_LAPS` when the file has the section.
    pub fn new(scene: &mut Scene, data_folder: &Path, track_node: NodeId, groove: Option<(f32, f32)>) -> DynamicTrackManager {
        let mut d = DynamicTrackManager { is_active: true, reset_to_target: false, grooves: Vec::new(), max_number_of_laps: 0.0, starting_laps_count: 0.0 };
        match IniReader::load(&data_folder.join("data/groove.ini")) {
            Err(_) => d.is_active = false,
            Ok(ini) => {
                let mut i = 0;
                while i < ini.get_int("HEADER", "GROOVES_NUMBER").unwrap_or(0) {
                    let section = format!("GROOVE_{i}");
                    i += 1;
                    let get = |key: &str| ini.get_float(&section, key).unwrap_or(0.0);
                    let (min, max, mult) = (get("MIN"), get("MAX"), get("MULT"));
                    let name = ini.get_string(&section, "NAME");
                    let mut g = GrooveMesh { min_alpha: min, max_alpha: max, current_alpha: min, target_alpha: min, mult, var: None, mesh: None };
                    let mesh = scene.find_child_by_name(track_node, &name, true).filter(|n| matches!(scene.nodes[*n].kind, NodeKind::Mesh(_)));
                    if let Some(n) = mesh {
                        g.mesh = Some(n);
                        if let NodeKind::Mesh(m) = &scene.nodes[n].kind {
                            if let Some(id) = m.material {
                                let material = &mut scene.materials[id.0 as usize];
                                if let Some(var) = material.get_var_reporting("alpha") {
                                    material.update_var(var, |v| v.f_value = min);
                                    g.var = Some((id, var));
                                }
                            }
                        }
                    }
                    d.grooves.push(g);
                }
            }
        }
        if let Some((max_laps, starting)) = groove {
            d.max_number_of_laps = max_laps;
            d.starting_laps_count = starting;
        }
        if d.max_number_of_laps == 0.0 {
            d.max_number_of_laps = 100.0;
        }
        d
    }

    /// `DynamicTrackManager::update` 0x1401cdba0. `dynamic_grip`: `Track::dynamicGripLevel`
    /// when the track's grip changes with the session; `laps`: the laps of every car, added up.
    pub fn update(&mut self, scene: &mut Scene, dt: f32, dynamic_grip: Option<f32>, lap_counts: &[u32]) {
        if !self.is_active {
            return;
        }
        let set = |scene: &mut Scene, g: &GrooveMesh, v: f32| {
            if let Some((material, var)) = g.var {
                scene.materials[material.0 as usize].update_var(var, |m| m.f_value = v);
            }
        };
        if let Some(grip) = dynamic_grip {
            let mut t = (grip - f32::from_bits(0x3f66_6666)) * 10.0;
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if t > 1.0 {
                t = 1.0;
            } else if !(t >= 0.0) {
                t = 0.0;
            }
            for g in &self.grooves {
                if g.var.is_some() {
                    set(scene, g, (g.max_alpha - g.min_alpha) * t + g.min_alpha);
                }
            }
            return;
        }
        let mut laps = self.starting_laps_count;
        for count in lap_counts {
            laps += *count as f32;
        }
        let clamp = |v: f32, min: f32, max: f32| if v > max { max } else if v >= min { v } else { min };
        for k in 0..self.grooves.len() {
            let g = &mut self.grooves[k];
            let target = (laps / (g.mult * self.max_number_of_laps)) * (g.max_alpha - g.min_alpha) + g.min_alpha;
            g.target_alpha = clamp(target, g.min_alpha, g.max_alpha);
            let d = (g.target_alpha - g.current_alpha) * dt;
            if d > 0.0 && !self.reset_to_target {
                let c = g.current_alpha + d;
                g.current_alpha = if c >= g.target_alpha { g.target_alpha } else { c };
            } else {
                g.current_alpha = g.target_alpha;
                self.reset_to_target = false;
            }
            let g = &self.grooves[k];
            if g.var.is_some() {
                set(scene, g, clamp(g.current_alpha, g.min_alpha, g.max_alpha));
            }
        }
    }

    /// The meshes `setGrooveMeshVisibility` switches off while a cube map's faces are drawn.
    pub fn meshes(&self) -> Vec<NodeId> {
        self.grooves.iter().filter_map(|g| g.mesh).collect()
    }
}

/// `DynamicTrackObject` (0x38 bytes): a model that drifts over the track (a balloon, an
/// aircraft).
pub struct DynamicTrackObject {
    pub node: NodeId,
    pub pos: [f32; 3],
    pub pos_range: [f32; 3],
    pub vel: [f32; 3],
    pub org_pos: [f32; 3],
}

/// `ksRandVec3f` 0x1401cc4e0: a number in -range .. range for each axis, drawn z, y, x.
fn rand_vec3(rand: &mut MsvcRand, range: [f32; 3]) -> [f32; 3] {
    let c = f32::from_bits(0x3800_0100);
    let mut out = [0.0f32; 3];
    for k in [2usize, 1, 0] {
        let r = range[k];
        out[k] = (rand.next() as f32 * c) * (r - (-r)) + (-r);
    }
    out
}

/// `TrackAvatar::initDynamicObjects` 0x1401c9840: the `[DYNAMIC_OBJECT_n]` sections of the
/// track's `models.ini` (`models_<layout>.ini`). Each is there with its `PROBABILITY`, as many
/// times as `MULT` draws, at a random place with a random speed, all from the main thread's
/// `rand()`. The models become children of the track's model.
pub fn init_dynamic_objects(graphics: &mut Graphics, scene: &mut Scene, track_folder: &Path, layout: &str, model: NodeId) -> Vec<DynamicTrackObject> {
    let mut objects = Vec::new();
    let file = if layout.is_empty() { track_folder.join("models.ini") } else { track_folder.join(format!("models_{layout}.ini")) };
    let Ok(ini) = IniReader::load(&file) else { return objects };
    let c = f32::from_bits(0x3800_0100);
    let io = crate::model::Kn5Io::new();
    let mut i = 0;
    loop {
        let section = format!("DYNAMIC_OBJECT_{i}");
        if !ini.has_section(&section) {
            break;
        }
        i += 1;
        let probability = ini.get_int(&section, "PROBABILITY").unwrap_or(0);
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !((graphics.crt_rand.next() as f32 * c) * 100.0 < probability as f32) {
            continue;
        }
        let mult: Vec<f32> = ini.get_string(&section, "MULT").split(',').map(|p| p.trim().parse::<f64>().unwrap_or(0.0) as f32).collect();
        let (x, y) = (mult.first().copied().unwrap_or(0.0), mult.get(1).copied().unwrap_or(0.0));
        let n = ((graphics.crt_rand.next() as f32 * c) * ((y + 1.0) - x) + x) as i32;
        for _ in 0..n.max(0) {
            let name = ini.get_string(&section, "FILE");
            let path = track_folder.join(&name);
            let filename = crate::model::path_text(&path);
            // (a file that is not there: the game goes on with an empty node)
            let node = match io.load(graphics, scene, &filename, &path) {
                Ok(node) => node,
                Err(_) => scene.node(&format!("KN5: {filename}")),
            };
            let get3 = |key: &str| ini.get_float3(&section, key).unwrap_or([0.0; 3]);
            let (mut pos, mut pos_range, mut vel) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
            if ini.get_string(&section, "POS_MODE") == "RANDOM" {
                let center = get3("RND_POS_CENTER");
                pos_range = get3("RND_POS_RANGE");
                let r = rand_vec3(&mut graphics.crt_rand, pos_range);
                pos = [center[0] + r[0], r[1] + center[1], r[2] + center[2]];
            }
            if ini.get_string(&section, "VEL_MODE") == "RANDOM" {
                let base = get3("RND_VEL_BASE");
                let r = rand_vec3(&mut graphics.crt_rand, get3("RND_VEL_RANGE"));
                vel = [r[0] + base[0], r[1] + base[1], r[2] + base[2]];
            }
            scene.compile(graphics, node);
            scene.add_child(model, node);
            objects.push(DynamicTrackObject { node, pos, pos_range, vel, org_pos: pos });
        }
    }
    objects
}

/// `TrackAvatar::updateDynamicObjects` 0x1401ccb30.
pub fn update_dynamic_objects(scene: &mut Scene, objects: &mut [DynamicTrackObject], dt: f32) {
    for o in objects {
        for k in 0..3 {
            o.pos[k] = dt * o.vel[k] + o.pos[k];
            scene.nodes[o.node].matrix.m[3][k] = o.pos[k];
        }
    }
}
