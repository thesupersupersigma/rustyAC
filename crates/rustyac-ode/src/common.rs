//! Basic types of the single-precision ODE build (`include/ode/common.h`).

/// `dVector3`: three values plus one of padding.
pub type Vector3 = [f32; 4];
/// `dQuaternion`: (w, x, y, z).
pub type Quaternion = [f32; 4];
/// `dMatrix3`: 3x4, row-major (`R[4 * row + col]`), the fourth column is padding.
/// `world = R * local`.
pub type Matrix3 = [f32; 12];

/// `dPAD(a)`: row length ODE uses for an `a`-wide matrix (next multiple of four).
#[inline]
pub fn pad(a: usize) -> usize {
    if a > 1 {
        ((a - 1) | 3) + 1
    } else {
        a
    }
}
