// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test oracle for `rustyac-ode`, the Rust port of the rigid-body library inside Assetto Corsa.
//!
//! ode_oracle micro [--type <name,name>] [--worlds <n>] [--steps <n>] [--first-seed <n>] [--verbose]
//!                  [--record <dir>]
//!     Synthetic worlds stepped in the game's own ODE (functions called inside the mapped
//!     acs.exe; the game never starts) and in the Rust port, compared bit for bit after
//!     every step. Types: free_body, ball, dball, dball_zero, fixed, slider, auto_disable, car,
//!     strut_car, random. The full run (no option that narrows it) writes
//!     `oracle/ode/micro_results.md`, any other run `oracle/ode/micro_partial.md`.
//!     `--record` also stores the reference states.
//! ode_oracle golden
//!     Writes the op-stream golden files of `crates/rustyac-ode/tests/golden_ops.rs`: a few
//!     small worlds of every type with a hash of the game's state after every step.
//! ode_oracle replay [<recording> ...] [--verbose]
//!     Replays car_oracle recordings (default: every `oracle/car/*.carrec`) through the Rust
//!     port: per-step test and free run. Needs no acs.exe. Writes `oracle/ode/replay_results.md`
//!     (`replay_partial.md` when recordings are named).
//! ode_oracle excerpt
//!     Writes the small golden excerpts of `settle` and `slalom` used by `cargo test`.
//! ode_oracle matrix
//!     The game's solver and maths routines one by one against the Rust versions: `_dFactorLDLT`,
//!     `_dSolveLDLT`, `_dInvertPDMatrix`, `_dIsPositiveDefinite` and `_dDot` on random matrices of
//!     1 to 64 rows, and the small rotation and vector functions on random and awkward input.
//!     Writes `oracle/ode/matrix_results.md` and the vectors of
//!     `crates/rustyac-ode/tests/golden_functions.rs`.
//! ode_oracle bench [--steps <n>] [--feedback]
//!     Step time of the car layout: the game's ODE against the Rust port, without joint feedback
//!     buffers as in the game, or with them.
//!
//! Common: [--acs <path to acs.exe>]

#[path = "../../car_oracle/src/acs.rs"]
#[allow(dead_code)]
mod acs;
mod engine;
mod matrix_check;
mod micro;
#[path = "../../car_oracle/src/record.rs"]
#[allow(dead_code)]
mod record;
mod replay;
#[path = "../../car_oracle/src/sites.rs"]
#[allow(dead_code)]
mod sites;

use std::path::{Path, PathBuf};

use engine::{AcEngine, Engine, Kind, RustEngine};
use micro::{Outcome, Scene};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

fn usage() -> String {
    "usage: ode_oracle micro [--type <name,name>] [--worlds <n>] [--steps <n>] [--first-seed <n>] [--verbose] [--record <dir>]\n       \
     ode_oracle golden\n       \
     ode_oracle replay [<recording> ...] [--verbose]\n       \
     ode_oracle excerpt\n       \
     ode_oracle matrix\n       \
     ode_oracle bench [--steps <n>] [--feedback]\n       \
     common: [--acs <path to acs.exe>]"
        .to_string()
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

struct Args {
    command: String,
    files: Vec<PathBuf>,
    types: Vec<String>,
    worlds: Option<usize>,
    steps: Option<usize>,
    first_seed: Option<u64>,
    verbose: bool,
    feedback: bool,
    record: Option<PathBuf>,
    acs: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let mut a = Args {
        command: it.next().ok_or_else(usage)?,
        files: Vec::new(),
        types: Vec::new(),
        worlds: None,
        steps: None,
        first_seed: None,
        verbose: false,
        feedback: false,
        record: None,
        acs: PathBuf::from(DEFAULT_ACS),
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let number = |text: String| text.parse::<usize>().map_err(|_| format!("{text}: not a number"));
        match flag.as_str() {
            "--type" => a.types = value()?.split(',').map(str::to_string).collect(),
            "--worlds" => a.worlds = Some(number(value()?)?),
            "--steps" => a.steps = Some(number(value()?)?),
            "--first-seed" => a.first_seed = Some(number(value()?)? as u64),
            "--verbose" => a.verbose = true,
            "--feedback" => a.feedback = true,
            "--record" => a.record = Some(PathBuf::from(value()?)),
            "--acs" => a.acs = PathBuf::from(value()?),
            other if !other.starts_with("--") => a.files.push(PathBuf::from(other)),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    Ok(a)
}

fn main() {
    let result = parse_args().and_then(|args| match args.command.as_str() {
        "micro" => micro_command(&args),
        "golden" => golden_command(&args),
        "replay" => replay::replay_command(&args.files, args.verbose),
        "excerpt" => replay::excerpt_command(),
        "bench" => bench_command(&args),
        "matrix" => matrix_command(&args),
        _ => Err(usage()),
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// (name, what it covers, default number of worlds, default steps)
const TYPES: [(&str, &str, usize, usize); 10] = [
    ("free_body", "one free body: gravity, forces, torques, gyroscopic term, rotation update", 200, 1000),
    ("ball", "one ball joint (two bodies, or one body and the world)", 100, 1000),
    ("dball", "one fixed-length rod (DBall)", 100, 1000),
    ("dball_zero", "a rod of length zero: the fallback directions of the DBall row", 100, 1000),
    ("fixed", "one fixed joint", 100, 1000),
    ("slider", "one slider joint; some with every motor parameter set and with stops far away", 100, 1000),
    ("auto_disable", "damped chains with ODE's auto-disable on; bodies fall asleep, are woken and disabled", 100, 1000),
    ("car", "F2004 layout: body, fuel tank, 4 hubs, 20 rods, 1 fixed joint; springs, tyres, steering", 20, 5000),
    ("strut_car", "strut car: body, tank, 4 hubs, 4 strut bodies; per corner a slider, a ball joint and 3 rods", 20, 5000),
    (
        "random",
        "1 to 8 bodies, random chains of all joint types, loops, several islands; half of them also use \
         what the game never does (other world settings, kinematic bodies, disabled joints, re-attached joints)",
        1000,
        1000,
    ),
];

fn scene_of(kind: &str, seed: u64) -> Result<Scene, String> {
    Ok(match kind {
        "free_body" => Scene::free_body(seed),
        "ball" => Scene::single_joint(Kind::Ball, seed),
        "dball" => Scene::single_joint(Kind::DBall, seed),
        "dball_zero" => Scene::dball_zero(seed),
        "fixed" => Scene::single_joint(Kind::Fixed, seed),
        "slider" => Scene::single_joint(Kind::Slider, seed),
        "auto_disable" => Scene::auto_disable(seed),
        "car" => Scene::car(seed),
        "strut_car" => Scene::strut_car(seed),
        "random" => Scene::random(seed),
        other => return Err(format!("no world type {other:?}")),
    })
}

/// Keeps the port's "not implemented: bounded rows" message off the screen (the engine wrapper
/// catches that panic and the world ends there); every other panic is reported as usual.
fn quiet_bounded_rows() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !info.to_string().contains(engine::BOUNDED_ROWS_MESSAGE) {
            default_hook(info);
        }
    }));
}

fn micro_command(args: &Args) -> Result<(), String> {
    quiet_bounded_rows();
    for wanted in &args.types {
        if !TYPES.iter().any(|t| t.0 == wanted) {
            let names: Vec<&str> = TYPES.iter().map(|t| t.0).collect();
            return Err(format!("no world type {wanted:?} (types: {})", names.join(", ")));
        }
    }
    let acs = acs::Acs::load(&args.acs)?;
    engine::init_ode(&acs);
    let out_dir = repo_root().join("oracle/ode");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    if let Some(dir) = &args.record {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut table = String::from(
        "| world type | what it covers | worlds | steps | steps bit-exact | worlds bit-exact to the end | set-up bit-exact \
         | largest matrix | hash of the reference states | first difference |\n|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let mut failed = false;
    let mut compared = 0;
    let mut notes = String::new();
    let mut operations = String::new();
    for (name, about, default_worlds, default_steps) in TYPES {
        if !args.types.is_empty() && !args.types.iter().any(|t| t == name) {
            continue;
        }
        let worlds = args.worlds.unwrap_or(default_worlds);
        let steps = args.steps.unwrap_or(default_steps);
        let mut total = Outcome { setup_exact: true, hash: 0xcbf2_9ce4_8422_2325, ..Outcome::default() };
        let mut exact_worlds = 0;
        let mut setup_exact_worlds = 0;
        let mut not_finite = 0;
        let mut no_feedback = 0;
        let mut ended_early = 0;
        let mut steps_cut = 0;
        let mut first: Option<String> = None;
        for w in 0..worlds {
            let seed = args.first_seed.unwrap_or(0) + w as u64;
            let mut scene = scene_of(name, seed)?;
            let mut ac = AcEngine::new(&acs);
            let mut rust = RustEngine::new();
            let mut record = args.record.as_ref().map(|_| Vec::new());
            let outcome = micro::run(&mut scene, &mut ac, &mut rust, steps, record.as_mut(), None);
            total.max_rows = total.max_rows.max(outcome.max_rows);
            if let (Some(dir), Some(words)) = (&args.record, record) {
                let mut bytes = Vec::with_capacity(words.len() * 4 + 64);
                bytes.extend_from_slice(b"ACODEMIC");
                bytes.extend_from_slice(&(scene.bodies as u32).to_le_bytes());
                bytes.extend_from_slice(&(scene.joints as u32).to_le_bytes());
                bytes.extend_from_slice(&(steps as u32).to_le_bytes());
                for w in words {
                    bytes.extend_from_slice(&w.to_le_bytes());
                }
                let path = dir.join(format!("{}.odemicro", scene.name.replace('#', "_")));
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            total.steps += outcome.steps;
            total.exact_steps += outcome.exact_steps;
            for byte in outcome.hash.to_le_bytes() {
                total.hash ^= byte as u64;
                total.hash = total.hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            exact_worlds += (outcome.exact_steps == outcome.steps && outcome.setup_exact) as usize;
            setup_exact_worlds += outcome.setup_exact as usize;
            not_finite += outcome.not_finite as usize;
            no_feedback += !scene.feedback as usize;
            if let Some(at) = outcome.bounded_at {
                ended_early += 1;
                steps_cut += steps - at;
                if args.verbose {
                    println!("  {}: ended at step {at}, a slider reached a stop (bounded row)", scene.name);
                }
            }
            total.nan_steps += outcome.nan_steps;
            total.asleep += outcome.asleep;
            total.short_rods += outcome.short_rods;
            total.last_resort += outcome.last_resort;
            total.idle_joints += outcome.idle_joints;
            for k in 0..28 {
                total.setup_ops[k] += outcome.setup_ops[k];
                total.step_ops[k] += outcome.step_ops[k];
            }
            if let Some(text) = outcome.first {
                if args.verbose {
                    println!("  {}: {text} ({} of {} steps exact)", scene.name, outcome.exact_steps, outcome.steps);
                }
                if first.is_none() {
                    first = Some(format!("{}, {text}", scene.name));
                }
            }
        }
        failed |= total.exact_steps != total.steps || setup_exact_worlds != worlds;
        compared += total.steps;
        let percent = |part: usize, whole: usize| {
            if part == whole {
                format!("100 % ({part}/{whole})")
            } else {
                format!("{:.4} % ({part}/{whole})", 100.0 * part as f64 / whole.max(1) as f64)
            }
        };
        let line = format!(
            "| `{name}` | {about} | {worlds} | {} | {} | {} | {} | {} | `{:016x}` | {} |",
            total.steps,
            percent(total.exact_steps, total.steps),
            percent(exact_worlds, worlds),
            percent(setup_exact_worlds, worlds),
            total.max_rows,
            total.hash,
            first.unwrap_or_else(|| "none".into()),
        );
        println!("{line}");
        table.push_str(&line);
        table.push('\n');
        for note in [
            (no_feedback > 0).then(|| {
                format!("{no_feedback} of the {worlds} worlds ran as the game does, without joint feedback buffers")
            }),
            (not_finite > 0).then(|| {
                format!(
                    "{not_finite} of the {worlds} worlds reached a value that is not finite; {} steps had a NaN \
                     in the reference state (NaN against NaN counts as equal)",
                    total.nan_steps
                )
            }),
            (ended_early > 0).then(|| {
                format!(
                    "{ended_early} of the {worlds} worlds were ended early, in the step in which a slider reached \
                     one of its far stops, because ODE then adds a bounded row (stage 2); {steps_cut} steps were \
                     not run for that reason"
                )
            }),
            (total.asleep > 0).then(|| format!("bodies were disabled (asleep) in {} body-steps", total.asleep)),
            (total.idle_joints > 0).then(|| {
                format!(
                    "joints were in no island (disabled, detached by a destroyed body, between sleeping bodies or \
                     with no moving body) in {} joint-steps",
                    total.idle_joints
                )
            }),
            (total.short_rods > 0).then(|| {
                format!(
                    "the rod's anchors were less than 1e-7 m apart in {} joint-steps (the row takes the direction of \
                     the anchors' relative velocity); in {} of them that velocity was below 1e-7 too (last resort: \
                     the direction (1, 0, 0))",
                    total.short_rods, total.last_resort
                )
            }),
        ]
        .into_iter()
        .flatten()
        {
            println!("  note: {note}");
            notes.push_str(&format!("- `{name}`: {note}\n"));
        }
        let list = |counts: &[usize; 28]| {
            let parts: Vec<String> = (1..28)
                .filter(|&k| counts[k] > 0)
                .map(|k| format!("{} {}", micro::OP_NAMES[k], counts[k]))
                .collect();
            if parts.is_empty() { "nothing".to_string() } else { parts.join(", ") }
        };
        operations.push_str(&format!(
            "- `{name}`: before the first step: {}. Between steps: {}.\n",
            list(&total.setup_ops),
            list(&total.step_ops)
        ));
    }
    if !notes.is_empty() {
        table.push_str("\nNotes:\n\n");
        table.push_str(&notes);
    }
    table.push_str("\nOperations applied to both engines (number of calls):\n\n");
    table.push_str(&operations);
    // only the full run may replace the table the report is built from
    let full = args.types.is_empty() && args.worlds.is_none() && args.steps.is_none() && args.first_seed.is_none();
    let path = out_dir.join(if full { "micro_results.md" } else { "micro_partial.md" });
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if compared == 0 {
        return Err("no step was compared".into());
    }
    if failed {
        return Err("at least one step of at least one world differs".into());
    }
    Ok(())
}

/// (world type, seed, steps) of the golden files.
const GOLDEN: [(&str, u64, usize); 31] = [
    ("free_body", 1, 150),
    ("free_body", 2, 150),
    ("ball", 0, 150),
    ("ball", 2, 150),
    ("ball", 5, 150),
    ("dball", 1, 150),
    ("dball", 2, 150),
    ("dball_zero", 0, 60),
    ("dball_zero", 1, 60),
    ("dball_zero", 3, 60),
    ("fixed", 0, 150),
    ("fixed", 5, 150),
    ("slider", 1, 150),
    ("slider", 3, 150),
    ("slider", 5, 150),
    ("auto_disable", 1, 500),
    ("auto_disable", 2, 500),
    ("car", 1, 80),
    ("strut_car", 1, 80),
    // plain worlds; 14 and 61 have the largest matrices
    ("random", 1, 150),
    ("random", 14, 200),
    ("random", 61, 150),
    // re-attached joints, disabled joints, body flags written by hand
    ("random", 4, 300),
    ("random", 9, 300),
    // other gravity, damping and step sizes
    ("random", 6, 300),
    ("random", 21, 300),
    ("random", 52, 200),
    // a body and a rod added on the way; a body destroyed on the way
    ("random", 7, 450),
    ("random", 53, 480),
    // auto-disable on; a single body that starts disabled
    ("random", 20, 300),
    ("random", 42, 150),
];

/// Small worlds of every type written as op streams with a hash of the game's state after
/// every step, for the crate's own test (which then needs no game).
fn golden_command(args: &Args) -> Result<(), String> {
    quiet_bounded_rows();
    let acs = acs::Acs::load(&args.acs)?;
    engine::init_ode(&acs);
    let dir = repo_root().join("crates/rustyac-ode/tests/data/ops");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut total = 0;
    // `--worlds n` only looks at random worlds (to choose seeds for the list above)
    let scan: Vec<(&str, u64, usize)> = (0..args.worlds.unwrap_or(0) as u64)
        .map(|w| ("random", args.first_seed.unwrap_or(0) + w, args.steps.unwrap_or(480)))
        .collect();
    let list: &[(&str, u64, usize)] = if scan.is_empty() { &GOLDEN } else { &scan };
    for &(name, seed, steps) in list {
        let mut scene = scene_of(name, seed)?;
        let mut ac = AcEngine::new(&acs);
        let mut rust = RustEngine::new();
        let mut golden = micro::Golden { bytes: Vec::new() };
        let outcome = micro::run(&mut scene, &mut ac, &mut rust, steps, None, Some(&mut golden));
        if outcome.exact_steps != outcome.steps || !outcome.setup_exact {
            return Err(format!("{}: the Rust port differs ({})", scene.name, outcome.first.unwrap_or_default()));
        }
        if outcome.bounded_at.is_some() && scan.is_empty() {
            return Err(format!("{}: reaches a slider stop, not usable as a golden world", scene.name));
        }
        if scan.is_empty() {
            let path = dir.join(format!("{name}_{seed}.odeops"));
            std::fs::write(&path, &golden.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        total += golden.bytes.len();
        println!(
            "{:<16} {:>4} steps {:>7} bytes  bodies {} joints {} rows {} feedback {} asleep {} idle joints {} \
             short rods {} (last resort {}) not finite {} h {} gravity {:?} damping {:?} destroyed {} re-attached {} \
             flags {}/{} kinematic {}",
            format!("{name}_{seed}"),
            steps,
            golden.bytes.len(),
            scene.bodies,
            scene.joints,
            outcome.max_rows,
            scene.feedback,
            outcome.asleep,
            outcome.idle_joints,
            outcome.short_rods,
            outcome.last_resort,
            outcome.not_finite,
            scene.h,
            scene.gravity,
            scene.damping,
            outcome.step_ops[13],
            outcome.step_ops[19],
            outcome.setup_ops[14] + outcome.setup_ops[26],
            outcome.step_ops[26],
            outcome.setup_ops[16],
        );
    }
    if scan.is_empty() {
        println!("wrote {} files, {total} bytes, to {}", GOLDEN.len(), dir.display());
    }
    Ok(())
}

/// The solver and maths routines alone (see `matrix_check.rs`).
fn matrix_command(args: &Args) -> Result<(), String> {
    let acs = acs::Acs::load(&args.acs)?;
    let table = matrix_check::matrix_command(&acs)?;
    let out_dir = repo_root().join("oracle/ode");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let path = out_dir.join("matrix_results.md");
    std::fs::write(&path, table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Rough step time of the car layout in both engines (each stepped on its own, same forces;
/// no joint feedback buffers, as in the game, unless `--feedback` is given).
fn bench_command(args: &Args) -> Result<(), String> {
    let acs = acs::Acs::load(&args.acs)?;
    engine::init_ode(&acs);
    let steps = args.steps.unwrap_or(20_000);
    let mut best = [f64::MAX; 2];
    for _round in 0..5 {
        for which in 0..2 {
            let mut scene = Scene::car(0);
            let mut ac = AcEngine::new(&acs);
            let mut rust = RustEngine::new();
            // both are built (the scene script reads the reference state), one is timed
            let mut driver_ac = micro::Driver::default();
            let mut driver_rust = micro::Driver::default();
            ac.set_world(scene.gravity, scene.erp, scene.cfm, scene.damping);
            rust.set_world(scene.gravity, scene.erp, scene.cfm, scene.damping);
            ac.set_feedback(args.feedback);
            rust.set_feedback(args.feedback);
            for op in &scene.setup.clone() {
                driver_ac.apply(&mut ac, op);
                driver_rust.apply(&mut rust, op);
            }
            let mut in_step = std::time::Duration::ZERO;
            for step in 0..steps {
                let reference: Vec<_> = (0..scene.bodies).map(|b| ac.body_state(b)).collect();
                for op in scene.before_step(step, &reference) {
                    driver_ac.apply(&mut ac, &op);
                    driver_rust.apply(&mut rust, &op);
                }
                if which == 0 {
                    let t = std::time::Instant::now();
                    ac.step(micro::H);
                    in_step += t.elapsed();
                    rust.step(micro::H);
                } else {
                    ac.step(micro::H);
                    let t = std::time::Instant::now();
                    rust.step(micro::H);
                    in_step += t.elapsed();
                }
            }
            best[which] = best[which].min(in_step.as_secs_f64() / steps as f64);
        }
    }
    println!(
        "car layout (6 bodies, 21 joints, 26 rows, {} joint feedback buffers), {steps} steps, best of 5: acs.exe \
         dWorldStep {:.2} us/step, rustyac-ode World::step {:.2} us/step (ratio {:.2})",
        if args.feedback { "with" } else { "no" },
        best[0] * 1e6,
        best[1] * 1e6,
        best[1] / best[0]
    );
    Ok(())
}
