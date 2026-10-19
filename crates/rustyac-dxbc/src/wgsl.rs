// SPDX-License-Identifier: MIT OR Apache-2.0

//! A vertex and pixel shader pair written again as WGSL.
//!
//! The registers keep their Direct3D meaning: every temporary, input and output is a
//! `vec4<u32>` holding the register's bits, and each instruction reads its sources as the type
//! it works in (`bitcast`, which never changes a bit). Bindings: constant buffer `b<n>` is
//! `@group(0) @binding(n)`, texture `t<n>` `@group(1) @binding(n)`, sampler `s<n>`
//! `@group(2) @binding(n)`. Varyings sit at `@location(register)`; the pixel shader's
//! interpolation modes are given to the vertex shader's outputs too, as WebGPU wants.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::sm4::{op, Index, Instruction, Operand};
use crate::{Shader, Stage};

/// One texture register as the pair uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureUse {
    pub slot: u32,
    /// `D3D10_SB_RESOURCE_DIMENSION`: 2 1D, 3 2D, 4 2D multisampled, 5 3D, 6 cube, 8 2D array
    pub dimension: u32,
    /// read with a comparison (`texture_depth_*`)
    pub depth: bool,
    /// `D3D10_SB_RESOURCE_RETURN_TYPE` of x: 1 unorm, 2 snorm, 3 sint, 4 uint, 5 float
    pub return_type: u32,
    pub vertex: bool,
    pub pixel: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplerUse {
    pub slot: u32,
    pub comparison: bool,
    pub vertex: bool,
    pub pixel: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CBufferUse {
    pub slot: u32,
    /// bytes the shaders declare (16 per register)
    pub size: u32,
    pub vertex: bool,
    pub pixel: bool,
}

/// The translated pair and what it binds.
#[derive(Clone, Debug)]
pub struct Translated {
    pub vs: String,
    pub ps: String,
    pub textures: Vec<TextureUse>,
    pub samplers: Vec<SamplerUse>,
    pub cbuffers: Vec<CBufferUse>,
    /// vertex input register of each `ISGN` element of the vertex shader: (semantic, index,
    /// register)
    pub vertex_inputs: Vec<(String, u32, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    F,
    I,
    U,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::F => "f32",
            Ty::I => "i32",
            Ty::U => "u32",
        }
    }
}

const XYZW: [char; 4] = ['x', 'y', 'z', 'w'];

fn swz(s: [u8; 4]) -> String {
    s.iter().map(|&c| XYZW[c as usize]).collect()
}

/// `@interpolate(...)` of `D3D10_SB_INTERPOLATION_MODE`.
fn interpolation(mode: u32) -> &'static str {
    match mode {
        1 => " @interpolate(flat)",
        3 => " @interpolate(perspective, centroid)",
        4 => " @interpolate(linear)",
        5 => " @interpolate(linear, centroid)",
        6 => " @interpolate(perspective, sample)",
        7 => " @interpolate(linear, sample)",
        _ => "",
    }
}

/// Which texture registers are read with a comparison, in either stage.
fn depth_slots(shader: &Shader, out: &mut BTreeMap<u32, bool>) {
    for i in &shader.program.instructions {
        if matches!(i.opcode, op::SAMPLE_C | op::SAMPLE_C_LZ) {
            out.insert(i.operands[2].reg(), true);
        }
    }
}

struct Emitter<'a> {
    shader: &'a Shader,
    depth: &'a BTreeMap<u32, bool>,
    out: String,
    indent: usize,
    tmp: usize,
    /// slot -> (dimension, return type)
    textures: BTreeMap<u32, (u32, u32)>,
    comparison: BTreeMap<u32, bool>,
    cbuffers: BTreeMap<u32, u32>,
}

impl Emitter<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn index(&self, i: &Index) -> Result<String, String> {
        Ok(match i {
            Index::Imm(v) => format!("{v}u"),
            Index::Rel(o, base) => {
                let c = XYZW[o.swizzle()[0] as usize];
                format!("({}.{c} + {base}u)", self.raw_base(o)?)
            }
        })
    }

    /// The register an operand names, before its swizzle, as `vec4<u32>`.
    fn raw_base(&self, o: &Operand) -> Result<String, String> {
        Ok(match o.kind {
            0 => format!("r{}", o.reg()),
            1 => {
                if o.index.len() != 1 || !matches!(o.index[0], Index::Imm(_)) {
                    return Err("indexed inputs are not handled".into());
                }
                format!("v{}", o.reg())
            }
            2 => format!("o{}", o.reg()),
            4 => format!("vec4<u32>({:#x}u, {:#x}u, {:#x}u, {:#x}u)", o.imm[0], o.imm[1], o.imm[2], o.imm[3]),
            8 => {
                let slot = o.reg();
                let at = o.index.get(1).ok_or("constant buffer read without a register")?;
                format!("cb{slot}[{}]", self.index(at)?)
            }
            9 => format!("icb[{}]", self.index(o.index.first().ok_or("icb read without an index")?)?),
            other => return Err(format!("operand type {other} cannot be read")),
        })
    }

    /// A source operand as `vec4<u32>` bits, swizzled, without modifiers.
    fn raw(&self, o: &Operand) -> Result<String, String> {
        let base = self.raw_base(o)?;
        let s = o.swizzle();
        Ok(if o.kind == 4 || s == [0, 1, 2, 3] { base } else { format!("{base}.{}", swz(s)) })
    }

    /// A source operand as `vec4<ty>`, with its `-` and `|.|`.
    fn src(&self, o: &Operand, ty: Ty) -> Result<String, String> {
        let raw = self.raw(o)?;
        let ty = if ty == Ty::U && (o.neg || o.abs) { Ty::I } else { ty };
        let mut e = if ty == Ty::U { raw } else { format!("bitcast<vec4<{}>>({raw})", ty.name()) };
        if o.abs {
            e = format!("abs({e})");
        }
        if o.neg {
            e = format!("(-{e})");
        }
        Ok(e)
    }

    fn scalar_u(&self, o: &Operand) -> Result<String, String> {
        Ok(format!("{}.x", self.raw(o)?))
    }

    /// Stores a `vec4<u32>` value into the destination's written components.
    fn store(&mut self, d: &Operand, value: String) -> Result<(), String> {
        let name = match d.kind {
            0 => format!("r{}", d.reg()),
            2 => format!("o{}", d.reg()),
            12 => "odepth".to_string(),
            13 => return Ok(()),
            other => return Err(format!("operand type {other} cannot be written")),
        };
        let mask = d.mask();
        if mask == 0xf {
            self.line(&format!("{name} = {value};"));
            return Ok(());
        }
        let t = self.tmp;
        self.tmp += 1;
        self.line(&format!("let x{t} = {value};"));
        for (c, n) in XYZW.iter().enumerate() {
            if mask & (1 << c) != 0 {
                self.line(&format!("{name}.{n} = x{t}.{n};"));
            }
        }
        Ok(())
    }

    /// Stores a `vec4<f32>` result, saturated when the instruction says so.
    fn store_f(&mut self, ins: &Instruction, value: String) -> Result<(), String> {
        let v = if ins.saturate { format!("clamp({value}, vec4<f32>(0.0), vec4<f32>(1.0))") } else { value };
        self.store(&ins.operands[0], format!("bitcast<vec4<u32>>({v})"))
    }

    fn texture_coords(&self, dimension: u32, a: &str) -> Result<String, String> {
        Ok(match dimension {
            2 => format!("{a}.x"),
            3 => format!("{a}.xy"),
            5 | 6 => format!("{a}.xyz"),
            8 => format!("{a}.xy, i32(round({a}.z))"),
            other => return Err(format!("sampling resource dimension {other} is not handled")),
        })
    }

    fn offset(&self, ins: &Instruction, dimension: u32) -> String {
        if ins.offset == [0; 3] {
            return String::new();
        }
        let [u, v, w] = ins.offset;
        match dimension {
            5 => format!(", vec3<i32>({u}, {v}, {w})"),
            2 => format!(", {u}"),
            _ => format!(", vec2<i32>({u}, {v})"),
        }
    }

    /// `sample*` and `ld*`: the texel as `vec4<u32>`, before the resource's swizzle.
    fn texel(&self, ins: &Instruction) -> Result<String, String> {
        let res = &ins.operands[2];
        let slot = res.reg();
        let (dimension, ret) = *self.textures.get(&slot).ok_or(format!("t{slot} is not declared"))?;
        let depth = self.depth.contains_key(&slot);
        let a = self.src(&ins.operands[1], Ty::F)?;
        let tex = format!("t{slot}");
        let vertex = self.shader.stage == Stage::Vertex;
        let sampler = || -> Result<String, String> { Ok(format!("s{}", ins.operands.get(3).ok_or("sample without a sampler")?.reg())) };
        let wrap = |v: String| -> String {
            if depth {
                format!("bitcast<vec4<u32>>(vec4<f32>({v}, 0.0, 0.0, 1.0))")
            } else if ret == 3 {
                format!("bitcast<vec4<u32>>({v})")
            } else if ret == 4 {
                v
            } else {
                format!("bitcast<vec4<u32>>({v})")
            }
        };
        let off = self.offset(ins, dimension);
        Ok(match ins.opcode {
            op::SAMPLE | op::SAMPLE_B | op::SAMPLE_L => {
                let coords = self.texture_coords(dimension, &format!("({a})"))?;
                let s = sampler()?;
                let call = match ins.opcode {
                    op::SAMPLE if !vertex => format!("textureSample({tex}, {s}, {coords}{off})"),
                    op::SAMPLE => format!("textureSampleLevel({tex}, {s}, {coords}, {}{off})", if depth { "0" } else { "0.0" }),
                    op::SAMPLE_B => format!("textureSampleBias({tex}, {s}, {coords}, {}.x{off})", self.src(&ins.operands[4], Ty::F)?),
                    _ => {
                        let lod = self.src(&ins.operands[4], Ty::F)?;
                        let lod = if depth { format!("i32({lod}.x)") } else { format!("{lod}.x") };
                        format!("textureSampleLevel({tex}, {s}, {coords}, {lod}{off})")
                    }
                };
                wrap(call)
            }
            op::SAMPLE_C | op::SAMPLE_C_LZ => {
                let coords = self.texture_coords(dimension, &format!("({a})"))?;
                let s = sampler()?;
                let reference = format!("{}.x", self.src(&ins.operands[4], Ty::F)?);
                let f = if ins.opcode == op::SAMPLE_C && !vertex { "textureSampleCompare" } else { "textureSampleCompareLevel" };
                format!("bitcast<vec4<u32>>(vec4<f32>({f}({tex}, {s}, {coords}, {reference}{off})))")
            }
            op::LD | op::LD_MS => {
                let a = self.src(&ins.operands[1], Ty::I)?;
                let (coords, last) = match dimension {
                    3 => (format!("({a}).xy"), format!("({a}).w")),
                    4 => (format!("({a}).xy"), format!("{}.x", self.src(&ins.operands[3], Ty::I)?)),
                    5 => (format!("({a}).xyz"), format!("({a}).w")),
                    8 => (format!("({a}).xy, ({a}).z"), format!("({a}).w")),
                    other => return Err(format!("loading resource dimension {other} is not handled")),
                };
                wrap(format!("textureLoad({tex}, {coords}, {last})"))
            }
            _ => unreachable!(),
        })
    }

    fn instruction(&mut self, ins: &Instruction) -> Result<(), String> {
        let o = &ins.operands;
        let bin_f = |e: &Self, f: &dyn Fn(String, String) -> String| -> Result<String, String> { Ok(f(e.src(&o[1], Ty::F)?, e.src(&o[2], Ty::F)?)) };
        let un_f = |e: &Self, f: &str| -> Result<String, String> { Ok(format!("{f}({})", e.src(&o[1], Ty::F)?)) };
        let cmp = |e: &Self, ty: Ty, sym: &str| -> Result<String, String> {
            Ok(format!("select(vec4<u32>(0u), vec4<u32>(0xffffffffu), {} {sym} {})", e.src(&o[1], ty)?, e.src(&o[2], ty)?))
        };
        let cond = |e: &Self| -> Result<String, String> { Ok(format!("{} {} 0u", e.scalar_u(&o[0])?, if ins.nonzero { "!=" } else { "==" })) };
        match ins.opcode {
            op::DCL_GLOBAL_FLAGS | op::DCL_TEMPS | op::DCL_RESOURCE | op::DCL_CONSTANT_BUFFER | op::DCL_SAMPLER | op::NOP => {}
            op::DCL_INPUT | op::DCL_INPUT_SGV | op::DCL_INPUT_SIV | op::DCL_INPUT_PS | op::DCL_INPUT_PS_SGV | op::DCL_INPUT_PS_SIV | op::DCL_OUTPUT | op::DCL_OUTPUT_SGV | op::DCL_OUTPUT_SIV => {}
            op::ADD => {
                let v = bin_f(self, &|a, b| format!("{a} + {b}"))?;
                self.store_f(ins, v)?
            }
            op::MUL => {
                let v = bin_f(self, &|a, b| format!("{a} * {b}"))?;
                self.store_f(ins, v)?
            }
            op::DIV => {
                let v = bin_f(self, &|a, b| format!("{a} / {b}"))?;
                self.store_f(ins, v)?
            }
            op::MIN => {
                let v = bin_f(self, &|a, b| format!("min({a}, {b})"))?;
                self.store_f(ins, v)?
            }
            op::MAX => {
                let v = bin_f(self, &|a, b| format!("max({a}, {b})"))?;
                self.store_f(ins, v)?
            }
            op::MAD => {
                let v = format!("{} * {} + {}", self.src(&o[1], Ty::F)?, self.src(&o[2], Ty::F)?, self.src(&o[3], Ty::F)?);
                self.store_f(ins, v)?
            }
            op::DP2 | op::DP3 | op::DP4 => {
                let n = match ins.opcode {
                    op::DP2 => "xy",
                    op::DP3 => "xyz",
                    _ => "xyzw",
                };
                let v = format!("vec4<f32>(dot(({}).{n}, ({}).{n}))", self.src(&o[1], Ty::F)?, self.src(&o[2], Ty::F)?);
                self.store_f(ins, v)?
            }
            op::EXP => {
                let v = un_f(self, "exp2")?;
                self.store_f(ins, v)?
            }
            op::LOG => {
                let v = un_f(self, "log2")?;
                self.store_f(ins, v)?
            }
            op::RSQ => {
                let v = un_f(self, "inverseSqrt")?;
                self.store_f(ins, v)?
            }
            op::SQRT => {
                let v = un_f(self, "sqrt")?;
                self.store_f(ins, v)?
            }
            op::FRC => {
                let v = un_f(self, "fract")?;
                self.store_f(ins, v)?
            }
            op::ROUND_NE => {
                let v = un_f(self, "round")?;
                self.store_f(ins, v)?
            }
            op::ROUND_NI => {
                let v = un_f(self, "floor")?;
                self.store_f(ins, v)?
            }
            op::ROUND_PI => {
                let v = un_f(self, "ceil")?;
                self.store_f(ins, v)?
            }
            op::ROUND_Z => {
                let v = un_f(self, "trunc")?;
                self.store_f(ins, v)?
            }
            op::DERIV_RTX => {
                let v = un_f(self, "dpdx")?;
                self.store_f(ins, v)?
            }
            op::DERIV_RTY => {
                let v = un_f(self, "dpdy")?;
                self.store_f(ins, v)?
            }
            op::MOV => {
                if ins.saturate || o[1].neg || o[1].abs {
                    let v = self.src(&o[1], Ty::F)?;
                    self.store_f(ins, v)?
                } else {
                    let v = self.raw(&o[1])?;
                    self.store(&o[0], v)?
                }
            }
            op::MOVC => {
                let c = format!("{} != vec4<u32>(0u)", self.raw(&o[1])?);
                if ins.saturate || o[2].neg || o[2].abs || o[3].neg || o[3].abs {
                    let v = format!("select({}, {}, {c})", self.src(&o[3], Ty::F)?, self.src(&o[2], Ty::F)?);
                    self.store_f(ins, v)?
                } else {
                    let v = format!("select({}, {}, {c})", self.raw(&o[3])?, self.raw(&o[2])?);
                    self.store(&o[0], v)?
                }
            }
            op::EQ => {
                let v = cmp(self, Ty::F, "==")?;
                self.store(&o[0], v)?
            }
            op::NE => {
                let v = cmp(self, Ty::F, "!=")?;
                self.store(&o[0], v)?
            }
            op::LT => {
                let v = cmp(self, Ty::F, "<")?;
                self.store(&o[0], v)?
            }
            op::GE => {
                let v = cmp(self, Ty::F, ">=")?;
                self.store(&o[0], v)?
            }
            op::IEQ => {
                let v = cmp(self, Ty::I, "==")?;
                self.store(&o[0], v)?
            }
            op::INE => {
                let v = cmp(self, Ty::I, "!=")?;
                self.store(&o[0], v)?
            }
            op::ILT => {
                let v = cmp(self, Ty::I, "<")?;
                self.store(&o[0], v)?
            }
            op::IGE => {
                let v = cmp(self, Ty::I, ">=")?;
                self.store(&o[0], v)?
            }
            op::ULT => {
                let v = cmp(self, Ty::U, "<")?;
                self.store(&o[0], v)?
            }
            op::UGE => {
                let v = cmp(self, Ty::U, ">=")?;
                self.store(&o[0], v)?
            }
            op::IADD => {
                let v = format!("bitcast<vec4<u32>>({} + {})", self.src(&o[1], Ty::I)?, self.src(&o[2], Ty::I)?);
                self.store(&o[0], v)?
            }
            op::IMAD => {
                let v = format!("bitcast<vec4<u32>>({} * {} + {})", self.src(&o[1], Ty::I)?, self.src(&o[2], Ty::I)?, self.src(&o[3], Ty::I)?);
                self.store(&o[0], v)?
            }
            op::IMAX | op::IMIN => {
                let f = if ins.opcode == op::IMAX { "max" } else { "min" };
                let v = format!("bitcast<vec4<u32>>({f}({}, {}))", self.src(&o[1], Ty::I)?, self.src(&o[2], Ty::I)?);
                self.store(&o[0], v)?
            }
            op::UMAX | op::UMIN => {
                let f = if ins.opcode == op::UMAX { "max" } else { "min" };
                let v = format!("{f}({}, {})", self.src(&o[1], Ty::U)?, self.src(&o[2], Ty::U)?);
                self.store(&o[0], v)?
            }
            op::INEG => {
                let v = format!("bitcast<vec4<u32>>(-{})", self.src(&o[1], Ty::I)?);
                self.store(&o[0], v)?
            }
            op::AND | op::OR | op::XOR => {
                let sym = match ins.opcode {
                    op::AND => "&",
                    op::OR => "|",
                    _ => "^",
                };
                let v = format!("{} {sym} {}", self.src(&o[1], Ty::U)?, self.src(&o[2], Ty::U)?);
                self.store(&o[0], v)?
            }
            op::NOT => {
                let v = format!("~{}", self.src(&o[1], Ty::U)?);
                self.store(&o[0], v)?
            }
            op::ISHL | op::USHR => {
                let sym = if ins.opcode == op::ISHL { "<<" } else { ">>" };
                let v = format!("{} {sym} ({} & vec4<u32>(31u))", self.src(&o[1], Ty::U)?, self.src(&o[2], Ty::U)?);
                self.store(&o[0], v)?
            }
            op::ISHR => {
                let v = format!("bitcast<vec4<u32>>({} >> ({} & vec4<u32>(31u)))", self.src(&o[1], Ty::I)?, self.src(&o[2], Ty::U)?);
                self.store(&o[0], v)?
            }
            op::FTOI => {
                let v = format!("bitcast<vec4<u32>>(vec4<i32>({}))", self.src(&o[1], Ty::F)?);
                self.store(&o[0], v)?
            }
            op::FTOU => {
                let v = format!("vec4<u32>({})", self.src(&o[1], Ty::F)?);
                self.store(&o[0], v)?
            }
            op::ITOF => {
                let v = format!("vec4<f32>({})", self.src(&o[1], Ty::I)?);
                self.store_f(ins, v)?
            }
            op::UTOF => {
                let v = format!("vec4<f32>({})", self.src(&o[1], Ty::U)?);
                self.store_f(ins, v)?
            }
            op::SINCOS => {
                let a = self.src(&o[2], Ty::F)?;
                if o[0].kind != 13 {
                    self.store(&o[0], format!("bitcast<vec4<u32>>(sin({a}))"))?;
                }
                if o[1].kind != 13 {
                    let v = if ins.saturate { format!("clamp(cos({a}), vec4<f32>(0.0), vec4<f32>(1.0))") } else { format!("cos({a})") };
                    self.store(&o[1], format!("bitcast<vec4<u32>>({v})"))?;
                }
            }
            op::IF => {
                let c = cond(self)?;
                self.line(&format!("if ({c}) {{"));
                self.indent += 1;
            }
            op::ELSE => {
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
            }
            op::ENDIF | op::ENDLOOP => {
                self.indent -= 1;
                self.line("}");
            }
            op::LOOP => {
                self.line("loop {");
                self.indent += 1;
            }
            op::BREAK => self.line("break;"),
            op::CONTINUE => self.line("continue;"),
            op::BREAKC => {
                let c = cond(self)?;
                self.line(&format!("if ({c}) {{ break; }}"));
            }
            op::CONTINUEC => {
                let c = cond(self)?;
                self.line(&format!("if ({c}) {{ continue; }}"));
            }
            op::DISCARD => {
                let c = cond(self)?;
                self.line(&format!("if ({c}) {{ discard; }}"));
            }
            op::RET => self.line("return;"),
            op::RETC => {
                let c = cond(self)?;
                self.line(&format!("if ({c}) {{ return; }}"));
            }
            op::SAMPLE | op::SAMPLE_B | op::SAMPLE_L | op::SAMPLE_C | op::SAMPLE_C_LZ | op::LD | op::LD_MS => {
                let t = self.texel(ins)?;
                let s = o[2].swizzle();
                let v = if s == [0, 1, 2, 3] { t } else { format!("({t}).{}", swz(s)) };
                self.store(&o[0], v)?
            }
            op::RESINFO => {
                let res = &o[2];
                let slot = res.reg();
                let (dimension, _) = *self.textures.get(&slot).ok_or(format!("t{slot} is not declared"))?;
                let tex = format!("t{slot}");
                let mip = self.scalar_u(&o[1])?;
                let dims = match dimension {
                    4 => format!("vec4<u32>(textureDimensions({tex}), 0u, 1u)"),
                    5 => format!("vec4<u32>(textureDimensions({tex}, {mip}), textureNumLevels({tex}))"),
                    8 => format!("vec4<u32>(textureDimensions({tex}, {mip}), textureNumLayers({tex}), textureNumLevels({tex}))"),
                    _ => format!("vec4<u32>(textureDimensions({tex}, {mip}), 0u, textureNumLevels({tex}))"),
                };
                // _uint keeps integers; the default gives floats, _rcpFloat their inverses
                let v = match ins.controls & 3 {
                    2 => dims,
                    1 => format!("bitcast<vec4<u32>>(vec4<f32>(1.0) / vec4<f32>({dims}))"),
                    _ => format!("bitcast<vec4<u32>>(vec4<f32>({dims}))"),
                };
                let s = res.swizzle();
                let v = if s == [0, 1, 2, 3] { v } else { format!("({v}).{}", swz(s)) };
                if ins.saturate {
                    return Err("resinfo_sat is not handled".into());
                }
                self.store(&o[0], v)?
            }
            other => return Err(format!("opcode {other} is not handled")),
        }
        Ok(())
    }
}

/// Translates one stage. `interp` gives each pixel shader input register its mode, which the
/// vertex shader's outputs must repeat.
fn stage(shader: &Shader, depth: &BTreeMap<u32, bool>, interp: &BTreeMap<u32, u32>) -> Result<String, String> {
    let mut e = Emitter { shader, depth, out: String::new(), indent: 0, tmp: 0, textures: BTreeMap::new(), comparison: BTreeMap::new(), cbuffers: BTreeMap::new() };
    let mut temps = 0;
    for i in &shader.program.instructions {
        match i.opcode {
            op::DCL_TEMPS => temps = i.words.first().copied().unwrap_or(0),
            op::DCL_RESOURCE => {
                let ret = i.words.first().copied().unwrap_or(0x5555) & 0xf;
                e.textures.insert(i.operands[0].reg(), (i.controls & 0x1f, ret));
            }
            op::DCL_SAMPLER => {
                e.comparison.insert(i.operands[0].reg(), i.controls & 0xf == 1);
            }
            op::DCL_CONSTANT_BUFFER => {
                let o = &i.operands[0];
                let size = match o.index.get(1) {
                    Some(Index::Imm(n)) => *n,
                    _ => return Err("constant buffer without its size".into()),
                };
                e.cbuffers.insert(o.reg(), size);
            }
            _ => {}
        }
    }
    let mut head = String::new();
    if shader.stage == Stage::Pixel {
        // AC's shaders sample inside branches on per-pixel values, as Direct3D allows
        head.push_str("diagnostic(off, derivative_uniformity);\n\n");
    }
    for (slot, size) in &e.cbuffers {
        writeln!(head, "@group(0) @binding({slot}) var<uniform> cb{slot}: array<vec4<u32>, {size}>;").unwrap();
    }
    for (slot, (dimension, ret)) in &e.textures {
        let t = match ret {
            3 => "i32",
            4 => "u32",
            _ => "f32",
        };
        let d = depth.contains_key(slot);
        let ty = match (dimension, d) {
            (2, false) => format!("texture_1d<{t}>"),
            (3, false) => format!("texture_2d<{t}>"),
            (3, true) => "texture_depth_2d".into(),
            (4, false) => format!("texture_multisampled_2d<{t}>"),
            (4, true) => "texture_depth_multisampled_2d".into(),
            (5, false) => format!("texture_3d<{t}>"),
            (6, false) => format!("texture_cube<{t}>"),
            (6, true) => "texture_depth_cube".into(),
            (8, false) => format!("texture_2d_array<{t}>"),
            (8, true) => "texture_depth_2d_array".into(),
            (other, _) => return Err(format!("resource dimension {other} is not handled")),
        };
        writeln!(head, "@group(1) @binding({slot}) var t{slot}: {ty};").unwrap();
    }
    for (slot, comparison) in &e.comparison {
        writeln!(head, "@group(2) @binding({slot}) var s{slot}: {};", if *comparison { "sampler_comparison" } else { "sampler" }).unwrap();
    }
    if !shader.program.icb.is_empty() {
        let n = shader.program.icb.len() / 4;
        let items: Vec<String> = shader.program.icb.chunks_exact(4).map(|c| format!("vec4<u32>({:#x}u, {:#x}u, {:#x}u, {:#x}u)", c[0], c[1], c[2], c[3])).collect();
        writeln!(head, "var<private> icb: array<vec4<u32>, {n}> = array<vec4<u32>, {n}>({});", items.join(", ")).unwrap();
    }

    // inputs and outputs, one register each
    let mut in_fields = Vec::new();
    let mut in_copy = Vec::new();
    let mut out_fields = Vec::new();
    let mut out_copy = Vec::new();
    let mut in_regs = std::collections::BTreeSet::new();
    let mut out_regs = std::collections::BTreeSet::new();
    match shader.stage {
        Stage::Vertex => {
            for el in &shader.inputs {
                if !in_regs.insert(el.register) {
                    continue;
                }
                let r = el.register;
                match el.system_value {
                    0 => {
                        let ty = match el.component_type {
                            1 => "u32",
                            2 => "i32",
                            _ => "f32",
                        };
                        in_fields.push(format!("@location({r}) v{r}: vec4<{ty}>"));
                        in_copy.push(if ty == "u32" { format!("v{r} = input.v{r};") } else { format!("v{r} = bitcast<vec4<u32>>(input.v{r});") });
                    }
                    6 => {
                        in_fields.push(format!("@builtin(vertex_index) v{r}: u32"));
                        in_copy.push(format!("v{r} = vec4<u32>(input.v{r}, 0u, 0u, 0u);"));
                    }
                    8 => {
                        in_fields.push(format!("@builtin(instance_index) v{r}: u32"));
                        in_copy.push(format!("v{r} = vec4<u32>(input.v{r}, 0u, 0u, 0u);"));
                    }
                    other => return Err(format!("vertex input system value {other} is not handled")),
                }
            }
            for el in &shader.outputs {
                if !out_regs.insert(el.register) {
                    continue;
                }
                let r = el.register;
                match el.system_value {
                    1 => {
                        out_fields.push(format!("@builtin(position) o{r}: vec4<f32>"));
                    }
                    0 => {
                        let mode = interp.get(&r).copied().unwrap_or(2);
                        out_fields.push(format!("@location({r}){} o{r}: vec4<f32>", interpolation(mode)));
                    }
                    other => return Err(format!("vertex output system value {other} is not handled")),
                }
                out_copy.push(format!("output.o{r} = bitcast<vec4<f32>>(o{r});"));
            }
        }
        Stage::Pixel => {
            for el in &shader.inputs {
                let r = el.register;
                match el.system_value {
                    1 => {
                        if in_regs.insert(r) {
                            in_fields.push(format!("@builtin(position) v{r}: vec4<f32>"));
                            in_copy.push(format!("v{r} = bitcast<vec4<u32>>(input.v{r});"));
                        }
                    }
                    9 => {
                        let c = XYZW[el.mask.trailing_zeros().min(3) as usize];
                        in_fields.push(format!("@builtin(front_facing) ff{r}: bool"));
                        in_copy.push(format!("v{r}.{c} = select(0u, 0xffffffffu, input.ff{r});"));
                        in_regs.insert(r);
                    }
                    10 => {
                        let c = XYZW[el.mask.trailing_zeros().min(3) as usize];
                        in_fields.push(format!("@builtin(sample_index) si{r}: u32"));
                        in_copy.push(format!("v{r}.{c} = input.si{r};"));
                        in_regs.insert(r);
                    }
                    0 => {}
                    other => return Err(format!("pixel input system value {other} is not handled")),
                }
            }
            for (r, mode) in interp {
                if in_regs.insert(*r) {
                    in_fields.push(format!("@location({r}){} v{r}: vec4<f32>", interpolation(*mode)));
                    in_copy.push(format!("v{r} = bitcast<vec4<u32>>(input.v{r});"));
                }
            }
            for el in &shader.outputs {
                let r = el.register;
                // shader model 4 output signatures leave SV_Target and SV_Depth unnamed (0)
                let sv = match (el.system_value, el.name.to_ascii_uppercase().as_str()) {
                    (0, "SV_TARGET") => 64,
                    (0, "SV_DEPTH") => 65,
                    (sv, _) => sv,
                };
                match sv {
                    64 => {
                        if out_regs.insert(r) {
                            out_fields.push(format!("@location({r}) o{r}: vec4<f32>"));
                            out_copy.push(format!("output.o{r} = bitcast<vec4<f32>>(o{r});"));
                        }
                    }
                    65 => {
                        out_fields.push("@builtin(frag_depth) depth: f32".to_string());
                        out_copy.push("output.depth = bitcast<f32>(odepth.x);".to_string());
                    }
                    other => return Err(format!("pixel output system value {other} is not handled")),
                }
            }
        }
    }
    for r in &in_regs {
        writeln!(head, "var<private> v{r}: vec4<u32>;").unwrap();
    }
    for r in &out_regs {
        writeln!(head, "var<private> o{r}: vec4<u32>;").unwrap();
    }
    if shader.stage == Stage::Pixel && shader.outputs.iter().any(|el| el.system_value == 65 || el.name.eq_ignore_ascii_case("SV_Depth")) {
        head.push_str("var<private> odepth: vec4<u32>;\n");
    }

    e.line("fn run() {");
    e.indent = 1;
    for r in 0..temps {
        e.line(&format!("var r{r}: vec4<u32>;"));
    }
    for ins in &shader.program.instructions {
        e.instruction(ins)?;
    }
    e.indent = 0;
    e.line("}");

    let mut s = head;
    s.push('\n');
    if !in_fields.is_empty() {
        writeln!(s, "struct In {{\n    {},\n}}\n", in_fields.join(",\n    ")).unwrap();
    }
    writeln!(s, "struct Out {{\n    {},\n}}\n", out_fields.join(",\n    ")).unwrap();
    s.push_str(&e.out);
    let attr = if shader.stage == Stage::Vertex { "@vertex" } else { "@fragment" };
    let param = if in_fields.is_empty() { "" } else { "input: In" };
    writeln!(s, "\n{attr}\nfn main({param}) -> Out {{").unwrap();
    for c in &in_copy {
        writeln!(s, "    {c}").unwrap();
    }
    s.push_str("    run();\n    var output: Out;\n");
    for c in &out_copy {
        writeln!(s, "    {c}").unwrap();
    }
    s.push_str("    return output;\n}\n");
    Ok(s)
}

/// Translates a vertex and pixel shader that are drawn together.
pub fn translate_pair(vs: &Shader, ps: &Shader) -> Result<Translated, String> {
    if vs.stage != Stage::Vertex || ps.stage != Stage::Pixel {
        return Err("translate_pair wants a vertex shader and a pixel shader".into());
    }
    let mut interp = BTreeMap::new();
    for i in &ps.program.instructions {
        if matches!(i.opcode, op::DCL_INPUT_PS) {
            interp.insert(i.operands[0].reg(), i.controls & 0xf);
        }
    }
    // the varyings link by register: each pixel input must be on the same register of the
    // vertex shader, under the same name
    for el in ps.inputs.iter().filter(|el| el.system_value == 0 && interp.contains_key(&el.register)) {
        let found = vs.outputs.iter().any(|v| v.register == el.register && v.name.eq_ignore_ascii_case(&el.name) && v.index == el.index);
        if !found {
            return Err(format!("pixel input {}{} on v{} has no vertex output on the same register", el.name, el.index, el.register));
        }
    }
    let mut depth = BTreeMap::new();
    depth_slots(vs, &mut depth);
    depth_slots(ps, &mut depth);
    let vs_text = stage(vs, &depth, &interp)?;
    let ps_text = stage(ps, &depth, &interp)?;

    let mut textures: BTreeMap<u32, TextureUse> = BTreeMap::new();
    let mut samplers: BTreeMap<u32, SamplerUse> = BTreeMap::new();
    let mut cbuffers: BTreeMap<u32, CBufferUse> = BTreeMap::new();
    for (shader, vertex) in [(vs, true), (ps, false)] {
        for i in &shader.program.instructions {
            let slot = i.operands.first().map(|o| o.reg()).unwrap_or(0);
            match i.opcode {
                op::DCL_RESOURCE => {
                    let t = textures.entry(slot).or_insert(TextureUse { slot, dimension: i.controls & 0x1f, depth: depth.contains_key(&slot), return_type: i.words.first().copied().unwrap_or(5) & 0xf, vertex: false, pixel: false });
                    if vertex { t.vertex = true } else { t.pixel = true }
                }
                op::DCL_SAMPLER => {
                    let s = samplers.entry(slot).or_insert(SamplerUse { slot, comparison: i.controls & 0xf == 1, vertex: false, pixel: false });
                    if vertex { s.vertex = true } else { s.pixel = true }
                }
                op::DCL_CONSTANT_BUFFER => {
                    let size = match i.operands[0].index.get(1) {
                        Some(Index::Imm(n)) => *n * 16,
                        _ => 0,
                    };
                    let c = cbuffers.entry(slot).or_insert(CBufferUse { slot, size, vertex: false, pixel: false });
                    c.size = c.size.max(size);
                    if vertex { c.vertex = true } else { c.pixel = true }
                }
                _ => {}
            }
        }
    }
    let vertex_inputs = vs.inputs.iter().map(|el| (el.name.clone(), el.index, el.register)).collect();
    Ok(Translated { vs: vs_text, ps: ps_text, textures: textures.into_values().collect(), samplers: samplers.into_values().collect(), cbuffers: cbuffers.into_values().collect(), vertex_inputs })
}
