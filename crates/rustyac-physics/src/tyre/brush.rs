// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `BrushSlipProvider` and `BrushTyreModel`: the slip curve of the old tyre path
//! (tyres.ini `VERSION` below 10, `Tyre::addTyreForces`).
//!
//! The curve turns "how much the contact patch slides" into "how much of the peak grip is
//! used": a cubic rise up to the peak at `slipMax`, then a fall towards `asy`. For `VERSION`
//! 5 and later ([`BrushTyreModel::solve_v5`]) the slip at the peak grows with the load between
//! `maxSlip0` and `maxSlip1`; before that ([`BrushTyreModel::solve`]) the stiffness `CF` falls
//! with the load by `CF1`, and the force path never falls after the peak.
//!
//! The `VERSION >= 10` path reads only `Fz0`, `maxSlip0`, `maxSlip1`, `falloffSpeed` and `asy`
//! from here: `Tyre::setCompound` copies them into the SCTM.

use crate::math::tanf;

/// AC's `BrushOutput` (8 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BrushOutput {
    /// `force`: the share of the peak force, 0..1 and a little above
    pub force: f32,
    /// `slip`: the slip divided by the slip at the peak
    pub slip: f32,
}

/// AC's `BrushTyreModel` (its `BrushTyreModelData`, 0x1c bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushTyreModel {
    /// `CF`: stiffness (from `FRICTION_LIMIT_ANGLE`); `solve` only
    pub cf: f32,
    /// `xu` (`XMU`, `VERSION` below 5 only); `solve` only
    pub xu: f32,
    /// `CF1`: change of the stiffness with load (`FLEX * -50000`); `solve` only
    pub cf1: f32,
    /// `Fz0`: the reference load, N
    pub fz0: f32,
    /// `maxSlip0`: the slip at the peak at the reference load
    pub max_slip0: f32,
    /// `maxSlip1`: the slip at the peak at twice the reference load
    pub max_slip1: f32,
    /// `falloffSpeed`
    pub falloff_speed: f32,
}

impl Default for BrushTyreModel {
    /// `BrushTyreModel::BrushTyreModel` @ 0x1402cb350.
    fn default() -> BrushTyreModel {
        BrushTyreModel { cf: 1200.0, xu: 0.0, cf1: -10.0, fz0: 2000.0, max_slip0: 0.2, max_slip1: 0.4, falloff_speed: 2.0 }
    }
}

impl BrushTyreModel {
    /// `BrushTyreModel::getCFFromSlipAngle` @ 0x1402cb380: the stiffness that puts the peak
    /// at this slip angle (degrees).
    pub fn get_cf_from_slip_angle(angle: f32) -> f32 {
        ((1.0 / tanf(angle * 0.017453)) * 3.0) * 78.125
    }

    /// `BrushTyreModel::solve` @ 0x1402cb3c0 (`VERSION` below 5). PDB order of the arguments:
    /// slip, friction, load, the share of `CF1`, the level the curve falls to.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn solve(&self, slip: f32, friction: f32, load: f32, cf1_mix: f32, asy: f32) -> BrushOutput {
        let cf = ((load * 0.0005 - 1.0) * self.cf1) * cf1_mix + self.cf;
        let third = self.cf * 0.333_333_34;
        // (`comiss third, cf` + `ja`: a NaN `cf` is kept)
        let c = if third > cf { third } else { cf };
        let k = ((c * 2.0) * 0.08) * 0.08;
        let slip_max = 1.0 / (k / (friction * 3.0));
        let nd = slip / slip_max;
        let force = if slip > slip_max {
            (1.0 - asy) / ((slip - slip_max) * 2.0 + 1.0) + asy
        } else {
            (((1.0 - nd) * (1.0 - nd)) * (k * slip)) * (friction * self.xu + 1.0) + (3.0 - nd * 2.0) * (nd * nd)
        };
        BrushOutput { force, slip: nd }
    }

    /// `BrushTyreModel::solveV5` @ 0x1402cb4e0 (`VERSION` 5 and later).
    pub fn solve_v5(&self, slip: f32, load: f32, asy: f32) -> BrushOutput {
        let peak_slip = ((load - self.fz0) / self.fz0) * (self.max_slip1 - self.max_slip0) + self.max_slip0;
        let k = ((((1.0 / peak_slip) * 3.0) * 78.125) * 2.0) * 0.0064;
        let slip_max = 1.0 / (k * 0.333_333_34);
        let nd = slip / slip_max;
        let force = if slip > slip_max {
            (1.0 - asy) / ((slip - slip_max) * self.falloff_speed + 1.0) + asy
        } else {
            ((1.0 - nd) * (1.0 - nd)) * (k * slip) + (3.0 - nd * 2.0) * (nd * nd)
        };
        BrushOutput { force, slip: nd }
    }
}

/// AC's `BrushSlipProvider` (0x38 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSlipProvider {
    /// `brushModel`
    pub brush_model: BrushTyreModel,
    /// `asy`: the level the curve falls to after the peak (`FALLOFF_LEVEL`)
    pub asy: f32,
    /// `version`: 0, or 5 for tyres.ini `VERSION` 5 and later
    pub version: i32,
    /// `maximum`: the highest value of the curve at 2000 N
    pub maximum: f32,
    /// `maxSlip`: the slip where it is reached
    pub max_slip: f32,
}

impl Default for BrushSlipProvider {
    /// `BrushSlipProvider::BrushSlipProvider()` @ 0x1402b3030.
    fn default() -> BrushSlipProvider {
        BrushSlipProvider { brush_model: BrushTyreModel::default(), asy: 1.0, version: 0, maximum: 0.0, max_slip: 0.0 }
    }
}

impl BrushSlipProvider {
    /// `BrushSlipProvider::BrushSlipProvider(maxAngle, xu, flex)` @ 0x1402b2f80. The `xu`
    /// argument is not stored (the loader writes the member afterwards).
    pub fn new(max_angle: f32, _xu: f32, flex: f32) -> BrushSlipProvider {
        let mut provider = BrushSlipProvider::default();
        provider.brush_model.cf = BrushTyreModel::get_cf_from_slip_angle(max_angle);
        provider.brush_model.cf1 = flex * -50000.0;
        provider.recompute_maximum();
        provider
    }

    /// `BrushSlipProvider::calcMaximum` @ 0x1402b3090: the highest value of the curve at this
    /// load and the slip where it is reached, by trying the slips 0, 0.001, ... below 1.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn calc_maximum(&self, load: f32) -> (f32, f32) {
        let load = if load >= 0.0 { load } else { 2000.0 };
        let (mut maximum, mut max_slip) = (0.0f32, 0.0f32);
        let mut slip = 0.0f32;
        loop {
            let out = if self.version < 5 {
                self.brush_model.solve(slip, 1.0, load, 1.0, self.asy)
            } else {
                self.brush_model.solve_v5(slip, load, self.asy)
            };
            if out.force > maximum {
                maximum = out.force;
                max_slip = slip;
            }
            slip += 0.001;
            if !(slip < 1.0) {
                break;
            }
        }
        (maximum, max_slip)
    }

    /// `BrushSlipProvider::recomputeMaximum` @ 0x1402b3230.
    pub fn recompute_maximum(&mut self) {
        (self.maximum, self.max_slip) = self.calc_maximum(2000.0);
    }

    /// `BrushSlipProvider::getSlipForce` @ 0x1402b3190. Of AC's `TyreSlipInput` only `slip`
    /// and `load` are read. Below `VERSION` 5 the curve is asked with a fall-off level of 1,
    /// whatever `use_asy` says: it never falls after the peak.
    pub fn get_slip_force(&self, slip: f32, load: f32, use_asy: bool) -> BrushOutput {
        if self.version >= 5 {
            self.brush_model.solve_v5(slip, load, if use_asy { self.asy } else { 1.0 })
        } else {
            self.brush_model.solve(slip, 1.0, load, 1.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_have_the_bits_of_the_game() {
        assert_eq!(0.0005f32.to_bits(), 0x3a03_126f);
        assert_eq!(0.333_333_34f32.to_bits(), 0x3eaa_aaab);
        assert_eq!(0.08f32.to_bits(), 0x3da3_d70a);
        assert_eq!(78.125f32.to_bits(), 0x429c_4000);
        assert_eq!(0.0064f32.to_bits(), 0x3bd1_b717);
        assert_eq!((-50000.0f32).to_bits(), 0xc743_5000);
        assert_eq!(0.017453f32.to_bits(), 0x3c8e_f998);
        assert_eq!(0.001f32.to_bits(), 0x3a83_126f);
    }

    #[test]
    fn the_curve_peaks_at_one_and_falls_to_the_level() {
        let mut provider = BrushSlipProvider { version: 5, asy: 0.9, ..BrushSlipProvider::default() };
        provider.recompute_maximum();
        assert!((provider.maximum - 1.0).abs() < 1e-3, "{}", provider.maximum);
        let far = provider.get_slip_force(50.0, 2000.0, true);
        assert!((far.force - 0.9).abs() < 0.01);
        // the old curve does not fall in the force path
        let old = BrushSlipProvider::new(7.5, 0.0, 0.0);
        assert_eq!(old.get_slip_force(50.0, 2000.0, true).force, 1.0);
    }
}
