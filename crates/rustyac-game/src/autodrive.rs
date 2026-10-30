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

use crate::input_file::event;
use crate::sim::{CarProbe, DriverSource};

/// Share of the grip the driver plans with (1 would be the limit of a car it knows nothing of).
const PACE: f32 = 0.80;
/// It never plans for more than this, m/s.
const TOP_SPEED: f32 = 95.0;

// The follower's three trigonometric calls. On Windows they are Rust's own, as they always
// were (a recorded `--autodrive` run is not to change); on another target (wasm) Rust's come
// from another library, so the C runtime's algorithms of `rustyac-math` are used, which give
// the same bits as Windows' wherever the two have been compared.
#[cfg(windows)]
fn atan2(y: f32, x: f32) -> f32 {
    y.atan2(x)
}
#[cfg(windows)]
fn sin(x: f32) -> f32 {
    x.sin()
}
#[cfg(windows)]
fn atan(x: f32) -> f32 {
    x.atan()
}
#[cfg(not(windows))]
use rustyac_math::{atan2f as atan2, atanf as atan, sinf as sin};

/// The line follower.
#[derive(Default)]
pub struct AutoDriver {
    track: Option<Arc<Track>>,
    probe: CarProbe,
    /// It has reached the racing line (from a pit box it follows the pit lane's line first).
    on_line: bool,
    /// Where the car was in the step before: a jump (back to the pits, back on track, a new
    /// session) makes it start again from where it stands.
    last_position: Option<[f32; 3]>,
    /// It has driven since it was put down (a car held on a grid is not stuck), and the seconds
    /// it has stood since with the gas down.
    moved: bool,
    stuck: f32,
    /// Commands for the game ([`DriverSource::take_events`]).
    events: u32,
    /// How much less than planned the car slows at full brake (m/s², twice over), from the
    /// full-brake spells so far (`brake_spell`: s and m/s lost) and the last step's pedal and speed.
    brake_loss: f32,
    brake_spell: (f32, f32),
    last_brake: f32,
    last_speed: f32,
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

/// How far `position` is from the line, seen from above (squared metres).
fn gap(spline: &AiSpline, position: &[f32; 3]) -> f32 {
    let q = spline.spline.points[spline.spline.closest_point_index(position) as usize].point;
    (q[0] - position[0]).powi(2) + (q[2] - position[2]).powi(2)
}

/// The speed the bends ahead allow, m/s: for every point up to 320 m ahead the speed its
/// curvature allows (grip that grows with the square of the speed), plus what can be braked
/// away on the way there.
fn allowed_speed(spline: &AiSpline, index: usize, brake_loss: f32) -> f32 {
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
        allowed = allowed.min((corner * corner + (2.0 * 21.0 * PACE - brake_loss) * ahead).sqrt());
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

    fn take_events(&mut self) -> (u32, i32) {
        (std::mem::take(&mut self.events), 0)
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
        let Some(track) = self.track.clone() else { return };
        let Some(fast) = track.ai_line().filter(|line| line.point_count() >= 16) else { return };
        let car = self.probe;
        if self.last_position.is_some_and(|p| (p[0] - car.position[0]).powi(2) + (p[1] - car.position[1]).powi(2) + (p[2] - car.position[2]).powi(2) > 100.0) {
            (self.on_line, self.moved, self.stuck) = (false, false, 0.0);
        }
        self.last_position = Some(car.position);
        // a car that slows less at full brake than planned (a GT car, not a Formula 1 car) brakes
        // earlier from then on: every 0.25 s spell of full brake above 10 m/s is measured
        if self.last_brake >= 1.0 && input.speed > 10.0 {
            self.brake_spell = (self.brake_spell.0 + dt, self.brake_spell.1 + self.last_speed - input.speed);
            if self.brake_spell.0 >= 0.25 {
                let short = 2.0 * 21.0 * PACE - 2.0 * self.brake_spell.1 / self.brake_spell.0;
                self.brake_loss = self.brake_loss.max(short);
                self.brake_spell = (0.0, 0.0);
            }
        } else {
            self.brake_spell = (0.0, 0.0);
        }
        self.last_speed = input.speed;
        // from a pit box the racing line is beyond the pit wall: the pit lane's line while it is the nearer one
        let pit = track.pit_lane_spline.as_ref().filter(|pit| !self.on_line && pit.point_count() >= 16 && gap(pit, &car.position) < gap(fast, &car.position));
        self.on_line = pit.is_none();
        let (spline, npos) = match pit {
            Some(pit) => (pit, pit.spline.world_to_spline(&car.position, -1)),
            None => (fast, if car.npos >= 0.0 { car.npos } else { fast.spline.world_to_spline(&car.position, -1) }),
        };
        let length = spline.length();
        let index = spline.spline.closest_point_index(&car.position) as usize;

        // the wheel: at a point of the line ahead
        let look = (5.0 + 0.22 * input.speed).clamp(6.0, 26.0);
        let p = spline.spline.spline_to_world(spline.spline.wrap_position(npos + look / length));
        let d = [p[0] - car.position[0], p[1] - car.position[1], p[2] - car.position[2]];
        let to_left = d[0] * car.left[0] + d[1] * car.left[1] + d[2] * car.left[2];
        let to_front = d[0] * car.forward[0] + d[1] * car.forward[1] + d[2] * car.forward[2];
        let alpha = atan2(to_left, to_front.max(0.5));
        let distance = (to_left * to_left + to_front * to_front).sqrt().max(1.0);
        let mut wheel_angle = atan(2.0 * car.wheelbase * sin(alpha) / distance);
        // a car short of grip (see brake_loss) turns the wheel at speed no further than about
        // three times its braking in lateral pull: more only spins it
        if self.brake_loss > 0.0 && input.speed > 10.0 {
            let most = car.wheelbase * 1.5 * (2.0 * 21.0 * PACE - self.brake_loss) / (input.speed * input.speed);
            wheel_angle = wheel_angle.clamp(-most, most);
        }
        // positive turns right
        controls.steer = (-wheel_angle / car.max_wheel_angle.max(0.05)).clamp(-1.0, 1.0);

        // the pedals
        // (in the pit lane at most 60 km/h, under any pit limiter)
        let error = allowed_speed(spline, index, self.brake_loss).min(if pit.is_some() { 16.7 } else { TOP_SPEED }) - input.speed;
        if error >= 0.0 {
            controls.gas = (0.25 + error * 0.4).clamp(0.0, 1.0);
        } else {
            controls.brake = (-error * 0.25).clamp(0.0, 1.0);
        }
        self.last_brake = controls.brake;
        // stuck (spun against a wall, beached in a gravel trap): after 3 s standing with the gas
        // down, back on the track (the game's "to track"); a drive that never stops is unchanged
        self.moved |= input.speed > 5.0;
        self.stuck = if self.moved && input.speed < 1.0 && error > 1.0 { self.stuck + dt } else { 0.0 };
        if self.stuck > 3.0 {
            (self.stuck, self.events) = (0.0, event::TO_TRACK);
        }
    }
}

/// Task 45b's drift drive (`--drift-drive`, on the drift track from its start): round the first
/// bends to the open tarmac south of the start, then down the long road with the throttle holding
/// the tail out and the wheels pointing where the car goes. What it drives is kept with
/// `--record` as an input file; its drifts score (Task 45b's report).
#[derive(Debug, Default)]
pub struct DriftDriver {
    probe: CarProbe,
    /// the car's velocity over the last step (m/s, from where the probes put it)
    velocity: [f32; 3],
    next: usize,
}

/// The route: x, z (m), the top speed on the way there (km/h) and whether it is a slide stretch;
/// after the last point, the brakes.
const DRIFT_ROUTE: [(f32, f32, f32, bool); 13] = [
    (-75.0, 186.0, 40.0, false),
    (-88.0, 180.0, 35.0, false),
    (-96.0, 168.0, 40.0, false),
    (-97.0, 150.0, 45.0, false),
    (-95.0, 136.0, 40.0, false),
    (-88.0, 122.0, 35.0, false),
    (-78.0, 116.0, 30.0, false),
    (-70.0, 102.0, 45.0, true),
    (-71.0, 92.0, 45.0, true),
    (-76.0, 82.0, 45.0, true),
    (-81.0, 72.0, 45.0, true),
    (-83.0, 62.0, 45.0, true),
    (-70.0, 52.0, 45.0, true),
];

impl DriverSource for DriftDriver {
    fn wants_probe(&self) -> bool {
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        // once a step (the device may be read more often)
        self.velocity = [0, 1, 2].map(|k| (probe.position[k] - self.probe.position[k]) / 0.003);
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
        let (car, v) = (self.probe, self.velocity);
        let flat = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[2] * b[2];
        // a point passed (near, or close behind the car) is done
        while let Some(&(x, z, ..)) = DRIFT_ROUTE.get(self.next) {
            let d = [x - car.position[0], 0.0, z - car.position[2]];
            let far = d[0].hypot(d[2]);
            if far > 7.0 && (far > 12.0 || flat(d, car.forward) > 0.0) {
                break;
            }
            self.next += 1;
        }
        let Some(&(x, z, top, slide)) = DRIFT_ROUTE.get(self.next) else {
            controls.brake = 1.0;
            return;
        };
        let d = [x - car.position[0], 0.0, z - car.position[2]];
        let to_left = atan2(flat(d, car.left), flat(d, car.forward));
        let (along, across) = (flat(v, car.forward), flat(v, car.left));
        let beta = if along.hypot(across) > 2.0 { atan2(across, along) } else { 0.0 };
        let kmh = v[0].hypot(v[2]) * 3.6;
        // in a slide the wheels point where the car goes; past ~17 degrees the front tyres
        // only push straight on
        let aim = if slide && beta.abs() > 0.25 { beta } else { to_left.clamp(-0.3, 0.3) };
        controls.steer = (-aim / car.max_wheel_angle.max(0.1)).clamp(-1.0, 1.0);
        if slide {
            // the throttle holds the slide angle
            controls.gas = match beta.abs() {
                b if b < 0.35 => 1.0,
                b if b < 0.6 => 0.75,
                b if b < 0.8 => 0.45,
                _ => 0.15,
            };
            if kmh < 25.0 {
                controls.gas = controls.gas.max(0.6);
            }
            if kmh > top {
                controls.gas = 0.0;
            }
        } else {
            controls.gas = if beta.abs() > 0.7 { 0.3 } else if kmh < top { 1.0 } else { 0.0 };
            controls.brake = if kmh > top + 8.0 { 0.6 } else { 0.0 };
        }
    }
}
