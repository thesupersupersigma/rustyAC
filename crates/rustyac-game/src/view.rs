// SPDX-License-Identifier: GPL-3.0-or-later

//! What the display needs of the car after a step: where its parts are and what the HUD
//! shows. Read out of the physics car, never written back.

use rustyac_physics::car::Abs;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::sim::GameSim;

/// A 4x4 matrix as the physics has it: row vectors, the translation in the fourth row
/// (Direct3D's convention). Row 0 is the part's left (+x), row 1 its up, row 2 its forward.
pub type Mat = [[f32; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// How many loose objects the picture follows at a time.
pub const MOVED_OBJECTS: usize = 8;

/// The car after one physics step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarView {
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
    /// The hybrid system and the engine brake, for a car that has them.
    pub hybrid: HybridView,
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
}

impl Default for CarView {
    fn default() -> CarView {
        CarView {
            moved_objects: [(0, IDENTITY); MOVED_OBJECTS],
            moved_object_count: 0,
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
            hybrid: HybridView::default(),
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
    pub fn physics_state(&self) -> rustyac_render::car::CarPhysicsState {
        let m = |m: &Mat| Mat44f { m: *m };
        rustyac_render::car::CarPhysicsState {
            world_matrix: m(&self.body),
            suspension_matrix: [m(&self.hubs[0]), m(&self.hubs[1]), m(&self.hubs[2]), m(&self.hubs[3])],
            tyre_matrix: [m(&self.tyre_matrix[0]), m(&self.tyre_matrix[1]), m(&self.tyre_matrix[2]), m(&self.tyre_matrix[3])],
            wheel_angular_speed: self.wheel_angular_speed,
            steer: self.steer_deg,
        }
    }

    /// The car as it is after the last step.
    pub fn capture(sim: &GameSim, drive_seconds: f64) -> CarView {
        let car = &sim.car.car;
        // TrackObject::update: the picture's node takes the body's matrix
        let mut moved_objects = [(0u16, IDENTITY); MOVED_OBJECTS];
        let mut moved_object_count = 0u8;
        for (number, object) in car.core.track_objects.iter().enumerate() {
            let matrix = car.core.get_world_matrix(object.body).m;
            let home = object.org_matrix.m[3];
            let away = (0..3).any(|k| matrix[3][k] != home[k]);
            if (away || car.core.is_enabled(object.body)) && (moved_object_count as usize) < MOVED_OBJECTS {
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
            speed_kmh: car.speed * 3.6,
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
            device: sim.car.device.source.device_id(),
            damage: car.damage_zone_level,
            contacts: car.core.contact_joints().len() as u32,
            engine_life: 1000.0,
            air: car.env.ambient_temperature,
            air_density: car.air_density,
            road: car.env.road_temperature,
            grip: car.env.dynamic_grip_level,
            wind_kmh: car.env.wind_speed * 3.6,
            wind_deg: car.env.wind_direction_deg,
            compound: SurfaceName::new(car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index as usize)).map_or("", |c| c.name.as_str())),
            setup: SurfaceName::new(&car.setup_name),
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
        }
        if let Some(drivetrain) = &car.drivetrain {
            view.rpm = drivetrain.get_engine_rpm();
            view.gear = drivetrain.base().current_gear;
            view.rpm_limit = drivetrain.engine().get_limiter_rpm() as f32;
            view.engine_life = drivetrain.engine().base().life_left as f32;
        }
        if let Some(track) = &sim.track {
            for index in 0..4.min(car.tyres.len()) {
                if let Some(surface) = &car.tyres[index].surface_def {
                    view.surfaces[index] = SurfaceName::new(track.surface_key(surface));
                }
            }
            let tp = &car.transponder;
            let db = &sim.lap_db;
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
            };
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
    fn a_teleport_is_not_blended() {
        let a = CarView { body: rot_y(0.0, [0.0, 0.3, 500.0]), ..CarView::default() };
        let b = CarView { body: rot_y(0.0, [0.0, 0.3, 0.0]), ..CarView::default() };
        assert_eq!(CarView::between(&a, &b, 0.5).body, b.body);
    }
}
