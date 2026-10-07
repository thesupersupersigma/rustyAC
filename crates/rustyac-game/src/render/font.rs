// SPDX-License-Identifier: GPL-3.0-or-later

//! The HUD's letters: the printable ASCII characters of a fixed-width system font, drawn
//! once into a bitmap with GDI (which needs no window) and used as a texture.

use windows::core::w;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GdiFlush, GetTextMetricsW, SelectObject, SetBkColor,
    SetTextColor, TextOutW, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS,
    FIXED_PITCH, FW_BOLD, HGDIOBJ, OUT_DEFAULT_PRECIS, TEXTMETRICW,
};

/// Characters per row of the atlas; the characters are 32..=126, then one cell of solid white.
pub const COLUMNS: usize = 16;
pub const ROWS: usize = 6;
/// The cell that is all white (for filled rectangles).
pub const SOLID: u32 = 127;

pub struct FontBitmap {
    /// Size of one character cell, pixels.
    pub cell_width: usize,
    pub cell_height: usize,
    pub width: usize,
    pub height: usize,
    /// Coverage, one byte per pixel, rows from the top.
    pub pixels: Vec<u8>,
    /// What "scale 1" shrinks the bitmap's letters by: text sizes are given for letters 24
    /// pixels high, whatever height the bitmap was drawn at.
    unit: f32,
}

impl FontBitmap {
    /// Draws the characters at `pixel_height`. Without GDI's font (it cannot really fail) the
    /// bitmap is blank cells, so text is invisible but nothing else breaks.
    pub fn new(pixel_height: i32) -> FontBitmap {
        // SAFETY: GDI calls on objects created here and released before returning; the DIB's
        // memory is read only while the bitmap is alive.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let font = CreateFontW(
                -pixel_height,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                FIXED_PITCH.0 as u32,
                w!("Consolas"),
            );
            let old_font = SelectObject(dc, HGDIOBJ(font.0));
            let mut metrics = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut metrics);
            let cell_width = (metrics.tmAveCharWidth.max(4) + 2) as usize;
            let cell_height = (metrics.tmHeight.max(8) + 2) as usize;
            let (width, height) = (cell_width * COLUMNS, cell_height * ROWS);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // negative: rows from the top
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut pixels = vec![0u8; width * height];
            if let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
                SetTextColor(dc, COLORREF(0x00ff_ffff));
                SetBkColor(dc, COLORREF(0));
                for code in 32u16..127 {
                    let cell = (code - 32) as usize;
                    let x = (cell % COLUMNS * cell_width + 1) as i32;
                    let y = (cell / COLUMNS * cell_height + 1) as i32;
                    let _ = TextOutW(dc, x, y, &[code]);
                }
                let _ = GdiFlush();
                if !bits.is_null() {
                    let source = std::slice::from_raw_parts(bits as *const u8, width * height * 4);
                    for (pixel, bgra) in pixels.iter_mut().zip(source.chunks_exact(4)) {
                        // white text on black: any channel is the coverage
                        *pixel = bgra[1];
                    }
                }
                SelectObject(dc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteDC(dc);
            // the solid cell
            let cell = (SOLID - 32) as usize;
            for y in 0..cell_height {
                for x in 0..cell_width {
                    pixels[(cell / COLUMNS * cell_height + y) * width + cell % COLUMNS * cell_width + x] = 255;
                }
            }
            FontBitmap { cell_width, cell_height, width, height, pixels, unit: 24.0 / pixel_height as f32 }
        }
    }

    /// The texture coordinates of a character's cell: left, top, right, bottom. The cell's
    /// one-pixel border is left out.
    pub fn uv(&self, code: u32) -> [f32; 4] {
        let code = if (32..=SOLID).contains(&code) { code } else { b'?' as u32 };
        let cell = (code - 32) as usize;
        let (x, y) = (cell % COLUMNS * self.cell_width, cell / COLUMNS * self.cell_height);
        if code == SOLID {
            // the middle of the solid cell: no bleeding from its neighbours
            let (u, v) = ((x + self.cell_width / 2) as f32 / self.width as f32, (y + self.cell_height / 2) as f32 / self.height as f32);
            return [u, v, u, v];
        }
        [
            (x + 1) as f32 / self.width as f32,
            (y + 1) as f32 / self.height as f32,
            (x + self.cell_width - 1) as f32 / self.width as f32,
            (y + self.cell_height - 1) as f32 / self.height as f32,
        ]
    }

    /// The size of a character as drawn at scale 1.
    pub fn glyph(&self) -> (f32, f32) {
        ((self.cell_width - 2) as f32 * self.unit, (self.cell_height - 2) as f32 * self.unit)
    }
}
