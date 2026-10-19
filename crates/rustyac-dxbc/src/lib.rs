// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiled Direct3D shaders (DXBC containers, shader model 4) read in plain Rust, so they can
//! run where Direct3D is not: the container's chunks, the reflection `D3DReflect` gives
//! (constant buffers, their variables, bound textures and samplers), the input and output
//! signatures, and the program itself, written again as WGSL ([`translate_pair`]) for wgpu.
//!
//! Nothing here calls Windows; it builds for wasm32 as it does natively.

mod sm4;
mod wgsl;

pub use sm4::{Instruction, Operand, Program};
pub use wgsl::{translate_pair, Translated};

/// Which stage a shader is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Pixel,
    Vertex,
}

/// `D3D_SHADER_INPUT_TYPE`
pub const SIT_CBUFFER: u32 = 0;
pub const SIT_TBUFFER: u32 = 1;
pub const SIT_TEXTURE: u32 = 2;
pub const SIT_SAMPLER: u32 = 3;

/// One `D3D11_SHADER_INPUT_BIND_DESC`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    /// `D3D_SHADER_INPUT_TYPE`
    pub kind: u32,
    pub return_type: u32,
    /// `D3D_SRV_DIMENSION`
    pub dimension: u32,
    pub samples: u32,
    pub bind_point: u32,
    pub bind_count: u32,
    pub flags: u32,
}

/// One `D3D11_SHADER_VARIABLE_DESC` (the parts kept).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub flags: u32,
}

/// One `D3D11_SHADER_BUFFER_DESC`, with its variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstantBuffer {
    pub name: String,
    pub size: u32,
    pub kind: u32,
    pub flags: u32,
    pub variables: Vec<Variable>,
}

/// The `RDEF` chunk: what `ID3D11ShaderReflection` reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reflection {
    pub constant_buffers: Vec<ConstantBuffer>,
    pub bindings: Vec<Binding>,
    pub creator: String,
}

impl Reflection {
    /// `GetResourceBindingDescByName`
    pub fn binding(&self, name: &str) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.name == name)
    }
}

/// One `D3D11_SIGNATURE_PARAMETER_DESC` of `ISGN` / `OSGN`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigElement {
    pub name: String,
    pub index: u32,
    /// `D3D_NAME`: 0 none, 1 position, 9 front face, 64 target, 65 depth ...
    pub system_value: u32,
    /// `D3D_REGISTER_COMPONENT_TYPE`: 1 uint, 2 sint, 3 float
    pub component_type: u32,
    pub register: u32,
    pub mask: u8,
    pub rw_mask: u8,
}

/// A whole shader read from its container.
#[derive(Clone, Debug)]
pub struct Shader {
    pub stage: Stage,
    pub major: u32,
    pub minor: u32,
    pub reflection: Reflection,
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    pub program: Program,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("read past the end at {at}"))
}

fn cstr_at(b: &[u8], at: usize) -> Result<String, String> {
    let s = b.get(at..).ok_or_else(|| format!("string past the end at {at}"))?;
    let end = s.iter().position(|&c| c == 0).ok_or("string without its end")?;
    Ok(s[..end].iter().map(|&c| c as char).collect())
}

/// One chunk of a `DXBC` container: (four-letter tag, contents).
pub type Chunk<'a> = ([u8; 4], &'a [u8]);

/// The chunks of a `DXBC` container, as (four-letter tag, contents).
pub fn chunks(blob: &[u8]) -> Result<Vec<Chunk<'_>>, String> {
    if blob.get(..4) != Some(b"DXBC") {
        return Err("not a DXBC container".into());
    }
    let count = u32_at(blob, 28)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = u32_at(blob, 32 + 4 * i)? as usize;
        let tag = blob.get(at..at + 4).ok_or("chunk past the end")?;
        let size = u32_at(blob, at + 4)? as usize;
        let data = blob.get(at + 8..at + 8 + size).ok_or("chunk past the end")?;
        out.push(([tag[0], tag[1], tag[2], tag[3]], data));
    }
    Ok(out)
}

fn reflection(d: &[u8]) -> Result<Reflection, String> {
    let (cb_count, cb_at) = (u32_at(d, 0)? as usize, u32_at(d, 4)? as usize);
    let (bind_count, bind_at) = (u32_at(d, 8)? as usize, u32_at(d, 12)? as usize);
    let major = (u32_at(d, 16)? >> 4) & 0xf;
    let creator = cstr_at(d, u32_at(d, 24)? as usize)?;
    // shader model 5 variables carry four more words (texture and sampler ranges)
    let var_size = if major >= 5 { 40 } else { 24 };
    let mut bindings = Vec::with_capacity(bind_count);
    for i in 0..bind_count {
        let at = bind_at + 32 * i;
        let w = |k: usize| u32_at(d, at + 4 * k);
        bindings.push(Binding { name: cstr_at(d, w(0)? as usize)?, kind: w(1)?, return_type: w(2)?, dimension: w(3)?, samples: w(4)?, bind_point: w(5)?, bind_count: w(6)?, flags: w(7)? });
    }
    let mut constant_buffers = Vec::with_capacity(cb_count);
    for i in 0..cb_count {
        let at = cb_at + 24 * i;
        let w = |k: usize| u32_at(d, at + 4 * k);
        let (var_count, var_at) = (w(1)? as usize, w(2)? as usize);
        let mut variables = Vec::with_capacity(var_count);
        for j in 0..var_count {
            let v = var_at + var_size * j;
            variables.push(Variable { name: cstr_at(d, u32_at(d, v)? as usize)?, offset: u32_at(d, v + 4)?, size: u32_at(d, v + 8)?, flags: u32_at(d, v + 12)? });
        }
        constant_buffers.push(ConstantBuffer { name: cstr_at(d, w(0)? as usize)?, size: w(3)?, flags: w(4)?, kind: w(5)?, variables });
    }
    Ok(Reflection { constant_buffers, bindings, creator })
}

fn signature(d: &[u8]) -> Result<Vec<SigElement>, String> {
    let count = u32_at(d, 0)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 8 + 24 * i;
        let masks = u32_at(d, at + 20)?;
        out.push(SigElement {
            name: cstr_at(d, u32_at(d, at)? as usize)?,
            index: u32_at(d, at + 4)?,
            system_value: u32_at(d, at + 8)?,
            component_type: u32_at(d, at + 12)?,
            register: u32_at(d, at + 16)?,
            mask: masks as u8,
            rw_mask: (masks >> 8) as u8,
        });
    }
    Ok(out)
}

/// Reads a compiled shader: its reflection, signatures and program.
pub fn parse(blob: &[u8]) -> Result<Shader, String> {
    let mut reflection_ = None;
    let (mut inputs, mut outputs, mut code) = (None, None, None);
    for (tag, data) in chunks(blob)? {
        match &tag {
            b"RDEF" => reflection_ = Some(reflection(data)?),
            b"ISGN" => inputs = Some(signature(data)?),
            b"OSGN" => outputs = Some(signature(data)?),
            b"SHDR" | b"SHEX" => code = Some(data),
            _ => {}
        }
    }
    let code = code.ok_or("no SHDR/SHEX chunk")?;
    let version = u32_at(code, 0)?;
    let stage = match version >> 16 {
        0 => Stage::Pixel,
        1 => Stage::Vertex,
        other => return Err(format!("program type {other} is not a pixel or vertex shader")),
    };
    let words: Vec<u32> = code.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let length = (words.get(1).copied().ok_or("no program length")? as usize).min(words.len());
    let program = sm4::decode(&words[2..length])?;
    Ok(Shader {
        stage,
        major: (version >> 4) & 0xf,
        minor: version & 0xf,
        reflection: reflection_.unwrap_or_default(),
        inputs: inputs.unwrap_or_default(),
        outputs: outputs.unwrap_or_default(),
        program,
    })
}
