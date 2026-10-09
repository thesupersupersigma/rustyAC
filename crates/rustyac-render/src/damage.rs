// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `VisualDamageManager` (0xd0 bytes): what a hit does to the picture of the car.
//! `VisualDamageManager::VisualDamageManager` 0x1401d2890, `initScratchableParts` 0x1401d5320
//! (its visitor 0x1401d4240), `collectAllMeshes` 0x1401d4fb0, `damageZoneFromString`
//! 0x1401d50c0, `getTotalDamageLevel` 0x1401d51d0, `update` 0x1401d56f0.
//!
//! Three things follow `CarPhysicsState::damageZoneLevel` (km/h of closing speed per zone:
//! front, rear, left, right, centre): the scratches of the paint (`damageZones` of every
//! `ksPerPixelMultiMap_damage_dirt` material, each mesh with a material of its own), the
//! cracked glass (`glassDamage` of one `ksBrokenGlass` material per zone: 0 until the hit is
//! hard enough, so the glass meshes are drawn every frame and show nothing), and the body
//! parts of `data/damage.ini` that hang askew and shake.

use std::path::Path;

use rustyac_math::{sinf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::graphics::Graphics;
use crate::material::MaterialId;
use crate::scene::{NodeId, NodeKind, Scene};
use crate::state::CarPhysicsState;

/// The game's degrees to radians.
const DEG: f32 = f32::from_bits(0x3c8e_f998);

/// `DamageRange`: closing speeds, km/h.
#[derive(Clone, Copy, Debug)]
pub struct DamageRange {
    pub min_speed: f32,
    pub max_speed: f32,
}

/// `BreakablePart` (0x98 bytes).
pub struct BreakablePart {
    pub node: NodeId,
    pub damage_level: f32,
    pub org_matrix: Mat44f,
    pub phase: f32,
    pub damage_zone: i32,
    pub range: DamageRange,
    pub static_damage_axis: Vec3f,
    pub static_damage_angle: f32,
    pub oscillation_axis: Vec3f,
    /// radians: min, max
    pub oscillation_angle: [f32; 2],
    pub oscillation_g_mult: f32,
    pub oscillation_old_angle: f32,
    pub allowed_g: [f32; 3],
}

/// `ScratchablePart` (0x18 bytes).
struct ScratchablePart {
    mesh: NodeId,
    var_damage: Option<usize>,
    range: DamageRange,
}

/// `GlassBreakablePart` (0x28 bytes).
struct GlassBreakablePart {
    meshes: Vec<NodeId>,
    var_damage: Option<(MaterialId, usize)>,
    range: DamageRange,
}

pub struct VisualDamageManager {
    pub breakable_parts: Vec<BreakablePart>,
    scrathable_parts: Vec<ScratchablePart>,
    glass_breakable_parts: Vec<GlassBreakablePart>,
    pub oscillation_enabled: bool,
    pub glass_damage_threshold: f32,
    /// the game's process-wide `damageEnabled`: a car's `damage.ini` was read
    pub damage_enabled: bool,
    /// `GameObject::isActive`: off while the game or a replay is paused
    pub is_active: bool,
}

const GLASS_DAMAGE_NAMES: [&str; 5] = ["DAMAGE_GLASS_FRONT", "DAMAGE_GLASS_REAR", "DAMAGE_GLASS_LEFT", "DAMAGE_GLASS_RIGHT", "DAMAGE_GLASS_CENTER"];

fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

fn mesh_material(scene: &Scene, n: NodeId) -> Option<MaterialId> {
    match &scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material,
        NodeKind::SkinnedMesh(mesh) => mesh.material,
        _ => None,
    }
}

fn set_mesh_material(scene: &mut Scene, n: NodeId, id: MaterialId) {
    match &mut scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material = Some(id),
        NodeKind::SkinnedMesh(mesh) => mesh.material = Some(id),
        _ => {}
    }
}

fn is_mesh(scene: &Scene, n: NodeId) -> bool {
    matches!(scene.nodes[n].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_))
}

/// An axis of the file, made a unit vector (a zero axis stays).
fn axis(v: [f32; 3]) -> Vec3f {
    let [mut x, mut y, mut z] = v;
    let l = sqrtf((x * x + y * y) + z * z);
    if l != 0.0 && !l.is_nan() {
        let inv = 1.0 / l;
        x *= inv;
        y *= inv;
        z *= inv;
    }
    Vec3f::new(x, y, z)
}

/// `damageZoneFromString` 0x1401d50c0.
fn damage_zone_from_string(name: &str) -> i32 {
    match name {
        "FRONT" => 0,
        "REAR" => 1,
        "LEFT" => 2,
        "RIGHT" => 3,
        "CENTER" => 4,
        _ => {
            println!("ERROR: DAMAGE ZONE {name} NOT FOUND");
            -1
        }
    }
}

/// `collectAllMeshes` 0x1401d4fb0: a mesh ends its branch.
fn collect_all_meshes(scene: &Scene, n: NodeId, out: &mut Vec<NodeId>) {
    if is_mesh(scene, n) {
        out.push(n);
        return;
    }
    for &child in &scene.nodes[n].children {
        collect_all_meshes(scene, child, out);
    }
}

impl VisualDamageManager {
    /// `VisualDamageManager::VisualDamageManager` 0x1401d2890. `folder` is the car's folder.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &Path, body_transform: NodeId) -> Result<VisualDamageManager, String> {
        let mut manager = VisualDamageManager {
            breakable_parts: Vec::new(),
            scrathable_parts: Vec::new(),
            glass_breakable_parts: (0..5).map(|_| GlassBreakablePart { meshes: Vec::new(), var_damage: None, range: DamageRange { min_speed: 20.0, max_speed: 60.0 } }).collect(),
            oscillation_enabled: true,
            glass_damage_threshold: f32::from_bits(0x3f26_6666),
            damage_enabled: false,
            is_active: true,
        };
        let game = graphics.game_folder.clone();
        if let Ok(ini) = IniReader::load(&game.join("system/cfg/assetto_corsa.ini")) {
            manager.glass_damage_threshold = ini.get_float("DAMAGE", "GLASS_THRESHOLD").unwrap_or(0.0);
        }
        if let Ok(ini) = IniReader::load(&folder.join("data/damage.ini")) {
            manager.damage_enabled = true;
            if ini.has_section("OSCILLATIONS") {
                manager.oscillation_enabled = ini.get_int("OSCILLATIONS", "ENABLED").unwrap_or(0) > 0;
            }
            manager.init_scratchable_parts(graphics, scene, &ini, body_transform)?;
            // ([DAMAGE] INITIAL_LEVEL is handed to a car that has no physics yet: nothing happens)
            let mut n = 0;
            loop {
                let section = format!("VISUAL_OBJECT_{n}");
                if !ini.has_section(&section) {
                    break;
                }
                let name = ini.get_string(&section, "NAME");
                let mut found = Vec::new();
                scene.find_children_by_name(body_transform, &name, &mut found);
                let min = ini.get_float(&section, "MIN_SPEED").unwrap_or(0.0);
                let full = ini.get_float(&section, "FULL_SPEED").unwrap_or(0.0);
                let zone = damage_zone_from_string(&ini.get_string(&section, "DAMAGE_ZONE"));
                let static_axis = axis(ini.get_float3(&section, "STATIC_ROTATION_AXIS").unwrap_or([0.0; 3]));
                let static_angle = ini.get_float(&section, "STATIC_ROTATION_ANGLE").unwrap_or(0.0) * DEG;
                let oscillation_axis = axis(ini.get_float3(&section, "OSCILLATION_AXIS").unwrap_or([0.0; 3]));
                let max_angle = ini.get_float(&section, "OSCILLATION_MAX_ANGLE").unwrap_or(0.0) * DEG;
                let min_angle = ini.get_float(&section, "OSCILLATION_MIN_ANGLE").unwrap_or(0.0) * DEG;
                let g_mult = 1.0 - ini.get_float(&section, "MULT_G").unwrap_or(0.0);
                let allowed_g = ini.get_float3(&section, "ALLOWED_G").unwrap_or([0.0; 3]);
                for node in found {
                    manager.breakable_parts.push(BreakablePart {
                        node,
                        damage_level: 1.0,
                        org_matrix: scene.nodes[node].matrix,
                        phase: 0.0,
                        damage_zone: zone,
                        range: DamageRange { min_speed: min, max_speed: full },
                        static_damage_axis: static_axis,
                        static_damage_angle: static_angle,
                        oscillation_axis,
                        oscillation_angle: [min_angle, max_angle],
                        oscillation_g_mult: g_mult,
                        oscillation_old_angle: 0.0,
                        allowed_g,
                    });
                }
                n += 1;
            }
            for (zone, name) in GLASS_DAMAGE_NAMES.iter().enumerate() {
                if ini.has_section(name) {
                    manager.glass_breakable_parts[zone].range = DamageRange { min_speed: ini.get_float(name, "MIN_SPEED").unwrap_or(0.0), max_speed: ini.get_float(name, "FULL_SPEED").unwrap_or(0.0) };
                }
            }
        }
        // the glass: one material per zone, with the game's own crack texture and no damage
        for (zone, name) in GLASS_DAMAGE_NAMES.iter().enumerate() {
            let mut k = 1;
            let mut meshes = Vec::new();
            while let Some(node) = scene.find_child_by_name(body_transform, &format!("{name}_{k}"), true) {
                collect_all_meshes(scene, node, &mut meshes);
                k += 1;
            }
            if meshes.is_empty() {
                continue;
            }
            let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{}/content/texture/DAMAGE_GLASS.dds", crate::model::path_text(&game)));
            let Some(source) = mesh_material(scene, meshes[0]) else {
                continue;
            };
            let mut material = scene.materials[source.0 as usize].clone_material(graphics)?;
            material.set_texture("txNormal", texture);
            material.double_face = true;
            let var = material.get_var_reporting("glassDamage");
            if let Some(var) = var {
                material.update_var(var, |v| v.f_value = 0.0);
            }
            let id = MaterialId(scene.materials.len() as u32);
            scene.materials.push(material);
            for &mesh in &meshes {
                set_mesh_material(scene, mesh, id);
            }
            manager.glass_breakable_parts[zone].var_damage = var.map(|v| (id, v));
            manager.glass_breakable_parts[zone].meshes = meshes;
        }
        Ok(manager)
    }

    /// `initScratchableParts` 0x1401d5320: every mesh below the body whose shader is the
    /// paint with damage gets a material of its own.
    fn init_scratchable_parts(&mut self, graphics: &mut Graphics, scene: &mut Scene, ini: &IniReader, body_transform: NodeId) -> Result<(), String> {
        let mut order = Vec::new();
        fn visit(scene: &Scene, n: NodeId, out: &mut Vec<NodeId>) {
            out.push(n);
            for &child in &scene.nodes[n].children {
                visit(scene, child, out);
            }
        }
        visit(scene, body_transform, &mut order);
        for n in order {
            let Some(id) = mesh_material(scene, n) else {
                continue;
            };
            let shader = scene.materials[id.0 as usize].shader.map(|s| graphics.shaders.get(s).name.clone()).unwrap_or_default();
            if shader != "ksPerPixelMultiMap_damage_dirt" && shader != "ksPerPixelMultiMap_damage_dirt_sunspot" {
                continue;
            }
            let mut clone = scene.materials[id.0 as usize].clone_material(graphics)?;
            let range = if ini.has_section("SCRATCHES") {
                let max = ini.get_float("SCRATCHES", "MAX_SPEED").unwrap_or(0.0);
                DamageRange { min_speed: ini.get_float("SCRATCHES", "MIN_SPEED").unwrap_or(0.0), max_speed: max }
            } else {
                DamageRange { min_speed: 20.0, max_speed: 0.0 }
            };
            let var = clone.get_var_reporting("damageZones");
            if let Some(var) = var {
                clone.update_var(var, |v| v.f_value4 = [0.0; 4]);
            }
            let new_id = MaterialId(scene.materials.len() as u32);
            scene.materials.push(clone);
            set_mesh_material(scene, n, new_id);
            if var.is_some() {
                self.scrathable_parts.push(ScratchablePart { mesh: n, var_damage: None, range });
            }
        }
        Ok(())
    }

    /// `getTotalDamageLevel` 0x1401d51d0: left, rear, right, front.
    fn get_total_damage_level(zones: &[f32; 5], range: &DamageRange) -> [f32; 4] {
        let f = |i: usize| clamp01((zones[i] - range.min_speed) / (range.max_speed - range.min_speed));
        [f(2), f(1), f(3), f(0)]
    }

    /// `VisualDamageManager::update` 0x1401d56f0. `dt` is the frame's time (times the replay's
    /// multiplier while a replay plays); `pause_menu` is whether the pause menu shows.
    pub fn update(&mut self, graphics: &mut Graphics, scene: &mut Scene, state: &CarPhysicsState, dt: f32, pause_menu: bool) {
        if !self.is_active || pause_menu || !self.damage_enabled {
            return;
        }
        graphics.clear_texture_slot(21);
        let zones = &state.damage_zone_level;
        for p in &mut self.scrathable_parts {
            let Some(material) = mesh_material(scene, p.mesh) else {
                continue;
            };
            let material = &mut scene.materials[material.0 as usize];
            if p.var_damage.is_none() {
                p.var_damage = material.get_var_reporting("damageZones");
            }
            if let Some(var) = p.var_damage {
                let v = Self::get_total_damage_level(zones, &p.range);
                material.update_var(var, |m| m.f_value4 = v);
            }
        }
        for (i, glass) in self.glass_breakable_parts.iter().enumerate() {
            if let Some((material, var)) = glass.var_damage {
                let den = glass.range.max_speed - glass.range.min_speed;
                let t = clamp01((zones[i] - glass.range.min_speed) / den);
                let g = if t > self.glass_damage_threshold { 1.0 } else { 0.0 };
                scene.materials[material.0 as usize].update_var(var, |m| m.f_value = g);
            }
        }
        for b in &mut self.breakable_parts {
            // (a part with an unknown zone reads the float before the array in the game)
            let lvl = if b.damage_zone >= 0 { zones[b.damage_zone as usize] } else { state.tyre_virtual_km[3] };
            let den = b.range.max_speed - b.range.min_speed;
            let level = clamp01((lvl - b.range.min_speed) / den);
            b.damage_level = level;
            if level == 0.0 {
                scene.nodes[b.node].matrix = b.org_matrix;
                continue;
            }
            let r = Mat44f::create_from_axis_angle(&b.static_damage_axis, level * b.static_damage_angle);
            let mut matrix = xm_matrix_multiply(&r, &b.org_matrix);
            if self.oscillation_enabled {
                let ratio = state.engine_rpm / (state.limiter_rpm as f32);
                let ratio = if ratio > 1.0 {
                    1.0
                } else if ratio >= f32::from_bits(0x3e99_999a) {
                    ratio
                } else {
                    f32::from_bits(0x3e99_999a)
                };
                b.phase = ((ratio * level) * 1000.0) + b.phase;
                let s1 = sinf(b.phase);
                let a1 = ((s1 as f64) * ((ratio as f64) * f64::from_bits(0x3f60_624d_d2f1_a9fc))) as f32;
                matrix = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&b.oscillation_axis, a1), &matrix);
                let gx = clamp01(state.acc_g[0].abs() * 0.5) * b.allowed_g[0];
                let gy = clamp01(state.acc_g[1].abs() * f32::from_bits(0x3ecc_cccd)) * b.allowed_g[1];
                let gz = clamp01(state.acc_g[2].abs() * f32::from_bits(0x3e4c_cccd)) * b.allowed_g[2];
                let s = ((gy * gy) + (gx * gx)) + (gz * gz);
                let len = if s == 0.0 || s.is_nan() { 0.0 } else { sqrtf(s) };
                let len = clamp01(len * level);
                let [lo, hi] = b.oscillation_angle;
                let a = ((((hi - lo) * len) + lo) - b.oscillation_old_angle) * (dt * b.oscillation_g_mult) + b.oscillation_old_angle;
                let a = if a > hi {
                    hi
                } else if a >= lo {
                    a
                } else {
                    lo
                };
                b.oscillation_old_angle = a;
                matrix = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&b.oscillation_axis, a), &matrix);
            }
            scene.nodes[b.node].matrix = matrix;
        }
    }
}
