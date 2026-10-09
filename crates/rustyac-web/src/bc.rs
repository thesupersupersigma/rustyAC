// SPDX-License-Identifier: GPL-3.0-or-later

//! Textures for a card that may not take them as stored.
//!
//! AC's textures are mostly block compressed (DXT1 / DXT3 / DXT5 = BC1 / BC2 / BC3). WebGPU
//! and WebGL2 only take those where the device offers the feature (desktop GPUs do, many
//! phones and ARM Chromebooks do not). [`for_card`] unpacks them to RGBA for such a device,
//! builds the mip chain of an image that came with a single level, and sets the unused
//! alpha byte of the "X8" layout.

use rustyac_game::render::dds::{Image, Level, B8G8R8A8_UNORM, B8G8R8X8_UNORM, BC1_UNORM, BC2_UNORM, BC3_UNORM, R8G8B8A8_UNORM};

fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [((r * 527 + 23) >> 6) as u8, ((g * 259 + 33) >> 6) as u8, ((b * 527 + 23) >> 6) as u8]
}

/// The sixteen colours of a BC1 colour block (also the colour half of BC2 / BC3, where the
/// four-colour mode is always used).
fn color_block(block: &[u8], always_four: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |x: u8, y: u8, wx: u32, wy: u32, d: u32| ((x as u32 * wx + y as u32 * wy + d / 2) / d) as u8;
    let mut palette = [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [0; 4], [0; 4]];
    if c0 > c1 || always_four {
        palette[2] = [mix(a[0], b[0], 2, 1, 3), mix(a[1], b[1], 2, 1, 3), mix(a[2], b[2], 2, 1, 3), 255];
        palette[3] = [mix(a[0], b[0], 1, 2, 3), mix(a[1], b[1], 1, 2, 3), mix(a[2], b[2], 1, 2, 3), 255];
    } else {
        palette[2] = [mix(a[0], b[0], 1, 1, 2), mix(a[1], b[1], 1, 1, 2), mix(a[2], b[2], 1, 1, 2), 255];
        palette[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    std::array::from_fn(|i| palette[((bits >> (2 * i)) & 3) as usize])
}

/// The sixteen alphas of a BC3 alpha block.
fn alpha_block(block: &[u8]) -> [u8; 16] {
    let (a0, a1) = (block[0] as u32, block[1] as u32);
    let mut table = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for k in 1..7u32 {
            table[k as usize + 1] = ((7 - k) * a0 + k * a1 + 3) / 7;
        }
    } else {
        for k in 1..5u32 {
            table[k as usize + 1] = ((5 - k) * a0 + k * a1 + 2) / 5;
        }
        table[6] = 0;
        table[7] = 255;
    }
    let mut bits = 0u64;
    for (k, byte) in block[2..8].iter().enumerate() {
        bits |= (*byte as u64) << (8 * k);
    }
    std::array::from_fn(|i| table[((bits >> (3 * i)) & 7) as usize] as u8)
}

/// One level of a BC1 / BC2 / BC3 image as RGBA.
fn unpack_level(format: u32, data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let block_bytes = if format == BC1_UNORM { 8 } else { 16 };
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let blocks_wide = width.div_ceil(4).max(1) as usize;
    for (number, block) in data.chunks_exact(block_bytes).enumerate() {
        let (bx, by) = ((number % blocks_wide) as u32 * 4, (number / blocks_wide) as u32 * 4);
        if by >= height {
            break;
        }
        let pixels = match format {
            BC1_UNORM => color_block(block, false),
            BC2_UNORM => {
                let mut pixels = color_block(&block[8..], true);
                for (i, pixel) in pixels.iter_mut().enumerate() {
                    let nibble = (block[i / 2] >> (4 * (i % 2))) & 15;
                    pixel[3] = nibble * 17;
                }
                pixels
            }
            _ => {
                let mut pixels = color_block(&block[8..], true);
                for (pixel, alpha) in pixels.iter_mut().zip(alpha_block(block)) {
                    pixel[3] = alpha;
                }
                pixels
            }
        };
        for (i, pixel) in pixels.iter().enumerate() {
            let (x, y) = (bx + (i % 4) as u32, by + (i / 4) as u32);
            if x < width && y < height {
                let at = (y as usize * width as usize + x as usize) * 4;
                out[at..at + 4].copy_from_slice(pixel);
            }
        }
    }
    out
}

/// The next smaller level of an RGBA image (each pixel the mean of up to four).
fn halve(data: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = ((x * 2 + dx).min(width - 1), (y * 2 + dy).min(height - 1));
                let at = (sy as usize * width as usize + sx as usize) * 4;
                for k in 0..4 {
                    sum[k] += data[at + k] as u32;
                }
            }
            let at = (y as usize * w as usize + x as usize) * 4;
            for k in 0..4 {
                out[at + k] = ((sum[k] + 2) / 4) as u8;
            }
        }
    }
    (out, w, h)
}

/// An RGBA image from its largest level, with the whole mip chain.
fn with_mips(top: Vec<u8>, width: u32, height: u32, format: u32) -> Image {
    let mut image = Image { format, data: Vec::new(), levels: Vec::new() };
    let (mut level, mut w, mut h) = (top, width, height);
    loop {
        let start = image.data.len();
        image.data.extend_from_slice(&level);
        image.levels.push(Level { width: w, height: h, pitch: w * 4, bytes: start..image.data.len() });
        if w == 1 && h == 1 {
            break;
        }
        (level, w, h) = halve(&level, w, h);
    }
    image
}

/// The image as a browser's card takes it. `unpack`: no block compression on this device.
pub fn for_card(mut image: Image, unpack: bool) -> Result<Image, String> {
    let top = image.levels.first().ok_or("an image without a level")?.clone();
    match image.format {
        BC1_UNORM | BC2_UNORM | BC3_UNORM if unpack => {
            // the file's own smaller levels are kept: they were made from the full picture
            let mut out = Image { format: R8G8B8A8_UNORM, data: Vec::new(), levels: Vec::new() };
            for level in &image.levels {
                let pixels = unpack_level(image.format, &image.data[level.bytes.clone()], level.width, level.height);
                let start = out.data.len();
                out.data.extend_from_slice(&pixels);
                out.levels.push(Level { width: level.width, height: level.height, pitch: level.width * 4, bytes: start..out.data.len() });
            }
            if out.levels.len() == 1 && top.width.max(top.height) > 4 {
                return Ok(with_mips(out.data, top.width, top.height, R8G8B8A8_UNORM));
            }
            Ok(out)
        }
        BC1_UNORM | BC2_UNORM | BC3_UNORM => {
            // a compressed level must be whole blocks on the web: drop the levels below 4 px
            // unless the image itself is that small
            let keep = image.levels.iter().take_while(|l| l.width >= 4 && l.height >= 4).count().max(1);
            if top.width % 4 != 0 || top.height % 4 != 0 {
                return for_card(image, true);
            }
            image.levels.truncate(keep);
            let end = image.levels.last().map_or(0, |l| l.bytes.end);
            image.data.truncate(end);
            Ok(image)
        }
        B8G8R8X8_UNORM | B8G8R8A8_UNORM | R8G8B8A8_UNORM => {
            if image.format == B8G8R8X8_UNORM {
                for pixel in image.data.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
                image.format = B8G8R8A8_UNORM;
            }
            if image.wants_generated_mips() {
                let format = image.format;
                let data = image.data[top.bytes.clone()].to_vec();
                return Ok(with_mips(data, top.width, top.height, format));
            }
            Ok(image)
        }
        other if unpack => Err(format!("DXGI format {other} cannot be unpacked here")),
        _ => Ok(image),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bc1_block_unpacks_to_its_two_colours_and_the_two_between() {
        // colour 0 pure red (0xf800), colour 1 pure blue (0x001f), indices 0, 1, 2, 3 repeated
        let block = [0x00, 0xf8, 0x1f, 0x00, 0b1110_0100, 0b1110_0100, 0b1110_0100, 0b1110_0100];
        let image = Image { format: BC1_UNORM, data: block.to_vec(), levels: vec![Level { width: 4, height: 4, pitch: 8, bytes: 0..8 }] };
        let out = for_card(image, true).unwrap();
        assert_eq!(out.format, R8G8B8A8_UNORM);
        assert_eq!(&out.data[0..16], &[255, 0, 0, 255, 0, 0, 255, 255, 170, 0, 85, 255, 85, 0, 170, 255]);
    }

    #[test]
    fn a_bc3_block_takes_its_alpha_from_the_first_half() {
        let mut block = [0u8; 16];
        block[0] = 255; // alpha 0
        block[1] = 0; // alpha 1
        block[2] = 0b0000_1000; // pixel 0 -> index 0 (255), pixel 1 -> index 1 (0)
        block[8..12].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]); // white, white
        let image = Image { format: BC3_UNORM, data: block.to_vec(), levels: vec![Level { width: 4, height: 4, pitch: 16, bytes: 0..16 }] };
        let out = for_card(image, true).unwrap();
        assert_eq!(&out.data[0..8], &[255, 255, 255, 255, 255, 255, 255, 0]);
    }

    #[test]
    fn a_single_level_gets_a_mip_chain() {
        let data: Vec<u8> = (0..8 * 8).flat_map(|i| [(i * 4) as u8, 0, 0, 255]).collect();
        let image = Image { format: R8G8B8A8_UNORM, data, levels: vec![Level { width: 8, height: 8, pitch: 32, bytes: 0..256 }] };
        let out = for_card(image, false).unwrap();
        assert_eq!(out.levels.iter().map(|l| l.width).collect::<Vec<_>>(), vec![8, 4, 2, 1]);
        assert_eq!(out.data.len(), (64 + 16 + 4 + 1) * 4);
    }
}
