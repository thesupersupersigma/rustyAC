// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's picture drawn by `rustyac-render`, the port of Assetto Corsa's own renderer:
//! what `Sim` does around it (the scene graph, loading the track and the car, the camera of
//! the moment, the frame) and rustyAC's own HUD on top.

use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::car::CarAvatar;
use rustyac_render::post::CameraForwardYebis;
use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;
use rustyac_render::model::{path_text, Kn5Io};
use rustyac_render::scene::{NodeId, Scene};
use rustyac_render::sky::SkyBox;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::core::{Interface, PCSTR};
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Foundation::HWND;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Graphics::Direct3D::ID3DBlob;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Graphics::Direct3D11::*;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Graphics::Dxgi::Common::*;
#[cfg(all(windows, not(feature = "wgpu")))]
use windows::Win32::Graphics::Dxgi::*;

#[cfg(all(windows, not(feature = "wgpu")))]
use super::font::FontBitmap;
use super::hud::HudInfo;
#[cfg(all(windows, not(feature = "wgpu")))]
use super::hud::{self, HudVertex};
use super::scene::{CameraFrame, CameraMode, Drivable, DrivingCamera};
use crate::view::CarView;
#[cfg(all(windows, not(feature = "wgpu")))]
use std::time::Instant;

/// shortcut: std's clock panics in the browser, so the load and frame times read 0 there; use the
/// page's `performance.now()` once those times are shown in the page (the HUD task).
#[cfg(any(not(windows), feature = "wgpu"))]
struct Instant;
#[cfg(any(not(windows), feature = "wgpu"))]
impl Instant {
    fn now() -> Instant {
        Instant
    }
    fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

#[cfg(all(windows, not(feature = "wgpu")))]
const HUD_CAPACITY: usize = 24_000;

#[cfg(all(windows, not(feature = "wgpu")))]
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

#[cfg(all(windows, not(feature = "wgpu")))]
fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

#[cfg(all(windows, not(feature = "wgpu")))]
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
#[cfg(all(windows, not(feature = "wgpu")))]
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

#[cfg(all(windows, not(feature = "wgpu")))]
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

/// kgl on wgpu's adapter: in the page the page's device (given to kgl beforehand, this is not
/// looked at); natively (the proofs) WARP with `warp`, else RUSTYAC_WGPU_ADAPTER=gpu (the
/// default) or swiftshader (Vulkan on the CPU, picked by VK_DRIVER_FILES).
#[cfg(any(not(windows), feature = "wgpu"))]
fn wgpu_adapter(warp: bool) -> Result<rustyac_render::kgl::Adapter, String> {
    use rustyac_render::kgl::Adapter;
    Ok(match (warp, std::env::var("RUSTYAC_WGPU_ADAPTER").as_deref()) {
        (true, _) => Adapter::Warp,
        (_, Ok("gpu") | Err(_)) => Adapter::Gpu,
        (_, Ok("swiftshader")) => Adapter::SwiftShader,
        (_, Ok(other)) => return Err(format!("RUSTYAC_WGPU_ADAPTER={other:?} is not gpu or swiftshader")),
    })
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
    /// `--world-detail <0..5>`: `[ASSETTOCORSA] WORLD_DETAIL` whatever `video.ini` says
    pub world_detail: Option<i32>,
    /// what `rand()` starts from once the track is loaded
    pub render_seed: u32,
    /// `--cube-size <n>`: `[CUBEMAP] SIZE` whatever the video settings say
    pub cube_size: Option<i32>,
    /// `assists.ini [ASSISTS] IDEAL_LINE` (or `--ideal-line`): the ideal line shows from the start
    pub ideal_line: bool,
    /// the session's kind as the pit box marker reads it (1 practice, 2 qualifying, 3 race:
    /// the marker can show; 4 hot lap and above: never)
    pub session_type: i32,
    /// a race start: the seconds the start lights count down from (`RaceManager::startRace`
    /// gives 20, give or take one); `None`: a session that has started (no countdown)
    pub start_lights: Option<f32>,
    /// `--video-ini <file>`: this file instead of `Documents/Assetto Corsa/cfg/video.ini`
    pub video_ini: Option<PathBuf>,
    /// `--graphics max|low`: a built-in `video.ini`; the player's file is not read
    pub preset: Option<GraphicsPreset>,
    /// `--aa <1|2|4|8>`: `[VIDEO] AASAMPLES` whatever the video settings say
    pub aa_samples: Option<i32>,
    /// `--post <0|1>`, `--fxaa <0|1>`, `--motion-blur <0..12>`: `[POST_PROCESS] ENABLED`, `FXAA`
    /// and `[EFFECTS] MOTION_BLUR` whatever the video settings say
    pub post: Option<bool>,
    pub fxaa: Option<bool>,
    pub motion_blur: Option<f32>,
    /// `--hdr-levels`: with post-processing on, the engine's HDR light levels as in the game
    /// (washed out until YEBIS is there to map them)
    pub hdr_levels: bool,
}

/// What `race.ini` gives the renderer (rustyac.exe and the page's Full mode read it the same
/// way); without the file: rustyAC's own defaults, midday and clear with the sun moving.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceLook {
    pub sun_angle: f32,
    pub sun_animation: Option<(f32, f32)>,
    pub weather: String,
    pub groove: Option<(f32, f32)>,
    /// `[SESSION_0] TYPE` when above 0
    pub session_type: Option<i32>,
}

impl RaceLook {
    pub fn of(race: Option<&IniReader>) -> RaceLook {
        let mut look = RaceLook { sun_angle: -16.0, sun_animation: Some((1.0, 0.2)), weather: "3_clear".to_string(), groove: None, session_type: None };
        let Some(ini) = race else { return look };
        if ini.has_section("LIGHTING") {
            look.sun_angle = ini.get_float("LIGHTING", "SUN_ANGLE").unwrap_or(look.sun_angle);
            look.sun_animation = Some((ini.get_float("LIGHTING", "TIME_MULT").unwrap_or(0.0), ini.get_float("LIGHTING", "CLOUD_SPEED").unwrap_or(0.0)));
        } else {
            look.sun_animation = None;
        }
        look.session_type = ini.get_int("SESSION_0", "TYPE").ok().filter(|t| *t > 0);
        look.weather = if ini.has_section("WEATHER") { ini.get_string("WEATHER", "NAME") } else { String::new() };
        if ini.has_section("GROOVE") {
            look.groove = Some((ini.get_float("GROOVE", "MAX_LAPS").unwrap_or(0.0), ini.get_float("GROOVE", "STARTING_LAPS").unwrap_or(0.0)));
        }
        look
    }

    /// A race start: the 20 s of `RaceManager::startRace` when the session is a race.
    pub fn start_lights(&self) -> Option<f32> {
        (self.session_type == Some(3)).then_some(20.0)
    }
}

/// `--graphics max|low`: every setting of `video.ini` this renderer knows at the highest / the
/// lowest value Assetto Corsa's own launcher offers for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsPreset {
    Max,
    Low,
}

impl GraphicsPreset {
    pub fn from_name(name: &str) -> Result<GraphicsPreset, String> {
        match name {
            "max" => Ok(GraphicsPreset::Max),
            "low" => Ok(GraphicsPreset::Low),
            other => Err(format!("--graphics {other}: max or low")),
        }
    }

    /// The `video.ini` the launcher would write with every slider at its end, four samples per
    /// pixel, and post-processing off: `[POST_PROCESS] ENABLED=0` until YEBIS is ported (Task
    /// 23b). The post-processing frame without YEBIS' tone mapping is a picture the game never
    /// draws (Task 23), so the preset does not switch it on; with it off, FXAA and motion blur
    /// do not exist in acs.exe either. `--post 1 --fxaa 1 --motion-blur 6` ask for that frame.
    ///
    /// The reflection cube map is 1024, not the launcher's highest 2048 (Task 22c): with six
    /// faces drawn again every frame a 2048 map costs 13 to 50 ms a frame on the PC this was
    /// measured on (plain `acs.exe` pays the same: its own renderer was timed beside the port's),
    /// 1024 costs 3 ms more than 512 or nothing. `--cube-size 2048` gives the launcher's.
    fn video_ini(self) -> &'static str {
        match self {
            GraphicsPreset::Max => {
                "[VIDEO]\nANISOTROPIC=16\nAASAMPLES=4\nSHADOW_MAP_SIZE=4096\n[ASSETTOCORSA]\nWORLD_DETAIL=5\n[CUBEMAP]\nSIZE=1024\nFACES_PER_FRAME=6\n[EFFECTS]\nSMOKE=5\nRENDER_SMOKE_IN_MIRROR=1\nMOTION_BLUR=0\n[MIRROR]\nSIZE=1024\nHQ=1\n[POST_PROCESS]\nENABLED=0\nFXAA=0\n"
            }
            GraphicsPreset::Low => {
                "[VIDEO]\nANISOTROPIC=0\nAASAMPLES=1\nSHADOW_MAP_SIZE=512\n[ASSETTOCORSA]\nWORLD_DETAIL=0\n[CUBEMAP]\nSIZE=0\nFACES_PER_FRAME=0\n[EFFECTS]\nSMOKE=0\nRENDER_SMOKE_IN_MIRROR=0\nMOTION_BLUR=0\n[MIRROR]\nSIZE=0\nHQ=0\n[POST_PROCESS]\nENABLED=0\nFXAA=0\n"
            }
        }
    }
}

/// The `video.ini` of this run and what to call it: `--graphics`' built-in one, `--video-ini`'s
/// file, or the player's own (which is only ever read).
fn video_ini_of(options: &AcOptions) -> Result<(Option<IniReader>, String), String> {
    if let Some(preset) = options.preset {
        let name = if preset == GraphicsPreset::Max { "--graphics max" } else { "--graphics low" };
        return Ok((Some(IniReader::from_text(Path::new("video.ini"), preset.video_ini())), name.to_string()));
    }
    if let Some(file) = &options.video_ini {
        // (IniReader::load of a missing file is an empty file, not an error)
        if !rustyac_content::vfs::is_file(file) {
            return Err(format!("--video-ini {}: no such file", file.display()));
        }
        let ini = IniReader::load(file).map_err(|e| format!("--video-ini {}: {e}", file.display()))?;
        return Ok((Some(ini), file.display().to_string()));
    }
    // (a missing file is the game's defaults, not an empty file: IniReader::load would give one)
    let documents = rustyac_content::install::documents().map(|p| p.join("cfg/video.ini")).filter(|p| p.is_file());
    Ok((documents.and_then(|p| IniReader::load(&p).ok()), "video.ini".to_string()))
}

/// What `cfg/video.ini` asks for that this renderer does not do yet: one line each.
fn video_settings(width: u32, height: u32, exact: bool, ini: Option<&IniReader>, name: &str) -> (VideoSettings, i32, i32, f32, (bool, i32), f32, Vec<String>) {
    let mut video = VideoSettings { width: width as i32, height: height as i32, is_fullscreen: false, ..VideoSettings::default() };
    let mut notes = Vec::new();
    let (mut cube_size, mut cube_faces, mut cube_far) = (512, 0, 0.0f32);
    let Some(ini) = ini else {
        notes.push("no Documents/Assetto Corsa/cfg/video.ini: the game's defaults are used".to_string());
        video.world_detail = 5;
        // TyreSmoke / EngineSmoke without the file: the Normal level
        video.smoke = None;
        return (video, cube_size, cube_faces, cube_far, (false, 1), 0.0, notes);
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
    // loadVideoSettings 0x1400c18d0: anything below 1 is 1 (AAQUALITY is not read by acs.exe)
    video.aa_samples = int("VIDEO", "AASAMPLES").max(1);
    // loadVideoSettings: [POST_PROCESS] ENABLED and FXAA; Sim::Sim 0x140192070: [EFFECTS]
    // MOTION_BLUR, 0 to 12. In acs.exe FXAA and motion blur only exist in the post-processing
    // camera's frame ([EFFECTS] FXAA is read by nothing).
    video.pp_hdr_enabled = int("POST_PROCESS", "ENABLED") != 0;
    video.pp_fxaa = int("POST_PROCESS", "FXAA") != 0;
    let blur = ini.get_float("EFFECTS", "MOTION_BLUR").unwrap_or(0.0);
    let motion_blur = if blur > 12.0 {
        12.0
    } else if blur >= 0.0 {
        blur
    } else {
        0.0
    };
    if !video.pp_hdr_enabled && (video.pp_fxaa || motion_blur > 0.0) {
        notes.push(format!("{name} [POST_PROCESS] ENABLED=0: FXAA and MOTION_BLUR do nothing then, as in acs.exe (--fxaa 1 / --motion-blur <n> switch them on with the post-processing frame)"));
    }
    video.smoke = Some(int("EFFECTS", "SMOKE"));
    video.mirror_size = int("MIRROR", "SIZE");
    video.mirror_smoke = int("EFFECTS", "RENDER_SMOKE_IN_MIRROR") > 0;
    // the high-quality mirror's target has the screen's AASAMPLES (GraphicsManager::videoSettings)
    let mirror_hq = (int("MIRROR", "HQ") > 0, int("VIDEO", "AASAMPLES").max(1));
    // values Content Manager writes for Custom Shaders Patch: plain acs.exe cannot make a
    // shadow map or a cube map of such a size and then draws everything in shadow, without
    // reflections (checked with the game's own code in the render oracle)
    if video.shadow_map_size < 0 {
        if exact {
            notes.push(format!("{name} SHADOW_MAP_SIZE={}: no shadow map of that size can be made: everything is drawn in shadow, as plain acs.exe does", video.shadow_map_size));
        } else {
            notes.push(format!("{name} SHADOW_MAP_SIZE={} is a Custom Shaders Patch value; plain acs.exe would draw everything in shadow with it. 2048 is used (--video-ini-exact: follow the file)", video.shadow_map_size));
            video.shadow_map_size = 2048;
        }
    }
    if cube_size <= 0 {
        if exact {
            notes.push(format!("{name} [CUBEMAP] SIZE={cube_size}: no cube map of that size can be made: nothing is reflected, as in plain acs.exe"));
        } else {
            notes.push(format!("{name} [CUBEMAP] SIZE={cube_size} is a Custom Shaders Patch value; plain acs.exe would reflect nothing with it. 512 is used (--video-ini-exact: follow the file)"));
            cube_size = 512;
        }
    }
    (video, cube_size, cube_faces, cube_far, mirror_hq, motion_blur, notes)
}

/// The five pictures of `CarFakeShadow::generateFakeShadow` (four wheels of 64 x 64, the body of
/// 512 x 512, RGBA) as the PNG files the game would have saved, in rustyAC's own folder:
/// `<the program's folder>/rustyac_cache/shadows/<car>/`.
fn shadow_cache_folder() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.parent().ok_or("the program has no folder")?.join("rustyac_cache").join("shadows"))
}

/// … with a stamp of the car's files beside them, so that the next start finds them and makes
/// them again only when the car changed.
fn save_generated_shadows(car: &str, car_folder: &std::path::Path, pictures: &[Vec<u8>]) -> Result<PathBuf, String> {
    let folder = shadow_cache_folder()?.join(car);
    // (the stamp goes last: a half-written set is never taken for a whole one)
    let _ = rustyac_content::guard::remove_file(folder.join(rustyac_render::fake_shadow::STAMP_FILE));
    rustyac_content::guard::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for (i, picture) in pictures.iter().enumerate() {
        let (name, side) = if i < 4 { (format!("tyre_{i}_shadow.png"), 64u32) } else { ("body_shadow.png".to_string(), 512u32) };
        let file = rustyac_content::guard::create(folder.join(&name)).map_err(|e| format!("{name}: {e}"))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(picture).map_err(|e| e.to_string())?;
    }
    rustyac_content::guard::write(folder.join(rustyac_render::fake_shadow::STAMP_FILE), rustyac_render::fake_shadow::car_files_stamp(car_folder)).map_err(|e| e.to_string())?;
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
/// A picture read back from the screen: width, height, RGBA (or why it could not be read).
pub type ScreenShot = Result<(u32, u32, Vec<u8>), String>;

pub struct AcRenderer {
    graphics: Graphics,
    scene: Scene,
    camera: CameraForwardYebis,
    root: NodeId,
    blurred: NodeId,
    unblurred: NodeId,
    track_node: NodeId,
    cars_node: NodeId,
    car_shadows: NodeId,
    sim_nodes: rustyac_render::car::SimNodes,
    /// `video.ini [ASSETTOCORSA] LOCK_STEER`, `HIDE_ARMS`, `HIDE_STEER`
    cockpit_flags: (bool, bool, bool),
    /// `Sim::cars`: every car of the session in the order they were made (the index is the
    /// car's guid; 0 is the player's)
    cars: Vec<CarAvatar>,
    /// each car's folder and skin, for the pit crews
    crew_cars: Vec<rustyac_render::pit_crew::CrewCar>,
    /// Task 28c: a replay shows: every car's state comes from its recorder, not from its view
    pub replay_states: Option<Vec<rustyac_physics::car_physics_state::CarPhysicsState>>,
    /// `GhostCar`: the see-through car of the player's best lap, and its recorder (the physics
    /// thread's) when the session has one
    ghost: Option<rustyac_render::ghost_car::GhostCar>,
    pub ghost_rig: Option<std::sync::Arc<std::sync::Mutex<crate::ghost::GhostRig>>>,
    /// `PitCrew`: the four mechanics who change the player's tyres
    tyre_crew: Option<rustyac_render::tyre_crew::PitCrew>,
    /// the crew's sound and camera, between the picture and the game's loop
    pub crew: CrewLink,
    /// the commits and the ends of pit stops the crew was told of
    crew_seen: (u32, u32),
    /// the camera fade a commit asked for: (`fadeTime`, `fadeTotalTime`)
    crew_fade: Option<(f32, f32)>,
    crew_inputs: Option<rustyac_render::tyre_crew::CrewInputs>,
    /// `RaceManager::carsToBeLoaded`
    cars_to_be_loaded: i32,
    /// `ACCameraManager::currentGlobalCameraIndex`: what `CarNodeSorter::sort` reads
    global_camera_index: i32,
    /// `Sim::mirrorTextureRenderer` and `Sim::virtualMirrorRenderer`: only with mirrors on
    mirror: Option<rustyac_render::mirror::MirrorTextureRenderer>,
    virtual_mirror: Option<rustyac_render::mirror::VirtualMirrorRenderer>,
    /// `TrackAvatar::idealLine` and `DrivingAssistManager::idealLine`
    ideal_line: Option<NodeId>,
    ideal_on: bool,
    /// `StartingLights`, `StartingTrackSemaphore`, and `RaceManager::currentSession.startTime`
    /// on the picture's clock (`None`: no race start)
    /// `TrackAvatar::lollipopCrews`, the marker of `PitStop`, and what they are built from
    /// once the car is there: the track's model, its data folder, its pit boxes (on the road)
    /// and the track itself for the rays
    lollipops: Vec<rustyac_render::pit_crew::LollipopCrew>,
    pit_marker: Option<rustyac_render::pit_crew::PitIndicator>,
    pit_setup: Option<(NodeId, PathBuf, Vec<Mat44f>, rustyac_physics::track::Track)>,
    /// the gates of a time-attack session (`TimeAttack`) and the time lines of the track
    gates: Option<std::rc::Rc<std::cell::RefCell<rustyac_render::time_attack::TimeAttackGates>>>,
    gate_sectors: usize,
    start_lights: Option<rustyac_render::start_lights::StartingLights>,
    semaphore: Option<rustyac_render::start_lights::StartingTrackSemaphore>,
    race_start_ms: Option<f64>,
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
    /// the status told in the frame before this one
    told_replay_status_before: i32,
    /// `CarView::new_sessions` of the last frame
    told_new_sessions: u32,
    /// `TrackAvatar::dynamicObjects`: balloons and aircraft that drift over the track
    dynamic_objects: Vec<rustyac_render::crowds::DynamicTrackObject>,
    track_folder: Option<String>,
    options: AcOptions,
    #[cfg(all(windows, not(feature = "wgpu")))]
    hud: HudPass,
    /// AC's GUI and HUD (none: only the 2D pass's states are set, as with no form on the screen)
    pub gui: Option<Box<super::gui::GameGui>>,
    /// Task 43c: FormWings' handler of the render-finished node (made with the GUI)
    wings_outlines: Option<std::rc::Rc<std::cell::RefCell<rustyac_render::wings::WingsOutlines>>>,
    suspension_lines: Option<std::rc::Rc<std::cell::RefCell<rustyac_render::suspensions::SuspensionWireframe>>>,
    /// the picture `ScreenCapturer` took this frame (the scene before the GUI and the HUD)
    pub pre_gui_shot: Option<ScreenShot>,
    /// Task 40: the 2D pass's size when it is not the 3D picture's (the page's render scale:
    /// AC's GUI at the window's size over a smaller picture; kgl on wgpu only)
    pub gui_size: Option<(i32, i32)>,
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
    /// `GraphicsManager::useCustomSunDirection` / `customSunDirection` (the Photo mode's sun);
    /// None: the sun of the time of day.
    /// Photo mode's END: `RayPicker::pick` at the screen's centre from a camera (its field of
    /// view, its matrix) against the whole scene (`Sim::rootNode`).
    pub fn pick_centre(&self, fov: f32, matrix: &rustyac_physics::vecmath::Mat44f) -> f32 {
        let (w, h) = (self.graphics.video.width, self.graphics.video.height);
        self.scene.ray_picker_pick(self.root, w as f32, h as f32, w / 2, h / 2, fov, matrix)
    }

    pub fn set_custom_sun(&mut self, direction: Option<rustyac_physics::vecmath::Vec3f>) {
        self.graphics.use_custom_sun_direction = direction.is_some();
        if let Some(d) = direction {
            self.graphics.custom_sun_direction = d;
        }
    }

    pub fn new(width: u32, height: u32, options: AcOptions) -> Result<AcRenderer, String> {
        let (video_ini, video_name) = video_ini_of(&options)?;
        // a preset is followed to the letter: the launcher's "None" for reflections is SIZE=0
        let exact = options.video_exact || options.preset.is_some();
        let (mut video, cube_size, mut cube_faces, cube_far, mirror_hq, mut motion_blur, mut notes) = video_settings(width, height, exact, video_ini.as_ref(), &video_name);
        if let Some(size) = options.cube_size {
            // (what the file's own size would have been is no longer of interest)
            notes.retain(|n| !n.contains("[CUBEMAP] SIZE="));
            notes.push(format!("--cube-size {size}: the reflection cube map's side"));
        }
        let cube_size = options.cube_size.unwrap_or(cube_size);
        if options.preset.is_some() {
            notes.retain(|n| !n.contains("[CUBEMAP] SIZE=0"));
            notes.insert(0, format!("{video_name}: the built-in settings, your video.ini is not read"));
            if options.preset == Some(GraphicsPreset::Max) {
                notes.insert(1, "  (4 samples per pixel; post-processing stays off until YEBIS is ported: --post 1 --fxaa 1 --motion-blur 6 switch its frame on)".to_string());
            }
        } else if options.video_ini.is_some() {
            notes.insert(0, format!("the video settings of {video_name}"));
        }
        if let Some(samples) = options.aa_samples {
            video.aa_samples = samples;
            notes.push(format!("--aa {samples}: samples per pixel"));
        }
        // --post / --fxaa / --motion-blur: the settings of the post-processing frame whatever
        // the video settings say (FXAA and blur bring the frame with them: they live in it)
        if let Some(on) = options.post {
            video.pp_hdr_enabled = on;
        }
        if let Some(on) = options.fxaa {
            video.pp_fxaa = on;
            video.pp_hdr_enabled |= on;
        }
        if let Some(amount) = options.motion_blur {
            motion_blur = amount;
            video.pp_hdr_enabled |= amount > 0.0;
        }
        if video.pp_hdr_enabled {
            notes.retain(|n| !n.contains("do nothing then"));
            // YEBIS is Task 23b: without its tone mapper the engine's HDR levels wash the picture
            // out, so everything outside the camera's frame keeps the levels of "off"
            video.ldr_levels = !options.hdr_levels;
            let mut parts = vec![format!("{} samples", video.aa_samples)];
            if video.pp_fxaa {
                parts.push("FXAA".to_string());
            }
            if motion_blur > 0.0 {
                parts.push(format!("motion blur {motion_blur}"));
            }
            notes.push(format!(
                "post-processing on: the game's post-processing frame ({}) without YEBIS itself (its filter, tone mapping, glare, depth of field come in Task 23b){}",
                parts.join(", "),
                if options.hdr_levels { "; --hdr-levels: the engine's HDR light levels, which only YEBIS brings back into range" } else { "; the light levels stay those of post-processing off" }
            ));
        }
        // the high-quality mirror's target has the screen's samples
        let mirror_hq = (options.mirror_hq.unwrap_or(mirror_hq.0), video.aa_samples);
        if let Some(size) = options.mirror_size {
            video.mirror_size = size;
        }
        if let Some(detail) = options.world_detail {
            video.world_detail = detail.clamp(0, 5);
        }
        if let Some(faces) = options.cube_faces {
            cube_faces = faces.clamp(0, 6);
        }
        #[cfg(all(windows, not(feature = "wgpu")))]
        let device = DeviceOptions { warp: options.warp, window: None, log: options.gpu_log.is_some() };
        // (the page's device, given to kgl beforehand: the adapter is the page's choice)
        #[cfg(any(not(windows), feature = "wgpu"))]
        let device = DeviceOptions { adapter: wgpu_adapter(options.warp)?, log: options.gpu_log.is_some() };
        #[cfg(any(not(windows), feature = "wgpu"))]
        let adapter = format!("{:?} on wgpu", device.adapter);
        let mut graphics = Graphics::new(video, device, &options.game)?;
        // Sim::Sim: srand(the time in milliseconds)
        graphics.crt_rand = rustyac_physics::session::MsvcRand(options.render_seed);
        // SAFETY: COM calls on the live device.
        #[cfg(all(windows, not(feature = "wgpu")))]
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
        // Sim::createCamera 0x1401982e0: the plain camera, or the post-processing one (the same
        // thing when post-processing is off); blur only with post-processing. Sim::Sim: the
        // solve's quality is twice the setting.
        // Sim::initHDRLevels 0x140199b00 (the camera managers hand the pair to the camera)
        let hdr_levels = rustyac_render::post::init_hdr_levels(&options.game);
        let use_blur = motion_blur > 0.0 && graphics.video.pp_hdr_enabled;
        let mut camera = CameraForwardYebis::new(&mut graphics, use_blur)?;
        camera.blur_quality = motion_blur * 2.0;
        camera.base.camera.min_exposure = hdr_levels.min_exposure;
        camera.base.camera.max_exposure = hdr_levels.max_exposure;
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
        if std::env::var_os("RUSTYAC_DIAG_NO_CUBE_MIPS").is_some_and(|v| v == "1") {
            camera.cube_map_renderer.diagnosis_no_mips = true;
            notes.push("RUSTYAC_DIAG_NO_CUBE_MIPS=1: NOT Assetto Corsa, a diagnosis switch: the reflection cube map's mip levels are made once, not every frame".into());
        }
        #[cfg(all(windows, not(feature = "wgpu")))]
        let hud = HudPass::new(&graphics.kgl.device)?;
        // CarAvatar::initCommonPostPhysics: the cockpit switches of video.ini
        let cockpit_flags = {
            let ini = &video_ini;
            let flag = |key: &str| ini.as_ref().is_some_and(|i| i.has_section("ASSETTOCORSA") && i.get_int("ASSETTOCORSA", key).unwrap_or(0) != 0);
            (flag("LOCK_STEER"), flag("HIDE_ARMS"), flag("HIDE_STEER"))
        };
        // Sim::Sim: the mirror texture and the virtual mirror (gameplay.ini says whether it shows)
        let mirror = match graphics.video.mirror_size {
            0 => None,
            size => {
                let smoke = graphics.video.mirror_smoke;
                Some(rustyac_render::mirror::MirrorTextureRenderer::new(&mut graphics, size, smoke, mirror_hq.0.then_some(mirror_hq.1))?)
            }
        };
        let virtual_mirror = mirror.as_ref().map(|_| {
            let ini = rustyac_content::install::documents().map(|p| p.join("cfg/gameplay.ini")).and_then(|p| IniReader::load(&p).ok());
            let active = match options.preset {
                // every picture there is / the fewest
                Some(preset) => preset == GraphicsPreset::Max,
                None => ini.is_some_and(|i| i.has_section("VIRTUAL_MIRROR") && i.get_int("VIRTUAL_MIRROR", "ACTIVE").unwrap_or(0) != 0),
            };
            rustyac_render::mirror::VirtualMirrorRenderer::new(&graphics, active)
        });
        Ok(AcRenderer { pre_gui_shot: None, gui_size: None, ghost: None, ghost_rig: None, mirror, virtual_mirror, ideal_line: None, ideal_on: options.ideal_line, lollipops: Vec::new(), pit_marker: None, pit_setup: None, gates: None, gate_sectors: 0, start_lights: None, semaphore: None, race_start_ms: None, time_ms: 0.0, sun: None, grooves: None, crowds: Vec::new(), object_nodes: Vec::new(), moved_objects: Vec::new(), told_paused: false, told_replay_status: -1, told_replay_status_before: -1, told_new_sessions: 0, dynamic_objects: Vec::new(), graphics, scene, camera, root, blurred, unblurred, track_node, cars_node: cars, car_shadows, sim_nodes: rustyac_render::car::SimNodes { root, cars, skid_marks, particles, car_shadows, before_cars, render_finished }, cockpit_flags, cars: Vec::new(), crew_cars: Vec::new(), replay_states: None, tyre_crew: None, crew: CrewLink::default(), crew_seen: (0, 0), crew_fade: None, crew_inputs: None, cars_to_be_loaded: 1, global_camera_index: 0, track_folder: None, options, #[cfg(all(windows, not(feature = "wgpu")))] hud, gui: None, wings_outlines: None, suspension_lines: None, adapter, software: false, draw_calls: 0, triangles: 0, frames: 0, notes })
    }

    /// AC's GUI and HUD are made (after the cars are loaded) and drawn from the next frame on.
    pub fn enable_gui(&mut self, setup: super::gui::GuiSetup) -> Result<(), String> {
        self.gui = Some(Box::new(super::gui::GameGui::new(&mut self.graphics, setup)?));
        // FormWings::FormWings adds its handler to the render-finished node (after the cars')
        let outlines = std::rc::Rc::new(std::cell::RefCell::new(rustyac_render::wings::WingsOutlines::default()));
        self.scene.add_event_handler(self.sim_nodes.render_finished, outlines.clone());
        self.wings_outlines = Some(outlines);
        // and ACSuspension::ACSuspension after it
        let lines = std::rc::Rc::new(std::cell::RefCell::new(rustyac_render::suspensions::SuspensionWireframe::new(&self.graphics)));
        self.scene.add_event_handler(self.sim_nodes.render_finished, lines.clone());
        self.suspension_lines = Some(lines);
        Ok(())
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
        let started = Instant::now();
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
        // TrackAvatar::initDynamicObjects
        self.dynamic_objects = rustyac_render::crowds::init_dynamic_objects(&mut self.graphics, &mut self.scene, folder, layout, model);
        self.track_folder = Some(path_text(folder));
        let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
        // the track as the physics has it: the rays of the ideal line and of the pit crew, the
        // pit boxes where initRespawnPositionSet put them
        match rustyac_physics::track::loader::load_track(folder, layout) {
            Ok((track, _)) => {
                // TrackAvatar::TrackAvatar: the ideal line, from data/ideal_line.ai or else the AI line
                match self.build_ideal_line(&track, &data) {
                    Ok(line) => {
                        self.scene.compile(&self.graphics, line);
                        self.scene.add_child(self.track_node, line);
                        self.scene.nodes[line].is_active = self.ideal_on;
                        self.ideal_line = Some(line);
                    }
                    Err(message) => println!("no ideal line: {message}"),
                }
                // TimeAttack::TimeAttack (a time-attack session): the gates of data/overlays.ini
                if self.options.session_type == 5 {
                    self.gate_sectors = track.sectors_normalized_positions.len();
                    let overlays = rustyac_render::time_attack::read_overlays(&data, self.gate_sectors);
                    if overlays.is_empty() {
                        println!("time attack: this track has no data/overlays.ini (Assetto Corsa writes it at the first time-attack session there): no gates");
                    } else {
                        let gates = std::rc::Rc::new(std::cell::RefCell::new(rustyac_render::time_attack::TimeAttackGates::new(&mut self.graphics, overlays)));
                        self.scene.add_event_handler(self.sim_nodes.before_cars, gates.clone());
                        self.gates = Some(gates);
                    }
                }
                let pits: Vec<Mat44f> = track.spawn_positions.get("PIT").map(|slots| slots.iter().map(|i| track.helper_nodes[*i].local).collect()).unwrap_or_default();
                self.pit_setup = Some((model, data.clone(), pits, track));
            }
            Err(message) => println!("no ideal line and no pit crew: {message}"),
        }
        self.graphics.load_track_lighting(&data.join("data/lighting.ini"));
        // TrackAvatar::TrackAvatar: the grooves, then the crowds (which leave rand() seeded)
        let grooves = rustyac_render::crowds::DynamicTrackManager::new(&mut self.scene, &data, self.track_node, self.options.groove);
        self.graphics.cube_map_hidden = grooves.meshes();
        self.grooves = Some(grooves);
        match rustyac_render::crowds::camera_facing(&mut self.graphics, &mut self.scene, &data, self.track_node, self.blurred, self.options.render_seed) {
            Ok(crowds) => self.crowds = crowds,
            Err(message) => println!("WARNING: no crowds: {message}"),
        }
        // Sim::Sim: StartingLights and StartingTrackSemaphore
        match rustyac_render::start_lights::StartingLights::new(&mut self.graphics, &mut self.scene, self.track_node, &data) {
            Ok(lights) => self.start_lights = Some(lights),
            Err(message) => println!("WARNING: no start lights: {message}"),
        }
        match rustyac_render::start_lights::StartingTrackSemaphore::new(&mut self.graphics, &mut self.scene, self.track_node, &data) {
            Ok(semaphore) => self.semaphore = Some(semaphore),
            Err(message) => println!("WARNING: no semaphore: {message}"),
        }
        self.start_race();
        Ok(format!("track models: {} files, {} textures, {} materials, loaded in {:.2} s", models.len(), self.graphics.resources.textures.len(), self.scene.materials.len(), started.elapsed().as_secs_f64()))
    }

    /// The ideal line of `TrackAvatar::TrackAvatar` 0x1401c5250: `IdealLine::IdealLine` on
    /// `data/ideal_line.ai` when the track has one, else on its AI line, with the track's own
    /// rays (straight down from every sample).
    fn build_ideal_line(&mut self, track: &rustyac_physics::track::Track, data: &Path) -> Result<NodeId, String> {
        use rustyac_physics::track::spline::AiSpline;
        use rustyac_physics::vecmath::Vec3f;
        let file = data.join("data/ideal_line.ai");
        let own;
        let spline = if rustyac_physics::data::exists(&file) {
            let bytes = rustyac_content::vfs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            own = AiSpline::parse_fast(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            &own
        } else {
            track.ai_spline.as_ref().ok_or("the track has neither data/ideal_line.ai nor an AI line")?
        };
        if spline.point_count() == 0 || spline.length() <= 0.0 || spline.length().is_nan() {
            return Err("the line has no points".into());
        }
        let down = Vec3f::new(0.0, -1.0, 0.0);
        rustyac_render::ideal_line::build(&mut self.graphics, &mut self.scene, spline, &|from| track.ray_cast_hit(&Vec3f::new(from[0], from[1], from[2]), &down, 100.0).map(|hit| hit.contact.pos))
    }

    /// `RaceManager::startRace` 0x140141470 for a race: the start is `lead` seconds from now,
    /// give or take a second (a number of `rand()`). Nothing without `--start-lights` (or a
    /// race in race.ini).
    pub fn start_race(&mut self) {
        self.race_start_ms = self.options.start_lights.map(|lead| {
            let jitter = (self.graphics.crt_rand.next() as f32 * f32::from_bits(0x3800_0100)) * 2000.0 - 1000.0;
            (lead * 1000.0) as f64 + self.time_ms + jitter as f64
        });
    }

    /// Task 28: the referee's race start, `ms` from now (`None`: no race, or none set yet).
    pub fn set_race_start_in(&mut self, ms: Option<f64>) {
        self.race_start_ms = ms.filter(|ms| ms.abs() < 1.0e9).map(|ms| self.time_ms + ms);
    }

    /// How many meshes of the track are start lights, and whether it has a semaphore.
    pub fn start_lights_summary(&self) -> (usize, bool) {
        (self.start_lights.as_ref().map_or(0, |l| l.track_light_count()), self.semaphore.as_ref().is_some_and(|s| !s.is_empty()))
    }

    /// `DrivingAssistManager::setIdealLine` 0x1400fcb00 from the key (Ctrl+I in the game): the
    /// ideal line on or off. The answer is what it is now; `None`: the track has no line.
    pub fn toggle_ideal_line(&mut self) -> Option<bool> {
        self.ideal_line?;
        self.ideal_on = !self.ideal_on;
        Some(self.ideal_on)
    }

    /// `RaceManager::carsToBeLoaded`: how many cars the session has. To be said before the
    /// first car is loaded (the tyre smoke's size depends on it).
    pub fn set_cars_to_be_loaded(&mut self, count: usize) {
        self.cars_to_be_loaded = count as i32;
    }

    /// How many cars are loaded.
    pub fn car_count(&self) -> usize {
        self.cars.len()
    }

    /// `Sim::addCar` 0x1401978a0, the picture's part: `CarAvatar::init3D` 0x1400d3b90 and
    /// `initCommonPostPhysics`. `folder` is the car's folder in the game. The first car loaded
    /// is the player's (guid 0), the next ones get guid 1, 2, … as the game counts them.
    pub fn load_car(&mut self, folder: &Path, steer_lock: f32, tyre_width: [f32; 4], max_gear: i32) -> Result<String, String> {
        let started = Instant::now();
        let skin = match &self.options.skin {
            Some(skin) => skin.clone(),
            None => {
                let mut skins: Vec<String> = rustyac_content::vfs::read_dir(&folder.join("skins")).map(|d| d.into_iter().filter(|(_, dir)| *dir).map(|(name, _)| name).collect()).unwrap_or_default();
                skins.sort();
                skins.first().cloned().unwrap_or_default()
            }
        };
        let guid = self.cars.len() as i32;
        let mut car = CarAvatar::init_3d(&mut self.graphics, &mut self.scene, self.cars_node, &path_text(folder), folder, &skin, Some(steer_lock), guid)?;
        car.max_gear = max_gear;
        car.cars_to_be_loaded = self.cars_to_be_loaded;
        if let Some(mirror) = &self.mirror {
            car.init_mirror_materials(&mut self.graphics, &mut self.scene, &mirror.texture)?;
        }
        (car.lock_virtual_steer, car.hide_arms_in_cockpit, car.hide_steer) = self.cockpit_flags;
        car.init_common_post_physics(&mut self.graphics, &mut self.scene, &self.sim_nodes, tyre_width)?;
        let summary = format!("car model {}: {} levels of detail, skin {skin}, loaded in {:.2} s", folder.display(), car.lods.len(), started.elapsed().as_secs_f64());
        self.cars.push(car);
        self.crew_cars.push(rustyac_render::pit_crew::CrewCar { folder_text: path_text(folder), folder: folder.to_path_buf(), skin: skin.clone(), guid });
        Ok(summary)
    }

    /// `Sim::Sim`, once the cars are there: `TrackAvatar::initPitCrew` (a lollipop man for every
    /// car that has a pit box), then the marker of `PitStop::PitStop` at the player's box.
    /// By wheel: the wheel gun the mechanic there is on while a stop runs (the member's state
    /// 2: unscrewing, 4: screwing), which is the event `PitCrew::updateCurrentPosition` asks
    /// about in this frame.
    /// `Sim::addCar`'s picture part for a car of the physics: its folder (where its model is),
    /// its skin (one the car does not have: the first), and what the picture reads of its data.
    /// `None`: the car has no 3D model in the game's folder.
    pub fn load_car_info(&mut self, info: &crate::sim::CarInfo, skin: Option<String>) -> Result<Option<String>, String> {
        let Some(folder) = crate::sim::find_car_model(&info.name, &info.data_path).and_then(|kn5| kn5.parent().map(|p| p.to_path_buf())) else {
            return Ok(None);
        };
        let skin = skin.filter(|s| rustyac_content::vfs::is_dir(&folder.join("skins").join(s)));
        self.set_skin(skin);
        let steer_lock = IniReader::load(&info.data_path.join("car.ini")).ok().and_then(|ini| ini.get_float("CONTROLS", "STEER_LOCK").ok()).unwrap_or(0.0);
        // CarPhysicsInfo::tyreWidth
        let mut tyre_width = [0.0f32; 4];
        if let Ok(tyres) = IniReader::load(&info.data_path.join("tyres.ini")) {
            let front = tyres.get_float("FRONT", "WIDTH").unwrap_or(0.0);
            let rear = tyres.get_float("REAR", "WIDTH").unwrap_or(0.0);
            tyre_width = [front, front, rear, rear];
        }
        // CarPhysicsInfo::maxGear
        let max_gear = IniReader::load(&info.data_path.join("drivetrain.ini")).ok().and_then(|ini| ini.get_int("GEARS", "COUNT").ok()).unwrap_or(0);
        self.load_car(&folder, steer_lock, tyre_width, max_gear).map(Some)
    }

    pub fn crew_guns_wanted(&self) -> [Option<bool>; 4] {
        let mut wanted = [None; 4];
        if let Some(crew) = self.tyre_crew.as_ref().filter(|crew| crew.pit_stop_on) {
            for (k, m) in crew.members.iter().enumerate().take(4) {
                wanted[k] = match m.state {
                    2 => Some(false),
                    4 => Some(true),
                    _ => None,
                };
            }
        }
        wanted
    }

    pub fn init_pit_crew(&mut self) -> String {
        let mut summary = String::new();
        if self.crew_cars.is_empty() {
            return summary;
        }
        if let Some((model, data, pits, track)) = &self.pit_setup {
            use rustyac_physics::vecmath::Vec3f;
            let cars = &self.crew_cars;
            let down = Vec3f::new(0.0, -1.0, 0.0);
            let ray = |from: &[f32; 3], length: f32| track.ray_cast_hit(&Vec3f::new(from[0], from[1], from[2]), &down, length).map(|hit| hit.contact.pos);
            match rustyac_render::pit_crew::init_pit_crew(&mut self.graphics, &mut self.scene, *model, data, pits, cars, &ray) {
                Ok(crews) => self.lollipops = crews,
                Err(message) => println!("WARNING: no pit crew: {message}"),
            }
            if let Some(pit) = pits.first() {
                // cfg/gameplay.ini [PIT_INDICATOR] ONLY_IN_PITLINE: on when the key is missing
                let mut only_in_pitline = true;
                let gameplay = self.graphics.documents_folder.join("cfg/gameplay.ini");
                if rustyac_content::vfs::is_file(&gameplay) {
                    if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&gameplay) {
                        if ini.has_key("PIT_INDICATOR", "ONLY_IN_PITLINE") {
                            only_in_pitline = ini.get_int("PIT_INDICATOR", "ONLY_IN_PITLINE").unwrap_or(0) > 0;
                        }
                    }
                }
                match rustyac_render::pit_crew::PitIndicator::new(&mut self.graphics, &mut self.scene, self.track_node, pit, only_in_pitline) {
                    Ok(marker) => self.pit_marker = Some(marker),
                    Err(message) => println!("WARNING: no pit box marker: {message}"),
                }
            }
            // PitStop::PitStop: the tyre changers of the player's car
            if let (Some(crew_car), Some(car)) = (self.crew_cars.first(), self.cars.first()) {
                let car_ini = crew_car.folder.join("data/car.ini");
                let seconds = IniReader::load(&car_ini).ok().and_then(|ini| ini.get_float("PIT_STOP", "TYRE_CHANGE_TIME_SEC").ok()).unwrap_or(10.0);
                match rustyac_render::tyre_crew::PitCrew::new(&mut self.graphics, &mut self.scene, self.blurred, crew_car, car.car_node, seconds) {
                    Ok(crew) => self.tyre_crew = Some(crew),
                    Err(message) => println!("WARNING: no tyre changers: {message}"),
                }
            }
            summary.push_str(&format!("pit crew: {} lollipop man, the marker {}, the tyre changers {}", self.lollipops.len(), if self.pit_marker.is_some() { "loaded" } else { "missing" }, if self.tyre_crew.is_some() { "loaded" } else { "missing" }));
            if !self.lollipops.is_empty() && self.graphics.video.world_detail < 5 {
                summary.push_str(" (the lollipop man only shows with WORLD_DETAIL 5, as in the game)");
            }
        }
        summary
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
        // Sim::onPostLoad, after the cube map: CarAvatar::onPostLoad of every car in the order
        // they were made (the flat ground shadows)
        for car in &mut self.cars {
            self.graphics.shadow_cache = shadow_cache_folder().ok();
            car.on_post_load(&mut self.graphics, &mut self.scene, self.car_shadows);
            // A car without body_shadow.png: the game draws the five pictures, saves them into
            // the car's folder and shows them from its next start on. rustyAC never writes
            // there: the pictures go into a folder of its own, next to the program, and are
            // loaded from it at once. At the next start they are taken from that folder, and
            // made again only when the car's files changed.
            if let Some(shadow) = &car.fake_shadow {
                let mut shadow = shadow.borrow_mut();
                if shadow.from_cache {
                    println!("{} has no body_shadow.png: its ground shadows come from rustyac_cache (made at an earlier start)", car.folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                }
                if let Some(pictures) = shadow.generated.take() {
                    let name = car.folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    match save_generated_shadows(&name, &car.folder, &pictures) {
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
        self.cars.first().filter(|car| car.backfire.is_some()).map(|car| car.backfire_triggered)
    }

    /// The same of every car, in the order they were made.
    pub fn backfires_triggered(&self) -> Vec<Option<bool>> {
        self.cars.iter().map(|car| car.backfire.is_some().then_some(car.backfire_triggered)).collect()
    }

    pub fn size(&self) -> (u32, u32) {
        (self.graphics.video.width as u32, self.graphics.video.height as u32)
    }

    /// The page's device for the next [`AcRenderer::new`] (the renderer cannot open one in a
    /// browser): the features kgl uses that the adapter has.
    #[cfg(any(not(windows), feature = "wgpu"))]
    pub fn give_device(device: rustyac_render::wgpu::Device, queue: rustyac_render::wgpu::Queue, adapter: rustyac_render::wgpu::Features) {
        rustyac_render::kgl::Kgl::give_device(device, queue, rustyac_render::kgl::wanted_features(adapter));
    }

    /// The features to ask the adapter for (see [`AcRenderer::give_device`]).
    #[cfg(any(not(windows), feature = "wgpu"))]
    pub fn wanted_features(adapter: rustyac_render::wgpu::Features) -> rustyac_render::wgpu::Features {
        rustyac_render::kgl::wanted_features(adapter)
    }

    /// The last picture into the page's canvas, and the frame's work to the device.
    #[cfg(any(not(windows), feature = "wgpu"))]
    pub fn present_to(&mut self, target: &rustyac_render::wgpu::TextureView, format: rustyac_render::wgpu::TextureFormat) {
        self.graphics.kgl.present_to(target, format);
        self.graphics.kgl.swap_buffers();
    }

    /// The canvas has a new size.
    #[cfg(any(not(windows), feature = "wgpu"))]
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.graphics.on_resize(width.max(16) as i32, height.max(16) as i32)?;
        self.camera.on_resize(&mut self.graphics)
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
    /// `world` is every car of the session (the player's first), `focus` the focused car
    /// (`Sim::focusedCarIndex`): the camera is its, and so are the mirror, the dashboard that
    /// lives and the HUD.
    pub fn draw(&mut self, world: &[CarView], focus: usize, driving: &DrivingCamera, frame: &CameraFrame, info: &HudInfo, dt: f32) {
        // Game::onIdle's clock (ksGetQPTTime) for Game::stats (Task 33)
        let frame_start = Instant::now();
        self.console_sets();
        let view = &world[0];
        let focus = focus.min(world.len() - 1).min(self.cars.len().saturating_sub(1));
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
            // the window's replay: the transport's own state (playing at first)
            (true, _) if info.replay_transport => info.replay_events.last().map(|e| e.0).unwrap_or(self.told_replay_status.max(0)),
            (true, false) => 0,
            (true, true) => 1,
        };
        let replay_scale = match (info.replay_transport, replay_status) {
            (true, _) => info.replay_scale,
            (false, 1) => 0.0,
            (false, _) => 1.0,
        };
        // StartingLights' two handlers, and RaceManager::update: the countdown stands while the
        // pause menu shows and in a replay
        if let Some(lights) = &mut self.start_lights {
            if info.replay {
                if self.told_replay_status == -1 {
                    lights.on_replay_status_changed(6);
                }
            } else if info.paused != self.told_paused {
                lights.on_pause_mode_changed(info.paused);
            }
        }
        if info.paused || info.replay {
            if let Some(start) = &mut self.race_start_ms {
                *start += (dt * 1000.0) as f64;
            }
        }
        // (every car's handlers, in the order the cars were made)
        for car in &mut self.cars {
            if info.replay {
                if self.told_replay_status == -1 {
                    // ReplayManager::startReplayMode: evOnReplayStarted, then status 6
                    car.on_replay_started_or_stopped(true);
                    car.on_replay_status_changed(&mut self.scene, 6, 1.0, 2.0);
                }
                if info.replay_transport {
                    for &(status, time_mult, slow_mo) in &info.replay_events {
                        // ReplayManager::stop: evOnReplayRewind (the skid marks go), then the status
                        if status == crate::replay_manager::STOP {
                            car.on_replay_rewind();
                        }
                        car.on_replay_status_changed(&mut self.scene, status, time_mult, slow_mo);
                        if let Some(grooves) = &mut self.grooves {
                            grooves.reset_to_target = true;
                        }
                    }
                } else if replay_status != self.told_replay_status {
                    car.on_replay_status_changed(&mut self.scene, replay_status, replay_scale, 2.0);
                    if let Some(grooves) = &mut self.grooves {
                        grooves.reset_to_target = true;
                    }
                }
            } else if info.paused != self.told_paused {
                car.on_pause_mode_changed(info.paused);
            }
        }
        self.told_paused = info.paused && !info.replay;
        self.told_replay_status_before = self.told_replay_status;
        self.told_replay_status = replay_status;
        // SunAnimator::update: the sun, the lighting's clock, the clouds' drift. A rustyAC
        // replay is the drive run again, so its sun moves as it did; a paused replay's stands.
        if let Some(sun) = &mut self.sun {
            // (the game takes the sun of a replay from its recorded frames: it moves at the
            // replay's speed)
            if replay_scale != 0.0 {
                sun.update(&mut self.graphics, self.camera.base.sky_box.as_mut(), if info.replay { dt * replay_scale } else { dt }, self.told_paused, None);
            }
        }
        // Sim::evOnNewSession (N: a new car at the spawn point), its handlers in the order they
        // were registered: the track's (the drifting objects are scattered again), the car's
        // (smoke and dirt go); and the race start is counted down again
        if view.new_sessions != self.told_new_sessions {
            self.told_new_sessions = view.new_sessions;
            rustyac_render::crowds::on_new_session(&mut self.graphics.crt_rand, &mut self.dynamic_objects, self.time_ms, 0.0);
            for car in &mut self.cars {
                car.on_new_session(&mut self.scene);
            }
            self.start_race();
        }
        // Game::update, the track's children: LollipopCrew::update (the player's man follows the
        // pit stop's state: up while the car is in the pit lane, down once it has stopped in its box)
        let pit_state = self.pit_marker.as_ref().map(|m| m.state);
        for crew in &mut self.lollipops {
            // (another car's man is up while that car stands in its pit box: CarAvatar::isInPit
            // 0x1400d8bd0, the car within 1.5 m of its pit position)
            let in_pit = match (world.get(crew.guid as usize), self.pit_setup.as_ref().and_then(|setup| setup.2.get(crew.guid as usize))) {
                (Some(car), Some(pit)) => {
                    let (dx, dy, dz) = (car.body[3][0] - pit.m[3][0], car.body[3][1] - pit.m[3][1], car.body[3][2] - pit.m[3][2]);
                    let squared = (dy * dy + dx * dx) + dz * dz;
                    (if squared != 0.0 { squared.sqrt() } else { 0.0 }) < 1.5
                }
                _ => false,
            };
            crew.update(&mut self.scene, dt, pit_state, in_pit);
        }
        // ACCameraManager::mode and CameraDrivableManager::currentMode
        let modes = match driving.mode {
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
            // (eCar is 1: ACCameraManager::update's jump table)
            CameraMode::Car => (1, 0),
        };
        // ACCameraManager::setMode 0x1400340d0 / setDrivableCarIndex: currentGlobalCameraIndex
        // is 0 in the cockpit and the drivable view's number + 1; the car cameras leave it
        match modes {
            (0, _) => self.global_camera_index = 0,
            (2, drivable) => self.global_camera_index = drivable + 1,
            _ => {}
        }
        // Sim::update 0x14019ef90: CarNodeSorter::sort, before the cars update
        if self.cars.len() > 1 {
            let list: Vec<(NodeId, i32)> = self.cars.iter().map(|car| (car.car_node, car.guid)).collect();
            self.scene.sort_cars(self.cars_node, &list, self.global_camera_index);
        }
        // where each car stands in real time (RaceManager::carsRealTimePosition): by laps, then
        // by the place along the lap
        let mut order: Vec<usize> = (0..world.len()).collect();
        order.sort_by(|a, b| (world[*b].lap.laps, world[*b].lap.position).partial_cmp(&(world[*a].lap.laps, world[*a].lap.position)).unwrap_or(std::cmp::Ordering::Equal));
        let cars_count = self.cars.len().min(world.len());
        let mut states = Vec::with_capacity(cars_count);
        for (index, (car, view)) in self.cars.iter_mut().zip(world).enumerate() {
            let (camera_mode, drivable_mode) = modes;
            car.game_time_ms = self.time_ms;
            // PhysicsAvatar::update: CarAvatar::setNewPhysicsState (a frame without time is a
            // still: the car is shown as its state has it, not as the frame before left it)
            // (in a replay the car's state provider is its ReplayRecorder: CarAvatar::onStartReplay)
            let from_provider = match self.replay_states.as_ref().and_then(|states| states.get(index)) {
                Some(state) => *state,
                None => view.physics_state(),
            };
            let state = car.set_new_physics_state(&from_provider, if dt > 0.0 { dt } else { 1.0 });
            car.view = rustyac_render::car::ViewState { camera_mode, drivable_mode, focused_car_index: focus as i32, camera_position: [frame.matrix[3][0], frame.matrix[3][1], frame.matrix[3][2]], camera_matrix: rustyac_physics::vecmath::Mat44f { m: frame.matrix }, use_pro_view: false };
            // what the car's objects read beside the physics state: the aids' levels and the air
            // (CarAvatar::getTCMode, getABSMode, PhysicsEngine::ambientTemperature), the wings
            // (CarAvatar::wingsStatus), CarPhysicsInfo, the session, the pause menu and the replay
            car.tc_level = view.tc_mode.0;
            car.abs_level = view.abs_mode.0;
            car.ambient_temperature = view.air;
            car.wing_angles.clear();
            car.wing_angles.extend_from_slice(&view.wing_angles[..view.wing_count as usize]);
            car.max_fuel = view.max_fuel;
            car.km_per_liter = view.km_per_liter;
            car.kers_max_j = view.kers_max_j;
            car.ers_max_j = view.ers_max_j;
            car.has_kers = view.hybrid.has_kers;
            car.has_energy_store = view.hybrid.has_kers || view.hybrid.has_ers;
            car.drivetrain = Some((view.total_torque, view.drive_ratio));
            car.pause_menu = self.told_paused;
            car.replay_mode = info.replay;
            car.replay_scale = replay_scale;
            // rustyAC's sessions are practice (hot lap from the line)
            car.session_type = 1;
            let place = order.iter().position(|i| *i == index).unwrap_or(0) as i32;
            car.leaderboard_position = place + 1;
            car.real_time_position = place;
            car.cars_count = cars_count as i32;
            // NetCarStateProvider::update: a remote car nobody drives is not there
            if index > 0 {
                self.scene.nodes[car.car_node].is_active = !view.disconnected;
            }
            car.update(&mut self.graphics, &mut self.scene, &state, dt);
            // CarAvatar::initGhostCar's GhostCar, a child of the player's avatar: its update
            if index == 0 && self.ghost_rig.is_some() && self.ghost.is_none() {
                if let Some(crew_car) = self.crew_cars.first() {
                    // RaceManager::initGhostcar
                    self.scene.nodes[self.sim_nodes.before_cars].is_active = true;
                    match rustyac_render::ghost_car::GhostCar::new(&mut self.graphics, &mut self.scene, self.sim_nodes.before_cars, &crew_car.folder_text, &crew_car.folder) {
                        Ok(ghost) => self.ghost = Some(ghost),
                        Err(message) => {
                            println!("WARNING: no ghost car: {message}");
                            self.ghost_rig = None;
                        }
                    }
                }
            }
            if let (0, Some(rig), Some(ghost)) = (index, &self.ghost_rig, &mut self.ghost) {
                let mut rig = rig.lock().unwrap();
                let b = self.scene.nodes[car.body_transform].matrix.m[3];
                ghost.update(&mut self.scene, &mut rig.recorder, state.lap_time, &|m| car.make_body_matrix(m), &[b[0], b[1], b[2]], info.replay);
            }
            states.push(state);
        }
        // Game::evOnPostUpdate, after every car's update: each car's handlers in the order the
        // cars were made
        for (car, state) in self.cars.iter_mut().zip(&states) {
            car.post_update(&mut self.scene, state, dt, &Mat44f { m: frame.matrix }, frame.fov, false);
        }
        // PitStop::update raised evPitStopCommit / evPitStopOver; then the pit stop's handler of
        // evOnPostUpdate: PitCrew::pitCrewUpdate
        if let (Some(crew), Some(pit), Some(state)) = (&mut self.tyre_crew, view.lap.pit_stop, states.first()) {
            use rustyac_render::tyre_crew::{CrewInputs, CrewPitStop, CrewStart};
            if info.replay && self.told_replay_status_before == -1 {
                // Sim::evOnReplayStatusChanged, status 6: entering a replay hides the crew
                crew.on_replay_status_changed(&mut self.scene, 6);
            }
            let inputs = CrewInputs {
                replay: info.replay,
                pause_menu: self.told_paused,
                session_type: self.options.session_type,
                pit: CrewPitStop { state: pit.state, pit_timer: pit.pit_timer, max_pit_timer: pit.max_pit_timer, tyre_time: pit.tyre_time },
                tyre_matrix: state.tyre_matrix,
                tyre_contact_point: state.tyre_contact_point,
                focused: focus == 0,
            };
            if pit.commits != self.crew_seen.0 {
                self.crew_seen.0 = pit.commits;
                let start = CrewStart { tyre_change_time: pit.tyre_change_time, total_time: pit.total_time, force_change_camera: pit.force_change_camera };
                println!("pit stop {} at {:.1} s: {:.1} s in all, {:.1} s for the tyres{}", pit.commits, self.time_ms * 0.001, pit.total_time, pit.tyre_change_time, if pit.changing_tyres { " (the crew comes)" } else { "" });
                crew.on_pit_stop_commit(&mut self.crew, &start, pit.changing_tyres);
                if let Some(total) = self.crew.fade.take() {
                    self.crew_fade = Some((0.0, total));
                }
            }
            if pit.overs != self.crew_seen.1 {
                self.crew_seen.1 = pit.overs;
                crew.on_pit_stop_over(&mut self.crew);
            }
            crew.pit_crew_update(&mut self.scene, &mut self.crew, dt, &inputs);
            self.crew_inputs = Some(inputs);
        }
        if let Some(gates) = &self.gates {
            gates.borrow_mut().in_replay = info.replay;
        }
        // PitStop::update, after the cars: the pit box marker
        let pit_inputs = self.cars.first().map(|car| {
            let b = self.scene.nodes[car.body_transform].matrix.m[3];
            rustyac_render::pit_crew::PitInputs { car_position: [b[0], b[1], b[2]], speed: view.speed_ms, in_pitlane: view.lap.in_pit_lane, session_type: self.options.session_type, pause_menu: self.told_paused, replay: info.replay }
        });
        if let (Some(marker), Some(inputs)) = (&mut self.pit_marker, &pit_inputs) {
            match view.lap.pit_stop.map(|pit| (pit.state, pit.visible)) {
                // Task 28b: the pit stop itself runs with the referee: the marker follows it
                Some((state, visible)) => {
                    let rand = &mut self.graphics.crt_rand;
                    marker.follow(&mut self.scene, &mut || {
                        rand.next();
                    }, state, visible)
                }
                // (a session without one: the marker's own walk through the states)
                None => {
                    let rand = &mut self.graphics.crt_rand;
                    marker.update(&mut self.scene, &mut || {
                        rand.next();
                    }, inputs);
                }
            }
        }
        // TrackAvatar::update: the drifting objects (the pause menu does not stop them)
        rustyac_render::crowds::update_dynamic_objects(&mut self.scene, &mut self.dynamic_objects, dt);
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
            let laps: Vec<u32> = world.iter().map(|v| v.lap.laps).collect();
            grooves.update(&mut self.scene, dt, view.dynamic_track.then_some(view.grip), &laps);
        }
        self.frames += 1;
        #[cfg(all(windows, not(feature = "wgpu")))]
        let logging = self.options.gpu_log.is_some() && self.frames == 2;
        #[cfg(all(windows, not(feature = "wgpu")))]
        if logging {
            rustyac_render::gpulog::begin_capture("rustyac.exe");
        }
        // DrivingAssistManager::update 0x1400fcb20: every frame
        if let Some(line) = self.ideal_line {
            self.scene.nodes[line].is_active = self.ideal_on;
        }
        // RaceManager::getTimeToSessionStart: a session without a race start began at 0
        let session_start = self.race_start_ms.unwrap_or(0.0);
        let tts = (session_start - self.time_ms) as f32;
        // StartingTrackSemaphore::update (Game::update: before the scene)
        if let Some(semaphore) = &mut self.semaphore {
            semaphore.update(&mut self.scene, tts);
        }
        let update_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        self.graphics.begin_scene();
        self.scene.traverse(self.root);
        // Sim::renderScene: the mirror before the picture (MirrorTextureRenderer::render
        // 0x140114410: the focused car's rear view, with that car hidden)
        if let (Some(mirror), Some(car)) = (&mut self.mirror, self.cars.get(focus)) {
            let active = self.virtual_mirror.as_ref().is_some_and(|v| v.active);
            if mirror.wants_render(modes.0, modes.1, active) {
                let nodes = car.visibility_nodes();
                let s = rustyac_render::mirror::MirrorScene {
                    root: self.root,
                    particles: self.sim_nodes.particles,
                    before_cars: self.sim_nodes.before_cars,
                    ideal_line: self.ideal_line,
                    body_transform: car.body_transform,
                    car_nodes: &nodes,
                    mirror_position: car.mirror_position,
                };
                mirror.render(&mut self.graphics, &mut self.scene, self.camera.base.sky_box.as_mut(), &s);
            }
        }
        // FormWings::onRenderNode: the player's wings while the app shows
        if let (Some(outlines), Some(gui)) = (&self.wings_outlines, &self.gui) {
            let shown = gui.apps.made.iter().any(|&(name, id)| name == "wings" && gui.gui.controls[id].visible);
            let wings = &gui.setup.wings;
            outlines.borrow_mut().frame = (shown && !wings.is_empty()).then(|| {
                let lifts = &view.wing_states[..view.wing_state_count as usize];
                let rows = wings.iter().zip(lifts).map(|(w, s)| rustyac_render::wings::WingOutline { position: w.position, span: w.span, chord: w.chord, is_vertical: w.is_vertical, lift_vector: [s.lift_vector.x, s.lift_vector.y, s.lift_vector.z] });
                // shortcut: the body's matrix of the frame's view stands for physicsState.worldMatrix; a replay's car may differ
                (view.body, rows.collect())
            });
        }
        // ACSuspension's handler: the player's suspensions while the app shows
        if let (Some(lines), Some(gui)) = (&self.suspension_lines, &self.gui) {
            let shown = gui.apps.made.iter().any(|&(name, id)| name == "suspension" && gui.gui.controls[id].visible);
            let f = gui.setup.suspension;
            let m = |m: [[f32; 4]; 4]| rustyac_physics::vecmath::Mat44f { m };
            let keys = gui.apps.suspension_keys;
            lines.borrow_mut().frame = shown.then(|| rustyac_render::suspensions::SuspensionFrame {
                // shortcut: the frame's view stands for physicsState (worldMatrix, tyreMatrix); a replay's car may differ
                world: m(view.body),
                tyre_matrix: view.tyre_matrix.map(m),
                lines: std::array::from_fn(|w| view.sus_lines[w][..view.sus_line_count[w] as usize].iter().map(|l| ([l.p0.x, l.p0.y, l.p0.z], [l.p1.x, l.p1.y, l.p1.z], l.color)).collect()),
                ride_pickup_point: f.ride_pickup_point,
                tyre_width: f.tyre_width,
                tyre_radius: f.tyre_radius,
                fuel_tank_position: f.fuel_tank_position,
                wheel_nodes: std::array::from_fn(|w| self.cars.first().map_or([0.0; 3], |c| c.wheel_node_position(&self.scene, w))),
                keys,
            });
        }
        // (a frame without time, a screenshot's first: the blur solve divides by it)
        let blur_dt = if dt > 0.0 { dt } else { 1.0 / 60.0 };
        if let Err(message) = self.camera.render(&mut self.graphics, &mut self.scene, Some(self.blurred), self.unblurred, self.root, blur_dt) {
            eprintln!("WARNING: the frame was not drawn: {message}");
        }
        self.draw_calls = self.graphics.stats.dip_calls;
        self.triangles = self.graphics.stats.triangles;
        // (the page's render scale: the 2D pass at the window's size, the picture stretched)
        #[cfg(any(not(windows), feature = "wgpu"))]
        let small = match self.gui_size.filter(|&size| self.gui.is_some() && size != (self.graphics.video.width, self.graphics.video.height)) {
            Some((w, h)) => match self.graphics.kgl.begin_gui_screen(w, h) {
                Ok(()) => {
                    let small = (self.graphics.video.width, self.graphics.video.height);
                    (self.graphics.video.width, self.graphics.video.height) = (w, h);
                    self.graphics.set_viewport(0, 0, w, h);
                    Some(small)
                }
                Err(why) => {
                    eprintln!("WARNING: the GUI at the window's size: {why}");
                    None
                }
            },
            None => None,
        };
        self.graphics.set_screen_space_mode();
        // Game::evOnPreGUI: the handler of ScreenCapturer first (it was hung up first)
        // the statistics apps' numbers: Game::stats so far, GraphicsManager::stats of the 3D part
        let render_ms = frame_start.elapsed().as_secs_f64() * 1000.0 - update_ms;
        // shortcut: the statistics apps get no frame numbers on wgpu (the page has no clock in
        // here), give them the page's performance.now() when those apps matter there
        #[cfg(all(windows, not(feature = "wgpu")))]
        if let Some(gui) = &mut self.gui {
            gui.frame_stats(update_ms, render_ms, &self.graphics.stats, &info.timing);
        }
        if self.gui.as_mut().is_some_and(|gui| gui.pre_gui()) {
            // (F8's picture: the page reads none back, the browser takes screenshots)
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.pre_gui_shot = Some(self.graphics.kgl.read_screen());
            }
        }
        if let (Some(mirror), Some(v)) = (&self.mirror, &mut self.virtual_mirror) {
            v.render(&mut self.graphics, &mirror.texture, modes.0);
        }
        // ksgui::GUI::render -> doRender2D 0x1402539d0: the states of the 2D pass, then the game
        // screen with its forms and the app bar (the HUD objects' update pass runs right before:
        // nothing between here and the game's place for it reads what they change)
        match &mut self.gui {
            Some(gui) => {
                let camera = &self.camera.forward.base.camera;
                let (matrix, view_matrix, perspective) = (camera.matrix, camera.get_view_matrix(), camera.get_perspective_matrix(&self.graphics));
                let mode = match driving.mode {
                    super::scene::CameraMode::Cockpit => 0,
                    super::scene::CameraMode::Car => 1,
                    super::scene::CameraMode::Drivable => 2,
                };
                gui.gui.sim.sun_angle = self.options.sun_angle;
                gui.update(world, focus, info, (&matrix, &view_matrix, &perspective), mode, self.time_ms, dt);
                // GPUProfilerApp: GPUProfiler::getResults, and its setActive(true)
                let rows = self.graphics.gpu_profiler.queries.iter().map(|q| rustyac_gui::app_gpu_profiler::GpuQueryRow { name: q.name.to_string(), last_result: q.last_result, last_ts: q.last_ts });
                gui.gui.apps.gpu_profile = rows.collect();
                gui.render(&mut self.graphics, dt);
                self.graphics.gpu_profiler.is_active |= gui.gui.apps.gpu_profiler_active;
                // FormDriverModel: what it wrote into car 0's driverModel_HR, its updateHeadMovement calls
                if let (Some(l), Some(d)) = (gui.gui.apps.driver_model.as_mut(), self.cars.first_mut().and_then(|c| c.driver_hr.as_mut())) {
                    for (i, v) in l.head.iter_mut().enumerate() {
                        if let Some(v) = v.take() {
                            *[&mut d.max_g, &mut d.roll_max_deg, &mut d.pitch_max_deg, &mut d.yaw_max_deg, &mut d.filter][i] = v;
                        }
                    }
                    // shortcut: set every frame, so a car whose LOCK_VIRTUAL_STEER clears it on board keeps it; follow updateForces calls if that matters
                    d.lock_animation |= l.lock_animation;
                    for (dt, acc) in l.head_updates.drain(..) {
                        d.update_head_movement(&mut self.scene, dt, dt, &acc);
                    }
                }
            }
            None => {
                self.graphics.reset_render_states();
                self.graphics.set_depth_mode(rustyac_render::graphics::DEPTH_OFF);
                self.graphics.set_blend_mode(rustyac_render::graphics::BLEND_ALPHA);
            }
        }
        self.graphics.trigger_query("GUI");
        // Game::renderHUD: TimeAttack::renderGuiStatus, the gate of the next time line
        if let Some(gates) = &self.gates {
            if !self.told_paused && !info.replay {
                // (the gate of the next checkpoint while the mode is on: TimeAttack's own sector)
                let ta = view.lap.time_attack.unwrap_or_default();
                gates.borrow_mut().set_active(ta.mode == 1, ta.sector_index as usize, self.gate_sectors);
            }
        }
        // Game::renderHUD: PitStop::renderHUD, the marker's part
        if let (Some(marker), Some(inputs)) = (&mut self.pit_marker, &pit_inputs) {
            marker.render_hud(&mut self.scene, inputs);
        }
        // Game::renderHUD: StartingLights::renderHUD writes the lamps after the scene is drawn
        // (they change in the next frame, as in the game)
        if let Some(lights) = &mut self.start_lights {
            if lights.is_active && lights.render_hud(&mut self.scene, self.told_paused, session_start < self.time_ms, self.race_start_ms.is_some(), tts) {
                lights.render_lamps(&mut self.graphics);
            }
        }
        // Game::renderHUD: the HUD objects of Sim (messages, banners, arrows, name tags, damage,
        // fuel light)
        if let Some(gui) = &mut self.gui {
            gui.render_hud(&mut self.graphics, dt);
            // Game::stats.cpuTime and renderHUDTime: the frame up to here
            let cpu_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
            gui.frame_end(cpu_ms, cpu_ms - update_ms - render_ms);
        }
        // Game::evOnPostGui: the camera manager's fade, over everything of the game's 2D pass
        if let Some((time, total)) = info.focus_fade {
            rustyac_render::gl::fade_overlay(&mut self.graphics, time, total);
        } else if let Some((time, total)) = self.crew_fade {
            // the fade a pit stop's commit asked for (ACCameraManager::fadeIn with 0.25 s): at
            // its black frame the crew is put round the car (the lambda 0x140032770)
            rustyac_render::gl::fade_overlay(&mut self.graphics, time, total);
        }
        if let Some((time, total)) = self.crew_fade {
            if time >= total * 2.0 {
                self.crew_fade = None;
            } else {
                if time >= total {
                    if let (Some(crew), Some(inputs)) = (&mut self.tyre_crew, &self.crew_inputs) {
                        let rand = &mut self.graphics.crt_rand;
                        crew.on_fade_in_finished(&mut self.scene, &mut self.crew, &mut || rand.next(), inputs);
                    }
                }
                self.crew_fade = Some((time + dt, total));
            }
        }
        // (the same handler, for a fade one of the menus asked for)
        if let Some(gui) = &mut self.gui {
            gui.post_gui(&mut self.graphics, dt);
        }
        #[cfg(all(windows, not(feature = "wgpu")))]
        if logging {
            let capture = rustyac_render::gpulog::end_capture();
            if let Some(path) = &self.options.gpu_log {
                match rustyac_content::guard::write(path, &capture.text) {
                    Ok(()) => println!("{}: the command log of frame 2 ({} calls, {} draws)", path.display(), capture.lines, capture.draws),
                    Err(e) => eprintln!("WARNING: {}: {e}", path.display()),
                }
            }
        }
        // the 2D drawing: GraphicsManager::resetRenderStates, then rustyAC's own HUD
        self.graphics.reset_render_states();
        self.graphics.kgl.set_screen_render_targets(true);
        #[cfg(any(not(windows), feature = "wgpu"))]
        if let Some((w, h)) = small {
            self.graphics.kgl.end_gui_screen();
            (self.graphics.video.width, self.graphics.video.height) = (w, h);
        }
        #[cfg(all(windows, not(feature = "wgpu")))]
        let (width, height) = self.size();
        #[cfg(all(windows, not(feature = "wgpu")))]
        self.hud.draw(&self.graphics.kgl.context, &world[focus], info, width, height);
    }

    /// What the motion blur remembers of the last frame (`lastWorldMatrix`, `currentBlurAngle`).
    pub fn blur_history(&self) -> (rustyac_physics::vecmath::Mat44f, f32) {
        (self.camera.last_world_matrix, self.camera.current_blur_angle)
    }

    pub fn set_blur_history(&mut self, history: (rustyac_physics::vecmath::Mat44f, f32)) {
        self.camera.last_world_matrix = history.0;
        self.camera.current_blur_angle = history.1;
    }

    /// The last picture as RGBA bytes, rows from the top.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_pixels(&mut self) -> Result<Vec<u8>, String> {
        let (_, _, mut pixels) = self.graphics.kgl.read_screen()?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(pixels)
    }

    /// Waits until the card has really drawn what was asked for.
    #[cfg(all(windows, not(feature = "wgpu")))]
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
            // RUSTYAC_DIAG_FINISH_SPIN=1 (a diagnosis switch): wait without giving the processor
            // away between two looks
            let spin = std::env::var_os("RUSTYAC_DIAG_FINISH_SPIN").is_some_and(|v| v == "1");
            for _ in 0..200_000 {
                let hr = context.GetData(&query, Some(&mut done as *mut i32 as *mut _), 4, 0);
                if hr.is_ok() && done != 0 {
                    break;
                }
                if spin {
                    std::hint::spin_loop();
                } else {
                    std::thread::yield_now();
                }
            }
        }
    }

    /// A swap chain on a window, for [`AcRenderer::present`].
    #[cfg(all(windows, not(feature = "wgpu")))]
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
    #[cfg(all(windows, not(feature = "wgpu")))]
    pub fn resize_swap_chain(&mut self, chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), String> {
        // SAFETY: nothing holds the back buffers between two frames.
        unsafe {
            self.graphics.kgl.context.OMSetRenderTargets(None, None);
            chain.ResizeBuffers(0, width.max(16), height.max(16), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(err("resizing the swap chain"))?;
        }
        self.graphics.on_resize(width.max(16) as i32, height.max(16) as i32)?;
        self.camera.on_resize(&mut self.graphics)
    }

    /// The console's sets of the scene's objects since the last frame.
    fn console_sets(&mut self) {
        let Some(gui) = &mut self.gui else { return };
        for (name, value) in std::mem::take(&mut gui.apps.console_sets) {
            rustyac_render::console::console_set(&mut self.graphics, &mut self.camera, self.cars.first_mut(), &name, value);
        }
    }

    /// Shows the last picture in the window. `Ok(false)`: the window is covered or minimised.
    #[cfg(all(windows, not(feature = "wgpu")))]
    pub fn present(&mut self, chain: &IDXGISwapChain1, vsync: bool) -> Result<bool, String> {
        // GraphicsManager::endScene: the GPU profiler's END_FRAME, the present, its endFrame
        self.graphics.trigger_query("END_FRAME");
        // SAFETY: the back buffer and the picture have the same size and format.
        let shown = unsafe {
            let back: ID3D11Texture2D = chain.GetBuffer(0).map_err(err("back buffer"))?;
            if let Some(screen) = &self.graphics.kgl.screen_render_target.texture {
                // (the game's own swap chain is multisampled and Present resolves it; here the
                // screen is a texture and the window's flip-model chain cannot be multisampled)
                if self.graphics.video.aa_samples != 1 && !self.graphics.video.pp_hdr_enabled {
                    self.graphics.kgl.context.ResolveSubresource(&back, 0, screen, 0, DXGI_FORMAT_R8G8B8A8_UNORM);
                } else {
                    self.graphics.kgl.context.CopyResource(&back, screen);
                }
            }
            let status = chain.Present(vsync as u32, DXGI_PRESENT(0));
            status.ok().map_err(err("present"))?;
            status.0 != 0x087A_0001
        };
        self.graphics.gpu_profiler.end_frame(&self.graphics.kgl);
        Ok(shown)
    }
}

/// One call of a wheel gun's `AudioEvent` the tyre changers made in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrewSound {
    /// `set3DAttributes` at the wheel's matrix (standing still), then `start`
    Start { wheel: usize, screw: bool, matrix: Mat44f },
    Stop { wheel: usize, screw: bool },
}

/// What the tyre changers ask of the camera manager.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrewCamera {
    /// `ACCameraManager::setMode`
    SetMode(i32),
    /// `CameraOnBoardFree::forceThetaPhi`
    ForceThetaPhi(f32, f32, f32),
    /// `CameraOnBoardFree::lockCamera`
    Lock(bool),
}

/// The tyre changers' sound and camera, between the picture (which runs the crew) and the
/// game's loop (which has the sound and the cameras): the loop writes `playing` and
/// `camera_mode` before a frame and takes `sound` and `camera` after it. Each event is asked
/// whether it plays at most once a frame, before anything is done to it, so the answers of
/// before the frame are the answers the game's code would get.
#[derive(Default)]
pub struct CrewLink {
    /// by wheel: the unscrew [0] and the screw [1] event play (`AudioEvent::isPlaying`)
    pub playing: [[bool; 2]; 4],
    pub sound: Vec<CrewSound>,
    /// `ACCameraManager::mode`
    pub camera_mode: i32,
    pub camera: Vec<CrewCamera>,
    fade: Option<f32>,
}

impl rustyac_render::tyre_crew::CrewHost for CrewLink {
    fn camera_mode(&mut self) -> i32 {
        self.camera_mode
    }
    fn set_camera_mode(&mut self, mode: i32, _a: bool, _b: bool) {
        self.camera.push(CrewCamera::SetMode(mode));
    }
    fn force_theta_phi(&mut self, theta: f32, phi: f32, radius: f32) {
        self.camera.push(CrewCamera::ForceThetaPhi(theta, phi, radius));
    }
    fn lock_camera(&mut self, locked: bool) {
        self.camera.push(CrewCamera::Lock(locked));
    }
    fn fade_in(&mut self, fade_total_time: f32) {
        self.fade = Some(fade_total_time);
    }
    fn event_is_playing(&mut self, wheel: usize, screw: bool) -> bool {
        self.playing[wheel][screw as usize]
    }
    fn event_start(&mut self, wheel: usize, screw: bool, matrix: &Mat44f) {
        self.sound.push(CrewSound::Start { wheel, screw, matrix: *matrix });
    }
    fn event_stop(&mut self, wheel: usize, screw: bool) {
        self.sound.push(CrewSound::Stop { wheel, screw });
    }
}
