// SPDX-License-Identifier: MIT OR Apache-2.0

//! The shader model 4 token stream (`SHDR`): opcode tokens, their operands and the
//! declarations, decoded as `d3d10tokenizedprogramformat.hpp` lays them out.

/// How a register is picked: a written mask, a four-way swizzle or one component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    None,
    Mask(u8),
    Swizzle([u8; 4]),
    One(u8),
}

/// One index of an operand: an immediate, a register, or a register plus an immediate.
#[derive(Clone, Debug, PartialEq)]
pub enum Index {
    Imm(u32),
    Rel(Box<Operand>, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operand {
    /// `D3D10_SB_OPERAND_TYPE`: 0 temp, 1 input, 2 output, 4 immediate, 6 sampler,
    /// 7 resource, 8 constant buffer, 9 immediate constant buffer, 12 depth out, 13 null
    pub kind: u32,
    pub components: u32,
    pub select: Select,
    pub index: Vec<Index>,
    pub imm: [u32; 4],
    pub neg: bool,
    pub abs: bool,
}

impl Operand {
    pub fn reg(&self) -> u32 {
        match self.index.first() {
            Some(Index::Imm(i)) => *i,
            _ => 0,
        }
    }

    /// The four source components after the swizzle (a mask reads its own components).
    pub fn swizzle(&self) -> [u8; 4] {
        match self.select {
            Select::Swizzle(s) => s,
            Select::One(c) => [c; 4],
            _ => [0, 1, 2, 3],
        }
    }

    /// The written components of a destination.
    pub fn mask(&self) -> u8 {
        match self.select {
            Select::Mask(m) => m,
            Select::One(c) => 1 << c,
            Select::Swizzle(_) => 0xf,
            Select::None => if self.components == 1 { 1 } else { 0xf },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    pub opcode: u32,
    /// bits 11..=23 of the opcode token
    pub controls: u32,
    pub saturate: bool,
    /// `_nz` (true) or `_z` of `if`, `breakc`, `discard`, `retc`
    pub nonzero: bool,
    /// `aoffimmi` texel offsets
    pub offset: [i32; 3],
    pub operands: Vec<Operand>,
    /// extra raw words: `dcl_temps` count, a system-value name, a return type ...
    pub words: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    /// `dcl_immediateConstantBuffer`
    pub icb: Vec<u32>,
}

pub mod op {
    pub const ADD: u32 = 0;
    pub const AND: u32 = 1;
    pub const BREAK: u32 = 2;
    pub const BREAKC: u32 = 3;
    pub const CONTINUE: u32 = 7;
    pub const CONTINUEC: u32 = 8;
    pub const DERIV_RTX: u32 = 11;
    pub const DERIV_RTY: u32 = 12;
    pub const DISCARD: u32 = 13;
    pub const DIV: u32 = 14;
    pub const DP2: u32 = 15;
    pub const DP3: u32 = 16;
    pub const DP4: u32 = 17;
    pub const ELSE: u32 = 18;
    pub const ENDIF: u32 = 21;
    pub const ENDLOOP: u32 = 22;
    pub const EQ: u32 = 24;
    pub const EXP: u32 = 25;
    pub const FRC: u32 = 26;
    pub const FTOI: u32 = 27;
    pub const FTOU: u32 = 28;
    pub const GE: u32 = 29;
    pub const IADD: u32 = 30;
    pub const IF: u32 = 31;
    pub const IEQ: u32 = 32;
    pub const IGE: u32 = 33;
    pub const ILT: u32 = 34;
    pub const IMAD: u32 = 35;
    pub const IMAX: u32 = 36;
    pub const IMIN: u32 = 37;
    pub const INE: u32 = 39;
    pub const INEG: u32 = 40;
    pub const ISHL: u32 = 41;
    pub const ISHR: u32 = 42;
    pub const ITOF: u32 = 43;
    pub const LD: u32 = 45;
    pub const LD_MS: u32 = 46;
    pub const LOG: u32 = 47;
    pub const LOOP: u32 = 48;
    pub const LT: u32 = 49;
    pub const MAD: u32 = 50;
    pub const MIN: u32 = 51;
    pub const MAX: u32 = 52;
    pub const CUSTOMDATA: u32 = 53;
    pub const MOV: u32 = 54;
    pub const MOVC: u32 = 55;
    pub const MUL: u32 = 56;
    pub const NE: u32 = 57;
    pub const NOP: u32 = 58;
    pub const NOT: u32 = 59;
    pub const OR: u32 = 60;
    pub const RESINFO: u32 = 61;
    pub const RET: u32 = 62;
    pub const RETC: u32 = 63;
    pub const ROUND_NE: u32 = 64;
    pub const ROUND_NI: u32 = 65;
    pub const ROUND_PI: u32 = 66;
    pub const ROUND_Z: u32 = 67;
    pub const RSQ: u32 = 68;
    pub const SAMPLE: u32 = 69;
    pub const SAMPLE_C: u32 = 70;
    pub const SAMPLE_C_LZ: u32 = 71;
    pub const SAMPLE_L: u32 = 72;
    pub const SAMPLE_B: u32 = 74;
    pub const SQRT: u32 = 75;
    pub const SINCOS: u32 = 77;
    pub const ULT: u32 = 79;
    pub const UGE: u32 = 80;
    pub const UMAX: u32 = 83;
    pub const UMIN: u32 = 84;
    pub const USHR: u32 = 85;
    pub const UTOF: u32 = 86;
    pub const XOR: u32 = 87;
    pub const DCL_RESOURCE: u32 = 88;
    pub const DCL_CONSTANT_BUFFER: u32 = 89;
    pub const DCL_SAMPLER: u32 = 90;
    pub const DCL_INPUT: u32 = 95;
    pub const DCL_INPUT_SGV: u32 = 96;
    pub const DCL_INPUT_SIV: u32 = 97;
    pub const DCL_INPUT_PS: u32 = 98;
    pub const DCL_INPUT_PS_SGV: u32 = 99;
    pub const DCL_INPUT_PS_SIV: u32 = 100;
    pub const DCL_OUTPUT: u32 = 101;
    pub const DCL_OUTPUT_SGV: u32 = 102;
    pub const DCL_OUTPUT_SIV: u32 = 103;
    pub const DCL_TEMPS: u32 = 104;
    pub const DCL_INDEXABLE_TEMP: u32 = 105;
    pub const DCL_GLOBAL_FLAGS: u32 = 106;
}

struct Reader<'a> {
    w: &'a [u32],
    at: usize,
}

impl Reader<'_> {
    fn next(&mut self) -> Result<u32, String> {
        let v = *self.w.get(self.at).ok_or("token stream ends inside an instruction")?;
        self.at += 1;
        Ok(v)
    }

    fn operand(&mut self) -> Result<Operand, String> {
        let t = self.next()?;
        let components = match t & 3 {
            0 => 0,
            1 => 1,
            2 => 4,
            _ => return Err("N-component operands are not shader model 4".into()),
        };
        let select = if components == 4 {
            match (t >> 2) & 3 {
                0 => Select::Mask(((t >> 4) & 0xf) as u8),
                1 => Select::Swizzle([((t >> 4) & 3) as u8, ((t >> 6) & 3) as u8, ((t >> 8) & 3) as u8, ((t >> 10) & 3) as u8]),
                _ => Select::One(((t >> 4) & 3) as u8),
            }
        } else {
            Select::None
        };
        let kind = (t >> 12) & 0xff;
        let dims = (t >> 20) & 3;
        let (mut neg, mut abs) = (false, false);
        if t >> 31 != 0 {
            let e = self.next()?;
            if e & 0x3f == 1 {
                let m = (e >> 6) & 0xff;
                neg = m & 1 != 0;
                abs = m & 2 != 0;
            }
        }
        let mut imm = [0; 4];
        if kind == 4 {
            for v in imm.iter_mut().take(components as usize) {
                *v = self.next()?;
            }
            if components == 1 {
                imm = [imm[0]; 4];
            }
        } else if kind == 5 {
            return Err("64-bit immediates are not handled".into());
        }
        let mut index = Vec::with_capacity(dims as usize);
        for d in 0..dims {
            let repr = (t >> (22 + 3 * d)) & 7;
            index.push(match repr {
                0 => Index::Imm(self.next()?),
                2 => Index::Rel(Box::new(self.operand()?), 0),
                3 => {
                    let base = self.next()?;
                    Index::Rel(Box::new(self.operand()?), base)
                }
                _ => return Err(format!("index representation {repr} is not handled")),
            });
        }
        Ok(Operand { kind, components, select, index, imm, neg, abs })
    }
}

/// Decodes the instructions after the version and length words.
pub fn decode(w: &[u32]) -> Result<Program, String> {
    let mut program = Program::default();
    let mut at = 0;
    while at < w.len() {
        let t = w[at];
        let opcode = t & 0x7ff;
        if opcode == op::CUSTOMDATA {
            let len = *w.get(at + 1).ok_or("customdata without its length")? as usize;
            if len < 2 || at + len > w.len() {
                return Err("customdata past the end".into());
            }
            // class 3: the immediate constant buffer
            if t >> 11 == 3 {
                program.icb = w[at + 2..at + len].to_vec();
            }
            at += len;
            continue;
        }
        let len = ((t >> 24) & 0x7f) as usize;
        if len == 0 || at + len > w.len() {
            return Err(format!("instruction {opcode} at word {at} has a bad length"));
        }
        let mut r = Reader { w: &w[..at + len], at: at + 1 };
        let mut offset = [0; 3];
        let mut ext = t >> 31 != 0;
        while ext {
            let e = r.next()?;
            ext = e >> 31 != 0;
            if e & 0x3f == 1 {
                let s4 = |v: u32| ((v as i32) << 28) >> 28;
                offset = [s4(e >> 9), s4(e >> 13), s4(e >> 17)];
            }
        }
        let controls = (t >> 11) & 0x1fff;
        let mut ins = Instruction { opcode, controls, saturate: t & (1 << 13) != 0, nonzero: t & (1 << 18) != 0, offset, operands: Vec::new(), words: Vec::new() };
        match opcode {
            // declarations made of raw words only
            op::DCL_GLOBAL_FLAGS | op::DCL_TEMPS => {}
            op::DCL_INDEXABLE_TEMP => return Err("indexable temporaries are not handled".into()),
            op::DCL_RESOURCE => {
                ins.operands.push(r.operand()?);
            }
            op::DCL_INPUT_SGV | op::DCL_INPUT_SIV | op::DCL_INPUT_PS_SGV | op::DCL_INPUT_PS_SIV | op::DCL_OUTPUT_SGV | op::DCL_OUTPUT_SIV => {
                ins.operands.push(r.operand()?);
            }
            op::DCL_CONSTANT_BUFFER | op::DCL_SAMPLER | op::DCL_INPUT | op::DCL_INPUT_PS | op::DCL_OUTPUT => {
                ins.operands.push(r.operand()?);
            }
            op::NOP | op::ELSE | op::ENDIF | op::LOOP | op::ENDLOOP | op::BREAK | op::CONTINUE | op::RET => {}
            _ if !(op::DCL_RESOURCE..=op::DCL_GLOBAL_FLAGS).contains(&opcode) => {
                while r.at < at + len {
                    ins.operands.push(r.operand()?);
                }
            }
            _ => return Err(format!("declaration {opcode} is not handled")),
        }
        while r.at < at + len {
            ins.words.push(r.next()?);
        }
        program.instructions.push(ins);
        at += len;
    }
    Ok(program)
}
