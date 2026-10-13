// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The 16-bit floats of a replay frame, as the game's copy of the `half_float` library makes
//! and reads them: `float2half_impl<-1>` with the tables at 0x1404d0f70 (base) and
//! 0x1404d1370 (shift), which truncates (no rounding, overflow becomes infinity), and
//! `half2float_impl` with the tables at 0x1404ce9d0 (mantissa), 0x1404d09d0 (exponent) and
//! 0x1404d0ad0 (offset), which is exact and keeps a NaN's payload as it is. The tables in the
//! program are the library's own; their entries are computed here.

/// `base_table[i]` and `shift_table[i]` for the nine top bits of a float.
fn base_shift(i: u32) -> (u16, u32) {
    let e = (i & 0xff) as i32 - 127;
    let s: u16 = if i >= 256 { 0x8000 } else { 0 };
    if e < -24 {
        (s, 24)
    } else if e < -14 {
        ((0x0400u16 >> (-e - 14)) | s, (-e - 1) as u32)
    } else if e <= 15 {
        ((((e + 15) as u16) << 10) | s, 13)
    } else if e < 128 {
        (0x7c00 | s, 24)
    } else {
        (0x7c00 | s, 13)
    }
}

/// float to half: `base[bits >> 23] + ((bits & 0x7fffff) >> shift[bits >> 23])`, a 16-bit add.
pub fn float_to_half(f: f32) -> u16 {
    let b = f.to_bits();
    let (base, shift) = base_shift(b >> 23);
    base.wrapping_add(((b & 0x007f_ffff) >> shift) as u16)
}

fn convert_mantissa(i: u32) -> u32 {
    let mut m = i << 13;
    let mut e = 0u32;
    while m & 0x0080_0000 == 0 {
        e = e.wrapping_sub(0x0080_0000);
        m <<= 1;
    }
    m &= !0x0080_0000;
    e = e.wrapping_add(0x3880_0000);
    m | e
}

fn mantissa(i: u32) -> u32 {
    if i == 0 {
        0
    } else if i < 1024 {
        convert_mantissa(i)
    } else {
        0x3800_0000 + ((i - 1024) << 13)
    }
}

fn exponent(e: u32) -> u32 {
    match e {
        0 => 0,
        1..=30 => e << 23,
        31 => 0x4780_0000,
        32 => 0x8000_0000,
        33..=62 => 0x8000_0000 + ((e - 32) << 23),
        _ => 0xc780_0000,
    }
}

/// half to float: `mantissa[offset[h >> 10] + (h & 0x3ff)] + exponent[h >> 10]`.
pub fn half_to_float(h: u16) -> f32 {
    let e = (h >> 10) as u32;
    let offset = if e == 0 || e == 32 { 0 } else { 1024 };
    f32::from_bits(mantissa(offset + (h & 0x3ff) as u32).wrapping_add(exponent(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(float_to_half(0.0), 0);
        assert_eq!(float_to_half(-0.0), 0x8000);
        assert_eq!(float_to_half(1.0), 0x3c00);
        assert_eq!(float_to_half(-2.0), 0xc000);
        assert_eq!(float_to_half(65504.0), 0x7bff);
        // overflow is infinity, and nothing is rounded up
        assert_eq!(float_to_half(1.0e6), 0x7c00);
        assert_eq!(float_to_half(f32::from_bits(0x3f80_1fff)), 0x3c00);
        assert_eq!(half_to_float(0x3c00), 1.0);
        assert_eq!(half_to_float(0xc000), -2.0);
        assert_eq!(half_to_float(0x0001), 5.960_464_5e-8);
        assert_eq!(half_to_float(0x7c00), f32::INFINITY);
        assert_eq!(half_to_float(0x7c01).to_bits(), 0x7f80_2000);
    }

    #[test]
    fn every_half_survives_the_round_trip() {
        for h in 0..=u16::MAX {
            assert_eq!(float_to_half(half_to_float(h)), h, "{h:04x}");
        }
    }
}
