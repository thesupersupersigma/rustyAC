// SPDX-License-Identifier: GPL-3.0-or-later

//! `acd_check`: is a car read out of `data.acd` in memory the same car as the one read from
//! its extracted files?
//!
//! For every car of the install (`<AC_ROOT>/content/cars/<car>`) that has a `data.acd`:
//! 1. every file of the archive is decrypted in memory and compared, byte for byte, with the
//!    file of the same name in `cardata/<car>` (the files `tools/acd_extract.py` wrote);
//! 2. the car is built twice, once from `content/cars/<car>/data` (which only exists inside
//!    the archive) and once from `cardata/<car>`, put on a flat road and driven for some
//!    steps with the pedal half down; every traced value of every step must have the same
//!    bits. A car the port refuses must be refused with the same words from both sources.
//!
//! Nothing is written anywhere: the decrypted bytes only ever exist in memory.
//!
//!     acd_check [--cardata <folder>] [--steps N] [--only <car>] [--verbose]

use std::path::{Path, PathBuf};

use rustyac_content::acd::Acd;
use rustyac_physics::car::replay::{snapshot, Ground};
use rustyac_physics::car::{ChassisEnvironment, ScriptedDevice, VanillaCar};
use rustyac_physics::vecmath::Vec3f;

const DT: f32 = 0.003;
const CLOCK: f64 = 60_000.0;

/// Builds the car and drives it: the traced values of every step, or the port's refusal.
fn drive(data: &Path, steps: usize) -> Result<Vec<Vec<u64>>, String> {
    let mut car = VanillaCar::new(data, ChassisEnvironment::default(), Box::new(Ground::Flat), 1, CLOCK, ScriptedDevice::default())?;
    car.car.autoclutch.use_auto_on_start = true;
    car.car.autoclutch.use_auto_on_change = true;
    car.car.auto_shifter.is_active = true;
    car.car.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
    car.car.force_position(&Vec3f::new(0.0, 0.0, 0.0));
    car.car.session_start()?;
    car.car.trace = Some(Default::default());
    car.device.controls.gas = 0.5;
    car.device.controls.steer = 0.1;
    let mut out = Vec::with_capacity(steps);
    for step in 0..steps {
        car.step(DT, CLOCK + (step as f64 + 1.0) * 3.0);
        out.push(snapshot(&car.car));
    }
    Ok(out)
}

/// The refusal without the folder it names (the two sources are in different places).
fn without_path(message: &str, data: &Path) -> String {
    message.replace(&data.display().to_string(), "<data>")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let verbose = args.iter().any(|a| a == "--verbose");
    let steps: usize = value("--steps").and_then(|s| s.parse().ok()).unwrap_or(300);
    let only = value("--only");
    let cardata = value("--cardata").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata"));
    let Some(root) = rustyac_content::install::ac_root() else {
        eprintln!("NOT TESTED: {}", rustyac_content::install::not_found_hint());
        std::process::exit(2);
    };
    let cars_folder = root.join("content").join("cars");
    let mut names: Vec<String> = std::fs::read_dir(&cars_folder)
        .map(|dir| dir.flatten().filter(|e| e.path().join("data.acd").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    if let Some(only) = &only {
        names.retain(|n| n == only);
    }
    println!("{} cars with a data.acd under {}", names.len(), cars_folder.display());

    let (mut files_same, mut files_differ, mut no_extract) = (0usize, 0usize, 0usize);
    let (mut cars_same, mut cars_refused_same, mut cars_differ) = (0usize, 0usize, 0usize);
    let (mut total_files, mut total_values) = (0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    for name in &names {
        let acd_path = cars_folder.join(name).join("data.acd");
        let archive = match Acd::open(&acd_path) {
            Ok(archive) => archive,
            Err(message) => {
                failures.push(format!("{name}: {message}"));
                continue;
            }
        };
        let extracted = cardata.join(name);
        if !extracted.join("car.ini").is_file() {
            no_extract += 1;
            if verbose {
                println!("{name}: {} files in the archive, no extracted folder to compare with", archive.len());
            }
            continue;
        }
        // 1. the bytes
        let mut differ = Vec::new();
        let entries: Vec<(&str, &[u8])> = archive.entries().collect();
        for (i, (file, bytes)) in entries.iter().enumerate() {
            total_files += 1;
            // a name that comes twice in an archive (seen in one mod, for two Lua scripts that
            // no physics reads): the extractor wrote both to the same file, the last one stayed
            if entries[i + 1..].iter().any(|(later, _)| later == file) {
                duplicates.push(format!("{name}/{file}"));
                continue;
            }
            match std::fs::read(extracted.join(file)) {
                Ok(on_disk) if on_disk == *bytes => {}
                Ok(_) => differ.push(format!("{file} (other bytes)")),
                Err(_) => differ.push(format!("{file} (not in the extracted folder)")),
            }
        }
        if differ.is_empty() {
            files_same += 1;
        } else {
            files_differ += 1;
            failures.push(format!("{name}: files differ: {}", differ.join(", ")));
        }
        // 2. the car
        let in_archive = cars_folder.join(name).join("data");
        let (a, b) = (drive(&in_archive, steps), drive(&extracted, steps));
        match (a, b) {
            (Ok(a), Ok(b)) => {
                let values: usize = a.iter().map(Vec::len).sum();
                total_values += values;
                if a == b {
                    cars_same += 1;
                    if verbose {
                        println!("{name}: {} files the same, {values} values over {steps} steps the same", archive.len());
                    }
                } else {
                    cars_differ += 1;
                    let step = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(0);
                    failures.push(format!("{name}: the two cars differ from step {step} on"));
                }
            }
            (Err(a), Err(b)) => {
                if without_path(&a, &in_archive) == without_path(&b, &extracted) {
                    cars_refused_same += 1;
                    if verbose {
                        println!("{name}: {} files the same; not a car the port drives yet ({})", archive.len(), without_path(&a, &in_archive));
                    }
                } else {
                    cars_differ += 1;
                    failures.push(format!("{name}: refused differently: {a} / {b}"));
                }
            }
            (a, b) => {
                cars_differ += 1;
                failures.push(format!("{name}: one source builds, the other does not: {:?} / {:?}", a.err(), b.err()));
            }
        }
    }
    println!("files: {files_same} cars with every file of the archive byte-identical to the extracted one ({total_files} files), {files_differ} cars with a difference, {no_extract} cars without an extracted folder");
    println!("cars:  {cars_same} built and driven {steps} steps from both sources with the same bits in every value ({total_values} values), {cars_refused_same} refused by the port with the same words from both, {cars_differ} different");
    if !duplicates.is_empty() {
        println!("note:  names that are twice in one archive (only the last entry was compared): {}", duplicates.join(", "));
    }
    for failure in &failures {
        println!("FAILED {failure}");
    }
    if failures.is_empty() {
        println!("RESULT: a car read out of data.acd is the car read from its extracted files");
    } else {
        std::process::exit(1);
    }
}
