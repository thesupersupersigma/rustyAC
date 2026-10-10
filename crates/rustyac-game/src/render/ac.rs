// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's picture drawn by `rustyac-render`, the port of Assetto Corsa's own renderer:
//! what `Sim` does around it (the scene graph, loading the track and the car, the camera of
//! the moment, the frame) and rustyAC's own HUD on top.

use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::car::CarAvatar;
use rustyac_render::forward::CameraForward;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;
use rustyac_render::model::{path_text, Kn5Io};
use rustyac_render::scene::{NodeId, Scene};
use rustyac_render::sky::SkyBox;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

use super::font::FontBitmap;
use super::hud::{self, HudInfo, HudVertex};
use super::scene::{CameraFrame, CameraMode, Drivable, DrivingCamera};
use crate::view::CarView;

const HUD_CAPACITY: usize = 24_000;

const HUD_SHADERS: &str = r#"
cbuffer PerHud : register(b0) { float4 Viewport; };
Texture2D FontTexture : register(t0);
SamplerState FontSampler : register(s0);
struct HudIn { float2 pos : POSITION; float2 uv : TEXCOORD0; float4 color : COLOR0; };
struct HudOut { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; float4 color : COLOR0; };

HudOut vs_hud(HudIn i) {
    HudOut o;
    o.pos = float4(i.pos.x / Viewport.x * 2.0 - 1.0, 1.0 - i.pos.y / Viewport.y * 2.0, 0.0, 1.0);
    o.uv = i.uv;
    o.color = i.color;
    return o;
}

float4 ps_hud(HudOut i) : SV_TARGET {
    return float4(i.color.rgb, i.color.a * FontTexture.Sample(FontSampler, i.uv).r);
}
"#;

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn compile(entry: &str, target: &str) -> Result<Vec<u8>, String> {
    let entry_z = format!("{entry}\0");
    let target_z = format!("{target}\0");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and both names are valid for the call; the blob is read within its
    // reported size.
    unsafe {
        D3DCompile(HUD_SHADERS.as_ptr() as *const _, HUD_SHADERS.len(), PCSTR::null(), None, None::<&windows::Win32::Graphics::Direct3D::ID3DInclude>, PCSTR(entry_z.as_ptr()), PCSTR(target_z.as_ptr()), 0, 0, &mut code, Some(&mut errors))
            .map_err(|e| format!("shader {entry}: {e}"))?;
        let blob = code.ok_or("the shader compiler gave no code")?;
        Ok(std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()).to_vec())
    }
}

/// rustyAC's own text and bars, drawn over the finished 3D picture.
struct HudPass {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    buffer: ID3D11Buffer,
    constants: ID3D11Buffer,
    blend: ID3D11BlendState,
    depth_off: ID3D11DepthStencilState,
    raster: ID3D11RasterizerState,
    sampler: ID3D11SamplerState,
    font_view: ID3D11ShaderResourceView,
    font: FontBitmap,
}

impl HudPass {
    fn new(device: &ID3D11Device) -> Result<HudPass, String> {
        let vs_code = compile("vs_hud", "vs_4_0")?;
        let ps_code = compile("ps_hud", "ps_4_0")?;
        let element = |name: &'static [u8], format: DXGI_FORMAT, offset: u32| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(name.as_ptr()),
            SemanticIndex: 0,
            Format: format,
            InputSlot: 0,
            AlignedByteOffset: offset,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        };
        let elements = [element(b"POSITION\0", DXGI_FORMAT_R32G32_FLOAT, 0), element(b"TEXCOORD\0", DXGI_FORMAT_R32G32_FLOAT, 8), element(b"COLOR\0", DXGI_FORMAT_R32G32B32A32_FLOAT, 16)];
        let font = FontBitmap::new(30);
        let dynamic = |bytes: usize, bind: D3D11_BIND_FLAG| -> Result<ID3D11Buffer, String> {
            let desc = D3D11_BUFFER_DESC { ByteWidth: bytes as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, ..Default::default() };
            let mut buffer = None;
            // SAFETY: a plain buffer creation.
            unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)).map_err(err("buffer"))? };
            buffer.ok_or("no buffer".to_string())
        };
        // SAFETY: creation calls with valid descriptions and out pointers.
        unsafe {
            let (mut vs, mut ps, mut layout) = (None, None, None);
            device.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(err("vertex shader"))?;
            device.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(err("pixel shader"))?;
            device.CreateInputLayout(&elements, &vs_code, Some(&mut layout)).map_err(err("input layout"))?;
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            device.CreateBlendState(&blend_desc, Some(&mut blend)).map_err(err("blend state"))?;
            let depth_desc = D3D11_DEPTH_STENCIL_DESC { DepthEnable: false.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_ALWAYS, ..Default::default() };
            let mut depth_off = None;
            device.CreateDepthStencilState(&depth_desc, Some(&mut depth_off)).map_err(err("depth state"))?;
            let raster_desc = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
            let mut raster = None;
            device.CreateRasterizerState(&raster_desc, Some(&mut raster)).map_err(err("rasterizer state"))?;
            let sampler_desc = D3D11_SAMPLER_DESC { Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR, AddressU: D3D11_TEXTURE_ADDRESS_CLAMP, AddressV: D3D11_TEXTURE_ADDRESS_CLAMP, AddressW: D3D11_TEXTURE_ADDRESS_CLAMP, MaxLOD: f32::MAX, ..Default::default() };
            let mut sampler = None;
            device.CreateSamplerState(&sampler_desc, Some(&mut sampler)).map_err(err("sampler"))?;
            let font_desc = D3D11_TEXTURE2D_DESC {
                Width: font.width as u32,
                Height: font.height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let font_data = D3D11_SUBRESOURCE_DATA { pSysMem: font.pixels.as_ptr() as *const _, SysMemPitch: font.width as u32, SysMemSlicePitch: 0 };
            let mut font_texture = None;
            device.CreateTexture2D(&font_desc, Some(&font_data), Some(&mut font_texture)).map_err(err("font texture"))?;
            let mut font_view = None;
            device.CreateShaderResourceView(font_texture.as_ref().ok_or("no font texture")?, None, Some(&mut font_view)).map_err(err("font view"))?;
            Ok(HudPass {
                vs: vs.ok_or("no shader")?,
                ps: ps.ok_or("no shader")?,
                layout: layout.ok_or("no layout")?,
                buffer: dynamic(HUD_CAPACITY * std::mem::size_of::<HudVertex>(), D3D11_BIND_VERTEX_BUFFER)?,
                constants: dynamic(16, D3D11_BIND_CONSTANT_BUFFER)?,
                blend: blend.ok_or("no blend state")?,
                depth_off: depth_off.ok_or("no depth state")?,
                raster: raster.ok_or("no rasterizer state")?,
                sampler: sampler.ok_or("no sampler")?,
                font_view: font_view.ok_or("no font view")?,
                font,
            })
        }
    }

    fn write<T: Copy>(context: &ID3D11DeviceContext, buffer: &ID3D11Buffer, data: &[T]) {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the buffer is dynamic and at least as large as `data`.
        unsafe {
            if context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)).is_ok() {
                std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, mapped.pData as *mut u8, std::mem::size_of_val(data));
                context.Unmap(buffer, 0);
            }
        }
    }

    /// Draws into the render target that is bound.
    fn draw(&self, context: &ID3D11DeviceContext, view: &CarView, info: &HudInfo, width: u32, height: u32) {
        let vertices = hud::build(&self.font, view, info, width as f32, height as f32);
        let count = vertices.len().min(HUD_CAPACITY);
        Self::write(context, &self.buffer, &vertices[..count]);
        Self::write(context, &self.constants, &[[width as f32, height as f32, 0.0, 0.0]]);
        let stride = std::mem::size_of::<HudVertex>() as u32;
        // SAFETY: state objects and buffers of this pass on the live context.
        unsafe {
            context.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            context.OMSetDepthStencilState(&self.depth_off, 0);
            context.RSSetState(&self.raster);
            context.IASetInputLayout(&self.layout);
            context.VSSetShader(&self.vs, None);
            context.PSSetShader(&self.ps, None);
            context.VSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(self.font_view.clone())]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.IASetVertexBuffers(0, 1, Some(&Some(self.buffer.clone())), Some(&stride), Some(&0));
            context.Draw(count as u32, 0);
        }
    }
}

/// What the game was asked for and what the player's `video.ini` says.
#[derive(Clone, Debug)]
pub struct AcOptions {
    /// `--warp`: Microsoft's software rasteriser instead of the graphics card.
    pub warp: bool,
    /// `--gpu-log <file>`: write the command log of one frame.
    pub gpu_log: Option<PathBuf>,
    /// The game's folder.
    pub game: PathBuf,
    /// race.ini `[LIGHTING] SUN_ANGLE` and `[WEATHER] NAME` (or rustyAC's own defaults).
    pub sun_angle: f32,
    pub weather: String,
    /// The car's skin folder; `None`: the first one.
    pub skin: Option<String>,
    /// `--video-ini-exact`: also follow the values of `video.ini` that plain acs.exe cannot
    /// make a proper picture with (a shadow map or cube map size of 0 or less).
    pub video_exact: bool,
    /// `--cube-faces <n>`: `[CUBEMAP] FACES_PER_FRAME` whatever `video.ini` says.
    pub cube_faces: Option<i32>,
    /// `--mirror-size <n>`: `[MIRROR] SIZE` whatever `video.ini` says.
    pub mirror_size: Option<i32>,
    /// race.ini `[LIGHTING] TIME_MULT` and `CLOUD_SPEED`; `None`: the file has no `[LIGHTING]`
    /// (the game then makes no `SunAnimator`: the sun and the clouds stand still)
    pub sun_animation: Option<(f32, f32)>,
    /// race.ini `[GROOVE] MAX_LAPS` and `STARTING_LAPS`, when the file has the section
    pub groove: Option<(f32, f32)>,
    /// `--mirror-hq <0|1>`: `[MIRROR] HQ` whatever `video.ini` says
    pub mirror_hq: Option<bool>,
    /// what `rand()` starts from once the track is loaded
    pub render_seed: u32,
}

/// What `cfg/video.ini` asks for that this renderer does not do yet: one line each.
fn video_settings(width: u32, height: u32, exact: bool) -> (VideoSettings, i32, i32, f32, bool, Vec<String>) {
    let mut video = VideoSettings { width: width as i32, height: height as i32, is_fullscreen: false, ..VideoSettings::default() };
    let mut notes = Vec::new();
    let (mut cube_size, mut cube_faces, mut cube_far) = (512, 0, 0.0f32);
    let documents = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents/Assetto Corsa/cfg/video.ini"));
    let Some(ini) = documents.and_then(|p| IniReader::load(&p).ok()) else {
        notes.push("no Documents/Assetto Corsa/cfg/video.ini: the game's defaults are used".to_string());
        video.world_detail = 5;
        // TyreSmoke / EngineSmoke without the file: the Normal level
        video.smoke = None;
        return (video, cube_size, cube_faces, cube_far, false, notes);
    };
    let int = |section: &str, key: &str| ini.get_int(section, key).unwrap_or(0);
    video.anisotropic = int("VIDEO", "ANISOTROPIC");
    video.shadow_map_size = match int("VIDEO", "SHADOW_MAP_SIZE") {
        0 => 2048,
        size => size,
    };
    video.world_detail = int("ASSETTOCORSA", "WORLD_DETAIL");
    cube_size = int("CUBEMAP", "SIZE");
    // Sim::initCubemaps 0x1401997a0
    cube_faces = int("CUBEMAP", "FACES_PER_FRAME").clamp(0, 6);
    cube_far = ini.get_float("CUBEMAP", "FARPLANE").unwrap_or(0.0);
    if int("VIDEO", "AASAMPLES") > 1 {
        notes.push(format!("video.ini AASAMPLES={}: multisampling comes in Task 23, the picture has one sample per pixel", int("VIDEO", "AASAMPLES")));
    }
    if int("POST_PROCESS", "ENABLED") != 0 {
        notes.push("video.ini [POST_PROCESS] ENABLED=1: post-processing comes in Task 23, the plain picture is drawn".to_string());
    }
    if ini.get_float("EFFECTS", "MOTION_BLUR").unwrap_or(0.0) > 0.0 {
        notes.push("video.ini MOTION_BLUR: motion blur comes in Task 23, treated as off".to_string());
    }
    video.smoke = Some(int("EFFECTS", "SMOKE"));
    video.mirror_size = int("MIRROR", "SIZE");
    video.mirror_smoke = int("EFFECTS", "RENDER_SMOKE_IN_MIRROR") > 0;
    let mirror_hq = int("MIRROR", "HQ") > 0;
    // values Content Manager writes for Custom Shaders Patch: plain acs.exe cannot make a
    // shadow map or a cube map of such a size and then draws everything in shadow, without
    // reflections (checked with the game's own code in the render oracle)
    if video.shadow_map_size < 0 {
        if exact {
            notes.push(format!("video.ini SHADOW_MAP_SIZE={}: no shadow map of that size can be made: everything is drawn in shadow, as plain acs.exe does", video.shadow_map_size));
        } else {
            notes.push(format!("video.ini SHADOW_MAP_SIZE={} is a Custom Shaders Patch value; plain acs.exe would draw everything in shadow with it. 2048 is used (--video-ini-exact: follow the file)", video.shadow_map_size));
            video.shadow_map_size = 2048;
        }
    }
    if cube_size <= 0 {
        if exact {
            notes.push(format!("video.ini [CUBEMAP] SIZE={cube_size}: no cube map of that size can be made: nothing is reflected, as in plain acs.exe"));
        } else {
            notes.push(format!("video.ini [CUBEMAP] SIZE={cube_size} is a Custom Shaders Patch value; plain acs.exe would reflect nothing with it. 512 is used (--video-ini-exact: follow the file)"));
            cube_size = 512;
        }
    }
    (video, cube_size, cube_faces, cube_far, mirror_hq, notes)
}

/// The five pictures of `CarFakeShadow::generateFakeShadow` (four wheels of 64 x 64, the body of
/// 512 x 512, RGBA) as the PNG files the game would have saved, in rustyAC's own folder:
/// `<the program's folder>/rustyac_cache/shadows/<car>/`.
fn save_generated_shadows(car: &str, pictures: &[Vec<u8>]) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = exe.parent().ok_or("the program has no folder")?.join("rustyac_cache").join("shadows").join(car);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for (i, picture) in pictures.iter().enumerate() {
        let (name, side) = if i < 4 { (format!("tyre_{i}_shadow.png"), 64u32) } else { ("body_shadow.png".to_string(), 512u32) };
        let file = std::fs::File::create(folder.join(&name)).map_err(|e| format!("{name}: {e}"))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(picture).map_err(|e| e.to_string())?;
    }
    Ok(folder)
}

/// The shadow ranges the game's camera of the moment asks for (`setShadowMapsSplits`).
fn splits_of(camera: &DrivingCamera) -> [f32; 4] {
    match camera.mode {
        // CameraOnBoard::update 0x1400c9ea0
        CameraMode::Cockpit => [f32::from_bits(0x3fa6_6666), 80.0, 250.0, 500.0],
        CameraMode::Drivable => match camera.drivable {
            // CameraDrivableManager::updateChase 0x1400c7120, ::updateBumper 0x1400c6df0
            Drivable::Chase | Drivable::Chase2 | Drivable::Bumper => [10.0, 50.0, 150.0, 500.0],
            // ::updateBonnet 0x1400c6ba0
            Drivable::Bonnet => [2.0, 30.0, 150.0, 500.0],
            // ::updateDash 0x1400c7b50
            Drivable::Dash => [f32::from_bits(0x3fa6_6666), 80.0, 250.0, 500.0],
        },
        // CameraCarManager::update 0x1400c47d0: its ranges were not read; the chase camera's
        CameraMode::Car => [10.0, 50.0, 150.0, 500.0],
    }
}

/// The game's picture, drawn by the port of AC's renderer.
pub struct AcRenderer {
    graphics: Graphics,
    scene: Scene,
    camera: CameraForward,
    root: NodeId,
    blurred: NodeId,
    track_node: NodeId,
    cars: NodeId,
    car_shadows: NodeId,
    sim_nodes: rustyac_render::car::SimNodes,
    /// `video.ini [ASSETTOCORSA] LOCK_STEER`, `HIDE_ARMS`, `HIDE_STEER`
    cockpit_flags: (bool, bool, bool),
    car: Option<CarAvatar>,
    /// `Sim::mirrorTextureRenderer` and `Sim::virtualMirrorRenderer`: only with mirrors on
    mirror: Option<rustyac_render::mirror::MirrorTextureRenderer>,
    virtual_mirror: Option<rustyac_render::mirror::VirtualMirrorRenderer>,
    /// `Game::gameTime.now`, milliseconds
    time_ms: f64,
    /// `RaceManager::initLighting`'s `SunAnimator`
    sun: Option<rustyac_render::sky::SunAnimator>,
    /// `TrackAvatar`'s `DynamicTrackManager` (the grooves) and the crowds of `CameraFacing`
    grooves: Option<rustyac_render::crowds::DynamicTrackManager>,
    crowds: Vec<std::rc::Rc<std::cell::RefCell<rustyac_render::crowds::StaticParticleSystem>>>,
    /// the nodes of the track's loose objects (`TrackObject::root`) in the physics' order, each
    /// with the matrix of the file; and the ones that are not at home
    object_nodes: Vec<(NodeId, Mat44f)>,
    moved_objects: Vec<usize>,
    /// what the car's objects were last told: the pause menu shows; the replay's status
    /// (-1: no replay yet)
    told_paused: bool,
    told_replay_status: i32,
    track_folder: Option<String>,
    options: AcOptions,
    hud: HudPass,
    pub adapter: String,
    pub software: bool,
    /// draw calls of the last frame
    pub draw_calls: i32,
    pub triangles: i32,
    frames: u64,
    /// what `video.ini` asks for that is not done yet
    pub notes: Vec<String>,
}

impl AcRenderer {
    pub fn new(width: u32, height: u32, options: AcOptions) -> Result<AcRenderer, String> {
        let (mut video, cube_size, mut cube_faces, cube_far, mirror_hq, notes) = video_settings(width, height, options.video_exact);
        let mirror_hq = options.mirror_hq.unwrap_or(mirror_hq);
        if let Some(size) = options.mirror_size {
            video.mirror_size = size;
        }
        if let Some(faces) = options.cube_faces {
            cube_faces = faces.clamp(0, 6);
        }
        let mut graphics = Graphics::new(video, DeviceOptions { warp: options.warp, window: None, log: options.gpu_log.is_some() }, &options.game)?;
        // Sim::Sim: srand(the time in milliseconds)
        graphics.crt_rand = rustyac_physics::session::MsvcRand(options.render_seed);
        // SAFETY: COM calls on the live device.
        let adapter = unsafe {
            graphics
                .kgl
                .device
                .cast::<IDXGIDevice>()
                .and_then(|d| d.GetAdapter())
                .and_then(|a| a.GetDesc())
                .map(|desc| {
                    let length = desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len());
                    String::from_utf16_lossy(&desc.Description[..length])
                })
                .unwrap_or_else(|_| "an unknown adapter".to_string())
        };
        // the loading screen's frame: the default state is there when the scene loads
        graphics.begin_scene();
        graphics.set_screen_space_mode();
        graphics.end_scene();

        // Sim::initSceneGraph 0x140199d70
        let mut scene = Scene::new();
        let root = scene.node("ROOT");
        let blurred = scene.node("BLURRED");
        let unblurred = scene.node("UNBLURRED");
        scene.add_child(root, blurred);
        scene.add_child(root, unblurred);
        let track_node = scene.node("TRACK");
        scene.add_child(blurred, track_node);
        let skid_marks = scene.node("SKIDMARKS");
        scene.add_child(blurred, skid_marks);
        let car_shadows = scene.node_event("CAR_SHADOWS");
        scene.add_child(blurred, car_shadows);
        let before_cars = scene.node_event("BEFORE_CARS_NODE");
        scene.add_child(unblurred, before_cars);
        let cars = scene.node("CARS");
        scene.add_child(unblurred, cars);
        let particles = scene.node("PARTICLES_NODE");
        scene.add_child(unblurred, particles);
        let render_finished = scene.node_event("RENDER FINISHED");
        scene.add_child(unblurred, render_finished);

        // Sim::createCamera 0x1401982e0, Sim::Sim, Sim::initCubemaps 0x1401997a0
        let mut camera = CameraForward::new(&mut graphics)?;
        camera.base.camera.clear_color = [0.3, 0.25, 0.25, 1.0];
        camera.base.camera.max_layer = graphics.video.world_detail as f32;
        // SkyBox::SkyBox: race.ini [WEATHER] NAME makes the clouds a first time
        let weather = (!options.weather.is_empty()).then(|| options.weather.clone());
        camera.base.sky_box = Some(SkyBox::new(&mut graphics, weather.as_deref())?);
        camera.base.camera.near_plane = 0.05;
        camera.base.camera.far_plane = 40000.0;
        camera.cube_map_renderer.faces_per_frame = cube_faces;
        if cube_far != 0.0 {
            camera.cube_map_renderer.set_camera_near_far_planes(f32::from_bits(0x3c23_d70a), cube_far);
        }
        camera.set_cubemap_size(&graphics, cube_size);
        let hud = HudPass::new(&graphics.kgl.device)?;
        // CarAvatar::initCommonPostPhysics: the cockpit switches of video.ini
        let cockpit_flags = {
            let ini = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents/Assetto Corsa/cfg/video.ini")).and_then(|p| IniReader::load(&p).ok());
            let flag = |key: &str| ini.as_ref().is_some_and(|i| i.has_section("ASSETTOCORSA") && i.get_int("ASSETTOCORSA", key).unwrap_or(0) != 0);
            (flag("LOCK_STEER"), flag("HIDE_ARMS"), flag("HIDE_STEER"))
        };
        // Sim::Sim: the mirror texture and the virtual mirror (gameplay.ini says whether it shows)
        let mirror = match graphics.video.mirror_size {
            0 => None,
            size => {
                let smoke = graphics.video.mirror_smoke;
                let samples = graphics.video.aa_samples;
                Some(rustyac_render::mirror::MirrorTextureRenderer::new(&mut graphics, size, smoke, mirror_hq.then_some(samples))?)
            }
        };
        let virtual_mirror = mirror.as_ref().map(|_| {
            let ini = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents/Assetto Corsa/cfg/gameplay.ini")).and_then(|p| IniReader::load(&p).ok());
            let active = ini.is_some_and(|i| i.has_section("VIRTUAL_MIRROR") && i.get_int("VIRTUAL_MIRROR", "ACTIVE").unwrap_or(0) != 0);
            rustyac_render::mirror::VirtualMirrorRenderer::new(&graphics, active)
        });
        Ok(AcRenderer { mirror, virtual_mirror, time_ms: 0.0, sun: None, grooves: None, crowds: Vec::new(), object_nodes: Vec::new(), moved_objects: Vec::new(), told_paused: false, told_replay_status: -1, graphics, scene, camera, root, blurred, track_node, cars, car_shadows, sim_nodes: rustyac_render::car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished }, cockpit_flags, car: None, track_folder: None, options, hud, adapter, software: false, draw_calls: 0, triangles: 0, frames: 0, notes })
    }

    pub fn is_warp(&self) -> bool {
        self.options.warp
    }

    /// The car's skin folder for [`AcRenderer::load_car`]; `None`: the first one.
    pub fn set_skin(&mut self, skin: Option<String>) {
        self.options.skin = skin;
    }

    /// `TrackAvatar::init3D` 0x1401c8740 and the lighting part of `TrackAvatar::TrackAvatar`.
    pub fn load_track(&mut self, folder: &Path, layout: &str) -> Result<String, String> {
        let started = std::time::Instant::now();
        let models = rustyac_physics::track::loader::track_models(folder, layout)?;
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let model = self.scene.node(&format!("TRACK {name}"));
        let mut io = Kn5Io::new();
        for entry in &models {
            let filename = path_text(&entry.file);
            io.skin_override_path.push(format!("{}/texture", rustyac_render::model::get_path(&filename)));
            let top = io.load(&mut self.graphics, &mut self.scene, &filename, &entry.file)?;
            self.scene.add_child(model, top);
            self.scene.nodes[top].matrix = rustyac_physics::track::loader::top_node_matrix(&self.scene.nodes[top].matrix, entry.position, entry.rotation);
        }
        self.scene.compile(&self.graphics, model);
        self.scene.add_child(self.track_node, model);
        self.scene.hide_helpers(model);
        // TrackAvatar::TrackAvatar: a TrackObject per AC_POBJECT node, which is switched on again
        self.object_nodes = self.scene.show_track_objects(model);
        self.track_folder = Some(path_text(folder));
        let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
        self.graphics.load_track_lighting(&data.join("data/lighting.ini"));
        // TrackAvatar::TrackAvatar: the grooves, then the crowds (which leave rand() seeded)
        let grooves = rustyac_render::crowds::DynamicTrackManager::new(&mut self.scene, &data, self.track_node, self.options.groove);
        self.graphics.cube_map_hidden = grooves.meshes();
        self.grooves = Some(grooves);
        match rustyac_render::crowds::camera_facing(&mut self.graphics, &mut self.scene, &data, self.track_node, self.blurred, self.options.render_seed) {
            Ok(crowds) => self.crowds = crowds,
            Err(message) => println!("WARNING: no crowds: {message}"),
        }
        Ok(format!("track models: {} files, {} textures, {} materials, loaded in {:.2} s", models.len(), self.graphics.resources.textures.len(), self.scene.materials.len(), started.elapsed().as_secs_f64()))
    }

    /// `CarAvatar::init3D` 0x1400d3b90. `folder` is the car's folder in the game.
    pub fn load_car(&mut self, folder: &Path, steer_lock: f32, tyre_width: [f32; 4], max_gear: i32) -> Result<String, String> {
        let started = std::time::Instant::now();
        let skin = match &self.options.skin {
            Some(skin) => skin.clone(),
            None => {
                let mut skins: Vec<String> = std::fs::read_dir(folder.join("skins")).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
                skins.sort();
                skins.first().cloned().unwrap_or_default()
            }
        };
        let mut car = CarAvatar::init_3d(&mut self.graphics, &mut self.scene, self.cars, &path_text(folder), folder, &skin, Some(steer_lock), 0)?;
        car.max_gear = max_gear;
        if let Some(mirror) = &self.mirror {
            car.init_mirror_materials(&mut self.graphics, &mut self.scene, &mirror.texture)?;
        }
        (car.lock_virtual_steer, car.hide_arms_in_cockpit, car.hide_steer) = self.cockpit_flags;
        car.init_common_post_physics(&mut self.graphics, &mut self.scene, &self.sim_nodes, tyre_width)?;
        let summary = format!("car model {}: {} levels of detail, skin {skin}, loaded in {:.2} s", folder.display(), car.lods.len(), started.elapsed().as_secs_f64());
        self.car = Some(car);
        Ok(summary)
    }

    /// What follows the loading in the game: the sun (`RaceManager::initLighting`), the weather
    /// (`Sim::applyCustomWeather`) and the reflection cube map (`Sim::initStaticCubemap`).
    pub fn finish_loading(&mut self) -> Result<(), String> {
        self.graphics.set_sun_angle(self.options.sun_angle);
        // RaceManager::initLighting 0x14013a3d0
        self.sun = self.options.sun_animation.map(|(time_mult, cloud_speed)| rustyac_render::sky::SunAnimator::new(self.options.sun_angle, time_mult, cloud_speed));
        if !self.options.weather.is_empty() {
            // Sim::applyCustomWeather 0x140198010: the clouds first
            if let Some(sky) = &mut self.camera.base.sky_box {
                sky.update_clouds_generation(&mut self.graphics, &self.options.weather.clone());
            }
            self.graphics.apply_custom_weather(&self.options.weather.clone());
        }
        rustyac_render::cubemap::init_static_cubemap(&mut self.camera, &mut self.graphics, &mut self.scene, self.track_folder.as_deref())?;
        // Sim::onPostLoad, after the cube map: CarAvatar::onPostLoad (the flat ground shadows)
        if let Some(car) = &mut self.car {
            car.on_post_load(&mut self.graphics, &mut self.scene, self.car_shadows);
            // A car without body_shadow.png: the game draws the five pictures, saves them into
            // the car's folder and shows them from its next start on. rustyAC never writes
            // there: the pictures go into a folder of its own, next to the program, and are
            // loaded from it at once (they are made again at every start: it takes a moment).
            if let Some(shadow) = &car.fake_shadow {
                let mut shadow = shadow.borrow_mut();
                if let Some(pictures) = shadow.generated.take() {
                    let name = car.folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    match save_generated_shadows(&name, &pictures) {
                        Ok(folder) => {
                            println!("{name} has no body_shadow.png: its ground shadows were generated into {}", folder.display());
                            shadow.load_shadows(&mut self.graphics, &path_text(&folder));
                        }
                        Err(message) => println!("WARNING: {name} has no body_shadow.png and the generated ones could not be kept ({message}): no ground shadow"),
                    }
                }
            }
        }
        Ok(())
    }

    /// `CarAvatar::evOnBackfireTriggered` of the last frame; `None` without a car that tests.
    pub fn backfire_triggered(&self) -> Option<bool> {
        self.car.as_ref().filter(|car| car.backfire.is_some()).map(|car| car.backfire_triggered)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.graphics.video.width as u32, self.graphics.video.height as u32)
    }

    /// F11, `VirtualMirrorRenderer::update` 0x1401d2190: the virtual mirror on or off. The
    /// answer is what it is now; `None`: `video.ini` has no mirrors.
    pub fn toggle_virtual_mirror(&mut self) -> Option<bool> {
        let mirror = self.virtual_mirror.as_mut()?;
        mirror.active = !mirror.active;
        Some(mirror.active)
    }

    /// Switches the virtual mirror (`--virtual-mirror`).
    pub fn set_virtual_mirror(&mut self, active: bool) {
        if let Some(mirror) = &mut self.virtual_mirror {
            mirror.active = active;
        }
    }

    /// One frame: `Game::onIdle` 0x140242730 as far as the picture goes, then the HUD.
    pub fn draw(&mut self, view: &CarView, driving: &DrivingCamera, frame: &CameraFrame, info: &HudInfo, dt: f32) {
        // the game camera of the moment
        let cam = &mut self.camera.base.camera;
        cam.fov = frame.fov;
        cam.matrix = Mat44f { m: frame.matrix };
        cam.near_plane = frame.near;
        let s = splits_of(driving);
        self.camera.base.set_shadow_maps_splits(&mut self.graphics, s[0], s[1], s[2], s[3]);
        // Game::update: the car's objects, then the handlers of evOnPostUpdate
        // GameTime::update 0x14044c250: a frame is never longer than a fifth of a second
        let dt = dt.min(0.2);
        self.time_ms += dt as f64 * 1000.0;
        // The pause and the replay as the game has them. Driving: P is the pause menu
        // (Sim::setPauseMode). A replay: P is the transport's pause (ReplayManager::pause /
        // play), and the replay pauses itself at its end; no pause menu shows.
        let replay_status = match (info.replay, info.paused || info.replay_over) {
            (false, _) => -1,
            (true, false) => 0,
            (true, true) => 1,
        };
        if let Some(car) = &mut self.car {
            if info.replay {
                if self.told_replay_status == -1 {
                    // ReplayManager::startReplayMode: evOnReplayStarted, then status 6
                    car.on_replay_started_or_stopped(true);
                    car.on_replay_status_changed(&mut self.scene, 6, 1.0, 2.0);
                }
                if replay_status != self.told_replay_status {
                    car.on_replay_status_changed(&mut self.scene, replay_status, if replay_status == 1 { 0.0 } else { 1.0 }, 2.0);
                    if let Some(grooves) = &mut self.grooves {
                        grooves.reset_to_target = true;
                    }
                }
            } else if info.paused != self.told_paused {
                car.on_pause_mode_changed(info.paused);
            }
        }
        self.told_paused = info.paused && !info.replay;
        self.told_replay_status = replay_status;
        // SunAnimator::update: the sun, the lighting's clock, the clouds' drift. A rustyAC
        // replay is the drive run again, so its sun moves as it did; a paused replay's stands.
        if let Some(sun) = &mut self.sun {
            if replay_status != 1 {
                sun.update(&mut self.graphics, self.camera.base.sky_box.as_mut(), dt, self.told_paused, None);
            }
        }
        let mut modes = (2, 0);
        if let Some(car) = &mut self.car {
            car.game_time_ms = self.time_ms;
            let state = view.physics_state();
            // ACCameraManager::mode and CameraDrivableManager::currentMode
            let (camera_mode, drivable_mode) = match driving.mode {
                CameraMode::Cockpit => (0, 0),
                CameraMode::Drivable => (
                    2,
                    match driving.drivable {
                        Drivable::Chase => 0,
                        Drivable::Chase2 => 1,
                        Drivable::Bonnet => 2,
                        Drivable::Bumper => 3,
                        Drivable::Dash => 4,
                    },
                ),
                CameraMode::Car => (4, 0),
            };
            modes = (camera_mode, drivable_mode);
            car.view = rustyac_render::car::ViewState { camera_mode, drivable_mode, focused_car_index: 0, camera_position: [frame.matrix[3][0], frame.matrix[3][1], frame.matrix[3][2]], camera_matrix: rustyac_physics::vecmath::Mat44f { m: frame.matrix }, use_pro_view: false };
            // what the car's objects read beside the physics state: the aids' levels and the air
            // (CarAvatar::getTCMode, getABSMode, PhysicsEngine::ambientTemperature), the wings
            // (CarAvatar::wingsStatus), CarPhysicsInfo, the session, the pause menu and the replay
            car.tc_level = view.tc_mode.0;
            car.abs_level = view.abs_mode.0;
            car.ambient_temperature = view.air;
            car.wing_angles.clear();
            car.wing_angles.extend_from_slice(&view.wing_angles[..view.wing_count as usize]);
            car.max_fuel = view.max_fuel;
            car.kers_max_j = view.kers_max_j;
            car.ers_max_j = view.ers_max_j;
            car.has_kers = view.hybrid.has_kers;
            car.has_energy_store = view.hybrid.has_kers || view.hybrid.has_ers;
            car.drivetrain = Some((view.total_torque, view.drive_ratio));
            car.pause_menu = self.told_paused;
            car.replay_mode = info.replay;
            car.replay_scale = if replay_status == 1 { 0.0 } else { 1.0 };
            // a session of one car: rustyAC's sessions are practice (hot lap from the line)
            car.session_type = 1;
            car.leaderboard_position = 1;
            car.real_time_position = 0;
            car.cars_count = 1;
            car.update(&mut self.graphics, &mut self.scene, &state, dt);
            car.post_update(&mut self.scene, &state, dt, &Mat44f { m: frame.matrix }, frame.fov, false);
        }
        // TrackObject::update 0x1401cf720: the node takes its body's matrix
        for number in self.moved_objects.drain(..) {
            let (node, home) = self.object_nodes[number];
            self.scene.nodes[node].matrix = home;
        }
        for &(number, matrix) in &view.moved_objects[..view.moved_object_count as usize] {
            if let Some(&(node, _)) = self.object_nodes.get(number as usize) {
                self.scene.nodes[node].matrix = Mat44f { m: matrix };
                self.moved_objects.push(number as usize);
            }
        }
        // DynamicTrackManager::update
        if let Some(grooves) = &mut self.grooves {
            grooves.update(&mut self.scene, dt, view.dynamic_track.then_some(view.grip), &[view.lap.laps]);
        }
        self.frames += 1;
        let logging = self.options.gpu_log.is_some() && self.frames == 2;
        if logging {
            rustyac_render::gpulog::begin_capture("rustyac.exe");
        }
        self.graphics.begin_scene();
        self.scene.traverse(self.root);
        // Sim::renderScene: the mirror before the picture
        if let (Some(mirror), Some(car)) = (&mut self.mirror, &self.car) {
            let active = self.virtual_mirror.as_ref().is_some_and(|v| v.active);
            if mirror.wants_render(modes.0, modes.1, active) {
                let nodes = car.visibility_nodes();
                let s = rustyac_render::mirror::MirrorScene {
                    root: self.root,
                    particles: self.sim_nodes.particles,
                    before_cars: self.sim_nodes.before_cars,
                    ideal_line: None,
                    body_transform: car.body_transform,
                    car_nodes: &nodes,
                    mirror_position: car.mirror_position,
                };
                mirror.render(&mut self.graphics, &mut self.scene, self.camera.base.sky_box.as_mut(), &s);
            }
        }
        if let Err(message) = self.camera.render(&mut self.graphics, &mut self.scene, Some(self.blurred), self.root) {
            eprintln!("WARNING: the frame was not drawn: {message}");
        }
        self.draw_calls = self.graphics.stats.dip_calls;
        self.triangles = self.graphics.stats.triangles;
        self.graphics.set_screen_space_mode();
        // Game::evOnPreGUI
        if let (Some(mirror), Some(v)) = (&self.mirror, &mut self.virtual_mirror) {
            v.render(&mut self.graphics, &mirror.texture, modes.0);
        }
        if logging {
            let capture = rustyac_render::gpulog::end_capture();
            if let Some(path) = &self.options.gpu_log {
                match std::fs::write(path, &capture.text) {
                    Ok(()) => println!("{}: the command log of frame 2 ({} calls, {} draws)", path.display(), capture.lines, capture.draws),
                    Err(e) => eprintln!("WARNING: {}: {e}", path.display()),
                }
            }
        }
        // the 2D drawing: GraphicsManager::resetRenderStates, then rustyAC's own HUD
        self.graphics.reset_render_states();
        self.graphics.kgl.set_screen_render_targets(true);
        let (width, height) = self.size();
        self.hud.draw(&self.graphics.kgl.context, view, info, width, height);
    }

    /// The last picture as RGBA bytes, rows from the top.
    pub fn read_pixels(&mut self) -> Result<Vec<u8>, String> {
        let (_, _, mut pixels) = self.graphics.kgl.read_screen()?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(pixels)
    }

    /// Waits until the card has really drawn what was asked for.
    pub fn finish(&self) {
        // SAFETY: a query created, issued and polled on the live context.
        unsafe {
            let desc = D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 };
            let mut query = None;
            if self.graphics.kgl.device.CreateQuery(&desc, Some(&mut query)).is_err() {
                return;
            }
            let Some(query) = query else { return };
            let context = &self.graphics.kgl.context;
            context.End(&query);
            context.Flush();
            let mut done: i32 = 0;
            for _ in 0..200_000 {
                let hr = context.GetData(&query, Some(&mut done as *mut i32 as *mut _), 4, 0);
                if hr.is_ok() && done != 0 {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }

    /// A swap chain on a window, for [`AcRenderer::present`].
    pub fn swap_chain(&self, window: HWND) -> Result<IDXGISwapChain1, String> {
        // SAFETY: COM calls on the live device with a valid window handle.
        unsafe {
            let factory: IDXGIFactory2 = self.graphics.kgl.device.cast::<IDXGIDevice>().and_then(|d| d.GetAdapter()).and_then(|a| a.GetParent()).map_err(err("DXGI factory"))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: 0,
                Height: 0,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            let chain = factory.CreateSwapChainForHwnd(&self.graphics.kgl.device, window, &desc, None, None).map_err(err("swap chain"))?;
            let _ = factory.MakeWindowAssociation(window, DXGI_MWA_NO_ALT_ENTER);
            Ok(chain)
        }
    }

    /// The window changed its size: the swap chain and the picture follow.
    pub fn resize_swap_chain(&mut self, chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), String> {
        // SAFETY: nothing holds the back buffers between two frames.
        unsafe {
            self.graphics.kgl.context.OMSetRenderTargets(None, None);
            chain.ResizeBuffers(0, width.max(16), height.max(16), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(err("resizing the swap chain"))?;
        }
        self.graphics.on_resize(width.max(16) as i32, height.max(16) as i32)
    }

    /// Shows the last picture in the window. `Ok(false)`: the window is covered or minimised.
    pub fn present(&self, chain: &IDXGISwapChain1, vsync: bool) -> Result<bool, String> {
        // SAFETY: the back buffer and the picture have the same size and format.
        unsafe {
            let back: ID3D11Texture2D = chain.GetBuffer(0).map_err(err("back buffer"))?;
            if let Some(screen) = &self.graphics.kgl.screen_render_target.texture {
                self.graphics.kgl.context.CopyResource(&back, screen);
            }
            let status = chain.Present(vsync as u32, DXGI_PRESENT(0));
            status.ok().map_err(err("present"))?;
            Ok(status.0 != 0x087A_0001)
        }
    }
}
