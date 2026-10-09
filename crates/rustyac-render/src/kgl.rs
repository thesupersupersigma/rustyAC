// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! kgl: the game's thin layer over one Direct3D 11 device, its immediate context and its swap
//! chain. State objects, render targets, buffers and the calls of the frame, each exactly as
//! `kgl.obj` makes them (addresses of the originals are given per function).

use std::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

/// `KGLVideoSettings`: what kgl keeps of the video settings.
#[derive(Clone, Copy, Debug)]
pub struct KglVideoSettings {
    pub aa_samples: i32,
    pub width: i32,
    pub height: i32,
    pub v_sync: bool,
    pub hdr_post_processing: bool,
    pub triple_buffer: bool,
}

/// Where the device comes from and what it draws into.
#[derive(Clone, Copy, Debug)]
pub struct DeviceOptions {
    /// Microsoft's software rasteriser instead of the graphics card.
    pub warp: bool,
    /// A swap chain on this window (as the game has), or none: then the "screen" is a texture.
    pub window: Option<HWND>,
    /// Route every call through the command log ([`crate::gpulog`]).
    pub log: bool,
}

/// `KGLRenderTarget` 0x30 bytes.
#[derive(Default)]
pub struct KglRenderTarget {
    pub texture: Option<ID3D11Texture2D>,
    pub render_target_view: Option<ID3D11RenderTargetView>,
    pub shader_resource_view: Option<ID3D11ShaderResourceView>,
    pub depth_view: Option<ID3D11DepthStencilView>,
    pub format: DXGI_FORMAT,
    pub width: i32,
    pub height: i32,
    pub samples: i32,
}

/// `KGLCubeMap`: the reflection cube map with its per-face views.
pub struct KglCubeMap {
    pub texture: Option<ID3D11Texture2D>,
    pub srv_cube: Option<ID3D11ShaderResourceView>,
    pub rtv_face: Vec<Option<ID3D11RenderTargetView>>,
    pub depth_texture: Option<ID3D11Texture2D>,
    pub dsv_depth: Option<ID3D11DepthStencilView>,
    pub size: i32,
    pub mips: i32,
}

/// `KGLVertexBuffer`.
pub struct KglVertexBuffer {
    pub buffer: Option<ID3D11Buffer>,
    pub stride: u32,
}

/// `KGLIndexBuffer`.
pub struct KglIndexBuffer {
    pub buffer: Option<ID3D11Buffer>,
}

/// `KGLCBuffer`.
pub struct KglCBuffer {
    pub buffer: Option<ID3D11Buffer>,
    pub size: i32,
}

pub struct Kgl {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub swap_chain: Option<IDXGISwapChain>,
    pub video: KglVideoSettings,
    pub screen_render_target: KglRenderTarget,
    pub screen_depth_target: KglRenderTarget,
    active_render_target_view: Option<ID3D11RenderTargetView>,
    active_depth_stencil_view: Option<ID3D11DepthStencilView>,
    blend_states: [Option<ID3D11BlendState>; 4],
    depth_states: [Option<ID3D11DepthStencilState>; 4],
    cull_states: [Option<ID3D11RasterizerState>; 6],
    /// `currentLayout`: the only redundancy filter inside kgl
    current_layout: *mut c_void,
    pub feature_level: D3D_FEATURE_LEVEL,
}

fn err<T>(what: &str, result: windows::core::Result<T>) -> Result<T, String> {
    result.map_err(|e| format!("{what}: {e}"))
}

impl Kgl {
    /// `kglInit` 0x1400184b0 with `initDX11` 0x14001a590, `createDeviceAndSwapChain`
    /// 0x14001a1d0, `createDepthBuffer` 0x14001acc0, `initBlendStates` 0x14001aeb0 and
    /// `initCullStates` 0x14001b0a0. (The game's throw-away first device and its font start-up
    /// are left out; the display-mode match is left out: the refresh rate stays 60/1.)
    pub fn init(video: KglVideoSettings, options: DeviceOptions) -> Result<Kgl, String> {
        let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
        let driver = if options.warp { D3D_DRIVER_TYPE_WARP } else { D3D_DRIVER_TYPE_HARDWARE };
        let mut device = None;
        let mut context = None;
        let mut swap_chain = None;
        let mut level = D3D_FEATURE_LEVEL_11_0;
        let mut video = video;
        unsafe {
            match options.window {
                Some(window) => {
                    let sd = DXGI_SWAP_CHAIN_DESC {
                        BufferDesc: DXGI_MODE_DESC {
                            Width: video.width as u32,
                            Height: video.height as u32,
                            RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
                        },
                        SampleDesc: DXGI_SAMPLE_DESC { Count: if video.hdr_post_processing { 1 } else { video.aa_samples as u32 }, Quality: 0 },
                        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                        BufferCount: if video.triple_buffer { 2 } else { 1 },
                        OutputWindow: window,
                        Windowed: true.into(),
                        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                        Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
                    };
                    err(
                        "D3D11CreateDeviceAndSwapChain",
                        D3D11CreateDeviceAndSwapChain(
                            None,
                            driver,
                            HMODULE::default(),
                            D3D11_CREATE_DEVICE_SINGLETHREADED,
                            Some(&levels),
                            D3D11_SDK_VERSION,
                            Some(&sd),
                            Some(&mut swap_chain),
                            Some(&mut device),
                            Some(&mut level),
                            Some(&mut context),
                        ),
                    )?;
                }
                None => {
                    err(
                        "D3D11CreateDevice",
                        D3D11CreateDevice(None, driver, HMODULE::default(), D3D11_CREATE_DEVICE_SINGLETHREADED, Some(&levels), D3D11_SDK_VERSION, Some(&mut device), Some(&mut level), Some(&mut context)),
                    )?;
                }
            }
        }
        let device = device.ok_or("no Direct3D 11 device")?;
        let mut context = context.ok_or("no Direct3D 11 context")?;
        if level == D3D_FEATURE_LEVEL_10_0 {
            // "FEATURE LEVEL D3D_FEATURE_LEVEL_10_0, DISABLE MSAA"
            video.aa_samples = 1;
        }
        if options.log {
            unsafe {
                let stand_in = crate::gpulog::install(device.as_raw(), context.as_raw())?;
                // the stand-in passes AddRef / Release on to the real context: it gets one
                // reference of its own, and the real context keeps the one it has
                let real = context.clone();
                context = ID3D11DeviceContext::from_raw(stand_in);
                std::mem::forget(real);
            }
        }
        let mut kgl = Kgl {
            device,
            context,
            swap_chain,
            video,
            screen_render_target: KglRenderTarget::default(),
            screen_depth_target: KglRenderTarget::default(),
            active_render_target_view: None,
            active_depth_stencil_view: None,
            blend_states: Default::default(),
            depth_states: Default::default(),
            cull_states: Default::default(),
            current_layout: std::ptr::null_mut(),
            feature_level: level,
        };
        unsafe {
            // initDX11: the depth buffer first, then the back buffer's view
            kgl.create_depth_buffer()?;
            let (texture, kept) = match &kgl.swap_chain {
                Some(chain) => (err("GetBuffer", chain.GetBuffer::<ID3D11Texture2D>(0))?, None),
                None => {
                    // no window: a texture stands in for the back buffer
                    let desc = D3D11_TEXTURE2D_DESC {
                        Width: video.width as u32,
                        Height: video.height as u32,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    };
                    let mut texture = None;
                    err("CreateTexture2D", kgl.device.CreateTexture2D(&desc, None, Some(&mut texture)))?;
                    let texture = texture.ok_or("no screen texture")?;
                    (texture.clone(), Some(texture))
                }
            };
            let mut rtv = None;
            err("CreateRenderTargetView", kgl.device.CreateRenderTargetView(&texture, None, Some(&mut rtv)))?;
            kgl.screen_render_target = KglRenderTarget { texture: kept, render_target_view: rtv, samples: -1, ..Default::default() };
            kgl.active_render_target_view = kgl.screen_render_target.render_target_view.clone();
            kgl.active_depth_stencil_view = kgl.screen_depth_target.depth_view.clone();

            // the four depth-stencil states, created in the order 0, 3, 1, 2; everything but the
            // three fields is zero, as the game leaves it
            for (index, enable, write, func) in [
                (0usize, true, D3D11_DEPTH_WRITE_MASK_ALL, D3D11_COMPARISON_LESS),
                (3, true, D3D11_DEPTH_WRITE_MASK_ALL, D3D11_COMPARISON_LESS_EQUAL),
                (1, true, D3D11_DEPTH_WRITE_MASK_ZERO, D3D11_COMPARISON_LESS_EQUAL),
                (2, false, D3D11_DEPTH_WRITE_MASK_ZERO, D3D11_COMPARISON_ALWAYS),
            ] {
                let mut desc: D3D11_DEPTH_STENCIL_DESC = std::mem::zeroed();
                desc.DepthEnable = enable.into();
                desc.DepthWriteMask = write;
                desc.DepthFunc = func;
                let mut state = None;
                let result = kgl.device.CreateDepthStencilState(&desc, Some(&mut state));
                if index == 1 {
                    err("CreateDepthStencilState (depthStencilStateZWriteOFF)", result)?;
                }
                kgl.depth_states[index] = state;
            }
            kgl.init_blend_states();
            kgl.init_cull_states();
        }
        Ok(kgl)
    }

    /// `createDepthBuffer` 0x14001acc0.
    unsafe fn create_depth_buffer(&mut self) -> Result<(), String> {
        let count = if self.video.hdr_post_processing { 1 } else { self.video.aa_samples as u32 };
        self.screen_depth_target = self.depth_target(self.video.width, self.video.height, count as i32)?;
        Ok(())
    }

    /// A depth texture with its two views, as `createDepthBuffer` and the depth branch of
    /// `KGLRenderTarget::KGLRenderTarget` 0x1400232d0 make it.
    unsafe fn depth_target(&self, width: i32, height: i32, samples: i32) -> Result<KglRenderTarget, String> {
        let td = D3D11_TEXTURE2D_DESC {
            Width: width as u32,
            Height: height as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: samples as u32, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_DEPTH_STENCIL.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        if self.device.CreateTexture2D(&td, None, Some(&mut texture)).is_err() {
            eprintln!("ERROR: CreateTexture2D depthStencilTexture failed");
        }
        // a size the device refuses (video.ini SHADOW_MAP_SIZE=-1): the game goes on with no
        // texture and no views
        let Some(texture) = texture else {
            return Ok(KglRenderTarget { texture: None, render_target_view: None, shader_resource_view: None, depth_view: None, format: DXGI_FORMAT_R32_TYPELESS, width, height, samples: -1 });
        };
        let mut dd: D3D11_DEPTH_STENCIL_VIEW_DESC = std::mem::zeroed();
        dd.Format = DXGI_FORMAT_D32_FLOAT;
        dd.ViewDimension = if samples == 1 { D3D11_DSV_DIMENSION_TEXTURE2D } else { D3D11_DSV_DIMENSION_TEXTURE2DMS };
        let mut dsv = None;
        let _ = self.device.CreateDepthStencilView(&texture, Some(&dd), Some(&mut dsv));
        let mut sd: D3D11_SHADER_RESOURCE_VIEW_DESC = std::mem::zeroed();
        sd.Format = DXGI_FORMAT_R32_FLOAT;
        sd.ViewDimension = if samples == 1 { D3D_SRV_DIMENSION_TEXTURE2D } else { D3D_SRV_DIMENSION_TEXTURE2DMS };
        sd.Anonymous.Texture2D = D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 };
        let mut srv = None;
        let _ = self.device.CreateShaderResourceView(&texture, Some(&sd), Some(&mut srv));
        Ok(KglRenderTarget { texture: Some(texture), render_target_view: None, shader_resource_view: srv, depth_view: dsv, format: DXGI_FORMAT_R32_TYPELESS, width, height, samples: -1 })
    }

    /// `initBlendStates` 0x14001aeb0: created in the order 0, 2, 1, 3.
    unsafe fn init_blend_states(&mut self) {
        for (index, a2c, enable, op_alpha, mask) in [
            (0usize, false, false, D3D11_BLEND_OP_ADD, 0x0fu8),
            (2, true, false, D3D11_BLEND_OP_MAX, 0x0f),
            (1, false, true, D3D11_BLEND_OP_MAX, 0x0f),
            (3, false, true, D3D11_BLEND_OP_MAX, 0x00),
        ] {
            let mut desc: D3D11_BLEND_DESC = std::mem::zeroed();
            desc.AlphaToCoverageEnable = a2c.into();
            desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: enable.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ONE,
                BlendOpAlpha: op_alpha,
                RenderTargetWriteMask: mask,
            };
            let mut state = None;
            let _ = self.device.CreateBlendState(&desc, Some(&mut state));
            self.blend_states[index] = state;
        }
    }

    /// `initCullStates` 0x14001b0a0: created in index order; state 0 is bound right after it
    /// is made.
    unsafe fn init_cull_states(&mut self) {
        let multisample = self.video.aa_samples != 1;
        for (index, fill, cull, bias, clamp, ms) in [
            (0usize, D3D11_FILL_SOLID, D3D11_CULL_FRONT, 0i32, 0.0f32, multisample),
            (1, D3D11_FILL_SOLID, D3D11_CULL_BACK, 0, 0.0, false),
            (2, D3D11_FILL_SOLID, D3D11_CULL_NONE, 0, 0.0, false),
            (3, D3D11_FILL_SOLID, D3D11_CULL_NONE, -100, f32::from_bits(0x3dcc_cccd), false),
            (4, D3D11_FILL_WIREFRAME, D3D11_CULL_NONE, 0, 0.0, false),
            (5, D3D11_FILL_SOLID, D3D11_CULL_FRONT, 0, 0.0, false),
        ] {
            let desc = D3D11_RASTERIZER_DESC {
                FillMode: fill,
                CullMode: cull,
                FrontCounterClockwise: false.into(),
                DepthBias: bias,
                DepthBiasClamp: clamp,
                SlopeScaledDepthBias: 0.0,
                DepthClipEnable: true.into(),
                ScissorEnable: false.into(),
                MultisampleEnable: ms.into(),
                AntialiasedLineEnable: false.into(),
            };
            let mut state = None;
            let _ = self.device.CreateRasterizerState(&desc, Some(&mut state));
            self.cull_states[index] = state;
            if index == 0 {
                self.context.RSSetState(self.cull_states[0].as_ref());
            }
        }
    }

    /// `kglSetDefaultState` 0x140018920.
    pub fn set_default_state(&mut self) {
        unsafe {
            self.context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.GSSetShader(None, None);
            self.context.DSSetShader(None, None);
            self.context.HSSetShader(None, None);
            self.context.PSSetShader(None, None);
            self.context.VSSetShader(None, None);
            self.context.CSSetShader(None, None);
            self.context.OMSetBlendState(self.blend_states[0].as_ref(), None, 0xffff_ffff);
            self.context.RSSetState(self.cull_states[2].as_ref());
            self.context.OMSetDepthStencilState(self.depth_states[0].as_ref(), 0);
        }
        self.current_layout = std::ptr::null_mut();
    }

    /// `kglSetBlendState` 0x140018ff0.
    pub fn set_blend_state(&self, index: i32) {
        unsafe { self.context.OMSetBlendState(self.blend_states[index as usize].as_ref(), None, 0xffff_ffff) }
    }

    /// `kglSetDepthState` 0x140019020.
    pub fn set_depth_state(&self, index: i32) {
        unsafe { self.context.OMSetDepthStencilState(self.depth_states[index as usize].as_ref(), 0) }
    }

    /// `kglSetCullState` 0x140019050.
    pub fn set_cull_state(&self, index: i32) {
        unsafe { self.context.RSSetState(self.cull_states[index as usize].as_ref()) }
    }

    /// `kglSetViewport` 0x140018ee0.
    pub fn set_viewport(&self, x: f32, y: f32, width: f32, height: f32) {
        let viewport = D3D11_VIEWPORT { TopLeftX: x, TopLeftY: y, Width: width, Height: height, MinDepth: 0.0, MaxDepth: 1.0 };
        unsafe { self.context.RSSetViewports(Some(&[viewport])) }
    }

    /// `kglSetSamplerPS` 0x140018cf0.
    pub fn set_sampler_ps(&self, sampler: &Option<ID3D11SamplerState>, slot: u32) {
        unsafe { self.context.PSSetSamplers(slot, Some(std::slice::from_ref(sampler))) }
    }

    /// `kglCreateSampler` 0x140018be0.
    pub fn create_sampler(&self, kind: i32, clamp: bool, max_anisotropy: i32, mip_lod_bias: f32) -> Option<ID3D11SamplerState> {
        let mut desc: D3D11_SAMPLER_DESC = unsafe { std::mem::zeroed() };
        desc.Filter = match kind {
            0 => D3D11_FILTER_ANISOTROPIC,
            1 => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            2 => D3D11_FILTER_MIN_MAG_MIP_POINT,
            3 => D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
            _ => D3D11_FILTER(0),
        };
        let address = if clamp { D3D11_TEXTURE_ADDRESS_CLAMP } else { D3D11_TEXTURE_ADDRESS_WRAP };
        desc.AddressU = address;
        desc.AddressV = address;
        desc.AddressW = address;
        desc.MipLODBias = mip_lod_bias;
        desc.MaxAnisotropy = if kind == 0 { max_anisotropy as u32 } else { 0 };
        desc.ComparisonFunc = if kind == 3 { D3D11_COMPARISON_LESS } else { D3D11_COMPARISON_FUNC(0) };
        desc.MinLOD = 0.0;
        desc.MaxLOD = f32::MAX;
        let mut sampler = None;
        match unsafe { self.device.CreateSamplerState(&desc, Some(&mut sampler)) } {
            Ok(()) => sampler,
            Err(_) => None,
        }
    }

    /// `kglSetRenderTarget` 0x140018d20: keeps the depth view that is active.
    pub fn set_render_target(&mut self, target: Option<&KglRenderTarget>) {
        unsafe {
            match target {
                Some(rt) => {
                    self.context.OMSetRenderTargets(Some(std::slice::from_ref(&rt.render_target_view)), self.active_depth_stencil_view.as_ref());
                    self.active_render_target_view = rt.render_target_view.clone();
                }
                None => {
                    self.context.OMSetRenderTargets(None, self.active_depth_stencil_view.as_ref());
                    self.active_render_target_view = None;
                }
            }
        }
    }

    /// `kglSetRenderTargets` 0x140018e40. `None` for the colour target still passes one (null)
    /// view.
    pub fn set_render_targets(&mut self, target: Option<&KglRenderTarget>, depth: Option<&KglRenderTarget>) {
        let dsv = depth.and_then(|d| d.depth_view.clone());
        self.active_depth_stencil_view = dsv.clone();
        let rtv = target.and_then(|t| t.render_target_view.clone());
        unsafe { self.context.OMSetRenderTargets(Some(std::slice::from_ref(&rtv)), dsv.as_ref()) };
        self.active_render_target_view = rtv;
    }

    /// `kglSetRenderTargets(kglGetScreenRenderTarget(), kglGetScreenDepthTarget())`, or with no
    /// depth target.
    pub fn set_screen_render_targets(&mut self, with_depth: bool) {
        let dsv = if with_depth { self.screen_depth_target.depth_view.clone() } else { None };
        self.active_depth_stencil_view = dsv.clone();
        let rtv = self.screen_render_target.render_target_view.clone();
        unsafe { self.context.OMSetRenderTargets(Some(std::slice::from_ref(&rtv)), dsv.as_ref()) };
        self.active_render_target_view = rtv;
    }

    /// `kglClearColor` 0x140018f80.
    pub fn clear_color(&self, rgba: &[f32; 4]) {
        if let Some(view) = &self.active_render_target_view {
            unsafe { self.context.ClearRenderTargetView(view, rgba) }
        }
    }

    /// `kglClearDepth` 0x140018fb0.
    pub fn clear_depth(&self, depth: f32) {
        if let Some(view) = &self.active_depth_stencil_view {
            unsafe { self.context.ClearDepthStencilView(view, D3D11_CLEAR_DEPTH.0, depth, 0) }
        }
    }

    /// `kglRenderTargetClear` 0x140018d90.
    pub fn render_target_clear(&self, target: &KglRenderTarget, rgba: &[f32; 4], depth: f32) {
        unsafe {
            if let Some(view) = &target.render_target_view {
                self.context.ClearRenderTargetView(view, rgba);
            }
            if let Some(view) = &target.depth_view {
                self.context.ClearDepthStencilView(view, D3D11_CLEAR_DEPTH.0, depth, 0);
            }
        }
    }

    /// `kglSetTexture` 0x140019380 / `kglSetTextureRT` 0x140018ec0 / `kglSetTextureCubeMap`
    /// 0x1400192c0: one pixel-shader slot.
    pub fn set_texture(&self, slot: u32, view: &Option<ID3D11ShaderResourceView>) {
        unsafe { self.context.PSSetShaderResources(slot, Some(std::slice::from_ref(view))) }
    }

    /// `kglSetShader` 0x140019dc0.
    pub fn set_shader(&mut self, layout: &Option<ID3D11InputLayout>, vs: &Option<ID3D11VertexShader>, ps: &Option<ID3D11PixelShader>) {
        let raw = layout.as_ref().map_or(std::ptr::null_mut(), |l| l.as_raw());
        unsafe {
            if raw != self.current_layout {
                self.context.IASetInputLayout(layout.as_ref());
                self.current_layout = raw;
            }
            self.context.VSSetShader(vs.as_ref(), None);
            self.context.PSSetShader(ps.as_ref(), None);
        }
    }

    /// `kglCreateCBuffer` 0x140019870 / `KGLCBuffer::KGLCBuffer` 0x140023cb0.
    pub fn create_cbuffer(&self, size: i32) -> KglCBuffer {
        let desc = D3D11_BUFFER_DESC { ByteWidth: size as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
        let mut buffer = None;
        unsafe {
            let _ = self.device.CreateBuffer(&desc, None, Some(&mut buffer));
        }
        if buffer.is_none() {
            eprintln!("ERROR: CBuffer CreateBuffer failed size={size}");
        }
        KglCBuffer { buffer, size }
    }

    /// `kglCBufferMap` 0x140019bb0: `UpdateSubresource` of the whole buffer.
    pub fn cbuffer_map(&self, cbuffer: &KglCBuffer, data: &[u8]) {
        if let Some(buffer) = &cbuffer.buffer {
            unsafe { self.context.UpdateSubresource(buffer, 0, None, data.as_ptr() as *const c_void, 0, 0) }
        }
    }

    /// `kglCBufferBind` 0x140019bf0: the vertex stage, then the pixel stage.
    pub fn cbuffer_bind(&self, cbuffer: &KglCBuffer, slot: i32) {
        unsafe {
            self.context.VSSetConstantBuffers(slot as u32, Some(std::slice::from_ref(&cbuffer.buffer)));
            self.context.PSSetConstantBuffers(slot as u32, Some(std::slice::from_ref(&cbuffer.buffer)));
        }
    }

    /// `kglCreateVertexBuffer` 0x140019fc0 / `kglCreateDynamicVertexBuffer` 0x14001a020.
    pub fn create_vertex_buffer(&self, bytes: &[u8], byte_size: usize, stride: u32, dynamic: bool) -> KglVertexBuffer {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: byte_size as u32,
            Usage: if dynamic { D3D11_USAGE_DYNAMIC } else { D3D11_USAGE_IMMUTABLE },
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: if dynamic { D3D11_CPU_ACCESS_WRITE.0 as u32 } else { 0 },
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: bytes.as_ptr() as *const c_void, SysMemPitch: 0, SysMemSlicePitch: 0 };
        let mut buffer = None;
        unsafe {
            let result = self.device.CreateBuffer(&desc, if bytes.is_empty() { None } else { Some(&init) }, Some(&mut buffer));
            if result.is_err() {
                eprintln!("FAILED TO CREATE VERTEX BUFFER size:{byte_size} stride:{stride} dynamic:{}", dynamic as i32);
            }
        }
        KglVertexBuffer { buffer, stride }
    }

    /// `kglCreateIndexBuffer` 0x14001a120 (16-bit indices: the only kind the game has).
    pub fn create_index_buffer(&self, indices: &[u16]) -> KglIndexBuffer {
        let desc = D3D11_BUFFER_DESC { ByteWidth: (indices.len() * 2) as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_INDEX_BUFFER.0 as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: indices.as_ptr() as *const c_void, SysMemPitch: 0, SysMemSlicePitch: 0 };
        let mut buffer = None;
        unsafe {
            let _ = self.device.CreateBuffer(&desc, Some(&init), Some(&mut buffer));
        }
        KglIndexBuffer { buffer }
    }

    /// `kglSetVertexBuffer` 0x14001a0d0.
    pub fn set_vertex_buffer(&self, vb: &KglVertexBuffer) {
        let offset = 0u32;
        unsafe { self.context.IASetVertexBuffers(0, 1, Some(&vb.buffer), Some(&vb.stride), Some(&offset)) }
    }

    /// `kglVertexBufferMap` 0x14001a090 (`KGLVertexBuffer::map` 0x140023e30): the bytes go
    /// into a dynamic buffer whose old content is thrown away.
    pub fn vertex_buffer_map(&self, vb: &KglVertexBuffer, data: &[u8]) {
        let Some(buffer) = &vb.buffer else {
            return;
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            if self.context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)).is_ok() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData as *mut u8, data.len());
            }
            self.context.Unmap(buffer, 0);
        }
    }

    /// `kglSetPrimitiveType` 0x140019340: 0 a triangle list, 1 a line list, 2 a line strip,
    /// 3 a triangle strip.
    pub fn set_primitive_type(&self, kind: i32) {
        let topology = match kind {
            0 => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            1 => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
            2 => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
            3 => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            _ => return,
        };
        unsafe { self.context.IASetPrimitiveTopology(topology) }
    }

    /// `kglDraw` 0x14001a1b0.
    pub fn draw(&self, vertex_count: i32, start_vertex: i32) {
        unsafe { self.context.Draw(vertex_count as u32, start_vertex as u32) }
    }

    /// `kglSetIndexBuffer` 0x14001a170.
    pub fn set_index_buffer(&self, ib: &KglIndexBuffer) {
        unsafe { self.context.IASetIndexBuffer(ib.buffer.as_ref(), DXGI_FORMAT_R16_UINT, 0) }
    }

    /// `kglDrawIndexed` 0x14001a190.
    pub fn draw_indexed(&self, index_count: i32, start_index: i32, base_vertex: i32) {
        unsafe { self.context.DrawIndexed(index_count as u32, start_index as u32, base_vertex) }
    }

    /// `KGLRenderTarget::KGLRenderTarget` 0x1400232d0, the colour branch
    /// (`kglCreateRenderTarget` 0x1400196e0).
    pub fn create_render_target(&self, format: DXGI_FORMAT, width: i32, height: i32, samples: i32, mips: i32) -> KglRenderTarget {
        unsafe {
            let _ = self.device.CheckMultisampleQualityLevels(format, samples as u32);
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width as u32,
                Height: height as u32,
                MipLevels: mips as u32,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: samples as u32, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: if mips != 1 { D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32 } else { 0 },
            };
            let mut texture = None;
            let _ = self.device.CreateTexture2D(&desc, None, Some(&mut texture));
            let mut rtv = None;
            let mut srv = None;
            if let Some(texture) = &texture {
                let _ = self.device.CreateRenderTargetView(texture, None, Some(&mut rtv));
                let _ = self.device.CreateShaderResourceView(texture, None, Some(&mut srv));
            }
            KglRenderTarget { texture, render_target_view: rtv, shader_resource_view: srv, depth_view: None, format, width, height, samples }
        }
    }

    /// `kglCreateRenderTargetDepth` 0x1400197a0.
    pub fn create_render_target_depth(&self, width: i32, height: i32, samples: i32) -> Result<KglRenderTarget, String> {
        unsafe {
            let _ = self.device.CheckMultisampleQualityLevels(DXGI_FORMAT_R32_TYPELESS, samples as u32);
            let mut target = self.depth_target(width, height, samples)?;
            target.samples = samples;
            Ok(target)
        }
    }

    /// `kglSwapBuffers` 0x140019080.
    pub fn swap_buffers(&self) {
        if let Some(chain) = &self.swap_chain {
            unsafe {
                let result = chain.Present(if self.video.v_sync { 1 } else { 0 }, DXGI_PRESENT(0));
                if result.is_err() {
                    eprintln!("swapChain->Present failed");
                }
            }
        }
    }

    /// The screen target's pixels, as RGBA rows (the back buffer, or the texture that stands
    /// in for it). Not a kgl function: `--screenshot` and the oracle read the frame with it.
    pub fn read_screen(&self) -> Result<(u32, u32, Vec<u8>), String> {
        unsafe {
            let source: ID3D11Texture2D = match (&self.swap_chain, &self.screen_render_target.texture) {
                (Some(chain), _) => err("GetBuffer", chain.GetBuffer(0))?,
                (None, Some(texture)) => texture.clone(),
                (None, None) => return Err("no screen target".into()),
            };
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            err("CreateTexture2D", self.device.CreateTexture2D(&desc, None, Some(&mut staging)))?;
            let staging = staging.ok_or("no staging texture")?;
            self.context.CopyResource(&staging, &source);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            err("Map", self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)))?;
            let mut pixels = Vec::with_capacity((desc.Width * desc.Height * 4) as usize);
            for row in 0..desc.Height as usize {
                let p = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                pixels.extend_from_slice(std::slice::from_raw_parts(p, desc.Width as usize * 4));
            }
            self.context.Unmap(&staging, 0);
            Ok((desc.Width, desc.Height, pixels))
        }
    }
}

impl Kgl {
    /// `kglCreateCubeMap` 0x140018a30 / `KGLCubeMap::KGLCubeMap` 0x140023530: a depth texture
    /// (with as many mip levels as the cube, as the game asks for), the cube, a view per face
    /// and the cube's shader view. A size the device refuses leaves every view empty.
    pub fn create_cube_map(&self, size: i32, hdr: bool, mips: i32) -> KglCubeMap {
        let mut cube = KglCubeMap { texture: None, srv_cube: None, rtv_face: vec![None; 6], depth_texture: None, dsv_depth: None, size, mips };
        unsafe {
            let depth_desc = D3D11_TEXTURE2D_DESC {
                Width: size as u32,
                Height: size as u32,
                MipLevels: mips as u32,
                ArraySize: 1,
                Format: DXGI_FORMAT_D16_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let _ = self.device.CreateTexture2D(&depth_desc, None, Some(&mut cube.depth_texture));
            let mut dd: D3D11_DEPTH_STENCIL_VIEW_DESC = std::mem::zeroed();
            dd.Format = DXGI_FORMAT_D16_UNORM;
            dd.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
            dd.Anonymous.Texture2D = D3D11_TEX2D_DSV { MipSlice: 0 };
            if let Some(texture) = &cube.depth_texture {
                let _ = self.device.CreateDepthStencilView(texture, Some(&dd), Some(&mut cube.dsv_depth));
            }
            let format = if hdr { DXGI_FORMAT_R16G16B16A16_FLOAT } else { DXGI_FORMAT_R8G8B8A8_UNORM };
            let cube_desc = D3D11_TEXTURE2D_DESC {
                Width: size as u32,
                Height: size as u32,
                MipLevels: mips as u32,
                ArraySize: 6,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: (D3D11_RESOURCE_MISC_GENERATE_MIPS.0 | D3D11_RESOURCE_MISC_TEXTURECUBE.0) as u32,
            };
            let _ = self.device.CreateTexture2D(&cube_desc, None, Some(&mut cube.texture));
            if let Some(texture) = &cube.texture {
                for face in 0..6u32 {
                    let mut rd: D3D11_RENDER_TARGET_VIEW_DESC = std::mem::zeroed();
                    rd.Format = format;
                    rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
                    rd.Anonymous.Texture2DArray = D3D11_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: face, ArraySize: 1 };
                    let _ = self.device.CreateRenderTargetView(texture, Some(&rd), Some(&mut cube.rtv_face[face as usize]));
                }
                let mut sd: D3D11_SHADER_RESOURCE_VIEW_DESC = std::mem::zeroed();
                sd.Format = format;
                sd.ViewDimension = D3D_SRV_DIMENSION_TEXTURECUBE;
                sd.Anonymous.TextureCube = D3D11_TEXCUBE_SRV { MostDetailedMip: 0, MipLevels: mips as u32 };
                let _ = self.device.CreateShaderResourceView(texture, Some(&sd), Some(&mut cube.srv_cube));
            }
        }
        cube
    }

    /// `kglCubeMapBeginFace` 0x140018b30. The viewport is set behind the manager's back (the
    /// lighting buffer's 1/width, 1/height do not follow).
    pub fn cube_map_begin_face(&mut self, cube: &KglCubeMap, face: usize) {
        unsafe {
            self.context.OMSetRenderTargets(Some(std::slice::from_ref(&cube.rtv_face[face])), cube.dsv_depth.as_ref());
        }
        self.set_viewport(0.0, 0.0, cube.size as f32, cube.size as f32);
        // only the depth view becomes the active one: the colour clear that follows goes to
        // whatever colour view was active before (the game never clears a cube face's colour)
        self.active_depth_stencil_view = cube.dsv_depth.clone();
    }

    /// `kglCubeMapGenerateMips` 0x140019310.
    pub fn cube_map_generate_mips(&self, cube: &KglCubeMap, face: i32) {
        if cube.mips > 1 && face == -1 {
            unsafe { self.context.GenerateMips(cube.srv_cube.as_ref()) }
        }
    }

    /// `kglSetTextureCubeMap` 0x1400192c0.
    pub fn set_texture_cube_map(&self, cube: Option<&KglCubeMap>, slot: u32) {
        let view = cube.and_then(|c| c.srv_cube.clone());
        self.set_texture(slot, &view);
    }
}

impl Kgl {
    /// `kglResizeBuffers` 0x1400193d0 for a screen that is a texture: nothing stays bound, the
    /// screen's colour and depth targets are made again and bound.
    pub fn resize_screen(&mut self, width: i32, height: i32) -> Result<(), String> {
        if self.swap_chain.is_some() {
            return Err("resizing a swap chain's buffers is the window's business here".into());
        }
        self.video.width = width;
        self.video.height = height;
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width as u32,
                Height: height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut texture = None;
            err("CreateTexture2D", self.device.CreateTexture2D(&desc, None, Some(&mut texture)))?;
            let texture = texture.ok_or("no screen texture")?;
            let mut rtv = None;
            err("CreateRenderTargetView", self.device.CreateRenderTargetView(&texture, None, Some(&mut rtv)))?;
            self.screen_render_target = KglRenderTarget { texture: Some(texture), render_target_view: rtv, samples: -1, ..Default::default() };
            self.create_depth_buffer()?;
        }
        self.set_screen_render_targets(true);
        Ok(())
    }
}
