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
//! Shortcut: the first frame has no auto exposure update (as in the game), every later frame
//! draws the luminance row 1 of the 128x64 target; Y3 ports the adaptation.

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

/// The vertices of draw `d` (0-based) of a `w` x `h` frame. `noise` is the draw's noise (draws
/// 3, 4 and 7); `vignette` is the half-height of the vignette rectangle.
pub fn draw_vertices(d: usize, w: f32, h: f32, noise: [f32; 4], vignette: f32) -> Vec<f32> {
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
        // TonemapToSurface 0x14041d7f0: row 1 of five of the 128x64 luminance target
        2 => {
            let (lw, lh) = (128.0f32, 64.0f32);
            let row = 1.0f32;
            rect_vertices([lw, 12.0], [0.0, lh * (row * 0.2) - 12.0, lw, lh * 0.2], &[[0.0, 0.2, 1.0, 0.4], zero, zero, noise_rect(0.0, 0.0), zero])
        }
        // DrawRectGPU_TonemapHDR 0x1403f1b10: vignette, noise and the 4x4 dither
        3 | 6 => {
            let aspect = w / h;
            let a = vignette;
            rect_vertices([w, h], [0.0, 0.0, w, h], &[full, zero, [-a * aspect, -a, a * aspect, a], noise_rect(1.0, aspect), [0.0, 0.0, w / 4.0, h / 4.0]])
        }
        7 => rect_vertices([w, h], [0.0, 0.0, w, h], &[full, zero, zero, zero]),
        _ => rect_vertices([w, h], [0.0, 0.0, w, h], &[full]),
    }
}

/// The shared vertex buffer ring's bookkeeping: which buffer and offset a draw of `size` bytes
/// lands in, and whether its map is a real discard.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ring {
    pub index: usize,
    pub cur: u32,
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
    use crate::yebis_cb::{filmic_table, used_all, Camera, Cbuffer, PostEffect, TECHNIQUES};
    use std::path::Path;
    use windows::core::PCSTR;
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;

    // samplers: border, linear, point, wrap
    const SB: usize = 0;
    const SL: usize = 1;
    const SP: usize = 2;
    const SW: usize = 3;

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
        context: ID3D11DeviceContext,
        vs: Vec<ID3D11VertexShader>,
        ps: Vec<ID3D11PixelShader>,
        layouts: Vec<ID3D11InputLayout>,
        strides: Vec<u32>,
        used: [Vec<(usize, usize)>; 8],
        filmic: [f32; 21],
        cb: ID3D11Buffer,
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
        /// One per round: the composite reads the first round's colour and the second's alpha.
        tonemap: [Target; 2],
        samplers: [ID3D11SamplerState; 4],
        raster: ID3D11RasterizerState,
        depth: ID3D11DepthStencilState,
        blend: ID3D11BlendState,
        cache: Cache,
        noise: Noise,
        frames: u64,
        filter: IniReader,
        video: IniReader,
        vignette: f32,
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
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
        };
        let mut v = None;
        err("CreateShaderResourceView", device.CreateShaderResourceView(t, Some(&desc), Some(&mut v)))?;
        v.ok_or_else(|| "yebis: no view".into())
    }

    unsafe fn target(device: &ID3D11Device, w: u32, h: u32, typeless: DXGI_FORMAT, format: DXGI_FORMAT, with_srv: bool) -> Result<Target, String> {
        let t = texture(device, w, h, typeless, D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0, None)?;
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } },
        };
        let mut rtv = None;
        err("CreateRenderTargetView", device.CreateRenderTargetView(&t, Some(&desc), Some(&mut rtv)))?;
        Ok(Target { rtv: rtv.ok_or("yebis: no view")?, srv: if with_srv { Some(srv(device, &t, format)?) } else { None } })
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
                let (mut vs, mut ps, mut layouts, mut strides) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                for name in TECHNIQUES {
                    let (v, p) = tfxo.technique(name).ok_or_else(|| format!("yebis: no technique {name}"))?;
                    let mut s = None;
                    err("CreateVertexShader", device.CreateVertexShader(v, None, Some(&mut s)))?;
                    vs.push(s.ok_or("yebis: no shader")?);
                    let mut s = None;
                    err("CreatePixelShader", device.CreatePixelShader(p, None, Some(&mut s)))?;
                    ps.push(s.ok_or("yebis: no shader")?);
                    // the input layout from the program's input signature: float2s, packed
                    let inputs = rustyac_dxbc::parse(v)?.inputs;
                    let names: Vec<std::ffi::CString> = inputs.iter().map(|e| std::ffi::CString::new(e.name.as_str()).unwrap_or_default()).collect();
                    let elements: Vec<D3D11_INPUT_ELEMENT_DESC> = inputs
                        .iter()
                        .zip(&names)
                        .enumerate()
                        .map(|(i, (e, n))| D3D11_INPUT_ELEMENT_DESC {
                            SemanticName: PCSTR(n.as_ptr().cast()),
                            SemanticIndex: e.index,
                            Format: [DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT][(8 - e.mask.leading_zeros()) as usize],
                            InputSlot: 0,
                            AlignedByteOffset: 8 * i as u32,
                            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                            InstanceDataStepRate: 0,
                        })
                        .collect();
                    let mut l = None;
                    err("CreateInputLayout", device.CreateInputLayout(&elements, v, Some(&mut l)))?;
                    layouts.push(l.ok_or("yebis: no layout")?);
                    strides.push(8 * inputs.len() as u32);
                }
                let tex1 = |c: u8| -> Result<ID3D11ShaderResourceView, String> {
                    let t = texture(device, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE.0, Some(&[c; 4]))?;
                    srv(device, &t, DXGI_FORMAT_R8G8B8A8_UNORM)
                };
                let dither = texture(device, 4, 4, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE.0, Some(&dither_texels()))?;
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
                let sampler = |filter: D3D11_FILTER, u: D3D11_TEXTURE_ADDRESS_MODE| -> Result<ID3D11SamplerState, String> {
                    let desc = D3D11_SAMPLER_DESC {
                        Filter: filter,
                        AddressU: u,
                        AddressV: u,
                        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                        MipLODBias: 0.0,
                        MaxAnisotropy: 0,
                        ComparisonFunc: D3D11_COMPARISON_NEVER,
                        BorderColor: [0.0; 4],
                        MinLOD: 0.0,
                        MaxLOD: 0.0,
                    };
                    let mut s = None;
                    err("CreateSamplerState", device.CreateSamplerState(&desc, Some(&mut s)))?;
                    s.ok_or_else(|| "yebis: no sampler".into())
                };
                let samplers = [
                    sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_BORDER)?,
                    sampler(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP)?,
                    sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP)?,
                    sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_WRAP)?,
                ];
                let mut raster = None;
                let rd = D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    FrontCounterClockwise: true.into(),
                    DepthBias: 0,
                    DepthBiasClamp: 0.0,
                    SlopeScaledDepthBias: 0.0,
                    DepthClipEnable: false.into(),
                    ScissorEnable: false.into(),
                    MultisampleEnable: true.into(),
                    AntialiasedLineEnable: false.into(),
                };
                err("CreateRasterizerState", device.CreateRasterizerState(&rd, Some(&mut raster)))?;
                let keep = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_ALWAYS };
                let dd = D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: false.into(),
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                    DepthFunc: D3D11_COMPARISON_LESS,
                    StencilEnable: false.into(),
                    StencilReadMask: 0xff,
                    StencilWriteMask: 0xff,
                    FrontFace: keep,
                    BackFace: keep,
                };
                let mut depth = None;
                err("CreateDepthStencilState", device.CreateDepthStencilState(&dd, Some(&mut depth)))?;
                let rt = D3D11_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: false.into(),
                    SrcBlend: D3D11_BLEND_ONE,
                    DestBlend: D3D11_BLEND_ZERO,
                    BlendOp: D3D11_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D11_BLEND_ONE,
                    DestBlendAlpha: D3D11_BLEND_ZERO,
                    BlendOpAlpha: D3D11_BLEND_OP_ADD,
                    RenderTargetWriteMask: 0xf,
                };
                let bd = D3D11_BLEND_DESC { AlphaToCoverageEnable: false.into(), IndependentBlendEnable: false.into(), RenderTarget: [rt; 8] };
                let mut blend = None;
                err("CreateBlendState", device.CreateBlendState(&bd, Some(&mut blend)))?;
                let mut ring = Vec::with_capacity(RING_COUNT);
                for _ in 0..RING_COUNT {
                    ring.push(buffer(device, RING_BYTES, D3D11_BIND_VERTEX_BUFFER.0, true, None)?);
                }
                let indices: [u16; 4] = [0, 1, 2, 3];
                let ib_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                Ok(Yebis {
                    context: context.clone(),
                    vs,
                    ps,
                    layouts,
                    strides,
                    used,
                    filmic,
                    cb: buffer(device, 4096, D3D11_BIND_CONSTANT_BUFFER.0, true, None)?,
                    cbuffer: Cbuffer::default(),
                    ring,
                    ring_at: Ring::default(),
                    ib: buffer(device, 8, D3D11_BIND_INDEX_BUFFER.0, false, Some(&ib_bytes))?,
                    black: tex1(0)?,
                    grey: tex1(0x7f)?,
                    white: tex1(0xff)?,
                    dither: srv(device, &dither, DXGI_FORMAT_R8G8B8A8_UNORM)?,
                    effect: [target(device, w, h, DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_FLOAT, true)?, target(device, w, h, DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_FLOAT, true)?],
                    current: 0,
                    effect_depth: dsv.ok_or("yebis: no view")?,
                    luminance: target(device, 128, 64, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, false)?,
                    tonemap: [target(device, w, h, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?, target(device, w, h, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, true)?],
                    samplers,
                    raster: raster.ok_or("yebis: no state")?,
                    depth: depth.ok_or("yebis: no state")?,
                    blend: blend.ok_or("yebis: no state")?,
                    cache: Cache::default(),
                    noise: Noise::default(),
                    frames: 0,
                    filter,
                    video,
                    vignette,
                    width,
                    height,
                })
            }
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
            c.GSSetShader(None, None);
            c.HSSetShader(None, None);
            c.DSSetShader(None, None);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            err("Map", c.Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)))?;
            c.VSSetShader(&self.vs[d], None);
            c.PSSetShader(&self.ps[d], None);
            for (slot, &s) in samplers.iter().enumerate() {
                if self.cache.samplers[slot] != Some(s) {
                    c.PSSetSamplers(slot as u32, Some(&[Some(self.samplers[s].clone())]));
                    self.cache.samplers[slot] = Some(s);
                }
            }
            c.PSSetShaderResources(0, Some(srvs));
            std::ptr::copy_nonoverlapping(upload.as_ptr(), m.pData.cast::<u8>(), 4096);
            c.Unmap(&self.cb, 0);
            c.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            let bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_le_bytes()).collect();
            let (index, offset, discard) = self.ring_at.alloc(bytes.len() as u32);
            let vb = self.ring[index].clone();
            let kind = if discard { D3D11_MAP_WRITE_DISCARD } else { D3D11_MAP_WRITE_NO_OVERWRITE };
            err("Map", c.Map(&vb, 0, kind, 0, Some(&mut m)))?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), m.pData.cast::<u8>().add(offset as usize), bytes.len());
            c.Unmap(&vb, 0);
            c.IASetInputLayout(&self.layouts[d]);
            let stride = self.strides[d];
            c.IASetVertexBuffers(0, 1, Some(&Some(vb)), Some(&stride), Some(&offset));
            c.IASetIndexBuffer(&self.ib, DXGI_FORMAT_R16_UINT, 0);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            c.DrawIndexed(3, 0, 0);
            let none: Vec<Option<ID3D11ShaderResourceView>> = vec![None; srvs.len()];
            c.PSSetShaderResources(0, Some(&none));
            Ok(())
        }

        /// `CPostEffect::ApplyEffects` for AC's chain: `depth` and `scene` are the game's
        /// (resolved) depth and colour, `destination` the back buffer or the FXAA target.
        pub fn render(&mut self, depth: &ID3D11ShaderResourceView, scene: &ID3D11ShaderResourceView, destination: &ID3D11RenderTargetView, camera: &Camera) -> Result<(), String> {
            let pe = PostEffect::new(&self.filter, &self.video, camera)?;
            let uploads = self.cbuffer.frame(&pe, &self.filmic, &self.used);
            let first = self.frames == 0;
            self.frames += 1;
            let (w, h) = (self.width as f32, self.height as f32);
            let (e0, e1) = (self.effect[0].srv.clone(), self.effect[1].srv.clone());
            let (t0, t1) = (self.tonemap[0].srv.clone(), self.tonemap[1].srv.clone());
            let (black, grey, white, dither) = (Some(self.black.clone()), Some(self.grey.clone()), Some(self.white.clone()), Some(self.dither.clone()));
            let (depth, scene) = (Some(depth.clone()), Some(scene.clone()));
            let plain = |d: usize| draw_vertices(d, w, h, [0.0; 4], 0.0);
            unsafe {
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
                self.draw(0, &uploads[0], &[SB, SB], &[depth.clone(), scene.clone()], &plain(0))?;
                self.current = 1;
                self.effect_with_depth();
                self.full_viewport();
                self.draw(1, &uploads[1], &[SL], std::slice::from_ref(&e0), &plain(1))?;
                if !first {
                    // UpdateAutoExposure: the luminance row
                    self.effect_of_eight();
                    self.target_only(&self.luminance.rtv.clone());
                    self.viewport(0.0, 0.0, 128.0, 64.0);
                    self.reset();
                    self.viewport(0.0, 12.0, 128.0, 12.0);
                    let n = self.noise.next(true);
                    self.draw(2, &uploads[2], &[SL, SP, SP], &[e1.clone(), black.clone(), grey.clone()], &draw_vertices(2, w, h, n, self.vignette))?;
                }
                self.reset();
                self.target_only(&self.tonemap[0].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                let n = self.noise.next(!first);
                let tonemap_vertices = draw_vertices(3, w, h, n, self.vignette);
                self.draw(3, &uploads[3], &[SP, SP, SP, SW], &[e1.clone(), black.clone(), grey.clone(), dither.clone()], &tonemap_vertices)?;
                // round two
                self.reset();
                self.current = 0;
                self.effect_with_depth();
                self.full_viewport();
                self.effect_of_eight();
                self.draw(4, &uploads[4], &[SB, SB], &[depth, scene], &plain(4))?;
                self.current = 1;
                self.effect_with_depth();
                self.full_viewport();
                self.draw(5, &uploads[5], &[SL], &[e0], &plain(5))?;
                self.reset();
                self.effect_of_eight();
                self.target_only(&self.tonemap[1].rtv.clone());
                self.full_viewport();
                self.full_viewport();
                self.draw(6, &uploads[6], &[SP, SP, SP, SW], &[e1, black.clone(), grey, dither], &draw_vertices(6, w, h, n, self.vignette))?;
                // the composite into the destination
                self.target_only(destination);
                self.full_viewport();
                self.full_viewport();
                self.draw(7, &uploads[7], &[SP; 6], &[t0, black.clone(), white.clone(), t1, black, white], &plain(7))?;
                // (the shader reset after this is the caller's resetYebisStates)
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                if d == 2 && first {
                    continue;
                }
                if d == 2 || d == 3 {
                    n = noise.next(!first);
                }
                let v = draw_vertices(d, w, h, n, a);
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
