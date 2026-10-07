// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! Lap timing and spawn points.
//!
//! * [`TimeLine`]: a timing gate between two helper nodes (`AC_TIME_n_L` / `_R`); gate 0 is
//!   start / finish, the others end a sector each.
//! * [`TimeTransponder`]: a car's lap timer. Every physics step it tests the hub of wheel 0
//!   against every gate; the time is a whole number of milliseconds that grows by 3 per step.
//! * [`LapInvalidator`]: counts the tyres on surfaces that are not track and, by the
//!   session's penalty mode, adds a "cut" to the lap or a time penalty.
//! * [`LapDb`]: the lap list above the transponder (the game's `RaceTimingServices`), which
//!   is what the lap displays read: a lap with cuts is listed but is never the best.
//! * The spawn sets (`AC_PIT_n`, `AC_START_n`, `AC_HOTLAP_START_n` ...), dropped onto the road.
//!
//! Gates and spawn points take their place from the helper node's **own** matrix, not from
//! its place in the world: the game does too.

use super::Track;
use crate::math::sqrtf;
use crate::tyre::{RayTrackCollisionProvider, SurfaceDef};
use crate::vecmath::Vec3f;

/// `TimeLine` (0x38 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeLine {
    /// `type`: 0 lap / sector, 1 start of an A-to-B track, 2 its finish.
    pub line_type: i32,
    /// `points`: the left and the right end, at height 0.
    pub points: [Vec3f; 2],
    /// `id`: its number among the track's lines.
    pub id: i32,
    /// `length`: the gate's width, squared.
    pub length: f32,
    /// `planeNormal`: level, across the gate, pointing the way cars drive.
    pub plane_normal: Vec3f,
}

impl TimeLine {
    /// `TimeLine::TimeLine` @ 0x1402cadb0
    pub fn new(p0: &Vec3f, p1: &Vec3f, id: i32, line_type: i32) -> TimeLine {
        let mut points = [*p0, *p1];
        points[0].y = 0.0;
        points[1].y = 0.0;
        let dy = points[0].y - points[1].y;
        let dx = points[0].x - points[1].x;
        let dz = points[0].z - points[1].z;
        let length = (dy * dy + dx * dx) + dz * dz;
        let (mut dx, mut dy, mut dz) = (points[0].x - points[1].x, points[0].y - points[1].y, points[0].z - points[1].z);
        let len = sqrtf((dy * dy + dx * dx) + dz * dz);
        if len < 0.0 || len > 0.0 {
            let inv = 1.0 / len;
            dx *= inv;
            dy *= inv;
            dz *= inv;
        }
        let zero = 0.0f32;
        let plane_normal = Vec3f::new((dy * zero) - dz, (dz * zero) - (dx * zero), dx - (dy * zero));
        TimeLine { line_type, points, id, length, plane_normal }
    }

    /// `TimeLine::check` @ 0x1402caef0: 0 = not at the gate, 1 = at it and before the line,
    /// 2 = at it and past the line. Only x and z of `pos` count.
    pub fn check(&self, pos: &Vec3f) -> i32 {
        let (px, pz) = (pos.x, pos.z);
        let [p0, p1] = &self.points;
        let a = -p0.y;
        let d0 = ((a * a) + (px - p0.x) * (px - p0.x)) + (pz - p0.z) * (pz - p0.z);
        if d0 > self.length {
            return 0;
        }
        let b = -p1.y;
        let d1 = ((b * b) + (px - p1.x) * (px - p1.x)) + (pz - p1.z) * (pz - p1.z);
        if d1 > self.length {
            return 0;
        }
        let s = ((-p0.y) * self.plane_normal.y + (px - p0.x) * self.plane_normal.x) + (pz - p0.z) * self.plane_normal.z;
        if s > 0.0 {
            2
        } else {
            1
        }
    }
}

/// `TimeLineStatus`: what a car remembers of one gate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimeLineStatus {
    /// `isValid`: crossed in this lap
    pub is_valid: bool,
    /// `lastResponse`: what [`TimeLine::check`] said in the last step
    pub last_response: i32,
    /// `lastTime`: the lap time at the last crossing, ms
    pub last_time: u32,
}

/// `OnLapCompletedEvent`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnLapCompletedEvent {
    pub car_index: u32,
    pub lap_time: u32,
    pub lap_count: u32,
    pub splits: Vec<u32>,
    pub event_time: f64,
    pub is_valid: bool,
    pub cuts: i32,
}

/// `OnSectorSplitEvent`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OnSectorSplitEvent {
    pub car_index: u32,
    pub sector_index: i32,
    pub sector_time: u32,
    pub cuts: i32,
}

/// What the car has to tell a finish-line crossing, and what the crossing may do to the car.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FinishContext {
    /// `Car::getPenaltyTime()`
    pub penalty_time: f64,
    /// `PenaltyManager::checkBlackFlag()`
    pub check_black_flag: bool,
}

/// What a step of the transponder asks of the car.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TransponderActions {
    /// `Car::setBlackFlag(true, reason)`: 4 = crossed the line with a time penalty, 1 = the
    /// pit penalty was not served.
    pub black_flag: Option<i32>,
    /// `PenaltyManager::decreasePitPenaltyLaps(lastLap != 0)` is due, with this argument.
    pub decrease_pit_penalty_laps: Option<bool>,
}

/// AC's `TimeTransponder` (0x98 bytes, `Car::transponder`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeTransponder {
    /// `t`: the running lap time, ms
    pub t: u32,
    pub last_lap: u32,
    pub best_lap: u32,
    pub lap_count: u32,
    /// `finishLinePassed`: true in the step a finish crossing was processed
    pub finish_line_passed: bool,
    pub was_last_lap_valid: bool,
    pub status: Vec<TimeLineStatus>,
    pub last_lap_splits: Vec<u32>,
    pub best_lap_splits: Vec<u32>,
    pub current_splits: Vec<u32>,
    /// `isFirstLapArmed`: hot-lap mode, the first crossing of the line starts the timer anew
    pub is_first_lap_armed: bool,
    /// `cuts`: times the car left the track in this lap
    pub cuts: i32,
    pub ext_invalid: bool,
    pub open_track_state: i32,
    pub is_open_track: bool,
    /// The car's number (`Car::physicsGUID`).
    pub car_index: u32,
    /// Laps and splits since the game last took them ([`TimeTransponder::take_events`]).
    pub lap_events: Vec<OnLapCompletedEvent>,
    pub split_events: Vec<OnSectorSplitEvent>,
}

impl TimeTransponder {
    /// `TimeTransponder::init` @ 0x1402907c0: for the lines the track has now.
    pub fn new(track: &Track, car_index: u32) -> TimeTransponder {
        let n = track.time_lines.len();
        TimeTransponder {
            status: vec![TimeLineStatus::default(); n],
            last_lap_splits: vec![0; n],
            best_lap_splits: vec![0; n],
            current_splits: vec![0; n],
            is_open_track: track.is_open,
            car_index,
            ..TimeTransponder::default()
        }
    }

    /// `TimeTransponder::isValid` @ 0x140290980: every line was crossed in this lap.
    pub fn is_valid(&self) -> bool {
        if self.is_open_track {
            return true;
        }
        if self.status.iter().any(|s| !s.is_valid) {
            return false;
        }
        !self.status.is_empty() && !self.ext_invalid
    }

    /// `TimeTransponder::invalidate` @ 0x1402908f0 (a teleport).
    pub fn invalidate(&mut self) {
        for s in &mut self.status {
            *s = TimeLineStatus::default();
        }
        for split in &mut self.current_splits {
            *split = 0;
        }
    }

    /// `TimeTransponder::reset` @ 0x140290e60 (a new session).
    pub fn reset(&mut self) {
        self.t = 0;
        self.last_lap = 0;
        self.best_lap = 0;
        self.lap_count = 0;
        self.cuts = 0;
        self.ext_invalid = false;
        for s in &mut self.status {
            *s = TimeLineStatus::default();
        }
        for split in self.best_lap_splits.iter_mut().chain(self.last_lap_splits.iter_mut()) {
            *split = 0;
        }
        self.open_track_state = 0;
    }

    /// `TimeTransponder::addCut`
    pub fn add_cut(&mut self) {
        self.cuts += 1;
    }

    /// `TimeTransponder::armFirstLap` (hot-lap sessions)
    pub fn arm_first_lap(&mut self) {
        self.is_first_lap_armed = true;
    }

    /// `TimeTransponder::onTimeLinePassed` @ 0x140290c20. `step_counter` is the engine's.
    fn on_time_line_passed(&mut self, i: usize, is_finish: bool, step_counter: u32, finish: &FinishContext, actions: &mut TransponderActions) {
        self.status[i].is_valid = true;
        // PhysicsEngine::getOddTimeOffset
        let off = (step_counter % 3) as f64;
        let time = (off + self.t as f64) as i64 as u32;
        self.status[i].last_time = time;
        if !is_finish {
            if i == 1 {
                self.current_splits[0] = self.status[1].last_time;
            } else {
                self.current_splits[i - 1] = self.status[i].last_time.wrapping_sub(self.status[i - 1].last_time);
            }
            return;
        }
        if finish.penalty_time > 0.0 {
            actions.black_flag = Some(4);
            self.invalidate();
            return;
        }
        if finish.check_black_flag {
            actions.black_flag = Some(1);
            self.invalidate();
            return;
        }
        self.finish_line_passed = true;
        if self.is_valid() {
            self.was_last_lap_valid = true;
            // the comparison is made with the time before the odd offset
            if self.t < self.best_lap || self.best_lap == 0 {
                self.best_lap = time;
            }
            self.lap_count = self.lap_count.wrapping_add(1);
            self.last_lap = time;
            let n = self.last_lap_splits.len();
            for k in 0..n {
                // a track with one line reads past its vector here in the game
                let at = |j: usize| self.status.get(j).map(|s| s.last_time).unwrap_or(0);
                self.last_lap_splits[k] = if k == 0 {
                    at(1)
                } else if k == n - 1 {
                    at(0).wrapping_sub(at(k))
                } else {
                    at(k + 1).wrapping_sub(at(k))
                };
            }
            self.t = 0;
        } else {
            self.was_last_lap_valid = false;
            if self.lap_count == 0 && self.is_first_lap_armed {
                self.t = 0;
                self.cuts = 0;
            }
        }
        self.ext_invalid = false;
        actions.decrease_pit_penalty_laps = Some(self.last_lap != 0);
    }

    /// `TimeTransponder::lap` @ 0x140290a50
    fn lap(&mut self, valid: bool, physics_time: f64) {
        if self.last_lap == 0 {
            return;
        }
        self.lap_events.push(OnLapCompletedEvent {
            car_index: self.car_index,
            lap_time: self.last_lap,
            lap_count: self.lap_count,
            splits: self.last_lap_splits.clone(),
            event_time: physics_time,
            is_valid: valid,
            cuts: self.cuts,
        });
        for s in &mut self.status {
            *s = TimeLineStatus::default();
        }
        self.cuts = 0;
    }

    /// `TimeTransponder::split` @ 0x140291150
    fn split(&mut self, sector_index: usize) {
        self.split_events.push(OnSectorSplitEvent {
            car_index: self.car_index,
            sector_index: sector_index as i32,
            sector_time: self.current_splits[sector_index],
            cuts: self.cuts,
        });
    }

    /// `TimeTransponder::step` @ 0x1402911f0. `probe` is the hub position of wheel 0 from
    /// this step's tyre step; `race_not_started` freezes the time (a race before its start).
    pub fn step(&mut self, track: &Track, probe: &Vec3f, step_counter: u32, physics_time: f64, race_not_started: bool, finish: &FinishContext) -> TransponderActions {
        let mut actions = TransponderActions::default();
        self.finish_line_passed = false;
        for (i, line) in track.time_lines.iter().enumerate() {
            let r = line.check(probe);
            if i >= self.status.len() {
                // the game crashes on purpose here: lines added after the car was built
                break;
            }
            if r == 2 && self.status[i].last_response == 1 {
                if !self.is_open_track {
                    if line.line_type == 0 {
                        self.on_time_line_passed(i, i == 0, step_counter, finish, &mut actions);
                        let valid = self.was_last_lap_valid && self.is_valid();
                        if i == 0 {
                            self.lap(valid, physics_time);
                        } else {
                            self.split(i - 1);
                        }
                    }
                } else if line.line_type == 1 {
                    self.open_track_state = 1;
                    self.cuts = 0;
                    self.t = 0;
                    self.ext_invalid = false;
                } else if line.line_type == 2 {
                    self.on_time_line_passed(i, true, step_counter, finish, &mut actions);
                    self.lap(true, physics_time);
                    self.open_track_state = 0;
                }
            }
            self.status[i].last_response = r;
        }
        if race_not_started {
            return actions;
        }
        self.t = (self.t as f64 + 3.0) as i64 as u32;
        if self.is_open_track && self.open_track_state == 0 {
            self.cuts = 0;
            self.t = 0;
        }
        actions
    }

    /// The laps and splits since the last call.
    pub fn take_events(&mut self) -> (Vec<OnLapCompletedEvent>, Vec<OnSectorSplitEvent>) {
        (std::mem::take(&mut self.lap_events), std::mem::take(&mut self.split_events))
    }
}

/// What [`LapInvalidator::step`] asks of the car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InvalidatorAction {
    /// `Car::addPenalty(seconds)`
    AddPenalty(f64),
    /// `TimeTransponder::addCut()`
    AddCut,
    /// `Car::clearPenalty()`
    ClearPenalty,
}

/// The car as [`LapInvalidator::step`] sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvalidatorInput<'a> {
    /// `PhysicsEngine::physicsTime`, ms
    pub physics_time: f64,
    /// `Car::lastCollisionWithCarTime`
    pub last_collision_with_car_time: f64,
    /// `Tyre::surfaceDef` of the four tyres after their step
    pub surfaces: [Option<&'a SurfaceDef>; 4],
    /// `PhysicsEngine::penaltyMode`: 0 time penalty, 1 invalidate the lap, 2 (performance
    /// meter), 3 nothing, 4 cut detection
    pub penalty_mode: i32,
    /// `PhysicsEngine::allowedTyresOut`: -1 = no limit
    pub allowed_tyres_out: i32,
    /// `Car::getPenaltyTime()`
    pub penalty_time: f64,
    /// `Car::getSpeed()`, m/s
    pub speed: f32,
    /// The car has a driver's device that does not suppress penalties.
    pub has_controls_provider: bool,
}

/// AC's `LapInvalidator` (`Car::lapInvalidator`; the player's car only).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LapInvalidator {
    /// `collisionSafeTime`: ms without a count after a collision with a car (and at the start)
    pub collision_safe_time: f64,
    /// `currentTyresOut`: -1 until the first step
    pub current_tyres_out: i32,
    pub is_in_penalty_zone: bool,
    pub last_black_flag_time: f32,
}

impl Default for LapInvalidator {
    fn default() -> LapInvalidator {
        LapInvalidator { collision_safe_time: 5000.0, current_tyres_out: -1, is_in_penalty_zone: false, last_black_flag_time: 0.0 }
    }
}

impl LapInvalidator {
    /// `LapInvalidator::step` @ 0x1402c0580 (with `onEnterPenaltyZone` @ 0x1402c03d0, whose
    /// mode 2, the performance-meter penalty, is not ported).
    pub fn step(&mut self, input: &InvalidatorInput) -> Option<InvalidatorAction> {
        if !((input.physics_time - input.last_collision_with_car_time) >= self.collision_safe_time) {
            self.current_tyres_out = 0;
            return None;
        }
        let mut n = 0;
        let mut max_bf = 0.0f32;
        for sd in input.surfaces.iter().flatten() {
            if !sd.is_valid_track {
                n += 1;
                if sd.black_flag_time > max_bf {
                    max_bf = sd.black_flag_time;
                }
            }
        }
        let mode = input.penalty_mode;
        if mode == 0 || mode == 4 {
            // ucomisd / ucomiss + je: a NaN counts as zero
            let zero = if mode == 0 { !(input.penalty_time < 0.0 || input.penalty_time > 0.0) } else { !(max_bf < 0.0 || max_bf > 0.0) };
            if zero {
                self.last_black_flag_time = 0.0;
            }
            if max_bf > self.last_black_flag_time {
                self.last_black_flag_time = max_bf;
                self.is_in_penalty_zone = false;
            }
        }
        let mut action = None;
        let kmh = input.speed * 3.6;
        if !self.is_in_penalty_zone {
            if input.has_controls_provider {
                let allowed = input.allowed_tyres_out;
                if n > allowed && allowed != -1 {
                    // LapInvalidator::onEnterPenaltyZone
                    action = match mode {
                        0 if max_bf > 0.0 && kmh > 35.0 => Some(InvalidatorAction::AddPenalty(max_bf as f64)),
                        1 => Some(InvalidatorAction::AddCut),
                        4 if max_bf > 0.0 && kmh > 35.0 => Some(InvalidatorAction::AddCut),
                        _ => None,
                    };
                    self.is_in_penalty_zone = true;
                }
            }
        } else if n == 0 {
            self.is_in_penalty_zone = false;
        } else if mode == 0 && !(kmh >= 35.0) {
            action = Some(InvalidatorAction::ClearPenalty);
        }
        self.current_tyres_out = n;
        action
    }
}

/// A lap in the lap list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lap {
    pub time: u32,
    pub cuts: i32,
    /// `isValid`: no cuts
    pub is_valid: bool,
    pub splits: Vec<u32>,
}

/// One car's part of the game's `RaceTimingServices` (the lap list the displays and the
/// shared memory read), written after the pseudo-C of `onLapCompleted` @ 0x140145300 and
/// `onSectorSplit` @ 0x140145940.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LapDb {
    pub laps: Vec<Lap>,
    /// The best lap without cuts (time 0: none yet).
    pub best_lap: Lap,
    /// The sector times of the lap being driven.
    pub current_splits: Vec<u32>,
    pub total_time: f64,
    /// The track's number of sectors (its lap / sector lines).
    pub sector_count: usize,
}

impl LapDb {
    pub fn new(sector_count: usize) -> LapDb {
        LapDb { sector_count, ..LapDb::default() }
    }

    /// `RaceTimingServices::onSectorSplit`
    pub fn on_sector_split(&mut self, ev: &OnSectorSplitEvent) {
        let index = ev.sector_index.max(0) as usize;
        if self.current_splits.len() <= index {
            self.current_splits.resize(index + 1, 0);
        }
        self.current_splits[index] = ev.sector_time;
    }

    /// `RaceTimingServices::onLapCompleted`: a lap that missed a line is dropped; a lap with
    /// cuts is listed and never the best.
    pub fn on_lap_completed(&mut self, ev: &OnLapCompletedEvent) {
        if !ev.is_valid {
            self.current_splits.clear();
            return;
        }
        let mut lap = Lap { time: ev.lap_time, cuts: ev.cuts, is_valid: ev.cuts == 0, splits: Vec::new() };
        if self.sector_count > 1 {
            // the last sector is what is left of the lap
            let sum: u32 = self.current_splits.iter().take(self.sector_count - 1).sum();
            self.current_splits.resize(self.sector_count - 1, 0);
            self.current_splits.push(ev.lap_time.wrapping_sub(sum));
            lap.splits = self.current_splits.clone();
        }
        self.total_time += ev.lap_time as f64;
        if ev.cuts == 0 && (self.best_lap.time == 0 || lap.time < self.best_lap.time) {
            self.best_lap = lap.clone();
        }
        self.laps.push(lap);
        self.current_splits.clear();
    }

    /// `RaceManager::getLastLap`: the last listed lap, or an empty one.
    pub fn last_lap(&self) -> Lap {
        self.laps.last().cloned().unwrap_or_default()
    }
}

/// `timeToString(ms, 3)` @ 0x140053110: `m:ss:mmm`, or `-:--:---` for no time.
pub fn time_to_string(ms: i32) -> String {
    if ms <= 0 {
        return "-:--:---".to_string();
    }
    let m = ms / 60000;
    let s = (ms - m * 60000) / 1000;
    format!("{m}:{s:02}:{:03}", ms - m * 60000 - s * 1000)
}

/// `TrackAvatar::initRespawnPositionSet` @ 0x1401cb8c0: the nodes `AC_<set>_0`, `AC_<set>_1`
/// ... up to the first missing number; each is dropped onto the road with a ray from 10 m
/// above (the node itself moves, so a second call casts from the new place), and joins the set.
pub fn init_respawn_position_set(track: &mut Track, set: &str) {
    for n in 0.. {
        let name = format!("AC_{set}_{n}");
        let Some(index) = track.helper_nodes.iter().position(|h| h.name == name) else { break };
        let m = track.helper_nodes[index].local.m;
        let org = Vec3f::new(m[3][0], m[3][1] + 10.0, m[3][2]);
        if let Some(hit) = track.ray_cast(&org, &Vec3f::new(0.0, -1.0, 0.0), 100.0) {
            let node = &mut track.helper_nodes[index].local.m[3];
            node[0] = hit.pos.x;
            node[1] = hit.pos.y;
            node[2] = hit.pos.z;
        }
        track.spawn_positions.entry(set.to_string()).or_default().push(index);
    }
}

/// `TrackAvatar::initTimeLines` @ 0x1401cbc90
pub fn init_time_lines(track: &mut Track, messages: &mut Vec<String>) {
    let find = |track: &Track, name: &str| -> Option<Vec3f> {
        let m = track.helper_nodes.iter().find(|h| h.name == name)?.local.m;
        Some(Vec3f::new(m[3][0], m[3][1], m[3][2]))
    };
    for i in 0.. {
        let (Some(left), Some(right)) = (find(track, &format!("AC_TIME_{i}_L")), find(track, &format!("AC_TIME_{i}_R"))) else { break };
        track.add_time_line(&left, &right, 0);
    }
    let ab = ["AC_AB_START_L", "AC_AB_START_R", "AC_AB_FINISH_L", "AC_AB_FINISH_R"].map(|name| find(track, name));
    if let [Some(sl), Some(sr), Some(fl), Some(fr)] = ab {
        messages.push("TRACK IS AN OPENTRACK".to_string());
        track.add_time_line(&sl, &sr, 1);
        track.add_time_line(&fl, &fr, 2);
    }
    if let (Some(left), Some(right)) = (find(track, "AC_OPEN_FINISH_L"), find(track, "AC_OPEN_FINISH_R")) {
        track.add_time_line(&left, &right, 0);
    }
}

impl Track {
    /// `Track::addTimeLine` @ 0x140278040: the line, and for a lap / sector line where its
    /// middle is along the AI line.
    pub fn add_time_line(&mut self, p0: &Vec3f, p1: &Vec3f, line_type: i32) {
        let id = self.time_lines.len() as i32;
        self.time_lines.push(TimeLine::new(p0, p1, id, line_type));
        if (line_type.wrapping_sub(1) as u32) <= 1 {
            self.is_open = true;
        } else {
            let mid = [(p1.x + p0.x) * 0.5, (p1.y + p0.y) * 0.5, (p1.z + p0.z) * 0.5];
            // the game reads the AI line unconditionally; a track without one has no sectors here
            if let Some(spline) = &self.ai_spline {
                let n = spline.spline.world_to_spline(&mid, -1);
                self.sectors_normalized_positions.push(n);
            }
        }
    }

    /// `TrackAvatar::getSpawnPosition` @ 0x1401c8200 as `CarAvatar::forcePosition` @
    /// 0x1400d2f30 hands it to the car: the point on the road and minus the node's z row,
    /// the direction of the car's tail (what `Car::forceRotation` takes).
    pub fn spawn_pose(&self, set: &str, index: usize) -> Option<(Vec3f, Vec3f)> {
        let node = *self.spawn_positions.get(set)?.get(index)?;
        let m = self.helper_nodes[node].local.m;
        Some((Vec3f::new(m[3][0], m[3][1], m[3][2]), Vec3f::new(-m[2][0], -m[2][1], -m[2][2])))
    }

    /// How many slots a spawn set has (`TrackAvatar::getMaxSlotsAvailable`).
    pub fn spawn_slots(&self, set: &str) -> usize {
        self.spawn_positions.get(set).map(Vec::len).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track with a start line across x at z = 0 and one sector line at z = 100, driven
    /// towards +z; the left end of a gate is at +x.
    fn track() -> Track {
        let mut track = Track::default();
        track.add_time_line(&Vec3f::new(8.0, 3.0, 0.0), &Vec3f::new(-8.0, 5.0, 0.0), 0);
        track.add_time_line(&Vec3f::new(8.0, 0.0, 100.0), &Vec3f::new(-8.0, 0.0, 100.0), 0);
        track
    }

    #[test]
    fn a_gate_is_level_and_knows_which_side_a_point_is_on() {
        let line = TimeLine::new(&Vec3f::new(8.0, 3.0, 0.0), &Vec3f::new(-8.0, 5.0, 0.0), 0, 0);
        assert_eq!((line.points[0].y, line.points[1].y, line.length), (0.0, 0.0, 256.0));
        assert_eq!((line.plane_normal.x, line.plane_normal.z), (0.0, 1.0));
        assert_eq!(line.check(&Vec3f::new(0.0, 50.0, -1.0)), 1);
        assert_eq!(line.check(&Vec3f::new(0.0, -7.0, 0.0)), 1);
        assert_eq!(line.check(&Vec3f::new(0.0, 0.0, 0.001)), 2);
        // out of reach of one end
        assert_eq!(line.check(&Vec3f::new(9.0, 0.0, 0.5)), 0);
        assert_eq!(line.check(&Vec3f::new(0.0, 0.0, 14.0)), 0);
    }

    /// Drives along z from `from` to `to` in steps of 0.5 m; returns the laps.
    fn drive(tp: &mut TimeTransponder, track: &Track, from: f32, to: f32, counter: &mut u32) {
        let mut z = from;
        while z < to {
            *counter += 1;
            tp.step(track, &Vec3f::new(1.0, 0.3, z), *counter, 0.0, false, &FinishContext::default());
            z += 0.5;
        }
    }

    #[test]
    fn a_lap_needs_every_line_and_the_time_counts_three_per_step() {
        let track = track();
        let mut tp = TimeTransponder::new(&track, 0);
        let mut counter = 0;
        // the first crossing of the finish line: the sector line is missing, no lap; the time runs on
        drive(&mut tp, &track, -5.0, 5.0, &mut counter);
        assert_eq!((tp.lap_count, tp.last_lap, tp.t), (0, 0, 60));
        assert!(tp.status[0].is_valid && tp.lap_events.is_empty());
        // the sector line
        drive(&mut tp, &track, 95.0, 105.0, &mut counter);
        assert_eq!(tp.split_events.len(), 1);
        assert!(tp.status[1].is_valid);
        // the finish line again: a lap of everything since the start
        drive(&mut tp, &track, -5.0, 5.0, &mut counter);
        assert_eq!(tp.lap_count, 1);
        // the crossing is seen in the step after the one that stood on the line
        let crossing_step = 20 + 20 + 12;
        assert_eq!(tp.last_lap, 3 * (crossing_step - 1) + crossing_step % 3);
        assert_eq!(tp.best_lap, tp.last_lap);
        assert_eq!(tp.lap_events.len(), 1);
        assert_eq!(tp.t, 3 * 9);
        // a second lap, with its splits
        drive(&mut tp, &track, 95.0, 105.0, &mut counter);
        drive(&mut tp, &track, -5.0, 5.0, &mut counter);
        assert_eq!(tp.lap_count, 2);
        let (laps, splits) = tp.take_events();
        assert_eq!((laps.len(), splits.len()), (2, 2));
        assert!(laps[1].is_valid && laps[1].lap_time == tp.last_lap && laps[1].splits.len() == 2);
        assert_eq!(laps[1].splits[0] + laps[1].splits[1], tp.last_lap);
    }

    #[test]
    fn an_armed_first_lap_starts_at_the_line_and_a_teleport_spoils_the_lap() {
        let track = track();
        let mut tp = TimeTransponder::new(&track, 0);
        tp.arm_first_lap();
        let mut counter = 0;
        drive(&mut tp, &track, -5.0, 5.0, &mut counter);
        // the crossing was in step 11: nine steps have run since
        assert_eq!((tp.lap_count, tp.t), (0, 27));
        // jumping over the sector line: the next crossing of the finish is not a lap
        tp.invalidate();
        drive(&mut tp, &track, -5.0, 5.0, &mut counter);
        assert_eq!(tp.lap_count, 0);
        assert!(!tp.was_last_lap_valid);
    }

    #[test]
    fn more_than_the_allowed_tyres_off_the_track_is_a_cut_once() {
        let road = SurfaceDef::default();
        let grass = SurfaceDef { is_valid_track: false, ..SurfaceDef::default() };
        let mut invalidator = LapInvalidator::default();
        let mut input = InvalidatorInput {
            physics_time: 1000.0,
            last_collision_with_car_time: 0.0,
            surfaces: [Some(&grass), Some(&grass), Some(&grass), Some(&road)],
            penalty_mode: 1,
            allowed_tyres_out: 2,
            penalty_time: 0.0,
            speed: 50.0,
            has_controls_provider: true,
        };
        // the first five seconds nothing counts
        assert_eq!(invalidator.step(&input), None);
        assert_eq!(invalidator.current_tyres_out, 0);
        input.physics_time = 6000.0;
        assert_eq!(invalidator.step(&input), Some(InvalidatorAction::AddCut));
        assert_eq!(invalidator.current_tyres_out, 3);
        // still out: no second cut
        assert_eq!(invalidator.step(&input), None);
        // two tyres back is not enough to arm it again
        input.surfaces = [Some(&grass), Some(&road), Some(&road), Some(&road)];
        assert_eq!(invalidator.step(&input), None);
        assert!(invalidator.is_in_penalty_zone);
        input.surfaces = [Some(&road); 4];
        assert_eq!(invalidator.step(&input), None);
        assert!(!invalidator.is_in_penalty_zone);
        // two tyres out is allowed
        input.surfaces = [Some(&grass), Some(&grass), Some(&road), None];
        assert_eq!(invalidator.step(&input), None);
        assert_eq!(invalidator.current_tyres_out, 2);
    }

    #[test]
    fn the_lap_list_keeps_a_lap_with_cuts_but_not_as_the_best() {
        let mut db = LapDb::new(2);
        db.on_sector_split(&OnSectorSplitEvent { sector_index: 0, sector_time: 40_000, ..Default::default() });
        db.on_lap_completed(&OnLapCompletedEvent { lap_time: 100_000, is_valid: true, cuts: 1, ..Default::default() });
        assert_eq!((db.laps.len(), db.best_lap.time), (1, 0));
        assert_eq!(db.laps[0].splits, vec![40_000, 60_000]);
        db.on_lap_completed(&OnLapCompletedEvent { lap_time: 101_000, is_valid: true, cuts: 0, ..Default::default() });
        assert_eq!(db.best_lap.time, 101_000);
        // a lap that missed a line is not listed
        db.on_lap_completed(&OnLapCompletedEvent { lap_time: 90_000, is_valid: false, cuts: 0, ..Default::default() });
        assert_eq!(db.laps.len(), 2);
        assert_eq!(time_to_string(101_234), "1:41:234");
        assert_eq!(time_to_string(0), "-:--:---");
    }
}
