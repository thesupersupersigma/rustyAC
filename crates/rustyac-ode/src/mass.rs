// SPDX-License-Identifier: BSD-3-Clause

//! Mass parameters: `ode/src/mass.cpp`.

use crate::common::{Matrix3, Real, Vector3};

/// `dMass`: total mass, centre of gravity in the body frame, and inertia tensor about the
/// body origin (3x4, row-major).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mass<R = f32> {
    pub mass: R,
    pub c: Vector3<R>,
    pub i: Matrix3<R>,
}

impl<R: Real> Default for Mass<R> {
    fn default() -> Self {
        Mass::zero()
    }
}

impl<R: Real> Mass<R> {
    /// `dMassSetZero` @ 0x140346d30 (CSP +0x168d4f0).
    pub fn zero() -> Self {
        Mass { mass: R::ZERO, c: [R::ZERO; 4], i: [R::ZERO; 12] }
    }

    /// `dMassSetParameters` @ 0x140346c60. (`dMassCheck`, which it calls, only prints.)
    #[allow(clippy::too_many_arguments)]
    pub fn parameters(
        mass: R,
        cgx: R,
        cgy: R,
        cgz: R,
        i11: R,
        i22: R,
        i33: R,
        i12: R,
        i13: R,
        i23: R,
    ) -> Self {
        let mut m = Mass::zero();
        m.mass = mass;
        m.c[0] = cgx;
        m.c[1] = cgy;
        m.c[2] = cgz;
        m.i[0] = i11;
        m.i[5] = i22;
        m.i[10] = i33;
        m.i[1] = i12;
        m.i[2] = i13;
        m.i[6] = i23;
        m.i[4] = i12;
        m.i[8] = i13;
        m.i[9] = i23;
        m
    }

    /// `dMassSetBoxTotal` @ 0x140346bc0 (CSP +0x168d670): a solid box of the given total mass
    /// and side lengths.
    ///
    /// The source says `total_mass / 12 * (ly*ly + lz*lz)`; the compiled code multiplies by
    /// the constant nearest to 1/12 (`0x3daaaaab`, CSP `0x3fb5555555555555`) instead of dividing.
    pub fn box_total(total_mass: R, lx: R, ly: R, lz: R) -> Self {
        let mut m = Mass::zero();
        m.mass = total_mass;
        let k = total_mass * R::ONE_TWELFTH;
        m.i[0] = (lz * lz + ly * ly) * k;
        m.i[5] = (lx * lx + lz * lz) * k;
        m.i[10] = (lx * lx + ly * ly) * k;
        m
    }
}
