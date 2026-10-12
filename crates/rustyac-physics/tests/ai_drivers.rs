// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 27: AC's AI drivers.
//!
//! `golden_ai_race`: an excerpt of a recording of the game's own `AIDriver` in two F2004s on
//! Spa's grid (`tools/car_oracle multi --scenario ai_race`): the wait for the green light,
//! the start and the first seconds of the race. The port's drivers are made from the
//! recording's seed and drive the port's cars from step 0; every step's row (the drivers'
//! controls, every body, the tyres, the drivers' own members) is held against the game's,
//! with no worker thread and with three. Needs Assetto Corsa's folder (the track with its
//! lines, the car's collider mesh): NOT TESTED without it.

use std::path::PathBuf;
use std::sync::Arc;

use rustyac_physics::engine::record::{Golden, ReplayPaths};

fn cardata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata")
}

fn game_root() -> Option<PathBuf> {
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    root.join("content").join("tracks").join("spa").is_dir().then_some(root)
}

#[test]
fn golden_ai_race() {
    let golden = Golden::from_bytes(include_bytes!("golden/ai_spa_race_2_3000.mcgold")).expect("the golden file");
    if !cardata().join("ks_ferrari_f2004").join("ai.ini").is_file() {
        eprintln!("NOT TESTED: cardata/ks_ferrari_f2004 is missing (the car's extracted data files)");
        return;
    }
    let Some(root) = game_root() else {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the AI golden replay needs the track spa with its AI lines and the car's collider.kn5");
        return;
    };
    let paths = ReplayPaths { cardata: cardata(), game_root: Some(root) };
    let recording = golden.recording().expect("the header");
    assert!(recording.cars.iter().all(|car| car.ai.is_some()), "every car of the excerpt has an AI driver");
    let track = match rustyac_physics::engine::record::load_track(&recording, &paths) {
        Ok(track) => track,
        Err(message) => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
    };
    assert!(golden.hashes.len() >= 2500, "the excerpt is too short to reach the start");
    for threads in [0, 3] {
        let result = golden.replay(&paths, Arc::clone(&track), threads).expect("the replay");
        assert_eq!(result, None, "with {threads} worker threads the port leaves the game's values at this step");
    }
    // the comparison can fail: another start of the drivers' random numbers
    let mut spoiled = golden.clone();
    for (key, value) in &mut spoiled.header {
        if key == "ai_seed" {
            *value = (value.parse::<u32>().expect("the seed") + 1).to_string();
        }
    }
    let result = spoiled.replay(&paths, Arc::clone(&track), 0).expect("the replay");
    assert!(result.is_some(), "another seed of the drivers' random numbers went unnoticed");
}
