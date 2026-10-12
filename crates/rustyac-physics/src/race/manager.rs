// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `RaceManager` (RaceManager.obj, 0x218 bytes): the session list, the session clock, the
//! start, when a session is over, the cars' places while they race, and the results.
//!
//! The game's manager is a main-thread object tied to `Sim`. Here it is a state machine: it
//! is told the clock and what it would read of every `CarAvatar` ([`CarView`]), and it
//! answers with what it would do to the rest of the game ([`Action`]), in the game's order.
//! A car is its index in `Sim::cars`. The camera's fade to black before a car is moved (2.5 s
//! in the game) is a timer here.

use super::results;
use super::timing::{BoardCar, Lap, LeaderboardEntry, RaceTimingServices};
use crate::data::ini::IniReader;
use crate::msvc_sort;
use crate::track::timing::OnLapCompletedEvent;

pub const SESSION_PRACTICE: i32 = 1;
pub const SESSION_QUALIFY: i32 = 2;
pub const SESSION_RACE: i32 = 3;
pub const SESSION_HOTLAP: i32 = 4;
pub const SESSION_DRAG: i32 = 7;

/// The length of `ACCameraManager::fadeIn` as the manager asks for it, seconds.
pub const FADE_SECONDS: f32 = 2.5;

/// `Session` (0x70 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub session_type: i32,
    pub is_timed_race: bool,
    /// never written by the game (the manager's own `hasAdditionalLap` is what counts)
    pub has_additional_lap: bool,
    pub laps: i32,
    pub duration_minutes: f32,
    /// -1: not decided yet
    pub overtime_ms: f32,
    /// `spawSet`
    pub spawn_set: String,
    pub name: String,
    /// ms on the game's clock
    pub start_time: f64,
    pub forced_position: i32,
    pub is_over: bool,
    pub leader_completed_laps: u32,
}

impl Default for Session {
    /// `Session::Session` @ 0x140094f70.
    fn default() -> Session {
        Session {
            session_type: SESSION_PRACTICE,
            is_timed_race: false,
            has_additional_lap: false,
            laps: 0,
            duration_minutes: 30.0,
            overtime_ms: -1.0,
            spawn_set: String::new(),
            name: "UNNAMED".to_string(),
            start_time: 0.0,
            forced_position: -1,
            is_over: false,
            leader_completed_laps: 0,
        }
    }
}

/// `SessionResult` (0x78 bytes); everything by car.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionResult {
    pub laps: Vec<Vec<Lap>>,
    pub best_laps: Vec<Lap>,
    /// the cars in finishing order
    pub positions: Vec<i32>,
    pub lap_count: Vec<i32>,
    pub total: Vec<f32>,
}

/// `OnSessionEndEvent` (0xe8 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnSessionEndEvent {
    pub current_session: Session,
    pub result: SessionResult,
}

/// What race.ini says of the sessions and the rules (`RaceManager::initOffline`
/// @ 0x14013a6c0, the part that is not the cars).
#[derive(Clone, Debug, PartialEq)]
pub struct SessionsIni {
    pub sessions: Vec<Session>,
    /// the manager's `hasAdditionalLap`: the LAST race session's `ADDITITIONAL_LAP` (sic)
    pub has_additional_lap: bool,
    /// `[RACE] FIXED_SETUP` > 0
    pub fixed_setup: bool,
    /// `[RACE] PENALTIES` > 0
    pub penalties_enabled: bool,
    /// `PhysicsAvatar::setJumpStartPenaltyMode`, when `[RACE] JUMP_START_PENALTY` is there
    pub jump_start_penalty_mode: Option<i32>,
    /// some `[SESSION_n]` has `TYPE=4`
    pub has_hotlap: bool,
}

impl SessionsIni {
    /// `PhysicsAvatar::setAllowedTyresOut`
    pub fn allowed_tyres_out(&self) -> i32 {
        if self.penalties_enabled {
            2
        } else {
            -1
        }
    }

    /// A session list in short: `p10,q5,r3` (practice 10 min, qualifying 5 min, a race of 3
    /// laps), `rt20` (a race of 20 minutes), `rt20+` (and the lap after), as race.ini would
    /// give it (`cars`: `[RACE] CARS`, the player's grid slot without `STARTING_POSITION`).
    pub fn from_list(list: &str, cars: usize, penalties: bool, jump_start: Option<i32>, base_pit_penalty_laps: i16) -> Result<SessionsIni, String> {
        let wrong = |part: &str| format!("{part:?} is not p<minutes>, q<minutes>, r<laps>, rt<minutes> or rt<minutes>+");
        let mut sessions = Vec::new();
        let mut additional = false;
        let mut last_race_laps = 0;
        for part in list.split(',').map(str::trim).filter(|part| !part.is_empty()) {
            let lower = part.to_ascii_lowercase();
            let session = if let Some(rest) = lower.strip_prefix("rt") {
                let plus = rest.ends_with('+');
                let minutes: i32 = rest.trim_end_matches('+').parse().map_err(|_| wrong(part))?;
                additional = plus;
                last_race_laps = 0;
                Session { session_type: SESSION_RACE, name: "Race".to_string(), laps: 0, duration_minutes: minutes as f32, is_timed_race: true, spawn_set: "START".to_string(), forced_position: cars as i32, ..Session::default() }
            } else if let Some(rest) = lower.strip_prefix('r') {
                let laps: i32 = rest.parse().map_err(|_| wrong(part))?;
                if laps < 1 {
                    return Err(wrong(part));
                }
                additional = false;
                last_race_laps = laps;
                Session { session_type: SESSION_RACE, name: "Race".to_string(), laps, duration_minutes: 0.0, spawn_set: "START".to_string(), forced_position: cars as i32, ..Session::default() }
            } else if let Some(rest) = lower.strip_prefix('p') {
                Session { session_type: SESSION_PRACTICE, name: "Practice".to_string(), duration_minutes: rest.parse::<i32>().map_err(|_| wrong(part))? as f32, spawn_set: "PIT".to_string(), ..Session::default() }
            } else if let Some(rest) = lower.strip_prefix('q') {
                Session { session_type: SESSION_QUALIFY, name: "Qualify".to_string(), duration_minutes: rest.parse::<i32>().map_err(|_| wrong(part))? as f32, spawn_set: "PIT".to_string(), ..Session::default() }
            } else {
                return Err(wrong(part));
            };
            sessions.push(session);
        }
        if sessions.is_empty() {
            return Err("the session list is empty".to_string());
        }
        let jump_start_penalty_mode = jump_start.map(|mode| if mode == 2 && base_pit_penalty_laps as i32 >= last_race_laps && last_race_laps != 0 { 1 } else { mode });
        Ok(SessionsIni { sessions, has_additional_lap: additional, fixed_setup: false, penalties_enabled: penalties, jump_start_penalty_mode, has_hotlap: false })
    }

    /// `base_pit_penalty_laps`: `PhysicsAvatar::getBasePitPenaltyLaps()`.
    pub fn from_ini(ini: &IniReader, base_pit_penalty_laps: i16) -> SessionsIni {
        // (INIReader::getInt of a key that is not there, or is no number, is 0)
        let int = |section: &str, key: &str| ini.get_int(section, key).unwrap_or(0);
        let mut sessions = Vec::new();
        let mut has_additional_lap = false;
        let mut has_hotlap = false;
        let mut last_race_laps = 0;
        for i in 0.. {
            let section = format!("SESSION_{i}");
            if !ini.has_section(&section) {
                break;
            }
            let mut s = Session { name: ini.get_string(&section, "NAME"), session_type: int(&section, "TYPE"), ..Session::default() };
            if s.session_type <= 0 {
            } else if s.session_type <= 2 {
                s.duration_minutes = int(&section, "DURATION_MINUTES") as f32;
            } else if s.session_type == SESSION_RACE {
                s.laps = int(&section, "LAPS");
                s.duration_minutes = 0.0;
                s.is_timed_race = false;
                has_additional_lap = false;
                if s.laps == 0 {
                    s.duration_minutes = int(&section, "DURATION_MINUTES") as f32;
                    s.is_timed_race = true;
                    has_additional_lap = int(&section, "ADDITITIONAL_LAP") != 0;
                }
                last_race_laps = s.laps;
                s.forced_position = if ini.has_key(&section, "STARTING_POSITION") { int(&section, "STARTING_POSITION") } else { int("RACE", "CARS") };
            } else if s.session_type == SESSION_HOTLAP {
                has_hotlap = true;
            }
            s.spawn_set = ini.get_string(&section, "SPAWN_SET");
            sessions.push(s);
        }
        if sessions.is_empty() {
            sessions.push(Session { session_type: SESSION_PRACTICE, duration_minutes: 120.0, spawn_set: "PIT".to_string(), ..Session::default() });
        }
        let penalties_enabled = int("RACE", "PENALTIES") > 0;
        let jump_start_penalty_mode = ini.has_key("RACE", "JUMP_START_PENALTY").then(|| {
            let mut mode = int("RACE", "JUMP_START_PENALTY");
            // a drive-through that could never be served becomes the teleport to the pits
            if mode == 2 && base_pit_penalty_laps as i32 >= last_race_laps && last_race_laps != 0 {
                mode = 1;
            }
            mode
        });
        SessionsIni { sessions, has_additional_lap, fixed_setup: int("RACE", "FIXED_SETUP") > 0, penalties_enabled, jump_start_penalty_mode, has_hotlap }
    }
}

/// What the manager reads of a car (`CarAvatar`) in a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct CarView {
    /// `physicsState.normalizedSplinePosition`
    pub normalized_spline_position: f32,
    /// `CarAvatar::isInPit()` @ 0x1400d8bd0: less than 1.5 m from the pit box
    pub is_in_pit: bool,
    /// `CarAvatar::filteredSpeed`, m/s
    pub filtered_speed: f32,
    /// `Car::isRetired` (`None`: a car without physics here, a remote player's)
    pub is_retired: Option<bool>,
    /// `CarAvatar::isConnected()`
    pub is_connected: bool,
    /// `CarAvatar::isBlackFlagged`
    pub is_black_flagged: bool,
    /// how many tyres carry load (`None`: no physics here)
    pub loaded_tyres: Option<u32>,
    /// `CarAvatar::getCurrentCompoundShortName()`
    pub compound: String,
    /// `driverInfo.name`
    pub driver_name: String,
    /// `CarAvatar::guiName`: the car's name as the menus show it
    pub gui_name: String,
}

impl Default for CarView {
    fn default() -> CarView {
        CarView { normalized_spline_position: 0.0, is_in_pit: false, filtered_speed: 0.0, is_retired: Some(false), is_connected: true, is_black_flagged: false, loaded_tyres: Some(4), compound: String::new(), driver_name: String::new(), gui_name: String::new() }
    }
}

/// `CarAvatar::update`'s filter of the speed (the end of 0x1400db830): `dt` in seconds.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn filter_speed(filtered: f32, speed: f32, dt: f32) -> f32 {
    let k = dt * 0.2;
    let k = if k > 1.0 {
        1.0
    } else if k >= 0.0 {
        k
    } else {
        0.0
    };
    (speed - filtered) * k + filtered
}

/// `RealTimeCarDesc` (0x10 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealTimeCarDesc {
    pub car: usize,
    pub crossed_for_the_first_time: bool,
    pub approx_pos: f32,
}

/// `RaceStatusCarDesc` (0x18 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaceStatusCarDesc {
    pub car: usize,
    pub reset_timer: f32,
    pub retired: bool,
    pub invalid_state_timer: f32,
    pub max_invalid_state_timer: f32,
}

/// What runs when the fade to black is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeJob {
    /// lambda @ 0x140133ef0: the player's car to its pit box, controls locked (qualifying over)
    PlayerToPit,
    /// lambda @ 0x140133c00: a car that finished the race to its pit box, controls locked
    CarToPit(usize),
    /// lambda @ 0x140133c90: every car to the pits, controls locked, the results shown
    EndRace,
    /// lambdas @ 0x140135270 / 0x140135360: the results, the next session, new wind
    NextSession,
    /// lambda @ 0x1401352f0: the same session again
    Restart,
}

/// What the manager does to the rest of the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `PhysicsEngine::penaltyMode` = 0 (cut gas, a race) or 1 (the lap does not count)
    PenaltyMode(i32),
    /// `CarAvatar::lockControlsUntilTime(0, 0, false)`, `CarAvatar::resetPenalty(true)` and
    /// the car's pit-lane times forgotten
    ResetCar(usize),
    /// `Sim::evOnNewSession`: the engine's session, every car to its place of the session's
    /// spawn set with its lap timer reset
    NewSession { session: Session, index: i32 },
    /// `CarAvatar::setControlsLock`
    ControlsLock { car: usize, locked: bool },
    /// `CarAvatar::goToSpawnPosition(L"PIT")`
    GoToPit(usize),
    /// `PhysicsEngine::setCurrentSessionStartTime`
    SessionStartTime(f64),
    /// the player quit a race before its end: `Car::isRetired = true` of car 0
    RetirePlayer,
    /// `RaceManager::evOnSessionEnd` (`index`: `getCurrentSessionIndex()` as the handlers
    /// see it)
    SessionEnd { event: OnSessionEndEvent, index: usize },
    /// `SystemMessage::addMessage(text, description)`
    Message { text: String, description: String },
    /// `RaceManager::generateWind`: new wind for the new session
    GenerateWind,
    /// `EndSessionDisplayer::displayResults` (it locks the player's controls)
    DisplayResults,
    /// `EndSessionDisplayer::hide` (it frees the player's controls)
    HideResults,
    /// `ACCameraManager::fadeIn`: the picture goes black over [`FADE_SECONDS`]
    FadeStarted,
}

/// `RaceManager`.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceManager {
    pub penalties_enabled: bool,
    pub fixed_setup: bool,
    pub pause_mode: bool,
    pub current_session_index: i32,
    /// `system/cfg/assetto_corsa.ini [RACE] OVER_TIME_MULT` (1.5 without the section)
    pub over_time_mult: f32,
    pub time_race_ended: bool,
    pub is_timed_last_lap: bool,
    pub has_additional_lap: bool,
    pub pit_window_start_time: f32,
    pub pit_window_end_time: f32,
    pub leader_last_lap: bool,
    pub current_session: Session,
    pub timing: RaceTimingServices,
    pub sessions: Vec<Session>,
    pub cars_real_time_position: Vec<RealTimeCarDesc>,
    pub cars_race_status: Vec<RaceStatusCarDesc>,
    pub leaderboard_history: Vec<Vec<LeaderboardEntry>>,
    pub session_history_list: Vec<OnSessionEndEvent>,
    /// `game->now` of the last call, ms
    pub now: f64,
    /// the camera's fade: seconds left and what follows
    pub fade: Option<(f32, FadeJob)>,
    /// the end-of-session panel is up
    pub results_displayed: bool,
    /// the current session's results were made (`finalizeCurrentSession` ran for it)
    pub finalized: bool,
    /// `optionsManager.options["AUTOFLIP_RECOVERY"]` is not 0
    pub autoflip_recovery: bool,
    /// `CarRaceInfo::spawnPositionIndex[L"START"]` of every car: its slot on the grid
    pub start_index: Vec<i32>,
    /// the global `SessionResult` the result processor works on (0x141559b70)
    pub processor_result: SessionResult,
    /// the player was retired for leaving a race early in this very frame: `Car::isRetired`
    /// is set already, the views of the frame do not show it yet
    pub player_retired_now: bool,
}

impl RaceManager {
    /// The constructor @ 0x140132db0 and `loadSessions` @ 0x14013ecb0 for an offline session
    /// list. Nothing runs until [`RaceManager::set_current_session`].
    pub fn new(ini: &SessionsIni, cars: usize, sector_count: usize, over_time_mult: f32) -> RaceManager {
        RaceManager {
            penalties_enabled: ini.penalties_enabled,
            fixed_setup: ini.fixed_setup,
            pause_mode: false,
            current_session_index: -1,
            over_time_mult,
            time_race_ended: false,
            is_timed_last_lap: false,
            has_additional_lap: ini.has_additional_lap,
            pit_window_start_time: 0.0,
            pit_window_end_time: 0.0,
            leader_last_lap: false,
            current_session: Session::default(),
            timing: RaceTimingServices::new(cars, sector_count),
            sessions: ini.sessions.clone(),
            cars_real_time_position: (0..cars).map(|car| RealTimeCarDesc { car, crossed_for_the_first_time: false, approx_pos: -1.0 }).collect(),
            cars_race_status: (0..cars).map(|car| RaceStatusCarDesc { car, reset_timer: 5.0, retired: false, invalid_state_timer: 0.0, max_invalid_state_timer: 5.0 }).collect(),
            leaderboard_history: vec![Vec::new(); ini.sessions.len()],
            session_history_list: Vec::new(),
            now: 0.0,
            fade: None,
            results_displayed: false,
            finalized: false,
            autoflip_recovery: false,
            start_index: (0..cars as i32).collect(),
            processor_result: SessionResult::default(),
            player_retired_now: false,
        }
    }

    pub fn car_count(&self) -> usize {
        self.cars_race_status.len()
    }

    /// `setCurrentSession` @ 0x140140df0.
    pub fn set_current_session(&mut self, index: u32, now: f64, actions: &mut Vec<Action>) {
        self.now = now;
        if index as usize >= self.sessions.len() {
            return;
        }
        self.current_session = self.sessions[index as usize].clone();
        self.current_session_index = index as i32;
        self.current_session.start_time = if self.current_session.session_type == SESSION_RACE { f64::MAX } else { now };
        actions.push(Action::PenaltyMode(if self.current_session.session_type == SESSION_RACE { 0 } else { 1 }));
        for car in 0..self.car_count() {
            actions.push(Action::ResetCar(car));
        }
        self.pit_window_start_time = 0.0;
        self.pit_window_end_time = 0.0;
        self.time_race_ended = false;
        self.is_timed_last_lap = false;
        self.current_session.is_over = false;
        for desc in &mut self.cars_real_time_position {
            desc.crossed_for_the_first_time = false;
            desc.approx_pos = -1.0;
        }
        // Sim::evOnNewSession: the timing services' handler is one of them
        self.timing.on_new_session(now as i64 as u32);
        actions.push(Action::NewSession { session: self.current_session.clone(), index: index as i16 as i32 });
        for status in &mut self.cars_race_status {
            status.reset_timer = 5.0;
            actions.push(Action::ControlsLock { car: status.car, locked: false });
            status.retired = false;
        }
        self.timing.reset_current_laps();
        // resetMandatoryPit @ 0x140140600
        for car in 0..self.car_count() {
            actions.push(Action::ResetCar(car));
        }
        self.pit_window_start_time = 0.0;
        self.pit_window_end_time = 0.0;
        self.finalized = false;
    }

    /// The end of `initOffline` @ 0x14013a6c0: `resetCurrentLaps` and `resetMandatoryPit`.
    pub fn reset_current_laps_and_mandatory_pit(&mut self, actions: &mut Vec<Action>) {
        self.timing.reset_current_laps();
        for car in 0..self.car_count() {
            actions.push(Action::ResetCar(car));
        }
        self.pit_window_start_time = 0.0;
        self.pit_window_end_time = 0.0;
    }

    /// `startRace` @ 0x140141470 (`delay_ms`: 20000 from `Sim::Sim`, 10000 from the click
    /// that ends the session's intro; `rand`: the C runtime's `rand()`, drawn only when this
    /// returns true beforehand from [`RaceManager::start_race_draws`]).
    pub fn start_race(&mut self, delay_ms: f32, rand: i32, actions: &mut Vec<Action>) {
        if self.start_race_draws() {
            self.current_session.start_time = delay_ms as f64 + self.now + (rand as f32 * results::RAND_SCALE * 2000.0 - 1000.0) as f64;
            actions.push(Action::SessionStartTime(self.current_session.start_time));
        }
        self.leader_last_lap = false;
    }

    /// Whether `startRace` would set a start (and draw a random number) now.
    pub fn start_race_draws(&self) -> bool {
        self.current_session.session_type == SESSION_RACE && self.time_to_session_start() > 0.0
    }

    /// `getTimeToSessionStart` @ 0x14013a0a0, ms.
    pub fn time_to_session_start(&self) -> f64 {
        self.current_session.start_time - self.now
    }

    /// `hasSessionStarted` @ 0x14013a170.
    pub fn has_session_started(&self) -> bool {
        self.current_session.start_time < self.now
    }

    /// `getSessionTimeLeft` @ 0x140139e60, ms.
    pub fn session_time_left(&self) -> f64 {
        self.current_session.duration_minutes as f64 * 60.0 * 1000.0 + self.current_session.start_time - self.now
    }

    /// `Car::isRetired` of a car as the game's code reads it right now.
    pub fn car_retired(&self, car: usize, cars: &[CarView]) -> Option<bool> {
        if car == 0 && self.player_retired_now && cars[car].is_retired.is_some() {
            Some(true)
        } else {
            cars[car].is_retired
        }
    }

    /// `getLapCount` @ 0x140138570.
    pub fn lap_count(&self, car: usize) -> u32 {
        self.timing.lap_count(car)
    }

    /// `getHasCompletedFlag` @ 0x140138390.
    pub fn has_completed_flag(&self, car: usize) -> bool {
        self.timing.lap_dbs[car].has_completed_last_lap
    }

    /// `setHasCompletedFlag` @ 0x140141270.
    pub fn set_has_completed_flag(&mut self, car: usize, value: bool) {
        self.timing.lap_dbs[car].has_completed_last_lap = value;
    }

    /// `isRaceOver` @ 0x14013eae0.
    pub fn is_race_over(&self, car: usize, cars: &[CarView]) -> bool {
        if self.current_session.session_type != SESSION_RACE {
            return false;
        }
        if !self.current_session.is_timed_race && self.lap_count(car) >= self.current_session.laps as u32 {
            return true;
        }
        if self.current_session.is_over {
            let status_car = self.cars_race_status[car].car;
            if self.has_completed_flag(status_car) || cars[status_car].is_in_pit || 1.0 > cars[status_car].filtered_speed {
                return true;
            }
        }
        false
    }

    fn board_cars(&self, cars: &[CarView]) -> Vec<BoardCar> {
        cars.iter().enumerate().map(|(i, c)| BoardCar { is_black_flagged: c.is_black_flagged, start_index: self.start_index.get(i).copied().unwrap_or(0) }).collect()
    }

    /// `RaceTimingServices::update` (every frame) and after a listed lap.
    pub fn update_leaderboard(&mut self, cars: &[CarView]) {
        let board = self.board_cars(cars);
        self.timing.update_leaderboard(self.current_session.session_type == SESSION_RACE, &board);
    }

    /// `onLapCompleted` @ 0x14013f380 (a car's `evOnLapCompleted`, on the main thread).
    pub fn on_lap_completed(&mut self, ev: &OnLapCompletedEvent, cars: &[CarView]) {
        let car = ev.car_index as usize;
        if self.timing.on_lap_completed(ev, &cars[car].compound) {
            self.update_leaderboard(cars);
        }
        match self.current_session.session_type {
            SESSION_QUALIFY => {
                if self.current_session.overtime_ms > 0.0 {
                    self.set_has_completed_flag(car, true);
                }
            }
            SESSION_RACE => {
                if !self.current_session.is_timed_race {
                    if self.current_session.leader_completed_laps < ev.lap_count {
                        self.current_session.leader_completed_laps = ev.lap_count;
                    }
                    if !self.current_session.is_over && self.is_race_over(car, cars) {
                        self.current_session.is_over = true;
                    }
                    if self.current_session.is_over {
                        self.set_has_completed_flag(car, true);
                    }
                } else if self.time_race_ended {
                    if self.has_additional_lap {
                        if self.is_timed_last_lap {
                            self.set_has_completed_flag(car, true);
                        }
                        self.is_timed_last_lap = true;
                    } else {
                        self.set_has_completed_flag(car, true);
                    }
                }
            }
            _ => {}
        }
    }

    /// `RaceTimingServices::onSectorSplit` (a car's `evOnSectorSplit`).
    pub fn on_sector_split(&mut self, ev: &crate::track::timing::OnSectorSplitEvent) {
        self.timing.on_sector_split(ev);
    }

    fn start_fade(&mut self, job: FadeJob, actions: &mut Vec<Action>) {
        // (a restart asks for a fade of 1 s, everything else for 2.5 s)
        self.fade = Some((if job == FadeJob::Restart { 1.0 } else { FADE_SECONDS }, job));
        actions.push(Action::FadeStarted);
    }

    /// `update` @ 0x140141550 while the game is paused or shows a replay: the session's
    /// clock stands still.
    pub fn update_paused(&mut self, dt: f32, now: f64, actions: &mut Vec<Action>) {
        self.now = now;
        self.current_session.start_time += (dt * 1000.0) as f64;
        actions.push(Action::SessionStartTime(self.current_session.start_time));
    }

    /// `update` @ 0x140141550, one frame (`dt` seconds, `now` ms) of an offline session, after
    /// `RaceTimingServices::update` (the leaderboard, every frame).
    /// `rand`: the C runtime's `rand()` (drawn when a session's results are made up).
    pub fn update(&mut self, dt: f32, now: f64, cars: &[CarView], rand: &mut dyn FnMut() -> i32, actions: &mut Vec<Action>) {
        self.now = now;
        self.player_retired_now = false;
        // the camera's fade, which the game's camera manager counts
        if let Some((left, job)) = self.fade {
            let left = left - dt;
            if left <= 0.0 {
                self.fade = None;
                self.run_fade_job(job, cars, rand, actions);
            } else {
                self.fade = Some((left, job));
            }
        }
        self.update_leaderboard(cars);
        if self.time_to_session_start() < 0.0 {
            self.reset_invalid_state_cars_to_pits(dt, cars, actions);
        }
        let duration = self.current_session.duration_minutes * 60.0 * 1000.0;
        let elapsed = (now - self.current_session.start_time) as f32;
        if self.current_session.session_type == SESSION_RACE {
            if !self.is_race_over(0, cars) {
                self.update_cars_real_time_positions(dt, cars);
            }
            if self.current_session.is_timed_race && !self.time_race_ended && duration < elapsed {
                self.time_race_ended = true;
                self.current_session.is_over = true;
            }
            if !self.current_session.is_over {
                for status in &mut self.cars_race_status {
                    if !status.retired && cars[status.car].is_retired == Some(true) {
                        actions.push(Action::Message { text: format!("Driver {} : {}", status.car, cars[status.car].driver_name), description: "Retired".to_string() });
                        status.retired = true;
                    }
                }
            } else {
                self.update_cars_end_race(dt, cars, actions);
            }
        }
        #[allow(clippy::float_cmp)]
        if self.current_session.start_time != 0.0 && 0.0 < self.current_session.duration_minutes && duration < elapsed && self.current_session.overtime_ms == -1.0 {
            let mut reference = 0.0f32;
            for status in &self.cars_race_status {
                let best = self.timing.lap_dbs[status.car].best_lap.time;
                if reference == 0.0 {
                    reference = best as f32;
                }
                if best != 0 && (best as f32) < reference {
                    reference = best as f32;
                }
            }
            let mut overtime = reference * self.over_time_mult;
            if overtime < 10000.0 {
                overtime = 10000.0;
            }
            self.current_session.overtime_ms = overtime;
            if self.current_session.session_type != SESSION_QUALIFY {
                self.current_session.overtime_ms = 0.0;
            }
        }
        let mut all_completed = true;
        if self.current_session.session_type == SESSION_QUALIFY && self.current_session.overtime_ms >= 0.0 {
            for i in 0..self.cars_race_status.len() {
                let car = self.cars_race_status[i].car;
                if cars[car].is_in_pit {
                    self.set_has_completed_flag(car, true);
                }
                if all_completed && !self.has_completed_flag(car) {
                    all_completed = false;
                }
            }
            if let Some(first) = self.cars_race_status.first().map(|s| s.car) {
                if self.has_completed_flag(first) && !cars[first].is_in_pit && self.fade.is_none() {
                    self.start_fade(FadeJob::PlayerToPit, actions);
                }
            }
        }
        let overtime = self.current_session.overtime_ms;
        #[allow(clippy::float_cmp)]
        if overtime != -1.0 && (overtime < elapsed - duration || all_completed) && self.current_session_index < self.sessions.len() as i32 - 1 && self.fade.is_none() {
            self.start_fade(FadeJob::NextSession, actions);
        }
    }

    fn run_fade_job(&mut self, job: FadeJob, cars: &[CarView], rand: &mut dyn FnMut() -> i32, actions: &mut Vec<Action>) {
        match job {
            FadeJob::PlayerToPit => {
                if let Some(first) = self.cars_race_status.first().map(|s| s.car) {
                    actions.push(Action::GoToPit(first));
                    actions.push(Action::ControlsLock { car: first, locked: true });
                }
            }
            FadeJob::CarToPit(car) => {
                actions.push(Action::GoToPit(car));
                actions.push(Action::ControlsLock { car, locked: true });
            }
            FadeJob::EndRace => {
                for (car, view) in cars.iter().enumerate() {
                    if !view.is_in_pit {
                        actions.push(Action::GoToPit(car));
                    }
                    actions.push(Action::ControlsLock { car, locked: true });
                }
                self.results_displayed = true;
                actions.push(Action::DisplayResults);
            }
            FadeJob::NextSession => {
                self.finalize_current_session(cars, rand, actions);
                let next = (self.current_session_index + 1) as u32;
                self.set_current_session(next, self.now, actions);
                actions.push(Action::GenerateWind);
            }
            FadeJob::Restart => {
                // EndSessionDisplayer::hide @ 0x14010b380: only a panel that is up
                if self.results_displayed {
                    self.results_displayed = false;
                    actions.push(Action::HideResults);
                }
                let index = self.current_session_index as u32;
                self.set_current_session(index, self.now, actions);
            }
        }
    }

    /// `skipCurrentSession` @ 0x140141350.
    pub fn skip_current_session(&mut self, actions: &mut Vec<Action>) {
        if self.current_session_index < self.sessions.len() as i32 - 1 && self.fade.is_none() {
            self.start_fade(FadeJob::NextSession, actions);
        }
    }

    /// `restartCurrentSession` @ 0x140140d00.
    pub fn restart_current_session(&mut self, actions: &mut Vec<Action>) {
        if self.fade.is_none() {
            self.start_fade(FadeJob::Restart, actions);
        }
    }

    /// `resetInvalidStateCarsToPits` @ 0x140140340.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn reset_invalid_state_cars_to_pits(&mut self, dt: f32, cars: &[CarView], actions: &mut Vec<Action>) {
        if !self.autoflip_recovery {
            return;
        }
        let dt = if dt > 0.5 { 0.5 } else { dt };
        for status in &mut self.cars_race_status {
            if let Some(loaded) = cars[status.car].loaded_tyres {
                if loaded < 2 {
                    status.invalid_state_timer += dt;
                } else {
                    status.invalid_state_timer = 0.0;
                }
            }
            if !(status.max_invalid_state_timer > status.invalid_state_timer) {
                actions.push(Action::GoToPit(status.car));
                status.invalid_state_timer = 0.0;
            }
        }
    }

    /// `updateCarsRealTimePositions` @ 0x1401428b0.
    #[allow(clippy::float_cmp)]
    pub fn update_cars_real_time_positions(&mut self, dt: f32, cars: &[CarView]) {
        let started = self.time_to_session_start() < 0.0;
        for desc in &mut self.cars_real_time_position {
            if cars[desc.car].normalized_spline_position < 0.5 && started {
                desc.crossed_for_the_first_time = true;
            }
        }
        let timing = &self.timing;
        let retired_now = self.player_retired_now;
        let retired = |car: usize| if car == 0 && retired_now && cars[car].is_retired.is_some() { Some(true) } else { cars[car].is_retired };
        // the comparator @ 0x140134bf0
        let before = |a: &RealTimeCarDesc, b: &RealTimeCarDesc| -> bool {
            let (ca, cb) = (&cars[a.car], &cars[b.car]);
            if let (Some(ra), Some(rb)) = (retired(a.car), retired(b.car)) {
                if !ra && rb {
                    return true;
                }
                if ra && !rb {
                    return false;
                }
            }
            if ca.is_connected && !cb.is_connected {
                return true;
            }
            if !ca.is_connected && cb.is_connected {
                return false;
            }
            let (la, lb) = (timing.lap_count(a.car), timing.lap_count(b.car));
            if la != lb {
                return la > lb;
            }
            if la == 0 && a.crossed_for_the_first_time != b.crossed_for_the_first_time {
                return a.crossed_for_the_first_time;
            }
            cb.normalized_spline_position < ca.normalized_spline_position
        };
        msvc_sort::sort(&mut self.cars_real_time_position, &before);
        for (i, desc) in self.cars_real_time_position.iter_mut().enumerate() {
            if desc.approx_pos == -1.0 {
                desc.approx_pos = i as f32;
            }
            desc.approx_pos = (i as f32 - desc.approx_pos) * dt + desc.approx_pos;
        }
        msvc_sort::sort(&mut self.cars_real_time_position, &|a: &RealTimeCarDesc, b: &RealTimeCarDesc| a.approx_pos < b.approx_pos);
    }

    /// `getCarRealTimePosition` @ 0x140137f90: from 0.
    pub fn car_real_time_position(&self, car: usize, _cars: &[CarView]) -> i32 {
        if self.current_session.session_type == SESSION_RACE && !self.has_session_started() {
            return self.start_index[car];
        }
        self.cars_real_time_position.iter().position(|desc| desc.car == car).map_or(0, |i| i as i32)
    }

    /// `getCarLeaderboardPosition` @ 0x140137ee0: from 1 (the real-time place, from 0, for a
    /// car the leaderboard does not list).
    pub fn car_leaderboard_position(&self, car: usize, cars: &[CarView]) -> i32 {
        let place = self.timing.car_leaderboard_position(car);
        if place == -1 {
            self.car_real_time_position(car, cars)
        } else {
            place
        }
    }

    /// `updateCarsEndRace` @ 0x140142650.
    fn update_cars_end_race(&mut self, dt: f32, cars: &[CarView], actions: &mut Vec<Action>) {
        let mut all_over = true;
        for i in 0..self.cars_race_status.len() {
            let car = self.cars_race_status[i].car;
            if self.is_race_over(car, cars) {
                if !cars[car].is_in_pit {
                    let left = self.cars_race_status[i].reset_timer - dt;
                    self.cars_race_status[i].reset_timer = left;
                    if left < 0.0 && car == 0 && self.fade.is_none() {
                        self.start_fade(FadeJob::CarToPit(car), actions);
                    }
                }
            } else {
                all_over = false;
            }
        }
        if !all_over {
            return;
        }
        if !self.results_displayed && self.fade.is_none() {
            self.start_fade(FadeJob::EndRace, actions);
        }
    }

    /// `finalizeCurrentSession` @ 0x140136b30, offline: the session's results, the event.
    pub fn finalize_current_session(&mut self, cars: &[CarView], rand: &mut dyn FnMut() -> i32, actions: &mut Vec<Action>) {
        let session = self.current_session.clone();
        if session.session_type == SESSION_RACE && !self.is_race_over(0, cars) {
            actions.push(Action::RetirePlayer);
            self.player_retired_now = true;
        }
        let mut scratch = std::mem::take(&mut self.processor_result);
        let complete = results::get_complete_ai_results(self, cars, &mut scratch, rand);
        self.processor_result = scratch;
        let mut result = SessionResult { laps: complete.laps, best_laps: complete.best_laps, positions: complete.positions, lap_count: complete.lap_count, total: vec![0.0; self.car_count()] };
        for &car in &result.positions {
            let car = car as usize;
            if car >= self.car_count() {
                continue;
            }
            result.total[car] = 0.0;
            if let Some(laps) = result.laps.get(car) {
                for lap in laps {
                    result.total[car] += lap.time as f32;
                }
            }
        }
        let index = self.current_session_index.max(0) as usize;
        if self.leaderboard_history.len() <= index {
            self.leaderboard_history.resize(index + 1, Vec::new());
        }
        self.leaderboard_history[index].clear();
        for &car in &result.positions {
            let car = car as usize;
            if car >= self.car_count() {
                continue;
            }
            self.leaderboard_history[index].push(LeaderboardEntry {
                car,
                total_time: result.total.get(car).copied().unwrap_or(0.0) as f64,
                // (through a float: a lap time above 2^24 ms is rounded)
                best_lap: result.best_laps.get(car).map_or(0, |lap| lap.time) as f32 as f64,
                // (the game tests the car against the size of the best-lap list here)
                laps: if car < result.best_laps.len() { result.lap_count.get(car).copied().unwrap_or(0) } else { 0 },
                is_race_mode: session.session_type == SESSION_RACE,
                is_black_flagged: cars[car].is_black_flagged,
                has_completed_last_lap: true,
            });
        }
        let event = OnSessionEndEvent { current_session: session, result };
        if self.session_history_list.len() <= index {
            self.session_history_list.resize(index + 1, OnSessionEndEvent::default());
        }
        self.session_history_list[index] = event.clone();
        self.finalized = true;
        // CarRaceInfo's handler (lambda @ 0x1400e7a20): the grid of the race to come is the
        // order of qualifying
        if event.current_session.session_type == SESSION_QUALIFY {
            for car in 0..self.car_count() {
                if let Some(k) = event.result.positions.iter().position(|p| *p == car as i32) {
                    self.start_index[car] = k as i32;
                }
            }
        }
        actions.push(Action::SessionEnd { event, index });
    }
}
