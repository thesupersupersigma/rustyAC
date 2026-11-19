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
    fn sqrt(self) -> Self;
}

impl Real for f32 {
    const FAST: bool = false;
    const ZERO: f32 = 0.0;
    const HALF: f32 = 0.5;
    const ONE: f32 = 1.0;
    const TWO: f32 = 2.0;
    const ONE_TWELFTH: f32 = f32::from_bits(0x3daa_aaab);
    fn sqrt(self) -> f32 {
        rustyac_math::sqrtf(self)
    }
}

impl Real for f64 {
    const FAST: bool = true;
    const ZERO: f64 = 0.0;
    const HALF: f64 = 0.5;
    const ONE: f64 = 1.0;
    const TWO: f64 = 2.0;
    const ONE_TWELFTH: f64 = f64::from_bits(0x3fb5_5555_5555_5555);
    fn sqrt(self) -> f64 {
        // sqrtsd/sqrtpd, correctly rounded like CSP's
        f64::sqrt(self)
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
