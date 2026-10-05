//! The game's SCTM object and its solve() argument structs.
//! Layouts: re/tyre/types/{SCTM,TyreModelInput,TyreModelOutput}.txt (from acs.pdb).

use crate::acs::{
    Acs, RVA_CURVE_ADD_VALUE, RVA_DEG2RAD_CONST, RVA_SCTM_CTOR, RVA_SCTM_SOLVE, RVA_SCTM_VFTABLE,
};
use crate::ini::Section;

const SCTM_SIZE: usize = 0x1e8;

// field offsets inside SCTM
const LS_MULT_Y: usize = 0x08;
const LS_EXP_Y: usize = 0x0c;
const LS_MULT_X: usize = 0x10;
const LS_EXP_X: usize = 0x14;
const FZ0: usize = 0x18;
const MAX_SLIP0: usize = 0x1c;
const MAX_SLIP1: usize = 0x20;
const ASY: usize = 0x24;
const FALLOFF_SPEED: usize = 0x28;
const SPEED_SENSITIVITY: usize = 0x2c;
const CAMBER_GAIN: usize = 0x30;
const DCAMBER0: usize = 0x34;
const DCAMBER1: usize = 0x38;
const CF_X_MULT: usize = 0x3c;
const DY_LOAD_CURVE: usize = 0x40;
const DX_LOAD_CURVE: usize = 0xc0;
const PRESSURE_CF_GAIN: usize = 0x140;
const BRAKE_DX_MOD: usize = 0x144;
const DCAMBER_CURVE: usize = 0x148;
const USE_SMOOTH_DCAMBER_CURVE: usize = 0x1c8;
const DCAMBER_BLEND: usize = 0x1cc;
const COMBINED_FACTOR: usize = 0x1d0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreModelInput {
    pub load: f32,
    pub slip_angle_rad: f32,
    pub slip_ratio: f32,
    pub camber_rad: f32,
    pub speed: f32,
    pub u: f32,
    pub tyre_index: i32,
    pub cp_length: f32,
    pub grain: f32,
    pub blister: f32,
    pub pressure_ratio: f32,
    pub use_simple_model: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreModelOutput {
    pub fy: f32,
    pub fx: f32,
    pub mz: f32,
    pub trail: f32,
    pub nd_slip: f32,
    pub dy: f32,
    pub dx: f32,
}

const _: () = assert!(std::mem::size_of::<TyreModelInput>() == 0x30);
const _: () = assert!(std::mem::size_of::<TyreModelOutput>() == 0x1c);

type SolveFn =
    extern "system" fn(*mut u8, *mut TyreModelOutput, *const TyreModelInput) -> *mut TyreModelOutput;

/// A live SCTM instance built by the game's own constructor.
pub struct Sctm {
    obj: *mut u8,
    solve: SolveFn,
}

/// The values copied into the object, kept for the CSV header/report.
#[derive(Debug)]
pub struct Params {
    pub fields: Vec<(&'static str, f32)>,
}

/// Values forced onto the object after the ini ones, to reach the branches of solve() that
/// the car's own data does not use. Curves are (reference, value) pairs.
#[derive(Default)]
pub struct Overrides {
    pub combined_factor: Option<f32>,
    pub dy_curve: Option<Vec<(f32, f32)>>,
    pub dx_curve: Option<Vec<(f32, f32)>>,
    pub dcamber_lut: Option<Vec<(f32, f32)>>,
    pub dcamber_smooth: bool,
}

impl Sctm {
    /// Mirrors what Tyre::initCompounds + Tyre::setCompound do for a VERSION >= 10 compound.
    pub fn new(
        acs: &Acs,
        sec: &Section,
        version: i32,
        overrides: &Overrides,
    ) -> Result<(Sctm, Params), String> {
        if version < 10 {
            return Err(format!("tyres.ini VERSION={version}: SCTM is only used for VERSION >= 10"));
        }
        for unsupported in ["DY_CURVE", "DX_CURVE", "DCAMBER_LUT"] {
            if sec.has(unsupported) {
                return Err(format!(
                    "[{}] uses {unsupported}; lookup-curve compounds are not supported by the oracle yet",
                    sec.name
                ));
            }
        }

        // 8-byte aligned, zeroed storage, never freed (the process exits after the sweeps)
        let storage = vec![0u64; SCTM_SIZE / 8].leak();
        let obj = storage.as_mut_ptr() as *mut u8;
        let ctor: extern "system" fn(*mut u8) -> *mut u8 =
            unsafe { std::mem::transmute(acs.addr(RVA_SCTM_CTOR)) };
        ctor(obj);
        let vft = unsafe { *(obj as *const usize) };
        if vft != acs.addr(RVA_SCTM_VFTABLE) {
            return Err(format!("SCTM constructor left vftable {vft:#x}, not SCTM's"));
        }

        let deg2rad = acs.read_f32(RVA_DEG2RAD_CONST);
        let fz0 = sec.float("FZ0")?;
        let ls_exp_x = sec.float("LS_EXPX")?;
        let ls_exp_y = sec.float("LS_EXPY")?;
        let mut limit_angle = sec.float("FRICTION_LIMIT_ANGLE")?;
        if limit_angle == 0.0 {
            limit_angle = 7.5;
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

        let fields: Vec<(&'static str, usize, f32)> = vec![
            ("lsMultY", LS_MULT_Y, acs.calc_load_sens_mult(sec.float("DY_REF")?, fz0, ls_exp_y)),
            ("lsExpY", LS_EXP_Y, ls_exp_y),
            ("lsMultX", LS_MULT_X, acs.calc_load_sens_mult(sec.float("DX_REF")?, fz0, ls_exp_x)),
            ("lsExpX", LS_EXP_X, ls_exp_x),
            ("Fz0", FZ0, fz0),
            ("maxSlip0", MAX_SLIP0, acs.tan(limit_angle * deg2rad)),
            ("maxSlip1", MAX_SLIP1, acs.tan((flex_gain + 1.0) * limit_angle * deg2rad)),
            ("asy", ASY, sec.float("FALLOFF_LEVEL")?),
            ("falloffSpeed", FALLOFF_SPEED, sec.float("FALLOFF_SPEED")?),
            ("speedSensitivity", SPEED_SENSITIVITY, sec.float("SPEED_SENSITIVITY")?),
            ("camberGain", CAMBER_GAIN, sec.float("CAMBER_GAIN")?),
            ("dcamber0", DCAMBER0, dcamber0),
            ("dcamber1", DCAMBER1, dcamber1),
            ("cfXmult", CF_X_MULT, sec.float("CX_MULT")?),
            ("pressureCfGain", PRESSURE_CF_GAIN, sec.float("PRESSURE_FLEX_GAIN")?),
            ("brakeDXMod", BRAKE_DX_MOD, brake_dx_mod),
            // TyreModelData's default is 0.0, which setCompound copies over SCTM's own 2.0
            (
                "combinedFactor",
                COMBINED_FACTOR,
                match overrides.combined_factor {
                    Some(v) => v,
                    None => sec.float_opt("COMBINED_FACTOR")?.unwrap_or(0.0),
                },
            ),
        ];
        unsafe {
            for &(_, offset, value) in &fields {
                (obj.add(offset) as *mut f32).write(value);
            }
            // false unless overridden: the ini has no DCAMBER_LUT
            obj.add(USE_SMOOTH_DCAMBER_CURVE).write(overrides.dcamber_smooth as u8);
            // the three Curves were built empty by the game's constructor; fill them with the
            // game's own Curve::addValue
            let add_value: extern "system" fn(*mut u8, f32, f32) =
                std::mem::transmute(acs.addr(RVA_CURVE_ADD_VALUE));
            for (offset, pairs) in [
                (DY_LOAD_CURVE, &overrides.dy_curve),
                (DX_LOAD_CURVE, &overrides.dx_curve),
                (DCAMBER_CURVE, &overrides.dcamber_lut),
            ] {
                for &(reference, value) in pairs.iter().flatten() {
                    add_value(obj.add(offset), reference, value);
                }
            }
        }
        let mut reported: Vec<(&'static str, f32)> =
            fields.iter().map(|&(name, _, value)| (name, value)).collect();
        // left at the constructor's value; setCompound never writes it
        reported.push(("dCamberBlend", unsafe { (obj.add(DCAMBER_BLEND) as *const f32).read() }));

        let solve: SolveFn = unsafe { std::mem::transmute(acs.addr(RVA_SCTM_SOLVE)) };
        Ok((Sctm { obj, solve }, Params { fields: reported }))
    }

    pub fn solve(&self, input: &TyreModelInput) -> TyreModelOutput {
        let mut out = TyreModelOutput::default();
        (self.solve)(self.obj, &mut out, input);
        out
    }
}
