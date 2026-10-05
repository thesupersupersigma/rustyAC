//! The tyre's per-step inputs and its live state: AC's `TyreInputs`, `TyreExternalInputs`
//! and `TyreStatus`. Fields are the PDB member names in snake_case (AC's spelling kept).

/// AC's `TyreInputs` (0xc bytes): torques other car systems put on the wheel, Nm.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreInputs {
    /// `brakeTorque`
    pub brake_torque: f32,
    /// `handBrakeTorque`
    pub hand_brake_torque: f32,
    /// `electricTorque`
    pub electric_torque: f32,
}

/// AC's `TyreExternalInputs` (0x10 bytes): the tyre test bench's override of load and slip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreExternalInputs {
    /// `isActive`
    pub is_active: bool,
    pub load: f32,
    /// `slipAngle`, rad.
    pub slip_angle: f32,
    /// `slipRatio`
    pub slip_ratio: f32,
}

/// AC's `TyreStatus` (0xb8 bytes): live state and outputs of one tyre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TyreStatus {
    /// Tyre deflection, m.
    pub depth: f32,
    /// Vertical load, N.
    pub load: f32,
    /// `camberRAD`
    pub camber_rad: f32,
    /// `slipAngleRAD` (after the relaxation lag).
    pub slip_angle_rad: f32,
    /// `slipRatio` (after the relaxation lag).
    pub slip_ratio: f32,
    /// `angularVelocity`, rad/s.
    pub angular_velocity: f32,
    /// `Fy`: lateral force, N.
    pub fy: f32,
    /// `Fx`: longitudinal force, N.
    pub fx: f32,
    /// `Mz`: aligning torque, Nm.
    pub mz: f32,
    /// `isLocked`
    pub is_locked: bool,
    /// `slipFactor`
    pub slip_factor: f32,
    /// `ndSlip`: combined slip over the slip at peak grip.
    pub nd_slip: f32,
    /// `distToGround`: wheel centre to contact point, m.
    pub dist_to_ground: f32,
    /// `Dy`
    pub dy: f32,
    /// `Dx`
    pub dx: f32,
    /// `D`
    pub d: f32,
    /// `dirtyLevel`
    pub dirty_level: f32,
    /// `rollingResistence`: rolling resistance torque, Nm.
    pub rolling_resistence: f32,
    /// `thermalInput`
    pub thermal_input: f32,
    /// `feedbackTorque`: net torque on the wheel this step, Nm.
    pub feedback_torque: f32,
    /// `loadedRadius`
    pub loaded_radius: f32,
    /// `effectiveRadius`
    pub effective_radius: f32,
    /// `liveRadius`
    pub live_radius: f32,
    /// `pressureStatic`, psi.
    pub pressure_static: f32,
    /// `pressureDynamic`, psi.
    pub pressure_dynamic: f32,
    /// `virtualKM`: wear distance.
    pub virtual_km: f64,
    /// `lastTempIMO`
    pub last_temp_imo: [f32; 3],
    /// `peakSA`
    pub peak_sa: f32,
    /// Graining, 0..100.
    pub grain: f64,
    /// Blistering, 0..100.
    pub blister: f64,
    /// 1 = inflated, 0 = punctured.
    pub inflation: f32,
    /// `flatSpot`, 0..1.
    pub flat_spot: f64,
    /// `lastGrain`
    pub last_grain: f32,
    /// `lastBlister`
    pub last_blister: f32,
    /// `normalizedSlideX`
    pub normalized_slide_x: f32,
    /// `normalizedSlideY`
    pub normalized_slide_y: f32,
    /// `finalDY`
    pub final_dy: f32,
    /// `wearMult`: the last value of the wear curve.
    pub wear_mult: f32,
}

impl Default for TyreStatus {
    /// What `Tyre::Tyre` @ 0x14026dbd0 leaves behind. The member initialisers set
    /// `inflation` and `wearMult` to 1, but the constructor then clears the whole struct,
    /// so both start at 0 (`Tyre::reset` puts `inflation` back to 1).
    fn default() -> TyreStatus {
        TyreStatus {
            depth: 0.0,
            load: 0.0,
            camber_rad: 0.0,
            slip_angle_rad: 0.0,
            slip_ratio: 0.0,
            angular_velocity: 0.0,
            fy: 0.0,
            fx: 0.0,
            mz: 0.0,
            is_locked: false,
            slip_factor: 0.0,
            nd_slip: 0.0,
            dist_to_ground: 0.0,
            dy: 0.0,
            dx: 0.0,
            d: 0.0,
            dirty_level: 0.0,
            rolling_resistence: 0.0,
            thermal_input: 0.0,
            feedback_torque: 0.0,
            loaded_radius: 0.0,
            effective_radius: 0.0,
            live_radius: 0.0,
            pressure_static: 26.0,
            pressure_dynamic: 26.0,
            virtual_km: 0.0,
            last_temp_imo: [-200.0; 3],
            peak_sa: 0.0,
            grain: 0.0,
            blister: 0.0,
            inflation: 0.0,
            flat_spot: 0.0,
            last_grain: 0.0,
            last_blister: 0.0,
            normalized_slide_x: 0.0,
            normalized_slide_y: 0.0,
            final_dy: 0.0,
            wear_mult: 0.0,
        }
    }
}
