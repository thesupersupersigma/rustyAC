// SPDX-License-Identifier: GPL-3.0-or-later

//! The command log: every call that reaches a Direct3D 11 device context, written as text with
//! nothing in it that depends on addresses or on the order things were created in.
//!
//! It is what the render oracle compares: the game's own renderer (run from `acs.exe` in the
//! oracle's process) and this crate's port are both logged by this one layer, so "the two logs
//! are the same file" means the two renderers asked Direct3D for exactly the same things in
//! the same order, with the same bytes in every constant buffer.
//!
//! How it works: the renderer is handed a stand-in for the immediate context, an object with a
//! vtable of its own whose methods write the call down and pass it on to the real context.
//! (Patching the real context's vtable does not work: Direct3D 11 swaps entries of it, such as
//! the draw calls, `Map` and the clears, between implementations as the pipeline state changes,
//! which silently removes a hook.) The device's vtable is patched in place for the few
//! `Create…` calls that give objects their names. Calls that matter get a hook that understands
//! their arguments; every other method gets a small machine-code stub that reports the call by
//! name. Objects are named by what they are, never by where they are:
//!
//! * a state object by its whole description,
//! * a shader by a hash of its bytecode, an input layout by a hash of its elements,
//! * a buffer by its size, bind flags and a hash of its current bytes (the bytes themselves are
//!   written out in full whenever a buffer is filled),
//! * a texture by its description and a hash of every mip of its content (read back once, the
//!   first time it is bound), a render target by its description.

use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Memory::{VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};

const CTX_SLOTS: usize = 115;
const DEV_SLOTS: usize = 43;

/// The real immediate context (the stand-in forwards to it).
static REAL_CTX: AtomicUsize = AtomicUsize::new(0);
/// The stand-in handed to the renderer.
static PROXY_CTX: AtomicUsize = AtomicUsize::new(0);
static ORIG_DEV: [AtomicUsize; DEV_SLOTS] = [const { AtomicUsize::new(0) }; DEV_SLOTS];
static INSTALLED: AtomicBool = AtomicBool::new(false);
static CAPTURE_FROM_INSTALL: AtomicBool = AtomicBool::new(false);

/// Start writing calls down as soon as the log is installed (the set-up of the renderer: its
/// state objects, constant buffers, render targets), not only from [`begin_capture`] on.
pub fn capture_from_install(on: bool) {
    CAPTURE_FROM_INSTALL.store(on, Ordering::SeqCst);
}
static STATE: Mutex<Option<State>> = Mutex::new(None);

thread_local! {
    /// Set while a hook is working: what the hook itself asks Direct3D is not logged.
    static INSIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Private data on a Direct3D object: the 64-bit name the log uses for it.
const TAG: GUID = GUID::from_u128(0x7a1c20c0_5d1b_4e8f_9a53_727573747941);

const CTX_NAMES: [&str; CTX_SLOTS] = [
    "QueryInterface", "AddRef", "Release", "GetDevice", "GetPrivateData", "SetPrivateData", "SetPrivateDataInterface",
    "VSSetConstantBuffers", "PSSetShaderResources", "PSSetShader", "PSSetSamplers", "VSSetShader", "DrawIndexed", "Draw",
    "Map", "Unmap", "PSSetConstantBuffers", "IASetInputLayout", "IASetVertexBuffers", "IASetIndexBuffer",
    "DrawIndexedInstanced", "DrawInstanced", "GSSetConstantBuffers", "GSSetShader", "IASetPrimitiveTopology",
    "VSSetShaderResources", "VSSetSamplers", "Begin", "End", "GetData", "SetPredication", "GSSetShaderResources",
    "GSSetSamplers", "OMSetRenderTargets", "OMSetRenderTargetsAndUnorderedAccessViews", "OMSetBlendState",
    "OMSetDepthStencilState", "SOSetTargets", "DrawAuto", "DrawIndexedInstancedIndirect", "DrawInstancedIndirect",
    "Dispatch", "DispatchIndirect", "RSSetState", "RSSetViewports", "RSSetScissorRects", "CopySubresourceRegion",
    "CopyResource", "UpdateSubresource", "CopyStructureCount", "ClearRenderTargetView", "ClearUnorderedAccessViewUint",
    "ClearUnorderedAccessViewFloat", "ClearDepthStencilView", "GenerateMips", "SetResourceMinLOD", "GetResourceMinLOD",
    "ResolveSubresource", "ExecuteCommandList", "HSSetShaderResources", "HSSetShader", "HSSetSamplers",
    "HSSetConstantBuffers", "DSSetShaderResources", "DSSetShader", "DSSetSamplers", "DSSetConstantBuffers",
    "CSSetShaderResources", "CSSetUnorderedAccessViews", "CSSetShader", "CSSetSamplers", "CSSetConstantBuffers",
    "VSGetConstantBuffers", "PSGetShaderResources", "PSGetShader", "PSGetSamplers", "VSGetShader", "PSGetConstantBuffers",
    "IAGetInputLayout", "IAGetVertexBuffers", "IAGetIndexBuffer", "GSGetConstantBuffers", "GSGetShader",
    "IAGetPrimitiveTopology", "VSGetShaderResources", "VSGetSamplers", "GetPredication", "GSGetShaderResources",
    "GSGetSamplers", "OMGetRenderTargets", "OMGetRenderTargetsAndUnorderedAccessViews", "OMGetBlendState",
    "OMGetDepthStencilState", "SOGetTargets", "RSGetState", "RSGetViewports", "RSGetScissorRects", "HSGetShaderResources",
    "HSGetShader", "HSGetSamplers", "HSGetConstantBuffers", "DSGetShaderResources", "DSGetShader", "DSGetSamplers",
    "DSGetConstantBuffers", "CSGetShaderResources", "CSGetUnorderedAccessViews", "CSGetShader", "CSGetSamplers",
    "CSGetConstantBuffers", "ClearState", "Flush", "GetType", "GetContextFlags", "FinishCommandList",
];

const DEV_NAMES: [&str; DEV_SLOTS] = [
    "QueryInterface", "AddRef", "Release", "CreateBuffer", "CreateTexture1D", "CreateTexture2D", "CreateTexture3D",
    "CreateShaderResourceView", "CreateUnorderedAccessView", "CreateRenderTargetView", "CreateDepthStencilView",
    "CreateInputLayout", "CreateVertexShader", "CreateGeometryShader", "CreateGeometryShaderWithStreamOutput",
    "CreatePixelShader", "CreateHullShader", "CreateDomainShader", "CreateComputeShader", "CreateClassLinkage",
    "CreateBlendState", "CreateDepthStencilState", "CreateRasterizerState", "CreateSamplerState", "CreateQuery",
    "CreatePredicate", "CreateCounter", "CreateDeferredContext", "OpenSharedResource", "CheckFormatSupport",
    "CheckMultisampleQualityLevels", "CheckCounterInfo", "CheckCounter", "CheckFeatureSupport", "GetPrivateData",
    "SetPrivateData", "SetPrivateDataInterface", "GetFeatureLevel", "GetCreationFlags", "GetDeviceRemovedReason",
    "GetImmediateContext", "SetExceptionMode", "GetExceptionMode",
];

/// FNV-1a, 64 bit.
#[derive(Clone, Copy)]
pub struct Fnv(pub u64);

impl Fnv {
    pub fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    pub fn bytes(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for b in bytes {
            h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = h;
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
}

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv::new()
    }
}

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = Fnv::new();
    h.bytes(bytes);
    h.0
}

struct BufInfo {
    size: u32,
    bind: u32,
    usage: i32,
    /// kept for buffers that can be rewritten; a buffer that cannot keeps only its hash
    bytes: Option<Vec<u8>>,
    hash: u64,
}

struct State {
    out: Vec<u8>,
    capturing: bool,
    buffers: HashMap<usize, BufInfo>,
    /// (resource, subresource) -> (pointer, kind of map) of a mapped buffer
    mapped: HashMap<(usize, u32), (usize, u32)>,
    device: usize,
    context: usize,
    lines: u64,
    draws: u64,
}

struct Guard(std::sync::MutexGuard<'static, Option<State>>);

impl Drop for Guard {
    fn drop(&mut self) {
        INSIDE.with(|c| c.set(false));
    }
}

impl std::ops::Deref for Guard {
    type Target = State;
    fn deref(&self) -> &State {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut State {
        self.0.as_mut().unwrap()
    }
}

/// The logger's state, unless this call was made by a hook itself.
fn enter() -> Option<Guard> {
    if INSIDE.with(|c| c.get()) {
        return None;
    }
    let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref()?;
    INSIDE.with(|c| c.set(true));
    Some(Guard(guard))
}

/// The real context.
fn real() -> P {
    REAL_CTX.load(Ordering::Relaxed) as P
}

/// A method of the real context, as its vtable has it right now.
fn orig_ctx<T: Copy>(slot: usize) -> T {
    debug_assert!(std::mem::size_of::<T>() == std::mem::size_of::<usize>());
    unsafe {
        let vtable = *(real() as *const *const usize);
        let p = *vtable.add(slot);
        std::mem::transmute_copy(&p)
    }
}

fn orig_dev<T: Copy>(slot: usize) -> T {
    let p = ORIG_DEV[slot].load(Ordering::Relaxed);
    debug_assert!(p != 0 && std::mem::size_of::<T>() == std::mem::size_of::<usize>());
    unsafe { std::mem::transmute_copy(&p) }
}

// --- names of things ---------------------------------------------------------------------------

unsafe fn tag_of(child: *mut c_void) -> Option<u64> {
    let child = ID3D11DeviceChild::from_raw_borrowed(&child)?;
    let mut value = 0u64;
    let mut size = 8u32;
    child.GetPrivateData(&TAG, &mut size, Some(&mut value as *mut u64 as *mut c_void)).ok()?;
    (size == 8).then_some(value)
}

unsafe fn set_tag(child: *mut c_void, value: u64) {
    if let Some(child) = ID3D11DeviceChild::from_raw_borrowed(&child) {
        let _ = child.SetPrivateData(&TAG, 8, Some(&value as *const u64 as *const c_void));
    }
}

/// (block width in pixels, bytes per block) of a format whose content the log can hash.
fn format_layout(format: DXGI_FORMAT) -> Option<(u32, u32)> {
    Some(match format.0 {
        2..=4 => (1, 16),     // R32G32B32A32
        9..=14 => (1, 8),     // R16G16B16A16
        15..=18 => (1, 8),    // R32G32
        23..=26 => (1, 4),    // R10G10B10A2, R11G11B10
        27..=32 => (1, 4),    // R8G8B8A8
        33..=38 => (1, 4),    // R16G16
        39..=43 => (1, 4),    // R32
        48..=52 => (1, 2),    // R8G8
        53..=59 => (1, 2),    // R16
        60..=65 => (1, 1),    // R8, A8
        70..=72 => (4, 8),    // BC1
        73..=75 => (4, 16),   // BC2
        76..=78 => (4, 16),   // BC3
        79..=81 => (4, 8),    // BC4
        82..=84 => (4, 16),   // BC5
        85 | 86 => (1, 2),    // B5G6R5, B5G5R5A1
        87..=93 => (1, 4),    // B8G8R8A8, B8G8R8X8
        94..=99 => (4, 16),   // BC6H, BC7
        115 => (1, 2),        // B4G4R4A4
        _ => return None,
    })
}

/// A hash of every mip of every slice of a texture, read back through a staging copy.
unsafe fn read_texture_hash(device: &ID3D11Device, context: &ID3D11DeviceContext, texture: &ID3D11Texture2D) -> Option<u64> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut desc);
    let (block, bytes_per_block) = format_layout(desc.Format)?;
    if desc.SampleDesc.Count != 1 {
        return None;
    }
    let mut staging_desc = desc;
    staging_desc.Usage = D3D11_USAGE_STAGING;
    staging_desc.BindFlags = 0;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    staging_desc.MiscFlags = 0;
    let mut staging = None;
    device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).ok()?;
    let staging = staging?;
    context.CopyResource(&staging, texture);
    let mut hash = Fnv::new();
    for slice in 0..desc.ArraySize {
        for mip in 0..desc.MipLevels {
            let sub = mip + slice * desc.MipLevels;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging, sub, D3D11_MAP_READ, 0, Some(&mut mapped)).ok()?;
            let width = (desc.Width >> mip).max(1);
            let height = (desc.Height >> mip).max(1);
            let row_bytes = (width.div_ceil(block) * bytes_per_block) as usize;
            let rows = height.div_ceil(block) as usize;
            hash.u32(sub);
            for row in 0..rows {
                let p = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                hash.bytes(std::slice::from_raw_parts(p, row_bytes));
            }
            context.Unmap(&staging, sub);
        }
    }
    Some(hash.0)
}

impl State {
    fn line(&mut self, text: std::fmt::Arguments) {
        if self.capturing {
            let _ = std::io::Write::write_fmt(&mut self.out, text);
            self.out.push(b'\n');
            self.lines += 1;
        }
    }

    unsafe fn device(&self) -> Option<ID3D11Device> {
        let p = self.device as *mut c_void;
        ID3D11Device::from_raw_borrowed(&p).cloned()
    }

    unsafe fn context(&self) -> Option<ID3D11DeviceContext> {
        let p = self.context as *mut c_void;
        ID3D11DeviceContext::from_raw_borrowed(&p).cloned()
    }

    fn buffer_name(&self, buffer: *mut c_void) -> String {
        if buffer.is_null() {
            return "-".into();
        }
        match self.buffers.get(&(buffer as usize)) {
            Some(info) => format!("B{}:{:x}:{:x}:{:016x}", info.size, info.bind, info.usage, info.hash),
            None => unsafe {
                let mut desc = D3D11_BUFFER_DESC::default();
                if let Some(b) = ID3D11Buffer::from_raw_borrowed(&buffer) {
                    b.GetDesc(&mut desc);
                }
                format!("B{}:{:x}:{:x}:unseen", desc.ByteWidth, desc.BindFlags, desc.Usage.0)
            },
        }
    }

    /// A resource by what it is: a buffer, or a texture with its description and (when it is
    /// not something the frame draws into) its content.
    unsafe fn resource_name(&mut self, resource: *mut c_void) -> String {
        if resource.is_null() {
            return "-".into();
        }
        let Some(res) = ID3D11Resource::from_raw_borrowed(&resource) else {
            return "?".into();
        };
        let dimension = res.GetType();
        if dimension == D3D11_RESOURCE_DIMENSION_BUFFER {
            // the same object under its ID3D11Buffer pointer (the interfaces share an address)
            return self.buffer_name(resource);
        }
        if dimension != D3D11_RESOURCE_DIMENSION_TEXTURE2D {
            return format!("resource-kind-{}", dimension.0);
        }
        let Ok(texture) = res.cast::<ID3D11Texture2D>() else {
            return "?".into();
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        let head = format!(
            "{}x{}a{}m{}f{}s{}.{}u{}b{:x}c{:x}x{:x}",
            desc.Width,
            desc.Height,
            desc.ArraySize,
            desc.MipLevels,
            desc.Format.0,
            desc.SampleDesc.Count,
            desc.SampleDesc.Quality,
            desc.Usage.0,
            desc.BindFlags,
            desc.CPUAccessFlags,
            desc.MiscFlags
        );
        let drawn_into = desc.BindFlags & (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_DEPTH_STENCIL.0) as u32 != 0;
        if drawn_into {
            return format!("RT[{head}]");
        }
        let content = match tag_of(resource) {
            Some(tag) => Some(tag),
            None => {
                let hash = match (self.device(), self.context()) {
                    (Some(device), Some(context)) => read_texture_hash(&device, &context, &texture),
                    _ => None,
                };
                if let Some(hash) = hash {
                    set_tag(resource, hash);
                }
                hash
            }
        };
        match content {
            Some(hash) => format!("T[{head}:{hash:016x}]"),
            None => format!("T[{head}:unhashed]"),
        }
    }

    unsafe fn srv_name(&mut self, view: *mut c_void) -> String {
        if view.is_null() {
            return "-".into();
        }
        let Some(v) = ID3D11ShaderResourceView::from_raw_borrowed(&view) else {
            return "?".into();
        };
        let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        v.GetDesc(&mut desc);
        let name = match v.GetResource() {
            Ok(r) => self.resource_name(r.as_raw()),
            Err(_) => "-".into(),
        };
        let detail = desc.Anonymous.Texture2DArray;
        let fields = view_fields(0, desc.ViewDimension.0, &[detail.MostDetailedMip, detail.MipLevels, detail.FirstArraySlice, detail.ArraySize]);
        format!("srv(f{} d{}{fields} {name})", desc.Format.0, desc.ViewDimension.0)
    }

    unsafe fn rtv_name(&mut self, view: *mut c_void) -> String {
        if view.is_null() {
            return "-".into();
        }
        let Some(v) = ID3D11RenderTargetView::from_raw_borrowed(&view) else {
            return "?".into();
        };
        let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
        v.GetDesc(&mut desc);
        let name = match v.GetResource() {
            Ok(r) => self.resource_name(r.as_raw()),
            Err(_) => "-".into(),
        };
        let detail = desc.Anonymous.Texture2DArray;
        let fields = view_fields(1, desc.ViewDimension.0, &[detail.MipSlice, detail.FirstArraySlice, detail.ArraySize]);
        format!("rtv(f{} d{}{fields} {name})", desc.Format.0, desc.ViewDimension.0)
    }

    unsafe fn dsv_name(&mut self, view: *mut c_void) -> String {
        if view.is_null() {
            return "-".into();
        }
        let Some(v) = ID3D11DepthStencilView::from_raw_borrowed(&view) else {
            return "?".into();
        };
        let mut desc = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
        v.GetDesc(&mut desc);
        let name = match v.GetResource() {
            Ok(r) => self.resource_name(r.as_raw()),
            Err(_) => "-".into(),
        };
        let detail = desc.Anonymous.Texture2DArray;
        let fields = view_fields(2, desc.ViewDimension.0, &[detail.MipSlice, detail.FirstArraySlice, detail.ArraySize]);
        format!("dsv(f{} d{} g{:x}{fields} {name})", desc.Format.0, desc.ViewDimension.0, desc.Flags)
    }

    /// The constant buffers a draw call sees, with the hash of what is in each right now.
    unsafe fn draw_buffers(&mut self) -> String {
        let Some(context) = self.context() else {
            return String::new();
        };
        let mut text = String::new();
        let mut vs: [Option<ID3D11Buffer>; 14] = Default::default();
        context.VSGetConstantBuffers(0, Some(&mut vs));
        let mut ps: [Option<ID3D11Buffer>; 14] = Default::default();
        context.PSGetConstantBuffers(0, Some(&mut ps));
        for (stage, list) in [("vs", &vs), ("ps", &ps)] {
            for (slot, buffer) in list.iter().enumerate() {
                if let Some(buffer) = buffer {
                    let _ = write!(text, " {stage}{slot}={}", self.buffer_name(buffer.as_raw()));
                }
            }
        }
        text
    }
}

/// The fields of a view description that mean something for its dimension: the rest of the
/// union is whatever was in that memory, in the caller's description and in `GetDesc`'s.
/// `kind`: 0 shader resource view, 1 render target view, 2 depth-stencil view.
fn view_fields(kind: u32, dimension: i32, union: &[u32]) -> String {
    let used = match (kind, dimension) {
        // SRV: TEXTURE2D, TEXTURECUBE: most detailed mip, mip levels; TEXTURE2DARRAY: + first slice, size
        (0, 4) | (0, 9) => 2,
        (0, 5) => 4,
        // RTV: TEXTURE2D: mip slice; TEXTURE2DARRAY: + first slice, size
        (1, 4) => 1,
        (1, 5) => 3,
        // DSV: TEXTURE2D: mip slice; TEXTURE2DARRAY: + first slice, size
        (2, 3) => 1,
        (2, 4) => 3,
        _ => 0,
    };
    let mut text = String::new();
    for v in &union[..used.min(union.len())] {
        let _ = write!(text, " {v}");
    }
    text
}

fn sampler_name(sampler: *mut c_void) -> String {
    if sampler.is_null() {
        return "-".into();
    }
    unsafe {
        let Some(s) = ID3D11SamplerState::from_raw_borrowed(&sampler) else {
            return "?".into();
        };
        let mut d = D3D11_SAMPLER_DESC::default();
        s.GetDesc(&mut d);
        format!(
            "samp({} {} {} {} {:08x} {} {} {:08x},{:08x},{:08x},{:08x} {:08x} {:08x})",
            d.Filter.0,
            d.AddressU.0,
            d.AddressV.0,
            d.AddressW.0,
            d.MipLODBias.to_bits(),
            d.MaxAnisotropy,
            d.ComparisonFunc.0,
            d.BorderColor[0].to_bits(),
            d.BorderColor[1].to_bits(),
            d.BorderColor[2].to_bits(),
            d.BorderColor[3].to_bits(),
            d.MinLOD.to_bits(),
            d.MaxLOD.to_bits()
        )
    }
}

fn blend_name(state: *mut c_void) -> String {
    if state.is_null() {
        return "-".into();
    }
    unsafe {
        let Some(s) = ID3D11BlendState::from_raw_borrowed(&state) else {
            return "?".into();
        };
        let mut d = D3D11_BLEND_DESC::default();
        s.GetDesc(&mut d);
        let mut text = format!("blend(a2c{} ind{}", d.AlphaToCoverageEnable.0, d.IndependentBlendEnable.0);
        for t in &d.RenderTarget {
            let _ = write!(
                text,
                " [{} {} {} {} {} {} {} {:x}]",
                t.BlendEnable.0, t.SrcBlend.0, t.DestBlend.0, t.BlendOp.0, t.SrcBlendAlpha.0, t.DestBlendAlpha.0, t.BlendOpAlpha.0, t.RenderTargetWriteMask
            );
        }
        text.push(')');
        text
    }
}

fn depth_name(state: *mut c_void) -> String {
    if state.is_null() {
        return "-".into();
    }
    unsafe {
        let Some(s) = ID3D11DepthStencilState::from_raw_borrowed(&state) else {
            return "?".into();
        };
        let mut d = D3D11_DEPTH_STENCIL_DESC::default();
        s.GetDesc(&mut d);
        format!(
            "depth({} {} {} st{} {:x} {:x} f[{} {} {} {}] b[{} {} {} {}])",
            d.DepthEnable.0,
            d.DepthWriteMask.0,
            d.DepthFunc.0,
            d.StencilEnable.0,
            d.StencilReadMask,
            d.StencilWriteMask,
            d.FrontFace.StencilFailOp.0,
            d.FrontFace.StencilDepthFailOp.0,
            d.FrontFace.StencilPassOp.0,
            d.FrontFace.StencilFunc.0,
            d.BackFace.StencilFailOp.0,
            d.BackFace.StencilDepthFailOp.0,
            d.BackFace.StencilPassOp.0,
            d.BackFace.StencilFunc.0
        )
    }
}

fn raster_name(state: *mut c_void) -> String {
    if state.is_null() {
        return "-".into();
    }
    unsafe {
        let Some(s) = ID3D11RasterizerState::from_raw_borrowed(&state) else {
            return "?".into();
        };
        let mut d = D3D11_RASTERIZER_DESC::default();
        s.GetDesc(&mut d);
        format!(
            "raster({} {} ccw{} {} {:08x} {:08x} clip{} sc{} ms{} aa{})",
            d.FillMode.0,
            d.CullMode.0,
            d.FrontCounterClockwise.0,
            d.DepthBias,
            d.DepthBiasClamp.to_bits(),
            d.SlopeScaledDepthBias.to_bits(),
            d.DepthClipEnable.0,
            d.ScissorEnable.0,
            d.MultisampleEnable.0,
            d.AntialiasedLineEnable.0
        )
    }
}

fn tagged(prefix: &str, object: *mut c_void) -> String {
    if object.is_null() {
        return "-".into();
    }
    match unsafe { tag_of(object) } {
        Some(tag) => format!("{prefix}:{tag:016x}"),
        None => format!("{prefix}:unseen"),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(text, "{b:02x}");
    }
    text
}

unsafe fn pointer_list(list: *const *mut c_void, count: u32) -> Vec<*mut c_void> {
    if list.is_null() {
        return vec![std::ptr::null_mut(); count as usize];
    }
    std::slice::from_raw_parts(list, count as usize).to_vec()
}

// --- hooks: the device -------------------------------------------------------------------------

type P = *mut c_void;

unsafe extern "system" fn dev_create_buffer(this: P, desc: *const D3D11_BUFFER_DESC, init: *const D3D11_SUBRESOURCE_DATA, out: *mut P) -> i32 {
    let orig: unsafe extern "system" fn(P, *const D3D11_BUFFER_DESC, *const D3D11_SUBRESOURCE_DATA, *mut P) -> i32 = orig_dev(3);
    let result = orig(this, desc, init, out);
    if result >= 0 && !out.is_null() && !(*out).is_null() && !desc.is_null() {
        if let Some(mut state) = enter() {
            let d = *desc;
            let size = d.ByteWidth as usize;
            let rewritable = d.Usage != D3D11_USAGE_IMMUTABLE;
            let initial = if !init.is_null() && !(*init).pSysMem.is_null() {
                Some(std::slice::from_raw_parts((*init).pSysMem as *const u8, size))
            } else {
                None
            };
            let hash = initial.map(hash_bytes).unwrap_or(0);
            let bytes = rewritable.then(|| initial.map(|b| b.to_vec()).unwrap_or_else(|| vec![0; size]));
            state.buffers.insert(*out as usize, BufInfo { size: d.ByteWidth, bind: d.BindFlags, usage: d.Usage.0, bytes, hash });
            let name = state.buffer_name(*out);
            state.line(format_args!("dev.CreateBuffer {name}"));
        }
    }
    result
}

unsafe extern "system" fn dev_create_input_layout(this: P, elements: *const D3D11_INPUT_ELEMENT_DESC, count: u32, code: *const c_void, len: usize, out: *mut P) -> i32 {
    let orig: unsafe extern "system" fn(P, *const D3D11_INPUT_ELEMENT_DESC, u32, *const c_void, usize, *mut P) -> i32 = orig_dev(11);
    let result = orig(this, elements, count, code, len, out);
    if result >= 0 && !out.is_null() && !(*out).is_null() {
        if let Some(mut state) = enter() {
            let mut hash = Fnv::new();
            for e in std::slice::from_raw_parts(elements, count as usize) {
                hash.bytes(std::ffi::CStr::from_ptr(e.SemanticName.0 as *const i8).to_bytes());
                for v in [e.SemanticIndex, e.Format.0 as u32, e.InputSlot, e.AlignedByteOffset, e.InputSlotClass.0 as u32, e.InstanceDataStepRate] {
                    hash.u32(v);
                }
            }
            set_tag(*out, hash.0);
            state.line(format_args!("dev.CreateInputLayout {}", tagged("layout", *out)));
        }
    }
    result
}

unsafe extern "system" fn dev_create_vertex_shader(this: P, code: *const c_void, len: usize, linkage: P, out: *mut P) -> i32 {
    let orig: unsafe extern "system" fn(P, *const c_void, usize, P, *mut P) -> i32 = orig_dev(12);
    let result = orig(this, code, len, linkage, out);
    if result >= 0 && !out.is_null() && !(*out).is_null() {
        if let Some(mut state) = enter() {
            set_tag(*out, hash_bytes(std::slice::from_raw_parts(code as *const u8, len)));
            state.line(format_args!("dev.CreateVertexShader {}", tagged("vs", *out)));
        }
    }
    result
}

unsafe extern "system" fn dev_create_pixel_shader(this: P, code: *const c_void, len: usize, linkage: P, out: *mut P) -> i32 {
    let orig: unsafe extern "system" fn(P, *const c_void, usize, P, *mut P) -> i32 = orig_dev(15);
    let result = orig(this, code, len, linkage, out);
    if result >= 0 && !out.is_null() && !(*out).is_null() {
        if let Some(mut state) = enter() {
            set_tag(*out, hash_bytes(std::slice::from_raw_parts(code as *const u8, len)));
            state.line(format_args!("dev.CreatePixelShader {}", tagged("ps", *out)));
        }
    }
    result
}

/// `Create…State`: the description exactly as the renderer passed it (what `GetDesc` gives back
/// later is Direct3D's tidied-up copy, and equal descriptions share one object).
macro_rules! create_state_hook {
    ($name:ident, $slot:expr, $label:expr, $size:expr) => {
        unsafe extern "system" fn $name(this: P, desc: *const u8, out: *mut P) -> i32 {
            let orig: unsafe extern "system" fn(P, *const u8, *mut P) -> i32 = orig_dev($slot);
            if !desc.is_null() {
                if let Some(mut state) = enter() {
                    let bytes = std::slice::from_raw_parts(desc, $size);
                    state.line(format_args!("{} {}", $label, hex(bytes)));
                }
            }
            orig(this, desc, out)
        }
    };
}

create_state_hook!(dev_create_blend_state, 20, "dev.CreateBlendState", 0x108);
create_state_hook!(dev_create_depth_stencil_state, 21, "dev.CreateDepthStencilState", 0x34);
create_state_hook!(dev_create_rasterizer_state, 22, "dev.CreateRasterizerState", 0x28);
create_state_hook!(dev_create_sampler_state, 23, "dev.CreateSamplerState", 0x34);

unsafe extern "system" fn dev_create_texture_2d(this: P, desc: *const D3D11_TEXTURE2D_DESC, init: *const D3D11_SUBRESOURCE_DATA, out: *mut P) -> i32 {
    let orig: unsafe extern "system" fn(P, *const D3D11_TEXTURE2D_DESC, *const D3D11_SUBRESOURCE_DATA, *mut P) -> i32 = orig_dev(5);
    if !desc.is_null() {
        if let Some(mut state) = enter() {
            let bytes = std::slice::from_raw_parts(desc as *const u8, std::mem::size_of::<D3D11_TEXTURE2D_DESC>());
            state.line(format_args!("dev.CreateTexture2D {} {}", hex(bytes), if init.is_null() { "empty" } else { "filled" }));
        }
    }
    orig(this, desc, init, out)
}

/// A view: its description as passed (or `null`) and what it is a view of.
macro_rules! create_view_hook {
    ($name:ident, $slot:expr, $label:expr, $kind:expr) => {
        unsafe extern "system" fn $name(this: P, resource: P, desc: *const u32, out: *mut P) -> i32 {
            let orig: unsafe extern "system" fn(P, P, *const u32, *mut P) -> i32 = orig_dev($slot);
            if let Some(mut state) = enter() {
                if state.capturing {
                    let text = if desc.is_null() {
                        "null".to_string()
                    } else if $kind == 2 {
                        // format, dimension, flags, then the union
                        let d = std::slice::from_raw_parts(desc, 6);
                        format!("f{} d{} g{:x}{}", d[0], d[1], d[2], view_fields(2, d[1] as i32, &d[3..6]))
                    } else {
                        // format, dimension, then the union
                        let d = std::slice::from_raw_parts(desc, if $kind == 0 { 6 } else { 5 });
                        format!("f{} d{}{}", d[0], d[1], view_fields($kind, d[1] as i32, &d[2..]))
                    };
                    let name = state.resource_name(resource);
                    state.line(format_args!("{} {text} {name}", $label));
                }
            }
            orig(this, resource, desc, out)
        }
    };
}

create_view_hook!(dev_create_shader_resource_view, 7, "dev.CreateShaderResourceView", 0);
create_view_hook!(dev_create_render_target_view, 9, "dev.CreateRenderTargetView", 1);
create_view_hook!(dev_create_depth_stencil_view, 10, "dev.CreateDepthStencilView", 2);

// --- hooks: the context ------------------------------------------------------------------------

macro_rules! set_buffers_hook {
    ($name:ident, $slot:expr, $label:expr) => {
        unsafe extern "system" fn $name(this: P, start: u32, count: u32, list: *const P) {
            let orig: unsafe extern "system" fn(P, u32, u32, *const P) = orig_ctx($slot);
            let this = { let _ = this; real() };
            if let Some(mut state) = enter() {
                if state.capturing {
                    let names: Vec<String> = pointer_list(list, count).into_iter().map(|b| state.buffer_name(b)).collect();
                    state.line(format_args!("{} {start} {count} {}", $label, names.join(" ")));
                }
            }
            orig(this, start, count, list)
        }
    };
}

macro_rules! set_views_hook {
    ($name:ident, $slot:expr, $label:expr) => {
        unsafe extern "system" fn $name(this: P, start: u32, count: u32, list: *const P) {
            let orig: unsafe extern "system" fn(P, u32, u32, *const P) = orig_ctx($slot);
            let this = { let _ = this; real() };
            if let Some(mut state) = enter() {
                if state.capturing {
                    let mut names = Vec::new();
                    for view in pointer_list(list, count) {
                        names.push(state.srv_name(view));
                    }
                    state.line(format_args!("{} {start} {count} {}", $label, names.join(" ")));
                }
            }
            orig(this, start, count, list)
        }
    };
}

macro_rules! set_samplers_hook {
    ($name:ident, $slot:expr, $label:expr) => {
        unsafe extern "system" fn $name(this: P, start: u32, count: u32, list: *const P) {
            let orig: unsafe extern "system" fn(P, u32, u32, *const P) = orig_ctx($slot);
            let this = { let _ = this; real() };
            if let Some(mut state) = enter() {
                if state.capturing {
                    let names: Vec<String> = pointer_list(list, count).into_iter().map(sampler_name).collect();
                    state.line(format_args!("{} {start} {count} {}", $label, names.join(" ")));
                }
            }
            orig(this, start, count, list)
        }
    };
}

macro_rules! set_shader_hook {
    ($name:ident, $slot:expr, $label:expr, $prefix:expr) => {
        unsafe extern "system" fn $name(this: P, shader: P, instances: *const P, count: u32) {
            let orig: unsafe extern "system" fn(P, P, *const P, u32) = orig_ctx($slot);
            let this = { let _ = this; real() };
            if let Some(mut state) = enter() {
                state.line(format_args!("{} {} {count}", $label, tagged($prefix, shader)));
            }
            orig(this, shader, instances, count)
        }
    };
}

set_buffers_hook!(ctx_vs_set_constant_buffers, 7, "VSSetConstantBuffers");
set_buffers_hook!(ctx_ps_set_constant_buffers, 16, "PSSetConstantBuffers");
set_buffers_hook!(ctx_gs_set_constant_buffers, 22, "GSSetConstantBuffers");
set_views_hook!(ctx_ps_set_shader_resources, 8, "PSSetShaderResources");
set_views_hook!(ctx_vs_set_shader_resources, 25, "VSSetShaderResources");
set_samplers_hook!(ctx_ps_set_samplers, 10, "PSSetSamplers");
set_samplers_hook!(ctx_vs_set_samplers, 26, "VSSetSamplers");
set_shader_hook!(ctx_ps_set_shader, 9, "PSSetShader", "ps");
set_shader_hook!(ctx_vs_set_shader, 11, "VSSetShader", "vs");

unsafe extern "system" fn ctx_draw_indexed(this: P, count: u32, start: u32, base: i32) {
    let orig: unsafe extern "system" fn(P, u32, u32, i32) = orig_ctx(12);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            state.draws += 1;
            let buffers = state.draw_buffers();
            state.line(format_args!("DrawIndexed {count} {start} {base}{buffers}"));
        }
    }
    orig(this, count, start, base)
}

unsafe extern "system" fn ctx_draw(this: P, count: u32, start: u32) {
    let orig: unsafe extern "system" fn(P, u32, u32) = orig_ctx(13);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            state.draws += 1;
            let buffers = state.draw_buffers();
            state.line(format_args!("Draw {count} {start}{buffers}"));
        }
    }
    orig(this, count, start)
}

unsafe extern "system" fn ctx_draw_indexed_instanced(this: P, count: u32, instances: u32, start: u32, base: i32, first: u32) {
    let orig: unsafe extern "system" fn(P, u32, u32, u32, i32, u32) = orig_ctx(20);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            state.draws += 1;
            let buffers = state.draw_buffers();
            state.line(format_args!("DrawIndexedInstanced {count} {instances} {start} {base} {first}{buffers}"));
        }
    }
    orig(this, count, instances, start, base, first)
}

unsafe extern "system" fn ctx_draw_instanced(this: P, count: u32, instances: u32, start: u32, first: u32) {
    let orig: unsafe extern "system" fn(P, u32, u32, u32, u32) = orig_ctx(21);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            state.draws += 1;
            let buffers = state.draw_buffers();
            state.line(format_args!("DrawInstanced {count} {instances} {start} {first}{buffers}"));
        }
    }
    orig(this, count, instances, start, first)
}

unsafe extern "system" fn ctx_map(this: P, resource: P, sub: u32, kind: u32, flags: u32, out: *mut D3D11_MAPPED_SUBRESOURCE) -> i32 {
    let orig: unsafe extern "system" fn(P, P, u32, u32, u32, *mut D3D11_MAPPED_SUBRESOURCE) -> i32 = orig_ctx(14);
    let this = { let _ = this; real() };
    let result = orig(this, resource, sub, kind, flags, out);
    if let Some(mut state) = enter() {
        let is_buffer = state.buffers.contains_key(&(resource as usize));
        if result >= 0 && !out.is_null() && is_buffer {
            // a discarded buffer comes back with whatever the driver has in that memory, and
            // the game does not always write all of it (GLRenderer fills 18 of 24 vertices):
            // start from zeros, so that the logged bytes are the same in every run
            if kind == D3D11_MAP_WRITE_DISCARD.0 as u32 && !(*out).pData.is_null() {
                let size = state.buffers[&(resource as usize)].size as usize;
                std::ptr::write_bytes((*out).pData as *mut u8, 0, size);
            }
            state.mapped.insert((resource as usize, sub), ((*out).pData as usize, kind));
        }
        if state.capturing {
            let name = if is_buffer {
                let info = &state.buffers[&(resource as usize)];
                format!("B{}:{:x}:{:x}", info.size, info.bind, info.usage)
            } else {
                state.resource_name(resource)
            };
            state.line(format_args!("Map {name} {sub} {kind} {flags:x}"));
        }
    }
    result
}

unsafe extern "system" fn ctx_unmap(this: P, resource: P, sub: u32) {
    let orig: unsafe extern "system" fn(P, P, u32) = orig_ctx(15);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        let mapped = state.mapped.remove(&(resource as usize, sub));
        let mut shown = None;
        if let (Some((pointer, kind)), Some(info)) = (mapped, state.buffers.get_mut(&(resource as usize))) {
            // anything but a read: the buffer's bytes are what is in the mapped memory now
            if kind != D3D11_MAP_READ.0 as u32 && pointer != 0 {
                let now = std::slice::from_raw_parts(pointer as *const u8, info.size as usize);
                info.hash = hash_bytes(now);
                if let Some(bytes) = info.bytes.as_mut() {
                    bytes.clear();
                    bytes.extend_from_slice(now);
                }
                shown = Some(hex(now));
            }
        }
        if state.capturing {
            match shown {
                Some(bytes) => {
                    let name = state.buffer_name(resource);
                    state.line(format_args!("Unmap {name} {sub} {bytes}"));
                }
                None => {
                    let name = if state.buffers.contains_key(&(resource as usize)) { state.buffer_name(resource) } else { "texture".into() };
                    state.line(format_args!("Unmap {name} {sub}"));
                }
            }
        }
    }
    orig(this, resource, sub)
}

unsafe extern "system" fn ctx_update_subresource(this: P, resource: P, sub: u32, region: *const D3D11_BOX, data: *const c_void, row_pitch: u32, depth_pitch: u32) {
    let orig: unsafe extern "system" fn(P, P, u32, *const D3D11_BOX, *const c_void, u32, u32) = orig_ctx(48);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        let region_text = if region.is_null() {
            "all".to_string()
        } else {
            let b = *region;
            format!("{},{},{},{},{},{}", b.left, b.top, b.front, b.right, b.bottom, b.back)
        };
        let mut shown = None;
        if let Some(info) = state.buffers.get_mut(&(resource as usize)) {
            let (from, to) = if region.is_null() { (0, info.size as usize) } else { ((*region).left as usize, ((*region).right as usize).min(info.size as usize)) };
            if !data.is_null() && to >= from {
                let new = std::slice::from_raw_parts(data as *const u8, to - from);
                let bytes = info.bytes.get_or_insert_with(|| vec![0; info.size as usize]);
                bytes[from..to].copy_from_slice(new);
                info.hash = hash_bytes(bytes);
                shown = Some(hex(new));
            }
        }
        if state.capturing {
            let name = state.resource_name(resource);
            match shown {
                Some(bytes) => state.line(format_args!("UpdateSubresource {name} {sub} {region_text} {bytes}")),
                None => state.line(format_args!("UpdateSubresource {name} {sub} {region_text} pitch {row_pitch} {depth_pitch}")),
            }
        }
    }
    orig(this, resource, sub, region, data, row_pitch, depth_pitch)
}

unsafe extern "system" fn ctx_ia_set_input_layout(this: P, layout: P) {
    let orig: unsafe extern "system" fn(P, P) = orig_ctx(17);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        state.line(format_args!("IASetInputLayout {}", tagged("layout", layout)));
    }
    orig(this, layout)
}

unsafe extern "system" fn ctx_ia_set_vertex_buffers(this: P, start: u32, count: u32, buffers: *const P, strides: *const u32, offsets: *const u32) {
    let orig: unsafe extern "system" fn(P, u32, u32, *const P, *const u32, *const u32) = orig_ctx(18);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let mut text = String::new();
            for (i, buffer) in pointer_list(buffers, count).into_iter().enumerate() {
                let stride = if strides.is_null() { 0 } else { *strides.add(i) };
                let offset = if offsets.is_null() { 0 } else { *offsets.add(i) };
                let _ = write!(text, " {} {stride} {offset}", state.buffer_name(buffer));
            }
            state.line(format_args!("IASetVertexBuffers {start} {count}{text}"));
        }
    }
    orig(this, start, count, buffers, strides, offsets)
}

unsafe extern "system" fn ctx_ia_set_index_buffer(this: P, buffer: P, format: u32, offset: u32) {
    let orig: unsafe extern "system" fn(P, P, u32, u32) = orig_ctx(19);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let name = state.buffer_name(buffer);
            state.line(format_args!("IASetIndexBuffer {name} {format} {offset}"));
        }
    }
    orig(this, buffer, format, offset)
}

unsafe extern "system" fn ctx_ia_set_primitive_topology(this: P, topology: u32) {
    let orig: unsafe extern "system" fn(P, u32) = orig_ctx(24);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        state.line(format_args!("IASetPrimitiveTopology {topology}"));
    }
    orig(this, topology)
}

unsafe extern "system" fn ctx_om_set_render_targets(this: P, count: u32, views: *const P, depth: P) {
    let orig: unsafe extern "system" fn(P, u32, *const P, P) = orig_ctx(33);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let mut names = Vec::new();
            for view in pointer_list(views, count) {
                names.push(state.rtv_name(view));
            }
            let depth_name = state.dsv_name(depth);
            state.line(format_args!("OMSetRenderTargets {count} [{}] {depth_name}", names.join(" ")));
        }
    }
    orig(this, count, views, depth)
}

unsafe extern "system" fn ctx_om_set_blend_state(this: P, blend: P, factor: *const f32, mask: u32) {
    let orig: unsafe extern "system" fn(P, P, *const f32, u32) = orig_ctx(35);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let factor_text = if factor.is_null() {
                "null".to_string()
            } else {
                let f = std::slice::from_raw_parts(factor, 4);
                format!("{:08x},{:08x},{:08x},{:08x}", f[0].to_bits(), f[1].to_bits(), f[2].to_bits(), f[3].to_bits())
            };
            state.line(format_args!("OMSetBlendState {} {factor_text} {mask:x}", blend_name(blend)));
        }
    }
    orig(this, blend, factor, mask)
}

unsafe extern "system" fn ctx_om_set_depth_stencil_state(this: P, depth: P, reference: u32) {
    let orig: unsafe extern "system" fn(P, P, u32) = orig_ctx(36);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        state.line(format_args!("OMSetDepthStencilState {} {reference}", depth_name(depth)));
    }
    orig(this, depth, reference)
}

unsafe extern "system" fn ctx_rs_set_state(this: P, raster: P) {
    let orig: unsafe extern "system" fn(P, P) = orig_ctx(43);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        state.line(format_args!("RSSetState {}", raster_name(raster)));
    }
    orig(this, raster)
}

unsafe extern "system" fn ctx_rs_set_viewports(this: P, count: u32, viewports: *const D3D11_VIEWPORT) {
    let orig: unsafe extern "system" fn(P, u32, *const D3D11_VIEWPORT) = orig_ctx(44);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let mut text = String::new();
            if !viewports.is_null() {
                for v in std::slice::from_raw_parts(viewports, count as usize) {
                    let _ = write!(
                        text,
                        " [{:08x} {:08x} {:08x} {:08x} {:08x} {:08x}]",
                        v.TopLeftX.to_bits(),
                        v.TopLeftY.to_bits(),
                        v.Width.to_bits(),
                        v.Height.to_bits(),
                        v.MinDepth.to_bits(),
                        v.MaxDepth.to_bits()
                    );
                }
            }
            state.line(format_args!("RSSetViewports {count}{text}"));
        }
    }
    orig(this, count, viewports)
}

unsafe extern "system" fn ctx_clear_render_target_view(this: P, view: P, colour: *const f32) {
    let orig: unsafe extern "system" fn(P, P, *const f32) = orig_ctx(50);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let c = std::slice::from_raw_parts(colour, 4);
            let name = state.rtv_name(view);
            state.line(format_args!(
                "ClearRenderTargetView {name} {:08x},{:08x},{:08x},{:08x}",
                c[0].to_bits(),
                c[1].to_bits(),
                c[2].to_bits(),
                c[3].to_bits()
            ));
        }
    }
    orig(this, view, colour)
}

unsafe extern "system" fn ctx_clear_depth_stencil_view(this: P, view: P, flags: u32, depth: f32, stencil: u8) {
    let orig: unsafe extern "system" fn(P, P, u32, f32, u8) = orig_ctx(53);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let name = state.dsv_name(view);
            state.line(format_args!("ClearDepthStencilView {name} {flags:x} {:08x} {stencil}", depth.to_bits()));
        }
    }
    orig(this, view, flags, depth, stencil)
}

unsafe extern "system" fn ctx_copy_resource(this: P, to: P, from: P) {
    let orig: unsafe extern "system" fn(P, P, P) = orig_ctx(47);
    let this = { let _ = this; real() };
    if let Some(mut state) = enter() {
        if state.capturing {
            let a = state.resource_name(to);
            let b = state.resource_name(from);
            state.line(format_args!("CopyResource {a} <- {b}"));
        }
    }
    orig(this, to, from)
}

/// Every method without a hook of its own: the call is reported by name.
extern "C" fn other_call(index: u32) {
    if let Some(mut state) = enter() {
        let slot = (index & 0xffff) as usize;
        if index >> 16 == 0 {
            state.line(format_args!("{}", CTX_NAMES[slot]));
        } else {
            state.line(format_args!("dev.{}", DEV_NAMES[slot]));
        }
    }
}

/// Machine code of a method of the stand-in context without a hook of its own: reports the call
/// (`other_call(slot)`) when `report` is set, then goes on into the real context's method with
/// every argument register as it was but `this`.
fn forward_code(slot: u32, report: bool) -> Vec<u8> {
    let mut c: Vec<u8> = Vec::new();
    if report {
        c.extend([
            0x51, 0x52, 0x41, 0x50, 0x41, 0x51, // push rcx, rdx, r8, r9
            0x48, 0x83, 0xec, 0x68, // sub rsp, 0x68
            0x0f, 0x11, 0x44, 0x24, 0x20, // movups [rsp+0x20], xmm0
            0x0f, 0x11, 0x4c, 0x24, 0x30, // movups [rsp+0x30], xmm1
            0x0f, 0x11, 0x54, 0x24, 0x40, // movups [rsp+0x40], xmm2
            0x0f, 0x11, 0x5c, 0x24, 0x50, // movups [rsp+0x50], xmm3
            0xb9,
        ]);
        c.extend(slot.to_le_bytes()); // mov ecx, slot
        c.extend([0x48, 0xb8]);
        c.extend((other_call as *const () as u64).to_le_bytes()); // mov rax, other_call
        c.extend([0xff, 0xd0]); // call rax
        c.extend([
            0x0f, 0x10, 0x44, 0x24, 0x20, // movups xmm0, [rsp+0x20]
            0x0f, 0x10, 0x4c, 0x24, 0x30,
            0x0f, 0x10, 0x54, 0x24, 0x40,
            0x0f, 0x10, 0x5c, 0x24, 0x50,
            0x48, 0x83, 0xc4, 0x68, // add rsp, 0x68
            0x41, 0x59, 0x41, 0x58, 0x5a, 0x59, // pop r9, r8, rdx, rcx
        ]);
    }
    c.extend([0x48, 0x8b, 0x49, 0x08]); // mov rcx, [rcx+8]   the real context
    c.extend([0x48, 0x8b, 0x01]); // mov rax, [rcx]     its vtable
    c.extend([0xff, 0xa0]); // jmp [rax + slot*8]
    c.extend((slot * 8).to_le_bytes());
    c
}

/// Machine code that reports a call of the device (`other_call(index)`) and goes on into the
/// real method with every argument register as it was.
fn stub_code(index: u32, original: usize) -> Vec<u8> {
    let mut c: Vec<u8> = vec![
        0x51, 0x52, 0x41, 0x50, 0x41, 0x51, // push rcx, rdx, r8, r9
        0x48, 0x83, 0xec, 0x68, // sub rsp, 0x68
        0x0f, 0x11, 0x44, 0x24, 0x20, // movups [rsp+0x20], xmm0
        0x0f, 0x11, 0x4c, 0x24, 0x30, // movups [rsp+0x30], xmm1
        0x0f, 0x11, 0x54, 0x24, 0x40, // movups [rsp+0x40], xmm2
        0x0f, 0x11, 0x5c, 0x24, 0x50, // movups [rsp+0x50], xmm3
        0xb9,
    ];
    c.extend(index.to_le_bytes()); // mov ecx, index
    c.extend([0x48, 0xb8]);
    c.extend((other_call as *const () as u64).to_le_bytes()); // mov rax, other_call
    c.extend([0xff, 0xd0]); // call rax
    c.extend([
        0x0f, 0x10, 0x44, 0x24, 0x20, // movups xmm0, [rsp+0x20]
        0x0f, 0x10, 0x4c, 0x24, 0x30,
        0x0f, 0x10, 0x54, 0x24, 0x40,
        0x0f, 0x10, 0x5c, 0x24, 0x50,
        0x48, 0x83, 0xc4, 0x68, // add rsp, 0x68
        0x41, 0x59, 0x41, 0x58, 0x5a, 0x59, // pop r9, r8, rdx, rcx
        0x48, 0xb8,
    ]);
    c.extend((original as u64).to_le_bytes()); // mov rax, original
    c.extend([0xff, 0xe0]); // jmp rax
    c
}

unsafe fn patch_slot(vtable: *mut usize, slot: usize, target: usize) -> Result<(), String> {
    let at = vtable.add(slot);
    let mut old = PAGE_PROTECTION_FLAGS(0);
    VirtualProtect(at as *const c_void, 8, PAGE_READWRITE, &mut old).map_err(|e| format!("VirtualProtect: {e}"))?;
    at.write(target);
    let mut ignored = PAGE_PROTECTION_FLAGS(0);
    let _ = VirtualProtect(at as *const c_void, 8, old, &mut ignored);
    Ok(())
}

/// The stand-in context: a vtable pointer and the real context, which the machine-code stubs
/// read at +8.
#[repr(C)]
struct Proxy {
    vtable: *const usize,
    real: *mut c_void,
}

/// `ID3D11Device::GetImmediateContext`: the stand-in, so that nobody gets around the log.
unsafe extern "system" fn dev_get_immediate_context(_this: P, out: *mut P) {
    let proxy = PROXY_CTX.load(Ordering::Relaxed) as P;
    // the reference the caller will release goes to the real context
    let add_ref: unsafe extern "system" fn(P) -> u32 = orig_ctx(1);
    add_ref(real());
    *out = proxy;
}

/// Starts logging the calls of this device and its immediate context. Once per process.
/// Returns the stand-in context: the renderer must make all its calls through that pointer.
///
/// # Safety
/// The two pointers must be a live `ID3D11Device` and its `ID3D11DeviceContext`, and they must
/// outlive every later use of the log and of the returned pointer.
#[allow(clippy::needless_range_loop)] // slots are vtable indices
pub unsafe fn install(device: *mut c_void, context: *mut c_void) -> Result<*mut c_void, String> {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return Err("the command log is already installed".into());
    }
    let stubs = VirtualAlloc(None, 0x8000, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if stubs.is_null() {
        return Err("VirtualAlloc failed".into());
    }
    let mut used = 0usize;
    let mut push = |code: Vec<u8>| -> usize {
        let at = stubs.add(used);
        std::ptr::copy_nonoverlapping(code.as_ptr(), at, code.len());
        used += code.len().next_multiple_of(16);
        assert!(used <= 0x8000);
        at as usize
    };

    let ctx_hooks: &[(usize, usize)] = &[
        (7, ctx_vs_set_constant_buffers as *const () as usize),
        (8, ctx_ps_set_shader_resources as *const () as usize),
        (9, ctx_ps_set_shader as *const () as usize),
        (10, ctx_ps_set_samplers as *const () as usize),
        (11, ctx_vs_set_shader as *const () as usize),
        (12, ctx_draw_indexed as *const () as usize),
        (13, ctx_draw as *const () as usize),
        (14, ctx_map as *const () as usize),
        (15, ctx_unmap as *const () as usize),
        (16, ctx_ps_set_constant_buffers as *const () as usize),
        (17, ctx_ia_set_input_layout as *const () as usize),
        (18, ctx_ia_set_vertex_buffers as *const () as usize),
        (19, ctx_ia_set_index_buffer as *const () as usize),
        (20, ctx_draw_indexed_instanced as *const () as usize),
        (21, ctx_draw_instanced as *const () as usize),
        (22, ctx_gs_set_constant_buffers as *const () as usize),
        (24, ctx_ia_set_primitive_topology as *const () as usize),
        (25, ctx_vs_set_shader_resources as *const () as usize),
        (26, ctx_vs_set_samplers as *const () as usize),
        (33, ctx_om_set_render_targets as *const () as usize),
        (35, ctx_om_set_blend_state as *const () as usize),
        (36, ctx_om_set_depth_stencil_state as *const () as usize),
        (43, ctx_rs_set_state as *const () as usize),
        (44, ctx_rs_set_viewports as *const () as usize),
        (47, ctx_copy_resource as *const () as usize),
        (48, ctx_update_subresource as *const () as usize),
        (50, ctx_clear_render_target_view as *const () as usize),
        (53, ctx_clear_depth_stencil_view as *const () as usize),
    ];
    let dev_hooks: &[(usize, usize)] = &[
        (3, dev_create_buffer as *const () as usize),
        (5, dev_create_texture_2d as *const () as usize),
        (7, dev_create_shader_resource_view as *const () as usize),
        (9, dev_create_render_target_view as *const () as usize),
        (10, dev_create_depth_stencil_view as *const () as usize),
        (20, dev_create_blend_state as *const () as usize),
        (21, dev_create_depth_stencil_state as *const () as usize),
        (22, dev_create_rasterizer_state as *const () as usize),
        (23, dev_create_sampler_state as *const () as usize),
        (11, dev_create_input_layout as *const () as usize),
        (12, dev_create_vertex_shader as *const () as usize),
        (15, dev_create_pixel_shader as *const () as usize),
    ];

    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(State {
        out: Vec::new(),
        capturing: CAPTURE_FROM_INSTALL.load(Ordering::SeqCst),
        buffers: HashMap::new(),
        mapped: HashMap::new(),
        device: device as usize,
        context: context as usize,
        lines: 0,
        draws: 0,
    });

    REAL_CTX.store(context as usize, Ordering::SeqCst);
    let dev_vtable = *(device as *const *mut usize);
    for slot in 0..DEV_SLOTS {
        ORIG_DEV[slot].store(*dev_vtable.add(slot), Ordering::SeqCst);
    }
    // the stand-in context: a hook or a reporting stub for everything from VSSetConstantBuffers
    // on but the getters (they change nothing); plain forwarding for those and for IUnknown
    let mut vtable = vec![0usize; CTX_SLOTS + 64];
    for slot in 0..vtable.len() {
        let getter = slot < 7 || (72..=109).contains(&slot) || slot == 56 || slot >= 112;
        vtable[slot] = match ctx_hooks.iter().find(|h| h.0 == slot) {
            Some(hook) => hook.1,
            None => push(forward_code(slot as u32, !getter)),
        };
    }
    let proxy = Box::leak(Box::new(Proxy { vtable: Box::leak(vtable.into_boxed_slice()).as_ptr(), real: context }));
    PROXY_CTX.store(proxy as *mut Proxy as usize, Ordering::SeqCst);
    // the device: what it creates (the checks and getters stay as they are)
    for slot in 3..=28 {
        let original = ORIG_DEV[slot].load(Ordering::SeqCst);
        let target = match dev_hooks.iter().find(|h| h.0 == slot) {
            Some(hook) => hook.1,
            None => push(stub_code(0x1_0000 | slot as u32, original)),
        };
        patch_slot(dev_vtable, slot, target)?;
    }
    patch_slot(dev_vtable, 40, dev_get_immediate_context as *const () as usize)?;
    Ok(proxy as *mut Proxy as *mut c_void)
}

pub fn installed() -> bool {
    INSTALLED.load(Ordering::SeqCst)
}

/// From here on every call is written down. The log starts with the state the frame inherits.
pub fn begin_capture(title: &str) {
    let Some(mut state) = enter() else {
        return;
    };
    state.capturing = true;
    state.line(format_args!("# {title}"));
    unsafe {
        let Some(context) = state.context() else {
            return;
        };
        // what is bound when the frame starts
        let mut rtvs: [Option<ID3D11RenderTargetView>; 8] = Default::default();
        let mut dsv = None;
        context.OMGetRenderTargets(Some(&mut rtvs), Some(&mut dsv));
        let mut names = Vec::new();
        for view in &rtvs {
            names.push(state.rtv_name(view.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw())));
        }
        let depth = state.dsv_name(dsv.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()));
        state.line(format_args!("start.targets [{}] {depth}", names.join(" ")));

        let mut blend = None;
        let mut factor = [0f32; 4];
        let mut mask = 0u32;
        context.OMGetBlendState(Some(&mut blend), Some(&mut factor), Some(&mut mask));
        state.line(format_args!(
            "start.blend {} {:08x},{:08x},{:08x},{:08x} {mask:x}",
            blend_name(blend.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw())),
            factor[0].to_bits(),
            factor[1].to_bits(),
            factor[2].to_bits(),
            factor[3].to_bits()
        ));
        let mut depth_state = None;
        let mut reference = 0u32;
        context.OMGetDepthStencilState(Some(&mut depth_state), Some(&mut reference));
        state.line(format_args!("start.depth {} {reference}", depth_name(depth_state.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()))));
        let raster = context.RSGetState().ok();
        state.line(format_args!("start.raster {}", raster_name(raster.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()))));
        let mut count = 16u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 16];
        context.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        for v in &viewports[..count as usize] {
            state.line(format_args!(
                "start.viewport {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}",
                v.TopLeftX.to_bits(),
                v.TopLeftY.to_bits(),
                v.Width.to_bits(),
                v.Height.to_bits(),
                v.MinDepth.to_bits(),
                v.MaxDepth.to_bits()
            ));
        }
        let topology = context.IAGetPrimitiveTopology();
        let layout = context.IAGetInputLayout().ok();
        state.line(format_args!("start.ia topology {} {}", topology.0, tagged("layout", layout.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()))));
        let mut vbs: [Option<ID3D11Buffer>; 4] = Default::default();
        let mut strides = [0u32; 4];
        let mut offsets = [0u32; 4];
        context.IAGetVertexBuffers(0, 4, Some(vbs.as_mut_ptr()), Some(strides.as_mut_ptr()), Some(offsets.as_mut_ptr()));
        for (slot, vb) in vbs.iter().enumerate() {
            if let Some(vb) = vb {
                let name = state.buffer_name(vb.as_raw());
                state.line(format_args!("start.vb {slot} {name} {} {}", strides[slot], offsets[slot]));
            }
        }
        let mut ib = None;
        let mut ib_format = DXGI_FORMAT_UNKNOWN;
        let mut ib_offset = 0u32;
        context.IAGetIndexBuffer(Some(&mut ib), Some(&mut ib_format), Some(&mut ib_offset));
        let ib_name = state.buffer_name(ib.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()));
        state.line(format_args!("start.ib {ib_name} {} {ib_offset}", ib_format.0));

        let mut vs = None;
        context.VSGetShader(&mut vs, None, None);
        let mut ps = None;
        context.PSGetShader(&mut ps, None, None);
        state.line(format_args!(
            "start.shaders {} {}",
            tagged("vs", vs.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw())),
            tagged("ps", ps.as_ref().map_or(std::ptr::null_mut(), |v| v.as_raw()))
        ));
        for stage in ["vs", "ps"] {
            let mut cbs: [Option<ID3D11Buffer>; 14] = Default::default();
            let mut srvs: [Option<ID3D11ShaderResourceView>; 32] = Default::default();
            let mut samplers: [Option<ID3D11SamplerState>; 16] = Default::default();
            if stage == "vs" {
                context.VSGetConstantBuffers(0, Some(&mut cbs));
                context.VSGetShaderResources(0, Some(&mut srvs));
                context.VSGetSamplers(0, Some(&mut samplers));
            } else {
                context.PSGetConstantBuffers(0, Some(&mut cbs));
                context.PSGetShaderResources(0, Some(&mut srvs));
                context.PSGetSamplers(0, Some(&mut samplers));
            }
            for (slot, cb) in cbs.iter().enumerate() {
                if let Some(cb) = cb {
                    let name = state.buffer_name(cb.as_raw());
                    let bytes = state.buffers.get(&(cb.as_raw() as usize)).and_then(|i| i.bytes.as_deref()).map(hex).unwrap_or_default();
                    state.line(format_args!("start.{stage}.cb {slot} {name} {bytes}"));
                }
            }
            for (slot, srv) in srvs.iter().enumerate() {
                if let Some(srv) = srv {
                    let name = state.srv_name(srv.as_raw());
                    state.line(format_args!("start.{stage}.srv {slot} {name}"));
                }
            }
            for (slot, sampler) in samplers.iter().enumerate() {
                if let Some(sampler) = sampler {
                    state.line(format_args!("start.{stage}.sampler {slot} {}", sampler_name(sampler.as_raw())));
                }
            }
        }
    }
}

/// What a capture held: the text, and how many draw calls were in it.
pub struct Capture {
    pub text: Vec<u8>,
    pub draws: u64,
    pub lines: u64,
}

/// Ends the capture and hands out the log.
pub fn end_capture() -> Capture {
    let Some(mut state) = enter() else {
        return Capture { text: Vec::new(), draws: 0, lines: 0 };
    };
    state.capturing = false;
    let capture = Capture { text: std::mem::take(&mut state.out), draws: state.draws, lines: state.lines };
    state.draws = 0;
    state.lines = 0;
    capture
}
