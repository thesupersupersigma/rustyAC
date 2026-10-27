// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 43: what `ACClient` keeps of a session's laps, and the session's results made from it
//! (`RaceManager`'s online end of a session), for `race_out.json` after an online session.
//!
//! Cars go by their number in the session (`CarAvatar::getGuid`): 0 is the player's car,
//! then the other slots in the order of the server's car list.

use rustyac_physics::race::{Lap, OnSessionEndEvent, Session, SessionResult};

use crate::messages::{LeaderboardRow, NO_TIME};

/// One car's laps: `NetCarStateProvider` +0x120 / +0x160 / +0x118, or `ACClient`'s
/// `localCarCurrentSplits` / `localCarLaps` / `localCarBestLap` for car 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarLaps {
    pub current_splits: Vec<u32>,
    pub laps: Vec<Lap>,
    pub best: u32,
}

/// `ACClient`'s laps of the session and its `sessionResultsGT` (by car number, in order).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionLaps {
    /// the track's sector count (`bestSplits` has as many)
    pub sectors: usize,
    pub cars: Vec<CarLaps>,
    /// `sessionResultsGT`: (car, time ms, laps) in the server's order; empty in a new
    /// practice or qualifying until the first lap
    pub results: Vec<(usize, u32, u32)>,
}

impl SessionLaps {
    pub fn new(cars: usize, sectors: usize) -> SessionLaps {
        SessionLaps { sectors, cars: vec![CarLaps::default(); cars], results: Vec::new() }
    }

    /// `ACClient::resetCurrentLaps` @ 0x140051930 (a new session).
    pub fn reset(&mut self) {
        for car in &mut self.cars {
            *car = CarLaps::default();
        }
    }

    /// `ACClient::addSplitToBest` @ 0x1400442e0 on the car's current splits: the player's
    /// own (`onSectorSplit` @ 0x14004ff70) or another car's (`onRemoteSectorSplit`
    /// @ 0x14004fdc0).
    pub fn sector_split(&mut self, car: usize, sector: usize, time: u32) {
        let Some(car) = self.cars.get_mut(car) else { return };
        if car.current_splits.len() <= sector {
            car.current_splits.resize(sector + 1, 0);
        }
        car.current_splits[sector] = time;
    }

    /// `ACClient::onRemoteLapCompleted` @ 0x14004f470 for the car of the message's slot: the
    /// lap goes on its list (with the car's compound now), then `storeSplitToLap`: the last
    /// sector is the lap's time less the others.
    pub fn lap_completed(&mut self, car: usize, time: u32, cuts: u8, compound: &str) {
        let sectors = self.sectors;
        let Some(car) = self.cars.get_mut(car) else { return };
        car.laps.push(Lap { time, splits: Vec::new(), cuts: cuts as u32, is_valid: cuts == 0, compound: compound.to_string() });
        if cuts == 0 && (time < car.best || car.best == 0) {
            car.best = time;
        }
        if sectors > 0 {
            // shortcut: a sector the car missed reads 0 here; the game reads past its list then
            let before: u32 = (0..sectors - 1).map(|k| car.current_splits.get(k).copied().unwrap_or(0)).fold(0u32, u32::wrapping_add);
            car.current_splits.push(0);
            let last = car.current_splits.len() - 1;
            car.current_splits[last] = time.wrapping_sub(before);
            car.laps.last_mut().expect("just pushed").splits = car.current_splits.clone();
        }
        car.current_splits.clear();
    }

    /// The results the lap message brings (`lambda_e70edf`): `slot_car` turns a slot into a
    /// car number.
    pub fn set_results(&mut self, rows: &[LeaderboardRow], slot_car: impl Fn(u8) -> Option<usize>) {
        self.results = rows.iter().filter_map(|row| Some((slot_car(row.slot)?, if row.time == NO_TIME { 0 } else { row.time }, row.laps as u32))).collect();
    }

    /// `ACClient::onNewSession` @ 0x14004e4a0: a race starts with every car of `grid` at no
    /// time and no laps; another session with none.
    pub fn new_session_results(&mut self, race: bool, grid: &[u8], slot_car: impl Fn(u8) -> Option<usize>) {
        self.results = if race { grid.iter().filter_map(|slot| Some((slot_car(*slot)?, 0, 0))).collect() } else { Vec::new() };
    }

    /// `RaceManager::getLapCount` @ 0x140138570 of every car: the results' laps of the others
    /// (their laps listed without results), `local_lap_count` (`physicsState.lapCount`) for
    /// the player's.
    pub fn lap_counts(&self, local_lap_count: u32) -> Vec<u32> {
        (0..self.cars.len())
            .map(|car| {
                if car == 0 {
                    return local_lap_count;
                }
                // (lapCounter has a place for every car once any results came)
                if !self.results.is_empty() {
                    return self.results.iter().find(|r| r.0 == car).map_or(0, |r| r.2);
                }
                self.cars[car].laps.len() as u32
            })
            .collect()
    }

    /// The session's results as `RaceManager` makes them online: at the end of the program
    /// (`RaceManager::shutdown` @ 0x140141320) and when the server goes on from a practice or
    /// qualifying (`lambda_9cebb0` @ 0x140133f90). `session`: the manager's current session
    /// (`convertRemoteSession` of the server's).
    pub fn end_event(&self, session: &Session) -> OnSessionEndEvent {
        let cars = self.cars.len();
        let mut result = SessionResult { laps: self.cars.iter().map(|car| car.laps.clone()).collect(), best_laps: vec![Lap::default(); cars], positions: vec![0; cars], lap_count: vec![0; cars], total: vec![0.0; cars] };
        // getLeaderboard: mpCacheLeaderboard, the results in order (every car without laps
        // when there are none)
        let board: Vec<(usize, u32, u32)> = if self.results.is_empty() { (0..cars).map(|car| (car, 0, 0)).collect() } else { self.results.clone() };
        if session.session_type == 1 || session.session_type == 2 {
            for (car, laps) in self.cars.iter().enumerate() {
                // (a negative int is `Undefined`, 0)
                result.best_laps[car] = Lap { time: if (laps.best as i32) < 0 { 0 } else { laps.best }, is_valid: true, ..Lap::default() };
            }
        } else {
            for &(car, time, _) in &board {
                if let Some(total) = result.total.get_mut(car) {
                    *total = time as f32;
                }
            }
        }
        for (place, &(car, _, laps)) in board.iter().enumerate() {
            if let Some(position) = result.positions.get_mut(place) {
                *position = car as i32;
            }
            if let Some(count) = result.lap_count.get_mut(car) {
                *count = laps as i32;
            }
        }
        OnSessionEndEvent { current_session: session.clone(), result }
    }
}

/// `ACClient::getCurrentSessionIndex`: the first session of the server's list of the current
/// one's type (0 when none is).
pub fn session_index(kinds: &[u8], current: u8) -> usize {
    kinds.iter().position(|kind| *kind == current).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_sector_is_the_lap_less_the_others() {
        let mut laps = SessionLaps::new(2, 3);
        laps.sector_split(1, 0, 30_000);
        laps.sector_split(1, 1, 31_000);
        laps.lap_completed(1, 100_000, 0, "M");
        laps.lap_completed(1, 99_000, 2, "M");
        let car = &laps.cars[1];
        assert_eq!(car.laps[0].splits, vec![30_000, 31_000, 39_000]);
        assert!(!car.laps[1].is_valid);
        assert_eq!(car.best, 100_000);
        assert_eq!(laps.lap_counts(7), vec![7, 2]);
        laps.set_results(&[LeaderboardRow { slot: 4, time: 100_000, laps: 5, has_finished: false }, LeaderboardRow { slot: 2, time: NO_TIME, laps: 0, has_finished: false }], |slot| Some(if slot == 4 { 1 } else { 0 }));
        assert_eq!(laps.lap_counts(7), vec![7, 5]);
        let event = laps.end_event(&Session { session_type: 3, ..Session::default() });
        assert_eq!((event.result.positions, event.result.total), (vec![1, 0], vec![0.0, 100_000.0]));
        assert_eq!(session_index(&[1, 2, 3], 3), 2);
    }
}
