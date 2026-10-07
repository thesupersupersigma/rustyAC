// SPDX-License-Identifier: GPL-3.0-or-later

//! `--autodrive`: a driver that follows the track's AI line, for checks nobody sits at (a
//! whole lap for the lap timer, a camera ride for the frame rate) and for watching.
//!
//! It is not AC's AI: it aims the wheel at a point of the line a little ahead (pure pursuit)
//! and sets the pedals by the speed the bends ahead allow, with margins wide enough to stay
//! on the road without knowing the car. It reads the car like any driver (through
//! [`CarProbe`]) and answers through `Car::controls`, so a drive of it is recorded and
//! replayed like any other.

use std::sync::Arc;

use rustyac_physics::car::{CarControls, CarControlsInput};
use rustyac_physics::track::{AiSpline, Track};

use crate::sim::{CarProbe, DriverSource};

/// Share of the grip the driver plans with (1 would be the limit of a car it knows nothing of).
const PACE: f32 = 0.80;
/// It never plans for more than this, m/s.
const TOP_SPEED: f32 = 95.0;

/// The line follower.
#[derive(Default)]
pub struct AutoDriver {
    track: Option<Arc<Track>>,
    probe: CarProbe,
}

impl AutoDriver {
    pub fn new() -> AutoDriver {
        AutoDriver::default()
    }
}

/// Curvature (1/m, positive to the left) of the line around point `i`, seen from above.
fn curvature(spline: &AiSpline, i: usize, span: usize) -> f32 {
    let points = &spline.spline.points;
    let n = points.len();
    let a = points[(i + n - span) % n].point;
    let b = points[i % n].point;
    let c = points[(i + span) % n].point;
    let (abx, abz, bcx, bcz) = (b[0] - a[0], b[2] - a[2], c[0] - b[0], c[2] - b[2]);
    let cross = abz * bcx - abx * bcz;
    let (ab, bc, ac) = ((abx * abx + abz * abz).sqrt(), (bcx * bcx + bcz * bcz).sqrt(), ((c[0] - a[0]).powi(2) + (c[2] - a[2]).powi(2)).sqrt());
    if ab * bc * ac < 1e-6 {
        0.0
    } else {
        2.0 * cross / (ab * bc * ac)
    }
}

/// The speed the bends ahead allow, m/s: for every point up to 320 m ahead the speed its
/// curvature allows (grip that grows with the square of the speed), plus what can be braked
/// away on the way there.
fn allowed_speed(spline: &AiSpline, index: usize) -> f32 {
    let points = &spline.spline.points;
    let n = points.len();
    let here = points[index % n].point_length;
    let length = spline.length();
    let mut allowed = TOP_SPEED;
    let mut i = index;
    while i < index + n {
        let mut ahead = points[i % n].point_length - here;
        if ahead < 0.0 {
            ahead += length;
        }
        if ahead > 320.0 {
            break;
        }
        let k = curvature(spline, i, 5).abs();
        let (a0, a1) = (14.0 * PACE, 0.0036 * PACE);
        let corner = if k <= a1 * 1.08 { TOP_SPEED } else { (a0 / (k - a1)).sqrt() };
        allowed = allowed.min((corner * corner + 2.0 * 21.0 * PACE * ahead).sqrt());
        i += 3;
    }
    allowed
}

impl DriverSource for AutoDriver {
    fn set_track(&mut self, track: &Arc<Track>) {
        self.track = Some(Arc::clone(track));
    }

    fn wants_probe(&self) -> bool {
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, input: &CarControlsInput) {
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
        let Some(spline) = self.track.as_ref().and_then(|track| track.ai_spline.as_ref()) else { return };
        if spline.point_count() < 16 {
            return;
        }
        let car = &self.probe;
        let length = spline.length();
        let npos = if car.npos >= 0.0 { car.npos } else { spline.spline.world_to_spline(&car.position, -1) };
        let index = spline.spline.closest_point_index(&car.position) as usize;

        // the wheel: at a point of the line ahead
        let look = (5.0 + 0.22 * input.speed).clamp(6.0, 26.0);
        let p = spline.spline.spline_to_world(spline.spline.wrap_position(npos + look / length));
        let d = [p[0] - car.position[0], p[1] - car.position[1], p[2] - car.position[2]];
        let to_left = d[0] * car.left[0] + d[1] * car.left[1] + d[2] * car.left[2];
        let to_front = d[0] * car.forward[0] + d[1] * car.forward[1] + d[2] * car.forward[2];
        let alpha = to_left.atan2(to_front.max(0.5));
        let distance = (to_left * to_left + to_front * to_front).sqrt().max(1.0);
        let wheel_angle = (2.0 * car.wheelbase * alpha.sin() / distance).atan();
        // positive turns right
        controls.steer = (-wheel_angle / car.max_wheel_angle.max(0.05)).clamp(-1.0, 1.0);

        // the pedals
        let error = allowed_speed(spline, index) - input.speed;
        if error >= 0.0 {
            controls.gas = (0.25 + error * 0.4).clamp(0.0, 1.0);
        } else {
            controls.brake = (-error * 0.25).clamp(0.0, 1.0);
        }
    }
}
