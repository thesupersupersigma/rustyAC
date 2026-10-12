// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SessionResultProcessor` (SessionResultProcessor.obj): a session's results, with the laps
//! the AI cars would still have driven made up when a session ends before they are done.
//!
//! The game works on one global `SessionResult` (0x141559b70) that is only partly cleared
//! between calls; it is the manager's `processor_result` here.

#![allow(clippy::needless_range_loop)]

use super::manager::{CarView, RaceManager, SessionResult, SESSION_DRAG, SESSION_PRACTICE, SESSION_QUALIFY, SESSION_RACE};
use super::timing::Lap;
use crate::msvc_sort;

/// 0x38000100: what the game multiplies a `rand()` by (about 1 / 32767).
pub const RAND_SCALE: f32 = f32::from_bits(0x3800_0100);

/// `getCompleteAIResults` @ 0x140166c70. `rand`: the C runtime's `rand()`.
pub fn get_complete_ai_results(manager: &RaceManager, cars: &[CarView], res: &mut SessionResult, rand: &mut dyn FnMut() -> i32) -> SessionResult {
    match manager.current_session.session_type {
        SESSION_PRACTICE | SESSION_QUALIFY => process_qualify_results(manager, cars, res, rand),
        SESSION_RACE => process_race_results(manager, cars, res, rand),
        // processDragResults @ 0x140166d70: without a `DragMode` object nothing is touched
        SESSION_DRAG => {}
        _ => fetch_basic_results(manager, cars, res),
    }
    res.clone()
}

/// `fetchBasicResults` @ 0x140166230.
pub fn fetch_basic_results(manager: &RaceManager, cars: &[CarView], res: &mut SessionResult) {
    let n = manager.car_count();
    res.laps.clear();
    res.positions.clear();
    res.positions.resize(n, -1);
    for car in 0..n {
        res.laps.push(manager.timing.lap_dbs[car].laps.clone());
    }
    for (k, entry) in manager.timing.leaderboard.iter().enumerate() {
        if k < res.positions.len() {
            res.positions[k] = if manager.is_race_over(entry.car, cars) { entry.car as i32 } else { -1 };
        }
    }
}

#[derive(Clone, Default)]
struct BestLap {
    car: usize,
    best_lap: Lap,
    laps: Vec<Lap>,
}

/// The stream's count of elements as a float (`cvtsi2ss` of the 64-bit size).
fn count_f32(len: usize) -> f32 {
    len as i64 as f32
}

/// `processQualifyResults` @ 0x140167110 (practice and qualifying): every AI car gets the
/// laps it had time left for, near the best lap of its kind of car.
fn process_qualify_results(manager: &RaceManager, cars: &[CarView], res: &mut SessionResult, rand: &mut dyn FnMut() -> i32) {
    res.laps.clear();
    res.positions.clear();
    res.lap_count.clear();
    res.best_laps.clear();
    let n = manager.car_count();
    let timing = &manager.timing;
    let mut entries: Vec<BestLap> = Vec::with_capacity(n);
    let mut min_best = 0x7fff_ffffu32;
    for car in 0..n {
        let db = &timing.lap_dbs[car];
        entries.push(BestLap { car, best_lap: db.best_lap.clone(), laps: db.laps.clone() });
        if db.best_lap.time < min_best {
            min_best = db.best_lap.time;
        }
    }
    let mut best = Lap { time: 0x7fff_ffff, ..Lap::default() };
    let mut has_best = false;
    let mut best_car = 0usize;
    for car in 0..n {
        if manager.lap_count(car) != 0 {
            let lap = &timing.lap_dbs[car].best_lap;
            if lap.time < best.time {
                best = lap.clone();
                best_car = car;
                has_best = true;
            }
        }
    }
    let time_left = manager.session_time_left();
    if cvttsd2si(time_left) > min_best as i32 && n != 0 {
        for e in entries.iter_mut() {
            if e.car == 0 {
                continue;
            }
            let mut count = if e.laps.is_empty() { 1 } else { cvttsd2si(time_left / e.best_lap.time as f64) };
            while count > 0 {
                let mut tmp = best.clone();
                if !has_best {
                    tmp.time = 600_000;
                } else if cars[e.car].gui_name != cars[best_car].gui_name {
                    for other in 0..n {
                        if cars[other].gui_name == cars[e.car].gui_name && manager.lap_count(other) != 0 {
                            let lap = &timing.lap_dbs[other].best_lap;
                            if lap.time < tmp.time {
                                tmp = lap.clone();
                            }
                        }
                    }
                }
                if manager.lap_count(e.car) > 1 {
                    tmp = e.best_lap.clone();
                }
                let base = tmp.time;
                let r = rand();
                tmp.time = tmp.time.wrapping_add(((r as f32 * RAND_SCALE - 0.7) * 100.0) as i32 as u32);
                let d = base.wrapping_sub(tmp.time) as i32 as f32;
                let share = count_f32(tmp.splits.len());
                for s in tmp.splits.iter_mut() {
                    *s = s.wrapping_sub(cvttss2si(d / share) as u32);
                }
                if e.best_lap.time == 0 || tmp.time < e.best_lap.time {
                    e.best_lap = tmp.clone();
                }
                e.laps.push(tmp);
                count -= 1;
            }
        }
    }
    for e in &entries {
        res.best_laps.push(e.best_lap.clone());
        res.laps.push(e.laps.clone());
        res.lap_count.push(e.laps.len() as i32);
    }
    // (the game sorts the records themselves; the keys are all the comparator reads)
    let mut order: Vec<(usize, u32)> = entries.iter().map(|e| (e.car, e.best_lap.time)).collect();
    msvc_sort::sort(&mut order, &|a: &(usize, u32), b: &(usize, u32)| a.1 != 0 && (b.1 == 0 || a.1 < b.1));
    for (car, _) in order {
        res.positions.push(car as i32);
    }
}

/// `cvttsd2si` to 32 bits: out of range gives 0x80000000.
fn cvttsd2si(value: f64) -> i32 {
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// `cvttss2si` to 32 bits.
fn cvttss2si(value: f32) -> i32 {
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// `cvttss2si` to 64 bits, the low half kept.
fn cvttss2si64_u32(value: f32) -> u32 {
    if !(-9.223_372e18..9.223_372e18).contains(&value) {
        0
    } else {
        value as i64 as u32
    }
}

/// `getAverageLap` @ 0x140166730: the car's laps without the first one, averaged. `None`
/// where the game would divide by zero (a car without a lap: never asked).
fn get_average_lap(manager: &RaceManager, car: usize) -> Option<Lap> {
    let db = &manager.timing.lap_dbs[car];
    let mut laps = db.laps.clone();
    let mut out = Lap { splits: vec![0; manager.timing.last_lap(car).splits.len()], ..Lap::default() };
    if laps.len() > 1 {
        laps.remove(0);
    } else if laps.len() == 1 {
        // RaceManager::getCurrentLap @ 0x1401380b0: the splits of the lap being driven
        if let Some(&first) = db.current_splits.first() {
            if let Some(slot) = laps[0].splits.first_mut() {
                *slot = first;
            }
            laps[0].time = laps[0].splits.iter().fold(0u32, |a, &b| a.wrapping_add(b));
        }
    }
    if laps.is_empty() {
        return None;
    }
    for lap in &laps {
        out.time = out.time.wrapping_add(lap.time);
        for (j, slot) in out.splits.iter_mut().enumerate() {
            *slot = slot.wrapping_add(lap.splits.get(j).copied().unwrap_or(0));
        }
    }
    let m = laps.len() as u64;
    out.time = (out.time as u64 / m) as u32;
    for slot in out.splits.iter_mut() {
        *slot = (*slot as u64 / m) as u32;
    }
    Some(out)
}

/// `disqualifyCar` @ 0x140166100.
fn disqualify_car(car: i32, positions: &mut Vec<i32>) {
    if let Some(i) = positions.iter().position(|p| *p == car) {
        positions.remove(i);
        positions.push(car);
    }
}

/// `processRaceResults` @ 0x140167f30: the AI cars that were still racing get their missing
/// laps (their average lap, +-0.5 s); the order is laps, then total time, retired cars last.
fn process_race_results(manager: &RaceManager, cars: &[CarView], res: &mut SessionResult, rand: &mut dyn FnMut() -> i32) {
    fetch_basic_results(manager, cars, res);
    let n = manager.car_count();
    let retired = |car: usize| cars[car].is_retired == Some(true);
    for car in 0..n {
        if !manager.is_race_over(car, cars) {
            let place = manager.car_real_time_position(car, cars);
            if let Some(slot) = res.positions.get_mut(place.max(0) as usize) {
                *slot = car as i32;
            }
        }
    }
    if n > 0 && !manager.is_race_over(0, cars) {
        if let Some(k) = res.positions.iter().position(|p| *p == 0) {
            res.positions.remove(k);
        }
        res.positions.push(0);
    }
    let mut abort = false;
    let mut k = 0;
    while k < res.positions.len() {
        let g = res.positions[k];
        k += 1;
        if g == 0 {
            continue;
        }
        if g < 0 || g as usize >= n {
            abort = true;
            break;
        }
        if !retired(g as usize) {
            if manager.lap_count(g as usize) == 0 {
                abort = true;
                break;
            }
        } else {
            disqualify_car(g, &mut res.positions);
        }
    }
    if abort {
        res.laps.clear();
        for car in 1..n {
            res.lap_count.push(if !retired(car) { manager.current_session.laps } else { manager.lap_count(car) as i32 });
        }
        return;
    }
    let average: Vec<Lap> = (0..n).map(|car| if manager.lap_count(car) == 0 { Lap::default() } else { get_average_lap(manager, car).unwrap_or_default() }).collect();
    for car in 1..n {
        if !retired(car) && !manager.is_race_over(car, cars) {
            let mut remaining = manager.current_session.laps.wrapping_sub(manager.lap_count(car) as i32);
            if manager.current_session.is_over {
                remaining = 1;
            }
            let start = manager.lap_count(car);
            let end = start.wrapping_add(remaining as u32);
            let mut c = start;
            while c < end {
                let mut lap = average[car].clone();
                let r = rand();
                let f = (r as f32 * RAND_SCALE * 2.0 - 1.0) * 500.0;
                lap.time = cvttss2si64_u32(lap.time as u64 as f32 + f);
                let share = count_f32(lap.splits.len());
                for s in lap.splits.iter_mut() {
                    *s = cvttss2si64_u32(f / share + *s as f32);
                }
                res.laps[car].push(lap);
                c += 1;
            }
        }
    }
    // (avatarIndex, totalTime, lapCount) and (avatarIndex, lapCount)
    let mut finished: Vec<(i32, u32, u32)> = Vec::new();
    let mut out: Vec<(i32, u32)> = Vec::new();
    for car in 0..res.laps.len().min(n) {
        if !retired(car) {
            let total = res.laps[car].iter().fold(0u32, |a, lap| a.wrapping_add(lap.time));
            finished.push((car as i32, total, res.laps[car].len() as u32));
        } else {
            out.push((car as i32, manager.lap_count(car)));
        }
    }
    msvc_sort::sort(&mut finished, &|a: &(i32, u32, u32), b: &(i32, u32, u32)| if a.2 != b.2 { a.2 > b.2 } else { a.1 < b.1 });
    msvc_sort::sort(&mut out, &|a: &(i32, u32), b: &(i32, u32)| a.1 < b.1);
    for (k, car) in finished.iter().map(|f| f.0).chain(out.iter().map(|r| r.0)).enumerate() {
        if let Some(slot) = res.positions.get_mut(k) {
            *slot = car;
        }
    }
}
