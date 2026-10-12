// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `RaceTimingServices` (RaceTimingServices.obj, 0xc0 bytes): one lap list per car, the best
//! laps and sectors of the session, and the leaderboard.
//!
//! A car is its index in `Sim::cars` here (the game keys by `CarAvatar*`; its lap lists are
//! made in index order and looked up by pointer, which comes to the same).

use crate::msvc_sort;
use crate::track::timing::{OnLapCompletedEvent, OnSectorSplitEvent};

/// `Lap` (0x48 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lap {
    /// ms
    pub time: u32,
    pub splits: Vec<u32>,
    pub cuts: u32,
    pub is_valid: bool,
    /// the tyre compound's short name at the line
    pub compound: String,
}

/// `LapDB` (0xb0 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LapDb {
    pub total_time: f64,
    pub laps: Vec<Lap>,
    pub best_lap: Lap,
    /// `lastLapStartTime`: the clock at the last line crossing (the game starts it from its
    /// wall clock; here from the session's clock)
    pub last_lap_start_time: u32,
    pub current_splits: Vec<u32>,
    pub personal_best_splits: Vec<u32>,
    /// `bestLapSplit`: the last lap against the best one before it
    pub best_lap_split: i32,
    pub has_completed_last_lap: bool,
}

/// `LeaderboardEntry` (0x20 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LeaderboardEntry {
    pub car: usize,
    pub total_time: f64,
    pub best_lap: f64,
    pub laps: i32,
    pub is_race_mode: bool,
    pub is_black_flagged: bool,
    pub has_completed_last_lap: bool,
}

/// What the leaderboard reads of a car (`CarAvatar`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardCar {
    /// `CarAvatar::isBlackFlagged`
    pub is_black_flagged: bool,
    /// `CarAvatar::getSpawnPositionIndex(L"START")`
    pub start_index: i32,
}

/// `LeaderboardEntry::operator<` @ 0x140144130. Not a strict weak ordering (two cars without
/// a lap are each "before" the other outside a race): the result is whatever the game's sort
/// makes of it.
pub fn entry_before(a: &LeaderboardEntry, b: &LeaderboardEntry, start_index: &dyn Fn(usize) -> i32) -> bool {
    if b.is_black_flagged {
        return false;
    }
    if !a.is_race_mode {
        if a.best_lap > 0.0 && b.best_lap > 0.0 {
            return a.best_lap < b.best_lap;
        }
        return b.best_lap == 0.0;
    }
    if a.laps != b.laps {
        return b.laps < a.laps;
    }
    if a.laps == 0 {
        return start_index(a.car) < start_index(b.car);
    }
    a.total_time < b.total_time
}

/// `RaceTimingServices`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceTimingServices {
    /// `lapDBs`, by car
    pub lap_dbs: Vec<LapDb>,
    pub leaderboard: Vec<LeaderboardEntry>,
    /// `instanceBestTimes`: each car's best lap of the whole run (never cleared)
    pub instance_best_times: Vec<Lap>,
    /// `bestSplits`: empty until the first `resetCurrentLaps`
    pub best_splits: Vec<u32>,
    pub global_best_lap: u32,
    /// `sim->track->physicsTrack.sectorsNormalizedPositions.size()`
    pub sector_count: usize,
}

impl RaceTimingServices {
    /// The constructor @ 0x140143c60 (the leaderboard is made by the first `update`).
    pub fn new(cars: usize, sector_count: usize) -> RaceTimingServices {
        RaceTimingServices { lap_dbs: vec![LapDb::default(); cars], leaderboard: Vec::new(), instance_best_times: vec![Lap::default(); cars], best_splits: Vec::new(), global_best_lap: 0, sector_count }
    }

    /// `onNewSession` @ 0x140145870: new lap lists (`now`: the clock a new list starts from).
    pub fn on_new_session(&mut self, now: u32) {
        for db in &mut self.lap_dbs {
            *db = LapDb { last_lap_start_time: now, ..LapDb::default() };
        }
    }

    /// `resetCurrentLaps` @ 0x140145d30.
    pub fn reset_current_laps(&mut self) {
        self.best_splits = vec![0; self.sector_count];
        self.global_best_lap = 0;
        for db in &mut self.lap_dbs {
            db.personal_best_splits = vec![0; self.sector_count];
            db.current_splits.clear();
        }
    }

    /// `onLapCompleted` @ 0x140145300 (`compound`: `CarAvatar::getCurrentCompoundShortName`).
    /// The caller rebuilds the leaderboard afterwards. False: the lap was not listed.
    pub fn on_lap_completed(&mut self, e: &OnLapCompletedEvent, compound: &str) -> bool {
        if !e.is_valid {
            return false;
        }
        let car = e.car_index as usize;
        let db = &mut self.lap_dbs[car];
        if db.has_completed_last_lap {
            return false;
        }
        let mut lap = Lap { time: e.lap_time, splits: Vec::new(), cuts: e.cuts as u32, is_valid: e.cuts == 0, compound: compound.to_string() };
        if !self.best_splits.is_empty() {
            db.current_splits.push(0);
            let n = self.sector_count as i32;
            let last = db.current_splits.len() - 1;
            let mut i = 0;
            while i < n - 1 {
                db.current_splits[last] = db.current_splits[last].wrapping_add(db.current_splits[i as usize]);
                i += 1;
            }
            db.current_splits[last] = e.lap_time.wrapping_sub(db.current_splits[last]);
            if lap.is_valid {
                let value = db.current_splits[last];
                let g = self.best_splits.len() - 1;
                let p = db.personal_best_splits.len() - 1;
                if self.best_splits[g] == 0 || value < self.best_splits[g] {
                    self.best_splits[g] = value;
                    db.personal_best_splits[p] = value;
                } else if db.personal_best_splits[p] == 0 || value < db.personal_best_splits[p] {
                    db.personal_best_splits[p] = value;
                }
            }
            lap.splits = db.current_splits.clone();
        }
        db.best_lap_split = e.lap_time.wrapping_sub(db.best_lap.time) as i32;
        db.laps.push(lap.clone());
        db.total_time += e.lap_time as f64;
        if e.cuts == 0 {
            if db.best_lap.time == 0 || lap.time < db.best_lap.time {
                db.best_lap = lap.clone();
                if self.global_best_lap == 0 || lap.time < self.global_best_lap {
                    self.global_best_lap = lap.time;
                }
            }
            let best = &mut self.instance_best_times[car];
            if best.time == 0 || lap.time < best.time {
                *best = lap;
            }
        }
        db.current_splits.clear();
        db.last_lap_start_time = e.event_time as i64 as u32;
        true
    }

    /// `onSectorSplit` @ 0x140145940.
    pub fn on_sector_split(&mut self, e: &OnSectorSplitEvent) {
        let db = &mut self.lap_dbs[e.car_index as usize];
        let index = e.sector_index as u32 as usize;
        if index < db.current_splits.len() {
            db.current_splits[index] = e.sector_time;
        } else {
            db.current_splits.resize(index, 0);
            db.current_splits.push(e.sector_time);
        }
        if !self.best_splits.is_empty() && e.cuts == 0 {
            // (the game checks no bound here; a sector past the list is left alone)
            if index >= self.best_splits.len() || index >= db.personal_best_splits.len() {
                return;
            }
            let current = db.current_splits[index];
            if self.best_splits[index] == 0 || current < self.best_splits[index] {
                self.best_splits[index] = current;
                db.personal_best_splits[index] = current;
            } else if db.personal_best_splits[index] == 0 || current < db.personal_best_splits[index] {
                db.personal_best_splits[index] = current;
            }
        }
    }

    /// `updateLeaderboard` @ 0x140146660 (the game rebuilds it every frame and after a lap).
    pub fn update_leaderboard(&mut self, is_race: bool, cars: &[BoardCar]) {
        self.leaderboard.clear();
        for (car, db) in self.lap_dbs.iter().enumerate() {
            self.leaderboard.push(LeaderboardEntry {
                car,
                total_time: db.total_time,
                best_lap: db.best_lap.time as f64,
                laps: db.laps.len() as i32,
                is_race_mode: is_race,
                is_black_flagged: cars.get(car).is_some_and(|c| c.is_black_flagged),
                has_completed_last_lap: db.has_completed_last_lap,
            });
        }
        let start = |car: usize| cars.get(car).map_or(0, |c| c.start_index);
        msvc_sort::sort(&mut self.leaderboard, &|a: &LeaderboardEntry, b: &LeaderboardEntry| entry_before(a, b, &start));
    }

    /// `getCarLeaderboardPosition` @ 0x140144ce0: from 1; -1 for a car that is not listed.
    pub fn car_leaderboard_position(&self, car: usize) -> i32 {
        self.leaderboard.iter().position(|e| e.car == car).map_or(-1, |i| i as i32 + 1)
    }

    /// `getLapCount` @ 0x140144d80: every listed lap, with cuts or not.
    pub fn lap_count(&self, car: usize) -> u32 {
        self.lap_dbs[car].laps.len() as u32
    }

    /// `getLastLap` @ 0x140144f50.
    pub fn last_lap(&self, car: usize) -> Lap {
        self.lap_dbs[car].laps.last().cloned().unwrap_or_default()
    }

    /// `getSplit(car, i)` @ 0x140145210.
    pub fn split(&self, car: usize, index: i32) -> u32 {
        self.lap_dbs[car].current_splits.get(index as i64 as u64 as usize).copied().unwrap_or(0)
    }

    /// `getLastSplit` @ 0x140144fd0.
    pub fn last_split(&self, car: usize, index: i32) -> u32 {
        self.lap_dbs[car].laps.last().and_then(|lap| lap.splits.get(index as i64 as u64 as usize).copied()).unwrap_or(0)
    }

    /// `getBestSplit` @ 0x140144c00.
    pub fn best_split(&self, index: i32, global: bool, car: usize) -> u32 {
        let i = index as i64 as u64 as usize;
        if i >= self.sector_count {
            return 0;
        }
        if global {
            self.best_splits.get(i).copied().unwrap_or(0)
        } else {
            self.lap_dbs[car].personal_best_splits.get(i).copied().unwrap_or(0)
        }
    }

    /// `isBestSplit` @ 0x140145290 (the game checks no bound).
    pub fn is_best_split(&self, index: i32, time: i32, global: bool, car: usize) -> bool {
        if time == 0 {
            return false;
        }
        let list = if global { &self.best_splits } else { &self.lap_dbs[car].personal_best_splits };
        list.get(index as usize).is_some_and(|best| *best as i32 == time)
    }

    /// `shutdown` @ 0x140145f50: the text of `out/laps.ini`, the first car's laps without cuts
    /// (the game writes it in text mode: every line ends `\r\n`).
    pub fn laps_ini(&self) -> String {
        let empty = LapDb::default();
        let db = self.lap_dbs.first().unwrap_or(&empty);
        let mut out = String::new();
        out.push_str("[HEADER]\r\nVERSION=2\r\n");
        out.push_str(&format!("LAPS={}\r\n", db.laps.iter().filter(|lap| lap.is_valid).count()));
        for (k, lap) in db.laps.iter().filter(|lap| lap.is_valid).enumerate() {
            out.push_str(&format!("\r\n[LAP_{k}]\r\nTIME={}\r\nSPLITS={}\r\n", lap.time, lap.splits.len()));
            for (j, split) in lap.splits.iter().enumerate() {
                out.push_str(&format!("SPLIT_{j}={split}\r\n"));
            }
        }
        out
    }
}
