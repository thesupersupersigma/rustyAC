// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
//
// A Rust translation of parts of FW1FontWrapper (the library the game links for its fonts),
// which is under the MIT License:
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

//! FW1FontWrapper's parts that touch neither DirectWrite nor Direct3D, shared by both text
//! backends (`fw1.rs` on Direct3D 11, `fw1_wgpu.rs` on kgl's wgpu backend): the glyph sheets
//! and the atlas, the quads of a string by sheet, the shader constants, and the `fw1 …` lines
//! of the kgl call log (Direct3D 11's wrapper draws on the context itself, not through kgl, so
//! these lines are what both backends' logs say of text).

// the library's arithmetic is kept as it is (one mip level makes its alignment 1)
#![allow(clippy::modulo_one)]

use crate::kgl_log;

/// `kglBlitStringWide`'s flags: FW1_RESTORESTATE | FW1_NOWORDWRAP, and the alignment.
pub const FW1_CENTER: u32 = 0x1;
pub const FW1_RIGHT: u32 = 0x2;
pub const FW1_NOWORDWRAP: u32 = 0x10;
pub const FW1_ALIASED: u32 = 0x20;
pub const FW1_RESTORESTATE: u32 = 0x800;

/// `FW1_GLYPHCOORDS`.
#[derive(Clone, Copy, Default)]
pub(crate) struct GlyphCoords {
    pub tex_left: f32,
    pub tex_top: f32,
    pub tex_right: f32,
    pub tex_bottom: f32,
    pub pos_left: f32,
    pub pos_top: f32,
    pub pos_right: f32,
    pub pos_bottom: f32,
}

/// `FW1_GLYPHVERTEX`.
#[derive(Clone, Copy)]
pub(crate) struct GlyphVertex {
    pub x: f32,
    pub y: f32,
    pub glyph_index: u32,
    pub color: u32,
}

pub(crate) struct GlyphMetrics {
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: u32,
    pub height: u32,
}

pub(crate) const SHEET_WIDTH: u32 = 512;
pub(crate) const SHEET_HEIGHT: u32 = 512;
const MAX_GLYPHS_PER_SHEET: u32 = 2048;
const MAX_SHEETS: usize = 4096;
/// one mip level: `m_alignWidth`
const ALIGN: u32 = 1;

pub(crate) const VERTEX_BUFFER_SIZE: u32 = 4096 * 16;
pub(crate) const QUAD_VERTEX_SIZE: u32 = 20;
/// `m_maxIndexCount`
pub(crate) const MAX_INDEX_COUNT: u32 = (VERTEX_BUFFER_SIZE * 3) / (2 * QUAD_VERTEX_SIZE);

/// `CFW1GlyphVertexDrawer::createBuffers`' index buffer: two triangles a quad.
pub(crate) fn quad_indices() -> Vec<u16> {
    let mut indices = vec![0u16; MAX_INDEX_COUNT as usize];
    for i in 0..(MAX_INDEX_COUNT / 6) as usize {
        let base = (i * 4) as u16;
        indices[i * 6..i * 6 + 6].copy_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
    indices
}

/// `CFW1GlyphSheet` without its texture.
pub(crate) struct GlyphSheet {
    data: Vec<u8>,
    pub coords: Vec<GlyphCoords>,
    /// `HeightRange`: how far down each column is used
    heights: Vec<u32>,
    updated: u32,
    dirty: [u32; 4],
    closed: bool,
    is_static: bool,
}

impl GlyphSheet {
    fn new() -> GlyphSheet {
        GlyphSheet { data: vec![0u8; (SHEET_WIDTH * SHEET_HEIGHT) as usize], coords: Vec::new(), heights: vec![0u32; (SHEET_WIDTH / ALIGN) as usize], updated: 0, dirty: [0; 4], closed: false, is_static: false }
    }

    fn find_max(&self, start: u32, width: u32) -> u32 {
        let mut current = self.heights[start as usize];
        for i in 1..width {
            current = current.max(self.heights[(start + i) as usize]);
        }
        current
    }

    /// `HeightRange::findMin`: the leftmost place where a block of that width sits highest.
    fn find_min(&self, width: u32) -> (u32, u32) {
        let total = self.heights.len() as u32;
        let width = width.min(total);
        let mut current_max = self.find_max(0, width);
        let mut current_min = current_max;
        let mut min_x = 0;
        let mut i = 1u32;
        while i < total - width {
            if self.heights[(i + width - 1) as usize] >= current_max {
                current_max = self.heights[(i + width - 1) as usize];
            } else if self.heights[(i - 1) as usize] == current_max {
                current_max = self.find_max(i, width);
                if current_max < current_min {
                    current_min = current_max;
                    min_x = i;
                }
            }
            i += 1;
        }
        (min_x, current_min)
    }

    /// `CFW1GlyphSheet::InsertGlyph`. `pixels` are rows of `row_pitch` bytes of which every
    /// `stride`-th is taken.
    fn insert(&mut self, metrics: &GlyphMetrics, pixels: &[u8], row_pitch: u32, stride: u32, allow_oversized: bool) -> u32 {
        if self.closed || self.coords.len() as u32 >= MAX_GLYPHS_PER_SHEET {
            return u32::MAX;
        }
        let (width, height) = (metrics.width, metrics.height);
        let mut block_width = width / ALIGN + 1;
        if width % ALIGN != 0 {
            block_width += 1;
        }
        let mut block_height = height / ALIGN + 1;
        if height % ALIGN != 0 {
            block_height += 1;
        }
        let (block_x, block_y, position_x, position_y);
        if !self.coords.is_empty() || !allow_oversized {
            if ALIGN + width + ALIGN > SHEET_WIDTH {
                return u32::MAX;
            }
            let (x, y) = self.find_min(block_width);
            block_x = x;
            block_y = y;
            position_x = ALIGN + block_x * ALIGN;
            position_y = ALIGN + block_y * ALIGN;
            if position_y + height + ALIGN > SHEET_HEIGHT {
                return u32::MAX;
            }
        } else {
            block_x = 0;
            block_y = 0;
            position_x = ALIGN;
            position_y = ALIGN;
        }
        let update_width = block_width.min(self.heights.len() as u32);
        for i in 0..update_width {
            self.heights[(block_x + i) as usize] = block_y + block_height;
        }
        let coord_offset = ALIGN as f32 * 0.5;
        let aligned_width = (block_width - 1) * ALIGN;
        let aligned_height = (block_height - 1) * ALIGN;
        let coords = GlyphCoords {
            tex_left: (position_x as f32 - coord_offset) / SHEET_WIDTH as f32,
            tex_top: (position_y as f32 - coord_offset) / SHEET_HEIGHT as f32,
            tex_right: ((position_x + aligned_width) as f32 + coord_offset) / SHEET_WIDTH as f32,
            tex_bottom: ((position_y + aligned_height) as f32 + coord_offset) / SHEET_HEIGHT as f32,
            pos_left: metrics.offset_x - coord_offset,
            pos_top: metrics.offset_y - coord_offset,
            pos_right: metrics.offset_x + aligned_width as f32 + coord_offset,
            pos_bottom: metrics.offset_y + aligned_height as f32 + coord_offset,
        };
        let glyph_index = self.coords.len() as u32;
        self.coords.push(coords);
        let mut i = 0;
        while i < height && i < SHEET_HEIGHT - position_y {
            let mut j = 0;
            while j < width && j < SHEET_WIDTH - position_x {
                self.data[((position_y + i) * SHEET_WIDTH + position_x + j) as usize] = pixels[(i * row_pitch + j * stride) as usize];
                j += 1;
            }
            i += 1;
        }
        if self.updated == 0 {
            self.dirty = [position_x - ALIGN, position_y - ALIGN, (position_x + width + ALIGN).min(SHEET_WIDTH), (position_y + height + ALIGN).min(SHEET_HEIGHT)];
        } else {
            self.dirty[0] = self.dirty[0].min(position_x - ALIGN);
            self.dirty[1] = self.dirty[1].min(position_y - ALIGN);
            self.dirty[2] = self.dirty[2].max(position_x + width + ALIGN).min(SHEET_WIDTH);
            self.dirty[3] = self.dirty[3].max(position_y + height + ALIGN).min(SHEET_HEIGHT);
        }
        self.updated += 1;
        glyph_index
    }

    /// `CFW1GlyphSheet::Flush`: the changed rectangle goes to the texture (`upload` gets the
    /// box and the sheet's bytes from its top left corner on, rows of `SHEET_WIDTH`).
    fn flush(&mut self, index: usize, upload: &mut impl FnMut(usize, [u32; 4], &[u8])) {
        if self.is_static {
            return;
        }
        let updated = self.updated;
        self.updated = 0;
        if self.closed {
            self.is_static = true;
        }
        let [left, top, right, bottom] = self.dirty;
        if updated > 0 && right > left && bottom > top {
            if kgl_log::on() {
                let mut rows = Vec::new();
                for y in top..bottom {
                    rows.extend_from_slice(&self.data[(y * SHEET_WIDTH + left) as usize..(y * SHEET_WIDTH + right) as usize]);
                }
                kgl_log::line(format_args!("fw1 atlas update sheet{index} {left} {top} {right} {bottom} {:016x}", kgl_log::hash(&rows)));
            }
            upload(index, self.dirty, &self.data[(top * SHEET_WIDTH + left) as usize..]);
        }
        if self.is_static {
            self.data = Vec::new();
        }
    }
}

/// `CFW1GlyphAtlas`: the sheets, each with the texture `T` of its backend.
pub(crate) struct GlyphAtlas<T> {
    pub sheets: Vec<(GlyphSheet, T)>,
    current_sheet_index: usize,
    flushed_sheet_index: usize,
}

impl<T> GlyphAtlas<T> {
    pub fn new() -> GlyphAtlas<T> {
        GlyphAtlas { sheets: Vec::new(), current_sheet_index: 0, flushed_sheet_index: 0 }
    }

    /// `CFW1GlyphAtlas::InsertGlyph` (+ `InsertSheet`): into the first open sheet that takes
    /// it, else into a new one (`texture` makes its texture). The id is the sheet's index and
    /// the glyph's index in it.
    pub fn insert(&mut self, metrics: &GlyphMetrics, pixels: &[u8], row_pitch: u32, stride: u32, texture: impl FnOnce() -> Option<T>) -> u32 {
        for i in self.current_sheet_index..self.sheets.len() {
            let index = self.sheets[i].0.insert(metrics, pixels, row_pitch, stride, true);
            if index != u32::MAX {
                return ((i as u32) << 16) | index;
            }
        }
        if self.sheets.len() < MAX_SHEETS {
            if let Some(texture) = texture() {
                let mut sheet = GlyphSheet::new();
                let index = sheet.insert(metrics, pixels, row_pitch, stride, true);
                let sheet_index = self.sheets.len();
                kgl_log::line(format_args!("fw1 new_sheet sheet{sheet_index}"));
                self.sheets.push((sheet, texture));
                if self.sheets.len() > self.current_sheet_index + 4 {
                    self.sheets[self.current_sheet_index].0.closed = true;
                    self.current_sheet_index += 1;
                }
                if index != u32::MAX {
                    return ((sheet_index as u32) << 16) | index;
                }
            }
        }
        u32::MAX
    }

    /// `CFW1GlyphAtlas::Flush`: the sheets not yet flushed for good.
    pub fn flush(&mut self, mut upload: impl FnMut(&T, [u32; 4], &[u8])) {
        let (first, end) = (self.flushed_sheet_index, self.sheets.len());
        self.flushed_sheet_index = self.current_sheet_index;
        for (i, (sheet, texture)) in self.sheets[first..end].iter_mut().enumerate() {
            sheet.flush(first + i, &mut |_, rect, bytes| upload(texture, rect, bytes));
        }
    }
}

/// One fill of the vertex buffer and its draws: (sheet, index count, base vertex).
pub(crate) struct Batch {
    pub bytes: Vec<u8>,
    pub draws: Vec<(usize, u32, i32)>,
}

/// `CFW1TextGeometry::GetGlyphVerticesTemp` + `CFW1GlyphVertexDrawer::drawGlyphsAsQuads`: the
/// glyphs by sheet, in the order they came, as quads of 20 bytes, in buffer fills.
pub(crate) fn batches<T>(vertices: &[GlyphVertex], max_sheet_index: u32, atlas: &GlyphAtlas<T>) -> Vec<Batch> {
    let sheet_count = max_sheet_index as usize + 1;
    let mut counts = vec![0u32; sheet_count];
    for v in vertices {
        counts[(v.glyph_index >> 16) as usize] += 1;
    }
    let mut starts = vec![0u32; sheet_count];
    let mut start = 0;
    for (s, count) in starts.iter_mut().zip(&counts) {
        *s = start;
        start += count;
    }
    let mut sorted = vertices.to_vec();
    for v in vertices {
        let sheet = (v.glyph_index >> 16) as usize;
        sorted[starts[sheet] as usize] = GlyphVertex { glyph_index: v.glyph_index & 0xffff, ..*v };
        starts[sheet] += 1;
    }
    let mut max_vertex_count = VERTEX_BUFFER_SIZE / QUAD_VERTEX_SIZE;
    if max_vertex_count > 4 * (MAX_INDEX_COUNT / 6) {
        max_vertex_count = 4 * (MAX_INDEX_COUNT / 6);
    }
    max_vertex_count -= max_vertex_count % 4;
    let total = sorted.len() as u32;
    let mut current_sheet = 0usize;
    let mut current_vertex = 0u32;
    let mut next_sheet_start = counts[0];
    let mut out = Vec::new();
    while current_vertex < total {
        let vertex_count = ((total - current_vertex) * 4).min(max_vertex_count);
        let mut batch = Batch { bytes: Vec::with_capacity((vertex_count * QUAD_VERTEX_SIZE) as usize), draws: Vec::new() };
        let mut drawn = 0u32;
        while drawn < vertex_count {
            while current_vertex >= next_sheet_start {
                current_sheet += 1;
                next_sheet_start += counts[current_sheet];
            }
            let draw_count = (vertex_count - drawn).min((next_sheet_start - current_vertex) * 4);
            for i in 0..draw_count / 4 {
                let v = sorted[(current_vertex + i) as usize];
                let c = atlas.sheets[current_sheet].0.coords[v.glyph_index as usize];
                let corners = [(v.x + c.pos_left, v.y + c.pos_top, c.tex_left, c.tex_top), (v.x + c.pos_right, v.y + c.pos_top, c.tex_right, c.tex_top), (v.x + c.pos_left, v.y + c.pos_bottom, c.tex_left, c.tex_bottom), (v.x + c.pos_right, v.y + c.pos_bottom, c.tex_right, c.tex_bottom)];
                for (px, py, u, t) in corners {
                    for f in [px, py, u, t] {
                        batch.bytes.extend_from_slice(&f.to_le_bytes());
                    }
                    batch.bytes.extend_from_slice(&v.color.to_le_bytes());
                }
            }
            batch.draws.push((current_sheet, (draw_count / 2) * 3, drawn as i32));
            drawn += draw_count;
            current_vertex += draw_count / 4;
        }
        out.push(batch);
    }
    out
}

/// `CFW1GlyphRenderStates::UpdateShaderConstants`: pixels of the viewport (512 x 512 without
/// one) to clip space, and no clip rectangle.
pub(crate) fn constants(viewport: Option<(f32, f32)>) -> [u8; 80] {
    let (w, h) = viewport.filter(|(w, h)| *w >= 1.0 && *h >= 1.0).unwrap_or((512.0, 512.0));
    let mut constants = [0f32; 20];
    constants[0] = 2.0 / w;
    constants[12] = -1.0;
    constants[5] = -2.0 / h;
    constants[13] = 1.0;
    constants[10] = 1.0;
    constants[15] = 1.0;
    constants[16..20].copy_from_slice(&[f32::MAX; 4]);
    let mut bytes = [0u8; 80];
    for (i, c) in constants.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&c.to_le_bytes());
    }
    bytes
}

/// `Font::getFontWrapper`'s key: "family[ Italic][ Bold]".
pub(crate) fn font_key(family: &str, italic: bool, bold: bool) -> String {
    format!("{family}{}{}", if italic { " Italic" } else { "" }, if bold { " Bold" } else { "" })
}

/// `kglBlitStringWide`'s flags of an alignment (0 left, 1 centre, 2 right).
pub(crate) fn blit_flags(align: u32) -> u32 {
    match align {
        1 => FW1_RESTORESTATE | FW1_NOWORDWRAP | FW1_CENTER,
        2 => FW1_RESTORESTATE | FW1_NOWORDWRAP | FW1_RIGHT,
        _ => FW1_RESTORESTATE | FW1_NOWORDWRAP,
    }
}

// ----- the kgl call log's lines of text (the same on both backends) -----

pub(crate) fn log_create_font(key: &str, made: Option<usize>, shaders: Option<(&[u8], &[u8])>) {
    if !kgl_log::on() {
        return;
    }
    match (made, shaders) {
        (Some(n), Some((vs, ps))) => kgl_log::line(format_args!("fw1 create_font {key:?} vs {:016x} ps {:016x} = font{n}", kgl_log::hash(vs), kgl_log::hash(ps))),
        _ => kgl_log::line(format_args!("fw1 create_font {key:?} = -")),
    }
}

pub(crate) fn log_draw_string(font: usize, x: f32, y: f32, text: &[u16], size: f32, flags: u32, color: u32) {
    kgl_log::line(format_args!("fw1 draw_string font{font} {x} {y} {size} {flags:x} {color:08x} {:?}", String::from_utf16_lossy(text)));
}

/// The glyphs a string was laid out into: (x, y, glyph of the face, colour) each, and the
/// buffer fills with their draws. Where the glyphs' pixels went (the quads' corners and texture
/// coordinates) is the atlas's, which the `fw1 atlas` lines cover.
pub(crate) fn log_geometry(placed: &[(f32, f32, u16, u32)], batches: &[Batch], constants: &[u8; 80]) {
    if !kgl_log::on() {
        return;
    }
    let mut bytes = Vec::with_capacity(placed.len() * 14);
    for (x, y, glyph, color) in placed {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&glyph.to_le_bytes());
        bytes.extend_from_slice(&color.to_le_bytes());
    }
    kgl_log::line(format_args!("fw1 glyphs {} {:016x} constants {:016x}", placed.len(), kgl_log::hash(&bytes), kgl_log::hash(constants)));
    for batch in batches {
        kgl_log::line(format_args!("fw1 fill {}", batch.bytes.len() / QUAD_VERTEX_SIZE as usize));
        for (sheet, count, base) in &batch.draws {
            kgl_log::line(format_args!("fw1 draw sheet{sheet} {count} {base}"));
        }
    }
}

/// `RUSTYAC_FW1_GLYPHS=<file>`: every glyph rasterised, appended to that file as one line
/// (font, size, glyph, offsets, size and the pixels in hex), to hold one backend's glyphs
/// against the other's.
pub(crate) fn dump_glyph(key: &str, size: f32, glyph: u16, metrics: &GlyphMetrics, pixels: &[u8], row_pitch: u32, stride: u32) {
    let Some(path) = std::env::var_os("RUSTYAC_FW1_GLYPHS") else { return };
    let mut line = format!("{key:?} {size} {glyph} {} {} {} {} ", metrics.offset_x, metrics.offset_y, metrics.width, metrics.height);
    for i in 0..metrics.height {
        for j in 0..metrics.width {
            line.push_str(&format!("{:02x}", pixels[(i * row_pitch + j * stride) as usize]));
        }
    }
    line.push('\n');
    append(&path, &line);
}

/// `RUSTYAC_FW1_GLYPHS`: and every string's layout, a line `layout` with its glyphs' places.
pub(crate) fn dump_layout(key: &str, size: f32, x: f32, y: f32, flags: u32, text: &[u16], placed: &[(f32, f32, u16, u32)]) {
    let Some(path) = std::env::var_os("RUSTYAC_FW1_GLYPHS") else { return };
    let mut line = format!("layout {key:?} {size} {x} {y} {flags:x} {:?}", String::from_utf16_lossy(text));
    for (x, y, glyph, _) in placed {
        line.push_str(&format!(" {x},{y},{glyph}"));
    }
    line.push('\n');
    append(&path, &line);
}

fn append(path: &std::ffi::OsStr, line: &str) {
    use std::io::Write;
    if let Ok(mut file) = rustyac_content::guard::append(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_pack_and_batch() {
        let mut atlas: GlyphAtlas<()> = GlyphAtlas::new();
        let white = [0xffu8; 256];
        assert_eq!(atlas.insert(&GlyphMetrics { offset_x: 0.0, offset_y: 0.0, width: 16, height: 16 }, &white, 16, 1, || Some(())), 0);
        let id = atlas.insert(&GlyphMetrics { offset_x: 1.0, offset_y: -10.0, width: 4, height: 10 }, &white, 4, 1, || Some(()));
        assert_eq!(id, 1);
        // the second glyph goes right of the first block (17 columns wide), on the top row
        let c = atlas.sheets[0].0.coords[1];
        assert_eq!(c.tex_left, 17.5 / 512.0);
        assert_eq!(c.pos_top, -10.5);
        let mut uploads = 0;
        atlas.flush(|_, rect, bytes| {
            uploads += 1;
            assert_eq!(rect, [0, 0, 23, 18]);
            assert_eq!(bytes[512 + 1], 0xff);
        });
        assert_eq!(uploads, 1);
        let v = GlyphVertex { x: 10.0, y: 20.0, glyph_index: 1, color: 0xff00_00ff };
        let b = batches(&[v, v], 0, &atlas);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].bytes.len(), 2 * 4 * 20);
        assert_eq!(b[0].draws, vec![(0, 12, 0)]);
        assert_eq!(quad_indices()[6..12], [4, 5, 6, 5, 7, 6]);
    }
}
