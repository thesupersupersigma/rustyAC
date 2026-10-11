// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 25: an online session against the game's own code.
//!
//! `golden_laguna_join`: an excerpt of `tools/car_oracle net --scenario net_block` on the tape
//! of a live session (Kunos' `acServer`, Laguna Seca, four clients): the local F2004 stands on
//! the racing line, the three other cars of the tape arrive (two F2004s and a Z4 GT3, which
//! runs into it). The excerpt holds the local car's inputs, the tape's messages and, per
//! step, the hash of what the game's own `ACClient` and `NetCarStateProvider` made of them:
//! the local car, every remote car's body and state, every outgoing message. The port
//! replays it and must give the same hash at every step. Needs Assetto Corsa's folder (the
//! track, the cars' collider meshes): NOT TESTED without it.

use std::path::PathBuf;

use rustyac_net::replay::NetGolden;
use rustyac_net::transport::Lane;
use rustyac_physics::engine::record::ReplayPaths;

fn cardata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata")
}

fn game_root() -> Option<PathBuf> {
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    root.join("content").join("tracks").join("ks_laguna_seca").is_dir().then_some(root)
}

#[test]
fn golden_laguna_join() {
    let golden = NetGolden::from_bytes(include_bytes!("golden/laguna_join_3000.netgold")).expect("the golden file");
    for car in ["ks_ferrari_f2004", "bmw_z4_gt3"] {
        if !cardata().join(car).join("suspensions.ini").is_file() {
            eprintln!("NOT TESTED: cardata/{car} is missing (the car's extracted data files)");
            return;
        }
    }
    let Some(root) = game_root() else {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the online golden replay needs the track ks_laguna_seca and the cars' collider.kn5");
        return;
    };
    let paths = ReplayPaths { cardata: cardata(), game_root: Some(root) };
    assert!(golden.hashes.len() >= 3000, "the excerpt is too short to reach the hit");
    match golden.replay(&paths) {
        Ok(result) => assert_eq!(result, None, "the port leaves the game's values at this step"),
        Err(message) if message.contains("track") => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
        Err(message) => panic!("the replay: {message}"),
    }
    // the comparison can fail: one bit of one car state that arrived
    let mut spoiled = golden.clone();
    let entry = spoiled.tape.entries.iter_mut().find(|entry| entry.lane == Lane::DatagramIn && entry.tick >= 1500 && entry.data.first() == Some(&0x46)).expect("a car state after step 1500");
    entry.data[12] ^= 1;
    let result = spoiled.replay(&paths).expect("the replay");
    assert!(result.is_some_and(|step| step >= 1500), "a changed bit of a car state went unnoticed");
}
