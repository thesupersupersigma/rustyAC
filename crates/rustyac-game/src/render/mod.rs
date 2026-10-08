// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug view: a throw-away Direct3D 11 renderer that draws the ground (an endless flat
//! grid, or a track's kn5 models with their diffuse textures), the car (boxes and cylinders,
//! or its own kn5 model) and a text HUD, lit by one sun and an ambient term. It is not AC's
//! renderer and shares nothing with it but the API and the model files; when AC's renderer is
//! ported this module goes away.
//!
//! Everything is drawn into an off-screen target (4x multisampled where the card can), which
//! is then copied to the window or read back for `--screenshot`: the window is optional.

pub mod dds;
pub mod font;
pub mod hud;
pub mod models;
pub mod scene;

use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT,
    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use crate::view::{CarView, Mat};
use font::FontBitmap;
use hud::{HudInfo, HudVertex};
use models::{frustum, sphere_visible, GpuModel, ModelOptions, ModelStats};
use scene::{cube, cylinder, mul, mul_precise, perspective_reversed, point, rotate_pitch, scale_then, translation, view_matrix, CameraFrame, CarShape, Vertex};

const SHADERS: &str = r#"
cbuffer PerDraw : register(b0) {
    row_major float4x4 World;
    row_major float4x4 WorldView;   // World x the camera's view matrix, worked out in double precision
    row_major float4x4 Proj;
    float4 Color;
    float4 Light;       // xyz: the direction the light travels, w: ambient share
    float4 Camera;      // xyz: the camera's position, w: fog density per metre
    float4 Fog;         // rgb: the colour of the horizon
    float4 Params;      // x: pixels with less alpha are not drawn, y: 1 = lit from both sides,
                        // z: repeats of the detail texture (0 = none)
    float4 MultRG;      // the repeats of the detail textures R (xy) and G (zw)
    float4 MultBA;      // ... B (xy) and A (zw)
    float4 Layer;       // x: 0 plain, 1 multilayer by world position, 2 multilayer by uv, 3 grass;
                        // y: magicMult (grass: gain); z, w: ksAmbient, ksDiffuse (z < 0: none)
    float4 Layer2;      // x: the diffuse texture's uv factor, y: alpha factor
};
struct VSIn { float3 pos : POSITION; float3 normal : NORMAL; };
struct VSOut { float4 pos : SV_POSITION; float3 normal : NORMAL; float3 world : TEXCOORD0; };

VSOut vs_mesh(VSIn i) {
    VSOut o;
    float4 w = mul(float4(i.pos, 1.0), World);
    o.pos = mul(mul(float4(i.pos, 1.0), WorldView), Proj);
    o.normal = mul(float4(i.normal, 0.0), World).xyz;
    o.world = w.xyz;
    return o;
}

float3 fogged(float3 c, float3 world) {
    float d = length(world - Camera.xyz);
    return lerp(c, Fog.rgb, 1.0 - exp(-d * Camera.w));
}

float4 ps_mesh(VSOut i) : SV_TARGET {
    float3 n = normalize(i.normal);
    float lit = saturate(dot(n, -Light.xyz));
    float3 c = Color.rgb * (Light.w + (1.0 - Light.w) * lit);
    return float4(fogged(c, i.world), Color.a);
}

Texture2D Diffuse : register(t0);
Texture2D Detail : register(t1);
Texture2D Mask : register(t2);      // txMask (grass: txVariation)
Texture2D DetailR : register(t3);
Texture2D DetailG : register(t4);
Texture2D DetailB : register(t5);
Texture2D DetailA : register(t6);
SamplerState DiffuseSampler : register(s0);
struct ModelIn { float3 pos : POSITION; float3 normal : NORMAL; float2 uv : TEXCOORD0; };
struct ModelOut { float4 pos : SV_POSITION; float3 normal : NORMAL; float3 world : TEXCOORD0; float2 uv : TEXCOORD1; };

ModelOut vs_model(ModelIn i) {
    ModelOut o;
    float4 w = mul(float4(i.pos, 1.0), World);
    o.pos = mul(mul(float4(i.pos, 1.0), WorldView), Proj);
    o.normal = mul(float4(i.normal, 0.0), World).xyz;
    o.world = w.xyz;
    o.uv = i.uv;
    return o;
}

// a kn5 mesh: its colour as AC's pixel shader of that material mixes it, one sun, ambient light
float4 ps_model(ModelOut i) : SV_TARGET {
    float4 t = Diffuse.Sample(DiffuseSampler, i.uv * Layer2.x) * Color;
    if (Layer.x == 1.0 || Layer.x == 2.0) {
        // ksMultilayer*: the diffuse texture only shades; the colour is four tiled detail
        // textures weighted by the mask's channels (the weights are not normalised)
        float2 p = (Layer.x == 1.0) ? i.world.xz : i.uv;
        float4 m = Mask.Sample(DiffuseSampler, i.uv);
        float3 d = DetailG.Sample(DiffuseSampler, p * MultRG.zw).rgb * m.g + DetailR.Sample(DiffuseSampler, p * MultRG.xy).rgb * m.r
                 + DetailB.Sample(DiffuseSampler, p * MultBA.xy).rgb * m.b + DetailA.Sample(DiffuseSampler, p * MultBA.zw).rgb * m.a;
        t.rgb = t.rgb * d * Layer.y;
        t.a = 1.0;
    } else if (Layer.x == 3.0) {
        // ksGrass: the blades' colour varied over the ground
        t.rgb += t.rgb * (Mask.Sample(DiffuseSampler, i.world.xz * MultRG.xy).rgb - 0.5) * Layer.y;
    }
    if (Params.z > 0.0) {
        // AC's detail texture: the colour where the diffuse texture's alpha is 0
        t.rgb *= lerp(Detail.Sample(DiffuseSampler, i.uv * Params.z).rgb, float3(1.0, 1.0, 1.0), t.a);
        t.a = 1.0;
    }
    t.a *= Layer2.y;
    clip(t.a - Params.x);
    float3 n = normalize(i.normal);
    float d = dot(n, -Light.xyz);
    float lit = lerp(saturate(d), abs(d) * 0.5 + 0.5, Params.y);
    float3 c;
    if (Layer.z >= 0.0) {
        // the shape of AC's light: the sky's share by how far the surface faces up
        // (ksAmbient), the sun's by the angle (ksDiffuse); the two strengths stand in for
        // the weather's colours
        c = saturate(t.rgb * (1.2 * Layer.z * saturate(0.75 + 0.25 * n.y) + 2.0 * Layer.w * lit));
    } else {
        c = t.rgb * (Light.w + (1.0 - Light.w) * lit);
    }
    return float4(fogged(c, i.world), t.a);
}

// lines of a grid with cells of `size` metres, about one pixel wide at any distance
float grid(float2 p, float size) {
    float2 q = p / size;
    float2 w = max(fwidth(q), 1e-6);
    float2 g = abs(frac(q - 0.5) - 0.5) / w;
    return 1.0 - saturate(min(g.x, g.y));
}

float4 ps_ground(VSOut i) : SV_TARGET {
    float2 p = i.world.xz;
    float d = length(i.world - Camera.xyz);
    // 10 m tiles in two greys, so that speed is seen even where the lines blur
    float2 tile = floor(p / 10.0);
    float checker = frac((tile.x + tile.y) * 0.5) * 2.0;
    float3 c = lerp(float3(0.215, 0.220, 0.230), float3(0.255, 0.260, 0.270), checker);
    c += grid(p, 1.0) * 0.07 * saturate(1.0 - d / 45.0);
    c += grid(p, 10.0) * 0.16 * saturate(1.0 - d / 500.0);
    c += grid(p, 100.0) * 0.30 * saturate(1.0 - d / 2500.0);
    // the world's axes through the spawn point: x red, z blue
    float2 a = abs(p) / max(fwidth(p), 1e-6);
    c = lerp(c, float3(0.20, 0.35, 0.85), (1.0 - saturate(a.x - 1.0)) * 0.8);
    c = lerp(c, float3(0.85, 0.25, 0.20), (1.0 - saturate(a.y - 1.0)) * 0.8);
    return float4(fogged(c, i.world), 1.0);
}

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

#[repr(C)]
#[derive(Clone, Copy)]
struct DrawConstants {
    world: Mat,
    /// The camera's view matrix; what goes to the card in its place is `world` x `view`.
    view: Mat,
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

/// A car's kn5 model and the nodes the physics moves.
struct CarModel {
    model: GpuModel,
    /// `WHEEL_LF` ... (turn with the wheel) and `SUSP_LF` ... (follow the hub)
    wheels: [Option<usize>; 4],
    hubs: [Option<usize>; 4],
    /// `STEER_HR`, the steering wheel, and its matrix at rest
    steer: Option<(usize, Mat)>,
}

/// What the last frame drew of the models.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawStats {
    pub meshes: u32,
    pub triangles: u64,
}

/// The colour of the sky at the horizon; the ground fades into it.
const HORIZON: [f32; 4] = [0.52, 0.62, 0.74, 1.0];
/// As far as anything is drawn, m.
const FAR: f32 = 4000.0;
/// HUD vertices the dynamic buffer holds.
const HUD_CAPACITY: usize = 24_000;

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn compile(entry: &str, target: &str) -> Result<Vec<u8>, String> {
    let entry_z = format!("{entry}\0");
    let target_z = format!("{target}\0");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and both names are valid for the call; the blobs are read within
    // their reported sizes.
    unsafe {
        let result = D3DCompile(
            SHADERS.as_ptr() as *const _,
            SHADERS.len(),
            PCSTR::null(),
            None,
            None::<&windows::Win32::Graphics::Direct3D::ID3DInclude>,
            PCSTR(entry_z.as_ptr()),
            PCSTR(target_z.as_ptr()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        );
        if let Err(e) = result {
            let text = errors
                .map(|blob| String::from_utf8_lossy(std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).to_string())
                .unwrap_or_default();
            return Err(format!("shader {entry}: {e} {text}"));
        }
        let blob = code.ok_or("the shader compiler gave no code")?;
        Ok(std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()).to_vec())
    }
}

struct Targets {
    width: u32,
    height: u32,
    color: ID3D11Texture2D,
    color_view: ID3D11RenderTargetView,
    depth_view: ID3D11DepthStencilView,
    /// The finished picture, one sample per pixel.
    resolved: ID3D11Texture2D,
}

pub struct DebugRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    samples: u32,
    targets: Targets,
    mesh_vs: ID3D11VertexShader,
    mesh_ps: ID3D11PixelShader,
    ground_ps: ID3D11PixelShader,
    mesh_layout: ID3D11InputLayout,
    model_vs: ID3D11VertexShader,
    model_ps: ID3D11PixelShader,
    model_layout: ID3D11InputLayout,
    model_sampler: ID3D11SamplerState,
    white: ID3D11ShaderResourceView,
    track: Option<GpuModel>,
    car_model: Option<CarModel>,
    /// Draw the car as boxes even when its model is loaded (`--boxes`).
    pub boxes: bool,
    /// What the last frame drew of the track and the car model.
    pub drawn: DrawStats,
    hud_vs: ID3D11VertexShader,
    hud_ps: ID3D11PixelShader,
    hud_layout: ID3D11InputLayout,
    cube: (ID3D11Buffer, u32),
    cylinder: (ID3D11Buffer, u32),
    ground: (ID3D11Buffer, u32),
    hud_buffer: ID3D11Buffer,
    draw_constants: ID3D11Buffer,
    hud_constants: ID3D11Buffer,
    raster: ID3D11RasterizerState,
    /// AC's state for every mesh of a model (`cullStates[0]`, "eCullFront"): one side only.
    raster_model: ID3D11RasterizerState,
    /// AC's `eDepthNormal`: written, and the first thing drawn at a depth stays.
    depth_model: ID3D11DepthStencilState,
    /// AC's `eAlphaToCoverage` blend state (the "alpha tested" materials).
    blend_coverage: ID3D11BlendState,
    depth_on: ID3D11DepthStencilState,
    depth_read: ID3D11DepthStencilState,
    depth_off: ID3D11DepthStencilState,
    blend: ID3D11BlendState,
    sampler: ID3D11SamplerState,
    font_view: ID3D11ShaderResourceView,
    pub font: FontBitmap,
    /// The graphics card's name and whether it is the software rasteriser.
    pub adapter: String,
    pub software: bool,
}

fn vertex_buffer<T: Copy>(device: &ID3D11Device, data: &[T]) -> Result<(ID3D11Buffer, u32), String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of_val(data) as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        ..Default::default()
    };
    let initial = D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr() as *const _, ..Default::default() };
    let mut buffer = None;
    // SAFETY: `data` outlives the call, which copies it.
    unsafe { device.CreateBuffer(&desc, Some(&initial), Some(&mut buffer)).map_err(err("vertex buffer"))? };
    Ok((buffer.ok_or("no vertex buffer")?, data.len() as u32))
}

fn dynamic_buffer(device: &ID3D11Device, bytes: usize, bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer, String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let mut buffer = None;
    // SAFETY: a plain buffer creation.
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)).map_err(err("buffer"))? };
    buffer.ok_or("no buffer".to_string())
}

#[allow(clippy::too_many_arguments)]
fn texture(device: &ID3D11Device, width: u32, height: u32, format: DXGI_FORMAT, samples: u32, usage: D3D11_USAGE, bind: u32, cpu: u32) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: samples, Quality: 0 },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu,
        MiscFlags: 0,
    };
    let mut out = None;
    // SAFETY: a plain texture creation.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut out)).map_err(err("texture"))? };
    out.ok_or("no texture".to_string())
}

fn targets(device: &ID3D11Device, width: u32, height: u32, samples: u32) -> Result<Targets, String> {
    let (width, height) = (width.max(16), height.max(16));
    let color = texture(device, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, samples, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET.0 as u32, 0)?;
    let depth = texture(device, width, height, DXGI_FORMAT_D32_FLOAT, samples, D3D11_USAGE_DEFAULT, D3D11_BIND_DEPTH_STENCIL.0 as u32, 0)?;
    let resolved = texture(device, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 1, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET.0 as u32, 0)?;
    let (mut color_view, mut depth_view) = (None, None);
    // SAFETY: views of textures created just above.
    unsafe {
        device.CreateRenderTargetView(&color, None, Some(&mut color_view)).map_err(err("render target view"))?;
        device.CreateDepthStencilView(&depth, None, Some(&mut depth_view)).map_err(err("depth view"))?;
    }
    Ok(Targets { width, height, color, color_view: color_view.ok_or("no view")?, depth_view: depth_view.ok_or("no view")?, resolved })
}

impl DebugRenderer {
    /// A device (the graphics card, or Windows' software rasteriser where there is none) and
    /// everything the picture needs, for a picture of `width` x `height`.
    pub fn new(width: u32, height: u32) -> Result<DebugRenderer, String> {
        let levels: [D3D_FEATURE_LEVEL; 3] = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
        let create = |driver: D3D_DRIVER_TYPE| -> Result<(ID3D11Device, ID3D11DeviceContext), windows::core::Error> {
            let (mut device, mut context) = (None, None);
            // SAFETY: out pointers to locals.
            unsafe {
                D3D11CreateDevice(
                    None::<&IDXGIAdapter>,
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )?;
            }
            Ok((device.unwrap(), context.unwrap()))
        };
        let (device, context, software) = match create(D3D_DRIVER_TYPE_HARDWARE) {
            Ok((device, context)) => (device, context, false),
            Err(_) => {
                let (device, context) = create(D3D_DRIVER_TYPE_WARP).map_err(err("Direct3D 11 device"))?;
                (device, context, true)
            }
        };
        // SAFETY: COM calls on the live device.
        let adapter = unsafe {
            device
                .cast::<IDXGIDevice>()
                .and_then(|d| d.GetAdapter())
                .and_then(|a| a.GetDesc())
                .map(|desc| {
                    let length = desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len());
                    String::from_utf16_lossy(&desc.Description[..length])
                })
                .unwrap_or_else(|_| "unknown adapter".to_string())
        };
        // SAFETY: a capability query.
        let samples = if unsafe { device.CheckMultisampleQualityLevels(DXGI_FORMAT_R8G8B8A8_UNORM, 4) }.is_ok_and(|levels| levels > 0) { 4 } else { 1 };

        let mesh_vs_code = compile("vs_mesh", "vs_4_0")?;
        let mesh_ps_code = compile("ps_mesh", "ps_4_0")?;
        let ground_ps_code = compile("ps_ground", "ps_4_0")?;
        let model_vs_code = compile("vs_model", "vs_4_0")?;
        let model_ps_code = compile("ps_model", "ps_4_0")?;
        let hud_vs_code = compile("vs_hud", "vs_4_0")?;
        let hud_ps_code = compile("ps_hud", "ps_4_0")?;
        let element = |name: &'static [u8], format: DXGI_FORMAT, offset: u32| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(name.as_ptr()),
            SemanticIndex: 0,
            Format: format,
            InputSlot: 0,
            AlignedByteOffset: offset,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        };
        let mesh_elements = [element(b"POSITION\0", DXGI_FORMAT_R32G32B32_FLOAT, 0), element(b"NORMAL\0", DXGI_FORMAT_R32G32B32_FLOAT, 12)];
        let model_elements = [
            element(b"POSITION\0", DXGI_FORMAT_R32G32B32_FLOAT, 0),
            element(b"NORMAL\0", DXGI_FORMAT_R32G32B32_FLOAT, 12),
            element(b"TEXCOORD\0", DXGI_FORMAT_R32G32_FLOAT, 24),
        ];
        let hud_elements = [
            element(b"POSITION\0", DXGI_FORMAT_R32G32_FLOAT, 0),
            element(b"TEXCOORD\0", DXGI_FORMAT_R32G32_FLOAT, 8),
            element(b"COLOR\0", DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
        ];
        let font = FontBitmap::new(30);
        // SAFETY: creation calls with valid descriptors and out pointers; every created
        // object is checked before use.
        unsafe {
            let (mut mesh_vs, mut mesh_ps, mut ground_ps, mut hud_vs, mut hud_ps) = (None, None, None, None, None);
            device.CreateVertexShader(&mesh_vs_code, None, Some(&mut mesh_vs)).map_err(err("vertex shader"))?;
            device.CreatePixelShader(&mesh_ps_code, None, Some(&mut mesh_ps)).map_err(err("pixel shader"))?;
            device.CreatePixelShader(&ground_ps_code, None, Some(&mut ground_ps)).map_err(err("pixel shader"))?;
            device.CreateVertexShader(&hud_vs_code, None, Some(&mut hud_vs)).map_err(err("vertex shader"))?;
            device.CreatePixelShader(&hud_ps_code, None, Some(&mut hud_ps)).map_err(err("pixel shader"))?;
            let (mut model_vs, mut model_ps, mut model_layout) = (None, None, None);
            device.CreateVertexShader(&model_vs_code, None, Some(&mut model_vs)).map_err(err("vertex shader"))?;
            device.CreatePixelShader(&model_ps_code, None, Some(&mut model_ps)).map_err(err("pixel shader"))?;
            device.CreateInputLayout(&model_elements, &model_vs_code, Some(&mut model_layout)).map_err(err("input layout"))?;
            let (mut mesh_layout, mut hud_layout) = (None, None);
            device.CreateInputLayout(&mesh_elements, &mesh_vs_code, Some(&mut mesh_layout)).map_err(err("input layout"))?;
            device.CreateInputLayout(&hud_elements, &hud_vs_code, Some(&mut hud_layout)).map_err(err("input layout"))?;

            let raster_desc = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                // both sides are drawn: the shapes are closed, and winding needs no thought
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                MultisampleEnable: true.into(),
                ..Default::default()
            };
            let mut raster = None;
            device.CreateRasterizerState(&raster_desc, Some(&mut raster)).map_err(err("rasterizer state"))?;
            // kn5 meshes are one-sided in the game (`initCullStates` @ 0x14001b0a0, state 0:
            // CullMode FRONT, FrontCounterClockwise FALSE); a board is modelled as two faces
            // back to back, which fight for the same pixels when both sides of each are drawn
            let raster_model_desc = D3D11_RASTERIZER_DESC { CullMode: D3D11_CULL_FRONT, FrontCounterClockwise: false.into(), ..raster_desc };
            let mut raster_model = None;
            device.CreateRasterizerState(&raster_model_desc, Some(&mut raster_model)).map_err(err("rasterizer state"))?;
            // the depth runs from 1 (near) to 0 (far), see `perspective_reversed`: "nearer" is
            // "greater". `strict`: what is drawn first at a depth stays (AC's eDepthNormal is LESS)
            let depth_state_of = |enable: bool, write: bool, strict: bool| -> Result<ID3D11DepthStencilState, String> {
                let desc = D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: enable.into(),
                    DepthWriteMask: if write { D3D11_DEPTH_WRITE_MASK_ALL } else { D3D11_DEPTH_WRITE_MASK_ZERO },
                    DepthFunc: if strict { D3D11_COMPARISON_GREATER } else { D3D11_COMPARISON_GREATER_EQUAL },
                    ..Default::default()
                };
                let mut state = None;
                device.CreateDepthStencilState(&desc, Some(&mut state)).map_err(err("depth state"))?;
                state.ok_or("no depth state".to_string())
            };
            let depth_state = |enable: bool, write: bool| depth_state_of(enable, write, false);
            // AC's blend state 2 (`initBlendStates` @ 0x14001aeb0): no blending, the pixel's
            // alpha decides how many of its samples are covered
            let mut coverage_desc = D3D11_BLEND_DESC { AlphaToCoverageEnable: true.into(), ..Default::default() };
            coverage_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: false.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ONE,
                BlendOpAlpha: D3D11_BLEND_OP_MAX,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend_coverage = None;
            device.CreateBlendState(&coverage_desc, Some(&mut blend_coverage)).map_err(err("blend state"))?;
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
            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut sampler = None;
            device.CreateSamplerState(&sampler_desc, Some(&mut sampler)).map_err(err("sampler"))?;

            // the models' textures repeat and are seen at flat angles
            let model_sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_ANISOTROPIC,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MaxAnisotropy: 8,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut model_sampler = None;
            device.CreateSamplerState(&model_sampler_desc, Some(&mut model_sampler)).map_err(err("sampler"))?;
            // a white pixel for materials without a picture
            let white_desc = D3D11_TEXTURE2D_DESC {
                Width: 1,
                Height: 1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let white_pixel = [255u8; 4];
            let white_data = D3D11_SUBRESOURCE_DATA { pSysMem: white_pixel.as_ptr() as *const _, SysMemPitch: 4, SysMemSlicePitch: 0 };
            let (mut white_texture, mut white) = (None, None);
            device.CreateTexture2D(&white_desc, Some(&white_data), Some(&mut white_texture)).map_err(err("white texture"))?;
            device.CreateShaderResourceView(white_texture.as_ref().ok_or("no white texture")?, None, Some(&mut white)).map_err(err("white view"))?;

            // the font
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

            let up = [0.0, 1.0, 0.0];
            let corner = |x: f32, z: f32| Vertex { position: [x, 0.0, z], normal: up };
            let quad = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
            Ok(DebugRenderer {
                samples,
                targets: targets(&device, width, height, samples)?,
                mesh_vs: mesh_vs.ok_or("no shader")?,
                mesh_ps: mesh_ps.ok_or("no shader")?,
                ground_ps: ground_ps.ok_or("no shader")?,
                mesh_layout: mesh_layout.ok_or("no layout")?,
                model_vs: model_vs.ok_or("no shader")?,
                model_ps: model_ps.ok_or("no shader")?,
                model_layout: model_layout.ok_or("no layout")?,
                model_sampler: model_sampler.ok_or("no sampler")?,
                white: white.ok_or("no white view")?,
                track: None,
                car_model: None,
                boxes: false,
                drawn: DrawStats::default(),
                hud_vs: hud_vs.ok_or("no shader")?,
                hud_ps: hud_ps.ok_or("no shader")?,
                hud_layout: hud_layout.ok_or("no layout")?,
                cube: vertex_buffer(&device, &cube())?,
                cylinder: vertex_buffer(&device, &cylinder(28))?,
                ground: vertex_buffer(&device, &quad)?,
                hud_buffer: dynamic_buffer(&device, HUD_CAPACITY * std::mem::size_of::<HudVertex>(), D3D11_BIND_VERTEX_BUFFER)?,
                draw_constants: dynamic_buffer(&device, std::mem::size_of::<DrawConstants>(), D3D11_BIND_CONSTANT_BUFFER)?,
                hud_constants: dynamic_buffer(&device, 16, D3D11_BIND_CONSTANT_BUFFER)?,
                raster: raster.ok_or("no rasterizer state")?,
                raster_model: raster_model.ok_or("no rasterizer state")?,
                depth_model: depth_state_of(true, true, true)?,
                blend_coverage: blend_coverage.ok_or("no blend state")?,
                depth_on: depth_state(true, true)?,
                depth_read: depth_state(true, false)?,
                depth_off: depth_state(false, false)?,
                blend: blend.ok_or("no blend state")?,
                sampler: sampler.ok_or("no sampler")?,
                font_view: font_view.ok_or("no font view")?,
                font,
                adapter,
                software,
                device,
                context,
            })
        }
    }

    /// Puts a track's kn5 models on the card; from then on they are drawn instead of the grid.
    pub fn load_track(&mut self, files: &[std::path::PathBuf], placements: &[models::Placement], options: &ModelOptions) -> Result<ModelStats, String> {
        let model = GpuModel::load(&self.device, &self.context, files, placements, options)?;
        let stats = model.stats.clone();
        self.track = Some(model);
        Ok(stats)
    }

    /// Puts a car's kn5 model on the card; it is drawn instead of the boxes unless
    /// [`DebugRenderer::boxes`] is set.
    pub fn load_car(&mut self, file: &std::path::Path, options: &ModelOptions) -> Result<ModelStats, String> {
        let model = GpuModel::load(&self.device, &self.context, &[file.to_path_buf()], &[], options)?;
        let stats = model.stats.clone();
        let names = ["LF", "RF", "LR", "RR"];
        let wheels = names.map(|n| model.find_node(&format!("WHEEL_{n}")));
        let hubs = names.map(|n| model.find_node(&format!("SUSP_{n}")));
        let steer = model.find_node("STEER_HR").map(|node| (node, model.nodes[node].local));
        self.car_model = Some(CarModel { model, wheels, hubs, steer });
        Ok(stats)
    }

    pub fn has_track(&self) -> bool {
        self.track.is_some()
    }

    pub fn has_car_model(&self) -> bool {
        self.car_model.is_some()
    }

    /// Draws a kn5 model. `planes` and the LOD ranges are only used with `cull`.
    fn draw_model(&self, model: &GpuModel, base: &DrawConstants, planes: &[[f32; 4]; 6], eye: [f32; 3], cull: bool) -> DrawStats {
        let mut stats = DrawStats::default();
        let stride = std::mem::size_of::<rustyac_content::Vertex>() as u32;
        let mut transparent: Vec<(f32, usize)> = Vec::new();
        // with one sample per pixel there is no coverage to dither: a plain alpha test then
        let coverage = self.samples > 1;
        // SAFETY: state objects, buffers and views are owned by `self` / `model` and alive.
        unsafe {
            let c = &self.context;
            c.RSSetState(&self.raster_model);
            c.IASetInputLayout(&self.model_layout);
            c.VSSetShader(&self.model_vs, None);
            c.PSSetShader(&self.model_ps, None);
            c.PSSetSamplers(0, Some(&[Some(self.model_sampler.clone())]));
            let mut draw = |index: usize, transparent_pass: bool| {
                let mesh = &model.meshes[index];
                let material = &model.materials[mesh.material];
                // `Material::apply` @ 0x14020a6e0: the blend state is the material's; the depth
                // state is the material's in the opaque pass and "no write" in the other
                match material.blend_mode {
                    1 => c.OMSetBlendState(&self.blend, None, 0xffff_ffff),
                    2 if coverage => c.OMSetBlendState(&self.blend_coverage, None, 0xffff_ffff),
                    _ => c.OMSetBlendState(None, None, 0xffff_ffff),
                }
                let depth = match material.depth_mode {
                    _ if transparent_pass => &self.depth_read,
                    1 => &self.depth_read,
                    2 => &self.depth_off,
                    _ => &self.depth_model,
                };
                c.OMSetDepthStencilState(depth, 0);
                let alpha_ref = if material.blend_mode == 2 && !coverage { 0.5 } else { 0.0 };
                let [r, g, b, a] = material.mult;
                let ks = material.ks.unwrap_or([-1.0, -1.0]);
                let constants = DrawConstants {
                    world: model.world[mesh.node],
                    color: material.color,
                    params: [alpha_ref, if material.foliage { 1.0 } else { 0.0 }, material.detail.as_ref().map(|d| d.1).unwrap_or(0.0), 0.0],
                    mult_rg: [r[0], r[1], g[0], g[1]],
                    mult_ba: [b[0], b[1], a[0], a[1]],
                    layer: [material.kind as f32, material.magic, ks[0], ks[1]],
                    layer2: [material.uv_mult, material.alpha_scale, 0.0, 0.0],
                    ..*base
                };
                self.upload(&constants);
                let white = || Some(self.white.clone());
                let texture = material.texture.clone().or_else(white);
                let detail = material.detail.as_ref().map(|d| d.0.clone()).or_else(white);
                let layer = |k: usize| material.layers[k].clone().or_else(white);
                c.PSSetShaderResources(0, Some(&[texture, detail, layer(0), layer(1), layer(2), layer(3), layer(4)]));
                c.IASetVertexBuffers(0, 1, Some(&Some(mesh.vertices.clone())), Some(&stride), Some(&0));
                c.IASetIndexBuffer(&mesh.indices, models::INDEX_FORMAT, 0);
                c.DrawIndexed(mesh.index_count, 0, 0);
                stats.meshes += 1;
                stats.triangles += mesh.index_count as u64 / 3;
            };
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
                // the game's two passes (`CameraShadowMapped::renderPass` @ 0x14020cf20): the
                // meshes that are not `isTransparent`, in the file's order; then the others
                if mesh.transparent {
                    transparent.push((distance, index));
                } else {
                    draw(index, false);
                }
            }
            // (the game draws these in the file's order too; farthest first is kinder to
            // panes of glass behind each other)
            transparent.sort_by(|a, b| b.0.total_cmp(&a.0));
            for &(_, index) in &transparent {
                draw(index, true);
            }
            c.OMSetBlendState(None, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_on, 0);
            c.RSSetState(&self.raster);
            c.IASetInputLayout(&self.mesh_layout);
            c.VSSetShader(&self.mesh_vs, None);
            c.PSSetShader(&self.mesh_ps, None);
        }
        stats
    }

    pub fn size(&self) -> (u32, u32) {
        (self.targets.width, self.targets.height)
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if (width.max(16), height.max(16)) != self.size() {
            self.targets = targets(&self.device, width, height, self.samples)?;
        }
        Ok(())
    }

    fn write<T: Copy>(&self, buffer: &ID3D11Buffer, data: &[T]) {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the buffer is dynamic and at least as large as `data` (the callers see to it).
        unsafe {
            if self.context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)).is_ok() {
                std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, mapped.pData as *mut u8, std::mem::size_of_val(data));
                self.context.Unmap(buffer, 0);
            }
        }
    }

    /// Hands a draw's constants to the card, `view` replaced by `world` x `view`.
    fn upload(&self, constants: &DrawConstants) {
        let on_card = DrawConstants { view: mul_precise(&constants.world, &constants.view), ..*constants };
        self.write(&self.draw_constants, std::slice::from_ref(&on_card));
    }

    fn solid(&self, mesh: &(ID3D11Buffer, u32), constants: &DrawConstants) {
        self.upload(constants);
        let stride = std::mem::size_of::<Vertex>() as u32;
        // SAFETY: the buffers are alive; the pointers are to locals that outlive the calls.
        unsafe {
            self.context.IASetVertexBuffers(0, 1, Some(&Some(mesh.0.clone())), Some(&stride), Some(&0));
            self.context.Draw(mesh.1, 0);
        }
    }

    /// Draws one frame into the off-screen picture.
    pub fn draw(&mut self, view: &CarView, shape: &CarShape, camera: &CameraFrame, info: &HudInfo) {
        let (width, height) = self.size();
        let projection = perspective_reversed(camera.fov, width as f32 / height as f32, camera.near.max(0.02), FAR);
        let view_matrix = view_matrix(&camera.matrix);
        let view_proj = mul(&view_matrix, &projection);
        let eye = camera.matrix[3];
        let base = DrawConstants {
            world: crate::view::IDENTITY,
            view: view_matrix,
            proj: projection,
            color: [1.0; 4],
            // the sun: from the left front, high
            light: [-0.35, -0.80, -0.48, 0.42],
            // on a track the view is long: thinner haze
            camera: [eye[0], eye[1], eye[2], if self.track.is_some() { 0.00022 } else { 0.0011 }],
            fog: HORIZON,
            params: [0.0; 4],
            mult_rg: [0.0; 4],
            mult_ba: [0.0; 4],
            layer: [0.0, 1.0, -1.0, -1.0],
            layer2: [1.0, 1.0, 0.0, 0.0],
        };
        let planes = frustum(&view_proj);
        let eye3 = [eye[0], eye[1], eye[2]];
        // the car's model follows the physics: the body with the model's offset, the wheels
        // and hubs where the physics has them, the steering wheel turned
        let use_car_model = self.car_model.is_some() && !self.boxes;
        if use_car_model {
            if let Some(car) = self.car_model.as_mut() {
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
                    car.model.nodes[node].local = mul(&turn, &rest);
                }
                car.model.update(&root, &fixed);
            }
        }
        let mut drawn = DrawStats::default();
        let viewport = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        // SAFETY: state objects and views owned by `self`; slices and pointers are to locals.
        unsafe {
            let c = &self.context;
            c.OMSetRenderTargets(Some(&[Some(self.targets.color_view.clone())]), &self.targets.depth_view);
            c.RSSetViewports(Some(&[viewport]));
            c.ClearRenderTargetView(&self.targets.color_view, &HORIZON);
            // (the depth runs from 1 at the near plane to 0 at the far one)
            c.ClearDepthStencilView(&self.targets.depth_view, D3D11_CLEAR_DEPTH.0, 0.0, 0);
            c.RSSetState(&self.raster);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.IASetInputLayout(&self.mesh_layout);
            c.VSSetShader(&self.mesh_vs, None);
            c.VSSetConstantBuffers(0, Some(&[Some(self.draw_constants.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.draw_constants.clone())]));
            c.OMSetBlendState(None, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_on, 0);

            let body = &view.body;
            let flat: Mat = if let Some(track) = &self.track {
                // the track's models
                let stats = self.draw_model(track, &base, &planes, eye3, true);
                drawn.meshes += stats.meshes;
                drawn.triangles += stats.triangles;
                // the shadow lies in the plane the four tyres stand on
                let mut drop = 0.0;
                for k in 0..4 {
                    let w = view.wheels[k][3];
                    drop += (w[0] - body[3][0]) * body[1][0] + (w[1] - body[3][1]) * body[1][1] + (w[2] - body[3][2]) * body[1][2] - view.tyre_radius[k];
                }
                mul(&translation(0.0, drop * 0.25 + 0.012, 0.0), body)
            } else {
                // the ground: one big square that moves with the camera in whole 100 m steps
                c.PSSetShader(&self.ground_ps, None);
                let snap = |x: f32| (x / 100.0).round() * 100.0;
                let ground = scale_then(FAR, 1.0, FAR, &translation(snap(eye[0]), 0.0, snap(eye[2])));
                self.solid(&self.ground, &DrawConstants { world: ground, ..base });
                let heading = {
                    let (x, z) = (body[2][0], body[2][2]);
                    let length = (x * x + z * z).sqrt().max(1e-6);
                    [x / length, z / length]
                };
                [[heading[1], 0.0, -heading[0], 0.0], [0.0, 1.0, 0.0, 0.0], [heading[0], 0.0, heading[1], 0.0], [body[3][0], 0.012, body[3][2], 1.0]]
            };

            // the car's shadow: a dark patch on the road under the body
            c.PSSetShader(&self.mesh_ps, None);
            c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_read, 0);
            let (mut x_max, mut z_min, mut z_max) = (0.5f32, -1.0f32, 1.0f32);
            for b in &shape.boxes {
                x_max = x_max.max(b.centre[0].abs() + b.size[0] * 0.5);
                z_min = z_min.min(b.centre[2] - b.size[2] * 0.5);
                z_max = z_max.max(b.centre[2] + b.size[2] * 0.5);
            }
            let shadow = mul(&scale_then(x_max * 2.0, 0.002, z_max - z_min, &translation(0.0, 0.0, (z_max + z_min) * 0.5)), &flat);
            self.solid(&self.cube, &DrawConstants { world: shadow, color: [0.0, 0.0, 0.0, 0.42], ..base });
            c.OMSetBlendState(None, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_on, 0);

            if use_car_model {
                if let Some(car) = &self.car_model {
                    let stats = self.draw_model(&car.model, &base, &planes, eye3, false);
                    drawn.meshes += stats.meshes;
                    drawn.triangles += stats.triangles;
                }
            }
            // the body
            for b in shape.boxes.iter().filter(|_| !use_car_model) {
                let world = mul(&scale_then(b.size[0], b.size[1], b.size[2], &translation(b.centre[0], b.centre[1], b.centre[2])), body);
                self.solid(&self.cube, &DrawConstants { world, color: [b.color[0], b.color[1], b.color[2], 1.0], ..base });
            }
            // the wheels: tyre, rim, and a bar across the rim that shows the spin
            for wheel in (0..4).filter(|_| !use_car_model) {
                let m = &view.wheels[wheel];
                let (radius, width, rim) = (view.tyre_radius[wheel], view.tyre_width[wheel], view.rim_radius[wheel]);
                // a tyre past its peak slip reddens
                let slip = (view.wheel_slip[wheel] - 1.0).clamp(0.0, 1.0);
                let tyre = [0.09 + 0.5 * slip, 0.09, 0.10, 1.0];
                self.solid(&self.cylinder, &DrawConstants { world: scale_then(width, radius, radius, m), color: tyre, ..base });
                self.solid(&self.cylinder, &DrawConstants { world: scale_then(width * 1.03, rim, rim, m), color: [0.55, 0.56, 0.60, 1.0], ..base });
                self.solid(&self.cube, &DrawConstants { world: scale_then(width * 1.06, rim * 1.9, rim * 0.5, m), color: [0.95, 0.85, 0.2, 1.0], ..base });
            }

            // the HUD
            let vertices = hud::build(&self.font, view, info, width as f32, height as f32);
            let count = vertices.len().min(HUD_CAPACITY);
            self.write(&self.hud_buffer, &vertices[..count]);
            self.write(&self.hud_constants, &[[width as f32, height as f32, 0.0, 0.0]]);
            let stride = std::mem::size_of::<HudVertex>() as u32;
            c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_off, 0);
            c.IASetInputLayout(&self.hud_layout);
            c.VSSetShader(&self.hud_vs, None);
            c.PSSetShader(&self.hud_ps, None);
            c.VSSetConstantBuffers(0, Some(&[Some(self.hud_constants.clone())]));
            c.PSSetShaderResources(0, Some(&[Some(self.font_view.clone())]));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            c.IASetVertexBuffers(0, 1, Some(&Some(self.hud_buffer.clone())), Some(&stride), Some(&0));
            c.Draw(count as u32, 0);

            // the finished picture
            if self.samples > 1 {
                c.ResolveSubresource(&self.targets.resolved, 0, &self.targets.color, 0, DXGI_FORMAT_R8G8B8A8_UNORM);
            } else {
                c.CopyResource(&self.targets.resolved, &self.targets.color);
            }
        }
        self.drawn = drawn;
    }

    /// The last picture as RGBA bytes, rows from the top.
    pub fn read_pixels(&mut self) -> Result<Vec<u8>, String> {
        let (width, height) = self.size();
        let staging = texture(&self.device, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 1, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32)?;
        let mut out = vec![0u8; (width * height * 4) as usize];
        // SAFETY: the staging texture is mapped for reading and read within its rows.
        unsafe {
            self.context.CopyResource(&staging, &self.targets.resolved);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(err("reading the picture back"))?;
            for y in 0..height as usize {
                let row = std::slice::from_raw_parts((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), width as usize * 4);
                out[y * width as usize * 4..(y + 1) * width as usize * 4].copy_from_slice(row);
            }
            self.context.Unmap(&staging, 0);
        }
        // the picture is opaque whatever the blending left in the alpha channel
        for pixel in out.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(out)
    }

    /// Waits until the card has really drawn what was asked for (for timing frames that are
    /// not shown).
    pub fn finish(&self) {
        // SAFETY: a query created, issued and polled on the live context.
        unsafe {
            let desc = D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 };
            let mut query = None;
            if self.device.CreateQuery(&desc, Some(&mut query)).is_err() {
                return;
            }
            let Some(query) = query else { return };
            self.context.End(&query);
            self.context.Flush();
            let mut done: i32 = 0;
            for _ in 0..200_000 {
                let hr = self.context.GetData(&query, Some(&mut done as *mut i32 as *mut _), 4, 0);
                if hr.is_ok() && done != 0 {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }

    /// A swap chain on a window, for [`DebugRenderer::present`].
    pub fn swap_chain(&self, window: HWND) -> Result<IDXGISwapChain1, String> {
        // SAFETY: COM calls on the live device with a valid window handle.
        unsafe {
            let factory: IDXGIFactory2 = self
                .device
                .cast::<IDXGIDevice>()
                .and_then(|d| d.GetAdapter())
                .and_then(|a| a.GetParent())
                .map_err(err("DXGI factory"))?;
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
            let chain = factory.CreateSwapChainForHwnd(&self.device, window, &desc, None, None).map_err(err("swap chain"))?;
            let _ = factory.MakeWindowAssociation(window, DXGI_MWA_NO_ALT_ENTER);
            Ok(chain)
        }
    }

    /// The window changed its size: the swap chain and the picture follow.
    pub fn resize_swap_chain(&mut self, chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), String> {
        // SAFETY: nothing holds the back buffers between two frames.
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            chain.ResizeBuffers(0, width.max(16), height.max(16), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(err("resizing the swap chain"))?;
        }
        self.resize(width, height)
    }

    /// Shows the last picture in the window. `Ok(false)`: the window is covered or minimised
    /// and nothing was shown (the caller should not spin).
    pub fn present(&self, chain: &IDXGISwapChain1, vsync: bool) -> Result<bool, String> {
        // SAFETY: the back buffer and the picture have the same size and format.
        unsafe {
            let back: ID3D11Texture2D = chain.GetBuffer(0).map_err(err("back buffer"))?;
            self.context.CopyResource(&back, &self.targets.resolved);
            let status = chain.Present(vsync as u32, DXGI_PRESENT(0));
            status.ok().map_err(err("present"))?;
            // DXGI_STATUS_OCCLUDED
            Ok(status.0 != 0x087A_0001)
        }
    }
}

/// Writes RGBA pixels as a PNG.
pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    let mut writer = encoder.write_header().map_err(|e| format!("{}: {e}", path.display()))?;
    writer.write_image_data(rgba).map_err(|e| format!("{}: {e}", path.display()))?;
    // the end of the file is written here, not when the writer is dropped (which could not say that it failed)
    writer.finish().map_err(|e| format!("{}: {e}", path.display()))
}
