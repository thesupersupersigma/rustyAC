// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The game's own compiled shaders, read from the player's install at run time:
//! `ShaderManager::getShader` 0x140227b40, `KGLShader::loadShaderBinary` 0x14001f0c0,
//! `KGLShader::reflectVars` 0x14001f5f0, `getInputLayout` 0x14001eb00, `Shader::reflectVars`
//! 0x14021a550, `CBuffer`, `ShaderVariable`. No shader is compiled, rewritten or translated.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D::D3D_SIT_TEXTURE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::kgl::{Kgl, KglCBuffer};

/// `CBuffer` (0x28 bytes): a constant buffer with its CPU copy and a dirty flag.
pub struct CBuffer {
    pub size: i32,
    pub slot: i32,
    pub is_system: bool,
    pub kid: Option<KglCBuffer>,
    pub data: Vec<u8>,
    pub touched: bool,
}

impl CBuffer {
    /// `CBuffer::CBuffer()` 0x140219900.
    pub fn empty() -> CBuffer {
        CBuffer { size: 0, slot: 0, is_system: false, kid: None, data: Vec::new(), touched: false }
    }

    /// `CBuffer::init` 0x140219990.
    pub fn init(kgl: &Kgl, slot: i32, size: i32) -> CBuffer {
        CBuffer { size, slot, is_system: false, kid: Some(kgl.create_cbuffer(size)), data: vec![0; size as usize], touched: true }
    }

    /// `CBuffer::set` 0x140219a50: no compare, any write marks the buffer.
    pub fn set(&mut self, src: &[u8], offset: usize) {
        self.data[offset..offset + src.len()].copy_from_slice(src);
        self.touched = true;
    }

    pub fn set_f32(&mut self, value: f32, offset: usize) {
        self.set(&value.to_le_bytes(), offset);
    }

    pub fn set_f32s(&mut self, values: &[f32], offset: usize) {
        for (i, v) in values.iter().enumerate() {
            self.data[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.touched = true;
    }

    /// `CBuffer::touch` 0x140219a80.
    pub fn touch(&mut self) {
        self.touched = true;
    }

    /// `CBuffer::commit` 0x140219920.
    pub fn commit(&mut self, kgl: &Kgl) {
        if let Some(kid) = &self.kid {
            if self.touched {
                kgl.cbuffer_map(kid, &self.data);
                kgl.cbuffer_bind(kid, self.slot);
            } else if !self.is_system {
                kgl.cbuffer_bind(kid, self.slot);
            }
        }
        self.touched = false;
    }
}

/// What kgl's reflection records of one constant buffer.
#[derive(Clone, Debug)]
pub struct KglShaderCBuffer {
    pub name: String,
    pub size: u32,
    pub slot: u32,
}

/// What kgl's reflection records of one variable.
#[derive(Clone, Debug)]
pub struct KglShaderVar {
    pub name: String,
    pub cbuffer_name: String,
    pub cbuffer_slot: u32,
    pub size: u32,
    pub offset: u32,
}

#[derive(Clone, Debug)]
pub struct KglShaderTexture {
    pub name: String,
    pub slot: u32,
}

/// `KGLShader` (0x90 bytes).
pub struct KglShader {
    pub vs: Option<ID3D11VertexShader>,
    pub ps: Option<ID3D11PixelShader>,
    pub input_layout: Option<ID3D11InputLayout>,
    pub is_alpha_tested: bool,
    pub il_type: i32,
    pub cbuffers: Vec<KglShaderCBuffer>,
    pub vars: Vec<KglShaderVar>,
    pub textures: Vec<KglShaderTexture>,
}

/// `eVariableType`, from the byte size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableType {
    Float,
    Float2,
    Float3,
    Float4,
    Matrix,
    Other,
}

/// `ShaderVariable` (0x40 bytes). `buffer` is an index into the owner's list of constant
/// buffers (the shader's, or a material's own copies).
#[derive(Clone, Debug)]
pub struct ShaderVariable {
    pub name: String,
    pub kind: VariableType,
    pub size: i32,
    pub offset: i32,
    pub buffer: Option<usize>,
}

impl ShaderVariable {
    /// `ShaderVariable::ShaderVariable` 0x140209460.
    pub fn new(name: &str, buffer: Option<usize>, offset: i32, size: i32) -> ShaderVariable {
        let kind = match size {
            4 => VariableType::Float,
            8 => VariableType::Float2,
            12 => VariableType::Float3,
            16 => VariableType::Float4,
            64 => VariableType::Matrix,
            _ => {
                if name != "bones" {
                    println!("ERROR: Variable type cannot be found");
                }
                VariableType::Other
            }
        };
        ShaderVariable { name: name.to_string(), kind, size, offset, buffer }
    }
}

/// `ShaderResource` (0x30 bytes): a texture register a material may fill.
#[derive(Clone, Debug)]
pub struct ShaderResource {
    pub name: String,
    pub slot: i32,
}

/// `Shader` (0xc8 bytes).
pub struct Shader {
    pub name: String,
    pub vars: Vec<ShaderVariable>,
    pub resources: Vec<ShaderResource>,
    /// the shader's own constant buffers (materials work on copies of their own)
    pub cbuffers: Vec<CBuffer>,
    pub is_alpha_tested: bool,
    pub il_type: i32,
    pub guid: i32,
    pub kid: KglShader,
}

impl Shader {
    /// `Shader::getVar` 0x14021a3c0.
    pub fn get_var(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }

    /// Writes one of the shader's own variables (`ShaderVariable::set` 0x140209600).
    pub fn set_var(&mut self, index: usize, bytes: &[u8]) {
        let var = &self.vars[index];
        if let Some(buffer) = var.buffer {
            let (offset, size) = (var.offset as usize, var.size as usize);
            self.cbuffers[buffer].set(&bytes[..size], offset);
        }
    }
}

/// An index into [`ShaderManager::shaders`]: what the game's `Shader*` is to its caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShaderId(pub usize);

/// `ShaderManager` (0x28 bytes), with kgl's process-wide map of input layouts.
pub struct ShaderManager {
    /// `<game folder>/system/shaders/win`
    folder: PathBuf,
    pub shaders: Vec<Shader>,
    /// `inputLayouts` 0x141560f30: one layout per vertex kind for the whole process
    input_layouts: BTreeMap<i32, Option<ID3D11InputLayout>>,
    /// `Shader::guidCount`
    guid_count: i32,
}

/// The four constant buffers `GraphicsManager::getCBuffer` 0x1402028f0 knows by name.
pub fn is_system_cbuffer(name: &str) -> bool {
    matches!(name, "cbCamera" | "cbLighting" | "cbShadowMaps" | "cbPerObject")
}

fn element(name: PCSTR, index: u32, format: DXGI_FORMAT, offset: u32) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC { SemanticName: name, SemanticIndex: index, Format: format, InputSlot: 0, AlignedByteOffset: offset, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, InstanceDataStepRate: 0 }
}

unsafe fn text(p: PCSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // `string2wstring` 0x14001eaa0 widens byte by byte
    p.as_bytes().iter().map(|b| *b as char).collect()
}

impl ShaderManager {
    pub fn new(game_folder: &Path) -> ShaderManager {
        ShaderManager { folder: game_folder.join("system/shaders/win"), shaders: Vec::new(), input_layouts: BTreeMap::new(), guid_count: 0 }
    }

    pub fn get(&self, id: ShaderId) -> &Shader {
        &self.shaders[id.0]
    }

    pub fn get_mut(&mut self, id: ShaderId) -> &mut Shader {
        &mut self.shaders[id.0]
    }

    /// `ShaderManager::getShader` 0x140227b40. The game ends the process when a shader is
    /// missing (`GraphicsManager::getShader` 0x140202c60); here that is an error.
    pub fn get_shader(&mut self, kgl: &Kgl, name: &str) -> Result<ShaderId, String> {
        if let Some(found) = self.shaders.iter().position(|s| s.name == name) {
            return Ok(ShaderId(found));
        }
        let ps = self.folder.join(format!("{name}_ps.fxo"));
        let vs = self.folder.join(format!("{name}_vs.fxo"));
        if !(ps.is_file() && vs.is_file()) {
            println!("ERROR: Shader {name} NOT FOUND, RETURNING NULL");
            return Err(format!("the shader {name} is not in {}", self.folder.display()));
        }
        let kid = self.load_shader_binary(kgl, name, &vs, &ps)?;
        // Shader::Shader 0x140219de0, Shader::initShaderBinary 0x14021a490
        self.guid_count += 1;
        let mut shader = Shader { name: name.to_string(), vars: Vec::new(), resources: Vec::new(), cbuffers: Vec::new(), is_alpha_tested: kid.is_alpha_tested, il_type: kid.il_type, guid: self.guid_count, kid };
        // Shader::reflectVars 0x14021a550
        for d in &shader.kid.cbuffers {
            if !is_system_cbuffer(&d.name) {
                shader.cbuffers.push(CBuffer::init(kgl, d.slot as i32, d.size as i32));
            }
        }
        for d in &shader.kid.vars {
            if is_system_cbuffer(&d.cbuffer_name) {
                continue;
            }
            let buffer = shader.cbuffers.iter().position(|c| c.slot == d.cbuffer_slot as i32);
            if shader.vars.iter().any(|v| v.name == d.name) {
                continue;
            }
            shader.vars.push(ShaderVariable::new(&d.name, buffer, d.offset as i32, d.size as i32));
        }
        for d in &shader.kid.textures {
            shader.resources.push(ShaderResource { name: d.name.clone(), slot: d.slot as i32 });
        }
        self.shaders.push(shader);
        Ok(ShaderId(self.shaders.len() - 1))
    }

    /// `KGLShader::loadShaderBinary` 0x14001f0c0.
    fn load_shader_binary(&mut self, kgl: &Kgl, name: &str, vs_path: &Path, ps_path: &Path) -> Result<KglShader, String> {
        let meta_path = self.folder.join(format!("{name}_meta.ini"));
        let meta = IniReader::load(&meta_path).ok();
        if meta.is_none() {
            println!("ERROR: Cannot find shader meta {}", meta_path.display());
        }
        let flag = |key: &str| meta.as_ref().and_then(|m| m.get_int("METADATA", key).ok()).unwrap_or(0) != 0;
        let is_alpha_tested = flag("ALPHATEST");
        let il_type = if flag("PARTICLE") {
            2
        } else if flag("SKINNED") {
            1
        } else {
            0
        };
        let mut shader = KglShader { vs: None, ps: None, input_layout: None, is_alpha_tested, il_type, cbuffers: Vec::new(), vars: Vec::new(), textures: Vec::new() };

        // createVertexShader 0x14001f490: reflect, create, then the input layout
        let blob = std::fs::read(vs_path).map_err(|e| format!("{}: {e}", vs_path.display()))?;
        unsafe {
            reflect_vars(&mut shader, &blob)?;
            if kgl.device.CreateVertexShader(&blob, None, Some(&mut shader.vs)).is_err() {
                println!("ERROR: CreateVertexShader failed ({name})");
            } else {
                shader.input_layout = self.input_layout(kgl, &blob, il_type);
            }
        }
        // createPixelShader 0x14001f550
        let blob = std::fs::read(ps_path).map_err(|e| format!("{}: {e}", ps_path.display()))?;
        unsafe {
            reflect_vars(&mut shader, &blob)?;
            if kgl.device.CreatePixelShader(&blob, None, Some(&mut shader.ps)).is_err() {
                println!("ERROR: CreatePixelShader failed ({name})");
            }
        }
        Ok(shader)
    }

    /// `getInputLayout` 0x14001eb00: made once per vertex kind, with the bytecode of the first
    /// vertex shader of that kind.
    unsafe fn input_layout(&mut self, kgl: &Kgl, bytecode: &[u8], il_type: i32) -> Option<ID3D11InputLayout> {
        if let Some(layout) = self.input_layouts.get(&il_type) {
            return layout.clone();
        }
        let position = PCSTR(c"POSITION".as_ptr().cast());
        let normal = PCSTR(c"NORMAL".as_ptr().cast());
        let texcoord = PCSTR(c"TEXCOORD".as_ptr().cast());
        let tangent = PCSTR(c"TANGENT".as_ptr().cast());
        let color = PCSTR(c"COLOR".as_ptr().cast());
        let elements: Vec<D3D11_INPUT_ELEMENT_DESC> = match il_type {
            // MeshVertex, 44 bytes
            0 => vec![element(position, 0, DXGI_FORMAT_R32G32B32_FLOAT, 0), element(normal, 0, DXGI_FORMAT_R32G32B32_FLOAT, 12), element(texcoord, 0, DXGI_FORMAT_R32G32_FLOAT, 24), element(tangent, 0, DXGI_FORMAT_R32G32B32_FLOAT, 32)],
            // SkinnedMeshVertex, 76 bytes
            1 => vec![
                element(position, 0, DXGI_FORMAT_R32G32B32_FLOAT, 0),
                element(normal, 0, DXGI_FORMAT_R32G32B32_FLOAT, 12),
                element(texcoord, 0, DXGI_FORMAT_R32G32_FLOAT, 24),
                element(tangent, 0, DXGI_FORMAT_R32G32B32_FLOAT, 32),
                element(texcoord, 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 44),
                element(texcoord, 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 60),
            ],
            // ParticleVertex, 36 bytes
            2 => vec![element(position, 0, DXGI_FORMAT_R32G32B32_FLOAT, 0), element(color, 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 12), element(texcoord, 0, DXGI_FORMAT_R32G32_FLOAT, 28)],
            _ => return None,
        };
        let mut layout = None;
        let _ = kgl.device.CreateInputLayout(&elements, bytecode, Some(&mut layout));
        self.input_layouts.insert(il_type, layout.clone());
        layout
    }
}

/// `KGLShader::reflectVars` 0x14001f5f0, for one blob (the vertex shader's first, then the
/// pixel shader's).
unsafe fn reflect_vars(shader: &mut KglShader, blob: &[u8]) -> Result<(), String> {
    let mut reflection: Option<ID3D11ShaderReflection> = None;
    D3DReflect(blob.as_ptr() as *const c_void, blob.len(), &ID3D11ShaderReflection::IID, &mut reflection as *mut _ as *mut *mut c_void).map_err(|e| format!("D3DReflect: {e}"))?;
    let reflection = reflection.ok_or("D3DReflect gave nothing")?;
    let mut sd = D3D11_SHADER_DESC::default();
    reflection.GetDesc(&mut sd).map_err(|e| format!("ID3D11ShaderReflection::GetDesc: {e}"))?;
    for i in 0..sd.ConstantBuffers {
        let Some(cb) = reflection.GetConstantBufferByIndex(i) else {
            continue;
        };
        let mut cbd = D3D11_SHADER_BUFFER_DESC::default();
        if cb.GetDesc(&mut cbd).is_err() {
            continue;
        }
        let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
        let _ = reflection.GetResourceBindingDescByName(cbd.Name, &mut bd);
        let slot = bd.BindPoint;
        let cb_name = text(cbd.Name);
        // KGLShader::addCBuffer 0x14001fe70: the first of a name is kept, and a second name
        // on a slot that is taken is dropped
        if !shader.cbuffers.iter().any(|c| c.name == cb_name || c.slot == slot) {
            shader.cbuffers.push(KglShaderCBuffer { name: cb_name.clone(), size: cbd.Size, slot });
        }
        for j in 0..cbd.Variables {
            let Some(v) = cb.GetVariableByIndex(j) else {
                continue;
            };
            let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
            if v.GetDesc(&mut vd).is_err() {
                continue;
            }
            let name = text(vd.Name);
            if !shader.vars.iter().any(|x| x.name == name) {
                shader.vars.push(KglShaderVar { name, cbuffer_name: cb_name.clone(), cbuffer_slot: slot, size: vd.Size, offset: vd.StartOffset });
            }
        }
    }
    for k in 0..sd.BoundResources {
        let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
        if reflection.GetResourceBindingDesc(k, &mut bd).is_err() {
            continue;
        }
        // registers 6..=19 are the engine's own (shadow maps, the cube map)
        if bd.Type == D3D_SIT_TEXTURE && !(6..=19).contains(&bd.BindPoint) {
            shader.textures.push(KglShaderTexture { name: text(bd.Name), slot: bd.BindPoint });
        }
    }
    Ok(())
}
