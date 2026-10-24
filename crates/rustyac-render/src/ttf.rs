// SPDX-License-Identifier: GPL-3.0-or-later

//! NOT Assetto Corsa: what DirectWrite does for FW1FontWrapper where there is no DirectWrite
//! (kgl's wgpu backend, the browser): reads the game's TrueType fonts (`content/fonts/*.ttf`),
//! picks a face by family, weight and style, lays a string out as DirectWrite's natural layout
//! does (advances of the design units, unrounded, with the font's pair kerning) and draws a glyph
//! as grey coverage of its outline. Not hinted: the glyphs' pixels are close to DirectWrite's, not
//! the same (docs/port/web_hud.md has how close).

/// One TrueType face: its bytes and where its tables are.
pub struct Face {
    data: Vec<u8>,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub units_per_em: u16,
    /// `usWinAscent`: DirectWrite's ascent, where the first baseline is
    pub ascent: u16,
    /// DirectWrite's line height: `usWinAscent` + `usWinDescent` + what hhea's line spacing has
    /// beyond them
    line_height: i32,
    num_glyphs: u16,
    num_h_metrics: u16,
    long_loca: bool,
    hmtx: usize,
    loca: usize,
    glyf: usize,
    cmap: Option<(usize, u16)>,
    kern_lookups: Vec<usize>,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]))
}

fn i16_at(d: &[u8], at: usize) -> i16 {
    u16_at(d, at) as i16
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Face {
    pub fn parse(data: Vec<u8>) -> Option<Face> {
        let d = &data;
        let tables = u16_at(d, 4) as usize;
        let table = |tag: &[u8; 4]| (0..tables).map(|i| 12 + 16 * i).find(|&r| d.get(r..r + 4) == Some(tag)).map(|r| u32_at(d, r + 8) as usize);
        let head = table(b"head")?;
        let hhea = table(b"hhea")?;
        let maxp = table(b"maxp")?;
        let os2 = table(b"OS/2")?;
        let name = table(b"name")?;
        let mut face = Face {
            family: String::new(),
            weight: u16_at(d, os2 + 4),
            italic: u16_at(d, os2 + 62) & 0x201 != 0,
            units_per_em: u16_at(d, head + 18),
            ascent: u16_at(d, os2 + 74),
            line_height: {
                let win = u16_at(d, os2 + 74) as i32 + u16_at(d, os2 + 76) as i32;
                win + (i16_at(d, hhea + 4) as i32 - i16_at(d, hhea + 6) as i32 + i16_at(d, hhea + 8) as i32 - win).max(0)
            },
            num_glyphs: u16_at(d, maxp + 4),
            num_h_metrics: u16_at(d, hhea + 34),
            long_loca: i16_at(d, head + 50) != 0,
            hmtx: table(b"hmtx")?,
            loca: table(b"loca")?,
            glyf: table(b"glyf")?,
            cmap: None,
            kern_lookups: Vec::new(),
            data: Vec::new(),
        };
        if face.units_per_em == 0 {
            return None;
        }
        // the family: the typographic one (16) when there is one, else 1; Windows' English names
        let count = u16_at(d, name + 2) as usize;
        let strings = name + u16_at(d, name + 4) as usize;
        let mut best = (0, String::new());
        for i in 0..count {
            let r = name + 6 + 12 * i;
            let (platform, encoding, language, id) = (u16_at(d, r), u16_at(d, r + 2), u16_at(d, r + 4), u16_at(d, r + 6));
            if platform != 3 || !(encoding == 1 || encoding == 0) || !(id == 1 || id == 16) {
                continue;
            }
            let rank = if id == 16 { 2 } else { 1 } * 2 + (language == 0x409) as i32;
            if rank > best.0 {
                let (len, at) = (u16_at(d, r + 8) as usize, strings + u16_at(d, r + 10) as usize);
                let units: Vec<u16> = (0..len / 2).map(|k| u16_at(d, at + 2 * k)).collect();
                best = (rank, String::from_utf16_lossy(&units));
            }
        }
        face.family = best.1;
        // cmap: Unicode full (3,10) before BMP (3,1) before Unicode (0,*)
        if let Some(cmap) = table(b"cmap") {
            let mut rank = 0;
            for i in 0..u16_at(d, cmap + 2) as usize {
                let r = cmap + 4 + 8 * i;
                let (platform, encoding) = (u16_at(d, r), u16_at(d, r + 2));
                let sub = cmap + u32_at(d, r + 4) as usize;
                let format = u16_at(d, sub);
                let k = match (platform, encoding, format) {
                    (3, 10, 12) => 4,
                    (0, _, 12) => 3,
                    (3, 1, 4) => 2,
                    (0, _, 4) => 1,
                    _ => 0,
                };
                if k > rank {
                    rank = k;
                    face.cmap = Some((sub, format));
                }
            }
        }
        if let Some(gpos) = table(b"GPOS") {
            face.kern_lookups = gpos_kern_lookups(d, gpos);
        }
        face.data = data;
        Some(face)
    }

    pub fn glyph_count(&self) -> u16 {
        self.num_glyphs
    }

    /// The glyph of a character (0 for none).
    pub fn glyph(&self, c: u32) -> u16 {
        let d = &self.data;
        let Some((sub, format)) = self.cmap else { return 0 };
        if format == 4 {
            if c > 0xffff {
                return 0;
            }
            let segs = u16_at(d, sub + 6) as usize / 2;
            let ends = sub + 14;
            let starts = ends + 2 * segs + 2;
            let deltas = starts + 2 * segs;
            let offsets = deltas + 2 * segs;
            for s in 0..segs {
                if c as u16 > u16_at(d, ends + 2 * s) {
                    continue;
                }
                let start = u16_at(d, starts + 2 * s);
                if (c as u16) < start {
                    return 0;
                }
                let delta = u16_at(d, deltas + 2 * s);
                let range = u16_at(d, offsets + 2 * s) as usize;
                if range == 0 {
                    return (c as u16).wrapping_add(delta);
                }
                let at = offsets + 2 * s + range + 2 * (c as usize - start as usize);
                let g = u16_at(d, at);
                return if g == 0 { 0 } else { g.wrapping_add(delta) };
            }
            0
        } else {
            let groups = u32_at(d, sub + 12) as usize;
            for i in 0..groups {
                let r = sub + 16 + 12 * i;
                let (start, end, first) = (u32_at(d, r), u32_at(d, r + 4), u32_at(d, r + 8));
                if (start..=end).contains(&c) {
                    return (first + c - start) as u16;
                }
            }
            0
        }
    }

    pub fn advance(&self, glyph: u16) -> u16 {
        let i = glyph.min(self.num_h_metrics.saturating_sub(1)) as usize;
        u16_at(&self.data, self.hmtx + 4 * i)
    }

    /// The pair kerning between two glyphs, in design units: GPOS's `kern` feature (DirectWrite
    /// leaves the old `kern` table alone: Segoe UI's regular face has only that, and is placed
    /// without it).
    fn kerning(&self, left: u16, right: u16) -> (i32, bool) {
        let mut total = 0;
        let mut skip = false;
        for &lookup in &self.kern_lookups {
            if let Some((x, second)) = pair_lookup(&self.data, lookup, left, right) {
                total += x;
                skip |= second;
            }
        }
        (total, skip)
    }

    /// DirectWrite's natural layout of one run: the glyphs and their advances in pixels.
    pub fn layout(&self, text: &[u16], size: f32) -> Vec<(u16, f32)> {
        let scale = size / self.units_per_em as f32;
        let glyphs: Vec<u16> = char::decode_utf16(text.iter().copied()).map(|c| self.glyph(c.map_or(0xfffd, |c| c as u32))).collect();
        let mut advances: Vec<i32> = glyphs.iter().map(|g| self.advance(*g) as i32).collect();
        let mut i = 0;
        while i + 1 < glyphs.len() {
            let (k, skip) = self.kerning(glyphs[i], glyphs[i + 1]);
            advances[i] += k;
            i += if skip { 2 } else { 1 };
        }
        glyphs.into_iter().zip(advances).map(|(g, a)| (g, a as f32 * scale)).collect()
    }

    /// A glyph's outline: closed contours of points in design units, curves flattened to lines
    /// fine enough for `scale` pixels a unit.
    fn outline(&self, glyph: u16, scale: f32, depth: u32, out: &mut Vec<Vec<(f32, f32)>>, transform: [f32; 6]) {
        let d = &self.data;
        if glyph >= self.num_glyphs || depth > 8 {
            return;
        }
        let (start, end) = if self.long_loca {
            (u32_at(d, self.loca + 4 * glyph as usize) as usize, u32_at(d, self.loca + 4 * glyph as usize + 4) as usize)
        } else {
            (u16_at(d, self.loca + 2 * glyph as usize) as usize * 2, u16_at(d, self.loca + 2 * glyph as usize + 2) as usize * 2)
        };
        if end <= start {
            return;
        }
        let g = self.glyf + start;
        let contours = i16_at(d, g);
        let map = |x: f32, y: f32| (transform[0] * x + transform[2] * y + transform[4], transform[1] * x + transform[3] * y + transform[5]);
        if contours >= 0 {
            let contours = contours as usize;
            let ends: Vec<usize> = (0..contours).map(|i| u16_at(d, g + 10 + 2 * i) as usize).collect();
            let points = ends.last().map_or(0, |e| e + 1);
            let mut at = g + 10 + 2 * contours;
            at += 2 + u16_at(d, at) as usize;
            let mut flags = Vec::with_capacity(points);
            while flags.len() < points {
                let Some(&f) = d.get(at) else { return };
                at += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let Some(&n) = d.get(at) else { return };
                    at += 1;
                    for _ in 0..n {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(points);
            let mut coords = |short: u8, same: u8| {
                let mut v = 0i32;
                let mut values = Vec::with_capacity(points);
                for f in &flags {
                    if f & short != 0 {
                        let b = *d.get(at).unwrap_or(&0) as i32;
                        at += 1;
                        v += if f & same != 0 { b } else { -b };
                    } else if f & same == 0 {
                        v += i16_at(d, at) as i32;
                        at += 2;
                    }
                    values.push(v as f32);
                }
                values
            };
            let xs = coords(2, 16);
            let ys = coords(4, 32);
            let mut first = 0;
            for &last in &ends {
                if last < first || last >= points {
                    break;
                }
                let pts: Vec<(f32, f32, bool)> = (first..=last).map(|i| (xs[i], ys[i], flags[i] & 1 != 0)).collect();
                first = last + 1;
                out.push(flatten(&pts, scale).into_iter().map(|(x, y)| map(x, y)).collect());
            }
        } else {
            let mut at = g + 10;
            loop {
                let flags = u16_at(d, at);
                let child = u16_at(d, at + 2);
                at += 4;
                let (dx, dy) = if flags & 1 != 0 {
                    at += 4;
                    (i16_at(d, at - 4) as f32, i16_at(d, at - 2) as f32)
                } else {
                    at += 2;
                    (d[at - 2] as i8 as f32, d[at - 1] as i8 as f32)
                };
                let f2dot14 = |at: usize| i16_at(d, at) as f32 / 16384.0;
                let (mut a, mut b, mut c, mut e) = (1.0, 0.0, 0.0, 1.0);
                if flags & 8 != 0 {
                    a = f2dot14(at);
                    e = a;
                    at += 2;
                } else if flags & 0x40 != 0 {
                    a = f2dot14(at);
                    e = f2dot14(at + 2);
                    at += 4;
                } else if flags & 0x80 != 0 {
                    a = f2dot14(at);
                    b = f2dot14(at + 2);
                    c = f2dot14(at + 4);
                    e = f2dot14(at + 6);
                    at += 8;
                }
                // (only offsets in x and y are taken: ARGS_ARE_XY_VALUES, as the game's fonts have)
                let t = [a, b, c, e, dx, dy];
                let combined = [
                    transform[0] * t[0] + transform[2] * t[1],
                    transform[1] * t[0] + transform[3] * t[1],
                    transform[0] * t[2] + transform[2] * t[3],
                    transform[1] * t[2] + transform[3] * t[3],
                    transform[0] * t[4] + transform[2] * t[5] + transform[4],
                    transform[1] * t[4] + transform[3] * t[5] + transform[5],
                ];
                self.outline(child, scale, depth + 1, out, combined);
                if flags & 0x20 == 0 {
                    break;
                }
            }
        }
    }

    /// A glyph drawn at `size` pixels an em on a whole-pixel origin, near DirectWrite's natural
    /// grey rendering (FW1's glyph target): each row is the outline across its middle, each
    /// pixel the share of it inside (non-zero winding). DirectWrite's glyphs are also hinted by
    /// the font's TrueType instructions, which this does not run. shortcut: no hinting (about
    /// 12/255 per pixel from DirectWrite's), port a TrueType interpreter if the text must match.
    /// Its box (left and top from the origin, y down, width and height) is DirectWrite's black
    /// box: the rows whose middles the outline spans, its columns widened by a quarter of a pixel
    /// each side (found against DirectWrite's glyphs). A glyph without pixels is 0 x 0.
    pub fn raster(&self, glyph: u16, size: f32) -> (i32, i32, u32, u32, Vec<u8>) {
        let scale = size / self.units_per_em as f32;
        let mut contours = Vec::new();
        self.outline(glyph, scale, 0, &mut contours, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        // the outline in pixels, y down
        let contours: Vec<Vec<(f32, f32)>> = contours.iter().map(|c| c.iter().map(|&(x, y)| (x * scale, -y * scale)).collect()).collect();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in contours.iter().flatten() {
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        if x0 > x1 {
            return (0, 0, 0, 0, Vec::new());
        }
        let (left, right) = ((x0 - 0.25).floor() as i32, (x1 + 0.25).ceil() as i32);
        let (top, bottom) = ((y0 - 0.5).ceil() as i32, (y1 - 0.5).floor() as i32 + 1);
        if right <= left || bottom <= top {
            return (0, 0, 0, 0, Vec::new());
        }
        let (w, h) = ((right - left) as usize, (bottom - top) as usize);
        let mut pixels = vec![0u8; w * h];
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        for row in 0..h {
            let yc = (top + row as i32) as f32 + 0.5;
            crossings.clear();
            for contour in &contours {
                for i in 0..contour.len() {
                    let (a, b) = (contour[i], contour[(i + 1) % contour.len()]);
                    if (a.1 <= yc) != (b.1 <= yc) {
                        let x = a.0 + (yc - a.1) * (b.0 - a.0) / (b.1 - a.1) - left as f32;
                        crossings.push((x, if a.1 < b.1 { 1 } else { -1 }));
                    }
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            // the row's spans inside the outline, each pixel covered by its share of them
            let mut cover = vec![0f32; w];
            let (mut winding, mut start) = (0, 0.0);
            for &(x, dir) in &crossings {
                let was = winding != 0;
                winding += dir;
                if !was && winding != 0 {
                    start = x;
                } else if was && winding == 0 {
                    let (a, b) = (start.max(0.0), x.min(w as f32));
                    let mut c = a.floor();
                    while c < b {
                        cover[c as usize] += (b.min(c + 1.0) - a.max(c)).max(0.0);
                        c += 1.0;
                    }
                }
            }
            for (pixel, c) in pixels[row * w..(row + 1) * w].iter_mut().zip(&cover) {
                *pixel = (c.min(1.0) * 255.0 + 0.5) as u8;
            }
        }
        (left, top, w as u32, h as u32, pixels)
    }
}

/// A contour of on- and off-curve points as a polyline (quadratic curves cut into pieces of
/// about a third of a pixel's sag).
fn flatten(points: &[(f32, f32, bool)], scale: f32) -> Vec<(f32, f32)> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    // start on an on-curve point (or between two off-curve ones)
    let start = points.iter().position(|p| p.2);
    let (sx, sy) = match start {
        Some(i) => (points[i].0, points[i].1),
        None => ((points[0].0 + points[1 % n].0) / 2.0, (points[0].1 + points[1 % n].1) / 2.0),
    };
    let first = start.unwrap_or(0);
    let mut out = vec![(sx, sy)];
    let mut control: Option<(f32, f32)> = None;
    let mut last = (sx, sy);
    let curve = |out: &mut Vec<(f32, f32)>, p0: (f32, f32), c: (f32, f32), p1: (f32, f32)| {
        let dx = (p0.0 - 2.0 * c.0 + p1.0) * scale;
        let dy = (p0.1 - 2.0 * c.1 + p1.1) * scale;
        let segments = ((dx * dx + dy * dy).sqrt() * 3.0).sqrt().ceil().clamp(1.0, 64.0) as usize;
        for k in 1..=segments {
            let t = k as f32 / segments as f32;
            let u = 1.0 - t;
            out.push((u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1));
        }
    };
    for k in 1..=n {
        let p = points[(first + k) % n];
        let at = (p.0, p.1);
        let end = k == n && start.is_none();
        match (p.2 || end, control) {
            (true, None) => {
                out.push(if end { (sx, sy) } else { at });
                last = *out.last().unwrap_or(&at);
            }
            (true, Some(c)) => {
                let to = if end { (sx, sy) } else { at };
                curve(&mut out, last, c, to);
                control = None;
                last = to;
            }
            (false, None) => control = Some(at),
            (false, Some(c)) => {
                let mid = ((c.0 + at.0) / 2.0, (c.1 + at.1) / 2.0);
                curve(&mut out, last, c, mid);
                last = mid;
                control = Some(at);
            }
        }
    }
    if let Some(c) = control {
        curve(&mut out, last, c, (sx, sy));
    }
    out
}

/// The lookups of GPOS's `kern` feature for Latin text (the script `latn`, else `DFLT`, its
/// default language).
fn gpos_kern_lookups(d: &[u8], gpos: usize) -> Vec<usize> {
    let scripts = gpos + u16_at(d, gpos + 4) as usize;
    let features = gpos + u16_at(d, gpos + 6) as usize;
    let lookups = gpos + u16_at(d, gpos + 8) as usize;
    let script = |tag: &[u8; 4]| (0..u16_at(d, scripts) as usize).map(|i| scripts + 2 + 6 * i).find(|&r| d.get(r..r + 4) == Some(tag)).map(|r| scripts + u16_at(d, r + 4) as usize);
    let Some(script) = script(b"latn").or_else(|| script(b"DFLT")) else { return Vec::new() };
    let lang = u16_at(d, script) as usize;
    if lang == 0 {
        return Vec::new();
    }
    let lang = script + lang;
    let mut indices = Vec::new();
    for i in 0..u16_at(d, lang + 4) as usize {
        let f = u16_at(d, lang + 6 + 2 * i) as usize;
        let r = features + 2 + 6 * f;
        if d.get(r..r + 4) == Some(b"kern") {
            let feature = features + u16_at(d, r + 4) as usize;
            for k in 0..u16_at(d, feature + 2) as usize {
                indices.push(u16_at(d, feature + 4 + 2 * k) as usize);
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices.into_iter().map(|i| lookups + u16_at(d, lookups + 2 + 2 * i) as usize).collect()
}

fn coverage(d: &[u8], at: usize, glyph: u16) -> Option<usize> {
    match u16_at(d, at) {
        1 => {
            let n = u16_at(d, at + 2) as usize;
            let (mut lo, mut hi) = (0, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                match u16_at(d, at + 4 + 2 * mid).cmp(&glyph) {
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                    std::cmp::Ordering::Equal => return Some(mid),
                }
            }
            None
        }
        2 => (0..u16_at(d, at + 2) as usize).map(|i| at + 4 + 6 * i).find(|&r| (u16_at(d, r)..=u16_at(d, r + 2)).contains(&glyph)).map(|r| (u16_at(d, r + 4) + glyph - u16_at(d, r)) as usize),
        _ => None,
    }
}

fn class(d: &[u8], at: usize, glyph: u16) -> u16 {
    match u16_at(d, at) {
        1 => {
            let start = u16_at(d, at + 2);
            let n = u16_at(d, at + 4);
            if glyph >= start && glyph - start < n {
                u16_at(d, at + 6 + 2 * (glyph - start) as usize)
            } else {
                0
            }
        }
        2 => (0..u16_at(d, at + 2) as usize).map(|i| at + 4 + 6 * i).find(|&r| (u16_at(d, r)..=u16_at(d, r + 2)).contains(&glyph)).map_or(0, |r| u16_at(d, r + 4)),
        _ => 0,
    }
}

/// A value record's x advance and its size in bytes.
fn value(d: &[u8], at: usize, format: u16) -> (i32, usize) {
    let x_advance = if format & 4 != 0 { i16_at(d, at + 2 * (format & 3).count_ones() as usize) as i32 } else { 0 };
    (x_advance, 2 * format.count_ones() as usize)
}

/// One pair-positioning lookup (type 2, or 9 around it): the first glyph's x advance change
/// and whether the second glyph is positioned too (then it starts no pair of its own).
fn pair_lookup(d: &[u8], lookup: usize, left: u16, right: u16) -> Option<(i32, bool)> {
    let kind = u16_at(d, lookup);
    for s in 0..u16_at(d, lookup + 4) as usize {
        let mut sub = lookup + u16_at(d, lookup + 6 + 2 * s) as usize;
        let mut kind = kind;
        if kind == 9 {
            kind = u16_at(d, sub + 2);
            sub += u32_at(d, sub + 4) as usize;
        }
        if kind != 2 {
            continue;
        }
        let Some(index) = coverage(d, sub + u16_at(d, sub + 2) as usize, left) else { continue };
        let (f1, f2) = (u16_at(d, sub + 4), u16_at(d, sub + 6));
        let (_, size1) = value(d, 0, f1);
        let (_, size2) = value(d, 0, f2);
        match u16_at(d, sub) {
            1 => {
                let set = sub + u16_at(d, sub + 10 + 2 * index) as usize;
                let record = 2 + size1 + size2;
                let (mut lo, mut hi) = (0, u16_at(d, set) as usize);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let r = set + 2 + record * mid;
                    match u16_at(d, r).cmp(&right) {
                        std::cmp::Ordering::Less => lo = mid + 1,
                        std::cmp::Ordering::Greater => hi = mid,
                        std::cmp::Ordering::Equal => return Some((value(d, r + 2, f1).0, f2 != 0)),
                    }
                }
            }
            2 => {
                let c1 = class(d, sub + u16_at(d, sub + 8) as usize, left) as usize;
                let c2 = class(d, sub + u16_at(d, sub + 10) as usize, right) as usize;
                let (n1, n2) = (u16_at(d, sub + 12) as usize, u16_at(d, sub + 14) as usize);
                if c1 < n1 && c2 < n2 {
                    let r = sub + 16 + (c1 * n2 + c2) * (size1 + size2);
                    return Some((value(d, r, f1).0, f2 != 0));
                }
            }
            _ => {}
        }
    }
    None
}

/// `IDWriteTextLayout::Draw` of FW1FontWrapper's layouts (no wrapping, width 0), through
/// `DrawGlyphRun`: line by line (the line height is DirectWrite's ascent + descent + line gap),
/// each line's run starts at the point (moved left by the line's width for `FW1_RIGHT`, by half
/// of it for `FW1_CENTER`; its trailing spaces are a run of their own, outside the width), the
/// first baseline is the ascent below the point, and each glyph sits on the whole pixel nearest
/// its pen position. The glyphs with their places; line breaks have none.
pub fn place(face: &Face, text: &[u16], size: f32, x: f32, y: f32, flags: u32) -> Vec<(f32, f32, u16)> {
    let scale = size / face.units_per_em as f32;
    let mut out = Vec::new();
    let mut baseline = y + face.ascent as f32 * scale;
    let mut rest = text;
    loop {
        let end = rest.iter().position(|c| *c == 10 || *c == 13).unwrap_or(rest.len());
        let line = &rest[..end];
        let glyphs = face.layout(line, size);
        let body = glyphs.len() - line.iter().rev().take_while(|c| **c == b' ' as u16).count().min(glyphs.len());
        let width: f32 = glyphs[..body].iter().map(|g| g.1).sum();
        let mut run_x = if flags & crate::fw1_atlas::FW1_RIGHT != 0 {
            x - width
        } else if flags & crate::fw1_atlas::FW1_CENTER != 0 {
            x - width / 2.0
        } else {
            x
        };
        let row = (baseline + 0.5).floor();
        let mut pen = 0.0;
        let mut scripts = char::decode_utf16(line.iter().copied()).map(|c| script(c.map_or(0xfffd, |c| c as u32)));
        let mut run_script = 0;
        for (i, (glyph, advance)) in glyphs.into_iter().enumerate() {
            // a new run where the script changes (characters of no script go with the run)
            let script = scripts.next().unwrap_or(0);
            let new_script = script != 0 && run_script != 0 && script != run_script;
            if script != 0 {
                run_script = script;
            }
            if i == 0 || i == body || new_script {
                pen = (run_x + 0.5).floor();
            }
            out.push(((pen + 0.5).floor(), row, glyph));
            pen += advance;
            run_x += advance;
        }
        if end == rest.len() {
            return out;
        }
        // CR LF is one break
        let skip = if rest[end] == 13 && rest.get(end + 1) == Some(&10) { 2 } else { 1 };
        rest = &rest[end + skip..];
        baseline += face.line_height as f32 * scale;
    }
}

/// The script of a character as DirectWrite's itemizer splits runs by it, roughly: 0 for the
/// characters of every script (spaces, digits, punctuation, symbols), else a number per script.
fn script(c: u32) -> u32 {
    let letter = char::from_u32(c).is_some_and(char::is_alphabetic);
    match c {
        _ if !letter => 0,
        0..=0x024f | 0x1e00..=0x1eff => 1,
        0x0370..=0x03ff | 0x1f00..=0x1fff => 2,
        0x0400..=0x052f => 3,
        _ => 100 + (c >> 7),
    }
}

/// The faces of a font folder, and DirectWrite's choice among them.
#[derive(Default)]
pub struct Collection {
    pub faces: Vec<Face>,
}

impl Collection {
    /// The face of a family closest to a weight and style (CSS's rule, which DirectWrite's
    /// matching follows): the style first, then the weight.
    pub fn find(&self, family: &str, weight: u16, italic: bool) -> Option<usize> {
        let of_family: Vec<usize> = (0..self.faces.len()).filter(|&i| self.faces[i].family.eq_ignore_ascii_case(family)).collect();
        let styled: Vec<usize> = of_family.iter().copied().filter(|&i| self.faces[i].italic == italic).collect();
        let pool = if styled.is_empty() { of_family } else { styled };
        let distance = |w: u16| -> (u32, i32) {
            let w = w as i32;
            let target = weight as i32;
            if w == target {
                (0, 0)
            } else if (400..=500).contains(&target) {
                // 400 and 500: up to 500 first, then down, then up
                if w > target && w <= 500 {
                    (1, w - target)
                } else if w < target {
                    (2, target - w)
                } else {
                    (3, w - target)
                }
            } else if target < 400 {
                if w < target {
                    (1, target - w)
                } else {
                    (2, w - target)
                }
            } else if w > target {
                (1, w - target)
            } else {
                (2, target - w)
            }
        };
        pool.into_iter().min_by_key(|&i| distance(self.faces[i].weight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RUSTYAC_FW1_TRUTH=<glyphs file of fw1.rs>` (its `layout` lines, DirectWrite's) and
    /// `RUSTYAC_FONTS=<content/fonts>`: how many strings this layout places the same.
    #[test]
    #[ignore]
    fn layouts_match_directwrite() {
        let truth = std::fs::read_to_string(std::env::var("RUSTYAC_FW1_TRUTH").unwrap()).unwrap();
        let folder = std::path::PathBuf::from(std::env::var("RUSTYAC_FONTS").unwrap());
        let mut collection = Collection::default();
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            if entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")) {
                collection.faces.extend(Face::parse(std::fs::read(entry.path()).unwrap()));
            }
        }
        let (mut same, mut total, mut shown) = (0, 0, 0);
        for line in truth.lines().filter(|l| l.starts_with("layout ")) {
            // layout "key" size x y flags "text" x,y,g ...
            let rest = &line[8..];
            let key_end = rest.find('"').unwrap();
            let key = &rest[..key_end];
            let mut it = rest[key_end + 2..].splitn(4, ' ');
            let (Some(Ok(size)), Some(Ok(x)), Some(Ok(y)), Some(rest)) = (it.next().map(str::parse::<f32>), it.next().map(str::parse::<f32>), it.next().map(str::parse::<f32>), it.next()) else { continue };
            let Some((Ok(flags), rest)) = rest.split_once(' ').map(|(f, r)| (u32::from_str_radix(f, 16), r)) else { continue };
            let text_end = rest.rfind("\" ").map_or(rest.len(), |i| i + 1);
            let text: String = serde_like_unquote(&rest[..text_end]);
            let want: Vec<String> = rest[text_end..].split_whitespace().map(str::to_string).collect();
            let bold = key.ends_with(" Bold");
            let base = key.trim_end_matches(" Bold");
            let italic = base.ends_with(" Italic");
            let family = base.trim_end_matches(" Italic");
            let Some(face) = collection.find(family, if bold { 700 } else { 400 }, italic) else { continue };
            let wide: Vec<u16> = text.encode_utf16().collect();
            let got: Vec<String> = place(&collection.faces[face], &wide, size, x, y, flags).iter().map(|(x, y, g)| format!("{x},{y},{g}")).collect();
            total += 1;
            if got == want {
                same += 1;
            } else if shown < 12 {
                shown += 1;
                let first = got.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
                println!("{key} {size} {flags:x} {text:?}: at {first} got {:?} want {:?} (n {} vs {})", got.get(first), want.get(first), got.len(), want.len());
            }
        }
        println!("layouts the same: {same} of {total}");
    }

    /// Rust's `{:?}` of a string, back.
    fn serde_like_unquote(quoted: &str) -> String {
        let inner = &quoted[1..quoted.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('0') => out.push('\0'),
                Some('u') => {
                    let hex: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                    out.extend(char::from_u32(u32::from_str_radix(&hex, 16).unwrap_or(0xfffd)));
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    /// A square and a triangle from points: full pixels inside, half on the diagonal.
    #[test]
    fn curves_flatten() {
        let curve = flatten(&[(0.0, 0.0, true), (10.0, 10.0, false), (20.0, 0.0, true)], 1.0);
        assert_eq!(curve.first(), Some(&(0.0, 0.0)));
        assert!(curve.contains(&(10.0, 5.0)), "{curve:?}");
    }

    /// `RUSTYAC_FW1_TRUTH=<glyphs file of fw1.rs>` (DirectWrite's glyphs) and `RUSTYAC_FONTS`:
    /// how close [`Face::raster`] draws them.
    #[test]
    #[ignore]
    fn glyphs_match_directwrite() {
        let truth = std::fs::read_to_string(std::env::var("RUSTYAC_FW1_TRUTH").unwrap()).unwrap();
        let folder = std::path::PathBuf::from(std::env::var("RUSTYAC_FONTS").unwrap());
        let mut collection = Collection::default();
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            if entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")) {
                collection.faces.extend(Face::parse(std::fs::read(entry.path()).unwrap()));
            }
        }
        let (mut glyphs, mut same_box, mut same_pixels, mut pixels, mut diff, mut shown) = (0, 0, 0, 0u64, 0u64, 0);
        for line in truth.lines().filter(|l| l.starts_with('"')) {
            let end = line[1..].find('"').unwrap() + 1;
            let key = &line[1..end];
            let f: Vec<&str> = line[end + 2..].split(' ').collect();
            let (size, glyph): (f32, u16) = (f[0].parse().unwrap(), f[1].parse().unwrap());
            let want: (i32, i32, u32, u32) = (f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap(), f[5].parse().unwrap());
            let want_px: Vec<u8> = (0..f[6].len() / 2).map(|i| u8::from_str_radix(&f[6][2 * i..2 * i + 2], 16).unwrap()).collect();
            let bold = key.ends_with(" Bold");
            let base = key.trim_end_matches(" Bold");
            let italic = base.ends_with(" Italic");
            let Some(face) = collection.find(base.trim_end_matches(" Italic"), if bold { 700 } else { 400 }, italic) else { continue };
            let (l, t, w, h, px) = collection.faces[face].raster(glyph, size);
            glyphs += 1;
            same_box += ((l, t, w, h) == want) as u32;
            same_pixels += ((l, t, w, h) == want && px == want_px) as u32;
            let (x0, y0) = (l.min(want.0), t.min(want.1));
            let (x1, y1) = ((l + w as i32).max(want.0 + want.2 as i32), (t + h as i32).max(want.1 + want.3 as i32));
            let at = |px: &[u8], (l, t, w, h): (i32, i32, u32, u32), x: i32, y: i32| if x >= l && x < l + w as i32 && y >= t && y < t + h as i32 { px[((y - t) * w as i32 + x - l) as usize] as i32 } else { 0 };
            for y in y0..y1 {
                for x in x0..x1 {
                    pixels += 1;
                    diff += (at(&px, (l, t, w, h), x, y) - at(&want_px, want, x, y)).unsigned_abs() as u64;
                }
            }
            if (l, t, w, h) != want && shown < 8 {
                shown += 1;
                println!("{key} {size} {glyph}: got {:?} want {want:?}", (l, t, w, h));
            }
        }
        println!("glyphs {glyphs}, same box {same_box}, same pixels {same_pixels}, mean |diff| {:.2}/255", diff as f64 / pixels.max(1) as f64);
    }

}
