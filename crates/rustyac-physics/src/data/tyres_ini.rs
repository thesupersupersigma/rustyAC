//! tyres.ini -> tyre parameters: a 1:1 port of `Tyre::initCompounds` (0x140280800).
//!
//! Key order, version gates, defaults and the arithmetic follow the original. Every value
//! this produces is compared against what the game's own `Tyre::init` loads by
//! `tools/tyre_oracle` (`tyre_compare params`).
//!
//! Left out, none of it reachable for `VERSION >= 10` physics:
//! * the brush slip provider's own numbers (`CF`, `CF1`, `maximum`, `maxSlip`);
//! * `generateCompoundNames` (`VERSION < 4`);
//! * `PhysicsEngine::isTyreLegal`, which drops compounds a race does not allow (a rule of
//!   the session, not of the tyre): every compound is kept.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // comparisons branch like the original on NaN

use std::path::Path;

use super::ini::IniReader;
use crate::math::{powf, tanf};
use crate::tyre::{calc_load_sens_mult, BrushSlipProvider, TyreCompoundDef, VanillaSctm};

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

    /// The left wheel's `Tyre::index` (0..3 = FL, FR, RL, RR).
    pub fn tyre_index(self) -> i32 {
        match self {
            Axle::Front => 0,
            Axle::Rear => 2,
        }
    }
}

/// Everything `Tyre::initCompounds` takes from tyres.ini for one wheel. The `Option`s are
/// values the game only overwrites when the section exists.
#[derive(Clone, Debug, Default)]
pub struct TyresIni {
    /// `[HEADER] VERSION`
    pub version: i32,
    /// `[EXPLOSION] TEMPERATURE` -> `Tyre::explosionTemperature`
    pub explosion_temperature: Option<f32>,
    /// `[VIRTUALKM] USE_LOAD` -> `Tyre::useLoadForVKM`
    pub use_load_for_vkm: Option<bool>,
    /// `[ADDITIONAL1] BLANKETS_TEMP` -> `Tyre::blanketTemperature`
    pub blanket_temperature: Option<f32>,
    /// `[ADDITIONAL1] PRESSURE_TEMPERATURE_GAIN` -> `Tyre::pressureTemperatureGain`
    pub pressure_temperature_gain: Option<f32>,
    /// `[ADDITIONAL1] CAMBER_TEMP_SPREAD_K` (only when not 0) -> `thermalModel.camberSpreadK`
    pub camber_spread_k: Option<f32>,
    /// `Tyre::compoundDefs`
    pub compound_defs: Vec<TyreCompoundDef>,
}

/// `Tyre::loadSensExpD` @ 0x140283240
pub fn load_sens_exp_d(exp: f32, mult: f32, load: f32) -> f32 {
    if load == 0.0 {
        return 0.0;
    }
    powf(load, exp) * mult / load
}

/// `Tyre::loadSensLinearD` @ 0x140283280
pub fn load_sens_linear_d(d0: f32, d1: f32, load: f32) -> f32 {
    load * 0.0005 * d1 + d0
}

/// `Tyre::initCompounds` @ 0x140280800. `data_path` is the folder holding tyres.ini and its
/// `.lut` files; `index` is the wheel (0..3 = FL, FR, RL, RR), which picks the `FRONT` or
/// `REAR` sections.
pub fn init_compounds(data_path: &Path, index: i32) -> Result<TyresIni, String> {
    let ini = IniReader::load(&data_path.join("tyres.ini"))?;
    if !ini.ready {
        return Err(format!("{}: cannot be opened", ini.filename.display()));
    }
    let section_prefix = ["FRONT", "FRONT", "REAR", "REAR"][index as usize];
    let thermal_prefix = [
        "THERMAL_FRONT",
        "THERMAL_FRONT",
        "THERMAL_REAR",
        "THERMAL_REAR",
    ][index as usize];

    let mut out = TyresIni {
        version: ini.get_int("HEADER", "VERSION")?,
        ..TyresIni::default()
    };
    if ini.has_section("EXPLOSION") {
        out.explosion_temperature = Some(ini.get_float("EXPLOSION", "TEMPERATURE")?);
    }
    if ini.has_section("VIRTUALKM") {
        out.use_load_for_vkm = Some(ini.get_int("VIRTUALKM", "USE_LOAD")? != 0);
    }
    if ini.has_section("ADDITIONAL1") {
        out.blanket_temperature = Some(ini.get_float("ADDITIONAL1", "BLANKETS_TEMP")?);
        out.pressure_temperature_gain =
            Some(ini.get_float("ADDITIONAL1", "PRESSURE_TEMPERATURE_GAIN")?);
        let spread = ini.get_float("ADDITIONAL1", "CAMBER_TEMP_SPREAD_K")?;
        if spread != 0.0 {
            out.camber_spread_k = Some(spread);
        }
    }

    for n in 0.. {
        let section = if n > 0 {
            format!("{section_prefix}_{n}")
        } else {
            section_prefix.to_string()
        };
        if !ini.has_section(&section) {
            break;
        }
        let thermal_section = if n > 0 {
            format!("{thermal_prefix}_{n}")
        } else {
            thermal_prefix.to_string()
        };
        let mut def = load_compound(&ini, data_path, &section, &thermal_section)?;
        def.index = out.compound_defs.len() as u32;
        out.compound_defs.push(def);
    }
    Ok(out)
}

/// The body of the compound loop of `Tyre::initCompounds`.
fn load_compound(
    ini: &IniReader,
    data_path: &Path,
    section: &str,
    thermal_section: &str,
) -> Result<TyreCompoundDef, String> {
    let float = |key: &str| ini.get_float(section, key);
    let thermal = |key: &str| ini.get_float(thermal_section, key);

    let mut def = TyreCompoundDef::new();
    def.section = section.to_string();
    let version = ini.get_int("HEADER", "VERSION")?;
    def.model_data.version = version;
    def.name = ini.get_string(section, "NAME");
    if version > 3 {
        def.short_name = ini.get_string(section, "SHORT_NAME");
        def.name = format!("{} ({})", def.name, def.short_name);
    }

    def.data.width = float("WIDTH")?;
    if def.data.width <= 0.0 {
        def.data.width = 0.15;
    }
    def.data.radius = float("RADIUS")?;
    def.data.rim_radius = if version < 3 {
        0.13
    } else {
        float("RIM_RADIUS")?
    };
    def.model_data.flex_k = float("FLEX")?;
    let mut friction_limit_angle = float("FRICTION_LIMIT_ANGLE")?;
    let _xmu = float("XMU")?;
    if friction_limit_angle == 0.0 {
        friction_limit_angle = 7.5;
    }
    // BrushSlipProvider(friction_limit_angle, xmu, flex): brush-only numbers, not ported
    let mut slip_provider = BrushSlipProvider::default();

    if version > 9 {
        def.model_data.cf_x_mult = float("CX_MULT")?;
        def.data.radius_raise_k = float("RADIUS_ANGULAR_K")? * 0.001;
        if ini.has_key(section, "BRAKE_DX_MOD") {
            let brake_dx_mod = float("BRAKE_DX_MOD")?;
            def.model_data.brake_dx_mod = if brake_dx_mod == 0.0 {
                1.0
            } else {
                brake_dx_mod + 1.0
            };
        }
        if ini.has_key(section, "COMBINED_FACTOR") {
            def.model_data.combined_factor = float("COMBINED_FACTOR")?;
        }
    }

    if version < 5 {
        def.model_data.dy0 = float("DY0")?;
        def.model_data.dy1 = float("DY1")?;
        def.model_data.dx0 = float("DX0")?;
        def.model_data.dx1 = float("DX1")?;
        slip_provider.asy = 0.85;
    } else {
        let fz0 = float("FZ0")?;
        def.model_data.ls_exp_x = float("LS_EXPX")?;
        def.model_data.ls_exp_y = float("LS_EXPY")?;
        def.model_data.dx0 = float("DX_REF")?;
        def.model_data.dy0 = float("DY_REF")?;
        def.model_data.ls_mult_x =
            calc_load_sens_mult(def.model_data.dx0, fz0, def.model_data.ls_exp_x);
        def.model_data.ls_mult_y =
            calc_load_sens_mult(def.model_data.dy0, fz0, def.model_data.ls_exp_y);
        slip_provider.asy = 0.92;
        slip_provider.fz0 = fz0;
        let flex_gain = float("FLEX_GAIN")?;
        slip_provider.max_slip0 = tanf(friction_limit_angle * DEG_TO_RAD);
        slip_provider.max_slip1 = tanf((flex_gain + 1.0) * friction_limit_angle * DEG_TO_RAD);
        slip_provider.version = 5;
        if ini.has_key(section, "DY_CURVE") {
            def.model_data.dy_load_curve = ini.get_curve(section, "DY_CURVE")?;
        }
        if ini.has_key(section, "DX_CURVE") {
            def.model_data.dx_load_curve = ini.get_curve(section, "DX_CURVE")?;
        }
    }
    if version > 6 {
        slip_provider.asy = float("FALLOFF_LEVEL")?;
        slip_provider.falloff_speed = float("FALLOFF_SPEED")?;
    }
    def.slip_provider = slip_provider;

    def.model_data.speed_sensitivity = float("SPEED_SENSITIVITY")?;
    def.model_data.relaxation_length = float("RELAXATION_LENGTH")?;
    def.model_data.rr0 = float("ROLLING_RESISTANCE_0")?;
    def.model_data.rr1 = float("ROLLING_RESISTANCE_1")?;
    if version == 1 {
        def.model_data.rr_sa = float("ROLLING_RESISTANCE_SA")?;
        def.model_data.rr_sr = float("ROLLING_RESISTANCE_SR")?;
    } else {
        def.model_data.rr_slip = float("ROLLING_RESISTANCE_SLIP")?;
    }
    def.model_data.camber_gain = float("CAMBER_GAIN")?;
    def.model_data.dcamber0 = float("DCAMBER_0")?;
    def.model_data.dcamber1 = float("DCAMBER_1")?;
    if def.model_data.dcamber0 == 0.0 || def.model_data.dcamber1 == 0.0 {
        def.model_data.dcamber0 = 0.1;
        def.model_data.dcamber1 = -0.8;
    }
    if ini.has_key(section, "DCAMBER_LUT") {
        def.model_data.d_camber_curve = ini.get_curve(section, "DCAMBER_LUT")?;
        def.model_data.use_smooth_d_camber_curve = ini.get_int(section, "DCAMBER_LUT_SMOOTH")? != 0;
    }

    def.data.angular_inertia = float("ANGULAR_INERTIA")?;
    def.data.d = float("DAMP")?;
    def.data.k = float("RATE")?;
    if def.data.angular_inertia == 0.0 {
        def.data.angular_inertia = 1.2;
    }
    if def.data.d == 0.0 {
        def.data.d = 400.0;
    }
    if def.data.k == 0.0 {
        def.data.k = 220000.0;
    }
    if def.model_data.dx0 == 0.0 {
        def.model_data.dx0 = def.model_data.dy0 * 1.2;
    }
    if def.model_data.dx1 == 0.0 {
        def.model_data.dx1 = def.model_data.dy1 * 0.1;
    }

    def.pressure_static = float("PRESSURE_STATIC")?;
    if def.pressure_static == 0.0 {
        def.pressure_static = 26.0;
    }
    def.model_data.pressure_ref = def.pressure_static;
    def.model_data.pressure_spring_gain = float("PRESSURE_SPRING_GAIN")?;
    if def.model_data.pressure_spring_gain == 0.0 {
        def.model_data.pressure_spring_gain = 1000.0;
    }
    def.model_data.pressure_flex_gain = float("PRESSURE_FLEX_GAIN")?;
    def.model_data.pressure_rr_gain = float("PRESSURE_RR_GAIN")?;
    def.model_data.pressure_gain_d = float("PRESSURE_D_GAIN")?;
    def.model_data.ideal_pressure = float("PRESSURE_IDEAL")?;
    if def.model_data.ideal_pressure == 0.0 {
        def.model_data.ideal_pressure = 26.0;
    }

    if ini.has_section(thermal_section) {
        def.thermal_patch_data.surface_transfer = thermal("SURFACE_TRANSFER")?;
        def.thermal_patch_data.patch_transfer = thermal("PATCH_TRANSFER")?;
        def.thermal_patch_data.patch_core_transfer = thermal("CORE_TRANSFER")?;
        def.data.thermal_friction_k = thermal("FRICTION_K")?;
        def.data.thermal_rolling_k = thermal("ROLLING_K")?;
        if version > 4 {
            def.thermal_patch_data.internal_core_transfer = thermal("INTERNAL_CORE_TRANSFER")?;
            if ini.has_key(thermal_section, "COOL_FACTOR") {
                def.thermal_patch_data.cool_factor_gain =
                    (thermal("COOL_FACTOR")? - 1.0) * 0.000324;
            }
        }
        if version > 5 {
            def.data.thermal_rolling_surface_k = thermal("SURFACE_ROLLING_K")?;
        }
        // not INIReader::getCurve: always a file name, never an inline table
        let name = ini.get_string(thermal_section, "PERFORMANCE_CURVE");
        def.thermal_performance_curve.load(&data_path.join(name))?;
    }
    let name = ini.get_string(section, "WEAR_CURVE");
    def.model_data.wear_curve.load(&data_path.join(name))?;
    def.model_data.wear_curve.scale(0.01);

    // the temperature window in which the performance curve is at (or above) 100 %
    let performance = &def.thermal_performance_curve;
    for i in 0..performance.get_count() {
        let (temperature, value) = performance.get_pair_at_index(i);
        if value >= 1.0 {
            def.data.grain_threshold = temperature;
            break;
        }
    }
    let mut i = performance.get_count() - 1;
    while i > 0 {
        let (temperature, value) = performance.get_pair_at_index(i);
        if value >= 1.0 {
            def.data.blister_threshold = temperature;
            def.data.optimum_temp = temperature;
            break;
        }
        i -= 1;
    }

    def.model_data.max_wear_mult = 100.0;
    for i in 0..def.model_data.wear_curve.get_count() {
        let (km, value) = def.model_data.wear_curve.get_pair_at_index(i);
        if !(value >= def.model_data.max_wear_mult) {
            def.model_data.max_wear_mult = value;
            def.model_data.max_wear_km = km;
        }
    }

    if version > 2 {
        // read from the thermal section's name even when that section does not exist
        def.data.blister_gamma = thermal("BLISTER_GAMMA")?;
        def.data.blister_gain = thermal("BLISTER_GAIN")?;
        def.data.grain_gamma = thermal("GRAIN_GAMMA")?;
        def.data.grain_gain = thermal("GRAIN_GAIN")?;
    }

    let d_at_3000 = if version < 5 {
        load_sens_linear_d(def.model_data.dy0, def.model_data.dy1, 3000.0)
    } else {
        load_sens_exp_d(def.model_data.ls_exp_y, def.model_data.ls_mult_y, 3000.0)
    };
    def.data.softness_index = d_at_3000 - 1.0;
    if !(def.data.softness_index >= 0.0) {
        def.data.softness_index = 0.0;
    }
    Ok(def)
}

/// One tyre compound of one axle, as far as the SCTM force model is concerned.
#[derive(Clone, Debug)]
pub struct SctmCompound {
    /// The ini section it came from (`FRONT`, `REAR_1`, ...).
    pub section: String,
    /// `NAME (SHORT_NAME)`
    pub name: String,
    /// `SHORT_NAME`
    pub short_name: String,
    pub sctm: VanillaSctm,
}

/// Loads `<car_dir>/tyres.ini` and returns the SCTM of one compound, set up the way
/// `Tyre::setCompound` does. `compound` matches the section name, `NAME` or `SHORT_NAME`
/// (case-insensitive); `None` picks the first compound (section `FRONT` / `REAR`).
pub fn load_sctm(
    car_dir: &Path,
    axle: Axle,
    compound: Option<&str>,
) -> Result<SctmCompound, String> {
    let tyres = init_compounds(car_dir, axle.tyre_index())?;
    if tyres.version < 10 {
        return Err(format!(
            "tyres.ini VERSION={}: SCTM is only used for VERSION >= 10",
            tyres.version
        ));
    }
    let def = find_compound(&tyres.compound_defs, compound).ok_or_else(|| {
        let names: Vec<&str> = tyres
            .compound_defs
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        format!(
            "compound {:?} not found for the {} axle; available: {}",
            compound.unwrap_or(""),
            axle.name(),
            names.join(", ")
        )
    })?;
    let mut sctm = VanillaSctm::default();
    def.mirror_into_sctm(&mut sctm);
    Ok(SctmCompound {
        section: def.section.clone(),
        name: def.name.clone(),
        short_name: def.short_name.clone(),
        sctm,
    })
}

/// The compound whose section, `NAME`, `NAME (SHORT_NAME)` or `SHORT_NAME` is `wanted`.
pub fn find_compound<'a>(
    defs: &'a [TyreCompoundDef],
    wanted: Option<&str>,
) -> Option<&'a TyreCompoundDef> {
    let Some(wanted) = wanted else {
        return defs.first();
    };
    defs.iter().find(|def| {
        let plain = def
            .name
            .rsplit_once(" (")
            .map_or(def.name.as_str(), |(n, _)| n);
        [&def.section, &def.name, &def.short_name]
            .iter()
            .any(|n| n.eq_ignore_ascii_case(wanted))
            || plain.eq_ignore_ascii_case(wanted)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn deg_to_rad_literal_matches_the_game() {
        assert_eq!(super::DEG_TO_RAD.to_bits(), 0x3c8ef998);
    }

    #[test]
    fn load_sensitivity_helpers() {
        assert_eq!(super::load_sens_exp_d(0.8, 9.0, 0.0), 0.0);
        assert_eq!(super::load_sens_linear_d(1.5, -0.2, 2000.0), 1.3);
    }
}
