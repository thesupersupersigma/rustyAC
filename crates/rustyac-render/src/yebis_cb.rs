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
//! Not ported (not on this chain): the glare's star, ghost, afterimage and anamorphic flare, DOF,
//! light shafts, lens distortion, feedback, vignette simulation, HUE / SEPIA, tone-map functions
//! other than 0, 1 and the filmic default, auto exposure with a delay, a metering window, glare
//! or another tone map.
//!
//! Step 3 (Task Y2c), `AutoExposure`: the exposure over time from the luminance row draws, the
//! analysis down to 1x1 and its readback. Task Y5a, `Glare`: the glare's bloom.

use rustyac_math::{expf, logf, powf, sqrtf};
use rustyac_physics::data::ini::IniReader;

/// Program pair of each draw, in order: round one 1-4, round two 5-7, composite 8, then the
/// luminance analysis: the 2:1 steps and the 4x2 to 1x1 step, then the heat shimmer's
/// particles with 1-4 octaves (13-16), then the glare's bloom (17-27, Task Y5a: bright pass,
/// the 7- and 11-tap gaussians, the 2x2 resample, the copy, the levels' sum for 3-7 levels), then
/// the star's (27-36, Task Y5b: the 3-tap pass, the 2- and 3-source sums, the 2-8 streak maximum), then the ghost's two passes (37-38, Task Y5c).
pub const TECHNIQUES: [&str; 39] = [
    "MadAlphaColorT2_R_S1",
    "TransformMadT1_S5",
    "TonemapHDR_Exposure",
    "TonemapHDR_PreMatrix_Dither_Vignette_Gamma",
    "DepthOfField_BlurFactorAlphaFromDepthTextureTypeCastT1_S1_RVALUE_ALPHA_RGB",
    "TransformMadT1_S5",
    "TonemapHDR_Dither_Vignette_Exposure_LINEARSAT",
    "CompositeColorAndAlphaEffectsMask_A_A",
    "MadT1_S1",
    "MadT1_S2",
    "MadT1_S5",
    "TonemapHDR_PreMatrix_Dither_Gamma",
    "TonemapHDR_Dither_Exposure_LINEARSAT",
    "HeatShimmerParticle_S1",
    "HeatShimmerParticle_S2",
    "HeatShimmerParticle_S3",
    "HeatShimmerParticle_S4",
    "BrightPassT1_Vignette_THRESHOLD_S1",
    "MadT1_S7",
    "MadT1_S11",
    "MadT1_S4",
    "MadT1",
    "MadLumGammaT3",
    "MadLumGammaT4",
    "MadLumGammaT5",
    "MadLumGammaT6",
    "MadLumGammaT7",
    "MadT1_S3",
    "MadT2",
    "MadT3",
    "MaxColorT2",
    "MaxColorT3",
    "MaxColorT4",
    "MaxColorT5",
    "MaxColorT6",
    "MaxColorT7",
    "MaxColorT8",
    "GlareGhost_PointSymmetryDistScaleT4_S8_TrimT1_S8",
    "GlareGhost_PointSymmetryT3_S8",
];

/// The glare's techniques by name (`TECHNIQUES` index).
pub const T_BRIGHT: usize = 17;
pub const T_GAUSS7: usize = 18;
pub const T_GAUSS11: usize = 19;
pub const T_UP: usize = 20;
pub const T_COPY: usize = 21;
/// `MadLumGammaT<n>` is `T_SUM + n - 3`.
pub const T_SUM: usize = 22;
/// `MadT1_S<n>` (n taps) is `T_TAPS[n - 1]`; `MadT<n>` is `T_MAD2 + n - 2` (`T_COPY` for one);
/// `MaxColorT<n>` is `T_MAX2 + n - 2`.
pub const T_TAPS: [usize; 4] = [8, 9, 27, 20];
pub const T_MAD2: usize = 28;
pub const T_MAX2: usize = 30;
/// The ghost's two passes (Task Y5c).
pub const T_GHOST4: usize = 37;
pub const T_GHOST3: usize = 38;

// $Globals byte offsets (c-register x 16)
const DEPTH_CAST: usize = 0;
const SCREEN_SPACE_SCALE: usize = 80;
const CONVERT_DEPTH: usize = 272;
const TEX_OFFSET: usize = 800;
const TAPS: usize = 544;
const MODULATE: usize = 2464;
const OFFSET: usize = 2976;
const SCALER: usize = 2336;
const GAMMA: usize = 3232;
const DITHER: usize = 3236;
const MAX_MAPPING_LUM: usize = 3328;
const COLOR_TRANSFORM: usize = 3360;
const PRE_TONEMAP: usize = 3424;
const PRE_TONEMAP_GLARE: usize = 3488;
const TRANSFORM: usize = 3584;

const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
/// BT.709 luma weights (YEBIS's colour code uses them as immediates).
const LUM: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// What the camera and the frame hand YEBIS (`updateYebisParameters` 0x140029e30).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub near: f32,
    pub far: f32,
    pub fov_deg: f32,
    pub width: u32,
    pub height: u32,
    /// The camera's exposure (40 by default) and `[EXPOSURE]` of the graphics settings.
    pub exposure: f32,
    pub exposure_multiplier: f32,
    /// The frame time auto exposure moves by.
    pub dt: f32,
    /// `chromabActive`: draws 2 and 6, off in VR (`YebisPP::updatePPCameraValues` 0x14002bcc0)
    pub chromatic_aberration: bool,
    /// The tone map takes auto exposure's current value (`YebisPP::updatePP` 0x14002c270 does
    /// for the first eye only: the second eye keeps the first's)
    pub new_exposure: bool,
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
    /// `[AUTO_EXPOSURE]` on: MIN, MAX and the middle grey `updatePP` hands YEBIS.
    pub auto_exposure: Option<[f32; 3]>,
    /// video.ini's GLARE on.
    pub glare: Option<Glare>,
}

/// `CGlareDef::ms_pGlareLib` (`InitializeStaticGlareLibs` 0x140412eb0 from the GLAREDEF table at
/// 0x141521c90): glare (+8), bloom (+0xc), ghost (+0x10) and star (+0x18) luminance and the
/// dispersion parameter (+0x24) of the 14 `[GLARE] SHAPE`s.
const GLARE_LIB: [[f32; 5]; 14] = [
    [0.0, 0.0, 0.0, 0.0, 0.5],
    [1.0, 1.0, 0.0, 0.0, 0.0],
    [1.0, 1.0, 1.0, 0.0, 0.0],
    [1.0, 1.0, 1.0, 0.5, 0.5],
    [1.2, 1.2, 1.0, 1.0, 0.99],
    [1.0, 1.0, 1.0, 0.5, 0.5],
    [1.0, 1.5, 1.0, 1.0, -0.99],
    [1.0, 1.5, 1.0, 1.0, -1.0],
    [1.0, 1.5, 1.0, 1.0, -0.99],
    [1.0, 1.5, 1.0, 1.0, -1.0],
    [1.0, 1.5, 1.0, 1.0, -0.99],
    [1.0, 1.5, 1.0, 1.0, -1.0],
    [1.0, 1.5, 1.0, 0.5, 0.5],
    [1.0, 1.5, 1.0, 0.5, 0.5],
];

/// Each glare shape's star (`ms_pGlareLib` +0x1c, +0x20: its `CStarDef` library entry and its
/// inclination) and that entry's streak length (+0x10) and odd streaks' length (+0x14) from the
/// STARDEF table at 0x141521bb0 (every entry's +0xc is 1).
const GLARE_STAR: [(usize, f32); 14] = [
    (0, 0.0),
    (0, 0.0),
    (0, 0.0),
    (1, 0.0),
    (1, 0.0),
    (1, 0.0),
    (2, 0.6544985),
    (2, -0.6544985),
    (3, 0.0872665),
    (3, -0.0872665),
    (4, 0.0),
    (4, std::f32::consts::FRAC_PI_4),
    (5, 1.5707964),
    (5, 0.0),
];
const STAR_LIB: [(f32, f32); 7] = [(0.0, 0.0), (1.25, 0.0), (2.4, 0.0), (2.4, 0.0), (2.4, 1.3), (4.0, 0.0), (3.0, 0.0)];

/// `CPostEffect::Initialize` 0x1403bbb80's glare table (0x141521650): the glare target's share
/// of the base width and its bloom level count, by quality.
pub const GLARE_QUALITY: [(f32, usize); 8] = [(0.125, 2), (0.15, 3), (0.2, 4), (0.25, 5), (0.375, 6), (0.5, 7), (0.75, 8), (1.0, 9)];

/// The glare quality YEBIS is made with: `CameraForwardYebis::initPostProcessing` 0x140024ca0
/// scales video.ini's GLARE (0-5) to `min(GetMaxGlareQuality() (7), 5)`.
pub fn glare_quality(video: &IniReader) -> usize {
    let g = video.get_int("POST_PROCESS", "GLARE").unwrap_or(0);
    ((5.0f32 * (g as f32 / 5.0)) as i32).clamp(0, 7) as usize
}

/// The glare's bloom (Task Y5a): the `[GLARE]` values `setPostProcessing` hands YEBIS and what
/// `CRenderGlare::GenerateGlare` 0x14041d470 makes of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Glare {
    pub quality: usize,
    /// LUMINANCE (CPostEffect+0x66c) and THRESHOLD (+0x668)
    pub luminance: f32,
    pub threshold: f32,
    /// CRenderGlare+0x104: the R11G11B10 range 64 x GENERATION_RANGE_SCALE
    pub range: f32,
    /// BLOOM_GAUSSIAN_RADIUS_SCALE (+0x314), BLOOM_FILTER_THRESHOLD (+0x310), BLOOM_LUMINANCE_GAMMA (+0x1a8)
    pub radius_scale: f32,
    pub cutoff: f32,
    pub gamma: f32,
    /// the glare definition's glare and bloom luminance, bloom dispersion and its first level
    pub glare_lum: f32,
    pub bloom_lum: f32,
    pub dispersion: f32,
    pub base_level: i32,
    pub star: Option<Star>,
    /// GHOST (CRenderGlare+0xe4: the ghost's two targets and the trim circle exist)
    pub ghost: Option<Ghost>,
}

/// The ghost (Task Y5c): the custom shape's ghost luminance (CGlareDef+0x18) and halo (+0x1c),
/// GHOST_CONCENTRIC_DISTORTION (CRenderGlare+0x198) and the diaphragm that turns it (+0x14c).
#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    pub lum: f32,
    pub halo: f32,
    pub concentric: f32,
    pub diaphragm: (i32, f32, f32),
}

/// The star (Task Y5b): the custom shape's `CStarDef` and the star settings `setPostProcessing` hands YEBIS.
#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    /// SHAPE_STAR_STREAKS_NUMBER, the even and odd streaks' length, the first streak's angle, and
    /// whether the diaphragm turns it
    pub lines: usize,
    pub length: f32,
    pub secondary: f32,
    pub inclination: f32,
    pub rotation: bool,
    /// the glare definition's star luminance (+0x28), CRenderGlare+0x31c (STAR_SOFTNESS) and +0x318 (STAR_FILTER_THRESHOLD)
    pub lum: f32,
    pub softness: f32,
    pub threshold: f32,
    /// `[DIAPHRAGM]` TYPE, ROTATE_SCALE and ROTATE_OFFSET (CPostEffect+0xb64, +0xb68, +0xb6c)
    pub diaphragm: (i32, f32, f32),
}

/// One ghost pass's `$Globals` (see `Cbuffer::ghost`).
#[derive(Clone, Debug, PartialEq)]
pub struct GhostPass {
    pub centres: [[f32; 4]; 16],
    pub p32: Vec<[f32; 4]>,
    pub scaler: [[f32; 4]; 8],
    pub colours: [[f32; 4]; 8],
}

/// YEBIS's xorshift128 (with an arithmetic shift) as GenerateGlare_Ghost and `CTextureUtil::Construct` use it.
fn xorshift(s: &mut [u32; 4]) -> f32 {
    let [x, y, z, w] = *s;
    let t = x << 11 ^ x;
    let n = w ^ ((t as i32 >> 8) as u32) ^ (w >> 19) ^ t;
    *s = [y, z, w, n];
    (n & 0x7fff_ffff) as f32 * 4.656613e-10
}

/// `s_avGhostCenterDistortion` 0x14155ec40 (filled by `CTextureUtil::Construct` 0x1403e50b0).
fn ghost_distortion() -> [[f32; 2]; 8] {
    let mut s = [1, 0x6c07_8967, 0x714a_cb41, 0x4807_7044];
    std::array::from_fn(|_| {
        let a = xorshift(&mut s);
        [xorshift(&mut s) * 2.0 - 1.0, a * 2.0 - 1.0]
    })
}

/// `CRenderGlare::GenerateGlare_Ghost` 0x1404185b0 with no anamorphic flare, no distortion or sharpness and
/// +0x1a0 = +0x30c = 1 (a float glare): the two passes' registers and the levels they read, (ratio, ratio + 1,
/// ratio + 2) clamped to the `levels`. `f` is its luminance, `angle` the diaphragm's (+0x14c), `rect` the first
/// source's rectangle (`src_rect`) and `aspect` its width over its height.
pub fn ghost_passes(g: &Ghost, f: f32, ratio: i32, levels: usize, angle: f32, rect: [f32; 2], aspect: f32) -> ([GhostPass; 2], [usize; 3]) {
    use rustyac_math::{cosf, powf};
    let maxl = levels as i32 - 1;
    let c = |i: i32| if -1 < i { i.min(maxl) } else { 0 };
    let w = |i: i32| powf(4.0, (c(i) - i).min(0) as f32);
    let tab = ghost_distortion();
    let centre = [rect[0] * 0.5, rect[1] * 0.5, 0.0, 0.0];
    let scaler = |x: [f32; 8]| std::array::from_fn(|k| [if x[k] == 0.0 { 1000.0 } else { 1.0 / x[k] }, 0.0, 0.0, 0.0]);
    let tint = |c: [f32; 4], w: f32| c.map(|c| w * c);
    // pass 1: the concentric scales from the generator, four of them turned by the diaphragm
    let mut s = [0x47db_4eb0, 0x8979_c4d7, 0xa533_a50c, 0x3c16_9c8a];
    let y: [f32; 8] = std::array::from_fn(|_| {
        let v = xorshift(&mut s) * 2.0 - 1.0;
        let sign = if 0.0 < v { 1.0 } else if 0.0 <= v { 0.0 } else { -1.0 };
        let mut v = powf(v.abs(), 0.5) * sign;
        if v < 0.0 {
            v *= 0.5;
        }
        powf(2.5, g.concentric * v)
    });
    let mut x = [-0.33, -0.56, -0.75, -0.4, -0.85, -0.25, -0.5, -1.0f32];
    x[1] *= cosf(0.0 - angle * 3.0) * 0.25 + 0.75;
    x[3] *= cosf(angle * 0.7 - 0.0) * 0.5 + 1.5;
    x[4] *= cosf((angle * 2.1 - 0.2) - 0.0) * 0.25 + 0.7;
    x[6] *= cosf((0.4 - angle * 1.4) - 0.0) * 0.3 + 0.7;
    let x = x.map(|x| if x <= -1.0 { -1.0 } else { x });
    let (w0, w1, w2) = (w(ratio), w(ratio + 1), w(ratio + 2));
    let mut colours = [
        tint([0.05, 0.020000001, 0.030000001, 0.0], w0),
        tint([0.015000001, 0.05, 0.030000001, 0.0], w0),
        tint([0.030000001, 0.015000001, 0.05, 0.0], w0),
        tint([0.05, 0.030000001, 0.015000001, 0.0], w1),
        tint([0.015000001, 0.05, 0.035, 0.0], w1),
        tint([0.0225, 0.045, 0.075, 0.0], w2),
        tint([0.015000001, 0.05, 0.025, 0.0], w2),
        [0.0; 4],
    ];
    colours[7] = [0.175, 0.125, 0.25, 0.0].map(|c| w2 * c * 0.3333);
    // PointSymmetryT4_S8_TrimT1_S8's DistScale program: the scales' y, the aspect's mapping, the distortion x 0
    let (fw, fh) = if aspect <= 1.0 { (rect[0], aspect * rect[1]) } else { (rect[0] / aspect, rect[1]) };
    let (sx, sy) = (2.0 / fw, 2.0 / fh);
    let mut p32: Vec<[f32; 4]> = y.iter().map(|&y| [y, 0.0, 0.0, 0.0]).collect();
    p32.push([sx, sy, -1.0 / fw, -1.0 / fh]);
    p32.push([1.0 / sx, 1.0 / sy, -((-1.0 / fw) / sx), -((-1.0 / fh) / sy)]);
    p32.push([0.0, 0.0, rect[0], rect[1]]);
    p32.extend(tab.iter().map(|t| [t[0] * 0.0, t[1] * 0.0, 0.0, 0.0]));
    let mut centres = [[0.0; 4]; 16];
    centres[..8].fill(centre);
    let one = GhostPass { centres, p32, scaler: scaler(x), colours };
    // pass 2 (PointSymmetryT3_S8): fixed scales (the generated ones go only to the unused .y), the halo
    // weighted by its own power per level, the distortion x 0 in reverse order
    let f2 = |c: [f32; 4], w: f32| c.map(|c| (f * c) * w);
    let h = g.halo * 1.414;
    let wh = w2 * if h != 1.0 { powf(h, (c(ratio + 2) - ratio) as f32) } else { 1.0 };
    let colours = [
        f2([0.15, 0.375, 1.5, 0.0], 1.0),
        f2([1.0, 0.2, 0.5, 0.0], 1.0),
        f2([0.6, 1.8, 1.6, 0.0], 1.0),
        f2([1.44, 0.72, 4.0800004, 0.0], 1.0),
        f2([0.032, 0.008, 0.16000001, 0.0], wh),
        f2([0.014, 0.002, 0.020000001, 0.0], wh),
        f2([0.001, 0.004, 0.020000001, 0.0], w0),
        f2([0.010000001, 0.0005, 0.002, 0.0], w0),
    ];
    for (k, c) in centres[8..].iter_mut().enumerate() {
        *c = [tab[7 - k][0] * 0.0, tab[7 - k][1] * 0.0, 0.0, 0.0];
    }
    let two = GhostPass { centres, p32: Vec::new(), scaler: scaler([1.0, 0.2, -0.3, -1.0, -7.0, 4.0, 2.0, -1.6]), colours };
    ([one, two], [c(ratio) as usize, c(ratio + 1) as usize, c(ratio + 2) as usize])
}

/// `CPostEffect::GetDiaphragmRotateAngle` 0x1403bb140 at the field of view `fov` (+0x534).
// shortcut: its depth of field aperture term is left out (no aperture with DOF off, as the logs show), port it with DOF
pub fn diaphragm_angle(d: (i32, f32, f32), fov: f32) -> f32 {
    use rustyac_math::{logf, tanf};
    let (mode, scale, offset) = d;
    let mut r = match mode {
        0 => tanf(fov * 0.5) * -1.0 + (std::f32::consts::FRAC_PI_4 - tanf(0.39269876) * -1.0),
        1 => logf(tanf(fov * 0.5)) * -1.0 + (std::f32::consts::FRAC_PI_4 - logf(tanf(0.39269876)) * -1.0),
        2 => (1.0 / tanf(fov * 0.5)) * 0.2 + (std::f32::consts::FRAC_PI_4 - (1.0 / tanf(0.39269876)) * 0.2),
        3 => scale,
        _ => 0.0,
    };
    if mode != 3 {
        r *= scale;
    }
    r + offset
}

impl Glare {
    fn new(filter: &IniReader, quality: usize) -> Result<Glare, String> {
        let f = |k: &str| filter.get_float("GLARE", k);
        let i = |k: &str| filter.get_int("GLARE", k);
        // what changes the targets or adds draws: Task Y5b and on
        // (AFTER_IMAGE and ANAMORPHIC are 0 in every shipped filter)
        for (key, want) in [("AFTER_IMAGE", 0), ("ANAMORPHIC", 0), ("BRIGHT_PASS", 0), ("BLOOM_NUM_LEVELS", 0), ("BLUR", 1), ("PRECISION", 0)] {
            if i(key)? != want {
                return Err(format!("yebis: [GLARE] {key} other than {want} not ported"));
            }
        }
        let shape = i("SHAPE")?;
        let lib = *GLARE_LIB.get(shape as usize).ok_or_else(|| format!("yebis: [GLARE] SHAPE={shape} is not a glare shape"))?;
        // CGlareDef::Initialize 0x140412bf0: the dispersion from the table's +0x24
        let p9 = lib[4];
        let mut dispersion = p9.abs() * 0.8;
        if p9.abs() < 1.0 && p9 < 0.0 && -1.0 < p9 {
            dispersion = 0.4;
        }
        let (mut glare_lum, mut bloom_lum, mut ghost, mut star, mut base_level) = (lib[0], lib[1], lib[2], lib[3], 4);
        // setPostProcessing 0x140025080: USE_CUSTOM_SHAPE scales the library shape
        let custom = i("USE_CUSTOM_SHAPE")? != 0;
        if custom {
            glare_lum *= f("SHAPE_LUMINANCE")?;
            bloom_lum *= f("SHAPE_BLOOM_LUMINANCE")?;
            dispersion *= f("SHAPE_BLOOM_DISPERSION")?;
            base_level = i("SHAPE_BLOOM_DISPERSION_BASE_LEVEL")?;
            ghost *= f("SHAPE_GHOST_LUMINANCE")?;
            star *= f("SHAPE_STAR_LUMINANCE")?;
        }
        let luminance = f("LUMINANCE")?;
        // setPostProcessing calls SetDiaphragmRotate only with a scale or an offset (CPostEffect::Initialize: 1, 1, 0)
        let d = |k: &str| filter.get_float("DIAPHRAGM", k);
        let (scale, offset) = (d("ROTATE_SCALE")?, d("ROTATE_OFFSET")? * 0.017453);
        let diaphragm = if scale != 0.0 || offset != 0.0 { (filter.get_int("DIAPHRAGM", "TYPE")?, scale, offset) } else { (1, 1.0, 0.0) };
        // GenerateGlare_Ghost 0x1404185b0 with the custom shape's halo, no distortion and no sharpness; a
        // concentric distortion of 0 picks the other program (no shipped filter does either)
        let ghost = if i("GHOST")? == 0 {
            None
        } else {
            let concentric = f("GHOST_CONCENTRIC_DISTORTION")?;
            if ghost > 0.0 && (!custom || f("SHAPE_GHOST_DISTORTION")? != 0.0 || i("SHAPE_GHOST_SHARPENESS")? != 0 || concentric == 0.0) {
                return Err("yebis: a library shape's ghost, a ghost distortion or sharpness, or GHOST_CONCENTRIC_DISTORTION=0 not ported".into());
            }
            let halo = if custom { f("SHAPE_GHOST_HALO_LUMINANCE")? } else { 1.0 };
            Some(Ghost { lum: ghost, halo, concentric, diaphragm })
        };
        // CRenderGlare::GenerateGlare_Star 0x140417200 draws with a star luminance, streaks, a length, a glare luminance
        // and LUMINANCE (+0x108); every library star has a dispersion (its colours: not ported)
        let star = match (star > 0.0, custom) {
            (false, _) => None,
            (true, false) => return Err("yebis: a library glare shape's star (its dispersion) not ported".into()),
            (true, true) => {
                for key in ["SHAPE_STAR_DISPERSION", "SHAPE_STAR_FORCE_DISPERSION", "STAR_LENGTH_FOV_DEPENDENCE"] {
                    if f(key)? != 0.0 {
                        return Err(format!("yebis: [GLARE] {key} other than 0 not ported"));
                    }
                }
                let (entry, inclination) = GLARE_STAR[shape as usize];
                let (length, secondary) = STAR_LIB[entry];
                let lines = i("SHAPE_STAR_STREAKS_NUMBER")?;
                let length = length * f("SHAPE_STAR_LENGTH")?;
                let (secondary, inclination) = (secondary * f("SHAPE_STAR_SECONDARY_LENGTH")?, inclination * f("SHAPE_STAR_INCLINATION_ANGLE")?);
                let rotation = i("SHAPE_STAR_ROTATION")? != 0;
                // SetGlareStarSoftness 0x1403cb460 (+0xe0 is 0.25), SetGlareStarFilterThreshold 0x1403cb570
                let softness = f("STAR_SOFTNESS")?;
                let threshold = f("STAR_FILTER_THRESHOLD")?;
                (lines > 0 && length > 0.0 && glare_lum > 0.0 && luminance > 0.0).then_some(Star {
                    lines: lines as usize,
                    length,
                    secondary,
                    inclination,
                    rotation,
                    lum: star,
                    softness: if softness > 0.0 { softness } else { 0.25 },
                    threshold: threshold.clamp(1e-05, 0.9999),
                    diaphragm,
                })
            }
        };
        let threshold = f("THRESHOLD")?;
        if threshold == 0.0 {
            return Err("yebis: [GLARE] THRESHOLD=0 (no bright pass threshold) not ported".into());
        }
        Ok(Glare {
            quality,
            luminance,
            threshold,
            range: f("GENERATION_RANGE_SCALE")? * 64.0,
            radius_scale: f("BLOOM_GAUSSIAN_RADIUS_SCALE")?,
            cutoff: f("BLOOM_FILTER_THRESHOLD")?,
            gamma: f("BLOOM_LUMINANCE_GAMMA")?,
            glare_lum,
            bloom_lum,
            dispersion,
            base_level,
            star,
            ghost,
        })
    }
}

impl PostEffect {
    /// `filter` is `system/cfg/ppfilters/<[POST_PROCESS] FILTER>.ini`.
    pub fn new(filter: &IniReader, video: &IniReader, cam: &Camera) -> Result<PostEffect, String> {
        for key in ["DOF", "RAYS_OF_GOD"] {
            if video.get_int("POST_PROCESS", key).unwrap_or(0) != 0 {
                return Err(format!("yebis: [POST_PROCESS] {key} is on: not ported (Y5-Y7)"));
            }
        }
        // CameraForwardYebis's constructor: glareEnabled = GLARE != 0
        let glare = match glare_quality(video) {
            0 => None,
            q => Some(Glare::new(filter, q)?),
        };
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
        let auto_exposure = if i("AUTO_EXPOSURE", "ENABLED")? != 0 {
            let checks = [("DELAY", 0.0), ("METERING_WIDTH", 100.0), ("METERING_HEIGHT", 100.0), ("METERING_OFFSET_X", 0.0), ("METERING_OFFSET_Y", 0.0), ("INFLUENCED_BY_GLARE", 0.0)];
            for (key, want) in checks {
                if f("AUTO_EXPOSURE", key)? != want {
                    return Err(format!("yebis: [AUTO_EXPOSURE] {key} other than {want} not ported"));
                }
            }
            if tonemap != 2 {
                return Err("yebis: auto exposure with a tone map other than filmic not ported".into());
            }
            // YebisPP::updatePP 0x14002c270
            let grey = ((cam.exposure / 30.0) * f("AUTO_EXPOSURE", "TARGET")?) * cam.exposure_multiplier;
            Some([f("AUTO_EXPOSURE", "MIN")?, f("AUTO_EXPOSURE", "MAX")?, grey])
        } else {
            None
        };
        // with auto exposure on, the caller puts `AutoExposure::frame`'s exposure here
        let exposure = if auto_exposure.is_some() { 1.0 } else { f("TONEMAPPING", "EXPOSURE")? };

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
            auto_exposure,
            glare,
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
            // the alpha round makes no glare (GetEffectiveTonemapParameters' glare factor 0)
            glare: None,
            ..self.clone()
        }
    }

    /// GetDepthConversionFactor 0x1403c3600 from CURRENT (0) to EFFECTFACTOR_OPTIMIZED (2).
    pub(crate) fn depth_factor(&self) -> [f32; 4] {
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

pub(crate) fn diag(x: f32, y: f32, z: f32) -> [f32; 16] {
    [x, 0., 0., 0., 0., y, 0., 0., 0., 0., z, 0., 0., 0., 0., 1.]
}

/// MATRIXT44_MatrixMultiply: a x b, row-major.
pub(crate) fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|k| {
        let (i, j) = (k / 4, k % 4);
        ((a[i * 4] * b[j] + a[i * 4 + 1] * b[4 + j]) + a[i * 4 + 2] * b[8 + j]) + a[i * 4 + 3] * b[12 + j]
    })
}

/// GPUMath_MatrixInverse<float> 0x1403cc630: Gauss-Jordan on columns with a column permutation.
pub(crate) fn inverse(m: &[f32; 16]) -> [f32; 16] {
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

/// GPUMath_ColorMatrix_XYZ_to_RGB 0x1403a1630: the standard sRGB / BT.709 (D65) XYZ-to-RGB matrix,
/// column by column, as the code's immediates.
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
    /// the (u, v) offsets of the next analysis draw, from `box_taps`
    pub taps: Vec<[f32; 2]>,
}

impl Default for Cbuffer {
    fn default() -> Cbuffer {
        Cbuffer { store: [0; 4096], taps: vec![[0.0; 2]] }
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

    /// The puts and the upload of one draw of `TECHNIQUES[t]`; `used[t]` are its variable ranges.
    pub fn draw(&mut self, t: usize, pe: &PostEffect, filmic: &[f32; 21], used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        match t {
            0 => {
                self.put(SCREEN_SPACE_SCALE, &[1.0, -1.0]);
                // the first tap's offset is set (zero), the second keeps what the analysis left
                self.put(TAPS, &[0.0; 4]);
                self.depth_alpha(pe)
            }
            1 | 5 => self.chromatic_aberration(pe),
            2 => self.tonemap(pe, filmic, false),
            3 | 11 => self.tonemap(pe, filmic, true),
            4 => self.depth_blur_factor(pe),
            6 | 12 => self.tonemap(&pe.round_two(), filmic, true),
            // EndPostEffectScene_Process 0x1403b84d0: the glare's share, GetEffectiveGlareLuminanceScale (1)
            // x +0x704 x the glare's +0x108 x +0x45c (AC never calls SetAlphaOutputParameters: 1)
            7 => {
                let glare = pe.glare.as_ref().map_or(0.0, |g| 1.0 * pe.mapping_div * (g.luminance / pe.mapping_div) * 1.0);
                self.put(MODULATE, &[if pe.alpha_out == 2 { pe.alpha_out_scale } else { 1.0 }, 0.0, 0.0, glare])
            }
            // CTextureLuminance::Analyze's Resample steps: `taps` with equal weights
            8..=10 => {
                let taps: Vec<f32> = self.taps.iter().flat_map(|&[u, v]| [u, v, 0.0, 0.0]).collect();
                self.put(TAPS, &taps);
                self.put(MODULATE, &vec![1.0 / self.taps.len() as f32; taps.len()]);
            }
            _ => unreachable!("no technique {t}"),
        }
        self.upload(&used[t])
    }

    /// A glare draw (Task Y5a): `DrawRectGPU_MadT1_Sn`'s taps (the first 8 in one array, the rest
    /// in the next, as the game's S11 draw writes them), each tap's colour (or the bright pass's
    /// three vectors), `DrawRectGPU_MadLumGammaTn`'s gamma.
    pub fn glare(&mut self, t: usize, taps: &[[f32; 2]], colours: &[[f32; 4]], gamma: Option<f32>, used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        for (k, &[u, v]) in taps.iter().enumerate() {
            let at = if k < 8 { TAPS + k * 16 } else { TEX_OFFSET + (k - 8) * 16 };
            self.put(at, &[u, v, 0.0, 0.0]);
        }
        self.put(MODULATE, colours.as_flattened());
        if let Some(g) = gamma {
            self.put(GAMMA, &[g]);
        }
        self.upload(&used[t])
    }

    /// `DrawRectGPU_MaxColorTn` 0x1403ee580: each source's weight in all four channels and one colour (+0x10c).
    pub fn max_color(&mut self, t: usize, weights: &[f32], colour: [f32; 4], used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        let w: Vec<f32> = weights.iter().flat_map(|&w| [w; 4]).collect();
        self.put(MODULATE, &w);
        self.put(OFFSET, &colour);
        self.upload(&used[t])
    }

    /// A ghost pass (`CTextureUtil::PointSymmetryT4_S8_TrimT1_S8` 0x1403fe210, `PointSymmetryT3_S8` 0x1403fef10):
    /// the 8 centres and 8 centre distortions, the DistScale program's 19 registers, the 8 scalers, the 8 colours.
    pub fn ghost(&mut self, t: usize, centres: &[[f32; 4]; 16], p32: &[[f32; 4]], scaler: &[[f32; 4]; 8], colours: &[[f32; 4]; 8], used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        self.put(TAPS, centres.as_flattened());
        self.put(TEX_OFFSET, p32.as_flattened());
        self.put(SCALER, scaler.as_flattened());
        self.put(MODULATE, colours.as_flattened());
        self.upload(&used[t])
    }

    /// The heat shimmer's copy of the effect surface: `DrawRectGPU_MadT1_Sn` with one tap at
    /// zero and a white modulate (`TECHNIQUES[8]`).
    pub fn copy(&mut self, used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        self.put(SCREEN_SPACE_SCALE, &[1.0, -1.0]);
        self.put(TAPS, &[0.0; 4]);
        self.put(MODULATE, &[1.0; 4]);
        self.upload(&used[8])
    }

    /// `DrawIndicesGPU_HeatShimmerParticles` 0x1403fc950's variables.
    pub fn heat(&mut self, d: &crate::yebis_heat::Draw, used: &[Vec<(usize, usize)>]) -> [u8; 4096] {
        self.put_matrix(TRANSFORM, &d.transform[0]);
        self.put_matrix(TRANSFORM + 64, &d.transform[1]);
        self.put(TEX_OFFSET, &d.offset);
        self.put_matrix(COLOR_TRANSFORM, &d.matrix);
        self.put(MODULATE, &d.modulate);
        self.upload(&used[d.technique])
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
        // GetEffectiveTonemapParameters 0x1403badc0: the glare's factor, 1 with the glare on (round one's tone map)
        let g = if last && pe.glare.is_some() { 1.0 } else { 0.0 };
        self.put(MODULATE, &[p6, g, 1.0, c[3], c[0], c[1], c[2], c[3]]);
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
            self.put_matrix(PRE_TONEMAP_GLARE, &mul(&diag(g, g, g), &pe.color_matrix));
        }
        let f = if pe.dither == -f32::MAX { 0.003921569 } else { pe.dither };
        if f != f32::MAX && f != 0.0 {
            self.put(DITHER, &[f, f * -0.5]);
        }
    }
}

/// `GetColorWeight7_Gauss7` 0x1403acf40 / `GetColorWeight11_Gauss11` 0x1403ad220: the offsets
/// (texels of a texture `size` wide) and weights of `n` (7 or 11) taps of a gaussian of `radius`,
/// the weights normalised by their sum (centre, then +1, -1, +2, -2, ...).
pub fn gauss_weights(n: usize, size: u32, radius: f32) -> (Vec<f32>, Vec<f32>) {
    let r = if radius <= 1e-8 { 1e-8 } else { radius };
    let f = 1.0 / (r * 2.0 * r);
    let inv = 1.0 / size as f32;
    let x: Vec<f32> = (0..n / 2).map(|i| i as f32 + 1.0).collect();
    let e: Vec<f32> = x.iter().map(|&x| expf(-((x * 1.0) * (x * 1.0) * f))).collect();
    let c = expf(-(1.0 * 0.0 * 1.0 * 0.0 * f));
    let mut sum = e[0] * 2.0 + c;
    for v in &e[1..] {
        sum += v * 2.0;
    }
    let w = 1.0 / sum;
    let (mut offsets, mut weights) = (vec![0.0], vec![c * w]);
    for (x, e) in x.iter().zip(&e) {
        offsets.extend([x * inv, -(x * inv)]);
        weights.extend([e * w, e * w]);
    }
    (offsets, weights)
}

/// `GaussianFilterMax29x29_2Pass` 0x14040a560's tap count for a largest radius `radius` and the
/// bloom's cutoff: the first `GetGaussianArray` 0x1403a85f0 entry from 2 on below the cutoff; 4
/// is the 7x7 filter, 6 the 11x11, the rest are not ported (the 3x3 and the filter's own path).
pub fn gauss_taps(radius: f32, cutoff: f32) -> Result<usize, String> {
    let norm = 1.0 / (sqrtf(6.2831855) * radius);
    let k = 1.0 / (radius * 2.0 * radius);
    let below = (2..15).find(|&i| {
        let a = expf(-((i * i) as f32 * k)) * norm;
        a <= cutoff && cutoff != a
    });
    match below {
        Some(4) => Ok(7),
        Some(6) => Ok(11),
        b => Err(format!("yebis: a bloom gaussian of radius {radius} ({b:?}) not ported (Y5b)")),
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

/// What the luminance step of a frame draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Luminance {
    /// One of the 5 rows of the 128x64 luminance target (`TonemapToSurface`).
    Row(u32),
    /// `CTextureLuminance::Analyze`: down to 1x1 and the copy into the read-back texture.
    Analyze,
    /// The last analysis is not read back yet.
    Skip,
}

/// Auto exposure over time: YEBIS's `CTextureLuminanceManage` (`UpdateFrame` 0x140433230, a
/// 17-slot history of read-back luminance) and `UpdateAutoExposure`'s exposure step, for the
/// filmic tone map, no delay, the whole screen metered and no glare.
pub struct AutoExposure {
    frames: [u32; 17],  // +0x7c
    lum: [f32; 17],     // +0x368
    exposure: [f32; 17], // +0x3f0
    at: i32,            // +0x58
    pending: bool,      // +0x5c
    since: u32,         // +0x60
    adjust: f32,        // +0x4c
    value: f32,         // +0x54
    rows: i32,          // +0x48
    current: f32,       // the manager's +0x64
    tonemap: f32,       // CPostEffect+0x6f4, the tone map's exposure
}

impl Default for AutoExposure {
    fn default() -> AutoExposure {
        AutoExposure { frames: [1; 17], lum: [0.18; 17], exposure: [1.0; 17], at: 0, pending: false, since: 0, adjust: 0.0, value: 1.0, rows: 0, current: 0.0, tonemap: 1.0 }
    }
}

/// HDRTMAP_DecodeTonemap for the filmic tone map.
fn decode(x: f32) -> f32 {
    let r = -logf(1.0 - x.clamp(0.0, 1.0));
    if r > 0.0 {
        r
    } else {
        0.0
    }
}

impl AutoExposure {
    /// An analysis waits for its read-back.
    pub fn waiting(&self) -> bool {
        self.pending
    }

    /// One frame: `readback` is the 1x1 RGBA of the last analysis once it is there, `ae` is
    /// `PostEffect::auto_exposure`, `new_exposure` `Camera::new_exposure`. Returns this frame's
    /// tone-map exposure and luminance step.
    /// CPostEffect+0x6f4 as this frame starts (what `frame` returns): the camera's new exposure or the last one.
    pub fn exposure(&self, new_exposure: bool) -> f32 {
        if new_exposure {
            self.value
        } else {
            self.tonemap
        }
    }

    pub fn frame(&mut self, readback: Option<[u8; 4]>, ae: [f32; 3], dt: f32, new_exposure: bool) -> (f32, Luminance) {
        let [min, max, grey] = ae;
        if new_exposure {
            self.tonemap = self.value;
        }
        let e_now = self.tonemap;
        let rate = 1.0 - powf(0.050000012, (dt * 6.0) / 0.1);
        self.since += 1;
        if let (true, Some(b)) = (self.pending, readback) {
            let c = b.map(|v| v as f32 * 0.003921569);
            let lum = ((c[1] * 0.7152) + (c[0] * 0.2126)) + (c[2] * 0.0722);
            let (k, next) = (self.at as usize % 17, (self.at as usize + 1) % 17);
            self.frames[k] = self.since;
            self.lum[k] = ((lum - self.lum[next]) * rate) + self.lum[next];
            self.at = if self.at == 0 { 16 } else { self.at - 1 };
            self.pending = false;
            self.since = 0;
        }
        let (b, a) = ((self.at as usize + 1) % 17, (self.at as usize + 2) % 17);
        let t = self.since as f32 / self.frames[b] as f32;
        let t = if !(t >= 0.0) { 0.0 } else if t > 1.0 { 1.0 } else { t };
        let mean = ((self.lum[b] - self.lum[a]) * t) + self.lum[a];
        let current = decode(mean) / (((self.exposure[b] - self.exposure[a]) * t) + self.exposure[a]);
        let step = if self.rows > 4 {
            if self.pending {
                Luminance::Skip
            } else {
                self.rows = -1;
                self.exposure[self.at as usize % 17] = e_now;
                self.pending = true;
                Luminance::Analyze
            }
        } else {
            Luminance::Row(self.rows as u32)
        };
        // UpdateAutoExposure: move log2 exposure towards the target, at most 8 stops a second
        let l2 = logf(2.0);
        let e = logf(e_now + f32::MIN_POSITIVE) / l2;
        let v = decode(grey) / (current + f32::MIN_POSITIVE);
        let lp = logf(1.0) / l2;
        let target = (((logf(v) / l2) - lp) * 0.85) + lp;
        let a = 1.0 - powf(0.5, dt / 0.3);
        let want = ((a * (target - e)) + e) - e;
        let x = self.adjust;
        let (to, ok) = if target <= e {
            let limit = -(8.0 * dt);
            let f13 = if want <= limit { limit } else { want };
            let new = (a * (f13 - x)) + x;
            self.adjust = if f13 <= new { if new <= 0.0 { new } else { 0.0 } } else { f13 };
            let to = self.adjust + e;
            (to, target < to)
        } else {
            let limit = 8.0 * dt;
            let f13 = if limit <= want { limit } else { want };
            let new = (a * (f13 - x)) + x;
            self.adjust = if new < 0.0 { 0.0 } else if new <= f13 { new } else { f13 };
            let to = self.adjust + e;
            (to, to < target)
        };
        let to = if ok {
            to
        } else {
            self.adjust = 0.0;
            target
        };
        let p = powf(2.0, to);
        self.value = if p < min { min } else if p > max { max } else { p };
        self.rows += 1;
        self.current = current;
        (e_now, step)
    }
}

/// Used `$Globals` ranges of a technique's two programs (D3D_SVF_USED), read from the bytecode at
/// run time. A variable is copied up to the end of its register or the next variable, whichever
/// comes first (measured: the 8-byte depth cast goes up with its register's .zw).
pub fn used(vs: &[u8], ps: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let mut r = Vec::new();
    for blob in [vs, ps] {
        for cb in rustyac_dxbc::parse(blob)?.reflection.constant_buffers {
            if cb.size != 4096 {
                return Err(format!("constant buffer {} is {} bytes, not 4096", cb.name, cb.size));
            }
            let vars = &cb.variables;
            for (k, v) in vars.iter().enumerate().filter(|(_, v)| v.flags & 2 != 0) {
                let next = vars.get(k + 1).map_or(4096, |n| n.offset);
                let end = next.min((v.offset + v.size).div_ceil(16) * 16);
                r.push((v.offset as usize, (end - v.offset) as usize));
            }
        }
    }
    Ok(r)
}

/// The taps of `CTextureUtil::Resample` 0x1403f6710 from a filtered `src` into `dst`: per axis
/// GPUTexUtil_GetTexCoordOffset_Box 0x1403a7e20 (at most 5) and _Resample 0x1403a7c90, v outer.
pub fn box_taps(src: (u32, u32), dst: (u32, u32)) -> Vec<[f32; 2]> {
    let axis = |s: u32, d: u32| -> Vec<f32> {
        let (s, d) = (s as f32, d as f32);
        let mut n = ((s * 0.5) / d).ceil() as i32;
        if n > 1 && d >= 8.0 {
            let f = (n - 1) as f32 * d * 2.0;
            if f < s && s - (n * 2 - 2) as f32 <= f {
                n -= 1;
            }
        }
        let n = n.min(5) as f32;
        let step = (n * 2.0) / n;
        (0..n as i32).map(|i| (i as f32 * step - (n - 1.0)) * (1.0 / s)).collect()
    };
    let (u, v) = (axis(src.0, dst.0), axis(src.1, dst.1));
    v.iter().flat_map(|&y| u.iter().map(move |&x| [x, y])).collect()
}

/// `used` for every technique.
pub fn used_all(tfxo: &crate::yebis_tfxo::Tfxo) -> Result<Vec<Vec<(usize, usize)>>, String> {
    let mut out = vec![Vec::new(); TECHNIQUES.len()];
    for (o, name) in out.iter_mut().zip(TECHNIQUES) {
        let (vs, ps) = tfxo.technique(name).ok_or_else(|| format!("no technique {name}"))?;
        *o = used(vs, ps)?;
    }
    Ok(out)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::yebis_tfxo::Tfxo;
    use std::path::{Path, PathBuf};

    #[test]
    #[allow(clippy::excessive_precision)] // the game's values as its log prints them
    fn ghost_passes_follow_the_game() {
        // the default filter's draws 897-898 of re/scratch/taskY5/g3 (192x108 glare, five levels, F 2.688)
        let g = Ghost { lum: 0.15, halo: 0.005, concentric: 1.0, diaphragm: (1, 1.0, 0.0) };
        let ([one, two], levels) = ghost_passes(&g, 2.6880002, 0, 5, 0.0349, [1.0, 1.0], 192.0 / 108.0);
        assert_eq!(levels, [0, 1, 2]);
        let y = [1.87552905, 0.700101078, 0.727419972, 1.71940994, 0.755011559, 0.793704391, 0.634749413, 2.33311915];
        assert_eq!(one.p32[..8].iter().map(|r| r[0]).collect::<Vec<_>>(), y);
        assert_eq!(one.p32[8..11], [[3.55555558, 2.0, -1.77777779, -1.0], [0.28125, 0.5, 0.5, 0.5], [0.0, 0.0, 1.0, 1.0]]);
        // (the diaphragm angle is rounded here: pass 1's turned scalers to 1e-4)
        let sx = [-3.030303, -1.78816319, -1.33333337, -1.25009334, -1.2410078, -4.0, -2.03729224, -1.0];
        assert!(one.scaler.iter().zip(sx).all(|(s, x)| (s[0] - x).abs() < 1e-4), "{:?}", one.scaler);
        assert_eq!(one.colours[7], [0.0583274998, 0.0416624993, 0.0833249986, 0.0]);
        assert_eq!(two.scaler.map(|s| s[0]), [1.0, 5.0, -3.33333325, -1.0, -0.142857149, 0.25, 0.5, -0.625]);
        let game = [
            [0.40320006, 1.00800014, 4.03200054, 0.0],
            [2.6880002, 0.53760004, 1.3440001, 0.0],
            [1.61280024, 4.83840036, 4.30080032, 0.0],
            [3.87072039, 1.93536019, 10.967042, 0.0],
            [4.2995016e-06, 1.0748754e-06, 2.1497508e-05, 0.0],
            [1.88103184e-06, 2.6871885e-07, 2.6871885e-06, 0.0],
            [0.00268800044, 0.0107520018, 0.053760007, 0.0],
            [0.0268800035, 0.00134400022, 0.00537600089, 0.0],
        ];
        assert_eq!(two.colours, game);
        // the distortion's signs (as -0 / +0 bytes) in the game's order
        let signs: Vec<bool> = ghost_distortion().iter().flat_map(|t| [t[0] < 0.0, t[1] < 0.0]).collect();
        assert_eq!(signs, [true, false, false, true, true, true, true, false, false, true, true, false, true, true, true, false]);
    }

    #[test]
    fn box_taps_follow_the_game() {
        // 16:9: 2:1 steps with one tap, 4x2 to 1x1 with two (the Y2c uploads)
        assert_eq!(box_taps((128, 64), (64, 32)), vec![[0.0, 0.0]]);
        assert_eq!(box_taps((4, 2), (1, 1)), vec![[-0.25, 0.0], [0.25, 0.0]]);
        // triple 128x24: 2:3 first step, then 16x2 to 1x1 capped at five taps
        assert_eq!(box_taps((128, 24), (64, 8)), vec![[0.0, -1.0 / 24.0], [0.0, 1.0 / 24.0]]);
        assert_eq!(box_taps((16, 2), (1, 1)).len(), 5);
    }

    const SCENES: [(&str, &str); 3] = [("r23_09", "spa_chase_r23_09"), ("r23_10", "spa_chase_r23_10"), ("r23_16", "monza_chase_r23_16")];
    const NAMES: [&str; 8] = ["depth factor", "CA", "luminance", "tone map", "depth factor 2", "CA 2", "tone map 2", "composite"];

    /// The last 8 constant buffer uploads of an oracle `RENDER_ORACLE_YEBIS=1` log (git-ignored,
    /// re/scratch/taskY2b/runs: the game side zeroes CRenderGlare+0x108, so draw 8 is +0).
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
        let cam = Camera { near: 1.0, far: 1000.0, fov_deg: 60.0, width: 1280, height: 720, exposure: 40.0, exposure_multiplier: 1.0, dt: 0.016666668, chromatic_aberration: true, new_exposure: true };
        let mut pe = PostEffect::new(&filter, &video, &cam).unwrap();
        // the logged frame is the second, after one auto exposure step from 1 to MAX
        pe.exposure = 0.5;
        let exe = std::fs::read(root.join("acs.exe")).unwrap();
        let filmic = filmic_table(&exe).unwrap();
        let tfxo = Tfxo::from_exe(&exe).unwrap();
        let used = used_all(&tfxo).unwrap();
        let mut cb = Cbuffer::default();
        (0..8).for_each(|t| _ = cb.draw(t, &pe, &filmic, &used));
        (0..8).map(|t| cb.draw(t, &pe, &filmic, &used)).collect()
    }

    fn runs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/scratch/taskY2b/runs")
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
                let ok = game[d] == ours[d];
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
        let Some(game) = logged(&runs().join("r23_09/spa_chase_r23_09.ac.gpulog")) else {
            eprintln!("no oracle log: skipped");
            return;
        };
        let text = std::fs::read_to_string(root.join("system/cfg/ppfilters/default.ini")).unwrap();
        assert!(text.contains("SATURATION=0.95"));
        let ours = rust_frame(&root, Some(&text.replace("SATURATION=0.95", "SATURATION=0.9")));
        let differ: Vec<usize> = (0..8).filter(|&d| game[d] != ours[d]).map(|d| d + 1).collect();
        assert_eq!(differ, [4]);
    }

    /// The game's auto exposure (re/scratch/taskY2c ae_a16: `YebisPP` +0x54, +0x4c and the
    /// luminance manager's +0x64 after frames 1-15 of the y16 run), fed its two read-backs.
    #[test]
    fn auto_exposure_follows_the_games() {
        const GAME: [(u32, u32, u32); 15] = [
            (0x3f000000, 0x3ba2bd76, 0x3e4b36ba),
            (0x3f000000, 0x3c033acf, 0x3e4b36ba),
            (0x3f000000, 0x3c3334b6, 0x3e4b36ba),
            (0x3f000000, 0x3c615ea6, 0x3e4b36ba),
            (0x3f000000, 0x3c86e513, 0x3e4b36ba),
            (0x3f000000, 0x3c9c440a, 0x3e4b36ba),
            (0x3f000000, 0x3caaeacd, 0x3e997906),
            (0x3f000000, 0x3cb3f9a8, 0x3eda2106),
            (0x3f000000, 0x3cb8203c, 0x3f15f66b),
            (0x3f000000, 0x3cafa2a0, 0x3f4aa003),
            (0x3f000000, 0x3c000a80, 0x3f87f498),
            (0x3f000000, 0x00000000, 0x3ff83139),
            (0x3effddfc, 0xba445471, 0x3ff6e77f),
            (0x3eff9bc3, 0xbabf499f, 0x3ff59f69),
            (0x3eff3b24, 0xbb0bb67c, 0x3ff458f8),
        ];
        let grey = ((40.0f32 / 30.0) * 0.32) * 1.0;
        assert_eq!(grey, 0.42666668);
        let mut ae = AutoExposure::default();
        let mut steps = Vec::new();
        for n in 0..16 {
            let readback = match n {
                6 => Some([168, 164, 155, 0]),
                12 => Some([158, 155, 149, 0]),
                _ => None,
            };
            let (e, step) = ae.frame(readback, [0.2, 0.5, grey], 0.016666668, true);
            steps.push(step);
            // the tone map of frame n uses the exposure after frame n-1
            if n != 1 {
                assert_eq!(e, if n == 0 { 1.0 } else { f32::from_bits(GAME[n - 2].0) }, "frame {n} exposure");
            }
            if n > 0 {
                let got = (ae.value.to_bits(), ae.adjust.to_bits(), ae.current.to_bits());
                assert_eq!(got, GAME[n - 1], "frame {n}: {} {} {}", ae.value, ae.adjust, ae.current);
            }
        }
        use Luminance::*;
        assert_eq!(steps, [Row(0), Row(1), Row(2), Row(3), Row(4), Analyze, Row(0), Row(1), Row(2), Row(3), Row(4), Analyze, Row(0), Row(1), Row(2), Row(3)]);
    }

    #[test]
    fn inverse_undoes_the_ca_frame() {
        let mut s = diag(360.0, 360.0, 1.0);
        s[12..15].copy_from_slice(&[640.0, 360.0, 0.0]);
        assert_eq!(mul(&s, &inverse(&s)), IDENTITY);
    }

    /// The game's bloom gaussians at 1280x720, QUALITY=3, GLARE=3 (taskY5 b3 frame, draws 857-860).
    #[test]
    #[allow(clippy::excessive_precision)] // the game's logged digits
    fn bloom_gaussians_follow_the_games() {
        let r0 = 0.5f32 * 2.2 * 0.95;
        let r1 = (2.0 * r0) * sqrtf(0.75);
        assert_eq!((gauss_taps(r0, 0.002), gauss_taps(r1, 0.002)), (Ok(7), Ok(11)));
        let ch = |r: f32, n| [(r * 6.15e-07) / 6.15e-07, (r * 5.4e-07) / 6.15e-07, (r * 4.65e-07) / 6.15e-07].map(|c| gauss_weights(n, 192, c).1);
        let w = ch(r0, 7);
        assert_eq!([w[0][0], w[1][0], w[2][0]], [0.381958097, 0.434813946, 0.504909158]);
        assert_eq!([w[0][1], w[1][2], w[2][5]], [0.241639242, 0.240095347, 0.000373895833]);
        let w = ch(r1, 11);
        assert_eq!([w[0][0], w[1][0], w[2][0], w[0][9], w[2][10]], [0.220872134, 0.251133025, 0.29152301, 0.00486488082, 0.000368210458]);
        let (o, w) = gauss_weights(11, 108, 0.0);
        assert_eq!((o[1], o[10], w[0], w[1]), (1.0 / 108.0, -(5.0 * (1.0 / 108.0)), 1.0, 0.0));
    }
}
