// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//! YEBIS's settings block and its 4 KB constant buffer for the 8 draws of AC's default frame
//! (docs/atlas/yebis.md s8, Task Y2a).
//!
//! Step 1, `PostEffect`: the `PPFX::CPostEffect` fields this chain reads, filled as
//! `CameraForwardYebis::setPostProcessing` 0x140025080 fills them from the ppfilter
//! (`readPPSetOptions` 0x140026150), video.ini and the camera, and `round_two()`: what
//! `EndPostEffectScene_Process` 0x1403b84d0 forces for `PFXALPHAOUT_EFFECTRESULT_ALPHA`.
//!
//! Step 2, `Cbuffer`: YEBIS keeps one global parameter store; each draw's setters write into it
//! and the upload zeroes 4096 bytes and copies every variable the draw's two programs use (whole
//! declared size). Leftovers carry over between draws and frames, so a frame from a fresh
//! store differs from the game's; the second frame is the steady state the game logs.
//!
//! Not ported (not on this chain): glare, DOF, light shafts, heat shimmer, lens distortion,
//! feedback, vignette simulation, HUE / SEPIA, auto exposure over time (Y3), tone-map
//! functions other than 0, 1 and the filmic default.

use rustyac_math::powf;
use rustyac_physics::data::ini::IniReader;

/// Program pair of each draw, in order: round one 1-4, round two 5-7, composite 8.
pub const TECHNIQUES: [&str; 8] = [
    "MadAlphaColorT2_R_S1",
    "TransformMadT1_S5",
    "TonemapHDR_Exposure",
    "TonemapHDR_PreMatrix_Dither_Vignette_Gamma",
    "DepthOfField_BlurFactorAlphaFromDepthTextureTypeCastT1_S1_RVALUE_ALPHA_RGB",
    "TransformMadT1_S5",
    "TonemapHDR_Dither_Vignette_Exposure_LINEARSAT",
    "CompositeColorAndAlphaEffectsMask_A_A",
];

// $Globals byte offsets (c-register x 16)
const DEPTH_CAST: usize = 0;
const SCREEN_SPACE_SCALE: usize = 80;
const CONVERT_DEPTH: usize = 272;
const MODULATE: usize = 2464;
const OFFSET: usize = 2976;
const GAMMA: usize = 3232;
const DITHER: usize = 3236;
const MAX_MAPPING_LUM: usize = 3328;
const PRE_TONEMAP: usize = 3424;
const PRE_TONEMAP_GLARE: usize = 3488;
const TRANSFORM: usize = 3584;

const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
const LUM: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// What the camera and the frame hand YEBIS (`updateYebisParameters` 0x140029e30).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub near: f32,
    pub far: f32,
    pub fov_deg: f32,
    pub width: u32,
    pub height: u32,
}

/// The `CPostEffect` fields of this chain; the comments name their offsets.
#[derive(Clone, Debug, PartialEq)]
pub struct PostEffect {
    pub near: f32,   // +0x52c
    pub far: f32,    // +0x530
    pub fov: f32,    // +0x534, radians
    pub aspect: f32,          // +0x2fc
    pub exposure: f32,        // +0x6f4
    pub exposure_scale: f32,  // +0x660
    pub gamma: f32,           // +0x6f8
    pub tonemap: i32,         // +0x6fc, PFXTM_*
    pub mapping: f32,         // +0x700
    pub mapping_div: f32,     // +0x704
    pub color: [f32; 4],      // +0x708
    pub alpha_out_scale: f32, // +0x714
    pub curve: f32,           // +0x718, SetCharacteristicCurveParameters
    pub dither: f32,          // +0x75c
    pub vignette_optics: bool, // +0x950
    pub vignette: [f32; 4],   // +0x958..+0x964
    pub alpha_out: i32,       // +0x458, 2 = PFXALPHAOUT_EFFECTRESULT_ALPHA
    pub color_matrix: [f32; 16], // PFXMTX_TONEMAP_COLOR_HDRSPACE
    pub ca_samples: i32,      // +0x994
    pub ca_lateral: [f32; 2], // +0x99c
    pub ca_uniform: [f32; 2], // +0x9a4
    pub depth_cast: [f32; 4], // +0x410
    pub depth: [f32; 5],      // +0xb8c..+0xb9c
}

impl PostEffect {
    /// `filter` is `system/cfg/ppfilters/<[POST_PROCESS] FILTER>.ini`.
    pub fn new(filter: &IniReader, video: &IniReader, cam: &Camera) -> Result<PostEffect, String> {
        for key in ["DOF", "GLARE", "RAYS_OF_GOD", "HEAT_SHIMMER"] {
            if video.get_int("POST_PROCESS", key).unwrap_or(0) != 0 {
                return Err(format!("yebis: [POST_PROCESS] {key} is on: not ported (Y5-Y7)"));
            }
        }
        let f = |s: &str, k: &str| filter.get_float(s, k);
        let i = |s: &str, k: &str| filter.get_int(s, k);
        if i("TONEMAPPING", "HDR")? != 1 {
            return Err("yebis: [TONEMAPPING] HDR=0 (PFXTM_REMAP_REINHARD_SENSITOMETRIC) not ported".into());
        }
        // UpdateParameters_ChangeFormat 0x1403b8f00: -1 becomes PFXTM_FILMIC (2) with AC's formats
        let tonemap = match i("TONEMAPPING", "FUNCTION")? {
            -1 => 2,
            v @ 0..=2 => v,
            v => return Err(format!("yebis: [TONEMAPPING] FUNCTION={v} not ported")),
        };
        if i("CHROMATIC_ABERRATION", "ENABLED")? == 0 {
            return Err("yebis: chromatic aberration off changes the draw list: not ported".into());
        }
        // shortcut: auto exposure is taken as settled at MAX, as in the oracle's frames; Y3 ports adaptation
        let exposure = if i("AUTO_EXPOSURE", "ENABLED")? != 0 { f("AUTO_EXPOSURE", "MAX")? } else { f("TONEMAPPING", "EXPOSURE")? };

        let mut m = IDENTITY;
        if f("COLOR", "HUE")? != 0.0 || f("COLOR", "SEPIA")? != 0.0 {
            return Err("yebis: [COLOR] HUE / SEPIA not ported".into());
        }
        let v = f("COLOR", "SATURATION")?;
        if v != 1.0 {
            m = mul(&m, &saturation(v));
        }
        let v = f("COLOR", "BRIGHTNESS")?;
        if v != 1.0 {
            m = mul(&m, &diag(v, v, v));
        }
        let v = f("COLOR", "CONTRAST")?;
        if v != 1.0 {
            let mut c = diag(v, v, v);
            let t = (1.0 - v) * 0.5;
            c[12..15].copy_from_slice(&[t, t, t]);
            m = mul(&m, &c);
        }
        for (key, invert) in [("COLOR_TEMP", false), ("WHITE_BALANCE", true)] {
            let k = f("COLOR", key)?;
            if k != 6500.0 {
                let c = rgb_from_temperature(k, invert)?;
                m = mul(&m, &diag(c[0], c[1], c[2]));
            }
        }

        let near = cam.near;
        let far = cam.far.clamp(1.0, 1000.0);
        // UpdateParameters_DepthOfFieldOpticalParameters 0x1403b92f0, DOF off, float depth formats
        let b8c = (near / (near - far)) * 64000.0;
        let b90 = -64000.0f32;
        Ok(PostEffect {
            near,
            far,
            fov: cam.fov_deg * 0.017453,
            aspect: cam.width as f32 / cam.height as f32,
            exposure: exposure.max(f32::MIN_POSITIVE),
            exposure_scale: 1.0,
            gamma: f("TONEMAPPING", "GAMMA")?.max(0.0),
            tonemap,
            mapping: f("TONEMAPPING", "MAPPING_FACTOR")?,
            mapping_div: 1.0,
            color: [0.0, 0.0, 0.0, 1.0],
            alpha_out_scale: 1.0,
            curve: 0.5,
            dither: -f32::MAX,
            vignette_optics: false,
            vignette: [1.0, 0.0, 0.5, 0.5],
            alpha_out: 2,
            color_matrix: m,
            ca_samples: i("CHROMATIC_ABERRATION", "SAMPLES")?,
            ca_lateral: filter.get_float2("CHROMATIC_ABERRATION", "LATERAL_DISPERSION")?,
            ca_uniform: filter.get_float2("CHROMATIC_ABERRATION", "UNIFORM_DISPERSION")?,
            // measured (state dump of r23_09): the depth texture's cast for AC's float depth
            depth_cast: [1.0, 0.0, 1.0, -0.0],
            depth: [b8c, b90, 1.0 / b8c, -(b90 / b8c), far],
        })
    }

    /// Round two of `PFXALPHAOUT_EFFECTRESULT_ALPHA`: exposure, gamma, mapping and the colour
    /// matrices forced to 1 / identity, tone map PFXTM_LINEARSAT.
    pub fn round_two(&self) -> PostEffect {
        PostEffect {
            exposure: 1.0,
            exposure_scale: 1.0,
            gamma: 1.0,
            tonemap: 1,
            mapping: 1.0,
            mapping_div: 1.0,
            color_matrix: IDENTITY,
            ..self.clone()
        }
    }

    /// GetDepthConversionFactor 0x1403c3600 from CURRENT (0) to EFFECTFACTOR_OPTIMIZED (2).
    fn depth_factor(&self) -> [f32; 4] {
        let [b8c, b90, _, _, b9c] = self.depth;
        let (near, far) = (self.near, self.far);
        let mut l = ((far - near) * b9c) / (far * near);
        let mut y = l * 0.0 + (1.0 - b9c / near);
        l *= b8c;
        y = b8c * y + b90;
        [l, y, 1.0 / l, -(y / l)]
    }
}

/// The filmic curve table (`GetFimlicCurveNormalizationParameter` 0x14040d940) read from the
/// player's `acs.exe` (21 floats at file offset 0x527cc0, VA 0x1405290c0).
pub fn filmic_table(exe: &[u8]) -> Result<[f32; 21], String> {
    let b = exe.get(0x527cc0..0x527cc0 + 84).ok_or("yebis: acs.exe too short for the filmic table")?;
    let t: [f32; 21] = std::array::from_fn(|k| f32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap()));
    if t[0] != 0.0 || t[20] != 1.0 || t.windows(2).any(|w| w[0] >= w[1]) {
        return Err("yebis: no filmic table at 0x527cc0 (not acs.exe 1.16?)".into());
    }
    Ok(t)
}

fn diag(x: f32, y: f32, z: f32) -> [f32; 16] {
    [x, 0., 0., 0., 0., y, 0., 0., 0., 0., z, 0., 0., 0., 0., 1.]
}

/// MATRIXT44_MatrixMultiply: a x b, row-major.
fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|k| {
        let (i, j) = (k / 4, k % 4);
        ((a[i * 4] * b[j] + a[i * 4 + 1] * b[4 + j]) + a[i * 4 + 2] * b[8 + j]) + a[i * 4 + 3] * b[12 + j]
    })
}

/// GPUMath_MatrixInverse<float> 0x1403cc630: Gauss-Jordan on columns with a column permutation.
fn inverse(m: &[f32; 16]) -> [f32; 16] {
    let mut a = *m;
    let mut inv = IDENTITY;
    let mut p = [0usize, 1, 2, 3];
    for k in 0..4 {
        let mut best = k;
        for j in k + 1..4 {
            if a[k * 4 + p[best]].abs() < a[k * 4 + p[j]].abs() {
                best = j;
            }
        }
        if a[k * 4 + p[best]] == 0.0 {
            return inv;
        }
        p.swap(k, best);
        let c = p[k];
        let f = 1.0 / a[k * 4 + c];
        for r in k..4 {
            a[r * 4 + c] *= f;
        }
        for r in 0..4 {
            inv[r * 4 + c] *= f;
        }
        for i in (0..4).filter(|&i| i != k) {
            let col = p[i];
            let g = a[k * 4 + col] / a[k * 4 + c];
            for r in k..4 {
                a[r * 4 + col] -= g * a[r * 4 + c];
            }
            for r in 0..4 {
                inv[r * 4 + col] -= g * inv[r * 4 + c];
            }
        }
    }
    std::array::from_fn(|k| inv[(k / 4) * 4 + p[k % 4]])
}

/// MultiplyColorMatrixSaturation, luminance weights, not linear-preserving (param_5 false).
fn saturation(s: f32) -> [f32; 16] {
    let [lr, lg, lb] = LUM;
    let mut m = [0.0; 16];
    m[0] = (1.0 - lr) * s + lr;
    m[1] = (0.0 - lr) * s + lr;
    m[2] = m[1];
    m[4] = (0.0 - lg) * s + lg;
    m[5] = (1.0 - lg) * s + lg;
    m[6] = m[4];
    m[8] = (0.0 - lb) * s + lb;
    m[9] = m[8];
    m[10] = (1.0 - lb) * s + lb;
    m[15] = 1.0;
    m
}

/// GPUMath_ColorMatrix_XYZ_to_RGB 0x1403a1630 (sRGB).
const XYZ_TO_RGB: [u32; 12] = [
    0x404f6402, 0xbf782129, 0x3d63ef28, 0, 0xbfc4c155, 0x3ff02079, 0xbe50f0a6, 0, 0xbeff3ffb, 0x3d2a36a0, 0x3f8755f8, 0,
];

/// Vec3Transform with a 4x3 matrix (translation 0 here).
fn xyz_to_rgb(v: [f32; 3]) -> [f32; 3] {
    let m: [f32; 12] = XYZ_TO_RGB.map(f32::from_bits);
    let [x, y, z] = v;
    [((y * m[4] + x * m[0]) + z * m[8]) + 0.0, ((x * m[1] + y * m[5]) + z * m[9]) + 0.0, ((x * m[2] + y * m[6]) + z * m[10]) + 0.0]
}

/// CIE xy of a black body, K in [4000, 25000] (the CIE daylight fit, in f64).
fn cie_xy(k: f32) -> Result<[f32; 2], String> {
    if k < 4000.0 {
        return Err(format!("yebis: colour temperature {k} K under 4000 not ported"));
    }
    let d = k as f64;
    let d2 = d * d;
    let x = if k > 7000.0 {
        (1901800.0 / d2 - 2006400000.0000002 / (d2 * d)) + 247.48000000000002 / d + 0.23704
    } else {
        (2967800.0 / d2 - 4607000000.0 / (d2 * d)) + 99.11 / d + 0.244063
    };
    Ok([x as f32, ((x * 2.87 - x * x * 3.0) - 0.275) as f32])
}

fn rgb_of(k: f32) -> Result<[f32; 3], String> {
    let [x, y] = cie_xy(k)?;
    let w = 1.0 / y;
    Ok(xyz_to_rgb([x * w, 1.0, ((1.0 - x) - y) * w]))
}

/// RGBFromColorTemperature 0x1403ccca0 with Y = 1, luminance 1, linear (the colour matrix's
/// MultiplyMatrixColorTemperature 0x1403caa60 / WhiteBalance 0x1403cab60, which inverts).
fn rgb_from_temperature(k: f32, invert: bool) -> Result<[f32; 3], String> {
    let k = k.clamp(2000.0, 25000.0);
    if k == 6500.0 {
        return Ok([1.0; 3]);
    }
    let d65 = rgb_of(6500.0)?;
    let c = rgb_of(k)?;
    let mut v: [f32; 3] = std::array::from_fn(|n| c[n] * (1.0 / d65[n]));
    if invert {
        v = v.map(|e| 1.0 / e);
    }
    let v = v.map(|e| if e <= 0.0 { 0.0 } else { e });
    let sum = (v[1] * LUM[1] + v[0] * LUM[0]) + v[2] * LUM[2];
    Ok(if sum != 0.0 { v.map(|e| e * (1.0 / sum)) } else { v })
}

/// The 3 / 5 / 7-tap colour weights of DrawRectGPU_ChromaticAberration 0x1403f0120 (code
/// immediates).
fn ca_weights(n: usize) -> Vec<[f32; 4]> {
    let w: &[[u32; 4]] = match n {
        3 => &[[0x3f800000, 0, 0, 0x3eaaaaab], [0, 0x3f800000, 0, 0x3eaaaaab], [0, 0, 0x3f800000, 0x3eaaaaab]],
        5 => &[
            [0x3f155556, 0, 0, 0x3e4cccce],
            [0x3ed55556, 0x3e2aaaab, 0, 0x3e4cccce],
            [0, 0x3f2aaaab, 0, 0x3e4cccce],
            [0, 0x3e2aaaab, 0x3eaaaaab, 0x3e4cccce],
            [0, 0, 0x3f2aaaab, 0x3e4cccce],
        ],
        _ => &[
            [0x3e266667, 0, 0, 0x3e124925],
            [0x3f100000, 0, 0, 0x3e124925],
            [0x3e800000, 0x3e800000, 0, 0x3e124925],
            [0, 0x3f000000, 0, 0x3e124925],
            [0, 0x3e800000, 0x3e800000, 0x3e124925],
            [0, 0, 0x3f000000, 0x3e124925],
            [0x3ccccccd, 0, 0x3e800000, 0x3e124925],
        ],
    };
    w.iter().map(|r| r.map(f32::from_bits)).collect()
}

/// YEBIS's global parameter store, 4096 bytes laid out as `$Globals`.
pub struct Cbuffer {
    store: [u8; 4096],
}

impl Default for Cbuffer {
    fn default() -> Cbuffer {
        Cbuffer { store: [0; 4096] }
    }
}

impl Cbuffer {
    fn put(&mut self, at: usize, v: &[f32]) {
        for (k, e) in v.iter().enumerate() {
            self.store[at + k * 4..at + k * 4 + 4].copy_from_slice(&e.to_le_bytes());
        }
    }

    /// Matrices go up transposed (column i of the CPU matrix is register i).
    fn put_matrix(&mut self, at: usize, m: &[f32; 16]) {
        self.put(at, &std::array::from_fn::<f32, 16, _>(|k| m[(k % 4) * 4 + k / 4]));
    }

    /// The bytes one draw uploads: zeros, then each used (offset, size) range from the store.
    pub fn upload(&self, used: &[(usize, usize)]) -> [u8; 4096] {
        let mut out = [0; 4096];
        for &(o, s) in used {
            out[o..o + s].copy_from_slice(&self.store[o..o + s]);
        }
        out
    }

    /// One frame's 8 uploads; `used[d]` are the variable ranges of `TECHNIQUES[d]`.
    pub fn frame(&mut self, pe: &PostEffect, filmic: &[f32; 21], used: &[Vec<(usize, usize)>; 8]) -> Vec<[u8; 4096]> {
        let two = pe.round_two();
        let mut out = Vec::with_capacity(8);
        self.put(SCREEN_SPACE_SCALE, &[1.0, -1.0]);
        for draw in 0..8 {
            match draw {
                0 => self.depth_alpha(pe),
                1 | 5 => self.chromatic_aberration(pe),
                2 => self.tonemap(pe, filmic, false),
                3 => self.tonemap(pe, filmic, true),
                4 => self.depth_blur_factor(pe),
                6 => self.tonemap(&two, filmic, true),
                _ => self.put(MODULATE, &[if pe.alpha_out == 2 { pe.alpha_out_scale } else { 1.0 }, 0.0, 0.0, 0.0]),
            }
            out.push(self.upload(&used[draw]));
        }
        out
    }

    /// Draw 1: DrawRectGPU_DepthOfField_FromBlurFactorTexture 0x1403f48b0 (depth into alpha).
    fn depth_alpha(&mut self, pe: &PostEffect) {
        let c = pe.depth_factor();
        let p = pe.depth_cast;
        let s = c[0] * p[2];
        let o = c[1] + p[3] * c[0];
        self.put(MODULATE, &[s, s, s, s, 1.0, 1.0, 1.0, 1.0]);
        self.put(OFFSET, &[o, o, o, o]);
    }

    /// Draw 5: DrawRectGPU_DepthOfField_BlurFactorAlphaFromDepthTexture (_Process 0x1403f4030).
    fn depth_blur_factor(&mut self, pe: &PostEffect) {
        let c = pe.depth_cast;
        self.put(DEPTH_CAST, &[c[2], c[3], c[0], c[1]]);
        self.put(CONVERT_DEPTH, &pe.depth_factor());
        self.put(MODULATE, &[1.0; 4]);
    }

    /// Draws 2 / 6: DrawRectGPU_ChromaticAberration 0x1403f0120.
    fn chromatic_aberration(&mut self, pe: &PostEffect) {
        let n = if pe.ca_samples < 4 { 3 } else if pe.ca_samples > 6 { 7 } else { 5 };
        let w = ca_weights(n);
        self.put(MODULATE, w.as_flattened());
        let nf = n as f32;
        let step = if n == 7 { 0.25 } else { 1.0 / (nf - 1.0) };
        // p7, p8: the source rect in texture coordinates, 1 x 1
        let mut s = diag(1.0 / (pe.aspect * 2.0), 0.5, 1.0);
        s[12..15].copy_from_slice(&[0.5, 0.5, 0.0]);
        let si = inverse(&s);
        let lat = pe.ca_lateral.map(|v| v * -1.0);
        let uni = pe.ca_uniform;
        let sign = |v: f32| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
        let kx = powf(lat[0].abs() + 1.0, sign(lat[0]) * step);
        let ky = powf(lat[1].abs() + 1.0, sign(lat[1]) * step);
        let (px, py) = (powf(kx, nf), powf(ky, nf));
        let mut dx = if px > 1.0 { 1.0 / px } else { 1.0 };
        let mut dy = if py > 1.0 { 1.0 / py } else { 1.0 };
        let mut tx = uni[0] * -1.0 * 0.5 * -1.0;
        let mut ty = uni[1] * -1.0 * 0.5 * -1.0;
        for tap in 0..n {
            let mut m = mul(&IDENTITY, &si);
            m = mul(&m, &diag(dx, dy, 1.0));
            let mut t = IDENTITY;
            t[12] = tx;
            t[13] = ty;
            m = mul(&m, &t);
            m = mul(&m, &s);
            self.put_matrix(TRANSFORM + tap * 64, &m);
            dx *= kx;
            dy *= ky;
            tx += uni[0] * -1.0 * step;
            ty += uni[1] * -1.0 * step;
        }
    }

    /// Draws 3 / 4 / 7: DrawRectGPU_TonemapHDR 0x1403f1b10. `last` is the tone map into the
    /// destination (draws 4 and 7), which gets the vignette offsets, gamma and dither; draw 3 is
    /// TonemapToSurface into the luminance target.
    fn tonemap(&mut self, pe: &PostEffect, filmic: &[f32; 21], last: bool) {
        // the +0x50c table: PFXTM_* -> EHDRTONEMAP
        let id = [0, 1, 10, 2, 4, 6, 8, 14, 1][pe.tonemap as usize];
        // ids 0, 1 and 10 (the only ones ported) skip the 0xbaa8 rescale and take p6 /= p13
        let p6 = (pe.exposure / pe.exposure_scale) / pe.mapping_div;
        // SetTonemapEffectParameters 0x1403e5470
        let lum = if id == 10 {
            let c = powf(pe.curve, 0.33333334);
            [c * 2.0891273 + 1.0, filmic_norm(filmic, c), 0.0, 0.0]
        } else {
            [0.0; 4]
        };
        self.put(MAX_MAPPING_LUM, &lum);
        let c = pe.color;
        self.put(MODULATE, &[p6, 0.0, 1.0, c[3], c[0], c[1], c[2], c[3]]);
        let (p28, p29) = if pe.vignette_optics { (pe.vignette[0], pe.vignette[1]) } else { (0.0, 0.0) };
        let (p30, p31) = if last { (pe.vignette[2], pe.vignette[3]) } else { (0.0, 0.0) };
        let tex = f32::from_bits((p29 * 0.1) as u32);
        self.put(OFFSET, &[2.0, -1.0, tex, 0.0, p31, 0.0, 0.0, 0.0, 2.0, -1.0, p28 * 0.5, 1.0, p30, 0.0, 0.0, 0.0]);
        if !last {
            return;
        }
        if pe.gamma != 0.0 && pe.gamma != 1.0 {
            self.put(GAMMA, &[1.0 / pe.gamma]);
        }
        // p16 (ColorTransformMatrix) is the identity in AC's frame: never written
        if pe.color_matrix != IDENTITY {
            self.put_matrix(PRE_TONEMAP, &mul(&diag(p6, p6, p6), &pe.color_matrix));
            self.put_matrix(PRE_TONEMAP_GLARE, &mul(&diag(0.0, 0.0, 0.0), &pe.color_matrix));
        }
        let f = if pe.dither == -f32::MAX { 0.003921569 } else { pe.dither };
        if f != f32::MAX && f != 0.0 {
            self.put(DITHER, &[f, f * -0.5]);
        }
    }
}

/// GetFimlicCurveNormalizationParameter 0x14040d940: linear lookup in 0.05 steps.
fn filmic_norm(t: &[f32; 21], v: f32) -> f32 {
    let x = (v.clamp(0.0, 1.0) - 0.0) / 0.05;
    let i = x.floor() as i32;
    let j = if i + 1 > 19 { 20 } else { i + 1 };
    let (i, j) = (i as usize, j as usize);
    (t[j] - t[i]) * (x - i as f32) + t[i]
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::yebis_tfxo::Tfxo;
    use std::path::{Path, PathBuf};

    const SCENES: [(&str, &str); 3] = [("r23_09", "spa_chase"), ("r23_10", "spa_chase"), ("r23_16", "monza_chase")];
    const NAMES: [&str; 8] = ["depth factor", "CA", "luminance", "tone map", "depth factor 2", "CA 2", "tone map 2", "composite"];

    /// Used `$Globals` ranges of a technique's two programs (D3D_SVF_USED). A variable is copied
    /// up to the end of its register or the next variable, whichever comes first (measured: the
    /// 8-byte depth cast goes up with its register's .zw).
    fn used(tfxo: &Tfxo, name: &str) -> Vec<(usize, usize)> {
        let (vs, ps) = tfxo.technique(name).unwrap();
        let mut r = Vec::new();
        for blob in [vs, ps] {
            for cb in rustyac_dxbc::parse(blob).unwrap().reflection.constant_buffers {
                assert_eq!(cb.size, 4096, "{name} {}", cb.name);
                let vars = &cb.variables;
                for (k, v) in vars.iter().enumerate().filter(|(_, v)| v.flags & 2 != 0) {
                    let next = vars.get(k + 1).map_or(4096, |n| n.offset);
                    let end = next.min((v.offset + v.size).div_ceil(16) * 16);
                    r.push((v.offset as usize, (end - v.offset) as usize));
                }
            }
        }
        r
    }

    /// The last 8 constant buffer uploads of an oracle `RENDER_ORACLE_YEBIS=1` log (git-ignored,
    /// re/scratch/taskY2a/runs).
    fn logged(path: &Path) -> Option<Vec<Vec<u8>>> {
        let text = std::fs::read_to_string(path).ok()?;
        let ups: Vec<Vec<u8>> = text
            .lines()
            .filter(|l| l.starts_with("Unmap B4096"))
            .map(|l| {
                let hex = l.rsplit(' ').next().unwrap();
                (0..hex.len()).step_by(2).map(|k| u8::from_str_radix(&hex[k..k + 2], 16).unwrap()).collect()
            })
            .collect();
        Some(ups[ups.len() - 8..].to_vec())
    }

    fn rust_frame(root: &Path, filter_text: Option<&str>) -> Vec<[u8; 4096]> {
        let path = root.join("system/cfg/ppfilters/default.ini");
        let filter = match filter_text {
            Some(t) => IniReader::from_text(&path, t),
            None => IniReader::load(&path).unwrap(),
        };
        let video = IniReader::from_text(Path::new("video.ini"), "[POST_PROCESS]\nENABLED=1\nFILTER=default\nQUALITY=0\nGLARE=0\nDOF=0\n");
        let cam = Camera { near: 1.0, far: 1000.0, fov_deg: 60.0, width: 1280, height: 720 };
        let pe = PostEffect::new(&filter, &video, &cam).unwrap();
        let exe = std::fs::read(root.join("acs.exe")).unwrap();
        let filmic = filmic_table(&exe).unwrap();
        let tfxo = Tfxo::from_exe(&exe).unwrap();
        let used: [Vec<(usize, usize)>; 8] = std::array::from_fn(|d| used(&tfxo, TECHNIQUES[d]));
        let mut cb = Cbuffer::default();
        cb.frame(&pe, &filmic, &used);
        cb.frame(&pe, &filmic, &used)
    }

    /// Draw 8's alpha is CRenderGlare+0x108 x 0 with the glare off: +0 or -0 by chance.
    fn same(d: usize, a: &[u8], b: &[u8]) -> bool {
        let mask = |x: &[u8]| {
            let mut x = x.to_vec();
            if d == 7 {
                x[MODULATE + 15] &= 0x7f;
            }
            x
        };
        mask(a) == mask(b)
    }

    fn runs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/scratch/taskY2a/runs")
    }

    #[test]
    fn uploads_equal_the_games() {
        let Some(root) = rustyac_content::install::ac_root() else {
            eprintln!("no Assetto Corsa install: skipped");
            return;
        };
        let ours = rust_frame(&root, None);
        let mut checked = 0;
        for (scene, file) in SCENES {
            let Some(game) = logged(&runs().join(scene).join(format!("{file}.ac.gpulog"))) else {
                eprintln!("{scene}: no oracle log: skipped");
                continue;
            };
            for d in 0..8 {
                let ok = same(d, &ours[d], &game[d]);
                eprintln!("{scene} draw {} {:<15} {}", d + 1, NAMES[d], if ok { "IDENTICAL" } else { "DIFFERENT" });
                if !ok {
                    let first = (0..4096).find(|&k| ours[d][k] != game[d][k]).unwrap();
                    panic!("{scene} draw {}: first byte {first} (c{}): ours {:?} game {:?}", d + 1, first / 16, &ours[d][first & !3..(first & !3) + 4], &game[d][first & !3..(first & !3) + 4]);
                }
            }
            checked += 1;
        }
        eprintln!("{checked} scenes compared");
    }

    /// A planted ppfilter change ([COLOR] SATURATION .95 -> .9) shows in the tone map's upload only.
    #[test]
    fn planted_saturation_change_is_caught() {
        let Some(root) = rustyac_content::install::ac_root() else {
            eprintln!("no Assetto Corsa install: skipped");
            return;
        };
        let Some(game) = logged(&runs().join("r23_09/spa_chase.ac.gpulog")) else {
            eprintln!("no oracle log: skipped");
            return;
        };
        let text = std::fs::read_to_string(root.join("system/cfg/ppfilters/default.ini")).unwrap();
        assert!(text.contains("SATURATION=0.95"));
        let ours = rust_frame(&root, Some(&text.replace("SATURATION=0.95", "SATURATION=0.9")));
        let differ: Vec<usize> = (0..8).filter(|&d| !same(d, &ours[d], &game[d])).map(|d| d + 1).collect();
        assert_eq!(differ, [4]);
    }

    #[test]
    fn inverse_undoes_the_ca_frame() {
        let mut s = diag(360.0, 360.0, 1.0);
        s[12..15].copy_from_slice(&[640.0, 360.0, 0.0]);
        assert_eq!(mul(&s, &inverse(&s)), IDENTITY);
    }
}
