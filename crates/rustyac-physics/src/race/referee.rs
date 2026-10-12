// SPDX-License-Identifier: GPL-3.0-or-later

//! The referee among the cars: what in the game is the main thread's side of every car
//! (`CarAvatar`: the lap and sector events it hands on, its filtered speed, its pit box) and
//! the jobs the manager queues for the physics thread, around the ported [`RaceManager`],
//! [`FlagManager`] and [`JsonOutputFile`].
//!
//! The game's main thread runs once a picture, whenever that is. Here a "frame" of the
//! referee is every third physics step (9 ms), whatever is drawn: the same session gives the
//! same result on every machine, with any number of worker threads, and in a replay.

// (a NaN falls through the game's `ucomiss` + `je` as it does here)
#![allow(clippy::double_comparisons)]

use std::sync::Arc;

use super::flags::{FlagCar, FlagManager, PlayerPenalty, Shown};
use super::hud::{PenaltyMessages, PlayerState};
use super::manager::{filter_speed, Action, CarView, OnSessionEndEvent, RaceManager, SessionsIni, SESSION_HOTLAP, SESSION_RACE};
use super::output::{JsonOutputFile, Player};
use crate::ai::SessionInfo;
use crate::car::RollingChassis;
use crate::session::MsvcRand;
use crate::track::timing::{OnLapCompletedEvent, OnSectorSplitEvent};
use crate::track::Track;

/// Physics steps per frame of the referee.
pub const FRAME_STEPS: u32 = 3;
/// A frame's length, s.
pub const FRAME_DT: f32 = 0.009;

/// The cars and the engine as the referee needs them.
pub trait World {
    fn car_count(&self) -> usize;
    fn car(&self, index: usize) -> &RollingChassis;
    fn car_mut(&mut self, index: usize) -> &mut RollingChassis;
    /// `PhysicsEngine::setSessionInfo` with its handlers (every car, every AI driver), and no
    /// contacts for 250 steps.
    fn set_session_info(&mut self, info: SessionInfo);
    /// `PhysicsEngine::setCurrentSessionStartTime`.
    fn set_session_start_time(&mut self, ms: f64);
    /// `PhysicsEngine::penaltyMode`.
    fn set_penalty_mode(&mut self, mode: i32);
    /// `RaceManager::generateWind`: the wind of a new session.
    fn generate_wind(&mut self);
}

/// How a session is set up for the referee.
#[derive(Clone, Debug)]
pub struct RefereeSetup {
    pub sessions: SessionsIni,
    /// `OVER_TIME_MULT`
    pub over_time_mult: f32,
    /// every car's slot of the `START` set at the beginning (`CarRaceInfo`)
    pub start_index: Vec<i32>,
    /// every car's slot of the `PIT` set
    pub pit_index: Vec<i32>,
    pub players: Vec<Player>,
    /// the seed of the referee's `rand()` (the start's random second, made-up laps)
    pub seed: u32,
    /// `startRace`'s delay for the first session, ms (20000 in the game)
    pub first_start_delay_ms: f32,
    /// and for a race that follows another session (10000: the click that ends the intro)
    pub next_start_delay_ms: f32,
    /// the track's `data/overlays.ini [YELLOW_FLAG] DISTANCE`
    pub yellow_flag_distance: Option<f32>,
    /// the track's layout folder, for the name in `race_out.json`
    pub layout: String,
}

/// The referee of a session list.
pub struct Referee {
    pub manager: RaceManager,
    pub flags: FlagManager,
    /// the flags on the screen after the last frame
    pub shown: Shown,
    pub json: JsonOutputFile,
    pub messages: PenaltyMessages,
    /// the penalty messages of the last frame
    pub message_lines: Vec<[String; 2]>,
    pub players: Vec<Player>,
    pub rand: MsvcRand,
    /// `CarAvatar::filteredSpeed`, by car
    pub filtered_speed: Vec<f32>,
    pub pit_index: Vec<i32>,
    track: Arc<Track>,
    step_in_frame: u32,
    laps: Vec<OnLapCompletedEvent>,
    splits: Vec<OnSectorSplitEvent>,
    /// the last session's end, for the results table
    pub last_results: Option<OnSessionEndEvent>,
    /// `SystemMessage` lines and what else the referee said, oldest first
    pub log: Vec<String>,
    /// the views of the last frame
    pub views: Vec<CarView>,
    next_start_delay_ms: f32,
    /// the player's car was black-flagged when the last frame looked
    was_black_flagged: bool,
    /// frames run
    pub frames: u64,
    /// every action of the manager with the frame it came in (kept for proofs)
    pub trace: Option<Vec<(u64, Action)>>,
    /// a hash of the whole state ([`Referee::dump`]) is taken at the end of every frame
    pub hash_frames: bool,
    pub state_hash: u64,
}

impl Referee {
    /// The manager and its first session (`RaceManager::initOffline` from the timing
    /// services on, and `Sim::Sim`'s `startRace`). `now`: the clock, ms.
    pub fn new(setup: RefereeSetup, track: Arc<Track>, world: &mut dyn World, now: f64) -> Referee {
        let cars = world.car_count();
        let mut manager = RaceManager::new(&setup.sessions, cars, track.sectors_normalized_positions.len(), setup.over_time_mult);
        for (slot, index) in manager.start_index.iter_mut().zip(&setup.start_index) {
            *slot = *index;
        }
        let length = track.ai_spline.as_ref().map_or(0.0, |spline| spline.length());
        let json = JsonOutputFile::new(&track.name, &setup.layout, setup.sessions.sessions.len() as i32);
        let mut referee = Referee {
            flags: FlagManager::new(cars, length, setup.yellow_flag_distance),
            shown: Shown::default(),
            json,
            messages: PenaltyMessages::default(),
            message_lines: Vec::new(),
            players: setup.players.clone(),
            rand: MsvcRand(setup.seed),
            filtered_speed: vec![0.0; cars],
            pit_index: setup.pit_index.clone(),
            track,
            step_in_frame: 0,
            laps: Vec::new(),
            splits: Vec::new(),
            last_results: None,
            log: Vec::new(),
            views: Vec::new(),
            next_start_delay_ms: setup.next_start_delay_ms,
            was_black_flagged: false,
            frames: 0,
            trace: None,
            hash_frames: false,
            state_hash: 0,
            manager,
        };
        let mut actions = Vec::new();
        referee.manager.set_current_session(0, now, &mut actions);
        referee.manager.reset_current_laps_and_mandatory_pit(&mut actions);
        referee.apply(actions, world);
        referee.start_race(setup.first_start_delay_ms, world);
        referee
    }

    fn start_race(&mut self, delay_ms: f32, world: &mut dyn World) {
        let mut actions = Vec::new();
        let draw = if self.manager.start_race_draws() { self.rand.next() } else { 0 };
        self.manager.start_race(delay_ms, draw, &mut actions);
        self.apply(actions, world);
    }

    /// A car as the manager sees it.
    fn view(&self, index: usize, car: &RollingChassis) -> CarView {
        let position = car.core.get_position(car.body);
        let pit = &car.pit_position.m;
        let (dx, dy, dz) = (position.x - pit[3][0], position.y - pit[3][1], position.z - pit[3][2]);
        // CarAvatar::isInPit @ 0x1400d8bd0
        let squared = dy * dy + dx * dx + dz * dz;
        let distance = if squared < 0.0 || squared > 0.0 { crate::math::sqrtf(squared) } else { 0.0 };
        let compound = car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index.max(0) as usize)).map(|def| def.short_name.clone()).unwrap_or_default();
        CarView {
            normalized_spline_position: car.spline_locator_data.npos,
            is_in_pit: distance < 1.5,
            filtered_speed: self.filtered_speed[index],
            is_retired: Some(car.is_retired),
            is_connected: true,
            // (the game's flag of the avatar is only ever set for the player's car)
            is_black_flagged: index == 0 && car.black_flagged,
            loaded_tyres: Some(car.tyres.iter().filter(|tyre| tyre.status.load > 0.0).count() as u32),
            compound,
            driver_name: self.players.get(index).map(|p| p.name.clone()).unwrap_or_default(),
            gui_name: self.players.get(index).map(|p| p.car.clone()).unwrap_or_default(),
        }
    }

    fn flag_car(&self, car: &RollingChassis) -> FlagCar {
        let m = car.core.get_world_matrix(car.body).m;
        // CarAvatar::getRoadDirection @ 0x1400d3630 (Spline::getSplineDirection @ 0x1401ee1d0,
        // with the point the car's own locator is at for the nearest one)
        let against_road = match self.track.ai_spline.as_ref().filter(|spline| spline.point_count() >= 2) {
            Some(spline) => {
                let count = spline.point_count() as u32;
                let mut from = car.spline_locator_data.current_index.min(count - 1);
                let to = (from + 1) % count;
                if to == from {
                    from = from.wrapping_sub(1).min(count - 1);
                }
                let (a, b) = (spline.spline.point_at(from), spline.spline.point_at(to));
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let length = crate::math::sqrtf(d[1] * d[1] + d[0] * d[0] + d[2] * d[2]);
                let k = if length < 0.0 || length > 0.0 { 1.0 / length } else { 1.0 };
                d[0] * k * -m[2][0] + d[1] * k * -m[2][1] + d[2] * k * -m[2][2]
            }
            None => 0.0,
        };
        FlagCar {
            is_in_pitlane: car.is_in_pit_lane(),
            tyres_off_track: car.tyres.iter().filter(|tyre| tyre.surface_def.as_ref().is_none_or(|surface| !surface.is_valid_track)).count() as u32,
            gear: car.drivetrain.as_ref().map_or(1, |drivetrain| drivetrain.base().current_gear),
            speed: car.speed,
            against_road,
        }
    }

    /// The player's car as the messages read it.
    pub fn player_state(car: &RollingChassis) -> PlayerState {
        PlayerState {
            penalty_time: car.get_penalty_time(),
            min_speed_clear_disabled: car.disable_min_speed_penalty_clear,
            lock_controls_time: if car.lock_controls_time > car.physics_time { car.lock_controls_time - car.physics_time } else { 0.0 },
            pit_penalty_laps: car.penalty_manager.pit_penalty_laps,
            current_lap_valid: car.transponder.cuts == 0,
            is_black_flagged: car.black_flagged,
        }
    }

    /// A car's lap and sector events of the last step (`Car::evOnLapCompleted` /
    /// `evOnSectorSplit`, which `CarAvatar` queues for the main thread).
    pub fn events(&mut self, car: usize, laps: &[OnLapCompletedEvent], splits: &[OnSectorSplitEvent]) {
        self.laps.extend(laps.iter().cloned().map(|mut lap| {
            lap.car_index = car as u32;
            lap
        }));
        self.splits.extend(splits.iter().map(|split| OnSectorSplitEvent { car_index: car as u32, ..*split }));
    }

    /// After every physics step, at the step's clock, once the cars' events of the step were
    /// handed in ([`Referee::events`]): every [`FRAME_STEPS`] steps a frame of the game's
    /// main thread runs.
    pub fn after_step(&mut self, world: &mut dyn World, now: f64) {
        self.step_in_frame += 1;
        if self.step_in_frame < FRAME_STEPS {
            return;
        }
        self.step_in_frame = 0;
        self.frame(world, now);
    }

    fn views_of(&mut self, world: &dyn World) -> Vec<CarView> {
        (0..world.car_count()).map(|index| self.view(index, world.car(index))).collect()
    }

    /// One frame of the main thread.
    pub fn frame(&mut self, world: &mut dyn World, now: f64) {
        self.frames += 1;
        self.manager.now = now;
        // CarAvatar::update of every car: its filtered speed, then its queued events
        for index in 0..world.car_count() {
            self.filtered_speed[index] = filter_speed(self.filtered_speed[index], world.car(index).speed, FRAME_DT);
        }
        let mut views = self.views_of(world);
        for split in std::mem::take(&mut self.splits) {
            self.manager.on_sector_split(&split);
        }
        for lap in std::mem::take(&mut self.laps) {
            self.manager.on_lap_completed(&lap, &views);
        }
        // the black flag's event (PhysicsEngine::evOnFlagEvent), for the player's car
        if let Some(car) = (world.car_count() > 0).then(|| world.car(0)) {
            if car.black_flagged != self.was_black_flagged {
                self.was_black_flagged = car.black_flagged;
                self.flags.on_black_flag(car.black_flagged, car.black_flag_description);
                if let Some(view) = views.first_mut() {
                    view.is_black_flagged = car.black_flagged;
                }
            }
        }
        let mut actions = Vec::new();
        let mut rand = self.rand;
        self.manager.update(FRAME_DT, now, &views, &mut || rand.next(), &mut actions);
        self.rand = rand;
        let new_session = actions.iter().any(|action| matches!(action, Action::NewSession { .. }));
        self.apply(actions, world);
        if new_session && self.manager.current_session.session_type == SESSION_RACE {
            // (the click that ends the session's intro, at once)
            self.start_race(self.next_start_delay_ms, world);
        }
        // FlagManager::update and the messages
        let flag_cars: Vec<FlagCar> = (0..world.car_count()).map(|index| self.flag_car(world.car(index))).collect();
        if world.car_count() > 0 {
            let player = Self::player_state(world.car(0));
            let penalty = PlayerPenalty { penalty_time: player.penalty_time, pit_penalty_laps: player.pit_penalty_laps };
            self.shown = self.flags.update(FRAME_DT, &self.manager, &views, &flag_cars, &penalty, self.manager.results_displayed);
            self.message_lines = self.messages.frame(FRAME_DT, self.manager.current_session.session_type, &player, false);
        }
        self.views = views;
        if self.hash_frames {
            self.state_hash = dump_hash(&self.dump(world));
        }
    }

    fn apply(&mut self, actions: Vec<Action>, world: &mut dyn World) {
        for action in actions {
            if let Some(trace) = &mut self.trace {
                trace.push((self.frames, action.clone()));
            }
            match action {
                Action::PenaltyMode(mode) => world.set_penalty_mode(mode),
                Action::ResetCar(car) => world.car_mut(car).queue(|car| {
                    // CarAvatar::lockControlsUntilTime(0, 0, false) and resetPenalty(true)
                    car.lock_controls_until(0.0, 0.0);
                    car.penalty_manager.reset_penalty();
                    car.penalty_manager.reset();
                }),
                Action::NewSession { session, index } => {
                    // (the results panel's own button, which the game's player presses to go
                    // on, takes the panel down: EndSessionDisplayer::hide)
                    if self.manager.results_displayed {
                        self.manager.results_displayed = false;
                        world.car_mut(0).queue(|car| car.lock_controls(false));
                    }
                    self.flags.clear_flag();
                    world.set_session_info(SessionInfo { session_type: session.session_type, start_time_ms: session.start_time, time_secs: (session.duration_minutes * 60.0) as f64, laps: session.laps, index });
                    // CarAvatar::onNewSession of every car
                    for car in 0..world.car_count() {
                        let slot = if session.spawn_set == "START" { self.manager.start_index[car] } else { self.pit_index.get(car).copied().unwrap_or(car as i32) };
                        let armed = session.session_type == SESSION_HOTLAP;
                        self.teleport(world, car, &session.spawn_set, slot, Some(armed));
                    }
                }
                Action::ControlsLock { car, locked } => world.car_mut(car).queue(move |car| car.lock_controls(locked)),
                Action::GoToPit(car) => {
                    let slot = self.pit_index.get(car).copied().unwrap_or(car as i32);
                    self.teleport(world, car, "PIT", slot, None);
                }
                Action::SessionStartTime(ms) => world.set_session_start_time(ms),
                Action::RetirePlayer => world.car_mut(0).is_retired = true,
                Action::SessionEnd { event, index } => {
                    let cars = world.car_count();
                    self.json.end_session(index, &event, cars, None);
                    self.log.push(format!("{} over", event.current_session.name));
                    self.last_results = Some(event);
                }
                Action::Message { text, description } => self.log.push(format!("{text} {description}")),
                Action::GenerateWind => world.generate_wind(),
                Action::DisplayResults => world.car_mut(0).queue(|car| car.lock_controls(true)),
                Action::HideResults => world.car_mut(0).queue(|car| car.lock_controls(false)),
                Action::FadeStarted => {}
            }
        }
    }

    /// `CarAvatar::goToSpawnPosition`: the car on its slot of a spawn set, repaired
    /// (`CarAvatar::forcePosition`'s job on the physics thread). `new_session`: the lap timer
    /// starts again too (`CarAvatar::onNewSession`), armed for a hot lap or not.
    fn teleport(&mut self, world: &mut dyn World, car: usize, set: &str, slot: i32, new_session: Option<bool>) {
        let Some((position, tail)) = self.track.spawn_pose(set, slot.max(0) as usize) else {
            if let Some(armed) = new_session {
                world.car_mut(car).queue(move |car| {
                    car.transponder.reset();
                    if armed {
                        car.transponder.arm_first_lap();
                    }
                });
            }
            return;
        };
        let chassis = world.car_mut(car);
        chassis.fuel_lap_evaluator.ignore_lap = true;
        chassis.queue(move |car| {
            car.force_rotation(&tail);
            car.force_position(&position);
            car.set_damage_level(0.0);
            car.reset_suspension_damage_level();
            if let Some(armed) = new_session {
                car.transponder.reset();
                if armed {
                    car.transponder.arm_first_lap();
                }
            }
        });
    }

    /// `RaceManager::skipCurrentSession` (the "next session" button).
    pub fn skip_session(&mut self, world: &mut dyn World) {
        let mut actions = Vec::new();
        self.manager.skip_current_session(&mut actions);
        self.apply(actions, world);
    }

    /// `RaceManager::restartCurrentSession`.
    pub fn restart_session(&mut self, world: &mut dyn World) {
        let mut actions = Vec::new();
        self.manager.restart_current_session(&mut actions);
        self.apply(actions, world);
    }

    /// `RaceManager::shutdown` @ 0x140141320 and `Sim::writeOutputJson`: the current
    /// session's results are made if they were not, and the two files' bytes are returned:
    /// `race_out.json` and `laps.ini`.
    pub fn shutdown(&mut self, world: &mut dyn World) -> (Vec<u8>, Vec<u8>) {
        if !self.manager.finalized {
            let views = self.views_of(world);
            let mut actions = Vec::new();
            let mut rand = self.rand;
            self.manager.finalize_current_session(&views, &mut || rand.next(), &mut actions);
            self.rand = rand;
            self.apply(actions, world);
        }
        let best = self.manager.timing.instance_best_times.first().map_or(0, |lap| lap.time as i32);
        (self.json.file(&self.players, best), self.manager.timing.laps_ini().into_bytes())
    }
}

/// A `Lap` in words.
pub fn lap_text(lap: &super::timing::Lap) -> String {
    format!("{}/{:?}/{}/{}/{}", lap.time, lap.splits, lap.cuts, lap.is_valid as u8, lap.compound)
}

/// A session's end in words.
pub fn session_end_text(event: &OnSessionEndEvent) -> String {
    let laps: Vec<String> = event.result.laps.iter().map(|laps| format!("[{}]", laps.iter().map(lap_text).collect::<Vec<_>>().join(" "))).collect();
    let total: Vec<String> = event.result.total.iter().map(|t| format!("{:08x}", t.to_bits())).collect();
    format!(
        "type {} positions {:?} lapCount {:?} total {:?} best [{}] laps {}",
        event.current_session.session_type,
        event.result.positions,
        event.result.lap_count,
        total,
        event.result.best_laps.iter().map(lap_text).collect::<Vec<_>>().join(" "),
        laps.join("")
    )
}

/// Every field of the manager's state under a name, as text: what `tools/car_oracle` reads
/// out of the game's own `RaceManager` and `RaceTimingServices` under the same names
/// (`penalty_mode`: `PhysicsEngine::penaltyMode`).
pub fn dump(manager: &RaceManager, penalty_mode: i32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut put = |key: &str, value: String| out.push((key.to_string(), value));
    put("currentSessionIndex", manager.current_session_index.to_string());
    put("flags", format!("{} {} {}", manager.time_race_ended as u8, manager.is_timed_last_lap as u8, manager.has_additional_lap as u8));
    put("pitWindow", format!("{:08x} {:08x}", manager.pit_window_start_time.to_bits(), manager.pit_window_end_time.to_bits()));
    let s = &manager.current_session;
    put(
        "session",
        format!(
            "type {} timed {} laps {} duration {:08x} overtime {:08x} spawn {:?} name {:?} start {:016x} forced {} over {} leaderLaps {}",
            s.session_type,
            s.is_timed_race as u8,
            s.laps,
            s.duration_minutes.to_bits(),
            s.overtime_ms.to_bits(),
            s.spawn_set,
            s.name,
            s.start_time.to_bits(),
            s.forced_position,
            s.is_over as u8,
            s.leader_completed_laps
        ),
    );
    put("penaltyMode", penalty_mode.to_string());
    put("realTime", manager.cars_real_time_position.iter().map(|d| format!("{}:{}:{:08x}", d.car, d.crossed_for_the_first_time as u8, d.approx_pos.to_bits())).collect::<Vec<_>>().join(" "));
    put("raceStatus", manager.cars_race_status.iter().map(|d| format!("{}:{:08x}:{}:{:08x}", d.car, d.reset_timer.to_bits(), d.retired as u8, d.invalid_state_timer.to_bits())).collect::<Vec<_>>().join(" "));
    for (car, db) in manager.timing.lap_dbs.iter().enumerate() {
        put(
            &format!("lapDB {car}"),
            format!(
                "total {:016x} best {} lastStart {} current {:?} personal {:?} bestLapSplit {} completed {} laps {}",
                db.total_time.to_bits(),
                lap_text(&db.best_lap),
                db.last_lap_start_time,
                db.current_splits,
                db.personal_best_splits,
                db.best_lap_split,
                db.has_completed_last_lap as u8,
                db.laps.iter().map(lap_text).collect::<Vec<_>>().join(" ")
            ),
        );
    }
    put("bestSplits", format!("{:?} global {}", manager.timing.best_splits, manager.timing.global_best_lap));
    let entry = |e: &super::timing::LeaderboardEntry| format!("{}:{:016x}:{:016x}:{}:{}{}{}", e.car, e.total_time.to_bits(), e.best_lap.to_bits(), e.laps, e.is_race_mode as u8, e.is_black_flagged as u8, e.has_completed_last_lap as u8);
    put("leaderboard", manager.timing.leaderboard.iter().map(entry).collect::<Vec<_>>().join(" "));
    put("grid", manager.start_index.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
    put("resultsDisplayed", (manager.results_displayed as u8).to_string());
    for (i, event) in manager.session_history_list.iter().enumerate() {
        put(&format!("history {i}"), session_end_text(event));
    }
    for (i, board) in manager.leaderboard_history.iter().enumerate() {
        put(&format!("leaderboardHistory {i}"), board.iter().map(entry).collect::<Vec<_>>().join(" "));
    }
    out
}

/// What the referee's jobs leave in a car's physics, in words (`Car::lockControlsTime`,
/// `isControlsLocked`, `blackFlagged`, `isRetired`, the penalty manager, the lap's cuts).
pub fn car_text(car: &RollingChassis) -> String {
    let p = &car.penalty_manager;
    format!(
        "lockUntil {:016x} locked {} black {} retired {} penalty {} {}{} {} {:08x} cuts {}",
        car.lock_controls_time.to_bits(),
        car.is_controls_locked as u8,
        car.black_flagged as u8,
        car.is_retired as u8,
        p.pending_penalty_type,
        p.is_serving_pit_penalty as u8,
        p.rearm_pit_penalty as u8,
        p.pit_penalty_laps,
        p.in_pit_meter_counter.to_bits(),
        car.transponder.cuts
    )
}

/// One number for a state in words (FNV-1a).
pub fn dump_hash(dump: &[(String, String)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for (key, value) in dump {
        for byte in key.bytes().chain([0]).chain(value.bytes()).chain([10]) {
            hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

impl Referee {
    /// The referee's whole state among these cars, in words.
    pub fn dump(&self, world: &dyn World) -> Vec<(String, String)> {
        let penalty_mode = if world.car_count() > 0 { world.car(0).env.penalty_mode } else { 0 };
        let mut out = dump(&self.manager, penalty_mode);
        for index in 0..world.car_count() {
            out.push((format!("car {index}"), car_text(world.car(index))));
        }
        out
    }
}
