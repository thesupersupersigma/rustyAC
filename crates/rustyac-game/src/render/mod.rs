//! The debug view: a throw-away Direct3D 11 renderer that draws an endless flat ground with
//! a grid, the car as boxes and cylinders, and a text HUD. It is not AC's renderer and shares
//! nothing with it but the API; when AC's renderer is ported this module goes away.
//!
//! Everything is drawn into an off-screen target (4x multisampled where the card can), which
//! is then copied to the window or read back for `--screenshot`: the window is optional.

pub mod font;
pub mod hud;
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
use scene::{cube, cylinder, mul, perspective, scale_then, translation, view_matrix, CameraFrame, CarShape, Vertex};

const SHADERS: &str = r#"
cbuffer PerDraw : register(b0) {
    row_major float4x4 World;
    row_major float4x4 ViewProj;
    float4 Color;
    float4 Light;       // xyz: the direction the light travels, w: ambient share
    float4 Camera;      // xyz: the camera's position, w: fog density per metre
    float4 Fog;         // rgb: the colour of the horizon
};
struct VSIn { float3 pos : POSITION; float3 normal : NORMAL; };
struct VSOut { float4 pos : SV_POSITION; float3 normal : NORMAL; float3 world : TEXCOORD0; };

VSOut vs_mesh(VSIn i) {
    VSOut o;
    float4 w = mul(float4(i.pos, 1.0), World);
    o.pos = mul(w, ViewProj);
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
    view_proj: Mat,
    color: [f32; 4],
    light: [f32; 4],
    camera: [f32; 4],
    fog: [f32; 4],
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
            let depth_state = |enable: bool, write: bool| -> Result<ID3D11DepthStencilState, String> {
                let desc = D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: enable.into(),
                    DepthWriteMask: if write { D3D11_DEPTH_WRITE_MASK_ALL } else { D3D11_DEPTH_WRITE_MASK_ZERO },
                    DepthFunc: D3D11_COMPARISON_LESS_EQUAL,
                    ..Default::default()
                };
                let mut state = None;
                device.CreateDepthStencilState(&desc, Some(&mut state)).map_err(err("depth state"))?;
                state.ok_or("no depth state".to_string())
            };
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

    fn solid(&self, mesh: &(ID3D11Buffer, u32), constants: &DrawConstants) {
        self.write(&self.draw_constants, std::slice::from_ref(constants));
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
        let projection = perspective(camera.fov, width as f32 / height as f32, camera.near.max(0.02), FAR);
        let view_proj = mul(&view_matrix(&camera.matrix), &projection);
        let eye = camera.matrix[3];
        let base = DrawConstants {
            world: crate::view::IDENTITY,
            view_proj,
            color: [1.0; 4],
            // the sun: from the left front, high
            light: [-0.35, -0.80, -0.48, 0.42],
            camera: [eye[0], eye[1], eye[2], 0.0011],
            fog: HORIZON,
        };
        let viewport = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        // SAFETY: state objects and views owned by `self`; slices and pointers are to locals.
        unsafe {
            let c = &self.context;
            c.OMSetRenderTargets(Some(&[Some(self.targets.color_view.clone())]), &self.targets.depth_view);
            c.RSSetViewports(Some(&[viewport]));
            c.ClearRenderTargetView(&self.targets.color_view, &HORIZON);
            c.ClearDepthStencilView(&self.targets.depth_view, D3D11_CLEAR_DEPTH.0 as u32, 1.0, 0);
            c.RSSetState(&self.raster);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.IASetInputLayout(&self.mesh_layout);
            c.VSSetShader(&self.mesh_vs, None);
            c.VSSetConstantBuffers(0, Some(&[Some(self.draw_constants.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.draw_constants.clone())]));
            c.OMSetBlendState(None, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_on, 0);

            // the ground: one big square that moves with the camera in whole 100 m steps
            c.PSSetShader(&self.ground_ps, None);
            let snap = |x: f32| (x / 100.0).round() * 100.0;
            let ground = scale_then(FAR, 1.0, FAR, &translation(snap(eye[0]), 0.0, snap(eye[2])));
            self.solid(&self.ground, &DrawConstants { world: ground, ..base });

            // the car's shadow: a dark patch on the road under the body
            c.PSSetShader(&self.mesh_ps, None);
            c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            c.OMSetDepthStencilState(&self.depth_read, 0);
            let body = &view.body;
            let heading = {
                let (x, z) = (body[2][0], body[2][2]);
                let length = (x * x + z * z).sqrt().max(1e-6);
                [x / length, z / length]
            };
            let flat: Mat = [[heading[1], 0.0, -heading[0], 0.0], [0.0, 1.0, 0.0, 0.0], [heading[0], 0.0, heading[1], 0.0], [body[3][0], 0.012, body[3][2], 1.0]];
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

            // the body
            for b in &shape.boxes {
                let world = mul(&scale_then(b.size[0], b.size[1], b.size[2], &translation(b.centre[0], b.centre[1], b.centre[2])), body);
                self.solid(&self.cube, &DrawConstants { world, color: [b.color[0], b.color[1], b.color[2], 1.0], ..base });
            }
            // the wheels: tyre, rim, and a bar across the rim that shows the spin
            for wheel in 0..4 {
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

    /// Shows the last picture in the window.
    pub fn present(&self, chain: &IDXGISwapChain1, vsync: bool) -> Result<(), String> {
        // SAFETY: the back buffer and the picture have the same size and format.
        unsafe {
            let back: ID3D11Texture2D = chain.GetBuffer(0).map_err(err("back buffer"))?;
            self.context.CopyResource(&back, &self.targets.resolved);
            chain.Present(vsync as u32, DXGI_PRESENT(0)).ok().map_err(err("present"))
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
    writer.write_image_data(rgba).map_err(|e| format!("{}: {e}", path.display()))
}
