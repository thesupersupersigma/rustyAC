// SPDX-License-Identifier: GPL-3.0-or-later

//! kn5 models on the graphics card, for the debug view: every drawn mesh with its diffuse
//! texture, nothing else of AC's materials. (One exception: where a material says
//! `useDetail`, the diffuse texture is only shading and panel lines and the colour is in the
//! detail texture, so that one is multiplied in, as AC does; a car would be grey without.)
//! A track is loaded once and never moves; a car's nodes are moved every frame (body,
//! wheels, hubs, steering wheel).
//!
//! Kept light: textures are capped in size (the largest mip levels of a chain are left on
//! disk) and halved again until the whole set fits the budget; vertices and textures go
//! straight from the file to the card, one mesh at a time; when drawing, meshes outside the
//! picture, outside their own LOD range or too small to see are skipped.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use rustyac_content::kn5::{Kn5, Kn5Reader, NodeClass};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R16_UINT, DXGI_SAMPLE_DESC};

use super::dds::{self, Image};
use super::scene::mul;
use crate::view::{Mat, IDENTITY};

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
    pub load_seconds: f64,
    /// Problems worth a line (unreadable textures ...), at most a handful.
    pub notes: Vec<String>,
}

pub struct GpuMesh {
    pub vertices: ID3D11Buffer,
    pub indices: ID3D11Buffer,
    pub index_count: u32,
    pub material: usize,
    pub node: usize,
    /// Bounding sphere in the mesh's own space.
    pub centre: [f32; 3],
    pub radius: f32,
    pub lod_in: f32,
    pub lod_out: f32,
    /// Drawn after the opaque meshes, blended.
    pub blended: bool,
}

pub struct GpuMaterial {
    pub texture: Option<ID3D11ShaderResourceView>,
    /// The detail texture and how often it repeats over the diffuse one (`useDetail`).
    pub detail: Option<(ID3D11ShaderResourceView, f32)>,
    pub color: [f32; 4],
    /// Pixels with less alpha are not drawn (0 = all are).
    pub alpha_ref: f32,
    pub blend: bool,
    /// Lit from both sides the same (leaves, fences).
    pub foliage: bool,
}

pub struct GpuNode {
    pub name: String,
    pub parent: Option<usize>,
    pub local: Mat,
    pub active: bool,
}

pub struct GpuModel {
    pub nodes: Vec<GpuNode>,
    /// Every node's world matrix; [`GpuModel::update`] fills it.
    pub world: Vec<Mat>,
    pub meshes: Vec<GpuMesh>,
    pub materials: Vec<GpuMaterial>,
    pub stats: ModelStats,
}

/// How to load a set of models.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOptions {
    /// Longest texture side kept, pixels (0 = as stored).
    pub texture_size: u32,
    /// Megabytes of texture the set may take; the size cap is halved until it fits.
    pub texture_budget_mb: u32,
    /// Draw no textures at all: one colour per material.
    pub flat: bool,
}

impl Default for ModelOptions {
    fn default() -> ModelOptions {
        ModelOptions { texture_size: 1024, texture_budget_mb: 1200, flat: false }
    }
}

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn buffer<T: Copy>(device: &ID3D11Device, data: &[T], bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer, String> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: std::mem::size_of_val(data) as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: bind.0 as u32, ..Default::default() };
    let initial = D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr() as *const _, ..Default::default() };
    let mut out = None;
    // SAFETY: `data` outlives the call, which copies it.
    unsafe { device.CreateBuffer(&desc, Some(&initial), Some(&mut out)).map_err(err("model buffer"))? };
    out.ok_or("no buffer".to_string())
}

fn upload(device: &ID3D11Device, context: &ID3D11DeviceContext, image: &Image) -> Result<ID3D11ShaderResourceView, String> {
    let top = &image.levels[0];
    let generate = image.wants_generated_mips();
    let desc = D3D11_TEXTURE2D_DESC {
        Width: top.width,
        Height: top.height,
        MipLevels: if generate { 0 } else { image.levels.len() as u32 },
        ArraySize: 1,
        Format: DXGI_FORMAT(image.format as i32),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: if generate { D3D11_USAGE_DEFAULT } else { D3D11_USAGE_IMMUTABLE },
        BindFlags: if generate { (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32 } else { D3D11_BIND_SHADER_RESOURCE.0 as u32 },
        CPUAccessFlags: 0,
        MiscFlags: if generate { D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32 } else { 0 },
    };
    let initial: Vec<D3D11_SUBRESOURCE_DATA> = image
        .levels
        .iter()
        .map(|level| D3D11_SUBRESOURCE_DATA { pSysMem: image.data[level.bytes.clone()].as_ptr() as *const _, SysMemPitch: level.pitch, SysMemSlicePitch: 0 })
        .collect();
    let (mut texture, mut view) = (None, None);
    // SAFETY: the descriptors describe `image.data`, which outlives the calls (they copy it).
    unsafe {
        if generate {
            device.CreateTexture2D(&desc, None, Some(&mut texture)).map_err(err("texture"))?;
            let texture = texture.as_ref().ok_or("no texture")?;
            context.UpdateSubresource(texture, 0, None, image.data[top.bytes.clone()].as_ptr() as *const _, top.pitch, 0);
            device.CreateShaderResourceView(texture, None, Some(&mut view)).map_err(err("texture view"))?;
            context.GenerateMips(view.as_ref().ok_or("no texture view")?);
        } else {
            device.CreateTexture2D(&desc, Some(initial.as_ptr()), Some(&mut texture)).map_err(err("texture"))?;
            device.CreateShaderResourceView(texture.as_ref().ok_or("no texture")?, None, Some(&mut view)).map_err(err("texture view"))?;
        }
    }
    view.ok_or("no texture view".to_string())
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

impl GpuModel {
    /// Loads kn5 files as one model. Each file's root hangs under the model's root with
    /// `placements[i]` (the identity where the list is shorter).
    pub fn load(device: &ID3D11Device, context: &ID3D11DeviceContext, files: &[PathBuf], placements: &[Mat], options: &ModelOptions) -> Result<GpuModel, String> {
        let start = Instant::now();
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
                // nothing to draw it with
                return false;
            }
            // a car's cracked glass is only shown after a crash
            let hidden = kn5.materials.get(mesh.material_id as usize).is_some_and(|m| m.shader == "ksBrokenGlass");
            !hidden && mesh.is_renderable && mesh.is_visible && mesh.index_count >= 3 && mesh.vertex_count > 0
        };
        // a texture is looked for in its own file first, then in the others (a track's models
        // share them)
        let mut where_is: HashMap<&str, (usize, usize)> = HashMap::new();
        for (file, kn5) in kn5s.iter().enumerate() {
            for (index, texture) in kn5.textures.iter().enumerate() {
                where_is.entry(texture.name.as_str()).or_insert((file, index));
            }
        }
        let locate = |file: usize, name: &str| -> Option<(usize, usize)> {
            kn5s[file].textures.iter().position(|t| t.name == name).map(|i| (file, i)).or_else(|| where_is.get(name).copied())
        };
        // the detail texture of a material that multiplies one in
        let detail_of = |material: &rustyac_content::Material| -> Option<(String, f32)> {
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
                let names = [material.diffuse().map(str::to_string), detail_of(material).map(|d| d.0)];
                for name in names.into_iter().flatten() {
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
        let mut views: HashMap<(usize, usize), ID3D11ShaderResourceView> = HashMap::new();
        if !options.flat {
            loop {
                let mut total = 0u64;
                for &(file, index) in &wanted {
                    let entry = &kn5s[file].textures[index];
                    let head = readers[file].texture_head(entry, 148).map_err(|e| e.to_string())?;
                    if let Ok(plan) = dds::plan_dds(&head, entry.size as usize, cap) {
                        total += plan.bytes() as u64;
                    }
                }
                if total <= options.texture_budget_mb as u64 * 1_048_576 || (cap != 0 && cap <= 64) {
                    break;
                }
                cap = if cap == 0 { 2048 } else { cap / 2 };
            }
            // one at a time from the file to the card
            for &(file, index) in &wanted {
                let entry = &kn5s[file].textures[index];
                let bytes = readers[file].texture(entry).map_err(|e| e.to_string())?;
                match dds::parse_image(&bytes, cap).and_then(|image| upload(device, context, &image).map(|view| (view, image.bytes()))) {
                    Ok((view, bytes)) => {
                        stats.textures += 1;
                        stats.texture_bytes += bytes as u64;
                        views.insert((file, index), view);
                    }
                    Err(e) => {
                        if stats.notes.len() < 6 {
                            stats.notes.push(format!("texture {}: {e}", entry.name));
                        }
                    }
                }
            }
        }
        stats.texture_size = cap;

        let mut model = GpuModel { nodes: Vec::new(), world: Vec::new(), meshes: Vec::new(), materials: Vec::new(), stats: ModelStats::default() };
        model.nodes.push(GpuNode { name: String::new(), parent: None, local: IDENTITY, active: true });
        for (file, kn5) in kn5s.iter().enumerate() {
            let node_base = model.nodes.len();
            let material_base = model.materials.len();
            for material in &kn5.materials {
                let diffuse = material.diffuse().unwrap_or("");
                let texture = locate(file, diffuse).and_then(|key| views.get(&key).cloned());
                let shader = material.shader.as_str();
                let foliage = shader.contains("Tree") || shader.contains("Grass") || material.alpha_tested;
                let mut color = if texture.is_some() { [1.0; 4] } else { flat_color(&material.name, shader, diffuse) };
                if texture.is_none() {
                    stats.flat_materials += 1;
                    if material.alpha_blend_mode != 0 && color[3] == 1.0 {
                        color[3] = 0.6;
                    }
                }
                let detail = detail_of(material).and_then(|(name, repeat)| Some((locate(file, &name).and_then(|key| views.get(&key).cloned())?, repeat)));
                model.materials.push(GpuMaterial {
                    texture,
                    detail,
                    color,
                    alpha_ref: if material.alpha_tested { 0.5 } else { 0.0 },
                    blend: material.alpha_blend_mode == 1,
                    foliage,
                });
            }
            for (index, node) in kn5.nodes.iter().enumerate() {
                let (parent, local) = match node.parent {
                    Some(parent) => (Some(node_base + parent), node.matrix),
                    None => (Some(0), mul(&node.matrix, placements.get(file).unwrap_or(&IDENTITY))),
                };
                model.nodes.push(GpuNode { name: node.name.clone(), parent, local, active: node.active });
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
                let material = material_base + (mesh.material_id as usize).min(kn5.materials.len().saturating_sub(1));
                stats.meshes += 1;
                stats.triangles += indices.len() as u64 / 3;
                stats.geometry_bytes += (vertices.len() * 32 + indices.len() * 2) as u64;
                model.meshes.push(GpuMesh {
                    vertices: buffer(device, &vertices, D3D11_BIND_VERTEX_BUFFER)?,
                    indices: buffer(device, &indices, D3D11_BIND_INDEX_BUFFER)?,
                    index_count: indices.len() as u32 / 3 * 3,
                    material,
                    node: node_base + index,
                    centre: mesh.bounding_centre,
                    radius: mesh.bounding_radius,
                    lod_in: mesh.lod_in,
                    lod_out: mesh.lod_out,
                    blended: mesh.is_transparent || model.materials.get(material).is_some_and(|m| m.blend),
                });
            }
        }
        readers.clear();
        stats.load_seconds = start.elapsed().as_secs_f64();
        model.stats = stats;
        model.world = vec![IDENTITY; model.nodes.len()];
        model.update(&IDENTITY, &[]);
        Ok(model)
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

pub const INDEX_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16_UINT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::scene::{perspective, view_matrix};

    #[test]
    fn the_frustum_keeps_what_is_ahead_and_drops_what_is_behind() {
        // a camera at the origin looking along -z
        let view_proj = mul(&view_matrix(&IDENTITY), &perspective(60.0, 16.0 / 9.0, 1.0, 1000.0));
        let planes = frustum(&view_proj);
        assert!(sphere_visible(&planes, [0.0, 0.0, -50.0], 1.0));
        assert!(!sphere_visible(&planes, [0.0, 0.0, 50.0], 1.0));
        assert!(!sphere_visible(&planes, [500.0, 0.0, -50.0], 1.0));
        assert!(sphere_visible(&planes, [500.0, 0.0, -50.0], 480.0));
        assert!(!sphere_visible(&planes, [0.0, 0.0, -2000.0], 10.0));
    }

    #[test]
    fn names_pick_a_flat_colour() {
        assert_eq!(flat_color("grass_ext", "ksGrass", ""), [0.24, 0.38, 0.16, 1.0]);
        assert_eq!(flat_color("m", "ksPerPixel", "asph_spa.dds")[0], 0.27);
        assert_eq!(flat_color("x", "y", "z")[0], 0.52);
    }
}
