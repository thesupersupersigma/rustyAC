// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run --release -p rustyac-web --example race_proof -- --race <race.ini> [--assists
//! <assists.ini>] --seed <n> --tape <drive.ryin> [--out <folder>] [--drag-drive]`: a drive from the launcher's
//! files as the page makes it (`rustyac_web::race::setup_of`), driven with the inputs of a drive
//! the desktop recorded, step for step (`rustyac_web::race::replay_race`). Builds for the
//! desktop and for `wasm32-wasip1`; the page runs the same function (`Game::race_proof`).
//!
//! Prints the chained hash of every car's bodies (the desktop's `--state-hash`), the number of
//! steps and cars, the hash of race_out.json and whether the page's setup is the drive's.
//! `--out` gets `steps.bin` (8 bytes per step: every car's bodies after it), `race_out.json`,
//! `header.txt` (the page's setup) and `tape_header.txt` (the recorded drive's).

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut race, mut assists, mut seed, mut tape, mut out) = (None, None, None, None, None);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--race" => race = Some(value()?),
            "--assists" => assists = Some(value()?),
            "--seed" => seed = Some(value()?.parse::<u32>().map_err(|e| format!("--seed: {e}"))?),
            "--tape" => tape = Some(value()?),
            "--out" => out = Some(value()?),
            // rustyac.exe --drag-drive's drive (the page's ?dragdrive=1)
            "--drag-drive" => rustyac_web::session::DRAG_DRIVE.with(|on| on.set(true)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
    let text = |path: &str| read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let race = text(&race.ok_or("--race <race.ini> is needed")?)?;
    let assists = assists.map(|path| text(&path)).transpose()?;
    let tape = read(&tape.ok_or("--tape <drive.ryin> is needed")?)?;
    let seed = seed.ok_or("--seed <n> is needed")?;
    let proof = rustyac_web::race::replay_race(&race, assists.as_deref(), seed, &tape)?;
    println!("state hash {:016x}: {} cars, {} steps", proof.chain, proof.cars, proof.step_hashes.len());
    match &proof.race_out {
        Some(json) => println!("race_out.json {} bytes (hash {:016x})", json.len(), fnv(json)),
        None => println!("race_out.json: none (no referee)"),
    }
    println!("setup: {}", if proof.header == proof.tape_header { "the recorded drive's".to_string() } else { "DIFFERS from the recorded drive's (header.txt, tape_header.txt)".to_string() });
    if let Some(out) = out {
        let out = std::path::Path::new(&out);
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let write = |name: &str, bytes: &[u8]| std::fs::write(out.join(name), bytes).map_err(|e| format!("{name}: {e}"));
        write("steps.bin", &proof.step_hashes.iter().flat_map(|h| h.to_le_bytes()).collect::<Vec<u8>>())?;
        write("header.txt", proof.header.as_bytes())?;
        write("tape_header.txt", proof.tape_header.as_bytes())?;
        if let Some(json) = &proof.race_out {
            write("race_out.json", json)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("race_proof: {message}");
        std::process::exit(1);
    }
}
