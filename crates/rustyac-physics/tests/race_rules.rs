// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28: the referee against what the oracle approved.
//!
//! `tools/car_oracle race-micro --seed <s> --kind <list> --count <cars> --steps <frames>` runs
//! a made-up drive (`rustyac_physics::race::toy`) through the game's own `RaceManager`,
//! `RaceTimingServices`, `SessionResultProcessor`, `FlagManager` and `JsonOutputFile` and
//! through the port, compares every field after every frame and the bytes of `race_out.json`,
//! and prints a `golden:` line: one hash over the port's state after every frame and the hash
//! of the file. The lines below are from runs that ended `referee: IDENTICAL`; this test runs
//! the same drives through the port alone. No file of the game is needed.

use rustyac_physics::race::toy;

/// (seed, session list, cars, frames, the hash of every frame's state, the hash of race_out.json)
const GOLDEN: [(u32, usize, usize, usize, u64, u64); 8] = [
    (3, 0, 3, 30000, 0x44cedb921363ef53, 0xba13a184bc0c0496),
    (10, 1, 4, 30000, 0x372f1e3cbc2e78eb, 0x1390b6dc9aafa737),
    (17, 2, 5, 30000, 0x31f64034173ab954, 0xad598b6dec031a09),
    (24, 3, 6, 30000, 0xb1bab787449e2f17, 0x65ffd2716a444688),
    (31, 4, 7, 30000, 0xf157c20e2a3dd73c, 0x05ee5ac38f1f7f26),
    (38, 5, 8, 30000, 0xd3bc26d8eee6a1bb, 0x58d86cf1ad6afd96),
    (45, 6, 9, 30000, 0x67c29bfb31a66aa2, 0xd9ebc5ed5a09a4ed),
    (52, 7, 10, 30000, 0x85a127c2f7007b0c, 0xc08b21d35c384270),
];

#[test]
fn the_referee_is_what_the_oracle_approved() {
    for (seed, list, cars, frames, state, file) in GOLDEN {
        let (hash, json, drive) = toy::run(seed, list, cars, frames);
        let json_hash = json.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, byte| (h ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3));
        assert_eq!(hash, state, "seed {seed}, session list {list}: the referee's state over {frames} frames ({} laps, {} sessions ended)", drive.laps, drive.sessions_ended);
        assert_eq!(json_hash, file, "seed {seed}, session list {list}: race_out.json");
        assert!(json.starts_with(b"\r\n{\r\n\t\"track\": \"toy_track"), "race_out.json starts as the game's does");
    }
}

/// A weekend through the port: practice, qualifying, race, every session ending by itself.
#[test]
fn a_weekend_runs_through_its_sessions() {
    let (_, json, drive) = toy::run(3, 0, 3, 30000);
    let text = String::from_utf8(json).unwrap();
    assert_eq!(drive.manager.current_session_index, 2, "the race is the last session");
    for name in ["\"name\": \"Practice\"", "\"name\": \"Qualify\"", "\"name\": \"Race\"", "\"raceResult\": ["] {
        assert!(text.contains(name), "race_out.json has {name}");
    }
    assert!(drive.flag_frames[3] > 0, "the chequered flag was shown");
}

/// The comparison can fail: a frame a microsecond longer gives another state.
#[test]
fn another_frame_length_is_another_result() {
    let (mut drive, _) = toy::PortDrive::new(24, 3, 6);
    drive.fault = true;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for _ in 0..30000 {
        let frame = drive.begin_frame();
        drive.finish_frame(frame);
        hash = (hash ^ rustyac_physics::race::referee::dump_hash(&drive.dump())).wrapping_mul(0x0000_0100_0000_01b3);
    }
    drive.shutdown();
    hash = (hash ^ rustyac_physics::race::referee::dump_hash(&drive.dump())).wrapping_mul(0x0000_0100_0000_01b3);
    assert_ne!(hash, GOLDEN[3].4);
}
