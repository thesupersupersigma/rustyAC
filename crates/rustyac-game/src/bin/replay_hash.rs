// SPDX-License-Identifier: GPL-3.0-or-later

//! `replay_hash <file.ryin> [--out <file>] [--remap <from>=<to>]`: replays a recorded drive and hashes the car's
//! whole state after every step. Builds for the desktop and for `wasm32-wasip1`, so a wasm
//! build of the physics can be held against the desktop's step by step
//! (`docs/port/web.md`).
//!
//! `--out` gets 16 bytes per step: the FNV-1a 64 hash of the state's exact bits (what
//! `--dump-states` of the game writes: bodies, joints, tyres, every traced value, the force
//! tape), then the same hash with every NaN replaced by one NaN. Two builds that differ only
//! in the sign or payload of a NaN differ in the first number and agree in the second.
//!
//! `--remap` replaces the start of the paths a replay stores (its car, its track, its saved
//! setup): a file recorded with `C:/Program Files (x86)/.../assettocorsa/content/tracks/spa`
//! finds the same folder where a WASI run has it mounted (`/ac/content/tracks/spa`).

use rustyac_game::dump::StepDump;
use rustyac_game::input_file::InputFile;
use rustyac_game::sim::{GameSim, ReplaySource};

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = (*hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn canonical32(word: u32) -> u32 {
    if f32::from_bits(word).is_nan() {
        f32::NAN.to_bits()
    } else {
        word
    }
}

fn canonical64(word: u64) -> u64 {
    if f64::from_bits(word).is_nan() {
        f64::NAN.to_bits()
    } else {
        word
    }
}

/// (exact, NaN-blind) hashes of one step.
fn hashes(dump: &StepDump) -> (u64, u64) {
    let mut exact = 0xcbf2_9ce4_8422_2325u64;
    let mut blind = exact;
    for &word in &dump.state {
        fnv(&mut exact, &word.to_le_bytes());
        // the saved state does not say which words are floats; no counter in it reaches a NaN's
        // bit pattern
        fnv(&mut blind, &canonical32(word).to_le_bytes());
    }
    for value in &dump.trace {
        fnv(&mut exact, &value.word.to_le_bytes());
        let word = match value.kind {
            'd' => canonical64(value.word),
            'i' => value.word,
            _ => canonical32(value.word as u32) as u64,
        };
        fnv(&mut blind, &word.to_le_bytes());
    }
    for &word in &dump.snapshot {
        fnv(&mut exact, &word.to_le_bytes());
        fnv(&mut blind, &word.to_le_bytes());
    }
    for call in &dump.tape {
        for hash in [&mut exact, &mut blind] {
            fnv(hash, &call.body.to_le_bytes());
            fnv(hash, &call.kind.to_le_bytes());
        }
        for v in [call.a, call.b, call.facc, call.tacc] {
            for x in v {
                fnv(&mut exact, &x.to_bits().to_le_bytes());
                fnv(&mut blind, &canonical32(x.to_bits()).to_le_bytes());
            }
        }
    }
    (exact, blind)
}

/// `--remap`: the path with its start replaced, compared without case and with either slash.
fn remapped(path: &str, remap: &[(String, String)]) -> String {
    let plain = |text: &str| text.replace('\\', "/").to_ascii_lowercase();
    for (from, to) in remap {
        if plain(path).starts_with(&plain(from)) {
            return format!("{to}{}", path[from.len()..].replace('\\', "/"));
        }
    }
    path.to_string()
}

fn run(replay: &str, out: Option<&String>, remap: &[(String, String)]) -> Result<(), String> {
    let mut file = InputFile::read(std::path::Path::new(replay))?;
    file.setup.car = remapped(&file.setup.car, remap);
    file.setup.track = remapped(&file.setup.track, remap);
    if let Some(track) = file.setup.oracle.as_mut().and_then(|oracle| oracle.track.as_mut()) {
        track.folder = remapped(&track.folder, remap);
    }
    if let Some(setup) = file.setup.oracle.as_mut().and_then(|oracle| oracle.conditions.setup_file.as_mut()) {
        *setup = remapped(&setup.to_string_lossy(), remap).into();
    }
    if let Some(setup) = &mut file.setup.session.setup_file {
        *setup = remapped(&setup.to_string_lossy(), remap).into();
    }
    let mut sim = GameSim::new(file.setup.clone(), Box::new(ReplaySource::default()))?;
    let mut bytes = Vec::with_capacity(file.steps.len() * 16);
    let (mut all_exact, mut all_blind) = (0xcbf2_9ce4_8422_2325u64, 0xcbf2_9ce4_8422_2325u64);
    for step in &file.steps {
        sim.step_recorded(step)?;
        let (exact, blind) = hashes(&StepDump::capture(&sim.car.car));
        fnv(&mut all_exact, &exact.to_le_bytes());
        fnv(&mut all_blind, &blind.to_le_bytes());
        bytes.extend_from_slice(&exact.to_le_bytes());
        bytes.extend_from_slice(&blind.to_le_bytes());
    }
    if let Some(out) = out {
        std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    }
    println!(
        "{} steps, car {:?}, track {:?} {:?}, maths {:?}, hash {all_exact:016x}, NaN-blind {all_blind:016x}",
        sim.steps,
        file.setup.car,
        file.setup.track,
        file.setup.layout,
        rustyac_math::backend()
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(replay) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: replay_hash <file.ryin> [--out <hashes file>] [--remap <from>=<to>]");
        std::process::exit(2);
    };
    let out = args.iter().position(|a| a == "--out").and_then(|at| args.get(at + 1));
    // (may be given more than once)
    let remap: Vec<(String, String)> = args
        .windows(2)
        .filter(|pair| pair[0] == "--remap")
        .filter_map(|pair| pair[1].rsplit_once('='))
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect();
    if let Err(message) = run(replay, out, &remap) {
        eprintln!("replay_hash: {message}");
        std::process::exit(1);
    }
}
