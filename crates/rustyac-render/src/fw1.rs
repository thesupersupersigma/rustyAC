// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//
// The text drawing below is a Rust translation of FW1FontWrapper (the library the game links
// for its fonts), which is under the MIT License:
//
//   FW1FontWrapper, Copyright (c) 2011 Erik Rufelt
//
//   Permission is hereby granted, free of charge, to any person obtaining a copy of this
//   software and associated documentation files (the "Software"), to deal in the Software
//   without restriction, including without limitation the rights to use, copy, modify, merge,
//   publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//   to whom the Software is furnished to do so, subject to the following conditions:
//
//   The above copyright notice and this permission notice shall be included in all copies or
//   substantial portions of the Software.
//
//   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//   INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//   PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//   FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//   OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//   DEALINGS IN THE SOFTWARE.

//! The game's text: kgl's font functions (`kglInitFonts` 0x140018210, `kglCreateFont`
//! 0x1400198b0, `kglBlitStringWide` 0x140019aa0, `kglSetFontLocale` 0x140018540) and the
//! FW1FontWrapper build inside acs.exe (object files `CFW1*.obj`), which lays text out with
//! DirectWrite, rasterises each glyph once through a GDI bitmap target into 512 x 512 one-byte
//! glyph sheets and draws a string as indexed quads.
//!
//! What the game's build of the library does that the public source does not: a font wrapper
//! is made with a font collection of the game's own (`content/fonts/*.ttf`, where the game
//! ships its Segoe UI and Consolas) for the text format, while the glyph provider keeps the
//! system collection (so a face of the game's collection has no "unique name" and gets font
//! index 0). The geometry shader is switched off: every glyph is four vertices.

// the library's arithmetic is kept as it is (one mip level makes its alignment 1), and the
// wrappers are boxed because DirectWrite's callbacks hold their addresses
#![allow(clippy::modulo_one, clippy::vec_box, clippy::only_used_in_recursion)]

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use windows::core::{w, Interface, BOOL, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, E_FAIL, E_NOINTERFACE, E_NOTIMPL, RECT, S_OK};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, FillRect, GetCurrentObject, GetObjectW, DIBSECTION, HBRUSH, HDC, OBJ_BITMAP};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::kgl::Kgl;

pub use crate::fw1_atlas::{FW1_ALIASED, FW1_CENTER, FW1_NOWORDWRAP, FW1_RESTORESTATE, FW1_RIGHT};
use crate::fw1_atlas::{GlyphAtlas, GlyphMetrics, GlyphVertex, MAX_INDEX_COUNT, QUAD_VERTEX_SIZE, SHEET_HEIGHT, SHEET_WIDTH, VERTEX_BUFFER_SIZE};

type D3DCompile = unsafe extern "system" fn(*const c_void, usize, PCSTR, *const c_void, *const c_void, PCSTR, PCSTR, u32, u32, *mut Option<ID3DBlob>, *mut Option<ID3DBlob>) -> HRESULT;

// ---------------------------------------------------------------------------------------------
// the shaders of CFW1GlyphRenderStates (createQuadShaders 0x1404443f0, createPixelShaders
// 0x140444060), compiled at start-up with D3DCompile of d3dcompiler_43.dll, optimisation level 3
// ---------------------------------------------------------------------------------------------

const VS_SIMPLE: &str = "cbuffer ShaderConstants : register(b0) {\r\n\tfloat4x4 TransformMatrix : packoffset(c0);\r\n};\r\n\r\nstruct VSIn {\r\n\tfloat4 Position : POSITION;\r\n\tfloat4 GlyphColor : GLYPHCOLOR;\r\n};\r\n\r\nstruct VSOut {\r\n\tfloat4 Position : SV_Position;\r\n\tfloat4 GlyphColor : COLOR;\r\n\tfloat2 TexCoord : TEXCOORD;\r\n};\r\n\r\nVSOut VS(VSIn Input) {\r\n\tVSOut Output;\r\n\t\r\n\tOutput.Position = mul(TransformMatrix, float4(Input.Position.xy, 0.0f, 1.0f));\r\n\tOutput.GlyphColor = Input.GlyphColor;\r\n\tOutput.TexCoord = Input.Position.zw;\r\n\t\r\n\treturn Output;\r\n}\r\n\0";

const VS_CLIP: &str = "cbuffer ShaderConstants : register(b0) {\r\n\tfloat4x4 TransformMatrix : packoffset(c0);\r\n\tfloat4 ClipRect : packoffset(c4);\r\n};\r\n\r\nstruct VSIn {\r\n\tfloat4 Position : POSITION;\r\n\tfloat4 GlyphColor : GLYPHCOLOR;\r\n};\r\n\r\nstruct VSOut {\r\n\tfloat4 Position : SV_Position;\r\n\tfloat4 GlyphColor : COLOR;\r\n\tfloat2 TexCoord : TEXCOORD;\r\n\tfloat4 ClipDistance : CLIPDISTANCE;\r\n};\r\n\r\nVSOut VS(VSIn Input) {\r\n\tVSOut Output;\r\n\t\r\n\tOutput.Position = mul(TransformMatrix, float4(Input.Position.xy, 0.0f, 1.0f));\r\n\tOutput.GlyphColor = Input.GlyphColor;\r\n\tOutput.TexCoord = Input.Position.zw;\r\n\tOutput.ClipDistance = ClipRect + float4(Input.Position.xy, -Input.Position.xy);\r\n\t\r\n\treturn Output;\r\n}\r\n\0";

const PS_SIMPLE: &str = "SamplerState sampler0 : register(s0);\r\nTexture2D<float> tex0 : register(t0);\r\n\r\nstruct PSIn {\r\n\tfloat4 Position : SV_Position;\r\n\tfloat4 GlyphColor : COLOR;\r\n\tfloat2 TexCoord : TEXCOORD;\r\n};\r\n\r\nfloat4 PS(PSIn Input) : SV_Target {\r\n\tfloat a = tex0.Sample(sampler0, Input.TexCoord);\r\n\t\r\n\tif(a == 0.0f)\r\n\t\tdiscard;\r\n\t\r\n\treturn (a * Input.GlyphColor.a) * float4(Input.GlyphColor.rgb, 1.0f);\r\n}\r\n\0";

const PS_CLIP: &str = "SamplerState sampler0 : register(s0);\r\nTexture2D<float> tex0 : register(t0);\r\n\r\nstruct PSIn {\r\n\tfloat4 Position : SV_Position;\r\n\tfloat4 GlyphColor : COLOR;\r\n\tfloat2 TexCoord : TEXCOORD;\r\n\tfloat4 ClipDistance : CLIPDISTANCE;\r\n};\r\n\r\nfloat4 PS(PSIn Input) : SV_Target {\r\n\tclip(Input.ClipDistance);\r\n\t\r\n\tfloat a = tex0.Sample(sampler0, Input.TexCoord);\r\n\t\r\n\tif(a == 0.0f)\r\n\t\tdiscard;\r\n\t\r\n\treturn (a * Input.GlyphColor.a) * float4(Input.GlyphColor.rgb, 1.0f);\r\n}\r\n\0";

// ---------------------------------------------------------------------------------------------
// COM objects of our own (IUnknown by hand: three small interfaces DirectWrite calls back)
// ---------------------------------------------------------------------------------------------

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

#[repr(C)]
struct ComHead {
    table: *const usize,
    count: u32,
}

unsafe extern "system" fn com_add_ref(this: *mut ComHead) -> u32 {
    (*this).count += 1;
    (*this).count
}

/// The objects live as long as the fonts do: a release only counts down.
unsafe extern "system" fn com_release(this: *mut ComHead) -> u32 {
    (*this).count = (*this).count.saturating_sub(1);
    (*this).count
}

unsafe fn query(this: *mut ComHead, wanted: *const GUID, own: &[GUID], out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_INVALIDARG;
    }
    if *wanted == IID_IUNKNOWN || own.contains(&*wanted) {
        (*this).count += 1;
        *out = this as *mut c_void;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

/// `CCollectionLoader` (table 0x141417570): the loader of the game's own font collection.
#[repr(C)]
struct CollectionLoader {
    head: ComHead,
    folder: PathBuf,
}

unsafe extern "system" fn loader_query(this: *mut ComHead, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    query(this, iid, &[IDWriteFontCollectionLoader::IID], out)
}

/// `CCollectionLoader::CreateEnumeratorFromKey` 0x140017de0: every `content/fonts/*.ttf`, in
/// the order the folder lists them, whatever the key.
unsafe extern "system" fn loader_create_enumerator(this: *mut CollectionLoader, factory: *mut c_void, _key: *const c_void, _key_size: u32, out: *mut *mut c_void) -> HRESULT {
    let Some(factory) = IDWriteFactory::from_raw_borrowed(&factory) else { return E_FAIL };
    let mut files = Vec::new();
    let this = &*this;
    if let Ok(list) = std::fs::read_dir(&this.folder) {
        for entry in list.filter_map(|e| e.ok()) {
            let path = entry.path();
            let is_ttf = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf"));
            if !is_ttf || !path.is_file() {
                continue;
            }
            let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain([0]).collect();
            if let Ok(file) = factory.CreateFontFileReference(PCWSTR(wide.as_ptr()), None) {
                files.push(file);
            }
        }
    }
    let enumerator = Box::leak(Box::new(FontEnum { head: ComHead { table: std::ptr::null(), count: 1 }, files, current: -1 }));
    let table: &'static [usize; 5] = Box::leak(Box::new([enum_query as *const () as usize, com_add_ref as *const () as usize, com_release as *const () as usize, enum_move_next as *const () as usize, enum_current as *const () as usize]));
    enumerator.head.table = table.as_ptr();
    *out = enumerator as *mut FontEnum as *mut c_void;
    S_OK
}

/// `CFontEnum` (table 0x141417598).
#[repr(C)]
struct FontEnum {
    head: ComHead,
    files: Vec<IDWriteFontFile>,
    current: i32,
}

unsafe extern "system" fn enum_query(this: *mut ComHead, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    query(this, iid, &[IDWriteFontFileEnumerator::IID], out)
}

/// `CFontEnum::MoveNext` 0x140017c80.
unsafe extern "system" fn enum_move_next(this: *mut FontEnum, has_current: *mut BOOL) -> HRESULT {
    let this = &mut *this;
    this.current += 1;
    *has_current = BOOL::from((this.current as usize) < this.files.len());
    S_OK
}

/// `CFontEnum::GetCurrentFontFile` 0x140017c10.
unsafe extern "system" fn enum_current(this: *mut FontEnum, out: *mut *mut c_void) -> HRESULT {
    let this = &*this;
    match this.files.get(this.current as usize) {
        Some(file) => {
            *out = file.clone().into_raw();
            S_OK
        }
        None => {
            *out = std::ptr::null_mut();
            E_FAIL
        }
    }
}

/// `CFW1TextRenderer`'s DirectWrite side: what a text layout calls back while it is drawn.
#[repr(C)]
struct TextRenderer {
    head: ComHead,
    /// the wrapper being drawn with, and the flags and colour of the call
    wrapper: *mut FontWrapper,
    flags: u32,
    color: u32,
    cached_map: Option<usize>,
    cached_face: *mut c_void,
    cached_size: f32,
}

unsafe extern "system" fn renderer_query(this: *mut ComHead, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    query(this, iid, &[IDWritePixelSnapping::IID, IDWriteTextRenderer::IID], out)
}

/// `CFW1TextRenderer::IsPixelSnappingDisabled`.
unsafe extern "system" fn renderer_snapping(_this: *mut TextRenderer, _context: *mut c_void, disabled: *mut BOOL) -> HRESULT {
    *disabled = BOOL::from(false);
    S_OK
}

/// `CFW1TextRenderer::GetCurrentTransform`: the identity.
unsafe extern "system" fn renderer_transform(_this: *mut TextRenderer, _context: *mut c_void, transform: *mut DWRITE_MATRIX) -> HRESULT {
    *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };
    S_OK
}

/// `CFW1TextRenderer::GetPixelsPerDip`: 96, as the library has it.
unsafe extern "system" fn renderer_pixels_per_dip(_this: *mut TextRenderer, _context: *mut c_void, pixels: *mut f32) -> HRESULT {
    *pixels = 96.0;
    S_OK
}

/// `CFW1TextRenderer::DrawGlyphRun` 0x14044b… : one vertex per glyph, positions rounded to
/// whole pixels.
unsafe extern "system" fn renderer_glyph_run(this: *mut TextRenderer, _context: *mut c_void, baseline_x: f32, baseline_y: f32, _measuring: i32, run: *const DWRITE_GLYPH_RUN, _description: *const c_void, _effect: *mut c_void) -> HRESULT {
    let renderer = &mut *this;
    let wrapper = &mut *renderer.wrapper;
    let run = &*run;
    let face_raw: *mut c_void = std::mem::transmute_copy(&run.fontFace);
    let Some(face) = IDWriteFontFace::from_raw_borrowed(&face_raw) else { return S_OK };
    let map = if renderer.cached_map.is_some() && face_raw == renderer.cached_face && run.fontEmSize == renderer.cached_size {
        renderer.cached_map
    } else {
        let map = wrapper.glyph_map_from_font(face, run.fontEmSize, renderer.flags);
        renderer.cached_map = map;
        renderer.cached_face = face_raw;
        renderer.cached_size = run.fontEmSize;
        map
    };
    let y = (baseline_y + 0.5).floor();
    let mut position_x = (baseline_x + 0.5).floor();
    for i in 0..run.glyphCount as usize {
        let glyph = *run.glyphIndices.add(i);
        let atlas_id = wrapper.atlas_id_from_glyph_index(map, glyph, face, renderer.flags);
        let advance = *run.glyphAdvances.add(i);
        if run.bidiLevel & 1 != 0 {
            position_x -= advance;
        }
        wrapper.vertices.push(GlyphVertex { x: (position_x + 0.5).floor(), y, glyph_index: atlas_id, color: renderer.color });
        wrapper.placed.push(((position_x + 0.5).floor(), y, glyph, renderer.color));
        wrapper.max_sheet_index = wrapper.max_sheet_index.max(atlas_id >> 16);
        if run.bidiLevel & 1 == 0 {
            position_x += advance;
        }
    }
    S_OK
}

unsafe extern "system" fn renderer_line(_this: *mut TextRenderer, _context: *mut c_void, _x: f32, _y: f32, _line: *const c_void, _effect: *mut c_void) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn renderer_inline_object(_this: *mut TextRenderer, _context: *mut c_void, _x: f32, _y: f32, _object: *mut c_void, _sideways: BOOL, _right_to_left: BOOL, _effect: *mut c_void) -> HRESULT {
    E_NOTIMPL
}

// ---------------------------------------------------------------------------------------------
// glyph sheets and the atlas (CFW1GlyphSheet, CFW1GlyphAtlas)
// ---------------------------------------------------------------------------------------------

/// `CFW1GlyphSheet::createDeviceResources`: a sheet's texture and its view.
type SheetTexture = (ID3D11Texture2D, Option<ID3D11ShaderResourceView>);

fn sheet_texture(device: &ID3D11Device) -> Option<SheetTexture> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: SHEET_WIDTH,
        Height: SHEET_HEIGHT,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)).ok()? };
    let texture = texture?;
    let mut view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view)).ok()? };
    Some((texture, view))
}

// ---------------------------------------------------------------------------------------------
// the glyph rasteriser (CFW1DWriteRenderTarget)
// ---------------------------------------------------------------------------------------------

struct GlyphTarget {
    target: IDWriteBitmapRenderTarget,
    dc: HDC,
    brush: HBRUSH,
    bits: *const u8,
    width_bytes: u32,
    bytes_pixel: u32,
    width: u32,
    height: u32,
    /// `m_renderingParams`: by rendering mode (default 0, aliased 1)
    params: BTreeMap<i32, IDWriteRenderingParams>,
}

impl GlyphTarget {
    /// `CFW1DWriteRenderTarget::initRenderTarget` + `createRenderTarget`: a 384 x 384 bitmap.
    fn new(factory: &IDWriteFactory) -> Option<GlyphTarget> {
        unsafe {
            let interop = factory.GetGdiInterop().ok()?;
            let target = interop.CreateBitmapRenderTarget(None, 384, 384).ok()?;
            let _ = target.SetPixelsPerDip(1.0);
            let dc = target.GetMemoryDC();
            if dc.is_invalid() {
                return None;
            }
            let brush = CreateSolidBrush(COLORREF(0));
            let bitmap = GetCurrentObject(dc, OBJ_BITMAP);
            let mut dib = DIBSECTION::default();
            let got = GetObjectW(bitmap, std::mem::size_of::<DIBSECTION>() as i32, Some(&mut dib as *mut DIBSECTION as *mut c_void));
            if (got as usize) < std::mem::size_of::<DIBSECTION>() {
                return None;
            }
            let mut params = BTreeMap::new();
            for mode in [DWRITE_RENDERING_MODE_DEFAULT, DWRITE_RENDERING_MODE_ALIASED] {
                if let Ok(p) = factory.CreateCustomRenderingParams(1.0, 0.0, 0.0, DWRITE_PIXEL_GEOMETRY_FLAT, mode) {
                    params.insert(mode.0, p);
                }
            }
            if params.is_empty() {
                return None;
            }
            Some(GlyphTarget { target, dc, brush, bits: dib.dsBm.bmBits as *const u8, width_bytes: dib.dsBm.bmWidthBytes as u32, bytes_pixel: dib.dsBm.bmBitsPixel as u32 / 8, width: 384, height: 384, params })
        }
    }

    /// `CFW1DWriteRenderTarget::DrawGlyphTemp` 0x140449f80: one glyph drawn white on black at
    /// (2, 2); the metrics of what was drawn and where its pixels are.
    fn draw(&self, face: &IDWriteFontFace, glyph: u16, size: f32, aliased: bool) -> Option<(GlyphMetrics, *const u8, u32, u32)> {
        unsafe {
            let mut font = DWRITE_FONT_METRICS::default();
            face.GetMetrics(&mut font);
            let mut metrics = DWRITE_GLYPH_METRICS::default();
            face.GetDesignGlyphMetrics(&glyph, 1, &mut metrics, false).ok()?;
            // initGlyphData
            let scale = size / font.designUnitsPerEm as f32;
            let l = metrics.leftSideBearing as f32 * scale;
            let t = metrics.topSideBearing as f32 * scale;
            let r = metrics.rightSideBearing as f32 * scale;
            let b = metrics.bottomSideBearing as f32 * scale;
            let v = metrics.verticalOriginY as f32 * scale;
            let aw = metrics.advanceWidth as f32 * scale;
            let ah = metrics.advanceHeight as f32 * scale;
            let offset_x = l.floor();
            let offset_y = t.floor() - v.floor();
            let max_width = (aw - r - l + 2.0) as i32;
            let max_height = (ah - b - t + 2.0) as i32;

            let advance = 0.0f32;
            let offset = DWRITE_GLYPH_OFFSET::default();
            let run = DWRITE_GLYPH_RUN {
                fontFace: ManuallyDrop::new(Some(face.clone())),
                fontEmSize: size,
                glyphCount: 1,
                glyphIndices: &glyph,
                glyphAdvances: &advance,
                glyphOffsets: &offset,
                isSideways: BOOL::from(false),
                bidiLevel: 0,
            };
            let mut rect = RECT { left: 0, top: 0, right: 2 + max_width + 5, bottom: 2 + max_height + 5 };
            FillRect(self.dc, &rect, self.brush);
            let (mode, measuring) = if aliased { (DWRITE_RENDERING_MODE_ALIASED, DWRITE_MEASURING_MODE_GDI_CLASSIC) } else { (DWRITE_RENDERING_MODE_DEFAULT, DWRITE_MEASURING_MODE_NATURAL) };
            let params = self.params.get(&mode.0).or_else(|| self.params.values().next())?;
            let drawn = self.target.DrawGlyphRun(2.0 - offset_x, 2.0 - offset_y, measuring, &run, params, COLORREF(0x00ff_ffff), Some(&mut rect));
            let mut run = run;
            ManuallyDrop::drop(&mut run.fontFace);
            drawn.ok()?;
            rect.left = rect.left.max(0);
            rect.top = rect.top.max(0);
            rect.right = rect.right.min(self.width as i32);
            rect.bottom = rect.bottom.min(self.height as i32);
            let out = GlyphMetrics { offset_x: offset_x + rect.left as f32 - 2.0, offset_y: offset_y + rect.top as f32 - 2.0, width: (rect.right - rect.left) as u32, height: (rect.bottom - rect.top) as u32 };
            let pixels = self.bits.add(rect.top as usize * self.width_bytes as usize + rect.left as usize * self.bytes_pixel as usize);
            Some((out, pixels, self.width_bytes, self.bytes_pixel))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// one font wrapper (CFW1FontWrapper with its atlas, provider, vertex drawer and render states)
// ---------------------------------------------------------------------------------------------

/// `CFW1GlyphProvider::GlyphMap`.
struct GlyphMap {
    font_size: f32,
    font_flags: u32,
    glyphs: Vec<u32>,
}

pub struct FontWrapper {
    factory: IDWriteFactory,
    /// the system collection: the provider's
    system_collection: IDWriteFontCollection,
    text_format: Option<IDWriteTextFormat>,
    /// `Font::fontWrappers`' key, and the compiled shaders (for the kgl call log)
    key: String,
    shader_code: (Vec<u8>, Vec<u8>),
    atlas: GlyphAtlas<SheetTexture>,
    // CFW1GlyphProvider
    fonts: Vec<(IDWriteFontFace, Vec<u16>)>,
    font_map: HashMap<(u32, u32, u32), usize>,
    maps: Vec<GlyphMap>,
    target: Option<GlyphTarget>,
    // CFW1TextGeometry
    vertices: Vec<GlyphVertex>,
    max_sheet_index: u32,
    placed: Vec<(f32, f32, u16, u32)>,
    // CFW1GlyphVertexDrawer
    vertex_buffer: Option<ID3D11Buffer>,
    index_buffer: Option<ID3D11Buffer>,
    // CFW1GlyphRenderStates
    vs_quad: Option<ID3D11VertexShader>,
    _vs_clip_quad: Option<ID3D11VertexShader>,
    quad_layout: Option<ID3D11InputLayout>,
    ps: Option<ID3D11PixelShader>,
    _ps_clip: Option<ID3D11PixelShader>,
    constant_buffer: Option<ID3D11Buffer>,
    blend: Option<ID3D11BlendState>,
    sampler: Option<ID3D11SamplerState>,
    raster: Option<ID3D11RasterizerState>,
    depth: Option<ID3D11DepthStencilState>,
    renderer: *mut TextRenderer,
}

unsafe fn compile(compile: D3DCompile, source: &str, entry: PCSTR, profile: PCSTR) -> Option<Vec<u8>> {
    let mut code = None;
    // D3DCOMPILE_OPTIMIZATION_LEVEL3; the size counts the string's end, as sizeof does
    let result = compile(source.as_ptr() as *const c_void, source.len(), PCSTR::null(), std::ptr::null(), std::ptr::null(), entry, profile, 0x8000, 0, &mut code, std::ptr::null_mut());
    if result.is_err() {
        return None;
    }
    let code = code?;
    Some(std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()).to_vec())
}

impl FontWrapper {
    /// `CFW1Factory::CreateFontWrapper` 0x1404415a0 with the parameters of `kglCreateFont`:
    /// the atlas (and with its "no glyph" block the first sheet), the provider, the vertex
    /// drawer's buffers, the render states, then the text format.
    fn new(kgl: &Kgl, key: &str, d3d_compile: D3DCompile, collection: &Option<IDWriteFontCollection>, family: &[u16], weight: i32, style: i32, locale: &[u16]) -> Option<Box<FontWrapper>> {
        unsafe {
            let device = &kgl.device;
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let mut system_collection = None;
            factory.GetSystemFontCollection(&mut system_collection, false).ok()?;
            let system_collection = system_collection?;

            let mut wrapper = Box::new(FontWrapper {
                factory,
                system_collection,
                text_format: None,
                key: key.to_string(),
                shader_code: (Vec::new(), Vec::new()),
                atlas: GlyphAtlas::new(),
                fonts: Vec::new(),
                font_map: HashMap::new(),
                maps: Vec::new(),
                target: None,
                vertices: Vec::new(),
                max_sheet_index: 0,
                placed: Vec::new(),
                vertex_buffer: None,
                index_buffer: None,
                vs_quad: None,
                _vs_clip_quad: None,
                quad_layout: None,
                ps: None,
                _ps_clip: None,
                constant_buffer: None,
                blend: None,
                sampler: None,
                raster: None,
                depth: None,
                renderer: std::ptr::null_mut(),
            });
            // CFW1GlyphAtlas::initGlyphAtlas: glyph 0, a white block of 16 x 16
            let white = [0xffu8; 256];
            if wrapper.atlas.insert(&GlyphMetrics { offset_x: 0.0, offset_y: 0.0, width: 16, height: 16 }, &white, 16, 1, || sheet_texture(device)) == u32::MAX {
                return None;
            }

            // CFW1GlyphVertexDrawer::createBuffers
            let desc = D3D11_BUFFER_DESC { ByteWidth: VERTEX_BUFFER_SIZE, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, MiscFlags: 0, StructureByteStride: 0 };
            device.CreateBuffer(&desc, None, Some(&mut wrapper.vertex_buffer)).ok()?;
            let indices = crate::fw1_atlas::quad_indices();
            let desc = D3D11_BUFFER_DESC { ByteWidth: 2 * MAX_INDEX_COUNT, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_INDEX_BUFFER.0 as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
            let init = D3D11_SUBRESOURCE_DATA { pSysMem: indices.as_ptr() as *const c_void, SysMemPitch: 0, SysMemSlicePitch: 0 };
            device.CreateBuffer(&desc, Some(&init), Some(&mut wrapper.index_buffer)).ok()?;

            // CFW1GlyphRenderStates::initRenderResources 0x140444b00 (no geometry shader)
            let level = kgl.feature_level.0;
            let (vs_profile, ps_profile) = if level >= 0xb000 {
                (windows::core::s!("vs_5_0"), windows::core::s!("ps_5_0"))
            } else if level >= 0xa000 {
                (windows::core::s!("vs_4_0"), windows::core::s!("ps_4_0"))
            } else if level >= 0x9300 {
                (windows::core::s!("vs_4_0_level_9_3"), windows::core::s!("ps_4_0_level_9_3"))
            } else {
                (windows::core::s!("vs_4_0_level_9_1"), windows::core::s!("ps_4_0_level_9_1"))
            };
            let vs_code = compile(d3d_compile, VS_SIMPLE, windows::core::s!("VS"), vs_profile)?;
            device.CreateVertexShader(&vs_code, None, Some(&mut wrapper.vs_quad)).ok()?;
            let vs_clip_code = compile(d3d_compile, VS_CLIP, windows::core::s!("VS"), vs_profile)?;
            device.CreateVertexShader(&vs_clip_code, None, Some(&mut wrapper._vs_clip_quad)).ok()?;
            let elements = [
                D3D11_INPUT_ELEMENT_DESC { SemanticName: windows::core::s!("POSITION"), SemanticIndex: 0, Format: DXGI_FORMAT_R32G32B32A32_FLOAT, InputSlot: 0, AlignedByteOffset: 0, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, InstanceDataStepRate: 0 },
                D3D11_INPUT_ELEMENT_DESC { SemanticName: windows::core::s!("GLYPHCOLOR"), SemanticIndex: 0, Format: DXGI_FORMAT_R8G8B8A8_UNORM, InputSlot: 0, AlignedByteOffset: D3D11_APPEND_ALIGNED_ELEMENT, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, InstanceDataStepRate: 0 },
            ];
            device.CreateInputLayout(&elements, &vs_code, Some(&mut wrapper.quad_layout)).ok()?;
            let ps_code = compile(d3d_compile, PS_SIMPLE, windows::core::s!("PS"), ps_profile)?;
            device.CreatePixelShader(&ps_code, None, Some(&mut wrapper.ps)).ok()?;
            let ps_clip_code = compile(d3d_compile, PS_CLIP, windows::core::s!("PS"), ps_profile)?;
            device.CreatePixelShader(&ps_clip_code, None, Some(&mut wrapper._ps_clip)).ok()?;
            wrapper.shader_code = (vs_code, ps_code);
            // createConstantBuffer: the transform and the clip rectangle
            let desc = D3D11_BUFFER_DESC { ByteWidth: 80, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, MiscFlags: 0, StructureByteStride: 0 };
            device.CreateBuffer(&desc, None, Some(&mut wrapper.constant_buffer)).ok()?;
            // createRenderStates
            let mut blend = D3D11_BLEND_DESC::default();
            for target in blend.RenderTarget.iter_mut().take(4) {
                *target = D3D11_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: true.into(),
                    SrcBlend: D3D11_BLEND_ONE,
                    DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                    BlendOp: D3D11_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D11_BLEND_ONE,
                    DestBlendAlpha: D3D11_BLEND_ZERO,
                    BlendOpAlpha: D3D11_BLEND_OP_ADD,
                    RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
                };
            }
            device.CreateBlendState(&blend, Some(&mut wrapper.blend)).ok()?;
            let sampler = D3D11_SAMPLER_DESC { Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR, AddressU: D3D11_TEXTURE_ADDRESS_CLAMP, AddressV: D3D11_TEXTURE_ADDRESS_CLAMP, AddressW: D3D11_TEXTURE_ADDRESS_CLAMP, MaxLOD: f32::MAX, ..Default::default() };
            device.CreateSamplerState(&sampler, Some(&mut wrapper.sampler)).ok()?;
            let raster = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, FrontCounterClockwise: false.into(), DepthClipEnable: true.into(), ..Default::default() };
            device.CreateRasterizerState(&raster, Some(&mut wrapper.raster)).ok()?;
            let depth = D3D11_DEPTH_STENCIL_DESC::default();
            device.CreateDepthStencilState(&depth, Some(&mut wrapper.depth)).ok()?;

            // CFW1FontWrapper::initFontWrapper 0x1404425f0: the text format, in the game's collection
            if !family.is_empty() && family[0] != 0 {
                let family: Vec<u16> = family.iter().copied().chain([0]).collect();
                let locale: Vec<u16> = locale.iter().copied().chain([0]).collect();
                let format = wrapper.factory.CreateTextFormat(PCWSTR(family.as_ptr()), collection.as_ref(), DWRITE_FONT_WEIGHT(weight), DWRITE_FONT_STYLE(style), DWRITE_FONT_STRETCH(5), 32.0, PCWSTR(locale.as_ptr())).ok()?;
                wrapper.text_format = Some(format);
            }

            let table: &'static [usize; 10] = Box::leak(Box::new([
                renderer_query as *const () as usize,
                com_add_ref as *const () as usize,
                com_release as *const () as usize,
                renderer_snapping as *const () as usize,
                renderer_transform as *const () as usize,
                renderer_pixels_per_dip as *const () as usize,
                renderer_glyph_run as *const () as usize,
                renderer_line as *const () as usize,
                renderer_line as *const () as usize,
                renderer_inline_object as *const () as usize,
            ]));
            let renderer = Box::leak(Box::new(TextRenderer { head: ComHead { table: table.as_ptr(), count: 1 }, wrapper: std::ptr::null_mut(), flags: 0, color: 0, cached_map: None, cached_face: std::ptr::null_mut(), cached_size: 0.0 }));
            renderer.wrapper = &mut *wrapper;
            wrapper.renderer = renderer;
            Some(wrapper)
        }
    }

    /// `CFW1GlyphProvider::getUniqueNameFromFontFace`: family and face name in the provider's
    /// (the system's) collection; empty for a face that is not of it.
    unsafe fn unique_name(&self, face: &IDWriteFontFace) -> Vec<u16> {
        let mut name = Vec::new();
        let Ok(font) = self.system_collection.GetFontFromFontFace(face) else { return name };
        let read = |strings: &IDWriteLocalizedStrings, name: &mut Vec<u16>| {
            let mut index = 0u32;
            let mut exists = BOOL::from(false);
            if strings.FindLocaleName(w!("en-us"), &mut index, &mut exists).is_err() || !exists.as_bool() {
                index = 0;
            }
            if strings.GetCount() > index {
                if let Ok(length) = strings.GetStringLength(index) {
                    let mut text = vec![0u16; length as usize + 1];
                    if strings.GetString(index, &mut text).is_ok() {
                        let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                        name.extend_from_slice(&text[..end]);
                    }
                }
            }
        };
        if let Ok(family) = font.GetFontFamily() {
            if let Ok(names) = family.GetFamilyNames() {
                read(&names, &mut name);
            }
        }
        if let Ok(names) = font.GetFaceNames() {
            read(&names, &mut name);
        }
        if !name.is_empty() {
            let simulations = face.GetSimulations();
            if simulations == DWRITE_FONT_SIMULATIONS_BOLD {
                name.extend("SimBold".encode_utf16());
            } else if simulations == DWRITE_FONT_SIMULATIONS_OBLIQUE {
                name.extend("SimOblique".encode_utf16());
            }
        }
        name
    }

    /// `CFW1GlyphProvider::getFontIndexFromFontFace`.
    unsafe fn font_index(&mut self, face: &IDWriteFontFace) -> u32 {
        if let Some(i) = self.fonts.iter().position(|(known, _)| known.as_raw() == face.as_raw()) {
            return i as u32;
        }
        let name = self.unique_name(face);
        if name.is_empty() {
            return 0;
        }
        if let Some(i) = self.fonts.iter().position(|(_, known)| *known == name) {
            self.fonts[i].0 = face.clone();
            return i as u32;
        }
        self.fonts.push((face.clone(), name));
        self.fonts.len() as u32 - 1
    }

    /// `CFW1GlyphProvider::GetGlyphMapFromFont`.
    unsafe fn glyph_map_from_font(&mut self, face: &IDWriteFontFace, size: f32, flags: u32) -> Option<usize> {
        let index = self.font_index(face);
        let id = (index, flags & FW1_ALIASED, size.to_bits());
        if let Some(map) = self.font_map.get(&id) {
            return Some(*map);
        }
        let map = self.maps.len();
        self.maps.push(GlyphMap { font_size: size, font_flags: flags, glyphs: vec![u32::MAX; face.GetGlyphCount() as usize] });
        self.font_map.insert(id, map);
        self.insert_new_glyph(map, 0, face);
        Some(map)
    }

    /// `CFW1GlyphProvider::insertNewGlyph`.
    unsafe fn insert_new_glyph(&mut self, map: usize, glyph: u16, face: &IDWriteFontFace) -> u32 {
        if self.target.is_none() {
            self.target = GlyphTarget::new(&self.factory);
        }
        let Some(target) = self.target.take() else { return u32::MAX };
        let (size, aliased) = (self.maps[map].font_size, self.maps[map].font_flags & FW1_ALIASED != 0);
        let mut id = u32::MAX;
        if let Some((metrics, pixels, row_pitch, stride)) = target.draw(face, glyph, size, aliased) {
            let pixels: &[u8] = if metrics.height == 0 { &[] } else { std::slice::from_raw_parts(pixels, ((metrics.height - 1) * row_pitch + metrics.width * stride) as usize) };
            crate::fw1_atlas::dump_glyph(&self.key, size, glyph, &metrics, pixels, row_pitch, stride);
            id = self.maps[map].glyphs[glyph as usize];
            if id == u32::MAX {
                // any sheet knows the device (the first was made with the wrapper)
                let device = self.atlas.sheets.first().and_then(|(_, (texture, _))| texture.GetDevice().ok());
                id = match device {
                    Some(device) => self.atlas.insert(&metrics, pixels, row_pitch, stride, || sheet_texture(&device)),
                    None => u32::MAX,
                };
                if id != u32::MAX {
                    self.maps[map].glyphs[glyph as usize] = id;
                }
            }
        }
        self.target = Some(target);
        id
    }

    /// `CFW1GlyphProvider::GetAtlasIdFromGlyphIndex`.
    unsafe fn atlas_id_from_glyph_index(&mut self, map: Option<usize>, glyph: u16, face: &IDWriteFontFace, flags: u32) -> u32 {
        let Some(map) = map else { return 0 };
        if glyph as usize >= self.maps[map].glyphs.len() {
            return 0;
        }
        let mut id = self.maps[map].glyphs[glyph as usize];
        if id == u32::MAX {
            id = self.insert_new_glyph(map, glyph, face);
        }
        if id == u32::MAX {
            id = self.maps[map].glyphs[0];
            if id == u32::MAX {
                id = if glyph == 0 { 0 } else { self.atlas_id_from_glyph_index(Some(map), 0, face, flags) };
            }
            if self.maps[map].glyphs[glyph as usize] == u32::MAX {
                self.maps[map].glyphs[glyph as usize] = id;
            }
            if id == u32::MAX {
                id = 0;
            }
        }
        id
    }

    /// `CFW1FontWrapper::DrawString` 0x140447eb0 (context, string, size, x, y, colour, flags):
    /// the layout rectangle is the point, word wrap is off.
    pub fn draw_string(&mut self, context: &ID3D11DeviceContext, text: &[u16], size: f32, x: f32, y: f32, color: u32, flags: u32) {
        let flags = flags | FW1_NOWORDWRAP;
        unsafe {
            // CFW1FontWrapper::createTextLayout 0x140442480
            let Some(format) = &self.text_format else { return };
            let Ok(layout) = self.factory.CreateTextLayout(text, format, 0.0, 0.0) else { return };
            let all = DWRITE_TEXT_RANGE { startPosition: 0, length: text.len() as u32 };
            let _ = layout.SetFontSize(size, all);
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            if flags & FW1_RIGHT != 0 {
                let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
            } else if flags & FW1_CENTER != 0 {
                let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }

            // CFW1FontWrapper::DrawTextLayout 0x140447f80: the geometry …
            self.vertices.clear();
            self.placed.clear();
            self.max_sheet_index = 0;
            // … AnalyzeTextLayout 0x140447ad0 -> CFW1TextRenderer::DrawTextLayout
            let renderer = &mut *self.renderer;
            renderer.wrapper = self;
            renderer.flags = flags;
            renderer.color = color;
            renderer.cached_map = None;
            renderer.cached_face = std::ptr::null_mut();
            renderer.cached_size = 0.0;
            let raw = self.renderer as *mut c_void;
            if let Some(callbacks) = IDWriteTextRenderer::from_raw_borrowed(&raw) {
                let _ = layout.Draw(None, callbacks, x, y);
            }
            crate::fw1_atlas::dump_layout(&self.key, size, x, y, flags, text, &self.placed);
            // CFW1GlyphAtlas::Flush
            self.atlas.flush(|(texture, _), [left, top, right, bottom], bytes| {
                let region = D3D11_BOX { left, top, front: 0, right, bottom, back: 1 };
                context.UpdateSubresource(texture, 0, Some(&region), bytes.as_ptr() as *const c_void, SHEET_WIDTH, 0);
            });
            // … and DrawGeometry 0x140447c20
            self.draw_geometry(context);
        }
    }

    /// `CFW1FontWrapper::DrawGeometry` with FW1_RESTORESTATE and without the geometry shader:
    /// the state is saved, the wrapper's own set, the quads drawn and the state put back.
    unsafe fn draw_geometry(&mut self, context: &ID3D11DeviceContext) {
        if self.vertices.is_empty() {
            return;
        }
        let saved = SavedState::save(context);

        // CFW1GlyphRenderStates::SetStates
        context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context.IASetInputLayout(self.quad_layout.as_ref());
        context.VSSetShader(self.vs_quad.as_ref(), None);
        context.PSSetShader(self.ps.as_ref(), None);
        context.VSSetConstantBuffers(0, Some(std::slice::from_ref(&self.constant_buffer)));
        context.GSSetShader(None, None);
        context.DSSetShader(None, None);
        context.HSSetShader(None, None);
        context.OMSetBlendState(self.blend.as_ref(), None, 0xffff_ffff);
        context.OMSetDepthStencilState(self.depth.as_ref(), 0);
        context.RSSetState(self.raster.as_ref());
        context.PSSetSamplers(0, Some(std::slice::from_ref(&self.sampler)));

        // CFW1GlyphRenderStates::UpdateShaderConstants: pixels of the viewport to clip space
        let mut count = 1u32;
        let mut viewport = D3D11_VIEWPORT::default();
        context.RSGetViewports(&mut count, Some(&mut viewport));
        let constants = crate::fw1_atlas::constants((count > 0).then_some((viewport.Width, viewport.Height)));
        if let Some(buffer) = &self.constant_buffer {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)).is_ok() {
                std::ptr::copy_nonoverlapping(constants.as_ptr(), mapped.pData as *mut u8, 80);
                context.Unmap(buffer, 0);
            }
        }

        // CFW1GlyphVertexDrawer::DrawVertices -> drawGlyphsAsQuads
        let batches = crate::fw1_atlas::batches(&self.vertices, self.max_sheet_index, &self.atlas);
        crate::fw1_atlas::log_geometry(&self.placed, &batches, &constants);
        context.IASetIndexBuffer(self.index_buffer.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        context.IASetVertexBuffers(0, 1, Some(&self.vertex_buffer), Some(&QUAD_VERTEX_SIZE), Some(&0));
        let mut active_sheet = usize::MAX;
        for batch in &batches {
            let Some(buffer) = &self.vertex_buffer else { break };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)).is_err() {
                break;
            }
            std::ptr::copy_nonoverlapping(batch.bytes.as_ptr(), mapped.pData as *mut u8, batch.bytes.len());
            context.Unmap(buffer, 0);
            for &(sheet, index_count, base) in &batch.draws {
                if sheet != active_sheet {
                    // CFW1GlyphSheet::BindSheet
                    let (_, (_, view)) = &self.atlas.sheets[sheet];
                    context.PSSetShaderResources(0, Some(std::slice::from_ref(view)));
                    active_sheet = sheet;
                }
                context.DrawIndexed(index_count, 0, base);
            }
        }

        saved.restore(context);
    }
}

/// `CFW1StateSaver`: what `saveCurrentState` reads and `restoreSavedState` sets again, in the
/// library's order (feature level 11).
struct SavedState {
    topology: D3D_PRIMITIVE_TOPOLOGY,
    layout: Option<ID3D11InputLayout>,
    blend: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth: Option<ID3D11DepthStencilState>,
    stencil_ref: u32,
    raster: Option<ID3D11RasterizerState>,
    vs: Option<ID3D11VertexShader>,
    vs_buffer: [Option<ID3D11Buffer>; 1],
    ps: Option<ID3D11PixelShader>,
    ps_view: [Option<ID3D11ShaderResourceView>; 1],
    sampler: [Option<ID3D11SamplerState>; 1],
    gs: Option<ID3D11GeometryShader>,
    gs_buffer: [Option<ID3D11Buffer>; 1],
    gs_view: [Option<ID3D11ShaderResourceView>; 1],
    hs: Option<ID3D11HullShader>,
    ds: Option<ID3D11DomainShader>,
    vertex_buffer: Option<ID3D11Buffer>,
    vertex_stride: u32,
    vertex_offset: u32,
    index_buffer: Option<ID3D11Buffer>,
    index_format: DXGI_FORMAT,
    index_offset: u32,
}

impl SavedState {
    unsafe fn save(context: &ID3D11DeviceContext) -> SavedState {
        let mut s = SavedState {
            topology: D3D_PRIMITIVE_TOPOLOGY(0),
            layout: None,
            blend: None,
            blend_factor: [0.0; 4],
            sample_mask: 0xffff_ffff,
            depth: None,
            stencil_ref: 0,
            raster: None,
            vs: None,
            vs_buffer: [None],
            ps: None,
            ps_view: [None],
            sampler: [None],
            gs: None,
            gs_buffer: [None],
            gs_view: [None],
            hs: None,
            ds: None,
            vertex_buffer: None,
            vertex_stride: 0,
            vertex_offset: 0,
            index_buffer: None,
            index_format: DXGI_FORMAT_UNKNOWN,
            index_offset: 0,
        };
        s.topology = context.IAGetPrimitiveTopology();
        s.layout = context.IAGetInputLayout().ok();
        context.OMGetBlendState(Some(&mut s.blend), Some(&mut s.blend_factor), Some(&mut s.sample_mask));
        context.OMGetDepthStencilState(Some(&mut s.depth), Some(&mut s.stencil_ref));
        s.raster = context.RSGetState().ok();
        context.VSGetShader(&mut s.vs, None, None);
        context.VSGetConstantBuffers(0, Some(&mut s.vs_buffer));
        context.PSGetShader(&mut s.ps, None, None);
        context.PSGetShaderResources(0, Some(&mut s.ps_view));
        context.PSGetSamplers(0, Some(&mut s.sampler));
        context.GSGetShader(&mut s.gs, None, None);
        context.GSGetConstantBuffers(0, Some(&mut s.gs_buffer));
        context.GSGetShaderResources(0, Some(&mut s.gs_view));
        context.HSGetShader(&mut s.hs, None, None);
        context.DSGetShader(&mut s.ds, None, None);
        context.IAGetVertexBuffers(0, 1, Some(&mut s.vertex_buffer), Some(&mut s.vertex_stride), Some(&mut s.vertex_offset));
        context.IAGetIndexBuffer(Some(&mut s.index_buffer), Some(&mut s.index_format), Some(&mut s.index_offset));
        s
    }

    unsafe fn restore(&self, context: &ID3D11DeviceContext) {
        context.IASetPrimitiveTopology(self.topology);
        context.IASetInputLayout(self.layout.as_ref());
        context.OMSetBlendState(self.blend.as_ref(), Some(&self.blend_factor), self.sample_mask);
        context.OMSetDepthStencilState(self.depth.as_ref(), self.stencil_ref);
        context.RSSetState(self.raster.as_ref());
        context.VSSetShader(self.vs.as_ref(), Some(&[]));
        context.VSSetConstantBuffers(0, Some(&self.vs_buffer));
        context.PSSetShader(self.ps.as_ref(), Some(&[]));
        context.PSSetShaderResources(0, Some(&self.ps_view));
        context.PSSetSamplers(0, Some(&self.sampler));
        context.GSSetShader(self.gs.as_ref(), Some(&[]));
        context.GSSetConstantBuffers(0, Some(&self.gs_buffer));
        context.GSSetShaderResources(0, Some(&self.gs_view));
        context.HSSetShader(self.hs.as_ref(), Some(&[]));
        context.DSSetShader(self.ds.as_ref(), Some(&[]));
        context.IASetVertexBuffers(0, 1, Some(&self.vertex_buffer), Some(&self.vertex_stride), Some(&self.vertex_offset));
        context.IASetIndexBuffer(self.index_buffer.as_ref(), self.index_format, self.index_offset);
    }
}

// ---------------------------------------------------------------------------------------------
// kgl's fonts and `Font::fontWrappers`
// ---------------------------------------------------------------------------------------------

/// The game's font state: kgl's globals (`pFW1Factory`, `pCollectionLoader`,
/// `acFontCollection`, the locale) and `Font::fontWrappers`, the map from "family[ Italic][
/// Bold]" to a wrapper.
pub struct Fonts {
    collection: Option<IDWriteFontCollection>,
    locale: Vec<u16>,
    d3d_compile: Option<D3DCompile>,
    by_key: BTreeMap<Vec<u16>, Option<usize>>,
    wrappers: Vec<Box<FontWrapper>>,
}

impl Fonts {
    /// `kglInitFonts` 0x140018210: the game's own font collection, of `content/fonts/*.ttf`.
    pub fn new(game_folder: &Path) -> Fonts {
        let mut fonts = Fonts { collection: None, locale: Vec::new(), d3d_compile: None, by_key: BTreeMap::new(), wrappers: Vec::new() };
        unsafe {
            if let Ok(module) = LoadLibraryW(w!("d3dcompiler_43.dll")) {
                fonts.d3d_compile = GetProcAddress(module, windows::core::s!("D3DCompile")).map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, D3DCompile>(f));
            }
            if fonts.d3d_compile.is_none() {
                println!("ERROR: d3dcompiler_43.dll (the DirectX runtime the game installs) is not there: no text");
            }
            let Ok(factory) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) else {
                println!("ERROR: DWriteCreateFactory failed");
                return fonts;
            };
            let table: &'static [usize; 4] = Box::leak(Box::new([loader_query as *const () as usize, com_add_ref as *const () as usize, com_release as *const () as usize, loader_create_enumerator as *const () as usize]));
            let loader = Box::leak(Box::new(CollectionLoader { head: ComHead { table: table.as_ptr(), count: 1 }, folder: game_folder.join("content/fonts") }));
            let raw = loader as *mut CollectionLoader as *mut c_void;
            if let Some(loader) = IDWriteFontCollectionLoader::from_raw_borrowed(&raw) {
                if factory.RegisterFontCollectionLoader(loader).is_err() {
                    println!("ERROR: RegisterFontCollectionLoader");
                }
                match factory.CreateCustomFontCollection(loader, std::ptr::null(), 0) {
                    Ok(collection) => fonts.collection = Some(collection),
                    Err(_) => println!("ERROR: CreateCustomFontCollection"),
                }
            }
        }
        fonts
    }

    /// `kglSetFontLocale` 0x140018540: for the fonts made from now on.
    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.encode_utf16().collect();
        println!("kglSetFontLocale {locale}");
    }

    /// `Font::getFontWrapper` 0x1402010d0: one wrapper for a family, italic or not, bold or not
    /// (`None`: it could not be made; asked again the next time).
    pub fn get_font_wrapper(&mut self, kgl: &Kgl, family: &str, italic: bool, bold: bool) -> Option<usize> {
        let mut key: Vec<u16> = family.encode_utf16().collect();
        if italic {
            key.extend(" Italic".encode_utf16());
        }
        if bold {
            key.extend(" Bold".encode_utf16());
        }
        if let Some(Some(known)) = self.by_key.get(&key) {
            return Some(*known);
        }
        let made = self.create_font(kgl, family, italic, bold);
        if made.is_none() {
            println!("ERROR: pFW1Factory->CreateFontWrapper({family}) failed");
        }
        let shaders = made.map(|n| (&self.wrappers[n].shader_code.0[..], &self.wrappers[n].shader_code.1[..]));
        crate::fw1_atlas::log_create_font(&String::from_utf16_lossy(&key), made, shaders);
        self.by_key.insert(key, made);
        made
    }

    /// `kglCreateFont` 0x1400198b0.
    fn create_font(&mut self, kgl: &Kgl, family: &str, italic: bool, bold: bool) -> Option<usize> {
        println!("KGL CREATE FONT {family} with locale {}", String::from_utf16_lossy(&self.locale));
        // with a locale set, any family with "Segoe UI" in its name becomes "Segoe"
        let name = if !self.locale.is_empty() && family.contains("Segoe UI") { "Segoe" } else { family };
        let name: Vec<u16> = name.encode_utf16().collect();
        let Some(compile) = self.d3d_compile else {
            println!("ERROR: pFW1Factory->CreateFontWrapper failed");
            return None;
        };
        match FontWrapper::new(kgl, &crate::fw1_atlas::font_key(family, italic, bold), compile, &self.collection, &name, if bold { 700 } else { 400 }, if italic { 2 } else { 0 }, &self.locale) {
            Some(wrapper) => {
                self.wrappers.push(wrapper);
                Some(self.wrappers.len() - 1)
            }
            None => {
                println!("ERROR: pFW1Factory->CreateFontWrapper failed");
                None
            }
        }
    }

    /// `kglBlitStringWide` 0x140019aa0: `align` 0 left, 1 centre, 2 right of (x, y); `color` is
    /// 0xAABBGGRR.
    pub fn blit_string(&mut self, kgl: &Kgl, wrapper_index: usize, x: f32, y: f32, text: &[u16], size: f32, align: u32, color: u32) {
        let flags = crate::fw1_atlas::blit_flags(align);
        if let Some(wrapper) = self.wrappers.get_mut(wrapper_index) {
            crate::fw1_atlas::log_draw_string(wrapper_index, x, y, text, size, flags, color);
            wrapper.draw_string(&kgl.context, text, size, x, y, color, flags);
        }
    }
}
