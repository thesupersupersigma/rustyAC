// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//! YEBIS's 8 draws of AC's default frame on Direct3D 11 (docs/atlas/yebis.md s8, Task Y2b).
//!
//! The device layer of `PPFX::CPostEffect::ApplyEffects` 0x1403c5cd0 as far as this chain goes:
//! YEBIS's state cache (blend, rasterizer and per-slot samplers, dropped by its state reset),
//! its render targets, the shared vertex buffer ring of `CTextureUtil` (768 dynamic buffers of
//! 192 bytes, `MapSharedVertexBuffer` 0x1403e4e60 / `IGPUBuffer::Map` 0x140437b30), the
//! rectangle builder `DrawRectGPU_GetVerticesAndTexCoords` 0x1403e5ce0, the noise generator of
//! the dither and the one 4 KB constant buffer filled by `yebis_cb`.
//!
//! The programs and the filmic table are read from the player's acs.exe at start; nothing of
//! YEBIS is in this source but the call order, the state descriptions and the small constants.
//!
//! Auto exposure (Task Y2c): frames draw one of the five rows of the luminance target (128x64 at 16:9) in
//! turn, then one frame reduces it to 1x1 and copies that into a read-back texture that a later
//! frame reads without waiting; `yebis_cb::AutoExposure` turns the result into the exposure.

use rustyac_physics::data::ini::IniReader;

/// Bytes of one shared vertex buffer, and how many there are.
const RING_BYTES: u32 = 192;
const RING_COUNT: usize = 768;

/// The process-wide noise of YEBIS's dither offsets: four linear congruential states.
#[derive(Clone, Copy, Debug)]
pub struct Noise([u32; 4]);

impl Default for Noise {
    fn default() -> Noise {
        Noise([1, 0x3391, 0x61e9b, 0xbf1])
    }
}

impl Noise {
    /// The four values of a draw, stepping first when `step`: two offsets in -1..1 and two signs.
    pub fn next(&mut self, step: bool) -> [f32; 4] {
        if step {
            for s in &mut self.0 {
                *s = s.wrapping_mul(0x5d58_8b65).wrapping_add(0x0100_0001);
            }
        }
        let v = |s: u32| ((s >> 14) as f32 / 262143.0) * 2.0 - 1.0;
        let sign = |s: u32| if v(s) < 0.0 { -1.0 } else { 1.0 };
        [v(self.0[0]), v(self.0[1]), sign(self.0[2]), sign(self.0[3])]
    }
}

/// `DrawRectGPU_GetVerticesAndTexCoords`: one triangle over the rectangle (x, y, w, h) of a
/// viewport of `size`, with a texture rectangle (u0, v0, u1, v1) per coordinate set.
pub fn rect_vertices(size: [f32; 2], rect: [f32; 4], tex: &[[f32; 4]]) -> Vec<f32> {
    let [w_vp, h_vp] = size;
    let [x, y, w, h] = rect;
    let fx0 = x - w_vp * 0.5;
    let mut fy0 = y - h_vp * 0.5;
    let mut fx1 = fx0 + w;
    let fy1 = fy0 + h;
    fx1 = (fx1 - fx0) * 2.0 + fx0;
    fy0 = (fy0 - fy1) * 2.0 + fy1;
    let (sx, sy) = (2.0 / w_vp, 2.0 / h_vp);
    let (x0, y1, x1, y0) = ((fx0 + 0.0) * sx, (fy1 + 0.0) * sy, (fx1 + 0.0) * sx, (fy0 + 0.0) * sy);
    let tc: Vec<[f32; 6]> = tex
        .iter()
        .map(|&[u0, v0, u1, v1]| {
            let u1 = (u1 - u0) * 2.0 + u0;
            let v0 = (v0 - v1) * 2.0 + v1;
            [u0, v1, u0, v0, u1, v1]
        })
        .collect();
    let mut out = Vec::new();
    for (k, p) in [[x0, y1], [x0, y0], [x1, y1]].into_iter().enumerate() {
        out.extend_from_slice(&p);
        for t in &tc {
            out.extend_from_slice(&t[k * 2..k * 2 + 2]);
        }
    }
    out
}

/// The top of luminance row `row` of a target `lh` high; its viewport starts at the whole part.
pub fn row_y(row: u32, lh: f32) -> f32 {
    lh * (row as f32 * 0.2)
}

/// The luminance target of a `w` x `h` YEBIS and its analysis targets down to the 1x1 result:
/// `CPostEffect::Initialize` 0x1403bbb80 (an eighth, power-of-two floor, at most 128 wide at the
/// frame's aspect) and `CTextureLuminance::CreateSystemObjects` 0x140431b70 (2:1 steps).
pub fn luminance_sizes(w: i32, h: i32) -> ((u32, u32), Vec<(u32, u32)>) {
    let floor2 = |v: u32| 2f64.powi(((v as f64).ln() / 2f64.ln()) as i32) as u32;
    let (mut lw, mut lh) = (floor2(w as u32 >> 3), floor2(h as u32 >> 3));
    if lw > 128 {
        lw = 128;
        lh = (128.0 / (w as f32 / h as f32) + 0.49999) as u32;
    }
    let (mut aw, mut ah) = (floor2(lw), floor2(lh));
    let n = ((1.0 / aw as f32).max(1.0 / ah as f32).ln() / 0.5f32.ln()).ceil() as i32 - 1;
    let mut chain = Vec::new();
    for _ in 0..n.max(0) {
        (aw, ah) = ((aw as f32 / 2.0 + 0.99) as u32, (ah as f32 / 2.0 + 0.99) as u32);
        chain.push((aw, ah));
    }
    chain.push((1, 1));
    ((lw, lh), chain)
}

/// YEBIS's base width, `m_uiEffectBufferCriterionWidth` (`CameraForwardYebis::initRenderTargets`
/// 0x140029a90): `[POST_PROCESS] QUALITY` fifths of the frame's width, or 640 (at most the width) at 0.
pub fn base_width(quality: i32, w: i32) -> i32 {
    match (quality as f32 / 5.0 * w as f32) as i32 {
        0 => w.min(640),
        b => b,
    }
}

/// The glare target (`CPostEffect::Initialize` 0x1403bbb80): the glare quality's share of the
/// base width (`yebis_cb::GLARE_QUALITY`, 0.125 at GLARE=0).
pub fn glare_size(base: i32, quality: usize, w: i32, h: i32) -> (u32, u32) {
    effect_size(base, crate::yebis_cb::GLARE_QUALITY[quality].0, w, h)
}

/// One bloom level of the glare (`CRenderGlare::CreateSystemObjects` 0x140413c00, the +0x278
/// list): its size, whether it has its own three targets (the levels past the first two, k > 0),
/// the size it is resampled up to (+0x28) and its radius step (+8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlareLevel {
    pub size: (u32, u32),
    pub own: bool,
    pub up: Option<(u32, u32)>,
    pub step: f32,
}

/// The glare's bloom levels: the quality table's count, at most `log2(width)` (GLAREPARAM::FillDefaults,
/// the +0x30 constant 2 levels at full glare size), each later one halved.
pub fn glare_levels(base: i32, quality: usize, w: i32, h: i32) -> Vec<GlareLevel> {
    let (gw, gh) = glare_size(base, quality, w, h);
    let cc = 2;
    let n = (crate::yebis_cb::GLARE_QUALITY[quality].1 as i32).min(cc - 2 + (rustyac_math::logf(gw as f32) / rustyac_math::logf(2.0)) as i32);
    (0..n)
        .map(|i| {
            let k = i - cc + 1;
            let scale = if k > 0 { rustyac_math::powf(2.0, -(k as f32)) } else { 1.0 };
            let size = if k > 0 { ((gw as f32 * scale + 0.99) as u32, (gh as f32 * scale + 0.99) as u32) } else { (gw, gh) };
            let up = (k >= 1 && ((w as u32 >> 4) >= size.0 || i == n - 1)).then_some((size.0 * 2, size.1 * 2));
            GlareLevel { size, own: k > 0, up, step: rustyac_math::powf(2.0, i as f32) * scale }
        })
        .collect()
}

/// A `factor` of the base width (at most the base and the frame's width) at the frame's aspect,
/// at least 16 on each side (`CPostEffect::Initialize` 0x1403bbb80, the glare and the depth of field).
fn effect_size(base: i32, factor: f32, w: i32, h: i32) -> (u32, u32) {
    let aspect = w as f32 / h as f32;
    let mut a = ((base as f32 * factor + 0.99) as i64).min(base as i64).min(w as i64);
    let mut gh = (a as f32 / aspect + 0.499999) as i64;
    if a < 16 || gh < 16 {
        if a < gh {
            a = 16;
            gh = (16.0 / aspect + 0.499999) as i64;
        } else {
            gh = 16;
            a = (aspect * a as f32 + 0.499999) as i64;
        }
    }
    (a as u32, gh as u32)
}

/// The depth of field's two levels: DOF=0's 0.25 of the base width (`CPostEffect::Initialize`
/// 0x1403bbb80), then its double, at most the frame, only when the first is at most 128 wide or
/// an eighth of the frame (`CreateApertureFilterLevelParams` 0x14041f750, the DOF table's +0x14
/// and +0x18), else the same size again.
pub fn dof_sizes(base: i32, w: i32, h: i32) -> [(u32, u32); 2] {
    let (a, dh) = effect_size(base, 0.25, w, h);
    if a <= 128 || a as f32 <= w as f32 * 0.125 {
        [(a, dh), ((2 * a).min(w as u32), (2 * dh).min(h as u32))]
    } else {
        [(a, dh), (a, dh)]
    }
}

/// One depth of field level with DOF on (Task Y6, `CreateApertureFilterLevelParams` 0x14041f750,
/// `DOFLEVELPARAM::Initialize` 0x14041e420): its size, hexagon passes, its diameter over its height (+0x30)
/// and whether it is doubled (its result drawn at twice the size: not ported).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DofLevel {
    pub size: (u32, u32),
    pub passes: u32,
    pub l30: f32,
    pub doubled: bool,
}

/// The levels of video.ini DOF `q` (1-5): the quality table 0x141521690's scale, level count, combination
/// (shift, radius per level) and doubling width and ratio.
pub fn dof_levels(q: i32, base: i32, w: i32, h: i32) -> Vec<DofLevel> {
    let (scale, count, c1, dbl, ratio) = match q {
        1 => (0.25, 2, false, 160, 0.125),
        2 => (0.375, 2, false, 160, 0.125),
        3 => (0.5, 3, true, 160, 0.125),
        4 => (0.5, 3, false, 320, 0.25),
        _ => (0.5, 3, true, 320, 0.25),
    };
    let (mut lw, mut lh) = effect_size(base, scale, w, h);
    let small = |lw: u32| lw <= dbl || lw as f32 <= w as f32 * ratio;
    // combination 3's first radius is 1 only when the level is nearly the frame's width
    let r0 = (w as f32 / lw as f32) < 1.2;
    let combo: [(u32, u32); 3] = if c1 { [(0, 0), (1, 1), (1, 2)] } else { [(0, r0 as u32), (0, 2), (1, 2)] };
    let mut idx = small(lw) as usize;
    let mut shift = 0;
    combo[..count]
        .iter()
        .map(|&(sh, radius)| {
            if sh != shift {
                shift = sh;
                lw = ((lw as f32 * 0.5 + 0.99) as u32).max(3);
                lh = ((lh as f32 * 0.5 + 0.99) as u32).max(3);
                if small(lw) {
                    idx = (idx + 1).min(3);
                }
            }
            let n = radius + 1;
            let diam = rustyac_math::powf(2.0, n as f32) * 2.0 - 1.0 + [0.0, 0.5, 0.75, 1.0][idx];
            DofLevel { size: (lw, lh), passes: n, l30: diam / lh as f32, doubled: idx > 0 }
        })
        .collect()
}

/// The pool's R16G16B16A16 textures in the order the pool (0x140411900) gets them: the depth of
/// field's two levels, then its halvings of the frame (`CDepthOfField::CreateSystemObjects`
/// 0x1404200a0), a size already there shared. The level asks for its two only when doubled
/// (`DOFLEVELPARAM::Initialize` 0x14041e420: +0x60 counts up when the sizes differ, +0x70 is the
/// doubled flag +0x28).
pub fn half_pool(base: i32, w: i32, h: i32) -> Vec<(u32, u32)> {
    let [d0, d1] = dof_sizes(base, w, h);
    let mut out = if d0 == d1 { vec![] } else { vec![d0, d1] };
    for s in halvings(d0, w, h) {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Halvings of the frame, enough to come down to `to` (`ceil(log(max((to + 1) / frame)) / log 0.5) - 1`).
fn halvings(to: (u32, u32), w: i32, h: i32) -> Vec<(u32, u32)> {
    let r = ((to.0 + 1) as f32 / w as f32).max((to.1 + 1) as f32 / h as f32);
    let n = ((r.ln() / 0.5f32.ln()).ceil() as i32 - 1).max(0);
    let (mut pw, mut ph) = (w as u32, h as u32);
    (0..n)
        .map(|_| {
            (pw, ph) = ((pw as f32 * 0.5 + 0.99) as u32, (ph as f32 * 0.5 + 0.99) as u32);
            (pw, ph)
        })
        .collect()
}

/// The sizes `CDepthOfField::ClearAllGPUResources` 0x14042b930 clears: its own pool's level 0
/// (this+0x468), its halvings (this+0x458), level 0's double (the level's +0xa0); a halving the
/// double shares is cleared twice.
pub fn half_pairs(base: i32, w: i32, h: i32) -> Vec<(u32, u32)> {
    let [d0, d1] = dof_sizes(base, w, h);
    let mut out = vec![d0];
    out.extend(halvings(d0, w, h));
    out.push(d1);
    out
}

/// The glare pool's halvings of the frame: enough to come down to the glare's size.
pub fn glare_pool(base: i32, quality: usize, w: i32, h: i32) -> Vec<(u32, u32)> {
    halvings(glare_size(base, quality, w, h), w, h)
}

/// The vertices of draw `d` (0-based) of a `w` x `h` frame. `noise` is the draw's noise (draws
/// 3, 4 and 7); `vignette` is the half-height of the vignette rectangle.
pub fn draw_vertices(d: usize, w: f32, h: f32, noise: [f32; 4], vignette: f32, row: u32) -> Vec<f32> {
    let full = [0.0, 0.0, 1.0, 1.0];
    let zero = [0.0; 4];
    // the noise offsets of the 1x1 noise texture; the luminance draw passes 0 for both scales,
    // which makes NaN and +inf (x86's default NaN: black_box keeps the compiler from folding it)
    let noise_rect = |p32: f32, p33: f32| noise_rect(noise, p32, p33);
    match d {
        // TonemapToSurface 0x14041d7f0: one of the five rows of the `w` x `h` luminance target
        2 => {
            let y = row_y(row, h);
            let v = row as f32 * 0.2;
            rect_vertices([w, (h * 0.2).trunc()], [0.0, y - y.trunc(), w, h * 0.2], &[[0.0, v, 1.0, v + 0.2], zero, zero, noise_rect(0.0, 0.0), zero])
        }
        // DrawRectGPU_TonemapHDR 0x1403f1b10: vignette, noise and the 4x4 dither
        3 | 6 => tonemap_vertices(w, h, noise_rect(1.0, w / h), vignette, [0.0; 4]),
        7 => rect_vertices([w, h], [0.0, 0.0, w, h], &[full, zero, zero, zero]),
        _ => rect_vertices([w, h], [0.0, 0.0, w, h], &[full]),
    }
}

/// The noise texture's rectangle from this frame's noise offsets and scales.
pub fn noise_rect(noise: [f32; 4], p32: f32, p33: f32) -> [f32; 4] {
    let (p32, p33) = (std::hint::black_box(p32), std::hint::black_box(p33));
    let [n0, n1, s2, s3] = noise;
    [n0, n1, n0 + s2 * (((1024.0 / 1.0) * 2.0 * p33) / p32), n1 + s3 * (((1024.0 / 1.0) * 2.0) / p32)]
}

/// A glare draw's source rectangle (`DrawRectGPU_*` callers in `CTextureUtil`): the source's size times
/// its texture's inverse size (+0x88, +0x8c) plus its offset (0), so 0.99999994 rather than 1 at
/// some sizes (47 x (1/47)).
pub fn src_rect(w: u32, h: u32) -> [f32; 4] {
    let (w, h) = (w as f32, h as f32);
    [0.0, 0.0, w * (1.0 / w) + 0.0, h * (1.0 / h) + 0.0]
}

/// The vignette's rectangle (`DrawRectGPU_TonemapHDR`, `DrawRectGPU_BrightPass`): its half-height
/// `vignette` at the frame's aspect; no vignette leaves it zero (+0, not -0).
pub fn vignette_rect(w: f32, h: f32, vignette: f32) -> [f32; 4] {
    let (aspect, a) = (w / h, vignette);
    if a == 0.0 {
        [0.0; 4]
    } else {
        [-a * aspect, -a, a * aspect, a]
    }
}

/// The tone map's vertices: the whole source, the glare's coordinates (the whole glare texture
/// when it is on, `DrawRectGPU_TonemapHDR` 0x1403f1b10), the vignette, the noise, the 4x4 dither.
/// `glare`: the glare result's rectangle (`src_rect`), zero without the glare.
pub fn tonemap_vertices(w: f32, h: f32, noise: [f32; 4], vignette: f32, glare: [f32; 4]) -> Vec<f32> {
    rect_vertices([w, h], [0.0, 0.0, w, h], &[[0.0, 0.0, 1.0, 1.0], glare, vignette_rect(w, h, vignette), noise, [0.0, 0.0, w / 4.0, h / 4.0]])
}

/// The shared vertex buffer ring's bookkeeping: which buffer and offset a draw of `size` bytes
/// lands in, and whether its map is a real discard.
#[derive(Clone, Copy, Debug)]
pub struct Ring {
    pub index: usize,
    pub cur: u32,
}

impl Default for Ring {
    /// The game's ring starts on its last buffer: its first move to a new buffer (frame 0's third
    /// draw) is already the discard of buffer 0 (seen in a frame-0 log, Task Y2c).
    fn default() -> Self {
        Ring { index: RING_COUNT - 1, cur: 0 }
    }
}

impl Ring {
    /// (buffer, offset, discard)
    pub fn alloc(&mut self, size: u32) -> (usize, u32, bool) {
        let wrap = RING_BYTES <= self.cur + size;
        if wrap {
            self.cur = 0;
        }
        let offset = self.cur;
        self.cur = offset + size;
        let mut discard = false;
        if wrap {
            self.index += 1;
            if self.index >= RING_COUNT {
                self.index = 0;
                discard = true;
            }
        }
        (self.index, offset, discard)
    }
}

/// `[VIGNETTING]` of the filter: the half-height of the vignette rectangle.
pub fn vignette(filter: &IniReader) -> Result<f32, String> {
    if filter.get_float("VIGNETTING", "FOV_DEPENDENCE")? != 0.0 {
        return Err("yebis: [VIGNETTING] FOV_DEPENDENCE is not 0: not ported".into());
    }
    // SetVignetteParameters: tan(pi / 8) times the strength
    Ok(rustyac_math::tanf(std::f32::consts::FRAC_PI_8) * filter.get_float("VIGNETTING", "STRENGTH")?)
}

#[cfg(all(windows, not(feature = "backend-wgpu")))]
pub use device::Yebis;

#[cfg(all(windows, not(feature = "backend-wgpu")))]
mod device {
    use super::*;
    use crate::yebis_cb::{filmic_table, used_all, AutoExposure, Camera, Cbuffer, Glare, Luminance, PostEffect, Shafts, Star, TECHNIQUES, T_BRIGHT, T_BRIGHT_LUM, T_BRIGHT_LUM_PLAIN, T_BRIGHT_PLAIN, T_COPY, T_GAUSS11, T_GAUSS7, T_GHOST3, T_GHOST4, T_LINEARSAT, T_NO_GAMMA, T_CORONA, T_MAD2, T_MAD4, T_MAX2, T_RADIAL, T_SHAFT_MASK, T_SUM, T_TAPS, T_TAPS_ODD, T_UP};
    use std::path::Path;
    use windows::core::PCSTR;
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;

    // samplers, as numbered in `IGPUDevice`'s set of 16: border, linear, point, wrap, linear with border
    const SB: usize = 1;
    const SL: usize = 4;
    const SP: usize = 5;
    const SW: usize = 13;
    const SLB: usize = 0;
    const SLBM: usize = 2;
    /// trilinear wrap: the film grain's and the spectrum's own mode (the light shafts)
    const SW_TRI: usize = 15;

    #[derive(Clone)]
    struct Target {
        rtv: ID3D11RenderTargetView,
        srv: Option<ID3D11ShaderResourceView>,
    }

    /// What YEBIS keeps set between its calls; its reset drops it all.
    #[derive(Default)]
    struct Cache {
        blend: bool,
        raster: bool,
        samplers: [Option<usize>; 16],
    }

    pub struct Yebis {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        vs: Vec<Option<ID3D11VertexShader>>,
        ps: Vec<Option<ID3D11PixelShader>>,
        /// Every technique's bytecode: YEBIS makes a program on its first draw.
        code: Vec<(Vec<u8>, Vec<u8>)>,
        /// Every technique's program numbers in the YEBIS container: a program is made once per number.
        ids: Vec<(usize, usize)>,
        /// Each technique's input elements; a layout is shared by every program with the same ones.
        inputs: Vec<Vec<(std::ffi::CString, u32, DXGI_FORMAT)>>,
        layouts: Vec<Option<ID3D11InputLayout>>,
        strides: Vec<u32>,
        used: Vec<Vec<(usize, usize)>>,
        filmic: [f32; 21],
        /// Made with the first program.
        cb: Option<ID3D11Buffer>,
        /// What YEBIS makes and keeps but the port does not use yet (state modes, generated textures, buffers).
        _kept: Vec<windows::core::IUnknown>,
        cbuffer: Cbuffer,
        ring: Vec<ID3D11Buffer>,
        ring_at: Ring,
        ib: ID3D11Buffer,
        black: ID3D11ShaderResourceView,
        grey: ID3D11ShaderResourceView,
        white: ID3D11ShaderResourceView,
        dither: ID3D11ShaderResourceView,
        /// Three surfaces, as YEBIS's render-texture array: a draw never reads the one it writes
        /// (the third holds the heat shimmer's copy).
        effect: [Target; 3],
        /// The heat shimmer: the film grain and gaussian textures, three vertex buffers of
        /// `particles` quads used in turn (`heat_at` the last one mapped) and their index buffer.
        grain: ID3D11ShaderResourceView,
        gaussian: ID3D11ShaderResourceView,
        heat_vb: Vec<ID3D11Buffer>,
        heat_ib: ID3D11Buffer,
        heat_at: usize,
        particles: u32,
        /// The copy's sampler: the source texture's own sampler mode (`DrawRectGPU_MadT1_Sn`).
        copy_sampler: usize,
        /// Each effect surface's own mode: point/clamp once a tone map has read it (see `copy_sampler`): the
        /// second, or the first with the depth of field.
        frame_clamped: [bool; 2],
        /// Each texture's saved mode (its last sampler's address, as a sampler index of linear filtering), which
        /// `CTextureUtil::Resample` keeps, by texture (Task Y6; only the depth of field reads them).
        modes: std::collections::HashMap<usize, usize>,
        /// The effect surface bound as the target.
        current: usize,
        effect_depth: ID3D11DepthStencilView,
        luminance: Target,
        luminance_size: (u32, u32),
        analysis: Vec<(Target, ID3D11Texture2D, (u32, u32))>,
        readback: ID3D11Texture2D,
        auto_exposure: AutoExposure,
        /// One per round: the composite reads the first round's colour and the second's alpha.
        tonemap: [Target; 2],
        samplers: Vec<ID3D11SamplerState>,
        raster: ID3D11RasterizerState,
        depth: ID3D11DepthStencilState,
        blend: ID3D11BlendState,
        cache: Cache,
        /// `CTinyEffect` +0x90: the highest texture slot end of the pixel programs made so far, emptied before each draw.
        srv_slots: usize,
        /// `CPostEffect::RegisterDeviceTexture`'s map (+0xc0): each source view once, with the view made on it.
        sources: Vec<(ID3D11ShaderResourceView, windows::core::IUnknown)>,
        noise: Noise,
        root: std::path::PathBuf,
        filter: IniReader,
        video: IniReader,
        /// The vignette's half-height, from the filter's `[VIGNETTING] STRENGTH` at init.
        pub vignette: f32,
        width: i32,
        height: i32,
        /// the target bound when YEBIS was made, and how many targets the teardown releases
        /// (only the size: a kept view of the back buffer would make `ResizeBuffers` fail)
        screen: Option<(u32, u32)>,
        released: usize,
        /// The glare (Task Y5a): its targets in the game's order with their sizes (0-2 the bright pass's,
        /// 3 the composite, 4 its double, 5 the levels' sum), the bloom levels (source, result, temporary
        /// and the double as indices into it) and the pool of halvings of the frame.
        glare: Vec<(Target, u32, u32)>,
        glare_levels: Vec<(super::GlareLevel, [usize; 3], Option<usize>)>,
        glare_pool: Vec<(Target, u32, u32)>,
        /// the star's first target (Task Y5b): 8 streaks, the two passes' pair, then the streaks' maximum
        star_at: usize,
        /// The light shafts' mask and result (glare targets `shaft_at`, `shaft_at + 1`) and the corona's spectrum.
        shaft_at: usize,
        spectrum: ID3D11ShaderResourceView,
        /// the ghost's trim circle (Task Y5c), only with GHOST on; its two targets follow the star's maximum
        trim: Option<ID3D11ShaderResourceView>,
        /// The depth of field with DOF on (Task Y6): the frame's halvings, then each level with its source,
        /// front blur factor, result and two work targets.
        dof_halves: Vec<(Target, u32, u32)>,
        dof_levels: Vec<(super::DofLevel, [Target; 5])>,
    }

    /// The bytes of one input element.
    fn format_bytes(f: DXGI_FORMAT) -> u32 {
        match f {
            DXGI_FORMAT_R32_FLOAT => 4,
            DXGI_FORMAT_R32G32_FLOAT => 8,
            DXGI_FORMAT_R32G32B32_FLOAT => 12,
            _ => 16,
        }
    }

    /// The heat shimmer's three vertex buffers and its quads' index buffer.
    unsafe fn heat_buffers(device: &ID3D11Device, particles: u32) -> Result<(Vec<ID3D11Buffer>, ID3D11Buffer), String> {
        let mut vb = Vec::with_capacity(3);
        for _ in 0..3 {
            vb.push(buffer(device, particles * 0xb0, D3D11_BIND_VERTEX_BUFFER.0, true, None)?);
        }
        let quads: Vec<u8> = (0..particles as u16).flat_map(|i| [4 * i, 4 * i + 1, 4 * i + 2, 4 * i + 2, 4 * i + 1, 4 * i + 3]).flat_map(|i| i.to_le_bytes()).collect();
        Ok((vb, buffer(device, particles * 12, D3D11_BIND_INDEX_BUFFER.0, false, Some(&quads))?))
    }

    fn err<T>(what: &str, r: windows::core::Result<T>) -> Result<T, String> {
        r.map_err(|e| format!("yebis: {what}: {e}"))
    }

    unsafe fn texture(device: &ID3D11Device, w: u32, h: u32, format: DXGI_FORMAT, bind: i32, data: Option<&[u8]>) -> Result<ID3D11Texture2D, String> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init = data.map(|d| D3D11_SUBRESOURCE_DATA { pSysMem: d.as_ptr().cast(), SysMemPitch: w * 4, SysMemSlicePitch: 0 });
        let mut t = None;
        err("CreateTexture2D", device.CreateTexture2D(&desc, init.as_ref().map(|i| i as *const _), Some(&mut t)))?;
        t.ok_or_else(|| "yebis: no texture".into())
    }

    unsafe fn srv(device: &ID3D11Device, t: &ID3D11Texture2D, format: DXGI_FORMAT) -> Result<ID3D11ShaderResourceView, String> {
        let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: u32::MAX } },
        };
        let mut v = None;
        err("CreateShaderResourceView", device.CreateShaderResourceView(t, Some(&desc), Some(&mut v)))?;
        v.ok_or_else(|| "yebis: no view".into())
    }

    unsafe fn target(device: &ID3D11Device, w: u32, h: u32, typeless: DXGI_FORMAT, format: DXGI_FORMAT, with_srv: bool) -> Result<Target, String> {
        Ok(target_texture(device, w, h, typeless, format, with_srv)?.0)
    }

    unsafe fn target_texture(device: &ID3D11Device, w: u32, h: u32, typeless: DXGI_FORMAT, format: DXGI_FORMAT, with_srv: bool) -> Result<(Target, ID3D11Texture2D), String> {
        // the game's order: texture, shader view, target view (an R11G11B10 target view's format is 0)
        let t = texture(device, w, h, typeless, D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0, None)?;
        let srv = if with_srv { Some(srv(device, &t, format)?) } else { None };
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: if format == DXGI_FORMAT_R11G11B10_FLOAT { DXGI_FORMAT_UNKNOWN } else { format },
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } },
        };
        let mut rtv = None;
        err("CreateRenderTargetView", device.CreateRenderTargetView(&t, Some(&desc), Some(&mut rtv)))?;
        Ok((Target { rtv: rtv.ok_or("yebis: no view")?, srv }, t))
    }

    unsafe fn buffer(device: &ID3D11Device, size: u32, bind: i32, dynamic: bool, data: Option<&[u8]>) -> Result<ID3D11Buffer, String> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: size,
            Usage: if dynamic { D3D11_USAGE_DYNAMIC } else { D3D11_USAGE_IMMUTABLE },
            BindFlags: bind as u32,
            CPUAccessFlags: if dynamic { D3D11_CPU_ACCESS_WRITE.0 as u32 } else { 0 },
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let init = data.map(|d| D3D11_SUBRESOURCE_DATA { pSysMem: d.as_ptr().cast(), SysMemPitch: 0, SysMemSlicePitch: 0 });
        let mut b = None;
        err("CreateBuffer", device.CreateBuffer(&desc, init.as_ref().map(|i| i as *const _), Some(&mut b)))?;
        b.ok_or_else(|| "yebis: no buffer".into())
    }

    /// `CTextureUtil::InitializeDevice` 0x1403e6890's 4x4 dither: a Bayer matrix, its row's first
    /// value in green and blue.
    fn dither_texels() -> [u8; 64] {
        const M: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
        let b = |v: f32| ((v * 0.0625 + 0.03125) * 255.9999) as u8;
        let mut out = [0u8; 64];
        for i in 0..16 {
            let a = i / 4 * 4;
            out[i * 4..i * 4 + 4].copy_from_slice(&[b(M[i]), b(M[a]), b(M[a]), b(M[i])]);
        }
        out
    }

    impl Yebis {
        /// Everything YEBIS makes before its first frame. `filter` is the ppfilter of video.ini's
        /// `[POST_PROCESS] FILTER`; `root` holds the player's acs.exe. `width` x `height` is the
        /// destination's size; `base` YEBIS's base width (`m_uiEffectBufferCriterionWidth`).
        pub fn new(device: &ID3D11Device, context: &ID3D11DeviceContext, root: &Path, filter: IniReader, video: IniReader, width: i32, height: i32, base: i32, screen: Option<ID3D11RenderTargetView>, particles: u32) -> Result<Yebis, String> {
            let exe = std::fs::read(root.join("acs.exe")).map_err(|e| format!("yebis: acs.exe: {e}"))?;
            let tfxo = crate::yebis_tfxo::Tfxo::from_exe(&exe)?;
            let filmic = filmic_table(&exe)?;
            let used = used_all(&tfxo)?;
            let vignette = vignette(&filter)?;
            unsafe {
                // every program is made on its first draw (see draw)
                let (mut code, mut inputs, mut strides, mut ids) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                for name in TECHNIQUES {
                    let (v, p) = tfxo.technique(name).ok_or_else(|| format!("yebis: no technique {name}"))?;
                    let t = tfxo.techniques.iter().find(|t| t.name.strip_prefix("tech_") == Some(name)).ok_or("yebis: no technique")?;
                    ids.push((t.vs, t.ps));
                    // the input layout from the program's input signature, packed
                    let elements: Vec<(std::ffi::CString, u32, DXGI_FORMAT)> = rustyac_dxbc::parse(v)?
                        .inputs
                        .iter()
                        .map(|e| {
                            let format = [DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT][(8 - e.mask.leading_zeros()) as usize];
                            (std::ffi::CString::new(e.name.as_str()).unwrap_or_default(), e.index, format)
                        })
                        .collect();
                    strides.push(elements.iter().map(|e| format_bytes(e.2)).sum());
                    inputs.push(elements);
                    code.push((v.to_vec(), p.to_vec()));
                }
                let n = code.len();
                let mut kept: Vec<windows::core::IUnknown> = Vec::new();
                let state = |r: windows::core::Result<()>, what: &str, s: Option<windows::core::IUnknown>| -> Result<windows::core::IUnknown, String> {
                    err(what, r)?;
                    s.ok_or_else(|| format!("yebis: no {what}"))
                };
                // IGPUDevice::InitializeDevice_CreateDeviceStates 0x1403d4fa0: the draw-rect defaults (YEBIS's reset
                // sets them), then the blend/raster pair of every render-state mode, then 16 samplers
                let raster_desc = |scissor: bool| D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    FrontCounterClockwise: true.into(),
                    DepthBias: 0,
                    DepthBiasClamp: 0.0,
                    SlopeScaledDepthBias: 0.0,
                    DepthClipEnable: false.into(),
                    ScissorEnable: scissor.into(),
                    MultisampleEnable: true.into(),
                    AntialiasedLineEnable: false.into(),
                };
                // (enable, src, dst, src alpha, dst alpha) by mode, `_RenderStateMode` 0x1403d4b00; only target 0 is filled
                let blend_desc = |mode: usize, mask: u8| {
                    let (on, s, d, sa, da) = [(false, 2, 2, 2, 2), (true, 5, 6, 5, 6), (true, 2, 2, 2, 2), (true, 2, 4, 2, 6), (true, 2, 0xe, 2, 0xe)][mode];
                    let mut bd = D3D11_BLEND_DESC::default();
                    bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                        BlendEnable: on.into(),
                        SrcBlend: D3D11_BLEND(s),
                        DestBlend: D3D11_BLEND(d),
                        BlendOp: D3D11_BLEND_OP_ADD,
                        SrcBlendAlpha: D3D11_BLEND(sa),
                        DestBlendAlpha: D3D11_BLEND(da),
                        BlendOpAlpha: D3D11_BLEND_OP_ADD,
                        RenderTargetWriteMask: mask,
                    };
                    bd
                };
                let mut raster = None;
                err("CreateRasterizerState", device.CreateRasterizerState(&raster_desc(false), Some(&mut raster)))?;
                let keep = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_ALWAYS };
                // zeroed first: a struct literal leaves the two padding bytes after the masks as stack garbage,
                // which the call log shows
                let mut dd: D3D11_DEPTH_STENCIL_DESC = std::mem::zeroed();
                dd.DepthFunc = D3D11_COMPARISON_ALWAYS;
                dd.StencilReadMask = 0xff;
                dd.StencilWriteMask = 0xff;
                dd.FrontFace = keep;
                dd.BackFace = keep;
                let mut depth = None;
                err("CreateDepthStencilState", device.CreateDepthStencilState(&dd, Some(&mut depth)))?;
                let mut blend = None;
                err("CreateBlendState", device.CreateBlendState(&blend_desc(0, 0xf), Some(&mut blend)))?;
                for p in 0..64usize {
                    let (mode, mask, scissor) = (p & 7, (p >> 3) & 3, p >> 5 & 1);
                    if mode > 4 || mask > 2 {
                        continue;
                    }
                    let mut b = None;
                    let r = device.CreateBlendState(&blend_desc(mode, [0xf, 7, 8][mask]), Some(&mut b));
                    kept.push(state(r, "CreateBlendState", b.map(|b| b.into()))?);
                    let mut s = None;
                    let r = device.CreateRasterizerState(&raster_desc(scissor == 1), Some(&mut s));
                    kept.push(state(r, "CreateRasterizerState", s.map(|s| s.into()))?);
                }
                // `_SamplerStateMode` 0x1403d4dd0: border, clamp, mirror, wrap; each linear and point without
                // mips, then linear and trilinear with them
                let mut samplers = Vec::with_capacity(16);
                for p in 0..16usize {
                    let address = D3D11_TEXTURE_ADDRESS_MODE(4 - (p >> 2) as i32);
                    let desc = D3D11_SAMPLER_DESC {
                        Filter: D3D11_FILTER([0x14, 0, 0x14, 0x15][p & 3]),
                        AddressU: address,
                        AddressV: address,
                        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                        MipLODBias: 0.0,
                        MaxAnisotropy: 1,
                        ComparisonFunc: D3D11_COMPARISON_FUNC(0),
                        BorderColor: [0.0; 4],
                        MinLOD: 0.0,
                        MaxLOD: if p & 3 < 2 { 0.0 } else { f32::MAX },
                    };
                    let mut s = None;
                    err("CreateSamplerState", device.CreateSamplerState(&desc, Some(&mut s)))?;
                    samplers.push(s.ok_or("yebis: no sampler")?);
                }
                // `InitializeDeviceBuffers` 0x1403d4330: the bound target again, with a viewport of its size
                // (this+0x20, the target given at the start: on a remake the caller's destination, unbound)
                let mut bound: [Option<ID3D11RenderTargetView>; 1] = [None];
                let mut bound_depth = None;
                match screen {
                    Some(rtv) => bound[0] = Some(rtv),
                    None => context.OMGetRenderTargets(Some(&mut bound), Some(&mut bound_depth)),
                }
                let mut screen = None;
                if let Some(rtv) = &bound[0] {
                    let t: ID3D11Texture2D = err("GetResource", rtv.GetResource().and_then(|r| windows::core::Interface::cast(&r)))?;
                    let mut d = D3D11_TEXTURE2D_DESC::default();
                    t.GetDesc(&mut d);
                    context.OMSetRenderTargets(Some(&bound), bound_depth.as_ref());
                    context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: d.Width as f32, Height: d.Height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
                    screen = Some((rtv.clone(), d.Width, d.Height));
                }
                // CTextureUtil::InitializeDevice 0x1403e6890: each texture made empty, its view, then filled
                let made = |w: u32, h: u32, mips: u32, format: DXGI_FORMAT, bind: i32| -> Result<ID3D11Texture2D, String> {
                    let desc = D3D11_TEXTURE2D_DESC {
                        Width: w,
                        Height: h,
                        MipLevels: mips,
                        ArraySize: 1,
                        Format: format,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: bind as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    };
                    let mut t = None;
                    err("CreateTexture2D", device.CreateTexture2D(&desc, None, Some(&mut t)))?;
                    t.ok_or_else(|| "yebis: no texture".into())
                };
                let filled = |w: u32, h: u32, mips: u32, format: DXGI_FORMAT, levels: &[Vec<u8>], texel: u32| -> Result<ID3D11ShaderResourceView, String> {
                    let t = made(w, h, mips, format, D3D11_BIND_SHADER_RESOURCE.0)?;
                    let view = srv(device, &t, format)?;
                    for (l, data) in levels.iter().enumerate() {
                        context.UpdateSubresource(&t, l as u32, None, data.as_ptr().cast(), (w >> l).max(1) * texel, 0);
                    }
                    Ok(view)
                };
                let rgba = DXGI_FORMAT_R8G8B8A8_UNORM;
                // the luminance analyser's 16x16 target (its InitializeDevice), its view before its target view
                let t = made(16, 16, 1, DXGI_FORMAT_R8G8B8A8_TYPELESS, D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0)?;
                kept.push(srv(device, &t, rgba)?.into());
                let mut rtv = None;
                let rd = D3D11_RENDER_TARGET_VIEW_DESC { Format: rgba, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D, Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } } };
                err("CreateRenderTargetView", device.CreateRenderTargetView(&t, Some(&rd), Some(&mut rtv)))?;
                kept.push(rtv.ok_or("yebis: no view")?.into());
                let dither = filled(4, 4, 1, rgba, &[dither_texels().to_vec()], 4)?;
                // film grain (PFX_INITPARAM always asks for it), spectrum, radius fade: full mip chains but the fade's 5
                let grain = crate::yebis_tex::mipmaps(256, 256, crate::yebis_tex::grain(&exe)?, true, true);
                let grain = filled(256, 256, 0, rgba, &grain, 4)?;
                let spectrum = crate::yebis_tex::mipmaps(256, 1, &crate::yebis_tex::spectrum(&crate::yebis_tex::Cie::from_exe(&exe)?), true, true);
                let spectrum = filled(256, 1, 0, rgba, &spectrum, 4)?;
                let gaussian = filled(64, 64, 5, DXGI_FORMAT_A8_UNORM, &crate::yebis_tex::gaussian(), 1)?;
                let black = filled(1, 1, 1, rgba, &[vec![0; 4]], 4)?;
                let grey = filled(1, 1, 1, rgba, &[vec![0x7f; 4]], 4)?;
                let white = filled(1, 1, 1, rgba, &[vec![0xff; 4]], 4)?;
                // a depth surface of the analyser's size
                let t = made(16, 16, 1, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL.0)?;
                let dsv_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                    Flags: 0,
                    Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
                };
                let mut dsv = None;
                err("CreateDepthStencilView", device.CreateDepthStencilView(&t, Some(&dsv_desc), Some(&mut dsv)))?;
                kept.push(dsv.ok_or("yebis: no view")?.into());
                // 16x16 ramp: texel i is byte i four times
                let ramp: Vec<u8> = (0..256u32).flat_map(|i| [i as u8; 4]).collect();
                kept.push(filled(16, 16, 1, rgba, &[ramp], 4)?.into());
                // vertex rings and index buffers; the heat-shimmer buffers for PFX_INITPARAM's particles, 1 view
                let mut ring = Vec::with_capacity(RING_COUNT);
                for _ in 0..RING_COUNT {
                    ring.push(buffer(device, RING_BYTES, D3D11_BIND_VERTEX_BUFFER.0, true, None)?);
                }
                let indices: [u16; 4] = [0, 1, 2, 3];
                let ib_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                let ib = buffer(device, 8, D3D11_BIND_INDEX_BUFFER.0, false, Some(&ib_bytes))?;
                for _ in 0..RING_COUNT {
                    kept.push(buffer(device, 32, D3D11_BIND_INDEX_BUFFER.0, true, None)?.into());
                }
                let (heat_vb, heat_ib) = heat_buffers(device, particles)?;
                // the frame's targets in the game's order (its f10 remake log, Task Y3d-2); those YEBIS
                // draws use we keep by name, the rest only to make what the game makes
                let (w, h) = (width as u32, height as u32);
                let f9 = (DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_FLOAT);
                let f27 = (DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM);
                let f26 = (DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R11G11B10_FLOAT);
                // `keep` also lists the target view with its size for the clear pass below
                type Views = Vec<(ID3D11RenderTargetView, u32, u32)>;
                let keep = |kept: &mut Vec<windows::core::IUnknown>, list: &mut Views, t: Target, tw: u32, th: u32| {
                    list.push((t.rtv.clone(), tw, th));
                    kept.push(t.rtv.into());
                    kept.extend(t.srv.map(|s| s.into()));
                };
                let effect = [target(device, w, h, f9.0, f9.1, true)?, target(device, w, h, f9.0, f9.1, true)?, target(device, w, h, f9.0, f9.1, true)?];
                let frame: Views = effect.iter().map(|t| (t.rtv.clone(), w, h)).collect();
                let depth_tex = texture(device, w, h, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL.0, None)?;
                let dsv_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                    Flags: 0,
                    Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
                };
                let mut dsv = None;
                err("CreateDepthStencilView", device.CreateDepthStencilView(&depth_tex, Some(&dsv_desc), Some(&mut dsv)))?;
                let tonemap = [target(device, w, h, f27.0, f27.1, true)?, target(device, w, h, f27.0, f27.1, true)?];
                // the glare (`CRenderGlare::CreateSystemObjects` 0x140413c00): the bright pass's three (+0x248), the
                // composite (+0x2b8), its double (+0x2d0: at most half the frame wide, at the glare's aspect), the levels'
                // sum (+0x270), the first two levels' result and temporary, each later level's three and its double
                // (+0x28), then 15 more (+0x280 star x10, +0x288, +0x290, the ghost's +0x298 x2 with GHOST on, +0x2c0)
                let quality = crate::yebis_cb::glare_quality(&video);
                let (ga, gh) = super::glare_size(base, quality, width, height);
                let dw = (2 * ga).min(w >> 1);
                let mut sizes = vec![(ga, gh); 4];
                sizes.push((dw, (dw as f32 / (ga as f32 / gh as f32)) as u32));
                sizes.extend([(ga, gh); 5]);
                let mut glare_levels = Vec::new();
                for (i, l) in super::glare_levels(base, quality, width, height).into_iter().enumerate() {
                    let (s0, s1) = l.size;
                    if s0 < 4 || s1 < 4 {
                        // shortcut: the game's rule for levels under 4 texels is not read, never at AC's sizes
                        return Err(format!("yebis: a glare level of {s0}x{s1} not ported"));
                    }
                    let at = sizes.len();
                    let idx = match (i, l.own) {
                        (0, _) => [2, 6, 7],
                        (_, false) => [6, 8, 9],
                        _ => {
                            sizes.extend([l.size; 3]);
                            [at + 2, at, at + 1]
                        }
                    };
                    let up = l.up.map(|u| {
                        sizes.push(u);
                        sizes.len() - 1
                    });
                    glare_levels.push((l, idx, up));
                }
                let star_at = sizes.len();
                // initPostProcessing 0x140024ca0: m_bGlareGhost (readPPSetOptions' default is on)
                let ghost = filter.get_int("GLARE", "GHOST").unwrap_or(1) != 0;
                sizes.extend(vec![(ga, gh); if ghost { 15 } else { 13 }]);
                // RAYS_OF_GOD (init flag 0x10000000): the shafts' mask (+0x2d8) and result (+0x2e0) last, and the
                // bright pass's first and third (+0x248) and the halvings in R16G16B16A16
                let shafts = quality > 0 && video.get_int("POST_PROCESS", "RAYS_OF_GOD").unwrap_or(0) != 0;
                let shaft_at = sizes.len();
                if shafts {
                    sizes.extend([(ga, gh); 2]);
                }
                let mut glare: Vec<(Target, u32, u32)> = Vec::new();
                let mut glare_views = Views::new();
                // (the double, +0x2d0, only with a result blur: CreateSystemObjects 0x140413c00; Task Y5d)
                // GLAREPARAM::FillDefaults 0x1404131e0 also turns it off when the max width (half the frame) is not
                // above the glare's, i.e. when the double would be no wider (PP_QUALITY 5 with GLARE 5; Task Y6)
                let blur = filter.get_float("GLARE", "BLUR").unwrap_or(1.0) > 0.0 && dw > ga;
                for (i, &(gw, gg)) in sizes.iter().enumerate() {
                    if i == 4 && !blur {
                        glare.push(glare[3].clone());
                        continue;
                    }
                    let f = if shafts && (i == 0 || i == 2) { f9 } else { f26 };
                    let t = target(device, gw, gg, f.0, f.1, true)?;
                    glare.push((t.clone(), gw, gg));
                    keep(&mut kept, &mut glare_views, t, gw, gg);
                }
                // the ghost's trim circle (+0x2a0, CreateDeviceResources 0x140415520: only with the ghost's targets)
                let trim = if ghost { Some(filled(64, 64, 1, DXGI_FORMAT_A8_UNORM, &[crate::yebis_tex::trim_circle()], 1)?) } else { None };
                kept.extend(trim.clone().map(|t| t.into()));
                let mut dof = Views::new();
                for (dw, dh) in super::dof_sizes(base, width, height) {
                    keep(&mut kept, &mut dof, target(device, dw, dh, f9.0, f9.1, true)?, dw, dh);
                }
                // shortcut: DOF on gets its own targets after all the others, not the game's pool order or its clears; port them when a resize row differs
                let dof_q = video.get_int("POST_PROCESS", "DOF").unwrap_or(0);
                let (mut dof_halves, mut dof_levels) = (vec![], vec![]);
                if dof_q > 0 {
                    let levels = super::dof_levels(dof_q, base, width, height);
                    for (hw, hh) in super::halvings(levels[0].size, width, height) {
                        dof_halves.push((target(device, hw, hh, f9.0, f9.1, true)?, hw, hh));
                    }
                    for l in levels {
                        let t = || target(device, l.size.0, l.size.1, f9.0, f9.1, true);
                        dof_levels.push((l, [t()?, t()?, t()?, t()?, t()?]));
                    }
                }
                let (luminance_size, chain) = super::luminance_sizes(width, height);
                let luminance = target(device, luminance_size.0, luminance_size.1, f27.0, f27.1, true)?;
                // auto exposure's 1x1 result and its CPU copy; then the auto focus's pair and copy
                let one = target_texture(device, 1, 1, f27.0, f27.1, true)?;
                let rd = D3D11_TEXTURE2D_DESC {
                    Width: 1,
                    Height: 1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut readback = None;
                err("CreateTexture2D", device.CreateTexture2D(&rd, None, Some(&mut readback)))?;
                keep(&mut kept, &mut Views::new(), target(device, 1, 1, f27.0, f27.1, true)?, 1, 1);
                keep(&mut kept, &mut Views::new(), target(device, 1, 1, f27.0, f27.1, true)?, 1, 1);
                let mut focus = None;
                err("CreateTexture2D", device.CreateTexture2D(&rd, None, Some(&mut focus)))?;
                kept.push(focus.ok_or("yebis: no texture")?.into());
                // the texture pool: the glare's halvings, the R16G16B16A16 ones, the luminance chain above the 1x1
                let mut glare_pool = Views::new();
                let mut glare_pool_targets = Vec::new();
                for (pw, ph) in super::glare_pool(base, quality, width, height) {
                    let f = if shafts { f9 } else { f26 };
                    let t = target(device, pw, ph, f.0, f.1, true)?;
                    glare_pool_targets.push((t.clone(), pw, ph));
                    keep(&mut kept, &mut glare_pool, t, pw, ph);
                }
                let mut half_pool = Views::new();
                for (pw, ph) in super::half_pool(base, width, height) {
                    keep(&mut kept, &mut half_pool, target(device, pw, ph, f9.0, f9.1, true)?, pw, ph);
                }
                let mut analysis = Vec::new();
                for &(aw, ah) in &chain[..chain.len() - 1] {
                    let (t, tex) = target_texture(device, aw, ah, f27.0, f27.1, true)?;
                    analysis.push((t, tex, (aw, ah)));
                }
                analysis.push((one.0, one.1, (1, 1)));
                let effect_depth = dsv.ok_or("yebis: no view")?;
                // the clear pass (`CPostEffect::Initialize` 0x1403bbb80, its end): each target bound alone with a
                // viewport of its size and cleared to 0, the bound target put back (no depth) after each group
                let clear_group = |views: &[(ID3D11RenderTargetView, u32, u32)], with_depth: bool| {
                    for (rtv, cw, ch) in views {
                        context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), if with_depth { Some(&effect_depth) } else { None });
                        context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: *cw as f32, Height: *ch as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
                        context.ClearRenderTargetView(rtv, &[0.0; 4]);
                        if with_depth {
                            context.ClearDepthStencilView(&effect_depth, D3D11_CLEAR_DEPTH.0, 1.0, 0);
                        }
                    }
                    if let Some((rtv, sw, sh)) = &screen {
                        context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                        context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: *sw as f32, Height: *sh as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
                    }
                };
                let chain_views: Views = analysis[..analysis.len() - 1].iter().map(|(t, _, (aw, ah))| (t.rtv.clone(), *aw, *ah)).collect();
                // 1. the pool, one texture a group
                for v in glare_pool.iter().chain(&half_pool).chain(&chain_views) {
                    clear_group(std::slice::from_ref(v), false);
                }
                // 2. the frame's three with the depth
                clear_group(&frame, true);
                // 3. the glare's 24 of its size in the game's groups, the double alone as the 20th, then each later
                // level's three and its double
                let own: Vec<Vec<usize>> = glare_levels.iter().filter(|l| l.0.own).map(|l| [l.1[1], l.1[2], l.1[0]].into_iter().chain(l.2).collect()).collect();
                let mut order: Views = glare_views.iter().enumerate().filter(|(i, _)| *i != 4 && !own.iter().flatten().any(|j| j == i)).map(|(_, v)| v.clone()).collect();
                // (CRenderGlare::ClearAllGPUResources 0x14041c400: +0x288, the star's ten, the ghost's pair if made, ...)
                let groups: &[usize] = if ghost { &[1, 10, 2, 1, 3, 1, 1, 1, 2, 2] } else { &[1, 10, 1, 3, 1, 1, 1, 2, 2] };
                order.insert(if ghost { 19 } else { 17 }, glare_views[4].clone());
                let mut at = 0;
                for &n in groups {
                    clear_group(&order[at..at + n], false);
                    at += n;
                }
                for group in &own {
                    let views: Views = group.iter().map(|&i| glare_views[i].clone()).collect();
                    clear_group(&views, false);
                }
                // 4. the glare's pool again; 5. the depth of field's, by size (see half_pairs)
                for v in &glare_pool {
                    clear_group(std::slice::from_ref(v), false);
                }
                for size in super::half_pairs(base, width, height) {
                    // the levels' own targets when the pool has no pair (no double: DOFLEVELPARAM::Initialize)
                    if let Some(v) = half_pool.iter().chain(&dof).find(|v| (v.1, v.2) == size) {
                        clear_group(std::slice::from_ref(v), false);
                    }
                }
                // 6. the luminance target and the exposure's 1x1; the bound target once more
                let last = &analysis[analysis.len() - 1];
                clear_group(&[(luminance.rtv.clone(), luminance_size.0, luminance_size.1)], false);
                clear_group(&[(last.0.rtv.clone(), 1, 1)], false);
                clear_group(&[], false);
                Ok(Yebis {
                    device: device.clone(),
                    context: context.clone(),
                    vs: vec![None; n],
                    ps: vec![None; n],
                    code,
                    ids,
                    inputs,
                    layouts: vec![None; n],
                    strides,
                    used,
                    filmic,
                    cb: None,
                    _kept: kept,
                    cbuffer: Cbuffer::default(),
                    ring,
                    ring_at: Ring::default(),
                    ib,
                    black,
                    grey,
                    white,
                    dither,
                    effect,
                    grain,
                    gaussian,
                    heat_vb,
                    heat_ib,
                    // the first map is the discard of buffer 0
                    heat_at: 2,
                    particles,
                    copy_sampler: SLB,
                    frame_clamped: [false; 2],
                    modes: Default::default(),
                    current: 0,
                    effect_depth,
                    luminance,
                    luminance_size,
                    analysis,
                    readback: readback.ok_or("yebis: no texture")?,
                    auto_exposure: AutoExposure::default(),
                    tonemap,
                    samplers,
                    raster: raster.ok_or("yebis: no state")?,
                    depth: depth.ok_or("yebis: no state")?,
                    blend: blend.ok_or("yebis: no state")?,
                    cache: Cache::default(),
                    srv_slots: 0,
                    sources: Vec::new(),
                    noise: Noise::default(),
                    root: root.to_path_buf(),
                    filter,
                    video,
                    vignette,
                    width,
                    height,
                    // `CRenderTexture::UninitializeDevice` 0x1403d29f0 puts the screen back once for each texture
                    // holding a GPU texture (+0x28), in `CPostEffect::Uninitialize` 0x1403c08a0's order:
                    // the wrapped scene colour and depth (+0xc0 map), CRenderGlare (+0x108: +0x248 ×3, +0x270,
                    // +0x2b8, +0x2c0, +0x2d0, +0x268 ×4, +0x280 ×10, +0x288, +0x290, +0x298 ×2, then its pool),
                    // the DOF's two levels (+0x110, then its pool), the render buffers (+0x100 ×3), the colour
                    // alpha dest (+0x118 ×2), the AE query (+0x1c8: 128x64, then the chain), the AF query
                    // (+0x1d0: two 1x1) and CTextureUtil's 16x16 analyser
                    released: 2 + glare.len() + 2 + 3 + 2 + 1 + 2 + 1 + glare_pool_targets.len() + super::half_pool(base, width, height).len() + chain.len(),
                    screen: screen.map(|(_, w, h)| (w, h)),
                    glare,
                    glare_levels,
                    glare_pool: glare_pool_targets,
                    star_at,
                    shaft_at,
                    spectrum,
                    trim,
                    dof_halves,
                    dof_levels,
                })
            }
        }

        /// `CPostEffect::Uninitialize` 0x1403c08a0: the screen bound with its viewport once at the start,
        /// once per released target (`CRenderTexture::UninitializeDevice` 0x1403d29f0) and once at the end
        /// `screen` is the caller's destination (this+0x20), not kept here: a kept view of the back buffer
        /// would make `ResizeBuffers` fail.
        pub fn teardown(&self, screen: Option<ID3D11RenderTargetView>) {
            if let (Some((w, h)), Some(rtv)) = (&self.screen, &screen) {
                for _ in 0..self.released + 2 {
                    unsafe {
                        self.context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                        self.context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: *w as f32, Height: *h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
                    }
                }
            }
        }

        /// `CameraForwardYebis::onResize` 0x140029da0: the game destroys YEBIS and makes it again
        /// at the new size, so every carried-over value (parameter store, exposure) starts over; the noise goes on
        pub fn remade(&self, width: i32, height: i32, camera_width: i32, screen: Option<ID3D11RenderTargetView>) -> Result<Yebis, String> {
            let base = super::base_width(self.video.get_int("POST_PROCESS", "QUALITY").unwrap_or(0), camera_width);
            let mut yebis = Yebis::new(&self.device, &self.context, &self.root, self.filter.clone(), self.video.clone(), width, height, base, screen, self.particles)?;
            yebis.noise = self.noise; // process-wide in the game, it outlives the context (resize log, Task Y2c)
            Ok(yebis)
        }

        unsafe fn viewport(&self, x: f32, y: f32, w: f32, h: f32) {
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: x, TopLeftY: y, Width: w, Height: h, MinDepth: 0.0, MaxDepth: 1.0 }]));
        }

        unsafe fn full_viewport(&self) {
            self.viewport(0.0, 0.0, self.width as f32, self.height as f32);
        }

        /// The effect target with YEBIS's depth.
        unsafe fn effect_with_depth(&self) {
            self.context.OMSetRenderTargets(Some(&[Some(self.effect[self.current].rtv.clone())]), &self.effect_depth);
        }

        /// The effect target in slot 0 of 8, no depth.
        unsafe fn effect_of_eight(&self) {
            let mut views: [Option<ID3D11RenderTargetView>; 8] = Default::default();
            views[0] = Some(self.effect[self.current].rtv.clone());
            self.context.OMSetRenderTargets(Some(&views), None);
        }

        unsafe fn target_only(&self, rtv: &ID3D11RenderTargetView) {
            self.context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
        }

        /// YEBIS's state reset: its default states, every resource slot empty, the cache dropped.
        unsafe fn reset(&mut self) {
            let c = &self.context;
            c.RSSetState(&self.raster);
            c.OMSetDepthStencilState(&self.depth, 0);
            c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            let none: [Option<ID3D11ShaderResourceView>; 128] = std::array::from_fn(|_| None);
            c.PSSetShaderResources(0, Some(&none));
            c.VSSetShaderResources(0, Some(&none));
            c.GSSetShaderResources(0, Some(&none));
            c.SOSetTargets(0, None, None);
            self.cache = Cache::default();
        }

        /// One glare draw into `to` (a viewport of its size): `taps`, `colours`, an optional gamma.
        #[allow(clippy::too_many_arguments)]
        unsafe fn glare_draw(&mut self, t: usize, to: usize, taps: &[[f32; 2]], colours: &[[f32; 4]], gamma: Option<f32>, samplers: &[usize], srvs: &[Option<ID3D11ShaderResourceView>], vertices: Option<Vec<f32>>) -> Result<(), String> {
            self.draw_into(t, self.glare[to].clone(), taps, colours, gamma, samplers, srvs, vertices)
        }

        /// `glare_draw` into any target.
        #[allow(clippy::too_many_arguments)]
        unsafe fn draw_into(&mut self, t: usize, to: (Target, u32, u32), taps: &[[f32; 2]], colours: &[[f32; 4]], gamma: Option<f32>, samplers: &[usize], srvs: &[Option<ID3D11ShaderResourceView>], vertices: Option<Vec<f32>>) -> Result<(), String> {
            let (target, w, h) = to;
            self.target_only(&target.rtv);
            self.viewport(0.0, 0.0, w as f32, h as f32);
            let u = self.cbuffer.glare(t, taps, colours, gamma, &self.used);
            let v = vertices.unwrap_or_else(|| draw_vertices(t, w as f32, h as f32, [0.0; 4], 0.0, 0));
            self.draw(t, &u, samplers, srvs, &v)
        }

        /// `CTextureUtil::Resample` 0x1403f6710 from `from` (`w` x `h`) into glare target `to`: one box tap
        /// (MadT1_S1) or, with `spread`, `GetTexCoordOffset_Box` 0x1403a7e20's 2x2 taps that far apart
        /// (MadT1_S4); a source of another size is read through the linear border sampler.
        /// `mode`: the source's own mode (a sampler index of linear filtering: border, clamp, mirror ...).
        unsafe fn resample(&mut self, from: (Option<ID3D11ShaderResourceView>, u32, u32), to: (Target, u32, u32), spread: f32, mode: usize) -> Result<(), String> {
            let (src, w, h) = from;
            let (tw, th) = (to.1, to.2);
            // the same size samples points
            let sampler = mode + ((w, h) == (tw, th)) as usize;
            if spread > 0.0 {
                let (x, y) = ((1.0 / w as f32) * spread, (1.0 / h as f32) * spread);
                let v = rect_vertices([tw as f32, th as f32], [0.0, 0.0, tw as f32, th as f32], &[src_rect(w, h)]);
                return self.draw_into(T_UP, to, &[[-x, -y], [x, -y], [-x, y], [x, y]], &[[0.25; 4]; 4], None, &[sampler], &[src], Some(v));
            }
            let taps = crate::yebis_cb::box_taps((w, h), (tw, th));
            if taps.len() != 1 {
                return Err(format!("yebis: a glare resample of {w}x{h} to {tw}x{th} ({} taps) not ported", taps.len()));
            }
            let v = rect_vertices([tw as f32, th as f32], [0.0, 0.0, tw as f32, th as f32], &[src_rect(w, h)]);
            self.draw_into(8, to, &[[taps[0][0], taps[0][1]]], &[[1.0; 4]], None, &[sampler], &[src], Some(v))
        }

        /// The glare's bloom (`CRenderGlare::GenerateGlare` 0x14041d470 with no star, ghost, afterimage or
        /// light shafts): the frame halved to the glare size, its bright pass, each level's two-pass
        /// gaussian, the levels summed and the result doubled; returns the double the tone map reads.
        unsafe fn bloom(&mut self, pe: &PostEffect, g: &Glare, exposure: f32, frame: Option<ID3D11ShaderResourceView>) -> Result<(Option<ID3D11ShaderResourceView>, u32, u32), String> {
            let srv = |t: &Target| t.srv.clone();
            let (fw, fh) = (self.width as u32, self.height as u32);
            // GenerateGlare_Source 0x1404160f0: halvings while the pool's next texture is narrower; Resample keeps
            // the frame texture's address (clamp after last frame's tone map) and makes it linear
            let mut cur = (frame, fw, fh);
            let mut sampler = if self.frame_clamped[self.current] { SL } else { SLB };
            for k in 0..self.glare_pool.len() {
                let (t, pw, ph) = self.glare_pool[k].clone();
                if pw >= cur.1 {
                    break;
                }
                let (tw, th) = (pw, ph);
                let taps = crate::yebis_cb::box_taps((cur.1, cur.2), (tw, th));
                if taps.len() != 1 {
                    return Err("yebis: a glare source halving with more than one tap not ported".into());
                }
                self.target_only(&t.rtv);
                self.viewport(0.0, 0.0, tw as f32, th as f32);
                let u = self.cbuffer.glare(8, &[[taps[0][0], taps[0][1]]], &[[1.0; 4]], None, &self.used);
                let v = rect_vertices([tw as f32, th as f32], [0.0, 0.0, tw as f32, th as f32], &[src_rect(cur.1, cur.2)]);
                self.draw(8, &u, &[sampler], &[cur.0.clone()], &v)?;
                cur = (srv(&t), tw, th);
                sampler = SLB;
            }
            // GenerateGlare_Source_BrightPass 0x140415a60 -> DrawRectGPU_BrightPass 0x1403ef1e0: exposure over
            // its scale, the generation range and the threshold, the vignette's rectangle as the second coordinates
            let (_, gw, gh) = self.glare[2].clone();
            let e = exposure / pe.exposure_scale;
            let r = 1.0 / g.range;
            // GenerateGlare_Source_BrightPass 0x140415a60: the vignette program only with a vignette
            let (plain, with) = if g.bright_luminance { (T_BRIGHT_LUM_PLAIN, T_BRIGHT_LUM) } else { (T_BRIGHT_PLAIN, T_BRIGHT) };
            let (t, v) = if self.vignette == 0.0 {
                (plain, rect_vertices([gw as f32, gh as f32], [0.0, 0.0, gw as f32, gh as f32], &[src_rect(cur.1, cur.2)]))
            } else {
                (with, rect_vertices([gw as f32, gh as f32], [0.0, 0.0, gw as f32, gh as f32], &[src_rect(cur.1, cur.2), vignette_rect(fw as f32, fh as f32, self.vignette)]))
            };
            // (without a halving it reads the frame itself, at the frame's address: PP_QUALITY 5 with GLARE 5)
            let sampler = if (cur.1, cur.2) == (gw, gh) { SB } else { sampler };
            self.glare_draw(t, 2, &[[0.0, 0.0]], &[[e, e, e, 1.0], [r, r, r, 1.0], [g.threshold; 4]], None, &[sampler], &[cur.0.clone()], Some(v))?;
            // GenerateGlare_Bloom 0x140416360: per level a gaussian of its radius (red, green and blue at their
            // wavelengths), the first at full size, the later ones from the level before
            let levels = self.glare_levels.clone();
            let n = levels.len();
            let base = rustyac_math::powf(2.0, -((2.min(n) as f32) - 1.0)) * 2.2 * g.radius_scale;
            for (i, (l, [from, result, temp], up)) in levels.iter().enumerate() {
                if l.own {
                    let prev = levels[i - 1].1[1];
                    let (t, w, h) = self.glare[prev].clone();
                    self.resample((srv(&t), w, h), self.glare[*from].clone(), 0.0, SLB)?;
                }
                let radius = if i > 0 { (l.step * base) * rustyac_math::sqrtf(0.75) } else { l.step * base };
                let rgb = [(radius * 6.15e-07) / 6.15e-07, (radius * 5.4e-07) / 6.15e-07, (radius * 4.65e-07) / 6.15e-07, 0.0];
                let taps = crate::yebis_cb::gauss_taps(rgb[0].max(rgb[1]).max(rgb[2]), g.cutoff)?;
                // GaussianFilterMax7x7_2Pass 0x140408da0, ..11x11.. 0x140409970, or the 29x29's own MadT1_Sn path
                let (t, n) = match taps {
                    7 => (T_GAUSS7, 7),
                    11 => (T_GAUSS11, 11),
                    5 => (T_TAPS_ODD[0], 15),
                    9 => (T_TAPS_ODD[1], 15),
                    13 => (T_TAPS_ODD[2], 15),
                    _ => (T_TAPS_ODD[3], 15),
                };
                let (w, h) = l.size;
                for (pass, (src, to, size)) in [(*from, *temp, w), (*temp, *result, h)].into_iter().enumerate() {
                    let ws: Vec<(Vec<f32>, Vec<f32>)> = rgb.iter().map(|&c| crate::yebis_cb::gauss_weights(n, size, c)).collect();
                    let offsets: Vec<[f32; 2]> = ws[0].0[..taps].iter().map(|&o| if pass == 0 { [o, 0.0] } else { [0.0, o] }).collect();
                    let colours: Vec<[f32; 4]> = (0..taps).map(|k| [ws[0].1[k], ws[1].1[k], ws[2].1[k], ws[3].1[k]]).collect();
                    let s = srv(&self.glare[src].0);
                    let v = rect_vertices([w as f32, h as f32], [0.0, 0.0, w as f32, h as f32], &[src_rect(w, h)]);
                    self.glare_draw(t, to, &offsets, &colours, None, &[SB], &[s], Some(v))?;
                }
                if let Some(u) = up {
                    self.resample((srv(&self.glare[*result].0), w, h), self.glare[*u].clone(), 0.375, SLB)?;
                }
            }
            // GenerateGlare_Bloom_CompositeSubLevels 0x140416be0: the levels summed with GetBloomCompositeLuminanceScale,
            // tinted by the diffraction colours from the dispersion's base level on, then the gamma
            let (_, sw, sh) = self.glare[5].clone();
            let scale = g.range * 0.035 * g.glare_lum * (g.luminance / pe.mapping_div) * g.bloom_lum;
            let ratio = (rustyac_math::logf(gw as f32 / fw as f32) / rustyac_math::logf(2.0)) as i32 + 2;
            let first = (ratio - 1 + g.base_level).min(n as i32 - 3);
            const DIFFRACTION: [[f32; 4]; 3] = [[0.0, 0.0, 1.0, 1.0], [0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
            let (mut srvs, mut samplers, mut colours) = (Vec::new(), Vec::new(), Vec::new());
            for (i, (l, idx, up)) in levels.iter().enumerate() {
                let (t, w, h) = self.glare[up.unwrap_or(idx[1])].clone();
                srvs.push(srv(&t));
                samplers.push(if (w, h) == (sw, sh) { SP } else { SL });
                let _ = l;
                let f = (1.0 / rustyac_math::powf(1.0, i as f32)) * scale;
                let i = i as i32;
                if g.dispersion <= 0.0 || i < first || first < 0 {
                    colours.push([f; 4]);
                } else {
                    let d = DIFFRACTION[((i - first).max(0) % 3) as usize];
                    let c = d.map(|d| (d - 1.0) * g.dispersion + 1.0);
                    let f = (3.0 / (c[0].abs() + c[1].abs() + c[2].abs())) * f;
                    colours.push(c.map(|c| c * f));
                }
            }
            if !(3..=7).contains(&n) {
                return Err(format!("yebis: a bloom of {n} levels not ported"));
            }
            // (the source rectangle is the first level's, at the sum's size)
            let v = rect_vertices([sw as f32, sh as f32], [0.0, 0.0, sw as f32, sh as f32], &[src_rect(gw, gh)]);
            self.glare_draw(T_SUM + n - 3, 5, &[], &colours, Some(1.0 / g.gamma), &samplers, &srvs, Some(v))?;
            // GenerateGlare_Composite 0x14041afb0: the sum (and the star), then doubled with the result blur's spread
            let mut parts = vec![5];
            if let Some(s) = &g.star {
                parts.push(self.star(pe, g, s)?);
            }
            if let Some(h) = &g.ghost {
                // IsEnabledGhost 0x140413780
                if g.luminance > 0.0 && g.glare_lum > 0.0 && h.lum > 0.0 {
                    parts.push(self.ghost(pe, g, h, ratio)?);
                }
            }
            if let Some(sh) = &g.shafts {
                if let Some(sun) = pe.sun {
                    parts.push(self.shafts(pe, g, sh, exposure, sun)?);
                }
            }
            self.mad(3, &parts, &vec![[1.0; 4]; parts.len()])?;
            // (a double no wider than the glare: FillDefaults' blur off, see the creation)
            if g.blur <= 0.0 || self.glare[4].1 <= gw {
                return Ok((srv(&self.glare[3].0), gw, gh));
            }
            self.resample((srv(&self.glare[3].0), gw, gh), self.glare[4].clone(), g.blur.min(1.0) * 0.5, SLB)?;
            let (t, dw, dh) = self.glare[4].clone();
            Ok((srv(&t), dw, dh))
        }

        /// `CTextureUtil::MadTn` 0x140400be0 of glare targets `from` into `to`: the first one's rectangle, each read
        /// through the point sampler at the target's size, else the linear one.
        unsafe fn mad(&mut self, to: usize, from: &[usize], colours: &[[f32; 4]]) -> Result<(), String> {
            let t = match from.len() {
                1 => T_COPY,
                4 => T_MAD4,
                n => T_MAD2 + n - 2,
            };
            let (samplers, srvs, v) = self.sources(to, from);
            self.glare_draw(t, to, &[], colours, None, &samplers, &srvs, Some(v))
        }

        /// The samplers, views and vertices of a draw of glare targets `from` into `to` (see `mad`).
        fn sources(&self, to: usize, from: &[usize]) -> (Vec<usize>, Vec<Option<ID3D11ShaderResourceView>>, Vec<f32>) {
            let (_, tw, th) = self.glare[to].clone();
            let samplers = from.iter().map(|&i| if (self.glare[i].1, self.glare[i].2) == (tw, th) { SP } else { SL }).collect();
            let srvs = from.iter().map(|&i| self.glare[i].0.srv.clone()).collect();
            let (_, w, h) = self.glare[from[0]].clone();
            (samplers, srvs, rect_vertices([tw as f32, th as f32], [0.0, 0.0, tw as f32, th as f32], &[src_rect(w, h)]))
        }

        /// The star (`GenerateGlare_Star` 0x140417200, `GenerateGlare_StarStreaks` 0x140417570; a custom shape has no
        /// dispersion, so every streak colour is white, a library one colours pass p with row 7-p of `Star::colours`
        /// and, forced, draws its last pass back at the first direction with the table's own colours): the bright pass softened with the first two levels' results, each
        /// streak in passes of up to four taps, each pass four times longer, then the streaks' maximum. Returns the
        /// target the composite adds.
        unsafe fn star(&mut self, pe: &PostEffect, g: &Glare, s: &Star) -> Result<usize, String> {
            use rustyac_math::{cosf, logf, powf, sinf};
            // the softened source, L[3] (+0x248's fourth: glare[1], which nothing else draws)
            let f = s.softness;
            let levels = self.glare_levels.clone();
            let two = levels.len() > 1;
            let mut from = vec![2, levels[0].1[1]];
            let mut colours = vec![[1.0 - f; 4], [f * if two { 1.0 - f } else { 1.0 }; 4]];
            if two {
                from.push(levels[1].1[1]);
                colours.push([f * f; 4]);
            }
            self.mad(1, &from, &colours)?;
            let n = s.lines.min(8);
            let (lines, work, dst) = (self.star_at, [self.star_at + 8, self.star_at + 9], self.star_at + 10);
            let (_, ww, wh) = self.glare[work[0]].clone();
            let (w, h) = (ww as f32, wh as f32);
            let maxdim = if w <= h { h } else { w };
            let mut base = s.inclination;
            if 6.2831855 <= base.abs() {
                base = (6.2831855 / n as f32) * -0.5;
            }
            if s.rotation {
                base -= crate::yebis_cb::diaphragm_angle(s.diaphragm, pe.fov);
            }
            // CRenderGlare+0xa8/+0xac: the glare size
            let (_, ga, gh) = self.glare[2].clone();
            let aspect = (ga as f32 / gh as f32) / (w / h);
            // GetStarCompositeLuminanceScale 0x14041bcd0 (CRenderGlare+0xe6 is always 0: 0.04)
            // GenerateGlare_StarStreaks 0x140417570's order: the definition's factor first (sepia's star tells)
            let lum = g.range * 0.035 * g.glare_lum * (g.luminance / pe.mapping_div) * (0.04 * s.lum);
            let k = powf(lum, 0.2);
            // GetStarRadiusScale 0x14041baa0 with no FOV dependence; the glare's R11G11B10 keeps the threshold unclamped
            let radius = 1.0 * ga as f32 * 0.003125;
            let (table, rows) = s.colours();
            let mut weights = vec![1.0f32; n];
            for (line, weight) in weights.iter_mut().enumerate() {
                let len = 1.0f32;
                let mut atten = if s.secondary > 0.0 && line % 2 == 1 { s.secondary } else { s.length };
                if atten <= 1e-05 {
                    atten = 1e-05;
                }
                let angle = (((360.0 / s.lines as f32) * std::f32::consts::PI) / 180.0) * line as f32;
                let first = [sinf(base - angle) * (len / w), cosf(base - angle) * (len / h) * aspect];
                let mut dir = first;
                let mut a = ((len / radius) / 6.0) / atten;
                let spread = (1.0 / a) * 1.5;
                if s.force {
                    a /= 1.5;
                }
                let x = (logf(s.threshold) / logf(0.5)) / a;
                let (passes, last_taps) = if s.force {
                    ((logf(x) / logf(4.0) + 1.0) as i32, 4)
                } else {
                    let passes = (logf(x) / logf(4.0)).ceil() as i32;
                    let last_taps = match ((x * 4.0) / powf(4.0, passes as f32)).ceil() as i32 {
                        0 => 4,
                        t => t.min(4),
                    };
                    (passes, last_taps)
                };
                let (mut src, mut ping) = (1, 0);
                let mut p = 0;
                while p < passes {
                    let last = p as f32 == (passes - 1) as f32;
                    let limit = if s.force { p as f32 == (passes - 2) as f32 } else { last };
                    let b = if p == 0 { 0.1875 } else { k };
                    if p > 0 {
                        *weight *= 1.0 / k;
                    }
                    let ws: Vec<f32> = (0..4)
                        .map(|j| {
                            let e = if !s.force {
                                j as f32 * a
                            } else if last {
                                j as f32 * 0.25
                            } else {
                                j as f32 * a * 4.0
                            };
                            powf(0.5, e) * b
                        })
                        .collect();
                    let q = powf(4.0, p as f32);
                    let nt = if s.force { 4 } else { ((((q * len - 1.0) + maxdim) / (q * len)).ceil() as i32).min(4) };
                    let taps = nt.min(if limit { last_taps } else { 4 }) as usize;
                    let to = if !last && taps > 3 { work[ping] } else { lines + line };
                    let (_, tw, th) = self.glare[to].clone();
                    let (_, sw, sh) = self.glare[src].clone();
                    let [_, _, su, sv] = src_rect(sw, sh);
                    let forced = s.force && last;
                    if forced {
                        dir = first;
                    }
                    let offsets: Vec<[f32; 2]> = (0..taps)
                        .map(|j| if forced { [((j as f32 * dir[0] + 0.0) * su) * spread, ((j as f32 * dir[1] + 0.0) * sv) * spread] } else { [(j as f32 * dir[0] + 0.0) * su, (j as f32 * dir[1] + 0.0) * sv] })
                        .collect();
                    // shortcut: rows past the eighth pass would read before the game's table, no shipped star gets there
                    let row = rows[7usize.saturating_sub(p as usize)];
                    let colours: Vec<[f32; 4]> = ws[..taps].iter().enumerate().map(|(j, &w)| (if forced { table[j] } else { row[j] }).map(|c| w * c)).collect();
                    let v = rect_vertices([tw as f32, th as f32], [0.0, 0.0, tw as f32, th as f32], &[[0.0, 0.0, su, sv]]);
                    let view = self.glare[src].0.srv.clone();
                    self.glare_draw(T_TAPS[taps - 1], to, &offsets, &colours, None, &[SLB], &[view], Some(v))?;
                    dir = [dir[0] * 4.0, dir[1] * 4.0];
                    a *= 4.0;
                    src = to;
                    ping = (ping + 1) % 2;
                    if taps < 4 {
                        break;
                    }
                    p += 1;
                }
            }
            // MaxColorTn 0x1404010a0 at gamma +0x1ac (1): one streak is MadTn of its weighted colour
            let c = lum * 5.3333335;
            let from: Vec<usize> = (lines..lines + n).collect();
            if n == 1 {
                self.mad(dst, &from, &[[c * weights[0]; 4]])?;
            } else {
                let t = T_MAX2 + n - 2;
                let (samplers, srvs, v) = self.sources(dst, &from);
                let (target, tw, th) = self.glare[dst].clone();
                self.target_only(&target.rtv);
                self.viewport(0.0, 0.0, tw as f32, th as f32);
                let u = self.cbuffer.max_color(t, &weights, [c; 4], &self.used);
                self.draw(t, &u, &samplers, &srvs, &v)?;
            }
            Ok(dst)
        }

        /// The ghost (`GenerateGlare_Ghost` 0x1404185b0): three levels and the trim circle mirrored through the
        /// centre into the ghost's second target, then that and two levels mirrored again into its first, which
        /// the composite adds. Each pass is a four-vertex draw of its first source's rectangle.
        unsafe fn ghost(&mut self, pe: &PostEffect, g: &Glare, h: &crate::yebis_cb::Ghost, ratio: i32) -> Result<usize, String> {
            let (dst, mid) = (self.star_at + 12, self.star_at + 13);
            let levels: Vec<usize> = self.glare_levels.iter().map(|l| l.1[1]).collect();
            // GenerateGlare_Ghost 0x1404185b0's order
            let f = g.range * 0.035 * g.glare_lum * (g.luminance / pe.mapping_div) * ((h.lum * 0.05) * 20.0);
            let angle = crate::yebis_cb::diaphragm_angle(h.diaphragm, pe.fov);
            // pass 1 reads the level at the ratio first, pass 2 the ghost's second target (Task Y5d)
            let (_, w, hh) = self.glare[levels[ratio.clamp(0, levels.len() as i32 - 1) as usize]].clone();
            let [_, _, u, v] = src_rect(w, hh);
            let (_, mw, mh) = self.glare[mid].clone();
            let [_, _, u2, v2] = src_rect(mw, mh);
            let ([one, two], at) = crate::yebis_cb::ghost_passes(h, f, ratio, levels.len(), angle, [u, v], [u2, v2], w as f32 / hh as f32);
            let [a, b, c] = at.map(|i| self.glare[levels[i]].0.srv.clone());
            for (t, to, pass, srvs, [u, v]) in [(T_GHOST4, mid, one, vec![a.clone(), b, c.clone(), c.clone(), self.trim.clone()], [u, v]), (T_GHOST3, dst, two, vec![self.glare[mid].0.srv.clone(), c, a], [u2, v2])] {
                let (target, tw, th) = self.glare[to].clone();
                self.target_only(&target.rtv);
                self.viewport(0.0, 0.0, tw as f32, th as f32);
                let up = self.cbuffer.ghost(t, &pass.centres, &pass.p32, &pass.scaler, &pass.colours, &self.used);
                // DrawRectGPU_GetVerticesAndTexCoords 0x1403e5ce0's positions: (x - size/2) * (2/size), not 1 in f32
                // at some odd sizes (564 wide: 0.99999994)
                let (hw, hh2, sx, sy) = (-(tw as f32) * 0.5, -(th as f32) * 0.5, 2.0 / tw as f32, 2.0 / th as f32);
                let (x0, x1, y0, y1) = (hw * sx, (hw + tw as f32) * sx, hh2 * sy, (hh2 + th as f32) * sy);
                let quad = [x0, y1, 0.0, v, x0, y0, 0.0, 0.0, x1, y1, u, v, x1, y0, u, 0.0];
                self.draw(t, &up, &vec![SLBM; srvs.len()], &srvs, &quad)?;
            }
            Ok(dst)
        }

        /// The light shafts (`GenerateGlare_LightShaft` 0x14041a5d0) while the sun is in front of the camera:
        /// the bright pass masked by the sun's angle and the film grain, smeared towards the sun in passes of up
        /// to four taps, each four times longer, then the diffraction ring. Returns the target the composite adds.
        unsafe fn shafts(&mut self, pe: &PostEffect, g: &Glare, sh: &Shafts, exposure: f32, sun: ([f32; 2], [f32; 3])) -> Result<usize, String> {
            use rustyac_math::{atanf, logf, powf, tanf};
            let (pos, sun_colour) = sun;
            let mut c = sh.colour(sun_colour);
            let (mask, result) = (self.shaft_at, self.shaft_at + 1);
            let work = [self.star_at + 8, self.star_at + 9];
            // SetPerspectiveByFrustum 0x14041d360: CRenderGlare+0x128 and +0x124
            let mut t128 = tanf(pe.fov * 0.5);
            if t128 <= 0.0 {
                t128 = tanf(std::f32::consts::FRAC_PI_8);
            }
            let fov = atanf(t128) * 2.0;
            // the screen rectangle the sun's angle is measured over (DrawRectGPU_LightShaftMask, DrawRectGPU_Corona)
            let sun_rect = |f: f32, wf: f32, hf: f32| {
                let fx = f * (wf / hf);
                let x0 = -fx;
                let (dx, dy) = (fx - x0, f - -f);
                let (x0, y0) = (x0 - dx * 0.0, -f - dy * 0.0);
                [dx * ((0.0 - pos[0]) + 0.5) + x0, dy * ((0.0 - pos[1]) + 0.5) + y0, dx * ((1.0 - pos[0]) + 0.5) + x0, dy * ((1.0 - pos[1]) + 0.5) + y0]
            };
            // UpdateParameters_Base 0x1403b46c0: the depth threshold through GetDepthConversionFactor
            let df = pe.depth_factor();
            let depth = df[0] * sh.depth_threshold + df[1];
            // CTextureUtil::LightShaftMask 0x140401d90 -> DrawRectGPU_LightShaftMask 0x1403f5460
            let (target, w, h) = self.glare[mask].clone();
            let (_, bw, bh) = self.glare[2].clone();
            let (wf, hf) = (w as f32, h as f32);
            self.target_only(&target.rtv);
            self.viewport(0.0, 0.0, wf, hf);
            let u = self.cbuffer.shaft(T_SHAFT_MASK, [sh.noise_mask, sh.glare_ratio, exposure / g.range, depth], [256.0, sh.noise_frequency, (pos[1] + pos[0]) + 0.0, 0.0], &[], &self.used);
            let v = rect_vertices([wf, hf], [0.0, 0.0, wf, hf], &[src_rect(bw, bh), [0.0, 0.0, (wf / hf) * 4.0, 4.0], sun_rect((sh.angle_attenuation * 0.125) * tanf(fov * 0.5), wf, hf)]);
            let srvs = [self.glare[2].0.srv.clone(), Some(self.grain.clone())];
            self.draw(T_SHAFT_MASK, &u, &[SLB, SW_TRI], &srvs, &v)?;
            // the radial passes: GetLightShaftRadiusScale 0x14041bb30 (GetStarRadiusScale(1.0), the FOV term in)
            let radius = (1.0 / (t128 * 2.0)) * (bw as f32 * 0.003125); // (machine order: the width term first)
            let mut a = (0.0625 / radius) / sh.length;
            let x = (logf(0.001) / logf(0.5)) / a;
            let passes = ((logf(x) / logf(4.0)).ceil() as i32).min(8);
            let last_taps = match ((x * 4.0) / powf(4.0, passes as f32)).ceil() as i32 {
                0 => 4,
                t => t.min(4),
            };
            let base = powf(t128 * 0.25, 1.0 / passes as f32);
            if sh.ring == 0.0 && sh.ring_colour[..3] != [1.0; 3] {
                for (v, r) in c.iter_mut().zip(&sh.ring_colour[..3]) {
                    *v *= r;
                }
            }
            // GetLightShaftCompositeLuminanceScale 0x14041bdf0, then four
            let scale = ((((g.range * 0.035) * g.glare_lum) * (g.luminance / pe.mapping_div)) * 0.25) * 4.0;
            let (mut src, mut ping) = (mask, 0);
            for i in 0..passes {
                let last = i == passes - 1;
                let weight = if i == 0 { base * 0.25 } else { base };
                let taps = if last { last_taps as usize } else { 4 };
                let coloured = last || taps < 4;
                let to = if coloured && sh.ring == 0.0 { result } else { work[ping] };
                let (_, tw, th) = self.glare[to].clone();
                let (_, sw, sh_) = self.glare[src].clone();
                let (swf, shf) = (sw as f32, sh_ as f32);
                let (px, py) = (((swf * pos[0]) * (1.0 / swf)) + 0.0, ((shf * pos[1]) * (1.0 / shf)) + 0.0);
                let (su, sv) = (swf * (1.0 / swf) + 0.0, shf * (1.0 / shf) + 0.0);
                let q = (powf(4.0, i as f32) as i32) as f32;
                let hh = th as f32;
                let rects: Vec<[f32; 4]> = (0..taps)
                    .map(|k| {
                        let s = 1.0 / (((k as f32 * q) + hh) / hh);
                        [((0.0 - px) * s) + px, ((0.0 - py) * s) + py, ((su - px) * s) + px, ((sv - py) * s) + py]
                    })
                    .collect();
                let colours: Vec<[f32; 4]> = (0..taps)
                    .map(|k| {
                        // the game's vector powf (__vdecl_powf4), not the scalar one
                        let w = rustyac_math::powf4_half(k as f32 * a) * weight;
                        if coloured {
                            [c[0] * w, c[1] * w, c[2] * w, c[3] * w].map(|v| v * scale)
                        } else {
                            [w; 4]
                        }
                    })
                    .collect();
                let v = rect_vertices([tw as f32, hh], [0.0, 0.0, tw as f32, hh], &rects);
                let view = self.glare[src].0.srv.clone();
                self.glare_draw(T_RADIAL + taps - 1, to, &[], &colours, None, &[SLB], &[view], Some(v))?;
                a *= 4.0;
                src = to;
                ping = (ping + 1) % 2;
                if taps < 4 {
                    break;
                }
            }
            // CTextureUtil::Corona 0x140401f70 -> DrawRectGPU_Corona 0x1403f5800: the ring around the sun
            if sh.ring != 0.0 {
                let (_, sw, sh_) = self.glare[src].clone();
                let (target, w, h) = self.glare[result].clone();
                let (wf, hf) = (w as f32, h as f32);
                self.target_only(&target.rtv);
                self.viewport(0.0, 0.0, wf, hf);
                let colours = [sh.ring_colour, [1.5, 1.5, 1.5, 0.0], [1.0, 1.0, 1.0, 0.0]];
                let u = self.cbuffer.shaft(T_CORONA, [sh.ring, sh.ring_attenuation, sh.ring_order, 0.0], [2.0, 4.0, 0.0, 1.0], &colours, &self.used);
                let ring = sun_rect((std::f32::consts::PI / sh.ring_radius) * tanf(fov * 0.5), wf, hf);
                let v = rect_vertices([wf, hf], [0.0, 0.0, wf, hf], &[src_rect(sw, sh_), ring]);
                let srvs = [self.glare[src].0.srv.clone(), Some(self.spectrum.clone()), Some(self.grain.clone())];
                self.draw(T_CORONA, &u, &[SLB, SW_TRI, SW_TRI], &srvs, &v)?;
            }
            Ok(result)
        }

        /// Technique `d`'s input layout: its inputs packed one after another.
        /// `CPostEffect::ApplyEffects_DepthOfField` 0x1403b3610 (`CDepthOfField::ApplyDepthOfField` 0x140430ed0) after a
        /// round's first draw: the effect surface halved down to each drawn level's source, each level's front blur
        /// factor and hexagon passes, then the levels composited into the other effect surface (Task Y6).
        unsafe fn dof(&mut self, pe: &PostEffect, d: &crate::yebis_cb::Dof) -> Result<(), String> {
            use crate::yebis_cb::{dof_counts, dof_level_factors, hexagon_taps, T_DOF_BLEND, T_DOF_FRONT, T_DOF_LEVELS, T_GAUSS7};
            let l30: Vec<f32> = self.dof_levels.iter().map(|l| l.0.l30).collect();
            // the edge flags (+0x140): AC's EDGE_QUALITY call fails below DOF 5, which keeps the table's
            let flags = if d.quality == 1 { 0 } else { 2 };
            let f = dof_level_factors(&l30, d.ratio, flags);
            let (near_n, far_n) = dof_counts(&f, pe.near, pe.far, d.focus);
            let n = near_n.max(far_n);
            if n == 0 || self.dof_levels[..n].iter().any(|l| l.0.doubled) {
                return Err(format!("yebis: the depth of field with {n} levels (a doubled one, or none): not ported"));
            }
            // the pool then hands the glare's halving a level's texture, and their sampler modes mix
            if pe.glare.is_some() && self.dof_levels[..n].iter().any(|l| self.glare_pool.iter().any(|g| (g.1, g.2) == l.0.size)) {
                return Err("yebis: the depth of field with a level of a glare halving's size (PP_QUALITY 5): not ported".into());
            }
            self.reset();
            let (w, h) = (self.width as u32, self.height as u32);
            let mut cur = (self.effect[0].srv.clone(), w, h);
            // each source keeps its own mode
            for (t, hw, hh) in self.dof_halves.clone() {
                let mode = self.mode(&cur.0);
                self.resample(cur, (t.clone(), hw, hh), 0.0, mode)?;
                cur = (t.srv, hw, hh);
            }
            // ApplyDepthOfField_Source: a level's source is resampled only when its size differs
            let mut srcs = vec![];
            let levels = self.dof_levels[..n].to_vec();
            for (l, t) in levels {
                if (cur.1, cur.2) != l.size {
                    let mode = self.mode(&cur.0);
                    self.resample(cur, (t[0].clone(), l.size.0, l.size.1), 0.0, mode)?;
                    cur = (t[0].srv.clone(), l.size.0, l.size.1);
                }
                srcs.push(cur.0.clone());
            }
            // ApplyDepthOfField_ApertureFilterLevel_EAPERTUREFM_DISC 0x140425f60
            let [_, _, z, sw] = d.store;
            let levels = self.dof_levels[..n].to_vec();
            for (i, (l, t)) in levels.into_iter().enumerate() {
                let (lw, lh) = l.size;
                let v = || rect_vertices([lw as f32, lh as f32], [0.0, 0.0, lw as f32, lh as f32], &[src_rect(lw, lh)]);
                let mut src = srcs[i].clone();
                if flags & 2 != 0 && i < near_n {
                    let a = -f[i][2];
                    // (the front blur factor puts its source's mode back: RENDER_ORACLE_SAMPLER_LOG)
                    let (key, mode) = (src.as_ref().map(|v| self.mode_key(v)), self.mode(&src));
                    self.draw_into(T_DOF_BLEND, (t[1].clone(), lw, lh), &[], &[[1.0, 1.0, 1.0, a * z], [0.0, 0.0, 0.0, f[i][3] + a * sw]], None, &[9], &[src], Some(v()))?;
                    if let Some(key) = key {
                        self.modes.insert(key, mode);
                    }
                    src = t[1].srv.clone();
                }
                for p in 0..l.passes {
                    let (taps, colours) = hexagon_taps(d.angle, p, l.passes, lw, lh);
                    let to = if p + 1 == l.passes { t[2].clone() } else { t[3 + (p & 1) as usize].clone() };
                    self.draw_into(T_GAUSS7, (to.clone(), lw, lh), &taps, &colours, None, &[8], &[src], Some(v()))?;
                    src = to.srv;
                }
            }
            // ApplyDepthOfField_Composite 0x140428da0 into the other surface
            self.current = 1;
            self.effect_with_depth();
            self.full_viewport();
            let t = if flags & 2 != 0 { T_DOF_FRONT + far_n * 4 + near_n } else { T_DOF_LEVELS + n };
            let factors: Vec<[f32; 2]> = std::iter::once([0.0; 2]).chain(f[..n].iter().map(|l| [l[0], l[1]])).collect();
            let u = self.cbuffer.dof_composite(t, d.store[0], &factors, &self.used);
            let mut srvs = vec![self.effect[0].srv.clone()];
            srvs.extend(self.dof_levels[..n].iter().map(|l| l.1[2].srv.clone()));
            let mut samplers = vec![SL; n + 1];
            samplers[0] = SP;
            let (l0w, l0h) = self.dof_levels[0].0.size;
            let v = rect_vertices([w as f32, h as f32], [0.0, 0.0, w as f32, h as f32], &[src_rect(w, h), src_rect(l0w, l0h)]);
            self.draw(t, &u, &samplers, &srvs, &v)?;
            // the composite read the first surface point/clamp: that becomes its own mode (the glare's halving reads it so)
            self.frame_clamped[0] = true;
            self.effect_of_eight();
            Ok(())
        }

        unsafe fn layout(&self, d: usize) -> Result<ID3D11InputLayout, String> {
            let elements: Vec<D3D11_INPUT_ELEMENT_DESC> = self.inputs[d]
                .iter()
                .enumerate()
                .map(|(i, (name, index, format))| D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(name.as_ptr().cast()),
                    SemanticIndex: *index,
                    Format: *format,
                    InputSlot: 0,
                    AlignedByteOffset: self.inputs[d][..i].iter().map(|e| format_bytes(e.2)).sum(),
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                })
                .collect();
            let mut l = None;
            err("CreateInputLayout", self.device.CreateInputLayout(&elements, &self.code[d].0, Some(&mut l)))?;
            l.ok_or_else(|| "yebis: no layout".into())
        }

        /// One draw: `samplers` are (slot, sampler) and only the changed ones are set.
        unsafe fn mode_key(&self, srv: &ID3D11ShaderResourceView) -> usize {
            use windows::core::Interface;
            srv.GetResource().map_or(0, |r| r.as_raw() as usize)
        }

        /// The saved mode of `srv`'s texture (border before any sampler bound it).
        unsafe fn mode(&self, srv: &Option<ID3D11ShaderResourceView>) -> usize {
            srv.as_ref().and_then(|v| self.modes.get(&self.mode_key(v)).copied()).unwrap_or(SLB)
        }

        unsafe fn draw(&mut self, d: usize, upload: &[u8; 4096], samplers: &[usize], srvs: &[Option<ID3D11ShaderResourceView>], vertices: &[f32]) -> Result<(), String> {
            self.pass(d, upload, samplers, srvs)?;
            let c = self.context.clone();
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            let bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_le_bytes()).collect();
            let (index, offset, discard) = self.ring_at.alloc(bytes.len() as u32);
            let vb = self.ring[index].clone();
            let kind = if discard { D3D11_MAP_WRITE_DISCARD } else { D3D11_MAP_WRITE_NO_OVERWRITE };
            err("Map", c.Map(&vb, 0, kind, 0, Some(&mut m)))?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), m.pData.cast::<u8>().add(offset as usize), bytes.len());
            c.Unmap(&vb, 0);
            c.IASetInputLayout(self.layouts[d].as_ref());
            let stride = self.strides[d];
            c.IASetVertexBuffers(0, 1, Some(&Some(vb)), Some(&stride), Some(&offset));
            c.IASetIndexBuffer(&self.ib, DXGI_FORMAT_R16_UINT, 0);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            // three vertices, or the ghost's four (DrawRectGPU_Effect_XY_TEX2D1)
            c.DrawIndexed(vertices.len() as u32 * 4 / stride, 0, 0);
            let none: Vec<Option<ID3D11ShaderResourceView>> = vec![None; srvs.len()];
            c.PSSetShaderResources(0, Some(&none));
            Ok(())
        }

        /// `CTinyEffect::SetPassState` and the variables' upload: states, programs, samplers, textures, layout.
        unsafe fn pass(&mut self, d: usize, upload: &[u8; 4096], samplers: &[usize], srvs: &[Option<ID3D11ShaderResourceView>]) -> Result<(), String> {
            let c = self.context.clone();
            if !self.cache.blend {
                c.OMSetBlendState(&self.blend, Some(&[0.0; 4]), 0xffff_ffff);
                self.cache.blend = true;
            }
            if !self.cache.raster {
                c.RSSetState(&self.raster);
                self.cache.raster = true;
            }
            // CTinyEffect::SetPassState empties the slots before it makes the pass's programs
            if self.srv_slots > 0 {
                c.PSSetShaderResources(0, Some(&vec![None; self.srv_slots]));
            }
            // a program is made on its first draw unless another technique made the same one: the same number in
            // YEBIS's container (MadT1 and MadT1_S1 have equal pixel bytecode but two numbers, two programs)
            if self.vs[d].is_none() {
                let same = (0..self.code.len()).find(|&j| self.vs[j].is_some() && self.ids[j].0 == self.ids[d].0);
                self.vs[d] = match same {
                    Some(j) => self.vs[j].clone(),
                    None => {
                        let mut s = None;
                        err("CreateVertexShader", self.device.CreateVertexShader(&self.code[d].0, None, Some(&mut s)))?;
                        s
                    }
                };
            }
            if self.cb.is_none() {
                self.cb = Some(buffer(&self.device, 4096, D3D11_BIND_CONSTANT_BUFFER.0, true, None)?);
            }
            if self.ps[d].is_none() {
                let same = (0..self.code.len()).find(|&j| self.ps[j].is_some() && self.ids[j].1 == self.ids[d].1);
                self.ps[d] = match same {
                    Some(j) => self.ps[j].clone(),
                    None => {
                        let mut s = None;
                        err("CreatePixelShader", self.device.CreatePixelShader(&self.code[d].1, None, Some(&mut s)))?;
                        self.srv_slots = self.srv_slots.max(srvs.len()); // CTinyEffect::CreateParameter
                        s
                    }
                };
            }
            let cb = self.cb.clone().ok_or("yebis: no cbuffer")?;
            c.GSSetShader(None, None);
            c.HSSetShader(None, None);
            c.DSSetShader(None, None);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            err("Map", c.Map(&cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)))?;
            c.VSSetShader(self.vs[d].as_ref(), None);
            c.PSSetShader(self.ps[d].as_ref(), None);
            for (slot, &s) in samplers.iter().enumerate() {
                if self.cache.samplers[slot] != Some(s) {
                    c.PSSetSamplers(slot as u32, Some(&[Some(self.samplers[s].clone())]));
                    self.cache.samplers[slot] = Some(s);
                }
            }
            c.PSSetShaderResources(0, Some(srvs));
            for (srv, &s) in srvs.iter().zip(samplers) {
                if let Some(srv) = srv {
                    self.modes.insert(self.mode_key(srv), s & !3);
                }
            }
            std::ptr::copy_nonoverlapping(upload.as_ptr(), m.pData.cast::<u8>(), 4096);
            c.Unmap(&cb, 0);
            c.VSSetConstantBuffers(0, Some(&[Some(cb.clone())]));
            // a pixel program without variables gets no buffer (the depth of field's TS0_1 composite, Task Y6)
            if rustyac_dxbc::parse(&self.code[d].1).map_or(true, |s| !s.reflection.constant_buffers.is_empty()) {
                c.PSSetConstantBuffers(0, Some(&[Some(cb)]));
            }
            if self.layouts[d].is_none() {
                let same = (0..self.code.len()).find(|&j| self.layouts[j].is_some() && self.inputs[j] == self.inputs[d]);
                self.layouts[d] = match same {
                    Some(j) => self.layouts[j].clone(),
                    None => Some(self.layout(d)?),
                };
            }
            Ok(())
        }

        /// `CTextureUtil::HeatShimmerParticles` 0x1403fdc70 after a round's first draw: the effect
        /// surface copied into the third one, then the particles drawn over the copy.
        unsafe fn shimmer(&mut self, pe: &PostEffect, heat: &crate::yebis_heat::Shimmer, e0: &Option<ID3D11ShaderResourceView>) -> Result<(), String> {
            let (w, h) = (self.width as f32, self.height as f32);
            self.current = 2;
            self.effect_with_depth();
            self.full_viewport();
            let u = self.cbuffer.copy(&self.used);
            self.draw(8, &u, &[self.copy_sampler], std::slice::from_ref(e0), &draw_vertices(8, w, h, [0.0; 4], 0.0, 0))?;
            let depth = pe.depth_factor();
            let d = crate::yebis_heat::draw(&heat.particles, &heat.view, &heat.frustum, [depth[2], depth[3]], [w, h], [1.0, 1.0], &heat.params);
            let c = self.context.clone();
            // three buffers in turn, the first map of each round of three a discard
            self.heat_at = (self.heat_at + 1) % 3;
            let kind = if self.heat_at == 0 { D3D11_MAP_WRITE_DISCARD } else { D3D11_MAP_WRITE_NO_OVERWRITE };
            let vb = self.heat_vb[self.heat_at].clone();
            let bytes: Vec<u8> = d.vertices.iter().flat_map(|v| v.to_le_bytes()).collect();
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            err("Map", c.Map(&vb, 0, kind, 0, Some(&mut m)))?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), m.pData.cast::<u8>(), bytes.len());
            c.Unmap(&vb, 0);
            let u = self.cbuffer.heat(&d, &self.used);
            let srvs = [e0.clone(), Some(self.grain.clone()), Some(self.gaussian.clone())];
            self.pass(d.technique, &u, &[8, 15, 3], &srvs)?;
            c.IASetInputLayout(self.layouts[d.technique].as_ref());
            let stride = self.strides[d.technique];
            c.IASetVertexBuffers(0, 1, Some(&Some(vb)), Some(&stride), Some(&0));
            c.IASetIndexBuffer(&self.heat_ib, DXGI_FORMAT_R16_UINT, 0);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.DrawIndexed(heat.particles.len() as u32 * 6, 0, 0);
            c.PSSetShaderResources(0, Some(&[None, None, None]));
            self.effect_of_eight();
            Ok(())
        }

        /// The 1x1 analysis result, if the GPU has finished it (YEBIS never waits).
        unsafe fn read_back(&self) -> Option<[u8; 4]> {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&self.readback, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut m)).ok()?;
            let mut out = [0u8; 4];
            std::ptr::copy_nonoverlapping(m.pData.cast::<u8>(), out.as_mut_ptr(), 4);
            self.context.Unmap(&self.readback, 0);
            Some(out)
        }

        /// `CPostEffect::ApplyEffects` for AC's chain: `depth` and `scene` are the game's
        /// (resolved) depth and colour, `destination` the back buffer or the FXAA target.
        /// `heat` is the shimmer's particles with video.ini's HEAT_SHIMMER on and the shimmer active.
        pub fn render(&mut self, depth: &ID3D11ShaderResourceView, scene: &ID3D11ShaderResourceView, destination: &ID3D11RenderTargetView, camera: &Camera, heat: Option<&crate::yebis_heat::Input>) -> Result<(), String> {
            // CPostEffect+0x2fc: the aspect is set once at Initialize, so a resized frame keeps the made size's
            let camera = &Camera { width: self.width as u32, height: self.height as u32, ..*camera };
            let mut pe = PostEffect::new(&self.filter, &self.video, camera)?;
            let (w, h) = (self.width as f32, self.height as f32);
            let (e0, e1) = (self.effect[0].srv.clone(), self.effect[1].srv.clone());
            let shimmer = heat.and_then(|h| h.shimmer(&pe, self.particles as usize));
            // without chromatic aberration (VR) draws 2 and 6 go and the effect stays in its first surface
            let ca = camera.chromatic_aberration;
            if shimmer.is_some() && pe.dof.is_some() {
                return Err("yebis: the heat shimmer with the depth of field: not ported".into());
            }
            if shimmer.is_some() && !ca {
                return Err("yebis: the heat shimmer without chromatic aberration: not ported".into());
            }
            // with the shimmer, round one's and two's second draws read its copy
            let e0s = if shimmer.is_some() { self.effect[2].srv.clone() } else { e0.clone() };
            let (t0, t1) = (self.tonemap[0].srv.clone(), self.tonemap[1].srv.clone());
            let (black, grey, white, dither) = (Some(self.black.clone()), Some(self.grey.clone()), Some(self.white.clone()), Some(self.dither.clone()));
            let plain = |d: usize| draw_vertices(d, w, h, [0.0; 4], 0.0, 0);
            unsafe {
                // YebisPP::applyPostProcessing: SetEffectSource then SetDepthFactorSource(R32_TYPELESS); a view not in
                // the map is registered with a target or depth view on its texture (CreateRenderTargetOrDepthStencilViewFromShaderResourceView)
                // (0x1404340b0: a render target view when the texture binds as one, else a depth view)
                for view in [scene, depth] {
                    if self.sources.iter().any(|(v, _)| v == view) {
                        continue;
                    }
                    let t = view.GetResource().map_err(|e| format!("yebis: {e}"))?;
                    let mut td = D3D11_TEXTURE2D_DESC::default();
                    err("GetResource", windows::core::Interface::cast::<ID3D11Texture2D>(&t))?.GetDesc(&mut td);
                    let made: windows::core::IUnknown = if td.BindFlags & D3D11_BIND_RENDER_TARGET.0 as u32 == 0 {
                        let d = D3D11_DEPTH_STENCIL_VIEW_DESC {
                            Format: DXGI_FORMAT_D32_FLOAT,
                            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                            Flags: 0,
                            Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
                        };
                        let mut v = None;
                        err("CreateDepthStencilView", self.device.CreateDepthStencilView(&t, Some(&d), Some(&mut v)))?;
                        v.ok_or("yebis: no view")?.into()
                    } else {
                        let d = D3D11_RENDER_TARGET_VIEW_DESC { Format: DXGI_FORMAT_UNKNOWN, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D, Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } } };
                        let mut v = None;
                        err("CreateRenderTargetView", self.device.CreateRenderTargetView(&t, Some(&d), Some(&mut v)))?;
                        v.ok_or("yebis: no view")?.into()
                    };
                    self.sources.push((view.clone(), made));
                }
                // CTextureUtil::Resample 0x1403f6710: its second texture (the scene) gets filter 1 (point) when it is the
                // destination's size, else filter 0 (MIN_MAG_LINEAR_MIP_POINT), keeping the texture's border address
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                if let Some(t) = scene.GetResource().ok().and_then(|r| windows::core::Interface::cast::<ID3D11Texture2D>(&r).ok()) {
                    t.GetDesc(&mut desc);
                }
                let ss = if (desc.Width as i32, desc.Height as i32) == (self.width, self.height) { SB } else { SLB };
                let (depth, scene) = (Some(depth.clone()), Some(scene.clone()));
                // BeginPostEffectScene / EndPostEffectScene: round one
                self.current = 0;
                self.effect_with_depth();
                self.full_viewport();
                self.full_viewport();
                self.reset();
                self.reset();
                self.effect_of_eight();
                self.effect_with_depth();
                self.full_viewport();
                self.effect_of_eight();
                let u = self.cbuffer.draw(0, &pe, &self.filmic, &self.used);
                self.draw(0, &u, &[SB, ss], &[depth.clone(), scene.clone()], &plain(0))?;
                if let Some(s) = &shimmer {
                    self.shimmer(&pe, s, &e0)?;
                }
                if let Some(d) = &pe.dof {
                    self.dof(&pe, d)?;
                }
                // with the depth of field the aberration reads its composite (e1) and draws into e0
                let e0s = if pe.dof.is_some() { e1.clone() } else { e0s };
                if ca {
                    self.current ^= 1;
                    self.effect_with_depth();
                    self.full_viewport();
                    let u = self.cbuffer.draw(1, &pe, &self.filmic, &self.used);
                    self.draw(1, &u, &[SL], std::slice::from_ref(&e0s), &plain(1))?;
                }
                let src = self.effect[self.current].srv.clone();
                let glare = match pe.glare.clone() {
                    Some(g) => {
                        self.reset();
                        // (without chromatic aberration (VR) the effect surface is not bound again, Task Y5d)
                        if ca {
                            self.effect_of_eight();
                        }
                        // GenerateGlare (EndPostEffectScene_InternalProcess 0x1403b5f40) runs before ApplyEffects_UpdateAutoExposure:
                        // CPostEffect+0x6f4 as the camera set it
                        // (without auto exposure: the camera's own, Task Y5d)
                        let exposure = if pe.auto_exposure.is_some() { self.auto_exposure.exposure(camera.new_exposure) } else { pe.exposure };
                        Some(self.bloom(&pe, &g, exposure, src.clone())?)
                    }
                    None => None,
                };
                // UpdateAutoExposure: last analysis's result if the GPU has it, then this frame's step
                let readback = if self.auto_exposure.waiting() { self.read_back() } else { None };
                let step = pe.auto_exposure.map(|ae| {
                    let (e, step) = self.auto_exposure.frame(readback, ae, camera.dt, camera.new_exposure, pe.tonemap == 2);
                    pe.exposure = e;
                    step
                });
                match step {
                    Some(Luminance::Row(row)) => {
                        // (the game's VR log: without chromatic aberration the effect surface is not bound again;
                        // with the glare it was bound before the glare's draws)
                        if ca && glare.is_none() {
                            self.effect_of_eight();
                        }
                        self.target_only(&self.luminance.rtv.clone());
                        let (lw, lh) = (self.luminance_size.0 as f32, self.luminance_size.1 as f32);
                        self.viewport(0.0, 0.0, lw, lh);
                        self.reset();
                        self.viewport(0.0, row_y(row, lh).trunc(), lw, (lh * 0.2).trunc());
                        let n = self.noise.next(false);
                        let t2 = if pe.tonemap == 1 { T_LINEARSAT[0] } else { 2 };
                        let u = self.cbuffer.draw(t2, &pe, &self.filmic, &self.used);
                        self.draw(t2, &u, &[SL, SP, SP], &[src.clone(), black.clone(), grey.clone()], &draw_vertices(2, lw, lh, n, self.vignette, row))?;
                    }
                    Some(Luminance::Analyze) => {
                        // CTextureLuminance::Analyze: the luminance target down to 1x1, then the CPU copy
                        if ca && glare.is_none() {
                            self.effect_of_eight();
                        }
                        let mut src = self.luminance.srv.clone();
                        let last = self.analysis.len() - 1;
                        for k in 0..=last {
                            let (aw, ah) = self.analysis[k].2;
                            let from = if k == 0 { self.luminance_size } else { self.analysis[k - 1].2 };
                            self.cbuffer.taps = crate::yebis_cb::box_taps(from, (aw, ah));
                            let t = match self.cbuffer.taps.len() {
                                1 => 8,
                                2 => 9,
                                5 => 10,
                                n => return Err(format!("yebis: no analysis program with {n} taps")),
                            };
                            self.target_only(&self.analysis[k].0.rtv.clone());
                            self.viewport(0.0, 0.0, aw as f32, ah as f32);
                            let u = self.cbuffer.draw(t, &pe, &self.filmic, &self.used);
                            self.draw(t, &u, &[SLB], std::slice::from_ref(&src), &draw_vertices(t, aw as f32, ah as f32, [0.0; 4], 0.0, 0))?;
                            src = self.analysis[k].0.srv.clone();
                        }
                        self.context.CopyResource(&self.readback, &self.analysis[last].1);
                    }
                    _ => {}
                }
                self.reset();
                if step.is_none() && glare.is_none() {
                    // with auto exposure off YEBIS binds the effect surface after the reset (Task Y3, ae_off filter),
                    // not again after the glare (Task Y5d)
                    self.effect_of_eight();
                }
                self.target_only(&self.tonemap[0].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                let n = self.noise.next(true);
                // DrawRectGPU_TonemapHDR 0x1403f1b10 drops the vignette program when its rectangle is zero
                let (t3, t6) = match (self.vignette == 0.0, pe.tonemap == 1, pe.gamma != 0.0 && pe.gamma != 1.0) {
                    (false, false, true) => (3, 6),
                    (true, false, true) => (11, 12),
                    (false, true, true) => (T_LINEARSAT[1], 6),
                    (true, true, true) => (T_LINEARSAT[2], 12),
                    (v, l, false) => (T_NO_GAMMA[v as usize + 2 * l as usize], if v { 12 } else { 6 }),
                };
                let u = self.cbuffer.draw(t3, &pe, &self.filmic, &self.used);
                // DrawRectGPU_TonemapHDR 0x1403f1b10 ends by giving its first texture its second's saved mode
                let restored = self.mode(glare.as_ref().map_or(&black, |g| &g.0));
                if let Some((bloom, bw, bh)) = &glare {
                    let v = tonemap_vertices(w, h, noise_rect(n, 1.0, w / h), self.vignette, src_rect(*bw, *bh));
                    self.draw(t3, &u, &[SP, SL, SP, SW], &[src.clone(), bloom.clone(), grey.clone(), dither.clone()], &v)?;
                } else {
                    self.draw(t3, &u, &[SP, SP, SP, SW], &[src.clone(), black.clone(), grey.clone(), dither.clone()], &draw_vertices(3, w, h, n, self.vignette, 0))?;
                }
                if let Some(v) = &src {
                    self.modes.insert(self.mode_key(v), restored);
                }
                self.frame_clamped[self.current] = true; // the tone map read the glare's source (e1, or e0 without chromatic aberration or with the depth of field)
                if shimmer.is_some() {
                    // in a shimmer frame the tonemap's source is the texture the shimmer copies from, and
                    // DrawRectGPU_TonemapHDR 0x1403f1b10 ends by giving it black's mode (point, clamp):
                    // it restores its second texture's saved mode into the first (RENDER_ORACLE_SAMPLER_LOG)
                    self.copy_sampler = SP;
                }
                // round two
                self.reset();
                self.current = 0;
                self.effect_with_depth();
                self.full_viewport();
                self.effect_of_eight();
                let u = self.cbuffer.draw(4, &pe, &self.filmic, &self.used);
                self.draw(4, &u, &[SB, ss], &[depth, scene], &plain(4))?;
                if let Some(s) = &shimmer {
                    self.shimmer(&pe, s, &e0)?;
                }
                if let Some(d) = &pe.dof {
                    self.dof(&pe, d)?;
                }
                if ca {
                    self.current ^= 1;
                    self.effect_with_depth();
                    self.full_viewport();
                    let u = self.cbuffer.draw(5, &pe, &self.filmic, &self.used);
                    self.draw(5, &u, &[SL], &[e0s], &plain(5))?;
                }
                self.reset();
                if ca {
                    self.effect_of_eight();
                }
                self.target_only(&self.tonemap[1].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                let u = self.cbuffer.draw(t6, &pe, &self.filmic, &self.used);
                self.draw(t6, &u, &[SP, SP, SP, SW], &[src, black.clone(), grey, dither], &draw_vertices(6, w, h, n, self.vignette, 0))?;
                // the composite into the destination
                self.target_only(destination);
                self.full_viewport();
                self.full_viewport();
                let u = self.cbuffer.draw(7, &pe, &self.filmic, &self.used);
                match glare {
                    Some((bloom, _, _)) => self.draw(7, &u, &[SP, SP, SP, SP, SL, SP], &[t0, black, white.clone(), t1, bloom, white], &plain(7))?,
                    None => self.draw(7, &u, &[SP; 6], &[t0, black.clone(), white.clone(), t1, black, white], &plain(7))?,
                }
                // (the shader reset after this is the caller's resetYebisStates)
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dof_levels_follow_the_game() {
        // the game's DOF=1..4 logs at 1280x720, PP_QUALITY 3 (Task Y6)
        let l = |q| dof_levels(q, 768, 1280, 720).iter().map(|l| (l.size, l.passes, l.doubled)).collect::<Vec<_>>();
        assert_eq!(l(1), [((192, 108), 1, false), ((192, 108), 3, false)]);
        assert_eq!(l(2), [((288, 162), 1, false), ((288, 162), 3, false)]);
        assert_eq!(l(3), [((384, 216), 1, false), ((192, 108), 2, false), ((192, 108), 3, false)]);
        assert_eq!(l(4), [((384, 216), 1, false), ((384, 216), 3, false), ((192, 108), 3, true)]);
        assert_eq!(dof_levels(3, 768, 1280, 720)[0].l30, 3.0 / 216.0);
    }

    #[test]
    fn luminance_sizes_follow_the_game() {
        let chain = vec![(64, 32), (32, 16), (16, 8), (8, 4), (4, 2), (1, 1)];
        assert_eq!(luminance_sizes(1280, 720), ((128, 64), chain.clone()));
        assert_eq!(luminance_sizes(1024, 576), ((128, 64), chain));
        // triple screens: 128 wide at the frame's aspect (the game's tri log, Task Y3)
        assert_eq!(luminance_sizes(5760, 1080).0, (128, 24));
    }

    /// The game's f10 remake logs (Task Y3d-2): rszfx at 1280x720, trirsz at 3840x720.
    #[test]
    fn glare_sizes_follow_the_game() {
        // the game's glare targets at 1024x576, QUALITY=3 (taskY5 resize logs) and 1280x720 (the b3 frame)
        let b = base_width(3, 1024);
        assert_eq!([0, 1, 3, 5].map(|q| glare_size(b, q, 1024, 576)), [(77, 43), (93, 52), (154, 87), (307, 173)]);
        let own = |q| -> Vec<_> { glare_levels(b, q, 1024, 576).iter().filter(|l| l.own).map(|l| (l.size, l.up)).collect() };
        assert_eq!(glare_levels(b, 0, 1024, 576).len(), 2);
        assert_eq!(own(1), vec![((47, 26), Some((94, 52)))]);
        assert_eq!(own(3), vec![((77, 44), None), ((39, 22), Some((78, 44))), ((20, 11), Some((40, 22)))]);
        assert_eq!(own(5).len(), 5);
        assert_eq!(own(5)[4], ((10, 6), Some((20, 12))));
        assert_eq!(glare_pool(b, 3, 1024, 576), vec![(512, 288), (256, 144)]);
        assert_eq!(glare_pool(b, 5, 1024, 576), vec![(512, 288)]);
        let l = glare_levels(base_width(3, 1280), 3, 1280, 720);
        assert_eq!(l.iter().map(|l| (l.size, l.up, l.step)).collect::<Vec<_>>()[2..], [((96, 54), None, 2.0), ((48, 27), Some((96, 54)), 2.0), ((24, 14), Some((48, 28)), 2.0)]);
        assert_eq!((l[0].step, l[1].step), (1.0, 2.0));
        assert_eq!((base_width(0, 1280), base_width(0, 600), base_width(1, 1280)), (640, 600, 256));
        assert_eq!(glare_size(base_width(0, 1280), 0, 1280, 720), (80, 45));
        assert_eq!(glare_size(base_width(0, 3840), 0, 3840, 720), (427, 16));
        assert_eq!(glare_size(base_width(0, 5760), 0, 5760, 1080), (427, 16));
        assert_eq!(glare_pool(base_width(0, 1280), 0, 1280, 720), vec![(640, 360), (320, 180), (160, 90)]);
        assert_eq!(glare_pool(base_width(0, 3840), 0, 3840, 720), vec![(1920, 360), (960, 180), (480, 90)]);
        assert_eq!(half_pool(base_width(0, 1280), 1280, 720), vec![(160, 90), (320, 180), (640, 360)]);
        assert_eq!(half_pool(base_width(0, 3840), 3840, 720), vec![(160, 30), (320, 60), (1920, 360), (960, 180), (480, 90), (240, 45)]);
        assert_eq!(half_pool(base_width(0, 5760), 5760, 1080), vec![(160, 30), (320, 60), (2880, 540), (1440, 270), (720, 135), (360, 68), (180, 34)]);
        assert_eq!(half_pairs(base_width(0, 1280), 1280, 720), vec![(160, 90), (640, 360), (320, 180), (320, 180)]);
        assert_eq!(half_pairs(base_width(0, 3840), 3840, 720), vec![(160, 30), (1920, 360), (960, 180), (480, 90), (240, 45), (320, 60)]);
        // 160 > 128 and > 1024/8: the game's second level stays 160x120
        assert_eq!(half_pool(base_width(0, 1024), 1024, 770), vec![(512, 385), (256, 193)]);
        assert_eq!(half_pairs(base_width(0, 1024), 1024, 770), vec![(160, 120), (512, 385), (256, 193), (160, 120)]);
    }

    /// The TrimCircle's hash after its upload in the game's logs (the gpulog texture hash: level 0, then the rows).
    #[test]
    fn trim_circle_equals_the_games() {
        let mut h = crate::gpulog::Fnv::new();
        h.u32(0);
        h.bytes(&crate::yebis_tex::trim_circle());
        assert_eq!(h.0, 0x5ab5_db75_01af_4869);
    }

    /// The payloads of the 8 vertex uploads of the r23_09 game frame (the second YEBIS frame).
    #[test]
    fn vertices_equal_the_games() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/scratch/taskY2b/ac_yebis09.txt");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("no game log: skipped");
            return;
        };
        let game: Vec<&str> = text.lines().filter(|l| l.starts_with("Unmap B192")).map(|l| l.rsplit(' ').next().unwrap()).collect();
        assert_eq!(game.len(), 8);
        let (w, h) = (1280.0, 720.0);
        let filter = IniReader::from_text(std::path::Path::new("f.ini"), "[VIGNETTING]\nSTRENGTH=0.035\nFOV_DEPENDENCE=0\n");
        let a = vignette(&filter).unwrap();
        let mut buffers = vec![[0u8; RING_BYTES as usize]; RING_COUNT];
        let mut ring = Ring::default();
        let mut noise = Noise::default();
        let mut ours = Vec::new();
        for frame in 0..2 {
            let first = frame == 0;
            let mut n = [0.0; 4];
            for d in 0..8 {
                if d == 2 {
                    n = noise.next(false);
                } else if d == 3 {
                    n = noise.next(true);
                }
                // the luminance draw is in its 128x64 target
                let v = if d == 2 { draw_vertices(d, 128.0, 64.0, n, a, 1) } else { draw_vertices(d, w, h, n, a, 1) };
                let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
                let (i, off, discard) = ring.alloc(bytes.len() as u32);
                if discard {
                    buffers[i] = [0; RING_BYTES as usize];
                }
                buffers[i][off as usize..off as usize + bytes.len()].copy_from_slice(&bytes);
                if !first {
                    ours.push(buffers[i].iter().map(|b| format!("{b:02x}")).collect::<String>());
                }
            }
        }
        for d in 0..8 {
            assert_eq!(ours[d], game[d], "draw {}", d + 1);
        }
    }
}
