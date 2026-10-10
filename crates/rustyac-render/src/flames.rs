// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Exhaust flames: `Flames` (0x158 bytes; `Flames::Flames` 0x1400ff990, `loadTextures`
//! 0x140103620, `loadFlames` 0x140101f90, `readFlamePresets` 0x1401041a0, `copyFlameVector`
//! 0x140100e60, `cycleTexture` 0x140100f00, `drawBackfireState` 0x140101020, `drawFlame`
//! 0x1401014e0, `onNodeRenderEvent` 0x140103aa0, `update` 0x1401046a0, the backfire handler
//! 0x140100070) and `BackfireParams` (constructor 0x1400cccb0, `checkBackfire` 0x1400d26b0)
//! with the test at the head of `CarAvatar::update` 0x1400db830.
//!
//! Only the game's "version 2" flames are here (cars with `data/flame_presets.ini`: every
//! car of the game that has flames): quads that face the camera, drawn after everything else
//! of the transparent pass, timed in frames. Version 1 (three crossed quads, cars without
//! presets) has no car in the game and is not ported.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rustyac_math::{cosf, sinf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::MsvcRand;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::gl::{GlRenderer, GL_QUADS};
use crate::graphics::{Graphics, BLEND_ALPHA};
use crate::scene::{NodeEventHandler, NodeId, OnNodeRenderEvent, Scene};
use crate::state::CarPhysicsState;
use crate::texture::Texture;

/// `BackfireParams` (0x28 bytes), from `sounds.ini [BACKFIRE]`.
#[derive(Clone, Copy, Debug)]
pub struct BackfireParams {
    pub max_gas: f32,
    pub new_max_gas: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub trigger_gas: f32,
    pub new_trigger_gas: f32,
    pub last_gas: f32,
    pub trigger_ready: bool,
}

impl BackfireParams {
    /// `BackfireParams::BackfireParams` 0x1400cccb0.
    pub fn new(folder: &Path) -> BackfireParams {
        let mut p = BackfireParams { max_gas: 0.25, new_max_gas: 0.25, min_rpm: 9750.0, max_rpm: 24000.0, trigger_gas: 0.8, new_trigger_gas: 0.8, last_gas: 0.6, trigger_ready: false };
        if let Ok(ini) = IniReader::load(&folder.join("data/sounds.ini")) {
            let get = |key: &str| ini.get_float("BACKFIRE", key).unwrap_or(0.0);
            p.max_gas = get("MAXGAS");
            if !(f32::from_bits(0x3e99_999a) >= p.max_gas) {
                p.max_gas = f32::from_bits(0x3e99_999a);
            }
            p.min_rpm = get("MINRPM");
            p.max_rpm = get("MAXRPM");
            p.trigger_gas = get("TRIGGERGAS");
        }
        p
    }

    /// `BackfireParams::checkBackfire` 0x1400d26b0: a lift after hard throttle, at revs, with
    /// "fuel in the exhaust" built up for more than a second.
    #[allow(clippy::double_comparisons)]
    pub fn check(&mut self, state: &CarPhysicsState, fuel_in_exhaust: &mut f32, ai_active: bool, dt: f32) -> bool {
        if 0.0 >= state.fuel {
            return false;
        }
        if 0.0 >= state.engine_life_left {
            return false;
        }
        let gas = state.gas;
        if gas > self.last_gas && (gas < 0.0 || gas > 0.0) {
            self.last_gas = gas;
            self.new_trigger_gas = gas * self.trigger_gas;
            self.new_max_gas = self.max_gas * gas;
        }
        if !(self.new_trigger_gas >= gas) {
            self.trigger_ready = true;
        }
        if !self.trigger_ready {
            return false;
        }
        let rpm = state.engine_rpm;
        let all = !(gas >= self.new_max_gas) && !gas.is_nan() && (gas < 0.0 || gas > 0.0) && rpm > self.min_rpm && !(rpm > self.max_rpm) && !(1.0 >= *fuel_in_exhaust);
        if all {
            self.trigger_ready = false;
            if ai_active && f32::from_bits(0x3dcc_cccd) >= state.brake {
                return false;
            }
            true
        } else {
            let x = dt + *fuel_in_exhaust;
            *fuel_in_exhaust = if !(x > 10.0) { x } else { 10.0 };
            false
        }
    }
}

/// `FlameTexture` (0x68 bytes): one quad of a preset.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlameTexture {
    pub offset: [f32; 3],
    pub size: f32,
    pub trigger_time: f32,
    /// 0: the flash textures, 1: the body's, 2: the tail's
    pub flametype: i32,
    pub color: [f32; 4],
}

/// `FlameInstanceV2` (0xa8 bytes): one exhaust.
#[derive(Clone, Debug)]
pub struct FlameInstance {
    pub matrix: Mat44f,
    pub size_mult: f32,
    pub is_left: bool,
    pub start: Vec<FlameTexture>,
    pub loop_: Vec<FlameTexture>,
    pub end: Vec<FlameTexture>,
    pub flash: Vec<FlameTexture>,
}

/// `FlameGroup` (0x38 bytes). `state`: 0 idle, 1 start, 2 loop, 3 end.
#[derive(Clone, Debug)]
pub struct FlameGroup {
    pub fuel_in_exhaust: f32,
    pub fuel_diff: f32,
    pub group: i16,
    pub state: i32,
    pub vis_time: f32,
    pub frame_time_switch: i32,
    pub flash_times: f32,
    pub flames: Vec<FlameInstance>,
}

/// `INIReader::getFloat3` / `getFloat4`: numbers with commas between them.
fn floats<const N: usize>(ini: &IniReader, section: &str, key: &str) -> [f32; N] {
    let text = ini.get_string(section, key);
    let mut out = [0.0f32; N];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.trim().parse::<f64>().unwrap_or(0.0) as f32;
    }
    out
}

/// A length the game tests with `ucomiss …, 0; je`: neither 0 nor a NaN.
fn usable(length: f32) -> bool {
    length != 0.0 && !length.is_nan()
}

/// `mat44f::createTarget` 0x14005ff50.
fn create_target(eye: &[f32; 3], target: &[f32; 3]) -> Mat44f {
    let mut out = Mat44f::IDENTITY;
    let (mut fx, mut fy, mut fz) = (target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]);
    let l = sqrtf((fx * fx + fy * fy) + fz * fz);
    if usable(l) {
        let inv = 1.0 / l;
        fx *= inv;
        fy *= inv;
        fz *= inv;
    }
    let mut ay = fz * 0.0 - fx * 0.0;
    let mut az = fx - fy * 0.0;
    let mut ax = fy * 0.0 - fz;
    let l = sqrtf((ay * ay + ax * ax) + az * az);
    if usable(l) {
        let inv = 1.0 / l;
        az *= inv;
        ax *= inv;
        ay *= inv;
    }
    let mut uy = ax * fz - az * fx;
    let mut ux = az * fy - fz * ay;
    let mut uz = fx * ay - ax * fy;
    let l = sqrtf((uy * uy + ux * ux) + uz * uz);
    if usable(l) {
        let inv = 1.0 / l;
        uy *= inv;
        uz *= inv;
        ux *= inv;
    }
    if !(uy >= 0.0) {
        uz *= -1.0;
        ux *= -1.0;
        uy *= -1.0;
    }
    crate::fake_shadow::set_from_heading_up(&mut out, &[fx, fy, fz], &[ux, uy, uz]);
    out.m[3][0] = eye[0];
    out.m[3][1] = eye[1];
    out.m[3][2] = eye[2];
    out
}

/// `mat44f::createBillboard` 0x14007cf30: a matrix at `object` whose z axis points from the
/// camera to it (`camera_forward` when the two are within a centimetre).
fn create_billboard(object: &[f32; 3], camera: &[f32; 3], up: &[f32; 3], camera_forward: &[f32; 3]) -> Mat44f {
    let (dx, dy, dz) = (object[0] - camera[0], object[1] - camera[1], object[2] - camera[2]);
    let len2 = (dy * dy + dx * dx) + dz * dz;
    let (lx, ly, lz) = if len2 >= f32::from_bits(0x38d1_b717) {
        let inv = 1.0 / sqrtf(len2);
        (inv * dx, inv * dy, inv * dz)
    } else {
        (camera_forward[0], camera_forward[1], camera_forward[2])
    };
    let mut rx = up[1] * lz - up[2] * ly;
    let mut ry = up[2] * lx - up[0] * lz;
    let mut rz = up[0] * ly - up[1] * lx;
    let l = sqrtf((ry * ry + rx * rx) + rz * rz);
    if usable(l) {
        let inv = 1.0 / l;
        rz *= inv;
        rx *= inv;
        ry *= inv;
    }
    Mat44f {
        m: [[rx, ry, rz, 0.0], [rz * ly - ry * lz, rx * lz - rz * lx, ry * lx - rx * ly, 0.0], [lx, ly, lz, 0.0], [object[0], object[1], object[2], 1.0]],
    }
}

pub struct Flames {
    textures: Vec<Texture>,
    flash_textures: Vec<Texture>,
    tail_textures: Vec<Texture>,
    texture_flash_index: u16,
    texture_body_index: u16,
    texture_tail_index: u16,
    rotation: i32,
    pub groups: Vec<FlameGroup>,
    cycle_index: i32,
    pub flash_threshold: f32,
    is_flashing: bool,
    current_dt: f32,
    /// 2 with `flame_presets.ini`; 1 (not ported) draws nothing
    pub version: u16,
    pub burn_fuel_mult: f32,
    gl: GlRenderer,
    body_transform: NodeId,
    /// `Sim::sceneCamera`'s matrix, as of the frame's update
    pub scene_camera: Mat44f,
}

impl Flames {
    /// `Flames::Flames` 0x1400ff990: the handler goes onto the `RENDER FINISHED` node.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &Path, body_transform: NodeId, render_finished: NodeId) -> Result<Rc<RefCell<Flames>>, String> {
        let mut flames = Flames {
            textures: Vec::new(),
            flash_textures: Vec::new(),
            tail_textures: Vec::new(),
            texture_flash_index: 0,
            texture_body_index: 0,
            texture_tail_index: 0,
            rotation: 0,
            groups: Vec::new(),
            cycle_index: 0,
            flash_threshold: 2.0,
            is_flashing: false,
            current_dt: 1.0,
            version: if IniReader::load(&folder.join("data/flame_presets.ini")).is_ok() { 2 } else { 1 },
            burn_fuel_mult: 10.0,
            gl: GlRenderer::new(graphics, 6),
            body_transform,
            scene_camera: Mat44f::IDENTITY,
        };
        flames.load_textures(graphics, &folder.join("texture/flames"));
        flames.load_flames(folder)?;
        let flames = Rc::new(RefCell::new(flames));
        scene.add_event_handler(render_finished, flames.clone());
        Ok(flames)
    }

    /// `Flames::loadTextures` 0x140103620: by the first letter of the file's name, in the
    /// folder's order (NTFS: by name, case ignored).
    fn load_textures(&mut self, graphics: &mut Graphics, dir: &Path) {
        if self.version != 2 {
            return;
        }
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| entries.flatten().filter(|e| e.path().is_file()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.to_ascii_lowercase().ends_with(".dds")).collect())
            .unwrap_or_default();
        names.sort_by_key(|n| n.to_uppercase());
        let dir_text = crate::model::path_text(dir);
        for name in names {
            let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{dir_text}/{name}"));
            match name.chars().next() {
                Some('f') => self.flash_textures.push(texture),
                Some('l') => self.tail_textures.push(texture),
                _ => self.textures.push(texture),
            }
        }
    }

    /// `Flames::readFlamePresets` 0x1401041a0. The flash presets are read three times over.
    fn read_presets(folder: &Path) -> [Vec<FlameTexture>; 4] {
        let mut out: [Vec<FlameTexture>; 4] = Default::default();
        let Ok(ini) = IniReader::load(&folder.join("data/flame_presets.ini")) else { return out };
        for pass in 0..2 {
            for (kind, word) in ["START_", "LOOP_", "END_"].iter().enumerate() {
                let mut n = 0;
                loop {
                    let name = if pass == 0 { format!("PRESET_{word}{n}") } else { format!("PRESET_FLASH_{n}") };
                    if !ini.has_section(&name) {
                        break;
                    }
                    let t = FlameTexture {
                        offset: floats(&ini, &name, "OFFSET"),
                        size: ini.get_float(&name, "SIZE").unwrap_or(0.0),
                        trigger_time: ini.get_int(&name, "TRIGGER").unwrap_or(0) as f32,
                        flametype: ini.get_int(&name, "TYPE").unwrap_or(0),
                        color: floats(&ini, &name, "RGB"),
                    };
                    out[if pass == 0 { kind } else { 3 }].push(t);
                    n += 1;
                }
            }
        }
        out
    }

    /// `Flames::copyFlameVector` 0x140100e60.
    fn copy_flame_vector(src: &[FlameTexture], size_mult: f32, is_left: bool) -> Vec<FlameTexture> {
        let mut dst = src.to_vec();
        for t in &mut dst {
            t.offset[2] = (t.offset[2] * -1.0) * size_mult;
            t.offset[1] = (t.offset[1] * -1.0) * size_mult;
            t.offset[0] = ((if is_left { 1 } else { -1 }) as f32 * t.offset[0]) * size_mult;
            t.size *= size_mult;
        }
        dst
    }

    /// `Flames::loadFlames` 0x140101f90.
    fn load_flames(&mut self, folder: &Path) -> Result<(), String> {
        self.groups.clear();
        if self.version != 2 {
            return Ok(());
        }
        let presets = Self::read_presets(folder);
        let Ok(ini) = IniReader::load(&folder.join("data/flames.ini")) else {
            println!("WARNING: flames.ini not found");
            return Ok(());
        };
        if ini.has_key("HEADER", "BURN_FUEL_MULT") {
            self.burn_fuel_mult = ini.get_float("HEADER", "BURN_FUEL_MULT").unwrap_or(0.0);
        }
        if ini.has_key("HEADER", "FLASH_THRESHOLD") {
            self.flash_threshold = ini.get_float("HEADER", "FLASH_THRESHOLD").unwrap_or(0.0);
        }
        let mut i = 0;
        loop {
            let section = format!("FLAME_{i}");
            if !ini.has_section(&section) {
                break;
            }
            let mut dir = rustyac_physics::vecmath::Vec3f::new(0.0, 0.0, 0.0);
            let d: [f32; 3] = floats(&ini, &section, "DIRECTION");
            (dir.x, dir.y, dir.z) = (d[0], d[1], d[2]);
            dir.normalize();
            let mut matrix = create_target(&[dir.x, dir.y, dir.z], &[0.0; 3]);
            let p: [f32; 3] = floats(&ini, &section, "POSITION");
            matrix.m[3][0] = p[0];
            matrix.m[3][1] = p[1];
            matrix.m[3][2] = p[2];
            let is_left = ini.has_key(&section, "IS_LEFT") && ini.get_int(&section, "IS_LEFT").unwrap_or(0) != 0;
            let size_mult = if ini.has_key(&section, "SIZE_MULT") { ini.get_float(&section, "SIZE_MULT").unwrap_or(0.0) } else { 1.0 };
            let group = ini.get_int(&section, "GROUP").unwrap_or(0) as i16;
            let copy = |v: &[FlameTexture]| Self::copy_flame_vector(v, size_mult, is_left);
            let inst = FlameInstance { matrix, size_mult, is_left, start: copy(&presets[0]), loop_: copy(&presets[1]), end: copy(&presets[2]), flash: copy(&presets[3]) };
            match self.groups.iter_mut().find(|g| g.group == group) {
                Some(g) => g.flames.push(inst),
                None => self.groups.push(FlameGroup { fuel_in_exhaust: 0.0, fuel_diff: 0.0, group, state: 0, vis_time: -1.0, frame_time_switch: 0, flash_times: 0.0, flames: vec![inst] }),
            }
            i += 1;
        }
        // Flames::checkTextures 0x140100c40: the game stops here
        for g in &self.groups {
            for inst in &g.flames {
                for t in inst.start.iter().chain(&inst.loop_).chain(&inst.end).chain(&inst.flash) {
                    let missing = match t.flametype {
                        0 => self.flash_textures.is_empty(),
                        1 => self.textures.is_empty(),
                        2 => self.tail_textures.is_empty(),
                        _ => false,
                    };
                    if missing {
                        return Err(format!("{}: a flame needs textures that texture/flames does not have", folder.display()));
                    }
                }
            }
        }
        Ok(())
    }

    /// The handler of `CarAvatar::evOnBackfireTriggered` 0x140100070: one `rand()` for every
    /// group that is idle.
    pub fn on_backfire(&mut self, car_fuel_in_exhaust: f32, rand: &mut MsvcRand) {
        if self.version != 2 {
            return;
        }
        let c = f32::from_bits(0x3800_0100);
        for g in &mut self.groups {
            if g.state != 0 {
                continue;
            }
            g.state = 1;
            g.frame_time_switch = 0;
            g.fuel_diff = 0.0;
            g.flash_times = 0.0;
            g.fuel_in_exhaust = car_fuel_in_exhaust;
            if g.fuel_in_exhaust > self.flash_threshold {
                g.vis_time = 0.0;
                g.fuel_in_exhaust -= self.flash_threshold;
                g.fuel_diff = rand.next() as f32 * c + 1.0;
                self.is_flashing = false;
            } else {
                g.vis_time = -5.0;
                let mut x = (self.flash_threshold - g.fuel_in_exhaust) + 1.0;
                let r = rand.next();
                x -= 1.0;
                let t = (r as f32 * c) * x + 1.0;
                g.flash_times = (t as i32) as i16 as i32 as f32;
                self.is_flashing = true;
                g.state = 3;
            }
        }
    }

    /// `Flames::update` 0x1401046a0. `game_dt`: `Game::gameTime.deltaT`. True when the car's
    /// own "fuel in the exhaust" is to be emptied.
    pub fn update(&mut self, dt: f32, game_dt: f32, connected: bool, replay_time_mult: Option<f32>) -> bool {
        let mut dt = dt;
        if let Some(m) = replay_time_mult {
            dt *= m;
        }
        let mut empty = false;
        if self.version != 2 {
            return false;
        }
        for g in &mut self.groups {
            self.current_dt = dt / game_dt;
            if g.state != 0 {
                empty = true;
                if connected {
                    if g.fuel_diff > 0.0 {
                        g.fuel_in_exhaust += g.fuel_diff;
                        g.fuel_diff = 0.0;
                    }
                    if g.fuel_in_exhaust > 0.0 && g.state != 1 {
                        g.fuel_in_exhaust -= self.current_dt / self.burn_fuel_mult;
                    }
                    if 0.0 > g.fuel_in_exhaust && g.state != 3 {
                        g.state = 3;
                        g.vis_time = 0.0;
                        g.fuel_in_exhaust = 0.0;
                    }
                } else {
                    g.state = 0;
                    g.vis_time = 0.0;
                }
            }
        }
        empty
    }

    /// `Flames::reset` 0x140104600.
    pub fn reset(&mut self) {
        for g in &mut self.groups {
            g.state = 0;
            g.vis_time = 0.0;
        }
    }

    /// `Flames::cycleTexture` 0x140100f00.
    fn cycle_texture(&mut self, flametype: i32, k: u16) {
        let (index, n) = match flametype {
            0 => (&mut self.texture_flash_index, self.flash_textures.len()),
            1 => (&mut self.texture_body_index, self.textures.len()),
            2 => (&mut self.texture_tail_index, self.tail_textures.len()),
            _ => return,
        };
        if n != 0 {
            *index = (index.wrapping_add(k.wrapping_add(1)) as usize % n) as u16;
        }
    }

    /// `Flames::drawFlame` 0x1401014e0 (version 2).
    fn draw_flame(&mut self, scene: &Scene, graphics: &mut Graphics, phase: i32, gi: usize) {
        if self.groups[gi].state == 0 {
            return;
        }
        let body = scene.nodes[self.body_transform].matrix;
        let cam = self.scene_camera.m;
        let mut pending = false;
        for ii in 0..self.groups[gi].flames.len() {
            self.cycle_index += 1;
            let (matrix, is_left) = (self.groups[gi].flames[ii].matrix, self.groups[gi].flames[ii].is_left);
            let n = {
                let inst = &self.groups[gi].flames[ii];
                let v = if self.is_flashing { &inst.flash } else { match phase { 1 => &inst.start, 2 => &inst.loop_, _ => &inst.end } };
                v.len()
            };
            for idx in 0..n {
                let tex = {
                    let inst = &self.groups[gi].flames[ii];
                    let v = if self.is_flashing { &inst.flash } else { match phase { 1 => &inst.start, 2 => &inst.loop_, _ => &inst.end } };
                    v[idx]
                };
                let (state, vis_time) = (self.groups[gi].state, self.groups[gi].vis_time);
                let draw = match phase {
                    1 => {
                        let mut draw = false;
                        if vis_time >= tex.trigger_time {
                            draw = true;
                            pending = idx != n - 1;
                        }
                        draw || state == 2
                    }
                    2 => {
                        let draw = vis_time >= tex.trigger_time;
                        if draw {
                            pending = idx != n - 1;
                        }
                        draw
                    }
                    3 => {
                        let draw = !(vis_time > tex.trigger_time);
                        if draw {
                            pending = true;
                        }
                        draw
                    }
                    _ => false,
                };
                if !draw {
                    continue;
                }
                let (r, o) = (&matrix.m, &tex.offset);
                let p: [f32; 3] = std::array::from_fn(|k| (((-r[2][k] * o[2]) + r[3][k]) + r[1][k] * o[1]) + r[0][k] * o[0]);
                let mut a = matrix;
                a.m[3][0] = p[0];
                a.m[3][1] = p[1];
                a.m[3][2] = p[2];
                let c = xm_matrix_multiply(&a, &body);
                let pos = [c.m[3][0], c.m[3][1], c.m[3][2]];
                let bill = create_billboard(&pos, &[cam[3][0], cam[3][1], cam[3][2]], &[cam[1][0], cam[1][1], cam[1][2]], &[-cam[2][0], -cam[2][1], -cam[2][2]]);
                let cycle = self.cycle_index;
                let switch = self.groups[gi].frame_time_switch == 0;
                let add = match tex.flametype {
                    0 => {
                        self.cycle_texture(0, cycle as u16);
                        let t = self.flash_textures[self.texture_flash_index as usize].clone();
                        graphics.set_texture(0, &t);
                        0
                    }
                    1 => {
                        if switch {
                            self.cycle_texture(1, cycle as u16);
                        }
                        let t = self.textures[self.texture_body_index as usize].clone();
                        graphics.set_texture(0, &t);
                        cycle.wrapping_mul(10)
                    }
                    2 => {
                        if switch {
                            self.cycle_texture(2, cycle as u16);
                        }
                        let t = self.tail_textures[self.texture_tail_index as usize].clone();
                        graphics.set_texture(0, &t);
                        cycle.wrapping_mul(20)
                    }
                    _ => 0,
                };
                self.rotation = self.rotation.wrapping_add(add) % 360;
                let angle = ((if is_left { 1 } else { -1 }) * self.rotation) as f32 * f32::from_bits(0x3c8e_f998);
                let cs = cosf(angle);
                let s = sinf(angle);
                let t = 1.0 - cs;
                let z0 = 0.0f32;
                let u = t * z0;
                let v = s * z0;
                let rot = Mat44f { m: [[cs + u, s + u, u - v, 0.0], [u - s, cs + u, v + u, 0.0], [v + u, u - v, t + cs, 0.0], [0.0, 0.0, 0.0, 1.0]] };
                let world = xm_matrix_multiply(&rot, &bill);
                graphics.set_world_matrix(&world);
                let gl = &mut self.gl;
                gl.color4f(tex.color[0], tex.color[1], tex.color[2], tex.color[3]);
                graphics.set_blend_mode(BLEND_ALPHA);
                gl.begin(GL_QUADS, None);
                let size = tex.size;
                for ((tu, tv), (x, y)) in [((0.0, 0.0), (-size, -size)), ((0.0, 1.0), (size, -size)), ((1.0, 1.0), (size, size)), ((1.0, 0.0), (-size, size))] {
                    gl.tex_coord2f(tu, tv);
                    gl.vertex3f(x, y, 0.0);
                }
                gl.end(graphics);
            }
        }
        let current_dt = self.current_dt;
        let g = &mut self.groups[gi];
        if !pending {
            match g.state {
                1 => {
                    g.state = 2;
                    g.vis_time = 0.0;
                }
                2 => g.vis_time = 0.0,
                3 => {
                    if g.flash_times > 0.0 {
                        g.vis_time = -5.0;
                        g.fuel_in_exhaust = 0.0;
                        g.flash_times -= current_dt;
                    } else {
                        g.state = 0;
                        g.vis_time = 0.0;
                        g.fuel_in_exhaust = 0.0;
                    }
                }
                _ => {}
            }
        }
    }
}

impl NodeEventHandler for Flames {
    /// `Flames::onNodeRenderEvent` 0x140103aa0: in the transparent pass, while the body shows.
    fn on_node_render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        if event.pass_id != 1 || !scene.nodes[self.body_transform].is_active {
            return;
        }
        graphics.set_cull_mode(2);
        graphics.set_blend_mode(BLEND_ALPHA);
        if self.version != 2 {
            return;
        }
        self.cycle_index = 0;
        for gi in 0..self.groups.len() {
            self.groups[gi].frame_time_switch = (self.groups[gi].frame_time_switch + 1) % 10;
            self.cycle_index += 1;
            // Flames::drawBackfireState 0x140101020
            match self.groups[gi].state {
                1 => self.draw_flame(scene, graphics, 1, gi),
                2 => {
                    self.draw_flame(scene, graphics, 1, gi);
                    self.draw_flame(scene, graphics, 2, gi);
                }
                3 => self.draw_flame(scene, graphics, 3, gi),
                _ => {}
            }
            self.groups[gi].vis_time += self.current_dt;
        }
    }
}
