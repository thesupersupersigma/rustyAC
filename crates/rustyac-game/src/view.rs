//! What the display needs of the car after a step: where its parts are and what the HUD
//! shows. Read out of the physics car, never written back.

use rustyac_physics::car::Abs;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::sim::GameSim;

/// A 4x4 matrix as the physics has it: row vectors, the translation in the fourth row
/// (Direct3D's convention). Row 0 is the part's left (+x), row 1 its up, row 2 its forward.
pub type Mat = [[f32; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

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
    /// Each wheel's world matrix with its spin (`localWheelRotation` x the hub's matrix),
    /// in the order LF, RF, LR, RR.
    pub wheels: [Mat; 4],
    /// Each hub's world matrix (steering and camber, no spin).
    pub hubs: [Mat; 4],
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
}

impl Default for CarView {
    fn default() -> CarView {
        CarView {
            steps: 0,
            sim_seconds: 0.0,
            drive_seconds: 0.0,
            body: IDENTITY,
            wheels: [IDENTITY; 4],
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
    /// The car as it is after the last step.
    pub fn capture(sim: &GameSim, drive_seconds: f64) -> CarView {
        let car = &sim.car.car;
        let mut view = CarView {
            steps: sim.steps,
            sim_seconds: sim.sim_seconds(),
            drive_seconds,
            body: car.core.get_world_matrix(car.body).m,
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
            ..CarView::default()
        };
        for index in 0..4.min(car.tyres.len()) {
            let tyre = &car.tyres[index];
            let hub: Mat44f = car.suspensions[index].get_hub_world_matrix(&car.core);
            view.hubs[index] = hub.m;
            view.wheels[index] = xm_matrix_multiply(&tyre.local_wheel_rotation, &hub).m;
            view.tyre_radius[index] = tyre.data.radius;
            view.tyre_width[index] = tyre.data.width;
            view.rim_radius[index] = tyre.data.rim_radius;
            view.wheel_load[index] = tyre.status.load;
            view.wheel_slip[index] = tyre.status.nd_slip;
        }
        if let Some(drivetrain) = &car.drivetrain {
            view.rpm = drivetrain.get_engine_rpm();
            view.gear = drivetrain.base().current_gear;
            view.rpm_limit = drivetrain.engine().get_limiter_rpm() as f32;
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
