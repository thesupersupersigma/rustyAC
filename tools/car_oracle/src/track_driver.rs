// SPDX-License-Identifier: GPL-3.0-or-later

//! The scripted drivers of the track scenarios: they follow the track's AI line.
//!
//! The driver looks at the game's car as the last step left it (position, heading, speed,
//! the game's own position along the line) and aims the wheel at a point of the AI line a
//! little ahead (pure pursuit), with the pedals set by a speed the bends ahead allow. All of
//! it only decides the controls that are recorded with the run; the comparison replays those,
//! so how good a driver this is does not matter, only that the car goes where the test wants
//! it: over the crest, over the kerbs, onto the grass.

use rustyac_physics::track::{AiSpline, Track};
use rustyac_physics::vecmath::Vec3f;

use crate::scenario::{CarView, Controls, DT};

/// Which track scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    /// From the hot-lap start: flat out down the pit straight, brake for the first corner.
    Launch,
    /// From 300 m before the lowest point of the first sector: flat out through it.
    EauRouge,
    /// Through the last chicane on its kerbs.
    Kerbs,
    /// A lap, as far as a minute goes.
    Lap,
    /// Along a straight with the two right wheels on the grass for a while.
    Grass,
    /// Over the timing lines one after the other, teleported from line to line.
    Timing,
    /// Task 13. At 60 km/h straight on at the first corner: into the barrier.
    WallLow,
    /// Flat out and straight on at a fast bend: into the barrier at more than 200 km/h.
    WallHigh,
    /// The same run with the wheel let go: straight on, over the run-off (which slows the
    /// car) and into the barrier at about 195 km/h, one corner first.
    WallGravel,
    /// Steered off the road at a shallow angle and along the wall.
    WallSlide,
    /// As `EauRouge`: the floor meets the road in the compression.
    Bottoming,
    /// The last chicane much too fast, deep over its kerbs.
    KerbStrike,
    /// Put down upside down, later on its side, and left alone.
    Rollover,
    /// Task 18. From the hot-lap start one whole lap, until the game has counted it.
    FullLap,
    /// A point-to-point track: through the start gate, some of the road, a jump to before
    /// the finish gate, through it.
    Run,
    /// A point-to-point track: the whole run.
    RunFull,
    /// At the loose object nearest to the racing line.
    ObjectHit,
}

/// What the driver thinks the car can do: sideways grip at no speed and what every (m/s)^2
/// adds to it (downforce), and braking, all in m/s^2 before the scenario's pace factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    pub lateral: f32,
    pub downforce: f32,
    pub braking: f32,
    /// Top speed the driver allows himself, m/s.
    pub top: f32,
}

impl Default for Grip {
    /// The F2004 (the numbers the Spa scenarios of Task 12 were written with).
    fn default() -> Grip {
        Grip { lateral: 14.0, downforce: 0.0036, braking: 21.0, top: 95.0 }
    }
}

impl Grip {
    /// By the car's folder name: the cars with wings and slicks drive as the F2004 does,
    /// everything else as a road car on road tyres.
    pub fn of(car: &str) -> Grip {
        if car.contains("f2004") || car.contains("formula") || car.contains("_f1") || car.contains("sf15") || car.contains("f138") {
            Grip::default()
        } else if car.contains("gt3") || car.contains("gt2") || car.contains("gte") {
            Grip { lateral: 11.0, downforce: 0.0012, braking: 13.0, top: 80.0 }
        } else {
            Grip { lateral: 7.8, downforce: 0.0, braking: 7.5, top: 70.0 }
        }
    }
}

/// The loose object of a track that is easiest to drive at: the one nearest to the racing
/// line, not further out than a few metres beyond the track's edge. Returns its number,
/// its place along the line in metres, and how far to the left of the line it stands.
pub fn nearest_object(track: &Track) -> Option<(usize, f32, f32)> {
    let spline = track.ai_spline.as_ref()?;
    let length = spline.length();
    let mut best: Option<(usize, f32, f32)> = None;
    for (number, object) in track.objects.iter().enumerate() {
        let p = [object.matrix.m[3][0], object.matrix.m[3][1], object.matrix.m[3][2]];
        let n = spline.spline.world_to_spline(&p, -1);
        let on_line = spline.spline.spline_to_world(n);
        let f = spline.payload_at_position(n).forward_vector;
        let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-6);
        let (lx, lz) = (f[2] / l, -f[0] / l);
        let lateral = (p[0] - on_line[0]) * lx + (p[2] - on_line[2]) * lz;
        let sides = spline.payload_at_position(n).sides;
        let edge = if lateral > 0.0 { sides[0] } else { sides[1] };
        // on the level of the road, within reach, and not where a lap starts or ends
        if (p[1] - on_line[1]).abs() > 1.2 || lateral.abs() > edge + 4.0 || !(0.04..0.96).contains(&n) {
            continue;
        }
        if best.is_none_or(|(_, _, other)| lateral.abs() < other.abs()) {
            best = Some((number, n * length, lateral));
        }
    }
    best
}

impl TrackKind {
    /// The scenarios that leave the road on purpose (their runs do not end there).
    pub fn leaves_the_road(self) -> bool {
        matches!(
            self,
            TrackKind::WallLow | TrackKind::WallHigh | TrackKind::WallGravel | TrackKind::WallSlide | TrackKind::KerbStrike | TrackKind::Rollover | TrackKind::ObjectHit
        )
    }
}

/// The body's axes (rows: x = left, y = up, z = forward) of a car that stands level with its
/// tail towards `tail` and is then rolled about its length by `roll` radians.
pub fn rolled_rows(tail: [f32; 3], roll: f32) -> [[f32; 3]; 3] {
    let l = (tail[0] * tail[0] + tail[2] * tail[2]).sqrt().max(1e-6);
    let z = [-tail[0] / l, 0.0, -tail[2] / l];
    let x = [z[2], 0.0, -z[0]];
    let y = [0.0f32, 1.0, 0.0];
    let (s, c) = roll.sin_cos();
    [
        [c * x[0] + s * y[0], c * x[1] + s * y[1], c * x[2] + s * y[2]],
        [c * y[0] - s * x[0], c * y[1] - s * x[1], c * y[2] - s * x[2]],
        z,
    ]
}

/// The timing lines the timing scenario crosses, in order: the start line (the armed first
/// crossing), two laps' worth of all three lines, and the start line once more.
const TIMING_PLAN: [usize; 8] = [0, 1, 2, 0, 1, 2, 0, 0];
/// It is put down this far before a line and leaves this far after it, metres.
const TIMING_BEFORE: f32 = 60.0;
const TIMING_AFTER: f32 = 30.0;

/// Where a scenario starts, as (position on the road, direction of the tail), whether the
/// first lap is armed (a hot-lap start), and a note for the recording's header.
pub fn spawn(kind: TrackKind, track: &mut Track) -> Result<(Vec3f, Vec3f, bool, String), String> {
    let on_line = |track: &Track, metres: f32, what: &str| -> Result<(Vec3f, Vec3f, bool, String), String> {
        let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
        let n = (metres / spline.length()).rem_euclid(1.0);
        let (position, tail) = track.pose_on_ai_line_at(n).ok_or("the AI line is too short")?;
        Ok((position, tail, false, format!("{what}, {metres:.0} m along the AI line")))
    };
    match kind {
        TrackKind::WallLow => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let corner = tightest_point(spline, 0.005, 0.07);
            on_line(track, corner - 110.0, "110 m before the tightest point of the first corner")
        }
        TrackKind::WallHigh | TrackKind::WallGravel => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let bend = tightest_point(spline, 0.80, 0.885);
            on_line(track, bend - 560.0, "560 m before the tightest point of the fast bend at 80 to 88 % of the lap")
        }
        TrackKind::Bottoming => spawn(TrackKind::EauRouge, track),
        TrackKind::KerbStrike => spawn(TrackKind::Kerbs, track),
        TrackKind::Rollover => spawn(TrackKind::Grass, track),
        TrackKind::WallSlide => {
            rustyac_physics::track::init_respawn_position_set(track, "HOTLAP_START");
            let (position, tail) = track.spawn_pose("HOTLAP_START", 0).ok_or("the track has no AC_HOTLAP_START_0")?;
            Ok((position, tail, false, "AC_HOTLAP_START_0".to_string()))
        }
        TrackKind::ObjectHit => {
            let (_, metres, _) = nearest_object(track).ok_or("the track has no loose object within reach of its AI line")?;
            on_line(track, metres - 140.0, "140 m before the loose object nearest to the AI line")
        }
        TrackKind::Launch | TrackKind::Lap | TrackKind::FullLap | TrackKind::Run | TrackKind::RunFull => {
            // RaceManager::initOffline for a hot-lap session
            rustyac_physics::track::init_respawn_position_set(track, "HOTLAP_START");
            let (position, tail) = track.spawn_pose("HOTLAP_START", 0).ok_or("the track has no AC_HOTLAP_START_0")?;
            Ok((position, tail, true, "AC_HOTLAP_START_0".to_string()))
        }
        TrackKind::EauRouge => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let lowest = lowest_point(spline, 0.03, 0.20);
            on_line(track, lowest - 300.0, "300 m before the lowest point of the first fifth of the lap")
        }
        TrackKind::Kerbs => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let tightest = tightest_point(spline, 0.90, 0.985);
            on_line(track, tightest - 200.0, "200 m before the tightest point of the last chicane")
        }
        TrackKind::Grass => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let lowest = lowest_point(spline, 0.03, 0.20);
            on_line(track, lowest + 600.0, "600 m after the lowest point of the first fifth of the lap (the straight up the hill)")
        }
        TrackKind::Timing => {
            let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
            let line = *track.sectors_normalized_positions.first().ok_or("the track has no timing line")?;
            let mut start = on_line(track, line * spline.length() - TIMING_BEFORE, "60 m before the start line")?;
            // as in a hot-lap session: the first crossing of the line starts the timer anew
            start.2 = true;
            Ok(start)
        }
    }
}

/// Metres along the line of its lowest point between two lap fractions.
fn lowest_point(spline: &AiSpline, from: f32, to: f32) -> f32 {
    let length = spline.length();
    let mut best = (f32::MAX, 0.0);
    for p in &spline.spline.points {
        let n = p.point_length / length;
        if n >= from && n <= to && p.point[1] < best.0 {
            best = (p.point[1], p.point_length);
        }
    }
    best.1
}

/// Curvature (1/m, positive to the left) of the line around point `i`, from the points
/// `span` before and after it, seen from above.
fn curvature(spline: &AiSpline, i: usize, span: usize) -> f32 {
    let points = &spline.spline.points;
    let n = points.len();
    let a = points[(i + n - span) % n].point;
    let b = points[i % n].point;
    let c = points[(i + span) % n].point;
    // with +x to the left of a car heading +z, a left turn has a positive cross product here
    let (abx, abz, bcx, bcz) = (b[0] - a[0], b[2] - a[2], c[0] - b[0], c[2] - b[2]);
    let cross = abz * bcx - abx * bcz;
    let (ab, bc, ac) = ((abx * abx + abz * abz).sqrt(), (bcx * bcx + bcz * bcz).sqrt(), ((c[0] - a[0]).powi(2) + (c[2] - a[2]).powi(2)).sqrt());
    if ab * bc * ac < 1e-6 {
        0.0
    } else {
        2.0 * cross / (ab * bc * ac)
    }
}

/// Metres along the line of its tightest point between two lap fractions.
fn tightest_point(spline: &AiSpline, from: f32, to: f32) -> f32 {
    let length = spline.length();
    let mut best = (0.0f32, 0.0);
    for (i, p) in spline.spline.points.iter().enumerate() {
        let n = p.point_length / length;
        if n >= from && n <= to {
            let k = curvature(spline, i, 4).abs();
            if k > best.0 {
                best = (k, p.point_length);
            }
        }
    }
    best.1
}

/// What the line follower keeps between steps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Follower {
    /// The sideways aim, metres to the left of the line, eased towards its target.
    lateral: f32,
    /// The run is over: the car is off the track for good.
    pub ended: bool,
    /// The timing scenario: which entry of its plan the car is driving at, and whether it
    /// has been seen before that line since it was put down.
    hop: usize,
    before_line: bool,
    /// Task 13: metres along the line of the place a wall scenario stops steering at, and
    /// whether it has got there.
    release_at: Option<f32>,
    released: bool,
    /// The wheel after that place (the fast wall scenario leans towards the outside of the bend).
    release_steer: f32,
    /// The rollover scenario: where the car is put down (position on the road, tail).
    put_down: Option<([f32; 3], [f32; 3])>,
    /// Task 18. What the driver thinks the car can do.
    pub grip: Grip,
    /// When the game counted the lap or the run (seconds since the release).
    done_at: Option<f32>,
    /// The point-to-point scenario has made its jump.
    jumped: bool,
    /// The object scenario: its object (place along the line in metres, metres to the left).
    object: Option<(f32, f32)>,
}

/// The speed the bends ahead allow, m/s: for every point up to 320 m ahead the speed its
/// curvature allows, plus what can be braked away on the way there.
fn allowed_speed(spline: &AiSpline, index: usize, pace: f32, grip: &Grip) -> f32 {
    let points = &spline.spline.points;
    let n = points.len();
    let here = points[index % n].point_length;
    let length = spline.length();
    let mut allowed = grip.top;
    let mut i = index;
    loop {
        let mut ahead = points[i % n].point_length - here;
        if ahead < 0.0 {
            ahead += length;
        }
        if ahead > 320.0 {
            break;
        }
        let k = curvature(spline, i, 5).abs();
        // grip that grows with the square of the speed (downforce): v^2 k = a0 + a1 v^2
        let (a0, a1) = (grip.lateral * pace, grip.downforce * pace);
        let corner = if k <= a1 * 1.08 { grip.top } else { (a0 / (k - a1)).sqrt() };
        let braked = (corner * corner + 2.0 * grip.braking * pace * ahead).sqrt();
        allowed = allowed.min(braked);
        i += 3;
        if i >= index + n {
            break;
        }
    }
    allowed
}

impl Follower {
    /// The controls of one step. `t` is the time since the car was released.
    pub fn controls(&mut self, kind: TrackKind, track: &Track, car: &CarView, t: f32, c: &mut Controls) {
        let Some(spline) = &track.ai_spline else { return };
        let length = spline.length();
        let position = [car.position[0], car.position[1], car.position[2]];
        // the game's own place along the line (-1 before its first step)
        let npos = if car.npos >= 0.0 { car.npos } else { spline.spline.world_to_spline(&position, -1) };
        let index = spline.spline.closest_point_index(&position) as usize;
        let payload = spline.payload_at_position(npos);

        if kind == TrackKind::Timing {
            // how far past the line of this hop the car is, along the lap
            let line = track.sectors_normalized_positions[TIMING_PLAN[self.hop]];
            let mut past = (npos - line) * length;
            if past > length * 0.5 {
                past -= length;
            } else if past < -length * 0.5 {
                past += length;
            }
            if past < -5.0 {
                self.before_line = true;
            }
            if self.before_line && past > TIMING_AFTER {
                if self.hop + 1 == TIMING_PLAN.len() {
                    self.ended = true;
                    return;
                }
                self.hop += 1;
                self.before_line = false;
                let next = track.sectors_normalized_positions[TIMING_PLAN[self.hop]];
                let n = spline.spline.wrap_position(next - TIMING_BEFORE / length);
                if let Some((p, tail)) = track.pose_on_ai_line_at(n) {
                    // the last one spoils the lap, as every teleport of the game's own does
                    c.teleport = if self.hop + 1 == TIMING_PLAN.len() { 2 } else { 1 };
                    c.teleport_position = [p.x, p.y, p.z];
                    c.teleport_tail = [tail.x, tail.y, tail.z];
                }
                return;
            }
        }

        if matches!(kind, TrackKind::FullLap | TrackKind::Run | TrackKind::RunFull) && car.laps >= 1 {
            // the game has counted the lap (or the run): three seconds more, then the end
            let done = *self.done_at.get_or_insert(t);
            if t - done > 3.0 {
                self.ended = true;
                return;
            }
        }
        if kind == TrackKind::Run && !self.jumped && t >= 40.0 {
            // put down 250 m before the finish gate, without spoiling anything
            self.jumped = true;
            if let Some(gate) = track.time_lines.iter().find(|line| line.line_type == 2) {
                let mid = [(gate.points[0].x + gate.points[1].x) * 0.5, (gate.points[0].y + gate.points[1].y) * 0.5, (gate.points[0].z + gate.points[1].z) * 0.5];
                let at = spline.spline.world_to_spline(&mid, -1);
                let n = spline.spline.wrap_position(at - 250.0 / length);
                if let Some((p, tail)) = track.pose_on_ai_line_at(n) {
                    c.teleport = 1;
                    c.teleport_position = [p.x, p.y, p.z];
                    c.teleport_tail = [tail.x, tail.y, tail.z];
                }
            }
            return;
        }
        if kind == TrackKind::ObjectHit {
            if self.object.is_none() {
                self.object = nearest_object(track).map(|(_, metres, lateral)| (metres, lateral));
            }
            // the job of a new session, while the object lies where the car left it
            c.reset_objects = car.step == (14.0 / DT) as usize;
        }

        if kind == TrackKind::Rollover {
            // nobody drives: the car is put down on its roof, later on its side
            if self.put_down.is_none() {
                let n = spline.spline.wrap_position(npos);
                if let Some((p, tail)) = track.pose_on_ai_line_at(n) {
                    self.put_down = Some(([p.x, p.y, p.z], [tail.x, tail.y, tail.z]));
                }
            }
            if let Some((p, tail)) = self.put_down {
                let roll = match car.step {
                    400 => Some(std::f32::consts::PI),
                    1900 => Some(std::f32::consts::FRAC_PI_2),
                    _ => None,
                };
                // a car that was asleep when it was put down stays frozen in the air (the
                // game's sleeping rule only looks at speed, spin, the tyres' last loads and
                // the throttle): a second of throttle wakes it, and it drops
                if (400..750).contains(&car.step) || (1900..2250).contains(&car.step) {
                    c.gas = 0.6;
                }
                if let Some(roll) = roll {
                    c.teleport = 3;
                    c.teleport_position = [p[0], p[1] + 1.25, p[2]];
                    c.teleport_tail = tail;
                    c.teleport_rows = rolled_rows(tail, roll);
                }
            }
            return;
        }
        if matches!(kind, TrackKind::WallLow | TrackKind::WallHigh | TrackKind::WallGravel) {
            // follow the line up to a place before the bend, then hold the wheel straight
            let release_at = *self.release_at.get_or_insert_with(|| match kind {
                TrackKind::WallLow => tightest_point(spline, 0.005, 0.07) - 45.0,
                _ => tightest_point(spline, 0.80, 0.885) - 90.0,
            });
            let mut to_go = release_at - npos * length;
            if to_go < -length * 0.5 {
                to_go += length;
            } else if to_go > length * 0.5 {
                to_go -= length;
            }
            if to_go <= 0.0 && !self.released {
                self.released = true;
                if kind == TrackKind::WallHigh {
                    // a little wheel towards the outside of the bend: less of the run-off
                    // (which slows the car) before the barrier. Positive turns right; a
                    // positive curvature is a bend to the left.
                    let amount = 0.06f32;
                    let mut bend = 0.0f32;
                    for (i, p) in spline.spline.points.iter().enumerate() {
                        if (0.80..=0.885).contains(&(p.point_length / length)) {
                            let k = curvature(spline, i, 4);
                            if k.abs() > bend.abs() {
                                bend = k;
                            }
                        }
                    }
                    self.release_steer = if bend >= 0.0 { amount } else { -amount };
                }
            }
        }

        // where to be beside the line
        let k_here = curvature(spline, index + 6, 5);
        let target_lateral = match kind {
            // the inner wheels over the kerb: the edge of the track is beyond the kerbs
            TrackKind::Kerbs if k_here > 0.012 => payload.sides[0] - 0.9,
            TrackKind::Kerbs if k_here < -0.012 => -(payload.sides[1] - 0.9),
            // two wheels off: the car's middle just over the right edge
            TrackKind::Grass if (5.0..9.5).contains(&t) => -(payload.sides[1] + 0.15),
            // deep over the inner kerbs
            TrackKind::KerbStrike if k_here > 0.008 => payload.sides[0] - 0.15,
            TrackKind::KerbStrike if k_here < -0.008 => -(payload.sides[1] - 0.15),
            // further and further to the left, far beyond the edge of the road
            TrackKind::WallSlide if t > 2.5 => (payload.sides[0] + 30.0).min((t - 2.5) * 6.0),
            // the car's middle at the object from 90 m before it until 12 m past it
            TrackKind::ObjectHit => match self.object {
                Some((metres, lateral)) => {
                    let mut to_go = metres - npos * length;
                    if to_go < -length * 0.5 {
                        to_go += length;
                    } else if to_go > length * 0.5 {
                        to_go -= length;
                    }
                    if (-12.0..90.0).contains(&to_go) {
                        lateral
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            },
            _ => 0.0,
        };
        self.lateral += (target_lateral - self.lateral) * (DT / 0.35).min(1.0);

        // the wheel: at a point of the line ahead, moved sideways
        let look = (5.0 + 0.22 * car.speed).clamp(6.0, 26.0);
        let ahead = spline.spline.wrap_position(npos + look / length);
        let p = spline.spline.spline_to_world(ahead);
        let f = spline.payload_at_position(ahead).forward_vector;
        // left of the line's direction, level
        let (lx, lz) = {
            let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-6);
            (f[2] / l, -f[0] / l)
        };
        let aim = [p[0] + lx * self.lateral, p[1], p[2] + lz * self.lateral];
        let d = [aim[0] - position[0], aim[1] - position[1], aim[2] - position[2]];
        let to_left = d[0] * car.left[0] + d[1] * car.left[1] + d[2] * car.left[2];
        let to_front = d[0] * car.forward[0] + d[1] * car.forward[1] + d[2] * car.forward[2];
        let alpha = to_left.atan2(to_front.max(0.5));
        let distance = (to_left * to_left + to_front * to_front).sqrt().max(1.0);
        let wheel_angle = (2.0 * 3.05 * alpha.sin() / distance).atan();
        // positive turns right
        c.steer = (-wheel_angle / car.max_wheel_angle.max(0.05)).clamp(-1.0, 1.0);

        // the pedals
        let (pace, limit) = match kind {
            TrackKind::Launch => (0.80, 95.0),
            TrackKind::Lap => (0.78, 95.0),
            TrackKind::Kerbs => (0.55, 38.0),
            TrackKind::Grass => (0.80, 50.0),
            TrackKind::Timing => (0.60, 36.0),
            TrackKind::EauRouge | TrackKind::Bottoming => (10.0, 200.0),
            TrackKind::WallLow => (0.8, 16.7),
            TrackKind::WallHigh | TrackKind::WallGravel => (10.0, 200.0),
            TrackKind::WallSlide => (0.8, 28.0),
            TrackKind::KerbStrike => (1.15, 52.0),
            TrackKind::Rollover => (0.0, 0.0),
            TrackKind::FullLap => (0.78, 95.0),
            TrackKind::Run | TrackKind::RunFull => (0.70, 95.0),
            TrackKind::ObjectHit => (0.7, 19.5),
        };
        let target = allowed_speed(spline, index, pace, &self.grip).min(limit);
        let error = target - car.speed;
        if self.released {
            // straight on: the wheel is let go; the low-speed run keeps its 60 km/h, the fast
            // one stays flat out for two seconds and then brakes
            c.steer = self.release_steer;
        }
        let fast_wall = matches!(kind, TrackKind::WallHigh | TrackKind::WallGravel);
        if matches!(kind, TrackKind::EauRouge | TrackKind::Bottoming) || (fast_wall && t < 14.0) {
            c.gas = 1.0;
        } else if fast_wall {
            c.brake = 1.0;
        } else if kind == TrackKind::WallLow && self.released {
            let error = 16.7 - car.speed;
            c.gas = if t < 11.0 { (0.2 + error * 0.4).clamp(0.0, 1.0) } else { 0.0 };
        } else if error >= 0.0 {
            c.gas = (0.25 + error * 0.4).clamp(0.0, 1.0);
        } else {
            c.brake = (-error * 0.25).clamp(0.0, 1.0);
        }

        if matches!(kind, TrackKind::FullLap | TrackKind::Run | TrackKind::RunFull) && t < 5.0 {
            // a careful launch: the hot-lap start of some tracks is in a corner
            c.gas = c.gas.min(0.3 + 0.14 * t);
        }

        // off the track for good, or gone from the world: the body would be in a wall by now
        let side = if car.offset > 0.0 { payload.sides[0] } else { payload.sides[1] };
        if !kind.leaves_the_road() && t > 2.0 && (car.offset.abs() > side + 6.0 || !car.position[1].is_finite() || car.position[1] < -300.0) {
            self.ended = true;
        }
    }
}
