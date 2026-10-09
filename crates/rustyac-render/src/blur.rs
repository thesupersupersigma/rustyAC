// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The wheels at speed: `BlurredObjects` (`BlurredObjects::BlurredObjects` 0x1400c35d0,
//! `update` 0x1400c43b0) switches the nodes of `data/blurred_objects.ini` (rims and their
//! blurred twins) by the wheel's speed; `TyreBlur` (`TyreBlur::TyreBlur` 0x1401cf760,
//! `initTyreMaterials` 0x1401cf910, `processNode` 0x1401cfb10, `update` 0x1401cfdb0) gives
//! every `ksTyres` mesh a material of its own and writes its `blurLevel` and `dirtyLevel`.

use std::path::Path;

use rustyac_physics::data::ini::IniReader;

use crate::graphics::Graphics;
use crate::material::MaterialId;
use crate::scene::{NodeId, NodeKind, Scene};
use crate::state::CarPhysicsState;

/// `BlurredNode` (0x18 bytes).
pub struct BlurredNode {
    pub node: NodeId,
    pub wheel_index: usize,
    pub min_angular_speed: f32,
    pub max_angular_speed: f32,
}

/// `BlurredObjects` (0x78 bytes).
#[derive(Default)]
pub struct BlurredObjects {
    pub blurred_nodes: Vec<BlurredNode>,
}

impl BlurredObjects {
    /// `BlurredObjects::BlurredObjects` 0x1400c35d0. `wheel_nodes(i)` are the nodes the game's
    /// `getWheelTransform` gives for wheel `i`: one that holds every level's wheel, or one per
    /// level of detail. Where the game stops with a critical error this returns it.
    pub fn new(scene: &Scene, folder: &Path, wheel_nodes: &dyn Fn(usize) -> Vec<NodeId>) -> Result<BlurredObjects, String> {
        let mut objects = BlurredObjects::default();
        let Ok(ini) = IniReader::load(&folder.join("data/blurred_objects.ini")) else {
            return Ok(objects);
        };
        let mut index = 0;
        loop {
            let section = format!("OBJECT_{index}");
            if !ini.has_section(&section) {
                break;
            }
            let name = ini.get_string(&section, "NAME");
            let min = ini.get_float(&section, "MIN_SPEED").unwrap_or(0.0);
            let max = ini.get_float(&section, "MAX_SPEED").unwrap_or(0.0);
            let wheel = ini.get_int(&section, "WHEEL_INDEX").unwrap_or(0);
            // (the game also lets 4 through, which reads past the wheels; no car has it)
            if !(0..=3).contains(&wheel) {
                return Err(format!("BLURRED NODE {name} HAS INVALID WHEELINDEX={wheel}"));
            }
            let mut found = Vec::new();
            for w in wheel_nodes(wheel as usize) {
                scene.find_children_by_name(w, &name, &mut found);
            }
            if found.is_empty() {
                return Err(format!("BLURRED NODE ({name}) WHEELINDEX[{wheel}]: CANNOT LOCATE OBJECT"));
            }
            for node in found {
                objects.blurred_nodes.push(BlurredNode { node, wheel_index: wheel as usize, min_angular_speed: min, max_angular_speed: max });
            }
            index += 1;
        }
        Ok(objects)
    }

    /// `BlurredObjects::update` 0x1400c43b0. `scale` is 1, or the replay's time multiplier.
    pub fn update(&self, scene: &mut Scene, state: &CarPhysicsState, scale: f32) {
        for e in &self.blurred_nodes {
            let w = (scale * state.wheel_angular_speed[e.wheel_index]).abs();
            scene.nodes[e.node].is_active = w >= e.min_angular_speed && w <= e.max_angular_speed;
        }
    }
}

/// `TyreMaterials` (0x30 bytes): the two variables of every tyre material of one wheel.
#[derive(Default)]
struct TyreMaterials {
    var_blur_level: Vec<(MaterialId, Option<usize>)>,
    var_dirty_level: Vec<(MaterialId, Option<usize>)>,
}

/// `TyreBlur` (0x120 bytes).
#[derive(Default)]
pub struct TyreBlur {
    tyre_materials: [TyreMaterials; 4],
}

impl TyreBlur {
    /// `TyreBlur::TyreBlur` 0x1401cf760 with `initTyreMaterials` 0x1401cf910.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, wheel_nodes: &dyn Fn(usize) -> Vec<NodeId>) -> Result<TyreBlur, String> {
        let mut blur = TyreBlur::default();
        for w in 0..4 {
            for wheel in wheel_nodes(w) {
                let children = scene.nodes[wheel].children.clone();
                for child in children {
                    blur.process_node(graphics, scene, child, w)?;
                }
            }
        }
        Ok(blur)
    }

    /// `TyreBlur::processNode` 0x1401cfb10.
    fn process_node(&mut self, graphics: &mut Graphics, scene: &mut Scene, n: NodeId, w: usize) -> Result<(), String> {
        let material = match &scene.nodes[n].kind {
            NodeKind::Mesh(mesh) => mesh.material,
            NodeKind::SkinnedMesh(mesh) => mesh.material,
            _ => None,
        };
        if let Some(id) = material {
            let is_tyre = scene.materials[id.0 as usize].shader.is_some_and(|s| graphics.shaders.get(s).name == "ksTyres");
            if is_tyre {
                let clone = scene.materials[id.0 as usize].clone_material(graphics)?;
                let new_id = MaterialId(scene.materials.len() as u32);
                let (blur, dirty) = (clone.get_var_reporting("blurLevel"), clone.get_var_reporting("dirtyLevel"));
                scene.materials.push(clone);
                match &mut scene.nodes[n].kind {
                    NodeKind::Mesh(mesh) => mesh.material = Some(new_id),
                    NodeKind::SkinnedMesh(mesh) => mesh.material = Some(new_id),
                    _ => {}
                }
                self.tyre_materials[w].var_blur_level.push((new_id, blur));
                self.tyre_materials[w].var_dirty_level.push((new_id, dirty));
            }
        }
        let children = scene.nodes[n].children.clone();
        for child in children {
            self.process_node(graphics, scene, child, w)?;
        }
        Ok(())
    }

    /// `TyreBlur::update` 0x1401cfdb0.
    pub fn update(&self, scene: &mut Scene, state: &CarPhysicsState, scale: f32) {
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
            let dirty = clamp(state.tyre_dirty_level[i].abs()) * f32::from_bits(0x3f8c_cccd);
            let blur = clamp((state.wheel_angular_speed[i].abs() * scale) * f32::from_bits(0x3dcc_cccd));
            for (material, var) in &self.tyre_materials[i].var_blur_level {
                if let Some(var) = var {
                    scene.materials[material.0 as usize].update_var(*var, |v| v.f_value = blur);
                }
            }
            for (material, var) in &self.tyre_materials[i].var_dirty_level {
                if let Some(var) = var {
                    scene.materials[material.0 as usize].update_var(*var, |v| v.f_value = dirty);
                }
            }
        }
    }
}
