//! The textures YEBIS fills on the CPU at start-up (`CTextureUtil::InitializeDevice` 0x1403e6890),
//! with the game's float maths. The film grain and the CIE tables are read from the player's
//! acs.exe (file offsets of acs.exe 1.16), never copied here.

use rustyac_math::{expf, logf, powf, sqrtf};

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// `ppfx_GaussianNoise` (VA 0x140530e20): the 256x256 RGBA8 film grain.
pub fn grain(exe: &[u8]) -> Result<&[u8], String> {
    let b = exe.get(0x52fa20..0x52fa20 + 256 * 256 * 4).ok_or("yebis: acs.exe too short for the film grain")?;
    if b[..8] != [0x8d, 0x86, 0x77, 0x56, 0x88, 0x85, 0x6c, 0x67] {
        return Err("yebis: no film grain at 0x52fa20 (not acs.exe 1.16?)".into());
    }
    Ok(b)
}

/// A table of (values, wavelength) rows, 5 nm apart.
pub struct Cie {
    /// VA 0x141521000: 97 rows of X, Y, Z, wavelength.
    xyz: Vec<[f32; 4]>,
    /// VA 0x141520d60: 83 rows of photopic efficiency, wavelength.
    photopic: Vec<[f32; 2]>,
}

impl Cie {
    pub fn from_exe(exe: &[u8]) -> Result<Cie, String> {
        let x = exe.get(0x151f600..0x151f600 + 97 * 16).ok_or("yebis: acs.exe too short for the CIE table")?;
        let p = exe.get(0x151f360..0x151f360 + 83 * 8).ok_or("yebis: acs.exe too short for the CIE table")?;
        let xyz: Vec<[f32; 4]> = (0..97).map(|i| std::array::from_fn(|k| f32_at(x, i * 16 + k * 4))).collect();
        let photopic: Vec<[f32; 2]> = (0..83).map(|i| std::array::from_fn(|k| f32_at(p, i * 8 + k * 4))).collect();
        if xyz.windows(2).any(|w| w[1][3] - w[0][3] != 5.0) || photopic.windows(2).any(|w| w[1][1] - w[0][1] != 5.0) {
            return Err("yebis: no CIE tables at 0x151f600 / 0x151f360 (not acs.exe 1.16?)".into());
        }
        Ok(Cie { xyz, photopic })
    }

    /// The shared lookup of `GPUMath_Wavelength_to_CIEXYZ` 0x1403a0ac0 and
    /// `..._CIEPhotopicLuminousEfficiency` 0x1403a09f0.
    fn at(l: f32, first: f32, last: f32, n: usize) -> (usize, usize, f32) {
        let l = if l < first { first } else if last < l { last } else { l };
        let t = (l - first) / 5.0;
        let i = t.floor() as i32;
        let hi = (i + 1).min(n as i32 - 1);
        (i as usize, hi as usize, t - i as f32)
    }

    fn xyz(&self, l: f32) -> [f32; 3] {
        let (i, hi, f) = Cie::at(l, self.xyz[0][3], self.xyz[96][3], 97);
        std::array::from_fn(|k| (self.xyz[hi][k] - self.xyz[i][k]) * f + self.xyz[i][k])
    }

    fn photopic(&self, l: f32) -> f32 {
        let (i, hi, f) = Cie::at(l, self.photopic[0][1], self.photopic[82][1], 83);
        (self.photopic[hi][0] - self.photopic[i][0]) * f + self.photopic[i][0]
    }
}

/// XYZ to linear sRGB, as `GPUMath_Wavelength_to_RGB` adds it up.
fn rgb([x, y, z]: [f32; 3]) -> [f32; 3] {
    [(y * -1.53715 + x * 3.240479) + z * -0.498535 + 0.0, (x * -0.969256 + y * 1.875991) + z * 0.041556 + 0.0, (x * 0.055648 + y * -0.204043) + z * 1.057311 + 0.0]
}

/// `GPUMath_Wavelength_to_RGB` 0x1403a0db0 with the spectrum's arguments (sRGB, gamma 2.2,
/// normalised, soft clip 0.2).
fn wavelength_to_rgb(cie: &Cie, l: f32) -> [f32; 4] {
    // FUN_1403add60's table maximum: the largest channel over every row
    let mut top = [-f32::MAX; 3];
    for row in &cie.xyz {
        let c = rgb([row[0], row[1], row[2]]);
        for k in 0..3 {
            top[k] = if c[k] > top[k] { c[k] } else { top[k] };
        }
    }
    let m = top[0].max(top[1]).max(top[2]);
    let inv = 1.0 / m;
    let c = rgb(cie.xyz(l));
    let mut c = [inv * c[0], inv * c[1], inv * c[2], inv * 0.0];
    let pos = |v: f32| if 0.0 < v { v } else { 0.0 };
    let f = (pos(c[1]) * 0.7152 + pos(c[0]) * 0.2126 + pos(c[2]) * 0.0722) * 0.2;
    if f <= 0.0 {
        c = c.map(pos);
    } else {
        let e = expf(0.0 / f);
        for v in &mut c[..3] {
            let s = logf(expf(*v / f) + e) * f;
            *v = if s < *v || m < s { pos(*v) } else { s };
        }
    }
    c[3] = cie.photopic(l);
    c.map(|v| if 0.0 < v { powf(v, 0.45454544) } else { v })
}

/// `COLORT4<float>` to a byte: 1 or more is 255, then truncation, else 0 (NaN too).
fn byte(f: f32) -> u8 {
    if f >= 1.0 {
        255
    } else if 0.0 < f {
        (f * 255.9999) as i64 as u8
    } else {
        0
    }
}

/// The 256x1 spectrum, packed `GetABGR8` (bytes r, g, b, a).
pub fn spectrum(cie: &Cie) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024);
    for u in 0..256u32 {
        let c = if u == 0 || u == 255 { [0.0; 4] } else { wavelength_to_rgb(cie, u as f32 * 1.4901961 + 350.0) };
        out.extend(c.map(byte));
    }
    out
}

/// `GPUTexUtil_Build2DMipmaps` 0x1403de7e0 for RGBA8: each level the 2x2 average of the one
/// above, with an ordered-dither bias while the level is 4 or more on its long side.
pub fn mipmaps(w: u32, h: u32, base: &[u8], dither_rgb: bool, dither_a: bool) -> Vec<Vec<u8>> {
    const B: [f32; 16] = [0.0, 2.0, 0.5, 2.5, 3.0, 1.0, 3.5, 1.5, 0.75, 2.75, 0.25, 2.25, 3.75, 1.75, 3.25, 1.25];
    let levels = 32 - w.max(h).leading_zeros();
    let dim = |d: u32, l: u32| (d >> l).max(1);
    let mut out = vec![base.to_vec()];
    for l in 1..levels {
        let (lw, lh, pw, ph) = (dim(w, l), dim(h, l), dim(w, l - 1), dim(h, l - 1));
        let p = &out[l as usize - 1];
        let mut level = vec![0u8; (lw * lh * 4) as usize];
        let long = lw.max(lh) >= 4;
        for y in 0..lh {
            let (y0, y1) = if lh < ph { (2 * y, 2 * y + 1) } else { (y, y) };
            for x in 0..lw {
                let (x0, x1) = if lw < pw { (2 * x, 2 * x + 1) } else { (x, x) };
                let d = B[((y & 3) + (x & 3) * 4) as usize];
                for k in 0..4u32 {
                    let at = |xx: u32, yy: u32| p[((yy * pw + xx) * 4 + k) as usize] as u32;
                    let bias = if (if k < 3 { dither_rgb } else { dither_a }) && long { d } else { 2.0 };
                    let sum = at(x1, y1) + at(x0, y1) + at(x1, y0) + at(x0, y0);
                    level[((y * lw + x) * 4 + k) as usize] = ((bias + sum as f32) * 0.25) as i64 as u8;
                }
            }
        }
        out.push(level);
    }
    out
}

/// The 64x64 A8 radius fade, five levels.
pub fn gaussian() -> Vec<Vec<u8>> {
    (0..5)
        .map(|m| {
            let n = (64u32 >> m).max(1);
            let (iw, fh) = (1.0 / n as f32, n as f32);
            let ih = 1.0 / fh;
            let mut level = Vec::with_capacity((n * n) as usize);
            for y in 0..n {
                let fy = (y as f32 + 0.5) / fh;
                for x in 0..n {
                    let mut g = 0.0;
                    if !(1.0 <= iw || 1.0 <= ih) {
                        let nx = (((((x as f32 + 0.5) / n as f32 - iw * 0.5) * (1.0 / iw)) / (1.0 / iw - 1.0)) * 2.0) - 1.0;
                        let ny = ((((fy - ih * 0.5) * (1.0 / ih)) / (1.0 / ih - 1.0)) * 2.0) - 1.0;
                        let mut r = ny * ny + nx * nx;
                        if r != 0.0 && r != 1.0 {
                            r = sqrtf(r);
                        }
                        g = expf((r * r) / -0.34602073);
                        if 0.75 < r {
                            g = if 1.0 <= r { 0.0 } else { g * (1.0 - r) * 4.0 };
                        }
                    }
                    level.push(byte(g));
                }
            }
            level
        })
        .collect()
}
