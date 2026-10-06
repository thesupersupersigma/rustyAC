//! Mass parameters: `ode/src/mass.cpp`.

use crate::common::{Matrix3, Vector3};

/// `dMass`: total mass, centre of gravity in the body frame, and inertia tensor about the
/// body origin (3x4, row-major).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mass {
    pub mass: f32,
    pub c: Vector3,
    pub i: Matrix3,
}

impl Default for Mass {
    fn default() -> Self {
        Mass::zero()
    }
}

impl Mass {
    /// `dMassSetZero` @ 0x140346d30.
    pub fn zero() -> Mass {
        Mass { mass: 0.0, c: [0.0; 4], i: [0.0; 12] }
    }

    /// `dMassSetParameters` @ 0x140346c60. (`dMassCheck`, which it calls, only prints.)
    #[allow(clippy::too_many_arguments)]
    pub fn parameters(
        mass: f32,
        cgx: f32,
        cgy: f32,
        cgz: f32,
        i11: f32,
        i22: f32,
        i33: f32,
        i12: f32,
        i13: f32,
        i23: f32,
    ) -> Mass {
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

    /// `dMassSetBoxTotal` @ 0x140346bc0: a solid box of the given total mass and side lengths.
    ///
    /// The source says `total_mass / 12 * (ly*ly + lz*lz)`; the compiled code multiplies by
    /// the constant `0x3daaaaab` (the f32 nearest to 1/12) instead of dividing.
    pub fn box_total(total_mass: f32, lx: f32, ly: f32, lz: f32) -> Mass {
        const ONE_TWELFTH: f32 = f32::from_bits(0x3daa_aaab);
        let mut m = Mass::zero();
        m.mass = total_mass;
        let k = total_mass * ONE_TWELFTH;
        m.i[0] = (lz * lz + ly * ly) * k;
        m.i[5] = (lx * lx + lz * lz) * k;
        m.i[10] = (lx * lx + ly * ly) * k;
        m
    }
}
