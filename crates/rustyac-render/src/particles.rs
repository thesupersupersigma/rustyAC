// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Smoke: `ParticleSystem` (0x1b0 bytes; `ParticleSystem::ParticleSystem` 0x14025fbc0,
//! `addParticle` 0x1402602f0, `step` 0x140260df0, `render` 0x1402607e0), `ParticleGenerator`
//! (0xb0 bytes; constructor 0x140260fc0, `loadINI` 0x140261550, `generateParticle`
//! 0x1402611c0), `TyreSmoke` (0xa8 bytes; constructor 0x1401d0000, `update` 0x1401d0b80) and
//! `EngineSmoke` (0x90 bytes; constructor 0x140092de0, `update` 0x1400935e0).
//!
//! Every system is a node under the scene's `PARTICLES_NODE`, drawn in the transparent pass
//! as quads that face the camera, built on the CPU, oldest first, never sorted. The random
//! numbers are the C runtime's `rand()` of the main thread, eleven per particle.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::MsvcRand;
use rustyac_physics::vecmath::Mat44f;

use crate::graphics::Graphics;
use crate::kgl::{KglIndexBuffer, KglVertexBuffer};
use crate::scene::{NodeId, OnNodeRenderEvent, RenderableObject, Scene};
use crate::shader::ShaderId;
use crate::state::CarPhysicsState;
use crate::texture::Texture;

const VERTEX: usize = 36;

/// `Particle` (0x48 bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub half_size: [f32; 2],
    pub size_velocity: [f32; 2],
    pub color: [f32; 3],
    pub gravity: f32,
    pub drag: f32,
    pub time_alive: f32,
    pub opacity: f32,
    pub opacity_velocity: f32,
}

pub struct ParticleSystem {
    pub emissive_blend: f32,
    pub min_distance: f32,
    pub blend_mode: i32,
    pub depth_mode: i32,
    pub max_time_alive: f32,
    pub particles: Vec<Particle>,
    pub max_particles_count: i32,
    pub node: NodeId,
    buffer: KglVertexBuffer,
    ib: KglIndexBuffer,
    staging: Vec<u8>,
    texture: Texture,
    shader: Option<ShaderId>,
    var_emissive_blend: Option<usize>,
    var_shaded: Option<usize>,
    /// the last `render` drew: it ended with `MaterialFilter::resetMaterialCache`
    drew: bool,
}

impl ParticleSystem {
    /// `ParticleSystem::ParticleSystem` 0x14025fbc0; the node is not yet anyone's child.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, name: &str, max_particles: i32, texture: &Texture) -> Rc<RefCell<ParticleSystem>> {
        let max = max_particles.max(0) as usize;
        // generateIndexBuffer 0x1402603b0
        let mut indices = Vec::with_capacity(max * 6);
        for q in 0..max {
            let b = (q * 4) as u16;
            indices.extend_from_slice(&[b, b.wrapping_add(1), b.wrapping_add(2), b, b.wrapping_add(2), b.wrapping_add(3)]);
        }
        let ib = graphics.kgl.create_index_buffer(&indices);
        let mut staging = vec![0u8; max * 4 * VERTEX];
        let buffer = graphics.kgl.create_vertex_buffer(&staging, staging.len(), VERTEX as u32, true);
        let shader = graphics.shaders.get_shader(&graphics.kgl, "ksParticle").ok();
        let (mut var_emissive_blend, mut var_shaded) = (None, None);
        let min_distance = 1.5f32;
        if let Some(id) = shader {
            let s = graphics.shaders.get_mut(id);
            var_emissive_blend = s.get_var("emissiveBlend");
            let var_min_distance = s.get_var("minDistance");
            var_shaded = s.get_var("shaded");
            if let Some(v) = var_shaded {
                s.set_var(v, &0i32.to_le_bytes());
            }
            if let Some(v) = var_min_distance {
                s.set_var(v, &min_distance.to_le_bytes());
            }
        }
        // the corners' texture coordinates, written once
        for q in 0..max {
            for (k, uv) in [[0.0f32, 1.0f32], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]].iter().enumerate() {
                let at = (q * 4 + k) * VERTEX + 0x1c;
                staging[at..at + 4].copy_from_slice(&uv[0].to_le_bytes());
                staging[at + 4..at + 8].copy_from_slice(&uv[1].to_le_bytes());
            }
        }
        let node = scene.object_node(name);
        let system = Rc::new(RefCell::new(ParticleSystem {
            emissive_blend: 0.5,
            min_distance,
            blend_mode: 1,
            depth_mode: 1,
            max_time_alive: 10.0,
            particles: Vec::new(),
            max_particles_count: max_particles,
            node,
            buffer,
            ib,
            staging,
            texture: texture.clone(),
            shader,
            var_emissive_blend,
            var_shaded,
            drew: false,
        }));
        scene.set_renderable_object(node, system.clone());
        system
    }

    /// `ParticleSystem::addParticle` 0x1402602f0: a full system drops the particle.
    pub fn add_particle(&mut self, p: &Particle) {
        if (self.particles.len() as u64) < self.max_particles_count as u32 as u64 {
            self.particles.push(*p);
        }
    }

    /// `ParticleSystem::clearParticles` 0x140260340.
    pub fn clear_particles(&mut self) {
        self.particles.clear();
    }

    /// `ParticleSystem::step` 0x140260df0. After a particle is taken out the loop goes on with
    /// the next slot, so the one that moved into its place is not looked at in this step.
    pub fn step(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.particles.len() {
            let p = &self.particles[i];
            // comiss 0, opacity; jae: taken out when 0 >= opacity. Then kept only when
            // maxTimeAlive >= timeAlive.
            let dead = 0.0 >= p.opacity || !(self.max_time_alive >= p.time_alive);
            if dead {
                self.particles.remove(i);
                if i == self.particles.len() {
                    break;
                }
            }
            i += 1;
        }
        for p in &mut self.particles {
            p.time_alive = dt + p.time_alive;
            let nd = -(dt * p.drag);
            let ng = -(dt * p.gravity);
            let vx = p.velocity[0] * nd + p.velocity[0];
            let vy = ((nd * p.velocity[1]) + ng) + p.velocity[1];
            let vz = nd * p.velocity[2] + p.velocity[2];
            p.velocity = [vx, vy, vz];
            p.position[0] = dt * vx + p.position[0];
            p.position[1] = vy * dt + p.position[1];
            p.position[2] = vz * dt + p.position[2];
            p.half_size[0] = dt * p.size_velocity[0] + p.half_size[0];
            p.half_size[1] = dt * p.size_velocity[1] + p.half_size[1];
            p.opacity = dt * p.opacity_velocity + p.opacity;
        }
    }
}

impl RenderableObject for ParticleSystem {
    /// `ParticleSystem::render` 0x1402607e0: only in the transparent pass.
    fn render(&mut self, _scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) -> bool {
        self.drew = false;
        if event.pass_id != 1 || self.particles.is_empty() {
            return false;
        }
        let view = graphics.state.view_matrix;
        let m = |r: usize, c: usize| view.m[r][c];
        let mut n = 0usize;
        let mut drawn = 0u32;
        for p in &self.particles {
            if drawn >= self.max_particles_count as u32 {
                continue;
            }
            // comiss: a NaN is left out
            if !(p.opacity > 0.0) {
                continue;
            }
            let (hx, hy) = (p.half_size[0], p.half_size[1]);
            for (k, (dx, dy)) in [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)].into_iter().enumerate() {
                let x = (((dx * m(0, 0)) + (dy * m(0, 1))) + (m(0, 2) * 0.0)) + p.position[0];
                let y = (((dx * m(1, 0)) + (dy * m(1, 1))) + (m(1, 2) * 0.0)) + p.position[1];
                let z = (((dx * m(2, 0)) + (dy * m(2, 1))) + (m(2, 2) * 0.0)) + p.position[2];
                let at = (n + k) * VERTEX;
                for (i, v) in [x, y, z, p.color[0], p.color[1], p.color[2], p.opacity].into_iter().enumerate() {
                    self.staging[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            n += 4;
            drawn += 1;
        }
        if n == 0 {
            return false;
        }
        graphics.kgl.vertex_buffer_map(&self.buffer, &self.staging[..n * VERTEX]);
        graphics.set_texture(0, &self.texture);
        graphics.set_blend_mode(self.blend_mode);
        graphics.kgl.set_vertex_buffer(&self.buffer);
        graphics.kgl.set_index_buffer(&self.ib);
        if let Some(id) = self.shader {
            for (var, bytes) in [(self.var_emissive_blend, self.emissive_blend.to_le_bytes()), (self.var_shaded, 0i32.to_le_bytes())] {
                if let Some(v) = var {
                    let s = graphics.shaders.get_mut(id);
                    s.set_var(v, &bytes);
                    if let Some(b) = s.vars[v].buffer {
                        s.cbuffers[b].commit(&graphics.kgl);
                    }
                }
            }
            graphics.set_shader(id);
        }
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_cull_mode(0);
        graphics.commit_shader_changes();
        graphics.set_depth_mode(self.depth_mode);
        graphics.kgl.draw_indexed((drawn * 6) as i32, 0, 0);
        self.drew = true;
        false
    }

    fn resets_material_cache(&self) -> bool {
        self.drew
    }
}

/// `ParticleGenerator` (0xb0 bytes).
pub struct ParticleGenerator {
    pub velocity_base: [f32; 3],
    pub velocity_random: [f32; 3],
    pub size_base: [f32; 2],
    pub size_random: [f32; 2],
    pub size_velocity_base: [f32; 2],
    pub size_velocity_random: [f32; 2],
    pub color_base: [f32; 3],
    pub color_random: [f32; 3],
    pub drag_base: f32,
    pub drag_random: f32,
    pub gravity_base: f32,
    pub gravity_random: f32,
    pub opacity_base: f32,
    pub opacity_random: f32,
    pub opacity_velocity_base: f32,
    pub opacity_velocity_random: f32,
    pub frequency: f64,
    pub system: Rc<RefCell<ParticleSystem>>,
    pub last_generation_time: f64,
}

/// `INIReader::getVector2` / `getVector3`: numbers with commas between them, what is missing
/// is 0.
fn floats<const N: usize>(ini: &IniReader, key: &str) -> [f32; N] {
    let text = ini.get_string("SETTINGS", key);
    let mut out = [0.0f32; N];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.trim().parse::<f64>().unwrap_or(0.0) as f32;
    }
    out
}

/// `rand()` as the generator turns it into -1..1: three float operations in this order.
fn unit(rand: &mut MsvcRand) -> f32 {
    (rand.next() as f32 * f32::from_bits(0x3800_0100)) * 2.0 - 1.0
}

impl ParticleGenerator {
    /// `ParticleGenerator::ParticleGenerator` 0x140260fc0 and `loadINI` 0x140261550: a file
    /// that opens overwrites every value (a missing key is 0).
    pub fn new(game_folder: &Path, system: Rc<RefCell<ParticleSystem>>, file: &str, frequency: f32) -> ParticleGenerator {
        let mut g = ParticleGenerator {
            velocity_base: [0.0; 3],
            velocity_random: [1.0; 3],
            size_base: [1.0; 2],
            size_random: [0.0; 2],
            size_velocity_base: [f32::from_bits(0x3ecc_cccd); 2],
            size_velocity_random: [f32::from_bits(0x3dcc_cccd); 2],
            color_base: [1.0; 3],
            color_random: [f32::from_bits(0x3dcc_cccd); 3],
            drag_base: f32::from_bits(0x3dcc_cccd),
            drag_random: 0.0,
            gravity_base: f32::from_bits(0x3dcc_cccd),
            gravity_random: 0.0,
            opacity_base: f32::from_bits(0x3e99_999a),
            opacity_random: f32::from_bits(0x3e4c_cccd),
            opacity_velocity_base: f32::from_bits(0xbc23_d70a),
            opacity_velocity_random: 0.0,
            frequency: frequency as f64,
            system,
            last_generation_time: 0.0,
        };
        if let Ok(ini) = IniReader::load(&game_folder.join(file)) {
            let get = |key: &str| ini.get_float("SETTINGS", key).unwrap_or(0.0);
            g.system.borrow_mut().emissive_blend = get("EMISSIVE_BLEND");
            g.size_base = floats(&ini, "SIZE_BASE");
            g.size_random = floats(&ini, "SIZE_RANDOM");
            g.color_base = floats(&ini, "COLOR_BASE");
            g.color_random = floats(&ini, "COLOR_RANDOM");
            g.drag_base = get("DRAG_BASE");
            g.drag_random = get("DRAG_RANDOM");
            g.gravity_base = get("GRAVITY_BASE");
            g.gravity_random = get("GRAVITY_RANDOM");
            g.opacity_base = get("OPACITY_BASE");
            g.opacity_random = get("OPACITY_RANDOM");
            g.opacity_velocity_base = get("OPACITY_VELOCITY_BASE");
            g.opacity_velocity_random = get("OPACITY_VELOCITY_RANDOM");
            g.size_velocity_base = floats(&ini, "SIZE_VELOCITY_BASE");
            g.size_velocity_random = floats(&ini, "SIZE_VELOCITY_RANDOM");
            g.velocity_random = floats(&ini, "VELOCITY_RANDOM");
            g.velocity_base = floats(&ini, "VELOCITY_BASE");
            if g.frequency == 0.0 {
                g.frequency = 60.0;
            }
        }
        g
    }

    /// `ParticleGenerator::generateParticle` 0x1402611c0: at most one particle a call, none
    /// while the time since the last one is under `1 / (frequency * mult)` (the clock is in
    /// milliseconds).
    pub fn generate_particle(&mut self, pos: &[f32; 3], vel: &[f32; 3], now: f64, mult: f64, rand: &mut MsvcRand) {
        if self.frequency != -1.0 {
            let interval = 1.0 / (self.frequency * mult);
            // comisd; jb: goes on only when the difference is not below (a NaN leaves)
            if !(now - self.last_generation_time >= interval) {
                return;
            }
        }
        self.last_generation_time = now;
        let mut p = Particle { position: *pos, ..Particle::default() };
        p.velocity[2] = (vel[2] + self.velocity_base[2]) + unit(rand) * self.velocity_random[2];
        p.velocity[1] = (vel[1] + self.velocity_base[1]) + unit(rand) * self.velocity_random[1];
        p.velocity[0] = (vel[0] + self.velocity_base[0]) + unit(rand) * self.velocity_random[0];
        p.half_size[1] = ((unit(rand) * self.size_random[1]) + self.size_base[1]) * 0.5;
        p.half_size[0] = ((unit(rand) * self.size_random[0]) + self.size_base[0]) * 0.5;
        let c = unit(rand);
        p.color = [c * self.color_random[0] + self.color_base[0], c * self.color_random[1] + self.color_base[1], c * self.color_random[2] + self.color_base[2]];
        p.drag = unit(rand) * self.drag_random + self.drag_base;
        p.gravity = unit(rand) * self.gravity_random + self.gravity_base;
        p.opacity = unit(rand) * self.opacity_random + self.opacity_base;
        p.opacity_velocity = unit(rand) * self.opacity_velocity_random + self.opacity_velocity_base;
        let c = unit(rand);
        p.size_velocity = [c * self.size_velocity_random[0] + self.size_velocity_base[0], c * self.size_velocity_random[1] + self.size_velocity_base[1]];
        p.time_alive = 0.0;
        self.system.borrow_mut().add_particle(&p);
    }
}

/// What a frame gives the smoke besides the car's state.
#[derive(Clone, Copy, Debug)]
pub struct SmokeFrame {
    pub dt: f32,
    /// `Game::gameTime.now`, milliseconds
    pub now_ms: f64,
    /// `CarAvatar::isConnected`: the car's node is active
    pub connected: bool,
    /// the replay's speed while a replay plays (`ReplayManager::isInReplaymode`)
    pub replay_time_mult: Option<f32>,
    /// `ReplayManager::isActive`
    pub replay_active: bool,
}

/// The input velocity of every generator: a hundredth of the car's, and 1 m/s up.
fn velocity_in(state: &CarPhysicsState) -> [f32; 3] {
    let k = f32::from_bits(0x3c23_d70a);
    [state.velocity[0] * k, state.velocity[1] * k + 1.0, state.velocity[2] * k]
}

/// `video.ini [EFFECTS] SMOKE` as the two constructors read it: `None` when there is no file.
fn smoke_level(smoke: Option<i32>, table: [(i32, f32); 5]) -> Option<(i32, f32)> {
    match smoke {
        None => Some(table[2]),
        Some(0) => None,
        Some(n @ 1..=5) => Some(table[n as usize - 1]),
        Some(_) => Some(table[2]),
    }
}

/// `TyreSmoke`: one per wheel.
pub struct TyreSmoke {
    pub tyre_index: usize,
    /// `GameObject::isActive`: the pause and the replay's status switch the object off
    pub is_active: bool,
    pub time_mult: f32,
    pub trigger_slip_level: f32,
    /// (smoke, grass smoke, grass pieces) and their generators; `None`: smoke is off
    systems: Option<TyreSystems>,
}

struct TyreSystems {
    system: Rc<RefCell<ParticleSystem>>,
    grass: Rc<RefCell<ParticleSystem>>,
    pieces: Rc<RefCell<ParticleSystem>>,
    generator: ParticleGenerator,
    generator_grass: ParticleGenerator,
    generator_pieces: ParticleGenerator,
}

impl TyreSmoke {
    /// `TyreSmoke::TyreSmoke` 0x1401d0000. `cars_to_be_loaded`: `RaceManager::carsToBeLoaded`.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, particles_node: NodeId, tyre_index: usize, smoke: Option<i32>, cars_to_be_loaded: i32) -> TyreSmoke {
        let game = crate::model::path_text(&graphics.game_folder);
        let folder = graphics.game_folder.clone();
        let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/smoke_0.png"));
        let level = smoke_level(smoke, [(200, f32::from_bits(0x3d4c_cccd)), (400, f32::from_bits(0x3dcc_cccd)), (2000, 60.0), (4000, 120.0), (8000, 240.0)]);
        let systems = level.map(|(mut max, frequency)| {
            if cars_to_be_loaded > 25 && max >= 4000 {
                max = 4000;
            }
            let system = ParticleSystem::new(graphics, scene, "TYRE_SMOKE_PARTICLE", max, &texture);
            let generator = ParticleGenerator::new(&folder, system.clone(), "system/cfg/tyre_smoke.ini", frequency);
            scene.add_child(particles_node, system.borrow().node);
            let grass = ParticleSystem::new(graphics, scene, "TYRE_SMOKE_PARTICLE", max, &texture);
            let generator_grass = ParticleGenerator::new(&folder, grass.clone(), "system/cfg/tyre_smoke_grass.ini", frequency);
            let grass_texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/grass.png"));
            let pieces = ParticleSystem::new(graphics, scene, "TYRE_SMOKE_PARTICLE", max, &grass_texture);
            let generator_pieces = ParticleGenerator::new(&folder, pieces.clone(), "system/cfg/tyre_pieces_grass.ini", frequency);
            pieces.borrow_mut().max_time_alive = 3.0;
            scene.add_child(particles_node, grass.borrow().node);
            scene.add_child(particles_node, pieces.borrow().node);
            TyreSystems { system, grass, pieces, generator, generator_grass, generator_pieces }
        });
        let trigger_slip_level = IniReader::load(&folder.join("system/cfg/tyre_smoke.ini")).ok().map(|ini| ini.get_float("TRIGGERS", "SLIP_LEVEL").unwrap_or(0.0)).unwrap_or(0.0);
        TyreSmoke { tyre_index, is_active: true, time_mult: 1.0, trigger_slip_level, systems }
    }

    /// `TyreSmoke::update` 0x1401d0b80.
    pub fn update(&mut self, scene: &mut Scene, state: &CarPhysicsState, frame: &SmokeFrame, rand: &mut MsvcRand) {
        let Some(s) = &mut self.systems else { return };
        let i = self.tyre_index;
        if !frame.connected {
            for system in [&s.system, &s.grass, &s.pieces] {
                let mut system = system.borrow_mut();
                system.clear_particles();
                scene.nodes[system.node].is_active = false;
            }
            return;
        }
        for system in [&s.system, &s.grass, &s.pieces] {
            scene.nodes[system.borrow().node].is_active = true;
        }
        let dt = frame.dt * self.time_mult;
        s.system.borrow_mut().step(dt);
        s.grass.borrow_mut().step(dt);
        s.pieces.borrow_mut().step(dt);
        let threshold = (((state.load[1] + state.load[0]) + state.load[2]) + state.load[3]) * 0.125;
        if 0.0 >= self.time_mult {
            return;
        }
        let pos = state.tyre_contact_point[i];
        let vin = velocity_in(state);
        let mult = self.time_mult as f64;
        let moving = !(1.0 >= state.speed) || state.wheel_angular_speed[i].abs() > 8.0;
        let loaded = !(threshold >= state.load[i]);
        let surface = &state.tyre_surface_def[i];
        if moving && state.slip_ratio[i].abs() > self.trigger_slip_level && loaded && !(f32::from_bits(0x3f66_6666) > surface.grip_mod) {
            s.generator.generate_particle(&pos, &vin, frame.now_ms, mult, rand);
        }
        if moving && surface.dirt_additive_k.abs() > f32::from_bits(0x3dcc_cccd) && loaded {
            s.generator_pieces.generate_particle(&pos, &vin, frame.now_ms, mult, rand);
            s.generator_grass.generate_particle(&pos, &vin, frame.now_ms, mult, rand);
        }
    }

    /// `TyreSmoke::onReplayStatusChanged` 0x1401d0ad0.
    pub fn on_replay_status_changed(&mut self, status: i32, time_mult: f32, slow_mo: f32) {
        self.is_active = (status.wrapping_sub(1) as u32) > 1;
        if status == 5 {
            if slow_mo > 0.0 {
                self.time_mult = 1.0 / slow_mo;
            }
        } else if (status.wrapping_sub(3) as u32) <= 1 {
            self.time_mult = time_mult;
        } else {
            self.time_mult = 1.0;
        }
        if matches!(status, 2 | 6 | 7 | 9 | 10) {
            self.clear();
            self.time_mult = time_mult;
        }
    }

    /// The handler of `Sim::evOnNewSession`: every particle goes.
    pub fn clear(&mut self) {
        if let Some(s) = &self.systems {
            for system in [&s.system, &s.grass, &s.pieces] {
                system.borrow_mut().clear_particles();
            }
        }
    }
}

/// `EngineSmoke`: a dying engine smokes from between the rear wheels.
pub struct EngineSmoke {
    system: Option<(Rc<RefCell<ParticleSystem>>, ParticleGenerator)>,
    /// `GameObject::isActive`
    pub is_active: bool,
    /// `EngineSmokeStatus live` and `replay`: (oldValue, smokeTimer)
    pub live: (f32, f32),
    pub replay: (f32, f32),
    pub trigger_engine_life: f32,
    pub max_smoke_timer: f32,
}

impl EngineSmoke {
    /// `EngineSmoke::EngineSmoke` 0x140092de0.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, particles_node: NodeId, smoke: Option<i32>) -> EngineSmoke {
        let game = crate::model::path_text(&graphics.game_folder);
        let folder = graphics.game_folder.clone();
        let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/texture/smoke_0.png"));
        let level = smoke_level(smoke, [(500, 15.0), (1000, 30.0), (2000, 60.0), (4000, 120.0), (8000, 240.0)]);
        let mut max_smoke_timer = 20.0;
        let system = level.map(|(max, frequency)| {
            let system = ParticleSystem::new(graphics, scene, "ENGINE_SMOKE_PARTICLE", max, &texture);
            let generator = ParticleGenerator::new(&folder, system.clone(), "system/cfg/engine_smoke.ini", frequency);
            scene.add_child(particles_node, system.borrow().node);
            if let Ok(ini) = IniReader::load(&folder.join("system/cfg/engine_smoke.ini")) {
                if ini.has_section("LIFE_TIME") {
                    max_smoke_timer = ini.get_float("LIFE_TIME", "SECONDS").unwrap_or(0.0);
                }
            }
            (system, generator)
        });
        EngineSmoke { system, is_active: true, live: (1000.0, 0.0), replay: (1000.0, 0.0), trigger_engine_life: 25.0, max_smoke_timer }
    }

    /// `EngineSmoke::update` 0x1400935e0.
    pub fn update(&mut self, state: &CarPhysicsState, frame: &SmokeFrame, rand: &mut MsvcRand) {
        let Some((system, generator)) = &mut self.system else { return };
        if !frame.connected {
            return;
        }
        let mut mult = 1.0f32;
        let mut dt = frame.dt;
        if let Some(m) = frame.replay_time_mult {
            mult = m;
            dt *= mult;
        }
        system.borrow_mut().step(dt);
        let mut life = state.engine_life_left;
        if !(life >= 0.0) {
            life = 0.0;
        }
        if life > self.trigger_engine_life {
            return;
        }
        // the replay keeps a pair of its own (`ReplayManager::isActive`)
        let slot = if frame.replay_active { &mut self.replay } else { &mut self.live };
        if life != slot.0 {
            *slot = (life, self.max_smoke_timer);
        }
        if !(slot.1 > 0.0) {
            return;
        }
        let (t2, t3) = (&state.tyre_matrix[2].m[3], &state.tyre_matrix[3].m[3]);
        let pos = [(t2[0] + t3[0]) * 0.5, (t2[1] + t3[1]) * 0.5, (t2[2] + t3[2]) * 0.5];
        let vin = velocity_in(state);
        generator.generate_particle(&pos, &vin, frame.now_ms, mult as f64, rand);
        slot.1 -= dt;
    }

    /// The handler of `Sim::evOnReplayStatusChanged` 0x1400934c0.
    pub fn on_replay_status_changed(&mut self, status: i32) {
        self.is_active = (status.wrapping_sub(1) as u32) > 1;
        if status == 6 {
            self.replay = (1000.0, 0.0);
        }
        if matches!(status, 2 | 6 | 7 | 9 | 10) {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        if let Some((system, _)) = &self.system {
            system.borrow_mut().clear_particles();
        }
    }
}
