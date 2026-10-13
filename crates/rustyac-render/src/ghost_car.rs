// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GhostCar` (0x330 bytes): the transparent copy of the player's car that drives the best
//! lap again. Constructor 0x140104fe0, `loadGhostCarParameters` 0x140105fd0 (the game's
//! `system/cfg/ghost_car.ini`), `init3D` 0x140105520 (the car's first model with one flat
//! see-through material, its wheels and suspension arms on nodes of their own, under the
//! "before the cars" node), `assignGhostMaterial` 0x1401053a0, `update` 0x140106380,
//! `getGhostCarOpacity` 0x140105480. The lap itself is `GhostCarRecorder`'s
//! (`rustyac_physics::replay::ghost`).

use std::path::Path;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::replay::ghost::{ghost_car_opacity, GhostCarRecorder};
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::graphics::{Graphics, BLEND_ALPHA};
use crate::material::{Material, MaterialId};
use crate::model::Kn5Io;
use crate::scene::{NodeId, NodeKind, Scene};

const WHEEL_NAMES: [&str; 4] = ["WHEEL_LF", "WHEEL_RF", "WHEEL_LR", "WHEEL_RR"];
const SUS_NAMES: [&str; 4] = ["SUSP_LF", "SUSP_RF", "SUSP_LR", "SUSP_RR"];

/// `GhostCar`.
pub struct GhostCar {
    pub car_node: NodeId,
    pub body_transform: NodeId,
    wheel_transforms: [NodeId; 4],
    sus_transforms: [Option<NodeId>; 4],
    material: MaterialId,
    pub max_distance: f32,
    pub min_distance: f32,
    pub max_opacity: f32,
    pub gc_color: [f32; 3],
}

/// `GhostCar::assignGhostMaterial` 0x1401053a0: every mesh below the node gets the ghost's
/// material, casts no shadow and is drawn with the see-through things.
fn assign_ghost_material(scene: &mut Scene, node: NodeId, material: MaterialId) {
    match &mut scene.nodes[node].kind {
        NodeKind::Mesh(mesh) => mesh.material = Some(material),
        NodeKind::SkinnedMesh(mesh) => mesh.material = Some(material),
        _ => {}
    }
    if matches!(scene.nodes[node].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_)) {
        if let Some(r) = &mut scene.nodes[node].renderable {
            r.cast_shadows = false;
            r.is_transparent = true;
        }
    }
    let children = scene.nodes[node].children.clone();
    for child in children {
        assign_ghost_material(scene, child, material);
    }
}

impl GhostCar {
    /// `GhostCar::GhostCar` 0x140104fe0 for the car of `folder` (`folder_text`: the folder as
    /// the game's paths have it), below `before_cars`.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, before_cars: NodeId, folder_text: &str, folder: &Path) -> Result<GhostCar, String> {
        // loadGhostCarParameters: when the file is there every key is read (a missing one is 0)
        let (mut min_distance, mut max_distance, mut gc_color, mut max_opacity) = (1.0f32, 100.0f32, [1.0f32; 3], 0.6f32);
        let parameters = graphics.game_folder.join("system/cfg/ghost_car.ini");
        if parameters.is_file() {
            if let Ok(ini) = IniReader::load(&parameters) {
                min_distance = ini.get_float("GHOST_CAR", "MIN_DISTANCE").unwrap_or(0.0);
                max_distance = ini.get_float("GHOST_CAR", "MAX_DISTANCE").unwrap_or(0.0);
                max_opacity = ini.get_float("GHOST_CAR", "MAX_OPACITY").unwrap_or(0.0);
                gc_color = ini.get_float3("GHOST_CAR", "COLOR").unwrap_or([0.0; 3]);
                for c in &mut gc_color {
                    *c *= f32::from_bits(0x3b80_8081);
                }
                let mut v = Vec3f::new(gc_color[0], gc_color[1], gc_color[2]);
                v.normalize();
                gc_color = [v.x, v.y, v.z];
            }
        }
        // init3D
        let car_node = scene.bounding_sphere("CARNODE", 4.0);
        let body_transform = scene.node("BODYTR");
        let data = |file: &str| IniReader::load(&folder.join("data").join(file));
        let lods = data("lods.ini")?;
        let filename = lods.get_string("LOD_0", "FILE");
        let io = Kn5Io::new();
        let model_link = io.load(graphics, scene, &format!("{folder_text}/{filename}"), &folder.join(&filename))?;
        scene.compile(graphics, model_link);
        scene.add_child(car_node, body_transform);
        scene.add_child(body_transform, model_link);
        let suspensions = data("suspensions.ini")?;
        let offset = |name: &str| suspensions.get_float("GRAPHICS_OFFSETS", name).unwrap_or(0.0);
        let shifted = |x: f32| {
            let mut m = Mat44f::IDENTITY;
            m.m[3][0] = x;
            m
        };
        let mut wheel_transforms = [0; 4];
        let mut sus_transforms = [None; 4];
        for i in 0..4 {
            wheel_transforms[i] = scene.node("WHEEL_TRANSFORM");
            let wheel = scene.find_child_by_name(body_transform, WHEEL_NAMES[i], true).ok_or(format!("ERROR, CANNOT LOCATE {} FOR GHOSTCAR", WHEEL_NAMES[i]))?;
            scene.add_child(wheel_transforms[i], wheel);
            scene.nodes[wheel].matrix = shifted(offset(WHEEL_NAMES[i]));
            scene.add_child(car_node, wheel_transforms[i]);
            match scene.find_child_by_name(body_transform, SUS_NAMES[i], true) {
                Some(arm) => {
                    let node = scene.node("SUS_NODE");
                    scene.add_child(node, arm);
                    scene.nodes[node].matrix = scene.nodes[arm].matrix;
                    scene.nodes[arm].matrix = shifted(offset(SUS_NAMES[i]));
                    scene.add_child(car_node, node);
                    sus_transforms[i] = Some(node);
                }
                None => println!("ERROR, CANNOT LOCATE {}", SUS_NAMES[i]),
            }
        }
        if let NodeKind::BoundingSphere { delegate } = &mut scene.nodes[car_node].kind {
            *delegate = Some(body_transform);
        }
        scene.apply_no_cull(car_node);
        scene.add_child(before_cars, car_node);
        let mut material = Material::new("GHOST_MATERIAL");
        material.set_shader(graphics, "ksColourShader")?;
        material.blend_mode = BLEND_ALPHA;
        material.double_face = false;
        let id = MaterialId(scene.materials.len() as u32);
        scene.materials.push(material);
        assign_ghost_material(scene, car_node, id);
        Ok(GhostCar { car_node, body_transform, wheel_transforms, sus_transforms, material: id, max_distance, min_distance, max_opacity, gc_color })
    }

    /// `GhostCar::update` 0x140106380. `lap_time`: the player's running lap time
    /// (`CarPhysicsState::lapTime`); `make_body_matrix`: `CarAvatar::makeBodyMatrix` of the
    /// player's car; `player`: the player's body node's place; `replay`: a replay shows.
    pub fn update(&mut self, scene: &mut Scene, recorder: &mut GhostCarRecorder, lap_time: u32, make_body_matrix: &dyn Fn(&Mat44f) -> Mat44f, player: &[f32; 3], replay: bool) {
        if recorder.should_display_ghost_car() {
            recorder.set_lap_time(lap_time);
            if let Some(body) = recorder.get_body_matrix() {
                scene.nodes[self.body_transform].matrix = if recorder.version < 4 { body } else { make_body_matrix(&body) };
            }
            for i in 0..4 {
                if recorder.suspension_wheels_enabled != 0 {
                    if let Some(m) = recorder.get_tyre_matrix(i) {
                        scene.nodes[self.wheel_transforms[i]].matrix = m;
                    }
                    if let (Some(node), Some(m)) = (self.sus_transforms[i], recorder.get_suspension_matrix(i)) {
                        scene.nodes[node].matrix = m;
                    }
                }
            }
        }
        let visible = recorder.should_display_ghost_car() && !replay;
        let at = scene.nodes[self.body_transform].matrix.m[3];
        let opacity = ghost_car_opacity(&[at[0], at[1], at[2]], player, self.min_distance, self.max_distance, self.max_opacity);
        let colour = [self.gc_color[0], self.gc_color[1], self.gc_color[2], opacity];
        let material = &mut scene.materials[self.material.0 as usize];
        if let Some(index) = material.get_var_reporting("colour") {
            material.update_var(index, |v| v.f_value4 = colour);
        }
        scene.nodes[self.car_node].is_active = visible;
    }
}
