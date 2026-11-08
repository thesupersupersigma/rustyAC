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

/// The vertices of draw `d` (0-based) of a `w` x `h` frame. `noise` is the draw's noise (draws
/// 3, 4 and 7); `vignette` is the half-height of the vignette rectangle.
pub fn draw_vertices(d: usize, w: f32, h: f32, noise: [f32; 4], vignette: f32, row: u32) -> Vec<f32> {
    let full = [0.0, 0.0, 1.0, 1.0];
    let zero = [0.0; 4];
    // the noise offsets of the 1x1 noise texture; the luminance draw passes 0 for both scales,
    // which makes NaN and +inf (x86's default NaN: black_box keeps the compiler from folding it)
    let noise_rect = |p32: f32, p33: f32| {
        let (p32, p33) = (std::hint::black_box(p32), std::hint::black_box(p33));
        let [n0, n1, s2, s3] = noise;
        [n0, n1, n0 + s2 * (((1024.0 / 1.0) * 2.0 * p33) / p32), n1 + s3 * (((1024.0 / 1.0) * 2.0) / p32)]
    };
    match d {
        // TonemapToSurface 0x14041d7f0: one of the five rows of the `w` x `h` luminance target
        2 => {
            let y = row_y(row, h);
            let v = row as f32 * 0.2;
            rect_vertices([w, (h * 0.2).trunc()], [0.0, y - y.trunc(), w, h * 0.2], &[[0.0, v, 1.0, v + 0.2], zero, zero, noise_rect(0.0, 0.0), zero])
        }
        // DrawRectGPU_TonemapHDR 0x1403f1b10: vignette, noise and the 4x4 dither
        3 | 6 => {
            let aspect = w / h;
            let a = vignette;
            // no vignette: the rectangle is left zero (+0, not -0)
            let rect = if a == 0.0 { zero } else { [-a * aspect, -a, a * aspect, a] };
            rect_vertices([w, h], [0.0, 0.0, w, h], &[full, zero, rect, noise_rect(1.0, aspect), [0.0, 0.0, w / 4.0, h / 4.0]])
        }
        7 => rect_vertices([w, h], [0.0, 0.0, w, h], &[full, zero, zero, zero]),
        _ => rect_vertices([w, h], [0.0, 0.0, w, h], &[full]),
    }
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
    use crate::yebis_cb::{filmic_table, used_all, AutoExposure, Camera, Cbuffer, Luminance, PostEffect, TECHNIQUES};
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
        /// Each technique's input elements; a layout is shared by every program with the same ones.
        inputs: Vec<Vec<(std::ffi::CString, u32, DXGI_FORMAT)>>,
        layouts: Vec<Option<ID3D11InputLayout>>,
        strides: Vec<u32>,
        used: [Vec<(usize, usize)>; 13],
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
        /// Two surfaces, as YEBIS's render-texture array: a draw never reads the one it writes.
        effect: [Target; 2],
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
        noise: Noise,
        root: std::path::PathBuf,
        filter: IniReader,
        video: IniReader,
        /// The vignette's half-height, from the filter's `[VIGNETTING] STRENGTH` at init.
        pub vignette: f32,
        width: i32,
        height: i32,
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
        let t = texture(device, w, h, typeless, D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0, None)?;
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } },
        };
        let mut rtv = None;
        err("CreateRenderTargetView", device.CreateRenderTargetView(&t, Some(&desc), Some(&mut rtv)))?;
        let srv = if with_srv { Some(srv(device, &t, format)?) } else { None };
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
        /// `[POST_PROCESS] FILTER`; `root` holds the player's acs.exe.
        pub fn new(device: &ID3D11Device, context: &ID3D11DeviceContext, root: &Path, filter: IniReader, video: IniReader, width: i32, height: i32) -> Result<Yebis, String> {
            let exe = std::fs::read(root.join("acs.exe")).map_err(|e| format!("yebis: acs.exe: {e}"))?;
            let tfxo = crate::yebis_tfxo::Tfxo::from_exe(&exe)?;
            let filmic = filmic_table(&exe)?;
            let used = used_all(&tfxo)?;
            let vignette = vignette(&filter)?;
            unsafe {
                // every program is made on its first draw (see draw)
                let (mut code, mut inputs, mut strides) = (Vec::new(), Vec::new(), Vec::new());
                for name in TECHNIQUES {
                    let (v, p) = tfxo.technique(name).ok_or_else(|| format!("yebis: no technique {name}"))?;
                    // the input layout from the program's input signature: float2s, packed
                    let elements: Vec<(std::ffi::CString, u32, DXGI_FORMAT)> = rustyac_dxbc::parse(v)?
                        .inputs
                        .iter()
                        .map(|e| {
                            let format = [DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT][(8 - e.mask.leading_zeros()) as usize];
                            (std::ffi::CString::new(e.name.as_str()).unwrap_or_default(), e.index, format)
                        })
                        .collect();
                    strides.push(8 * elements.len() as u32);
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
                let dd = D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: false.into(),
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                    DepthFunc: D3D11_COMPARISON_ALWAYS,
                    StencilEnable: false.into(),
                    StencilReadMask: 0xff,
                    StencilWriteMask: 0xff,
                    FrontFace: keep,
                    BackFace: keep,
                };
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
                let mut bound: [Option<ID3D11RenderTargetView>; 1] = [None];
                let mut bound_depth = None;
                context.OMGetRenderTargets(Some(&mut bound), Some(&mut bound_depth));
                if let Some(rtv) = &bound[0] {
                    let t: ID3D11Texture2D = err("GetResource", rtv.GetResource().and_then(|r| windows::core::Interface::cast(&r)))?;
                    let mut d = D3D11_TEXTURE2D_DESC::default();
                    t.GetDesc(&mut d);
                    context.OMSetRenderTargets(Some(&bound), bound_depth.as_ref());
                    context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: d.Width as f32, Height: d.Height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
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
                kept.push(filled(256, 256, 0, rgba, &grain, 4)?.into());
                let spectrum = crate::yebis_tex::mipmaps(256, 1, &crate::yebis_tex::spectrum(&crate::yebis_tex::Cie::from_exe(&exe)?), true, true);
                kept.push(filled(256, 1, 0, rgba, &spectrum, 4)?.into());
                kept.push(filled(64, 64, 5, DXGI_FORMAT_A8_UNORM, &crate::yebis_tex::gaussian(), 1)?.into());
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
                // vertex rings and index buffers; the heat-shimmer buffers for PFX_INITPARAM's 256 particles, 1 view
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
                const PARTICLES: u32 = 256;
                for _ in 0..3 {
                    kept.push(buffer(device, PARTICLES * 0xb0, D3D11_BIND_VERTEX_BUFFER.0, true, None)?.into());
                }
                let quads: Vec<u8> = (0..PARTICLES as u16).flat_map(|i| [4 * i, 4 * i + 1, 4 * i + 2, 4 * i + 2, 4 * i + 1, 4 * i + 3]).flat_map(|i| i.to_le_bytes()).collect();
                kept.push(buffer(device, PARTICLES * 12, D3D11_BIND_INDEX_BUFFER.0, false, Some(&quads))?.into());
                let (w, h) = (width as u32, height as u32);
                let depth_tex = texture(device, w, h, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL.0, None)?;
                let dsv_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                    Flags: 0,
                    Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
                };
                let mut dsv = None;
                err("CreateDepthStencilView", device.CreateDepthStencilView(&depth_tex, Some(&dsv_desc), Some(&mut dsv)))?;
                let (luminance_size, chain) = super::luminance_sizes(width, height);
                let mut analysis = Vec::new();
                for (aw, ah) in chain {
                    let (t, tex) = target_texture(device, aw, ah, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?;
                    analysis.push((t, tex, (aw, ah)));
                }
                // the CPU copy of the 1x1 result
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
                Ok(Yebis {
                    device: device.clone(),
                    context: context.clone(),
                    vs: vec![None; n],
                    ps: vec![None; n],
                    code,
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
                    effect: [target(device, w, h, DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_FLOAT, true)?, target(device, w, h, DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_FLOAT, true)?],
                    current: 0,
                    effect_depth: dsv.ok_or("yebis: no view")?,
                    luminance: target(device, luminance_size.0, luminance_size.1, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?,
                    luminance_size,
                    analysis,
                    readback: readback.ok_or("yebis: no texture")?,
                    auto_exposure: AutoExposure::default(),
                    tonemap: [target(device, w, h, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?, target(device, w, h, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?],
                    samplers,
                    raster: raster.ok_or("yebis: no state")?,
                    depth: depth.ok_or("yebis: no state")?,
                    blend: blend.ok_or("yebis: no state")?,
                    cache: Cache::default(),
                    noise: Noise::default(),
                    root: root.to_path_buf(),
                    filter,
                    video,
                    vignette,
                    width,
                    height,
                })
            }
        }

        /// `CameraForwardYebis::onResize` 0x140029da0: the game destroys YEBIS and makes it again
        /// at the new size, so every carried-over value (parameter store, exposure) starts over; the noise goes on
        pub fn remade(&self, width: i32, height: i32) -> Result<Yebis, String> {
            let mut yebis = Yebis::new(&self.device, &self.context, &self.root, self.filter.clone(), self.video.clone(), width, height)?;
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

        /// Technique `d`'s input layout: its inputs packed one after another.
        unsafe fn layout(&self, d: usize) -> Result<ID3D11InputLayout, String> {
            let elements: Vec<D3D11_INPUT_ELEMENT_DESC> = self.inputs[d]
                .iter()
                .enumerate()
                .map(|(i, (name, index, format))| D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(name.as_ptr().cast()),
                    SemanticIndex: *index,
                    Format: *format,
                    InputSlot: 0,
                    AlignedByteOffset: 8 * i as u32,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                })
                .collect();
            let mut l = None;
            err("CreateInputLayout", self.device.CreateInputLayout(&elements, &self.code[d].0, Some(&mut l)))?;
            l.ok_or_else(|| "yebis: no layout".into())
        }

        /// One draw: `samplers` are (slot, sampler) and only the changed ones are set.
        unsafe fn draw(&mut self, d: usize, upload: &[u8; 4096], samplers: &[usize], srvs: &[Option<ID3D11ShaderResourceView>], vertices: &[f32]) -> Result<(), String> {
            let c = self.context.clone();
            if !self.cache.blend {
                c.OMSetBlendState(&self.blend, Some(&[0.0; 4]), 0xffff_ffff);
                self.cache.blend = true;
            }
            if !self.cache.raster {
                c.RSSetState(&self.raster);
                self.cache.raster = true;
            }
            let none: [Option<ID3D11ShaderResourceView>; 6] = Default::default();
            c.PSSetShaderResources(0, Some(&none));
            // a program is made on its first draw unless another technique made the same one
            if self.vs[d].is_none() {
                let same = (0..self.code.len()).find(|&j| self.vs[j].is_some() && self.code[j].0 == self.code[d].0);
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
                let same = (0..self.code.len()).find(|&j| self.ps[j].is_some() && self.code[j].1 == self.code[d].1);
                self.ps[d] = match same {
                    Some(j) => self.ps[j].clone(),
                    None => {
                        let mut s = None;
                        err("CreatePixelShader", self.device.CreatePixelShader(&self.code[d].1, None, Some(&mut s)))?;
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
            std::ptr::copy_nonoverlapping(upload.as_ptr(), m.pData.cast::<u8>(), 4096);
            c.Unmap(&cb, 0);
            c.VSSetConstantBuffers(0, Some(&[Some(cb.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(cb)]));
            if self.layouts[d].is_none() {
                let same = (0..self.code.len()).find(|&j| self.layouts[j].is_some() && self.inputs[j] == self.inputs[d]);
                self.layouts[d] = match same {
                    Some(j) => self.layouts[j].clone(),
                    None => Some(self.layout(d)?),
                };
            }
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
            c.DrawIndexed(3, 0, 0);
            let none: Vec<Option<ID3D11ShaderResourceView>> = vec![None; srvs.len()];
            c.PSSetShaderResources(0, Some(&none));
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
        pub fn render(&mut self, depth: &ID3D11ShaderResourceView, scene: &ID3D11ShaderResourceView, destination: &ID3D11RenderTargetView, camera: &Camera) -> Result<(), String> {
            let mut pe = PostEffect::new(&self.filter, &self.video, camera)?;
            let (w, h) = (self.width as f32, self.height as f32);
            let (e0, e1) = (self.effect[0].srv.clone(), self.effect[1].srv.clone());
            let (t0, t1) = (self.tonemap[0].srv.clone(), self.tonemap[1].srv.clone());
            let (black, grey, white, dither) = (Some(self.black.clone()), Some(self.grey.clone()), Some(self.white.clone()), Some(self.dither.clone()));
            let plain = |d: usize| draw_vertices(d, w, h, [0.0; 4], 0.0, 0);
            unsafe {
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
                self.current = 1;
                self.effect_with_depth();
                self.full_viewport();
                let u = self.cbuffer.draw(1, &pe, &self.filmic, &self.used);
                self.draw(1, &u, &[SL], std::slice::from_ref(&e0), &plain(1))?;
                // UpdateAutoExposure: last analysis's result if the GPU has it, then this frame's step
                let readback = if self.auto_exposure.waiting() { self.read_back() } else { None };
                let step = pe.auto_exposure.map(|ae| {
                    let (e, step) = self.auto_exposure.frame(readback, ae, camera.dt);
                    pe.exposure = e;
                    step
                });
                match step {
                    Some(Luminance::Row(row)) => {
                        self.effect_of_eight();
                        self.target_only(&self.luminance.rtv.clone());
                        let (lw, lh) = (self.luminance_size.0 as f32, self.luminance_size.1 as f32);
                        self.viewport(0.0, 0.0, lw, lh);
                        self.reset();
                        self.viewport(0.0, row_y(row, lh).trunc(), lw, (lh * 0.2).trunc());
                        let n = self.noise.next(false);
                        let u = self.cbuffer.draw(2, &pe, &self.filmic, &self.used);
                        self.draw(2, &u, &[SL, SP, SP], &[e1.clone(), black.clone(), grey.clone()], &draw_vertices(2, lw, lh, n, self.vignette, row))?;
                    }
                    Some(Luminance::Analyze) => {
                        // CTextureLuminance::Analyze: the luminance target down to 1x1, then the CPU copy
                        self.effect_of_eight();
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
                if step.is_none() {
                    // with auto exposure off YEBIS binds the effect surface after the reset (Task Y3, ae_off filter)
                    self.effect_of_eight();
                }
                self.target_only(&self.tonemap[0].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                let n = self.noise.next(true);
                // DrawRectGPU_TonemapHDR 0x1403f1b10 drops the vignette program when its rectangle is zero
                let (t3, t6) = if self.vignette == 0.0 { (11, 12) } else { (3, 6) };
                let tonemap_vertices = draw_vertices(3, w, h, n, self.vignette, 0);
                let u = self.cbuffer.draw(t3, &pe, &self.filmic, &self.used);
                self.draw(t3, &u, &[SP, SP, SP, SW], &[e1.clone(), black.clone(), grey.clone(), dither.clone()], &tonemap_vertices)?;
                // round two
                self.reset();
                self.current = 0;
                self.effect_with_depth();
                self.full_viewport();
                self.effect_of_eight();
                let u = self.cbuffer.draw(4, &pe, &self.filmic, &self.used);
                self.draw(4, &u, &[SB, ss], &[depth, scene], &plain(4))?;
                self.current = 1;
                self.effect_with_depth();
                self.full_viewport();
                let u = self.cbuffer.draw(5, &pe, &self.filmic, &self.used);
                self.draw(5, &u, &[SL], &[e0], &plain(5))?;
                self.reset();
                self.effect_of_eight();
                self.target_only(&self.tonemap[1].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                let u = self.cbuffer.draw(t6, &pe, &self.filmic, &self.used);
                self.draw(t6, &u, &[SP, SP, SP, SW], &[e1, black.clone(), grey, dither], &draw_vertices(6, w, h, n, self.vignette, 0))?;
                // the composite into the destination
                self.target_only(destination);
                self.full_viewport();
                self.full_viewport();
                let u = self.cbuffer.draw(7, &pe, &self.filmic, &self.used);
                self.draw(7, &u, &[SP; 6], &[t0, black.clone(), white.clone(), t1, black, white], &plain(7))?;
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
    fn luminance_sizes_follow_the_game() {
        let chain = vec![(64, 32), (32, 16), (16, 8), (8, 4), (4, 2), (1, 1)];
        assert_eq!(luminance_sizes(1280, 720), ((128, 64), chain.clone()));
        assert_eq!(luminance_sizes(1024, 576), ((128, 64), chain));
        // triple screens: 128 wide at the frame's aspect (the game's tri log, Task Y3)
        assert_eq!(luminance_sizes(5760, 1080).0, (128, 24));
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
