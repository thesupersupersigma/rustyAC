// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's picture: a small `wgpu` renderer (WebGPU where the browser has it, WebGL2
//! otherwise) that draws what the desktop's debug view draws: the track's kn5 models with
//! their textures and the multilayer ground mix, the car's kn5 model with its wheels, hubs
//! and steering wheel where the physics has them, one sun, distance fog, the car's shadow
//! patch. AC's own shaders (`.fxo`, Direct3D bytecode) are not used here.
//!
//! Every draw's constants go into one uniform buffer, written once a frame; a draw picks its
//! slice with a dynamic offset. A material is one bind group of seven textures and a sampler.

use std::collections::HashMap;
use std::path::PathBuf;

use rustyac_content::kn5::Vertex;
use rustyac_game::render::dds::{self, Image};
use rustyac_game::render::scene::{cube, mul, mul_precise, perspective_reversed, point, rotate_pitch, scale_then, translation, view_matrix, CameraFrame, CarShape};
use rustyac_game::view::{CarView, Mat, IDENTITY};
use wgpu::util::DeviceExt;

use crate::model::{frustum, sphere_visible, Material, Model, ModelOptions, Placement, Sink};

/// The horizon: the fog's colour and what the picture is cleared to.
const HORIZON: [f32; 4] = [0.52, 0.62, 0.74, 1.0];
/// The far plane, metres.
const FAR: f32 = 4000.0;
/// The sun: the direction its light travels (from the left front, high), and the ambient share.
const LIGHT: [f32; 4] = [-0.35, -0.80, -0.48, 0.42];
/// Fog density per metre on a track.
const FOG: f32 = 0.00022;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// One draw's constants, as `shader.wgsl` reads them.
#[repr(C)]
#[derive(Clone, Copy)]
struct DrawConstants {
    world: Mat,
    world_view: Mat,
    proj: Mat,
    color: [f32; 4],
    light: [f32; 4],
    camera: [f32; 4],
    fog: [f32; 4],
    params: [f32; 4],
    mult_rg: [f32; 4],
    mult_ba: [f32; 4],
    layer: [f32; 4],
    layer2: [f32; 4],
}

const DRAW_BYTES: usize = std::mem::size_of::<DrawConstants>();

fn bytes_of<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: `T` is one of this file's `repr(C)` structs of f32 (no padding), or kn5's `Vertex`.
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

fn slice_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: as `bytes_of`.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values)) }
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
}

/// A loaded model with its materials' bind groups.
struct Scene {
    model: Model,
    groups: Vec<wgpu::BindGroup>,
}

/// The car's model and the nodes the physics moves.
struct CarModel {
    scene: Scene,
    /// `WHEEL_LF` ... (turn with the wheel) and `SUSP_LF` ... (follow the hub)
    wheels: [Option<usize>; 4],
    hubs: [Option<usize>; 4],
    /// `STEER_HR`, the steering wheel, and its matrix at rest
    steer: Option<(usize, Mat)>,
}

/// Which pipeline a draw uses: the material's blend mode (0 none, 1 alpha blend, 2 alpha to
/// coverage) and depth mode (0 test and write, 1 test only, 2 none).
type PipelineKey = (u8, u8);

struct DrawCall {
    pipeline: PipelineKey,
    /// `None`: the solid pipeline (the shadow, the boxes).
    group: Option<(bool, usize)>,
    mesh: usize,
    index_count: u32,
    offset: u32,
}

/// What a frame drew.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawStats {
    pub meshes: u32,
    pub triangles: u64,
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// "WebGPU" or "WebGL2", and the adapter's name.
    pub backend: String,
    /// The card takes block-compressed textures as they are.
    pub compressed_textures: bool,
    pub samples: u32,
    max_texture_size: u32,
    color: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    shader: wgpu::ShaderModule,
    draw_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    solid: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    uniform_capacity: usize,
    uniform_stride: usize,
    sampler: wgpu::Sampler,
    white: usize,
    textures: Vec<wgpu::TextureView>,
    meshes: Vec<GpuMesh>,
    cube: usize,
    track: Option<Scene>,
    car: Option<CarModel>,
    scratch: Vec<u8>,
    calls: Vec<DrawCall>,
}

fn texture_format(format: u32) -> Option<(wgpu::TextureFormat, bool)> {
    Some(match format {
        dds::R8G8B8A8_UNORM => (wgpu::TextureFormat::Rgba8Unorm, false),
        dds::B8G8R8A8_UNORM => (wgpu::TextureFormat::Bgra8Unorm, false),
        dds::BC1_UNORM => (wgpu::TextureFormat::Bc1RgbaUnorm, true),
        dds::BC2_UNORM => (wgpu::TextureFormat::Bc2RgbaUnorm, true),
        dds::BC3_UNORM => (wgpu::TextureFormat::Bc3RgbaUnorm, true),
        dds::BC4_UNORM => (wgpu::TextureFormat::Bc4RUnorm, true),
        dds::BC5_UNORM => (wgpu::TextureFormat::Bc5RgUnorm, true),
        dds::BC7_UNORM => (wgpu::TextureFormat::Bc7RgbaUnorm, true),
        _ => return None,
    })
}

impl Sink for Gpu {
    fn texture(&mut self, image: &Image) -> Result<usize, String> {
        let (format, compressed) = texture_format(image.format).ok_or_else(|| format!("DXGI format {} is not drawn", image.format))?;
        if compressed && !self.compressed_textures {
            return Err("this device takes no block-compressed textures".to_string());
        }
        let top = &image.levels[0];
        if top.width.max(top.height) > self.max_texture_size {
            return Err(format!("{} x {} is larger than this device's textures ({})", top.width, top.height, self.max_texture_size));
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: top.width, height: top.height, depth_or_array_layers: 1 },
            mip_level_count: image.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, mip) in image.levels.iter().enumerate() {
            // a compressed level is copied in whole blocks
            let (width, height) = if compressed { (mip.width.div_ceil(4) * 4, mip.height.div_ceil(4) * 4) } else { (mip.width, mip.height) };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &image.data[mip.bytes.clone()],
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(mip.pitch), rows_per_image: None },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
        }
        self.textures.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        Ok(self.textures.len() - 1)
    }

    fn mesh(&mut self, vertices: &[Vertex], indices: &[u16]) -> Result<usize, String> {
        // (an index buffer's size is a multiple of four bytes on the web)
        let mut index_bytes = slice_bytes(indices).to_vec();
        index_bytes.resize(index_bytes.len().next_multiple_of(4), 0);
        self.meshes.push(GpuMesh {
            vertices: self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: slice_bytes(vertices), usage: wgpu::BufferUsages::VERTEX }),
            indices: self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &index_bytes, usage: wgpu::BufferUsages::INDEX }),
        });
        Ok(self.meshes.len() - 1)
    }
}

fn depth_state(mode: u8) -> wgpu::DepthStencilState {
    // the depth runs from 1 at the near plane to 0 at the far one
    let (compare, write) = match mode {
        // AC's `eDepthNormal`: the first drawn stays
        0 => (wgpu::CompareFunction::Greater, true),
        1 => (wgpu::CompareFunction::GreaterEqual, false),
        _ => (wgpu::CompareFunction::Always, false),
    };
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

impl Gpu {
    /// Opens the picture on a canvas. `prefer_webgl`: do not use WebGPU even where it is there.
    pub async fn new(canvas: web_sys::HtmlCanvasElement, prefer_webgl: bool, samples: u32) -> Result<Gpu, String> {
        let (width, height) = (canvas.width().max(16), canvas.height().max(16));
        let mut attempts = Vec::new();
        if !prefer_webgl {
            attempts.push(wgpu::Backends::BROWSER_WEBGPU);
        }
        attempts.push(wgpu::Backends::GL);
        let mut opened = None;
        let mut errors = Vec::new();
        for backends in attempts {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = backends;
            let instance = wgpu::Instance::new(descriptor);
            let surface = match instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone())) {
                Ok(surface) => surface,
                Err(e) => {
                    errors.push(format!("{backends:?}: {e}"));
                    continue;
                }
            };
            let options = wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), ..Default::default() };
            match instance.request_adapter(&options).await {
                Ok(adapter) => {
                    opened = Some((surface, adapter, backends));
                    break;
                }
                Err(e) => errors.push(format!("{backends:?}: {e}")),
            }
        }
        let Some((surface, adapter, backends)) = opened else {
            return Err(format!("this browser gives neither WebGPU nor WebGL2 ({})", errors.join("; ")));
        };
        let info = adapter.get_info();
        let compressed_textures = adapter.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
        limits.max_texture_dimension_2d = adapter.limits().max_texture_dimension_2d.min(8192);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: if compressed_textures { wgpu::Features::TEXTURE_COMPRESSION_BC } else { wgpu::Features::empty() },
                required_limits: limits.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("the graphics device could not be opened: {e}"))?;
        let caps = surface.get_capabilities(&adapter);
        // the colours are written as they are (no sRGB step), as the debug view does
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied()).ok_or("the canvas offers no pixel format")?;
        let mut config = surface.get_default_config(&adapter, width, height).ok_or("the canvas cannot be drawn to by this device")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let samples = if samples > 1 { 4 } else { 1 };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("rustyac"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(DRAW_BYTES as u64) },
                count: None,
            }],
        });
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let mut entries: Vec<wgpu::BindGroupLayoutEntry> = (0..7).map(texture_entry).collect();
        entries.push(wgpu::BindGroupLayoutEntry { binding: 7, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&draw_layout), Some(&material_layout)], immediate_size: 0 });
        let solid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&draw_layout)], immediate_size: 0 });
        let solid = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("solid"),
            layout: Some(&solid_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3] })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(1)),
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mesh"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform_stride = DRAW_BYTES.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment.max(1) as usize);
        let uniform_capacity = 2048;
        let (uniforms, uniform_group) = Gpu::uniform_buffer(&device, &draw_layout, uniform_capacity * uniform_stride);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let (color, depth) = Gpu::targets(&device, &config, samples);
        let mut gpu = Gpu {
            surface,
            device,
            queue,
            config,
            backend: format!("{} ({})", if backends == wgpu::Backends::BROWSER_WEBGPU { "WebGPU" } else { "WebGL2" }, info.name),
            compressed_textures,
            samples,
            max_texture_size: limits.max_texture_dimension_2d,
            color,
            depth,
            shader,
            draw_layout,
            material_layout,
            pipeline_layout,
            pipelines: HashMap::new(),
            solid,
            uniforms,
            uniform_group,
            uniform_capacity,
            uniform_stride,
            sampler,
            white: 0,
            textures: Vec::new(),
            meshes: Vec::new(),
            cube: 0,
            track: None,
            car: None,
            scratch: Vec::new(),
            calls: Vec::new(),
        };
        // the stand-in for a missing texture slot, and the box of the shadow
        let white = Image { format: dds::R8G8B8A8_UNORM, data: vec![255; 4], levels: vec![dds::Level { width: 1, height: 1, pitch: 4, bytes: 0..4 }] };
        gpu.white = gpu.texture(&white)?;
        let cube = cube();
        gpu.meshes.push(GpuMesh {
            vertices: gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: slice_bytes(&cube), usage: wgpu::BufferUsages::VERTEX }),
            indices: gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &[0; 4], usage: wgpu::BufferUsages::INDEX }),
        });
        gpu.cube = gpu.meshes.len() - 1;
        Ok(gpu)
    }

    fn uniform_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, bytes: usize) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("draws"), size: bytes as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: 0, size: wgpu::BufferSize::new(DRAW_BYTES as u64) }) }],
        });
        (buffer, group)
    }

    fn targets(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration, samples: u32) -> (Option<wgpu::TextureView>, wgpu::TextureView) {
        let target = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        ((samples > 1).then(|| target(config.format, "color")), target(DEPTH_FORMAT, "depth"))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.clamp(16, self.max_texture_size), height.clamp(16, self.max_texture_size));
        if (width, height) != self.size() {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            (self.color, self.depth) = Gpu::targets(&self.device, &self.config, self.samples);
        }
    }

    fn pipeline(&mut self, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let (blend, depth) = key;
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_model"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] })],
            },
            // `initCullStates` @ 0x14001b0a0: clockwise is the front, and the front is culled
            primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: Some(depth_state(depth)),
            multisample: wgpu::MultisampleState { count: self.samples, mask: !0, alpha_to_coverage_enabled: blend == 2 && self.samples > 1 },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_model"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: self.config.format, blend: (blend == 1).then_some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline);
    }

    fn scene(&mut self, files: &[PathBuf], placements: &[Placement], options: &ModelOptions) -> Result<Scene, String> {
        let model = Model::load(files, placements, options, self)?;
        let groups = model.materials.iter().map(|material| self.material_group(material)).collect();
        Ok(Scene { model, groups })
    }

    fn material_group(&self, material: &Material) -> wgpu::BindGroup {
        let view = |texture: Option<usize>| &self.textures[texture.unwrap_or(self.white)];
        let slots = [material.texture, material.detail.map(|d| d.0), material.layers[0], material.layers[1], material.layers[2], material.layers[3], material.layers[4]];
        let mut entries: Vec<wgpu::BindGroupEntry> = slots.iter().enumerate().map(|(k, slot)| wgpu::BindGroupEntry { binding: k as u32, resource: wgpu::BindingResource::TextureView(view(*slot)) }).collect();
        entries.push(wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.sampler) });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.material_layout, entries: &entries })
    }

    /// The options that fit this device: smaller textures where they have to be unpacked.
    pub fn model_options(&self, texture_size: u32) -> ModelOptions {
        let unpack = !self.compressed_textures;
        let size = if texture_size != 0 { texture_size } else if unpack { 512 } else { 1024 };
        ModelOptions { texture_size: size.min(self.max_texture_size), texture_budget_mb: if unpack { 512 } else { 768 }, unpack_bc: unpack }
    }

    /// Puts a track's models on the card. Returns what was loaded, for the page.
    pub fn load_track(&mut self, files: &[(PathBuf, Placement)], options: &ModelOptions) -> Result<String, String> {
        let (paths, places): (Vec<PathBuf>, Vec<Placement>) = files.iter().cloned().unzip();
        let scene = self.scene(&paths, &places, options)?;
        let s = &scene.model.stats;
        let summary = format!(
            "track: {} meshes, {} triangles, {} textures ({:.0} MB, up to {} px{}){}",
            s.meshes,
            s.triangles,
            s.textures,
            s.texture_bytes as f64 / 1_048_576.0,
            s.texture_size,
            if options.unpack_bc { ", unpacked" } else { "" },
            if s.notes.is_empty() { String::new() } else { format!("; {}", s.notes.join("; ")) }
        );
        self.track = Some(scene);
        Ok(summary)
    }

    /// Puts the car's model on the card.
    pub fn load_car(&mut self, file: &PathBuf, options: &ModelOptions) -> Result<String, String> {
        let scene = self.scene(std::slice::from_ref(file), &[], options)?;
        let node = |name: &str| scene.model.find_node(name);
        let car = CarModel {
            wheels: [node("WHEEL_LF"), node("WHEEL_RF"), node("WHEEL_LR"), node("WHEEL_RR")],
            hubs: [node("SUSP_LF"), node("SUSP_RF"), node("SUSP_LR"), node("SUSP_RR")],
            steer: node("STEER_HR").map(|n| (n, scene.model.nodes[n].local)),
            scene,
        };
        let s = &car.scene.model.stats;
        let summary = format!("car: {} meshes, {} triangles, {} textures ({:.0} MB)", s.meshes, s.triangles, s.textures, s.texture_bytes as f64 / 1_048_576.0);
        self.car = Some(car);
        Ok(summary)
    }

    fn push(&mut self, constants: &DrawConstants) -> u32 {
        let offset = self.scratch.len();
        // `view` holds the camera's view matrix until here: World x View in double precision
        let on_card = DrawConstants { world_view: mul_precise(&constants.world, &constants.world_view), ..*constants };
        self.scratch.extend_from_slice(bytes_of(&on_card));
        self.scratch.resize(offset + self.uniform_stride, 0);
        offset as u32
    }

    /// The draws of one model: the meshes that are not `isTransparent` in the file's order,
    /// then the others from the farthest to the nearest.
    fn model_calls(&mut self, car: bool, base: &DrawConstants, planes: &[[f32; 4]; 6], eye: [f32; 3], stats: &mut DrawStats) {
        let cull = !car;
        let coverage = self.samples > 1;
        let scene = if car { self.car.as_ref().map(|c| &c.scene) } else { self.track.as_ref() };
        let Some(scene) = scene else { return };
        let model = &scene.model;
        let mut order: Vec<(f32, usize, bool)> = Vec::with_capacity(model.meshes.len());
        let mut transparent: Vec<(f32, usize, bool)> = Vec::new();
        for (index, mesh) in model.meshes.iter().enumerate() {
            let centre = point(&model.world[mesh.node], mesh.centre);
            let d = [centre[0] - eye[0], centre[1] - eye[1], centre[2] - eye[2]];
            let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if cull && mesh.radius > 0.0 {
                // outside the picture, outside its own LOD range, or a speck
                if !sphere_visible(planes, centre, mesh.radius) {
                    continue;
                }
                if distance < mesh.lod_in || (mesh.lod_out > 0.0 && distance >= mesh.lod_out) {
                    continue;
                }
                if mesh.radius < distance * 0.002 {
                    continue;
                }
            }
            if mesh.transparent {
                transparent.push((distance, index, true));
            } else {
                order.push((distance, index, false));
            }
        }
        transparent.sort_by(|a, b| b.0.total_cmp(&a.0));
        order.extend(transparent);
        let mut draws: Vec<(DrawConstants, PipelineKey, usize, usize, u32)> = Vec::with_capacity(order.len());
        for (_, index, transparent_pass) in order {
            let mesh = &model.meshes[index];
            let material = &model.materials[mesh.material];
            // `Material::apply` @ 0x14020a6e0: the blend state is the material's; the depth
            // state is the material's in the opaque pass and "no write" in the other
            let depth = match material.depth_mode {
                _ if transparent_pass => 1,
                1 => 1,
                2 => 2,
                _ => 0,
            };
            let alpha_ref = if material.blend_mode == 2 && !coverage { 0.5 } else { 0.0 };
            let [r, g, b, a] = material.mult;
            let ks = material.ks.unwrap_or([-1.0, -1.0]);
            let constants = DrawConstants {
                world: model.world[mesh.node],
                color: material.color,
                params: [alpha_ref, if material.foliage { 1.0 } else { 0.0 }, material.detail.map(|d| d.1).unwrap_or(0.0), 0.0],
                mult_rg: [r[0], r[1], g[0], g[1]],
                mult_ba: [b[0], b[1], a[0], a[1]],
                layer: [material.kind as f32, material.magic, ks[0], ks[1]],
                layer2: [material.uv_mult, material.alpha_scale, 0.0, 0.0],
                ..*base
            };
            draws.push((constants, (material.blend_mode, depth), mesh.material, mesh.buffers, mesh.index_count));
            stats.meshes += 1;
            stats.triangles += mesh.index_count as u64 / 3;
        }
        for (constants, pipeline, material, mesh, index_count) in draws {
            self.pipeline(pipeline);
            let offset = self.push(&constants);
            self.calls.push(DrawCall { pipeline, group: Some((car, material)), mesh, index_count, offset });
        }
    }

    /// Draws one frame.
    pub fn draw(&mut self, view: &CarView, shape: &CarShape, camera: &CameraFrame) -> Result<DrawStats, String> {
        let (width, height) = self.size();
        let projection = perspective_reversed(camera.fov, width as f32 / height as f32, camera.near.max(0.02), FAR);
        let view_matrix = view_matrix(&camera.matrix);
        let view_proj = mul(&view_matrix, &projection);
        let eye = camera.matrix[3];
        let base = DrawConstants {
            world: IDENTITY,
            world_view: view_matrix,
            proj: projection,
            color: [1.0; 4],
            light: LIGHT,
            camera: [eye[0], eye[1], eye[2], FOG],
            fog: HORIZON,
            params: [0.0; 4],
            mult_rg: [0.0; 4],
            mult_ba: [0.0; 4],
            layer: [0.0, 1.0, -1.0, -1.0],
            layer2: [1.0, 1.0, 0.0, 0.0],
        };
        let planes = frustum(&view_proj);
        let eye3 = [eye[0], eye[1], eye[2]];
        // the track's loose objects follow their bodies
        if let Some(track) = &mut self.track {
            if track.model.place_objects(&view.moved_objects[..view.moved_object_count as usize]) {
                track.model.update(&IDENTITY, &[]);
            }
        }
        // the car's model follows the physics: the body with the model's offset, the wheels
        // and hubs where the physics has them, the steering wheel turned
        if let Some(car) = self.car.as_mut() {
            let o = shape.graphics_offset;
            let root = rotate_pitch(&mul(&translation(o[0], o[1], o[2]), &view.body), shape.graphics_pitch);
            let mut fixed: Vec<(usize, Mat)> = Vec::with_capacity(9);
            for k in 0..4 {
                if let Some(node) = car.hubs[k] {
                    fixed.push((node, view.hubs[k]));
                }
                if let Some(node) = car.wheels[k] {
                    fixed.push((node, view.wheels[k]));
                }
            }
            if let Some((node, rest)) = car.steer {
                let (sin, cos) = (view.steer_deg * 0.017_453_292).sin_cos();
                let turn: Mat = [[cos, sin, 0.0, 0.0], [-sin, cos, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
                car.scene.model.nodes[node].local = mul(&turn, &rest);
            }
            car.scene.model.update(&root, &fixed);
        }

        self.scratch.clear();
        self.calls.clear();
        let mut stats = DrawStats::default();
        self.model_calls(false, &base, &planes, eye3, &mut stats);
        // the car's shadow: a dark patch on the road under the body, in the plane the four
        // tyres stand on
        let body = &view.body;
        let mut drop = 0.0;
        for k in 0..4 {
            let w = view.wheels[k][3];
            drop += (w[0] - body[3][0]) * body[1][0] + (w[1] - body[3][1]) * body[1][1] + (w[2] - body[3][2]) * body[1][2] - view.tyre_radius[k];
        }
        let flat = mul(&translation(0.0, drop * 0.25 + 0.012, 0.0), body);
        let (mut x_max, mut z_min, mut z_max) = (0.5f32, -1.0f32, 1.0f32);
        for b in &shape.boxes {
            x_max = x_max.max(b.centre[0].abs() + b.size[0] * 0.5);
            z_min = z_min.min(b.centre[2] - b.size[2] * 0.5);
            z_max = z_max.max(b.centre[2] + b.size[2] * 0.5);
        }
        let shadow = mul(&scale_then(x_max * 2.0, 0.002, z_max - z_min, &translation(0.0, 0.0, (z_max + z_min) * 0.5)), &flat);
        let offset = self.push(&DrawConstants { world: shadow, color: [0.0, 0.0, 0.0, 0.42], ..base });
        self.calls.push(DrawCall { pipeline: (0, 0), group: None, mesh: self.cube, index_count: 36, offset });
        if self.car.is_some() {
            self.model_calls(true, &base, &planes, eye3, &mut stats);
        } else {
            // no model: the car's boxes
            for b in &shape.boxes {
                let world = mul(&scale_then(b.size[0], b.size[1], b.size[2], &translation(b.centre[0], b.centre[1], b.centre[2])), body);
                let offset = self.push(&DrawConstants { world, color: [b.color[0], b.color[1], b.color[2], 1.0], ..base });
                self.calls.push(DrawCall { pipeline: (0, 0), group: None, mesh: self.cube, index_count: 36, offset });
            }
        }

        // all the constants in one write
        let needed = self.scratch.len() / self.uniform_stride;
        if needed > self.uniform_capacity {
            self.uniform_capacity = needed.next_power_of_two();
            (self.uniforms, self.uniform_group) = Gpu::uniform_buffer(&self.device, &self.draw_layout, self.uniform_capacity * self.uniform_stride);
        }
        self.queue.write_buffer(&self.uniforms, 0, &self.scratch);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(stats);
            }
            other => return Err(format!("the canvas gave no picture to draw into ({other:?})")),
        };
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let (attachment, resolve) = match &self.color {
                Some(color) => (color, Some(&target)),
                None => (&target, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: attachment,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: HORIZON[0] as f64, g: HORIZON[1] as f64, b: HORIZON[2] as f64, a: 1.0 }),
                        store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut bound: Option<PipelineKey> = None;
            let mut solid = false;
            for call in &self.calls {
                let mesh = &self.meshes[call.mesh];
                match call.group {
                    Some((car, material)) => {
                        if bound != Some(call.pipeline) || solid {
                            pass.set_pipeline(&self.pipelines[&call.pipeline]);
                            bound = Some(call.pipeline);
                            solid = false;
                        }
                        let scene = if car { self.car.as_ref().map(|c| &c.scene) } else { self.track.as_ref() };
                        let Some(scene) = scene else { continue };
                        pass.set_bind_group(0, &self.uniform_group, &[call.offset]);
                        pass.set_bind_group(1, &scene.groups[material], &[]);
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..call.index_count, 0, 0..1);
                    }
                    None => {
                        if !solid {
                            pass.set_pipeline(&self.solid);
                            solid = true;
                        }
                        pass.set_bind_group(0, &self.uniform_group, &[call.offset]);
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.draw(0..call.index_count, 0..1);
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(stats)
    }
}
