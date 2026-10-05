//! tyres.ini -> [`VanillaSctm`] parameters, mirroring what `Tyre::initCompounds`
//! (0x140280800) and `Tyre::setCompound` (0x1402834e0) do for `[HEADER] VERSION >= 10`.
//!
//! The ini -> field transforms were read from the pseudo-C of those two functions (they are
//! the same ones `tools/sctm_oracle` applies); the two that involve maths go through the
//! same MSVCR120 functions the game calls.

use std::path::Path;

use super::ini::{Ini, Section};
use crate::math::tanf;
use crate::tyre::{calc_load_sens_mult, VanillaSctm};

/// The game's own degrees -> radians literal in `Tyre::initCompounds` (bits 0x3c8ef998).
const DEG_TO_RAD: f32 = 0.017453;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axle {
    Front,
    Rear,
}

impl Axle {
    pub fn parse(text: &str) -> Result<Axle, String> {
        match text.to_ascii_lowercase().as_str() {
            "front" => Ok(Axle::Front),
            "rear" => Ok(Axle::Rear),
            other => Err(format!("axle must be front or rear, not {other:?}")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axle::Front => "front",
            Axle::Rear => "rear",
        }
    }

    fn section_prefix(self) -> &'static str {
        match self {
            Axle::Front => "FRONT",
            Axle::Rear => "REAR",
        }
    }
}

/// One tyre compound of one axle, as far as the SCTM force model is concerned.
#[derive(Clone, Debug)]
pub struct SctmCompound {
    /// The ini section it came from (`FRONT`, `REAR_1`, ...).
    pub section: String,
    /// `NAME`
    pub name: String,
    /// `SHORT_NAME`
    pub short_name: String,
    pub sctm: VanillaSctm,
}

/// Loads `<car_dir>/tyres.ini`. `compound` matches `NAME` or `SHORT_NAME` (case-insensitive);
/// `None` picks the first compound (section `FRONT` / `REAR`).
pub fn load_sctm(
    car_dir: &Path,
    axle: Axle,
    compound: Option<&str>,
) -> Result<SctmCompound, String> {
    let ini = Ini::load(&car_dir.join("tyres.ini"))?;
    sctm_from_ini(&ini, axle, compound)
}

pub fn sctm_from_ini(
    ini: &Ini,
    axle: Axle,
    compound: Option<&str>,
) -> Result<SctmCompound, String> {
    let version = ini
        .section("HEADER")
        .ok_or("tyres.ini has no [HEADER]")?
        .int("VERSION")?;
    if version < 10 {
        return Err(format!(
            "tyres.ini VERSION={version}: SCTM is only used for VERSION >= 10"
        ));
    }
    let sec = find_compound(ini, axle, compound)?;
    Ok(SctmCompound {
        section: sec.name.clone(),
        name: sec.text("NAME").unwrap_or("").to_string(),
        short_name: sec.text("SHORT_NAME").unwrap_or("").to_string(),
        sctm: sctm_from_section(sec)?,
    })
}

/// Compound sections are `FRONT`, `FRONT_1`, `FRONT_2`, ... (same for `REAR`).
fn find_compound<'a>(
    ini: &'a Ini,
    axle: Axle,
    wanted: Option<&str>,
) -> Result<&'a Section, String> {
    let prefix = axle.section_prefix();
    let mut names = Vec::new();
    for n in 0.. {
        let name = if n == 0 {
            prefix.to_string()
        } else {
            format!("{prefix}_{n}")
        };
        let Some(sec) = ini.section(&name) else { break };
        let full = sec.text("NAME").unwrap_or("");
        let short = sec.text("SHORT_NAME").unwrap_or("");
        match wanted {
            None => return Ok(sec),
            Some(w) if w.eq_ignore_ascii_case(full) || w.eq_ignore_ascii_case(short) => {
                return Ok(sec)
            }
            _ => names.push(format!("\"{full}\" ({short})")),
        }
    }
    Err(format!(
        "compound {:?} not found for the {} axle; available: {}",
        wanted.unwrap_or(""),
        axle.name(),
        names.join(", ")
    ))
}

fn sctm_from_section(sec: &Section) -> Result<VanillaSctm, String> {
    for unsupported in ["DY_CURVE", "DX_CURVE", "DCAMBER_LUT"] {
        if sec.has(unsupported) {
            return Err(format!(
                "[{}] uses {unsupported}: reading lookup curves from car data is not ported yet \
                 (the model itself supports them)",
                sec.name
            ));
        }
    }

    let fz0 = sec.float("FZ0")?;
    let ls_exp_x = sec.float("LS_EXPX")?;
    let ls_exp_y = sec.float("LS_EXPY")?;
    let mut friction_limit_angle = sec.float("FRICTION_LIMIT_ANGLE")?;
    if friction_limit_angle == 0.0 {
        friction_limit_angle = 7.5;
    }
    let flex_gain = sec.float("FLEX_GAIN")?;
    let mut dcamber0 = sec.float("DCAMBER_0")?;
    let mut dcamber1 = sec.float("DCAMBER_1")?;
    if dcamber0 == 0.0 || dcamber1 == 0.0 {
        dcamber0 = 0.1;
        dcamber1 = -0.8;
    }
    let brake_dx_mod = match sec.float_opt("BRAKE_DX_MOD")? {
        Some(v) if v != 0.0 => v + 1.0,
        _ => 1.0,
    };

    Ok(VanillaSctm {
        ls_mult_y: calc_load_sens_mult(sec.float("DY_REF")?, fz0, ls_exp_y),
        ls_exp_y,
        ls_mult_x: calc_load_sens_mult(sec.float("DX_REF")?, fz0, ls_exp_x),
        ls_exp_x,
        fz0,
        max_slip0: tanf(friction_limit_angle * DEG_TO_RAD),
        max_slip1: tanf((flex_gain + 1.0) * friction_limit_angle * DEG_TO_RAD),
        asy: sec.float("FALLOFF_LEVEL")?,
        falloff_speed: sec.float("FALLOFF_SPEED")?,
        speed_sensitivity: sec.float("SPEED_SENSITIVITY")?,
        camber_gain: sec.float("CAMBER_GAIN")?,
        dcamber0,
        dcamber1,
        cf_x_mult: sec.float("CX_MULT")?,
        pressure_cf_gain: sec.float("PRESSURE_FLEX_GAIN")?,
        brake_dx_mod,
        // TyreModelData's default is 0.0, which setCompound copies over SCTM's own 2.0
        combined_factor: sec.float_opt("COMBINED_FACTOR")?.unwrap_or(0.0),
        // curves stay empty, useSmoothDCamberCurve false, dCamberBlend at the constructor's 1.0
        ..VanillaSctm::default()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn deg_to_rad_literal_matches_the_game() {
        assert_eq!(super::DEG_TO_RAD.to_bits(), 0x3c8ef998);
    }
}
