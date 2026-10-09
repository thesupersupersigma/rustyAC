// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Replays recordings of AC's own `Tyre::step` (made by `tools/tyre_oracle run`) through
//! `VanillaTyre` and compares every recorded value of every step bit for bit.
//!
//! tyre_compare --car <extracted car data dir> [--stats] <recording.csv> [<recording.csv> ...]
//!
//! Prints one table row per recording: steps, how many are bit-exact in every field, how
//! many are the same when any NaN counts as equal to any other NaN, and where the first
//! real difference is. `--stats` adds what each recording actually exercised.
//! Exit code 0 = everything bit-exact, 1 = differences, 2 = error.

use std::path::PathBuf;
use std::process::ExitCode;

use rustyac_physics::math::{self, Backend};
use rustyac_physics::tyre::rig::{self, output_fields, Recording};

struct Args {
    car: PathBuf,
    stats: bool,
    files: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let usage = "usage: tyre_compare --car <dir> [--stats] <recording.csv> [...]";
    let mut args = Args {
        car: PathBuf::new(),
        stats: false,
        files: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--car" => args.car = PathBuf::from(it.next().ok_or(usage)?),
            "--stats" => args.stats = true,
            flag if flag.starts_with("--") => {
                return Err(format!("unknown argument {flag}\n{usage}"))
            }
            file => args.files.push(PathBuf::from(file)),
        }
    }
    if args.car.as_os_str().is_empty() || args.files.is_empty() {
        return Err(usage.to_string());
    }
    Ok(args)
}

/// What a recording exercised, from AC's own outputs.
fn stats(recording: &Recording) -> String {
    let names = output_fields();
    let column = |name: &str| names.iter().position(|n| n == name).expect("known field");
    let f32s = |name: &str| -> Vec<f32> {
        let c = column(name);
        recording
            .outputs
            .iter()
            .map(|row| f32::from_bits(row[c] as u32))
            .collect()
    };
    let f64s = |name: &str| -> Vec<f64> {
        let c = column(name);
        recording
            .outputs
            .iter()
            .map(|row| f64::from_bits(row[c]))
            .collect()
    };
    let count = |name: &str, value: u64| {
        let c = column(name);
        recording
            .outputs
            .iter()
            .filter(|row| row[c] == value)
            .count()
    };
    let range = |values: &[f32]| {
        let low = values.iter().copied().fold(f32::INFINITY, f32::min);
        let high = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        format!("{low:.4}..{high:.4}")
    };
    let max64 = |values: &[f64]| values.iter().copied().fold(0.0, f64::max);
    let mut kinds = [0usize; 5];
    for i in 0..rig::MAX_CALLS {
        let c = column(&format!("call{i}.kind"));
        for row in &recording.outputs {
            kinds[row[c] as usize] += 1;
        }
    }
    let rim = {
        let radius = f32s("status.loadedRadius");
        let depth = f32s("status.depth");
        radius
            .iter()
            .zip(&depth)
            .filter(|(r, d)| **d > 0.1 && **r > 0.0)
            .count()
    };
    format!(
        "load {} N, Fy {} N, Fx {} N, wheel speed {} rad/s, slip angle {} rad, slip ratio {}, \
         core temp {} C, pressure {} psi, grip from temp {}, dirt {}, \
         max wear {:.4} vkm, max grain {:.3}, max blister {:.3}, max flat spot {:.4}, \
         steps locked {}, without ground {}, deflection over 10 cm {}, blankets on {}, deflated {}; \
         hub calls: addForceAtPos {}, addTorque {}, addLocalForceAndTorque {}, body addForceAtLocalPos {}",
        range(&f32s("status.load")),
        range(&f32s("status.Fy")),
        range(&f32s("status.Fx")),
        range(&f32s("status.angularVelocity")),
        range(&f32s("status.slipAngleRAD")),
        range(&f32s("status.slipRatio")),
        range(&f32s("thermal.coreTemp")),
        range(&f32s("status.pressureDynamic")),
        range(&f32s("thermal.thermalMultD")),
        range(&f32s("status.dirtyLevel")),
        max64(&f64s("status.virtualKM")),
        max64(&f64s("status.grain")),
        max64(&f64s("status.blister")),
        max64(&f64s("status.flatSpot")),
        count("status.isLocked", 1),
        count("hasSurfaceDef", 0),
        rim,
        count("tyreBlanketsOn", 1),
        count("status.inflation", 0),
        kinds[1],
        kinds[2],
        kinds[3],
        kinds[4],
    )
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;
    if math::backend() == Backend::Std {
        eprintln!("note: RUSTYAC_MATH=std: using Rust std maths and number parsing");
    }
    println!("| Scenario | Axle | Steps | Bit-exact steps | % | Same with NaN = NaN | % | First divergence |");
    println!("|---|---|---|---|---|---|---|---|");
    let mut all_exact = true;
    let mut total = (0usize, 0usize, 0usize);
    let mut notes = Vec::new();
    for file in &args.files {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let recording = Recording::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let replay = rig::replay(&recording, &args.car)?;
        let divergence = match &replay.first_divergence {
            None => "none".to_string(),
            Some(d) => {
                all_exact = false;
                format!(
                    "step {}, `{}`: AC {}, port {}",
                    d.step,
                    d.field,
                    rig::describe_word(&d.field, d.expected),
                    rig::describe_word(&d.field, d.got)
                )
            }
        };
        let scenario = recording.get("scenario").unwrap_or("?").to_string();
        let axle = recording.get("axle").unwrap_or("?").to_string();
        let percent = |count: usize| 100.0 * count as f64 / replay.steps.max(1) as f64;
        println!(
            "| {scenario} | {axle} | {} | {} | {:.4} | {} | {:.4} | {divergence} |",
            replay.steps,
            replay.exact_steps,
            percent(replay.exact_steps),
            replay.matching_steps,
            percent(replay.matching_steps),
        );
        total.0 += replay.steps;
        total.1 += replay.exact_steps;
        total.2 += replay.matching_steps;
        if !replay.field_mismatches.is_empty() {
            let worst: Vec<String> = replay
                .field_mismatches
                .iter()
                .take(12)
                .map(|(name, count)| format!("{name} x{count}"))
                .collect();
            notes.push(format!(
                "{scenario} {axle}: fields that differ: {}",
                worst.join(", ")
            ));
        }
        if args.stats {
            notes.push(format!("{scenario} {axle}: {}", stats(&recording)));
        }
    }
    println!(
        "| **Total** | | **{}** | **{}** | **{:.4}** | **{}** | **{:.4}** | |",
        total.0,
        total.1,
        100.0 * total.1 as f64 / total.0.max(1) as f64,
        total.2,
        100.0 * total.2 as f64 / total.0.max(1) as f64
    );
    for note in notes {
        println!("\n{note}");
    }
    Ok(all_exact)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
