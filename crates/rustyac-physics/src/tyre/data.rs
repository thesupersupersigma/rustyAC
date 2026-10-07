// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The tyre's loaded data: AC's `TyreData`, `TyreModelData`, `TyrePatchData` and
//! `TyreCompoundDef`, with the constructors' default values. Fields are the PDB member names
//! in snake_case; the ini key each one comes from is in [`crate::data::tyres_ini`].

use super::VanillaSctm;
use crate::curve::Curve;

/// AC's `TyreData` (0x48 bytes): carcass, geometry and thermal/wear constants of a compound.
#[derive(Clone, Debug, PartialEq)]
pub struct TyreData {
    pub width: f32,
    pub radius: f32,
    /// Vertical spring rate, N/m.
    pub k: f32,
    /// Vertical damping, N/(m/s).
    pub d: f32,
    /// `angularInertia`, kg m^2.
    pub angular_inertia: f32,
    /// `thermalFrictionK`
    pub thermal_friction_k: f32,
    /// `thermalRollingK`
    pub thermal_rolling_k: f32,
    /// `thermalRollingSurfaceK`
    pub thermal_rolling_surface_k: f32,
    /// `grainThreshold`: below this core temperature the tyre grains.
    pub grain_threshold: f32,
    /// `blisterThreshold`: above this core temperature the tyre blisters.
    pub blister_threshold: f32,
    /// `grainGamma`
    pub grain_gamma: f32,
    /// `blisterGamma`
    pub blister_gamma: f32,
    /// `grainGain`
    pub grain_gain: f32,
    /// `blisterGain`
    pub blister_gain: f32,
    /// `rimRadius`
    pub rim_radius: f32,
    /// `optimumTemp`
    pub optimum_temp: f32,
    /// `softnessIndex`
    pub softness_index: f32,
    /// `radiusRaiseK`: radius growth per rad/s of wheel speed.
    pub radius_raise_k: f32,
}

impl Default for TyreData {
    /// The values `Tyre::Tyre` @ 0x14026dbd0 and `TyreCompoundDef::TyreCompoundDef`
    /// @ 0x14027d060 both write.
    fn default() -> TyreData {
        TyreData {
            width: 0.15,
            radius: 0.3,
            k: 220000.0,
            d: 400.0,
            angular_inertia: 1.6,
            thermal_friction_k: 0.03,
            thermal_rolling_k: 0.5,
            thermal_rolling_surface_k: 0.0,
            grain_threshold: 0.0,
            blister_threshold: 9000.0,
            grain_gamma: 1.0,
            blister_gamma: 1.0,
            grain_gain: 0.0,
            blister_gain: 0.0,
            rim_radius: 0.0,
            optimum_temp: 80.0,
            softness_index: 0.0,
            radius_raise_k: 0.0,
        }
    }
}

/// AC's `TyreModelData` (0x290 bytes): the grip-model parameters of a compound.
#[derive(Clone, Debug)]
pub struct TyreModelData {
    /// `[HEADER] VERSION`; 10 or more selects the SCTM path.
    pub version: i32,
    pub dy0: f32,
    pub dy1: f32,
    pub dx0: f32,
    pub dx1: f32,
    /// `Fz0`: never written from the ini (the ini's `FZ0` goes to the slip provider and to
    /// the SCTM), so the relaxation-length and wear formulas always see the default 2000.
    pub fz0: f32,
    /// `flexK`
    pub flex_k: f32,
    /// `speedSensitivity`
    pub speed_sensitivity: f32,
    /// `relaxationLength`
    pub relaxation_length: f32,
    pub rr0: f32,
    pub rr1: f32,
    pub rr_sa: f32,
    pub rr_sr: f32,
    pub rr_slip: f32,
    /// `camberGain`
    pub camber_gain: f32,
    /// `pressureSpringGain`
    pub pressure_spring_gain: f32,
    /// `pressureFlexGain`
    pub pressure_flex_gain: f32,
    /// `pressureRRGain`
    pub pressure_rr_gain: f32,
    /// `pressureGainD`
    pub pressure_gain_d: f32,
    /// `idealPressure`
    pub ideal_pressure: f32,
    /// `pressureRef`
    pub pressure_ref: f32,
    /// `wearCurve`: grip multiplier over virtual km.
    pub wear_curve: Curve,
    pub dcamber0: f32,
    pub dcamber1: f32,
    /// `dyLoadCurve`
    pub dy_load_curve: Curve,
    /// `dxLoadCurve`
    pub dx_load_curve: Curve,
    /// `lsMultY`
    pub ls_mult_y: f32,
    /// `lsExpY`
    pub ls_exp_y: f32,
    /// `lsMultX`
    pub ls_mult_x: f32,
    /// `lsExpX`
    pub ls_exp_x: f32,
    /// `maxWearKM`
    pub max_wear_km: f32,
    /// `maxWearMult`
    pub max_wear_mult: f32,
    pub asy: f32,
    /// `cfXmult`
    pub cf_x_mult: f32,
    /// `brakeDXMod`
    pub brake_dx_mod: f32,
    /// `dCamberCurve`
    pub d_camber_curve: Curve,
    /// `useSmoothDCamberCurve`
    pub use_smooth_d_camber_curve: bool,
    /// `combinedFactor`
    pub combined_factor: f32,
}

impl Default for TyreModelData {
    /// `TyreModelData::TyreModelData` @ 0x14026de50
    fn default() -> TyreModelData {
        TyreModelData {
            version: 0,
            dy0: 1.4,
            dy1: -0.145,
            dx0: 1.680_000_1,
            dx1: -0.0145,
            fz0: 2000.0,
            flex_k: 0.005,
            speed_sensitivity: 0.003,
            relaxation_length: 0.8,
            rr0: 500.0,
            rr1: 0.0075,
            rr_sa: 0.0,
            rr_sr: 0.0,
            rr_slip: 0.0,
            camber_gain: 0.0,
            pressure_spring_gain: 20000.0,
            pressure_flex_gain: 1.0,
            pressure_rr_gain: 0.5,
            pressure_gain_d: 0.0,
            ideal_pressure: 0.0,
            pressure_ref: 26.0,
            wear_curve: Curve::new(),
            dcamber0: 0.1,
            dcamber1: -0.8,
            dy_load_curve: Curve::new(),
            dx_load_curve: Curve::new(),
            ls_mult_y: 0.0,
            ls_exp_y: 0.0,
            ls_mult_x: 0.0,
            ls_exp_x: 0.0,
            max_wear_km: 0.0,
            max_wear_mult: 0.0,
            asy: 1.0,
            cf_x_mult: 1.0,
            brake_dx_mod: 1.0,
            d_camber_curve: Curve::new(),
            use_smooth_d_camber_curve: false,
            combined_factor: 0.0,
        }
    }
}

/// AC's `TyrePatchData` (0x14 bytes): heat-transfer coefficients of the thermal model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TyrePatchData {
    /// `surfaceTransfer`
    pub surface_transfer: f32,
    /// `patchTransfer`
    pub patch_transfer: f32,
    /// `patchCoreTransfer`
    pub patch_core_transfer: f32,
    /// `internalCoreTransfer`
    pub internal_core_transfer: f32,
    /// `coolFactorGain`
    pub cool_factor_gain: f32,
}

impl Default for TyrePatchData {
    /// As set by `TyreThermalModel::TyreThermalModel` @ 0x14026df80.
    fn default() -> TyrePatchData {
        TyrePatchData {
            surface_transfer: 0.3,
            patch_transfer: 0.2,
            patch_core_transfer: 0.2,
            internal_core_transfer: 0.004,
            cool_factor_gain: 0.0,
        }
    }
}

/// The members of AC's `BrushSlipProvider` (and its `BrushTyreModelData`) that the
/// VERSION >= 10 path reads: `Tyre::setCompound` copies them into the SCTM. The brush model
/// itself (`CF`, `xu`, `CF1`, `maximum`, `maxSlip`) belongs to the old path and is not
/// ported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSlipProvider {
    /// `brushModel.data.Fz0`
    pub fz0: f32,
    /// `brushModel.data.maxSlip0`
    pub max_slip0: f32,
    /// `brushModel.data.maxSlip1`
    pub max_slip1: f32,
    /// `brushModel.data.falloffSpeed`
    pub falloff_speed: f32,
    pub asy: f32,
    pub version: i32,
}

impl Default for BrushSlipProvider {
    /// `BrushSlipProvider::BrushSlipProvider` @ 0x1402b3030 +
    /// `BrushTyreModel::BrushTyreModel` @ 0x1402cb350
    fn default() -> BrushSlipProvider {
        BrushSlipProvider {
            fz0: 2000.0,
            max_slip0: 0.2,
            max_slip1: 0.4,
            falloff_speed: 2.0,
            asy: 1.0,
            version: 0,
        }
    }
}

/// AC's `TyreCompoundDef` (0x3f0 bytes): one compound as loaded from tyres.ini.
#[derive(Clone, Debug, Default)]
pub struct TyreCompoundDef {
    pub index: u32,
    /// `name`: `NAME (SHORT_NAME)` for VERSION > 3.
    pub name: String,
    /// `shortName`
    pub short_name: String,
    /// The ini section it was read from (`FRONT`, `REAR_1`, ...). Not an AC member.
    pub section: String,
    /// `modelData`
    pub model_data: TyreModelData,
    pub data: TyreData,
    /// `slipProvider`
    pub slip_provider: BrushSlipProvider,
    /// `pressureStatic`
    pub pressure_static: f32,
    /// `thermalPatchData`
    pub thermal_patch_data: TyrePatchData,
    /// `thermalPerformanceCurve`: grip multiplier over temperature.
    pub thermal_performance_curve: Curve,
}

impl TyreCompoundDef {
    /// `TyreCompoundDef::TyreCompoundDef` @ 0x14027d060
    pub fn new() -> TyreCompoundDef {
        TyreCompoundDef {
            pressure_static: 26.0,
            ..TyreCompoundDef::default()
        }
    }

    /// The part of `Tyre::setCompound` @ 0x1402834e0 that mirrors a compound into the SCTM.
    pub fn mirror_into_sctm(&self, sctm: &mut VanillaSctm) {
        let model = &self.model_data;
        sctm.falloff_speed = self.slip_provider.falloff_speed;
        sctm.asy = self.slip_provider.asy;
        sctm.fz0 = self.slip_provider.fz0;
        sctm.ls_exp_x = model.ls_exp_x;
        sctm.ls_exp_y = model.ls_exp_y;
        sctm.ls_mult_x = model.ls_mult_x;
        sctm.ls_mult_y = model.ls_mult_y;
        sctm.max_slip0 = self.slip_provider.max_slip0;
        sctm.max_slip1 = self.slip_provider.max_slip1;
        sctm.speed_sensitivity = model.speed_sensitivity;
        sctm.camber_gain = model.camber_gain;
        sctm.dcamber0 = model.dcamber0;
        sctm.dcamber1 = model.dcamber1;
        sctm.dy_load_curve = model.dy_load_curve.clone();
        sctm.dx_load_curve = model.dx_load_curve.clone();
        sctm.pressure_cf_gain = model.pressure_flex_gain;
        sctm.cf_x_mult = model.cf_x_mult;
        sctm.brake_dx_mod = model.brake_dx_mod;
        sctm.d_camber_curve = model.d_camber_curve.clone();
        sctm.use_smooth_d_camber_curve = model.use_smooth_d_camber_curve;
        sctm.combined_factor = model.combined_factor;
    }
}
