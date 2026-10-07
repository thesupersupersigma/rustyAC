// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! `VanillaSctm`: 1:1 port of AC's `SCTM` tyre force model (acs.exe build 0x5a55e7a8).
//!
//! Transcribed from the disassembly of each function, because the pseudo-C regroups some
//! products. Every operation is a single f32 SSE operation in the original and a single f32
//! operation here, in the same order: no FMA, no reassociation, no f64 intermediates.
//! Comparisons are written the way the original branches, so NaN inputs take the same path.
//!
//! Fields are the PDB member names in snake_case (`lsMultY` -> `ls_mult_y`). `SCTM::solve`
//! has no named locals left in the PDB, so its locals are descriptive names of my own.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::manual_clamp)]

use super::{TyreCompoundDef, TyreModel, TyreModelInput, TyreModelOutput};
use crate::curve::Curve;
use crate::math::{cosf, powf, sinf, sqrtf, tanf};

/// AC's `SCTM` (0x1e8 bytes). The trailing members `dy0`, `dx0`, `pacE`, `pacCf`, `pacFlex`
/// are never initialised or read by the game and are left out.
#[derive(Clone, Debug)]
pub struct VanillaSctm {
    /// `lsMultY`: lateral load-sensitivity multiplier, see [`calc_load_sens_mult`].
    pub ls_mult_y: f32,
    /// `lsExpY` (`LS_EXPY`)
    pub ls_exp_y: f32,
    /// `lsMultX`
    pub ls_mult_x: f32,
    /// `lsExpX` (`LS_EXPX`)
    pub ls_exp_x: f32,
    /// `Fz0` (`FZ0`): reference load, N.
    pub fz0: f32,
    /// `maxSlip0`: tan of the peak slip angle at `Fz0`.
    pub max_slip0: f32,
    /// `maxSlip1`: tan of the peak slip angle at twice `Fz0`.
    pub max_slip1: f32,
    /// `asy` (`FALLOFF_LEVEL`): grip left far beyond the peak, as a fraction of the peak.
    pub asy: f32,
    /// `falloffSpeed` (`FALLOFF_SPEED`)
    pub falloff_speed: f32,
    /// `speedSensitivity` (`SPEED_SENSITIVITY`)
    pub speed_sensitivity: f32,
    /// `camberGain` (`CAMBER_GAIN`)
    pub camber_gain: f32,
    /// `dcamber0` (`DCAMBER_0`)
    pub dcamber0: f32,
    /// `dcamber1` (`DCAMBER_1`)
    pub dcamber1: f32,
    /// `cfXmult` (`CX_MULT`)
    pub cf_x_mult: f32,
    /// `dyLoadCurve` (`DY_CURVE`): replaces the exponent formula when not empty.
    pub dy_load_curve: Curve,
    /// `dxLoadCurve` (`DX_CURVE`)
    pub dx_load_curve: Curve,
    /// `pressureCfGain` (`PRESSURE_FLEX_GAIN`)
    pub pressure_cf_gain: f32,
    /// `brakeDXMod` (`1 + BRAKE_DX_MOD`)
    pub brake_dx_mod: f32,
    /// `dCamberCurve` (`DCAMBER_LUT`): replaces `dcamber0/1` when not empty.
    pub d_camber_curve: Curve,
    /// `useSmoothDCamberCurve` (`DCAMBER_LUT_SMOOTH`)
    pub use_smooth_d_camber_curve: bool,
    /// `dCamberBlend`: only ever the constructor's 1.0 in the game.
    pub d_camber_blend: f32,
    /// `combinedFactor` (`COMBINED_FACTOR`)
    pub combined_factor: f32,
}

impl Default for VanillaSctm {
    /// `SCTM::SCTM` @ 0x14044b8c0
    fn default() -> VanillaSctm {
        VanillaSctm {
            ls_mult_y: 0.0,
            ls_exp_y: 0.0,
            ls_mult_x: 0.0,
            ls_exp_x: 0.0,
            fz0: 0.0,
            max_slip0: 0.0,
            max_slip1: 0.0,
            asy: 0.0,
            falloff_speed: 0.0,
            speed_sensitivity: 0.0,
            camber_gain: 0.0,
            dcamber0: 0.0,
            dcamber1: 0.0,
            cf_x_mult: 1.0,
            dy_load_curve: Curve::new(),
            dx_load_curve: Curve::new(),
            pressure_cf_gain: 0.1,
            brake_dx_mod: 1.0,
            d_camber_curve: Curve::new(),
            use_smooth_d_camber_curve: false,
            d_camber_blend: 1.0,
            combined_factor: 2.0,
        }
    }
}

/// `calcLoadSensMult` @ 0x14027f770: the multiplier that makes `getStaticDY(targetLoad)`
/// come out as `targetD`. Parameter names are the PDB's.
pub fn calc_load_sens_mult(target_d: f32, target_load: f32, sens_exp: f32) -> f32 {
    (target_d * target_load) / powf(target_load, sens_exp)
}

impl VanillaSctm {
    /// `SCTM::getStaticDY` @ 0x14044bbb0
    pub fn get_static_dy(&self, load: f32) -> f32 {
        if self.dy_load_curve.get_count() < 1 {
            let mut d = 0.0;
            if load != 0.0 {
                d = powf(load, self.ls_exp_y) * self.ls_mult_y / load;
            }
            return d;
        }
        self.dy_load_curve.get_cubic_spline_value(load)
    }

    /// `SCTM::getStaticDX` @ 0x14044bb30
    pub fn get_static_dx(&self, load: f32) -> f32 {
        if self.dx_load_curve.get_count() < 1 {
            let mut d = 0.0;
            if load != 0.0 {
                d = powf(load, self.ls_exp_x) * self.ls_mult_x / load;
            }
            return d;
        }
        self.dx_load_curve.get_cubic_spline_value(load)
    }

    /// `SCTM::getPureFY` @ 0x14044ba70: the normalised force curve (0..1 of the peak) for
    /// stiffness `cf` at combined slip `slip`. The original also takes `D` and `load` and
    /// ignores both; they are kept so the signature matches. `asy` is passed in because
    /// `solve` temporarily overwrites `this->asy` for the simple model.
    fn get_pure_fy(&self, _d: f32, cf: f32, _load: f32, slip: f32, asy: f32) -> f32 {
        let stiffness = cf * 2.0 * 0.0064;
        let peak_slip = 1.0 / (stiffness / 3.0);
        // `comiss peak_slip, slip; jc`: beyond the peak, or NaN
        if !(peak_slip >= slip) {
            return (1.0 / ((slip - peak_slip) * self.falloff_speed + 1.0)) * (1.0 - asy) + asy;
        }
        let linear = stiffness * slip;
        let nd = slip / peak_slip;
        let one_minus = 1.0 - nd;
        (one_minus * one_minus) * linear + (3.0 - nd * 2.0) * (nd * nd)
    }
}

/// `x` limited to 0..1 the way the compiler emitted it: NaN passes through unchanged.
#[inline]
fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if 0.0 > x {
        0.0
    } else {
        x
    }
}

impl TyreModel for VanillaSctm {
    fn get_static_dy(&self, load: f32) -> f32 {
        VanillaSctm::get_static_dy(self, load)
    }

    fn set_compound(&mut self, def: &TyreCompoundDef) {
        def.mirror_into_sctm(self);
    }

    /// `SCTM::solve` @ 0x14044bc20
    fn solve(&self, r#in: &TyreModelInput) -> TyreModelOutput {
        let mut out = TyreModelOutput::default();
        if 0.0 >= r#in.load
            || (r#in.slip_angle_rad == 0.0 && r#in.slip_ratio == 0.0 && r#in.camber_rad == 0.0)
        {
            return out;
        }

        // the original saves this->asy, writes 1.0 into it and restores it at the end
        let asy = if r#in.use_simple_model { 1.0 } else { self.asy };

        // camber acts as extra slip angle
        let slip_angle = r#in.slip_angle_rad;
        let cambered_slip_angle = sinf(r#in.camber_rad) * self.camber_gain + slip_angle;
        let tan_slip_angle = tanf(cambered_slip_angle);
        let sin_slip_angle = sinf(slip_angle);

        // blistering: up to 20 % less peak grip
        let blister = clamp01(r#in.blister * 0.01);
        let blister_div = blister * 0.2 + 1.0;

        // peak friction from load, then the grip multiplier u
        out.dy = self.get_static_dy(r#in.load) / blister_div;
        let static_dx = self.get_static_dx(r#in.load) / blister_div;
        let mut d_y = r#in.u * out.dy;
        let mut d_x = r#in.u * static_dx;
        out.dx = static_dx;
        if 0.0 > r#in.slip_ratio {
            d_x *= self.brake_dx_mod;
        }

        // camber changes lateral peak grip; the sign says whether the wheel leans into or
        // away from the direction it is slipping in
        let camber = r#in.camber_rad;
        let mut camber_abs = camber.abs();
        if !(camber >= 0.0 && cambered_slip_angle >= 0.0)
            && (!(0.0 >= camber) || !(0.0 >= cambered_slip_angle))
        {
            camber_abs = -camber_abs;
        }
        let camber_signed = -camber_abs;
        if self.d_camber_curve.get_count() == 0 {
            let mut d_loss =
                camber_signed * self.dcamber0 - (camber_signed * camber_signed) * self.dcamber1;
            if -1.0 >= d_loss {
                d_loss = -0.9;
            }
            d_y += (d_y / (d_loss + 1.0) - d_y) * self.d_camber_blend;
        } else if !self.use_smooth_d_camber_curve {
            d_y *= self.d_camber_curve.get_value(camber_signed * 57.29578);
        } else {
            d_y *= self
                .d_camber_curve
                .get_cubic_spline_value(camber_signed * 57.29578);
        }

        // less grip the faster the rubber slides over the road
        let speed = r#in.speed;
        let slip_ratio = r#in.slip_ratio;
        let cos_slip_angle = cosf(r#in.slip_angle_rad);
        let slide_y = speed * sin_slip_angle;
        let slide_x = cos_slip_angle * (speed * slip_ratio);
        let slide_speed = sqrtf(slide_y * slide_y + slide_x * slide_x);
        // `cmova`: a NaN slip ratio also picks the limit
        let slip_ratio_limited = if slip_ratio > -0.99999 {
            slip_ratio
        } else {
            -0.99999
        };
        let speed_div = slide_speed * self.speed_sensitivity + 1.0;
        let pressure_cf = self.pressure_cf_gain * r#in.pressure_ratio + 1.0;
        d_y /= speed_div;
        d_x /= speed_div;

        // stiffness: set so the peak sits at maxSlip, which moves with load (and with u)
        let max_slip = ((r#in.load - self.fz0) / self.fz0) * (self.max_slip1 - self.max_slip0)
            + self.max_slip0;
        let max_slip = max_slip * ((r#in.u - 1.0) * 0.75 + 1.0);
        let combined_factor = self.combined_factor;
        let cf = (((1.0 / max_slip) * 3.0 * 78.125) / (r#in.grain * 0.01 + 1.0)) * pressure_cf;

        // combined slip
        let slip_div = slip_ratio_limited + 1.0;
        let slip_x = slip_ratio / slip_div;
        let slip_y = tan_slip_angle / slip_div;
        let slip = if 0.0 >= combined_factor || combined_factor == 2.0 {
            sqrtf(slip_y * slip_y + slip_x * slip_x)
        } else {
            let pow_y = powf(slip_y.abs(), combined_factor);
            let pow_x = powf(slip_x.abs(), combined_factor);
            powf(pow_y + pow_x, 1.0 / combined_factor)
        };

        let pure_x = self.get_pure_fy(d_x, cf * self.cf_x_mult, r#in.load, slip, asy);
        let force_x = pure_x * d_x;
        let pure_y = self.get_pure_fy(d_y, cf, r#in.load, slip, asy);
        out.fy = (pure_y * d_y) * (slip_y / slip) * r#in.load;
        out.fx = (slip_x / slip) * force_x * r#in.load;

        let nd_slip = slip / (1.0 / ((cf * 2.0 * 0.0064) / 3.0));
        out.nd_slip = nd_slip;

        // pneumatic trail: full at no slip, slightly negative past the peak
        let trail_shape = clamp01(1.0 - nd_slip * 0.8);
        let trail = (((3.0 - trail_shape * 2.0) * (trail_shape * trail_shape)) * 1.1 - 0.1)
            * r#in.cp_length
            * 0.12;
        // faded in below 1 m/s (the original stores the unscaled trail first, then overwrites it)
        let trail = trail * clamp01(speed);

        out.dy = d_y;
        out.dx = d_x;
        out.trail = trail;
        out.mz = -(trail * out.fy);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The f32 literals above round to the exact constants stored in acs.exe.
    #[test]
    fn literals_have_the_games_bit_patterns() {
        assert_eq!(0.01f32.to_bits(), 0x3c23d70a);
        assert_eq!(0.2f32.to_bits(), 0x3e4ccccd);
        assert_eq!(57.29578f32.to_bits(), 0x42652ee1);
        assert_eq!((-0.9f32).to_bits(), 0xbf666666);
        assert_eq!((-0.99999f32).to_bits(), 0xbf7fff58);
        assert_eq!(0.75f32.to_bits(), 0x3f400000);
        assert_eq!(78.125f32.to_bits(), 0x429c4000);
        assert_eq!(0.0064f32.to_bits(), 0x3bd1b717);
        assert_eq!(0.8f32.to_bits(), 0x3f4ccccd);
        assert_eq!(1.1f32.to_bits(), 0x3f8ccccd);
        assert_eq!(0.1f32.to_bits(), 0x3dcccccd);
        assert_eq!(0.12f32.to_bits(), 0x3df5c28f);
    }

    #[test]
    fn early_exit_is_all_zero() {
        let sctm = VanillaSctm::default();
        let zero = TyreModelOutput::default();
        let rolling = TyreModelInput {
            load: 4000.0,
            speed: 20.0,
            u: 1.0,
            ..Default::default()
        };
        assert_eq!(sctm.solve(&rolling), zero);
        let airborne = TyreModelInput {
            load: 0.0,
            slip_angle_rad: 0.1,
            ..rolling
        };
        assert_eq!(sctm.solve(&airborne), zero);
    }
}
