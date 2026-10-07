// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! `VanillaTyre`: 1:1 port of AC's `Tyre` for tyres.ini `VERSION >= 10` (acs.exe build
//! 0x5a55e7a8): everything `Tyre::step` does, in the original order.
//!
//! Transcribed from the disassembly. Every float operation is one f32 (or, where the
//! original widens, f64) operation in the same order; comparisons are written the way the
//! original branches, so NaN takes the same path. No FMA, no reassociation.
//!
//! Not ported (see `docs/port/tyre_step.md`): the `VERSION < 10` force path
//! (`Tyre::addTyreForces` and the brush model), the `onStepCompleted` callback and the
//! `shakeGenerator` member, neither of which `Tyre::step` uses for its own results.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_clamp,
    clippy::manual_range_contains,
    clippy::assign_op_pattern
)]

use std::path::Path;

use super::thermal::VanillaTyreThermalModel;
use super::{
    BrushSlipProvider, RayTrackCollisionProvider, SurfaceDef, Suspension, TorqueModeEx, TyreCar,
    TyreCompoundDef, TyreData, TyreExternalInputs, TyreInputs, TyreModel, TyreModelData,
    TyreModelInput, TyreStatus, VanillaSctm,
};
use crate::data::tyres_ini::init_compounds;
use crate::math::{
    acosf, asinf, atanf, cosf, dtest_inf_or_nan, fdtest_inf_or_nan, powf, sinf, sqrtf,
};
use crate::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

/// AC's `Tyre` (0x858 bytes). Fields are the PDB member names in snake_case.
///
/// The pointers the original keeps (`hub`, `rayCollisionProvider` / `rayCaster`, `car`) are
/// passed to the functions that use them instead.
pub struct VanillaTyre {
    pub inputs: TyreInputs,
    pub data: TyreData,
    /// `modelData`
    pub model_data: TyreModelData,
    pub status: TyreStatus,
    /// `worldRotation`: the hub matrix of this step with the translation zeroed.
    pub world_rotation: Mat44f,
    /// `unmodifiedContactPoint`: the raw ray hit.
    pub unmodified_contact_point: Vec3f,
    /// `contactPoint`
    pub contact_point: Vec3f,
    /// `contactNormal`
    pub contact_normal: Vec3f,
    /// `surfaceDef`: the surface under the wheel this step, `None` when airborne.
    pub surface_def: Option<SurfaceDef>,
    /// `absOverride`: brake torque multiplier written by the ABS.
    pub abs_override: f32,
    /// `thermalModel`
    pub thermal_model: VanillaTyreThermalModel,
    /// `compoundDefs`
    pub compound_defs: Vec<TyreCompoundDef>,
    /// `aiMult`: lateral grip multiplier for AI cars; above 1 also selects the simple model.
    pub ai_mult: f32,
    /// `slipProvider` (only the members the V10 path uses).
    pub slip_provider: BrushSlipProvider,
    /// `externalInputs`
    pub external_inputs: TyreExternalInputs,
    /// `roadRight`: the wheel's axle projected on the ground, unit length.
    pub road_right: Vec3f,
    /// `roadHeading`: the wheel's rolling direction projected on the ground, unit length.
    pub road_heading: Vec3f,
    /// `useLoadForVKM`
    pub use_load_for_vkm: bool,
    /// Driven wheels get their speed from the drivetrain instead of integrating it here.
    pub driven: bool,
    /// `oldAngularVelocity`
    pub old_angular_velocity: f32,
    /// `totalSlideVelocity`
    pub total_slide_velocity: f32,
    /// `localWheelRotation`: the wheel's spin as a matrix (for display).
    pub local_wheel_rotation: Mat44f,
    /// `worldPosition`: wheel centre.
    pub world_position: Vec3f,
    /// `slidingVelocityY`
    pub sliding_velocity_y: f32,
    /// `slidingVelocityX`
    pub sliding_velocity_x: f32,
    /// `roadVelocityX`
    pub road_velocity_x: f32,
    /// `roadVelocityY`
    pub road_velocity_y: f32,
    /// `totalHubVelocity`
    pub total_hub_velocity: f32,
    /// `rSlidingVelocityX`
    pub r_sliding_velocity_x: f32,
    /// `rSlidingVelocityY`
    pub r_sliding_velocity_y: f32,
    /// 0..3 = FL, FR, RL, RR.
    pub index: i32,
    /// `currentCompoundIndex`
    pub current_compound_index: i32,
    /// `tyreBlanketsOn`
    pub tyre_blankets_on: bool,
    /// `flatSpotK`
    pub flat_spot_k: f32,
    /// `tyreModel` (which in AC always points at the tyre's own `scTM`): the force model
    /// slot. Swap it with [`VanillaTyre::with_tyre_model`].
    pub tyre_model: Box<dyn TyreModel>,
    /// `explosionTemperature`
    pub explosion_temperature: f32,
    /// `blanketTemperature`
    pub blanket_temperature: f32,
    /// `pressureTemperatureGain`
    pub pressure_temperature_gain: f32,
    /// `localMX`: the tyre's reaction torque about the axle, Nm.
    pub local_mx: f32,
}

impl Default for VanillaTyre {
    fn default() -> VanillaTyre {
        VanillaTyre::new()
    }
}

/// The three-way sign the original spells out with two branches: a NaN counts as negative.
fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

/// Ordered and not zero: what `ucomiss x, 0` + `je` lets through. Not `x != 0.0`, which is
/// true for a NaN.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// `ksCalcSlipAngleRAD` @ 0x1402cb080. Parameter names are the PDB's.
pub fn ks_calc_slip_angle_rad(vy: f32, vx: f32) -> f32 {
    if ordered_nonzero(vx) {
        atanf(-(vy / vx.abs()))
    } else {
        0.0
    }
}

/// `ksCalcContactPatchLength` @ 0x1402cb040
pub fn ks_calc_contact_patch_length(radius: f32, deflection: f32) -> f32 {
    let unloaded = radius - deflection;
    if unloaded > 0.0 && radius > unloaded {
        sqrtf(radius * radius - unloaded * unloaded) * 2.0
    } else {
        0.0
    }
}

/// `ksCalcCamberRAD` @ 0x1402cafe0
pub fn ks_calc_camber_rad(road_normal: &Vec3f, sus_matrix: &Mat44f) -> f32 {
    let m = &sus_matrix.m;
    let dot = m[0][1] * road_normal.y + m[0][0] * road_normal.x + m[0][2] * road_normal.z;
    if dot > -1.0 && !(dot >= 1.0) {
        -asinf(dot)
    } else {
        -1.570_796_4
    }
}

impl VanillaTyre {
    /// `Tyre::Tyre` @ 0x14026dbd0, with AC's own SCTM as the force model.
    pub fn new() -> VanillaTyre {
        VanillaTyre::with_tyre_model(Box::new(VanillaSctm::default()))
    }

    /// `Tyre::Tyre` with another force model in the `tyreModel` slot.
    pub fn with_tyre_model(tyre_model: Box<dyn TyreModel>) -> VanillaTyre {
        VanillaTyre {
            inputs: TyreInputs::default(),
            data: TyreData::default(),
            model_data: TyreModelData::default(),
            status: TyreStatus::default(),
            world_rotation: Mat44f::default(),
            unmodified_contact_point: Vec3f::default(),
            contact_point: Vec3f::default(),
            contact_normal: Vec3f::default(),
            surface_def: None,
            abs_override: 0.0,
            thermal_model: VanillaTyreThermalModel::default(),
            compound_defs: Vec::new(),
            ai_mult: 1.0,
            slip_provider: BrushSlipProvider::default(),
            external_inputs: TyreExternalInputs::default(),
            road_right: Vec3f::default(),
            road_heading: Vec3f::default(),
            use_load_for_vkm: false,
            driven: false,
            old_angular_velocity: 0.0,
            total_slide_velocity: 0.0,
            local_wheel_rotation: Mat44f::default(),
            world_position: Vec3f::default(),
            sliding_velocity_y: 0.0,
            sliding_velocity_x: 0.0,
            road_velocity_x: 0.0,
            road_velocity_y: 0.0,
            total_hub_velocity: 0.0,
            r_sliding_velocity_x: 0.0,
            r_sliding_velocity_y: 0.0,
            index: 0,
            current_compound_index: 0,
            tyre_blankets_on: true,
            flat_spot_k: 0.15,
            tyre_model,
            explosion_temperature: 350.0,
            blanket_temperature: 80.0,
            pressure_temperature_gain: 0.16,
            local_mx: 0.0,
        }
    }

    /// `Tyre::init` @ 0x140280650. `data_path` is the folder with tyres.ini; `index` is the
    /// wheel (0..3). `ihub` is only asked for its matrix (by `reset`). PDB names: `ihub`,
    /// `dataPath`, `index`, `car`.
    pub fn init(
        &mut self,
        ihub: &mut dyn Suspension,
        data_path: &Path,
        index: i32,
        car: Option<&dyn TyreCar>,
    ) -> Result<(), String> {
        self.thermal_model.init(12, 3, car);
        self.index = index;
        self.local_wheel_rotation = Mat44f::IDENTITY;
        self.old_angular_velocity = 0.0;
        self.total_slide_velocity = 0.0;
        self.r_sliding_velocity_x = 0.0;
        self.r_sliding_velocity_y = 0.0;
        self.status.depth = 0.0;
        self.status.load = 0.0;
        self.status.slip_factor = 0.0;
        self.status.nd_slip = 0.0;
        self.surface_def = None;
        self.status.fy = 0.0;
        self.road_velocity_y = 0.0;
        self.total_hub_velocity = 0.0;
        self.sliding_velocity_y = 0.0;
        self.sliding_velocity_x = 0.0;
        self.road_velocity_x = 0.0;
        self.abs_override = 1.0;
        self.init_compounds(data_path, index)?;
        self.set_compound(0, ihub, car);
        Ok(())
    }

    /// `Tyre::initCompounds` @ 0x140280800, see [`crate::data::tyres_ini`].
    fn init_compounds(&mut self, data_path: &Path, index: i32) -> Result<(), String> {
        let tyres = init_compounds(data_path, index)?;
        if tyres.version < 10 {
            return Err(format!(
                "{}: VERSION={} uses AC's old tyre path, which is not ported (VERSION >= 10 only)",
                data_path.join("tyres.ini").display(),
                tyres.version
            ));
        }
        if tyres.compound_defs.is_empty() {
            return Err(format!(
                "{}: no compound for wheel {index}",
                data_path.join("tyres.ini").display()
            ));
        }
        if let Some(value) = tyres.explosion_temperature {
            self.explosion_temperature = value;
        }
        if let Some(value) = tyres.use_load_for_vkm {
            self.use_load_for_vkm = value;
        }
        if let Some(value) = tyres.blanket_temperature {
            self.blanket_temperature = value;
        }
        if let Some(value) = tyres.pressure_temperature_gain {
            self.pressure_temperature_gain = value;
        }
        if let Some(value) = tyres.camber_spread_k {
            self.thermal_model.camber_spread_k = value;
        }
        self.compound_defs = tyres.compound_defs;
        Ok(())
    }

    /// `Tyre::setCompound` @ 0x1402834e0. Returns false (and changes nothing) for an index
    /// that does not exist. The `evOnTyreCompoundChanged` event is not raised.
    pub fn set_compound(
        &mut self,
        cindex: i32,
        hub: &mut dyn Suspension,
        car: Option<&dyn TyreCar>,
    ) -> bool {
        if cindex < 0 || cindex as usize >= self.compound_defs.len() {
            return false;
        }
        let def = self.compound_defs[cindex as usize].clone();
        self.model_data = def.model_data.clone();
        self.data = def.data.clone();
        self.thermal_model.patch_data = def.thermal_patch_data;
        self.thermal_model.performance_curve = def.thermal_performance_curve.clone();
        self.slip_provider = def.slip_provider;
        self.current_compound_index = cindex;
        self.status.pressure_static = def.pressure_static;
        self.status.pressure_dynamic = def.pressure_static;
        self.status.dirty_level = 0.0;
        self.status.virtual_km = 0.0;
        self.thermal_model.reset(car);
        self.reset(hub, car);
        self.model_data.asy = self.slip_provider.asy;
        self.tyre_model.set_compound(&def);
        true
    }

    /// `Tyre::reset` @ 0x140283380
    pub fn reset(&mut self, hub: &mut dyn Suspension, car: Option<&dyn TyreCar>) {
        self.status.last_grain = self.status.grain as f32;
        self.status.last_blister = self.status.blister as f32;
        self.status.is_locked = true;
        self.status.inflation = 1.0;
        self.status.flat_spot = 0.0;
        self.status.slip_ratio = 0.0;
        self.status.angular_velocity = 0.0;
        self.status.slip_angle_rad = 0.0;
        self.status.fy = 0.0;
        self.status.fx = 0.0;
        self.status.mz = 0.0;
        self.status.grain = 0.0;
        self.status.blister = 0.0;
        self.r_sliding_velocity_x = 0.0;
        self.r_sliding_velocity_y = 0.0;
        self.read_hub_matrix(hub);
        // `comisd 0.001, virtualKM` + `jae`: also for a distance that is not a number
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(0.001f32 as f64 >= self.status.virtual_km) {
            self.status.last_temp_imo = self.thermal_model.get_imo();
        }
        self.status.virtual_km = 0.0;
        self.status.dirty_level = 0.0;
        self.status.feedback_torque = 0.0;
        self.abs_override = 1.0;
        self.thermal_model.reset(car);
        self.tyre_blankets_on = true;
    }

    /// `worldRotation = hub->getHubWorldMatrix()`, translation moved to `worldPosition`.
    fn read_hub_matrix(&mut self, hub: &mut dyn Suspension) {
        self.world_rotation = hub.get_hub_world_matrix();
        let row = self.world_rotation.m[3];
        self.world_position = Vec3f::new(row[0], row[1], row[2]);
        self.world_rotation.m[3][0] = 0.0;
        self.world_rotation.m[3][1] = 0.0;
        self.world_rotation.m[3][2] = 0.0;
    }

    /// `Tyre::step` @ 0x140283800 for `VERSION >= 10`.
    ///
    /// `rcp` is the tyre's `rayCollisionProvider` (`None` = no ground, the wheel is always
    /// airborne), `car` its `Car*` (`None` = the tyre test bench).
    pub fn step(
        &mut self,
        dt: f32,
        hub: &mut dyn Suspension,
        rcp: Option<&dyn RayTrackCollisionProvider>,
        mut car: Option<&mut dyn TyreCar>,
    ) {
        self.status.feedback_torque = 0.0;
        self.status.fx = 0.0;
        self.status.mz = 0.0;
        self.status.slip_factor = 0.0;
        self.status.rolling_resistence = 0.0;
        self.sliding_velocity_y = 0.0;
        self.sliding_velocity_x = 0.0;
        self.total_slide_velocity = 0.0;
        self.total_hub_velocity = 0.0;
        self.surface_def = None;
        self.read_hub_matrix(hub);

        if fdtest_inf_or_nan(self.status.angular_velocity) {
            // "TYRE ANG VELOCITY IS NAN!"
            self.status.angular_velocity = 0.0;
        }

        let torque_mode = car.as_deref().map(|car| car.torque_mode_ex());
        if !self.status.is_locked && torque_mode == Some(TorqueModeEx::ReactionTorques) {
            let torque = self.inputs.electric_torque
                + self.inputs.brake_torque
                + self.inputs.hand_brake_torque;
            let m = &self.world_rotation.m;
            let axle_torque = Vec3f::new(m[0][0] * torque, m[0][1] * torque, m[0][2] * torque);
            hub.add_torque(&axle_torque);
        }

        // one ray, from 2 m above the wheel centre straight down
        let org = Vec3f::new(
            self.world_position.x,
            self.world_position.y + 2.0,
            self.world_position.z,
        );
        let hit = rcp.and_then(|rcp| rcp.ray_cast(&org, &Vec3f::new(0.0, -1.0, 0.0), 2.0));
        match hit {
            // the wheel has to be reasonably upright to touch the ground at all
            Some(hit) if !(0.35 >= self.world_rotation.m[1][1]) => {
                self.surface_def = Some(hit.surface_def);
                self.unmodified_contact_point = hit.pos;
                let surface = hit.surface_def;
                let up = self.world_rotation.m[1];
                let (nx, ny, nz) = (hit.normal.x, hit.normal.y, hit.normal.z);
                let mut normal = hit.normal;
                let dot = ny * up[1] + nx * up[0] + nz * up[2];
                if dot > 0.96 {
                    // the wheel centre dropped onto the ground plane
                    let dx = hit.pos.x - self.world_position.x;
                    let dy = hit.pos.y - self.world_position.y;
                    let dz = hit.pos.z - self.world_position.z;
                    let distance = ny * dy + nx * dx + nz * dz;
                    self.contact_point = Vec3f::new(
                        nx * distance + self.world_position.x,
                        ny * distance + self.world_position.y,
                        nz * distance + self.world_position.z,
                    );
                } else {
                    // more than acos(0.96) between wheel and ground: turn the normal back
                    // towards the wheel's up axis and keep the raw hit point
                    let angle = if dot > -1.0 && !(dot >= 1.0) {
                        acosf(dot)
                    } else {
                        0.0
                    };
                    let angle = angle - acosf(0.96);
                    let mut axis = Vec3f::new(
                        up[2] * ny - up[1] * nz,
                        up[0] * nz - up[2] * nx,
                        up[1] * nx - up[0] * ny,
                    );
                    let length = sqrtf(axis.y * axis.y + axis.x * axis.x + axis.z * axis.z);
                    if ordered_nonzero(length) {
                        let scale = 1.0 / length;
                        axis.x *= scale;
                        axis.y *= scale;
                        axis.z *= scale;
                    }
                    let rot_mat = Mat44f::create_from_axis_angle(&axis, angle);
                    let r = &rot_mat.m;
                    normal = Vec3f::new(
                        r[0][0] * nx + r[1][0] * ny + r[2][0] * nz + r[3][0],
                        r[0][1] * nx + r[1][1] * ny + r[2][1] * nz + r[3][1],
                        r[0][2] * nx + r[1][2] * ny + r[2][2] * nz + r[3][2],
                    );
                    self.contact_point = hit.pos;
                }

                if ordered_nonzero(surface.sin_height) {
                    let s = sinf(surface.sin_length * self.contact_point.x);
                    let c = cosf(surface.sin_length * self.contact_point.z);
                    self.contact_point.y -= (s * c + 1.0) * surface.sin_height;
                }
                if ordered_nonzero(surface.granularity) {
                    let (x, z) = (self.contact_point.x, self.contact_point.z);
                    let mut y = self.contact_point.y;
                    for (l, h) in [(1.0f32, 0.005f32), (5.8, 0.005), (11.4, 0.01)] {
                        let s = sinf(l * x);
                        let c = cosf(l * z);
                        y += (s * c + 1.0) * h * -0.6;
                    }
                    self.contact_point.y = y;
                }
                self.contact_normal = normal;

                let pos = self.contact_point;
                self.add_ground_contact(&pos, &normal, hub);
                self.add_tyre_forces_v10(&pos, &normal, &surface, dt, hub, car.as_deref());

                if surface.damping > 0.0 {
                    // loose surfaces drag on the whole car body
                    if let Some(car) = car.as_deref_mut() {
                        let damping = -surface.damping;
                        let velocity = car.body_get_velocity();
                        let x = damping * velocity.x;
                        let y = damping * velocity.y;
                        let z = damping * velocity.z;
                        let mass = car.body_get_mass();
                        let force = Vec3f::new(x * mass, y * mass, z * mass);
                        car.body_add_force_at_local_pos(&force, &Vec3f::default());
                    }
                }
            }
            _ => {
                self.status.nd_slip = 0.0;
                self.status.fy = 0.0;
            }
        }

        let hand_brake_torque = self.inputs.hand_brake_torque;
        let mut brake_torque = self.inputs.brake_torque * self.abs_override;
        if !(brake_torque > hand_brake_torque) {
            brake_torque = hand_brake_torque;
        }
        let feedback_torque = self.status.rolling_resistence
            - (sign(self.status.angular_velocity) * brake_torque + self.local_mx)
            + self.inputs.electric_torque;
        self.status.feedback_torque = feedback_torque;
        // a NaN here is only reported ("NaN feedbackTorque"), not corrected

        if !self.driven {
            self.update_angular_speed(dt);
            self.step_rotation_matrix(dt, car.as_deref());
        } else {
            self.update_locked_state(dt);
            if sign(self.old_angular_velocity) != sign(self.status.angular_velocity)
                && 1.0 > self.total_hub_velocity
            {
                self.status.is_locked = true;
            }
            self.old_angular_velocity = self.status.angular_velocity;
        }

        if !(self.total_hub_velocity >= 10.0) {
            self.status.slip_factor =
                (self.total_hub_velocity * 0.1).abs() * self.status.slip_factor;
        }
        self.step_thermal_model(dt, car.as_deref());
        self.status.pressure_dynamic = (self.thermal_model.core_temp - 26.0)
            * self.pressure_temperature_gain
            + self.status.pressure_static;
        self.step_grain_blister(dt, self.total_hub_velocity, car.as_deref());
        self.step_flat_spot(dt, self.total_hub_velocity, car.as_deref());
    }

    /// `Tyre::addGroundContact` @ 0x14027d980: tyre deflection, vertical load from the
    /// carcass spring and damper, pushed into the hub. PDB names: `pos`, `normal`.
    fn add_ground_contact(&mut self, pos: &Vec3f, normal: &Vec3f, hub: &mut dyn Suspension) {
        let dx = self.world_position.x - pos.x;
        let dy = self.world_position.y - pos.y;
        let dz = self.world_position.z - pos.z;
        let squared = dy * dy + dx * dx + dz * dz;
        self.status.dist_to_ground = if ordered_nonzero(squared) {
            sqrtf(squared)
        } else {
            0.0
        };
        let world_vel = hub.get_point_velocity(pos);

        let mut radius = if ordered_nonzero(self.data.radius_raise_k) {
            self.status.angular_velocity.abs() * self.data.radius_raise_k + self.data.radius
        } else {
            self.data.radius
        };
        if !(self.status.inflation >= 1.0) {
            radius = (radius - self.data.rim_radius) * self.status.inflation + self.data.rim_radius;
        }
        self.status.live_radius = radius;
        self.status.effective_radius = radius;

        if self.status.dist_to_ground > radius {
            self.status.loaded_radius = radius;
            self.status.depth = 0.0;
            self.status.load = 0.0;
            self.status.fx = 0.0;
            self.status.mz = 0.0;
            self.status.fy = 0.0;
            self.r_sliding_velocity_x = 0.0;
            self.r_sliding_velocity_y = 0.0;
            self.status.nd_slip = 0.0;
        } else {
            let depth = radius - self.status.dist_to_ground;
            let loaded_radius = radius - depth;
            self.status.depth = depth;
            self.status.loaded_radius = loaded_radius;
            let k = if loaded_radius > self.data.rim_radius {
                let k = (self.status.pressure_dynamic - self.model_data.pressure_ref)
                    * self.model_data.pressure_spring_gain
                    + self.data.k;
                if k >= 0.0 {
                    k
                } else {
                    0.0
                }
            } else {
                // down on the rim
                200000.0
            };
            let load =
                -((world_vel.x * normal.x + world_vel.y * normal.y + world_vel.z * normal.z)
                    * self.data.d)
                    + depth * k;
            self.status.load = load;
            let force = Vec3f::new(load * normal.x, load * normal.y, load * normal.z);
            hub.add_force_at_pos(&force, pos, self.driven, false);
            if 0.0 > self.status.load {
                self.status.load = 0.0;
            }
        }
        if self.external_inputs.is_active {
            self.status.load = self.external_inputs.load;
        }
    }

    /// `Tyre::addTyreForcesV10` @ 0x14027ed60: slip, the force model, forces into the hub,
    /// rolling resistance and wear distance. PDB names: `pos`, `normal`, `surfaceDef`, `dt`.
    fn add_tyre_forces_v10(
        &mut self,
        pos: &Vec3f,
        normal: &Vec3f,
        surface_def: &SurfaceDef,
        dt: f32,
        hub: &mut dyn Suspension,
        car: Option<&dyn TyreCar>,
    ) {
        let m = self.world_rotation.m;
        // the wheel's rolling direction (-z) and axle (x), projected on the ground plane
        let (hx, hy, hz) = (-m[2][0], -m[2][1], -m[2][2]);
        let (rx, ry, rz) = (m[0][0], m[0][1], m[0][2]);
        let dot = hx * normal.x + hy * normal.y + hz * normal.z;
        self.road_heading = Vec3f::new(
            hx - dot * normal.x,
            hy - dot * normal.y,
            hz - dot * normal.z,
        );
        let dot = rx * normal.x + ry * normal.y + rz * normal.z;
        self.road_right = Vec3f::new(
            rx - dot * normal.x,
            ry - dot * normal.y,
            rz - dot * normal.z,
        );
        self.road_right.normalize();
        self.road_heading.normalize();

        let hub_avel = hub.get_hub_angular_velocity();
        let wheel_angular_speed =
            hub_avel.y * ry + hub_avel.x * rx + hub_avel.z * rz + self.status.angular_velocity;
        let hub_vel_at_cp = hub.get_point_velocity(pos);
        self.sliding_velocity_y = hub_vel_at_cp.y * self.road_right.y
            + hub_vel_at_cp.x * self.road_right.x
            + hub_vel_at_cp.z * self.road_right.z;
        self.road_velocity_x = -(hub_vel_at_cp.y * self.road_heading.y
            + hub_vel_at_cp.x * self.road_heading.x
            + hub_vel_at_cp.z * self.road_heading.z);
        let mut slip_angle = ks_calc_slip_angle_rad(self.sliding_velocity_y, self.road_velocity_x);
        self.sliding_velocity_x =
            wheel_angular_speed * self.status.effective_radius - self.road_velocity_x;
        let road_speed = self.road_velocity_x.abs();
        let mut slip_ratio = if ordered_nonzero(road_speed) {
            self.sliding_velocity_x / road_speed
        } else {
            0.0
        };
        if self.external_inputs.is_active {
            slip_ratio = self.external_inputs.slip_ratio;
            slip_angle = self.external_inputs.slip_angle;
        }

        let camber_rad = ks_calc_camber_rad(&self.contact_normal, &self.world_rotation);
        self.status.camber_rad = camber_rad;
        let load = self.status.load;
        let sliding_y_squared = self.sliding_velocity_y * self.sliding_velocity_y;
        let hub_speed = sqrtf(self.road_velocity_x * self.road_velocity_x + sliding_y_squared);
        self.total_hub_velocity = hub_speed;

        // relaxation length: longer under load, shorter the more the tyre already slides
        let nd_slip = if self.status.nd_slip > 1.0 {
            1.0
        } else if self.status.nd_slip >= 0.0 {
            self.status.nd_slip
        } else {
            0.0
        };
        let relaxation_length = self.model_data.relaxation_length;
        let loaded_length = ((load / self.model_data.fz0 * relaxation_length - relaxation_length)
            * 0.3
            + relaxation_length)
            * 2.0;
        let relaxation = (relaxation_length - loaded_length) * nd_slip + loaded_length;

        if !(hub_speed >= 1.0) {
            // nearly standing still: slip from the sliding speeds themselves
            slip_ratio = self.sliding_velocity_x * 0.5;
            if slip_ratio > 1.0 {
                slip_ratio = 1.0;
            } else if !(slip_ratio >= -1.0) {
                slip_ratio = -1.0;
            }
            slip_angle = self.sliding_velocity_y * -5.5;
            if slip_angle > 1.0 {
                slip_angle = 1.0;
            } else if !(slip_angle >= -1.0) {
                slip_angle = -1.0;
            }
        }
        let lag = |target: f32, old: f32| -> f32 {
            let difference = target - old;
            if !ordered_nonzero(relaxation) {
                return target;
            }
            let rate = hub_speed * dt / relaxation;
            if rate > 1.0 {
                target
            } else if !(rate >= 0.04) {
                0.04 * difference + old
            } else if !(rate >= 1.0) {
                rate * difference + old
            } else {
                target
            }
        };
        self.status.slip_ratio = lag(slip_ratio, self.status.slip_ratio);
        self.status.slip_angle_rad = lag(slip_angle, self.status.slip_angle_rad);
        if !(load > 0.0) {
            self.status.slip_angle_rad = 0.0;
            self.status.slip_ratio = 0.0;
        }

        let dynamic_grip_level = match car {
            Some(car) => car.dynamic_grip_level(),
            None => 1.0,
        };
        let slide_speed =
            sqrtf(self.sliding_velocity_x * self.sliding_velocity_x + sliding_y_squared);
        let corrected_d = self.get_corrected_d(1.0, true);
        let input = TyreModelInput {
            load,
            slip_angle_rad: self.status.slip_angle_rad,
            slip_ratio: self.status.slip_ratio,
            camber_rad,
            speed: hub_speed,
            u: corrected_d * surface_def.grip_mod * dynamic_grip_level,
            tyre_index: self.index,
            cp_length: ks_calc_contact_patch_length(self.status.live_radius, self.status.depth),
            grain: self.status.grain as f32,
            blister: self.status.blister as f32,
            pressure_ratio: self.status.pressure_dynamic / self.model_data.ideal_pressure - 1.0,
            use_simple_model: !(1.0 >= self.ai_mult),
        };
        let out = self.tyre_model.solve(&input);
        self.status.dy = out.dy;
        self.status.dx = out.dx;
        self.status.fy = out.fy * self.ai_mult;
        self.status.fx = -out.fx;
        self.step_dirty_level(
            dt,
            (self.status.effective_radius * self.status.angular_velocity).abs(),
            surface_def,
        );
        self.step_puncture(dt, self.total_hub_velocity, car);
        // overwrites the Mz stepDirtyLevel has just scaled
        self.status.mz = out.mz;

        let (fx, fy) = (self.status.fx, self.status.fy);
        let force = Vec3f::new(
            fy * self.road_right.x + fx * self.road_heading.x,
            fy * self.road_right.y + fx * self.road_heading.y,
            fy * self.road_right.z + fx * self.road_heading.z,
        );
        // a NaN force is only reported ("TYRE GENERATED NAN FORCE")
        match car.map(|car| car.torque_mode_ex()) {
            None | Some(TorqueModeEx::Original) => {
                hub.add_force_at_pos(&force, pos, self.driven, true);
                self.local_mx = -(self.status.loaded_radius * self.status.fx);
            }
            Some(mode) => self.add_tyre_force_to_hub(pos, &force, mode, hub),
        }
        let aligning = Vec3f::new(out.mz * normal.x, out.mz * normal.y, out.mz * normal.z);
        hub.add_torque(&aligning);

        let angular_velocity = self.status.angular_velocity;
        let angular_speed = angular_velocity.abs();
        if angular_speed > 1.0 {
            let effective_radius = self.status.effective_radius;
            let rolling_speed = effective_radius * angular_velocity;
            let pressure_dynamic = self.status.pressure_dynamic;
            let mut pressure_mult = (self.model_data.ideal_pressure / pressure_dynamic - 1.0)
                * self.model_data.pressure_rr_gain
                + 1.0;
            if !(pressure_dynamic > 0.0) {
                pressure_mult = 0.0;
            }
            let mut resistance = (rolling_speed * rolling_speed * self.model_data.rr1
                + self.model_data.rr0)
                * sign(rolling_speed)
                * pressure_mult;
            if angular_speed > 20.0 {
                let slip_term = if self.model_data.version >= 2 {
                    let nd_slip = self.status.nd_slip;
                    let clamped = if nd_slip > 1.0 {
                        1.0
                    } else if nd_slip >= 0.0 {
                        nd_slip
                    } else {
                        0.0
                    };
                    clamped * self.model_data.rr_slip * pressure_mult
                } else {
                    let sr_gain = pressure_mult * self.model_data.rr_sr;
                    let sa_gain = pressure_mult * self.model_data.rr_sa;
                    let angle_term = (self.status.slip_angle_rad * 57.295_78).abs() * sa_gain;
                    let ratio = self.status.slip_ratio.abs();
                    let clamped = if ratio > 1.0 {
                        1.0
                    } else if ratio >= 0.0 {
                        ratio
                    } else {
                        0.0
                    };
                    clamped * sr_gain + angle_term
                };
                resistance *= slip_term * 0.001 + 1.0;
            }
            self.status.rolling_resistence =
                -(self.status.load * 0.001 * resistance * effective_radius);
        }

        if let Some(car) = car {
            let load_mult = if self.use_load_for_vkm {
                self.status.load / self.model_data.fz0
            } else {
                1.0
            };
            let distance = slide_speed * dt * car.tyre_consumption_rate() * load_mult;
            self.status.virtual_km = distance as f64 * 0.001 + self.status.virtual_km;
        }
        self.status.d = self.tyre_model.get_static_dy(self.status.load);
        self.status.nd_slip = out.nd_slip;
    }

    /// `Tyre::addTyreForceToHub` @ 0x14027dc00: for the non-original torque modes, the grip
    /// force is turned into a torque in the hub's own frame so that the part about the axle
    /// can be left out (or handed to the drivetrain). PDB names: `pos`, `force`,
    /// `worldMatrix`, `inv`, `world_torques`, `world_drive_torque`.
    fn add_tyre_force_to_hub(
        &mut self,
        pos: &Vec3f,
        force: &Vec3f,
        mode: TorqueModeEx,
        hub: &mut dyn Suspension,
    ) {
        let mut world_matrix = self.world_rotation;
        world_matrix.m[3][0] = self.world_position.x;
        world_matrix.m[3][1] = self.world_position.y;
        world_matrix.m[3][2] = self.world_position.z;
        let inv = xm_matrix_inverse(&world_matrix).m;

        let local_force = [
            force.y * inv[1][0] + force.x * inv[0][0] + force.z * inv[2][0],
            force.y * inv[1][1] + force.x * inv[0][1] + force.z * inv[2][1],
            force.y * inv[1][2] + force.x * inv[0][2] + force.z * inv[2][2],
        ];
        let local_pos = [
            pos.y * inv[1][0] + pos.x * inv[0][0] + pos.z * inv[2][0] + inv[3][0],
            pos.y * inv[1][1] + pos.x * inv[0][1] + pos.z * inv[2][1] + inv[3][1],
            pos.y * inv[1][2] + pos.x * inv[0][2] + pos.z * inv[2][2] + inv[3][2],
        ];
        // local_pos x local_force
        let tx = local_force[2] * local_pos[1] - local_force[1] * local_pos[2];
        let ty = local_pos[2] * local_force[0] - local_force[2] * local_pos[0];
        let tz = local_force[1] * local_pos[0] - local_pos[1] * local_force[0];

        let m = &self.world_rotation.m;
        // back to world: `axle` is the part about the wheel's own axis (row 0)
        let to_world = |axle: f32, up: f32, forward: f32| {
            Vec3f::new(
                m[1][0] * up + m[0][0] * axle + m[2][0] * forward,
                m[1][1] * up + m[0][1] * axle + m[2][1] * forward,
                m[1][2] * up + m[0][2] * axle + m[2][2] * forward,
            )
        };
        let mut world_torques = Vec3f::default();
        let mut world_drive_torque = Vec3f::default();
        match mode {
            TorqueModeEx::ReactionTorques => world_torques = to_world(0.0, ty, tz),
            TorqueModeEx::DriveTorques if !self.driven => world_torques = to_world(tx, ty, tz),
            TorqueModeEx::DriveTorques => {
                world_drive_torque = Vec3f::new(
                    m[0][0] * tx + m[1][0] * 0.0 + m[2][0] * 0.0,
                    m[0][1] * tx + m[1][1] * 0.0 + m[2][1] * 0.0,
                    m[0][2] * tx + m[1][2] * 0.0 + m[2][2] * 0.0,
                );
                world_torques = to_world(0.0, ty, tz);
            }
            _ => {}
        }
        hub.add_local_force_and_torque(force, &world_torques, &world_drive_torque);
        self.local_mx = -tx;
    }

    /// `Tyre::getCorrectedD` @ 0x140280190: grip multiplier from temperature, pressure and
    /// wear. `store_wear_mult` stands for the original's optional `wear_mult` out pointer
    /// (which is `&status.wearMult`).
    pub fn get_corrected_d(&mut self, d: f32, store_wear_mult: bool) -> f32 {
        let mut corrected = self
            .thermal_model
            .get_corrected_d(d, self.status.camber_rad)
            / ((self.status.pressure_dynamic - self.model_data.ideal_pressure).abs()
                * self.model_data.pressure_gain_d
                + 1.0);
        if self.model_data.wear_curve.get_count() != 0 {
            let wear_mult = self
                .model_data
                .wear_curve
                .get_value(self.status.virtual_km as f32);
            corrected *= wear_mult;
            if store_wear_mult {
                self.status.wear_mult = wear_mult;
            }
        }
        corrected
    }

    /// `Tyre::stepDirtyLevel` @ 0x1402843d0: picks up dirt off-track, cleans on tarmac, and
    /// scales this step's forces by the result. PDB names: `dt`, `hubSpeed`.
    fn step_dirty_level(&mut self, dt: f32, hub_speed: f32, surface_def: &SurfaceDef) {
        let dirty_level = self.status.dirty_level;
        if !(dirty_level >= 5.0) {
            self.status.dirty_level =
                hub_speed * surface_def.dirt_additive_k * 0.03 * dt + dirty_level;
        }
        if !ordered_nonzero(surface_def.dirt_additive_k) {
            let dirty_level = self.status.dirty_level;
            if dirty_level > 0.0 {
                self.status.dirty_level = dirty_level - hub_speed * 0.015 * dt;
            }
            if 0.0 > self.status.dirty_level {
                self.status.dirty_level = 0.0;
            }
        }
        let scaled = self.status.dirty_level * 0.05;
        let clamped = if scaled > 1.0 {
            1.0
        } else if scaled >= 0.0 {
            scaled
        } else {
            0.0
        };
        // at most 20 % of the grip is lost
        let grip = if 1.0 - clamped > 0.8 {
            1.0 - clamped
        } else {
            0.8
        };
        // `ucomiss 1.0, aiMult` + `jne`: an AI multiplier of exactly 1 (or a NaN)
        if self.ai_mult == 1.0 || self.ai_mult.is_nan() {
            self.status.mz *= grip;
            self.status.fx *= grip;
            self.status.fy *= grip;
        }
    }

    /// `Tyre::stepPuncture` @ 0x1402849b0: a stripe hotter than the explosion temperature
    /// deflates the tyre (only with mechanical damage on).
    fn step_puncture(&mut self, _dt: f32, _hub_speed: f32, car: Option<&dyn TyreCar>) {
        let imo = self.thermal_model.get_imo();
        let Some(car) = car else {
            return;
        };
        if 0.0 >= car.mechanical_damage_rate() {
            return;
        }
        let limit = self.explosion_temperature;
        if imo[0] > limit || imo[1] > limit || imo[2] > limit {
            self.status.inflation = 0.0;
        }
    }

    /// `Tyre::updateLockedState` @ 0x140285070: a locked wheel stays locked while the brake
    /// can hold the tyre's torque and the wheel is not driven.
    fn update_locked_state(&mut self, _dt: f32) {
        if !self.status.is_locked {
            return;
        }
        let hand_brake_torque = self.inputs.hand_brake_torque;
        let mut brake_torque = self.abs_override * self.inputs.brake_torque;
        let tyre_torque = (self.status.loaded_radius * self.status.fx).abs();
        if !(brake_torque > hand_brake_torque) {
            brake_torque = hand_brake_torque;
        }
        self.status.is_locked = brake_torque.abs() >= tyre_torque
            && !(self.status.angular_velocity.abs() >= 1.0)
            && !self.driven;
    }

    /// `Tyre::updateAngularSpeed` @ 0x140284fa0: integrates the wheel speed (non-driven
    /// wheels) and locks the wheel when its speed changes sign.
    fn update_angular_speed(&mut self, dt: f32) {
        self.update_locked_state(dt);
        let angular_velocity = self.status.feedback_torque / self.data.angular_inertia * dt
            + self.status.angular_velocity;
        self.status.angular_velocity = angular_velocity;
        if sign(self.old_angular_velocity) != sign(angular_velocity) {
            self.status.is_locked = true;
        }
        self.old_angular_velocity = angular_velocity;
        if self.status.is_locked {
            self.status.angular_velocity = 0.0;
        }
        if !(self.status.angular_velocity.abs() >= 1.0) {
            self.status.angular_velocity *= 0.9;
        }
    }

    /// `Tyre::stepRotationMatrix` @ 0x140284b80: spins `localWheelRotation` about its x axis.
    pub fn step_rotation_matrix(&mut self, dt: f32, car: Option<&dyn TyreCar>) {
        let Some(car) = car else {
            return;
        };
        if self.status.angular_velocity.abs() > 0.1 && !car.is_sleeping() {
            let rotation = Mat44f::create_from_axis_angle(
                &Vec3f::new(1.0, 0.0, 0.0),
                dt * self.status.angular_velocity,
            );
            self.local_wheel_rotation = xm_matrix_multiply(&rotation, &self.local_wheel_rotation);
            self.local_wheel_rotation.m[3][0] = 0.0;
            self.local_wheel_rotation.m[3][1] = 0.0;
            self.local_wheel_rotation.m[3][2] = 0.0;
        }
    }

    /// `Tyre::stepThermalModel` @ 0x140284ca0: heat from sliding and rolling into the
    /// thermal model, then its own step and the tyre blankets.
    fn step_thermal_model(&mut self, dt: f32, car: Option<&dyn TyreCar>) {
        let dynamic_grip_level = match car {
            Some(car) => car.dynamic_grip_level(),
            None => 1.0,
        };
        let slide_speed = sqrtf(
            self.sliding_velocity_x * self.sliding_velocity_x
                + self.sliding_velocity_y * self.sliding_velocity_y,
        );
        self.status.thermal_input = slide_speed
            * (self.status.d * self.status.load * self.data.thermal_friction_k)
            * dynamic_grip_level;
        if let Some(surface) = self.surface_def {
            self.status.thermal_input *= surface.grip_mod;
        }

        if !fdtest_inf_or_nan(self.status.thermal_input) {
            let pressure_dynamic = self.status.pressure_dynamic;
            let ideal_pressure = self.model_data.ideal_pressure;
            let pressure_mult =
                (ideal_pressure / pressure_dynamic - 1.0) * self.model_data.pressure_rr_gain + 1.0;
            let mut rolling_k = self.data.thermal_rolling_k;
            if pressure_dynamic >= 0.0 {
                rolling_k *= pressure_mult;
            }
            let version = self.model_data.version;
            if version < 5 {
                self.status.thermal_input =
                    rolling_k * self.status.angular_velocity * self.status.load * 0.001
                        + self.status.thermal_input;
            }
            if version >= 6 {
                self.status.thermal_input = pressure_mult
                    * self.data.thermal_rolling_surface_k
                    * self.status.angular_velocity
                    * self.status.load
                    * 0.001
                    + self.status.thermal_input;
            }
            self.thermal_model.add_thermal_input(
                self.status.camber_rad,
                pressure_dynamic / ideal_pressure - 1.0,
                self.status.thermal_input,
                car,
            );
            if version >= 5 {
                self.thermal_model.add_thermal_core_input(
                    rolling_k * self.status.angular_velocity * self.status.load * 0.001,
                );
            }
            self.thermal_model.step(
                dt,
                self.status.angular_velocity,
                self.status.camber_rad,
                car,
            );
        }
        if let Some(car) = car {
            if car.allow_tyre_blankets() {
                self.step_tyre_blankets(dt, car);
            }
        }
    }

    /// `Tyre::stepTyreBlankets` @ 0x140284f10: while the blankets are on the whole tyre is
    /// held at the blanket temperature; they come off above 10 km/h.
    fn step_tyre_blankets(&mut self, _dt: f32, car: &dyn TyreCar) {
        if !self.tyre_blankets_on {
            return;
        }
        if car.get_speed() * 3.6 > 10.0 {
            self.tyre_blankets_on = false;
        }
        let temperature = if self.blanket_temperature >= self.data.optimum_temp {
            self.data.optimum_temp
        } else {
            self.blanket_temperature
        };
        self.thermal_model.set_temperature(temperature);
    }

    /// `Tyre::stepGrainBlister` @ 0x140284600. PDB names: `dt`, `hubVelocity`.
    fn step_grain_blister(&mut self, dt: f32, hub_velocity: f32, car: Option<&dyn TyreCar>) {
        let rate = match car {
            Some(car) if !(0.0 >= car.tyre_consumption_rate()) => car.tyre_consumption_rate(),
            _ => {
                self.status.grain = 0.0;
                self.status.blister = 0.0;
                return;
            }
        };
        if 0.0 >= self.status.load {
            return;
        }
        let mut nd_slip = self.status.nd_slip;
        if nd_slip >= 2.5 {
            nd_slip = 2.5;
        }
        let core_temp = self.thermal_model.core_temp;
        let slip_gain = powf(nd_slip, self.data.grain_gamma);

        // graining: sliding on a tyre that is too cold
        let grain_gain = self.data.grain_gain;
        let grain_threshold = self.data.grain_threshold;
        if let Some(surface) = self.surface_def {
            if grain_gain > 0.0
                && !(core_temp >= grain_threshold)
                && hub_velocity > 2.0
                && surface.grip_mod >= 0.95
            {
                let amount = (surface.grip_mod * hub_velocity * grain_gain) as f64
                    * ((grain_threshold - core_temp) as f64 * 0.0001);
                if !dtest_inf_or_nan(amount) {
                    self.status.grain =
                        slip_gain as f64 * dt as f64 * amount * rate as f64 + self.status.grain;
                }
            }
            // ... and it wears off again with distance
            let amount = (hub_velocity * surface.grip_mod * self.data.grain_gain) as f64 * 5e-05;
            if !dtest_inf_or_nan(amount) && amount > 0.0 {
                self.status.grain -= slip_gain as f64 * dt as f64 * amount * rate as f64;
            }
        }

        // blistering: sliding on a tyre that is too hot
        let blister_gain = self.data.blister_gain;
        let blister_threshold = self.data.blister_threshold;
        if let Some(surface) = self.surface_def {
            if blister_gain > 0.0
                && core_temp > blister_threshold
                && hub_velocity > 2.0
                && surface.grip_mod as f64 >= 0.95
            {
                let amount = (surface.grip_mod * self.total_hub_velocity * blister_gain) as f64
                    * ((core_temp - blister_threshold) as f64 * 0.0001);
                if !dtest_inf_or_nan(amount) {
                    let slip_gain = powf(nd_slip, self.data.blister_gamma);
                    self.status.blister =
                        slip_gain as f64 * dt as f64 * amount * rate as f64 + self.status.blister;
                }
            }
        }

        let grain = self.status.grain;
        self.status.grain = if grain > 100.0 {
            100.0
        } else if grain >= 0.0 {
            grain
        } else {
            0.0
        };
        let blister = self.status.blister;
        self.status.blister = if blister > 100.0 {
            100.0
        } else if blister >= 0.0 {
            blister
        } else {
            0.0
        };
    }

    /// `Tyre::stepFlatSpot` @ 0x140284500: a (nearly) locked wheel sliding along grinds a
    /// flat spot. PDB names: `dt`, `hubVelocity`.
    fn step_flat_spot(&mut self, dt: f32, hub_velocity: f32, car: Option<&dyn TyreCar>) {
        let locked = !(self.status.angular_velocity.abs() > 0.3) || -0.98 > self.status.slip_ratio;
        let (Some(surface), Some(car)) = (self.surface_def, car) else {
            return;
        };
        if !locked || !(hub_velocity > 3.0) {
            return;
        }
        let damage_rate = car.mechanical_damage_rate();
        if !ordered_nonzero(damage_rate) || !(surface.grip_mod as f64 >= 0.95) {
            return;
        }
        let amount = hub_velocity * self.flat_spot_k * self.status.load * surface.grip_mod;
        let flat_spot = amount as f64
            * 1e-05
            * dt as f64
            * damage_rate as f64
            * self.data.softness_index as f64
            + self.status.flat_spot;
        self.status.flat_spot = if flat_spot > 1.0 { 1.0 } else { flat_spot };
    }
}
