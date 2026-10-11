// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 24: how another car is named on the command line and in a recording's header.

use std::path::PathBuf;

use rustyac_game::input_file::{Opponent, OpponentDrive, SimSetup};

fn opponent(car: &str, skin: &str, drive: OpponentDrive) -> Opponent {
    Opponent { car: car.to_string(), skin: skin.to_string(), drive }
}

#[test]
fn an_opponent_as_text() {
    let file = |path: &str| OpponentDrive::Recorded(PathBuf::from(path));
    for (text, expected) in [
        ("bmw_m3_e30:auto", opponent("bmw_m3_e30", "", OpponentDrive::Auto)),
        ("bmw_m3_e30:PARKED", opponent("bmw_m3_e30", "", OpponentDrive::Parked)),
        ("bmw_m3_e30:red:auto", opponent("bmw_m3_e30", "red", OpponentDrive::Auto)),
        // a skin may be called like a way of driving
        ("bmw_m3_e30:auto:parked", opponent("bmw_m3_e30", "auto", OpponentDrive::Parked)),
        ("f2004:lap.ryin", opponent("f2004", "", file("lap.ryin"))),
        ("f2004:laps\\one.ryin", opponent("f2004", "", file("laps\\one.ryin"))),
        // the colon of a drive letter is not a separator: in the file, in the car, in both
        ("f2004:C:\\laps\\one.ryin", opponent("f2004", "", file("C:\\laps\\one.ryin"))),
        ("f2004:red:C:/laps/one.RYIN", opponent("f2004", "red", file("C:/laps/one.RYIN"))),
        ("D:\\cars\\mine:auto", opponent("D:\\cars\\mine", "", OpponentDrive::Auto)),
        ("D:\\cars\\mine:red:C:\\laps\\one.ryin", opponent("D:\\cars\\mine", "red", file("C:\\laps\\one.ryin"))),
    ] {
        let parsed = Opponent::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(parsed, expected, "{text}");
        // and back
        assert_eq!(Opponent::parse(&parsed.to_text()).unwrap(), expected, "{text} written and read again");
    }
    for wrong in ["bmw_m3_e30", ":auto", "bmw_m3_e30:fast", "bmw_m3_e30:lap.txt", ""] {
        assert!(Opponent::parse(wrong).is_err(), "{wrong:?} was taken");
    }
}

#[test]
fn opponents_in_a_header() {
    let mut setup = SimSetup { track: "spa".to_string(), ..SimSetup::default() };
    let before = setup.header();
    assert!(!before.contains("opponent"), "a one-car header changed");
    setup.opponents.push(opponent("bmw_m3_e30", "", OpponentDrive::Auto));
    setup.opponents.push(opponent("ks_ferrari_250_gto", "red", OpponentDrive::Parked));
    let text = setup.header();
    let (read, _) = SimSetup::parse_header(text.as_bytes()).expect("the header");
    assert_eq!(read.opponents, setup.opponents);
    // the worker threads are not a part of a recording
    assert_eq!(read.physics_threads, None);
}
