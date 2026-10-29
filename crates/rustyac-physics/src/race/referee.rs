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
use super::pitstop::{self, PitAction, PitCarView, PitFrame, PitStop, PitStopStart, QuickMenu};
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
    /// `startRace`'s delay for the first session, ms (20000 in the game, which is what
    /// `Sim::Sim` calls it with when the session does not begin in the ESC menu). Negative:
    /// the session begins in the ESC menu, and `startRace` waits for its "Drive"
    /// ([`Referee::drive`]), as in the game.
    pub first_start_delay_ms: f32,
    /// and for a race that follows another session (10000: the click that ends the intro);
    /// negative: that session waits for "Drive" too
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
    /// what the main thread posted for the engine on the physics thread: it runs at the
    /// start of the next step, in this order, before the cars' own queued jobs
    pending: Vec<EngineJob>,
    /// Task 28b: the player's pit stop (`Sim::pitStop`) and the menu it reads; `None`: a
    /// session without them (older recordings)
    pub pit: Option<PitStop>,
    pub menu: Option<QuickMenu>,
    /// what the player's `CarAvatar` keeps of its pit lane visits
    pub pit_avatar: PitAvatar,
    /// the mandatory pit window: laps of the leader (minutes in a timed race) at which it
    /// opens and closes; (0, 0): none, nothing is checked
    pub pit_window: (u16, u16),
    /// how much the player's car points against the road (the last frame's `FlagCar`): the
    /// wrong-way sign reads it
    pub player_against_road: f32,
    /// how often `Sim::evOnNewSession` has fired (a restart fires it too): the ESC menu that a
    /// session begins in comes up at each
    pub new_session_count: u32,
    /// the stops that began and ended in the last frame (`evPitStopCommit`, `evPitStopOver`):
    /// the pit crew and the sounds listen
    pub pit_began: Option<PitStopStart>,
    pub pit_over: bool,
    /// the player's stop was counted as the mandatory one in the last frame (a server is told)
    pub mandatory_pit_now: bool,
    /// proofs only: the stop is fed made-up states instead of the car's
    pub pit_feed: Option<pitstop::PitFeed>,
    /// the mouse menu's stop turned the player's mouse steering on or off (for the game to
    /// pass on to the keyboard's controls)
    pub mouse_steering: Option<bool>,
    /// proofs only: what `PitStop::getEstimatedTime` said of the last fed frame
    pub pit_estimate: f32,
}

/// What `CarAvatar` keeps of the player's car for the mandatory pit stop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitAvatar {
    /// `mandatoryPitstopDone`
    pub mandatory_pitstop_done: bool,
    /// `lastPitstopTime`: the clock (ms, as a float) of the last frame a stop's countdown ran
    pub last_pitstop_time: f32,
    /// `inPitlane`, `wasInPitlane`
    pub in_pitlane: bool,
    pub was_in_pitlane: bool,
    /// `pitLaneEntryTime`, `pitLaneExitTime`
    pub pit_lane_entry_time: f32,
    pub pit_lane_exit_time: f32,
}

impl PitAvatar {
    pub fn text(&self) -> String {
        format!("done {} last {:08x} in {}{} entry {:08x} exit {:08x}", self.mandatory_pitstop_done as u8, self.last_pitstop_time.to_bits(), self.in_pitlane as u8, self.was_in_pitlane as u8, self.pit_lane_entry_time.to_bits(), self.pit_lane_exit_time.to_bits())
    }
}

/// A job for the physics thread that is the engine's, not a car's.
#[derive(Clone, Copy, Debug)]
enum EngineJob {
    /// `PhysicsEngine::setSessionInfo` and its handlers (lambda @ 0x1401213f0)
    Session(SessionInfo),
    /// `PhysicsEngine::setCurrentSessionStartTime` (lambda @ 0x140135220)
    StartTime(f64),
    /// the new session's wind (lambda @ 0x140133ac0)
    Wind,
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
        let length = track.ai_line().map_or(0.0, |spline| spline.length());
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
            pending: Vec::new(),
            pit: None,
            menu: None,
            pit_avatar: PitAvatar::default(),
            pit_window: (0, 0),
            player_against_road: 0.0,
            new_session_count: 0,
            pit_began: None,
            pit_over: false,
            mandatory_pit_now: false,
            pit_feed: None,
            mouse_steering: None,
            pit_estimate: 0.0,
            manager,
        };
        let mut actions = Vec::new();
        referee.manager.set_current_session(0, now, &mut actions);
        referee.manager.reset_current_laps_and_mandatory_pit(&mut actions);
        referee.apply(actions, world);
        if setup.first_start_delay_ms >= 0.0 {
            referee.start_race(setup.first_start_delay_ms, world);
        } else {
            // ESCMenu::render: the player's controls are locked while the menu shows
            world.car_mut(0).queue(|car| car.lock_controls(true));
        }
        referee
    }

    /// Task 28b: the player's pit stop joins (`PitStop::PitStop` in `Sim::Sim`, for the pit
    /// box of the first car) with the menu it reads. As the first session's start leaves
    /// it: undefined until the car has been out of its box.
    pub fn enable_pit_stop(&mut self, menu: QuickMenu, world: &dyn World) {
        let m = &world.car(0).pit_position.m;
        let mut pit = PitStop::new([m[3][0], m[3][1], m[3][2]]);
        pit.state = pitstop::STATE_UNDEFINED;
        self.pit = Some(pit);
        self.menu = Some(menu);
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
        let against_road = match self.track.ai_line().filter(|spline| spline.point_count() >= 2) {
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

    /// Before every physics step: the engine's jobs the main thread posted since the last one.
    pub fn before_step(&mut self, world: &mut dyn World) {
        for job in std::mem::take(&mut self.pending) {
            match job {
                EngineJob::Session(info) => world.set_session_info(info),
                EngineJob::StartTime(ms) => world.set_session_start_time(ms),
                EngineJob::Wind => world.generate_wind(),
            }
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
        self.pit_began = None;
        self.pit_over = false;
        self.mandatory_pit_now = false;
        if self.pit.is_some() && world.car_count() > 0 {
            self.update_in_pitlane_state(world.car(0).is_in_pit_lane(), now);
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
        if new_session && self.manager.current_session.session_type == SESSION_RACE && self.next_start_delay_ms >= 0.0 {
            // (the click that ends the session's intro, at once)
            self.start_race(self.next_start_delay_ms, world);
        }
        if new_session {
            self.new_session_count = self.new_session_count.wrapping_add(1);
        }
        if new_session && self.next_start_delay_ms < 0.0 {
            // the ESC menu comes up again (the handler of the new session, 0x140195c60)
            world.car_mut(0).queue(|car| car.lock_controls(true));
        }
        self.pit_frame(world, now, &views);
        // FlagManager::update and the messages
        let flag_cars: Vec<FlagCar> = (0..world.car_count()).map(|index| self.flag_car(world.car(index))).collect();
        self.player_against_road = flag_cars.first().map_or(0.0, |car| car.against_road);
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
                Action::ResetCar(car) => {
                    world.car_mut(car).queue(|car| {
                        // CarAvatar::lockControlsUntilTime(0, 0, false) and resetPenalty(true)
                        car.lock_controls_until(0.0, 0.0);
                        car.penalty_manager.reset_penalty();
                        car.penalty_manager.reset();
                    });
                    // RaceManager::resetMandatoryPit @ 0x140140600
                    if car == 0 {
                        self.pit_avatar.last_pitstop_time = 0.0;
                        self.pit_avatar.mandatory_pitstop_done = false;
                        self.pit_avatar.pit_lane_entry_time = 0.0;
                        self.pit_avatar.pit_lane_exit_time = 0.0;
                    }
                }
                Action::NewSession { session, index } => {
                    // (the results panel's own button, which the game's player presses to go
                    // on, takes the panel down: EndSessionDisplayer::hide)
                    if self.manager.results_displayed {
                        self.manager.results_displayed = false;
                        world.car_mut(0).queue(|car| car.lock_controls(false));
                    }
                    self.flags.clear_flag();
                    self.pending.push(EngineJob::Session(SessionInfo { session_type: session.session_type, start_time_ms: session.start_time, time_secs: (session.duration_minutes * 60.0) as f64, laps: session.laps, index }));
                    // CarAvatar::onNewSession of every car
                    for car in 0..world.car_count() {
                        let slot = if session.spawn_set == "START" { self.manager.start_index[car] } else { self.pit_index.get(car).copied().unwrap_or(car as i32) };
                        let armed = session.session_type == SESSION_HOTLAP;
                        self.teleport(world, car, &session.spawn_set, slot, Some(armed));
                    }
                    // PitStop's handler of the new session (lambda @ 0x1400a7ce0), after the
                    // cars' (CarAvatar::forcePosition leaves the player's stop undefined too)
                    if let Some(pit) = &mut self.pit {
                        let mut out = Vec::new();
                        pit.on_new_session(&mut out);
                        self.apply_pit(out, world);
                    }
                }
                Action::ControlsLock { car, locked } => world.car_mut(car).queue(move |car| car.lock_controls(locked)),
                Action::GoToPit(car) => {
                    let slot = self.pit_index.get(car).copied().unwrap_or(car as i32);
                    self.teleport(world, car, "PIT", slot, None);
                    // CarAvatar::forcePosition @ 0x1400d2f30 of the player's car
                    if car == 0 {
                        if let Some(pit) = &mut self.pit {
                            pit.state = pitstop::STATE_UNDEFINED;
                        }
                    }
                }
                Action::SessionStartTime(ms) => self.pending.push(EngineJob::StartTime(ms)),
                Action::RetirePlayer => world.car_mut(0).is_retired = true,
                Action::SessionEnd { event, index } => {
                    let cars = world.car_count();
                    self.json.end_session(index, &event, cars, None);
                    self.log.push(format!("{} over", event.current_session.name));
                    self.last_results = Some(event);
                }
                Action::Message { text, description } => self.log.push(format!("{text} {description}")),
                Action::GenerateWind => self.pending.push(EngineJob::Wind),
                Action::DisplayResults => world.car_mut(0).queue(|car| car.lock_controls(true)),
                Action::HideResults => world.car_mut(0).queue(|car| car.lock_controls(false)),
                Action::FadeStarted => {}
            }
        }
    }

    /// `CarAvatar::updateInPitlaneState` @ 0x1400dd6d0 of the player's car: when it came
    /// onto the pit lane and left it, and at leaving, whether its stop was the mandatory one.
    fn update_in_pitlane_state(&mut self, in_pitlane: bool, now: f64) {
        let a = &mut self.pit_avatar;
        a.in_pitlane = in_pitlane;
        if in_pitlane {
            if !a.was_in_pitlane {
                a.was_in_pitlane = true;
                a.pit_lane_entry_time = now as f32;
            }
        } else if a.was_in_pitlane {
            a.was_in_pitlane = false;
            a.pit_lane_exit_time = now as f32;
            if self.manager.check_mandatory_pit(a.mandatory_pitstop_done, a.last_pitstop_time, a.pit_lane_entry_time, a.pit_lane_exit_time) {
                a.mandatory_pitstop_done = true;
                self.mandatory_pit_now = true;
                self.log.push("Mandatory pit stop done".to_string());
            }
        }
    }

    /// The mandatory pit window and `PitStop::update` of a frame.
    fn pit_frame(&mut self, world: &mut dyn World, now: f64, views: &[CarView]) {
        if world.car_count() == 0 {
            return;
        }
        if self.pit_window.1 != 0 && self.manager.update_pit_window(self.pit_window.0, self.pit_window.1, now, self.pit_avatar.mandatory_pitstop_done, self.pit_avatar.in_pitlane) {
            // CarAvatar::setBlackFlag(eMandatoryPit), and the car is out
            world.car_mut(0).queue(|car| car.set_black_flag_for(true, 3));
            world.car_mut(0).is_retired = true;
        }
        let (Some(mut pit), Some(mut menu)) = (self.pit.take(), self.menu.take()) else { return };
        let car = PitCarView::of(world.car(0));
        // (the avatar's flag is the flag manager's, a frame behind the car's)
        let car = PitCarView { is_black_flagged: self.was_black_flagged, ..car };
        let mut frame = PitFrame { dt: FRAME_DT, session_type: self.manager.current_session.session_type, race_over: self.manager.is_race_over(0, views), in_replay: false, pause_menu: false, esc_menu: false };
        let mut car = car;
        if let Some(feed) = &mut self.pit_feed {
            let fed = feed.next(pit.pit_position, &menu, world.car(0).max_fuel as f32);
            car = fed.car;
            frame.dt = fed.dt;
            if let Some((preset, values)) = fed.menu {
                menu.current_preset = preset;
                if let Some(slot) = menu.presets.get_mut(preset as usize) {
                    *slot = values;
                }
            }
            if let Some((zones, life)) = fed.damage {
                let chassis = world.car_mut(0);
                chassis.damage_zone_level = zones;
                if let Some(drivetrain) = &mut chassis.drivetrain {
                    drivetrain.engine_mut().base_mut().life_left = life;
                }
            }
            let chassis = world.car(0);
            if feed.mouse {
                // Task 34: a click on the old mouse menu, then what it would take
                let mut out = Vec::new();
                let compounds = pit.mouse.as_ref().map_or(0, |mouse| mouse.compounds.len());
                let click = match feed.mouse_click(compounds) {
                    Some(pitstop::FeedClick::Button(click)) => Some(click),
                    Some(pitstop::FeedClick::Gauge(x, y)) => pitstop::fuel_gauge_share(pitstop::FEED_GAUGE.0, pitstop::FEED_GAUGE.1, x, y).map(pitstop::MousePitClick::FuelGauge),
                    None => None,
                };
                if let Some(click) = click {
                    let mut rand = self.rand;
                    pit.mouse_click(click, chassis, &mut rand, &mut out);
                    self.rand = rand;
                }
                self.pit_estimate = pit.mouse.as_ref().map_or(0.0, |mouse| mouse.estimated_time(chassis));
                self.apply_pit(out, world);
            } else {
                let (share, tyres, body, engine, suspension) = fed.estimate;
                self.pit_estimate = pitstop::get_estimated_time(chassis, chassis.max_fuel as f32, share, tyres, body, engine, suspension);
            }
        }
        let mut out = Vec::new();
        let mut rand = self.rand;
        pit.update(&frame, &car, &mut menu, world.car(0), &mut rand, &mut out);
        self.rand = rand;
        if pit.timer_ran {
            self.pit_avatar.last_pitstop_time = now as f32;
        }
        // QuickMenuPitstop::renderHUD: the menu's coming and going
        let allowed = (1..=3).contains(&frame.session_type) && !self.manager.results_displayed && pit.app_interaction_enabled;
        menu.frame(FRAME_DT, car.is_in_pitlane, allowed);
        self.pit = Some(pit);
        self.menu = Some(menu);
        self.apply_pit(out, world);
    }

    /// An arrow key for the pit menu (`Sim::onKeyDown` -> `QuickMenuPitstop::sendInput`):
    /// 0x25 left, 0x26 up, 0x27 right, 0x28 down.
    pub fn pit_key(&mut self, key: u32, car: &RollingChassis) {
        if let Some(menu) = &mut self.menu {
            menu.send_input(key, &pitstop::QuickMenuCar::of(car));
        }
    }

    /// A click on the old mouse menu the window passed on (`PitStop::mouse_click`).
    pub fn mouse_pit_click(&mut self, click: pitstop::MousePitClick, world: &mut dyn World) {
        let Some(pit) = &mut self.pit else { return };
        let mut out = Vec::new();
        let mut rand = self.rand;
        pit.mouse_click(click, world.car(0), &mut rand, &mut out);
        self.rand = rand;
        self.apply_pit(out, world);
    }

    fn apply_pit(&mut self, actions: Vec<PitAction>, world: &mut dyn World) {
        for action in actions {
            match action {
                PitAction::ControlsLock(locked) => world.car_mut(0).queue(move |car| car.lock_controls(locked)),
                PitAction::Commit(start) => {
                    self.log.push(format!("Pit stop: {:.1} s", start.total_time));
                    self.pit_began = Some(start);
                }
                PitAction::Over => {
                    self.log.push("Pit stop over".to_string());
                    self.pit_over = true;
                }
                PitAction::Apply(choices) => world.car_mut(0).queue(move |car| pitstop::apply(car, &choices)),
                PitAction::MouseApply(choices) => world.car_mut(0).queue(move |car| pitstop::apply_mouse(car, &choices)),
                PitAction::MouseSteering(on) => self.mouse_steering = Some(on),
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

    /// The pause menu's "Back to pits", once the picture has faded (the lambda @ 0x14011ec90):
    /// outside a race `CarAvatar::goToSpawnPosition("PIT")`; in a race
    /// `CarAvatar::lockControlsUntilTime(20000, now, true)` @ 0x1400d8e20, whose job on the
    /// physics thread (@ 0x1400cf530) locks the controls for 20 s on top of the time a whole
    /// pit stop takes, before the car is put on its box all the same.
    pub fn back_to_pits(&mut self, world: &mut dyn World, now: f64) {
        if self.manager.current_session.session_type == SESSION_RACE {
            world.car_mut(0).queue(move |car| {
                let mut rand = MsvcRand(0);
                let time = car.get_pitstop_time(0.0, true, true, true, true, false, &mut rand);
                car.lock_controls_until((time.total * 1000.0) as f64 + 20000.0, now);
            });
        }
        self.apply(vec![Action::GoToPit(0)], world);
    }

    /// Task 30: the ESC menu shows (`ESCMenu::render` 0x1400fe420 locks the player's controls,
    /// every frame it is drawn).
    pub fn pits_lock(&mut self, world: &mut dyn World) {
        world.car_mut(0).queue(|car| car.lock_controls(true));
    }

    /// Task 30: the ESC menu's "Drive" (the job `Sim::startGame` leaves for the end of the
    /// fade, 0x140195cf0): `startRace(10000)`, and the player's controls are free again.
    pub fn drive(&mut self, world: &mut dyn World) {
        self.start_race(10000.0, world);
        world.car_mut(0).queue(|car| car.lock_controls(false));
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
        "lockUntil {:016x} locked {} black {} retired {} penalty {} {}{} {} {:08x} cuts {} time {:016x} {:016x}",
        car.lock_controls_time.to_bits(),
        car.is_controls_locked as u8,
        car.black_flagged as u8,
        car.is_retired as u8,
        p.pending_penalty_type,
        p.is_serving_pit_penalty as u8,
        p.rearm_pit_penalty as u8,
        p.pit_penalty_laps,
        p.in_pit_meter_counter.to_bits(),
        car.transponder.cuts,
        car.penalty_time.to_bits(),
        car.penalty_time_accumulator.to_bits()
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
        if let (Some(pit), Some(menu)) = (&self.pit, &self.menu) {
            out.push(("pit".to_string(), pit.text()));
            out.push(("pitMenu".to_string(), menu.values_text()));
            out.push(("pitAvatar".to_string(), self.pit_avatar.text()));
            if self.pit_feed.is_some() {
                out.push(("pitEstimate".to_string(), format!("{:08x}", self.pit_estimate.to_bits())));
            }
            if let Some(mouse) = &pit.mouse {
                out.push(("pitMouse".to_string(), mouse.text()));
            }
        }
        out
    }
}
