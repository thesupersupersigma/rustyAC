// SPDX-License-Identifier: GPL-3.0-or-later

//! On a real track (Task 12): the game's own commands around the lap timer, driven by the
//! line follower.
//!
//! Needs `cardata/ks_ferrari_f2004` (extracted game data, not in git) and Assetto Corsa's own
//! `content/tracks/spa` (`AC_ROOT`, else Steam's usual place); without them the test prints a
//! notice and passes without testing anything.

use std::path::PathBuf;

use rustyac_game::autodrive::AutoDriver;
use rustyac_game::input_file::{event, SimSetup};
use rustyac_game::sim::{find_track, GameSim, SpawnSequence};
use rustyac_game::view::CarView;

fn ready() -> bool {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
    if !data.join("suspensions.ini").is_file() {
        eprintln!("NOT TESTED: {} is missing (the F2004's extracted data files)", data.display());
        return false;
    }
    if let Err(message) = find_track("spa") {
        eprintln!("NOT TESTED: {message}");
        return false;
    }
    true
}

fn steps(sim: &mut GameSim, count: u32) -> CarView {
    for _ in 0..count {
        sim.step().unwrap();
    }
    CarView::capture(sim, 0.0)
}

#[test]
fn back_on_track_marks_the_lap_and_a_reset_starts_the_timer_again() {
    if !ready() {
        return;
    }
    let setup = SimSetup { track: "spa".to_string(), auto_shifter: true, ..SimSetup::default() };
    let mut sim = GameSim::new(setup, Box::new(SpawnSequence::new(AutoDriver::new(), true))).unwrap();

    // from the hot-lap start over the line, round the first corner and down the hill: 27 s
    let before = steps(&mut sim, 9000);
    assert!(before.lap.on_track, "the car is on a track with timing lines");
    assert!(before.lap.valid && before.lap.cuts == 0, "the line follower stays on the road: {:?}", before.lap);
    assert!(before.lap.current_ms > 3000 && before.lap.current_ms < 20_000, "the clock went back to zero at the line: {} ms", before.lap.current_ms);
    assert!(before.lap.position > 0.01 && before.lap.position < 0.2, "past the line: {}", before.lap.position);
    assert!(before.speed_kmh > 100.0, "driving: {} km/h", before.speed_kmh);

    // back on track: on the line where the car was, standing, and the lap is marked
    sim.car.device.source.request(event::TO_TRACK);
    let after = steps(&mut sim, 3);
    assert!(!after.lap.valid && after.lap.cuts == 1, "the lap in progress is marked as cut: {:?}", after.lap);
    assert!(after.speed_kmh < 5.0, "the car was put down: {} km/h", after.speed_kmh);
    assert!((after.lap.position - before.lap.position).abs() < 0.005, "where it was along the lap: {} then {}", before.lap.position, after.lap.position);
    assert!(sim.car.car.spline_locator.offset.abs() < 0.5, "on the line: {} m beside it", sim.car.car.spline_locator.offset);
    assert_eq!(after.lap.laps, before.lap.laps);

    // reset: at the hot-lap start again, the timer as at the session's start
    sim.car.device.source.request(event::RESET);
    let reset = steps(&mut sim, 3);
    assert!(reset.lap.valid && reset.lap.cuts == 0, "a fresh timer: {:?}", reset.lap);
    assert!(reset.lap.current_ms < 100, "the clock starts again: {} ms", reset.lap.current_ms);
    assert!(reset.lap.position > 0.9, "before the line again: {}", reset.lap.position);

    // and from there a lap begins at the line as it did the first time
    let again = steps(&mut sim, 9000);
    assert!(again.lap.valid && again.lap.position > 0.01 && again.lap.position < 0.2, "{:?}", again.lap);
    assert!(again.lap.current_ms > 3000 && again.lap.current_ms < 20_000, "{} ms", again.lap.current_ms);
}

/// Task 18, on a second track: the track's grip of a session (`[DYNAMIC_TRACK]` of race.ini).
/// A lap that the timer counts adds its gain, and a new session (a new car) starts from the
/// session's start grip plus `SESSION_TRANSFER` of what was gained.
#[test]
fn on_magione_a_lap_adds_grip_and_a_new_session_carries_part_of_it_over() {
    if !ready() {
        return;
    }
    if let Err(message) = find_track("magione") {
        eprintln!("NOT TESTED: {message}");
        return;
    }
    // SESSION_START 90 %, no random part, LAP_GAIN 1 (a lap adds 1 %), SESSION_TRANSFER 50 %
    let mut setup = SimSetup { track: "magione".to_string(), auto_shifter: true, session_transfer: true, ..SimSetup::default() };
    let mut grip = rustyac_physics::track::DynamicTrack::from_race_ini(90.0, 0.0, 1.0, 50.0, 0);
    grip.on_new_session(0, 0);
    grip.step(0);
    setup.env.dynamic_grip_level = grip.dynamic_grip_level;
    setup.session.dynamic_track = Some(grip);
    let mut sim = GameSim::new(setup, Box::new(SpawnSequence::new(AutoDriver::new(), true))).unwrap();
    let start = 90.0f32 * 0.01;
    let first = steps(&mut sim, 300);
    assert_eq!(sim.car.car.env.dynamic_grip_level, start, "the session starts at 90 %");
    assert!(first.lap.on_track);
    // over the line and once round: at most 150 s
    let mut view = first;
    for _ in 0..150 {
        view = steps(&mut sim, 333);
        if view.lap.laps >= 1 {
            break;
        }
    }
    assert_eq!(view.lap.laps, 1, "the line follower finishes a lap of Magione: {:?}", view.lap);
    steps(&mut sim, 3);
    let after_lap = 1.0f32 * (0.01f32 / 1.0) + start;
    assert_eq!(sim.car.car.env.dynamic_grip_level, after_lap, "one counted lap adds 1 %");

    // a new car is the next session: half of the gain stays
    sim.car.device.source.request(event::REBUILD);
    steps(&mut sim, 3);
    assert_eq!(sim.session_index, 1);
    let carried = (after_lap - start) * (50.0f32 * 0.01) + start;
    assert_eq!(sim.car.car.env.dynamic_grip_level, -1.0f32 * 0.0 + carried, "half of the gain is carried over");
    assert_eq!(CarView::capture(&sim, 0.0).lap.laps, 0, "a new session, a new lap list");
}
