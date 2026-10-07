// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `.kn5` model file.
//!
//! Layout (all little-endian; a string is an `i32` byte count followed by UTF-8 bytes):
//!
//! ```text
//! "sc6969"                          6 bytes
//! version            u32            5 or 6 in shipped content; above 5 one more u32 follows
//! texture count      i32
//!   active           i32            0: the record ends here (no name, no image)
//!   name             string
//!   size             u32            then `size` bytes: the image file (DDS, PNG ...) as it is
//! material count     i32
//!   name, shader     string, string
//!   alpha blend mode u8
//!   alpha tested     u8
//!   depth mode       i32            only above version 4
//!   property count   i32            name string, value f32, then 36 bytes (vec2, vec3, vec4)
//!   texture count    i32            slot name string (txDiffuse ...), slot i32, texture name string
//! node (recursive)
//!   class            i32            0 and 1 plain node, 2 mesh, 3 skinned mesh
//!   name             string
//!   child count      i32
//!   active           u8
//!   class 0:  nothing more
//!   class 1:  matrix 16 f32         rows; the translation is the fourth row
//!   class 2:  cast shadows, visible, transparent (u8 each); vertex count u32; vertices of
//!             44 bytes (position 3 f32, normal 3 f32, uv 2 f32, tangent 3 f32); index count
//!             u32; indices u16; material u32; layer u32; lod in f32; lod out f32; bounding
//!             sphere centre 3 f32 and radius f32; renderable u8
//!   class 3:  cast shadows, visible, transparent; bone count u32 (name string, matrix 16
//!             f32); vertex count u32; vertices of 76 bytes (as above, then 4 weights f32 and
//!             4 bone indices f32); index count u32; indices u16; material u32; layer u32;
//!             8 bytes
//!   then the children, in order
//! ```
//!
//! A node's world matrix is `own * parent` with row vectors (a point is `p * M`). Mesh nodes
//! have no matrix of their own. The game reads the format with `KN5IO::load` @ 0x1402151a0; it
//! never checks the magic, and version 1 files (32-bit indices) are not read here.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Bytes of one vertex of a mesh (class 2).
pub const VERTEX_STRIDE: u32 = 44;
/// Bytes of one vertex of a skinned mesh (class 3).
pub const SKINNED_VERTEX_STRIDE: u32 = 76;

/// A row-vector 4x4 matrix as stored in the file.
pub type Matrix = [[f32; 4]; 4];

pub const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `a * b` for row vectors: first `a`, then `b`.
pub fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
        }
    }
    out
}

/// One image stored in the file. The bytes stay on disk until [`Kn5Reader::texture`].
#[derive(Clone, Debug, PartialEq)]
pub struct TextureEntry {
    pub name: String,
    pub active: i32,
    /// Where the image file starts in the kn5.
    pub offset: u64,
    pub size: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShaderProperty {
    pub name: String,
    pub value: f32,
    pub value2: [f32; 2],
    pub value3: [f32; 3],
    pub value4: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureMapping {
    /// The shader's slot name: `txDiffuse`, `txNormal`, `txDetail` ...
    pub name: String,
    pub slot: i32,
    /// Name of a [`TextureEntry`] (of this file, or of another model of the same track).
    pub texture: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub shader: String,
    /// 0 opaque, 1 alpha blend, 2 alpha to coverage
    pub alpha_blend_mode: u8,
    pub alpha_tested: bool,
    /// 0 normal, 1 no depth write, 2 no depth test
    pub depth_mode: i32,
    pub properties: Vec<ShaderProperty>,
    pub textures: Vec<TextureMapping>,
}

impl Material {
    /// The texture in the slot named `txDiffuse`.
    pub fn diffuse(&self) -> Option<&str> {
        self.textures.iter().find(|t| t.name == "txDiffuse").map(|t| t.texture.as_str())
    }

    pub fn property(&self, name: &str) -> Option<&ShaderProperty> {
        self.properties.iter().find(|p| p.name == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeClass {
    /// 1: a transform with children
    Base,
    /// 2: a static mesh
    Mesh,
    /// 3: a mesh with bones
    SkinnedMesh,
}

/// The mesh of a node. Vertices and indices stay on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshInfo {
    pub cast_shadows: bool,
    pub is_visible: bool,
    pub is_transparent: bool,
    pub vertex_count: u32,
    pub vertex_offset: u64,
    /// 44 for a mesh, 76 for a skinned mesh; the first 44 bytes are the same.
    pub vertex_stride: u32,
    pub index_count: u32,
    pub index_offset: u64,
    pub material_id: u32,
    pub layer: u32,
    pub lod_in: f32,
    pub lod_out: f32,
    pub bounding_centre: [f32; 3],
    pub bounding_radius: f32,
    pub is_renderable: bool,
    /// Skinned meshes only: the bones (name, matrix).
    pub bones: Vec<(String, Matrix)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub class: NodeClass,
    pub name: String,
    pub active: bool,
    pub parent: Option<usize>,
    /// Indices into [`Kn5::nodes`], in file order.
    pub children: Vec<usize>,
    /// The node's own matrix; the identity for meshes.
    pub matrix: Matrix,
    pub mesh: Option<MeshInfo>,
}

/// One vertex as drawn: the first 32 bytes of the stored 44.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// The structure of a kn5 file. `nodes[0]` is the root; the list is in file order, which is
/// depth-first with every node before its children.
#[derive(Clone, Debug, PartialEq)]
pub struct Kn5 {
    pub path: PathBuf,
    pub version: u32,
    pub textures: Vec<TextureEntry>,
    pub materials: Vec<Material>,
    pub nodes: Vec<Node>,
}

struct Scanner {
    file: BufReader<File>,
    position: u64,
    length: u64,
}

impl Scanner {
    fn bytes<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut b = [0u8; N];
        self.file.read_exact(&mut b)?;
        self.position += N as u64;
        Ok(b)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.bytes::<1>()?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.bytes()?))
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.bytes()?))
    }

    fn count(&mut self, what: &str) -> io::Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as u64 > self.length {
            return Err(bad(format!("{what} count {n} at byte {}", self.position - 4)));
        }
        Ok(n as usize)
    }

    fn string(&mut self) -> io::Result<String> {
        let n = self.count("string byte")?;
        let mut b = vec![0u8; n];
        self.file.read_exact(&mut b)?;
        self.position += n as u64;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }

    fn matrix(&mut self) -> io::Result<Matrix> {
        let mut m = IDENTITY;
        for row in m.iter_mut() {
            for cell in row.iter_mut() {
                *cell = self.f32()?;
            }
        }
        Ok(m)
    }

    fn skip(&mut self, n: u64) -> io::Result<()> {
        if self.position + n > self.length {
            return Err(bad(format!("a block of {n} bytes at byte {} runs past the end of the file", self.position)));
        }
        self.file.seek_relative(n as i64)?;
        self.position += n;
        Ok(())
    }
}

fn bad(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Kn5 {
    /// Reads the structure of a kn5: a few hundred kilobytes even for a 400 MB track.
    pub fn open(path: &Path) -> io::Result<Kn5> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut s = Scanner { file: BufReader::with_capacity(1 << 16, file), position: 0, length };
        if &s.bytes::<6>()? != b"sc6969" {
            return Err(bad(format!("{} is not a kn5 file", path.display())));
        }
        let version = s.u32()?;
        if version == 1 {
            return Err(bad(format!("{} is a version 1 kn5 (32-bit indices), which is not read", path.display())));
        }
        if version > 5 {
            // a key: 0 in ordinary content
            s.u32()?;
        }

        let mut textures = Vec::new();
        for _ in 0..s.count("texture")? {
            let active = s.i32()?;
            if active == 0 {
                continue;
            }
            let name = s.string()?;
            let size = s.u32()?;
            textures.push(TextureEntry { name, active, offset: s.position, size });
            s.skip(size as u64)?;
        }

        let mut materials = Vec::new();
        for _ in 0..s.count("material")? {
            let name = s.string()?;
            let shader = s.string()?;
            let alpha_blend_mode = s.u8()?;
            let alpha_tested = s.u8()? != 0;
            let depth_mode = if version > 4 { s.i32()? } else { 0 };
            let mut properties = Vec::new();
            for _ in 0..s.count("shader property")? {
                let name = s.string()?;
                let value = s.f32()?;
                let value2 = [s.f32()?, s.f32()?];
                let value3 = [s.f32()?, s.f32()?, s.f32()?];
                let value4 = [s.f32()?, s.f32()?, s.f32()?, s.f32()?];
                properties.push(ShaderProperty { name, value, value2, value3, value4 });
            }
            let mut mappings = Vec::new();
            for _ in 0..s.count("texture slot")? {
                let name = s.string()?;
                let slot = s.i32()?;
                let texture = s.string()?;
                mappings.push(TextureMapping { name, slot, texture });
            }
            materials.push(Material { name, shader, alpha_blend_mode, alpha_tested, depth_mode, properties, textures: mappings });
        }

        let mut nodes = Vec::new();
        // (parent, children still to read); a loop, so a deep tree cannot overflow the stack
        let mut pending: Vec<(Option<usize>, usize)> = vec![(None, 1)];
        while let Some((parent, left)) = pending.pop() {
            if left == 0 {
                continue;
            }
            pending.push((parent, left - 1));
            let class_number = s.i32()?;
            let class = match class_number {
                0 | 1 => NodeClass::Base,
                2 => NodeClass::Mesh,
                3 => NodeClass::SkinnedMesh,
                other => return Err(bad(format!("node class {other} at byte {}", s.position - 4))),
            };
            let name = s.string()?;
            let child_count = s.count("child")?;
            let active = s.u8()? != 0;
            let mut matrix = IDENTITY;
            let mut mesh = None;
            match class {
                NodeClass::Base => {
                    if class_number == 1 {
                        matrix = s.matrix()?;
                    }
                }
                NodeClass::Mesh | NodeClass::SkinnedMesh => {
                    let cast_shadows = s.u8()? != 0;
                    let is_visible = s.u8()? != 0;
                    let is_transparent = s.u8()? != 0;
                    let skinned = class == NodeClass::SkinnedMesh;
                    let mut bones = Vec::new();
                    if skinned {
                        for _ in 0..s.count("bone")? {
                            let name = s.string()?;
                            bones.push((name, s.matrix()?));
                        }
                    }
                    let vertex_stride = if skinned { SKINNED_VERTEX_STRIDE } else { VERTEX_STRIDE };
                    let vertex_count = s.u32()?;
                    let vertex_offset = s.position;
                    s.skip(vertex_count as u64 * vertex_stride as u64)?;
                    let index_count = s.u32()?;
                    let index_offset = s.position;
                    s.skip(index_count as u64 * 2)?;
                    let material_id = s.u32()?;
                    let layer = s.u32()?;
                    let lod_in = s.f32()?;
                    let lod_out = s.f32()?;
                    let (bounding_centre, bounding_radius, is_renderable) = if skinned {
                        ([0.0; 3], 0.0, true)
                    } else {
                        ([s.f32()?, s.f32()?, s.f32()?], s.f32()?, s.u8()? != 0)
                    };
                    mesh = Some(MeshInfo {
                        cast_shadows,
                        is_visible,
                        is_transparent,
                        vertex_count,
                        vertex_offset,
                        vertex_stride,
                        index_count,
                        index_offset,
                        material_id,
                        layer,
                        lod_in,
                        lod_out,
                        bounding_centre,
                        bounding_radius,
                        is_renderable,
                        bones,
                    });
                }
            }
            let index = nodes.len();
            nodes.push(Node { class, name, active, parent, children: Vec::with_capacity(child_count), matrix, mesh });
            if let Some(parent) = parent {
                nodes[parent].children.push(index);
            }
            pending.push((Some(index), child_count));
        }
        if s.position != length {
            return Err(bad(format!("{}: {} bytes left after the last node", path.display(), length - s.position)));
        }
        Ok(Kn5 { path: path.to_path_buf(), version, textures, materials, nodes })
    }

    /// Opens the file again for the big blocks.
    pub fn reader(&self) -> io::Result<Kn5Reader> {
        Ok(Kn5Reader { file: File::open(&self.path)?, scratch: Vec::new() })
    }

    /// The node's matrix in the model: its own and those of all its parents.
    pub fn world_matrix(&self, node: usize) -> Matrix {
        let mut m = self.nodes[node].matrix;
        let mut at = self.nodes[node].parent;
        while let Some(parent) = at {
            m = mul(&m, &self.nodes[parent].matrix);
            at = self.nodes[parent].parent;
        }
        m
    }

    /// First node with this name, in file order.
    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    pub fn texture(&self, name: &str) -> Option<&TextureEntry> {
        self.textures.iter().find(|t| t.name == name)
    }
}

/// Fetches the big blocks of one kn5.
pub struct Kn5Reader {
    file: File,
    scratch: Vec<u8>,
}

impl Kn5Reader {
    fn block(&mut self, offset: u64, size: usize) -> io::Result<&[u8]> {
        self.scratch.resize(size, 0);
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut self.scratch)?;
        Ok(&self.scratch)
    }

    /// The image file of a texture, as stored (usually DDS).
    pub fn texture(&mut self, texture: &TextureEntry) -> io::Result<Vec<u8>> {
        Ok(self.block(texture.offset, texture.size as usize)?.to_vec())
    }

    /// The first `size` bytes of a texture's image file (its header), or all of a shorter one.
    pub fn texture_head(&mut self, texture: &TextureEntry, size: usize) -> io::Result<Vec<u8>> {
        Ok(self.block(texture.offset, size.min(texture.size as usize))?.to_vec())
    }

    /// The positions of a mesh's vertices: exactly the floats of the file.
    pub fn positions(&mut self, mesh: &MeshInfo) -> io::Result<Vec<[f32; 3]>> {
        let stride = mesh.vertex_stride as usize;
        let block = self.block(mesh.vertex_offset, mesh.vertex_count as usize * stride)?;
        Ok(block.chunks_exact(stride).map(|v| [f32_at(v, 0), f32_at(v, 4), f32_at(v, 8)]).collect())
    }

    /// Position, normal and texture coordinate of every vertex.
    pub fn vertices(&mut self, mesh: &MeshInfo) -> io::Result<Vec<Vertex>> {
        let stride = mesh.vertex_stride as usize;
        let block = self.block(mesh.vertex_offset, mesh.vertex_count as usize * stride)?;
        Ok(block
            .chunks_exact(stride)
            .map(|v| Vertex {
                position: [f32_at(v, 0), f32_at(v, 4), f32_at(v, 8)],
                normal: [f32_at(v, 12), f32_at(v, 16), f32_at(v, 20)],
                uv: [f32_at(v, 24), f32_at(v, 28)],
            })
            .collect())
    }

    pub fn indices(&mut self, mesh: &MeshInfo) -> io::Result<Vec<u16>> {
        let block = self.block(mesh.index_offset, mesh.index_count as usize * 2)?;
        Ok(block.chunks_exact(2).map(|i| u16::from_le_bytes([i[0], i[1]])).collect())
    }
}

fn f32_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
