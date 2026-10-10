// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What the car's materials show of its state:
//!
//! * `CarBrakeLights` (0xf0 bytes; `CarBrakeLights::CarBrakeLights` 0x1400ddbb0, `loadLights`
//!   0x1400de0b0, `getVars` 0x1400de020, `update` 0x1400e0450): the `ksEmissive` of the meshes
//!   of `data/lights.ini`: brake lights by the pedal, the other lights by the headlight switch
//!   (there is no reverse light in the game), the flash button, the pit-lane and KERS pulses.
//! * `AnimatedLights` (0x88 bytes; constructor 0x14005a850, `update` 0x14005aef0): lights
//!   that come out of the body, by `animations/lights.ksanim`, one second from shut to open.
//! * `BrakeDiscGraphics` (0x148 bytes; constructor 0x14005ca40, `update` 0x14005d930): the
//!   glow and the blur of the `ksBrakeDisc` meshes. The glow follows pedal x wheel speed, not
//!   the physics' disc temperature.
//! * `DynamicCarEffects` (0x88 bytes; constructor 0x140091230, `resetDirt` 0x140091970,
//!   `update` 0x140091ac0): the dirt on the paint, which grows while a tyre runs over a
//!   surface that makes dirt.

use std::path::Path;

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::ks_rand;

use crate::animator::{Animation, AnimationPlayer};
use crate::graphics::Graphics;
use crate::material::MaterialId;
use crate::scene::{NodeId, NodeKind, Scene};
use crate::state::CarPhysicsState;

fn mesh_material(scene: &Scene, n: NodeId) -> Option<MaterialId> {
    // `dynamic_cast<Mesh*>`: a skinned mesh is a mesh too
    match &scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material,
        NodeKind::SkinnedMesh(mesh) => mesh.material,
        _ => None,
    }
}

fn is_mesh(scene: &Scene, n: NodeId) -> bool {
    matches!(scene.nodes[n].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_))
}

/// Gives a mesh a copy of its material (`new Material(*mesh->material)`).
fn clone_mesh_material(graphics: &mut Graphics, scene: &mut Scene, n: NodeId) -> Result<Option<MaterialId>, String> {
    let Some(old) = mesh_material(scene, n) else {
        return Ok(None);
    };
    let clone = scene.materials[old.0 as usize].clone_material(graphics)?;
    let id = MaterialId(scene.materials.len() as u32);
    scene.materials.push(clone);
    match &mut scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material = Some(id),
        NodeKind::SkinnedMesh(mesh) => mesh.material = Some(id),
        _ => {}
    }
    Ok(Some(id))
}

/// `CarLightsVars` (0x30 bytes): one per mesh.
pub struct CarLightsVars {
    pub mesh: NodeId,
    /// the mesh's material and its `ksEmissive`, looked up at the first update
    matvar: Option<(MaterialId, Option<usize>)>,
    pub colors: [f32; 3],
    pub colors_off: [f32; 3],
    pub brake: bool,
    pub normal: bool,
    pub pitline: bool,
    pub kers: bool,
    pub flash: bool,
}

#[derive(Default)]
pub struct CarBrakeLights {
    pub vars: Vec<CarLightsVars>,
    pub front_lights_on: bool,
    pub front_flash_lights_on: bool,
    pub locked_lights: bool,
    pub flashing_time: f32,
    pub flashing_blink: f32,
    pub flashing_count: f32,
    pub flashing_repeat: i32,
    pub has_pit_lights: bool,
    pub pit_lights_on: bool,
    pub pit_light_time: f32,
    pub pit_light_blink: f32,
    pub has_kers_lights: bool,
    pub kers_lights_on: bool,
    pub kers_light_time: f32,
    pub kers_light_blink: f32,
}

/// `GraphicsManager::getLDRScale` 0x140202bb0.
fn get_ldr_scale(graphics: &Graphics, v: &[f32; 3]) -> f32 {
    if graphics.video.pp_hdr_enabled {
        return 1.0;
    }
    let m = if v[1] > v[2] { v[1] } else { v[2] };
    let m = if v[0] > m { v[0] } else { m };
    if m >= 1.0 {
        m
    } else {
        1.0
    }
}

impl CarBrakeLights {
    /// `CarBrakeLights::getVars` 0x1400de020.
    fn get_vars(&mut self, mesh: NodeId) -> usize {
        match self.vars.iter().position(|v| v.mesh == mesh) {
            Some(i) => i,
            None => {
                self.vars.push(CarLightsVars { mesh, matvar: None, colors: [0.0; 3], colors_off: [0.0; 3], brake: false, normal: false, pitline: false, kers: false, flash: false });
                self.vars.len() - 1
            }
        }
    }

    /// The colours of an entry are brought down so that the larger is 1 while the HDR
    /// post-processing is off.
    fn ldr_scaling(&mut self, graphics: &Graphics, i: usize) {
        let v = &mut self.vars[i];
        let (a, b) = (get_ldr_scale(graphics, &v.colors_off), get_ldr_scale(graphics, &v.colors));
        let f = if a <= b { b } else { a };
        if f > 1.0 {
            let inv = 1.0 / f;
            for k in 0..3 {
                v.colors[k] = inv * v.colors[k];
            }
            for k in 0..3 {
                v.colors_off[k] = inv * v.colors_off[k];
            }
        }
    }

    /// `CarBrakeLights::CarBrakeLights` 0x1400ddbb0 with `loadLights` 0x1400de0b0.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &Path, body_transform: NodeId) -> Result<CarBrakeLights, String> {
        let mut lights = CarBrakeLights::default();
        if let Ok(ini) = IniReader::load(&folder.join("data/lights.ini")) {
            let mut version = 1;
            if ini.has_section("HEADER") {
                version = ini.get_int("HEADER", "VERSION").unwrap_or(0);
                if ini.has_key("HEADER", "FLASHING_BLINK_TIME") {
                    lights.flashing_blink = ini.get_float("HEADER", "FLASHING_BLINK_TIME").unwrap_or(0.0);
                    lights.flashing_repeat = ini.get_float("HEADER", "FLASHING_REPEAT").unwrap_or(0.0) as i32;
                    lights.pit_light_blink = lights.flashing_blink;
                    lights.kers_light_blink = lights.flashing_blink;
                }
                if ini.has_key("HEADER", "PITLINE_BLINK_TIME") {
                    lights.pit_light_blink = ini.get_float("HEADER", "PITLINE_BLINK_TIME").unwrap_or(0.0);
                }
                if ini.has_key("HEADER", "KERS_BLINK_TIME") {
                    lights.kers_light_blink = ini.get_float("HEADER", "KERS_BLINK_TIME").unwrap_or(0.0);
                }
                if ini.has_key("HEADER", "NO_LIGHT_SWITCH") {
                    lights.locked_lights = ini.get_int("HEADER", "NO_LIGHT_SWITCH").unwrap_or(0) != 0;
                }
            }
            let mut n = 0;
            loop {
                let section = format!("BRAKE_{n}");
                if !ini.has_key(&section, "NAME") {
                    break;
                }
                let name = ini.get_string(&section, "NAME");
                let mut found = Vec::new();
                scene.find_children_by_name(body_transform, &name, &mut found);
                for node in found {
                    if !is_mesh(scene, node) {
                        continue;
                    }
                    println!("ADDING BRAKE LIGHT: {}", scene.nodes[node].name);
                    let Some(material) = clone_mesh_material(graphics, scene, node)? else {
                        continue;
                    };
                    if scene.materials[material.0 as usize].get_var_reporting("ksEmissive").is_none() {
                        println!("ERROR: BRAKE LIGHT {name} CAN'T FIND ksEmissive var");
                        continue;
                    }
                    let i = lights.get_vars(node);
                    lights.vars[i].colors = ini.get_float3(&section, "COLOR").unwrap_or([0.0; 3]);
                    lights.vars[i].colors_off = if version >= 2 { ini.get_float3(&section, "OFF_COLOR").unwrap_or([0.0; 3]) } else { [0.0; 3] };
                    lights.vars[i].brake = true;
                    lights.ldr_scaling(graphics, i);
                }
                n += 1;
            }
            let mut n = 0;
            loop {
                let section = format!("LIGHT_{n}");
                if !ini.has_key(&section, "NAME") {
                    break;
                }
                let name = ini.get_string(&section, "NAME");
                let mut found = Vec::new();
                scene.find_children_by_name(body_transform, &name, &mut found);
                for node in found {
                    if !is_mesh(scene, node) {
                        continue;
                    }
                    println!("ADDING LIGHT OBJECT {}", scene.nodes[node].name);
                    let Some(material) = clone_mesh_material(graphics, scene, node)? else {
                        continue;
                    };
                    let Some(emissive) = scene.materials[material.0 as usize].get_var_reporting("ksEmissive") else {
                        println!("ERROR: LIGHT LIGHT {name} CAN'T FIND ksEmissive var");
                        continue;
                    };
                    let emissive = scene.materials[material.0 as usize].vars[emissive].f_value3;
                    let flag = |key: &str| ini.has_key(&section, key) && ini.get_int(&section, key).unwrap_or(0) != 0;
                    let blocks = [flag("FLASH"), flag("PITLINE"), flag("KERS"), !ini.has_key(&section, "SPECIAL")];
                    for (block, on) in blocks.iter().enumerate() {
                        if !on {
                            continue;
                        }
                        let i = lights.get_vars(node);
                        lights.vars[i].colors = ini.get_float3(&section, "COLOR").unwrap_or([0.0; 3]);
                        lights.vars[i].colors_off = if ini.has_key(&section, "OFF_COLOR") && version >= 3 { ini.get_float3(&section, "OFF_COLOR").unwrap_or([0.0; 3]) } else { emissive };
                        match block {
                            0 => lights.vars[i].flash = true,
                            1 => {
                                lights.vars[i].pitline = true;
                                lights.has_pit_lights = true;
                            }
                            2 => {
                                lights.vars[i].kers = true;
                                lights.has_kers_lights = true;
                            }
                            _ => lights.vars[i].normal = true,
                        }
                        lights.ldr_scaling(graphics, i);
                    }
                }
                n += 1;
            }
        }
        // the first pit-lane pulse comes after two to three seconds
        lights.pit_light_time = ks_rand(graphics.crt_rand.next(), 2.0, 3.0);
        Ok(lights)
    }

    /// `CarBrakeLights::update` 0x1400e0450. `dt` is the frame's time times the replay's
    /// multiplier; `in_pit` is `CarAvatar::inPitlane`.
    pub fn update(&mut self, scene: &mut Scene, state: &CarPhysicsState, dt: f32, in_pit: bool) {
        if self.vars.is_empty() {
            return;
        }
        if self.vars[0].matvar.is_none() {
            for v in &mut self.vars {
                if let Some(material) = mesh_material(scene, v.mesh) {
                    v.matvar = Some((material, scene.materials[material.0 as usize].get_var_reporting("ksEmissive")));
                }
            }
        }
        self.front_lights_on = if self.locked_lights { false } else { state.status_bytes & 1 != 0 };
        // the flash button
        if (state.actions_state >> 10) & 1 != 0 && self.flashing_time == 0.0 && self.flashing_count == 0.0 && self.flashing_repeat != 0 && self.flashing_blink != 0.0 {
            self.front_flash_lights_on = !self.front_lights_on;
            self.flashing_time = self.flashing_blink;
            self.flashing_count = self.flashing_repeat as f32;
        }
        if self.flashing_time != 0.0 {
            let t = self.flashing_time - dt;
            self.flashing_time = if t > 0.0 { t } else { 0.0 };
            if self.flashing_time == 0.0 {
                let mut over = false;
                if self.front_flash_lights_on == self.front_lights_on {
                    self.flashing_count -= 1.0;
                    over = self.flashing_count == 0.0;
                }
                if !over {
                    self.front_flash_lights_on = !self.front_flash_lights_on;
                    self.flashing_time = self.flashing_blink;
                }
            }
        }
        if self.has_pit_lights {
            if in_pit && self.pit_light_time == 0.0 {
                self.pit_lights_on = true;
                self.pit_light_time = self.pit_light_blink;
            }
            if self.pit_light_time != 0.0 {
                let t = self.pit_light_time - dt;
                self.pit_light_time = if t > 0.0 { t } else { 0.0 };
                if self.pit_light_time == 0.0 && self.pit_lights_on {
                    self.pit_lights_on = false;
                    self.pit_light_time = self.pit_light_blink;
                }
            }
        }
        if self.has_kers_lights {
            if !in_pit && state.kers_is_charging && self.kers_light_time == 0.0 {
                self.kers_lights_on = true;
                self.kers_light_time = self.kers_light_blink;
            }
            if self.kers_light_time != 0.0 {
                let t = self.kers_light_time - dt;
                self.kers_light_time = if t > 0.0 { t } else { 0.0 };
                if self.kers_light_time == 0.0 && self.kers_lights_on {
                    self.kers_lights_on = false;
                    self.kers_light_time = self.kers_light_blink;
                }
            }
        }
        let brake = state.brake;
        let (front, flash_on, flashing, pit_on, kers_on) = (self.front_lights_on, self.front_flash_lights_on, self.flashing_count != 0.0, self.pit_lights_on, self.kers_lights_on);
        for v in &self.vars {
            let mut write = |c: [f32; 3]| {
                if let Some((material, Some(var))) = v.matvar {
                    scene.materials[material.0 as usize].update_var(var, |m| m.f_value3 = c);
                }
            };
            let pick = |on: bool| if on { v.colors } else { v.colors_off };
            if v.brake {
                write(if f32::from_bits(0x3c23_d70a) < brake {
                    v.colors
                } else if front {
                    v.colors_off
                } else {
                    [0.0; 3]
                });
            }
            if in_pit {
                if v.kers {
                    write(v.colors_off);
                }
                if v.pitline {
                    write(pick(pit_on));
                    continue;
                }
            } else {
                if v.pitline {
                    write(v.colors_off);
                }
                if v.kers {
                    write(pick(kers_on));
                    continue;
                }
            }
            if v.flash && flashing {
                write(pick(flash_on));
                continue;
            }
            if v.normal {
                write(pick(front));
            }
        }
    }
}

const DISC_NAME_KEYS: [&str; 4] = ["DISC_LF", "DISC_RF", "DISC_LR", "DISC_RR"];

pub struct BrakeDiscGraphics {
    glow_vars: [Vec<(MaterialId, Option<usize>)>; 4],
    blur_vars: [Vec<(MaterialId, Option<usize>)>; 4],
    pub current_glow_value: [f32; 4],
    pub max_glow: [f32; 4],
    pub lag_hot: f32,
    pub lag_cool: f32,
}

impl Default for BrakeDiscGraphics {
    fn default() -> BrakeDiscGraphics {
        BrakeDiscGraphics { glow_vars: Default::default(), blur_vars: Default::default(), current_glow_value: [0.0; 4], max_glow: [0.0; 4], lag_hot: f32::from_bits(0x3f66_6666), lag_cool: f32::from_bits(0x3f7d_70a4) }
    }
}

impl BrakeDiscGraphics {
    /// `BrakeDiscGraphics::BrakeDiscGraphics` 0x14005ca40.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &Path, car_node: NodeId) -> Result<BrakeDiscGraphics, String> {
        let mut discs = BrakeDiscGraphics::default();
        let Ok(ini) = IniReader::load(&folder.join("data/brakes.ini")) else {
            return Ok(discs);
        };
        if ini.has_section("DISCS_GRAPHICS") {
            let k = f32::from_bits(0x426f_ffff);
            discs.lag_hot = (1.0 - ini.get_float("DISCS_GRAPHICS", "LAG_HOT").unwrap_or(0.0)) * k;
            discs.lag_cool = (1.0 - ini.get_float("DISCS_GRAPHICS", "LAG_COOL").unwrap_or(0.0)) * k;
        }
        for i in 0..4 {
            discs.current_glow_value[i] = 0.0;
            let node_name = ini.get_string("DISCS_GRAPHICS", DISC_NAME_KEYS[i]);
            let mut found = Vec::new();
            scene.find_children_by_name(car_node, &node_name, &mut found);
            for node in found {
                let Some(material) = mesh_material(scene, node) else {
                    continue;
                };
                let shader = scene.materials[material.0 as usize].shader.map(|s| graphics.shaders.get(s).name.clone()).unwrap_or_default();
                if shader == "ksBrakeDisc" {
                    println!("BRAKE DISC: {} FOUND FOR: {}", scene.nodes[node].name, DISC_NAME_KEYS[i]);
                    if let Some(id) = clone_mesh_material(graphics, scene, node)? {
                        let m = &scene.materials[id.0 as usize];
                        discs.glow_vars[i].push((id, m.get_var_reporting("glowLevel")));
                        discs.blur_vars[i].push((id, m.get_var_reporting("blurLevel")));
                    }
                } else {
                    println!("WARNING: BRAKE DISC MESH : {} DOES NOT HAVE CORRECT ksBrakeDisc SHADER, HAS :{shader}", scene.nodes[node].name);
                }
            }
            println!("FOUND {} BRAKE DISCS MESHES FOR TYRE:{i}", discs.glow_vars[i].len());
            discs.max_glow[i] = ini.get_float("DISCS_GRAPHICS", if i < 2 { "FRONT_MAX_GLOW" } else { "REAR_MAX_GLOW" }).unwrap_or(0.0);
        }
        Ok(discs)
    }

    /// `BrakeDiscGraphics::update` 0x14005d930. `replay` is the replay's time multiplier while
    /// one plays.
    pub fn update(&mut self, scene: &mut Scene, state: &CarPhysicsState, dt: f32, replay: Option<f32>) {
        let (gate, scale) = match replay {
            Some(mult) => (mult, if mult == 0.0 || mult.is_nan() { 1.0 } else { mult }),
            None => (1.0, 1.0),
        };
        let clamp = |v: f32| {
            if v > 1.0 {
                1.0
            } else if v >= 0.0 {
                v
            } else {
                0.0
            }
        };
        for i in 0..4 {
            let w = state.wheel_angular_speed[i].abs();
            let gf = clamp((w - 10.0) * f32::from_bits(0x3bda_740e));
            let target = (self.max_glow[i] * state.brake) * gf;
            let blur = clamp((w * scale) * f32::from_bits(0x3dcc_cccd));
            if gate != 0.0 && !gate.is_nan() {
                let cur = self.current_glow_value[i];
                let k = clamp(dt * if target > cur { self.lag_hot } else { self.lag_cool });
                self.current_glow_value[i] = (target - cur) * k + cur;
            }
            let glow = self.current_glow_value[i];
            for (material, var) in &self.glow_vars[i] {
                if let Some(var) = var {
                    scene.materials[material.0 as usize].update_var(*var, |m| m.f_value = glow);
                }
            }
            for (material, var) in &self.blur_vars[i] {
                if let Some(var) = var {
                    scene.materials[material.0 as usize].update_var(*var, |m| m.f_value = blur);
                }
            }
        }
    }
}

pub struct DynamicCarEffects {
    dirt_vars: Vec<(MaterialId, Option<usize>)>,
    pub current_dirt_level: f32,
    pub dirt_multiplier: f32,
    /// the level when the replay began, put back when it ends
    pub replay_old_dirt_level: f32,
}

impl Default for DynamicCarEffects {
    fn default() -> DynamicCarEffects {
        DynamicCarEffects { dirt_vars: Vec::new(), current_dirt_level: 0.0, dirt_multiplier: 1.0, replay_old_dirt_level: 0.0 }
    }
}

impl DynamicCarEffects {
    /// `DynamicCarEffects::DynamicCarEffects` 0x140091230: the `dirt` of every mesh below the
    /// body whose shader is the paint with damage and dirt (the materials stay shared).
    pub fn new(graphics: &Graphics, scene: &mut Scene, body_transform: NodeId) -> DynamicCarEffects {
        let mut effects = DynamicCarEffects::default();
        fn visit(scene: &Scene, graphics: &Graphics, n: NodeId, out: &mut Vec<(MaterialId, Option<usize>)>) {
            if let Some(material) = mesh_material(scene, n) {
                let m = &scene.materials[material.0 as usize];
                if m.shader.is_some_and(|s| graphics.shaders.get(s).name == "ksPerPixelMultiMap_damage_dirt") {
                    out.push((material, m.get_var_reporting("dirt")));
                }
            }
            for &child in &scene.nodes[n].children {
                visit(scene, graphics, child, out);
            }
        }
        visit(scene, graphics, body_transform, &mut effects.dirt_vars);
        effects.reset_dirt(scene);
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/assetto_corsa.ini")) {
            effects.dirt_multiplier = ini.get_float("DIRT", "MULT").unwrap_or(0.0);
        }
        effects
    }

    fn push(&self, scene: &mut Scene, value: f32) {
        for (material, var) in &self.dirt_vars {
            if let Some(var) = var {
                scene.materials[material.0 as usize].update_var(*var, |m| m.f_value = value);
            }
        }
    }

    /// `DynamicCarEffects::resetDirt` 0x140091970 (also at every new session).
    pub fn reset_dirt(&mut self, scene: &mut Scene) {
        self.current_dirt_level = 0.0;
        self.push(scene, 0.0);
    }

    /// `DynamicCarEffects::updateReplayMode` 0x140091e40: the level as it is, every frame.
    pub fn update_replay_mode(&mut self, scene: &mut Scene) {
        self.push(scene, self.current_dirt_level);
    }

    /// The handler of `Sim::evOnReplayStatusChanged` 0x1400916c0.
    pub fn on_replay_status_changed(&mut self, scene: &mut Scene, status: i32) {
        if status == 6 {
            self.replay_old_dirt_level = self.current_dirt_level;
        } else if status == 7 {
            self.push(scene, self.replay_old_dirt_level);
            self.current_dirt_level = self.replay_old_dirt_level;
        }
    }

    /// `DynamicCarEffects::update` 0x140091ac0 (outside a replay).
    pub fn update(&mut self, scene: &mut Scene, state: &CarPhysicsState) {
        let old = self.current_dirt_level;
        let mut d = old;
        for i in 0..4 {
            let moving = 1.0 < state.speed || state.wheel_angular_speed[i].abs() > 4.0;
            let dirty = state.tyre_surface_def[i].dirt_additive_k.abs() > f32::from_bits(0x3dcc_cccd);
            let loaded = !(10.0 >= state.load[i]);
            if moving && dirty && loaded {
                let [vx, vy, vz] = state.velocity;
                let s = (vx * vx + vy * vy) + vz * vz;
                let len = if s == 0.0 || s.is_nan() { 0.0 } else { sqrtf(s) };
                d = ((len * self.dirt_multiplier) * f32::from_bits(0x3727_c5ac)) + d;
            }
        }
        d = if d > 1.0 {
            1.0
        } else if d >= 0.0 {
            d
        } else {
            0.0
        };
        self.current_dirt_level = d;
        if old != d && !(old.is_nan() || d.is_nan()) {
            self.push(scene, d);
        }
    }
}

/// `AnimatedLights`: at most one animation.
#[derive(Default)]
pub struct AnimatedLights {
    lights: Vec<AnimationPlayer>,
    pub front_lights_on: bool,
}

impl AnimatedLights {
    /// `AnimatedLights::AnimatedLights` 0x14005a850.
    pub fn new(scene: &Scene, folder: &Path, body_transform: NodeId) -> AnimatedLights {
        let mut lights = AnimatedLights::default();
        let path = folder.join("animations/lights.ksanim");
        if path.is_file() {
            let animation = Animation::load(&path);
            lights.lights.push(AnimationPlayer::new(&animation, scene, body_transform));
        }
        lights
    }

    /// `AnimatedLights::update` 0x14005aef0. `dt` is the frame's own time.
    pub fn update(&mut self, scene: &mut Scene, front_lights_on: bool, dt: f32) {
        self.front_lights_on = front_lights_on;
        for player in &mut self.lights {
            let pos = player.get_current_pos();
            let new_pos = if front_lights_on {
                if pos >= 1.0 {
                    1.0
                } else {
                    pos + dt
                }
            } else if pos > 0.0 {
                pos - dt
            } else {
                0.0
            };
            player.set_current_pos(scene, new_pos, false);
        }
    }
}
