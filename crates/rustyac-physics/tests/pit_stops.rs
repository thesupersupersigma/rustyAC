// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28b: the player's pit stop and the AI fix, against approved recordings of the game.
//!
//! The excerpts were written by `tools/car_oracle` (`multi --scenario pit_feed` /
//! `ai_pass_front`, then `multi-check <recording> --steps <n> --csv-out <file.mcgold>`): the
//! inputs of every step and a hash of the game's own values after it, among them the hash
//! over the game's `RaceManager`, `PitStop`, pit menu and what their jobs left in the cars.
//! The port's replay has to give the same rows, with and without worker threads.

use std::path::PathBuf;
use std::sync::Arc;

use rustyac_physics::engine::record::{Golden, ReplayPaths};
use rustyac_physics::race::pitstop::{PitCarView, PitFeed, PitFrame, PitStop, QuickMenu, QuickMenuCar, QuickMenuSettings, ITEM_FUEL, ITEM_REPAIR, ITEM_TYRE_COMPOUND, ITEM_TYRE_PRESSURE, ITEM_WING, STATE_FINISHED, STATE_IDLE, STATE_UNDEFINED, STATE_WAITING_FOR_DRIVER};

fn replay(bytes: &[u8], car: &str, key: &str) {
    let golden = Golden::from_bytes(bytes).expect("the golden file");
    let cardata = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata");
    if !cardata.join(car).join("suspensions.ini").is_file() {
        eprintln!("NOT TESTED: cardata/{car} is missing (the car's extracted data files)");
        return;
    }
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    if !root.join("content").join("tracks").join("magione").is_dir() {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the replay needs the track magione and the car's collider.kn5");
        return;
    }
    let paths = ReplayPaths { cardata, game_root: Some(root) };
    let recording = golden.recording().expect("the header");
    assert!(recording.get(key).is_some(), "the excerpt has the key {key}");
    let track = match rustyac_physics::engine::record::load_track(&recording, &paths) {
        Ok(track) => track,
        Err(message) => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
    };
    for threads in [0, 3] {
        let result = golden.replay(&paths, Arc::clone(&track), threads).expect("the replay");
        assert_eq!(result, None, "with {threads} worker threads the port leaves the game's values at this step");
    }
}

/// Pit stops on made-up states (`pit_feed`, seed 1): the game's `PitStop` is fed places,
/// speeds, fuel levels, menus and damage from a seed; the stops it then runs (the countdown
/// with the game's `rand()`, the choices applied to the game's car) are real.
#[test]
fn golden_pit_stops_on_fed_states() {
    replay(include_bytes!("golden/pit_feed_magione_1_3600.mcgold"), "bmw_m3_e30", "pit_feed");
}

/// Task 28's open question: an AI driver behind a car that sleeps on the grid reads its
/// speed as exactly 0 (the sleeping rule stops the body in the car's own step, before the
/// drivers of the later cars look). The port left the game's values at step 2127 of this drive.
#[test]
fn golden_ai_passes_a_car_asleep_on_the_grid() {
    replay(include_bytes!("golden/ai_pass_front_magione_2400.mcgold"), "bmw_m3_e30", "ai_seed");
}

fn menu() -> (QuickMenu, QuickMenuCar) {
    let car = QuickMenuCar { max_fuel: 70.0, compounds: 3, static_pressure: [26, 26, 24, 24], wing_angle: vec![0.0, 4.0] };
    let ini = rustyac_physics::data::ini::IniReader::from_text(
        std::path::Path::new("setup.ini"),
        "[PRESSURE_LF]\nNAME=Pressure LF\nMIN=15\nMAX=40\n[PRESSURE_RF]\nNAME=Pressure RF\nMIN=15\nMAX=40\n[PRESSURE_LR]\nNAME=Pressure LR\nMIN=15\nMAX=40\n[PRESSURE_RR]\nNAME=Pressure RR\nMIN=15\nMAX=40\n[WING_1]\nNAME=Rear\nMIN=0\nMAX=10\nPITSTOP=2.5\n[WING_2]\nNAME=No stop\nMIN=0\nMAX=10\n[WING_4]\nNAME=After a gap\nMIN=0\nMAX=10\nPITSTOP=1\n",
    );
    let settings = QuickMenuSettings { stay_in_car: false, auto_app_on_pitlane: true, presets_count: 3, visibility_max_time: 5.0 };
    (QuickMenu::build(&car, vec!["Soft".into(), "Medium".into(), "Hard".into()], &ini, &settings), car)
}

/// `QuickMenuPitstop::buildControls`: the items a car gets, in the menu's order, and the
/// values every preset starts with.
#[test]
fn the_menu_of_a_car_and_what_it_starts_with() {
    let (menu, _) = menu();
    let kinds: Vec<(i32, i32)> = menu.items.iter().map(|item| (item.kind, item.component_index)).collect();
    // fuel, compound, four pressures, the one wing a stop may change (WING_2 has no PITSTOP,
    // the list ends at the gap before WING_4), body, suspension, engine
    assert_eq!(kinds, [(ITEM_FUEL, -1), (ITEM_TYRE_COMPOUND, -1), (ITEM_TYRE_PRESSURE, 0), (ITEM_TYRE_PRESSURE, 1), (ITEM_TYRE_PRESSURE, 2), (ITEM_TYRE_PRESSURE, 3), (ITEM_WING, 1), (ITEM_REPAIR, 0), (ITEM_REPAIR, 1), (ITEM_REPAIR, 2)]);
    assert_eq!(menu.presets.len(), 3);
    for preset in &menu.presets {
        // no fuel, no tyre change, the compound's own pressures, no wing clicks, no repairs
        assert_eq!(preset, &[0.0, -1.0, 26.0, 26.0, 24.0, 24.0, 0.0, 0.0, 0.0, 0.0]);
    }
    assert_eq!((menu.get_max_fuel(), menu.get_wing_pitstop_time(1), menu.get_wing_pitstop_time(2)), (70.0, 2.5, 0.0));
}

/// `QuickMenuPitstop::sendInput` and the items' own functions.
#[test]
fn the_menu_keys() {
    let (left, up, right, down) = (0x25u32, 0x26u32, 0x27u32, 0x28u32);
    let (mut menu, car) = menu();
    // the first key only shows the menu, on its top line (the presets)
    menu.send_input(right, &car);
    assert!(menu.is_visible && menu.current_index == -1 && menu.current_preset == 0);
    menu.send_input(right, &car);
    menu.send_input(right, &car);
    menu.send_input(right, &car);
    assert_eq!(menu.current_preset, 2, "the last preset is the end");
    menu.send_input(left, &car);
    menu.send_input(up, &car);
    assert_eq!((menu.current_preset, menu.current_index), (1, -1));
    // fuel: whole litres from 0 to the tank
    menu.send_input(down, &car);
    menu.send_input(left, &car);
    for _ in 0..100 {
        menu.send_input(right, &car);
    }
    assert_eq!(menu.presets[1][0], 70.0);
    assert_eq!(menu.presets[0][0], 0.0, "the other presets keep theirs");
    // the compound: from "no change" up to the last one
    menu.send_input(down, &car);
    for _ in 0..5 {
        menu.send_input(right, &car);
    }
    assert_eq!(menu.presets[1][1], 2.0);
    // a pressure between the setup's limits
    menu.send_input(down, &car);
    for _ in 0..30 {
        menu.send_input(right, &car);
    }
    assert_eq!(menu.presets[1][2], 40.0);
    // the wing: clicks on top of the angle it has (4 of 0..10 here)
    for _ in 0..4 {
        menu.send_input(down, &car);
    }
    assert_eq!(menu.items[menu.current_index as usize].kind, ITEM_WING);
    for _ in 0..30 {
        menu.send_input(right, &car);
    }
    assert_eq!(menu.presets[1][6], 6.0);
    for _ in 0..30 {
        menu.send_input(left, &car);
    }
    assert_eq!(menu.presets[1][6], -4.0);
    // a repair is a switch; the last line is the end
    for _ in 0..9 {
        menu.send_input(down, &car);
    }
    assert_eq!(menu.current_index, 9);
    menu.send_input(right, &car);
    assert_eq!(menu.presets[1][9], 1.0);
    menu.send_input(left, &car);
    assert_eq!(menu.presets[1][9], 0.0);
    // the menu's values travel in a recording's header as text
    let text = menu.values_text();
    let (mut other, _) = self::menu();
    assert!(other.set_values_text(&text));
    assert_eq!((other.presets, other.current_preset), (menu.presets.clone(), menu.current_preset));
    // and in a setup's `.sp` file (the repairs are not kept there)
    let saved = menu.save_presets();
    let (mut loaded, _) = self::menu();
    loaded.load_presets(&rustyac_physics::data::ini::IniReader::from_text(std::path::Path::new("x.sp"), &saved));
    // (`loadPresets` keeps every value between its item's limits, a wing's clicks too: the
    // -4 clicks are below the wing's smallest angle, 0)
    let mut expected = menu.presets.clone();
    expected[1][6] = 0.0;
    assert_eq!(loaded.presets, expected);
}

/// The stop's state machine without a car's physics: what does not need `Car::getPitstopTime`.
#[test]
fn the_stop_waits_for_the_car_in_its_box() {
    let mut pit = PitStop::new([10.0, 0.0, 20.0]);
    assert_eq!(pit.state, STATE_FINISHED, "as the constructor leaves it");
    let mut out = Vec::new();
    pit.on_new_session(&mut out);
    assert_eq!(pit.state, STATE_UNDEFINED);
    let view = |x: f32, speed: f32, in_pitlane: bool| PitCarView { is_black_flagged: false, is_in_pitlane: in_pitlane, position: [x, 0.0, 20.0], speed, fuel: 10.0 };
    // (the states that are reached without asking the menu)
    let inside = view(11.0, 0.0, true);
    assert!(pit.is_inside_pit_area(&inside) && !pit.is_inside_pit_area(&view(13.0, 0.0, true)));
    assert!(!pit.is_inside_pit_area(&view(10.0 + 3.0, 0.0, true)), "3 m is outside");
    // in the box, slower than 1.5 km/h; then until 2 km/h
    assert!(pit.is_in_pit_position(&view(11.0, 1.49 / 3.6, true)));
    assert!(pit.is_in_pit_position(&view(11.0, 1.9 / 3.6, true)), "once there, it holds to 2 km/h");
    assert!(!pit.is_in_pit_position(&view(11.0, 2.1 / 3.6, true)));
    assert!(!pit.is_in_pit_position(&view(11.0, 1.9 / 3.6, true)), "and has to be slower than 1.5 km/h again");
    // the fed states are the same numbers for the same seed
    let (menu, _) = menu();
    let (mut a, mut b) = (PitFeed::new(7), PitFeed::new(7));
    for _ in 0..2000 {
        assert_eq!(a.next([10.0, 0.0, 20.0], &menu, 70.0), b.next([10.0, 0.0, 20.0], &menu, 70.0));
    }
    let _ = (STATE_IDLE, STATE_WAITING_FOR_DRIVER, PitFrame { dt: 0.009, session_type: 1, race_over: false, in_replay: false, pause_menu: false, esc_menu: false });
}
