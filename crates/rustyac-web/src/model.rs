// SPDX-License-Identifier: GPL-3.0-or-later

//! kn5 models for the browser's picture: every drawn mesh with its diffuse texture and, as
//! in the desktop's debug view (`rustyac-game/src/render/models.rs`, which this follows rule
//! for rule), the detail texture where a material multiplies one in and the layers of the
//! multilayer ground and grass shaders.
//!
//! No graphics API in here: the loader hands every texture and every mesh to a [`Sink`] as
//! soon as it is read and keeps only what drawing needs (nodes, materials, bounding
//! spheres). The wasm build's sink puts them on the graphics card; a test's sink counts.

use std::collections::HashMap;
use std::path::PathBuf;

use rustyac_content::kn5::{Kn5, Kn5Reader, Name, NodeClass, Vertex};
use rustyac_game::render::dds::{self, Image};
use rustyac_game::render::scene::mul;
use rustyac_game::view::{Mat, IDENTITY};

/// Where the loader's textures and meshes go. The numbers handed back are the sink's own.
pub trait Sink {
    fn texture(&mut self, image: &Image) -> Result<usize, String>;
    fn mesh(&mut self, vertices: &[Vertex], indices: &[u16]) -> Result<usize, String>;
}

/// What loading a set of models took and made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelStats {
    pub files: usize,
    pub meshes: usize,
    pub triangles: u64,
    pub geometry_bytes: u64,
    pub textures: usize,
    pub texture_bytes: u64,
    /// The longest texture side kept, pixels (0 = nothing was dropped).
    pub texture_size: u32,
    /// Materials drawn in a flat colour (no diffuse texture, or one that could not be read).
    pub flat_materials: usize,
    /// Problems worth a line (unreadable textures ...), at most a handful.
    pub notes: Vec<String>,
}

pub struct Mesh {
    /// The sink's number of the mesh's buffers.
    pub buffers: usize,
    pub index_count: u32,
    pub material: usize,
    pub node: usize,
    /// Bounding sphere in the mesh's own space.
    pub centre: [f32; 3],
    pub radius: f32,
    pub lod_in: f32,
    pub lod_out: f32,
    /// The kn5 mesh's `isTransparent`: drawn in the second pass, without writing depth.
    pub transparent: bool,
}

/// A material as the picture's one shader wants it; the texture numbers are the sink's.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub texture: Option<usize>,
    /// The detail texture and how often it repeats over the diffuse one (`useDetail`).
    pub detail: Option<(usize, f32)>,
    pub color: [f32; 4],
    /// AC's `Material::blendMode`: 0 opaque, 1 alpha blend, 2 alpha to coverage
    /// (`alphaTested`, which wins over 1).
    pub blend_mode: u8,
    /// AC's `Material::depthMode`: 0 normal, 1 no depth write, 2 no depth test.
    pub depth_mode: i32,
    /// Lit from both sides the same (leaves, fences).
    pub foliage: bool,
    /// 0 the diffuse texture; 1 `ksMultilayer` (four tiled detail textures weighted by a
    /// mask, laid out by world position); 2 `ksMultilayer_objsp` (the same by the mesh's uv);
    /// 3 `ksGrass`.
    pub kind: u8,
    /// `txMask` (kind 3: `txVariation`), `txDetailR`, `txDetailG`, `txDetailB`, `txDetailA`.
    pub layers: [Option<usize>; 5],
    pub mult: [[f32; 2]; 4],
    /// `magicMult` (kind 3: `gain`).
    pub magic: f32,
    /// The diffuse texture's uv factor (`ksPerPixelNM_UVMult`: 1 + `diffuseMult`).
    pub uv_mult: f32,
    /// `ksPerPixelAlpha`'s `alpha`.
    pub alpha_scale: f32,
    /// `ksAmbient`, `ksDiffuse`; `None`: the material does not say.
    pub ks: Option<[f32; 2]>,
}

fn layer_slots(shader: &str) -> (u8, &'static [&'static str]) {
    match shader {
        "ksMultilayer" | "ksMultilayer_fresnel_nm" => (1, &["txMask", "txDetailR", "txDetailG", "txDetailB", "txDetailA"]),
        "ksMultilayer_objsp" => (2, &["txMask", "txDetailR", "txDetailG", "txDetailB", "txDetailA"]),
        "ksGrass" => (3, &["txVariation"]),
        _ => (0, &[]),
    }
}

pub struct Node {
    pub name: Name,
    pub parent: Option<usize>,
    pub local: Mat,
    pub active: bool,
}

pub struct Model {
    pub nodes: Vec<Node>,
    /// Every node's world matrix; [`Model::update`] fills it.
    pub world: Vec<Mat>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub stats: ModelStats,
    /// A track's loose objects in the order the physics has them: the node and the matrix
    /// the file gives it.
    pub object_nodes: Vec<(usize, Mat)>,
    moved: Vec<usize>,
}

/// How to load a set of models.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOptions {
    /// Longest texture side kept, pixels (0 = as stored).
    pub texture_size: u32,
    /// Megabytes of texture the set may take; the size cap is halved until it fits.
    pub texture_budget_mb: u32,
    /// The card takes no block-compressed textures: they are unpacked to RGBA here.
    pub unpack_bc: bool,
}

/// A colour for a material that has no picture, from what its names say it is.
fn flat_color(material: &str, shader: &str, texture: &str) -> [f32; 4] {
    let text = format!("{material} {shader} {texture}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&["grass", "erba", "lawn"]) {
        [0.24, 0.38, 0.16, 1.0]
    } else if has(&["tree", "leaf", "forest", "bush"]) {
        [0.16, 0.30, 0.12, 1.0]
    } else if has(&["sand", "gravel"]) {
        [0.66, 0.58, 0.42, 1.0]
    } else if has(&["kerb", "curb"]) {
        [0.75, 0.25, 0.20, 1.0]
    } else if has(&["asph", "road", "tarmac", "track"]) {
        [0.27, 0.27, 0.29, 1.0]
    } else if has(&["glass"]) {
        [0.45, 0.55, 0.62, 0.5]
    } else if has(&["concrete", "wall", "barrier", "cement"]) {
        [0.56, 0.56, 0.54, 1.0]
    } else {
        [0.52, 0.52, 0.52, 1.0]
    }
}

/// Where the models file of a track puts one of its models: `POSITION` and `ROTATION`
/// (degrees) of its `[MODEL_n]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

/// How many times larger a block-compressed image is once unpacked to RGBA.
fn unpack_factor(format: u32) -> u64 {
    match format {
        dds::BC1_UNORM | dds::BC4_UNORM => 8,
        dds::BC2_UNORM | dds::BC3_UNORM | dds::BC5_UNORM | dds::BC7_UNORM => 4,
        _ => 1,
    }
}

impl Model {
    /// Loads kn5 files as one model. Each file's root hangs under the model's root with its
    /// own matrix; for a track model `placements[i]` changes that matrix as the game does
    /// (`TrackAvatar::init3D`).
    pub fn load(files: &[PathBuf], placements: &[Placement], options: &ModelOptions, sink: &mut dyn Sink) -> Result<Model, String> {
        let mut stats = ModelStats { files: files.len(), ..ModelStats::default() };
        let kn5s: Vec<Kn5> = files.iter().map(|f| Kn5::open(f).map_err(|e| format!("{}: {e}", f.display()))).collect::<Result<_, _>>()?;
        let mut readers: Vec<Kn5Reader> = kn5s.iter().map(|k| k.reader().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;

        // which textures are drawn at all: the diffuse slot of every material a drawn mesh uses
        let drawn = |kn5: &Kn5, node: usize| {
            let mut at = Some(node);
            while let Some(i) = at {
                // the game hides a track's helper nodes (spawn points, timing gates, pit crew
                // places) when it has read them: TrackAvatar::processPhysicsNode
                let helper = kn5.nodes[i].name.starts_with("AC_") && !kn5.nodes[i].name.starts_with("AC_POBJECT");
                if !kn5.nodes[i].active || helper {
                    return false;
                }
                at = kn5.nodes[i].parent;
            }
            let Some(mesh) = kn5.nodes[node].mesh.as_ref() else { return false };
            if kn5.materials.is_empty() {
                return false;
            }
            // a car's cracked glass is only shown after a crash
            let hidden = kn5.materials.get(mesh.material_id as usize).is_some_and(|m| m.shader == "ksBrokenGlass");
            !hidden && mesh.is_renderable && mesh.is_visible && mesh.index_count >= 3 && mesh.vertex_count > 0
        };
        // a texture is looked for in its own file first, then in the others (a track's models
        // share them)
        let mut where_is: HashMap<&[u8], (usize, usize)> = HashMap::new();
        for (file, kn5) in kn5s.iter().enumerate() {
            for (index, texture) in kn5.textures.iter().enumerate() {
                where_is.entry(texture.name.as_bytes()).or_insert((file, index));
            }
        }
        let locate = |file: usize, name: &Name| -> Option<(usize, usize)> {
            kn5s[file].textures.iter().position(|t| t.name == *name).map(|i| (file, i)).or_else(|| where_is.get(name.as_bytes()).copied())
        };
        // the detail texture of a material that multiplies one in
        let detail_of = |material: &rustyac_content::Material| -> Option<(Name, f32)> {
            let on = material.property("useDetail").is_some_and(|p| p.value >= 1.0) && material.shader.starts_with("ksPerPixel");
            let texture = material.textures.iter().find(|t| t.name == "txDetail")?;
            on.then(|| (texture.texture.clone(), material.property("detailUVMultiplier").map(|p| p.value).filter(|v| *v > 0.0).unwrap_or(1.0)))
        };
        let mut wanted: Vec<(usize, usize)> = Vec::new();
        for (file, kn5) in kn5s.iter().enumerate() {
            for node in 0..kn5.nodes.len() {
                if !drawn(kn5, node) {
                    continue;
                }
                let material = kn5.nodes[node].mesh.as_ref().map(|m| m.material_id as usize).unwrap_or(0);
                let Some(material) = kn5.materials.get(material) else { continue };
                let (_, slots) = layer_slots(&material.shader.display());
                let layers = slots.iter().filter_map(|slot| material.textures.iter().find(|t| t.name == *slot).map(|t| t.texture.clone()));
                let names = [material.diffuse().cloned(), detail_of(material).map(|d| d.0)];
                for name in names.into_iter().flatten().chain(layers) {
                    if let Some(found) = locate(file, &name) {
                        if !wanted.contains(&found) {
                            wanted.push(found);
                        }
                    }
                }
            }
        }

        // the size cap that fits the budget, worked out from the textures' headers alone
        let mut cap = options.texture_size;
        let mut numbers: HashMap<(usize, usize), usize> = HashMap::new();
        loop {
            let mut total = 0u64;
            for &(file, index) in &wanted {
                let entry = &kn5s[file].textures[index];
                let head = readers[file].texture_head(entry, 148).map_err(|e| e.to_string())?;
                if let Ok(plan) = dds::plan_dds(&head, entry.size as usize, cap) {
                    total += plan.bytes() as u64 * if options.unpack_bc { unpack_factor(plan.format) } else { 1 };
                }
            }
            if total <= options.texture_budget_mb as u64 * 1_048_576 || (cap != 0 && cap <= 64) {
                break;
            }
            cap = if cap == 0 { 2048 } else { cap / 2 };
        }
        // one at a time from the file to the sink
        for &(file, index) in &wanted {
            let entry = &kn5s[file].textures[index];
            let bytes = readers[file].texture(entry).map_err(|e| e.to_string())?;
            let image = dds::parse_image(&bytes, cap).and_then(|image| crate::bc::for_card(image, options.unpack_bc));
            match image.and_then(|image| sink.texture(&image).map(|number| (number, image.bytes()))) {
                Ok((number, bytes)) => {
                    stats.textures += 1;
                    stats.texture_bytes += bytes as u64;
                    numbers.insert((file, index), number);
                }
                Err(e) => {
                    if stats.notes.len() < 6 {
                        stats.notes.push(format!("texture {}: {e}", entry.name));
                    }
                }
            }
        }
        stats.texture_size = cap;

        let mut model = Model { nodes: Vec::new(), world: Vec::new(), meshes: Vec::new(), materials: Vec::new(), stats: ModelStats::default(), object_nodes: Vec::new(), moved: Vec::new() };
        model.nodes.push(Node { name: Name::default(), parent: None, local: IDENTITY, active: true });
        for (file, kn5) in kn5s.iter().enumerate() {
            let node_base = model.nodes.len();
            let material_base = model.materials.len();
            for material in &kn5.materials {
                let diffuse = material.diffuse();
                let texture = diffuse.and_then(|name| locate(file, name)).and_then(|key| numbers.get(&key).copied());
                // (as text only to guess a colour from the words in the names)
                let (shader, diffuse) = (material.shader.display(), diffuse.map(|name| name.display()).unwrap_or_default());
                let (shader, diffuse) = (&*shader, &*diffuse);
                let foliage = shader.contains("Tree") || shader.contains("Grass") || material.alpha_tested;
                let mut color = if texture.is_some() { [1.0; 4] } else { flat_color(&material.name.display(), shader, diffuse) };
                if texture.is_none() {
                    stats.flat_materials += 1;
                    if material.alpha_blend_mode != 0 && color[3] == 1.0 {
                        color[3] = 0.6;
                    }
                }
                let detail = detail_of(material).and_then(|(name, repeat)| Some((locate(file, &name).and_then(|key| numbers.get(&key).copied())?, repeat)));
                // the layers of a multilayer or grass shader; without its mask (or with no
                // textures at all) the material is drawn the plain way
                let (mut kind, slots) = layer_slots(shader);
                let mut layers: [Option<usize>; 5] = Default::default();
                for (slot, layer) in slots.iter().zip(layers.iter_mut()) {
                    *layer = material.textures.iter().find(|t| t.name == *slot).and_then(|t| locate(file, &t.texture)).and_then(|key| numbers.get(&key).copied());
                }
                if layers[0].is_none() || texture.is_none() {
                    kind = 0;
                }
                // a shader takes the member of a property that has its variable's type
                // (KN5IO::loadMaterialsBinary): a float2 for multA, and for all four in _objsp
                let scalar = |name: &str, default: f32| material.property(name).map_or(default, |p| p.value);
                let pair = |name: &str| material.property(name).map_or([0.0; 2], |p| p.value2);
                let both = |name: &str| [scalar(name, 0.0); 2];
                let (mult, magic) = match kind {
                    1 => ([both("multR"), both("multG"), both("multB"), pair("multA")], scalar("magicMult", 1.0)),
                    2 => ([pair("multR"), pair("multG"), pair("multB"), pair("multA")], scalar("magicMult", 1.0)),
                    3 => ([pair("scale"), [0.0; 2], [0.0; 2], [0.0; 2]], scalar("gain", 0.0)),
                    _ => ([[0.0; 2]; 4], 1.0),
                };
                let ks = match (material.property("ksAmbient"), material.property("ksDiffuse")) {
                    (Some(ambient), Some(diffuse)) if texture.is_some() => Some([ambient.value, diffuse.value]),
                    _ => None,
                };
                model.materials.push(Material {
                    texture,
                    detail,
                    color,
                    blend_mode: if material.alpha_tested {
                        2
                    } else if material.alpha_blend_mode != 0 {
                        1
                    } else {
                        0
                    },
                    depth_mode: material.depth_mode,
                    foliage,
                    kind,
                    layers,
                    mult,
                    magic,
                    uv_mult: if shader == "ksPerPixelNM_UVMult" { 1.0 + scalar("diffuseMult", 0.0) } else { 1.0 },
                    alpha_scale: if shader == "ksPerPixelAlpha" { scalar("alpha", 1.0) } else { 1.0 },
                    ks,
                });
            }
            for (index, node) in kn5.nodes.iter().enumerate() {
                let (parent, local) = match node.parent {
                    Some(parent) => (Some(node_base + parent), node.matrix),
                    None => (
                        Some(0),
                        match placements.get(file) {
                            Some(place) => rustyac_physics::track::loader::top_node_matrix(&rustyac_physics::vecmath::Mat44f { m: node.matrix }, place.position, place.rotation).m,
                            None => node.matrix,
                        },
                    ),
                };
                // a loose object as the physics finds it (TrackObject::TrackObject)
                if node.name.starts_with("AC_POBJECT") && node.children.first().is_some_and(|&child| kn5.nodes[child].class == NodeClass::Mesh && kn5.nodes[child].mesh.is_some()) {
                    model.object_nodes.push((model.nodes.len(), local));
                }
                model.nodes.push(Node { name: node.name.clone(), parent, local, active: node.active });
                if !drawn(kn5, index) {
                    continue;
                }
                let Some(mesh) = &node.mesh else { continue };
                if node.class == NodeClass::SkinnedMesh {
                    // bones are not animated here: a skinned mesh would be drawn in its bind pose
                    // at the model's origin, which is worse than leaving it out
                    continue;
                }
                let vertices = readers[file].vertices(mesh).map_err(|e| e.to_string())?;
                let indices = readers[file].indices(mesh).map_err(|e| e.to_string())?;
                let index_count = indices.len() / 3 * 3;
                let material = material_base + (mesh.material_id as usize).min(kn5.materials.len().saturating_sub(1));
                stats.meshes += 1;
                stats.triangles += indices.len() as u64 / 3;
                stats.geometry_bytes += (vertices.len() * 32 + indices.len() * 2) as u64;
                model.meshes.push(Mesh {
                    buffers: sink.mesh(&vertices, &indices[..index_count])?,
                    index_count: index_count as u32,
                    material,
                    node: node_base + index,
                    centre: mesh.bounding_centre,
                    radius: mesh.bounding_radius,
                    lod_in: mesh.lod_in,
                    lod_out: mesh.lod_out,
                    transparent: mesh.is_transparent,
                });
            }
        }
        model.stats = stats;
        model.world = vec![IDENTITY; model.nodes.len()];
        model.update(&IDENTITY, &[]);
        Ok(model)
    }

    /// Puts the nodes of a track's loose objects where their bodies are: `moved` lists the
    /// objects that are not at home (number, world matrix); every other object goes back
    /// to the matrix of the file. Returns whether anything changed.
    pub fn place_objects(&mut self, moved: &[(u16, Mat)]) -> bool {
        if moved.is_empty() && self.moved.is_empty() {
            return false;
        }
        for &number in &self.moved {
            let (node, home) = self.object_nodes[number];
            self.nodes[node].local = home;
        }
        self.moved.clear();
        for &(number, matrix) in moved {
            if let Some(&(node, _)) = self.object_nodes.get(number as usize) {
                self.nodes[node].local = matrix;
                self.moved.push(number as usize);
            }
        }
        true
    }

    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Recomputes every node's world matrix: the root is at `root`; a node listed in `fixed`
    /// is put where the list says, whatever its parents do.
    pub fn update(&mut self, root: &Mat, fixed: &[(usize, Mat)]) {
        for index in 0..self.nodes.len() {
            // parents come before their children in the list
            let world = match self.nodes[index].parent {
                None => *root,
                Some(parent) => mul(&self.nodes[index].local, &self.world[parent]),
            };
            self.world[index] = fixed.iter().find(|(node, _)| *node == index).map(|(_, m)| *m).unwrap_or(world);
        }
    }
}

/// The six planes of a view-projection matrix (row vectors, depth 0..1), pointing inwards.
pub fn frustum(view_proj: &Mat) -> [[f32; 4]; 6] {
    let column = |j: usize| [view_proj[0][j], view_proj[1][j], view_proj[2][j], view_proj[3][j]];
    let (x, y, z, w) = (column(0), column(1), column(2), column(3));
    let add = |a: [f32; 4], b: [f32; 4]| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
    let sub = |a: [f32; 4], b: [f32; 4]| [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]];
    let mut planes = [add(w, x), sub(w, x), add(w, y), sub(w, y), z, sub(w, z)];
    for plane in planes.iter_mut() {
        let length = (plane[0] * plane[0] + plane[1] * plane[1] + plane[2] * plane[2]).sqrt().max(1e-20);
        for value in plane.iter_mut() {
            *value /= length;
        }
    }
    planes
}

pub fn sphere_visible(planes: &[[f32; 4]; 6], centre: [f32; 3], radius: f32) -> bool {
    planes.iter().all(|p| p[0] * centre[0] + p[1] * centre[1] + p[2] * centre[2] + p[3] >= -radius)
}
