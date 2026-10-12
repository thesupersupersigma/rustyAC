// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FlagManager` (FlagManager.obj, 0x108 bytes): which flag the player is shown. Only the
//! decision is here; the game draws `content/gui/flags/<name>.png`.
//!
//! The game has no white flag: the value exists in the enum and in the shared memory's
//! table, and nothing ever sets it.

#![allow(clippy::needless_range_loop)]

use super::manager::{CarView, RaceManager, SESSION_DRAG, SESSION_QUALIFY, SESSION_RACE};

/// `FlagType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flag {
    #[default]
    None = 0,
    Blue = 1,
    Black = 2,
    Chequered = 3,
    Yellow = 4,
    White = 5,
    Penalty = 6,
}

impl Flag {
    /// The texture's name under `content/gui/flags/`.
    pub fn texture(self) -> Option<&'static str> {
        match self {
            Flag::Blue => Some("blueFlag"),
            Flag::Black => Some("blackFlag"),
            Flag::Chequered => Some("finish"),
            Flag::Yellow => Some("yellowFlag"),
            Flag::Penalty => Some("penalty"),
            Flag::None | Flag::White => None,
        }
    }

    /// The flag in words, for a display without the textures.
    pub fn words(self) -> &'static str {
        match self {
            Flag::None => "",
            Flag::Blue => "BLUE FLAG",
            Flag::Black => "BLACK FLAG",
            Flag::Chequered => "CHEQUERED FLAG",
            Flag::Yellow => "YELLOW FLAG",
            Flag::White => "WHITE FLAG",
            Flag::Penalty => "PENALTY FLAG",
        }
    }

    /// The shared memory's number of the flag (`SharedMemoryWriter`).
    pub fn shared_memory(self) -> i32 {
        match self {
            Flag::None => 0,
            Flag::Blue => 1,
            Flag::Yellow => 2,
            Flag::Black => 3,
            Flag::White => 4,
            Flag::Chequered => 5,
            Flag::Penalty => 6,
        }
    }
}

/// What the flags read of a car beyond the manager's [`CarView`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlagCar {
    /// `CarAvatar::isInPitlane()`
    pub is_in_pitlane: bool,
    /// how many of the four tyres are on a surface that is not valid track
    pub tyres_off_track: u32,
    /// `physicsState.gear`: 0 is reverse
    pub gear: i32,
    /// `physicsState.speed`, m/s
    pub speed: f32,
    /// `CarAvatar::getRoadDirection()` dot the car's backward axis: above 0.4 the car points
    /// against the road
    pub against_road: f32,
}

impl Default for FlagCar {
    fn default() -> FlagCar {
        FlagCar { is_in_pitlane: false, tyres_off_track: 0, gear: 1, speed: 0.0, against_road: 0.0 }
    }
}

/// What the flags read of the player's own penalties.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerPenalty {
    /// `CarAvatar::hasPenalty()`: `Car::getPenaltyTime()`, s
    pub penalty_time: f64,
    /// `PenaltyManager::pitPenaltyLaps`
    pub pit_penalty_laps: i16,
}

/// What a frame of [`FlagManager::update`] shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub blue: bool,
    pub black: bool,
    pub yellow: bool,
    pub penalty: bool,
    pub chequered: bool,
}

impl Shown {
    /// The flags on the screen, the most important first.
    pub fn list(&self) -> Vec<Flag> {
        [(self.chequered, Flag::Chequered), (self.black, Flag::Black), (self.penalty, Flag::Penalty), (self.yellow, Flag::Yellow), (self.blue, Flag::Blue)].iter().filter(|(on, _)| *on).map(|(_, flag)| *flag).collect()
    }
}

/// `PenaltyDescription` of a black flag as the caption under it says it.
pub fn black_flag_caption(description: i32) -> &'static str {
    match description {
        1 => "You have been disqualified for not clearing the JumpStart penalty",
        3 => "You have been disqualified for not doing the Mandatory Pit",
        4 => "You have been disqualified for not clearing the Track Cut penalty",
        _ => "",
    }
}

/// `FlagManager`.
#[derive(Clone, Debug, PartialEq)]
pub struct FlagManager {
    pub timer: f32,
    /// `lastPositionOnSpline`, by car
    pub last_position_on_spline: Vec<f32>,
    pub current_flag: Flag,
    /// `yellowFlagDistance`, m: 700, or the track's `data/overlays.ini [YELLOW_FLAG] DISTANCE`
    pub yellow_flag_distance: f32,
    /// the track's length, m
    pub track_length: f32,
    /// the caption under the black flag
    pub black_flag_caption: &'static str,
}

impl FlagManager {
    /// The constructor @ 0x14009ab60.
    pub fn new(cars: usize, track_length: f32, yellow_flag_distance: Option<f32>) -> FlagManager {
        FlagManager { timer: 0.0, last_position_on_spline: vec![-1.0; cars], current_flag: Flag::None, yellow_flag_distance: yellow_flag_distance.unwrap_or(700.0), track_length, black_flag_caption: "" }
    }

    /// `clearFlag` @ 0x14009cb60 (also the handler of a new session).
    pub fn clear_flag(&mut self) {
        self.current_flag = Flag::None;
        self.timer = -1.0;
    }

    /// The handler of `PhysicsEngine::evOnFlagEvent` (lambda @ 0x14009b830), for the
    /// player's car: `Car::setBlackFlag(flag, description)`.
    pub fn on_black_flag(&mut self, flag: bool, description: i32) {
        if flag {
            // setFlag(eBlackFlag) @ 0x14009cf00
            if self.timer <= 0.0 || self.current_flag != Flag::Black {
                self.current_flag = Flag::Black;
            }
            self.black_flag_caption = black_flag_caption(description);
        } else {
            self.clear_flag();
            self.black_flag_caption = "";
        }
    }

    /// `checkForBlue` @ 0x14009c390.
    fn check_for_blue(&mut self, manager: &RaceManager, cars: &[CarView], flag_cars: &[FlagCar]) {
        let my_laps = manager.lap_count(0);
        if flag_cars[0].is_in_pitlane {
            return;
        }
        let mut any = false;
        for i in 1..cars.len() {
            if cars[i].is_connected && manager.lap_count(i) > my_laps && !flag_cars[i].is_in_pitlane {
                let mut d = cars[0].normalized_spline_position - cars[i].normalized_spline_position;
                if d < 0.0 {
                    d += 1.0;
                }
                if d * self.track_length <= 100.0 {
                    // setBlueFlag @ 0x14009ce30
                    if !matches!(self.current_flag, Flag::Blue | Flag::Black | Flag::Yellow) {
                        self.current_flag = Flag::Blue;
                    }
                    any = true;
                }
            }
        }
        if any {
            return;
        }
        if self.current_flag == Flag::Blue {
            self.clear_flag();
        }
    }

    /// `checkForYellow` @ 0x14009c550.
    fn check_for_yellow(&mut self, manager: &RaceManager, cars: &[CarView], flag_cars: &[FlagCar]) {
        let mut yellow = false;
        for i in 1..cars.len() {
            if !cars[i].is_connected {
                continue;
            }
            let delta = (cars[i].normalized_spline_position - cars[0].normalized_spline_position) * self.track_length;
            if delta < self.yellow_flag_distance && delta > 0.0 {
                let car = &flag_cars[i];
                if car.tyres_off_track == 4 {
                    yellow = true;
                }
                if car.gear == 0 {
                    yellow = true;
                }
                if car.speed * 3.6 < 15.0 && !car.is_in_pitlane && manager.has_session_started() && (manager.now - manager.current_session.start_time) > 5000.0 {
                    yellow = true;
                }
                if car.against_road > 0.4 && cars[i].normalized_spline_position < self.last_position_on_spline[i] {
                    yellow = true;
                }
                self.last_position_on_spline[i] = cars[i].normalized_spline_position;
            }
        }
        if yellow {
            if matches!(self.current_flag, Flag::Black | Flag::Chequered) {
                return;
            }
            self.current_flag = Flag::Yellow;
            self.timer = 5.0;
            return;
        }
        if self.current_flag == Flag::Yellow {
            self.clear_flag();
        }
    }

    /// `checkforPenalty` @ 0x14009c9b0.
    fn check_for_penalty(&mut self, penalty: &PlayerPenalty) {
        if self.current_flag == Flag::Black {
            return;
        }
        if penalty.penalty_time > 0.0 {
            self.current_flag = Flag::Penalty;
        } else if penalty.pit_penalty_laps < 1 {
            if self.current_flag == Flag::Penalty {
                self.current_flag = Flag::None;
            }
        } else {
            self.current_flag = Flag::Penalty;
        }
    }

    /// `update` @ 0x14009d140, a frame in which the player's own car is watched and no menu
    /// or replay is up (the game leaves the function early otherwise, timer and all).
    /// `results_displayed`: the end-of-session panel is up.
    pub fn update(&mut self, dt: f32, manager: &RaceManager, cars: &[CarView], flag_cars: &[FlagCar], penalty: &PlayerPenalty, results_displayed: bool) -> Shown {
        let mut shown = Shown::default();
        if cars.is_empty() || flag_cars.len() < cars.len() {
            return shown;
        }
        if self.last_position_on_spline.len() < cars.len() {
            self.last_position_on_spline.resize(cars.len(), -1.0);
        }
        let session_type = manager.current_session.session_type;
        if session_type == SESSION_RACE {
            self.check_for_blue(manager, cars, flag_cars);
        }
        #[allow(clippy::float_cmp)]
        if self.yellow_flag_distance != 0.0 && session_type != SESSION_DRAG {
            self.check_for_yellow(manager, cars, flag_cars);
        }
        self.check_for_penalty(penalty);
        let race_over = manager.is_race_over(0, cars);
        let free = !race_over && !results_displayed;
        match self.current_flag {
            Flag::Blue => shown.blue = free,
            Flag::Black => shown.black = free,
            Flag::Yellow => shown.yellow = free && self.timer > 0.0,
            Flag::Penalty => shown.penalty = free,
            _ => {}
        }
        let chequered = if session_type == SESSION_QUALIFY { manager.session_time_left() < 0.0 } else { session_type == SESSION_RACE && race_over && !results_displayed };
        if chequered {
            self.current_flag = Flag::Chequered;
            self.timer = -1.0;
            shown.chequered = true;
        }
        self.timer -= dt;
        shown
    }
}
