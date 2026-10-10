// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The start lights on the track and the track's semaphore.
//!
//! `StartingLights` (0xf0 bytes): constructor 0x1401a1a20, `renderHUD` 0x1401a2e80. The
//! meshes `KS_START_LIGHT_<i>` of the track get a material of their own
//! (`data/startinglights.ini`), whose `ksEmissive` is lit one after the other in the seconds
//! before a race starts. The game writes it in its HUD pass, after the scene is drawn: a
//! change shows one frame later.
//!
//! `StartingTrackSemaphore` (0x88 bytes): constructor 0x1401a37a0, `update` 0x1401a4cc0,
//! `resetLights` 0x1401a4a50. The lamps of `data/semaphore.ini` (the drag strip and the hill
//! climb); written in the update, before the scene is drawn.

use std::path::Path;

use rustyac_physics::data::ini::IniReader;

use crate::graphics::Graphics;
use crate::material::MaterialId;
use crate::scene::{NodeId, NodeKind, Scene};

/// `TrackStartingLight` (0x30 bytes).
struct TrackStartingLight {
    emissive: [f32; 3],
    /// the mesh's own material and its `ksEmissive`
    material: MaterialId,
    var: Option<usize>,
}

pub struct StartingLights {
    /// `GameObject::isActive`: off while paused and in a replay (the two handlers)
    pub is_active: bool,
    /// the six lights of the HUD (0 grey, 1 red)
    pub lights: [i32; 6],
    pub random_time_offset_start: f32,
    pub random_time_offset_red_wait: f32,
    track_lights: Vec<TrackStartingLight>,
    lights_on: bool,
}

fn is_mesh(scene: &Scene, n: NodeId) -> bool {
    matches!(scene.nodes[n].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_))
}

/// `Node::getNodeChild<Mesh>` 0x1400e5d50, recursive: the first mesh of that name below the
/// node, each child before its own children.
fn get_mesh_child(scene: &Scene, node: NodeId, name: &str) -> Option<NodeId> {
    for &child in &scene.nodes[node].children {
        if scene.nodes[child].name == name && is_mesh(scene, child) {
            return Some(child);
        }
        if let Some(found) = get_mesh_child(scene, child, name) {
            return Some(found);
        }
    }
    None
}

/// `new Material(*mesh->material)`, put into the mesh.
fn clone_mesh_material(graphics: &mut Graphics, scene: &mut Scene, n: NodeId) -> Result<Option<MaterialId>, String> {
    let old = match &scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material,
        NodeKind::SkinnedMesh(mesh) => mesh.material,
        _ => None,
    };
    let Some(old) = old else { return Ok(None) };
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

fn set_emissive(scene: &mut Scene, material: MaterialId, var: Option<usize>, value: [f32; 3]) {
    if let Some(var) = var {
        scene.materials[material.0 as usize].update_var(var, |v| v.f_value3 = value);
    }
}

impl StartingLights {
    /// `StartingLights::StartingLights` 0x1401a1a20. `data_folder`: the folder that holds the
    /// track's (or its layout's) `data`. Two numbers are drawn from `rand()`.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, track_node: NodeId, data_folder: &Path) -> Result<StartingLights, String> {
        // (content/texture/off.png: the HUD's lamp, not part of the 3D scene)
        let k = f32::from_bits(0x3800_0100);
        let random_time_offset_start = (graphics.crt_rand.next() as f32 * k) * 1000.0 - 500.0;
        let random_time_offset_red_wait = (graphics.crt_rand.next() as f32 * k) * 1000.0 - 500.0;
        let mut lights = StartingLights { is_active: true, lights: [0; 6], random_time_offset_start, random_time_offset_red_wait, track_lights: Vec::new(), lights_on: false };
        let file = data_folder.join("data/startinglights.ini");
        if !rustyac_physics::data::exists(&file) {
            return Ok(lights);
        }
        let Ok(ini) = IniReader::load(&file) else { return Ok(lights) };
        if !ini.has_section("SETTINGS") {
            return Ok(lights);
        }
        let intensity = ini.get_float("SETTINGS", "INTENSITY").unwrap_or(0.0);
        let e = ini.get_float3("SETTINGS", "EMISSIVE").unwrap_or([0.0; 3]);
        let emissive = [intensity * e[0], intensity * e[1], intensity * e[2]];
        let mut index = 0;
        while let Some(mesh) = get_mesh_child(scene, track_node, &format!("KS_START_LIGHT_{index}")) {
            index += 1;
            let Some(material) = clone_mesh_material(graphics, scene, mesh)? else { continue };
            let diffuse = ini.get_float("SETTINGS", "DIFFUSE").unwrap_or(0.0);
            let m = &mut scene.materials[material.0 as usize];
            for (name, value) in [("ksDiffuse", diffuse), ("ksAmbient", diffuse), ("ksSpecular", 0.0)] {
                if let Some(var) = m.get_var_reporting(name) {
                    m.update_var(var, |v| v.f_value = value);
                }
            }
            let var = m.get_var_reporting("ksEmissive");
            lights.track_lights.push(TrackStartingLight { emissive, material, var });
        }
        Ok(lights)
    }

    /// How many meshes of the track are start lights.
    pub fn track_light_count(&self) -> usize {
        self.track_lights.len()
    }

    /// The handler of `Sim::evOnReplayStatusChanged` 0x1401a2960 (offline).
    pub fn on_replay_status_changed(&mut self, status: i32) {
        if status == 6 {
            self.is_active = false;
        }
        if status == 7 {
            self.is_active = true;
        }
    }

    /// The handler of `Sim::evOnPauseModeChanged` 0x1401a2920.
    pub fn on_pause_mode_changed(&mut self, paused: bool) {
        self.is_active = !paused;
    }

    /// `StartingLights::renderHUD` 0x1401a2e80 without its 2D drawing: to be called after the
    /// scene is drawn, when `is_active` (`Game::renderHUD` leaves an inactive object out).
    /// `pause_menu`: the pause menu shows. `started`: `RaceManager::hasSessionStarted`;
    /// `race`: the session is a race; `tts`: `RaceManager::getTimeToSessionStart`, ms.
    pub fn render_hud(&mut self, scene: &mut Scene, pause_menu: bool, started: bool, race: bool, tts: f32) {
        if pause_menu {
            return;
        }
        let active = !started && race;
        if !active {
            if self.lights_on {
                self.lights_on = false;
                for i in 0..self.track_lights.len() {
                    let (material, var) = (self.track_lights[i].material, self.track_lights[i].var);
                    set_emissive(scene, material, var, [0.0; 3]);
                }
            }
            return;
        }
        let t0 = self.random_time_offset_start;
        if tts >= t0 + 15000.0 {
            return;
        }
        let t1 = (t0 + 3000.0) + self.random_time_offset_red_wait;
        let t2 = t0 + 4000.0;
        // the HUD's six
        let n = self.lights.len() as f32;
        let per = t2 / n;
        let mut k = n;
        for light in &mut self.lights {
            if (tts - t1) > per * k {
                *light = 0;
            } else {
                self.lights_on = true;
                *light = 1;
            }
            k += -1.0;
        }
        // the track's
        let m = self.track_lights.len() as f32;
        if !(m > 0.0) {
            return;
        }
        let per = t2 / m;
        let d = tts - t1;
        let mut k = m;
        for i in 0..self.track_lights.len() {
            let (material, var, emissive) = (self.track_lights[i].material, self.track_lights[i].var, self.track_lights[i].emissive);
            let value = if d > per * k { [0.0; 3] } else { emissive };
            set_emissive(scene, material, var, value);
            k += -1.0;
        }
    }
}

/// `LightGroup`: the `ksEmissive` of every lamp of one step.
#[derive(Default)]
struct LightGroup {
    vars: Vec<(MaterialId, Option<usize>)>,
}

pub struct StartingTrackSemaphore {
    /// `semaphore`: the groups of type 0 (disqualified), 1 (ready), 2 (start); empty without
    /// a `semaphore.ini`
    semaphore: Vec<Vec<LightGroup>>,
    pub intensity: f32,
    pub disqualified_on: bool,
}

impl StartingTrackSemaphore {
    /// `StartingTrackSemaphore::StartingTrackSemaphore` 0x1401a37a0.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, track_node: NodeId, data_folder: &Path) -> Result<StartingTrackSemaphore, String> {
        let mut s = StartingTrackSemaphore { semaphore: Vec::new(), intensity: 1.0, disqualified_on: false };
        let file = data_folder.join("data/semaphore.ini");
        if !rustyac_physics::data::exists(&file) {
            return Ok(s);
        }
        let Ok(ini) = IniReader::load(&file) else { return Ok(s) };
        s.intensity = ini.get_float("COLOR", "INTENSITY").unwrap_or(0.0);
        for key in ["DISQUALIFIED", "READY", "START"] {
            let count = ini.get_int("GROUPS", key).unwrap_or(0).max(0) as usize;
            s.semaphore.push((0..count).map(|_| LightGroup::default()).collect());
        }
        let mut n = 0;
        loop {
            let section = format!("OBJECT_{n}");
            if !ini.has_section(&section) {
                break;
            }
            n += 1;
            let name = ini.get_string(&section, "NAME");
            let mut nodes = Vec::new();
            scene.find_children_by_name(track_node, &name, &mut nodes);
            for node in nodes {
                scene.nodes[node].is_active = true;
                if !is_mesh(scene, node) {
                    continue;
                }
                if let Some(r) = &mut scene.nodes[node].renderable {
                    r.is_visible = true;
                }
                let kind = ini.get_int(&section, "TYPE").unwrap_or(0);
                let order = ini.get_int(&section, "ORDER").unwrap_or(0);
                let Some(material) = clone_mesh_material(graphics, scene, node)? else { continue };
                let var = scene.materials[material.0 as usize].get_var_reporting("ksEmissive");
                // (the game trusts the file's numbers; a group that does not exist is left out here)
                match s.semaphore.get_mut(kind as usize).and_then(|groups| groups.get_mut(order as usize)) {
                    Some(group) => group.vars.push((material, var)),
                    None => println!("WARNING: semaphore.ini [{section}]: no group TYPE={kind} ORDER={order}"),
                }
            }
        }
        Ok(s)
    }

    /// Whether the track has a semaphore at all.
    pub fn is_empty(&self) -> bool {
        self.semaphore.is_empty()
    }

    fn light(scene: &mut Scene, group: &LightGroup, value: f32) {
        for &(material, var) in &group.vars {
            set_emissive(scene, material, var, [value; 3]);
        }
    }

    /// `StartingTrackSemaphore::update` 0x1401a4cc0. `tts`: its own `getTimeToSessionStart`
    /// 0x1401a48c0, ms (negative once the session runs).
    pub fn update(&mut self, scene: &mut Scene, tts: f32) {
        if self.semaphore.is_empty() {
            return;
        }
        // resetLights 0x1401a4a50
        for groups in &self.semaphore {
            for group in groups {
                Self::light(scene, group, 0.0);
            }
        }
        let intensity = self.intensity;
        if self.disqualified_on {
            for group in &self.semaphore[0] {
                Self::light(scene, group, intensity);
            }
            return;
        }
        if !(tts > 0.0) {
            for group in &self.semaphore[2] {
                Self::light(scene, group, intensity);
            }
        } else {
            let groups = &self.semaphore[1];
            let m = groups.len() as f32;
            let per = 4000.0 / m;
            let mut k = m;
            for group in groups {
                if !((tts - 3000.0) >= per * k) {
                    Self::light(scene, group, intensity);
                }
                k += -1.0;
            }
        }
    }
}
