// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! YEBIS's shader container (`TexUtil_D3D11`, a `.tfxo file`) read out of the player's own
//! `acs.exe` at run time: `GetData_ppfxTexUtil_tfxo` 0x140439ce0 hands it to
//! `CTinyEffect::CreateShaderProgram` 0x14043db70. None of its bytes are kept in rustyAC.
//!
//! Layout (docs/atlas/yebis.md s4): a 0x40 header of zero-padded text fields (magic, text
//! length, index length, program count, `hlsl`), a text index of
//! `tech_<name>,<vs>,<vs index>,<ps>,<ps index>` lines, then one 256-byte record per program
//! (224 bytes of name, offset and size as decimal text) and the programs back to back.

use std::path::Path;

pub struct Technique {
    pub name: String,
    pub vs: usize,
    pub ps: usize,
}

pub struct Tfxo {
    data: Vec<u8>,
    /// (name, start, size) into `data`
    programs: Vec<(String, usize, usize)>,
    pub techniques: Vec<Technique>,
}

fn field(bytes: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    text.trim_end_matches('\0').parse().map_err(|_| format!("tfxo: bad number field {text:?}"))
}

impl Tfxo {
    /// Finds the container inside `acs.exe`'s bytes: the `.tfxo file` magic followed by
    /// `hlsl` at +0x38 (the other hit of the magic is an error string).
    pub fn from_exe(exe: &[u8]) -> Result<Tfxo, String> {
        const MAGIC: &[u8; 16] = b".tfxo file\0\0\0\0\0\0";
        let base = (0..exe.len().saturating_sub(0x40))
            .find(|&i| exe[i..i + 16] == *MAGIC && exe[i + 0x38..i + 0x3c] == *b"hlsl")
            .ok_or("tfxo: no YEBIS shader container in acs.exe")?;
        let total = field(&exe[base + 0x10..base + 0x20])?;
        let index_len = field(&exe[base + 0x20..base + 0x30])?;
        let count = field(&exe[base + 0x30..base + 0x38])?;
        let data = exe.get(base..base + 0x40 + total).ok_or("tfxo: container runs past acs.exe")?.to_vec();
        let records = 0x40 + index_len;
        let first = records + count * 256;
        let mut programs = Vec::with_capacity(count);
        for k in 0..count {
            let r = data.get(records + k * 256..records + k * 256 + 256).ok_or("tfxo: records run past the end")?;
            let name = String::from_utf8_lossy(&r[..224]).trim_end_matches('\0').to_string();
            let (start, size) = (first + field(&r[224..240])?, field(&r[240..256])?);
            if start + size > data.len() || data.get(start..start + 4) != Some(b"DXBC") {
                return Err(format!("tfxo: program {k} {name} is not a DXBC blob"));
            }
            programs.push((name, start, size));
        }
        let index = String::from_utf8_lossy(&data[0x40..records]);
        let mut techniques = Vec::new();
        for line in index.lines().filter(|l| l.starts_with("tech_")) {
            let f: Vec<&str> = line.split(',').collect();
            let [name, _, vs, _, ps] = f[..] else { return Err(format!("tfxo: bad technique line {line}")) };
            let (vs, ps) = (vs.parse().map_err(|_| line.to_string())?, ps.parse().map_err(|_| line.to_string())?);
            if vs >= count || ps >= count {
                return Err(format!("tfxo: technique {name} points past the programs"));
            }
            techniques.push(Technique { name: name.to_string(), vs, ps });
        }
        Ok(Tfxo { data, programs, techniques })
    }

    /// Reads `acs.exe` from Assetto Corsa's folder.
    pub fn from_install(root: &Path) -> Result<Tfxo, String> {
        let exe = std::fs::read(root.join("acs.exe")).map_err(|e| format!("acs.exe: {e}"))?;
        Tfxo::from_exe(&exe)
    }

    pub fn program_count(&self) -> usize {
        self.programs.len()
    }

    pub fn program_name(&self, index: usize) -> &str {
        &self.programs[index].0
    }

    pub fn program(&self, index: usize) -> &[u8] {
        let (_, start, size) = self.programs[index];
        &self.data[start..start + size]
    }

    /// The vertex and pixel programs of `tech_<name>`.
    pub fn technique(&self, name: &str) -> Option<(&[u8], &[u8])> {
        let t = self.techniques.iter().find(|t| t.name.strip_prefix("tech_") == Some(name))?;
        Some((self.program(t.vs), self.program(t.ps)))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::gpulog::hash_bytes;

    /// The techniques of the default frame (docs/atlas/yebis.md s8) and the shader tags the
    /// oracle's real-YEBIS log (re/scratch/taskY1/y1) shows bound for them.
    const FRAME: [(&str, u64, u64); 7] = [
        ("MadAlphaColorT2_R_S1", 0x20c403425f52bd2a, 0xe63f5c5eb7f2a381),
        ("TransformMadT1_S5", 0xf2401e2969114091, 0xe2def5179bee7285),
        ("TonemapHDR_Exposure", 0xb3244b52ca4b93b5, 0xd2fdb9385624e376),
        ("TonemapHDR_PreMatrix_Dither_Vignette_Gamma", 0xb3244b52ca4b93b5, 0xb8549c3696468875),
        ("DepthOfField_BlurFactorAlphaFromDepthTextureTypeCastT1_S1_RVALUE_ALPHA_RGB", 0x5c593d0d18e76cf2, 0xa7cdd8559f4bb6dd),
        ("TonemapHDR_Dither_Vignette_Exposure_LINEARSAT", 0xb3244b52ca4b93b5, 0x30316e885216aafd),
        ("CompositeColorAndAlphaEffectsMask_A_A", 0x2e98bed6aa6d52d1, 0x1200229cbcb9f2b7),
    ];

    #[test]
    fn frame_shaders_are_the_ones_the_game_binds() {
        let Some(root) = rustyac_content::install::ac_root() else {
            eprintln!("no Assetto Corsa install: skipped");
            return;
        };
        let tfxo = Tfxo::from_install(&root).unwrap();
        assert_eq!(tfxo.program_count(), 2649);
        assert_eq!(tfxo.techniques.len(), 2653);
        for (name, vs, ps) in FRAME {
            let (v, p) = tfxo.technique(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!((hash_bytes(v), hash_bytes(p)), (vs, ps), "{name}");
        }
    }
}
