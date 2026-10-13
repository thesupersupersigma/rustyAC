// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `Font` (0x20 bytes): a font wrapper of kgl, a size factor, a colour and an optional drop
//! shadow. `Font::Font` 0x140200a60 / 0x140200bb0, `setColor` 0x140201530 / 0x1402015b0,
//! `blitString` 0x140200e20.

use crate::painter::Painter;

/// `eFontType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontType {
    /// "Segoe UI"
    Proportional,
    /// "Consolas"
    Monospaced,
    /// the name given
    Custom,
}

#[derive(Clone, Debug)]
pub struct Font {
    pub scale: f32,
    pub shadowed: bool,
    pub shadow_pixel_distance: f32,
    /// `kid`: the font wrapper
    pub kid: usize,
    /// 0xAABBGGRR
    pub color: u32,
    pub current_alpha: f32,
}

/// A colour cell as `Font::setColor` packs it: times 255, cut towards zero, the low byte.
fn byte(v: f32) -> u32 {
    ((v * 255.0) as i64 as u32) & 0xff
}

impl Font {
    /// `Font::Font(name, type, scale, italic, bold)` 0x140200a60.
    pub fn new(painter: &mut dyn Painter, name: &str, kind: FontType, scale: f32, italic: bool, bold: bool) -> Font {
        let family = match kind {
            FontType::Proportional => "Segoe UI",
            FontType::Monospaced => "Consolas",
            FontType::Custom => name,
        };
        let kid = painter.font_wrapper(family, italic, bold);
        Font { scale, shadowed: false, shadow_pixel_distance: 2.0, kid, color: 0xffff_ffff, current_alpha: 1.0 }
    }

    /// `Font::setColor(float, float, float, float)` 0x1402015b0.
    pub fn set_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.current_alpha = a;
        self.color = ((((byte(a) << 8) | byte(b)) << 8) | byte(g)) << 8 | byte(r);
    }

    /// `Font::setColor(const vec4f&)` 0x140201530.
    pub fn set_color4(&mut self, c: &[f32; 4]) {
        self.set_color(c[0], c[1], c[2], c[3]);
    }

    /// `Font::blitString` 0x140200e20: with a shadow the string is first drawn black, with the
    /// alpha of the colour, `shadowPixelDistance` to the right and down.
    pub fn blit_string(&mut self, painter: &mut dyn Painter, x: f32, y: f32, text: &str, size: f32, align: i32) {
        if self.shadowed {
            self.shadowed = false;
            let saved = self.color;
            self.color = byte(self.current_alpha) << 24;
            self.blit_string(painter, self.shadow_pixel_distance + x, self.shadow_pixel_distance + y, text, size, align);
            self.color = saved;
            self.shadowed = true;
        }
        let kgl_align = match align {
            1 => 2,
            2 => 1,
            _ => 0,
        };
        painter.blit_string_wide(self.kid, x, y, text, size * self.scale, kgl_align, self.color);
    }
}
