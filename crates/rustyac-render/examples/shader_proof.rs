// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-shader proof of the WGSL translation: every shader pair in AC's
//! `system/shaders/win` draws the same triangle with the same inputs twice, once as the
//! original bytecode on Direct3D 11 (WARP) and once as rustyac-dxbc's WGSL on wgpu, into
//! 32-bit float targets, and the two pictures are compared bit for bit (largest difference
//! in units in the last place, ULP).
//!
//! The inputs are made up but fixed (a seed from the shader's name): every matrix is the
//! identity, so the triangle covers the target, every other constant is in 0..1, the
//! textures are noise with all their mip levels, depth maps are noise in 0..1. Nothing
//! translated is written anywhere. Multisampled inputs (4 samples) are drawn by a small
//! pass of exact numbers on both sides; a pixel shader that writes SV_Depth gets a depth
//! target as well, read back and compared like the colour targets.
//!
//! `cargo run --release -p rustyac-render --features wgpu --example shader_proof -- [--adapter warp|swiftshader|gpu] [--flat] [--ops] [name...]`
//!
//! `--adapter swiftshader` is SwiftShader's Vulkan driver, found through VK_DRIVER_FILES.
//! `--flat`: every texture one value (half floats), so filtering drops out of the
//! comparison. `--ops`: single instructions alone (exp2, log2, rsqrt, ...) in place of AC's
//! pairs. For narrowing a difference down: PROOF_DIFF=1 / PROOF_VALUES=1 print values,
//! PROOF_DUMP=1 the WGSL, PROOF_PS_HLSL=file an HLSL pixel shader in place of the pair's on
//! both sides, PROOF_PS / PROOF_VS=file a WGSL one on the wgpu side, PROOF_LOG=1 wgpu's
//! warnings.

use std::collections::BTreeMap;
use std::ffi::c_void;

use rustyac_dxbc::Shader;
use windows::core::PCSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SIZE: u32 = 32;
const TEX: u32 = 16;
const MIPS: u32 = 5;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }

    /// 0..1 in steps of 1/65536 (exact in f32)
    fn unit(&mut self) -> f32 {
        (self.next() & 0xffff) as f32 / 65536.0
    }
}

/// One shader pair's inputs, the same for both devices.
struct Inputs {
    vertex_stride: u32,
    /// (semantic, index, format, offset)
    elements: Vec<(&'static str, u32, DXGI_FORMAT, u32)>,
    vertices: Vec<u8>,
    cbuffers: BTreeMap<u32, Vec<u8>>,
    /// slot -> (cube, depth, levels of (size, bytes))
    textures: BTreeMap<u32, (bool, bool, Vec<Vec<u8>>)>,
    /// slot -> comparison
    samplers: BTreeMap<u32, bool>,
    targets: Vec<u32>,
    /// multisampled inputs (RGBA16F, SIZE x SIZE, MS_COUNT samples), filled by a pass of [`MS_FILL`]'s numbers
    ms: Vec<u32>,
    /// the pixel shader writes SV_Depth: a depth target is drawn and read back as well
    writes_depth: bool,
}

const MS_COUNT: u32 = 4;

/// The numbers of a multisampled input, the same in HLSL (Direct3D 11) and WGSL: small
/// integers over 64, exact in a half float, from the pixel, the sample and the slot.
fn ms_fill_hlsl(slot: u32) -> String {
    format!(
        "float4 vs(uint i : SV_VertexID) : SV_Position {{ return float4(i == 1 ? 3.0 : -1.0, i == 2 ? 3.0 : -1.0, 0.0, 1.0); }}
         float4 ps(float4 p : SV_Position, uint s : SV_SampleIndex) : SV_Target {{
             uint4 k = uint4(0, 17, 34, 51) + {slot} * 3;
             uint b = (uint)p.x * 7 + (uint)p.y * 13 + s * 29;
             return float4((b + k) & 63) / 64.0;
         }}"
    )
}

fn ms_fill_wgsl(slot: u32) -> String {
    format!(
        "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
             return vec4<f32>(select(-1.0, 3.0, i == 1u), select(-1.0, 3.0, i == 2u), 0.0, 1.0);
         }}
         @fragment fn fs(@builtin(position) p: vec4<f32>, @builtin(sample_index) s: u32) -> @location(0) vec4<f32> {{
             let k = vec4<u32>(0u, 17u, 34u, 51u) + {slot}u * 3u;
             let b = u32(p.x) * 7u + u32(p.y) * 13u + s * 29u;
             return vec4<f32>((b + k) & vec4<u32>(63u)) / 64.0;
         }}"
    )
}

/// `--ops`: one instruction alone, compiled from HLSL here, over 4096 inputs (n = the value's
/// index), for telling which instruction a pair's differences come from
const OPS: [(&str, &str, &str); 11] = [
    ("op_exp2_any", "asfloat(((n * 0x9e3779b1u) >> 9) | 0x40000000u) * 4.0 - 12.0", "exp2(x)"),
    ("op_mad", "asfloat(((n * 0x9e3779b1u) >> 9) | 0x3f800000u) - 1.4", "x * x.yzwx + x.wxyz"),
    ("op_dp4", "asfloat(((n * 0x9e3779b1u) >> 9) | 0x3f800000u) - 1.4", "dot(x, x.yzwx).xxxx"),
    ("op_mul_add", "asfloat(((n * 0x9e3779b1u) >> 9) | 0x3f800000u) - 1.4", "x * x.yzwx - x.wxyz * x.zwxy"),
    ("op_exp2", "((float)n - 2048.0) / 128.0", "exp2(x)"),
    ("op_log2", "asfloat(0x38000000u + n * 0x10000u)", "log2(x)"),
    ("op_rsqrt", "asfloat(0x38000000u + n * 0x10000u)", "rsqrt(x)"),
    ("op_sqrt", "asfloat(0x38000000u + n * 0x10000u)", "sqrt(x)"),
    ("op_div", "asfloat(0x38000000u + n * 0x10000u)", "asfloat(0x3f800000u + n * 0x0711u) / x"),
    ("op_sin", "((float)n - 2048.0) / 256.0", "sin(x)"),
    ("op_cos", "((float)n - 2048.0) / 256.0", "cos(x)"),
];

fn op_blobs(name: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let (_, x, f) = OPS.iter().find(|o| o.0 == name)?;
    let vs = "float4 vs(float4 p : POSITION) : SV_Position { return p; }";
    let ps = format!(
        "float4 ps(float4 p : SV_Position) : SV_Target {{
             uint4 n = ((uint)p.x + (uint)p.y * 32) * 4 + uint4(0, 1, 2, 3);
             float4 x = {x};
             return {f};
         }}",
        x = x.replace("n *", "n * uint4(1,1,1,1) *").replace("(float)n", "(float4)n")
    );
    Some((hlsl(vs, "vs", "vs_4_1").unwrap(), hlsl(&ps, "ps", "ps_4_1").unwrap()))
}

/// `--flat` (passed on as PROOF_FLAT to the per-pair processes): textures of one value per mip level
fn flat() -> bool {
    std::env::var_os("PROOF_FLAT").is_some()
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn inputs(name: &str, vs: &Shader, ps: &Shader, t: &rustyac_dxbc::Translated) -> Result<Inputs, String> {
    let mut rng = Rng(name.bytes().fold(0x9e37_79b9_7f4a_7c15u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) | 1);
    // every input the vertex shader reads, as four floats (bone indices and weights for skinning)
    let names: Vec<(&'static str, u32)> = vs
        .inputs
        .iter()
        .filter(|e| e.system_value == 0)
        .map(|e| (&*String::leak(e.name.to_ascii_uppercase()), e.index))
        .collect();
    let elements: Vec<_> = names.iter().enumerate().map(|(k, (n, i))| (*n, *i, DXGI_FORMAT_R32G32B32A32_FLOAT, 16 * k as u32)).collect();
    let vertex_stride = 16 * elements.len().max(1) as u32;
    // one triangle over the whole target
    let corners = [[-1.0f32, -1.0], [3.0, -1.0], [-1.0, 3.0]];
    let mut vertices = Vec::new();
    for c in corners {
        for (n, i) in &names {
            let v = match (*n, *i) {
                ("POSITION", 0) => [c[0], c[1], 0.5, 1.0],
                ("TEXCOORD", 1) if vs.inputs.len() > 5 => [0.0, 1.0, 2.0, 3.0].map(|x: f32| (x + (rng.next() % 4) as f32) % 4.0),
                ("TEXCOORD", 2) if vs.inputs.len() > 5 => [0.25; 4],
                _ => [0; 4].map(|_| rng.unit() * 2.0 - 0.5),
            };
            vertices.extend(f32s(&v));
        }
    }
    if vertices.is_empty() {
        vertices = vec![0; 48];
    }
    let identity = f32s(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let mut cbuffers = BTreeMap::new();
    for c in &t.cbuffers {
        // the reflected size, or what the code reads when that is larger
        let mut size = c.size;
        for s in [vs, ps] {
            for b in &s.reflection.constant_buffers {
                if s.reflection.binding(&b.name).map(|x| x.bind_point) == Some(c.slot) {
                    size = size.max(b.size);
                }
            }
        }
        let mut bytes: Vec<u8> = (0..size / 4).flat_map(|_| rng.unit().to_le_bytes()).collect();
        for s in [vs, ps] {
            for b in &s.reflection.constant_buffers {
                if s.reflection.binding(&b.name).map(|x| x.bind_point) != Some(c.slot) {
                    continue;
                }
                for v in &b.variables {
                    if v.size >= 64 && v.size % 64 == 0 {
                        for k in 0..v.size / 64 {
                            let at = (v.offset + 64 * k) as usize;
                            if at + 64 <= bytes.len() {
                                bytes[at..at + 64].copy_from_slice(&identity);
                            }
                        }
                    }
                }
            }
        }
        cbuffers.insert(c.slot, bytes);
    }
    let mut textures = BTreeMap::new();
    let mut ms = Vec::new();
    for u in &t.textures {
        if u.dimension == 4 {
            if u.depth || u.return_type != 1 && u.return_type != 5 {
                return Err("multisampled depth or integer input".into());
            }
            ms.push(u.slot);
            continue;
        }
        let (cube, levels) = match u.dimension {
            3 => (false, if u.depth { 1 } else { MIPS }),
            6 => (true, if u.depth { 1 } else { MIPS }),
            other => return Err(format!("texture dimension {other}")),
        };
        if u.return_type != 5 && u.return_type != 1 {
            return Err(format!("texture return type {}", u.return_type));
        }
        let faces = if cube { 6 } else { 1 };
        let mut data = Vec::new();
        // one value for every texel of every level: every filter and every level of detail gives that
        // value, so only the shader's own arithmetic is left (colour as half floats in 0.125..1: half
        // to float is exact everywhere, 8-bit unorm to float is not)
        let texel: Vec<u8> = if u.depth { rng.unit().to_le_bytes().to_vec() } else { (0..4).flat_map(|_| (0x3000 + rng.next() % 0x0c00).to_le_bytes()[..2].to_vec()).collect() };
        for level in 0..levels {
            let n = (TEX >> level).max(1);
            let texels = (n * n * faces) as usize;
            data.push(if flat() {
                texel.repeat(texels)
            } else if u.depth {
                (0..texels).flat_map(|_| rng.unit().to_le_bytes()).collect()
            } else {
                (0..texels * 4).map(|_| rng.next() as u8).collect()
            });
        }
        textures.insert(u.slot, (cube, u.depth, data));
    }
    let samplers = t.samplers.iter().map(|s| (s.slot, s.comparison)).collect();
    let mut targets: Vec<u32> = ps.outputs.iter().filter(|o| o.name.eq_ignore_ascii_case("SV_Target")).map(|o| o.register).collect();
    targets.sort();
    targets.dedup();
    let writes_depth = ps.outputs.iter().any(|o| o.name.eq_ignore_ascii_case("SV_Depth"));
    Ok(Inputs { vertex_stride, elements, vertices, cbuffers, textures, samplers, targets, ms, writes_depth })
}

fn check<T>(what: &str, r: windows::core::Result<T>) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {e}"))
}

/// HLSL of the harness's own fill pass, compiled by d3dcompiler_47.
fn hlsl(source: &str, entry: &str, profile: &str) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
    let (entry, profile) = (std::ffi::CString::new(entry).unwrap(), std::ffi::CString::new(profile).unwrap());
    let (mut code, mut errors) = (None, None);
    let r = unsafe { D3DCompile(source.as_ptr().cast(), source.len(), PCSTR::null(), None, None, PCSTR(entry.as_ptr().cast()), PCSTR(profile.as_ptr().cast()), 0, 0, &mut code, Some(&mut errors)) };
    let blob = |b: &ID3DBlob| unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec() };
    if let Err(e) = r {
        return Err(format!("D3DCompile: {e} {}", errors.as_ref().map(|b| String::from_utf8_lossy(&blob(b)).into_owned()).unwrap_or_default()));
    }
    Ok(blob(&code.ok_or("no code")?))
}

/// The original bytecode on Direct3D 11 WARP: one float picture per target (and the depth
/// target's, last, when the pixel shader writes depth).
fn d3d11(vs_blob: &[u8], ps_blob: &[u8], inp: &Inputs) -> Result<Vec<Vec<u8>>, String> {
    unsafe {
        let (mut device, mut context) = (None, None);
        check(
            "D3D11CreateDevice",
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), D3D11_CREATE_DEVICE_SINGLETHREADED, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)),
        )?;
        let device: ID3D11Device = device.ok_or("no device")?;
        let context: ID3D11DeviceContext = context.ok_or("no context")?;
        let mut vs = None;
        check("CreateVertexShader", device.CreateVertexShader(vs_blob, None, Some(&mut vs)))?;
        let mut ps = None;
        check("CreatePixelShader", device.CreatePixelShader(ps_blob, None, Some(&mut ps)))?;
        let names: Vec<std::ffi::CString> = inp.elements.iter().map(|e| std::ffi::CString::new(e.0).unwrap()).collect();
        let descs: Vec<D3D11_INPUT_ELEMENT_DESC> = inp
            .elements
            .iter()
            .zip(&names)
            .map(|(e, n)| D3D11_INPUT_ELEMENT_DESC { SemanticName: PCSTR(n.as_ptr().cast()), SemanticIndex: e.1, Format: e.2, InputSlot: 0, AlignedByteOffset: e.3, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, InstanceDataStepRate: 0 })
            .collect();
        let mut il = None;
        check("CreateInputLayout", device.CreateInputLayout(&descs, vs_blob, Some(&mut il)))?;
        let buffer = |bytes: &[u8], bind: D3D11_BIND_FLAG| -> Result<ID3D11Buffer, String> {
            let desc = D3D11_BUFFER_DESC { ByteWidth: bytes.len() as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: bind.0 as u32, ..Default::default() };
            let init = D3D11_SUBRESOURCE_DATA { pSysMem: bytes.as_ptr() as *const c_void, ..Default::default() };
            let mut b = None;
            check("CreateBuffer", device.CreateBuffer(&desc, Some(&init), Some(&mut b)))?;
            b.ok_or("no buffer".into())
        };
        let vb = buffer(&inp.vertices, D3D11_BIND_VERTEX_BUFFER)?;
        for (slot, bytes) in &inp.cbuffers {
            let cb = buffer(bytes, D3D11_BIND_CONSTANT_BUFFER)?;
            context.VSSetConstantBuffers(*slot, Some(&[Some(cb.clone())]));
            context.PSSetConstantBuffers(*slot, Some(&[Some(cb)]));
        }
        for (slot, (cube, depth, levels)) in &inp.textures {
            let faces = if *cube { 6 } else { 1 };
            let texel = if !*depth && flat() { 8 } else { 4 }; // RGBA16F, or R32_FLOAT and RGBA8 alike
            let desc = D3D11_TEXTURE2D_DESC {
                Width: TEX,
                Height: TEX,
                MipLevels: levels.len() as u32,
                ArraySize: faces,
                Format: if *depth { DXGI_FORMAT_R32_FLOAT } else if flat() { DXGI_FORMAT_R16G16B16A16_FLOAT } else { DXGI_FORMAT_R8G8B8A8_UNORM },
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: if *cube { D3D11_RESOURCE_MISC_TEXTURECUBE.0 as u32 } else { 0 },
            };
            // subresources go face by face, each with all its levels
            let mut init = Vec::new();
            for face in 0..faces as usize {
                for (level, data) in levels.iter().enumerate() {
                    let n = (TEX >> level).max(1) as usize;
                    let face_bytes = n * n * texel;
                    init.push(D3D11_SUBRESOURCE_DATA { pSysMem: data[face * face_bytes..].as_ptr() as *const c_void, SysMemPitch: (n * texel) as u32, SysMemSlicePitch: 0 });
                }
            }
            let mut tex = None;
            check("CreateTexture2D", device.CreateTexture2D(&desc, Some(init.as_ptr()), Some(&mut tex)))?;
            let mut srv = None;
            check("CreateShaderResourceView", device.CreateShaderResourceView(&tex.unwrap(), None, Some(&mut srv)))?;
            context.VSSetShaderResources(*slot, Some(&[srv.clone()]));
            context.PSSetShaderResources(*slot, Some(&[srv]));
        }
        for (slot, comparison) in &inp.samplers {
            let desc = if *comparison {
                D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_LESS,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                }
            } else {
                D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                    AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                    AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                }
            };
            let mut s = None;
            check("CreateSamplerState", device.CreateSamplerState(&desc, Some(&mut s)))?;
            context.VSSetSamplers(*slot, Some(&[s.clone()]));
            context.PSSetSamplers(*slot, Some(&[s]));
        }
        for slot in &inp.ms {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: SIZE,
                Height: SIZE,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: MS_COUNT, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex = None;
            check("CreateTexture2D", device.CreateTexture2D(&desc, None, Some(&mut tex)))?;
            let tex = tex.unwrap();
            let mut rtv = None;
            check("CreateRenderTargetView", device.CreateRenderTargetView(&tex, None, Some(&mut rtv)))?;
            let source = ms_fill_hlsl(*slot);
            let (mut fvs, mut fps) = (None, None);
            check("CreateVertexShader", device.CreateVertexShader(&hlsl(&source, "vs", "vs_4_1")?, None, Some(&mut fvs)))?;
            check("CreatePixelShader", device.CreatePixelShader(&hlsl(&source, "ps", "ps_4_1")?, None, Some(&mut fps)))?;
            let mut rs = None;
            check("CreateRasterizerState", device.CreateRasterizerState(&D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() }, Some(&mut rs)))?;
            context.RSSetState(rs.as_ref());
            context.OMSetRenderTargets(Some(&[rtv]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: SIZE as f32, Height: SIZE as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(fvs.as_ref(), None);
            context.PSSetShader(fps.as_ref(), None);
            context.Draw(3, 0);
            context.OMSetRenderTargets(None, None);
            let mut srv = None;
            check("CreateShaderResourceView", device.CreateShaderResourceView(&tex, None, Some(&mut srv)))?;
            context.VSSetShaderResources(*slot, Some(&[srv.clone()]));
            context.PSSetShaderResources(*slot, Some(&[srv]));
        }
        let mut depth_target = None;
        let mut dsv = None;
        if inp.writes_depth {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: SIZE,
                Height: SIZE,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_D32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            check("CreateTexture2D", device.CreateTexture2D(&desc, None, Some(&mut depth_target)))?;
            check("CreateDepthStencilView", device.CreateDepthStencilView(depth_target.as_ref().unwrap(), None, Some(&mut dsv)))?;
            context.ClearDepthStencilView(dsv.as_ref().unwrap(), D3D11_CLEAR_DEPTH.0, 0.0, 0);
            let mut ds = None;
            let ds_desc = D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_ALWAYS, ..Default::default() };
            check("CreateDepthStencilState", device.CreateDepthStencilState(&ds_desc, Some(&mut ds)))?;
            context.OMSetDepthStencilState(ds.as_ref(), 0);
        }
        let mut targets = Vec::new();
        let mut views = Vec::new();
        let n_targets = inp.targets.iter().max().map_or(0, |m| m + 1);
        for r in 0..n_targets {
            if !inp.targets.contains(&r) {
                views.push(None);
                targets.push(None);
                continue;
            }
            let desc = D3D11_TEXTURE2D_DESC {
                Width: SIZE,
                Height: SIZE,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex = None;
            check("CreateTexture2D", device.CreateTexture2D(&desc, None, Some(&mut tex)))?;
            let tex = tex.unwrap();
            let mut rtv = None;
            check("CreateRenderTargetView", device.CreateRenderTargetView(&tex, None, Some(&mut rtv)))?;
            context.ClearRenderTargetView(rtv.as_ref().unwrap(), &[0.0; 4]);
            views.push(rtv);
            targets.push(Some(tex));
        }
        let mut rs_desc = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
        rs_desc.FrontCounterClockwise = false.into();
        let mut rs = None;
        check("CreateRasterizerState", device.CreateRasterizerState(&rs_desc, Some(&mut rs)))?;
        context.RSSetState(rs.as_ref());
        context.OMSetRenderTargets(Some(&views), dsv.as_ref());
        context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: SIZE as f32, Height: SIZE as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
        context.IASetInputLayout(il.as_ref());
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context.IASetVertexBuffers(0, 1, Some(&Some(vb)), Some(&inp.vertex_stride), Some(&0));
        context.VSSetShader(vs.as_ref(), None);
        context.PSSetShader(ps.as_ref(), None);
        context.Draw(3, 0);
        let mut out = Vec::new();
        for tex in targets.iter().flatten().chain(depth_target.as_ref()) {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            tex.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut staging = None;
            check("CreateTexture2D", device.CreateTexture2D(&desc, None, Some(&mut staging)))?;
            let staging = staging.unwrap();
            context.CopyResource(&staging, tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            check("Map", context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)))?;
            let mut pixels = Vec::new();
            let texel = if desc.Format == DXGI_FORMAT_D32_FLOAT { 4 } else { 16 };
            for row in 0..SIZE as usize {
                let p = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                pixels.extend_from_slice(std::slice::from_raw_parts(p, SIZE as usize * texel));
            }
            context.Unmap(&staging, 0);
            out.push(pixels);
        }
        Ok(out)
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

struct Wgpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
}

/// `warp`: Direct3D 12 WARP; `swiftshader`: Vulkan on a CPU adapter (SwiftShader, picked by
/// VK_DRIVER_FILES); `gpu`: Direct3D 12 on the graphics card.
fn open_wgpu(adapter: &str) -> Result<Wgpu, String> {
    let warp = adapter != "gpu";
    let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
    d.backends = if adapter == "swiftshader" { wgpu::Backends::VULKAN } else { wgpu::Backends::DX12 };
    // WGPU_VALIDATION=1 / WGPU_DEBUG=1 / WGPU_GPU_BASED_VALIDATION=1 turn on the D3D12 debug layer
    d.flags = d.flags.with_env();
    // WGPU_DX12_COMPILER=dxc with dxcompiler.dll on PATH: DXC in place of FXC
    d.backend_options = d.backend_options.with_env();
    let instance = wgpu::Instance::new(d);
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: warp, compatible_surface: None, apply_limit_buckets: false })).map_err(|e| e.to_string())?;
    let name = format!("{} ({:?})", adapter.get_info().name, adapter.get_info().backend);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), ..Default::default() })).map_err(|e| e.to_string())?;
    device.set_device_lost_callback(|reason, msg| eprintln!("wgpu device lost: {reason:?} {msg}"));
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| eprintln!("wgpu error: {e}")));
    Ok(Wgpu { device, queue, name })
}

fn vertex_format(f: DXGI_FORMAT) -> wgpu::VertexFormat {
    match f {
        DXGI_FORMAT_R32G32_FLOAT => wgpu::VertexFormat::Float32x2,
        DXGI_FORMAT_R32G32B32_FLOAT => wgpu::VertexFormat::Float32x3,
        _ => wgpu::VertexFormat::Float32x4,
    }
}

/// The WGSL translation on wgpu: one float picture per target.
fn wgpu_draw(g: &Wgpu, t: &rustyac_dxbc::Translated, inp: &Inputs) -> Result<Vec<Vec<u8>>, String> {
    use wgpu::util::DeviceExt;
    let device = &g.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(t.vs.as_str().into()) });
    let ps = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(t.ps.as_str().into()) });
    let vis = |v: bool, p: bool| match (v, p) {
        (true, true) => wgpu::ShaderStages::VERTEX_FRAGMENT,
        (true, false) => wgpu::ShaderStages::VERTEX,
        _ => wgpu::ShaderStages::FRAGMENT,
    };
    let cb_entries: Vec<_> = t.cbuffers.iter().map(|c| wgpu::BindGroupLayoutEntry { binding: c.slot, visibility: vis(c.vertex, c.pixel), ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }).collect();
    let tex_entries: Vec<_> = t
        .textures
        .iter()
        .map(|u| wgpu::BindGroupLayoutEntry {
            binding: u.slot,
            visibility: vis(u.vertex, u.pixel),
            ty: wgpu::BindingType::Texture {
                sample_type: if u.depth {
                    wgpu::TextureSampleType::Depth
                } else {
                    wgpu::TextureSampleType::Float { filterable: u.dimension != 4 }
                },
                view_dimension: if u.dimension == 6 { wgpu::TextureViewDimension::Cube } else { wgpu::TextureViewDimension::D2 },
                multisampled: u.dimension == 4,
            },
            count: None,
        })
        .collect();
    let sampler_entries: Vec<_> = t
        .samplers
        .iter()
        .map(|s| wgpu::BindGroupLayoutEntry { binding: s.slot, visibility: vis(s.vertex, s.pixel), ty: wgpu::BindingType::Sampler(if s.comparison { wgpu::SamplerBindingType::Comparison } else { wgpu::SamplerBindingType::Filtering }), count: None })
        .collect();
    let layouts: Vec<wgpu::BindGroupLayout> = [&cb_entries, &tex_entries, &sampler_entries].iter().map(|e| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: e })).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layouts[0]), Some(&layouts[1]), Some(&layouts[2])], immediate_size: 0 });

    let buffers: BTreeMap<u32, wgpu::Buffer> = inp.cbuffers.iter().map(|(slot, bytes)| (*slot, device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytes, usage: wgpu::BufferUsages::UNIFORM }))).collect();
    let mut views = BTreeMap::new();
    let mut fill_depth = Vec::new();
    for (slot, (cube, depth, levels)) in &inp.textures {
        let faces = if *cube { 6 } else { 1 };
        let size = wgpu::Extent3d { width: TEX, height: TEX, depth_or_array_layers: faces };
        let depth_format = if std::env::var_os("PROOF_D16").is_some() { wgpu::TextureFormat::Depth16Unorm } else { wgpu::TextureFormat::Depth32Float };
        let format = if *depth { depth_format } else if flat() { wgpu::TextureFormat::Rgba16Float } else { wgpu::TextureFormat::Rgba8Unorm };
        let usage = if *depth && std::env::var_os("PROOF_NO_FILL").is_none() { wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST };
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: levels.len() as u32, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        if *depth {
            if *cube {
                return Err("depth cube".into());
            }
            if std::env::var_os("PROOF_NO_FILL").is_none() {
                fill_depth.push((tex.clone(), levels[0].clone()));
            }
        } else {
            for (level, data) in levels.iter().enumerate() {
                let n = (TEX >> level).max(1);
                g.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * if flat() { 8 } else { 4 }), rows_per_image: Some(n) },
                    wgpu::Extent3d { width: n, height: n, depth_or_array_layers: faces },
                );
            }
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(if *cube { wgpu::TextureViewDimension::Cube } else { wgpu::TextureViewDimension::D2 }), ..Default::default() });
        views.insert(*slot, view);
    }
    let mut fill_ms = Vec::new();
    for slot in &inp.ms {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MS_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        views.insert(*slot, view.clone());
        fill_ms.push((*slot, view));
    }
    let samplers: BTreeMap<u32, wgpu::Sampler> = inp
        .samplers
        .iter()
        .map(|(slot, comparison)| {
            let d = if *comparison {
                let f = if std::env::var_os("PROOF_CMP_NEAREST").is_some() { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear };
                wgpu::SamplerDescriptor { mag_filter: f, min_filter: f, mipmap_filter: wgpu::MipmapFilterMode::Nearest, compare: Some(wgpu::CompareFunction::Less), ..Default::default() }
            } else {
                wgpu::SamplerDescriptor {
                    address_mode_u: wgpu::AddressMode::Repeat,
                    address_mode_v: wgpu::AddressMode::Repeat,
                    address_mode_w: wgpu::AddressMode::Repeat,
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: wgpu::MipmapFilterMode::Linear,
                    ..Default::default()
                }
            };
            (*slot, device.create_sampler(&d))
        })
        .collect();
    let groups: Vec<wgpu::BindGroup> = [
        buffers.iter().map(|(s, b)| wgpu::BindGroupEntry { binding: *s, resource: b.as_entire_binding() }).collect::<Vec<_>>(),
        views.iter().map(|(s, v)| wgpu::BindGroupEntry { binding: *s, resource: wgpu::BindingResource::TextureView(v) }).collect(),
        samplers.iter().map(|(s, v)| wgpu::BindGroupEntry { binding: *s, resource: wgpu::BindingResource::Sampler(v) }).collect(),
    ]
    .iter()
    .zip(&layouts)
    .map(|(entries, layout)| device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries }))
    .collect();

    // the vertex shader's input registers by semantic
    let attributes: Vec<wgpu::VertexAttribute> = inp
        .elements
        .iter()
        .filter_map(|e| {
            let reg = t.vertex_inputs.iter().find(|(n, i, _)| n.eq_ignore_ascii_case(e.0) && *i == e.1)?.2;
            Some(wgpu::VertexAttribute { format: vertex_format(e.2), offset: e.3 as u64, shader_location: reg })
        })
        .collect();
    let n_targets = inp.targets.iter().max().map_or(0, |m| m + 1);
    let color_targets: Vec<Option<wgpu::ColorTargetState>> =
        (0..n_targets).map(|r| inp.targets.contains(&r).then_some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba32Float, blend: None, write_mask: wgpu::ColorWrites::ALL })).collect();
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout { array_stride: inp.vertex_stride as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes })],
        },
        primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, cull_mode: None, ..Default::default() },
        depth_stencil: inp.writes_depth.then(|| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState { module: &ps, entry_point: Some("main"), compilation_options: Default::default(), targets: &color_targets }),
        multiview_mask: None,
        cache: None,
    });
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &inp.vertices, usage: wgpu::BufferUsages::VERTEX });
    let target_textures: Vec<Option<wgpu::Texture>> = (0..n_targets)
        .map(|r| {
            inp.targets.contains(&r).then(|| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            })
        })
        .collect();
    let target_views: Vec<Option<wgpu::TextureView>> = target_textures.iter().map(|t| t.as_ref().map(|t| t.create_view(&Default::default()))).collect();
    let depth_target = inp.writes_depth.then(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    });
    let depth_view = depth_target.as_ref().map(|t| t.create_view(&Default::default()));

    let mut encoder = device.create_command_encoder(&Default::default());
    // depth maps: written by a small pass from an R32Float copy of the same numbers
    for (tex, data) in &fill_depth {
        let src = device.create_texture_with_data(
            &g.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: TEX, height: TEX, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            data,
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var src: texture_2d<f32>;
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                     return vec4<f32>(select(-1.0, 3.0, i == 1u), select(-1.0, 3.0, i == 2u), 0.0, 1.0);
                 }
                 struct D { @builtin(frag_depth) d: f32 }
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> D {
                     return D(textureLoad(src, vec2<i32>(p.xy), 0).x);
                 }"
                .into(),
            ),
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Always), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipe.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.create_view(&Default::default())) }],
        });
        let view = tex.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipe);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    // multisampled inputs: every sample its own numbers, the same pass as on Direct3D 11
    for (slot, view) in &fill_ms {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(ms_fill_wgsl(*slot).into()) });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: MS_COUNT, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipe);
        pass.draw(0..3, 0..1);
    }
    {
        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = target_views
            .iter()
            .map(|v| v.as_ref().map(|view| wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }))
            .collect();
        let depth_stencil_attachment = depth_view.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment { view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &attachments, depth_stencil_attachment, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
        pass.set_pipeline(&pipeline);
        for (i, group) in groups.iter().enumerate() {
            pass.set_bind_group(i as u32, group, &[]);
        }
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..3, 0..1);
    }
    // (the depth target last, as on Direct3D 11; its 128-byte rows padded to 256 for the copy)
    let readbacks: Vec<(wgpu::Buffer, u32, u32)> = target_textures
        .iter()
        .flatten()
        .map(|t| (t, SIZE * 16, SIZE * 16, wgpu::TextureAspect::All))
        .chain(depth_target.iter().map(|t| (t, 256, SIZE * 4, wgpu::TextureAspect::DepthOnly)))
        .map(|(tex, row, used, aspect)| {
            let buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * SIZE) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect },
                wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
                wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            );
            (buf, row, used)
        })
        .collect();
    g.queue.submit([encoder.finish()]);
    if let Some(e) = block_on(scope.pop()) {
        return Err(format!("wgpu: {e}"));
    }
    let mut out = Vec::new();
    for (buf, row, used) in &readbacks {
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let bytes = buf.slice(..).get_mapped_range().map_err(|e| e.to_string())?.to_vec();
        out.push(bytes.chunks_exact(*row as usize).flat_map(|r| r[..*used as usize].to_vec()).collect());
        buf.unmap();
    }
    Ok(out)
}

/// Distance in representable floats (0 for the same bits, and for two NaNs).
fn ulps(a: u32, b: u32) -> u64 {
    let (fa, fb) = (f32::from_bits(a), f32::from_bits(b));
    if fa.is_nan() && fb.is_nan() {
        return 0;
    }
    if fa.is_nan() || fb.is_nan() {
        return u64::MAX;
    }
    let ord = |x: u32| -> i64 {
        if x & 0x8000_0000 != 0 {
            -((x & 0x7fff_ffff) as i64)
        } else {
            x as i64
        }
    };
    (ord(a) - ord(b)).unsigned_abs()
}

/// wgpu's warnings and the D3D12 debug layer's messages on stderr (PROOF_LOG=1)
struct Log;

impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{} {}: {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    if std::env::var_os("PROOF_LOG").is_some() {
        let _ = log::set_logger(&Log);
        log::set_max_level(log::LevelFilter::Warn);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--flat") {
        // SAFETY: set before any thread is started
        unsafe { std::env::set_var("PROOF_FLAT", "1") };
    }
    if args.iter().any(|a| a == "--ops") {
        // SAFETY: as above
        unsafe { std::env::set_var("PROOF_OPS", "1") };
    }
    let adapter = args.windows(2).find(|w| w[0] == "--adapter").map_or("gpu", |w| w[1].as_str()).to_string();
    let only: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--adapter")).map(|(_, a)| a).collect();
    let game = rustyac_content::install::ac_root().expect("an Assetto Corsa install");
    let folder = game.join("system/shaders/win");
    let g = open_wgpu(&adapter).expect("a wgpu adapter");
    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix("_vs.fxo").map(str::to_string)).collect();
    names.sort();
    if std::env::var_os("PROOF_OPS").is_some() {
        names = OPS.iter().map(|o| o.0.to_string()).collect();
    }
    let child = args.iter().any(|a| a == "--row");
    if !child {
        println!("D3D11 WARP against wgpu on {}\n", g.name);
        println!("| shader | pixels drawn | values | same bits | largest difference (ULP) | note |");
        println!("|---|---|---|---|---|---|");
    }
    if only.is_empty() {
        // one process per pair: after one lost device WARP's DX12 adapter is gone for the whole process
        let (mut exact, mut close, mut not_run) = (0, 0, 0);
        for name in &names {
            let out = std::process::Command::new(std::env::current_exe().unwrap()).args(["--adapter", adapter.as_str(), "--row", name]).output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            let row = text.lines().find(|l| l.starts_with(&format!("| {name} |"))).map(str::to_string).unwrap_or(format!("| {name} | | | | | not run: the process failed ({}) |", out.status));
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            match (cells[3], cells[4]) {
                ("", _) => not_run += 1,
                (v, s) if v == s => exact += 1,
                _ => close += 1,
            }
            println!("{row}");
        }
        println!("\n{exact} pairs the same to the bit, {close} with differences, {not_run} not run");
        return;
    }
    let (mut exact, mut close, mut not_run) = (0, 0, 0);
    for name in names.iter().filter(|n| only.is_empty() || only.iter().any(|o| o == n)) {
        let (vs_blob, mut ps_blob) = op_blobs(name).unwrap_or_else(|| (std::fs::read(folder.join(format!("{name}_vs.fxo"))).unwrap(), std::fs::read(folder.join(format!("{name}_ps.fxo"))).unwrap()));
        // PROOF_PS_HLSL: an HLSL pixel shader (entry `ps`, kept in a scratch folder) in place of the pair's own, on both sides
        if let Some(f) = std::env::var_os("PROOF_PS_HLSL") {
            ps_blob = hlsl(&std::fs::read_to_string(f).unwrap(), "ps", "ps_4_1").unwrap();
        }
        let (vs, ps) = (rustyac_dxbc::parse(&vs_blob).unwrap(), rustyac_dxbc::parse(&ps_blob).unwrap());
        let mut t = rustyac_dxbc::translate_pair(&vs, &ps).unwrap();
        // PROOF_PS / PROOF_VS: a WGSL file (kept in a scratch folder) drawn in place of the translation, for narrowing a failure down
        if let Some(f) = std::env::var_os("PROOF_PS") {
            t.ps = std::fs::read_to_string(f).unwrap();
        }
        if let Some(f) = std::env::var_os("PROOF_VS") {
            t.vs = std::fs::read_to_string(f).unwrap();
        }
        if std::env::var_os("PROOF_DUMP").is_some() {
            eprintln!("{}", t.ps);
        }
        let result = inputs(name, &vs, &ps, &t).and_then(|inp| Ok((d3d11(&vs_blob, &ps_blob, &inp)?, wgpu_draw(&g, &t, &inp)?)));
        let (a, b) = match result {
            Ok(x) => x,
            Err(e) => {
                not_run += 1;
                println!("| {name} | | | | | not run: {} |", e.lines().next().unwrap_or("").replace('|', "/"));
                continue;
            }
        };
        if std::env::var_os("PROOF_VALUES").is_some() {
            // PROOF_VALUES=1: the first pixels of both sides, bits of every channel
            for k in 0..8 {
                let px = |p: &[u8]| -> Vec<String> { p[k * 16..k * 16 + 16].chunks_exact(4).map(|c| format!("{:08x}", u32::from_le_bytes(c.try_into().unwrap()))).collect() };
                eprintln!("pixel {k}: {:?} against {:?}", px(&a[0]), px(&b[0]));
            }
        }
        let (mut drawn, mut values, mut same, mut worst) = (0, 0, 0, 0u64);
        for (x, y) in a.iter().zip(&b) {
            let texel = x.len() / (SIZE * SIZE) as usize;
            for (px, py) in x.chunks_exact(texel).zip(y.chunks_exact(texel)) {
                if px.iter().any(|&v| v != 0) {
                    drawn += 1;
                }
                for (cx, cy) in px.chunks_exact(4).zip(py.chunks_exact(4)) {
                    let (u, v) = (u32::from_le_bytes(cx.try_into().unwrap()), u32::from_le_bytes(cy.try_into().unwrap()));
                    values += 1;
                    if u == v {
                        same += 1;
                    }
                    worst = worst.max(ulps(u, v));
                    if u != v && std::env::var_os("PROOF_DIFF").is_some() && values - same < 12 {
                        // PROOF_DIFF=1: the first differing values, for narrowing a difference down
                        eprintln!("value {} (channel {}): {} {:08x} against {} {:08x}", values - 1, (values - 1) % 4, f32::from_bits(u), u, f32::from_bits(v), v);
                    }
                }
            }
        }
        if same == values {
            exact += 1;
        } else {
            close += 1;
        }
        let worst = if worst == u64::MAX { "NaN on one side".to_string() } else { worst.to_string() };
        println!("| {name} | {drawn} | {values} | {same} | {worst} | |");
    }
    if !child {
        println!("\n{exact} pairs the same to the bit, {close} with differences, {not_run} not run");
    }
}
