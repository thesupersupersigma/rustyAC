// SPDX-License-Identifier: GPL-3.0-or-later

//! Textures without `d3dx11_43.dll`: an own reader for dds files and Windows' image decoders
//! (WIC) for png / jpg / bmp.
//!
//! What it gives, held against D3DX with the game's arguments:
//! * block-compressed dds (DXT1 / DXT3 / DXT5, ATI1 / ATI2, DX10 headers): the levels stored
//!   in the file are uploaded as they are, which are D3DX's own bytes for every level of 4
//!   texels or more. Levels the file does not have are not made (D3DX builds them with its
//!   triangle filter), so such a texture has fewer mip levels here.
//! * uncompressed dds and png / jpg / bmp: converted to R8G8B8A8, level 0 like D3DX for
//!   8-bit channels; the smaller levels are made with a 2x2 box filter where D3DX uses its
//!   triangle (kn5 textures) or linear (loose files) filter: close, but not the same bits.
//!
//! So with the fallback the picture is not bit-identical to the game's.

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

use crate::kgl::Kgl;

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// A texture from the bytes of an image file, read without D3DX.
pub fn create(kgl: &Kgl, bytes: &[u8]) -> Result<ID3D11ShaderResourceView, String> {
    if bytes.len() >= 128 && &bytes[0..4] == b"DDS " {
        dds(kgl, bytes)
    } else {
        let (width, height, rgba) = decode_with_wic(bytes)?;
        rgba_with_mips(kgl, width, height, rgba)
    }
}

fn upload(kgl: &Kgl, width: u32, height: u32, format: DXGI_FORMAT, levels: &[(Vec<u8>, u32)]) -> Result<ID3D11ShaderResourceView, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: levels.len() as u32,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let init: Vec<D3D11_SUBRESOURCE_DATA> = levels.iter().map(|(data, pitch)| D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr().cast(), SysMemPitch: *pitch, SysMemSlicePitch: 0 }).collect();
    unsafe {
        let mut texture = None;
        kgl.device.CreateTexture2D(&desc, Some(init.as_ptr()), Some(&mut texture)).map_err(|e| format!("CreateTexture2D: {e}"))?;
        let texture = texture.ok_or("no texture")?;
        let mut view = None;
        kgl.device.CreateShaderResourceView(&texture, None, Some(&mut view)).map_err(|e| format!("CreateShaderResourceView: {e}"))?;
        view.ok_or_else(|| "no view".to_string())
    }
}

/// R8G8B8A8 with a full mip chain, each level the 2x2 box average of the one above.
fn rgba_with_mips(kgl: &Kgl, width: u32, height: u32, rgba: Vec<u8>) -> Result<ID3D11ShaderResourceView, String> {
    let mut levels = vec![(rgba, width * 4)];
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let above = &levels.last().unwrap().0;
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, y0) = ((x * 2).min(w - 1), (y * 2).min(h - 1));
                let (x1, y1) = ((x * 2 + 1).min(w - 1), (y * 2 + 1).min(h - 1));
                for c in 0..4 {
                    let at = |px: u32, py: u32| above[((py * w + px) * 4 + c) as usize] as u32;
                    next[((y * nw + x) * 4 + c) as usize] = ((at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1) + 2) / 4) as u8;
                }
            }
        }
        levels.push((next, nw * 4));
        w = nw;
        h = nh;
    }
    upload(kgl, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, &levels)
}

fn dds(kgl: &Kgl, bytes: &[u8]) -> Result<ID3D11ShaderResourceView, String> {
    let height = u32_at(bytes, 12);
    let width = u32_at(bytes, 16);
    let mip_count = u32_at(bytes, 28).max(1);
    let pf_flags = u32_at(bytes, 80);
    let four_cc = &bytes[84..88];
    let bit_count = u32_at(bytes, 88);
    let masks = [u32_at(bytes, 92), u32_at(bytes, 96), u32_at(bytes, 100), u32_at(bytes, 104)];
    let caps2 = u32_at(bytes, 112);
    if caps2 & 0x0020_0200 != 0 {
        return Err("cube and volume dds files are not read without D3DX".into());
    }
    let mut data = 128usize;
    const FOURCC: u32 = 0x4;
    let block_format = if pf_flags & FOURCC != 0 {
        match four_cc {
            b"DXT1" => Some((DXGI_FORMAT_BC1_UNORM, 8u32)),
            b"DXT2" | b"DXT3" => Some((DXGI_FORMAT_BC2_UNORM, 16)),
            b"DXT4" | b"DXT5" => Some((DXGI_FORMAT_BC3_UNORM, 16)),
            b"ATI1" | b"BC4U" => Some((DXGI_FORMAT_BC4_UNORM, 8)),
            b"ATI2" | b"BC5U" => Some((DXGI_FORMAT_BC5_UNORM, 16)),
            b"DX10" => {
                if bytes.len() < 148 {
                    return Err("short DX10 dds header".into());
                }
                data = 148;
                let format = DXGI_FORMAT(u32_at(bytes, 128) as i32);
                match format.0 {
                    70..=72 => Some((format, 8)),
                    73..=78 => Some((format, 16)),
                    79..=81 => Some((format, 8)),
                    82..=84 | 94..=99 => Some((format, 16)),
                    28 | 29 => None,
                    other => return Err(format!("dds format {other} is not read without D3DX")),
                }
            }
            other => return Err(format!("dds FourCC {:?} is not read without D3DX", String::from_utf8_lossy(other))),
        }
    } else {
        None
    };
    if let Some((format, block_bytes)) = block_format {
        let mut levels = Vec::new();
        let (mut w, mut h) = (width, height);
        for _ in 0..mip_count {
            let (bw, bh) = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
            let size = (bw * bh * block_bytes) as usize;
            if data + size > bytes.len() {
                break;
            }
            levels.push((bytes[data..data + size].to_vec(), bw * block_bytes));
            data += size;
            if w == 1 && h == 1 {
                break;
            }
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        if levels.is_empty() {
            return Err("dds file without pixel data".into());
        }
        return upload(kgl, width, height, format, &levels);
    }
    // uncompressed: level 0 to R8G8B8A8 through the channel masks
    let bytes_per_pixel = if pf_flags & FOURCC != 0 { 4 } else { (bit_count / 8) as usize };
    if !(1..=4).contains(&bytes_per_pixel) {
        return Err(format!("dds with {bit_count} bits per pixel is not read without D3DX"));
    }
    let masks = if pf_flags & FOURCC != 0 { [0xff, 0xff00, 0xff_0000, 0xff00_0000] } else { masks };
    let size = width as usize * height as usize * bytes_per_pixel;
    if data + size > bytes.len() {
        return Err("dds file shorter than its header says".into());
    }
    let luminance = pf_flags & 0x2_0000 != 0;
    let has_alpha = pf_flags & 0x1 != 0 || (pf_flags & 0x2 != 0);
    let channel = |pixel: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 0;
        }
        let shift = mask.trailing_zeros();
        let max = mask >> shift;
        let value = (pixel & mask) >> shift;
        ((value as f32 / max as f32) * 255.0 + 0.5) as u8
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for p in bytes[data..data + size].chunks_exact(bytes_per_pixel) {
        let mut pixel = 0u32;
        for (i, b) in p.iter().enumerate() {
            pixel |= (*b as u32) << (8 * i);
        }
        let r = channel(pixel, masks[0]);
        let (g, b) = if luminance { (r, r) } else { (channel(pixel, masks[1]), channel(pixel, masks[2])) };
        let a = if has_alpha && masks[3] != 0 { channel(pixel, masks[3]) } else { 255 };
        rgba.extend([r, g, b, a]);
    }
    rgba_with_mips(kgl, width, height, rgba)
}

/// png / jpg / bmp through the Windows Imaging Component, as straight 8-bit RGBA.
fn decode_with_wic(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("WIC: {e}"))?;
        let stream = factory.CreateStream().map_err(|e| format!("WIC stream: {e}"))?;
        stream.InitializeFromMemory(bytes).map_err(|e| format!("WIC stream: {e}"))?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).map_err(|e| format!("not an image WIC can read: {e}"))?;
        let frame = decoder.GetFrame(0).map_err(|e| format!("WIC frame: {e}"))?;
        let converter = factory.CreateFormatConverter().map_err(|e| format!("WIC converter: {e}"))?;
        converter.Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).map_err(|e| format!("WIC convert: {e}"))?;
        let (mut width, mut height) = (0u32, 0u32);
        converter.GetSize(&mut width, &mut height).map_err(|e| format!("WIC size: {e}"))?;
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        let source: IWICBitmapSource = converter.cast().map_err(|e| format!("WIC: {e}"))?;
        source.CopyPixels(std::ptr::null(), width * 4, &mut rgba).map_err(|e| format!("WIC pixels: {e}"))?;
        Ok((width, height, rgba))
    }
}
