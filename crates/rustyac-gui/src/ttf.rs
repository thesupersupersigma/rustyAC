// SPDX-License-Identifier: GPL-3.0-or-later

//! A text backend without DirectWrite, for where there is none (the browser): it reads the
//! same `.ttf` files the game hands to DirectWrite (`content/fonts`), turns a string into its
//! outlines and fills them into a coverage picture.
//!
//! This is rustyAC's own code, not a port: the game's text is DirectWrite's (through
//! FW1FontWrapper, `rustyac-render`'s `fw1.rs`), with its hinting, its kerning and its own
//! anti-aliasing. Here: the font's plain outlines at the same size and on the same base line,
//! the advances of `hmtx` with the pairs of `kern`, no hinting, exact area coverage. How close that comes
//! is measured by the render oracle's `text-fallback` (see `docs/port/gui_hud.md`).

/// One font file.
pub struct TtfFont {
    data: Vec<u8>,
    /// `name` 1: the family ("Segoe UI"), lower case
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub line_gap: f32,
    glyphs: u32,
    long_loca: bool,
    h_metrics: u32,
    cmap: usize,
    loca: usize,
    glyf: usize,
    hmtx: usize,
    /// the pairs of the `kern` table's first horizontal subtable of format 0, and how many
    kern: Option<(usize, usize)>,
}

/// A string as a picture: `alpha` is `width` x `height` bytes of coverage; its top left
/// corner sits (`left`, `top`) pixels from the pen's start at the top of the line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StringImage {
    pub width: usize,
    pub height: usize,
    pub left: i32,
    pub top: i32,
    pub alpha: Vec<u8>,
    /// how far the pen went
    pub advance: f32,
}

fn u16_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]) as u32)
}

fn i16_at(d: &[u8], at: usize) -> Option<i32> {
    Some(i16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]) as i32)
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*d.get(at)?, *d.get(at + 1)?, *d.get(at + 2)?, *d.get(at + 3)?]))
}

type Point = (f32, f32);

/// The outline of a glyph in font units: closed contours of lines and quadratic curves,
/// flattened into lines by whoever takes them.
#[derive(Default)]
struct Outline {
    contours: Vec<Vec<(Point, bool)>>,
}

impl TtfFont {
    pub fn parse(data: Vec<u8>) -> Option<TtfFont> {
        let d = &data[..];
        let tables = u16_at(d, 4)? as usize;
        let table = |tag: &[u8; 4]| -> Option<usize> { (0..tables).map(|i| 12 + i * 16).find(|at| d.get(*at..*at + 4) == Some(&tag[..])).and_then(|at| u32_at(d, at + 8)).map(|o| o as usize) };
        let (head, hhea, maxp) = (table(b"head")?, table(b"hhea")?, table(b"maxp")?);
        let (cmap_table, name) = (table(b"cmap")?, table(b"name")?);
        // the Unicode subtable: Windows full (3, 10), Windows BMP (3, 1), or Unicode (0, x)
        let mut cmap = None;
        let mut rank = 0;
        for i in 0..u16_at(d, cmap_table + 2)? as usize {
            let at = cmap_table + 4 + i * 8;
            let (platform, encoding, offset) = (u16_at(d, at)?, u16_at(d, at + 2)?, u32_at(d, at + 4)? as usize);
            let format = u16_at(d, cmap_table + offset)?;
            let r = match (platform, encoding, format) {
                (3, 10, 12) => 4,
                (0, _, 12) => 3,
                (3, 1, 4) => 2,
                (0, _, 4) => 1,
                _ => 0,
            };
            if r > rank {
                rank = r;
                cmap = Some(cmap_table + offset);
            }
        }
        // the family's name: Windows, UTF-16
        let mut family = String::new();
        let strings = name + u16_at(d, name + 4)? as usize;
        for i in 0..u16_at(d, name + 2)? as usize {
            let at = name + 6 + i * 12;
            if u16_at(d, at)? == 3 && u16_at(d, at + 6)? == 1 {
                let (length, offset) = (u16_at(d, at + 8)? as usize, u16_at(d, at + 10)? as usize);
                let units: Vec<u16> = (0..length / 2).filter_map(|k| u16_at(d, strings + offset + k * 2).map(|u| u as u16)).collect();
                family = String::from_utf16_lossy(&units).to_lowercase();
                break;
            }
        }
        let mac_style = u16_at(d, head + 44)?;
        // the line's metrics as DirectWrite takes them: the Windows ascent and descent of
        // OS/2 (its typographic ones when the font says so), else hhea's
        let (mut ascender, mut descender, mut line_gap) = (i16_at(d, hhea + 4)? as f32, i16_at(d, hhea + 6)? as f32, i16_at(d, hhea + 8)? as f32);
        if let Some(os2) = table(b"OS/2").filter(|at| at + 78 <= d.len()) {
            if u16_at(d, os2 + 62)? & 0x80 != 0 {
                (ascender, descender, line_gap) = (i16_at(d, os2 + 68)? as f32, i16_at(d, os2 + 70)? as f32, i16_at(d, os2 + 72)? as f32);
            } else {
                (ascender, descender, line_gap) = (u16_at(d, os2 + 74)? as f32, -(u16_at(d, os2 + 76)? as f32), 0.0);
            }
        }
        let mut kern = None;
        if let Some(at) = table(b"kern").filter(|at| u16_at(d, *at) == Some(0)) {
            let mut sub = at + 4;
            for _ in 0..u16_at(d, at + 2)? {
                let (length, coverage) = (u16_at(d, sub + 2)? as usize, u16_at(d, sub + 4)?);
                if coverage >> 8 == 0 && coverage & 1 != 0 && coverage & 6 == 0 {
                    kern = Some((sub + 14, u16_at(d, sub + 6)? as usize));
                    break;
                }
                sub += length.max(6);
            }
        }
        Some(TtfFont {
            family,
            bold: mac_style & 1 != 0,
            italic: mac_style & 2 != 0,
            units_per_em: u16_at(d, head + 18)? as f32,
            ascender,
            descender,
            line_gap,
            kern,
            glyphs: u16_at(d, maxp + 4)?,
            long_loca: i16_at(d, head + 50)? != 0,
            h_metrics: u16_at(d, hhea + 34)?,
            cmap: cmap?,
            loca: table(b"loca")?,
            glyf: table(b"glyf")?,
            hmtx: table(b"hmtx")?,
            data,
        })
    }

    /// The glyph of a character (0: the font has none).
    pub fn glyph_index(&self, c: char) -> u32 {
        let (d, t, c) = (&self.data[..], self.cmap, c as u32);
        let found = (|| -> Option<u32> {
            if u16_at(d, t)? == 12 {
                for i in 0..u32_at(d, t + 12)? as usize {
                    let at = t + 16 + i * 12;
                    let (start, end) = (u32_at(d, at)?, u32_at(d, at + 4)?);
                    if c >= start && c <= end {
                        return Some(u32_at(d, at + 8)? + (c - start));
                    }
                }
                return None;
            }
            if c > 0xffff {
                return None;
            }
            let segments = u16_at(d, t + 6)? as usize / 2;
            let (ends, starts) = (t + 14, t + 16 + segments * 2);
            let (deltas, offsets) = (starts + segments * 2, starts + segments * 4);
            for i in 0..segments {
                let end = u16_at(d, ends + i * 2)?;
                if c > end {
                    continue;
                }
                let start = u16_at(d, starts + i * 2)?;
                if c < start {
                    return None;
                }
                let (delta, offset) = (u16_at(d, deltas + i * 2)?, u16_at(d, offsets + i * 2)? as usize);
                if offset == 0 {
                    return Some((c + delta) & 0xffff);
                }
                let glyph = u16_at(d, offsets + i * 2 + offset + (c - start) as usize * 2)?;
                return Some(if glyph == 0 { 0 } else { (glyph + delta) & 0xffff });
            }
            None
        })();
        found.filter(|g| *g < self.glyphs).unwrap_or(0)
    }

    /// A glyph's advance in font units.
    pub fn advance(&self, glyph: u32) -> f32 {
        let i = glyph.min(self.h_metrics.saturating_sub(1)) as usize;
        u16_at(&self.data, self.hmtx + i * 4).unwrap_or(0) as f32
    }

    /// The `kern` table's shift between two glyphs, font units.
    pub fn kerning(&self, left: u32, right: u32) -> f32 {
        let Some((pairs, count)) = self.kern else { return 0.0 };
        let key = (left << 16) | (right & 0xffff);
        let (mut lo, mut hi) = (0usize, count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let Some(k) = u32_at(&self.data, pairs + mid * 6) else { return 0.0 };
            match k.cmp(&key) {
                std::cmp::Ordering::Equal => return i16_at(&self.data, pairs + mid * 6 + 4).unwrap_or(0) as f32,
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        0.0
    }

    fn glyph_range(&self, glyph: u32) -> Option<(usize, usize)> {
        let (d, g) = (&self.data[..], glyph as usize);
        let (a, b) = if self.long_loca { (u32_at(d, self.loca + g * 4)? as usize, u32_at(d, self.loca + g * 4 + 4)? as usize) } else { (u16_at(d, self.loca + g * 2)? as usize * 2, u16_at(d, self.loca + g * 2 + 2)? as usize * 2) };
        (b > a).then_some((self.glyf + a, self.glyf + b))
    }

    /// A glyph's contours, each point through `m` (a 2x2 matrix and a shift).
    fn outline(&self, glyph: u32, m: [f32; 6], out: &mut Outline, depth: u32) -> Option<()> {
        let d = &self.data[..];
        let Some((at, _)) = self.glyph_range(glyph) else { return Some(()) };
        let contours = i16_at(d, at)?;
        let put = |x: f32, y: f32| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        if contours >= 0 {
            let contours = contours as usize;
            let ends: Vec<usize> = (0..contours).filter_map(|i| u16_at(d, at + 10 + i * 2).map(|e| e as usize)).collect();
            let points = ends.last().map_or(0, |e| e + 1);
            let instructions = u16_at(d, at + 10 + contours * 2)? as usize;
            let mut p = at + 12 + contours * 2 + instructions;
            let mut flags = Vec::with_capacity(points);
            while flags.len() < points {
                let f = *d.get(p)?;
                p += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let repeat = *d.get(p)?;
                    p += 1;
                    for _ in 0..repeat {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(points);
            let mut read = |short: u8, same: u8| -> Option<Vec<i32>> {
                let mut v = 0i32;
                let mut out = Vec::with_capacity(points);
                for f in &flags {
                    if f & short != 0 {
                        let b = *d.get(p)? as i32;
                        p += 1;
                        v += if f & same != 0 { b } else { -b };
                    } else if f & same == 0 {
                        v += i16_at(d, p)?;
                        p += 2;
                    }
                    out.push(v);
                }
                Some(out)
            };
            let xs = read(2, 16)?;
            let ys = read(4, 32)?;
            let mut first = 0;
            for end in ends {
                if end < first || end >= points {
                    break;
                }
                out.contours.push((first..=end).map(|i| (put(xs[i] as f32, ys[i] as f32), flags[i] & 1 != 0)).collect());
                first = end + 1;
            }
            return Some(());
        }
        // a glyph made of others
        if depth > 8 {
            return Some(());
        }
        let mut p = at + 10;
        loop {
            let (flags, part) = (u16_at(d, p)?, u16_at(d, p + 2)?);
            p += 4;
            let (dx, dy) = if flags & 1 != 0 {
                let v = (i16_at(d, p)? as f32, i16_at(d, p + 2)? as f32);
                p += 4;
                v
            } else {
                let v = (*d.get(p)? as i8 as f32, *d.get(p + 1)? as i8 as f32);
                p += 2;
                v
            };
            let f2 = |at: usize| i16_at(d, at).map(|v| v as f32 / 16384.0);
            let mut t = [1.0, 0.0, 0.0, 1.0];
            if flags & 8 != 0 {
                t = [f2(p)?, 0.0, 0.0, f2(p)?];
                p += 2;
            } else if flags & 0x40 != 0 {
                t = [f2(p)?, 0.0, 0.0, f2(p + 2)?];
                p += 4;
            } else if flags & 0x80 != 0 {
                t = [f2(p)?, f2(p + 2)?, f2(p + 4)?, f2(p + 6)?];
                p += 8;
            }
            // (the two numbers are a shift when bit 1 says so; matching points are not done)
            let (dx, dy) = if flags & 2 != 0 { (dx, dy) } else { (0.0, 0.0) };
            let inner = [m[0] * t[0] + m[2] * t[1], m[1] * t[0] + m[3] * t[1], m[0] * t[2] + m[2] * t[3], m[1] * t[2] + m[3] * t[3], m[0] * dx + m[2] * dy + m[4], m[1] * dx + m[3] * dy + m[5]];
            self.outline(part, inner, out, depth + 1)?;
            if flags & 0x20 == 0 {
                break;
            }
        }
        Some(())
    }

    /// A string at `size` pixels an em, as DirectWrite lays a line out: the pen starts at the
    /// top left of the line, the base line is the font's ascender below it.
    pub fn string_image(&self, text: &str, size: f32) -> StringImage {
        let scale = size / self.units_per_em;
        let baseline = self.ascender * scale;
        let mut lines: Vec<(Point, Point)> = Vec::new();
        let mut pen = 0.0f32;
        let mut before = None;
        for c in text.chars() {
            let glyph = self.glyph_index(c);
            if let Some(before) = before {
                pen += self.kerning(before, glyph) * scale;
            }
            before = Some(glyph);
            let mut outline = Outline::default();
            // font units (y up) to pixels (y down) at the pen
            let _ = self.outline(glyph, [scale, 0.0, 0.0, -scale, pen, baseline], &mut outline, 0);
            for contour in &outline.contours {
                flatten(contour, &mut lines);
            }
            pen += self.advance(glyph) * scale;
        }
        let mut image = StringImage { advance: pen, ..StringImage::default() };
        if lines.is_empty() {
            return image;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (a, b) in &lines {
            x0 = x0.min(a.0).min(b.0);
            y0 = y0.min(a.1).min(b.1);
            x1 = x1.max(a.0).max(b.0);
            y1 = y1.max(a.1).max(b.1);
        }
        let (left, top) = (x0.floor() as i32 - 1, y0.floor() as i32 - 1);
        let (width, height) = ((x1.ceil() as i32 - left + 2) as usize, (y1.ceil() as i32 - top + 2) as usize);
        let mut acc = vec![0.0f32; width * height + 4];
        for (a, b) in &lines {
            line(&mut acc, width, height, (a.0 - left as f32, a.1 - top as f32), (b.0 - left as f32, b.1 - top as f32));
        }
        let mut sum = 0.0f32;
        image.alpha = acc[..width * height].iter().map(|a| {
            sum += a;
            (sum.abs().min(1.0) * 255.0 + 0.5) as u8
        }).collect();
        (image.width, image.height, image.left, image.top) = (width, height, left, top);
        image
    }
}

/// A closed contour of on-curve and control points as lines (a curve between two control
/// points passes through their middle).
fn flatten(contour: &[(Point, bool)], lines: &mut Vec<(Point, Point)>) {
    let n = contour.len();
    if n < 2 {
        return;
    }
    let mid = |a: Point, b: Point| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    // a point on the curve to start from
    let start = match contour.iter().position(|p| p.1) {
        Some(i) => i,
        None => {
            // all control points: start between the first two
            let s = mid(contour[0].0, contour[1].0);
            let mut pen = s;
            for i in 1..=n {
                let c = contour[i % n].0;
                let next = if i == n { s } else { mid(c, contour[(i + 1) % n].0) };
                quad(pen, c, next, lines);
                pen = next;
            }
            return;
        }
    };
    let first = contour[start].0;
    let mut pen = first;
    let mut control: Option<Point> = None;
    for k in 1..=n {
        let (p, on) = contour[(start + k) % n];
        match (control, on) {
            (None, true) => {
                lines.push((pen, p));
                pen = p;
            }
            (None, false) => control = Some(p),
            (Some(c), true) => {
                quad(pen, c, p, lines);
                pen = p;
                control = None;
            }
            (Some(c), false) => {
                let m = mid(c, p);
                quad(pen, c, m, lines);
                pen = m;
                control = Some(p);
            }
        }
    }
    if let Some(c) = control {
        quad(pen, c, first, lines);
    }
}

fn quad(p0: Point, p1: Point, p2: Point, lines: &mut Vec<(Point, Point)>) {
    let (dx, dy) = (p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1);
    let dev = dx * dx + dy * dy;
    if dev < 0.333 {
        lines.push((p0, p2));
        return;
    }
    let n = 1 + (3.0 * dev).sqrt().sqrt().floor() as usize;
    let mut pen = p0;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let p = (u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1);
        lines.push((pen, p));
        pen = p;
    }
}

/// A line's share of every pixel it crosses, as signed area to the right of it (summing along
/// a row gives the coverage).
fn line(acc: &mut [f32], width: usize, height: usize, p0: Point, p1: Point) {
    if p0.1 == p1.1 {
        return;
    }
    let (dir, p0, p1) = if p0.1 < p1.1 { (1.0f32, p0, p1) } else { (-1.0f32, p1, p0) };
    let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
    let mut x = p0.0;
    let y_first = p0.1.max(0.0) as usize;
    if p0.1 < 0.0 {
        x -= p0.1 * dxdy;
    }
    for y in y_first..height.min(p1.1.ceil() as usize) {
        let row = y * width;
        let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
        let x_next = x + dxdy * dy;
        let d = dy * dir;
        let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
        let x0_floor = x0.floor();
        let x0i = x0_floor as usize;
        let x1_ceil = x1.ceil();
        let x1i = x1_ceil as usize;
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x + x_next) - x0_floor;
            acc[row + x0i] += d - d * xmf;
            acc[row + x0i + 1] += d * xmf;
        } else {
            let s = 1.0 / (x1 - x0);
            let x0f = x0 - x0_floor;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = x1 - x1_ceil + 1.0;
            let am = 0.5 * s * x1f * x1f;
            acc[row + x0i] += d * a0;
            if x1i == x0i + 2 {
                acc[row + x0i + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - x0f);
                acc[row + x0i + 1] += d * (a1 - a0);
                for xi in x0i + 2..x1i - 1 {
                    acc[row + xi] += d * s;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                acc[row + x1i - 1] += d * (1.0 - a2 - am);
            }
            acc[row + x1i] += d * am;
        }
        x = x_next;
    }
}

/// The fonts of a folder, asked for as the widgets ask kgl (`Font::getFontWrapper`): a
/// family, italic or not, bold or not.
#[derive(Default)]
pub struct TtfText {
    pub fonts: Vec<TtfFont>,
}

impl TtfText {
    /// A font file's bytes (one that cannot be read is left out).
    pub fn add(&mut self, data: Vec<u8>) -> bool {
        match TtfFont::parse(data) {
            Some(font) => {
                self.fonts.push(font);
                true
            }
            None => false,
        }
    }

    /// The font of a family in a style: that style, else the family's plain one, else none.
    pub fn font_wrapper(&self, family: &str, italic: bool, bold: bool) -> Option<usize> {
        let family = family.to_lowercase();
        let of = |i: bool, b: bool| self.fonts.iter().position(|f| f.family == family && f.italic == i && f.bold == b);
        of(italic, bold).or_else(|| of(false, false)).or_else(|| self.fonts.iter().position(|f| f.family == family))
    }

    /// A string of a font as a picture and where it goes for `kglBlitStringWide`'s `align` (0
    /// left, 1 centre, 2 right of x): the picture's top left corner relative to (x, y).
    pub fn string_image(&self, wrapper: usize, text: &str, size: f32, align: u32) -> StringImage {
        let Some(font) = self.fonts.get(wrapper) else { return StringImage::default() };
        let mut image = font.string_image(text, size);
        let shift = match align {
            1 => -(image.advance * 0.5),
            2 => -image.advance,
            _ => 0.0,
        };
        image.left += shift.round() as i32;
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font of one glyph (a square of half an em), built here: the tables the reader needs.
    fn square_font() -> Vec<u8> {
        let be16 = |v: i32| (v as i16).to_be_bytes().to_vec();
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&be16(800));
        hhea[6..8].copy_from_slice(&be16(-200));
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&2u16.to_be_bytes());
        let hmtx: Vec<u8> = [[0u16, 0], [600, 0]].iter().flatten().flat_map(|v| v.to_be_bytes()).collect();
        // glyph 1: one contour, four points on the curve (0,0) (0,500) (500,500) (500,0)
        let mut glyf = Vec::new();
        for v in [1, 0, 0, 500, 500] {
            glyf.extend(be16(v));
        }
        glyf.extend(3u16.to_be_bytes());
        glyf.extend(0u16.to_be_bytes());
        glyf.extend([1u8, 1, 1, 1]);
        for v in [0, 0, 500, 0, 0, 500, 0, -500] {
            glyf.extend(be16(v));
        }
        let loca: Vec<u8> = [0u16, 0, (glyf.len() / 2) as u16].iter().flat_map(|v| v.to_be_bytes()).collect();
        // cmap: one subtable (3, 1) of format 4 with the segment 'A'..'A' -> glyph 1
        let mut cmap = Vec::new();
        for v in [0u16, 1, 3, 1] {
            cmap.extend(v.to_be_bytes());
        }
        cmap.extend(12u32.to_be_bytes());
        for v in [4u16, 32, 0, 4, 4, 1, 0, 0x41, 0xffff, 0, 0x41, 0xffff, (1i32 - 0x41) as u16, 1, 0, 0] {
            cmap.extend(v.to_be_bytes());
        }
        // name: one record (3, 1, 0x409, 1): "Sq"
        let mut name = Vec::new();
        for v in [0u16, 1, 18, 3, 1, 0x409, 1, 4, 0] {
            name.extend(v.to_be_bytes());
        }
        name.extend([0, b'S', 0, b'q']);
        let tables: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp), (b"name", name)];
        let mut file = vec![0u8, 1, 0, 0];
        file.extend((tables.len() as u16).to_be_bytes());
        file.extend([0u8; 6]);
        let mut offset = 12 + tables.len() * 16;
        for (tag, data) in &tables {
            file.extend(*tag);
            file.extend(0u32.to_be_bytes());
            file.extend((offset as u32).to_be_bytes());
            file.extend((data.len() as u32).to_be_bytes());
            offset += data.len();
        }
        for (_, data) in tables {
            file.extend(data);
        }
        file
    }

    #[test]
    fn a_square_is_filled() {
        let font = TtfFont::parse(square_font()).expect("the font reads");
        assert_eq!((font.family.as_str(), font.bold, font.italic, font.units_per_em), ("sq", false, false, 1000.0));
        assert_eq!((font.glyph_index('A'), font.glyph_index('B')), (1, 0));
        // 20 pixels an em: a square of 10 x 10 pixels whose top is 6 below the line's top
        let image = font.string_image("AA", 20.0);
        assert_eq!(image.advance, 24.0);
        let at = |x: i32, y: i32| image.alpha[((y - image.top) as usize) * image.width + (x - image.left) as usize];
        assert_eq!((at(0, 6), at(9, 15), at(5, 10)), (255, 255, 255));
        assert_eq!((at(10, 10), at(11, 10), at(5, 5), at(5, 16)), (0, 0, 0, 0));
        assert_eq!((at(12, 6), at(21, 15), at(22, 10)), (255, 255, 0));
        let covered: u32 = image.alpha.iter().map(|a| *a as u32).sum();
        assert_eq!(covered, 2 * 100 * 255);
        // half a pixel to the right by centring an odd advance: the edges share their pixels
        let image = font.string_image("A", 21.0);
        let total: u32 = image.alpha.iter().map(|a| *a as u32).sum();
        assert!((total as f32 / 255.0 - 10.5 * 10.5).abs() < 0.6, "{total}");
    }

    #[test]
    fn fonts_are_found_by_family_and_style() {
        let mut text = TtfText::default();
        assert!(text.add(square_font()));
        assert!(!text.add(vec![0; 16]));
        assert_eq!((text.font_wrapper("Sq", false, false), text.font_wrapper("SQ", true, true), text.font_wrapper("Other", false, false)), (Some(0), Some(0), None));
        let image = text.string_image(0, "A", 20.0, 2);
        assert_eq!(image.left, -12 - 1);
    }
}
