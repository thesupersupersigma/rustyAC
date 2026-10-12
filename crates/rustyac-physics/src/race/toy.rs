// SPDX-License-Identifier: GPL-3.0-or-later

//! Made-up drives for the referee's proofs: cars that lap a made-up track in a few seconds,
//! pit, retire, cut, get penalties and flags, and a player who presses "next session" and
//! "restart" now and then. `tools/car_oracle race-micro` feeds one such drive to the game's
//! own `RaceManager`, `RaceTimingServices` and `FlagManager` and to the port, and compares
//! every field after every frame; `tests/race_rules.rs` runs the same drives through the
//! port alone and holds the result against what the oracle run approved.

use super::flags::{FlagCar, FlagManager, PlayerPenalty, Shown};
use super::manager::{filter_speed, Action, CarView, RaceManager, Session, SessionsIni, SESSION_RACE};
use super::output::{JsonOutputFile, Player};
use crate::session::MsvcRand;
use crate::track::timing::{OnLapCompletedEvent, OnSectorSplitEvent};

/// A frame of the made-up drives, s.
pub const DT: f32 = 0.009;
/// The made-up track: its sectors and its length, m.
pub const SECTORS: usize = 3;
pub const TRACK_LENGTH: f32 = 3000.0;
pub const OVER_TIME_MULT: f32 = 1.2;

/// The session lists the made-up drives run through.
pub fn sessions(variant: usize, cars: usize) -> SessionsIni {
    let session = |kind: i32, name: &str, laps: i32, minutes: f32, spawn: &str| Session {
        session_type: kind,
        name: name.to_string(),
        laps,
        duration_minutes: minutes,
        is_timed_race: kind == SESSION_RACE && laps == 0,
        spawn_set: spawn.to_string(),
        forced_position: if kind == SESSION_RACE { cars as i32 } else { -1 },
        ..Session::default()
    };
    let (sessions, additional): (Vec<Session>, bool) = match variant % 8 {
        0 => (vec![session(1, "Practice", 0, 1.0, "PIT"), session(2, "Qualify", 0, 1.0, "PIT"), session(3, "Race", 3, 0.0, "START")], false),
        1 => (vec![session(3, "Race", 0, 1.0, "START")], true),
        2 => (vec![session(3, "Race", 0, 1.0, "START")], false),
        3 => (vec![session(3, "Quick Race", 5, 0.0, "START")], false),
        4 => (vec![session(2, "Qualify", 0, 1.0, "PIT"), session(3, "Race", 2, 0.0, "START")], false),
        5 => (vec![session(1, "Practice", 0, 0.0, "PIT"), session(3, "Race", 0, 1.0, "START"), session(1, "Practice", 0, 1.0, "PIT")], true),
        6 => (vec![session(2, "Qualify", 0, 2.0, "PIT"), session(2, "Qualify", 0, 1.0, "PIT"), session(3, "Race", 4, 0.0, "START"), session(3, "Race", 1, 0.0, "START")], false),
        _ => (vec![session(4, "Hotlap", 0, 0.0, "HOTLAP_START"), session(1, "Practice", 0, 1.0, "PIT"), session(3, "Race", 2, 0.0, "START")], false),
    };
    SessionsIni { sessions, has_additional_lap: additional, fixed_setup: false, penalties_enabled: variant.is_multiple_of(2), jump_start_penalty_mode: None, has_hotlap: variant % 8 == 7 }
}

/// The made-up cars' names: (driver, car, skin, tyre compound).
pub fn players(variant: usize, cars: usize) -> Vec<(Player, String)> {
    let models = ["car_a", "car_a", "car_b", "car_c"];
    (0..cars).map(|i| (Player { name: format!("Driver {i}"), car: models[(i + variant) % models.len()].to_string(), skin: format!("skin_{i}") }, ["S", "M", "H", "SM"][i % 4].to_string())).collect()
}

/// The layout of the made-up track, as `race_out.json` names it.
pub fn layout(variant: usize) -> &'static str {
    if variant.is_multiple_of(3) {
        "layout"
    } else {
        ""
    }
}

/// A made-up car.
#[derive(Clone, Debug)]
pub struct ToyCar {
    pub lap_seconds: f32,
    pub position: f32,
    lap_start: f64,
    sector_start: f64,
    sector: usize,
    laps: u32,
    in_pit_until: f64,
    parked: bool,
    pub retired: bool,
    retire_at: Option<f64>,
    pub black: bool,
    pub speed: f32,
    pub filtered: f32,
    cuts: i32,
    off_until: f64,
    reverse_until: f64,
    against_until: f64,
}

/// What the made-up player pressed in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    None,
    NextSession,
    RestartSession,
}

/// What a frame of a made-up drive gives the referee.
#[derive(Clone, Debug)]
pub struct ToyFrame {
    pub views: Vec<CarView>,
    /// `physicsState.speed` of every car, m/s
    pub speeds: Vec<f32>,
    pub splits: Vec<OnSectorSplitEvent>,
    pub laps: Vec<OnLapCompletedEvent>,
    pub flag_cars: Vec<FlagCar>,
    pub penalty: PlayerPenalty,
    /// the player's car was black-flagged or the flag was taken back in this frame
    pub black_flag: Option<bool>,
}

/// A made-up drive.
pub struct Toy {
    script: MsvcRand,
    pub cars: Vec<ToyCar>,
    compounds: Vec<String>,
    names: Vec<Player>,
    penalty: PlayerPenalty,
    was_black: bool,
}

impl Toy {
    pub fn new(seed: u32, variant: usize, cars: usize, now: f64) -> Toy {
        let mut toy = Toy { script: MsvcRand(seed ^ 0x5eed_0028), cars: Vec::new(), compounds: Vec::new(), names: Vec::new(), penalty: PlayerPenalty::default(), was_black: false };
        for (player, compound) in players(variant, cars) {
            let lap_seconds = 7.0 + 6.0 * toy.unit();
            toy.cars.push(ToyCar { lap_seconds, position: 0.0, lap_start: now, sector_start: now, sector: 0, laps: 0, in_pit_until: now + 1500.0, parked: false, retired: false, retire_at: None, black: false, speed: 0.0, filtered: 0.0, cuts: 0, off_until: 0.0, reverse_until: 0.0, against_until: 0.0 });
            toy.names.push(player);
            toy.compounds.push(compound);
        }
        toy
    }

    fn unit(&mut self) -> f32 {
        self.script.next() as f32 / 32768.0
    }

    /// What the referee's actions of a frame do to the made-up cars (before the next frame).
    pub fn apply(&mut self, actions: &[Action], now: f64) {
        for action in actions {
            match action {
                Action::NewSession { session, .. } => {
                    for index in 0..self.cars.len() {
                        let pit = if session.spawn_set == "PIT" { now + 1000.0 + 3000.0 * self.unit() as f64 } else { 0.0 };
                        let retire = if self.unit() < 0.15 { Some(now + 20000.0 + 60000.0 * self.unit() as f64) } else { None };
                        let car = &mut self.cars[index];
                        car.position = 0.0;
                        car.lap_start = now;
                        car.sector_start = now;
                        car.sector = 0;
                        car.laps = 0;
                        car.cuts = 0;
                        car.parked = false;
                        car.retired = false;
                        car.speed = 0.0;
                        car.in_pit_until = pit;
                        car.retire_at = retire;
                    }
                }
                Action::GoToPit(car) => {
                    self.cars[*car].parked = true;
                    self.cars[*car].speed = 0.0;
                }
                Action::RetirePlayer => self.cars[0].retired = true,
                _ => {}
            }
        }
    }

    /// The cars as the manager sees them now.
    pub fn views(&self, now: f64) -> Vec<CarView> {
        self.cars
            .iter()
            .enumerate()
            .map(|(index, car)| CarView {
                normalized_spline_position: car.position,
                is_in_pit: car.parked || now < car.in_pit_until,
                filtered_speed: car.filtered,
                is_retired: Some(car.retired),
                is_connected: true,
                is_black_flagged: car.black,
                loaded_tyres: Some(4),
                compound: self.compounds[index].clone(),
                driver_name: self.names[index].name.clone(),
                gui_name: self.names[index].car.clone(),
            })
            .collect()
    }

    /// One frame at clock `now` (ms): the cars drive (`started`: the session has started).
    pub fn frame(&mut self, now: f64, started: bool) -> ToyFrame {
        let mut splits = Vec::new();
        let mut laps = Vec::new();
        for index in 0..self.cars.len() {
            if let Some(at) = self.cars[index].retire_at {
                if now >= at && !self.cars[index].retired {
                    self.cars[index].retired = true;
                    self.cars[index].retire_at = None;
                }
            }
            if !self.cars[index].parked && !self.cars[index].retired && self.unit() < 0.0004 {
                self.cars[index].in_pit_until = now + 2000.0 + 4000.0 * self.unit() as f64;
            }
            if index == 0 && self.unit() < if self.cars[index].black { 0.0005 } else { 0.00003 } {
                self.cars[index].black = !self.cars[index].black;
            }
            let chance = self.unit();
            if chance < 0.0003 {
                self.cars[index].off_until = now + 1500.0 + 2500.0 * self.unit() as f64;
            } else if chance < 0.0005 {
                self.cars[index].reverse_until = now + 1000.0 + 2000.0 * self.unit() as f64;
            } else if chance < 0.0007 {
                self.cars[index].against_until = now + 1000.0 + 2000.0 * self.unit() as f64;
            }
            let in_pit = self.cars[index].parked || now < self.cars[index].in_pit_until;
            let driving = started && !in_pit && !self.cars[index].retired;
            self.cars[index].speed = if driving { TRACK_LENGTH / self.cars[index].lap_seconds } else { 0.0 };
            if driving {
                if self.unit() < 0.0006 {
                    self.cars[index].cuts += 1;
                }
                let car = &mut self.cars[index];
                let before = car.position;
                car.position += DT / car.lap_seconds;
                let line = (car.sector + 1) as f32 / SECTORS as f32;
                if before < line && car.position >= line && car.sector + 1 < SECTORS {
                    splits.push(OnSectorSplitEvent { car_index: index as u32, sector_index: car.sector as i32, sector_time: (now - car.sector_start) as u32, cuts: car.cuts });
                    car.sector += 1;
                    car.sector_start = now;
                }
                if self.cars[index].position >= 1.0 {
                    let valid = self.unit() >= 0.03;
                    let car = &mut self.cars[index];
                    car.position -= 1.0;
                    if valid {
                        car.laps += 1;
                    }
                    laps.push(OnLapCompletedEvent { car_index: index as u32, lap_time: (now - car.lap_start) as u32, lap_count: car.laps, splits: Vec::new(), event_time: now, is_valid: valid, cuts: car.cuts });
                    car.lap_start = now;
                    car.sector_start = now;
                    car.sector = 0;
                    car.cuts = 0;
                }
            }
            let car = &mut self.cars[index];
            car.filtered = filter_speed(car.filtered, car.speed, DT);
        }
        let views = self.views(now);
        // what the flags read: a car in its pit box is in the pit lane; now and then one is
        // off the track, in reverse, or points the wrong way
        let flag_cars = self
            .cars
            .iter()
            .zip(&views)
            .map(|(car, view)| FlagCar {
                is_in_pitlane: view.is_in_pit,
                tyres_off_track: if now < car.off_until { 4 } else { (car.cuts > 0) as u32 * 2 },
                gear: if now < car.reverse_until { 0 } else { 3 },
                speed: car.speed,
                against_road: if now < car.against_until { 0.9 } else { -0.8 },
            })
            .collect();
        let chance = self.unit();
        if chance < 0.0002 {
            self.penalty.penalty_time = if self.penalty.penalty_time > 0.0 || chance < 0.00004 { 0.0 } else { 5.0 };
        } else if chance < 0.0004 && (self.penalty.pit_penalty_laps > 0 || chance < 0.00024) {
            self.penalty.pit_penalty_laps = if self.penalty.pit_penalty_laps > 0 { 0 } else { 3 };
        }
        let black_flag = (self.cars[0].black != self.was_black).then(|| {
            self.was_black = self.cars[0].black;
            self.was_black
        });
        ToyFrame { speeds: self.cars.iter().map(|car| car.speed).collect(), views, splits, laps, flag_cars, penalty: self.penalty, black_flag }
    }

    /// What the made-up player presses after a frame.
    pub fn button(&mut self) -> Button {
        let button = self.unit();
        if button < 0.00012 {
            Button::NextSession
        } else if button < 0.00018 {
            Button::RestartSession
        } else {
            Button::None
        }
    }
}

/// The port's side of a made-up drive: the manager, the flags and the file writer, stepped
/// frame by frame. The oracle steps the game's own objects beside it.
pub struct PortDrive {
    pub toy: Toy,
    pub manager: RaceManager,
    pub flags: FlagManager,
    pub shown: Shown,
    pub json: JsonOutputFile,
    pub players: Vec<Player>,
    pub rand: MsvcRand,
    pub now: f64,
    pub penalty_mode: i32,
    /// the actions of the last frame, which the made-up cars feel before the next one
    pending: Vec<Action>,
    pub sessions_ended: usize,
    pub laps: usize,
    /// frames with each flag as the current one
    pub flag_frames: [usize; 7],
    /// the port's frame is a microsecond longer (a fault, to prove a comparison can fail)
    pub fault: bool,
}

/// What a frame of [`PortDrive`] did.
pub struct PortFrame {
    pub frame: ToyFrame,
    /// the manager's actions of the frame, in order (with the start of a race that follows a
    /// session change, and what the player's buttons did)
    pub actions: Vec<Action>,
    /// a new session began in the frame
    pub new_session: bool,
    pub button: Button,
}

impl PortDrive {
    /// The manager as `RaceManager::initOffline` leaves it and `Sim::Sim`'s `startRace`.
    /// Returns the drive and the actions of its start.
    pub fn new(seed: u32, variant: usize, cars: usize) -> (PortDrive, Vec<Action>) {
        let now = 100_000.0f64;
        let ini = sessions(variant, cars);
        let mut manager = RaceManager::new(&ini, cars, SECTORS, OVER_TIME_MULT);
        let mut actions = Vec::new();
        manager.set_current_session(0, now, &mut actions);
        manager.reset_current_laps_and_mandatory_pit(&mut actions);
        let mut rand = MsvcRand(seed);
        let draws = manager.start_race_draws();
        manager.start_race(20000.0, if draws { rand.next() } else { 0 }, &mut actions);
        let json = JsonOutputFile::new("toy_track", layout(variant), ini.sessions.len() as i32);
        let mut drive = PortDrive {
            toy: Toy::new(seed, variant, cars, now),
            manager,
            flags: FlagManager::new(cars, TRACK_LENGTH, None),
            shown: Shown::default(),
            json,
            players: players(variant, cars).into_iter().map(|(player, _)| player).collect(),
            rand,
            now,
            penalty_mode: 0,
            pending: Vec::new(),
            sessions_ended: 0,
            laps: 0,
            flag_frames: [0; 7],
            fault: false,
        };
        drive.absorb(&actions);
        drive.pending = actions.clone();
        (drive, actions)
    }

    fn absorb(&mut self, actions: &[Action]) {
        for action in actions {
            match action {
                Action::PenaltyMode(mode) => self.penalty_mode = *mode,
                Action::SessionEnd { event, index } => {
                    self.json.end_session(*index, event, self.manager.car_count(), None);
                    self.sessions_ended += 1;
                }
                _ => {}
            }
        }
    }

    /// The first half of a frame: the clock goes on, the cars drive. The oracle hands the
    /// frame to the game before [`PortDrive::finish_frame`].
    pub fn begin_frame(&mut self) -> ToyFrame {
        let pending = std::mem::take(&mut self.pending);
        if pending.iter().any(|action| matches!(action, Action::NewSession { .. })) {
            // (FlagManager's handler of the new-session event)
            self.flags.clear_flag();
        }
        self.toy.apply(&pending, self.now);
        self.now += (DT * 1000.0) as f64;
        self.manager.now = self.now;
        let started = self.manager.has_session_started();
        let frame = self.toy.frame(self.now, started);
        self.laps += frame.laps.len();
        if let Some(flag) = frame.black_flag {
            self.flags.on_black_flag(flag, 1);
        }
        frame
    }

    /// The second half: the events, the manager's frame, the start of a race that follows a
    /// session change, the player's buttons, the flags' frame.
    pub fn finish_frame(&mut self, frame: ToyFrame) -> PortFrame {
        for split in &frame.splits {
            self.manager.on_sector_split(split);
        }
        for lap in &frame.laps {
            self.manager.on_lap_completed(lap, &frame.views);
        }
        let mut actions = Vec::new();
        let mut rand = self.rand;
        self.manager.update(if self.fault { DT + 1.0e-6 } else { DT }, self.now, &frame.views, &mut || rand.next(), &mut actions);
        self.rand = rand;
        let new_session = actions.iter().any(|action| matches!(action, Action::NewSession { .. }));
        if new_session && self.manager.current_session.session_type == SESSION_RACE {
            let draws = self.manager.start_race_draws();
            self.manager.start_race(10000.0, if draws { self.rand.next() } else { 0 }, &mut actions);
        }
        let button = self.toy.button();
        match button {
            Button::NextSession => self.manager.skip_current_session(&mut actions),
            Button::RestartSession => self.manager.restart_current_session(&mut actions),
            Button::None => {}
        }
        self.absorb(&actions);
        self.shown = self.flags.update(DT, &self.manager, &frame.views, &frame.flag_cars, &frame.penalty, self.manager.results_displayed);
        self.flag_frames[self.flags.current_flag as usize] += 1;
        self.pending = actions.clone();
        PortFrame { frame, actions, new_session, button }
    }

    /// The whole state in words: the manager's ([`super::referee::dump`]) and the flags'.
    pub fn dump(&self) -> Vec<(String, String)> {
        let mut out = super::referee::dump(&self.manager, self.penalty_mode);
        let places: Vec<String> = self.flags.last_position_on_spline.iter().map(|p| format!("{:08x}", p.to_bits())).collect();
        let s = &self.shown;
        out.push(("flag".to_string(), format!("{} {:08x} [{}] {}{}{}{}{}", self.flags.current_flag as i32, self.flags.timer.to_bits(), places.join(" "), s.blue as u8, s.black as u8, s.chequered as u8, s.yellow as u8, s.penalty as u8)));
        out
    }

    /// The end of the program: the last session's results, and `race_out.json`.
    pub fn shutdown(&mut self) -> (Vec<Action>, Vec<u8>) {
        let views = self.toy.views(self.now);
        let mut actions = Vec::new();
        let mut rand = self.rand;
        self.manager.finalize_current_session(&views, &mut || rand.next(), &mut actions);
        self.rand = rand;
        self.absorb(&actions);
        let best = self.manager.timing.instance_best_times.first().map_or(0, |lap| lap.time as i32);
        let file = self.json.file(&self.players, best);
        (actions, file)
    }
}

/// A whole made-up drive through the port alone: one hash over every frame's state, and the
/// bytes of `race_out.json`.
pub fn run(seed: u32, variant: usize, cars: usize, frames: usize) -> (u64, Vec<u8>, PortDrive) {
    let (mut drive, _) = PortDrive::new(seed, variant, cars);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for _ in 0..frames {
        let frame = drive.begin_frame();
        drive.finish_frame(frame);
        hash = (hash ^ super::referee::dump_hash(&drive.dump())).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let (_, file) = drive.shutdown();
    hash = (hash ^ super::referee::dump_hash(&drive.dump())).wrapping_mul(0x0000_0100_0000_01b3);
    (hash, file, drive)
}
