// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whole-car test oracle: Assetto Corsa's own `PhysicsEngine` and one `Car` (constructed,
//! loaded and stepped by the game's code, mapped from acs.exe; the game itself never starts)
//! on a fake flat track, driven by scripted controls, with every force handed to a rigid
//! body and the full body / joint / tyre state recorded each step.
//!
//! car_oracle list
//! car_oracle run --scenario <name> [--out <dir>] [--steps <n>] [--floor] [--no-joint-forces] [--hash-only]
//!                [--car <folder under cardata/>]
//!     One scenario in this process. Prints `<name> steps=… bytes=… hash=…`. `--floor` adds a
//!     collision mesh under the car (file `<name>_floor.carrec`).
//! car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]
//!     Every scenario twice (fresh process each time), compares the two runs byte for byte,
//!     replays the tyres through the Rust port, and writes `<out>/results.md`.
//! car_oracle check <recording>
//!     Tyre cross-check and headline numbers of one recording.
//! car_oracle setup-check [--car <name>]
//!     Builds the car, does what the game's setup screen does at a session start and lets the
//!     game's own code print every value that changes ("Setup change for Car: …"), for
//!     comparing with a real game's log.txt.
//! car_oracle diff <recording> <recording> [--only <prefix,prefix>] [--ignore <part,part>]
//!     Value-by-value comparison of two recordings (fields and force tape).
//! car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>]
//!                [--every <n>] [--only <prefix,prefix>] [--csv-out <file>]
//!     Full-precision CSV of a recording.
//! car_oracle rays --track <track folder> [--count <n>] [--seed <n>]
//!     The ray micro-oracle: the track's meshes in the game's own ODE and in the Rust port,
//!     `count` rays (default 1,000,000) through both, every answer compared bit for bit.
//!     Writes `oracle/track/rays_results.md`.

mod acs;
mod check;
mod game;
mod record;
mod scenario;
mod sites;
mod track;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use record::{CsvOptions, Recording, Writer};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

fn usage() -> String {
    "usage: car_oracle list\n       \
     car_oracle run --scenario <name> [--out <dir>] [--steps <n>] [--floor] [--no-joint-forces] [--hash-only]\n       \
     car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]\n       \
     car_oracle check <recording>\n       \
     car_oracle setup-check [--car <name>]\n       \
     car_oracle diff <recording> <recording> [--only <prefix,prefix>] [--ignore <part,part>]\n       \
     car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>] [--every <n>] \
     [--only <prefix,prefix>] [--csv-out <file>]\n       \
     common: [--acs <path to acs.exe>] [--root <scratch game folder>] [--car <folder under cardata/>] [--verbose]"
        .to_string()
}

fn repo_root() -> PathBuf {
    // tools/car_oracle -> the repository (no canonicalize: it would turn the path into a \\?\ one)
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

struct Args {
    command: String,
    file: Option<PathBuf>,
    file2: Option<PathBuf>,
    scenario: String,
    out: PathBuf,
    steps: Option<usize>,
    joint_forces: bool,
    floor: bool,
    hash_only: bool,
    only: Vec<String>,
    ignore: Vec<String>,
    table: String,
    from: usize,
    to: usize,
    every: usize,
    csv_out: Option<PathBuf>,
    acs: PathBuf,
    root: PathBuf,
    car: String,
    verbose: bool,
    track: Option<PathBuf>,
    count: usize,
    seed: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let repo = repo_root();
    let mut a = Args {
        command: it.next().ok_or_else(usage)?,
        file: None,
        file2: None,
        scenario: String::new(),
        out: repo.join("oracle/car"),
        steps: None,
        joint_forces: true,
        floor: false,
        hash_only: false,
        only: Vec::new(),
        ignore: Vec::new(),
        table: "steps".into(),
        from: 0,
        to: usize::MAX,
        every: 1,
        csv_out: None,
        acs: PathBuf::from(DEFAULT_ACS),
        root: repo.join("re/scratch/car_oracle/root"),
        car: game::DEFAULT_CAR.to_string(),
        verbose: false,
        track: None,
        count: 1_000_000,
        seed: 12,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let number = |text: String| text.parse::<usize>().map_err(|_| format!("{text}: not a number"));
        match flag.as_str() {
            "--scenario" => a.scenario = value()?,
            "--out" => a.out = PathBuf::from(value()?),
            "--steps" => a.steps = Some(number(value()?)?),
            "--no-joint-forces" => a.joint_forces = false,
            "--floor" => a.floor = true,
            "--hash-only" => a.hash_only = true,
            "--only" => a.only = value()?.split(',').map(str::to_string).collect(),
            "--ignore" => a.ignore = value()?.split(',').map(str::to_string).collect(),
            "--table" => a.table = value()?,
            "--from" => a.from = number(value()?)?,
            "--to" => a.to = number(value()?)?,
            "--every" => a.every = number(value()?)?,
            "--csv-out" => a.csv_out = Some(PathBuf::from(value()?)),
            "--acs" => a.acs = PathBuf::from(value()?),
            "--root" => a.root = PathBuf::from(value()?),
            "--car" => a.car = value()?,
            "--verbose" => a.verbose = true,
            "--track" => a.track = Some(PathBuf::from(value()?)),
            "--count" => a.count = number(value()?)?,
            "--seed" => a.seed = number(value()?)? as u64,
            other if !other.starts_with("--") && a.file.is_none() => a.file = Some(PathBuf::from(other)),
            other if !other.starts_with("--") && a.file2.is_none() => a.file2 = Some(PathBuf::from(other)),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    Ok(a)
}

fn main() {
    let result = parse_args().and_then(|args| match args.command.as_str() {
        "list" => {
            for s in scenario::all().into_iter().chain(scenario::powertrain()).chain(scenario::whole()) {
                println!("{:24} {:6.1} s  {}", s.name, (s.steps - 1) as f32 * scenario::DT, s.about);
            }
            Ok(())
        }
        "run" => run(&args),
        "all" => all(&args),
        "check" => check_one(&args),
        "setup-check" => setup_check(&args),
        "diff" => diff(&args),
        "csv" => csv(&args),
        "rays" => rays(&args),
        _ => Err(usage()),
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// A track's folder: a path, or a name under the game's `content/tracks`.
fn track_folder(args: &Args, track: &Path) -> PathBuf {
    if track.is_dir() {
        return std::path::absolute(track).unwrap_or_else(|_| track.to_path_buf());
    }
    let game = args.acs.parent().unwrap_or(Path::new("."));
    game.join("content").join("tracks").join(track)
}

/// The ray micro-oracle.
fn rays(args: &Args) -> Result<(), String> {
    let track = args.track.as_ref().ok_or("rays needs --track <track folder>")?;
    let folder = track_folder(args, track);
    let repo = repo_root();
    game::prepare_root(&repo, &args.root, &args.car)?;
    track::prepare_root(&args.root, &folder)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let engine = game::new_engine(&acs, 1);
    let (report, ok) = track::rays(&acs, engine, &folder, args.count, args.seed)?;
    let path = track::results_path(&repo);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, format!("# Ray micro-oracle: the game's ODE / OPCODE against rustyac-ode\n\n`car_oracle rays --track {} --count {} --seed {}`\n\n{report}", track.display(), args.count, args.seed))
        .map_err(|e| e.to_string())?;
    println!("{report}");
    println!("written to {}", path.display());
    if ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

fn recording_path(out: &Path, name: &str) -> PathBuf {
    out.join(format!("{name}.carrec"))
}

/// One scenario, in this process (the game's objects cannot be torn down and rebuilt).
fn run(args: &Args) -> Result<(), String> {
    let mut found =
        scenario::find(&args.scenario).ok_or_else(|| format!("no scenario {:?} (see `car_oracle list`)", args.scenario))?;
    let scenario = &mut found;
    let name = if args.floor && !scenario.floor { format!("{}_floor", scenario.name) } else { scenario.name.to_string() };
    scenario.floor |= args.floor;
    let scenario = &*scenario;
    let repo = repo_root();
    // relative paths are the caller's; the game needs its own working directory
    let out = std::path::absolute(&args.out).map_err(|e| e.to_string())?;
    let data_hash = game::prepare_root(&repo, &args.root, &args.car)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;

    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let options = game::Options { joint_feedback: args.joint_forces, car: args.car.clone(), setup_check: false };
    let mut world = game::World::build(&acs, scenario, &options);
    let steps = args.steps.unwrap_or(scenario.steps);
    let mut meta = vec![
        ("scenario".to_string(), name.clone()),
        ("about".to_string(), scenario.about.to_string()),
        ("car".to_string(), args.car.clone()),
        ("car_data_hash".to_string(), format!("{data_hash:016x}")),
        ("dt".to_string(), format!("{:?}", scenario::DT)),
        ("clock_start_ms".to_string(), format!("{:?}", game::CLOCK_START_MS)),
        ("seed".to_string(), scenario.seed.to_string()),
        ("auto_clutch".to_string(), (scenario.auto_clutch as u8).to_string()),
        ("auto_shifter".to_string(), (scenario.auto_shifter as u8).to_string()),
        ("ground".to_string(), scenario.ground.describe()),
        ("floor_mesh".to_string(), (scenario.floor as u8).to_string()),
        ("joint_forces".to_string(), (args.joint_forces as u8).to_string()),
        ("wheels".to_string(), game::WHEELS.join(",")),
    ];
    meta.extend(world.facts());
    if scenario.whole.on {
        // only the whole-car scenarios: the older recordings keep their header (and hashes)
        meta.push(("whole".to_string(), "1".to_string()));
        meta.push(("pitlane".to_string(), (scenario.whole.pitlane as u8).to_string()));
        meta.push(("stability_gain".to_string(), format!("{:?}", scenario.whole.stability_gain)));
        meta.push(("wind_speed".to_string(), format!("{:?}", scenario.whole.wind_speed)));
        meta.push(("wind_direction_deg".to_string(), format!("{:?}", scenario.whole.wind_direction_deg)));
        meta.push(("damage".to_string(), scenario.whole.damage.map(|d| format!("{d:?}")).join(",")));
        meta.push(("penalty_mode".to_string(), if scenario.whole.penalty_cut_gas { "0" } else { "3" }.to_string()));
    }
    let path = recording_path(&out, &name);
    let mut writer = Writer::new((!args.hash_only).then_some(path.as_path()), meta).map_err(|e| e.to_string())?;
    let mut sites = BTreeSet::new();
    for i in 0..steps {
        let (row, calls) = world.step(i == 0);
        for call in &calls {
            sites.insert(call.site);
            if call.outer_site != 0 {
                sites.insert(call.outer_site);
            }
        }
        writer.step(&row, &calls).map_err(|e| e.to_string())?;
    }
    let mut trailer = String::new();
    for site in sites {
        trailer.push_str(&sites::describe(site));
        trailer.push('\n');
    }
    let (hash, bytes) = writer.finish(&trailer).map_err(|e| e.to_string())?;
    println!("{name} steps={steps} bytes={bytes} hash={hash:016x}");
    Ok(())
}

/// Runs `car_oracle run` for one scenario in a child process and returns its hash.
fn run_child(args: &Args, name: &str, hash_only: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.arg("run").arg("--scenario").arg(name).arg("--out").arg(&args.out);
    command.arg("--acs").arg(&args.acs).arg("--root").arg(&args.root).arg("--car").arg(&args.car);
    if let Some(steps) = args.steps {
        command.arg("--steps").arg(steps.to_string());
    }
    if hash_only {
        command.arg("--hash-only");
    }
    if !args.joint_forces {
        command.arg("--no-joint-forces");
    }
    let output = command.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "{name}: the run failed ({})\n{}{}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(name)?.split("hash=").nth(1).map(str::to_string))
        .ok_or_else(|| format!("{name}: no hash in the run's output:\n{stdout}"))
}

fn all(args: &Args) -> Result<(), String> {
    let repo = repo_root();
    let data = repo.join("cardata").join(&args.car);
    let mut table = String::from(
        "| scenario | steps | two runs identical | tyre forces = Rust tyre | all tyre values = Rust tyre | \
         tape closes | tape calls explained | tyre calls on tape | headline numbers |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    let mut notes = String::new();
    let mut failed = false;
    // the powertrain scenarios only run when they are named
    for scenario in scenario::all().into_iter().chain(scenario::powertrain()).chain(scenario::whole()) {
        if scenario.powertrain && args.only.is_empty() {
            continue;
        }
        if !args.only.is_empty() && !args.only.iter().any(|n| n == scenario.name) {
            continue;
        }
        let first = run_child(args, scenario.name, false)?;
        let second = run_child(args, scenario.name, true)?;
        let identical = first == second;
        let recording = Recording::read(&recording_path(&args.out, scenario.name))?;
        let report = check::check(&recording, &data)?;
        failed |= !identical
            || report.force_steps_matching != report.wheel_steps
            || report.full_steps_matching != report.wheel_steps
            || report.tape_steps_closed != recording.steps.len()
            || report.tape_calls_explained != report.tape_calls
            || report.tape_tyre_links != report.wheel_steps;
        let line = format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
            scenario.name,
            recording.steps.len(),
            if identical { format!("yes (`{first}`)") } else { format!("**NO** (`{first}` / `{second}`)") },
            check::percent(report.force_steps_matching, report.wheel_steps),
            check::percent(report.full_steps_matching, report.wheel_steps),
            check::percent(report.tape_steps_closed, recording.steps.len()),
            check::percent(report.tape_calls_explained, report.tape_calls),
            check::percent(report.tape_tyre_links, report.wheel_steps),
            report.headline.join("; "),
        );
        println!("{line}");
        for note in &report.notes {
            println!("    {note}");
            notes.push_str(&format!("- `{}`: {note}\n", scenario.name));
        }
        table.push_str(&line);
        table.push('\n');
    }
    if !notes.is_empty() {
        table.push_str("\nNotes:\n\n");
        table.push_str(&notes);
    }
    let path = args.out.join("results.md");
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if failed {
        return Err("at least one check of at least one scenario is below 100 %".into());
    }
    Ok(())
}

fn check_one(args: &Args) -> Result<(), String> {
    let file = args.file.as_ref().ok_or_else(usage)?;
    let recording = Recording::read(file)?;
    let car = recording.get("car").unwrap_or(game::DEFAULT_CAR);
    let report = check::check(&recording, &repo_root().join("cardata").join(car))?;
    println!(
        "{}: {} steps; tyre forces match in {} of wheel-steps, every tyre value in {}",
        recording.get("scenario").unwrap_or("?"),
        recording.steps.len(),
        check::percent(report.force_steps_matching, report.wheel_steps),
        check::percent(report.full_steps_matching, report.wheel_steps),
    );
    println!(
        "  force tape: {} of calls explain the accumulators bit for bit; nothing after the last call in {} of \
         steps; the tyre's hub calls equal the tape's in {} of wheel-steps",
        check::percent(report.tape_calls_explained, report.tape_calls),
        check::percent(report.tape_steps_closed, recording.steps.len()),
        check::percent(report.tape_tyre_links, report.wheel_steps),
    );
    for line in report.headline.iter().chain(&report.notes) {
        println!("  {line}");
    }
    Ok(())
}

/// What the session start does to a car's setup, printed by the game's own code.
fn setup_check(args: &Args) -> Result<(), String> {
    let repo = repo_root();
    game::prepare_root(&repo, &args.root, &args.car)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    acs.unbuffer_game_stdout();
    let scenarios = scenario::all();
    let options = game::Options { joint_feedback: false, car: args.car.clone(), setup_check: true };
    let world = game::World::build(&acs, &scenarios[0], &options);
    println!("{} values change: {}", world.setup_changes.len(), world.setup_changes.join(", "));
    Ok(())
}

/// Compares two recordings value by value (`--only` limits the fields by prefix; fields whose
/// name contains one of `--ignore`'s parts are skipped).
fn diff(args: &Args) -> Result<(), String> {
    let (a, b) = match (&args.file, &args.file2) {
        (Some(a), Some(b)) => (Recording::read(a)?, Recording::read(b)?),
        _ => return Err(usage()),
    };
    let report = record::diff(&a, &b, &args.only, &args.ignore);
    for line in &report {
        println!("{line}");
    }
    Ok(())
}

fn csv(args: &Args) -> Result<(), String> {
    let file = args.file.as_ref().ok_or_else(usage)?;
    let recording = Recording::read(file)?;
    let out = args.csv_out.clone().unwrap_or_else(|| file.with_extension(format!("{}.csv", args.table)));
    let options = CsvOptions {
        table: args.table.clone(),
        from: args.from,
        to: args.to,
        every: args.every,
        only: args.only.clone(),
    };
    let rows = record::to_csv(&recording, &options, &out)?;
    println!("{rows} rows -> {}", out.display());
    Ok(())
}
