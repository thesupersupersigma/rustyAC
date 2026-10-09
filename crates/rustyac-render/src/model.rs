// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! A kn5 model into the scene graph: `KN5IO::load` 0x1402151a0, `loadMaterialsBinary`
//! 0x140216240, `loadTexture` 0x1402171a0, `loadBinaryV2` 0x140215aa0,
//! `getSkinOverridenTexturePath` 0x140214f90. The file itself is read by `rustyac-content`.

use std::path::Path;

use rustyac_content::kn5::{Kn5, NodeClass};
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::graphics::{Graphics, BLEND_ALPHA, BLEND_ALPHA_TO_COVERAGE};
use crate::material::{Material, MaterialId};
use crate::scene::{MeshData, NodeId, Renderable, Scene, Sphere};
use crate::texture::Texture;

/// `KN5IO`: the importer, with the folders whose image files replace textures of the same
/// name (`addTextureFolder` 0x140214e90: a car's skin).
#[derive(Default)]
pub struct Kn5Io {
    pub skin_override_path: Vec<String>,
}

/// A path as text with forward slashes.
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `Path::getFileName` 0x140230910.
fn file_name(s: &str) -> &str {
    match s.rfind(['/', '\\']) {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// `Path::getPath` 0x140231bc0: without the last separator; the whole string when it has none.
pub fn get_path(s: &str) -> &str {
    match s.rfind(['/', '\\']) {
        Some(i) => &s[..i],
        None => s,
    }
}

impl Kn5Io {
    pub fn new() -> Kn5Io {
        Kn5Io::default()
    }

    /// `KN5IO::getSkinOverridenTexturePath` 0x140214f90.
    fn skin_overriden_texture_path(&self, name: &str) -> String {
        for folder in &self.skin_override_path {
            let p = format!("{folder}/{name}");
            if Path::new(&p).is_file() {
                println!("SKINNED TEXTURE {p} FOUND, OVERRIDING");
                return p;
            }
        }
        String::new()
    }

    /// The two tries of the loader: the file name of the texture, then the file name of what
    /// follows its last colon.
    fn skin_override(&self, name: &str) -> String {
        let first = self.skin_overriden_texture_path(file_name(name));
        if !first.is_empty() {
            return first;
        }
        let tail = match name.rfind(':') {
            Some(i) => &name[i + 1..],
            None => name,
        };
        self.skin_overriden_texture_path(file_name(tail))
    }

    /// `KN5IO::load` 0x1402151a0: the model's top node, not yet attached to anything.
    /// `filename` is the name the game would open the file by (`content/tracks/…/x.kn5`): its
    /// folder is part of the key under which the textures are shared; `file` is where the
    /// file is.
    pub fn load(&self, graphics: &mut Graphics, scene: &mut Scene, filename: &str, file: &Path) -> Result<NodeId, String> {
        println!("LOADING MODEL {filename}");
        let kn5 = Kn5::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut reader = kn5.reader().map_err(|e| format!("{}: {e}", file.display()))?;
        println!("VERSION={}", kn5.version);
        let dir = get_path(filename);

        // loadMaterialsBinary: the textures first (KN5IO::loadTexture, one per active entry)
        println!("Loading {} textures", kn5.textures.len());
        let mut textures: Vec<Texture> = Vec::new();
        for entry in &kn5.textures {
            let name = entry.name.display().into_owned();
            let ovr = self.skin_override(&name);
            let key = format!("{dir}::{name}");
            if entry.size == 0 {
                continue;
            }
            if ovr.is_empty() {
                if !graphics.resources.has_texture(&key) {
                    let bytes = reader.texture(entry).map_err(|e| format!("{}: {e}", file.display()))?;
                    textures.push(graphics.resources.get_texture_from_buffer(&graphics.kgl, &key, &bytes));
                } else {
                    textures.push(graphics.resources.get_texture_from_buffer(&graphics.kgl, &key, &[]));
                }
            } else {
                let mut t = graphics.resources.get_texture(&graphics.kgl, &ovr);
                t.file_name = key;
                textures.push(t);
            }
        }

        // the materials
        let first_material = scene.materials.len();
        for m in &kn5.materials {
            let mut mat = Material::new(&m.name.display());
            mat.set_shader(graphics, &m.shader.display())?;
            if m.alpha_blend_mode != 0 {
                mat.blend_mode = BLEND_ALPHA;
            }
            if m.alpha_tested {
                mat.blend_mode = BLEND_ALPHA_TO_COVERAGE;
            }
            if kn5.version >= 5 {
                mat.depth_mode = match m.depth_mode {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
            }
            for p in &m.properties {
                let name = p.name.display();
                let Some(index) = mat.get_var(&name) else {
                    println!("ERROR: shader variable {name} not found for material {}", mat.name);
                    continue;
                };
                mat.update_var(index, |v| v.f_value = p.value);
                mat.update_var(index, |v| v.f_value2 = p.value2);
                mat.update_var(index, |v| v.f_value3 = p.value3);
                mat.update_var(index, |v| v.f_value4 = p.value4);
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                if mat.vars[index].name == "ksSpecularEXP" && !(mat.vars[index].f_value >= 1.0) {
                    mat.update_var(index, |v| v.f_value = 1.0);
                }
            }
            for t in &m.textures {
                let slot_name = t.name.display();
                let Some(index) = mat.get_resource_index(&slot_name) else {
                    continue;
                };
                let tex_name = t.texture.display().into_owned();
                let key = format!("{dir}::{tex_name}");
                let ovr = self.skin_override(&tex_name);
                if ovr.is_empty() {
                    // MaterialList::getTextureFromName 0x14022b6c0, then the store
                    let mut texture = textures.iter().find(|x| x.file_name == key).cloned().unwrap_or_default();
                    if texture.kid.is_none() {
                        texture = graphics.resources.get_texture(&graphics.kgl, &key);
                    }
                    mat.resources[index].texture = texture;
                } else {
                    println!("Assign texture override for textureless kn5. RES: {}  FILENAME:{ovr}", mat.resources[index].name);
                    mat.resources[index].texture = graphics.resources.get_texture(&graphics.kgl, &ovr);
                }
                if mat.resources[index].texture.kid.is_none() {
                    println!("ERROR [{filename}] couldn't set texture {tex_name} for mat: {} res:{}", mat.name, mat.resources[index].name);
                }
            }
            scene.materials.push(mat);
        }
        let material_count = kn5.materials.len();

        // loadBinaryV2: the nodes, in file order (parents come before their children)
        let mut ids: Vec<NodeId> = Vec::with_capacity(kn5.nodes.len());
        for node in &kn5.nodes {
            let name = node.name.display().into_owned();
            let id = match (&node.class, &node.mesh) {
                (NodeClass::Mesh, Some(info)) => {
                    let mut renderable = Renderable::new();
                    renderable.cast_shadows = info.cast_shadows;
                    renderable.is_visible = info.is_visible;
                    renderable.is_transparent = info.is_transparent;
                    renderable.layer = info.layer as i32;
                    renderable.lod_in = info.lod_in;
                    renderable.lod_out = info.lod_out;
                    renderable.bounding_sphere = Sphere { center: Vec3f::new(info.bounding_centre[0], info.bounding_centre[1], info.bounding_centre[2]), radius: info.bounding_radius };
                    let vertices = if info.vertex_count != 0 { reader.vertex_bytes(info).map_err(|e| format!("{}: {e}", file.display()))? } else { Vec::new() };
                    let indices = if info.index_count != 0 { reader.indices(info).map_err(|e| format!("{}: {e}", file.display()))? } else { Vec::new() };
                    let mat_id = info.material_id as i32;
                    let material = if mat_id >= 0 && (mat_id as usize) < material_count {
                        Some(MaterialId((first_material + mat_id as usize) as u32))
                    } else {
                        println!("ERROR: matid={mat_id}");
                        None
                    };
                    let mesh = MeshData { vertices, indices, material, is_renderable: info.is_renderable, vb: None, ib: None, compiled_indices_count: 0, compiled_vertices_count: 0 };
                    scene.mesh(&name, renderable, mesh)
                }
                (NodeClass::SkinnedMesh, _) => {
                    // skinned meshes (the driver, a few flags) are drawn from Task 21 on
                    println!("NOTE: skinned mesh {name} of {filename} is not drawn yet");
                    let id = scene.node(&name);
                    scene.nodes[id].needs_matrix_ws = true;
                    id
                }
                _ => {
                    let id = scene.node(&name);
                    scene.nodes[id].matrix = Mat44f { m: node.matrix };
                    id
                }
            };
            scene.nodes[id].is_active = node.active;
            if let Some(parent) = node.parent {
                scene.add_child(ids[parent], id);
            }
            ids.push(id);
        }
        ids.first().copied().ok_or_else(|| format!("{}: no node in the model", file.display()))
    }
}
