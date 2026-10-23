// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Textures: `KGLTexture::KGLTexture` 0x140023820 (from a file) / 0x140023970 (from memory),
//! `Texture`, `ResourceStore`.
//!
//! The game has no image reader of its own: every texture goes through D3DX11
//! (`d3dx11_43.dll`, the June 2010 DirectX runtime the game installs), which also builds the
//! missing mip levels and converts png / jpg / bmp. To get the very same texels this module
//! calls the player's installed `d3dx11_43.dll` with the game's arguments. Without that DLL
//! [`crate::texture_fallback`] reads dds files itself and decodes the other formats with WIC;
//! where D3DX would have filtered (generated mips, resized or converted images) the fallback's
//! texels are close but not the same bits.

use rustyac_content::vfs::PathExt;
use std::collections::HashMap;
#[cfg(not(feature = "backend-wgpu"))]
use std::ffi::c_void;
use std::path::Path;

#[cfg(not(feature = "backend-wgpu"))]
use windows::core::{s, w, Interface};
#[cfg(not(feature = "backend-wgpu"))]
use windows::Win32::Graphics::Direct3D11::*;
#[cfg(not(feature = "backend-wgpu"))]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::kgl::{Format, Kgl, View};

/// `D3DX11_IMAGE_INFO`.
#[cfg(not(feature = "backend-wgpu"))]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct D3dxImageInfo {
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    mip_levels: u32,
    misc_flags: u32,
    format: i32,
    resource_dimension: i32,
    image_file_format: i32,
}

/// `D3DX11_IMAGE_LOAD_INFO`.
#[cfg(not(feature = "backend-wgpu"))]
#[repr(C)]
struct D3dxImageLoadInfo {
    width: u32,
    height: u32,
    depth: u32,
    first_mip_level: u32,
    mip_levels: u32,
    usage: i32,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
    format: i32,
    filter: u32,
    mip_filter: u32,
    src_info: *mut D3dxImageInfo,
}

#[cfg(not(feature = "backend-wgpu"))]
type GetImageInfoFromMemory = unsafe extern "system" fn(*const c_void, usize, *mut c_void, *mut D3dxImageInfo, *mut i32) -> i32;
#[cfg(not(feature = "backend-wgpu"))]
type CreateSrvFromFile = unsafe extern "system" fn(*mut c_void, *const u16, *const D3dxImageLoadInfo, *mut c_void, *mut *mut c_void, *mut i32) -> i32;
#[cfg(not(feature = "backend-wgpu"))]
type CreateSrvFromMemory = unsafe extern "system" fn(*mut c_void, *const c_void, usize, *const D3dxImageLoadInfo, *mut c_void, *mut *mut c_void, *mut i32) -> i32;

/// The two D3DX11 functions the game's texture constructor calls.
#[cfg(not(feature = "backend-wgpu"))]
#[derive(Clone, Copy)]
struct D3dx {
    get_image_info_from_memory: GetImageInfoFromMemory,
    create_srv_from_memory: CreateSrvFromMemory,
    create_srv_from_file: CreateSrvFromFile,
}

#[cfg(not(feature = "backend-wgpu"))]
fn d3dx() -> Option<D3dx> {
    static D3DX: std::sync::OnceLock<Option<D3dx>> = std::sync::OnceLock::new();
    *D3DX.get_or_init(|| unsafe {
        let module = LoadLibraryW(w!("d3dx11_43.dll")).ok()?;
        let info = GetProcAddress(module, s!("D3DX11GetImageInfoFromMemory"))?;
        let create = GetProcAddress(module, s!("D3DX11CreateShaderResourceViewFromMemory"))?;
        let from_file = GetProcAddress(module, s!("D3DX11CreateShaderResourceViewFromFileW"))?;
        Some(D3dx { get_image_info_from_memory: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetImageInfoFromMemory>(info), create_srv_from_memory: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateSrvFromMemory>(create), create_srv_from_file: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateSrvFromFile>(from_file) })
    })
}

/// Whether the player's `d3dx11_43.dll` is used (else the fallback reader).
#[cfg(not(feature = "backend-wgpu"))]
pub fn d3dx_available() -> bool {
    d3dx().is_some() && !FORCE_FALLBACK.load(std::sync::atomic::Ordering::Relaxed)
}

/// kgl on wgpu has no D3DX: every texture goes through the fallback reader.
#[cfg(feature = "backend-wgpu")]
pub fn d3dx_available() -> bool {
    false
}

static FORCE_FALLBACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Use the fallback reader even when D3DX is installed (to measure the difference).
pub fn force_fallback(on: bool) {
    FORCE_FALLBACK.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// `KGLTexture` (0x40 bytes): the view first, then the file name and the size.
pub struct KglTexture {
    pub shader_resource_view: Option<View>,
    pub width: i32,
    pub height: i32,
}

#[cfg(feature = "backend-wgpu")]
impl KglTexture {
    /// `KGLTexture::KGLTexture(void*, int)` 0x140023970 with the fallback reader.
    pub fn from_memory(kgl: &Kgl, bytes: &[u8]) -> KglTexture {
        KglTexture::from_file_bytes(kgl, bytes)
    }

    pub fn from_file_bytes(kgl: &Kgl, bytes: &[u8]) -> KglTexture {
        let mut texture = KglTexture { shader_resource_view: None, width: 0, height: 0 };
        match crate::texture_fallback::create(kgl, bytes) {
            Ok(view) => texture.shader_resource_view = Some(view),
            Err(message) => println!("ERROR: texture not readable without d3dx11_43.dll: {message}"),
        }
        texture.init_size();
        texture
    }

    /// `KGLTexture::KGLTexture(const std::wstring&)` 0x140023820 with the fallback reader.
    pub fn from_file(kgl: &Kgl, path: &Path) -> KglTexture {
        match rustyac_content::vfs::read(path) {
            Ok(bytes) => KglTexture::from_file_bytes(kgl, &bytes),
            Err(e) => {
                println!("ERROR: texture {} not readable without d3dx11_43.dll: {e}", path.display());
                KglTexture { shader_resource_view: None, width: 0, height: 0 }
            }
        }
    }

    /// `KGLTexture::initSize` 0x140023c10.
    fn init_size(&mut self) {
        if let Some((w, h, _, _)) = self.describe() {
            self.width = w as i32;
            self.height = h as i32;
        }
    }

    /// The size, mip count and format of the texture behind the view.
    pub fn describe(&self) -> Option<(u32, u32, u32, Format)> {
        // shortcut: no caller reads the format, so it is left empty here
        self.shader_resource_view.as_ref().map(Kgl::describe_view).map(|(w, h, m)| (w, h, m, Format::default()))
    }
}

#[cfg(not(feature = "backend-wgpu"))]
impl KglTexture {
    /// `KGLTexture::KGLTexture(void*, int)` 0x140023970: the image's own format, a full mip
    /// chain (missing levels made with the triangle filter), immutable.
    pub fn from_memory(kgl: &Kgl, bytes: &[u8]) -> KglTexture {
        let mut texture = KglTexture { shader_resource_view: None, width: 0, height: 0 };
        match d3dx() {
            Some(d3dx) if d3dx_available() => unsafe {
                let mut info = D3dxImageInfo::default();
                (d3dx.get_image_info_from_memory)(bytes.as_ptr() as *const c_void, bytes.len(), std::ptr::null_mut(), &mut info, std::ptr::null_mut());
                const DEFAULT: u32 = 0xffff_ffff; // D3DX11_DEFAULT
                let load = D3dxImageLoadInfo {
                    width: DEFAULT,
                    height: DEFAULT,
                    depth: DEFAULT,
                    first_mip_level: DEFAULT,
                    mip_levels: DEFAULT,
                    usage: D3D11_USAGE_IMMUTABLE.0,
                    bind_flags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    cpu_access_flags: 0,
                    misc_flags: 0,
                    format: info.format,
                    filter: DEFAULT,
                    mip_filter: 4, // D3DX11_FILTER_TRIANGLE
                    src_info: std::ptr::null_mut(),
                };
                let mut view: *mut c_void = std::ptr::null_mut();
                let result = (d3dx.create_srv_from_memory)(kgl.device.as_raw(), bytes.as_ptr() as *const c_void, bytes.len(), &load, std::ptr::null_mut(), &mut view, std::ptr::null_mut());
                if result < 0 || view.is_null() {
                    println!("ERROR: D3DX11CreateShaderResourceViewFromMemory failed?");
                } else {
                    texture.shader_resource_view = Some(ID3D11ShaderResourceView::from_raw(view));
                }
            },
            _ => match crate::texture_fallback::create(kgl, bytes) {
                Ok(view) => texture.shader_resource_view = Some(view),
                Err(message) => println!("ERROR: texture not readable without d3dx11_43.dll: {message}"),
            },
        }
        texture.init_size();
        texture
    }

    /// What [`KglTexture::from_file`] makes of a file whose bytes are these: D3DX with no load
    /// info (usage DEFAULT, D3DX's default mip filter), from memory.
    pub fn from_file_bytes(kgl: &Kgl, bytes: &[u8]) -> KglTexture {
        let mut texture = KglTexture { shader_resource_view: None, width: 0, height: 0 };
        match d3dx() {
            Some(d3dx) if d3dx_available() => unsafe {
                let mut view: *mut c_void = std::ptr::null_mut();
                let result = (d3dx.create_srv_from_memory)(kgl.device.as_raw(), bytes.as_ptr() as *const c_void, bytes.len(), std::ptr::null(), std::ptr::null_mut(), &mut view, std::ptr::null_mut());
                if result < 0 || view.is_null() {
                    println!("ERROR: D3DX11CreateShaderResourceViewFromMemory failed?");
                } else {
                    texture.shader_resource_view = Some(ID3D11ShaderResourceView::from_raw(view));
                }
            },
            _ => match crate::texture_fallback::create(kgl, bytes) {
                Ok(view) => texture.shader_resource_view = Some(view),
                Err(message) => println!("ERROR: texture not readable without d3dx11_43.dll: {message}"),
            },
        }
        texture.init_size();
        texture
    }

    /// `KGLTexture::KGLTexture(const std::wstring&)` 0x140023820: D3DX reads the file with all
    /// its defaults (no load info: usage DEFAULT, D3DX's default mip filter).
    pub fn from_file(kgl: &Kgl, path: &Path) -> KglTexture {
        let mut texture = KglTexture { shader_resource_view: None, width: 0, height: 0 };
        match d3dx() {
            Some(d3dx) if d3dx_available() => unsafe {
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let mut view: *mut c_void = std::ptr::null_mut();
                let result = (d3dx.create_srv_from_file)(kgl.device.as_raw(), wide.as_ptr(), std::ptr::null(), std::ptr::null_mut(), &mut view, std::ptr::null_mut());
                if result < 0 || view.is_null() {
                    println!("ERROR: D3DX11CreateShaderResourceViewFromFile failed for {}", path.display());
                } else {
                    texture.shader_resource_view = Some(ID3D11ShaderResourceView::from_raw(view));
                }
            },
            _ => match std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| crate::texture_fallback::create(kgl, &bytes)) {
                Ok(view) => texture.shader_resource_view = Some(view),
                Err(message) => println!("ERROR: texture {} not readable without d3dx11_43.dll: {message}", path.display()),
            },
        }
        texture.init_size();
        texture
    }

    /// `KGLTexture::initSize` 0x140023c10.
    fn init_size(&mut self) {
        let Some(view) = &self.shader_resource_view else {
            return;
        };
        unsafe {
            if let Ok(resource) = view.GetResource() {
                if let Ok(texture) = resource.cast::<ID3D11Texture2D>() {
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    texture.GetDesc(&mut desc);
                    self.width = desc.Width as i32;
                    self.height = desc.Height as i32;
                }
            }
        }
    }

    /// The format and mip count of the texture behind the view (for surveys and tests).
    pub fn describe(&self) -> Option<(u32, u32, u32, Format)> {
        let view = self.shader_resource_view.as_ref()?;
        unsafe {
            let texture = view.GetResource().ok()?.cast::<ID3D11Texture2D>().ok()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            Some((desc.Width, desc.Height, desc.MipLevels, desc.Format))
        }
    }
}

/// What the game's `KGLTexture*` is to the texture cache of the render state: an index into
/// [`ResourceStore::textures`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

/// `Texture` (0x28 bytes): a kgl texture and the name it was asked for.
#[derive(Clone, Debug, Default)]
pub struct Texture {
    pub kid: Option<TextureId>,
    pub file_name: String,
}

/// `ResourceStore` (0x20 bytes): the process-wide cache of textures by name. The name is
/// compared exactly: other slashes or another case make another texture.
pub struct ResourceStore {
    pub textures: Vec<KglTexture>,
    by_name: HashMap<String, Texture>,
}

impl Default for ResourceStore {
    fn default() -> ResourceStore {
        ResourceStore::new()
    }
}

impl ResourceStore {
    pub fn new() -> ResourceStore {
        ResourceStore { textures: Vec::new(), by_name: HashMap::new() }
    }

    pub fn view(&self, id: TextureId) -> Option<View> {
        self.textures[id.0 as usize].shader_resource_view.clone()
    }

    /// A texture D3DX could not make is kept all the same, with no view (the game does so).
    fn add(&mut self, texture: KglTexture) -> Option<TextureId> {
        if let Some(k) = crate::kgl::view_key(&texture.shader_resource_view) {
            crate::kgl_log::new_name(k, "tex");
        }
        self.textures.push(texture);
        Some(TextureId(self.textures.len() as u32 - 1))
    }

    /// `Texture::Texture(RenderTarget*)` 0x1401fde40 (`kglCreateTextureFromRT` 0x140018a90):
    /// a texture that is a render target's picture.
    pub fn texture_from_render_target(&mut self, target: &crate::kgl::KglRenderTarget) -> Texture {
        self.textures.push(KglTexture { shader_resource_view: target.shader_resource_view.clone(), width: target.width, height: target.height });
        Texture { kid: Some(TextureId(self.textures.len() as u32 - 1)), file_name: String::new() }
    }

    /// `kglTextureGetWidth` / `kglTextureGetHeight` of a texture of the store.
    pub fn size(&self, texture: &Texture) -> Option<(u32, u32)> {
        let id = texture.kid?;
        self.textures[id.0 as usize].describe().map(|(w, h, _, _)| (w, h))
    }

    /// `ResourceStore::hasTexture` 0x1402003c0.
    pub fn has_texture(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// `ResourceStore::getTexture` 0x1402000d0: by file name, read from disk the first time.
    pub fn get_texture(&mut self, kgl: &Kgl, name: &str) -> Texture {
        if let Some(found) = self.by_name.get(name) {
            return found.clone();
        }
        let path = Path::new(name);
        if !path.vfs_is_file() {
            return Texture { kid: None, file_name: String::new() };
        }
        let kid = self.add(KglTexture::from_file(kgl, path));
        let texture = Texture { kid, file_name: name.to_string() };
        self.by_name.insert(name.to_string(), texture.clone());
        texture
    }

    /// A file the game has just written itself (a track's `map.png`): kept under its name as
    /// `getTexture` would have read it from disk.
    pub fn add_file_texture(&mut self, kgl: &Kgl, name: &str, bytes: &[u8]) -> Texture {
        let kid = self.add(KglTexture::from_file_bytes(kgl, bytes));
        let texture = Texture { kid, file_name: name.to_string() };
        self.by_name.insert(name.to_string(), texture.clone());
        texture
    }

    /// `ResourceStore::getTextureFromBuffer` 0x140200250: by name, made from the bytes the
    /// first time.
    pub fn get_texture_from_buffer(&mut self, kgl: &Kgl, name: &str, bytes: &[u8]) -> Texture {
        if let Some(found) = self.by_name.get(name) {
            return found.clone();
        }
        let kid = self.add(KglTexture::from_memory(kgl, bytes));
        let texture = Texture { kid, file_name: name.to_string() };
        self.by_name.insert(name.to_string(), texture.clone());
        texture
    }
}
