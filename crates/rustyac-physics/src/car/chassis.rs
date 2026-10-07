// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The rolling chassis: AC's `Car` reduced to the parts that are ported (rigid bodies and
//! their masses, fuel, the four suspensions with their tyres, heave springs, anti-roll bars,
//! steering, force feedback, and, when they are installed, brakes, engine and drivetrain
//! with the shift helpers), wired together and stepped in the order of `Car::step` and
//! `Car::stepComponents`. Everything else comes in through a [`ChassisFeed`].

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::aero::{AeroModel, SlipStream, VanillaAero};
use super::aids::{AidsModel, VanillaAids};
use super::antiroll_bar::AntirollBar;
use super::body::{CollisionEvent, FixedJoint, ForceSource, PhysicsCore, RigidBody, Shape};
use super::colliders::{CarBounds, CarColliders, ColliderMesh};
use super::brakes::{BrakeModel, VanillaBrakes};
use super::drivetrain::{DrivetrainModel, OnGearRequestEvent, VanillaDrivetrain};
use super::engine::{EngineModel, VanillaEngine};
use super::feed::{CarControls, ChassisFeed, EngineFeed, VibrationDef};
use super::shift_assists::{AutoBlip, AutoShifter, Autoclutch, GearChanger};
use super::heave_spring::HeaveSpring;
use super::setup::SetupManager;
use super::suspension::{SuspensionModel, VanillaDwb};
use super::telemetry::{PhysicsPage, PhysicsPageWriter};
use crate::track::timing::{FinishContext, InvalidatorAction, InvalidatorInput};
use crate::track::{LapInvalidator, SplineLocator, TimeTransponder, Track};
use crate::track::spline::SplineLocatorData;
use crate::data::ini::IniReader;
use crate::math::{fdtest_inf_or_nan, powf, sinf, sqrtf};
use crate::tyre::rig;
use crate::tyre::{RayCastResult, RayTrackCollisionProvider, Suspension, TorqueModeEx, TyreCar, VanillaTyre};
use crate::vecmath::{Mat44f, Vec3f};

/// What the chassis reads from AC's `PhysicsEngine`, its `Track` and the driver's device:
/// session settings and a few constants of `system/cfg/assetto_corsa.ini`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChassisEnvironment {
    /// `PhysicsEngine::ambientTemperature`, deg C
    pub ambient_temperature: f32,
    /// `PhysicsEngine::roadTemperature`, deg C
    pub road_temperature: f32,
    /// `Track::dynamicGripLevel`
    pub dynamic_grip_level: f32,
    /// `PhysicsEngine::tyreConsumptionRate`
    pub tyre_consumption_rate: f32,
    /// `PhysicsEngine::mechanicalDamageRate`
    pub mechanical_damage_rate: f32,
    /// `PhysicsEngine::fuelConsumptionRate`
    pub fuel_consumption_rate: f32,
    /// `PhysicsEngine::allowTyreBlankets`
    pub allow_tyre_blankets: bool,
    /// `PhysicsEngine::flatSpotFFGain` (0.05 in the constructor)
    pub flat_spot_ff_gain: f32,
    /// `PhysicsEngine::gyroWheelGain`: 0.004 with `[FF_EXPERIMENTAL] ENABLE_GYRO`, else 0
    pub gyro_wheel_gain: f32,
    /// `PhysicsEngine::mzLowSpeedReduction.speedKMH` (`[LOW_SPEED_FF] SPEED_KMH`)
    pub mz_low_speed_reduction_speed_kmh: f32,
    /// `PhysicsEngine::mzLowSpeedReduction.minValue` (`[LOW_SPEED_FF] MIN_VALUE`)
    pub mz_low_speed_reduction_min_value: f32,
    /// `ICarControlsProvider::ffFilter` of the driver's device (0 = no filter)
    pub ff_filter: f32,
    /// `ICarControlsProvider::useFakeUndersteerFF`
    pub use_fake_understeer_ff: bool,
    /// The car is the first one created (`Car::physicsGUID == 0`): only that car switches its
    /// joints' ERP with speed.
    pub is_first_car: bool,
    /// `PhysicsEngine::sessionInfo.startTimeMS`: the physics clock at the session start. The
    /// gearbox is held and the automatic gearbox waits until then.
    pub session_start_time_ms: f64,
    /// `PhysicsEngine::lockGearboxAtStartTimeMS`
    pub lock_gearbox_at_start_time_ms: f64,
    /// `PhysicsEngine::penaltyRules.jumpStartPenaltyMode`: 0 the gearbox is locked on the
    /// grid, 1 a jump start teleports the car to its pit box, 2 it costs a drive-through
    pub jump_start_penalty_mode: i32,
    /// `PhysicsEngine::penaltyRules.basePitPenaltyLaps`
    pub base_pit_penalty_laps: i16,
    /// `PhysicsEngine::penaltyMode`: 0 cut gas, 1 invalidate lap, 2 recover time, 3 nothing,
    /// 4 cut detection
    pub penalty_mode: i32,
    /// `PhysicsEngine::damperMinValue`, `damperGain`: the force-feedback damper of the device
    pub damper_min_value: f32,
    pub damper_gain: f32,
    /// `PhysicsEngine::wind.vector`, m/s, world axes ([`ChassisEnvironment::step_wind`] keeps
    /// it up to date)
    pub wind: Vec3f,
    /// `PhysicsEngine::wind.speed`, m/s: the mean strength the vector swings around
    pub wind_speed: f32,
    /// `PhysicsEngine::wind.directionDeg` (kept, nothing in the physics reads it)
    pub wind_direction_deg: f32,
    /// `DRSManager::isDRSAvailable` of the track for this car: true on a track without DRS zones
    pub drs_zone_available: bool,
    /// `PhysicsEngine::allowedTyresOut`: more tyres than this off the track is a cut or a
    /// penalty (by `penalty_mode`); -1 = no limit (the engine's own default).
    pub allowed_tyres_out: i32,
    /// `PhysicsEngine::sessionInfo.type`: 1 practice, 2 qualifying, 3 race, 4 hot-lap. A
    /// race's lap timer stands still until the start.
    pub session_type: i32,
}

impl Default for ChassisEnvironment {
    /// The `PhysicsEngine` constructor's values with the game's shipped `assetto_corsa.ini`
    /// (26 deg C air, 30 deg C road as in `tools/car_oracle`), a plain device, the player's car.
    fn default() -> ChassisEnvironment {
        ChassisEnvironment {
            ambient_temperature: 26.0,
            road_temperature: 30.0,
            dynamic_grip_level: 1.0,
            tyre_consumption_rate: 1.0,
            mechanical_damage_rate: 1.0,
            fuel_consumption_rate: 1.0,
            allow_tyre_blankets: true,
            flat_spot_ff_gain: 0.05,
            gyro_wheel_gain: 0.0,
            mz_low_speed_reduction_speed_kmh: 3.0,
            mz_low_speed_reduction_min_value: 0.01,
            ff_filter: 0.0,
            use_fake_understeer_ff: false,
            is_first_car: true,
            session_start_time_ms: 0.0,
            lock_gearbox_at_start_time_ms: 0.0,
            jump_start_penalty_mode: 1,
            base_pit_penalty_laps: 3,
            penalty_mode: 3,
            damper_min_value: 0.0,
            damper_gain: 1.0,
            wind: Vec3f { x: 0.0, y: 0.0, z: 0.0 },
            wind_speed: 0.0,
            wind_direction_deg: 0.0,
            drs_zone_available: true,
            allowed_tyres_out: -1,
            session_type: 1,
        }
    }
}

impl ChassisEnvironment {
    /// `PhysicsEngine::setWind` @ 0x1402645a0 for a speed in m/s and a direction in degrees:
    /// the vector (0, 0, speed) turned about the world's up axis by minus the direction, as
    /// the product with the rotation matrix the game builds (its additions of zero included).
    pub fn set_wind(&mut self, speed: f32, direction_deg: f32) {
        let m = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), -(direction_deg * 0.017453)).m;
        let x = (m[1][0] * 0.0 + m[0][0] * 0.0) + m[2][0] * speed;
        let y = (m[1][1] * 0.0 + m[0][1] * 0.0) + m[2][1] * speed;
        let z = (m[1][2] * 0.0 + m[0][2] * 0.0) + m[2][2] * speed;
        self.wind_direction_deg = direction_deg;
        self.wind = Vec3f::new(x, y, z);
        self.wind_speed = speed;
    }

    /// `PhysicsEngine::stepWind` @ 0x140265380: the wind's strength swings by a tenth with a
    /// period of about a minute; its direction stays. Runs once per step before the cars.
    /// `physics_time` is the clock of the step, ms.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step_wind(&mut self, physics_time: f64) {
        if 0.01 > self.wind_speed {
            return;
        }
        let swing = crate::math::sin((physics_time - self.session_start_time_ms) * 0.0001);
        let strength = (swing as f32 * 0.1 + 1.0) * self.wind_speed;
        let (mut x, mut y, mut z) = (self.wind.x, self.wind.y, self.wind.z);
        let length = sqrtf((x * x + y * y) + z * z);
        if length < 0.0 || length > 0.0 {
            let inverse = 1.0 / length;
            x *= inverse;
            y *= inverse;
            z *= inverse;
        }
        let (x, y, z) = (strength * x, strength * y, strength * z);
        if fdtest_inf_or_nan(x) || fdtest_inf_or_nan(y) || fdtest_inf_or_nan(z) {
            return;
        }
        self.wind = Vec3f::new(x, y, z);
    }
}

/// AC's `ThermalObject` (0x20 bytes): `Car::water`, the water temperature. A display value;
/// nothing reads it back into the physics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalObject {
    /// `tmass`
    pub tmass: f32,
    /// `coolSpeedK`
    pub cool_speed_k: f32,
    /// `coolFactor`
    pub cool_factor: f32,
    /// `heatFactor`
    pub heat_factor: f32,
    /// `t`, deg C
    pub t: f32,
    /// `heatAccumulator`
    pub heat_accumulator: f32,
}

impl Default for ThermalObject {
    /// `ThermalObject::ThermalObject` @ 0x1402b2e80 with what `Car::initCarData` writes into
    /// the water's (`tmass` 20, `coolSpeedK` 0.002).
    fn default() -> ThermalObject {
        ThermalObject { tmass: 20.0, cool_speed_k: 0.002, cool_factor: 0.2, heat_factor: 1.0, t: 0.0, heat_accumulator: 0.0 }
    }
}

impl ThermalObject {
    /// `ThermalObject::step` @ 0x1402b2f00. `speed` in m/s.
    pub fn step(&mut self, dt: f32, ambient: f32, speed: f32) {
        let k = self.cool_speed_k * speed;
        let heat = self.heat_accumulator;
        let inverse = 1.0 / self.tmass;
        self.heat_accumulator = 0.0;
        let cooled = ((((1.0 - k) * ambient - self.t) * inverse) * dt) * self.cool_factor + self.t;
        self.t = cooled;
        if heat < 0.0 || heat > 0.0 {
            self.t = (((heat - cooled) * inverse) * dt) * self.heat_factor + cooled;
        }
    }
}

/// A change the game's main thread has queued for the physics thread.
pub type PreStepJob = Box<dyn FnOnce(&mut RollingChassis)>;

/// AC's `PenaltyManager` as far as a jump start writes it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenaltyManager {
    /// `pendingPenaltyType`: 5 = drive-through
    pub pending_penalty_type: i32,
    /// `pitPenaltyLaps`
    pub pit_penalty_laps: i16,
    /// How many `PenaltyRecord`s were pushed.
    pub penalty_records: u32,
}

/// The three-way clamp of the machine code to 0..1: above 1: 1; at least 0: itself; else 0.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// `ksSquareWave` @ 0x14022d2d0.
fn ks_square_wave(t: f32, period: f32) -> f32 {
    if sinf(t / period) > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// AC's `SteeringSystem` (0x40 bytes) without the four-wheel-steer controller (`ctrl_4ws.ini`,
/// 2 of the 113 cars; needs `DynamicController`, a later task).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SteeringSystem {
    /// `linearRatio`: metres of steering-rod shift per degree of `finalSteerAngleSignal`
    /// (`car.ini [CONTROLS] LINEAR_STEER_ROD_RATIO`).
    pub linear_ratio: f32,
}

/// What one tyre was told and did during its step (only kept while tracing).
#[derive(Clone, Debug)]
pub struct TyreTrace {
    /// The inputs of this `Tyre::step` in the layout of the single-wheel rig.
    pub input: rig::StepInput,
    /// `localWheelRotation` on entry.
    pub wheel_rotation_in: Mat44f,
    /// The calls the tyre made on its hub and on the car body.
    pub calls: Vec<rig::Call>,
    /// [`rig::snapshot`] right after the step.
    pub output: Vec<u64>,
    /// What the tyre's ray found.
    pub ray: RayTrace,
}

/// A tyre's ground ray in a step, for the comparison with the game on a track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayTrace {
    pub hit: bool,
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// The mesh that was hit (`SurfaceDef::user_pointer`), -1 for none.
    pub mesh: i32,
}

impl Default for RayTrace {
    fn default() -> RayTrace {
        RayTrace { hit: false, pos: [0.0; 3], normal: [0.0; 3], mesh: -1 }
    }
}

/// One body as `dWorldStep` finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyTrace {
    pub mass: f32,
    pub inertia: [f32; 3],
    pub pos: [f32; 3],
    pub q: [f32; 4],
    pub r: [f32; 9],
    pub lvel: [f32; 3],
    pub avel: [f32; 3],
    pub facc: [f32; 3],
    pub tacc: [f32; 3],
}

/// What a step looked like from the inside, for the comparison with the game's recordings.
#[derive(Clone, Debug, Default)]
pub struct StepTrace {
    pub tyres: Vec<TyreTrace>,
    /// The bodies at the entry of `dWorldStep`, in creation order.
    pub pre: Vec<BodyTrace>,
}

/// The rolling chassis of one car.
pub struct RollingChassis {
    /// The rigid-body world with this car's bodies and joints.
    pub core: PhysicsCore,
    pub env: ChassisEnvironment,
    /// The road under the wheels (`Track` as an `IRayTrackCollisionProvider`).
    pub ground: Box<dyn RayTrackCollisionProvider>,
    /// `Car::body`
    pub body: RigidBody,
    /// `Car::fuelTankBody`
    pub fuel_tank_body: RigidBody,
    /// `Car::fuelTankJoint`
    pub fuel_tank_joint: FixedJoint,
    /// `Car::suspensions`: LF, RF, LR, RR. The suspension slot.
    pub suspensions: Vec<Box<dyn SuspensionModel>>,
    /// `Car::tyres`
    pub tyres: Vec<VanillaTyre>,
    /// `Car::heaveSprings`: front, rear
    pub heave_springs: [HeaveSpring; 2],
    /// `Car::antirollBars`: front, rear
    pub antiroll_bars: [AntirollBar; 2],
    /// `Car::steeringSystem`
    pub steering_system: SteeringSystem,
    /// `Car::setupManager`
    pub setup_manager: SetupManager,
    /// `Car::brakeSystem`: the brake slot. `None`: the feed's `brakes` hook writes the brake
    /// torques instead.
    pub brake_system: Option<Box<dyn BrakeModel>>,
    /// `Car::drivetrain` (clutch, gearbox, differential, with the engine slot inside): the
    /// drivetrain slot. `None`: the feed supplies the driven wheels' speed, the gear and what
    /// the fuel burn reads of the engine.
    pub drivetrain: Option<Box<dyn DrivetrainModel>>,
    /// `Car::aeroMap` with `Car::drs`: the aero slot. `None`: the feed's `aero` hook makes the
    /// wings' force calls instead.
    pub aero: Option<Box<dyn AeroModel>>,
    /// `Car::tractionControl`, `Car::abs`, `Car::edl`, `Car::stabilityControl`,
    /// `Car::speedLimiter`: the aids slot. `None`: the feed's `edl`, `aids` and `stability`
    /// hooks stand in for them.
    pub aids: Option<Box<dyn AidsModel>>,
    /// `AeroMap::airDensity` as `Car::updateAirPressure` left it (also kept here for a chassis
    /// without an aero model), kg/m^3
    pub air_density: f32,
    /// `Car::damageZoneLevel`: front, rear, left, right, centre
    pub damage_zone_level: [f32; 5],
    /// `Car::slipStreamEffectGain`
    pub slip_stream_effect_gain: f32,
    /// `Car::slipStream`: this car's wake
    pub slip_stream: SlipStream,
    /// The writer of the `acpmf_physics` telemetry page, when the car has one
    /// ([`RollingChassis::install_telemetry`]).
    pub telemetry: Option<PhysicsPageWriter>,
    /// The track the car is on (its timing lines and AI line); `None` on the endless road
    /// of the test rigs. Set with [`RollingChassis::set_track`].
    pub track: Option<Arc<Track>>,
    /// `Car::transponder`: the lap timer
    pub transponder: TimeTransponder,
    /// `Car::splineLocator`, `Car::splineLocatorData`: the place along the AI line
    pub spline_locator: SplineLocator,
    pub spline_locator_data: SplineLocatorData,
    /// `Car::lapInvalidator` (it runs for the player's car only)
    pub lap_invalidator: LapInvalidator,
    /// `Car::carHalfWidth`: half the wider axle's track plus 0.3 m
    pub car_half_width: f32,
    /// `PhysicsEngine::stepCounter`: steps so far, this one included
    pub step_counter: u32,
    /// The page of the step that just ran.
    pub physics_page: Option<PhysicsPage>,
    /// `Car::powerClassIndex`: the engine's peak power over the car's mass, W/kg (the AI and
    /// the session set-up read it, the physics does not)
    pub power_class_index: f32,
    /// `Car::isRetired` (nothing in the step reads it)
    pub is_retired: bool,
    /// `ICarControlsProvider::getFFGlobalGain` of the device as of the last step
    pub ff_global_gain: f32,
    /// `Car::lastCollisionWithCarTime`, ms (the collision callback writes it; no contacts yet)
    pub last_collision_with_car_time: f64,
    /// What the game's main thread has queued for the physics thread
    /// (`PhysicsAvatar::stepCommandQueue`): run at the start of the next step.
    pub pre_step_jobs: Vec<PreStepJob>,
    /// `Car::unixName`: the car's folder name (a car called "spectator" collides with nothing
    /// but walls)
    pub unix_name: String,
    /// `Car::userFFGain`
    pub user_ff_gain: f32,
    /// `Car::lightsOn`
    pub lights_on: bool,
    /// `Car::lastLigthSwitchState`: the headlight button in the step before
    pub last_ligth_switch_state: bool,
    /// `Car::blackFlagged`: the controls are dead and the car is put into its pit box
    pub black_flagged: bool,
    /// `Car::pitPosition`: the pit box (rows: side, up, the direction the car faces, position)
    pub pit_position: Mat44f,
    /// `Car::penaltyTime`, s
    pub penalty_time: f64,
    /// `Car::penaltyTimeAccumulator`, s
    pub penalty_time_accumulator: f64,
    /// `Car::disableMinSpeedPenaltyClear`
    pub disable_min_speed_penalty_clear: bool,
    /// `Car::penaltyPerfTarget`
    pub penalty_perf_target: f64,
    /// `PerformanceMeter::getCurrentSplit().t` of the car: the lap-time difference the
    /// "recover time" penalty reads. The performance meter needs the track's racing line and
    /// is not ported; 0 as on a car that is not on a timed lap.
    pub performance_split: f64,
    /// `Car::penaltyManager`
    pub penalty_manager: PenaltyManager,
    /// `Car::vibrationPhase`, `Car::slipVibrationPhase`: only the device's rumble reads them
    pub vibration_phase: f32,
    pub slip_vibration_phase: f32,
    /// `Car::isCollisionOffForPits`: the body mesh does not collide with other cars
    pub is_collision_off_for_pits: bool,
    /// The collide bits `Car::updateColliderStatus` gives the car body's mesh collider
    /// (`IRigidBody::setMeshCollideMask(0, ...)`).
    pub mesh_collide_mask: u32,
    /// The floor boxes (`CarColliderManager`) and the collider mesh (`Car::initColliderMesh`)
    /// on the car's body.
    pub colliders: CarColliders,
    /// `Car::bounds`
    pub bounds: CarBounds,
    /// False: the body is a ghost, as in every recording made before Task 13 (no collision
    /// pass at all; the tyres' rays still see the track).
    pub collisions_enabled: bool,
    /// `Car::lastCollisionTime`, ms
    pub last_collision_time: f64,
    /// The `ACPhysicsEvent`s the collision callback pushed during the last step (the game's
    /// queue is emptied by its sound code every frame).
    pub physics_events: Vec<PhysicsEvent>,
    /// The `Car::evOnCollisionEvent` calls of the last step.
    pub collision_events: Vec<OnCollisionEvent>,
    /// How often the collision callback ran in the last step (= new contact joints).
    pub contact_callbacks: u32,
    /// What the last `dWorldStep` looked like (islands, rows, the solver's work).
    pub step_stats: rustyac_ode::StepStats,
    /// `Car::gridPosition`, `Car::hasGridPosition`: where the car stood before the start
    pub grid_position: Vec3f,
    pub has_grid_position: bool,
    /// How often `Car::evOnJumpStartEvent` was raised.
    pub jump_start_events: u32,
    /// The other cars' body positions (`PhysicsEngine::cars`), for the pit-lane ghosting rule;
    /// empty for a car alone.
    pub other_car_positions: Vec<Vec3f>,
    /// The other cars' wakes (`PhysicsEngine::slipStreams` without this car's own) as their
    /// last step left them: they thin the air this car drives in. Empty for a car alone.
    pub other_wakes: Vec<SlipStream>,
    /// `Car::autoClutch`, `Car::autoBlip`, `Car::autoShift`, `Car::gearChanger`: they run
    /// only with a drivetrain.
    pub autoclutch: Autoclutch,
    pub auto_blip: AutoBlip,
    pub auto_shifter: AutoShifter,
    pub gear_changer: GearChanger,
    /// `Car::water`: stepped only with a drivetrain (it is heated by the engine).
    pub water: ThermalObject,
    /// `Car::isGearboxLocked`
    pub is_gearbox_locked: bool,
    /// `Car::isControlsLocked`: the device is not asked and the car stands on its brakes
    pub is_controls_locked: bool,
    /// `Car::lockControlsTime`: the same until the physics clock reaches it, ms
    pub lock_controls_time: f64,
    /// `Car::isGentleStopping`: no throttle, a fifth of the brake pedal
    pub is_gentle_stopping: bool,
    /// `Car::controls`
    pub controls: CarControls,
    /// `Car::finalSteerAngleSignal`, degrees at the road wheels
    pub final_steer_angle_signal: f32,
    /// `Car::mass` (`[BASIC] TOTALMASS`), kg
    pub mass: f32,
    /// `Car::ffMult`
    pub ff_mult: f32,
    /// `Car::accG`: body acceleration in body axes, g
    pub acc_g: Vec3f,
    /// `Car::steerLock`, degrees
    pub steer_lock: f32,
    /// `Car::steerRatio`
    pub steer_ratio: f32,
    /// `Car::steerAssist`
    pub steer_assist: f32,
    /// `Car::torqueModeEx`
    pub torque_mode_ex: TorqueModeEx,
    /// `Car::lastVelocity`
    pub last_velocity: Vec3f,
    /// `Car::sleepingFrames`
    pub sleeping_frames: i32,
    /// `Car::framesToSleep`
    pub frames_to_sleep: i32,
    /// `Car::mzCurrent`
    pub mz_current: f32,
    /// `Car::lastFF`: the force-feedback number handed to the driver's device
    pub last_ff: f32,
    /// `Car::lastPureMZFF`
    pub last_pure_mz_ff: f32,
    /// `Car::lastGyroFF`
    pub last_gyro_ff: f32,
    /// `Car::lastSteerPosition`
    pub last_steer_position: f32,
    /// `Car::flatSpotPhase`
    pub flat_spot_phase: f32,
    /// `Car::bodyInertia`: the box of `[BASIC] INERTIA`, m
    pub body_inertia: Vec3f,
    /// `Car::fuel`, litres
    pub fuel: f64,
    /// `Car::fuelConsumptionK`
    pub fuel_consumption_k: f64,
    /// `Car::maxFuel`
    pub max_fuel: f64,
    /// `Car::requestedFuel`
    pub requested_fuel: f32,
    /// `Car::fuelKG`: kg per litre
    pub fuel_kg: f32,
    /// `Car::lastBodyMassUpdateTime`, ms
    pub last_body_mass_update_time: f64,
    /// `Car::fuelTankPos`
    pub fuel_tank_pos: Vec3f,
    /// `Car::ballastKG`
    pub ballast_kg: f32,
    /// `Car::valueCache.speed`: |body velocity| at the start of the step, m/s
    pub speed: f32,
    /// `Engine::fuelPressure` as the fuel burn leaves it (1, or 0 with an empty tank): an
    /// output towards the engine.
    pub fuel_pressure: f32,
    /// `PhysicsEngine::physicsTime` of the current step, ms.
    pub physics_time: f64,
    /// Filled during a step when it is `Some` on entry.
    pub trace: Option<StepTrace>,
    /// The car's data folder.
    pub data_path: PathBuf,
}

/// The C runtime's `rand()` (MSVCR120): `Suspension::Suspension` draws one number each.
struct MsvcRand(u32);

impl MsvcRand {
    fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7fff) as i32
    }
}

/// `ucomiss x, 0` + `je`: true for an ordered value that is not zero.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// `ksSawToothWave` @ 0x14022d2a0.
#[allow(clippy::manual_range_contains)]
pub fn ks_saw_tooth_wave(x: f32, length: f32) -> f32 {
    let half = length * 0.5;
    // cvttss2si: out of range (and NaN) gives i32::MIN
    let quotient = x / length;
    let whole = if quotient.is_nan() || quotient >= 2_147_483_648.0 || quotient < -2_147_483_648.0 {
        i32::MIN
    } else {
        quotient as i32
    };
    ((x - whole as f32 * length) - half) / half
}

/// The tyre's view of its hub: the suspension, with the rigid-body world it needs.
struct HubPort<'a, 'b> {
    core: &'a RefCell<&'b mut PhysicsCore>,
    suspension: &'a mut dyn SuspensionModel,
    trace: Option<&'a RefCell<TyreTrace>>,
    asked: [bool; 3],
}

fn arr(v: &Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

impl Suspension for HubPort<'_, '_> {
    fn get_hub_world_matrix(&mut self) -> Mat44f {
        let matrix = self.suspension.get_hub_world_matrix(&self.core.borrow());
        if let (Some(trace), false) = (self.trace, self.asked[0]) {
            let mut flat = [0.0; 16];
            for (i, value) in flat.iter_mut().enumerate() {
                *value = matrix.m[i / 4][i % 4];
            }
            trace.borrow_mut().input.hub_matrix = flat;
        }
        self.asked[0] = true;
        matrix
    }

    fn get_point_velocity(&mut self, p: &Vec3f) -> Vec3f {
        let velocity = self.suspension.get_point_velocity(&self.core.borrow(), p);
        if let (Some(trace), false) = (self.trace, self.asked[1]) {
            trace.borrow_mut().input.hub_velocity = arr(&velocity);
        }
        self.asked[1] = true;
        velocity
    }

    fn add_force_at_pos(&mut self, force: &Vec3f, pos: &Vec3f, driven: bool, add_to_steer_torque: bool) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call {
                kind: 1,
                a: arr(force),
                b: arr(pos),
                flags: driven as u32 | (add_to_steer_torque as u32) << 1,
                ..rig::Call::default()
            });
        }
        self.suspension.add_force_at_pos(&mut self.core.borrow_mut(), force, pos, driven, add_to_steer_torque);
    }

    fn add_torque(&mut self, torque: &Vec3f) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call { kind: 2, a: arr(torque), ..rig::Call::default() });
        }
        self.suspension.add_torque(&mut self.core.borrow_mut(), torque);
    }

    fn get_hub_angular_velocity(&mut self) -> Vec3f {
        let velocity = self.suspension.get_hub_angular_velocity(&self.core.borrow());
        if let (Some(trace), false) = (self.trace, self.asked[2]) {
            trace.borrow_mut().input.hub_angular_velocity = arr(&velocity);
        }
        self.asked[2] = true;
        velocity
    }

    fn add_local_force_and_torque(&mut self, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call {
                kind: 3,
                a: arr(force),
                b: arr(torque),
                c: arr(drive_torque),
                ..rig::Call::default()
            });
        }
        self.suspension.add_local_force_and_torque(&mut self.core.borrow_mut(), force, torque, drive_torque);
    }
}

/// The tyre's view of the car.
struct CarPort<'a, 'b> {
    core: &'a RefCell<&'b mut PhysicsCore>,
    body: RigidBody,
    env: &'a ChassisEnvironment,
    torque_mode_ex: TorqueModeEx,
    sleeping: bool,
    speed: f32,
    /// The surface-drag call on the car body, for the trace.
    calls: Vec<rig::Call>,
}

impl TyreCar for CarPort<'_, '_> {
    fn torque_mode_ex(&self) -> TorqueModeEx {
        self.torque_mode_ex
    }
    fn is_sleeping(&self) -> bool {
        self.sleeping
    }
    fn get_speed(&self) -> f32 {
        self.speed
    }
    fn dynamic_grip_level(&self) -> f32 {
        self.env.dynamic_grip_level
    }
    fn tyre_consumption_rate(&self) -> f32 {
        self.env.tyre_consumption_rate
    }
    fn mechanical_damage_rate(&self) -> f32 {
        self.env.mechanical_damage_rate
    }
    fn ambient_temperature(&self) -> f32 {
        self.env.ambient_temperature
    }
    fn road_temperature(&self) -> f32 {
        self.env.road_temperature
    }
    fn allow_tyre_blankets(&self) -> bool {
        self.env.allow_tyre_blankets
    }
    fn body_get_velocity(&mut self) -> Vec3f {
        self.core.borrow().get_velocity(self.body)
    }
    fn body_get_mass(&mut self) -> f32 {
        self.core.borrow().get_mass(self.body)
    }
    fn body_add_force_at_local_pos(&mut self, f: &Vec3f, p: &Vec3f) {
        self.calls.push(rig::Call { kind: 4, a: arr(f), b: arr(p), ..rig::Call::default() });
        let mut core = self.core.borrow_mut();
        let previous = core.source;
        core.source = ForceSource::Surface;
        core.add_force_at_local_pos(self.body, f, p);
        core.source = previous;
    }
}

/// The road, with what it answered kept for the trace.
struct GroundPort<'a> {
    ground: &'a dyn RayTrackCollisionProvider,
    trace: Option<&'a RefCell<TyreTrace>>,
}

impl RayTrackCollisionProvider for GroundPort<'_> {
    fn has_ray_caster(&self) -> bool {
        self.ground.has_ray_caster()
    }

    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult> {
        let hit = self.ground.ray_cast(org, dir, length);
        if let Some(trace) = self.trace {
            let mut trace = trace.borrow_mut();
            if let Some(hit) = &hit {
                trace.ray = RayTrace { hit: true, pos: arr(&hit.pos), normal: arr(&hit.normal), mesh: hit.surface_def.user_pointer as i32 };
            }
            let input = &mut trace.input;
            input.has_hit = hit.is_some();
            if let Some(hit) = &hit {
                input.ground_y = hit.pos.y;
                input.ground_normal = arr(&hit.normal);
                let s = hit.surface_def;
                input.grip_mod = s.grip_mod;
                input.dirt_additive_k = s.dirt_additive_k;
                input.sin_height = s.sin_height;
                input.sin_length = s.sin_length;
                input.damping = s.damping;
                input.granularity = s.granularity;
            }
        }
        hit
    }
}

impl RollingChassis {
    /// `Car::Car` @ 0x14026bf00, the ported part, in the constructor's order: car body and
    /// fuel tank, `Car::initCarData`, per wheel the suspension and its tyre, heave springs,
    /// steering, anti-roll bars, the first body-mass refresh, the setup items.
    ///
    /// `data_path` is the car's data folder (unpacked ini files). `rand_seed` is the state of
    /// the C runtime's `rand()` when the car is built (`srand(seed)`): each suspension draws
    /// one number for the side its steering rod bends to when damaged. `physics_time` is
    /// `PhysicsEngine::physicsTime` at that moment, ms.
    pub fn new(
        data_path: &Path,
        env: ChassisEnvironment,
        ground: Box<dyn RayTrackCollisionProvider>,
        rand_seed: u32,
        physics_time: f64,
    ) -> Result<RollingChassis, String> {
        let mut core = PhysicsCore::new();
        let body = core.create_rigid_body();
        let fuel_tank_body = core.create_rigid_body();

        // Car::initCarData @ 0x140272b30
        let ini = IniReader::load(&data_path.join("car.ini"))?;
        let mass = ini.get_float("BASIC", "TOTALMASS")?;
        if ini.has_section("EXPLICIT_INERTIA") {
            return Err(format!(
                "{}: [EXPLICIT_INERTIA] is not ported (no car in cardata has it)",
                data_path.join("car.ini").display()
            ));
        }
        let inertia = ini.get_float3("BASIC", "INERTIA")?;
        let body_inertia = Vec3f::new(inertia[0], inertia[1], inertia[2]);
        // `Car::updateBodyMass` takes its "explicit inertia" path whenever the box is (0,0,0)
        // (three `ucomiss` + `jne`: also for NaNs), with or without that section
        if !ordered_nonzero(body_inertia.x) && !ordered_nonzero(body_inertia.y) && !ordered_nonzero(body_inertia.z) {
            return Err(format!(
                "{}: [BASIC] INERTIA is missing or all zero: the game then treats the car like one with \
                 [EXPLICIT_INERTIA], which is not ported",
                data_path.join("car.ini").display()
            ));
        }
        core.set_mass_box(body, mass, body_inertia.x, body_inertia.y, body_inertia.z);
        let mut fuel_kg = 0.74;
        if ini.has_section("FUEL_EXT") {
            fuel_kg = ini.get_float("FUEL_EXT", "KG_PER_LITER")?;
        }
        let ff_mult = ini.get_float("CONTROLS", "FFMULT")? * 0.001;
        let steer_lock = ini.get_float("CONTROLS", "STEER_LOCK")?;
        let steer_ratio = ini.get_float("CONTROLS", "STEER_RATIO")?;
        let mut steer_linear_ratio = ini.get_float("CONTROLS", "LINEAR_STEER_ROD_RATIO")?;
        if !ordered_nonzero(steer_linear_ratio) {
            steer_linear_ratio = 0.003;
        }
        let mut steer_assist = ini.get_float("CONTROLS", "STEER_ASSIST")?;
        if !ordered_nonzero(steer_assist) {
            steer_assist = 1.0;
        }
        let fuel_consumption_k = ini.get_float("FUEL", "CONSUMPTION")? as f64;
        let mut fuel = ini.get_float("FUEL", "FUEL")? as f64;
        let mut max_fuel = ini.get_float("FUEL", "MAX_FUEL")? as f64;
        if max_fuel == 0.0 {
            max_fuel = 30.0;
        }
        let requested_fuel;
        if fuel == 0.0 {
            fuel = 30.0;
            requested_fuel = 30.0;
        } else {
            requested_fuel = fuel as f32;
        }
        let tank = ini.get_float3("FUELTANK", "POSITION")?;
        let fuel_tank_pos = Vec3f::new(tank[0], tank[1], tank[2]);
        core.set_mass_box(fuel_tank_body, 1.0, 0.5, 0.5, 0.5);
        core.set_position(fuel_tank_body, &fuel_tank_pos);
        let fuel_tank_joint = core.create_fixed_joint(fuel_tank_body, body);

        let mut chassis = RollingChassis {
            core,
            env,
            ground,
            body,
            fuel_tank_body,
            fuel_tank_joint,
            suspensions: Vec::new(),
            tyres: Vec::new(),
            heave_springs: [HeaveSpring::default(); 2],
            antiroll_bars: Default::default(),
            steering_system: SteeringSystem { linear_ratio: steer_linear_ratio },
            setup_manager: SetupManager::default(),
            brake_system: None,
            drivetrain: None,
            aero: None,
            aids: None,
            air_density: 1.221,
            damage_zone_level: [0.0; 5],
            slip_stream_effect_gain: 1.0,
            slip_stream: SlipStream::default(),
            telemetry: None,
            track: None,
            transponder: TimeTransponder::default(),
            spline_locator: SplineLocator::default(),
            spline_locator_data: SplineLocatorData::default(),
            lap_invalidator: LapInvalidator::default(),
            car_half_width: 0.0,
            step_counter: 0,
            physics_page: None,
            power_class_index: 0.0,
            is_retired: false,
            ff_global_gain: 1.0,
            last_collision_with_car_time: 0.0,
            pre_step_jobs: Vec::new(),
            // `content/cars/<name>/data` in the game; the extracted cars are `cardata/<name>`
            unix_name: {
                let folder = if data_path.file_name().is_some_and(|name| name == "data") { data_path.parent() } else { Some(data_path) };
                folder.and_then(|folder| folder.file_name()).map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
            },
            user_ff_gain: 1.0,
            lights_on: false,
            last_ligth_switch_state: false,
            black_flagged: false,
            pit_position: Mat44f { m: [[0.0; 4]; 4] },
            penalty_time: 0.0,
            penalty_time_accumulator: 0.0,
            disable_min_speed_penalty_clear: false,
            penalty_perf_target: 0.0,
            performance_split: 0.0,
            penalty_manager: PenaltyManager::default(),
            vibration_phase: 0.0,
            slip_vibration_phase: 0.0,
            is_collision_off_for_pits: false,
            mesh_collide_mask: 0x1e,
            colliders: CarColliders::default(),
            bounds: CarBounds::default(),
            collisions_enabled: true,
            last_collision_time: 0.0,
            physics_events: Vec::new(),
            collision_events: Vec::new(),
            contact_callbacks: 0,
            step_stats: rustyac_ode::StepStats::default(),
            grid_position: Vec3f::default(),
            has_grid_position: false,
            jump_start_events: 0,
            other_car_positions: Vec::new(),
            other_wakes: Vec::new(),
            autoclutch: Autoclutch::default(),
            auto_blip: AutoBlip::default(),
            auto_shifter: AutoShifter::default(),
            gear_changer: GearChanger::default(),
            water: ThermalObject::default(),
            is_gearbox_locked: false,
            is_controls_locked: false,
            lock_controls_time: 0.0,
            is_gentle_stopping: false,
            controls: CarControls::default(),
            final_steer_angle_signal: 0.0,
            mass,
            ff_mult,
            acc_g: Vec3f::default(),
            steer_lock,
            steer_ratio,
            steer_assist,
            torque_mode_ex: TorqueModeEx::Original,
            last_velocity: Vec3f::default(),
            sleeping_frames: 0,
            frames_to_sleep: 50,
            mz_current: 0.0,
            last_ff: 0.0,
            last_pure_mz_ff: 0.0,
            last_gyro_ff: 0.0,
            last_steer_position: 0.0,
            flat_spot_phase: 0.0,
            body_inertia,
            fuel,
            fuel_consumption_k,
            max_fuel,
            requested_fuel,
            fuel_kg,
            last_body_mass_update_time: -1.0e8,
            fuel_tank_pos,
            ballast_kg: 0.0,
            speed: 0.0,
            fuel_pressure: 0.0,
            physics_time,
            trace: None,
            data_path: data_path.to_path_buf(),
        };

        // the wheels: suspension, then its tyre
        let suspensions_ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        let mut rand = MsvcRand(rand_seed);
        for index in 0..4 {
            let section = if index < 2 { "FRONT" } else { "REAR" };
            let kind = suspensions_ini.get_string(section, "TYPE");
            if kind != "DWB" {
                return Err(format!(
                    "{}: [{section}] TYPE={kind}: only the double-wishbone suspension (DWB) is ported",
                    data_path.join("suspensions.ini").display()
                ));
            }
            let suspension = VanillaDwb::new(&mut chassis.core, body, data_path, index, rand.next())?;
            chassis.suspensions.push(Box::new(suspension));
            let mut tyre = VanillaTyre::new();
            let (result, _) =
                chassis.with_tyre_ports(index as usize, None, |hub, _ground, car| tyre.init(hub, data_path, index, Some(&*car)));
            result?;
            chassis.tyres.push(tyre);
        }

        // Car::initHeaveSprings @ 0x140273f40 (all four wheels are double wishbone here)
        chassis.heave_springs[0].init(data_path, true)?;
        chassis.heave_springs[1].init(data_path, false)?;
        // Car::buildARBS @ 0x14026f750
        chassis.antiroll_bars[0].k = suspensions_ini.get_float("ARB", "FRONT")?;
        chassis.antiroll_bars[1].k = suspensions_ini.get_float("ARB", "REAR")?;
        for (axle, name) in ["ctrl_arb_front.ini", "ctrl_arb_rear.ini"].into_iter().enumerate() {
            let path = data_path.join(name);
            if path.is_file() {
                chassis.antiroll_bars[axle].ctrl = super::DynamicController::load(&path)?;
            }
        }
        if data_path.join("ctrl_4ws.ini").is_file() {
            return Err(format!("{}: rear-wheel steering is not ported", data_path.join("ctrl_4ws.ini").display()));
        }
        chassis.sleeping_frames = 0;
        chassis.update_body_mass();
        // CarColliderManager::init: after the first mass refresh, before the setup items
        chassis.install_box_colliders()?;
        chassis.setup_manager = SetupManager::init(&chassis, data_path)?;
        Ok(chassis)
    }

    /// Gives the chassis its own brakes: AC's `BrakeSystem` built from the car's files
    /// (`BrakeSystem::init` @ 0x14028d690). From now on the feed's `brakes` hook is not called.
    pub fn install_brakes(&mut self) -> Result<(), String> {
        let brakes = VanillaBrakes::new(&self.data_path)?;
        self.set_brake_system(Box::new(brakes))
    }

    /// Puts a brake system into the brake slot (and registers its setup items).
    pub fn set_brake_system(&mut self, brakes: Box<dyn BrakeModel>) -> Result<(), String> {
        self.brake_system = Some(brakes);
        self.setup_manager = SetupManager::init(self, &self.data_path.clone())?;
        Ok(())
    }

    /// Gives the chassis its own engine and drivetrain, AC's `Engine` and `Drivetrain` built
    /// from the car's files (`Drivetrain::init` @ 0x140266dc0), and the shift helpers
    /// (`Autoclutch::init`, `AutoBlip::init`, `AutoShifter::init`, `GearChanger::init`). Call
    /// it before the session start: the drivetrain copies the wheel inertias of the tyres as
    /// they are right after `Car::Car`. From now on the feed's `engine`, `autoclutch`,
    /// `current_gear` and `drivetrain` hooks are not called.
    pub fn install_drivetrain(&mut self) -> Result<(), String> {
        let engine = VanillaEngine::new(&self.data_path)?;
        self.install_drivetrain_with_engine(Box::new(engine))
    }

    /// As [`RollingChassis::install_drivetrain`], with the given engine in the engine slot.
    pub fn install_drivetrain_with_engine(&mut self, engine: Box<dyn EngineModel>) -> Result<(), String> {
        let drivetrain = VanillaDrivetrain::new(self, engine)?;
        self.set_drivetrain(Box::new(drivetrain))
    }

    /// Puts a drivetrain into the drivetrain slot, loads the shift helpers and registers the
    /// setup items of drivetrain and engine.
    pub fn set_drivetrain(&mut self, drivetrain: Box<dyn DrivetrainModel>) -> Result<(), String> {
        self.drivetrain = Some(drivetrain);
        self.autoclutch = Autoclutch::new(&self.data_path)?;
        self.auto_blip = AutoBlip::new(&self.data_path)?;
        self.auto_shifter = AutoShifter::new(&self.data_path)?;
        self.gear_changer = GearChanger::default();
        self.setup_manager = SetupManager::init(self, &self.data_path.clone())?;
        Ok(())
    }

    /// Gives the chassis its own aerodynamics: AC's `AeroMap` and `DRS` built from the car's
    /// files (`Car::initAeroMap` @ 0x140272a80). From now on the feed's `aero` hook is not
    /// called.
    pub fn install_aero(&mut self) -> Result<(), String> {
        let (mut aero, slipstream) = VanillaAero::new(&self.data_path)?;
        for wing in &mut aero.base.wings {
            wing.status.front_share = super::aero::get_point_front_share(self, &wing.data.position);
        }
        if let Some((effect_gain_mult, speed_factor_mult)) = slipstream {
            self.slip_stream.effect_gain_mult = effect_gain_mult;
            self.slip_stream.speed_factor_mult = speed_factor_mult;
        }
        self.set_aero(Box::new(aero))
    }

    /// Puts an aero model into the aero slot (and registers the wings' setup items).
    pub fn set_aero(&mut self, aero: Box<dyn AeroModel>) -> Result<(), String> {
        self.aero = Some(aero);
        self.setup_manager = SetupManager::init(self, &self.data_path.clone())?;
        Ok(())
    }

    /// Gives the chassis its own driver aids, built from `electronics.ini` as `Car::Car` does
    /// (`EDL::init`, `ABS::init`, `TractionControl::init`, `SpeedLimiter::init`,
    /// `StabilityControl::init`). Call it after `install_drivetrain`: the differential lock
    /// asks the drivetrain which wheels are driven. From now on the feed's `edl` (for a car
    /// that also has its drivetrain), `aids` and `stability` hooks are not called.
    pub fn install_aids(&mut self) -> Result<(), String> {
        let traction_type = match &self.drivetrain {
            Some(drivetrain) => drivetrain.base().traction_type,
            None => super::TractionType::Rwd,
        };
        self.aids = Some(Box::new(VanillaAids::new(&self.data_path, traction_type)?));
        Ok(())
    }

    /// Puts the car on a track: what `Car::Car` @ 0x14026bf00 does with `ksPhysics->track`
    /// (`TimeTransponder::init`, `SplineLocator::init`, `carHalfWidth`). The road under the
    /// wheels is the chassis' `ground`, which has to be the same track.
    pub fn set_track(&mut self, track: Arc<Track>) {
        self.transponder = TimeTransponder::new(&track, 0);
        self.spline_locator = SplineLocator::default();
        self.spline_locator_data = SplineLocatorData::default();
        // RaceEngineer::getFrontTrack @ 0x14027c010 / getRearTrack @ 0x14027c9e0
        let front = (self.suspensions[0].get_base_position().x * 2.0).abs();
        let rear = (self.suspensions[2].get_base_position().x * 2.0).abs();
        self.car_half_width = (if front > rear { front } else { rear }) * 0.5 + 0.3;
        self.track = Some(track);
    }

    /// Attaches a telemetry writer: from now on every step leaves the `acpmf_physics` page of
    /// that step in `physics_page` (the game's `SharedMemoryWriter` on its physics thread).
    pub fn install_telemetry(&mut self) -> Result<(), String> {
        self.telemetry = Some(PhysicsPageWriter::new(self, &self.data_path.clone())?);
        Ok(())
    }

    /// The handlers of `Drivetrain::evOnGearRequest`, in the order `Car::Car` registers them:
    /// the automatic clutch (`Autoclutch::onGearRequest` @ 0x1402b9350), then the automatic
    /// throttle blip (lambda @ 0x1402b9880).
    pub fn on_gear_request(&mut self, event: &OnGearRequestEvent) {
        self.autoclutch.on_gear_request(event);
        self.auto_blip.on_gear_request(event, self.controls.clutch, self.physics_time);
    }

    /// `Tyre::stepRotationMatrix` @ 0x140284b80 of one wheel, as the drivetrain calls it for
    /// the driven wheels after it has written their speed.
    pub fn step_wheel_rotation(&mut self, index: usize, dt: f32) {
        let mut tyre = std::mem::take(&mut self.tyres[index]);
        self.with_tyre_ports(index, None, |_hub, _ground, car| tyre.step_rotation_matrix(dt, Some(&*car)));
        self.tyres[index] = tyre;
    }

    /// `Car::stepThermalObjects` @ 0x1402769f0: the water temperature.
    fn step_thermal_objects(&mut self, dt: f32) {
        let Some(drivetrain) = &self.drivetrain else { return };
        let rpm = drivetrain.get_engine_rpm();
        let engine = drivetrain.engine();
        if rpm > engine.minimum() as f32 * 0.8 {
            let heat = ((rpm / engine.get_limiter_rpm() as f32) * 20.0) * self.controls.gas + 85.0;
            self.water.heat_accumulator = heat + self.water.heat_accumulator;
        }
        self.water.step(dt, self.env.ambient_temperature, self.speed);
    }

    /// Runs `f` with the three things a tyre function needs: its hub, the road and the car.
    fn with_tyre_ports<R>(
        &mut self,
        index: usize,
        trace: Option<&RefCell<TyreTrace>>,
        f: impl FnOnce(&mut dyn Suspension, &dyn RayTrackCollisionProvider, &mut dyn TyreCar) -> R,
    ) -> (R, Vec<rig::Call>) {
        let sleeping = self.sleeping_frames > self.frames_to_sleep;
        let cell = RefCell::new(&mut self.core);
        let mut hub = HubPort { core: &cell, suspension: self.suspensions[index].as_mut(), trace, asked: [false; 3] };
        let mut car = CarPort {
            core: &cell,
            body: self.body,
            env: &self.env,
            torque_mode_ex: self.torque_mode_ex,
            sleeping,
            speed: self.speed,
            calls: Vec::new(),
        };
        let ground = GroundPort { ground: self.ground.as_ref(), trace };
        let result = f(&mut hub, &ground, &mut car);
        (result, car.calls)
    }

    /// `Car::isSleeping` @ 0x1402745e0.
    pub fn is_sleeping(&self) -> bool {
        self.sleeping_frames > self.frames_to_sleep
    }

    /// `Car::calcBodyMass` @ 0x14026fb70.
    pub fn calc_body_mass(&self) -> f32 {
        let mut hubs = 0.0f32;
        for suspension in &self.suspensions {
            hubs += suspension.get_mass(&self.core);
        }
        (self.mass - hubs) + self.ballast_kg
    }

    /// `Car::updateBodyMass` @ 0x140276c70: once per second of physics time the body gets
    /// `TOTALMASS` minus the hubs plus ballast, and the tank the mass of the fuel in it.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn update_body_mass(&mut self) {
        if !(self.physics_time - self.last_body_mass_update_time > 1000.0) {
            return;
        }
        let mass = self.calc_body_mass();
        self.core.set_mass_box(self.body, mass, self.body_inertia.x, self.body_inertia.y, self.body_inertia.z);
        let fuel_mass = (self.fuel_kg as f64 * self.fuel) as f32;
        // `comiss` + `cmovbe`: 0.1 unless the fuel weighs more
        let tank = if fuel_mass > 0.1 { fuel_mass } else { 0.1 };
        self.core.set_mass_box(self.fuel_tank_body, tank, 0.5, 0.5, 0.5);
        self.last_body_mass_update_time = self.physics_time;
    }

    /// `Car::getTotalMass` @ 0x140272570.
    pub fn get_total_mass(&self, with_fuel: bool) -> f32 {
        let mut total = self.core.get_mass(self.body);
        for suspension in &self.suspensions {
            total += suspension.get_mass(&self.core);
        }
        if with_fuel {
            total += self.fuel as f32 * self.fuel_kg;
        }
        total
    }

    /// `RaceEngineer::getBaseCarHeight` @ 0x14027ba00: how high the body origin sits above
    /// the road with the wheels at their design position.
    pub fn get_base_car_height(&self) -> f32 {
        let front = -(self.suspensions[0].get_base_position().y - self.tyres[0].data.radius);
        let rear = -(self.suspensions[2].get_base_position().y - self.tyres[2].data.radius);
        if front > rear {
            front
        } else {
            rear
        }
    }

    /// `Car::forceRotation` @ 0x140270040. `heading` is the direction the car's tail points.
    pub fn force_rotation(&mut self, heading: &Vec3f) {
        let h = heading;
        let mut z = h.x - h.y * 0.0;
        let mut x = h.y * 0.0 - h.z;
        let mut y = h.z * 0.0 - h.x * 0.0;
        let length = sqrtf((y * y + x * x) + z * z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            x *= inverse;
            y *= inverse;
            z *= inverse;
        }
        let matrix = Mat44f { m: [[x, y, z, 0.0], [0.0, 1.0, 0.0, 0.0], [-h.x, -h.y, -h.z, 0.0], [0.0, 0.0, 0.0, 1.0]] };
        self.core.set_rotation(self.body, &matrix);
        self.core.set_rotation(self.fuel_tank_body, &matrix);
        for suspension in &mut self.suspensions {
            suspension.attach(&mut self.core);
        }
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Teleport);
        self.core.stop(self.body);
        self.core.stop(self.fuel_tank_body);
        self.core.source = previous;
    }

    /// `Car::forcePosition` @ 0x14026fe10 (with `Car::reset` @ 0x1402758e0): puts the car on
    /// the road at `pos` (a point on the ground), with `Drivetrain::reset`,
    /// `BrakeSystem::reset` and neutral for a car that has those systems.
    pub fn force_position(&mut self, pos: &Vec3f) {
        self.force_position_with(pos, true);
    }

    /// `Car::forcePosition(pos, invalidateLap)`: with `invalidate_lap` off the lap in
    /// progress stays as it is (no caller in the game leaves it off; the oracle does).
    pub fn force_position_with(&mut self, pos: &Vec3f, invalidate_lap: bool) {
        let mut pos = *pos;
        pos.y += self.get_base_car_height() + 0.01;
        // Car::reset
        self.has_grid_position = false;
        self.water.t = 60.0;
        self.frames_to_sleep = 50;
        self.penalty_time = 0.0;
        self.penalty_time_accumulator = 0.0;
        self.fuel = self.requested_fuel as f64;
        self.is_collision_off_for_pits = false;
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Teleport);
        self.core.stop(self.body);
        self.core.set_position(self.body, &pos);
        let tank = self.core.local_to_world(self.body, &self.fuel_tank_pos);
        self.core.set_position(self.fuel_tank_body, &tank);
        for suspension in &mut self.suspensions {
            suspension.stop(&mut self.core);
            suspension.attach(&mut self.core);
        }
        if let Some(drivetrain) = &mut self.drivetrain {
            drivetrain.reset();
        }
        if let Some(brakes) = &mut self.brake_system {
            brakes.reset(self.env.ambient_temperature);
        }
        for index in 0..self.tyres.len() {
            let mut tyre = std::mem::take(&mut self.tyres[index]);
            self.with_tyre_ports(index, None, |hub, _ground, car| tyre.reset(hub, Some(&*car)));
            self.tyres[index] = tyre;
        }
        if let Some(mut drivetrain) = self.drivetrain.take() {
            drivetrain.set_current_gear(1, true, self);
            self.drivetrain = Some(drivetrain);
        }
        // the lap in progress does not count (the game's callers all pass `invalidateLap`)
        if invalidate_lap {
            self.transponder.invalidate();
        }
        self.core.stop(self.body);
        self.core.stop(self.fuel_tank_body);
        self.core.source = previous;
        self.frames_to_sleep = 50;
        self.spline_locator.reset();
    }

    /// `Tyre::setCompound` @ 0x1402834e0 on the four tyres: what the game's setup screen does
    /// with `tyres.ini [COMPOUND_DEFAULT] INDEX` when a session starts.
    pub fn set_compound(&mut self, compound: i32) -> Result<(), String> {
        for index in 0..self.tyres.len() {
            let mut tyre = std::mem::take(&mut self.tyres[index]);
            let (found, _) = self.with_tyre_ports(index, None, |hub, _ground, car| tyre.set_compound(compound, hub, Some(&*car)));
            self.tyres[index] = tyre;
            if !found {
                return Err(format!("compound index {compound} does not exist"));
            }
        }
        Ok(())
    }

    /// The session start of the player's car as the game's setup screen does it with the
    /// default setup: the default tyre compound, and every setup value pushed through the
    /// screen's whole-number spinners (applied by [`SetupManager::step`] in the first step).
    pub fn session_start(&mut self) -> Result<(), String> {
        let tyres = IniReader::load(&self.data_path.join("tyres.ini"))?;
        let compound = tyres.get_int("COMPOUND_DEFAULT", "INDEX")?;
        // an index that does not exist changes nothing in the game (`setCompound` says no and
        // nobody looks)
        let _ = self.set_compound(compound);
        let setup = IniReader::load(&self.data_path.join("setup.ini"))?;
        if setup.ready {
            let mut manager = std::mem::take(&mut self.setup_manager);
            manager.apply_setup_screen_defaults(self, &setup);
            self.setup_manager = manager;
        }
        Ok(())
    }

    /// One physics step: `Car::stepPreCacheValues` @ 0x1402768c0, `Car::step` @ 0x140275da0
    /// (with `Car::stepComponents` @ 0x1402764d0) and the rigid-body step
    /// (`PhysicsCore::step` @ 0x1402cd690), as `PhysicsEngine::step` @ 0x140264760 runs them
    /// for one car.
    ///
    /// `physics_time` is the engine's clock for this step, ms (it advances by `dt * 1000`).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, dt: f32, physics_time: f64, feed: &mut dyn ChassisFeed) {
        self.physics_time = physics_time;
        // PhysicsEngine::step counts its steps first of all
        self.step_counter = self.step_counter.wrapping_add(1);
        // PhysicsEngine::evOnPreStep: the queued jobs see the new step's clock
        for job in std::mem::take(&mut self.pre_step_jobs) {
            job(self);
        }
        if let Some(tape) = &mut self.core.tape {
            tape.clear();
        }
        if let Some(trace) = &mut self.trace {
            trace.tyres.clear();
            trace.pre.clear();
        }

        // Car::stepPreCacheValues
        let v = self.core.get_velocity(self.body);
        let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
        self.speed = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };
        if let Some(drivetrain) = &self.drivetrain {
            let mut mass = self.suspensions[0].get_mass(&self.core);
            mass += self.core.get_mass(self.body);
            mass = self.suspensions[1].get_mass(&self.core) + mass;
            mass = self.suspensions[2].get_mass(&self.core) + mass;
            mass = self.suspensions[3].get_mass(&self.core) + mass;
            self.power_class_index = drivetrain.engine().get_max_power_w() / mass;
        }

        // --- Car::step ---------------------------------------------------------------------
        // the first car softens its joints' error correction below 1 m/s
        if self.env.is_first_car {
            let v = self.core.get_velocity(self.body);
            let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
            let erp;
            if squared >= 1.0 {
                erp = 0.3;
                for suspension in &mut self.suspensions {
                    let cfm = suspension.base().base_cfm;
                    suspension.set_erp_cfm(&mut self.core, 0.3, cfm);
                }
            } else {
                erp = 0.9;
                for suspension in &mut self.suspensions {
                    suspension.set_erp_cfm(&mut self.core, 0.9, 1.0e-7);
                }
            }
            self.core.joint_set_erp_cfm(self.fuel_tank_joint.id, erp, -1.0);
        }

        // the lock is sampled before the controls are polled
        let locked = self.is_controls_locked || self.lock_controls_time > physics_time;
        self.poll_controls(dt, feed);

        // the headlight switch: a press toggles
        let action = feed.get_action(4);
        if action && !self.last_ligth_switch_state {
            self.lights_on = !self.lights_on;
        }
        self.last_ligth_switch_state = action;

        // a black-flagged car is put into its pit box, every step until it is there
        if self.black_flagged && !self.is_in_pits() {
            let m = self.pit_position.m;
            self.force_rotation(&Vec3f::new(-m[2][0], -m[2][1], -m[2][2]));
            self.force_position(&Vec3f::new(m[3][0], m[3][1], m[3][2]));
        }

        // Car::updateAirPressure: the other cars' wakes thin the air (a car alone has none)
        let wakes = std::mem::take(&mut self.other_wakes);
        self.update_air_pressure(&wakes);
        self.other_wakes = wakes;

        // fuel burn, from what the engine did in the step before
        let engine = match &self.drivetrain {
            Some(drivetrain) => {
                let base = drivetrain.engine().base();
                EngineFeed { rpm: drivetrain.get_engine_rpm(), gas_usage: base.gas_usage, turbo_boost: base.status.turbo_boost }
            }
            None => feed.engine(self),
        };
        let boost = if engine.turbo_boost >= 0.0 { engine.turbo_boost as f64 } else { 0.0 };
        let burnt = engine.rpm.abs() * dt * engine.gas_usage;
        self.fuel -= burnt as f64 * (boost + 1.0) * self.fuel_consumption_k * 0.001 * self.env.fuel_consumption_rate as f64;
        if self.fuel > 0.0 {
            self.fuel_pressure = 1.0;
        } else {
            self.fuel = 0.0;
            self.fuel_pressure = 0.0;
        }
        if let Some(drivetrain) = &mut self.drivetrain {
            drivetrain.engine_mut().base_mut().fuel_pressure = self.fuel_pressure;
        }
        self.update_body_mass();

        // the overrides of the driver's controls
        if locked {
            self.controls.gas = 0.0;
            self.controls.brake = 1.0;
            self.controls.steer = 0.0;
            self.controls.clutch = 0.0;
        }
        if self.is_gentle_stopping {
            self.controls.gas = 0.0;
            self.controls.brake = 0.2;
        }
        self.step_penalty(dt);

        // the steering wheel as a road-wheel angle
        let mut signal = self.steer_lock * self.controls.steer / self.steer_ratio;
        if fdtest_inf_or_nan(signal) {
            signal = 0.0;
        }
        self.final_steer_angle_signal = signal;

        let mut all_loaded = true;
        for tyre in &self.tyres {
            if 0.0 >= tyre.status.load {
                all_loaded = false;
                break;
            }
        }
        match &self.drivetrain {
            Some(drivetrain) => {
                let (engine_velocity, gear) = (drivetrain.base().engine.velocity, drivetrain.base().current_gear);
                self.autoclutch.step(&mut self.controls, self.speed, engine_velocity, gear, dt);
            }
            None => feed.autoclutch(self),
        }

        // the sleeping rule
        let mut asleep = false;
        if self.speed >= 0.5 {
            self.sleeping_frames = 0;
        } else {
            let w = self.core.get_angular_velocity(self.body);
            if (w.x * w.x + w.y * w.y) + w.z * w.z >= 1.0 {
                self.sleeping_frames = 0;
            } else {
                let driving = !(0.01 >= self.controls.gas) && !(0.01 >= self.controls.clutch) && {
                    let gear = match &self.drivetrain {
                        Some(drivetrain) => drivetrain.base().current_gear,
                        None => feed.current_gear(self),
                    };
                    gear != 1
                };
                if !driving && all_loaded {
                    self.sleeping_frames = self.sleeping_frames.wrapping_add(1);
                } else {
                    self.sleeping_frames = 0;
                }
                asleep = self.sleeping_frames > self.frames_to_sleep;
            }
        }
        if asleep {
            let previous = std::mem::replace(&mut self.core.source, ForceSource::Sleep);
            self.core.stop(self.body);
            self.core.stop(self.fuel_tank_body);
            self.core.source = previous;
        }

        // body acceleration in g, body axes
        let v = self.core.get_velocity(self.body);
        let inverse_dt = 1.0 / dt;
        let acc = Vec3f::new(
            ((v.x - self.last_velocity.x) * inverse_dt) * 0.101_978_384,
            ((v.y - self.last_velocity.y) * inverse_dt) * 0.101_978_384,
            ((v.z - self.last_velocity.z) * inverse_dt) * 0.101_978_384,
        );
        self.last_velocity = v;
        self.acc_g = self.core.world_to_local_normal(self.body, &acc);

        // Car::stepThermalObjects
        self.step_thermal_objects(dt);

        // --- Car::stepComponents -----------------------------------------------------------
        // 1: brakes
        match self.brake_system.take() {
            Some(mut brakes) => {
                brakes.step(self, dt);
                self.brake_system = Some(brakes);
            }
            None => feed.brakes(self),
        }
        // 2: electronic differential lock (the hook also carries what else an unported
        // system leaves in a tyre before its step)
        if self.aids.is_none() || self.drivetrain.is_none() {
            feed.edl(self);
        }
        if let Some(mut aids) = self.aids.take() {
            aids.step_edl(self, dt);
            self.aids = Some(aids);
        }
        // 3: suspensions
        for suspension in &mut self.suspensions {
            suspension.step(&mut self.core, dt);
        }
        // 4: tyres; force feedback right after the fourth
        for index in 0..self.tyres.len() {
            self.step_tyre(index, dt);
        }
        self.on_tyres_step_completed();
        if !self.is_controls_locked && !self.black_flagged {
            // the damper of the device fades out by 10 km/h
            let fade = clamp01(1.0 - (self.speed * 3.6) * 0.1);
            let damper = ((1.0 - self.env.damper_min_value) * fade + self.env.damper_min_value) * self.env.damper_gain;
            feed.send_ff(self.last_ff, damper, self.user_ff_gain);
        }
        // 5: heave springs
        for (axle, first) in [(0usize, 0usize), (1, 2)] {
            if ordered_nonzero(self.heave_springs[axle].k) {
                let (left, right) = self.suspensions[first..first + 2].split_at_mut(1);
                self.heave_springs[axle].step(&mut self.core, self.body, left[0].as_mut(), right[0].as_mut(), dt);
            }
        }
        // 6 to 9: DRS, aero, KERS, ERS
        match self.aero.take() {
            Some(mut aero) => {
                aero.step(self, dt);
                self.aero = Some(aero);
            }
            None => feed.aero(self),
        }
        // 10: steering: the rods move now, the solver turns the wheels at the end of this
        // step, the tyres see it in the next
        let offset = -(self.final_steer_angle_signal * self.steering_system.linear_ratio);
        self.suspensions[0].set_steer_length_offset(&mut self.core, offset);
        let offset = -(self.final_steer_angle_signal * self.steering_system.linear_ratio);
        self.suspensions[1].set_steer_length_offset(&mut self.core, offset);
        // 11 to 14: auto-blip, auto-shifter, gear changer, drivetrain
        match self.drivetrain.take() {
            Some(mut drivetrain) => {
                self.auto_blip.step(&mut self.controls, self.is_controls_locked, self.speed, self.physics_time);
                let mut shifter = self.auto_shifter;
                shifter.step(self, drivetrain.as_ref(), dt);
                self.auto_shifter = shifter;
                let mut changer = self.gear_changer;
                changer.step(self, drivetrain.as_mut());
                self.gear_changer = changer;
                drivetrain.step(self, dt);
                self.drivetrain = Some(drivetrain);
            }
            None => feed.drivetrain(self),
        }
        // 15: anti-roll bars; a bar with a controller takes its rate from it
        for (axle, first) in [(0usize, 0usize), (1, 2)] {
            if self.antiroll_bars[axle].ctrl.ready {
                let mut ctrl = std::mem::take(&mut self.antiroll_bars[axle].ctrl);
                self.antiroll_bars[axle].k = ctrl.eval(&super::CarSignals::of(self));
                self.antiroll_bars[axle].ctrl = ctrl;
            }
            let (left, right) = self.suspensions[first..first + 2].split_at_mut(1);
            self.antiroll_bars[axle].step(&mut self.core, self.body, left[0].as_mut(), right[0].as_mut(), dt);
        }
        // 16 to 18: ABS, traction control, speed limiter
        match self.aids.take() {
            Some(mut aids) => {
                aids.step(self, dt);
                self.aids = Some(aids);
            }
            None => feed.aids(self),
        }
        // 20: setup values that changed reach the car here
        let mut manager = std::mem::take(&mut self.setup_manager);
        manager.step(self);
        self.setup_manager = manager;
        // 21 to 29: telemetry, lap timing, stability control
        // 24: LapInvalidator::step, the player's car only
        if self.env.is_first_car && self.track.is_some() {
            let surfaces = [0, 1, 2, 3].map(|k| self.tyres.get(k).and_then(|t| t.surface_def.as_ref()));
            let input = InvalidatorInput {
                physics_time,
                last_collision_with_car_time: self.last_collision_with_car_time,
                surfaces,
                penalty_mode: self.env.penalty_mode,
                allowed_tyres_out: self.env.allowed_tyres_out,
                penalty_time: self.get_penalty_time(),
                speed: self.speed,
                has_controls_provider: true,
            };
            let mut invalidator = self.lap_invalidator;
            let action = invalidator.step(&input);
            self.lap_invalidator = invalidator;
            match action {
                Some(InvalidatorAction::AddPenalty(seconds)) => self.add_penalty(seconds),
                Some(InvalidatorAction::AddCut) => self.transponder.add_cut(),
                Some(InvalidatorAction::ClearPenalty) => self.clear_penalty(),
                None => {}
            }
        }
        // 26: SplineLocator::step, from where the body is before this step moves it
        if let Some(track) = self.track.clone() {
            // a track without an AI line has an empty one in the game, and the locator runs on it
            let empty = crate::track::AiSpline::default();
            let spline = track.ai_spline.as_ref().unwrap_or(&empty);
            let position = self.core.get_position(self.body);
            self.spline_locator.step(spline, &track.starting_bounds, &position, self.car_half_width);
        }
        // 27: StabilityControl::step
        match self.aids.take() {
            Some(mut aids) => {
                aids.step_stability(self, dt);
                self.aids = Some(aids);
            }
            None => feed.stability(self),
        }
        // 28: TimeTransponder::step, with the hub of wheel 0 as this step's tyre step saw it
        if let Some(track) = self.track.clone() {
            let probe = self.tyres[0].world_position;
            let finish = FinishContext {
                penalty_time: self.get_penalty_time(),
                check_black_flag: self.penalty_manager.pending_penalty_type == 5 && self.penalty_manager.pit_penalty_laps == 1,
            };
            let not_started = self.env.session_type == 3 && (self.env.session_start_time_ms - physics_time) > 0.0;
            let actions = self.transponder.step(&track, &probe, self.step_counter, physics_time, not_started, &finish);
            if actions.black_flag.is_some() {
                self.set_black_flag(true);
            }
            if actions.decrease_pit_penalty_laps == Some(true) && self.penalty_manager.pit_penalty_laps > 0 {
                self.penalty_manager.pit_penalty_laps -= 1;
            }
        }

        // --- the end of Car::step ------------------------------------------------------------
        self.update_collider_status();
        if self.env.is_first_car {
            self.step_jump_start();
        }

        // --- PhysicsCore::step ---------------------------------------------------------------
        let pre: Vec<BodyTrace> = match self.trace {
            Some(_) => self.core.bodies().map(|body| self.body_trace(body)).collect(),
            None => Vec::new(),
        };
        if let Some(trace) = &mut self.trace {
            trace.pre = pre;
        }
        self.step_core(dt);
        self.post_step(dt);

        // what the game's physics thread does after each step: the state snapshot, then the
        // shared-memory page
        self.ff_global_gain = feed.get_ff_global_gain();
        if let Some(mut writer) = self.telemetry.take() {
            writer.step_tyres_out(self);
            writer.snapshot(self);
            self.physics_page = writer.update_physics(self);
            self.telemetry = Some(writer);
        }
    }

    /// `Car::pollControls` @ 0x140274e70: the driver's device fills `Car::controls` and gets
    /// its rumble and its rev marks. A car with locked controls or a black flag does not ask
    /// its device (a lock by time alone still does).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn poll_controls(&mut self, dt: f32, feed: &mut dyn ChassisFeed) {
        let limiter = self.drivetrain.as_ref().map(|drivetrain| drivetrain.engine().get_limiter_rpm());
        if self.is_controls_locked || self.black_flagged {
            let c = &mut self.controls;
            c.gas = 0.0;
            c.brake = 0.0;
            c.steer = 0.0;
            c.clutch = 0.0;
            c.gear_up = false;
            c.gear_dn = false;
            c.kers = false;
            feed.set_vibrations(&VibrationDef::default());
            feed.send_ff(0.0, 0.0, self.user_ff_gain);
            feed.set_engine_rpm(0.0, 1000.0, limiter.unwrap_or(0) as f32);
            return;
        }
        feed.poll_controls(self);
        if let (Some(limiter), Some(drivetrain)) = (limiter, &self.drivetrain) {
            let limiter = limiter as f32;
            let high = limiter * 0.95;
            let low = limiter * 0.75;
            feed.set_engine_rpm(drivetrain.get_engine_rpm(), low, high);
        }

        // the rumble for the device, from what the previous step left
        let speed = self.speed;
        let phase = speed * dt + self.vibration_phase;
        self.vibration_phase = phase;
        let mut def = VibrationDef::default();
        let (mut gain, mut lengths, mut count, mut slip) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for (index, tyre) in self.tyres.iter().enumerate() {
            if let Some(surface) = &tyre.surface_def {
                let g = surface.vibration_gain;
                if g > gain {
                    gain = g;
                }
                let length = surface.vibration_length;
                if ordered_nonzero(length) && ordered_nonzero(g) {
                    lengths += length;
                    count += 1.0;
                }
            }
            let x = tyre.status.nd_slip * 0.75;
            if index == 0 {
                if x >= 0.0 {
                    slip = x;
                }
            } else if !(slip > x) {
                slip = x;
            }
        }
        if !(slip > 1.0) {
            slip *= slip;
        }
        let length = lengths / count;
        if ordered_nonzero(gain) && ordered_nonzero(length) {
            let front_left = clamp01(self.tyres[0].status.load);
            let front_right = clamp01(self.tyres[1].status.load);
            def.curbs = (((ks_saw_tooth_wave(phase, length) * front_left) * front_right) * gain) * clamp01(speed);
        }
        let slip_phase = dt + self.slip_vibration_phase;
        self.slip_vibration_phase = slip_phase;
        def.gforce = sinf(self.vibration_phase * 30.0) * clamp01(self.acc_g.y.abs());
        def.slips = sinf(slip_phase * 120.0) * clamp01(slip * 0.4);
        if let (Some(limiter), Some(drivetrain)) = (limiter, &self.drivetrain) {
            let rpm = ((drivetrain.base().engine.velocity as f32) * 0.159_155_07) * 60.0;
            def.engine = clamp01(rpm / limiter as f32);
        }
        def.curbs *= clamp01(speed);
        def.slips *= clamp01(speed);
        let abs_present = self.aids.as_ref().is_some_and(|aids| aids.base().abs.is_present);
        if abs_present && super::Abs::is_in_action(self) {
            def.abs = ks_square_wave(self.physics_time as f32, 100.0) * clamp01(speed);
        }
        feed.set_vibrations(&def);
    }

    /// The penalty timers of `Car::step` (the block at 0x14027614a). Nothing here touches the
    /// controls: "cut gas" names when the timer runs down, namely only while the driver
    /// himself keeps the throttle under 10 %.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_penalty(&mut self, dt: f32) {
        let penalty = self.penalty_time;
        if !(penalty > 0.0) {
            return;
        }
        match self.env.penalty_mode {
            2 => {
                // recover time
                self.penalty_time = self.penalty_perf_target - self.performance_split;
            }
            0 => {
                if 0.1 > self.controls.gas {
                    let left = self.penalty_time_accumulator - dt as f64;
                    self.penalty_time_accumulator = left;
                    if !(left > 0.0) {
                        self.penalty_time = 0.0;
                        self.penalty_time_accumulator = 0.0;
                        self.disable_min_speed_penalty_clear = false;
                    }
                } else {
                    self.penalty_time_accumulator = penalty;
                }
                if self.disable_min_speed_penalty_clear {
                    return;
                }
                // below 35 km/h the penalty is over
                let v = self.core.get_velocity(self.body);
                let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
                if !ordered_nonzero(squared) || !(sqrtf(squared) >= 9.722_222) {
                    self.penalty_time = 0.0;
                    self.penalty_time_accumulator = 0.0;
                }
            }
            _ => {}
        }
    }

    /// `Car::addPenalty` @ 0x14026f6a0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn add_penalty(&mut self, seconds: f64) {
        if !(0.0 >= self.penalty_time) {
            self.disable_min_speed_penalty_clear = true;
        }
        self.penalty_time = seconds + self.penalty_time;
        self.penalty_time_accumulator = seconds + self.penalty_time_accumulator;
    }

    /// `Car::clearPenalty` @ 0x14026fc50.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn clear_penalty(&mut self) {
        if !(0.0 >= self.penalty_time) {
            self.penalty_time = 0.0;
            self.penalty_time_accumulator = 0.0;
            self.disable_min_speed_penalty_clear = false;
        }
    }

    /// `Car::getPenaltyTime` @ 0x140270d50: the time still to serve.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_penalty_time(&self) -> f64 {
        if !(0.0 >= self.penalty_time) {
            self.penalty_time_accumulator
        } else {
            0.0
        }
    }

    /// Queues a change for the start of the next step, as the game's main thread does with
    /// everything it asks of the physics thread (locks, a gentle stop, a black flag, cockpit
    /// clicks, teleports). The job runs when the clock already shows that step's time.
    pub fn queue(&mut self, job: impl FnOnce(&mut RollingChassis) + 'static) {
        self.pre_step_jobs.push(Box::new(job));
    }

    /// `Car::onNewSession` @ 0x140274c30: a new session lifts a black flag and un-retires the
    /// car (the lap-time meter it also resets is not ported).
    pub fn on_new_session(&mut self) {
        self.is_retired = false;
        self.black_flagged = false;
    }

    /// `Car::lockControls` @ 0x1402745f0.
    pub fn lock_controls(&mut self, locked: bool) {
        self.is_controls_locked = locked;
    }

    /// `Car::lockControlsUntil` @ 0x140274600: `seconds_ms` from `now` (or on top of a lock
    /// that is still running); 0 ends the lock.
    pub fn lock_controls_until(&mut self, time_ms: f64, now: f64) {
        if !(time_ms < 0.0 || time_ms > 0.0) {
            self.lock_controls_time = 0.0;
        } else if self.lock_controls_time > self.physics_time {
            self.lock_controls_time += time_ms;
        } else {
            self.lock_controls_time = time_ms + now;
        }
    }

    /// `Car::setBlackFlag` @ 0x1402759f0 (the flag event of the engine is not ported).
    pub fn set_black_flag(&mut self, flag: bool) {
        self.black_flagged = flag;
    }

    /// `Car::isInPits` @ 0x140274530: the body is within 3 m of the pit box.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn is_in_pits(&self) -> bool {
        let p = self.core.get_position(self.body);
        let m = &self.pit_position.m;
        let (dx, dy, dz) = (p.x - m[3][0], p.y - m[3][1], p.z - m[3][2]);
        // `comiss` + `setbe`: true for a NaN
        !((dy * dy + dx * dx) + dz * dz > 9.0)
    }

    /// `Car::isInPitLane` @ 0x1402744e0: a tyre stands on a pit-lane surface.
    pub fn is_in_pit_lane(&self) -> bool {
        self.tyres.iter().any(|tyre| tyre.surface_def.as_ref().is_some_and(|surface| surface.is_pitlane))
    }

    /// `Car::updateColliderStatus` @ 0x140276df0: on the pit lane the body mesh stops
    /// colliding with other cars, until the car has left it and no other car is within 6 m;
    /// a car on its side or roof also collides with the road.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn update_collider_status(&mut self) {
        if self.is_in_pit_lane() {
            self.is_collision_off_for_pits = true;
        } else if self.is_collision_off_for_pits {
            let pos = self.core.get_position(self.body);
            let near = self.other_car_positions.iter().any(|q| {
                let (dx, dy, dz) = (q.x - pos.x, q.y - pos.y, q.z - pos.z);
                !((dy * dy + dx * dx) + dz * dz >= 36.0)
            });
            if !near {
                self.is_collision_off_for_pits = false;
            }
        }
        let mut mask = if self.unix_name == "spectator" {
            2
        } else if self.is_collision_off_for_pits {
            0x1a
        } else {
            0x1e
        };
        let up = self.core.get_world_matrix(self.body).m[1][1];
        if 0.25 > up {
            mask |= 1;
        }
        self.mesh_collide_mask = mask;
        // (the game writes the mask of mesh 0 unchecked; a car without a mesh has none)
        self.core.set_mesh_collide_mask(self.body, 0, mask);
    }

    /// `PhysicsEngine::hasSessionStarted` @ 0x140263c70.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn has_session_started(&self, offset_ms: f64) -> bool {
        !(offset_ms + self.env.session_start_time_ms >= self.physics_time)
    }

    /// `Car::stepJumpStart` @ 0x140276780 (the player's car only): before the start, inside
    /// the window in which the gearbox would be locked, moving more than 10 cm off the grid
    /// position is a jump start. There is no latch: it is punished again every step.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_jump_start(&mut self) {
        if self.has_session_started(0.0) {
            return;
        }
        let lock = self.env.lock_gearbox_at_start_time_ms;
        if !(lock < 0.0 || lock > 0.0) {
            return;
        }
        if !(0.0 >= self.lock_controls_time) {
            return;
        }
        let pos = self.core.get_position(self.body);
        if self.has_grid_position && !(self.env.session_start_time_ms - lock > self.physics_time) {
            let dz = pos.z - self.grid_position.z;
            let dx = pos.x - self.grid_position.x;
            if dz * dz + dx * dx > 0.010_000_001 {
                self.add_jump_start_penalty();
                self.jump_start_events += 1;
            }
            return;
        }
        self.grid_position = pos;
        self.has_grid_position = true;
    }

    /// `PenaltyManager::addJumpStartPenalty` @ 0x140265a90.
    fn add_jump_start_penalty(&mut self) {
        match self.env.jump_start_penalty_mode {
            1 => {
                // back to the pit box, controls dead for 20 s after the start
                let m = self.pit_position.m;
                self.force_position(&Vec3f::new(m[3][0], m[3][1], m[3][2]));
                self.force_rotation(&Vec3f::new(-m[2][0], -m[2][1], -m[2][2]));
                self.lock_controls_until(20000.0, self.env.session_start_time_ms);
            }
            2 => {
                self.penalty_manager.pending_penalty_type = 5;
                self.penalty_manager.pit_penalty_laps = self.env.base_pit_penalty_laps;
                self.penalty_manager.penalty_records += 1;
            }
            _ => {}
        }
    }

    /// `Car::postStep` @ 0x140275430, after the rigid-body step: the car's wake moves to where
    /// the car is now. (Its other half copies the racing-line locator's results, which need a
    /// track.)
    fn post_step(&mut self, dt: f32) {
        let velocity = self.core.get_velocity(self.body);
        let position = self.core.get_position(self.body);
        self.slip_stream.set_position(&position, &velocity);
        if let Some(track) = &self.track {
            let empty = crate::track::AiSpline::default();
            {
                let spline = track.ai_spline.as_ref().unwrap_or(&empty);
                let locator = self.spline_locator;
                // the game's step-completed handler calls Car::postStep with a time step of
                // zero (lambda @ 0x14026ef00: `xorps xmm1, xmm1`), so the "side velocity" it
                // divides by that is an infinity or a NaN, never a velocity
                let _ = dt;
                locator.post_step(&mut self.spline_locator_data, spline, &position, 0.0);
            }
        }
    }

    /// The state of a body as the comparison with the recordings wants it.
    pub fn body_trace(&self, body: RigidBody) -> BodyTrace {
        let b = self.core.world.body(body.id);
        let mut r = [0.0; 9];
        for row in 0..3 {
            for col in 0..3 {
                r[3 * row + col] = b.r[4 * row + col];
            }
        }
        BodyTrace {
            mass: b.mass.mass,
            inertia: [b.mass.i[0], b.mass.i[5], b.mass.i[10]],
            pos: [b.pos[0], b.pos[1], b.pos[2]],
            q: b.q,
            r,
            lvel: [b.lvel[0], b.lvel[1], b.lvel[2]],
            avel: [b.avel[0], b.avel[1], b.avel[2]],
            facc: [b.facc[0], b.facc[1], b.facc[2]],
            tacc: [b.tacc[0], b.tacc[1], b.tacc[2]],
        }
    }

    /// `Tyre::step` @ 0x140283800 of one wheel, with its hub, the road and the car.
    fn step_tyre(&mut self, index: usize, dt: f32) {
        let mut tyre = std::mem::take(&mut self.tyres[index]);
        let trace = self.trace.is_some().then(|| {
            let body_velocity = self.core.get_velocity(self.body);
            RefCell::new(TyreTrace {
                input: rig::StepInput {
                    hub_matrix: [0.0; 16],
                    hub_velocity: [0.0; 3],
                    hub_angular_velocity: [0.0; 3],
                    brake_torque: tyre.inputs.brake_torque,
                    hand_brake_torque: tyre.inputs.hand_brake_torque,
                    electric_torque: tyre.inputs.electric_torque,
                    abs_override: tyre.abs_override,
                    ai_mult: tyre.ai_mult,
                    driven: tyre.driven,
                    set_angular_velocity: Some(tyre.status.angular_velocity),
                    set_blankets: Some(tyre.tyre_blankets_on),
                    ext_active: tyre.external_inputs.is_active,
                    ext_load: tyre.external_inputs.load,
                    ext_slip_angle: tyre.external_inputs.slip_angle,
                    ext_slip_ratio: tyre.external_inputs.slip_ratio,
                    has_hit: false,
                    ground_y: 0.0,
                    ground_normal: [0.0; 3],
                    grip_mod: 1.0,
                    dirt_additive_k: 0.0,
                    sin_height: 0.0,
                    sin_length: 0.0,
                    damping: 0.0,
                    granularity: 0.0,
                    has_car: true,
                    torque_mode: match self.torque_mode_ex {
                        TorqueModeEx::Original => 0,
                        TorqueModeEx::ReactionTorques => 1,
                        TorqueModeEx::DriveTorques => 2,
                        TorqueModeEx::Other => 3,
                    },
                    car_speed: self.speed,
                    car_sleeping: self.is_sleeping(),
                    dynamic_grip_level: self.env.dynamic_grip_level,
                    tyre_consumption_rate: self.env.tyre_consumption_rate,
                    mechanical_damage_rate: self.env.mechanical_damage_rate,
                    ambient_temperature: self.env.ambient_temperature,
                    road_temperature: self.env.road_temperature,
                    allow_tyre_blankets: self.env.allow_tyre_blankets,
                    body_velocity: arr(&body_velocity),
                    body_mass: self.core.get_mass(self.body),
                },
                wheel_rotation_in: tyre.local_wheel_rotation,
                calls: Vec::new(),
                output: Vec::new(),
                ray: RayTrace::default(),
            })
        });
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Tyre);
        let ((), mut body_calls) =
            self.with_tyre_ports(index, trace.as_ref(), |hub, ground, car| tyre.step(dt, hub, Some(ground), Some(car)));
        self.core.source = previous;
        if let Some(trace) = trace {
            let mut trace = trace.into_inner();
            // the body call is recorded after the tyre's own hub calls, as in the rig
            trace.calls.append(&mut body_calls);
            trace.output = rig::snapshot(&tyre, &trace.calls);
            self.trace.as_mut().unwrap().tyres.push(trace);
        }
        self.tyres[index] = tyre;
    }

    /// `Car::getSteerFF` @ 0x140272180: the steering torque of the two front wheels as a
    /// force-feedback level.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_steer_ff(&mut self) -> f32 {
        let steer_torque = self.suspensions[1].get_steer_torque() + self.suspensions[0].get_steer_torque();
        let position = self.steer_lock * self.controls.steer;
        let ratio = self.steer_ratio.abs();
        let mut gyro = position - self.last_steer_position;
        self.mz_current = (self.mz_current - steer_torque) * self.env.ff_filter + steer_torque;
        gyro = gyro * 333.333_34;
        gyro = gyro / ratio;
        let w0 = self.tyres[0].status.angular_velocity;
        let w1 = self.tyres[1].status.angular_velocity;
        self.last_pure_mz_ff = self.mz_current * 1.4;
        gyro = gyro * ((w1.abs() + w0.abs()) * self.tyres[0].data.angular_inertia);
        gyro = gyro * self.env.gyro_wheel_gain;
        self.last_steer_position = position;
        let mut ff = -(self.mz_current + gyro) * 1.4;

        // flat-spot shake (flat spots and blisters are doubles)
        let s = |i: usize| self.tyres[i].status;
        let mut flat_spot = if s(0).flat_spot > s(1).flat_spot { s(0).flat_spot } else { s(1).flat_spot };
        let front = if s(0).blister > s(1).blister { s(0).blister } else { s(1).blister };
        let rear = if s(2).blister > s(3).blister { s(2).blister } else { s(3).blister };
        let blister = (if front > rear { front } else { rear }) * 0.003;
        if !(flat_spot > blister) {
            flat_spot = blister;
        }
        let rotation = w0.abs() + w1.abs();
        self.flat_spot_phase = rotation * 0.003 + self.flat_spot_phase;
        if rotation > 7.0 {
            let mut shake = ks_saw_tooth_wave(self.flat_spot_phase, 12.566_36);
            shake = shake * flat_spot as f32;
            shake = shake * self.env.flat_spot_ff_gain;
            shake = shake * (s(1).load + s(0).load);
            shake = shake * 0.5;
            shake = shake + 1.0;
            ff = ff + shake;
        }

        // the car's gain and the steer-assist curve
        if self.steer_assist != 1.0 && !self.steer_assist.is_nan() {
            let level = powf((ff * self.ff_mult).abs(), self.steer_assist);
            ff = if ff > 0.0 {
                1.0 * level
            } else if ff >= 0.0 {
                0.0 * level
            } else {
                -1.0 * level
            };
        } else {
            ff = ff * self.ff_mult;
        }
        self.last_gyro_ff = gyro * 1.4;
        ff = -ff;

        // optional lightening when the front tyres slide
        if self.env.use_fake_understeer_ff {
            let slide = (s(1).nd_slip + s(0).nd_slip) * 0.5 - 1.0;
            let clamped = |x: f32| {
                if x > 1.0 {
                    1.0
                } else if !(x >= 0.0) {
                    0.0
                } else {
                    x
                }
            };
            let mut divisor = clamped(slide) * 5.5 + 1.0;
            if divisor > 2.5 {
                divisor = 2.5;
            }
            ff = ff / divisor;
        }
        ff
    }

    /// `Car::onTyresStepCompleted` @ 0x140274cd0: runs at the end of the fourth tyre's step.
    /// The call to the device (`sendFF`) follows in [`RollingChassis::step`], which has the device.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn on_tyres_step_completed(&mut self) {
        self.last_ff = self.get_steer_ff();
        let speed_kmh = self.env.mz_low_speed_reduction_speed_kmh;
        if ordered_nonzero(speed_kmh) {
            let mut k = (self.speed * 3.6) / speed_kmh;
            if k > 1.0 {
                k = 1.0;
            } else if !(k >= 0.0) {
                k = 0.0;
            }
            let min_value = self.env.mz_low_speed_reduction_min_value;
            self.last_ff = ((1.0 - min_value) * k + min_value) * self.last_ff;
        }
    }
}

// --- the car body touches things -----------------------------------------------------------

/// The game's `ACPhysicsEvent` of type 0 (a collision), as `Car::onCollisionCallBack` pushes
/// it on the physics engine's event queue (the game's sounds read it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsEvent {
    /// `type`: 0 = collision
    pub kind: i32,
    /// `param1`: the car's `physicsGUID`
    pub param1: f32,
    /// `param2`: the contact's depth, m
    pub param2: f32,
    /// `param3`: -1
    pub param3: f32,
    /// `param4`: the closing speed along the contact normal, km/h (negative when parting)
    pub param4: f32,
    /// `vParam1`: the contact point
    pub v_param1: Vec3f,
    /// `vParam2`: the contact normal
    pub v_param2: Vec3f,
    /// `ulParam0`: the group (category) of the second shape
    pub ul_param0: u32,
}

/// What `Car::evOnCollisionEvent` hands its listeners: a contact that damages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnCollisionEvent {
    /// The other body (`None`: the track).
    pub body: Option<RigidBody>,
    /// Closing speed, km/h.
    pub rel_speed: f32,
    pub world_pos: Vec3f,
    /// The contact point in the car body's frame.
    pub rel_pos: Vec3f,
    /// The group of the second shape.
    pub collider_group: u32,
}

/// `Car::physicsGUID`: the number of cars the engine had when this one was built. The port
/// has one car.
const PHYSICS_GUID: u32 = 0;

impl RollingChassis {
    /// `CarColliderManager::init` -> `loadINI` @ 0x1402a37a0: the floor boxes of
    /// `colliders.ini` on the car's body, category 4, colliding with category 1 (track
    /// surfaces) only, in the car's own sub-space.
    fn install_box_colliders(&mut self) -> Result<(), String> {
        let boxes = super::colliders::load_boxes(&self.data_path)?;
        for def in &boxes {
            self.core.add_box_collider(self.body, &def.centre, &def.size, super::body::category::CAR, super::body::category::SURFACE, PHYSICS_GUID + 1);
        }
        self.colliders.boxes = boxes;
        Ok(())
    }

    /// `Car::initColliderMesh` @ 0x140273b20 (called by the game's `CarAvatar::initPhysics`
    /// when the car has a `collider.kn5`): the car's bounds, and the mesh on the body with
    /// category 4 and the collide bits 0x1e (walls, cars, loose objects).
    pub fn init_collider_mesh(&mut self, mesh: ColliderMesh) {
        self.bounds = super::colliders::bounds(&mesh);
        self.core.add_mesh_collider(self.body, mesh.vertices.clone(), mesh.indices.clone(), &mesh.matrix, super::body::category::CAR, 0x1e, PHYSICS_GUID + 1);
        self.colliders.mesh = Some(mesh);
    }

    /// The collider mesh from the game's folder (`content/cars/<name>/collider.kn5`), when
    /// there is one. `Ok(false)`: the car has none (it then has only its floor boxes).
    pub fn load_collider_mesh(&mut self, game_root: &Path) -> Result<bool, String> {
        let name = self.unix_name.clone();
        let loaded = super::colliders::load(&self.data_path, Some(game_root), &name)?;
        match loaded.mesh {
            Some(mesh) => {
                self.init_collider_mesh(mesh);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// `PhysicsEngine::setSessionInfo` @ 0x140264560, the part about collisions: at the start
    /// of a session the contacts are thrown away and no new ones are looked for during 250
    /// steps (0.75 s).
    pub fn reset_collisions_for_new_session(&mut self) {
        self.core.reset_collisions();
        self.core.set_no_collision_steps(250);
    }

    /// `PhysicsCore::step` @ 0x1402cd690 for the car's core: the collision pass with the
    /// game's callback (`PhysicsEngine::onCollisionCallBack` -> `Car::onCollisionCallBack`)
    /// after every new contact joint, then `dWorldStep`.
    fn step_core(&mut self, dt: f32) {
        self.physics_events.clear();
        self.collision_events.clear();
        if self.collisions_enabled {
            let track = self.track.clone();
            let own = self.core.statics.clone();
            let statics = match &track {
                Some(track) => Some(&track.world),
                None => own.as_deref(),
            };
            let events = self.core.collision_step(statics);
            self.contact_callbacks = events.len() as u32;
            for event in &events {
                self.on_collision_callback(event);
            }
        }
        self.step_stats = self.core.world_step(dt);
    }

    /// `PhysicsEngine::onCollisionCallBack` @ 0x140264020 (the pair is turned round when only
    /// the second body is a car's) and `Car::onCollisionCallBack` @ 0x140274650: the closing
    /// speed at the contact point, in km/h, is the damage; a zone keeps the largest it saw.
    /// Contacts with the ground (a track surface, a loose object) do not damage.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn on_collision_callback(&mut self, event: &CollisionEvent) {
        let (mut body_a, mut shape_a, mut body_b, mut shape_b) = (event.body_a, event.shape_a, event.body_b, event.shape_b);
        if body_a != Some(self.body) && body_b == Some(self.body) {
            std::mem::swap(&mut body_a, &mut body_b);
            std::mem::swap(&mut shape_a, &mut shape_b);
        }
        // not this car's body: nothing (hubs and the fuel tank have no shapes)
        if body_a != Some(self.body) {
            return;
        }
        let other = body_b;
        let is_ground = |shape: Option<Shape>| shape.is_some_and(|s| s.group == 1 || s.group == 0x10);
        let ground_a = is_ground(shape_a);
        let ground_b = is_ground(shape_b);
        self.last_collision_time = self.physics_time;
        let local = self.core.world_to_local(self.body, &event.pos);
        let v_other = match other {
            Some(other) => self.core.get_point_velocity(other, &event.pos),
            None => Vec3f::new(0.0, 0.0, 0.0),
        };
        let v_mine = self.core.get_point_velocity(self.body, &event.pos);
        let n = &event.normal;
        let rel_speed = -((((v_mine.y - v_other.y) * n.y + (v_mine.x - v_other.x) * n.x) + (v_mine.z - v_other.z) * n.z) * 3.6);
        if body_a.is_some() && body_b.is_some() && !ground_a && !ground_b {
            self.last_collision_with_car_time = self.physics_time;
        }
        let damaging = rel_speed > 0.0 && !ground_a && !ground_b;
        if damaging {
            let d = rel_speed * self.env.mechanical_damage_rate;
            if d > 150.0 {
                if let Some(drivetrain) = &mut self.drivetrain {
                    drivetrain.engine_mut().blow_up();
                }
            }
            let mut dir = Vec3f::new(local.x, 0.0, local.z);
            dir.normalize();
            let zone = if dir.z.abs() > 0.707 {
                if local.z > 0.0 {
                    0
                } else {
                    1
                }
            } else if local.x >= 0.0 {
                2
            } else {
                3
            };
            if d > self.damage_zone_level[zone] {
                self.damage_zone_level[zone] = d;
            }
            if d > self.damage_zone_level[4] {
                self.damage_zone_level[4] = d;
            }
        }
        // every call, even for ground contacts: the suspension of a corner is bent by the mean
        // of its two zones
        let dz = self.damage_zone_level;
        for (wheel, (a, b)) in [(0usize, 2usize), (0, 3), (1, 2), (1, 3)].into_iter().enumerate() {
            if dz[a] > 0.0 && dz[b] > 0.0 {
                self.suspensions[wheel].set_damage((dz[a] + dz[b]) * 0.5);
            }
        }
        // (the game reads the second shape's group without a null test; a box, the only shape
        // without an object, is always on the car's side)
        let group = shape_b.map_or(0, |s| s.group);
        self.physics_events.push(PhysicsEvent {
            kind: 0,
            param1: PHYSICS_GUID as f32,
            param2: event.depth,
            param3: -1.0,
            param4: rel_speed,
            v_param1: event.pos,
            v_param2: event.normal,
            ul_param0: group,
        });
        if damaging {
            self.collision_events.push(OnCollisionEvent { body: other, rel_speed, world_pos: event.pos, rel_pos: local, collider_group: group });
        }
    }

    /// `Car::setDamageLevel` @ 0x140275b20: all five zones.
    pub fn set_damage_level(&mut self, level: f32) {
        self.damage_zone_level = [level; 5];
    }

    /// `Car::resetSuspensionDamageLevel` @ 0x140275970.
    pub fn reset_suspension_damage_level(&mut self) {
        for suspension in self.suspensions.iter_mut() {
            suspension.reset_damage();
        }
    }
}
