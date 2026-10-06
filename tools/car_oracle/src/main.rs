//! Whole-car test oracle: Assetto Corsa's own `PhysicsEngine` and one `Car` (constructed,
//! loaded and stepped by the game's code, mapped from acs.exe; the game itself never starts)
//! on a fake flat track, driven by scripted controls, with every force handed to a rigid
//! body and the full body / joint / tyre state recorded each step.
//!
//! car_oracle list
//! car_oracle run --scenario <name> [--out <dir>] [--steps <n>] [--no-joint-forces] [--hash-only]
//!     One scenario in this process. Prints `<name> steps=… bytes=… hash=…`.
//! car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]
//!     Every scenario twice (fresh process each time), compares the two runs byte for byte,
//!     replays the tyres through the Rust port, and writes `<out>/results.md`.
//! car_oracle check <recording>
//!     Tyre cross-check and headline numbers of one recording.
//! car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>]
//!                [--every <n>] [--only <prefix,prefix>] [--csv-out <file>]
//!     Full-precision CSV of a recording.

mod acs;
mod check;
mod game;
mod record;
mod scenario;
mod sites;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use record::{CsvOptions, Recording, Writer};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

fn usage() -> String {
    "usage: car_oracle list\n       \
     car_oracle run --scenario <name> [--out <dir>] [--steps <n>] [--no-joint-forces] [--hash-only]\n       \
     car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]\n       \
     car_oracle check <recording>\n       \
     car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>] [--every <n>] \
     [--only <prefix,prefix>] [--csv-out <file>]\n       \
     common: [--acs <path to acs.exe>] [--root <scratch game folder>] [--verbose]"
        .to_string()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository folder")
}

struct Args {
    command: String,
    file: Option<PathBuf>,
    scenario: String,
    out: PathBuf,
    steps: Option<usize>,
    joint_forces: bool,
    hash_only: bool,
    only: Vec<String>,
    table: String,
    from: usize,
    to: usize,
    every: usize,
    csv_out: Option<PathBuf>,
    acs: PathBuf,
    root: PathBuf,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let repo = repo_root();
    let mut a = Args {
        command: it.next().ok_or_else(usage)?,
        file: None,
        scenario: String::new(),
        out: repo.join("oracle/car"),
        steps: None,
        joint_forces: true,
        hash_only: false,
        only: Vec::new(),
        table: "steps".into(),
        from: 0,
        to: usize::MAX,
        every: 1,
        csv_out: None,
        acs: PathBuf::from(DEFAULT_ACS),
        root: repo.join("re/scratch/car_oracle/root"),
        verbose: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let number = |text: String| text.parse::<usize>().map_err(|_| format!("{text}: not a number"));
        match flag.as_str() {
            "--scenario" => a.scenario = value()?,
            "--out" => a.out = PathBuf::from(value()?),
            "--steps" => a.steps = Some(number(value()?)?),
            "--no-joint-forces" => a.joint_forces = false,
            "--hash-only" => a.hash_only = true,
            "--only" => a.only = value()?.split(',').map(str::to_string).collect(),
            "--table" => a.table = value()?,
            "--from" => a.from = number(value()?)?,
            "--to" => a.to = number(value()?)?,
            "--every" => a.every = number(value()?)?,
            "--csv-out" => a.csv_out = Some(PathBuf::from(value()?)),
            "--acs" => a.acs = PathBuf::from(value()?),
            "--root" => a.root = PathBuf::from(value()?),
            "--verbose" => a.verbose = true,
            other if !other.starts_with("--") && a.file.is_none() => a.file = Some(PathBuf::from(other)),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    Ok(a)
}

fn main() {
    let result = parse_args().and_then(|args| match args.command.as_str() {
        "list" => {
            for s in scenario::all() {
                println!("{:24} {:6.1} s  {}", s.name, (s.steps - 1) as f32 * scenario::DT, s.about);
            }
            Ok(())
        }
        "run" => run(&args),
        "all" => all(&args),
        "check" => check_one(&args),
        "csv" => csv(&args),
        _ => Err(usage()),
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn recording_path(out: &Path, name: &str) -> PathBuf {
    out.join(format!("{name}.carrec"))
}

/// One scenario, in this process (the game's objects cannot be torn down and rebuilt).
fn run(args: &Args) -> Result<(), String> {
    let scenarios = scenario::all();
    let scenario = scenarios
        .iter()
        .find(|s| s.name == args.scenario)
        .ok_or_else(|| format!("no scenario {:?} (see `car_oracle list`)", args.scenario))?;
    let repo = repo_root();
    // relative paths are the caller's; the game needs its own working directory
    let out = std::path::absolute(&args.out).map_err(|e| e.to_string())?;
    game::prepare_root(&repo, &args.root)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;

    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let options = game::Options { joint_feedback: args.joint_forces };
    let mut world = game::World::build(&acs, scenario, &options);
    let steps = args.steps.unwrap_or(scenario.steps);
    let mut meta = vec![
        ("scenario".to_string(), scenario.name.to_string()),
        ("about".to_string(), scenario.about.to_string()),
        ("car".to_string(), game::CAR_NAME.to_string()),
        ("dt".to_string(), format!("{:?}", scenario::DT)),
        ("seed".to_string(), scenario.seed.to_string()),
        ("auto_clutch".to_string(), (scenario.auto_clutch as u8).to_string()),
        ("ground".to_string(), format!("{:?}", scenario.ground).replace(' ', "")),
        ("joint_forces".to_string(), (args.joint_forces as u8).to_string()),
        ("wheels".to_string(), game::WHEELS.join(",")),
    ];
    meta.extend(world.facts());
    let path = recording_path(&out, scenario.name);
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
    println!("{} steps={steps} bytes={bytes} hash={hash:016x}", scenario.name);
    Ok(())
}

/// Runs `car_oracle run` for one scenario in a child process and returns its hash.
fn run_child(args: &Args, name: &str, hash_only: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.arg("run").arg("--scenario").arg(name).arg("--out").arg(&args.out);
    command.arg("--acs").arg(&args.acs).arg("--root").arg(&args.root);
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
    let data = repo.join("cardata").join(game::CAR_NAME);
    let mut table = String::from(
        "| scenario | steps | two runs identical | tyre forces match | all tyre values match | headline numbers |\n\
         |---|---|---|---|---|---|\n",
    );
    let mut failed = false;
    for scenario in scenario::all() {
        if !args.only.is_empty() && !args.only.iter().any(|n| n == scenario.name) {
            continue;
        }
        let first = run_child(args, scenario.name, false)?;
        let second = run_child(args, scenario.name, true)?;
        let identical = first == second;
        let recording = Recording::read(&recording_path(&args.out, scenario.name))?;
        let report = check::check(&recording, &data)?;
        failed |= !identical || report.force_steps_matching != report.wheel_steps;
        let line = format!(
            "| `{}` | {} | {} | {} | {} | {} |",
            scenario.name,
            recording.steps.len(),
            if identical { format!("yes (`{first}`)") } else { format!("**NO** (`{first}` / `{second}`)") },
            check::percent(report.force_steps_matching, report.wheel_steps),
            check::percent(report.full_steps_matching, report.wheel_steps),
            report.headline.join("; "),
        );
        println!("{line}");
        for note in &report.notes {
            println!("    {note}");
        }
        table.push_str(&line);
        table.push('\n');
    }
    let path = args.out.join("results.md");
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if failed {
        return Err("at least one scenario is not deterministic or does not match the Rust tyre".into());
    }
    Ok(())
}

fn check_one(args: &Args) -> Result<(), String> {
    let file = args.file.as_ref().ok_or_else(usage)?;
    let recording = Recording::read(file)?;
    let report = check::check(&recording, &repo_root().join("cardata").join(game::CAR_NAME))?;
    println!(
        "{}: {} steps; tyre forces match in {} of wheel-steps, every tyre value in {}",
        recording.get("scenario").unwrap_or("?"),
        recording.steps.len(),
        check::percent(report.force_steps_matching, report.wheel_steps),
        check::percent(report.full_steps_matching, report.wheel_steps),
    );
    for line in report.headline.iter().chain(&report.notes) {
        println!("  {line}");
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
