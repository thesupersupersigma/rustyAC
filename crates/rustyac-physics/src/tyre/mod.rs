//! The tyre force model slot.
//!
//! [`TyreModel`] is AC's `ITyreModel`: one `solve` call turning the state of a contact patch
//! into forces. [`VanillaSctm`] is AC's only implementation of it (`SCTM`, used by cars whose
//! tyres.ini has `VERSION >= 10`).

mod data;
pub mod oracle_csv;
mod sctm;

pub use data::{BrushSlipProvider, TyreCompoundDef, TyreData, TyreModelData, TyrePatchData};
pub use sctm::{calc_load_sens_mult, VanillaSctm};

/// `TyreModelInput` (0x30 bytes in AC). Field order and types match the original.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreModelInput {
    /// Vertical load, N.
    pub load: f32,
    /// `slipAngleRAD`
    pub slip_angle_rad: f32,
    /// `slipRatio`
    pub slip_ratio: f32,
    /// `camberRAD`
    pub camber_rad: f32,
    /// Contact patch speed, m/s.
    pub speed: f32,
    /// Grip multiplier (temperature, wear, track surface...), 1 = nominal.
    pub u: f32,
    /// `tyreIndex`: 0..3 = FL, FR, RL, RR. Not read by `SCTM::solve`.
    pub tyre_index: i32,
    /// `cpLength`: contact patch length, m.
    pub cp_length: f32,
    /// Graining level, percent.
    pub grain: f32,
    /// Blistering level, percent.
    pub blister: f32,
    /// `pressureRatio`: pressure / ideal pressure - 1.
    pub pressure_ratio: f32,
    /// `useSimpleModel`: no grip fall-off past the peak.
    pub use_simple_model: bool,
}

/// `TyreModelOutput` (0x1c bytes in AC). Field order and types match the original.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreModelOutput {
    /// `Fy`: lateral force, N.
    pub fy: f32,
    /// `Fx`: longitudinal force, N.
    pub fx: f32,
    /// `Mz`: self-aligning torque, Nm.
    pub mz: f32,
    /// Pneumatic trail, m.
    pub trail: f32,
    /// `ndSlip`: combined slip divided by the slip at peak grip.
    pub nd_slip: f32,
    /// `Dy`: lateral peak friction coefficient actually used.
    pub dy: f32,
    /// `Dx`: longitudinal peak friction coefficient actually used.
    pub dx: f32,
}

/// The tyre force model slot (AC's `ITyreModel`).
pub trait TyreModel {
    /// `ITyreModel::solve` (vtable slot 1).
    fn solve(&self, input: &TyreModelInput) -> TyreModelOutput;
}
