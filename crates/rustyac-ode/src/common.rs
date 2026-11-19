// SPDX-License-Identifier: BSD-3-Clause

//! Basic types of the single-precision ODE build (`include/ode/common.h`).

use core::ops::{Add, Div, Mul, Neg, Sub};

/// `dReal`: `f32` is acs.exe's ODE 0.13.1; `f64` is CSP's ODE (master 66615b8a, Task C5c).
pub trait Real:
    Copy + PartialOrd + core::fmt::Debug + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self>
{
    /// CSP's ODE was compiled with reassociation: where its machine order differs from the
    /// source, the f64 code follows CSP's machine order (one `if R::FAST` per such place).
    const FAST: bool;
    const ZERO: Self;
    const HALF: Self;
    const ONE: Self;
    const TWO: Self;
    /// `1/12` as the compiled `dMassSetBoxTotal` multiplies by it.
    const ONE_TWELFTH: Self;
    /// `1/6` as `sinc` multiplies by it.
    const ONE_SIXTH: Self;
    fn sqrt(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn to_f64(self) -> f64;
    fn abs(self) -> Self;
    fn copysign(self, sign: Self) -> Self;
}

impl Real for f32 {
    const FAST: bool = false;
    const ZERO: f32 = 0.0;
    const HALF: f32 = 0.5;
    const ONE: f32 = 1.0;
    const TWO: f32 = 2.0;
    const ONE_TWELFTH: f32 = f32::from_bits(0x3daa_aaab);
    const ONE_SIXTH: f32 = 0.166_666_67;
    fn sqrt(self) -> f32 {
        rustyac_math::sqrtf(self)
    }
    fn sin(self) -> f32 {
        rustyac_math::sinf(self)
    }
    fn cos(self) -> f32 {
        rustyac_math::cosf(self)
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn abs(self) -> f32 {
        f32::abs(self)
    }
    fn copysign(self, sign: f32) -> f32 {
        f32::copysign(self, sign)
    }
}

impl Real for f64 {
    const FAST: bool = true;
    const ZERO: f64 = 0.0;
    const HALF: f64 = 0.5;
    const ONE: f64 = 1.0;
    const TWO: f64 = 2.0;
    const ONE_TWELFTH: f64 = f64::from_bits(0x3fb5_5555_5555_5555);
    const ONE_SIXTH: f64 = f64::from_bits(0x3fc5_5555_5555_5555);
    fn sqrt(self) -> f64 {
        // sqrtsd/sqrtpd, correctly rounded like CSP's
        f64::sqrt(self)
    }
    // CSP calls its statically linked CRT's `sin` / `cos` (dwrite.dll+0x1dbef50 / +0x1dbfb40);
    // these call the UCRT's
    fn sin(self) -> f64 {
        f64::sin(self)
    }
    fn cos(self) -> f64 {
        f64::cos(self)
    }
    fn to_f64(self) -> f64 {
        self
    }
    fn abs(self) -> f64 {
        f64::abs(self)
    }
    // CSP calls the CRT's `_copysign` (dwrite.dll+0x1dc98f0): magnitude of self, sign of `sign`
    fn copysign(self, sign: f64) -> f64 {
        f64::copysign(self, sign)
    }
}

/// `dVector3`: three values plus one of padding.
pub type Vector3<R = f32> = [R; 4];
/// `dQuaternion`: (w, x, y, z).
pub type Quaternion<R = f32> = [R; 4];
/// `dMatrix3`: 3x4, row-major (`R[4 * row + col]`), the fourth column is padding.
/// `world = R * local`.
pub type Matrix3<R = f32> = [R; 12];

/// `dPAD(a)`: row length ODE uses for an `a`-wide matrix (next multiple of four).
#[inline]
pub fn pad(a: usize) -> usize {
    if a > 1 {
        ((a - 1) | 3) + 1
    } else {
        a
    }
}
