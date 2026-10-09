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
}

/// What `cfg/video.ini` asks for that this renderer does not do yet: one line each.
fn video_settings(width: u32, height: u32, exact: bool) -> (VideoSettings, i32, i32, Vec<String>) {
    let mut video = VideoSettings { width: width as i32, height: height as i32, is_fullscreen: false, ..VideoSettings::default() };
    let mut notes = Vec::new();
    let (mut cube_size, mut cube_faces) = (512, 0);
    let documents = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents/Assetto Corsa/cfg/video.ini"));
    let Some(ini) = documents.and_then(|p| IniReader::load(&p).ok()) else {
        notes.push("no Documents/Assetto Corsa/cfg/video.ini: the game's defaults are used".to_string());
        video.world_detail = 5;
        return (video, cube_size, cube_faces, notes);
    };
    let int = |section: &str, key: &str| ini.get_int(section, key).unwrap_or(0);
    video.anisotropic = int("VIDEO", "ANISOTROPIC");
    video.shadow_map_size = match int("VIDEO", "SHADOW_MAP_SIZE") {
        0 => 2048,
        size => size,
    };
    video.world_detail = int("ASSETTOCORSA", "WORLD_DETAIL");
    cube_size = int("CUBEMAP", "SIZE");
    cube_faces = int("CUBEMAP", "FACES_PER_FRAME").clamp(0, 6);
    if int("VIDEO", "AASAMPLES") > 1 {
        notes.push(format!("video.ini AASAMPLES={}: multisampling comes in Task 22, the picture has one sample per pixel", int("VIDEO", "AASAMPLES")));
    }
    if int("POST_PROCESS", "ENABLED") != 0 {
        notes.push("video.ini [POST_PROCESS] ENABLED=1: post-processing comes in Task 22, the plain picture is drawn".to_string());
    }
    if ini.get_float("EFFECTS", "MOTION_BLUR").unwrap_or(0.0) > 0.0 {
        notes.push("video.ini MOTION_BLUR: motion blur comes in Task 22, treated as off".to_string());
    }
    if cube_faces > 0 {
        notes.push(format!("video.ini [CUBEMAP] FACES_PER_FRAME={cube_faces}: reflections that follow the car come in Task 21; the cube map drawn once at load is used"));
        cube_faces = 0;
    }
    if int("MIRROR", "SIZE") != 0 {
        notes.push("video.ini [MIRROR]: mirrors come in Task 21, treated as off".to_string());
    }
    if int("EFFECTS", "SMOKE") != 0 {
        notes.push("video.ini SMOKE: tyre smoke comes in Task 21, treated as off".to_string());
    }
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
    (video, cube_size, cube_faces, notes)
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
    car: Option<CarAvatar>,
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
        let (video, cube_size, cube_faces, notes) = video_settings(width, height, options.video_exact);
        let mut graphics = Graphics::new(video, DeviceOptions { warp: options.warp, window: None, log: options.gpu_log.is_some() }, &options.game)?;
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
        camera.base.sky_box = Some(SkyBox::new(&mut graphics)?);
        camera.base.camera.near_plane = 0.05;
        camera.base.camera.far_plane = 40000.0;
        camera.cube_map_renderer.faces_per_frame = cube_faces;
        camera.set_cubemap_size(&graphics, cube_size);
        let hud = HudPass::new(&graphics.kgl.device)?;
        Ok(AcRenderer { graphics, scene, camera, root, blurred, track_node, cars, car: None, track_folder: None, options, hud, adapter, software: false, draw_calls: 0, triangles: 0, frames: 0, notes })
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
        self.track_folder = Some(path_text(folder));
        let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
        self.graphics.load_track_lighting(&data.join("data/lighting.ini"));
        Ok(format!("track models: {} files, {} textures, {} materials, loaded in {:.2} s", models.len(), self.graphics.resources.textures.len(), self.scene.materials.len(), started.elapsed().as_secs_f64()))
    }

    /// `CarAvatar::init3D` 0x1400d3b90. `folder` is the car's folder in the game.
    pub fn load_car(&mut self, folder: &Path, steer_lock: f32) -> Result<String, String> {
        let started = std::time::Instant::now();
        let skin = match &self.options.skin {
            Some(skin) => skin.clone(),
            None => {
                let mut skins: Vec<String> = std::fs::read_dir(folder.join("skins")).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
                skins.sort();
                skins.first().cloned().unwrap_or_default()
            }
        };
        let car = CarAvatar::init_3d(&mut self.graphics, &mut self.scene, self.cars, &path_text(folder), folder, &skin, Some(steer_lock))?;
        let summary = format!("car model {}: {} levels of detail, skin {skin}, loaded in {:.2} s", folder.display(), car.lods.len(), started.elapsed().as_secs_f64());
        self.car = Some(car);
        Ok(summary)
    }

    /// What follows the loading in the game: the sun (`RaceManager::initLighting`), the weather
    /// (`Sim::applyCustomWeather`) and the reflection cube map (`Sim::initStaticCubemap`).
    pub fn finish_loading(&mut self) -> Result<(), String> {
        self.graphics.set_sun_angle(self.options.sun_angle);
        if !self.options.weather.is_empty() {
            self.graphics.apply_custom_weather(&self.options.weather.clone());
        }
        rustyac_render::cubemap::init_static_cubemap(&mut self.camera, &mut self.graphics, &mut self.scene, self.track_folder.as_deref())
    }

    pub fn size(&self) -> (u32, u32) {
        (self.graphics.video.width as u32, self.graphics.video.height as u32)
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
        if let Some(car) = &mut self.car {
            let state = view.physics_state();
            car.update(&mut self.scene, &state, dt);
            car.post_update(&mut self.scene, &state, dt, &Mat44f { m: frame.matrix }, frame.fov, false);
        }
        self.frames += 1;
        let logging = self.options.gpu_log.is_some() && self.frames == 2;
        if logging {
            rustyac_render::gpulog::begin_capture("rustyac.exe");
        }
        self.graphics.begin_scene();
        self.scene.traverse(self.root);
        if let Err(message) = self.camera.render(&mut self.graphics, &mut self.scene, Some(self.blurred), self.root) {
            eprintln!("WARNING: the frame was not drawn: {message}");
        }
        self.draw_calls = self.graphics.stats.dip_calls;
        self.triangles = self.graphics.stats.triangles;
        self.graphics.set_screen_space_mode();
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
