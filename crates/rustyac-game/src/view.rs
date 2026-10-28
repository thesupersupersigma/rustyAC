// SPDX-License-Identifier: GPL-3.0-or-later

//! What the display needs of the car after a step: where its parts are and what the HUD
//! shows. Read out of the physics car, never written back.

use rustyac_physics::car::Abs;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::sim::{aid_level_text, bias_text, AidNote, GameSim};

/// A 4x4 matrix as the physics has it: row vectors, the translation in the fourth row
/// (Direct3D's convention). Row 0 is the part's left (+x), row 1 its up, row 2 its forward.
pub type Mat = [[f32; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// How many loose objects the picture follows at a time.
pub const MOVED_OBJECTS: usize = 8;

/// How many other cars a [`CarView`] can show (Task 24: the debug view's boxes).
pub const OTHER_CARS: usize = 31;
/// Task 43c: the wings the car view carries (FormWings).
// shortcut: a car with more [WING_n] + [FIN_n] shows only the first 12 in the Wings app; raise if one appears
pub const MAX_WINGS: usize = 12;

/// What the game's `Car::getPhysicsState` @ 0x140270d70 hands the car's picture beyond the
/// members of [`CarView`]: the values the car's visual helpers read (ground shadows, skid
/// marks, smoke, disc glow, instruments).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderState {
    pub velocity: [f32; 3],
    pub local_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub local_angular_velocity: [f32; 3],
    /// degrees
    pub slip_angle: [f32; 4],
    pub slip_ratio: [f32; 4],
    /// `TyreStatus::slipFactor`
    pub tyre_slip: [f32; 4],
    pub dy: [f32; 4],
    pub mz: [f32; 4],
    pub dirty_level: [f32; 4],
    pub camber_rad: [f32; 4],
    pub loaded_radius: [f32; 4],
    pub suspension_travel: [f32; 4],
    pub contact_point: [[f32; 3]; 4],
    pub contact_normal: [[f32; 3]; 4],
    /// the plane through the first three tyres' contact points: normal, offset
    pub ground_plane: [f32; 4],
    /// the last surface each tyre touched
    pub surface_grip_mod: [f32; 4],
    pub surface_dirt_additive_k: [f32; 4],
    pub surface_is_valid_track: [bool; 4],
    pub surface_is_pitlane: [bool; 4],
    pub disc_temps: [f32; 4],
    pub core_temps: [f32; 4],
    /// `Car::getTyreThermalState` 0x1402727b0: the tread's temperatures as the game lays them
    /// out (element `e` of stripe `s` at `s + 3 * e`, the stripes turned round on the left
    /// tyres), the pressures, the grip multiplier, "hot"
    pub tyre_temps: [[[f32; 12]; 3]; 4],
    pub dynamic_pressure: [f32; 4],
    pub static_pressure: [f32; 4],
    pub thermal_mult: [f32; 4],
    pub is_hot: [bool; 4],
    /// `CarAvatar::getCurrentCompoundIdealPressure` 0x1400d3180: the compound's ideal
    /// pressure, cut to a whole number
    pub ideal_pressure: [i32; 4],
    /// `CarPhysicsState::wear` (-1: the tyre does not wear) and `wearMult`
    pub wear: [f32; 4],
    pub wear_mult: [f32; 4],
    pub virtual_km: [f32; 4],
    pub inflation: [f32; 4],
    pub flat_spot: [f32; 4],
    pub water: f32,
    pub turbo_boost: f32,
    pub turbo_bov: f32,
    pub drivetrain_speed: f32,
    pub is_gear_grinding: bool,
    pub is_engine_limiter_on: bool,
    pub cg_height: f32,
    pub time_stamp: f64,
}

/// The car after one physics step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarView {
    /// See [`RenderState`].
    pub render: RenderState,
    /// Physics steps so far, and the seconds they stand for.
    pub steps: u64,
    pub sim_seconds: f64,
    /// Seconds driven since the last spawn (the HUD's timer).
    pub drive_seconds: f64,
    /// The car body's world matrix.
    pub body: Mat,
    /// The track's loose objects that are not where the track has them (awake, or lying
    /// where a hit left them): the object's number and its body's world matrix. At most
    /// [`MOVED_OBJECTS`] are shown moved.
    pub moved_objects: [(u16, Mat); MOVED_OBJECTS],
    pub moved_object_count: u8,
    /// Task 24: the body matrices of the session's other cars (the first [`OTHER_CARS`]).
    /// Only the debug view draws them, as boxes; AC's renderer does not draw other cars yet.
    pub other_cars: [Mat; OTHER_CARS],
    pub other_car_count: u8,
    /// New sessions so far (`GameSim::new_sessions`)
    pub new_sessions: u32,
    /// `CarPhysicsState::performanceMeter` (s) and `performanceMeterSpeedDiffMS` (m/s): the
    /// car's `PerformanceMeter` (Task 33)
    pub performance_meter: (f32, f32),
    /// `TyreWearingDebug`'s rows of each tyre (Task 33b)
    pub tyre_debug: [rustyac_gui::app_tyre_debug::TyreDebugRow; 4],
    /// each wheel's `CarAvatar::getTyreCompoundIndex`, for the Tyre tester
    pub compound_index: [i32; 4],
    /// Task 33b: what `RaceEngineer` reads for the Car engineer app (no masses: see `car_physics`)
    pub engineer: rustyac_gui::app_car_engineer::CarEngineer,
    /// Task 43c: `CarAvatar::wingsStatus` of the first [`MAX_WINGS`] wings (FormWings)
    pub wing_states: [rustyac_physics::car::aero::WingState; MAX_WINGS],
    pub wing_state_count: u8,
    /// Task 43c: `CarAvatar::getSuspensionDebugLines(w)` (ACSuspension's 3D lines), up to
    /// [`rustyac_physics::engine::record::DEBUG_LINES`] a wheel
    pub sus_lines: [[rustyac_physics::car::suspension::DebugLine; rustyac_physics::engine::record::DEBUG_LINES]; 4],
    pub sus_line_count: [u8; 4],
    /// `CarAvatar::pitPosition`'s place (the pit box)
    pub pit_position: [f32; 3],
    /// every suspension's `getPackerRange`
    pub packer_range: [f32; 4],
    /// Each wheel's world matrix with its spin (`localWheelRotation` x the hub's matrix),
    /// in the order LF, RF, LR, RR.
    pub wheels: [Mat; 4],
    /// Each hub's world matrix (steering and camber, no spin).
    pub hubs: [Mat; 4],
    /// `CarPhysicsState::tyreMatrix`: the wheel's rotation with its spin as the tyre had it in
    /// its step (`Tyre::getFinalTyreRotation`), at the hub's place. What the game's renderer
    /// and its sound take as the wheel.
    pub tyre_matrix: [Mat; 4],
    /// `CarPhysicsState::wheelAngularSpeed`, rad/s: 0 while the tyre is locked or the car sleeps.
    pub wheel_angular_speed: [f32; 4],
    pub tyre_radius: [f32; 4],
    pub tyre_width: [f32; 4],
    pub rim_radius: [f32; 4],
    /// Tyre load, N, and slip over the slip at peak grip (1 = at the limit).
    pub wheel_load: [f32; 4],
    pub wheel_slip: [f32; 4],
    pub speed_kmh: f32,
    /// `Car::getSpeed`, m/s
    pub speed_ms: f32,
    pub rpm: f32,
    /// The limiter's revs (the end of the rev bar).
    pub rpm_limit: f32,
    /// 0 reverse, 1 neutral, 2 first gear ...
    pub gear: i32,
    /// `Car::controls` after the step: what the car really used.
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steer: f32,
    pub hand_brake: f32,
    /// The steering wheel's angle, degrees (steer x lock).
    pub steer_deg: f32,
    pub tc_in_action: bool,
    pub abs_in_action: bool,
    /// (level from 1, number of levels); (0, 0) while switched off.
    pub tc_mode: (u32, u32),
    pub abs_mode: (u32, u32),
    pub tc_present: bool,
    pub abs_present: bool,
    /// `BrakeSystem::getFrontBias` (the cockpit's bias if one is set, else the setup's), the
    /// value of `acpmf_physics.brakeBias`: the front share, 0..1. Negative: no brake system.
    pub brake_bias: f32,
    /// The last change of an aid or of the bias (shown for a moment), see [`CarView::aid_note`].
    pub note: AidNote,
    pub drs: bool,
    pub lights: bool,
    pub auto_shifter: bool,
    /// The car's acceleration in g, body axes (the chase camera leans with it).
    pub acc_g: [f32; 3],
    /// The steering force sent to the device, -1..1.
    pub ff: f32,
    pub fuel: f32,
    /// Which device drove (see `input_file::StepInput::device`).
    pub device: u32,
    /// The lap, on a track.
    pub lap: LapView,
    /// `Car::damageZoneLevel`: front, rear, left, right, the largest; km/h of closing speed.
    pub damage: [f32; 5],
    /// `ISuspension::getDamage` per wheel: 0 … 1.
    pub suspension_damage: [f32; 4],
    /// `Engine::lifeLeft`: 1000 when new, below zero when blown.
    pub engine_life: f32,
    /// Contact joints of the body in the last step.
    pub contacts: u32,
    /// The surface under each tyre (its `KEY` in `surfaces.ini`), where the car is on a track.
    pub surfaces: [SurfaceName; 4],
    /// The session's conditions: air and road temperature (deg C), the track's grip (1 =
    /// 100 %), the wind's mean speed (km/h) and the direction it was set with (degrees).
    pub air: f32,
    /// The air density at the car, kg/m^3.
    pub air_density: f32,
    pub road: f32,
    pub grip: f32,
    pub wind_kmh: f32,
    pub wind_deg: f32,
    /// The tyre compound's name and the loaded setup's (empty: the default setup).
    pub compound: SurfaceName,
    pub setup: SurfaceName,
    /// The compound's short name (`CarAvatar::getCurrentCompoundShortName`).
    pub compound_short: SurfaceName,
    /// The hybrid system and the engine brake, for a car that has them.
    pub hybrid: HybridView,
    /// What the dashboard's lights read: every wing's angle as the aero uses it
    /// (`Wing::getCurrentModifiedAngle`), the fuel tank's size, the energy stores' allowances in
    /// joules (0: none), the drivetrain's display torque and its ratio.
    pub wing_angles: [f32; 8],
    pub wing_count: u8,
    /// The track's grip changes with the session (`Track::dynamicTrack.enabled`).
    pub dynamic_track: bool,
    pub max_fuel: f64,
    /// `Car::userFFGain`, `Car::ffMult`, `Car::steerAssist` (the force feedback app's)
    pub user_ff_gain: f32,
    pub ff_mult: f32,
    pub steer_assist: f32,
    /// `CarAvatar::getKmPerLiter` and `FuelLapEvaluator::getFuelLaps` (the displays' `FUEL_CONS`
    /// and `EST_LAPS`)
    pub km_per_liter: f32,
    pub fuel_laps: f32,
    pub kers_max_j: f32,
    pub ers_max_j: f32,
    pub total_torque: f32,
    pub drive_ratio: f64,
    /// Task 25: another player's car that is not there (its slot is empty, or nothing has
    /// been heard of it for five seconds): not drawn, not heard (`carNode->isActive`).
    pub disconnected: bool,
}

/// What the displays show of a KERS or an ERS and of the cockpit's engine-brake setting.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HybridView {
    pub has_kers: bool,
    pub has_ers: bool,
    /// The battery, 0..1 (`kersCharge`), and how much of the motor is used now (`kersInput`).
    pub charge: f32,
    pub input: f32,
    /// Energy handed out in this lap, kJ (`kersCurrentKJ`), and the lap's allowance (0: none).
    pub used_kj: f32,
    pub max_kj: f32,
    /// The MGU-K is filling the battery.
    pub charging: bool,
    /// An ERS's cockpit: the delivery profile (index, how many, its name), the recovery level
    /// (0..10), the MGU-H mode (true: it fills the battery), and which of the three the car's
    /// cockpit offers.
    pub power_index: i32,
    pub power_count: i32,
    pub power_name: SurfaceName,
    pub recovery: i32,
    pub heat_charging: bool,
    pub offers: [bool; 3],
    /// The engine-brake setting (index, how many; the control exists with more than one).
    pub engine_brake: i32,
    pub engine_brake_count: i32,
}

/// A short text that can be copied about: a surface's key.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceName {
    bytes: [u8; 16],
}

impl SurfaceName {
    pub fn new(text: &str) -> SurfaceName {
        let mut bytes = [0u8; 16];
        for (slot, byte) in bytes.iter_mut().zip(text.bytes().filter(u8::is_ascii)) {
            *slot = byte;
        }
        SurfaceName { bytes }
    }

    pub fn as_str(&self) -> &str {
        let end = self.bytes.iter().position(|b| *b == 0).unwrap_or(self.bytes.len());
        std::str::from_utf8(&self.bytes[..end]).unwrap_or("")
    }
}

/// What the lap displays show.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LapView {
    /// The car is on a track with timing lines.
    pub on_track: bool,
    /// The running lap's time, the last lap's and the best lap's, ms (0 = none). The best is
    /// the best lap without a cut.
    pub current_ms: u32,
    pub last_ms: u32,
    pub best_ms: u32,
    /// The last lap had no cut.
    pub last_valid: bool,
    /// Laps in the list.
    pub laps: u32,
    /// The sector the car is in (0 = the first), the sectors of the track, and the sector
    /// times of the running lap so far (0 = not yet).
    pub sector: u32,
    pub sector_count: u32,
    pub sector_ms: [u32; 4],
    /// The running lap is still clean (`transponder.cuts == 0`), and how often it was cut.
    pub valid: bool,
    pub cuts: i32,
    /// Tyres on a surface that is not track, as the game counts them.
    pub tyres_out: i32,
    /// The place along the lap, 0..1 (`normalizedCarPosition`).
    pub position: f32,
    pub in_pit_lane: bool,
    /// Task 28b: the player's pit stop, when there is one: `PitStop::state` and whether the
    /// pit box marker shows (the lollipop man and the marker follow them)
    pub pit_stop: Option<PitStopView>,
}

/// The player's pit stop as the picture and the pit crew read it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitStopView {
    /// `PitStop::state`
    pub state: i32,
    /// the pit box marker shows
    pub visible: bool,
    /// `pitTimer`, `maxPitTimer`, `tyreTime`
    pub pit_timer: f32,
    pub max_pit_timer: f32,
    pub tyre_time: f32,
    /// how often `evPitStopCommit` and `evPitStopOver` were raised so far
    pub commits: u32,
    pub overs: u32,
    /// what the last commit carried (`PitStopStart`), and `isChangingTyres` of that stop
    pub tyre_change_time: f32,
    pub total_time: f32,
    pub force_change_camera: bool,
    pub changing_tyres: bool,
}

impl Default for CarView {
    fn default() -> CarView {
        CarView {
            render: RenderState::default(),
            moved_objects: [(0, IDENTITY); MOVED_OBJECTS],
            moved_object_count: 0,
            other_cars: [IDENTITY; OTHER_CARS],
            other_car_count: 0,
            new_sessions: 0,
            performance_meter: (0.0, 0.0),
            tyre_debug: Default::default(),
            compound_index: [0; 4],
            engineer: Default::default(),
            wing_states: Default::default(),
            wing_state_count: 0,
            sus_lines: Default::default(),
            sus_line_count: [0; 4],
            pit_position: [0.0; 3],
            packer_range: [0.0; 4],
            steps: 0,
            sim_seconds: 0.0,
            drive_seconds: 0.0,
            body: IDENTITY,
            wheels: [IDENTITY; 4],
            tyre_matrix: [IDENTITY; 4],
            wheel_angular_speed: [0.0; 4],
            hubs: [IDENTITY; 4],
            tyre_radius: [0.33; 4],
            tyre_width: [0.3; 4],
            rim_radius: [0.165; 4],
            wheel_load: [0.0; 4],
            wheel_slip: [0.0; 4],
            speed_kmh: 0.0,
            speed_ms: 0.0,
            rpm: 0.0,
            rpm_limit: 10_000.0,
            gear: 1,
            gas: 0.0,
            brake: 0.0,
            clutch: 0.0,
            steer: 0.0,
            hand_brake: 0.0,
            steer_deg: 0.0,
            tc_in_action: false,
            abs_in_action: false,
            tc_mode: (0, 0),
            abs_mode: (0, 0),
            tc_present: false,
            abs_present: false,
            brake_bias: 0.0,
            note: AidNote::default(),
            drs: false,
            lights: false,
            auto_shifter: false,
            acc_g: [0.0; 3],
            ff: 0.0,
            fuel: 0.0,
            device: 0,
            lap: LapView::default(),
            damage: [0.0; 5],
            suspension_damage: [0.0; 4],
            engine_life: 1000.0,
            contacts: 0,
            surfaces: [SurfaceName::default(); 4],
            air: 26.0,
            air_density: 0.0,
            road: 30.0,
            grip: 1.0,
            wind_kmh: 0.0,
            wind_deg: 0.0,
            compound: SurfaceName::default(),
            setup: SurfaceName::default(),
            compound_short: SurfaceName::default(),
            hybrid: HybridView::default(),
            wing_angles: [0.0; 8],
            wing_count: 0,
            dynamic_track: false,
            max_fuel: 0.0,
            user_ff_gain: 1.0,
            ff_mult: 0.0,
            steer_assist: 1.0,
            km_per_liter: 0.0,
            fuel_laps: -1.0,
            kers_max_j: 0.0,
            ers_max_j: 0.0,
            total_torque: 0.0,
            drive_ratio: 0.0,
            disconnected: false,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length > 1e-12 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        v
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Between two poses of a rigid part: the position on the straight line, the axes blended and
/// made square again. Good for the small turn of one 3 ms step; a fast wheel's spin of up to
/// a quarter turn per step is blended along the chord, which is all a spinning wheel needs.
pub fn lerp_pose(a: &Mat, b: &Mat, t: f32) -> Mat {
    let row = |k: usize| [lerp(a[k][0], b[k][0], t), lerp(a[k][1], b[k][1], t), lerp(a[k][2], b[k][2], t)];
    let mut z = normalize(row(2));
    let y = row(1);
    let mut x = normalize(cross(y, z));
    if dot(x, x) < 0.5 {
        // the blend collapsed (half a turn apart): take the nearer pose
        return if t < 0.5 { *a } else { *b };
    }
    let y = normalize(cross(z, x));
    // keep the handedness of the inputs
    if dot(x, [a[0][0], a[0][1], a[0][2]]) + dot(x, [b[0][0], b[0][1], b[0][2]]) < 0.0 {
        x = [-x[0], -x[1], -x[2]];
        z = normalize(cross(x, y));
    }
    [
        [x[0], x[1], x[2], 0.0],
        [y[0], y[1], y[2], 0.0],
        [z[0], z[1], z[2], 0.0],
        [lerp(a[3][0], b[3][0], t), lerp(a[3][1], b[3][1], t), lerp(a[3][2], b[3][2], t), 1.0],
    ]
}

impl CarView {
    /// The part of the game's `CarPhysicsState` its renderer poses the car from.
    pub fn physics_state(&self) -> rustyac_physics::car_physics_state::CarPhysicsState {
        let m = |m: &Mat| Mat44f { m: *m };
        let r = &self.render;
        let mut surfaces = [rustyac_physics::car_physics_state::SurfaceDef::default(); 4];
        let mut thermal = [rustyac_physics::car_physics_state::TyreThermalState::default(); 4];
        for i in 0..4 {
            surfaces[i].grip_mod = r.surface_grip_mod[i];
            surfaces[i].dirt_additive_k = r.surface_dirt_additive_k[i];
            surfaces[i].is_valid_track = r.surface_is_valid_track[i];
            surfaces[i].is_pitlane = r.surface_is_pitlane[i];
            thermal[i].core_temp = r.core_temps[i];
            thermal[i].temps = r.tyre_temps[i];
            thermal[i].dynamic_pressure = r.dynamic_pressure[i];
            thermal[i].static_pressure = r.static_pressure[i];
            thermal[i].mult = r.thermal_mult[i];
            thermal[i].is_hot = r.is_hot[i];
        }
        rustyac_physics::car_physics_state::CarPhysicsState {
            world_matrix: m(&self.body),
            suspension_matrix: [m(&self.hubs[0]), m(&self.hubs[1]), m(&self.hubs[2]), m(&self.hubs[3])],
            tyre_matrix: [m(&self.tyre_matrix[0]), m(&self.tyre_matrix[1]), m(&self.tyre_matrix[2]), m(&self.tyre_matrix[3])],
            wheel_angular_speed: self.wheel_angular_speed,
            steer: self.steer_deg,
            engine_rpm: if self.rpm >= 0.0 { self.rpm } else { 0.0 },
            is_engine_limiter_on: r.is_engine_limiter_on,
            gas: self.gas,
            brake: self.brake,
            clutch: self.clutch,
            gear: self.gear,
            speed: self.speed_ms,
            velocity: r.velocity,
            local_velocity: r.local_velocity,
            angular_velocity: r.angular_velocity,
            local_angular_velocity: r.local_angular_velocity,
            slip_angle: r.slip_angle,
            slip_ratio: r.slip_ratio,
            tyre_slip: r.tyre_slip,
            nd_slip: self.wheel_slip,
            load: self.wheel_load,
            dy: r.dy,
            mz: r.mz,
            tyre_dirty_level: r.dirty_level,
            tyre_surface_def: surfaces,
            cg_height: r.cg_height,
            acc_g: self.acc_g,
            tyre_contact_point: r.contact_point,
            tyre_contact_normal: r.contact_normal,
            camber_rad: r.camber_rad,
            tyre_radius: self.tyre_radius,
            tyre_loaded_radius: r.loaded_radius,
            suspension_travel: r.suspension_travel,
            drivetrain_speed: r.drivetrain_speed,
            turbo_boost: r.turbo_boost,
            turbo_bov: r.turbo_bov,
            is_gear_grinding: r.is_gear_grinding,
            tyre_virtual_km: r.virtual_km,
            damage_zone_level: self.damage,
            limiter_rpm: self.rpm_limit as i32,
            ground_plane: r.ground_plane,
            time_stamp: r.time_stamp,
            air_density: self.air_density,
            fuel: self.fuel,
            fuel_laps: self.fuel_laps,
            engine_life_left: self.engine_life,
            tyre_inflation: r.inflation,
            wear: r.wear,
            wear_mult: r.wear_mult,
            lap_time: self.lap.current_ms,
            last_lap: self.lap.last_ms,
            best_lap: self.lap.best_ms,
            lap_count: self.lap.laps,
            normalized_spline_position: self.lap.position,
            last_ff_pure: self.ff,
            kers_charge: self.hybrid.charge,
            kers_input: self.hybrid.input,
            kers_current_kj: self.hybrid.used_kj,
            kers_is_charging: self.hybrid.charging,
            sus_damage: self.suspension_damage,
            tyre_flat_spot: r.flat_spot,
            water: r.water,
            tyre_thermal_states: thermal,
            disc_temps: r.disc_temps,
            status_bytes: self.lights as u32 | if self.drs { 4 } else { 0 },
            performance_meter: self.performance_meter.0,
            performance_meter_speed_diff_ms: self.performance_meter.1,
            ..rustyac_physics::car_physics_state::CarPhysicsState::at_origin()
        }
    }

    /// The car as it is after the last step.
    pub fn capture(sim: &GameSim, drive_seconds: f64) -> CarView {
        CarView::capture_car(sim, None, drive_seconds)
    }

    /// Every car of the session after the last step, the player's first.
    pub fn capture_world(sim: &GameSim, drive_seconds: f64) -> Vec<CarView> {
        std::iter::once(CarView::capture(sim, drive_seconds))
            .chain((0..sim.opponents.len()).map(|index| CarView::capture_opponent(sim, index, drive_seconds)))
            .chain((0..sim.remote_count()).map(|index| CarView::capture_remote(sim, index)))
            .collect()
    }

    /// Another player's car (`sim.online`'s remote car `index`) as its
    /// `NetCarStateProvider` makes it up after the last step: the `CarPhysicsState` the
    /// game's avatar and sound of a remote car read.
    pub fn capture_remote(sim: &GameSim, index: usize) -> CarView {
        let Some((remote, provider)) = sim.online.as_ref().and_then(|session| Some((session.remote.get(index)?, session.net.cars.get(index)?))) else {
            return CarView { disconnected: true, ..CarView::default() };
        };
        let state = &provider.state;
        let own = &sim.car.car;
        let net_status = provider.net_states[0].status_bytes;
        let mut view = CarView {
            steps: sim.steps,
            sim_seconds: sim.sim_seconds(),
            body: state.world_matrix.m,
            new_sessions: sim.new_sessions,
            speed_ms: state.speed,
            speed_kmh: state.speed * 3.6,
            rpm: state.engine_rpm,
            rpm_limit: if state.limiter_rpm > 0 { state.limiter_rpm as f32 } else { 10_000.0 },
            gear: state.gear,
            gas: state.gas,
            brake: state.brake,
            steer_deg: state.steer,
            lights: state.status_bytes & 1 != 0,
            drs: net_status & rustyac_net::net_car::status::DRS != 0,
            acc_g: state.acc_g,
            damage: remote.damage,
            // (… and 20 l of fuel)
            fuel: 20.0,
            air: own.env.ambient_temperature,
            air_density: own.air_density,
            road: own.env.road_temperature,
            grip: own.env.dynamic_grip_level,
            disconnected: provider.is_disconnected,
            ..CarView::default()
        };
        for wheel in 0..4 {
            view.hubs[wheel] = state.suspension_matrix[wheel].m;
            view.tyre_matrix[wheel] = state.tyre_matrix[wheel].m;
            view.wheels[wheel] = state.tyre_matrix[wheel].m;
            view.wheel_angular_speed[wheel] = state.wheel_angular_speed[wheel];
            view.tyre_radius[wheel] = provider.geometry.tyre_radius[wheel];
            // NetCarStateProvider::setCarAvatar: every tyre of a remote car is 0.2 m wide
            view.tyre_width[wheel] = 0.2;
            view.rim_radius[wheel] = provider.geometry.tyre_radius[wheel] * 0.5;
            // (NetCarStateProvider::getPhysicsState hands every tyre out with a load of 100 N)
            view.wheel_load[wheel] = 100.0;
            view.wheel_slip[wheel] = state.nd_slip[wheel];
            let r = &mut view.render;
            r.slip_ratio[wheel] = state.slip_ratio[wheel];
            r.slip_angle[wheel] = state.slip_angle[wheel];
            r.loaded_radius[wheel] = provider.geometry.tyre_radius[wheel];
            r.contact_point[wheel] = state.tyre_contact_point[wheel];
            r.contact_normal[wheel] = state.tyre_contact_normal[wheel];
            r.inflation[wheel] = 1.0;
            if let Some(surface) = &state.tyre_surface_def[wheel] {
                r.surface_grip_mod[wheel] = surface.grip_mod;
                r.surface_dirt_additive_k[wheel] = surface.dirt_additive_k;
                r.surface_is_valid_track[wheel] = surface.is_valid_track;
                r.surface_is_pitlane[wheel] = surface.is_pitlane;
            }
        }
        view.render.velocity = state.velocity;
        view.render.ground_plane = state.ground_plane;
        view.render.drivetrain_speed = state.drivetrain_speed;
        view.render.time_stamp = state.time_stamp;
        view.lap.on_track = sim.track.is_some();
        view.lap.position = state.normalized_spline_position;
        view
    }

    /// Another car of the session (`sim.opponents[index]`) as it is after the last step: what
    /// its avatar and its sound read. What belongs to the player alone (the lap list, the fuel
    /// use, the notes, the loose objects) stays empty.
    pub fn capture_opponent(sim: &GameSim, index: usize, drive_seconds: f64) -> CarView {
        CarView::capture_car(sim, Some(index), drive_seconds)
    }

    fn capture_car(sim: &GameSim, opponent: Option<usize>, drive_seconds: f64) -> CarView {
        let player = opponent.is_none();
        let (car, device) = match opponent {
            None => (&sim.car.car, &sim.car.device),
            Some(index) => (&sim.opponents[index].car.car, &sim.opponents[index].car.device),
        };
        // TrackObject::update: the picture's node takes the body's matrix
        let mut moved_objects = [(0u16, IDENTITY); MOVED_OBJECTS];
        let mut moved_object_count = 0u8;
        let world = sim.world_core();
        for (number, object) in world.track_objects.iter().enumerate().filter(|_| player) {
            let matrix = world.get_world_matrix(object.body).m;
            let home = object.org_matrix.m[3];
            let away = (0..3).any(|k| matrix[3][k] != home[k]);
            if (away || world.is_enabled(object.body)) && (moved_object_count as usize) < MOVED_OBJECTS {
                moved_objects[moved_object_count as usize] = (number as u16, matrix);
                moved_object_count += 1;
            }
        }
        let mut view = CarView {
            steps: sim.steps,
            sim_seconds: sim.sim_seconds(),
            drive_seconds,
            body: car.core.get_world_matrix(car.body).m,
            moved_objects,
            moved_object_count,
            other_cars: {
                let mut others = [IDENTITY; OTHER_CARS];
                if player {
                    for (slot, other) in others.iter_mut().zip(&sim.opponents) {
                        *slot = other.car.car.core.get_world_matrix(other.car.car.body).m;
                    }
                }
                others
            },
            other_car_count: if player { sim.opponents.len().min(OTHER_CARS) as u8 } else { 0 },
            new_sessions: sim.new_sessions,
            performance_meter: car.performance_meter_state(),
            tyre_debug: std::array::from_fn(|k| {
                car.tyres.get(k).map_or(Default::default(), |t| rustyac_gui::app_tyre_debug::TyreDebugRow {
                    virtual_km: t.status.virtual_km,
                    wear_value: t.model_data.wear_curve.get_value(t.status.virtual_km as f32),
                    core_temp: t.thermal_model.core_temp,
                    active: t.thermal_model.is_active,
                })
            }),
            compound_index: std::array::from_fn(|k| car.tyres.get(k).map_or(0, |t| t.current_compound_index)),
            engineer: {
                let s = |k: usize| car.suspensions.get(k);
                let h = &car.heave_springs[0];
                rustyac_gui::app_car_engineer::CarEngineer {
                    k: [s(0).map_or(0.0, |s| s.get_k()), s(2).map_or(0.0, |s| s.get_k())],
                    z: [s(0).map_or(0.0, |s| s.get_base_position().z), s(2).map_or(0.0, |s| s.get_base_position().z)],
                    mass: car.core.get_mass(car.body),
                    dampers: [0, 2].map(|k| s(k).map_or([0.0; 2], |s| [s.damper().rebound_slow, s.damper().bump_slow])),
                    heave: h.is_present.then_some([h.k, h.damper.rebound_slow, h.damper.bump_slow]),
                    ..Default::default()
                }
            },
            wing_states: {
                let mut out = [rustyac_physics::car::aero::WingState::default(); MAX_WINGS];
                for (slot, wing) in out.iter_mut().zip(car.aero.as_ref().map_or(&[][..], |a| &a.base().wings[..])) {
                    *slot = wing.status;
                }
                out
            },
            wing_state_count: car.aero.as_ref().map_or(0, |a| a.base().wings.len().min(MAX_WINGS)) as u8,
            sus_lines: sus_lines(car).0,
            sus_line_count: sus_lines(car).1,
            pit_position: [car.pit_position.m[3][0], car.pit_position.m[3][1], car.pit_position.m[3][2]],
            packer_range: [0, 1, 2, 3].map(|k| car.suspensions.get(k).map_or(0.0, |s| s.get_packer_range())),
            speed_kmh: car.speed * 3.6,
            speed_ms: car.speed,
            gas: car.controls.gas,
            brake: car.controls.brake,
            clutch: car.controls.clutch,
            steer: car.controls.steer,
            hand_brake: car.controls.hand_brake,
            steer_deg: car.controls.steer * car.steer_lock,
            drs: car.controls.drs,
            lights: car.lights_on,
            auto_shifter: car.auto_shifter.is_active,
            acc_g: [car.acc_g.x, car.acc_g.y, car.acc_g.z],
            ff: car.last_ff,
            fuel: car.fuel as f32,
            device: device.source.device_id(),
            damage: car.damage_zone_level,
            contacts: sim.world_core().contact_joints().len() as u32,
            engine_life: 1000.0,
            air: car.env.ambient_temperature,
            air_density: car.air_density,
            road: car.env.road_temperature,
            grip: car.env.dynamic_grip_level,
            wind_kmh: car.env.wind_speed * 3.6,
            wind_deg: car.env.wind_direction_deg,
            compound: SurfaceName::new(car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index as usize)).map_or("", |c| c.name.as_str())),
            setup: SurfaceName::new(&car.setup_name),
            compound_short: SurfaceName::new(car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index as usize)).map_or("", |c| c.short_name.as_str())),
            brake_bias: car.brake_system.as_deref().map_or(-1.0, |brakes| brakes.get_front_bias()),
            note: if player { sim.aid_note } else { AidNote::default() },
            ..CarView::default()
        };
        for index in 0..4.min(car.tyres.len()) {
            let tyre = &car.tyres[index];
            let hub: Mat44f = car.suspensions[index].get_hub_world_matrix(&car.core);
            view.hubs[index] = hub.m;
            view.wheels[index] = xm_matrix_multiply(&tyre.local_wheel_rotation, &hub).m;
            // Car::getPhysicsState 0x140270d70
            let mut m = xm_matrix_multiply(&tyre.local_wheel_rotation, &tyre.world_rotation);
            m.m[3][0] = hub.m[3][0];
            m.m[3][1] = hub.m[3][1];
            m.m[3][2] = hub.m[3][2];
            view.tyre_matrix[index] = m.m;
            let asleep = car.sleeping_frames > car.frames_to_sleep;
            view.wheel_angular_speed[index] = if !tyre.status.is_locked && !asleep { tyre.status.angular_velocity } else { 0.0 };
            view.tyre_radius[index] = tyre.data.radius;
            view.tyre_width[index] = tyre.data.width;
            view.rim_radius[index] = tyre.data.rim_radius;
            view.wheel_load[index] = tyre.status.load;
            view.wheel_slip[index] = tyre.status.nd_slip;
            view.suspension_damage[index] = car.suspensions[index].get_damage();
            // Car::getPhysicsState 0x140270d70, the tyre's part
            let r = &mut view.render;
            let s = &tyre.status;
            r.slip_angle[index] = s.slip_angle_rad * f32::from_bits(0x4265_2ee1);
            r.slip_ratio[index] = s.slip_ratio;
            r.tyre_slip[index] = s.slip_factor;
            r.dy[index] = s.dy;
            r.mz[index] = s.mz;
            r.dirty_level[index] = s.dirty_level;
            r.camber_rad[index] = if index & 1 == 0 { s.camber_rad } else { -s.camber_rad };
            r.loaded_radius[index] = tyre.data.radius - s.depth;
            r.suspension_travel[index] = car.suspensions[index].get_status().travel;
            r.contact_point[index] = [tyre.contact_point.x, tyre.contact_point.y, tyre.contact_point.z];
            r.contact_normal[index] = [tyre.contact_normal.x, tyre.contact_normal.y, tyre.contact_normal.z];
            r.virtual_km[index] = s.virtual_km as f32;
            r.inflation[index] = s.inflation;
            r.flat_spot[index] = s.flat_spot as f32;
            r.core_temps[index] = tyre.thermal_model.core_temp;
            // Car::getTyreThermalState 0x1402727b0
            let model = &tyre.thermal_model;
            if model.stripes >= 3 && model.elements >= 12 {
                for s in 0..3i32 {
                    let stripe = if index & 1 == 0 { 2 - s } else { s };
                    for e in 0..12i32 {
                        let f = s as usize + 3 * e as usize;
                        r.tyre_temps[index][f / 12][f % 12] = model.patches[model.patch_index(stripe, e)].t;
                    }
                }
            }
            let ideal = tyre.model_data.ideal_pressure;
            r.ideal_pressure[index] = if ideal > -2147483649.0 && ideal < 2147483648.0 { ideal as i32 } else { i32::MIN };
            r.dynamic_pressure[index] = s.pressure_dynamic;
            r.static_pressure[index] = s.pressure_static;
            r.thermal_mult[index] = model.thermal_mult_d;
            r.is_hot[index] = tyre.data.blister_threshold <= model.practical_temp;
            // Car::getPhysicsState 0x140270d70: the wear
            let max = tyre.model_data.max_wear_km;
            if max == 0.0 {
                r.wear[index] = -1.0;
            } else {
                let w = s.virtual_km / max as f64;
                r.wear[index] = if 1.0 <= w { 1.0 } else { w as f32 };
                r.wear_mult[index] = s.wear_mult;
            }
            if let Some(surface) = &tyre.surface_def {
                r.surface_grip_mod[index] = surface.grip_mod;
                r.surface_dirt_additive_k[index] = surface.dirt_additive_k;
                r.surface_is_valid_track[index] = surface.is_valid_track;
                r.surface_is_pitlane[index] = surface.is_pitlane;
            }
            if let Some(brakes) = car.brake_system.as_deref() {
                r.disc_temps[index] = brakes.disc_temperatures()[index];
            }
        }
        {
            let r = &mut view.render;
            let v3 = |v: rustyac_physics::vecmath::Vec3f| [v.x, v.y, v.z];
            r.velocity = v3(car.core.get_velocity(car.body));
            r.local_velocity = v3(car.core.get_local_velocity(car.body));
            r.angular_velocity = v3(car.core.get_angular_velocity(car.body));
            r.local_angular_velocity = v3(car.core.get_local_angular_velocity(car.body));
            if car.tyres.len() >= 4 {
                let t = &car.tyres;
                r.ground_plane = rustyac_physics::car::aero::plane4f(&t[0].unmodified_contact_point, &t[1].unmodified_contact_point, &t[2].unmodified_contact_point);
                let mut sum = 0.0f32;
                for tyre in t.iter() {
                    sum += tyre.contact_point.y;
                }
                r.cg_height = car.core.get_position(car.body).y - sum * 0.25;
            }
            r.water = car.water.t;
            r.time_stamp = sim.sim_seconds() * 1000.0;
        }
        if let Some(drivetrain) = &car.drivetrain {
            view.rpm = drivetrain.get_engine_rpm();
            view.gear = drivetrain.base().current_gear;
            view.rpm_limit = drivetrain.engine().get_limiter_rpm() as f32;
            view.engine_life = drivetrain.engine().base().life_left as f32;
            let engine = drivetrain.engine();
            view.render.turbo_boost = engine.base().status.turbo_boost;
            view.render.turbo_bov = engine.base().bov;
            view.render.drivetrain_speed = (drivetrain.base().drive.velocity as f32).abs();
            view.render.is_gear_grinding = drivetrain.base().is_gear_grinding;
            view.render.is_engine_limiter_on = engine.is_limiter_on();
        }
        if let Some(track) = &sim.track {
            for index in 0..4.min(car.tyres.len()) {
                if let Some(surface) = &car.tyres[index].surface_def {
                    view.surfaces[index] = SurfaceName::new(track.surface_key(surface));
                }
            }
            let tp = &car.transponder;
            // (the lap list is the player's; another car shows its own running lap only)
            let empty = rustyac_physics::track::LapDb::default();
            let db = if player { &sim.lap_db } else { &empty };
            let mut sector_ms = [0u32; 4];
            for (slot, split) in sector_ms.iter_mut().zip(&db.current_splits) {
                *slot = *split;
            }
            let last = db.last_lap();
            view.lap = LapView {
                on_track: !track.time_lines.is_empty(),
                current_ms: tp.t,
                last_ms: last.time,
                best_ms: db.best_lap.time,
                last_valid: last.is_valid,
                laps: db.laps.len() as u32,
                sector: db.current_splits.len() as u32,
                sector_count: db.sector_count as u32,
                sector_ms,
                valid: tp.cuts == 0,
                cuts: tp.cuts,
                tyres_out: car.lap_invalidator.current_tyres_out,
                position: car.spline_locator_data.npos,
                in_pit_lane: car.is_in_pit_lane(),
                pit_stop: if player { sim.pit_stop_view() } else { None },
            };
            if !player {
                // another car: what its own transponder counted
                view.lap.laps = tp.lap_count;
                view.lap.last_ms = tp.last_lap;
                view.lap.best_ms = tp.best_lap;
            }
        }
        view.hybrid = HybridView {
            engine_brake: car.cockpit.engine_brake,
            engine_brake_count: car.engine_brake_settings(),
            ..HybridView::default()
        };
        if let Some(kers) = &car.kers {
            view.hybrid = HybridView { has_kers: true, charge: kers.charge, input: kers.input, used_kj: kers.current_j * 0.001, max_kj: kers.max_j * 0.001, ..view.hybrid };
        }
        if let Some(ers) = &car.ers {
            view.hybrid = HybridView {
                has_ers: true,
                charge: ers.charge as f32,
                input: ers.input,
                used_kj: ers.current_j * 0.001,
                max_kj: ers.max_j * 0.001,
                charging: ers.is_charging,
                power_index: car.cockpit.ers_power_index,
                power_count: ers.power_controllers.len() as i32,
                power_name: SurfaceName::new(ers.power_controllers.get(car.cockpit.ers_power_index as usize).map_or("", |c| c.name.as_str())),
                recovery: car.cockpit.ers_recovery,
                heat_charging: car.cockpit.ers_heat_charging,
                offers: [ers.cockpit_delivery_profile, ers.cockpit_recovery, ers.cockpit_mgu_h_mode],
                ..view.hybrid
            };
        }
        view.max_fuel = car.max_fuel;
        view.user_ff_gain = car.user_ff_gain;
        view.ff_mult = car.ff_mult;
        view.steer_assist = car.steer_assist;
        if player {
            view.km_per_liter = sim.fuel_evaluator.km_per_liter();
            view.fuel_laps = sim.fuel_evaluator.fuel_laps(car.fuel);
        }
        view.dynamic_track = sim.dynamic_track().is_some();
        if let Some(aero) = &car.aero {
            let wings = &aero.base().wings;
            view.wing_count = wings.len().min(8) as u8;
            for (slot, wing) in view.wing_angles.iter_mut().zip(wings) {
                *slot = wing.get_current_modified_angle();
            }
        }
        if let Some(kers) = &car.kers {
            view.kers_max_j = kers.max_j;
        }
        if let Some(ers) = &car.ers {
            view.ers_max_j = ers.max_j;
        }
        if let Some(drivetrain) = &car.drivetrain {
            view.total_torque = drivetrain.base().total_torque;
            view.drive_ratio = drivetrain.base().ratio;
        }
        if let Some(aids) = &car.aids {
            let aids = aids.base();
            view.tc_in_action = aids.traction_control.is_in_action;
            view.tc_mode = aids.traction_control.get_current_mode();
            view.tc_present = aids.traction_control.is_present;
            view.abs_in_action = aids.abs.is_present && aids.abs.is_active && Abs::is_in_action(car);
            view.abs_mode = aids.abs.get_current_mode();
            view.abs_present = aids.abs.is_present;
        }
        view
    }

    /// `TC 2/3`, `TC off` or `TC not fitted`.
    pub fn tc_text(&self) -> String {
        format!("TC {}", aid_level_text(self.tc_present, self.tc_mode))
    }

    /// `ABS 1/4`, `ABS off` or `ABS not fitted`.
    pub fn abs_text(&self) -> String {
        format!("ABS {}", aid_level_text(self.abs_present, self.abs_mode))
    }

    /// `Bias 58.0 %` (`Bias -` for a car without brakes).
    pub fn bias_text(&self) -> String {
        if self.brake_bias < 0.0 {
            "Bias -".to_string()
        } else {
            format!("Bias {}", bias_text(self.brake_bias))
        }
    }

    /// The note about the last change of an aid or of the bias, while it is fresh (2.5 s).
    pub fn aid_note(&self) -> Option<&str> {
        self.note.shown_at(self.steps)
    }

    /// The car between two steps, for a frame that falls between them: poses are blended, the
    /// HUD's numbers are the newer step's.
    pub fn between(a: &CarView, b: &CarView, t: f32) -> CarView {
        let t = t.clamp(0.0, 1.0);
        // a teleport (reset) is not a motion to blend through
        let jump = (0..3).map(|k| (a.body[3][k] - b.body[3][k]).abs()).fold(0.0, f32::max) > 2.0;
        if jump {
            return *b;
        }
        let mut out = *b;
        out.body = lerp_pose(&a.body, &b.body, t);
        for k in 0..(b.other_car_count as usize).min(a.other_car_count as usize) {
            out.other_cars[k] = lerp_pose(&a.other_cars[k], &b.other_cars[k], t);
        }
        for k in 0..4 {
            out.wheels[k] = lerp_pose(&a.wheels[k], &b.wheels[k], t);
            out.tyre_matrix[k] = lerp_pose(&a.tyre_matrix[k], &b.tyre_matrix[k], t);
            out.hubs[k] = lerp_pose(&a.hubs[k], &b.hubs[k], t);
        }
        out.speed_kmh = lerp(a.speed_kmh, b.speed_kmh, t);
        out.rpm = lerp(a.rpm, b.rpm, t);
        out
    }
}

/// What the Setup debug app reads of the focused car's `Car` (`focus`: 0 the player's, then the
/// other cars in their order).
pub fn setup_debug(sim: &GameSim, focus: usize) -> rustyac_gui::app_setup_debug::SetupDebugCar {
    // shortcut: an online remote car has no Car here, so the player's is shown; port NetCar's setup if wanted
    let car = sim.cars().nth(focus).unwrap_or(&sim.car.car);
    let d = car.drivetrain.as_ref().map(|d| d.base());
    let gear_ratios: Vec<f64> = d.map_or(Vec::new(), |d| d.gears.iter().map(|g| g.ratio).collect());
    rustyac_gui::app_setup_debug::SetupDebugCar {
        pressure_static: std::array::from_fn(|k| car.tyres.get(k).map_or(0.0, |t| t.status.pressure_static)),
        wing_angles: car.aero.as_ref().map_or(Vec::new(), |a| a.base().wings.iter().map(|w| w.status.angle).collect()),
        rod_lengths: std::array::from_fn(|k| car.suspensions.get(k).map_or(0.0, |s| s.base().rod_length)),
        // physicsInfo.maxGear: the forward gears (R and N lead the list)
        max_gear: gear_ratios.len().saturating_sub(2) as i32,
        gear_ratios,
        final_ratio: d.map_or(0.0, |d| d.final_ratio),
        tyre_compound: car.tyres.first().and_then(|t| t.compound_defs.get(t.current_compound_index as usize)).map_or(String::new(), |c| c.name.clone()),
    }
}

/// What `RaceEngineer::getDynamicIndex` reads of the player's car (the Car physics app): the
/// base positions' z of suspensions 0 and 2, the body's mass and inertia y (`getLocalInertia`:
/// `dMass::I[5]`).
/// Task 43c: what FormWings reads of the player's wings once (`physicsInfo.wingData`); the
/// states come with each frame's [`CarView::wings`].
pub fn wing_rows(sim: &GameSim) -> Vec<rustyac_gui::app_wings::WingRow> {
    let car = &sim.car.car;
    let wings = car.aero.as_ref().map_or(&[][..], |a| &a.base().wings[..]);
    wings
        .iter()
        .take(MAX_WINGS)
        .map(|w| rustyac_gui::app_wings::WingRow { name: w.data.name.clone(), chord: w.data.chord, span: w.data.span, position: [w.data.position.x, w.data.position.y, w.data.position.z], is_vertical: w.data.is_vertical, state: w.status })
        .collect()
}

/// `CarAvatar::initPhysics` 0x1400d7660's suspension facts and the car.ini fuel tank
/// `ACSuspension`'s constructor reads (missing: zeros, as the game's `getFloat3`).
type SusLines = ([[rustyac_physics::car::suspension::DebugLine; rustyac_physics::engine::record::DEBUG_LINES]; 4], [u8; 4]);

/// Every suspension's `getDebugLines` with the body's and its hub's matrices (cut at the first
/// `DEBUG_LINES`).
fn sus_lines(car: &rustyac_physics::car::RollingChassis) -> SusLines {
    let body = car.core.get_world_matrix(car.body);
    let mut out = SusLines::default();
    for (w, s) in car.suspensions.iter().take(4).enumerate() {
        let lines = s.get_debug_lines(&body, &s.get_hub_world_matrix(&car.core));
        out.1[w] = lines.len().min(rustyac_physics::engine::record::DEBUG_LINES) as u8;
        for (slot, line) in out.0[w].iter_mut().zip(lines) {
            *slot = line;
        }
    }
    out
}

pub fn suspension_facts(sim: &GameSim) -> rustyac_gui::app_suspension::SuspensionFacts {
    use rustyac_physics::car::suspension::kpi;
    let car = &sim.car.car;
    let v = |p: rustyac_physics::vecmath::Vec3f| [p.x, p.y, p.z];
    let (kpi_rad, scrub) = kpi(car.suspensions[0].upright(), car.tyres[0].data.radius);
    let ride = rustyac_physics::car::PhysicsPageWriter::new(car, &sim.data_path).map(|w| w.ride_pickup_point.map(v)).unwrap_or_default();
    let tank = rustyac_physics::data::ini::IniReader::load(&sim.data_path.join("car.ini")).ok().and_then(|ini| ini.get_float3("FUELTANK", "POSITION").ok()).unwrap_or_default();
    rustyac_gui::app_suspension::SuspensionFacts {
        caster: car.suspensions[0].caster_rad() * 57.29578,
        kpi_rad,
        scrub,
        sus_base_pos: std::array::from_fn(|i| v(car.suspensions[i].get_base_position())),
        ride_pickup_point: ride,
        tyre_width: std::array::from_fn(|i| car.tyres[i].data.width),
        tyre_radius: std::array::from_fn(|i| car.tyres[i].data.radius),
        fuel_tank_position: tank,
    }
}

pub fn engineer_masses(sim: &GameSim) -> rustyac_gui::app_car_engineer::CarEngineer {
    // CarAvatar::initPhysics 0x1400d7660: bodyMass = totalMass = the body's, then each hub's added
    let car = &sim.car.car;
    let body_mass = car.core.get_mass(car.body);
    let unsprung: [f32; 4] = std::array::from_fn(|k| car.suspensions.get(k).map_or(0.0, |s| s.get_mass(&car.core)));
    let mut total_mass = body_mass;
    for u in unsprung {
        total_mass += u;
    }
    rustyac_gui::app_car_engineer::CarEngineer { total_mass, body_mass, unsprung, ..Default::default() }
}

pub fn car_physics(sim: &GameSim) -> rustyac_gui::app_car_physics::CarPhysicsExtra {
    let car = &sim.car.car;
    let z = |k: usize| car.suspensions.get(k).map_or(0.0, |s| s.get_base_position().z);
    rustyac_gui::app_car_physics::CarPhysicsExtra { base_z: [z(0), z(2)], mass: car.core.get_mass(car.body), iyy: car.core.world.body(car.body.id).mass.i[5], ..Default::default() }
}

/// What the Telemetry app (`FormTelemetry::initChannels`) reads of the player's car once.
pub fn telemetry_car(sim: &GameSim) -> rustyac_gui::app_telemetry::TelemetryCar {
    let car = &sim.car.car;
    let d = car.drivetrain.as_ref();
    rustyac_gui::app_telemetry::TelemetryCar {
        limiter_rpm: d.map_or(0, |d| d.engine().get_limiter_rpm()),
        clutch_max_torque: d.map_or(0.0, |d| d.base().clutch_max_torque),
        awd2: d.is_some_and(|d| d.base().traction_type == rustyac_physics::car::drivetrain::TractionType::AwdNew),
        arb_ready: [car.antiroll_bars[0].ctrl.ready, car.antiroll_bars[1].ctrl.ready],
    }
}

/// The Telemetry app's channels (`names`, in its order) of the player's car after the last
/// step, before their scale. A channel rustyAC has no source for reads 0 (see
/// docs/port/apps_car_data.md).
pub fn telemetry_values(sim: &GameSim, names: &[String]) -> Vec<f32> {
    let car = &sim.car.car;
    let d = car.drivetrain.as_ref();
    names
        .iter()
        .map(|name| {
            let (base, wheel) = match name.rsplit_once('_') {
                Some((b, "LF")) => (b, 0),
                Some((b, "RF")) => (b, 1),
                Some((b, "LR")) => (b, 2),
                Some((b, "RR")) => (b, 3),
                _ => (name.as_str(), 0),
            };
            let tyre = car.tyres.get(wheel);
            let s = tyre.map(|t| &t.status);
            let sus = car.suspensions.get(wheel).map(|s| s.get_status());
            match base {
                "AWD2_LOCK" => d.map_or(0.0, |d| d.base().awd2.current_lock_torque),
                "ENGINE_RPM" => d.map_or(0.0, |d| d.get_engine_rpm()),
                "CLUTCH_TORQUE" => d.map_or(0.0, |d| d.base().current_clutch_torque),
                "DRV_ROOT_VEL" => d.map_or(0.0, |d| d.base().root_velocity as f32),
                "YAW_RATE" => car.core.get_local_angular_velocity(car.body).y,
                "LOAD" => s.map_or(0.0, |s| s.load),
                "AVEL" => s.map_or(0.0, |s| s.angular_velocity),
                "SA" => s.map_or(0.0, |s| s.slip_angle_rad),
                "SR" => s.map_or(0.0, |s| s.slip_ratio),
                "MZ" => s.map_or(0.0, |s| s.mz),
                "DY" => s.map_or(0.0, |s| s.dy),
                "FY" => s.map_or(0.0, |s| s.fy),
                "DX" => s.map_or(0.0, |s| s.dx),
                "CAMBER_DEG" => s.map_or(0.0, |s| s.camber_rad),
                "NDSLIP" => s.map_or(0.0, |s| s.nd_slip),
                "SUSTRVL" => sus.map_or(0.0, |s| s.travel),
                "DAMP_VEL" => sus.map_or(0.0, |s| s.damper_speed_ms),
                "CORE_TEMP" => tyre.map_or(0.0, |t| t.thermal_model.core_temp),
                // (the lambda: the 36 patches' sum times 1/36)
                "AVG_SURF_TEMP" => tyre.map_or(0.0, |t| t.thermal_model.patches.iter().take(36).fold(0.0f32, |sum, p| sum + p.t) * 0.027777778),
                "PRACT_TEMP" => tyre.map_or(0.0, |t| t.thermal_model.get_practical_temp(0.0)),
                "TEMP_MULT" => tyre.map_or(0.0, |t| t.thermal_model.thermal_mult_d),
                "BRAKE_PAD_INPUT" => tyre.map_or(0.0, |t| t.inputs.brake_torque),
                "ERS_KINETIC_REC" => car.ers.as_ref().map_or(0.0, |e| e.status_kinetic_recovery),
                "ERS_HEAT_REC" => car.ers.as_ref().map_or(0.0, |e| e.status_heat_recovery),
                "ERS_CHARGE" => car.ers.as_ref().map_or(0.0, |e| e.charge as f32),
                "AVG_REAR_TRAVEL" => (car.suspensions.get(2).map_or(0.0, |s| s.get_status().travel) + car.suspensions.get(3).map_or(0.0, |s| s.get_status().travel)) * 0.5,
                "BRAKE_DISC_T" => car.brake_system.as_deref().map_or(0.0, |b| b.disc_temperatures()[wheel]),
                "FF_PURE" => car.last_ff,
                "WATER_TEMP" => car.water.t,
                "ACC_LAT" => car.acc_g.x,
                "ACC_LON" => car.acc_g.z,
                "ARB_FRONT" => car.antiroll_bars[0].k,
                "ARB_REAR" => car.antiroll_bars[1].k,
                // (the car's controls provider as an AIDriver: the player's car while AI drives it)
                "AI_PUSH" => sim.car.device.ai.as_ref().map_or(0.0, |ai| ai.get_lateral_push(car) * 100.0),
                "AI_LIVE_OFFSET" => sim.car.device.ai.as_ref().map_or(0.0, |ai| ai.live_offset),
                "AI_DIST_CORNER" => sim.car.device.ai.as_ref().map_or(0.0, |ai| ai.offset_dist_from_corner),
                "ERS_TORQUE" => car.ers.as_ref().zip(d).map_or(0.0, |(e, d)| e.get_output_torque(d.base())),
                "OVERSTEER_FACTOR" => rustyac_physics::car::dynamic_controller::DynamicController::get_oversteer_factor(car),
                "STEER_DEG" => car.get_wheel_steer_angle_deg(wheel),
                // (Car::getPhysicsState's copies, which the main thread reads)
                "FF_FINAL" => car.last_ff_final(),
                // shortcut: the speed of this step stands for the previous snapshot's, exact but at the 2 m/s edge
                "RIDE_HEIGHT_F" => car.ride_heights(car.speed)[0],
                "RIDE_HEIGHT_R" => car.ride_heights(car.speed)[1],
                // SKID_VOL SKID_PITCH: the sound's, on the display thread (GameGui::telemetry_skids)
                _ => 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_y(angle: f32, at: [f32; 3]) -> Mat {
        let (s, c) = angle.sin_cos();
        [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [at[0], at[1], at[2], 1.0]]
    }

    #[test]
    fn a_blended_pose_is_square_and_half_way() {
        let a = rot_y(0.10, [1.0, 0.5, 2.0]);
        let b = rot_y(0.14, [1.3, 0.5, 2.6]);
        let m = lerp_pose(&a, &b, 0.5);
        let expected = rot_y(0.12, [1.15, 0.5, 2.3]);
        for r in 0..4 {
            for c in 0..4 {
                assert!((m[r][c] - expected[r][c]).abs() < 1e-4, "m[{r}][{c}] = {} / {}", m[r][c], expected[r][c]);
            }
        }
        // the ends are the ends
        assert_eq!(lerp_pose(&a, &b, 0.0)[3], a[3]);
        let end = lerp_pose(&a, &b, 1.0);
        for r in 0..3 {
            for c in 0..3 {
                assert!((end[r][c] - b[r][c]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn the_aids_and_the_bias_as_the_displays_word_them() {
        let mut view = CarView { tc_present: true, tc_mode: (2, 3), abs_present: true, abs_mode: (0, 0), brake_bias: 0.58, ..CarView::default() };
        assert_eq!((view.tc_text(), view.abs_text(), view.bias_text()), ("TC 2/3".to_string(), "ABS off".to_string(), "Bias 58.0 %".to_string()));
        // a single level reads 1/1; an aid the car does not have is "not fitted", not "off"
        view.tc_mode = (1, 1);
        view.abs_present = false;
        view.brake_bias = 0.725;
        assert_eq!((view.tc_text(), view.abs_text(), view.bias_text()), ("TC 1/1".to_string(), "ABS not fitted".to_string(), "Bias 72.5 %".to_string()));
        view.brake_bias = -1.0;
        assert_eq!(view.bias_text(), "Bias -");
        // a note is shown for 2.5 s of steps after it was made, and no longer
        assert_eq!(view.aid_note(), None);
        view.note = AidNote::new("TC 2/3", 1000);
        view.steps = 1000;
        assert_eq!(view.aid_note(), Some("TC 2/3"));
        view.steps = 1000 + AidNote::STEPS - 1;
        assert_eq!(view.aid_note(), Some("TC 2/3"));
        view.steps = 1000 + AidNote::STEPS;
        assert_eq!(view.aid_note(), None);
        // the longest note fits
        assert_eq!(AidNote::new("Brake bias 100.0 % (no change)", 0).as_str(), "Brake bias 100.0 % (no change)");
    }

    #[test]
    fn a_teleport_is_not_blended() {
        let a = CarView { body: rot_y(0.0, [0.0, 0.3, 500.0]), ..CarView::default() };
        let b = CarView { body: rot_y(0.0, [0.0, 0.3, 0.0]), ..CarView::default() };
        assert_eq!(CarView::between(&a, &b, 0.5).body, b.body);
    }
}
