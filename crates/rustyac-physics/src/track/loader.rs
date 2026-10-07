// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! Loading a track: the part of AC's `TrackAvatar` that the physics needs.
//!
//! `TrackAvatar::TrackAvatar` @ 0x1401c5250 loads the kn5 models listed in `models.ini`
//! (`init3D` @ 0x1401c8740), walks their nodes and turns every mesh whose name starts with a
//! number into a collision mesh with a surface (`initPhysics` @ 0x1401ca440,
//! `processPhysicsNode` @ 0x1401cc5e0, `addPhysicsMesh` @ 0x1401c78e0), loads the AI line,
//! puts the spawn points on the ground (`initRespawnPositionSet` @ 0x1401cb8c0) and builds the
//! timing lines from pairs of gate nodes (`initTimeLines` @ 0x1401cbc90).
//!
//! The physics vertices are the floats of the file: no node matrix and no `POSITION` /
//! `ROTATION` of `models.ini` is applied to them (the game does not either). Only the helper
//! nodes (`AC_...`) have world matrices.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rustyac_content::kn5::{Kn5, NodeClass};
use rustyac_content::track_files::TrackFiles;

use super::spline;
use super::surfaces::{get_sector_id, SurfacesManager};
use super::timing;
use super::Track;
use crate::vecmath::{xm_matrix_multiply, Mat44f};

/// A helper node of the track's models: a node whose name starts with `AC_`.
#[derive(Clone, Debug, PartialEq)]
pub struct HelperNode {
    pub name: String,
    /// `Node::matrix`: the node's own matrix. This, not the world matrix, is where the game
    /// takes spawn points and timing gates from (and what it changes when it drops a spawn
    /// point onto the road).
    pub local: Mat44f,
    /// `Node::getWorldMatrix` @ 0x14020e190, as loaded.
    pub world: Mat44f,
}

/// What loading found, for the console and the report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackLoadReport {
    pub files: Vec<PathBuf>,
    /// `TrackPhysicsStats`: physical meshes, their triangles, meshes per name number.
    pub objects: u32,
    pub tris: u64,
    pub groups: BTreeMap<i32, i32>,
    /// Vertices of the physical meshes.
    pub vertices: u64,
    /// Nodes of the OPCODE trees.
    pub tree_nodes: u64,
    /// Sub-spaces of the static space, in the order they were made.
    pub spaces: Vec<u32>,
    /// Meshes per surface `KEY` ("" = the default surface).
    pub per_key: BTreeMap<String, u32>,
    /// What the game prints: surfaces not found, ambiguous names, missing files.
    pub messages: Vec<String>,
    /// The helper nodes, in the order of the game's node tree.
    pub helpers: Vec<HelperNode>,
    pub seconds_models: f64,
    pub seconds_trees: f64,
    pub seconds_total: f64,
}

/// The game's folder a track folder sits in (`<game>/content/tracks/<track>`).
pub fn game_root(track_folder: &Path) -> Option<PathBuf> {
    let root = track_folder.parent()?.parent()?.parent()?;
    root.join("system").is_dir().then(|| root.to_path_buf())
}

/// Loads a track from `folder` (`content/tracks/<track>`), layout `config` ("" for none).
pub fn load_track(folder: &Path, config: &str) -> Result<(Track, TrackLoadReport), String> {
    let start = Instant::now();
    let files = TrackFiles::find(folder, config)?;
    let mut report = TrackLoadReport { files: files.models.iter().map(|m| m.file.clone()).collect(), ..TrackLoadReport::default() };
    let mut track = Track { name: files.name.clone(), config: config.to_string(), data_folder: files.data.parent().unwrap_or(folder).to_path_buf(), ..Track::default() };

    // SurfacesManager::SurfacesManager: the game's own surfaces, then the track's
    let system = match game_root(folder) {
        Some(root) => root.join("system").join("data").join("surfaces.ini"),
        // a track outside a game folder: only its own surfaces (and the built-in wall)
        None => folder.join("system_surfaces_not_found.ini"),
    };
    let manager = SurfacesManager::new(&system, &files.data.join("surfaces.ini"))?;
    for missing in &manager.missing {
        report.messages.push(format!("WARNING: Surface definition file {missing} not found"));
    }

    // TrackAvatar::init3D + initPhysics: every model in the order of models.ini, every node
    // before its children, children first to last
    let mut seconds_trees = 0.0;
    for model in &files.models {
        if model.rotation != [0.0; 3] {
            return Err(format!(
                "{}: a model with a ROTATION in models.ini is not ported (mat44f::createFromEuler has not been read)",
                model.file.display()
            ));
        }
        let kn5 = Kn5::open(&model.file).map_err(|e| format!("{}: {e}", model.file.display()))?;
        let mut reader = kn5.reader().map_err(|e| e.to_string())?;
        let mut world: Vec<Mat44f> = Vec::with_capacity(kn5.nodes.len());
        for (index, node) in kn5.nodes.iter().enumerate() {
            let mut local = Mat44f { m: node.matrix };
            let parent_world = match node.parent {
                Some(parent) => world[parent],
                None => {
                    // init3D: the top node's translation is always replaced by POSITION
                    local.m[3][0] = model.position[0];
                    local.m[3][1] = model.position[1];
                    local.m[3][2] = model.position[2];
                    Mat44f::IDENTITY
                }
            };
            world.push(xm_matrix_multiply(&local, &parent_world));

            if node.class == NodeClass::Mesh {
                // processPhysicsNode: a Mesh (never a skinned one) whose name starts with a
                // number other than 0, whatever its flags say
                let id = get_sector_id(&node.name);
                let mesh = node.mesh.as_ref().expect("a mesh node has a mesh");
                if id != 0 {
                    report.objects += 1;
                    *report.groups.entry(id).or_insert(0) += 1;
                    report.tris += mesh.index_count as u64 / 3;
                    // addPhysicsMesh: a mesh without vertices is left out
                    if mesh.vertex_count != 0 {
                        let found = manager.get_surface_desc_from_mesh_name(&node.name);
                        if let Some(message) = &found.error {
                            report.messages.push(message.clone());
                        }
                        let space = if found.surface.def.collision_category == 2 { (id as u32).wrapping_add(10_000) } else { id as u32 };
                        let vertices = reader.positions(mesh).map_err(|e| e.to_string())?;
                        let indices = reader.indices(mesh).map_err(|e| e.to_string())?;
                        report.vertices += vertices.len() as u64;
                        let key = found.key.clone().unwrap_or_default();
                        *report.per_key.entry(key.clone()).or_insert(0) += 1;
                        let before = Instant::now();
                        let made = track.add_surface(&node.name, &key, &found.surface.wav, vertices, indices, &found.surface.def, space);
                        seconds_trees += before.elapsed().as_secs_f64();
                        report.tree_nodes += track.world.meshes[made].data.model.nodes.len() as u64;
                    }
                }
            }
            // the game finds helpers by name among all nodes, meshes too, the first in the tree
            if node.name.starts_with("AC_") {
                report.helpers.push(HelperNode { name: node.name.clone(), local, world: world[index] });
            }
        }
    }
    // what the first ray of the game does once: the boxes of the meshes and of their spaces
    track.world.clean();
    report.spaces = track.world.spaces.iter().map(|s| s.id).collect();
    report.seconds_models = start.elapsed().as_secs_f64() - seconds_trees;
    report.seconds_trees = seconds_trees;

    // TrackAvatar::initPhysics: Track::initAISpline once the meshes are there
    spline::init_ai_spline(&mut track, &files.ai, &files.data, &mut report.messages)?;
    // TrackAvatar::TrackAvatar: three spawn sets, then the timing lines. (The set of a
    // session, `HOTLAP_START` for one, is made when the session is: init_respawn_position_set.)
    track.helper_nodes = report.helpers.clone();
    for set in ["PIT", "START", "TIME_ATTACK"] {
        timing::init_respawn_position_set(&mut track, set);
    }
    timing::init_time_lines(&mut track, &mut report.messages);
    report.seconds_total = start.elapsed().as_secs_f64();
    Ok((track, report))
}

impl TrackLoadReport {
    /// A few lines for the console.
    pub fn summary(&self, track: &Track) -> String {
        let keys: Vec<String> = self.per_key.iter().map(|(key, count)| format!("{} {count}", if key.is_empty() { "(no match)" } else { key })).collect();
        format!(
            "track {}{}: {} physics meshes, {} triangles, {} vertices in {} sub-spaces ({} tree nodes), {} timing lines, AI line {}, spawn sets {}\n\
             surfaces: {}\n\
             loaded in {:.2} s (models {:.2} s, collision trees {:.2} s)",
            track.name,
            if track.config.is_empty() { String::new() } else { format!(" / {}", track.config) },
            self.objects,
            self.tris,
            self.vertices,
            self.spaces.len(),
            self.tree_nodes,
            track.time_lines.len(),
            match &track.ai_spline {
                Some(spline) => format!("{} points, {:.1} m", spline.point_count(), spline.length()),
                None => "none".to_string(),
            },
            track.spawn_positions.iter().map(|(name, slots)| format!("{name} x{}", slots.len())).collect::<Vec<_>>().join(", "),
            keys.join(", "),
            self.seconds_total,
            self.seconds_models,
            self.seconds_trees
        )
    }
}
