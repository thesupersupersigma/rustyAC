// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! kgl on wgpu (`backend-wgpu` feature): the same functions, state objects and objects as
//! `kgl.rs` gives the renderer on Direct3D 11, here on WebGPU's model, so the renderer above
//! runs unchanged in a browser and on any wgpu adapter. AC's own compiled shaders go through
//! rustyac-dxbc's WGSL translation; the constant buffers are the same bytes.
//!
//! Direct3D 11 is immediate and WebGPU is recorded, so the calls are kept as Direct3D does them
//! and replayed: every draw takes a copy of the bound state (the constant buffers' and dynamic
//! vertex buffers' bytes into one upload buffer, the views, the samplers, the pipeline for the
//! states and targets), the draws on one pair of targets become one render pass, and a clear,
//! a target change, a resolve, mip making or a read ends it. Direct3D's rules where WebGPU has
//! none are kept here: a texture bound as a target is unbound from the shader slots, an empty
//! slot reads zeros, an empty sampler is the default one, a depth target sampled as R32F is a
//! copy made when it is read after being written.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::kgl_log;

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

/// NOT Assetto Corsa: see `kgl.rs`.
pub static EXPERIMENT_NO_ALPHA_TO_COVERAGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Which adapter [`Kgl::init`] opens (natively; the browser hands over its own device).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// Direct3D 12 WARP, Microsoft's software rasteriser
    Warp,
    /// the graphics card (Direct3D 12 on Windows, Vulkan elsewhere)
    Gpu,
    /// Vulkan on a CPU adapter: SwiftShader, picked by VK_DRIVER_FILES
    SwiftShader,
}

/// Where the device comes from. The screen is always a texture here (the page or the caller
/// shows it).
#[derive(Clone, Copy, Debug)]
pub struct DeviceOptions {
    pub adapter: Adapter,
    /// keep the kgl call log names (the log itself runs between `kgl_log::begin` and `end`)
    pub log: bool,
}

/// A DXGI format number, as the renderer above names its formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Format(pub i32);

pub const FORMAT_RGBA8: Format = Format(28);
pub const FORMAT_RGBA16F: Format = Format(10);
pub const FORMAT_R32F: Format = Format(41);

/// The DXGI names `texture_fallback` uses.
pub mod dxgi {
    pub use super::Format as DXGI_FORMAT;
    pub const DXGI_FORMAT_R8G8B8A8_UNORM: DXGI_FORMAT = DXGI_FORMAT(28);
    pub const DXGI_FORMAT_BC1_UNORM: DXGI_FORMAT = DXGI_FORMAT(71);
    pub const DXGI_FORMAT_BC2_UNORM: DXGI_FORMAT = DXGI_FORMAT(74);
    pub const DXGI_FORMAT_BC3_UNORM: DXGI_FORMAT = DXGI_FORMAT(77);
    pub const DXGI_FORMAT_BC4_UNORM: DXGI_FORMAT = DXGI_FORMAT(80);
    pub const DXGI_FORMAT_BC5_UNORM: DXGI_FORMAT = DXGI_FORMAT(83);
}

fn texture_format(f: Format) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as T;
    Some(match f.0 {
        2 => T::Rgba32Float,
        10 => T::Rgba16Float,
        16 => T::Rg32Float,
        24 => T::Rgb10a2Unorm,
        26 => T::Rg11b10Ufloat,
        28 => T::Rgba8Unorm,
        29 => T::Rgba8UnormSrgb,
        34 => T::Rg16Float,
        41 => T::R32Float,
        49 => T::Rg8Unorm,
        54 => T::R16Float,
        61 => T::R8Unorm,
        71 => T::Bc1RgbaUnorm,
        72 => T::Bc1RgbaUnormSrgb,
        74 => T::Bc2RgbaUnorm,
        75 => T::Bc2RgbaUnormSrgb,
        77 => T::Bc3RgbaUnorm,
        78 => T::Bc3RgbaUnormSrgb,
        80 => T::Bc4RUnorm,
        81 => T::Bc4RSnorm,
        83 => T::Bc5RgUnorm,
        84 => T::Bc5RgSnorm,
        87 => T::Bgra8Unorm,
        91 => T::Bgra8UnormSrgb,
        95 => T::Bc6hRgbUfloat,
        96 => T::Bc6hRgbFloat,
        98 => T::Bc7RgbaUnorm,
        99 => T::Bc7RgbaUnormSrgb,
        _ => return None,
    })
}

/// A texture with what the replay needs to know of it.
pub struct Tex {
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    samples: u32,
    depth: bool,
    /// passes that wrote this depth texture, and the R32F copy made at which count
    written: Cell<u64>,
    copy: RefCell<Option<(u64, wgpu::Texture, wgpu::TextureView)>>,
}

#[derive(Clone)]
pub struct Texture2D(Rc<Tex>);

pub struct ViewInner {
    tex: Rc<Tex>,
    view: wgpu::TextureView,
    cube: bool,
}

/// A view: what Direct3D's shader-resource, render-target and depth-stencil views are.
#[derive(Clone)]
pub struct View(Rc<ViewInner>);

pub struct SamplerInner {
    plain: wgpu::Sampler,
    comparison: wgpu::Sampler,
}

#[derive(Clone)]
pub struct Sampler(Rc<SamplerInner>);

pub struct ShaderBlob {
    id: u32,
    shader: rustyac_dxbc::Shader,
}

#[derive(Clone)]
pub struct VertexShader(Rc<ShaderBlob>);

#[derive(Clone)]
pub struct PixelShader(Rc<ShaderBlob>);

pub struct LayoutInner {
    id: u32,
    elements: Vec<(String, u32, wgpu::VertexFormat, u32)>,
}

#[derive(Clone)]
pub struct InputLayout(Rc<LayoutInner>);

pub struct BufInner {
    /// a buffer made once with its bytes (vertex buffers that are not dynamic, index buffers)
    gpu: Option<wgpu::Buffer>,
    /// the bytes of a constant buffer or a dynamic vertex buffer, as the last map left them
    bytes: RefCell<Vec<u8>>,
    /// bumped by each map; the copy in the upload buffer: (flush, version, offset, size)
    version: Cell<u64>,
    copy: Cell<Option<(u64, u64, u64, u64)>>,
    size: usize,
}

#[derive(Clone)]
pub struct Buffer(Rc<BufInner>);

/// `KGLRenderTarget` 0x30 bytes.
#[derive(Default)]
pub struct KglRenderTarget {
    pub texture: Option<Texture2D>,
    pub render_target_view: Option<View>,
    pub shader_resource_view: Option<View>,
    pub depth_view: Option<View>,
    pub format: Format,
    pub width: i32,
    pub height: i32,
    pub samples: i32,
}

/// `KGLCubeMap`: the reflection cube map with its per-face views.
pub struct KglCubeMap {
    pub texture: Option<Texture2D>,
    pub srv_cube: Option<View>,
    pub rtv_face: Vec<Option<View>>,
    pub depth_texture: Option<Texture2D>,
    pub dsv_depth: Option<View>,
    pub size: i32,
    pub mips: i32,
}

/// `KGLVertexBuffer`.
pub struct KglVertexBuffer {
    pub buffer: Option<Buffer>,
    pub stride: u32,
}

/// `KGLIndexBuffer`.
pub struct KglIndexBuffer {
    pub buffer: Option<Buffer>,
}

/// `KGLCBuffer`.
pub struct KglCBuffer {
    pub buffer: Option<Buffer>,
    pub size: i32,
}

/// The kgl log's key of an object (its address while it lives).
fn key<T>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as usize
}

pub fn view_key(view: &Option<View>) -> Option<usize> {
    view.as_ref().map(|v| key(&v.0))
}

fn buffer_key(buffer: &Option<Buffer>) -> Option<usize> {
    buffer.as_ref().map(|b| key(&b.0))
}

fn sampler_key(sampler: &Option<Sampler>) -> Option<usize> {
    sampler.as_ref().map(|s| key(&s.0))
}

fn log_name(key: Option<usize>) -> String {
    kgl_log::name(key)
}

/// What Direct3D 11 keeps bound.
#[derive(Clone)]
pub(crate) struct Bound {
    blend: i32,
    depth: i32,
    cull: i32,
    topology: i32,
    layout: Option<InputLayout>,
    vs: Option<VertexShader>,
    ps: Option<PixelShader>,
    viewport: [f32; 4],
    samplers: Vec<Option<Sampler>>,
    textures: Vec<Option<View>>,
    cbuffers: Vec<Option<(Buffer, usize)>>,
    vb: Option<(Buffer, u32)>,
    ib: Option<Buffer>,
    rtv: Option<View>,
    dsv: Option<View>,
    /// kgl's active colour view, which `clear_color` clears (a cube face is bound without it)
    clear_rtv: Option<View>,
}

const SLOTS: usize = 128;
/// FW1FontWrapper's blend state (ONE / INV_SRC_ALPHA, alpha ONE / ZERO), which no kgl index has.
pub(crate) const FW1_BLEND: i32 = 100;
const CB_SLOTS: usize = 14;

struct Program {
    vs: wgpu::ShaderModule,
    ps: wgpu::ShaderModule,
    t: rustyac_dxbc::Translated,
    groups: [wgpu::BindGroupLayout; 3],
    layout: wgpu::PipelineLayout,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct PipelineKey {
    program: (u32, u32),
    layout: u32,
    stride: u32,
    blend: i32,
    depth: i32,
    cull: i32,
    topology: i32,
    indexed: bool,
    color: Option<wgpu::TextureFormat>,
    depth_format: Option<wgpu::TextureFormat>,
    samples: u32,
}

#[derive(Clone, Copy)]
enum Call {
    Plain(u32, u32),
    Indexed(u32, u32, i32),
}

struct Draw {
    pipeline: Rc<wgpu::RenderPipeline>,
    program: Rc<Program>,
    cbuffers: Vec<(u32, u64, u64)>,
    textures: Vec<(u32, wgpu::TextureView)>,
    samplers: Vec<(u32, wgpu::Sampler)>,
    vb: Option<(Option<wgpu::Buffer>, u64)>,
    ib: Option<wgpu::Buffer>,
    viewport: [f32; 4],
    call: Call,
}

/// The recorded draws on one pair of targets, and the upload buffer's bytes.
struct Pass {
    color: Option<View>,
    depth: Option<View>,
    draws: Vec<Draw>,
}

struct Replay {
    encoder: Option<wgpu::CommandEncoder>,
    pass: Option<Pass>,
    upload: Vec<u8>,
    /// counts the flushes of the upload buffer (a copy in it is good for one)
    epoch: u64,
    programs: HashMap<(u32, u32), Option<Rc<Program>>>,
    pipelines: HashMap<PipelineKey, Option<Rc<wgpu::RenderPipeline>>>,
    blits: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    depth_copy: Option<wgpu::RenderPipeline>,
    warned: std::collections::HashSet<String>,
}

pub struct Kgl {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub features: wgpu::Features,
    pub video: KglVideoSettings,
    pub screen_render_target: KglRenderTarget,
    pub screen_depth_target: KglRenderTarget,
    bound: RefCell<Bound>,
    replay: RefCell<Replay>,
    dummy_2d: wgpu::TextureView,
    dummy_cube: wgpu::TextureView,
    dummy_depth: wgpu::TextureView,
    /// depth targets for pixel shaders that write depth with none bound, by size and samples
    scratch_depths: RefCell<HashMap<(u32, u32, u32), View>>,
    dummy_ms: wgpu::TextureView,
    default_sampler: Sampler,
    next_id: Cell<u32>,
}

/// Polls a future to its end (wgpu's native futures are ready once the device is polled).
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// The optional features kgl uses when the adapter has them: block-compressed textures (AC's
/// dds files), filtering and blending of 32-bit float targets (Direct3D 11 has both), lines for
/// the wireframe state.
pub fn wanted_features(adapter: wgpu::Features) -> wgpu::Features {
    adapter & (wgpu::Features::TEXTURE_COMPRESSION_BC | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE | wgpu::Features::POLYGON_MODE_LINE)
}

const FULLSCREEN_VS: &str = "
struct V { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: V;
    o.p = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}
";

/// WebGPU has one and four samples a pixel: kgl's 2 and 8 (AC's AA settings) are drawn with 4.
/// The targets keep the number asked for (the renderer and the kgl log see that one).
fn physical_samples(samples: u32) -> u32 {
    if samples > 1 {
        4
    } else {
        1
    }
}

thread_local! {
    static GIVEN: RefCell<Option<(wgpu::Device, wgpu::Queue, wgpu::Features)>> = const { RefCell::new(None) };
}

impl Kgl {
    /// `kglInit` 0x1400184b0 on a wgpu adapter: the device, the screen (a texture) and its depth
    /// buffer. The state objects are plain numbers here: each draw's pipeline is made from them.
    pub fn init(video: KglVideoSettings, options: DeviceOptions) -> Result<Kgl, String> {
        if let Some((device, queue, features)) = GIVEN.with(|g| g.borrow_mut().take()) {
            return Kgl::with_device(device, queue, features, video, options);
        }
        Kgl::open(video, options)
    }

    /// The device the next [`Kgl::init`] takes instead of opening one (the page's).
    pub fn give_device(device: wgpu::Device, queue: wgpu::Queue, features: wgpu::Features) {
        GIVEN.with(|g| *g.borrow_mut() = Some((device, queue, features)));
    }

    #[cfg(target_arch = "wasm32")]
    fn open(_video: KglVideoSettings, _options: DeviceOptions) -> Result<Kgl, String> {
        Err("in the browser kgl takes the page's device (Kgl::give_device)".into())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open(video: KglVideoSettings, options: DeviceOptions) -> Result<Kgl, String> {
        let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
        d.backends = match options.adapter {
            Adapter::SwiftShader => wgpu::Backends::VULKAN,
            _ if cfg!(windows) => wgpu::Backends::DX12,
            _ => wgpu::Backends::VULKAN,
        };
        d.flags = d.flags.with_env();
        d.backend_options = d.backend_options.with_env();
        let instance = wgpu::Instance::new(d);
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: options.adapter != Adapter::Gpu,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("wgpu adapter: {e}"))?;
        let features = wanted_features(adapter.features());
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("kgl"), required_features: features, required_limits: adapter.limits(), ..Default::default() }))
            .map_err(|e| format!("wgpu device: {e}"))?;
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| eprintln!("wgpu error: {e}")));
        Kgl::with_device(device, queue, features, video, options)
    }

    /// The rest of `kglInit` on a device that is already open (the browser's).
    pub fn with_device(device: wgpu::Device, queue: wgpu::Queue, features: wgpu::Features, video: KglVideoSettings, _options: DeviceOptions) -> Result<Kgl, String> {
        let dummy = |format, layers, samples: u32, cube: bool| {
            let mut usage = wgpu::TextureUsages::TEXTURE_BINDING;
            if samples > 1 || format == wgpu::TextureFormat::Depth32Float {
                usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
            }
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("kgl empty slot"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(if cube { wgpu::TextureViewDimension::Cube } else { wgpu::TextureViewDimension::D2 }), ..Default::default() })
        };
        let dummy_2d = dummy(wgpu::TextureFormat::Rgba8Unorm, 1, 1, false);
        let dummy_cube = dummy(wgpu::TextureFormat::Rgba8Unorm, 6, 1, true);
        let dummy_depth = dummy(wgpu::TextureFormat::Depth32Float, 1, 1, false);
        let dummy_ms = dummy(wgpu::TextureFormat::Rgba8Unorm, 1, 4, false);
        // Direct3D 11's default sampler: linear, clamped
        let default_sampler = Sampler(Rc::new(SamplerInner {
            plain: device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, lod_max_clamp: 32.0, ..Default::default() }),
            comparison: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                compare: Some(wgpu::CompareFunction::Never),
                lod_max_clamp: 32.0,
                ..Default::default()
            }),
        }));
        let mut kgl = Kgl {
            device,
            queue,
            features,
            video,
            screen_render_target: KglRenderTarget::default(),
            screen_depth_target: KglRenderTarget::default(),
            bound: RefCell::new(Bound {
                blend: 0,
                depth: 0,
                cull: 0,
                topology: 0,
                layout: None,
                vs: None,
                ps: None,
                viewport: [0.0, 0.0, video.width as f32, video.height as f32],
                samplers: vec![None; SLOTS],
                textures: vec![None; SLOTS],
                cbuffers: vec![None; CB_SLOTS],
                vb: None,
                ib: None,
                rtv: None,
                dsv: None,
                clear_rtv: None,
            }),
            replay: RefCell::new(Replay {
                encoder: None,
                pass: None,
                upload: Vec::new(),
                epoch: 0,
                programs: HashMap::new(),
                pipelines: HashMap::new(),
                blits: HashMap::new(),
                depth_copy: None,
                warned: Default::default(),
            }),
            dummy_2d,
            dummy_cube,
            dummy_depth,
            scratch_depths: RefCell::new(HashMap::new()),
            dummy_ms,
            default_sampler,
            next_id: Cell::new(0),
        };
        kgl.make_screen()?;
        Ok(kgl)
    }

    fn id(&self) -> u32 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }

    fn warn(&self, what: String) {
        if self.replay.borrow_mut().warned.insert(what.clone()) {
            eprintln!("kgl (wgpu): {what}");
        }
    }

    fn screen_samples(&self) -> i32 {
        if self.video.hdr_post_processing {
            1
        } else {
            self.video.aa_samples
        }
    }

    /// `initDX11`'s screen texture and `createDepthBuffer` 0x14001acc0.
    fn make_screen(&mut self) -> Result<(), String> {
        let samples = self.screen_samples();
        self.screen_depth_target = self.depth_target(self.video.width, self.video.height, samples);
        self.screen_depth_target.samples = -1;
        let screen = self.color_texture(wgpu::TextureFormat::Rgba8Unorm, self.video.width as u32, self.video.height as u32, 1, samples as u32, 1, "screen")?;
        let rtv = self.view(&screen, None, false, 0, 1, 0, 1);
        kgl_log::new_name(key(&rtv.0), "screen");
        self.screen_render_target = KglRenderTarget { texture: Some(Texture2D(screen)), render_target_view: Some(rtv), samples: -1, ..Default::default() };
        let mut b = self.bound.borrow_mut();
        b.rtv = self.screen_render_target.render_target_view.clone();
        b.clear_rtv = b.rtv.clone();
        b.dsv = self.screen_depth_target.depth_view.clone();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn color_texture(&self, format: wgpu::TextureFormat, width: u32, height: u32, layers: u32, samples: u32, mips: u32, label: &str) -> Result<Rc<Tex>, String> {
        if width == 0 || height == 0 {
            return Err(format!("{label}: an empty size"));
        }
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        if samples == 1 {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: layers },
            mip_level_count: mips,
            sample_count: physical_samples(samples),
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        Ok(Rc::new(Tex { texture, format, width, height, samples: physical_samples(samples), depth: false, written: Cell::new(0), copy: RefCell::new(None) }))
    }

    #[allow(clippy::too_many_arguments)]
    fn view(&self, tex: &Rc<Tex>, dimension: Option<wgpu::TextureViewDimension>, cube: bool, mip: u32, mips: u32, layer: u32, layers: u32) -> View {
        let view = tex.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension,
            base_mip_level: mip,
            mip_level_count: Some(mips),
            base_array_layer: layer,
            array_layer_count: Some(layers),
            ..Default::default()
        });
        View(Rc::new(ViewInner { tex: tex.clone(), view, cube }))
    }

    /// The depth branch of `KGLRenderTarget::KGLRenderTarget` 0x1400232d0: a 32-bit float depth
    /// texture that is also read by the shaders.
    fn depth_target(&self, width: i32, height: i32, samples: i32) -> KglRenderTarget {
        let limit = self.device.limits().max_texture_dimension_2d as i32;
        if width <= 0 || height <= 0 || width > limit || height > limit || samples < 1 {
            eprintln!("ERROR: CreateTexture2D depthStencilTexture failed");
            return KglRenderTarget { format: Format(39), width, height, samples: -1, ..Default::default() };
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: physical_samples(samples as u32),
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let tex = Rc::new(Tex { texture, format: wgpu::TextureFormat::Depth32Float, width: width as u32, height: height as u32, samples: physical_samples(samples as u32), depth: true, written: Cell::new(0), copy: RefCell::new(None) });
        let dsv = self.view(&tex, None, false, 0, 1, 0, 1);
        let srv = self.view(&tex, None, false, 0, 1, 0, 1);
        let name = kgl_log::new_name(key(&dsv.0), "depth");
        kgl_log::set_name(key(&srv.0), &name);
        KglRenderTarget { texture: Some(Texture2D(tex)), render_target_view: None, shader_resource_view: Some(srv), depth_view: Some(dsv), format: Format(39), width, height, samples: -1 }
    }

    /// `kglSetDefaultState` 0x140018920.
    pub fn set_default_state(&mut self) {
        kgl_log::line(format_args!("set_default_state"));
        let mut b = self.bound.borrow_mut();
        b.topology = 0;
        b.vs = None;
        b.ps = None;
        b.blend = 0;
        b.cull = 2;
        b.depth = 0;
        // the layout stays bound: only kgl's filter forgets it, as on Direct3D
    }

    /// `kglSetBlendState` 0x140018ff0.
    pub fn set_blend_state(&self, index: i32) {
        kgl_log::line(format_args!("set_blend_state {index}"));
        self.bound.borrow_mut().blend = index;
    }

    /// `kglSetDepthState` 0x140019020.
    pub fn set_depth_state(&self, index: i32) {
        kgl_log::line(format_args!("set_depth_state {index}"));
        self.bound.borrow_mut().depth = index;
    }

    /// `kglSetCullState` 0x140019050.
    pub fn set_cull_state(&self, index: i32) {
        kgl_log::line(format_args!("set_cull_state {index}"));
        self.bound.borrow_mut().cull = index;
    }

    /// `kglFlush` 0x140018df0: what is recorded goes to the device.
    pub fn flush(&self) {
        kgl_log::line(format_args!("flush"));
        self.submit();
    }

    /// `kglSetViewport` 0x140018ee0.
    pub fn set_viewport(&self, x: f32, y: f32, width: f32, height: f32) {
        kgl_log::line(format_args!("set_viewport {x} {y} {width} {height}"));
        self.bound.borrow_mut().viewport = [x, y, width, height];
    }

    /// `kglSetSamplerPS` 0x140018cf0.
    pub fn set_sampler_ps(&self, sampler: &Option<Sampler>, slot: u32) {
        kgl_log::line(format_args!("set_sampler_ps {} {slot}", log_name(sampler_key(sampler))));
        if let Some(s) = self.bound.borrow_mut().samplers.get_mut(slot as usize) {
            *s = sampler.clone();
        }
    }

    /// `kglCreateSampler` 0x140018be0. (Direct3D's mip LOD bias has no WebGPU field: it is
    /// left out.)
    pub fn create_sampler(&self, kind: i32, clamp: bool, max_anisotropy: i32, mip_lod_bias: f32) -> Option<Sampler> {
        let address = if clamp { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
        let (filter, mip) = match kind {
            2 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest),
            3 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest),
            _ => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear),
        };
        let anisotropy = if kind == 0 { max_anisotropy.clamp(1, 16) as u16 } else { 1 };
        let base = wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: address,
            address_mode_v: address,
            address_mode_w: address,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: mip,
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: anisotropy,
            border_color: None,
        };
        let plain = self.device.create_sampler(&base);
        let comparison = self.device.create_sampler(&wgpu::SamplerDescriptor { compare: Some(if kind == 3 { wgpu::CompareFunction::Less } else { wgpu::CompareFunction::Never }), anisotropy_clamp: 1, ..base });
        let sampler = Sampler(Rc::new(SamplerInner { plain, comparison }));
        let name = kgl_log::sampler_name(kind, clamp, max_anisotropy, mip_lod_bias);
        kgl_log::set_name(key(&sampler.0), &name);
        kgl_log::line(format_args!("create_sampler {kind} {clamp} {max_anisotropy} {mip_lod_bias} = {name}"));
        Some(sampler)
    }

    /// Direct3D 11 unbinds a resource from the shader slots when it becomes a target.
    fn unbind_targets(b: &mut Bound) {
        let targets: Vec<*const Tex> = [&b.rtv, &b.dsv].iter().filter_map(|v| v.as_ref().map(|v| Rc::as_ptr(&v.0.tex))).collect();
        for slot in b.textures.iter_mut() {
            if slot.as_ref().is_some_and(|v| targets.contains(&Rc::as_ptr(&v.0.tex))) {
                *slot = None;
            }
        }
    }

    /// `kglSetRenderTarget` 0x140018d20: keeps the depth view that is active.
    pub fn set_render_target(&mut self, target: Option<&KglRenderTarget>) {
        let rtv = target.and_then(|t| t.render_target_view.clone());
        kgl_log::line(format_args!("set_render_target {}", log_name(view_key(&rtv))));
        let mut b = self.bound.borrow_mut();
        b.clear_rtv = rtv.clone();
        b.rtv = rtv;
        Kgl::unbind_targets(&mut b);
    }

    /// `kglSetRenderTargets` 0x140018e40.
    pub fn set_render_targets(&mut self, target: Option<&KglRenderTarget>, depth: Option<&KglRenderTarget>) {
        let rtv = target.and_then(|t| t.render_target_view.clone());
        let dsv = depth.and_then(|d| d.depth_view.clone());
        kgl_log::line(format_args!("set_render_targets {} {}", log_name(view_key(&rtv)), log_name(view_key(&dsv))));
        let mut b = self.bound.borrow_mut();
        b.clear_rtv = rtv.clone();
        b.rtv = rtv;
        b.dsv = dsv;
        Kgl::unbind_targets(&mut b);
    }

    /// `kglSetRenderTargets(kglGetScreenRenderTarget(), kglGetScreenDepthTarget())`, or with no
    /// depth target.
    pub fn set_screen_render_targets(&mut self, with_depth: bool) {
        kgl_log::line(format_args!("set_screen_render_targets {with_depth}"));
        let dsv = if with_depth { self.screen_depth_target.depth_view.clone() } else { None };
        let mut b = self.bound.borrow_mut();
        b.rtv = self.screen_render_target.render_target_view.clone();
        b.clear_rtv = b.rtv.clone();
        b.dsv = dsv;
        Kgl::unbind_targets(&mut b);
    }

    /// `kglClearColor` 0x140018f80.
    pub fn clear_color(&self, rgba: &[f32; 4]) {
        kgl_log::line(format_args!("clear_color {rgba:?}"));
        let rtv = self.bound.borrow().clear_rtv.clone();
        if let Some(view) = rtv {
            self.clear(Some((&view, rgba)), None);
        }
    }

    /// `kglClearDepth` 0x140018fb0.
    pub fn clear_depth(&self, depth: f32) {
        kgl_log::line(format_args!("clear_depth {depth}"));
        let dsv = self.bound.borrow().dsv.clone();
        if let Some(view) = dsv {
            self.clear(None, Some((&view, depth)));
        }
    }

    /// `kglRenderTargetClear` 0x140018d90.
    pub fn render_target_clear(&self, target: &KglRenderTarget, rgba: &[f32; 4], depth: f32) {
        kgl_log::line(format_args!("render_target_clear {} {} {rgba:?} {depth}", log_name(view_key(&target.render_target_view)), log_name(view_key(&target.depth_view))));
        if let Some(view) = &target.render_target_view {
            self.clear(Some((view, rgba)), None);
        }
        if let Some(view) = &target.depth_view {
            self.clear(None, Some((view, depth)));
        }
    }

    /// `kglSetTexture` 0x140019380 / `kglSetTextureRT` 0x140018ec0: one pixel-shader slot. A
    /// view of a texture bound as a target is not bound, as on Direct3D 11.
    pub fn set_texture(&self, slot: u32, view: &Option<View>) {
        kgl_log::line(format_args!("set_texture {slot} {}", log_name(view_key(view))));
        let mut b = self.bound.borrow_mut();
        let is_target = |v: &View| [&b.rtv, &b.dsv].iter().any(|t| t.as_ref().is_some_and(|t| Rc::ptr_eq(&t.0.tex, &v.0.tex)));
        let view = view.clone().filter(|v| !is_target(v));
        if let Some(s) = b.textures.get_mut(slot as usize) {
            *s = view;
        }
    }

    /// `kglSetShader` 0x140019dc0.
    pub fn set_shader(&self, layout: &Option<InputLayout>, vs: &Option<VertexShader>, ps: &Option<PixelShader>) {
        kgl_log::line(format_args!(
            "set_shader {} {} {}",
            log_name(layout.as_ref().map(|l| key(&l.0))),
            log_name(vs.as_ref().map(|s| key(&s.0))),
            log_name(ps.as_ref().map(|s| key(&s.0)))
        ));
        let mut b = self.bound.borrow_mut();
        b.layout = layout.clone();
        b.vs = vs.clone();
        b.ps = ps.clone();
    }

    /// `KGLShader::createVertexShader`'s `CreateVertexShader`: the blob is kept, read.
    pub fn create_vertex_shader(&self, blob: &[u8]) -> Option<VertexShader> {
        let shader = rustyac_dxbc::parse(blob).ok().filter(|s| s.stage == rustyac_dxbc::Stage::Vertex)?;
        let vs = VertexShader(Rc::new(ShaderBlob { id: self.id(), shader }));
        let name = kgl_log::new_name(key(&vs.0), "vs");
        kgl_log::line(format_args!("create_vertex_shader {:016x} = {name}", kgl_log::hash(blob)));
        Some(vs)
    }

    pub fn create_pixel_shader(&self, blob: &[u8]) -> Option<PixelShader> {
        let shader = rustyac_dxbc::parse(blob).ok().filter(|s| s.stage == rustyac_dxbc::Stage::Pixel)?;
        let ps = PixelShader(Rc::new(ShaderBlob { id: self.id(), shader }));
        let name = kgl_log::new_name(key(&ps.0), "ps");
        kgl_log::line(format_args!("create_pixel_shader {:016x} = {name}", kgl_log::hash(blob)));
        Some(ps)
    }

    /// `getInputLayout`'s `CreateInputLayout`.
    pub fn create_input_layout(&self, elements: &[crate::shader::InputElement], _bytecode: &[u8]) -> Option<InputLayout> {
        let elements: Vec<_> = elements
            .iter()
            .map(|e| {
                let format = match e.format.0 {
                    2 => wgpu::VertexFormat::Float32x4,
                    6 => wgpu::VertexFormat::Float32x3,
                    16 => wgpu::VertexFormat::Float32x2,
                    28 => wgpu::VertexFormat::Unorm8x4,
                    _ => wgpu::VertexFormat::Float32,
                };
                (e.semantic.to_string(), e.index, format, e.offset)
            })
            .collect();
        let layout = InputLayout(Rc::new(LayoutInner { id: self.id(), elements }));
        let name = kgl_log::new_name(key(&layout.0), "layout");
        kgl_log::line(format_args!("create_input_layout {} = {name}", layout.0.elements.len()));
        Some(layout)
    }

    fn buffer(&self, gpu: Option<wgpu::Buffer>, bytes: Vec<u8>, size: usize) -> Buffer {
        Buffer(Rc::new(BufInner { gpu, bytes: RefCell::new(bytes), version: Cell::new(0), copy: Cell::new(None), size }))
    }

    /// `kglCreateCBuffer` 0x140019870.
    pub fn create_cbuffer(&self, size: i32) -> KglCBuffer {
        let buffer = self.buffer(None, vec![0; size.max(0) as usize], size.max(0) as usize);
        let name = kgl_log::new_name(key(&buffer.0), "cb");
        kgl_log::line(format_args!("create_cbuffer {size} = {name}"));
        KglCBuffer { buffer: Some(buffer), size }
    }

    /// `kglCBufferMap` 0x140019bb0: the whole buffer.
    pub fn cbuffer_map(&self, cbuffer: &KglCBuffer, data: &[u8]) {
        kgl_log::line(format_args!("cbuffer_map {} {:016x}", log_name(buffer_key(&cbuffer.buffer)), kgl_log::hash(&data[..data.len().min(cbuffer.size.max(0) as usize)])));
        if let Some(b) = &cbuffer.buffer {
            let mut bytes = b.0.bytes.borrow_mut();
            let n = bytes.len().min(data.len());
            bytes[..n].copy_from_slice(&data[..n]);
            b.0.version.set(b.0.version.get() + 1);
        }
    }

    /// `kglCBufferBind` 0x140019bf0.
    pub fn cbuffer_bind(&self, cbuffer: &KglCBuffer, slot: i32) {
        kgl_log::line(format_args!("cbuffer_bind {} {slot}", log_name(buffer_key(&cbuffer.buffer))));
        if let Some(s) = self.bound.borrow_mut().cbuffers.get_mut(slot as usize) {
            *s = cbuffer.buffer.clone().map(|b| (b, cbuffer.size.max(0) as usize));
        }
    }

    /// `kglCreateVertexBuffer` 0x140019fc0 / `kglCreateDynamicVertexBuffer` 0x14001a020.
    pub fn create_vertex_buffer(&self, bytes: &[u8], byte_size: usize, stride: u32, dynamic: bool) -> KglVertexBuffer {
        use wgpu::util::DeviceExt;
        let buffer = if byte_size == 0 {
            eprintln!("FAILED TO CREATE VERTEX BUFFER size:{byte_size} stride:{stride} dynamic:{}", dynamic as i32);
            None
        } else if dynamic {
            let mut data = vec![0u8; byte_size];
            let n = bytes.len().min(byte_size);
            data[..n].copy_from_slice(&bytes[..n]);
            Some(self.buffer(None, data, byte_size))
        } else {
            let mut data = bytes.to_vec();
            data.resize(byte_size, 0);
            let gpu = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vertices"), contents: &data, usage: wgpu::BufferUsages::VERTEX });
            Some(self.buffer(Some(gpu), Vec::new(), byte_size))
        };
        let name = match &buffer {
            Some(b) => kgl_log::new_name(key(&b.0), "vb"),
            None => "-".into(),
        };
        kgl_log::line(format_args!("create_vertex_buffer {byte_size} {stride} {dynamic} {:016x} = {name}", kgl_log::hash(bytes)));
        KglVertexBuffer { buffer, stride }
    }

    /// `kglCreateIndexBuffer` 0x14001a120.
    pub fn create_index_buffer(&self, indices: &[u16]) -> KglIndexBuffer {
        use wgpu::util::DeviceExt;
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let buffer = (!bytes.is_empty()).then(|| {
            let gpu = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("indices"), contents: &bytes, usage: wgpu::BufferUsages::INDEX });
            self.buffer(Some(gpu), Vec::new(), bytes.len())
        });
        let name = match &buffer {
            Some(b) => kgl_log::new_name(key(&b.0), "ib"),
            None => "-".into(),
        };
        kgl_log::line(format_args!("create_index_buffer {} {:016x} = {name}", indices.len(), kgl_log::hash(&bytes)));
        KglIndexBuffer { buffer }
    }

    /// `kglSetVertexBuffer` 0x14001a0d0.
    pub fn set_vertex_buffer(&self, vb: &KglVertexBuffer) {
        kgl_log::line(format_args!("set_vertex_buffer {} {}", log_name(buffer_key(&vb.buffer)), vb.stride));
        self.bound.borrow_mut().vb = vb.buffer.clone().map(|b| (b, vb.stride));
    }

    /// `kglVertexBufferMap` 0x14001a090: the whole content is new.
    pub fn vertex_buffer_map(&self, vb: &KglVertexBuffer, data: &[u8]) {
        kgl_log::line(format_args!("vertex_buffer_map {} {:016x}", log_name(buffer_key(&vb.buffer)), kgl_log::hash(data)));
        let Some(b) = &vb.buffer else {
            return;
        };
        let mut bytes = b.0.bytes.borrow_mut();
        let n = bytes.len().min(data.len());
        bytes[..n].copy_from_slice(&data[..n]);
        b.0.version.set(b.0.version.get() + 1);
    }

    /// `kglVertexBufferMapNoOverwrite` 0x14001a080: `length` bytes at `offset`.
    pub fn vertex_buffer_map_no_overwrite(&self, vb: &KglVertexBuffer, data: &[u8], offset: usize, length: usize) {
        kgl_log::line(format_args!("vertex_buffer_map_no_overwrite {} {offset} {length} {:016x}", log_name(buffer_key(&vb.buffer)), kgl_log::hash(&data[offset..offset + length])));
        let Some(b) = &vb.buffer else {
            return;
        };
        let mut bytes = b.0.bytes.borrow_mut();
        let end = (offset + length).min(bytes.len());
        if offset < end {
            bytes[offset..end].copy_from_slice(&data[offset..end]);
        }
        b.0.version.set(b.0.version.get() + 1);
    }

    /// `kglSetPrimitiveType` 0x140019340.
    pub fn set_primitive_type(&self, kind: i32) {
        kgl_log::line(format_args!("set_primitive_type {kind}"));
        if (0..=3).contains(&kind) {
            self.bound.borrow_mut().topology = kind;
        }
    }

    /// `kglDraw` 0x14001a1b0.
    pub fn draw(&self, vertex_count: i32, start_vertex: i32) {
        kgl_log::line(format_args!("draw {vertex_count} {start_vertex}"));
        self.record(Call::Plain(vertex_count as u32, start_vertex as u32));
    }

    /// `kglSetIndexBuffer` 0x14001a170.
    pub fn set_index_buffer(&self, ib: &KglIndexBuffer) {
        kgl_log::line(format_args!("set_index_buffer {}", log_name(buffer_key(&ib.buffer))));
        self.bound.borrow_mut().ib = ib.buffer.clone();
    }

    /// `kglDrawIndexed` 0x14001a190.
    pub fn draw_indexed(&self, index_count: i32, start_index: i32, base_vertex: i32) {
        kgl_log::line(format_args!("draw_indexed {index_count} {start_index} {base_vertex}"));
        self.record(Call::Indexed(index_count as u32, start_index as u32, base_vertex));
    }

    /// `kglCreateRenderTarget` 0x1400196e0.
    pub fn create_render_target(&self, format: Format, width: i32, height: i32, samples: i32, mips: i32) -> KglRenderTarget {
        let mut target = KglRenderTarget { format, width, height, samples, ..Default::default() };
        let made = texture_format(format).ok_or_else(|| format!("format {}", format.0)).and_then(|f| {
            let mips = if mips <= 0 { 32 - (width.max(height).max(1) as u32).leading_zeros() } else { mips as u32 };
            self.color_texture(f, width.max(0) as u32, height.max(0) as u32, 1, samples.max(1) as u32, mips, "render target")
        });
        match made {
            Ok(tex) => {
                let mips = tex.texture.mip_level_count();
                let rtv = self.view(&tex, None, false, 0, 1, 0, 1);
                let srv = self.view(&tex, None, false, 0, mips, 0, 1);
                let name = kgl_log::new_name(key(&rtv.0), "rt");
                kgl_log::set_name(key(&srv.0), &name);
                target.texture = Some(Texture2D(tex));
                target.render_target_view = Some(rtv);
                target.shader_resource_view = Some(srv);
            }
            Err(e) => self.warn(format!("no render target: {e}")),
        }
        kgl_log::line(format_args!("create_render_target {} {width} {height} {samples} {mips} = {}", format.0, log_name(view_key(&target.render_target_view))));
        target
    }

    /// `kglCreateRenderTargetDepth` 0x1400197a0.
    pub fn create_render_target_depth(&self, width: i32, height: i32, samples: i32) -> Result<KglRenderTarget, String> {
        let mut target = self.depth_target(width, height, samples);
        target.samples = samples;
        kgl_log::line(format_args!("create_render_target_depth {width} {height} {samples} = {}", log_name(view_key(&target.depth_view))));
        Ok(target)
    }

    /// `kglSwapBuffers` 0x140019080: here only the end of the frame's recording.
    pub fn swap_buffers(&self) {
        kgl_log::line(format_args!("swap_buffers"));
        self.submit();
    }

    /// The screen target's pixels, as RGBA rows (a multisampled screen resolved first).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_screen(&self) -> Result<(u32, u32, Vec<u8>), String> {
        kgl_log::line(format_args!("read_screen"));
        let view = self.screen_render_target.render_target_view.clone().ok_or("no screen target")?;
        let tex = view.0.tex.clone();
        let source = if tex.samples > 1 {
            let plain = self.color_texture(tex.format, tex.width, tex.height, 1, 1, 1, "resolved screen")?;
            let plain_view = self.view(&plain, None, false, 0, 1, 0, 1);
            self.resolve(&view, &plain_view);
            plain
        } else {
            tex
        };
        let pixels = self.read_texture(&source)?;
        Ok((source.width, source.height, pixels))
    }

    /// The screen's colour texture, for a caller that shows it (the page's canvas).
    pub fn screen_texture(&self) -> Option<&wgpu::Texture> {
        self.screen_render_target.texture.as_ref().map(|t| &t.0.texture)
    }
}

impl Kgl {
    /// `kglCreateCubeMap` 0x140018a30: a 16-bit depth texture, the cube, a view per face and the
    /// cube's shader view.
    pub fn create_cube_map(&self, size: i32, hdr: bool, mips: i32) -> KglCubeMap {
        let mut cube = KglCubeMap { texture: None, srv_cube: None, rtv_face: vec![None; 6], depth_texture: None, dsv_depth: None, size, mips };
        let format = if hdr { wgpu::TextureFormat::Rgba16Float } else { wgpu::TextureFormat::Rgba8Unorm };
        let levels = mips.max(1) as u32;
        if size > 0 && size as u32 <= self.device.limits().max_texture_dimension_2d {
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("cube depth"),
                size: wgpu::Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth16Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth = Rc::new(Tex { texture: depth, format: wgpu::TextureFormat::Depth16Unorm, width: size as u32, height: size as u32, samples: 1, depth: true, written: Cell::new(0), copy: RefCell::new(None) });
            cube.dsv_depth = Some(self.view(&depth, None, false, 0, 1, 0, 1));
            cube.depth_texture = Some(Texture2D(depth));
            if let Ok(tex) = self.color_texture(format, size as u32, size as u32, 6, 1, levels, "cube map") {
                for face in 0..6u32 {
                    cube.rtv_face[face as usize] = Some(self.view(&tex, Some(wgpu::TextureViewDimension::D2), false, 0, 1, face, 1));
                }
                cube.srv_cube = Some(self.view(&tex, Some(wgpu::TextureViewDimension::Cube), true, 0, levels, 0, 6));
                cube.texture = Some(Texture2D(tex));
            }
        }
        let name = match &cube.srv_cube {
            Some(v) => kgl_log::new_name(key(&v.0), "cube"),
            None => "-".into(),
        };
        for (face, v) in cube.rtv_face.iter().enumerate() {
            if let Some(v) = v {
                kgl_log::set_name(key(&v.0), &format!("{name}.{face}"));
            }
        }
        if let Some(v) = &cube.dsv_depth {
            kgl_log::set_name(key(&v.0), &format!("{name}.depth"));
        }
        kgl_log::line(format_args!("create_cube_map {size} {hdr} {mips} = {name}"));
        cube
    }

    /// `kglCubeMapBeginFace` 0x140018b30: the face and the cube's depth view are bound; only the
    /// depth view becomes kgl's active one (the colour clear that follows goes to the colour view
    /// that was active before, as in the game).
    pub fn cube_map_begin_face(&mut self, cube: &KglCubeMap, face: usize) {
        kgl_log::line(format_args!("cube_map_begin_face {} {face}", log_name(view_key(&cube.srv_cube))));
        {
            let mut b = self.bound.borrow_mut();
            b.rtv = cube.rtv_face.get(face).cloned().flatten();
            b.dsv = cube.dsv_depth.clone();
            Kgl::unbind_targets(&mut b);
        }
        self.set_viewport(0.0, 0.0, cube.size as f32, cube.size as f32);
    }

    /// The pixels of a colour render target of four bytes a pixel, rows from the top.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_render_target(&self, target: &KglRenderTarget) -> Result<Vec<u8>, String> {
        kgl_log::line(format_args!("read_render_target {}", log_name(view_key(&target.render_target_view))));
        let tex = target.texture.as_ref().ok_or("the render target has no texture")?;
        self.read_texture(&tex.0)
    }

    /// No reading back in the browser (it would have to wait for the GPU).
    #[cfg(target_arch = "wasm32")]
    pub fn read_render_target(&self, _target: &KglRenderTarget) -> Result<Vec<u8>, String> {
        Err("render targets are not read back in the browser".into())
    }

    /// `kglResolveRenderTarget` 0x140019650: the multisampled colour target into a plain one.
    pub fn resolve_render_target(&self, source: &KglRenderTarget, target: &KglRenderTarget, code: i32) {
        kgl_log::line(format_args!("resolve_render_target {} {} {code}", log_name(view_key(&source.render_target_view)), log_name(view_key(&target.render_target_view))));
        if let (Some(from), Some(to)) = (&source.render_target_view, &target.render_target_view) {
            self.resolve(from, to);
        }
    }

    /// `kglCubeMapGenerateMips` 0x140019310: each face's levels made from the one above, a
    /// bilinear sample at the centre of each 2x2 (the box filter of Direct3D's GenerateMips).
    pub fn cube_map_generate_mips(&self, cube: &KglCubeMap, face: i32) {
        kgl_log::line(format_args!("cube_map_generate_mips {} {face}", log_name(view_key(&cube.srv_cube))));
        if !(cube.mips > 1 && face == -1) {
            return;
        }
        let Some(tex) = &cube.texture else {
            return;
        };
        self.end_pass();
        let tex = &tex.0;
        let pipeline = self.blit_pipeline(tex.format);
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let mut replay = self.replay.borrow_mut();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        for layer in 0..6 {
            for level in 1..tex.texture.mip_level_count() {
                let one = |mip| tex.texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_mip_level: mip, mip_level_count: Some(1), base_array_layer: layer, array_layer_count: Some(1), ..Default::default() });
                let (src, dst) = (one(level - 1), one(level));
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("kgl mips"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &dst, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
    }

    /// `kglSetTextureCubeMap` 0x1400192c0.
    pub fn set_texture_cube_map(&self, cube: Option<&KglCubeMap>, slot: u32) {
        kgl_log::line(format_args!("set_texture_cube_map {} {slot}", log_name(cube.and_then(|c| view_key(&c.srv_cube)))));
        let view = cube.and_then(|c| c.srv_cube.clone());
        self.set_texture(slot, &view);
    }

    /// `kglResizeBuffers` 0x1400193d0 for a screen that is a texture.
    pub fn resize_screen(&mut self, width: i32, height: i32) -> Result<(), String> {
        kgl_log::line(format_args!("resize_screen {width} {height}"));
        self.end_pass();
        self.video.width = width;
        self.video.height = height;
        self.make_screen()?;
        self.set_screen_render_targets(true);
        Ok(())
    }

    /// A texture made from the bytes of its levels (`texture_fallback`'s upload): `levels` are
    /// each level's bytes with its row pitch.
    pub fn create_texture(&self, format: Format, width: u32, height: u32, levels: &[(Vec<u8>, u32)]) -> Result<View, String> {
        let f = texture_format(format).ok_or_else(|| format!("no wgpu format for DXGI format {}", format.0))?;
        if f.is_compressed() && !self.features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            return Err("the adapter has no block-compressed textures".into());
        }
        let (bw, bh) = f.block_dimensions();
        if !width.is_multiple_of(bw) || !height.is_multiple_of(bh) {
            return Err(format!("a block-compressed texture of {width}x{height}"));
        }
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("texture"),
            size,
            mip_level_count: levels.len().max(1) as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: f,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (data, pitch)) in levels.iter().enumerate() {
            let extent = size.mip_level_size(level as u32, wgpu::TextureDimension::D2).physical_size(f);
            let rows = extent.height / bh;
            if (*pitch as usize) * (rows as usize) > data.len() {
                return Err(format!("level {level} is short"));
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(*pitch), rows_per_image: Some(rows) },
                extent,
            );
        }
        let tex = Rc::new(Tex { texture, format: f, width, height, samples: 1, depth: false, written: Cell::new(0), copy: RefCell::new(None) });
        Ok(self.view(&tex, None, false, 0, levels.len().max(1) as u32, 0, 1))
    }

    /// FW1FontWrapper's states, saved and put back around its text (`FW1_RESTORESTATE`).
    pub(crate) fn save_bound(&self) -> Bound {
        self.bound.borrow().clone()
    }

    pub(crate) fn restore_bound(&self, bound: Bound) {
        *self.bound.borrow_mut() = bound;
    }

    pub(crate) fn viewport(&self) -> [f32; 4] {
        self.bound.borrow().viewport
    }

    /// A rectangle of a one-level texture made by [`Kgl::create_texture`] (`UpdateSubresource`
    /// of a box): `bytes` from its top left corner, rows of `pitch` bytes. The queue takes it
    /// before the draws recorded so far: only FW1's glyph sheets use it, which only add glyphs.
    pub(crate) fn update_texture(&self, view: &View, [left, top, right, bottom]: [u32; 4], bytes: &[u8], pitch: u32) {
        let t = &view.0.tex;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t.texture, mip_level: 0, origin: wgpu::Origin3d { x: left, y: top, z: 0 }, aspect: wgpu::TextureAspect::All },
            bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(bottom - top) },
            wgpu::Extent3d { width: right - left, height: bottom - top, depth_or_array_layers: 1 },
        );
    }

    /// The size and levels of a view's texture (`KglTexture::describe`).
    pub fn describe_view(view: &View) -> (u32, u32, u32) {
        let t = &view.0.tex;
        (t.width, t.height, t.texture.mip_level_count())
    }
}

// The replay
impl Kgl {
    fn encoder<'a>(device: &wgpu::Device, replay: &'a mut Replay) -> &'a mut wgpu::CommandEncoder {
        replay.encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("kgl") }))
    }

    /// Everything recorded goes to the queue.
    fn submit(&self) {
        self.end_pass();
        let encoder = self.replay.borrow_mut().encoder.take();
        if let Some(encoder) = encoder {
            self.queue.submit([encoder.finish()]);
        }
    }

    /// Takes a copy of a constant buffer's or dynamic vertex buffer's bytes into the upload
    /// buffer (once per map), `size` bytes at least.
    fn copy_in(replay: &mut Replay, buffer: &Buffer, size: usize, align: usize) -> (u64, u64) {
        let b = &buffer.0;
        let size = size.max(b.size).div_ceil(16) * 16;
        if let Some((epoch, version, offset, made)) = b.copy.get() {
            if epoch == replay.epoch && version == b.version.get() && made >= size as u64 {
                return (offset, size as u64);
            }
        }
        let at = replay.upload.len().div_ceil(align) * align;
        replay.upload.resize(at, 0);
        replay.upload.extend_from_slice(&b.bytes.borrow());
        replay.upload.resize(at + size, 0);
        b.copy.set(Some((replay.epoch, b.version.get(), at as u64, size as u64)));
        (at as u64, size as u64)
    }

    fn zeros(replay: &mut Replay, size: usize, align: usize) -> (u64, u64) {
        let at = replay.upload.len().div_ceil(align) * align;
        let size = size.max(16).div_ceil(16) * 16;
        replay.upload.resize(at + size, 0);
        (at as u64, size as u64)
    }

    /// `cover`: alpha to coverage on one sample, which WebGPU does not have: the pixel is kept
    /// where the alpha is at least a half, as Direct3D 11 does.
    fn program(&self, vs: &VertexShader, ps: &PixelShader, cover: bool) -> Option<Rc<Program>> {
        let k = (vs.0.id, ps.0.id | if cover { 1 << 31 } else { 0 });
        if let Some(p) = self.replay.borrow().programs.get(&k) {
            return p.clone();
        }
        let made = match rustyac_dxbc::translate_pair(&vs.0.shader, &ps.0.shader) {
            Ok(mut t) => {
                if cover && t.ps.contains("output.o0 =") {
                    t.ps = t.ps.replacen("    run();
    var output: Out;
", "    run();
    if (bitcast<vec4<f32>>(o0).w < 0.5) {
        discard;
    }
    var output: Out;
", 1);
                }
                let device = &self.device;
                let vis = |v: bool, p: bool| match (v, p) {
                    (true, true) => wgpu::ShaderStages::VERTEX_FRAGMENT,
                    (true, false) => wgpu::ShaderStages::VERTEX,
                    _ => wgpu::ShaderStages::FRAGMENT,
                };
                let cbs: Vec<_> = t
                    .cbuffers
                    .iter()
                    .map(|c| wgpu::BindGroupLayoutEntry { binding: c.slot, visibility: vis(c.vertex, c.pixel), ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None })
                    .collect();
                let filterable = self.features.contains(wgpu::Features::FLOAT32_FILTERABLE);
                let texs: Vec<_> = t
                    .textures
                    .iter()
                    .map(|u| wgpu::BindGroupLayoutEntry {
                        binding: u.slot,
                        visibility: vis(u.vertex, u.pixel),
                        ty: wgpu::BindingType::Texture {
                            sample_type: match (u.depth, u.return_type) {
                                (true, _) => wgpu::TextureSampleType::Depth,
                                (_, 3) => wgpu::TextureSampleType::Sint,
                                (_, 4) => wgpu::TextureSampleType::Uint,
                                _ => wgpu::TextureSampleType::Float { filterable: u.dimension != 4 && filterable },
                            },
                            view_dimension: if u.dimension == 6 { wgpu::TextureViewDimension::Cube } else { wgpu::TextureViewDimension::D2 },
                            multisampled: u.dimension == 4,
                        },
                        count: None,
                    })
                    .collect();
                let smps: Vec<_> = t
                    .samplers
                    .iter()
                    .map(|s| wgpu::BindGroupLayoutEntry {
                        binding: s.slot,
                        visibility: vis(s.vertex, s.pixel),
                        ty: wgpu::BindingType::Sampler(if s.comparison {
                            wgpu::SamplerBindingType::Comparison
                        } else if filterable {
                            wgpu::SamplerBindingType::Filtering
                        } else {
                            wgpu::SamplerBindingType::NonFiltering
                        }),
                        count: None,
                    })
                    .collect();
                let groups = [&cbs, &texs, &smps].map(|e| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: e }));
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&groups[0]), Some(&groups[1]), Some(&groups[2])], immediate_size: 0 });
                let vsm = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(t.vs.as_str().into()) });
                let psm = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ps"), source: wgpu::ShaderSource::Wgsl(t.ps.as_str().into()) });
                Some(Rc::new(Program { vs: vsm, ps: psm, t, groups, layout }))
            }
            Err(e) => {
                self.warn(format!("shader pair vs{} ps{} not translated: {e}", vs.0.id, ps.0.id));
                None
            }
        };
        self.replay.borrow_mut().programs.insert(k, made.clone());
        made
    }

    fn pipeline(&self, key: &PipelineKey, program: &Program, layout: &LayoutInner) -> Option<Rc<wgpu::RenderPipeline>> {
        if let Some(p) = self.replay.borrow().pipelines.get(key) {
            return p.clone();
        }
        let mut attributes = Vec::new();
        for (name, index, register) in &program.t.vertex_inputs {
            if name.starts_with("SV_") {
                continue;
            }
            match layout.elements.iter().find(|e| e.0.eq_ignore_ascii_case(name) && e.1 == *index) {
                Some(e) => attributes.push(wgpu::VertexAttribute { format: e.2, offset: e.3 as u64, shader_location: *register }),
                None => {
                    self.warn(format!("the input layout has no {name}{index}"));
                    self.replay.borrow_mut().pipelines.insert(key.clone(), None);
                    return None;
                }
            }
        }
        let topology = match key.topology {
            1 => wgpu::PrimitiveTopology::LineList,
            2 => wgpu::PrimitiveTopology::LineStrip,
            3 => wgpu::PrimitiveTopology::TriangleStrip,
            _ => wgpu::PrimitiveTopology::TriangleList,
        };
        let lines = matches!(key.topology, 1 | 2);
        let strip = matches!(key.topology, 2 | 3);
        let cull_mode = match key.cull {
            0 | 5 => Some(wgpu::Face::Front),
            1 => Some(wgpu::Face::Back),
            _ => None,
        };
        let polygon_mode = if key.cull == 4 && self.features.contains(wgpu::Features::POLYGON_MODE_LINE) { wgpu::PolygonMode::Line } else { wgpu::PolygonMode::Fill };
        let bias = if key.cull == 3 && !lines { wgpu::DepthBiasState { constant: -100, slope_scale: 0.0, clamp: f32::from_bits(0x3dcc_cccd) } } else { Default::default() };
        let depth_stencil = key.depth_format.map(|format| {
            let (write, compare) = match key.depth {
                0 => (true, wgpu::CompareFunction::Less),
                3 => (true, wgpu::CompareFunction::LessEqual),
                1 => (false, wgpu::CompareFunction::LessEqual),
                _ => (false, wgpu::CompareFunction::Always),
            };
            wgpu::DepthStencilState { format, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil: Default::default(), bias }
        });
        let blendable = |f: wgpu::TextureFormat| !matches!(f, wgpu::TextureFormat::R32Float | wgpu::TextureFormat::Rg32Float | wgpu::TextureFormat::Rgba32Float) || self.features.contains(wgpu::Features::FLOAT32_BLENDABLE);
        let targets: Vec<Option<wgpu::ColorTargetState>> = key
            .color
            .iter()
            .map(|&format| {
                let blend = matches!(key.blend, 1 | 3) && blendable(format);
                let add = |src_factor, dst_factor| wgpu::BlendComponent { src_factor, dst_factor, operation: wgpu::BlendOperation::Add };
                let fw1 = (key.blend == FW1_BLEND && blendable(format)).then_some(wgpu::BlendState { color: add(wgpu::BlendFactor::One, wgpu::BlendFactor::OneMinusSrcAlpha), alpha: add(wgpu::BlendFactor::One, wgpu::BlendFactor::Zero) });
                Some(wgpu::ColorTargetState {
                    format,
                    blend: blend
                        .then_some(wgpu::BlendState {
                            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max },
                        })
                        .or(fw1),
                    write_mask: if key.blend == 3 { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL },
                })
            })
            .collect();
        let a2c = key.blend == 2 && key.samples > 1 && !EXPERIMENT_NO_ALPHA_TO_COVERAGE.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kgl"),
            layout: Some(&program.layout),
            vertex: wgpu::VertexState {
                module: &program.vs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: key.stride as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes })],
            },
            primitive: wgpu::PrimitiveState {
                topology,
                strip_index_format: (strip && key.indexed).then_some(wgpu::IndexFormat::Uint16),
                front_face: wgpu::FrontFace::Cw,
                cull_mode,
                unclipped_depth: false,
                polygon_mode,
                conservative: false,
            },
            depth_stencil,
            multisample: wgpu::MultisampleState { count: key.samples, mask: !0, alpha_to_coverage_enabled: a2c },
            fragment: Some(wgpu::FragmentState { module: &program.ps, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: None,
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(e) = block_on(scope.pop()) {
            self.warn(format!("pipeline not made: {e}"));
            self.replay.borrow_mut().pipelines.insert(key.clone(), None);
            return None;
        }
        let pipeline = Some(Rc::new(pipeline));
        self.replay.borrow_mut().pipelines.insert(key.clone(), pipeline.clone());
        pipeline
    }

    /// The view a shader slot reads, as Direct3D 11 would give it.
    fn slot_view(&self, slot: u32, dimension: u32, depth: bool, bound: Option<&View>) -> wgpu::TextureView {
        let Some(v) = bound else {
            return match (depth, dimension) {
                (true, _) => self.dummy_depth.clone(),
                (_, 4) => self.dummy_ms.clone(),
                (_, 6) => self.dummy_cube.clone(),
                _ => self.dummy_2d.clone(),
            };
        };
        let t = &v.0.tex;
        let fits = match (depth, dimension) {
            (true, _) => t.depth && t.samples == 1 && !v.0.cube,
            (_, 4) => t.samples > 1,
            (_, 6) => v.0.cube,
            _ => t.samples == 1 && !v.0.cube,
        };
        if !fits {
            self.warn(format!("slot {slot} (dimension {dimension}, depth {depth}) gets a view that does not fit it"));
            return self.slot_view(slot, dimension, depth, None);
        }
        if t.depth && !depth && t.samples == 1 {
            return self.depth_as_float(t);
        }
        v.0.view.clone()
    }

    /// A depth texture read as R32F: a copy, made again when the depth was written since.
    fn depth_as_float(&self, t: &Rc<Tex>) -> wgpu::TextureView {
        if let Some((at, _, view)) = &*t.copy.borrow() {
            if *at == t.written.get() {
                return view.clone();
            }
        }
        let made = t.copy.borrow().as_ref().map(|(_, tex, _)| tex.clone());
        let target = made.unwrap_or_else(|| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth as R32F"),
                size: wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let view = target.create_view(&Default::default());
        let pipeline = {
            let mut replay = self.replay.borrow_mut();
            replay
                .depth_copy
                .get_or_insert_with(|| {
                    let source = format!(
                        "{FULLSCREEN_VS}
                        @group(0) @binding(0) var src: texture_depth_2d;
                        @fragment fn fs(v: V) -> @location(0) vec4<f32> {{
                            return vec4<f32>(textureLoad(src, vec2<i32>(v.p.xy), 0), 0.0, 0.0, 1.0);
                        }}"
                    );
                    let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("kgl depth copy"), source: wgpu::ShaderSource::Wgsl(source.into()) });
                    self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("kgl depth copy"),
                        layout: None,
                        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::R32Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    })
                })
                .clone()
        };
        let source = t.texture.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipeline.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source) }] });
        // the copy goes before the pass being recorded (which cannot write this depth texture:
        // a target is never bound to a slot)
        let mut replay = self.replay.borrow_mut();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kgl depth copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        *t.copy.borrow_mut() = Some((t.written.get(), target, view.clone()));
        view
    }

    fn blit_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut replay = self.replay.borrow_mut();
        replay
            .blits
            .entry(format)
            .or_insert_with(|| {
                let source = format!(
                    "{FULLSCREEN_VS}
                    @group(0) @binding(0) var src: texture_2d<f32>;
                    @group(0) @binding(1) var smp: sampler;
                    @fragment fn fs(v: V) -> @location(0) vec4<f32> {{ return textureSampleLevel(src, smp, v.uv, 0.0); }}"
                );
                let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("kgl blit"), source: wgpu::ShaderSource::Wgsl(source.into()) });
                self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("kgl blit"),
                    layout: None,
                    vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone()
    }

    /// Draws the screen texture into `target` (the page's canvas), scaled to its size.
    pub fn present_to(&self, target: &wgpu::TextureView, format: wgpu::TextureFormat) {
        self.end_pass();
        let Some(screen) = self.screen_render_target.render_target_view.clone() else {
            return;
        };
        let source = if screen.0.tex.samples > 1 {
            let Ok(plain) = self.color_texture(screen.0.tex.format, screen.0.tex.width, screen.0.tex.height, 1, 1, 1, "resolved screen") else {
                return;
            };
            let view = self.view(&plain, None, false, 0, 1, 0, 1);
            self.resolve(&screen, &view);
            view.0.view.clone()
        } else {
            screen.0.view.clone()
        };
        let pipeline = self.blit_pipeline(format);
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
        });
        let mut replay = self.replay.borrow_mut();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kgl present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// One draw: a copy of everything bound, added to the pass of the bound targets.
    fn record(&self, call: Call) {
        let b = self.bound.borrow();
        let (Some(vs), Some(ps)) = (b.vs.clone(), b.ps.clone()) else {
            return;
        };
        let (rtv, mut dsv) = (b.rtv.clone(), b.dsv.clone());
        if rtv.is_none() && dsv.is_none() {
            return;
        }
        let samples = rtv.as_ref().or(dsv.as_ref()).map_or(1, |v| v.0.tex.samples);
        if let (Some(c), Some(d)) = (&rtv, &dsv) {
            if c.0.tex.samples != d.0.tex.samples || c.0.tex.width != d.0.tex.width || c.0.tex.height != d.0.tex.height {
                drop(b);
                self.warn("a colour and a depth target of different sizes or samples".into());
                return;
            }
        }
        let Some(layout) = b.layout.clone() else {
            return;
        };
        let Some((vb, stride)) = b.vb.clone() else {
            return;
        };
        // target views are always mip 0, so the texture's size is the view's
        let (width, height) = rtv.as_ref().or(dsv.as_ref()).map_or((1, 1), |v| (v.0.tex.width, v.0.tex.height));
        let viewport = b.viewport;
        let topology = b.topology;
        let blend = b.blend;
        let depth = b.depth;
        let cull = b.cull;
        let textures = b.textures.clone();
        let samplers = b.samplers.clone();
        let cbuffers = b.cbuffers.clone();
        let ib = b.ib.clone();
        drop(b);
        let cover = blend == 2 && samples == 1 && !EXPERIMENT_NO_ALPHA_TO_COVERAGE.load(std::sync::atomic::Ordering::Relaxed);
        let Some(program) = self.program(&vs, &ps, cover) else {
            return;
        };
        // a pixel shader that writes depth with no depth target bound: Direct3D 11 drops the
        // depth, WebGPU wants an attachment, so it gets one of its own with the test off
        let mut depth = depth;
        if dsv.is_none() && program.t.ps.contains("frag_depth") {
            if let Some(c) = &rtv {
                dsv = Some(self.scratch_depth(c.0.tex.width, c.0.tex.height, c.0.tex.samples));
                depth = 2;
            }
        }
        let indexed = matches!(call, Call::Indexed(..));
        let key = PipelineKey {
            program: (vs.0.id, ps.0.id),
            layout: layout.0.id,
            stride,
            blend,
            depth,
            cull,
            topology,
            indexed,
            color: rtv.as_ref().map(|v| v.0.tex.format),
            depth_format: dsv.as_ref().map(|v| v.0.tex.format),
            samples,
        };
        let Some(pipeline) = self.pipeline(&key, &program, &layout.0) else {
            return;
        };
        // the viewport inside the target, as WebGPU wants it
        let [x, y, w, h] = viewport;
        let (x0, y0) = (x.max(0.0), y.max(0.0));
        let (x1, y1) = ((x + w).min(width as f32), (y + h).min(height as f32));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        if (x0, y0, x1, y1) != (x, y, x + w, y + h) {
            self.warn(format!("a viewport {viewport:?} outside its {width}x{height} target is cut"));
        }
        // the views, which may make depth copies: before the pass is touched
        let tex_views: Vec<(u32, wgpu::TextureView)> = program.t.textures.iter().map(|u| (u.slot, self.slot_view(u.slot, u.dimension, u.depth, textures.get(u.slot as usize).and_then(|v| v.as_ref())))).collect();
        let smp: Vec<(u32, wgpu::Sampler)> = program
            .t
            .samplers
            .iter()
            .map(|s| {
                let bound = samplers.get(s.slot as usize).and_then(|v| v.clone()).unwrap_or_else(|| self.default_sampler.clone());
                (s.slot, if s.comparison { bound.0.comparison.clone() } else { bound.0.plain.clone() })
            })
            .collect();
        // a draw past the end of its buffers is left out (Direct3D reads zeros there)
        match call {
            Call::Plain(count, start) if stride > 0 && (start as u64 + count as u64) * stride as u64 > vb.0.size as u64 => {
                self.warn("a draw past the end of its vertex buffer is left out".into());
                return;
            }
            Call::Indexed(count, start, _) if ib.as_ref().is_none_or(|ib| (start as u64 + count as u64) * 2 > ib.0.size as u64) => {
                self.warn("an indexed draw past the end of its index buffer (or with none) is left out".into());
                return;
            }
            _ => {}
        }
        let mut replay = self.replay.borrow_mut();
        // a pass on other targets ends here
        let same = replay.pass.as_ref().is_some_and(|p| same_view(&p.color, &rtv) && same_view(&p.depth, &dsv));
        if !same {
            drop(replay);
            self.end_pass();
            replay = self.replay.borrow_mut();
            replay.pass = Some(Pass { color: rtv.clone(), depth: dsv.clone(), draws: Vec::new() });
        }
        let align = self.device.limits().min_uniform_buffer_offset_alignment as usize;
        let cbs: Vec<(u32, u64, u64)> = program
            .t
            .cbuffers
            .iter()
            .map(|c| {
                let (offset, size) = match cbuffers.get(c.slot as usize).and_then(|v| v.as_ref()) {
                    Some((buffer, _)) => Kgl::copy_in(&mut replay, buffer, c.size as usize, align),
                    None => Kgl::zeros(&mut replay, c.size as usize, align),
                };
                (c.slot, offset, size)
            })
            .collect();
        let vb_source = match &vb.0.gpu {
            Some(gpu) => (Some(gpu.clone()), 0),
            None => (None, Kgl::copy_in(&mut replay, &vb, vb.0.size, 256).0),
        };
        let draw = Draw {
            pipeline,
            program,
            cbuffers: cbs,
            textures: tex_views,
            samplers: smp,
            vb: Some(vb_source),
            ib: ib.and_then(|b| b.0.gpu.clone()),
            viewport: [x0, y0, x1 - x0, y1 - y0],
            call,
        };
        replay.pass.as_mut().unwrap().draws.push(draw);
    }

    /// A depth target no kgl call made (it is not in the kgl log).
    fn scratch_depth(&self, width: u32, height: u32, samples: u32) -> View {
        self.scratch_depths
            .borrow_mut()
            .entry((width, height, samples))
            .or_insert_with(|| {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("kgl scratch depth"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let tex = Rc::new(Tex { texture, format: wgpu::TextureFormat::Depth32Float, width, height, samples, depth: true, written: Cell::new(0), copy: RefCell::new(None) });
                self.view(&tex, None, false, 0, 1, 0, 1)
            })
            .clone()
    }

    /// Encodes the pass being recorded.
    fn end_pass(&self) {
        let mut replay = self.replay.borrow_mut();
        let Some(pass) = replay.pass.take() else {
            return;
        };
        let upload = std::mem::take(&mut replay.upload);
        replay.epoch += 1;
        if pass.draws.is_empty() {
            return;
        }
        use wgpu::util::DeviceExt;
        let buffer = (!upload.is_empty()).then(|| self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("kgl upload"), contents: &upload, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::VERTEX }));
        let groups: Vec<[wgpu::BindGroup; 3]> = pass
            .draws
            .iter()
            .map(|d| {
                let cb: Vec<wgpu::BindGroupEntry> = d
                    .cbuffers
                    .iter()
                    .map(|(slot, offset, size)| wgpu::BindGroupEntry {
                        binding: *slot,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: buffer.as_ref().unwrap(), offset: *offset, size: wgpu::BufferSize::new(*size) }),
                    })
                    .collect();
                let tx: Vec<wgpu::BindGroupEntry> = d.textures.iter().map(|(slot, v)| wgpu::BindGroupEntry { binding: *slot, resource: wgpu::BindingResource::TextureView(v) }).collect();
                let sm: Vec<wgpu::BindGroupEntry> = d.samplers.iter().map(|(slot, s)| wgpu::BindGroupEntry { binding: *slot, resource: wgpu::BindingResource::Sampler(s) }).collect();
                [cb, tx, sm].iter().enumerate().map(|(i, e)| self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &d.program.groups[i], entries: e })).collect::<Vec<_>>().try_into().unwrap()
            })
            .collect();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        let color = pass.color.as_ref().map(|v| wgpu::RenderPassColorAttachment { view: &v.0.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } });
        let depth = pass.depth.as_ref().map(|v| wgpu::RenderPassDepthStencilAttachment { view: &v.0.view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("kgl pass"), color_attachments: &color.into_iter().map(Some).collect::<Vec<_>>(), depth_stencil_attachment: depth, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            for (d, g) in pass.draws.iter().zip(&groups) {
                rp.set_pipeline(&d.pipeline);
                for (i, group) in g.iter().enumerate() {
                    rp.set_bind_group(i as u32, group, &[]);
                }
                if let Some((gpu, offset)) = &d.vb {
                    match gpu {
                        Some(b) => rp.set_vertex_buffer(0, b.slice(..)),
                        None => rp.set_vertex_buffer(0, buffer.as_ref().unwrap().slice(*offset..)),
                    }
                }
                let [x, y, w, h] = d.viewport;
                rp.set_viewport(x, y, w, h, 0.0, 1.0);
                match d.call {
                    Call::Plain(count, start) => rp.draw(start..start + count, 0..1),
                    Call::Indexed(count, start, base) => {
                        if let Some(ib) = &d.ib {
                            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                            rp.draw_indexed(start..start + count, base, 0..1);
                        }
                    }
                }
            }
        }
        if let Some(v) = &pass.depth {
            v.0.tex.written.set(v.0.tex.written.get() + 1);
        }
    }

    /// A clear, as its own pass at its place among the passes.
    fn clear(&self, color: Option<(&View, &[f32; 4])>, depth: Option<(&View, f32)>) {
        self.end_pass();
        let mut replay = self.replay.borrow_mut();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        let c = color.map(|(v, rgba)| wgpu::RenderPassColorAttachment {
            view: &v.0.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: rgba[0] as f64, g: rgba[1] as f64, b: rgba[2] as f64, a: rgba[3] as f64 }), store: wgpu::StoreOp::Store },
        });
        let d = depth.map(|(v, z)| wgpu::RenderPassDepthStencilAttachment { view: &v.0.view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(z), store: wgpu::StoreOp::Store }), stencil_ops: None });
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("kgl clear"), color_attachments: &c.into_iter().map(Some).collect::<Vec<_>>(), depth_stencil_attachment: d, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None }));
        if let Some((v, _)) = depth {
            v.0.tex.written.set(v.0.tex.written.get() + 1);
        }
    }

    /// A multisampled colour view into a plain one of the same format (a pass that only
    /// resolves).
    fn resolve(&self, from: &View, to: &View) {
        if from.0.tex.samples == 1 || to.0.tex.samples != 1 || from.0.tex.format != to.0.tex.format {
            self.warn("a resolve between targets that do not fit".into());
            return;
        }
        self.end_pass();
        let mut replay = self.replay.borrow_mut();
        let encoder = Kgl::encoder(&self.device, &mut replay);
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kgl resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &from.0.view, depth_slice: None, resolve_target: Some(&to.0.view), ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
    }

    /// Level 0 of a texture of four bytes a pixel, read back (waits for the device).
    #[cfg(not(target_arch = "wasm32"))]
    fn read_texture(&self, tex: &Tex) -> Result<Vec<u8>, String> {
        let bpp = tex.format.block_copy_size(None).ok_or("a texture that cannot be read")?;
        let row = tex.width * bpp;
        let padded = row.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("kgl read"), size: (padded * tex.height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        self.end_pass();
        {
            let mut replay = self.replay.borrow_mut();
            let encoder = Kgl::encoder(&self.device, &mut replay);
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &tex.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: None } },
                wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
            );
        }
        self.submit();
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("wgpu poll: {e}"))?;
        let data = buffer.slice(..).get_mapped_range().map_err(|e| format!("wgpu map: {e}"))?.to_vec();
        buffer.unmap();
        let mut pixels = Vec::with_capacity((row * tex.height) as usize);
        for r in 0..tex.height as usize {
            pixels.extend_from_slice(&data[r * padded as usize..r * padded as usize + row as usize]);
        }
        Ok(pixels)
    }
}

fn same_view(a: &Option<View>, b: &Option<View>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(&a.0, &b.0),
        _ => false,
    }
}
