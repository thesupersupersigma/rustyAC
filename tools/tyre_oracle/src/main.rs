//! Tyre test oracle: Assetto Corsa's own `Tyre` (constructed, loaded and stepped by the
//! game's code, mapped from acs.exe) on a single-wheel rig.
//!
//! tyre_oracle coverage --car <dir> [--car <dir> ...] [--n <rows>] [--seed <seed>] [--csv-out <dir>]
//!     For every tyres.ini: let the game load it, compare every loaded value with the Rust
//!     loader, then compare `SCTM::solve` with `VanillaSctm` on random inputs.
//! tyre_oracle run --car <dir> [--axle front|rear|both] [--scenario <name>|all] [--out <dir>] [--check]
//!     Record scripted scenarios of `Tyre::step` for `tyre_compare`.

mod acs;
mod game;
mod params;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rustyac_physics::data::tyres_ini::{init_compounds, Axle};
use rustyac_physics::tyre::rig::scenarios::{self, Wheel, SCENARIOS};
use rustyac_physics::tyre::rig::{self, Recording, Rig, StepInput};
use rustyac_physics::tyre::{TyreModel, VanillaSctm};

use game::{Game, SctmInput};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

fn usage() -> String {
    "usage: tyre_oracle coverage --car <dir> [--car <dir> ...] [--n <rows per compound>] [--seed <seed>] \
     [--csv-out <dir>]\n       \
     tyre_oracle run --car <dir> [--axle front|rear|both] [--scenario <name>|all] [--out <dir>] [--check]\n       \
     common: [--acs <path to acs.exe>] [--verbose]   (--verbose keeps the game's own console output)"
        .to_string()
}

struct Args {
    command: String,
    cars: Vec<PathBuf>,
    acs: PathBuf,
    verbose: bool,
    n: usize,
    seed: u64,
    csv_out: Option<PathBuf>,
    axle: String,
    scenario: String,
    out: PathBuf,
    check: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let mut a = Args {
        command: it.next().ok_or_else(usage)?,
        cars: Vec::new(),
        acs: PathBuf::from(DEFAULT_ACS),
        verbose: false,
        n: 2000,
        seed: 1,
        csv_out: None,
        axle: "both".into(),
        scenario: "all".into(),
        out: PathBuf::from("oracle/tyre"),
        check: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .ok_or_else(|| format!("{flag} needs a value\n{}", usage()))
        };
        match flag.as_str() {
            "--car" => a.cars.push(PathBuf::from(value()?)),
            "--acs" => a.acs = PathBuf::from(value()?),
            "--verbose" => a.verbose = true,
            "--n" => {
                a.n = value()?
                    .parse()
                    .map_err(|_| "--n: not a row count".to_string())?
            }
            "--seed" => {
                a.seed = value()?
                    .parse()
                    .map_err(|_| "--seed: not a number".to_string())?
            }
            "--csv-out" => a.csv_out = Some(PathBuf::from(value()?)),
            "--axle" => a.axle = value()?.to_ascii_lowercase(),
            "--scenario" => a.scenario = value()?,
            "--out" => a.out = PathBuf::from(value()?),
            "--check" => a.check = true,
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    if a.cars.is_empty() {
        return Err(usage());
    }
    Ok(a)
}

/// The data path as the game wants it: forward slashes and a trailing one.
fn game_path(dir: &Path) -> String {
    let mut text = dir.to_string_lossy().replace('\\', "/");
    if !text.ends_with('/') {
        text.push('/');
    }
    text
}

fn car_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A resting rig, for constructing tyres.
fn idle_input(has_car: bool) -> StepInput {
    let mut input = scenarios::scenario("warmup", Axle::Front, &Wheel::default()).unwrap()[0];
    input.has_car = has_car;
    input
}

/// splitmix64: small, well mixed, and the same on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f32 {
        (lo + (hi - lo) * self.unit()) as f32
    }
}

/// The random `SCTM::solve` inputs of `sctm_oracle --sweep random` (same mix of normal
/// ranges and the special values the code branches on).
fn random_sctm_input(rng: &mut Rng, tyre_index: i32) -> SctmInput {
    let load = match rng.unit() {
        p if p < 0.90 => rng.range(50.0, 12000.0),
        p if p < 0.95 => rng.range(0.001, 50.0),
        p if p < 0.98 => 0.0,
        _ => rng.range(-1000.0, 0.0),
    };
    let slip_angle_rad = match rng.unit() {
        p if p < 0.70 => rng.range(-0.35, 0.35),
        p if p < 0.90 => rng.range(-1.5, 1.5),
        p if p < 0.95 => 0.0,
        _ => rng.range(-1e-4, 1e-4),
    };
    let slip_ratio = match rng.unit() {
        p if p < 0.60 => rng.range(-0.4, 0.4),
        p if p < 0.80 => rng.range(-1.2, 3.0),
        p if p < 0.90 => 0.0,
        p if p < 0.95 => rng.range(-1.0, -0.999),
        _ => rng.range(-1e-4, 1e-4),
    };
    let camber_rad = match rng.unit() {
        p if p < 0.70 => rng.range(-0.1, 0.1),
        p if p < 0.85 => 0.0,
        _ => rng.range(-0.5, 0.5),
    };
    let speed = match rng.unit() {
        p if p < 0.80 => rng.range(0.0, 100.0),
        p if p < 0.90 => rng.range(0.0, 1.0),
        p if p < 0.95 => 0.0,
        _ => rng.range(-5.0, 0.0),
    };
    let u = if rng.unit() < 0.85 {
        rng.range(0.3, 1.6)
    } else {
        1.0
    };
    let cp_length = rng.range(0.0, 0.3);
    let grain = if rng.unit() < 0.5 {
        0.0
    } else {
        rng.range(0.0, 100.0)
    };
    let blister = match rng.unit() {
        p if p < 0.50 => 0.0,
        p if p < 0.90 => rng.range(0.0, 100.0),
        _ => rng.range(-20.0, 150.0),
    };
    let pressure_ratio = if rng.unit() < 0.3 {
        0.0
    } else {
        rng.range(-0.8, 0.6)
    };
    SctmInput {
        load,
        slip_angle_rad,
        slip_ratio,
        camber_rad,
        speed,
        u,
        tyre_index,
        cp_length,
        grain,
        blister,
        pressure_ratio,
        use_simple_model: rng.unit() < 0.1,
    }
}

const SCTM_CSV_HEADER: &str = "compound,axle,load,slip_angle_deg,slip_angle_rad,slip_ratio,camber_deg,camber_rad,\
speed,u,tyre_index,cp_length,grain,blister,pressure_ratio,use_simple_model,Fy,Fx,Mz,trail,ndSlip,Dy,Dx";

struct CarCoverage {
    car: String,
    note: String,
    compounds: usize,
    curve_compounds: usize,
    values: usize,
    value_differences: usize,
    rows: usize,
    exact_rows: usize,
}

fn coverage_one(game: &Game, dir: &Path, args: &Args) -> CarCoverage {
    let mut result = CarCoverage {
        car: car_name(dir),
        note: String::new(),
        compounds: 0,
        curve_compounds: 0,
        values: 0,
        value_differences: 0,
        rows: 0,
        exact_rows: 0,
    };
    match init_compounds(dir, 0) {
        Err(e) => {
            result.note = format!("not loaded: {e}");
            return result;
        }
        Ok(tyres) if tyres.version < 10 => {
            result.note = format!("skipped: VERSION={}", tyres.version);
            return result;
        }
        Ok(_) => {}
    }
    let first = idle_input(false);
    game.set_world(&first);
    for axle in [Axle::Front, Axle::Rear] {
        let tyre = game.new_tyre(&game_path(dir), axle.tyre_index());
        let port = match Rig::new(dir, axle, 0, &first) {
            Ok(rig) => rig.tyre,
            Err(e) => {
                result.note = format!("{}: {e}", axle.name());
                return result;
            }
        };
        let comparison = params::compare(&tyre, &port);
        result.values += comparison.compared;
        result.value_differences += comparison.differences.len();
        for difference in comparison.differences.iter().take(8) {
            eprintln!("  {} {}: {difference}", result.car, axle.name());
        }

        for (k, def) in port
            .compound_defs
            .iter()
            .enumerate()
            .take(tyre.compound_count())
        {
            result.compounds += 1;
            let model = &def.model_data;
            if model.dy_load_curve.get_count() > 0
                || model.dx_load_curve.get_count() > 0
                || model.d_camber_curve.get_count() > 0
            {
                result.curve_compounds += 1;
            }
            tyre.set_compound(k as i32);
            let mut sctm = VanillaSctm::default();
            def.mirror_into_sctm(&mut sctm);
            let mut rng = Rng(args
                .seed
                .wrapping_add(k as u64 * 1000 + axle.tyre_index() as u64));
            let mut csv = String::from(SCTM_CSV_HEADER);
            csv.push('\n');
            let mut reported = false;
            for _ in 0..args.n {
                let input = random_sctm_input(&mut rng, axle.tyre_index());
                let ac = tyre.sctm_solve(&input);
                let ours = sctm.solve(&input.to_port());
                let exact = ac.bits() == game::port_bits(&ours);
                result.rows += 1;
                result.exact_rows += exact as usize;
                if !exact && !reported {
                    reported = true;
                    eprintln!("  {} {} compound {k}: first SCTM mismatch at {input:?}: AC {ac:?}, port {ours:?}",
                        result.car, axle.name());
                }
                if args.csv_out.is_some() {
                    let _ = writeln!(
                        csv,
                        "{},{},{:?},{:?},{:?},{:?},{:?},{:?},{:?},{:?},{},{:?},{:?},{:?},{:?},{},{:?},{:?},{:?},{:?},{:?},{:?},{:?}",
                        def.section, axle.name(), input.load, (input.slip_angle_rad as f64).to_degrees(),
                        input.slip_angle_rad, input.slip_ratio, (input.camber_rad as f64).to_degrees(),
                        input.camber_rad, input.speed, input.u, input.tyre_index, input.cp_length, input.grain,
                        input.blister, input.pressure_ratio, input.use_simple_model as u8,
                        ac.fy, ac.fx, ac.mz, ac.trail, ac.nd_slip, ac.dy, ac.dx
                    );
                }
            }
            if let Some(out) = &args.csv_out {
                let file = out.join(format!(
                    "{}_{}_{}.csv",
                    result.car,
                    axle.name(),
                    def.section
                ));
                if let Err(e) =
                    std::fs::create_dir_all(out).and_then(|_| std::fs::write(&file, csv))
                {
                    eprintln!("  {}: {e}", file.display());
                }
            }
        }
    }
    result
}

fn coverage(game: &Game, args: &Args) -> Result<bool, String> {
    println!("| Car | Compounds (front + rear) | ... with lookup curves | Loaded values compared | ... different | SCTM rows | Bit-exact | % |");
    println!("|---|---|---|---|---|---|---|---|");
    let mut total = [0usize; 6];
    let mut cars = 0;
    let mut skipped = Vec::new();
    for dir in &args.cars {
        let r = coverage_one(game, dir, args);
        if !r.note.is_empty() {
            skipped.push(format!("{}: {}", r.car, r.note));
            continue;
        }
        cars += 1;
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.4} |",
            r.car,
            r.compounds,
            r.curve_compounds,
            r.values,
            r.value_differences,
            r.rows,
            r.exact_rows,
            100.0 * r.exact_rows as f64 / r.rows.max(1) as f64
        );
        for (sum, value) in total.iter_mut().zip([
            r.compounds,
            r.curve_compounds,
            r.values,
            r.value_differences,
            r.rows,
            r.exact_rows,
        ]) {
            *sum += value;
        }
    }
    println!(
        "| **Total: {cars} cars** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** | **{:.4}** |",
        total[0], total[1], total[2], total[3], total[4], total[5],
        100.0 * total[5] as f64 / total[4].max(1) as f64
    );
    for line in &skipped {
        println!("{line}");
    }
    Ok(total[3] == 0 && total[4] == total[5])
}

fn run(game: &Game, args: &Args) -> Result<bool, String> {
    let dir = &args.cars[0];
    let axles: Vec<Axle> = match args.axle.as_str() {
        "both" => vec![Axle::Front, Axle::Rear],
        other => vec![Axle::parse(other)?],
    };
    let names: Vec<&str> = match args.scenario.as_str() {
        "all" => SCENARIOS.to_vec(),
        one => vec![SCENARIOS
            .iter()
            .copied()
            .find(|s| *s == one)
            .ok_or_else(|| format!("unknown scenario {one}; known: {}", SCENARIOS.join(", ")))?],
    };
    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let mut all_exact = true;
    for axle in axles {
        let tyres = init_compounds(dir, axle.tyre_index())?;
        let def = tyres
            .compound_defs
            .first()
            .ok_or("tyres.ini has no compound")?;
        let wheel = Wheel {
            radius: def.data.radius,
            rim_radius: def.data.rim_radius,
            rate: def.data.k,
            static_load: if axle == Axle::Front { 2800.0 } else { 3400.0 },
        };
        for name in &names {
            let inputs = scenarios::scenario(name, axle, &wheel).expect("known scenario");
            let compound = scenarios::compound_for(name).min(tyres.compound_defs.len() as i32 - 1);
            game.set_world(&inputs[0]);
            let tyre = game.new_tyre(&game_path(dir), axle.tyre_index());
            tyre.set_car(inputs[0].has_car);
            if !tyre.set_compound(compound) {
                return Err(format!("the game refused compound {compound}"));
            }
            let outputs: Vec<Vec<u64>> = inputs.iter().map(|input| tyre.step(input)).collect();
            let recording = Recording {
                header: vec![
                    ("scenario".into(), name.to_string()),
                    ("axle".into(), axle.name().into()),
                    ("car".into(), car_name(dir)),
                    ("compound".into(), compound.to_string()),
                    ("steps".into(), inputs.len().to_string()),
                    ("dt".into(), "0.003".into()),
                ],
                inputs,
                outputs,
            };
            let file = args
                .out
                .join(format!("{}_{}_{name}.csv", car_name(dir), axle.name()));
            std::fs::write(&file, recording.to_text())
                .map_err(|e| format!("{}: {e}", file.display()))?;
            print!("{}: {} steps", file.display(), recording.inputs.len());
            if args.check {
                let replay = rig::replay(&recording, dir)?;
                print!(
                    ", {} bit-exact, {} the same with NaN = NaN",
                    replay.exact_steps, replay.matching_steps
                );
                if let Some(d) = &replay.first_divergence {
                    all_exact = false;
                    print!(
                        "; first divergence at step {} in {}: AC {}, port {}",
                        d.step,
                        d.field,
                        rig::describe_word(&d.field, d.expected),
                        rig::describe_word(&d.field, d.got)
                    );
                }
            }
            println!();
        }
    }
    Ok(all_exact)
}

fn main() -> std::process::ExitCode {
    let result = parse_args().and_then(|args| {
        let acs = acs::Acs::load(&args.acs)?;
        if !args.verbose {
            acs.silence_game_stdout();
        }
        let game = Game::new(&acs);
        match args.command.as_str() {
            "coverage" => coverage(&game, &args),
            "run" => run(&game, &args),
            other => Err(format!("unknown command {other}\n{}", usage())),
        }
    });
    match result {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::from(2)
        }
    }
}
