// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle's standings: stand-ins for the functions of `RaceManager`,
//! `CarAvatar` and `ACClient` the game's tables call, answering with the script's standings
//! (`gui_seq_menu::race_link`), and the script's values written into the stand-in cars.
//!
//! The referee itself (who is where, with which times) is the proof of Task 28; here only what
//! the tables make of an answer is held against the port.

use rustyac_gui::leaderboard::{RaceEntry, RaceLap, RaceLink};

use crate::ac::{rd, wr, write_wstring, Game};
use crate::gui_ac::read_wstring;
use crate::gui_ac_hud::{acs, hud, input, jump, HudGame};
use crate::gui_seq_menu::action;

const VA_GET_SESSION_INFO: usize = 0x1_4013_9d70;
const VA_GET_CURRENT_SESSION_INDEX: usize = 0x1_4013_82d0;
const VA_GET_LEADERBOARD_FROM_SESSION: usize = 0x1_4013_8b90;
const VA_GET_LEADERBOARD: usize = 0x1_4013_8ab0;
const VA_GET_BEST_LAP_OF_CAR: usize = 0x1_4013_7c00;
const VA_GET_BEST_LAP: usize = 0x1_4013_7e80;
const VA_GET_BEST_SPLIT: usize = 0x1_4013_7eb0;
const VA_IS_BEST_SPLIT: usize = 0x1_4013_eaa0;
const VA_GET_CURRENT_LAP: usize = 0x1_4013_80b0;
const VA_GET_LAST_SPLIT: usize = 0x1_4013_89d0;
const VA_GET_SPLIT: usize = 0x1_4013_a070;
const VA_GET_HAS_COMPLETED_FLAG: usize = 0x1_4013_8390;
const VA_CAR_GET_COMPOUND_SHORT_NAME: usize = 0x1_400d_3200;
const VA_CAR_GET_BALLAST_KG: usize = 0x1_400d_3110;
const VA_CAR_GET_RESTRICTOR: usize = 0x1_400d_3600;
const VA_CLIENT_SEND_VOTE: usize = 0x1_4005_2ba0;
const VA_SIM_SET_FOCUSED_CAR_INDEX: usize = 0x1_4019_e6b0;

pub fn race() -> RaceLink {
    crate::gui_seq_menu::race_link(&input(|i| i.clone()))
}

fn car_of(car: *const u8) -> usize {
    unsafe { rd::<i32>(car, 0x1158) as usize }
}

unsafe fn empty_wstring(at: *mut u8) {
    std::ptr::write_bytes(at, 0, 0x20);
    wr(at, 0x18, 7usize);
}

/// A `Lap` (0x48 bytes) of the game from one of the script's.
pub unsafe fn write_lap(out: *mut u8, lap: &RaceLap) {
    std::ptr::write_bytes(out, 0, 0x48);
    wr(out, 0, lap.time);
    if !lap.splits.is_empty() {
        let splits = acs().alloc(lap.splits.len() * 4);
        for (i, split) in lap.splits.iter().enumerate() {
            wr(splits, i * 4, *split);
        }
        wr(out, 0x8, splits);
        wr(out, 0x10, splits.add(lap.splits.len() * 4));
        wr(out, 0x18, splits.add(lap.splits.len() * 4));
    }
    wr(out, 0x24, lap.is_valid as u8);
    empty_wstring(out.add(0x28));
}

unsafe extern "C" fn get_session_info(_this: *mut u8, out: *mut u8, index: i32) -> *mut u8 {
    let session = race().session_info(index);
    std::ptr::write_bytes(out, 0, 0x70);
    write_session(out, &session);
    out
}

/// A `Session` (0x70 bytes) of the game from one of the script's.
unsafe fn write_session(out: *mut u8, session: &rustyac_gui::leaderboard::RaceSession) {
    wr(out, 0, session.session_type);
    wr(out, 4, session.is_timed_race as u8);
    wr(out, 0x8, session.laps);
    wr(out, 0xc, session.duration_minutes);
    empty_wstring(out.add(0x18));
    empty_wstring(out.add(0x38));
    write_wstring(acs(), out.add(0x38), &session.name);
    wr(out, 0x58, session.start_time);
}

/// In place of `RaceManager::getCurrentSession` 0x140138220.
unsafe extern "C" fn get_current_session(_this: *mut u8, out: *mut u8) -> *mut u8 {
    let race = race();
    std::ptr::write_bytes(out, 0, 0x70);
    let mut session = race.session_info(race.current_session_index);
    if race.sessions.is_empty() {
        // (a script that names no sessions: the HUD's own session type, as its objects read it)
        session.session_type = input(|i| i.session_type);
    }
    write_session(out, &session);
    out
}

unsafe extern "C" fn get_current_session_index(_this: *mut u8) -> i32 {
    race().current_session_index
}

/// A `std::vector<LeaderboardEntry>` the game will free: made with its own `operator new`.
unsafe fn write_entries(out: *mut u8, entries: &[RaceEntry]) {
    std::ptr::write_bytes(out, 0, 0x18);
    if entries.is_empty() {
        return;
    }
    let cars = hud().map(|h| h.cars.clone()).unwrap_or_default();
    let block = acs().alloc(entries.len() * 0x20);
    for (i, entry) in entries.iter().enumerate() {
        let e = block.add(i * 0x20);
        wr(e, 0, cars.get(entry.car).copied().unwrap_or(std::ptr::null_mut()));
        wr(e, 0x8, entry.total_time);
        wr(e, 0x10, entry.best_lap);
        wr(e, 0x18, entry.laps);
        wr(e, 0x1e, entry.has_completed_last_lap as u8);
    }
    wr(out, 0, block);
    wr(out, 0x8, block.add(entries.len() * 0x20));
    wr(out, 0x10, block.add(entries.len() * 0x20));
}

unsafe extern "C" fn get_leaderboard_from_session(_this: *mut u8, out: *mut u8, index: i32) -> *mut u8 {
    write_entries(out, &race().leaderboard_from_session(index));
    out
}

unsafe extern "C" fn get_leaderboard(_this: *mut u8, out: *mut u8) -> *mut u8 {
    write_entries(out, &race().leaderboard);
    out
}

unsafe extern "C" fn get_best_lap_of_car(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    let time = race().cars.get(car_of(car)).map_or(0, |c| c.best_lap);
    write_lap(out, &RaceLap { time, splits: Vec::new(), is_valid: true });
    out
}

unsafe extern "C" fn get_best_lap(_this: *mut u8) -> u32 {
    race().global_best_lap
}

unsafe extern "C" fn get_best_split(_this: *mut u8, index: *const i32, global: *const bool, car: *mut u8) -> u32 {
    let race = race();
    let i = *index as usize;
    if *global {
        race.best_splits.get(i).copied().unwrap_or(0)
    } else {
        race.cars.get(car_of(car)).and_then(|c| c.personal_best_splits.get(i).copied()).unwrap_or(0)
    }
}

unsafe extern "C" fn is_best_split(_this: *mut u8, index: *const i32, time: *const i32, global: *const bool, car: *mut u8) -> bool {
    let race = race();
    let (i, time) = (*index as usize, *time);
    if time == 0 {
        return false;
    }
    let best = if *global { race.best_splits.get(i).copied() } else { race.cars.get(car_of(car)).and_then(|c| c.personal_best_splits.get(i).copied()) };
    best == Some(time as u32)
}

unsafe extern "C" fn get_current_lap(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    let splits = race().cars.get(car_of(car)).map(|c| c.current_splits.clone()).unwrap_or_default();
    write_lap(out, &RaceLap { time: 0, splits, is_valid: true });
    out
}

unsafe extern "C" fn get_last_split(_this: *mut u8, car: *mut u8, index: *const i32) -> u32 {
    race().cars.get(car_of(car)).and_then(|c| c.last_lap.splits.get(*index as usize).copied()).unwrap_or(0)
}

unsafe extern "C" fn get_split(_this: *mut u8, car: *mut u8, index: *const i32) -> u32 {
    race().cars.get(car_of(car)).and_then(|c| c.current_splits.get(*index as usize).copied()).unwrap_or(0)
}

unsafe extern "C" fn get_has_completed_flag(_this: *mut u8, car: *mut u8) -> bool {
    race().cars.get(car_of(car)).is_some_and(|c| c.has_completed_flag)
}

unsafe extern "C" fn get_compound_short_name(this: *mut u8, out: *mut u8) -> *mut u8 {
    empty_wstring(out);
    write_wstring(acs(), out, &race().cars.get(car_of(this)).map(|c| c.compound_short_name.clone()).unwrap_or_default());
    out
}

unsafe extern "C" fn get_ballast_kg(this: *mut u8) -> f32 {
    race().cars.get(car_of(this)).map_or(0.0, |c| c.ballast_kg)
}

unsafe extern "C" fn get_restrictor(this: *mut u8) -> f32 {
    race().cars.get(car_of(this)).map_or(0.0, |c| c.restrictor)
}

unsafe extern "C" fn send_vote(_this: *mut u8, kind: i32, value: bool, guid: i32) {
    action(format!("sendVote {kind} {} {guid}", value as u8));
}

unsafe extern "C" fn set_focused_car_index(_this: *mut u8, car: u32) {
    if (car as usize) < race().cars.len() {
        action(format!("setFocusedCarIndex {car}"));
    }
}

/// The stand-ins, once.
pub unsafe fn install(game: &Game) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // the dynamic initializer of the file's `wstringStream` (the rows' text is made in it)
    let init: extern "C" fn() = std::mem::transmute(game.acs.va(0x1_4001_2020));
    init();
    for (va, to) in [
        (VA_GET_SESSION_INFO, get_session_info as *const () as usize),
        (0x1_4013_8220, get_current_session as *const () as usize),
        (VA_GET_CURRENT_SESSION_INDEX, get_current_session_index as *const () as usize),
        (VA_GET_LEADERBOARD_FROM_SESSION, get_leaderboard_from_session as *const () as usize),
        (VA_GET_LEADERBOARD, get_leaderboard as *const () as usize),
        (VA_GET_BEST_LAP_OF_CAR, get_best_lap_of_car as *const () as usize),
        (VA_GET_BEST_LAP, get_best_lap as *const () as usize),
        (VA_GET_BEST_SPLIT, get_best_split as *const () as usize),
        (VA_IS_BEST_SPLIT, is_best_split as *const () as usize),
        (VA_GET_CURRENT_LAP, get_current_lap as *const () as usize),
        (VA_GET_LAST_SPLIT, get_last_split as *const () as usize),
        (VA_GET_SPLIT, get_split as *const () as usize),
        (VA_GET_HAS_COMPLETED_FLAG, get_has_completed_flag as *const () as usize),
        (VA_CAR_GET_COMPOUND_SHORT_NAME, get_compound_short_name as *const () as usize),
        (VA_CAR_GET_BALLAST_KG, get_ballast_kg as *const () as usize),
        (VA_CAR_GET_RESTRICTOR, get_restrictor as *const () as usize),
        (VA_CLIENT_SEND_VOTE, send_vote as *const () as usize),
        (VA_SIM_SET_FOCUSED_CAR_INDEX, set_focused_car_index as *const () as usize),
    ] {
        jump(game, va, to);
    }
}

/// The script's values of every car, where the game's code reads them itself: the skin's
/// folder, the short name, the black flag, "in the pit lane", the delta to the best lap.
pub unsafe fn sync(h: &HudGame) {
    let race = race();
    for (avatar, car) in h.cars.iter().zip(&race.cars) {
        let avatar = *avatar;
        for (at, text) in [(0x1138, &car.skin), (0x198, &car.gui_short_name)] {
            if rd::<usize>(avatar, at + 0x18) == 0 {
                empty_wstring(avatar.add(at));
            }
            if &read_wstring(avatar.add(at)) != text {
                write_wstring(acs(), avatar.add(at), text);
            }
        }
        wr(avatar, 0x1000, car.is_black_flagged as u8);
        wr(avatar, 0x129c, car.in_pitlane as u8);
        wr(avatar, 0x9a4, car.performance_meter);
    }
}
