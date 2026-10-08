// SPDX-License-Identifier: GPL-3.0-or-later

//! The golden test of the sound: the port replays a short recorded drive (the F2004's launch
//! at Spa, heard from the cockpit) and must make exactly the FMOD calls the game's own sound
//! code made for it: the same number of log lines with the same hash.
//!
//! It needs Assetto Corsa (its FMOD DLLs, its sound banks, the car and the track): without it
//! the test prints NOT TESTED and passes. Nothing is played: the mix goes to a WAV file in
//! Cargo's own temporary folder through FMOD's non-real-time writer.
//!
//! The golden file is written by `tools/audio_oracle golden` from the game's own run.

use std::path::Path;

use rustyac_audio::fmod::{log, raw};
use rustyac_audio::golden::{Golden, AUDIO_INI};

#[test]
fn the_port_makes_the_fmod_calls_of_the_game() {
    let golden_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/f2004_spa_launch_cockpit_240.augold");
    let golden = Golden::read(&golden_file).expect("the golden file");
    let Some(ac) = rustyac_content::install::ac_root() else {
        println!("NOT TESTED: {}", rustyac_content::install::not_found_hint());
        return;
    };
    let needed = [
        ac.join("fmod64.dll"),
        ac.join("fmodstudio64.dll"),
        ac.join("content/sfx/common.bank"),
        ac.join("content/cars").join(&golden.car).join("sfx").join(format!("{}.bank", golden.car)),
        ac.join("content/tracks").join(&golden.track),
    ];
    if let Some(missing) = needed.iter().find(|p| !p.exists()) {
        println!("NOT TESTED: {} is not there", missing.display());
        return;
    }
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join("audio_golden");
    std::fs::create_dir_all(work.join("cfg")).unwrap();
    let audio_ini = work.join("cfg/audio.ini");
    std::fs::write(&audio_ini, AUDIO_INI).unwrap();
    // the same output as the run the golden file is from: the devices FMOD lists (and so the
    // calls the engine makes while it looks for the player's device) depend on it
    if let Err(message) = raw::load(&ac, raw::Output::WavNrt { file: work.join("golden.wav"), rate: 48000, block: 800 }) {
        println!("NOT TESTED: {message}");
        return;
    }
    log::start_with_answers(None, &golden.answers).unwrap();
    golden.run(&ac, &audio_ini).expect("the session runs");
    let summary = log::finish().expect("a log");
    assert_eq!(
        (summary.lines, format!("{:016x}", summary.hash)),
        (golden.lines, format!("{:016x}", golden.hash)),
        "the port's FMOD call log is not the game's ({} frames of the {} at {})",
        golden.frames.len(),
        golden.car,
        golden.track
    );
    println!("the port made the game's {} FMOD calls and marks over {} frames", summary.lines, golden.frames.len());
}
