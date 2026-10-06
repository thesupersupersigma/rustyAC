//! Test oracle for `rustyac-ode`, the Rust port of the rigid-body library inside Assetto Corsa.
//!
//! ode_oracle micro [--type <name,name>] [--worlds <n>] [--steps <n>] [--first-seed <n>] [--verbose]
//!                  [--record <dir>]
//!     Synthetic worlds stepped in the game's own ODE (functions called inside the mapped
//!     acs.exe; the game never starts) and in the Rust port, compared bit for bit after
//!     every step. Types: free_body, ball, dball, fixed, slider, auto_disable, car, random. Writes
//!     `oracle/ode/micro_results.md`. `--record` also stores the reference states.
//! ode_oracle replay [<recording> ...] [--verbose]
//!     Replays car_oracle recordings (default: every `oracle/car/*.carrec`) through the Rust
//!     port: per-step test and free run. Needs no acs.exe. Writes `oracle/ode/replay_results.md`.
//! ode_oracle excerpt
//!     Writes the small golden excerpts of `settle` and `slalom` used by `cargo test`.
//! ode_oracle bench [--steps <n>]
//!     Step time of the car layout: the game's ODE against the Rust port.
//!
//! Common: [--acs <path to acs.exe>]

#[path = "../../car_oracle/src/acs.rs"]
#[allow(dead_code)]
mod acs;
mod engine;
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
     ode_oracle replay [<recording> ...] [--verbose]\n       \
     ode_oracle excerpt\n       \
     ode_oracle bench [--steps <n>]\n       \
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
    first_seed: u64,
    verbose: bool,
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
        first_seed: 0,
        verbose: false,
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
            "--first-seed" => a.first_seed = number(value()?)? as u64,
            "--verbose" => a.verbose = true,
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
        "replay" => replay::replay_command(&args.files, args.verbose),
        "excerpt" => replay::excerpt_command(),
        "bench" => bench_command(&args),
        _ => Err(usage()),
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// (name, what it covers, default number of worlds, default steps)
const TYPES: [(&str, &str, usize, usize); 8] = [
    ("free_body", "one free body: gravity, forces, torques, gyroscopic term, rotation update", 200, 1000),
    ("ball", "one ball joint (two bodies, or one body and the world)", 100, 1000),
    ("dball", "one fixed-length rod (DBall)", 100, 1000),
    ("fixed", "one fixed joint", 100, 1000),
    ("slider", "one slider joint", 100, 1000),
    ("auto_disable", "damped chains with ODE's auto-disable on; bodies fall asleep, are woken and disabled", 100, 1000),
    ("car", "F2004 layout: body, fuel tank, 4 hubs, 20 rods, 1 fixed joint; springs, tyres, steering", 20, 5000),
    ("random", "1 to 8 bodies, random chains of all joint types, loops, several islands", 1000, 1000),
];

fn scene_of(kind: &str, seed: u64) -> Result<Scene, String> {
    Ok(match kind {
        "free_body" => Scene::free_body(seed),
        "ball" => Scene::single_joint(Kind::Ball, seed),
        "dball" => Scene::single_joint(Kind::DBall, seed),
        "fixed" => Scene::single_joint(Kind::Fixed, seed),
        "slider" => Scene::single_joint(Kind::Slider, seed),
        "auto_disable" => Scene::auto_disable(seed),
        "car" => Scene::car(seed),
        "random" => Scene::random(seed),
        other => return Err(format!("no world type {other:?}")),
    })
}

fn micro_command(args: &Args) -> Result<(), String> {
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
    let mut notes = String::new();
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
        let mut asleep = 0;
        let mut first: Option<String> = None;
        for w in 0..worlds {
            let seed = args.first_seed + w as u64;
            let mut scene = scene_of(name, seed)?;
            let mut ac = AcEngine::new(&acs);
            let mut rust = RustEngine::new();
            let mut record = args.record.as_ref().map(|_| Vec::new());
            let outcome = micro::run(&mut scene, &mut ac, &mut rust, steps, record.as_mut());
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
            asleep += outcome.asleep;
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
            (not_finite > 0).then(|| format!("{not_finite} of the {worlds} worlds reached a value that is not finite")),
            (asleep > 0).then(|| format!("bodies were disabled (asleep) in {asleep} body-steps")),
        ]
        .into_iter()
        .flatten()
        {
            println!("  note: {note}");
            notes.push_str(&format!("- `{name}`: {note}\n"));
        }
    }
    if !notes.is_empty() {
        table.push_str("\nNotes:\n\n");
        table.push_str(&notes);
    }
    let path = out_dir.join("micro_results.md");
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if failed {
        return Err("at least one step of at least one world differs".into());
    }
    Ok(())
}

/// Rough step time of the car layout in both engines (each stepped on its own, same forces).
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
            ac.set_world([0.0, -9.806, 0.0], 0.3, 1e-7);
            rust.set_world([0.0, -9.806, 0.0], 0.3, 1e-7);
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
        "car layout (6 bodies, 21 joints, 26 rows), {steps} steps, best of 5: acs.exe dWorldStep {:.2} us/step, \
         rustyac-ode World::step {:.2} us/step (ratio {:.2})",
        best[0] * 1e6,
        best[1] * 1e6,
        best[1] / best[0]
    );
    Ok(())
}
