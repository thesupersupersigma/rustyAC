// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `Material`, `MaterialVar`, `MaterialResource` and the two material filters:
//! `Material::setShader` 0x14020b700, `createCBuffers` 0x14020a940, `initShaderVars`
//! 0x14020ac30, `Material::apply` 0x14020a6e0, `MaterialFilter::apply` 0x140219d90,
//! `MaterialFilterSM::apply` 0x140229a00.

use crate::graphics::{Graphics, BLEND_ALPHA_TO_COVERAGE, BLEND_OPAQUE, CULL_BACK, CULL_NONE};
use crate::shader::{CBuffer, ShaderId, ShaderVariable, VariableType};
use crate::texture::Texture;

/// `MaterialVar` (0x90 bytes): the values of one variable, in every shape, and the material's
/// own copy of the shader variable (an index into [`Material::shader_vars`]).
#[derive(Clone, Debug)]
pub struct MaterialVar {
    pub name: String,
    pub f_value: f32,
    pub f_value2: [f32; 2],
    pub f_value3: [f32; 3],
    pub f_value4: [f32; 4],
    pub m_value: [f32; 16],
}

/// `MaterialResource` (0x50 bytes).
#[derive(Clone, Debug)]
pub struct MaterialResource {
    pub slot: i32,
    pub texture: Texture,
    pub name: String,
}

/// What the game's `Material*` is to the material filter: an index into the renderer's list
/// of materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u32);

/// `Material` (0xb0 bytes).
pub struct Material {
    pub name: String,
    pub shader: Option<ShaderId>,
    pub double_face: bool,
    pub wire_frame: bool,
    pub vars: Vec<MaterialVar>,
    pub resources: Vec<MaterialResource>,
    pub cbuffers: Vec<CBuffer>,
    pub shader_vars: Vec<ShaderVariable>,
    pub depth_mode: i32,
    pub blend_mode: i32,
    pub cull_mode: i32,
    pub double_face_shadow: bool,
    /// `Shader::ilType` of the shader (1 = skinned), kept here for the shadow pass
    pub il_type: i32,
}

impl Material {
    /// `Material::Material` 0x140209970.
    pub fn new(name: &str) -> Material {
        Material {
            name: name.to_string(),
            shader: None,
            double_face: false,
            wire_frame: false,
            vars: Vec::new(),
            resources: Vec::new(),
            cbuffers: Vec::new(),
            shader_vars: Vec::new(),
            depth_mode: 0,
            blend_mode: 0,
            cull_mode: 0,
            double_face_shadow: false,
            il_type: 0,
        }
    }

    /// `Material::setShader` 0x14020b700 with `createCBuffers` 0x14020a940 and
    /// `initShaderVars` 0x14020ac30.
    pub fn set_shader(&mut self, graphics: &mut Graphics, name: &str) -> Result<(), String> {
        let id = graphics.shaders.get_shader(&graphics.kgl, name)?;
        if Some(id) == self.shader {
            return Ok(());
        }
        self.shader = Some(id);
        let shader = graphics.shaders.get(id);
        // createCBuffers: one buffer of the material's own per non-system buffer of the shader
        self.cbuffers.clear();
        for scb in &shader.cbuffers {
            self.cbuffers.push(CBuffer::init(&graphics.kgl, scb.slot, scb.size));
        }
        // initShaderVars
        let old_vars = std::mem::take(&mut self.vars);
        self.shader_vars.clear();
        for sv in &shader.vars {
            let slot = sv.buffer.map(|b| shader.cbuffers[b].slot);
            let buffer = slot.and_then(|slot| self.cbuffers.iter().position(|c| c.slot == slot));
            if buffer.is_none() {
                println!("ERROR: Material::getBufferFromSlot, Buffer not found! slot:{}", slot.unwrap_or(-1));
            }
            let nsv = ShaderVariable::new(&sv.name, buffer, sv.offset, sv.size);
            // MaterialVar::MaterialVar 0x140209c60: all zero, and the zero is written
            let mut mv = MaterialVar { name: nsv.name.clone(), f_value: 0.0, f_value2: [0.0; 2], f_value3: [0.0; 3], f_value4: [0.0; 4], m_value: [0.0; 16] };
            self.shader_vars.push(nsv);
            let index = self.shader_vars.len() - 1;
            self.write_var(index, &mv);
            for old in &old_vars {
                if old.name == mv.name {
                    // MaterialVar::copyValues 0x14020a850
                    mv.f_value = old.f_value;
                    mv.f_value4 = old.f_value4;
                    mv.f_value2 = old.f_value2;
                    mv.f_value3 = old.f_value3;
                    mv.m_value = old.m_value;
                    self.write_var(index, &mv);
                }
            }
            self.vars.push(mv);
        }
        if shader.is_alpha_tested {
            self.blend_mode = BLEND_ALPHA_TO_COVERAGE;
        }
        self.il_type = shader.il_type;
        let old_resources = std::mem::take(&mut self.resources);
        for sr in &shader.resources {
            let mut r = MaterialResource { slot: sr.slot, texture: Texture::default(), name: sr.name.clone() };
            for o in &old_resources {
                if o.name == r.name || o.slot == r.slot {
                    r.texture = o.texture.clone();
                }
            }
            self.resources.push(r);
        }
        Ok(())
    }

    /// `MaterialVar::set` 0x14005d880: the member that matches the variable's size goes into
    /// the material's constant buffer.
    fn write_var(&mut self, index: usize, value: &MaterialVar) {
        let var = &self.shader_vars[index];
        let Some(buffer) = var.buffer else {
            return;
        };
        let offset = var.offset as usize;
        let cb = &mut self.cbuffers[buffer];
        match var.kind {
            VariableType::Float => cb.set_f32s(&[value.f_value], offset),
            VariableType::Float2 => cb.set_f32s(&value.f_value2, offset),
            VariableType::Float3 => cb.set_f32s(&value.f_value3, offset),
            VariableType::Float4 => cb.set_f32s(&value.f_value4, offset),
            VariableType::Matrix => cb.set_f32s(&value.m_value, offset),
            VariableType::Other => {}
        }
    }

    /// `Material::Material(const Material&)` 0x140209a40: a material of its own with the same
    /// shader, textures and values. The cull mode and the wire-frame flag are not copied.
    pub fn clone_material(&self, graphics: &mut Graphics) -> Result<Material, String> {
        let mut m = Material::new(&self.name);
        m.blend_mode = self.blend_mode;
        m.double_face = self.double_face;
        if let Some(shader) = self.shader {
            let name = graphics.shaders.get(shader).name.clone();
            m.set_shader(graphics, &name)?;
        }
        m.resources = self.resources.clone();
        for i in 0..m.vars.len().min(self.vars.len()) {
            // MaterialVar::copyValues 0x14020a850
            let old = &self.vars[i];
            let mut mv = m.vars[i].clone();
            mv.f_value = old.f_value;
            mv.f_value4 = old.f_value4;
            mv.f_value2 = old.f_value2;
            mv.f_value3 = old.f_value3;
            mv.m_value = old.m_value;
            m.write_var(i, &mv);
            m.vars[i] = mv;
        }
        m.depth_mode = self.depth_mode;
        m.double_face_shadow = self.double_face_shadow;
        Ok(m)
    }

    /// `Material::getVar` 0x14020ab30 as the game's own code calls it: a missing variable is
    /// reported.
    pub fn get_var_reporting(&self, name: &str) -> Option<usize> {
        let found = self.get_var(name);
        if found.is_none() {
            println!("ERROR: Material::getVar CANT FIND VAR {name} in material {}", self.name);
        }
        found
    }

    /// `Material::getVar` 0x14020ab30.
    pub fn get_var(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }

    /// Changes a variable and writes it (`var->fValue = …; MaterialVar::set`).
    pub fn update_var(&mut self, index: usize, change: impl FnOnce(&mut MaterialVar)) {
        let mut value = self.vars[index].clone();
        change(&mut value);
        self.write_var(index, &value);
        self.vars[index] = value;
    }

    /// `MaterialVar::setFloat` 0x14005d8d0 by name; nothing when the shader has no such variable.
    pub fn set_float(&mut self, name: &str, value: f32) {
        if let Some(index) = self.get_var(name) {
            self.update_var(index, |v| v.f_value = value);
        }
    }

    /// `Material::getResourceIndex` 0x14020aa40.
    pub fn get_resource_index(&self, name: &str) -> Option<usize> {
        self.resources.iter().position(|r| r.name == name)
    }
}

/// `RenderPassID` of `CameraMeshFilter`.
pub const PASS_OPAQUE: i32 = 0;
pub const PASS_TRANSPARENT: i32 = 1;
pub const PASS_SHADOW: i32 = 2;

/// `Material::apply` 0x14020a6e0.
pub fn apply(material: &mut Material, graphics: &mut Graphics, pass_id: i32) {
    graphics.set_blend_mode(material.blend_mode);
    graphics.set_cull_mode(material.cull_mode);
    if pass_id == PASS_OPAQUE {
        graphics.set_depth_mode(material.depth_mode);
    }
    if material.double_face {
        graphics.set_cull_mode(CULL_NONE);
    }
    for r in &material.resources {
        graphics.set_texture(r.slot, &r.texture);
    }
    if let Some(shader) = material.shader {
        graphics.set_shader(shader);
    }
    for cb in &mut material.cbuffers {
        cb.commit(&graphics.kgl);
    }
}

/// `MaterialFilter` and `MaterialFilterSM`: what a mesh's material does before the draw.
pub enum MaterialFilter {
    /// `MaterialFilter` (0x10 bytes): the material is applied unless it is the one applied last.
    Plain { last_material: Option<MaterialId> },
    /// `MaterialFilterSM` (0x30 bytes): the shadow pass, where one of three shaders stands in.
    ShadowMap { sm_alpha_tested: ShaderId, sm_normal: ShaderId, sm_skinned: Option<ShaderId> },
}

impl MaterialFilter {
    /// `MaterialFilter::MaterialFilter` 0x140219d30.
    pub fn plain() -> MaterialFilter {
        MaterialFilter::Plain { last_material: None }
    }

    /// `MaterialFilterSM::MaterialFilterSM` 0x140229840.
    pub fn shadow_map(graphics: &mut Graphics) -> Result<MaterialFilter, String> {
        let sm_normal = graphics.shaders.get_shader(&graphics.kgl, "ksShadowGen")?;
        let sm_alpha_tested = graphics.shaders.get_shader(&graphics.kgl, "ksShadowGenAT")?;
        let sm_skinned = graphics.shaders.get_shader(&graphics.kgl, "ksShadowGenSKIN").ok();
        Ok(MaterialFilter::ShadowMap { sm_alpha_tested, sm_normal, sm_skinned })
    }

    /// `MaterialFilter::resetMaterialCache` 0x140219dd0.
    pub fn reset_material_cache(&mut self) {
        if let MaterialFilter::Plain { last_material } = self {
            *last_material = None;
        }
    }

    /// The virtual `apply` (vtable slot +8).
    pub fn apply(&mut self, id: MaterialId, material: &mut Material, graphics: &mut Graphics, pass_id: i32) {
        match self {
            MaterialFilter::Plain { last_material } => {
                if *last_material == Some(id) {
                    return;
                }
                apply(material, graphics, pass_id);
                *last_material = Some(id);
            }
            MaterialFilter::ShadowMap { sm_alpha_tested, sm_normal, sm_skinned } => {
                let shader = if material.il_type == 1 {
                    *sm_skinned
                } else if material.blend_mode != BLEND_OPAQUE {
                    if let Some(r) = material.resources.iter().find(|r| r.slot == 0) {
                        graphics.set_texture(r.slot, &r.texture);
                    }
                    Some(*sm_alpha_tested)
                } else {
                    Some(*sm_normal)
                };
                if let Some(shader) = shader {
                    graphics.set_shader(shader);
                }
                graphics.set_blend_mode(BLEND_OPAQUE);
                if material.blend_mode != BLEND_OPAQUE {
                    for cb in &mut material.cbuffers {
                        if cb.slot == 4 {
                            cb.commit(&graphics.kgl);
                        }
                    }
                }
                graphics.set_cull_mode(if material.double_face_shadow { CULL_NONE } else { CULL_BACK });
            }
        }
    }
}
