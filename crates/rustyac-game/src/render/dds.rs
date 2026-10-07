// SPDX-License-Identifier: GPL-3.0-or-later

//! Just enough of the DDS image format (and PNG, through the `png` crate) to put a kn5's
//! textures on the graphics card: block-compressed and 32/24-bit images with their mip chains.
//! Cube maps and volume textures are refused (the debug view has no use for them).

use std::ops::Range;

/// The formats the view hands to Direct3D, by their `DXGI_FORMAT` numbers.
pub const R8G8B8A8_UNORM: u32 = 28;
pub const BC1_UNORM: u32 = 71;
pub const BC2_UNORM: u32 = 74;
pub const BC3_UNORM: u32 = 77;
pub const BC4_UNORM: u32 = 80;
pub const BC5_UNORM: u32 = 83;
pub const BC7_UNORM: u32 = 98;
pub const B8G8R8A8_UNORM: u32 = 87;
pub const B8G8R8X8_UNORM: u32 = 88;

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    /// Bytes of one row (of blocks, for the compressed formats).
    pub pitch: u32,
    pub bytes: Range<usize>,
}

/// A decoded image file: one format, the mip levels from the largest down.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub format: u32,
    pub data: Vec<u8>,
    pub levels: Vec<Level>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Bytes of a block for the compressed formats, 0 for the others.
fn block_bytes(format: u32) -> u32 {
    match format {
        BC1_UNORM | BC4_UNORM => 8,
        BC2_UNORM | BC3_UNORM | BC5_UNORM | BC7_UNORM => 16,
        _ => 0,
    }
}

fn level_size(format: u32, width: u32, height: u32, source_bytes_per_pixel: u32) -> (u32, usize) {
    let block = block_bytes(format);
    if block != 0 {
        let pitch = width.div_ceil(4).max(1) * block;
        (pitch, pitch as usize * height.div_ceil(4).max(1) as usize)
    } else {
        let pitch = width * source_bytes_per_pixel;
        (pitch, pitch as usize * height as usize)
    }
}

/// Where the kept mip levels of a DDS file are, worked out from its header alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub format: u32,
    /// How the file's pixels are widened to four bytes, for the formats the card is not given as they are.
    expand: Option<Expand>,
    /// (width, height, row bytes in the file, offset, size in the file)
    kept: Vec<(u32, u32, u32, usize, usize)>,
}

/// An uncompressed pixel layout that is turned into R8G8B8A8: bytes per pixel and the bit
/// masks of red, green, blue and alpha (0: the channel is not stored).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Expand {
    bytes: u32,
    masks: [u32; 4],
    /// Grey: green and blue are the red channel.
    luminance: bool,
}

impl Expand {
    fn pixel(&self, source: &[u8]) -> [u8; 4] {
        let mut value = 0u32;
        for (k, byte) in source.iter().enumerate() {
            value |= (*byte as u32) << (8 * k);
        }
        let channel = |mask: u32, missing: u8| -> u8 {
            if mask == 0 {
                return missing;
            }
            let max = mask >> mask.trailing_zeros();
            ((((value & mask) >> mask.trailing_zeros()) as u64 * 255 + max as u64 / 2) / max as u64) as u8
        };
        let red = channel(self.masks[0], 0);
        let (green, blue) = if self.luminance { (red, red) } else { (channel(self.masks[1], 0), channel(self.masks[2], 0)) };
        [red, green, blue, channel(self.masks[3], 255)]
    }
}

impl Plan {
    /// Bytes the kept levels take once they are on the card.
    pub fn bytes(&self) -> usize {
        self.kept.iter().map(|&(w, h, _, _, size)| if self.expand.is_some() { w as usize * h as usize * 4 } else { size }).sum()
    }
}

/// Reads the header of a `.dds` file (`head`: at least its first 148 bytes, or all of a
/// shorter file) of `file_len` bytes. `max_size` drops the largest mip levels of an image
/// that has a mip chain until neither side is longer (0 = keep everything).
pub fn plan_dds(head: &[u8], file_len: usize, max_size: u32) -> Result<Plan, String> {
    let bytes = head;
    if bytes.len() < 128 || &bytes[..4] != b"DDS " || u32_at(bytes, 4) != 124 {
        return Err("not a DDS file".to_string());
    }
    let height = u32_at(bytes, 12);
    let width = u32_at(bytes, 16);
    // a level per halving of the longest side at most, whatever the header claims
    let mip_count = u32_at(bytes, 28).clamp(1, 15);
    let pf_flags = u32_at(bytes, 80);
    let four_cc = &bytes[84..88];
    let bit_count = u32_at(bytes, 88);
    let (r_mask, a_mask) = (u32_at(bytes, 92), u32_at(bytes, 104));
    let (g_mask, b_mask) = (u32_at(bytes, 96), u32_at(bytes, 100));
    // which of the file's channels exist: alpha only with its flag, grey by its flag or by having no green
    let alpha = if pf_flags & 0x1 != 0 { a_mask } else { 0 };
    let grey = pf_flags & 0x2_0000 != 0 || (g_mask == 0 && b_mask == 0);
    let generic = |bytes: u32| Some(Expand { bytes, masks: [r_mask, g_mask, b_mask, alpha], luminance: grey });
    let caps2 = u32_at(bytes, 112);
    if caps2 & 0x0020_fe00 != 0 {
        return Err("a cube map or volume texture".to_string());
    }
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(format!("a {width} x {height} image"));
    }
    let mut offset = 128;
    // (format handed to Direct3D, bytes per pixel in the file, how to widen them to 32 bits)
    let (format, source_bpp, expand) = if pf_flags & 0x4 != 0 {
        match four_cc {
            b"DXT1" => (BC1_UNORM, 0, None),
            b"DXT2" | b"DXT3" => (BC2_UNORM, 0, None),
            b"DXT4" | b"DXT5" => (BC3_UNORM, 0, None),
            b"ATI1" | b"BC4U" => (BC4_UNORM, 0, None),
            b"ATI2" | b"BC5U" => (BC5_UNORM, 0, None),
            b"DX10" => {
                if bytes.len() < 148 {
                    return Err("a cut-off DX10 header".to_string());
                }
                offset = 148;
                match u32_at(bytes, 128) {
                    70..=72 => (BC1_UNORM, 0, None),
                    73..=75 => (BC2_UNORM, 0, None),
                    76..=78 => (BC3_UNORM, 0, None),
                    79 | 80 => (BC4_UNORM, 0, None),
                    82 | 83 => (BC5_UNORM, 0, None),
                    97..=99 => (BC7_UNORM, 0, None),
                    27..=29 => (R8G8B8A8_UNORM, 4, None),
                    87 | 90 | 91 => (B8G8R8A8_UNORM, 4, None),
                    88 | 92 | 93 => (B8G8R8X8_UNORM, 4, None),
                    other => return Err(format!("DXGI format {other}")),
                }
            }
            other => return Err(format!("format {}", String::from_utf8_lossy(other))),
        }
    } else if bit_count == 32 {
        match (r_mask, g_mask, b_mask, alpha != 0) {
            (0x00ff_0000, 0x0000_ff00, 0x0000_00ff, true) => (B8G8R8A8_UNORM, 4, None),
            (0x00ff_0000, 0x0000_ff00, 0x0000_00ff, false) => (B8G8R8X8_UNORM, 4, None),
            (0x0000_00ff, 0x0000_ff00, 0x00ff_0000, true) => (R8G8B8A8_UNORM, 4, None),
            _ => (R8G8B8A8_UNORM, 4, generic(4)),
        }
    } else if bit_count == 24 || bit_count == 16 || bit_count == 8 {
        // 24-bit colour, 5:6:5 and its relatives, grey with or without alpha
        if r_mask == 0 && alpha == 0 {
            return Err(format!("{bit_count} bits per pixel without a colour mask"));
        }
        (R8G8B8A8_UNORM, bit_count / 8, generic(bit_count / 8))
    } else {
        return Err(format!("{bit_count} bits per pixel"));
    };

    let mut kept = Vec::new();
    let (mut w, mut h) = (width, height);
    for mip in 0..mip_count {
        let (pitch, size) = level_size(format, w, h, source_bpp);
        if offset + size > file_len {
            if kept.is_empty() && mip + 1 == mip_count {
                return Err("the image data is cut off".to_string());
            }
            break;
        }
        let last = mip + 1 == mip_count;
        let keep = max_size == 0 || (w <= max_size && h <= max_size) || last;
        // a block-compressed image must start on a size Direct3D takes (multiples of 4)
        let keep = keep && (block_bytes(format) == 0 || !kept.is_empty() || (w % 4 == 0 && h % 4 == 0) || last);
        if keep {
            kept.push((w, h, pitch, offset, size));
        }
        offset += size;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    if kept.is_empty() {
        return Err("no usable mip level".to_string());
    }
    Ok(Plan { format, expand, kept })
}

/// Reads a `.dds` file; see [`plan_dds`] for `max_size`.
pub fn parse_dds(bytes: &[u8], max_size: u32) -> Result<Image, String> {
    let plan = plan_dds(bytes, bytes.len(), max_size)?;
    let mut data = Vec::with_capacity(plan.bytes());
    let mut levels = Vec::new();
    for &(w, h, pitch, offset, size) in &plan.kept {
        let start = data.len();
        let source = &bytes[offset..offset + size];
        let pitch = match &plan.expand {
            None => {
                data.extend_from_slice(source);
                pitch
            }
            Some(expand) => {
                data.extend(source.chunks_exact(expand.bytes as usize).flat_map(|p| expand.pixel(p)));
                w * 4
            }
        };
        levels.push(Level { width: w, height: h, pitch, bytes: start..data.len() });
    }
    Ok(Image { format: plan.format, data, levels })
}

/// Reads a `.png` file into one RGBA level.
pub fn parse_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let pixels = &buffer[..info.buffer_size()];
    let data: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return Err("an indexed PNG".to_string()),
    };
    let level = Level { width: info.width, height: info.height, pitch: info.width * 4, bytes: 0..data.len() };
    Ok(Image { format: R8G8B8A8_UNORM, data, levels: vec![level] })
}

/// Whatever image file a kn5 holds.
pub fn parse_image(bytes: &[u8], max_size: u32) -> Result<Image, String> {
    if bytes.starts_with(b"DDS ") {
        parse_dds(bytes, max_size)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        parse_png(bytes)
    } else {
        Err("neither DDS nor PNG".to_string())
    }
}

impl Image {
    /// Bytes the image takes on the card.
    pub fn bytes(&self) -> usize {
        self.data.len()
    }

    /// One level without block compression: the card can make the smaller ones itself.
    pub fn wants_generated_mips(&self) -> bool {
        self.levels.len() == 1 && block_bytes(self.format) == 0 && self.levels[0].width.max(self.levels[0].height) > 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u32, height: u32, mips: u32, four_cc: &[u8; 4]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        b[4..8].copy_from_slice(&124u32.to_le_bytes());
        b[12..16].copy_from_slice(&height.to_le_bytes());
        b[16..20].copy_from_slice(&width.to_le_bytes());
        b[28..32].copy_from_slice(&mips.to_le_bytes());
        b[76..80].copy_from_slice(&32u32.to_le_bytes());
        b[80..84].copy_from_slice(&4u32.to_le_bytes());
        b[84..88].copy_from_slice(four_cc);
        b
    }

    #[test]
    fn a_dxt1_chain_is_split_into_levels_and_the_big_ones_can_be_dropped() {
        let mut file = header(16, 8, 5, b"DXT1");
        // 16x8: 4x2 blocks, 8x4: 2x1, 4x2: 1x1, 2x1: 1x1, 1x1: 1x1
        file.extend(std::iter::repeat_n(7u8, (8 + 2 + 1 + 1 + 1) * 8));
        let image = parse_dds(&file, 0).unwrap();
        assert_eq!(image.format, BC1_UNORM);
        assert_eq!(image.levels.len(), 5);
        assert_eq!((image.levels[0].width, image.levels[0].height, image.levels[0].pitch), (16, 8, 32));
        assert_eq!(image.levels[4].bytes.len(), 8);
        let small = parse_dds(&file, 8).unwrap();
        assert_eq!(small.levels.len(), 4);
        assert_eq!((small.levels[0].width, small.levels[0].height), (8, 4));
        assert_eq!(small.bytes(), (2 + 1 + 1 + 1) * 8);
    }

    #[test]
    fn sixteen_bit_pixels_are_widened_by_their_masks() {
        // 5:6:5, one pixel of pure red and one of pure green
        let mut file = header(2, 1, 1, b"\0\0\0\0");
        file[80..84].copy_from_slice(&0x40u32.to_le_bytes());
        file[88..92].copy_from_slice(&16u32.to_le_bytes());
        file[92..96].copy_from_slice(&0xf800u32.to_le_bytes());
        file[96..100].copy_from_slice(&0x07e0u32.to_le_bytes());
        file[100..104].copy_from_slice(&0x001fu32.to_le_bytes());
        file.extend_from_slice(&[0x00, 0xf8, 0xe0, 0x07]);
        let image = parse_dds(&file, 0).unwrap();
        assert_eq!(image.format, R8G8B8A8_UNORM);
        assert_eq!(image.data, [255, 0, 0, 255, 0, 255, 0, 255]);
        // grey with alpha: 8 bits each
        let mut file = header(1, 1, 1, b"\0\0\0\0");
        file[80..84].copy_from_slice(&0x2_0001u32.to_le_bytes());
        file[88..92].copy_from_slice(&16u32.to_le_bytes());
        file[92..96].copy_from_slice(&0x00ffu32.to_le_bytes());
        file[104..108].copy_from_slice(&0xff00u32.to_le_bytes());
        file.extend_from_slice(&[0x80, 0x40]);
        assert_eq!(parse_dds(&file, 0).unwrap().data, [0x80, 0x80, 0x80, 0x40]);
    }

    #[test]
    fn what_is_not_an_image_is_refused() {
        assert!(parse_image(b"hello", 0).is_err());
        assert!(parse_dds(&header(0, 8, 1, b"DXT1"), 0).is_err());
        assert!(parse_dds(&header(8, 8, 1, b"DXT1"), 0).is_err());
    }
}
