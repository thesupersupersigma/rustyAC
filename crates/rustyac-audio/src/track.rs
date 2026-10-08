// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `TrackAudio` (TrackAudio.obj): an ambience emitter on every `AC_AUDIO_*` dummy of the
//! track's models, and the reverb zones and occluders of the track's `data/audio_sources.ini`.
//! Also the surface-sound pool `Sim::loadTrack` fills.

use std::path::Path;

use rustyac_physics::data::ini::{append_path, IniReader};

use crate::engine::{AudioEngine, AudioOccluder, AudioReverb, EventId, Mat, Vec3, REVERB_OFF};

/// A node of the track's scene graph, as far as the sound looks at it.
#[derive(Clone, Debug)]
pub struct SceneNode {
    pub name: String,
    /// `Node::matrix`: the local matrix.
    pub matrix: Mat,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// A `Renderable` (mesh): never an emitter.
    pub renderable: bool,
    /// A `Mesh`'s vertex positions and indices, for an occluder.
    pub mesh: Option<SceneMesh>,
}

#[derive(Clone, Debug, Default)]
pub struct SceneMesh {
    pub positions: Vec<Vec3>,
    pub indices: Vec<u16>,
}

/// The scene under `Sim::trackNode` (node 0): the track's models in load order.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
}

pub const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

impl Scene {
    /// A scene with only the track node.
    pub fn new() -> Scene {
        Scene { nodes: vec![SceneNode { name: "TRACK_ROOT".to_string(), matrix: IDENTITY, parent: None, children: Vec::new(), renderable: false, mesh: None }] }
    }

    /// Adds a node under `parent` and returns its index.
    pub fn add(&mut self, parent: usize, name: &str, matrix: Mat, renderable: bool, mesh: Option<SceneMesh>) -> usize {
        let index = self.nodes.len();
        self.nodes.push(SceneNode { name: name.to_string(), matrix, parent: Some(parent), children: Vec::new(), renderable, mesh });
        self.nodes[parent].children.push(index);
        index
    }

    /// `Node::findChildrenByPrefix` @ 0x14020e050: depth first, the node itself included.
    pub fn find_children_by_prefix(&self, node: usize, prefix: &str, out: &mut Vec<usize>) {
        if self.nodes[node].name.starts_with(prefix) {
            out.push(node);
        }
        for &child in &self.nodes[node].children {
            self.find_children_by_prefix(child, prefix, out);
        }
    }

    /// `Node::findChildByName(name, true)` @ 0x14020de40: each child before its subtree.
    pub fn find_child_by_name(&self, node: usize, name: &str) -> Option<usize> {
        for &child in &self.nodes[node].children {
            if self.nodes[child].name == name {
                return Some(child);
            }
            if let Some(found) = self.find_child_by_name(child, name) {
                return Some(found);
            }
        }
        None
    }

    /// `Node::getWorldMatrix` @ 0x14020e190: local times the parent's world matrix, with
    /// `XMMatrixMultiply`'s order of additions.
    pub fn world_matrix(&self, node: usize) -> Mat {
        let a = self.nodes[node].matrix;
        match self.nodes[node].parent {
            None => a,
            Some(parent) => {
                let b = self.world_matrix(parent);
                let mut out = [0.0f32; 16];
                for r in 0..4 {
                    for c in 0..4 {
                        out[4 * r + c] = ((a[4 * r + 2] * b[8 + c]) + (a[4 * r] * b[c])) + ((a[4 * r + 3] * b[12 + c]) + (a[4 * r + 1] * b[4 + c]));
                    }
                }
                out
            }
        }
    }
}

/// AC's `TrackAudio` (0xb0 bytes).
pub struct TrackAudio {
    events: Vec<EventId>,
    reverbs: Vec<AudioReverb>,
    started: bool,
    occluders: Vec<AudioOccluder>,
}

impl TrackAudio {
    /// `TrackAudio::TrackAudio` @ 0x1401c24c0. `data_folder` is the track's folder, or its
    /// layout's (where `data/audio_sources.ini` is looked for).
    pub fn new(engine: &mut AudioEngine, scene: &Scene, data_folder: &Path) -> Result<TrackAudio, String> {
        let mut t = TrackAudio { events: Vec::new(), reverbs: Vec::new(), started: false, occluders: Vec::new() };
        let mut found = Vec::new();
        scene.find_children_by_prefix(0, "AC_AUDIO_", &mut found);
        for n in found {
            if scene.nodes[n].renderable {
                continue;
            }
            let e = engine.create_event("event:/common/ambience", REVERB_OFF);
            // the node's local matrix, not its world matrix
            engine.event_set_3d(e, &scene.nodes[n].matrix, &[0.0; 3]);
            t.events.push(e);
        }
        let ini = IniReader::load(&append_path(&append_path(data_folder, "data"), "audio_sources.ini"))?;
        if !ini.ready {
            return Ok(t);
        }
        let mut i = 0;
        loop {
            let sec = format!("REVERB_{i}");
            i += 1;
            if !ini.has_section(&sec) {
                break;
            }
            if ini.get_int(&sec, "ENABLED")? == 0 {
                continue;
            }
            let node_name = ini.get_string(&sec, "NODE");
            let min_d = ini.get_float(&sec, "MINDISTANCE")?;
            let max_d = ini.get_float(&sec, "MAXDISTANCE")?;
            let preset = ini.get_string(&sec, "PRESET");
            // the game stops with "Reverb node does not exist" here
            let n = scene.find_child_by_name(0, &node_name).ok_or_else(|| format!("{}: reverb node does not exist: {node_name}", ini.filename.display()))?;
            let w = scene.world_matrix(n);
            let pos = [w[12], w[13], w[14]];
            let mut reverb = AudioReverb::new(engine);
            reverb.enable(engine, true);
            reverb.set_3d(engine, &pos, min_d, max_d);
            if preset == "CUSTOM" {
                let keys = [
                    "DECAY_TIME",
                    "EARLY_DELAY",
                    "LATE_DELAY",
                    "HF_REFERENCE",
                    "HF_DECAY_RATIO",
                    "DIFFUSION",
                    "DENSITY",
                    "LOW_SHELF_FREQUENCY",
                    "LOW_SHELF_GAIN",
                    "HIGH_CUT",
                    "EARLY_LATE_MIX",
                    "WET_LEVEL",
                ];
                let mut p = [0.0f32; 12];
                for (k, key) in keys.iter().enumerate() {
                    p[k] = ini.get_float(&sec, key)?;
                }
                reverb.set_properties(engine, &p);
            } else {
                reverb.set_preset(engine, AudioReverb::preset_from_name(&preset));
            }
            // stored by a move, which drops the wet level: every stored reverb has 0
            t.reverbs.push(reverb.moved());
        }
        let mut i = 0;
        loop {
            let sec = format!("OCCLUDER_{i}");
            i += 1;
            if !ini.has_section(&sec) {
                break;
            }
            let enabled = ini.get_int(&sec, "ENABLED")? != 0;
            let mesh_name = ini.get_string(&sec, "MESH");
            let vol_occ = ini.get_float(&sec, "VOLUME_OCCLUSION")?;
            let double_sided = ini.get_int(&sec, "DOUBLESIDED")? != 0;
            // the game crashes on a missing node or one that is not a mesh
            let mesh = scene
                .find_child_by_name(0, &mesh_name)
                .and_then(|n| scene.nodes[n].mesh.as_ref())
                .ok_or_else(|| format!("{}: occluder mesh does not exist: {mesh_name}", ini.filename.display()))?;
            let n_idx = mesh.indices.len() as i32;
            let tris = n_idx / 3;
            let mut occluder = AudioOccluder::new(engine, tris, n_idx);
            occluder.enable(engine, enabled);
            occluder.set_volume_occlusion(engine, vol_occ);
            occluder.set_double_sided(engine, double_sided);
            for tri in 0..tris as usize {
                let p = |k: usize| &mesh.positions[mesh.indices[3 * tri + k] as usize];
                occluder.add_triangle(engine, p(0), p(1), p(2));
            }
            t.occluders.push(occluder);
        }
        Ok(t)
    }

    /// `TrackAudio::render` @ 0x1401c42a0: the emitters start on the first frame; every frame
    /// the strongest reverb the listener is in becomes the engine's `reverbValue`.
    pub fn render(&mut self, engine: &mut AudioEngine) {
        if !self.started {
            for &e in &self.events {
                engine.event_start(e);
            }
            self.started = true;
        }
        let mut max = 0.0f32;
        for reverb in &self.reverbs {
            let h = reverb.hear_value(engine);
            if h > max {
                max = h;
            }
        }
        engine.reverb_value = max;
    }

    /// `TrackAudio::~TrackAudio` @ 0x1401c3bd0
    pub fn destroy(mut self, engine: &mut AudioEngine) {
        for occluder in self.occluders.iter_mut() {
            occluder.release(engine);
        }
        for reverb in self.reverbs.iter_mut() {
            reverb.release(engine);
        }
        for e in self.events.drain(..) {
            engine.destroy_event(e);
        }
    }

    pub fn emitters(&self) -> usize {
        self.events.len()
    }

    pub fn reverbs(&self) -> usize {
        self.reverbs.len()
    }

    pub fn occluders(&self) -> usize {
        self.occluders.len()
    }
}

/// The surface sounds `Sim::loadTrack` @ 0x14019a4c0 puts into the engine's pool: twelve
/// instances of `event:/surfaces/<wav without its last four characters>` for every surface of
/// the track that names a wav, in the order of the surfaces' keys.
pub fn cache_surface_sounds(engine: &mut AudioEngine, surfaces: &[(String, String)]) {
    let mut sorted: Vec<&(String, String)> = surfaces.iter().collect();
    // std::map<std::wstring, SurfaceDef>: by UTF-16 unit, then by length
    sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    sorted.dedup_by(|a, b| a.0 == b.0);
    for (_, wav) in sorted {
        if !wav.is_empty() {
            let count = wav.chars().count();
            let stem: String = wav.chars().take(count.saturating_sub(4)).collect();
            engine.add_cache(&format!("event:/surfaces/{stem}"));
        }
    }
}
