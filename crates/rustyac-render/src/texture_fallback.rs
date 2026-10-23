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

#[cfg(all(windows, any(not(feature = "backend-wgpu"), test)))]
use windows::core::Interface;
#[cfg(not(feature = "backend-wgpu"))]
use windows::Win32::Graphics::Direct3D11::*;
#[cfg(not(feature = "backend-wgpu"))]
use windows::Win32::Graphics::Dxgi::Common::*;
#[cfg(all(windows, any(not(feature = "backend-wgpu"), test)))]
use windows::Win32::Graphics::Imaging::*;
#[cfg(all(windows, any(not(feature = "backend-wgpu"), test)))]
use windows::Win32::System::Com::*;

#[cfg(feature = "backend-wgpu")]
use crate::kgl::dxgi::*;
use crate::kgl::{Kgl, View};

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// A texture from the bytes of an image file, read without D3DX.
pub fn create(kgl: &Kgl, bytes: &[u8]) -> Result<View, String> {
    if bytes.len() >= 128 && &bytes[0..4] == b"DDS " {
        dds(kgl, bytes)
    } else {
        #[cfg(feature = "backend-wgpu")]
        let (width, height, rgba) = decode_image(bytes)?;
        #[cfg(not(feature = "backend-wgpu"))]
        let (width, height, rgba) = decode_with_wic(bytes)?;
        rgba_with_mips(kgl, width, height, rgba)
    }
}

#[cfg(feature = "backend-wgpu")]
fn upload(kgl: &Kgl, width: u32, height: u32, format: DXGI_FORMAT, levels: &[(Vec<u8>, u32)]) -> Result<View, String> {
    kgl.create_texture(format, width, height, levels)
}

#[cfg(not(feature = "backend-wgpu"))]
fn upload(kgl: &Kgl, width: u32, height: u32, format: DXGI_FORMAT, levels: &[(Vec<u8>, u32)]) -> Result<View, String> {
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
fn rgba_with_mips(kgl: &Kgl, width: u32, height: u32, rgba: Vec<u8>) -> Result<View, String> {
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

fn dds(kgl: &Kgl, bytes: &[u8]) -> Result<View, String> {
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

/// png / jpg / bmp without WIC (kgl on wgpu, also in the browser), as straight 8-bit RGBA.
#[cfg(feature = "backend-wgpu")]
fn decode_image(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        decode_jpg(bytes)
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else {
        decode_png(bytes)
    }
}

/// jpg through the jpeg-decoder crate. Its IDCT and chroma upsampling are not WIC's: the
/// texels can differ by a few steps (the test below measures it).
#[cfg(feature = "backend-wgpu")]
fn decode_jpg(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    let pixels = decoder.decode().map_err(|e| format!("not a jpg: {e}"))?;
    let info = decoder.info().ok_or("jpg without a header")?;
    let rgba = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], 255]).collect(),
        // shortcut: CMYK as 255 - (c + k) per channel, not WIC's colour handling; no AC texture seen is CMYK
        jpeg_decoder::PixelFormat::CMYK32 => pixels.chunks_exact(4).flat_map(|p| [255 - p[0].saturating_add(p[3]), 255 - p[1].saturating_add(p[3]), 255 - p[2].saturating_add(p[3]), 255]).collect(),
    };
    Ok((info.width as u32, info.height as u32, rgba))
}

/// Uncompressed bmp (1 / 4 / 8 bits through the palette, 16 / 24 / 32 bits, bit fields), as
/// WIC reads it: alpha only where the header has an alpha mask.
#[cfg(feature = "backend-wgpu")]
fn decode_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let short = || "bmp file shorter than its header says".to_string();
    if bytes.len() < 54 {
        return Err(short());
    }
    let offset = u32_at(bytes, 10) as usize;
    let header = u32_at(bytes, 14) as usize;
    let width = u32_at(bytes, 18) as i32;
    let height = u32_at(bytes, 22) as i32;
    let bits = u16::from_le_bytes([bytes[28], bytes[29]]) as usize;
    let compression = u32_at(bytes, 30);
    if width <= 0 || height == 0 || header < 40 {
        return Err(format!("bmp of {width} x {height} with a {header}-byte header is not read"));
    }
    if compression != 0 && compression != 3 {
        return Err(format!("compressed bmp (type {compression}) is not read"));
    }
    if ![1, 4, 8, 16, 24, 32].contains(&bits) {
        return Err(format!("bmp with {bits} bits per pixel is not read"));
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    // bit fields: right after a 40-byte header, else inside the longer header (alpha from 56 bytes on)
    let mask = |i: usize| if 54 + 4 * i + 4 <= bytes.len() { u32_at(bytes, 54 + 4 * i) } else { 0 };
    let masks = match (compression, bits) {
        (3, _) => [mask(0), mask(1), mask(2), if header >= 56 { mask(3) } else { 0 }],
        (_, 16) => [0x7c00, 0x3e0, 0x1f, 0],
        _ => [0xff_0000, 0xff00, 0xff, 0],
    };
    let colours = match u32_at(bytes, 46) as usize {
        0 => 1usize << bits.min(8),
        n => n,
    };
    let palette = 14 + header + if compression == 3 && header == 40 { 12 } else { 0 };
    let stride = (w * bits).div_ceil(32) * 4;
    if offset + stride * h > bytes.len() || (bits <= 8 && palette + 4 * colours > bytes.len()) {
        return Err(short());
    }
    let channel = |pixel: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let max = (mask >> shift) as u64;
        (((pixel & mask) >> shift) as u64 * 255 / max) as u8
    };
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        // rows are stored bottom-up unless the height is negative
        let row = &bytes[offset + stride * if height > 0 { h - 1 - y } else { y }..];
        for x in 0..w {
            let out = &mut rgba[(y * w + x) * 4..][..4];
            if bits <= 8 {
                let bit = x * bits;
                let index = ((row[bit / 8] as usize) >> (8 - bits - bit % 8)) & ((1 << bits) - 1);
                let at = palette + 4 * index.min(colours - 1);
                out.copy_from_slice(&[bytes[at + 2], bytes[at + 1], bytes[at], 255]);
            } else {
                let n = bits / 8;
                let pixel = row[x * n..][..n].iter().rev().fold(0u32, |v, &b| v << 8 | b as u32);
                out.copy_from_slice(&[channel(pixel, masks[0]), channel(pixel, masks[1]), channel(pixel, masks[2]), channel(pixel, masks[3])]);
            }
        }
    }
    Ok((w as u32, h as u32, rgba))
}

/// png through the png crate.
#[cfg(feature = "backend-wgpu")]
fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("not a png: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => buf[..n * 3].chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..n * 2].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => buf[..n].iter().flat_map(|&g| [g, g, g, 255]).collect(),
    };
    Ok((w, h, rgba))
}

/// png / jpg / bmp through the Windows Imaging Component, as straight 8-bit RGBA.
#[cfg(all(windows, any(not(feature = "backend-wgpu"), test)))]
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

/// kgl on wgpu's readers against WIC (what the Direct3D 11 path without D3DX reads) on the
/// install's own png, jpg and bmp files.
#[cfg(all(test, windows, feature = "backend-wgpu"))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn is(path: &Path, ext: &str) -> bool {
        path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
    }

    fn files(folder: &Path, ext: &str, n: usize, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            if out.iter().filter(|p| is(p, ext)).count() >= n {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                files(&path, ext, n, out);
            } else if is(&path, ext) {
                out.push(path);
            }
        }
    }

    #[test]
    fn same_texels_as_wic() {
        let game = PathBuf::from(std::env::var("AC_ROOT").unwrap_or_else(|_| r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa".to_string()));
        if !game.join("content").is_dir() {
            eprintln!("no Assetto Corsa install at {}: skipped", game.display());
            return;
        }
        let mut found = Vec::new();
        for (folder, ext, n) in [("content/texture", "jpg", 8), ("content/fonts", "bmp", 3), ("content/cars", "bmp", 6), ("content/texture", "png", 4)] {
            files(&game.join(folder), ext, n, &mut found);
        }
        assert!(found.iter().any(|p| is(p, "bmp")) && found.iter().any(|p| is(p, "jpg")), "no bmp or jpg found");
        for path in &found {
            let bytes = std::fs::read(path).unwrap();
            let Ok((ww, wh, wic)) = decode_with_wic(&bytes) else { continue };
            let (w, h, mine) = decode_image(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!((w, h), (ww, wh), "{}", path.display());
            let worst = wic.iter().zip(&mine).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            let differ = wic.iter().zip(&mine).filter(|(a, b)| a != b).count();
            println!("{}: {w} x {h}, {differ} of {} bytes differ, largest difference {worst}", path.display(), wic.len());
            assert!(if is(path, "jpg") { worst <= 4 } else { worst == 0 }, "{}: largest difference {worst}", path.display());
        }
    }
}
