// SPDX-License-Identifier: MIT OR Apache-2.0

//! SCTM test oracle: runs Assetto Corsa's own `SCTM::solve` over slip sweeps and writes CSVs.
//!
//! sctm_oracle --car <extracted car data dir> [--compound <name>] [--axle front|rear]
//!             [--sweep lateral|longitudinal|camber|combined|all|random] [--out <dir>]
//!             [--n <rows>] [--seed <seed>]                       (random sweep)
//!             [--combined-factor <x>] [--dy-curve <lut>] [--dx-curve <lut>]
//!             [--dcamber-lut <lut>] [--dcamber-smooth] [--tag <name>]

mod acs;
mod ini;
mod sctm;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sctm::{Overrides, Sctm, TyreModelInput, TyreModelOutput};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";
const DEFAULT_OUT: &str = "oracle/sctm";
const SWEEPS: [&str; 4] = ["lateral", "longitudinal", "camber", "combined"];

const LOADS: [f32; 8] = [1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0, 8000.0];
const COMBINED_LOADS: [f32; 3] = [2000.0, 4000.0, 6000.0];
const CAMBERS_DEG: [f64; 3] = [0.0, -2.0, -4.0];

struct Args {
    car: PathBuf,
    compound: Option<String>,
    axle: String,
    sweep: String,
    out: PathBuf,
    acs: PathBuf,
    speed: f32,
    u: f32,
    pressure_ratio: f32,
    n: usize,
    seed: u64,
    tag: Option<String>,
    overrides: Overrides,
}

fn usage() -> String {
    "usage: sctm_oracle --car <dir> [--compound <name>] [--axle front|rear] \
     [--sweep lateral|longitudinal|camber|combined|all|random] [--out <dir>]\n       \
     optional: [--acs <path to acs.exe>] [--speed <m/s>] [--u <grip multiplier>] \
     [--pressure-ratio <pressure/ideal - 1>]\n       \
     random sweep: [--n <rows>] [--seed <seed>]\n       \
     overrides: [--combined-factor <x>] [--dy-curve <lut>] [--dx-curve <lut>] \
     [--dcamber-lut <lut>] [--dcamber-smooth] [--tag <file name suffix>]"
        .to_string()
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        car: PathBuf::new(),
        compound: None,
        axle: "front".into(),
        sweep: "all".into(),
        out: PathBuf::from(DEFAULT_OUT),
        acs: PathBuf::from(DEFAULT_ACS),
        speed: 20.0, // TyreTester's default velocity
        u: 1.0,
        pressure_ratio: 0.0,
        n: 100_000,
        seed: 1,
        tag: None,
        overrides: Overrides::default(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let number = |v: String| v.parse::<f32>().map_err(|_| format!("{v}: not a number"));
        match flag.as_str() {
            "--car" => a.car = PathBuf::from(value()?),
            "--compound" => a.compound = Some(value()?),
            "--axle" => a.axle = value()?.to_ascii_lowercase(),
            "--sweep" => a.sweep = value()?.to_ascii_lowercase(),
            "--out" => a.out = PathBuf::from(value()?),
            "--acs" => a.acs = PathBuf::from(value()?),
            "--speed" => a.speed = number(value()?)?,
            "--u" => a.u = number(value()?)?,
            "--pressure-ratio" => a.pressure_ratio = number(value()?)?,
            "--n" => {
                let v = value()?;
                a.n = v.parse().map_err(|_| format!("{v}: not a row count"))?;
            }
            "--seed" => {
                let v = value()?;
                a.seed = v.parse().map_err(|_| format!("{v}: not a seed"))?;
            }
            "--tag" => a.tag = Some(value()?),
            "--combined-factor" => a.overrides.combined_factor = Some(number(value()?)?),
            "--dy-curve" => a.overrides.dy_curve = Some(load_lut(&value()?)?),
            "--dx-curve" => a.overrides.dx_curve = Some(load_lut(&value()?)?),
            "--dcamber-lut" => a.overrides.dcamber_lut = Some(load_lut(&value()?)?),
            "--dcamber-smooth" => a.overrides.dcamber_smooth = true,
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    if a.car.as_os_str().is_empty() {
        return Err(usage());
    }
    if a.axle != "front" && a.axle != "rear" {
        return Err(format!("--axle must be front or rear\n{}", usage()));
    }
    if a.sweep != "all" && a.sweep != "random" && !SWEEPS.contains(&a.sweep.as_str()) {
        return Err(format!("unknown sweep {}\n{}", a.sweep, usage()));
    }
    Ok(a)
}

/// A lookup table file: one `x|y` pair per line, `;` starts a comment.
fn load_lut(path: &str) -> Result<Vec<(f32, f32)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut pairs = Vec::new();
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let pair = line.split_once('|').and_then(|(x, y)| {
            Some((x.trim().parse::<f64>().ok()? as f32, y.trim().parse::<f64>().ok()? as f32))
        });
        pairs.push(pair.ok_or_else(|| format!("{path}: bad lut line {raw:?}"))?);
    }
    Ok(pairs)
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

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f32 {
        (lo + (hi - lo) * self.unit()) as f32
    }
}

/// Compound sections are FRONT / FRONT_1 / FRONT_2 ... (same for REAR), as in Tyre::initCompounds.
fn find_compound<'a>(
    ini: &'a ini::Ini,
    axle: &str,
    wanted: Option<&str>,
) -> Result<&'a ini::Section, String> {
    let prefix = axle.to_ascii_uppercase();
    let mut names = Vec::new();
    for n in 0.. {
        let name = if n == 0 { prefix.clone() } else { format!("{prefix}_{n}") };
        let Some(sec) = ini.section(&name) else { break };
        let full = sec.text("NAME").unwrap_or("");
        let short = sec.text("SHORT_NAME").unwrap_or("");
        match wanted {
            None => return Ok(sec),
            Some(w) if w.eq_ignore_ascii_case(full) || w.eq_ignore_ascii_case(short) => return Ok(sec),
            _ => names.push(format!("\"{full}\" ({short})")),
        }
    }
    Err(format!(
        "compound {:?} not found for the {axle} axle; available: {}",
        wanted.unwrap_or(""),
        names.join(", ")
    ))
}

/// Everything about one run that is the same for every row.
struct Bench<'a> {
    sctm: &'a Sctm,
    compound: &'a str,
    axle: &'a str,
    radius: f32,
    rate: f32,
    speed: f32,
    u: f32,
    pressure_ratio: f32,
    tyre_index: i32,
}

struct Row {
    slip_angle_deg: f64,
    camber_deg: f64,
    input: TyreModelInput,
    output: TyreModelOutput,
}

impl Bench<'_> {
    /// ksCalcContactPatchLength(radius, depth) with depth = load / spring rate (no damping).
    fn cp_length(&self, load: f32) -> f32 {
        let inner = self.radius - load / self.rate;
        if inner > 0.0 && inner < self.radius {
            (self.radius * self.radius - inner * inner).sqrt() * 2.0
        } else {
            0.0
        }
    }

    fn run(&self, load: f32, slip_angle_deg: f64, slip_ratio: f64, camber_deg: f64) -> Row {
        let input = TyreModelInput {
            load,
            slip_angle_rad: slip_angle_deg.to_radians() as f32,
            slip_ratio: slip_ratio as f32,
            camber_rad: camber_deg.to_radians() as f32,
            speed: self.speed,
            u: self.u,
            tyre_index: self.tyre_index,
            cp_length: self.cp_length(load),
            grain: 0.0,
            blister: 0.0,
            pressure_ratio: self.pressure_ratio,
            use_simple_model: false,
        };
        Row { slip_angle_deg, camber_deg, input, output: self.sctm.solve(&input) }
    }

    /// Random inputs over the whole domain solve() can be handed, not just the tidy operating
    /// point of the grid sweeps. Each input mixes a wide "normal driving" range with the
    /// special values its branches test for (exact zeros, the -0.99999 slip-ratio limit,
    /// speeds below 1 m/s, out-of-range blister values, ...).
    fn random(&self, n: usize, seed: u64) -> Vec<Row> {
        let mut rng = Rng(seed);
        let mut rows = Vec::with_capacity(n);
        for _ in 0..n {
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
            let u = if rng.unit() < 0.85 { rng.range(0.3, 1.6) } else { 1.0 };
            let cp_length = rng.range(0.0, 0.3);
            let grain = if rng.unit() < 0.5 { 0.0 } else { rng.range(0.0, 100.0) };
            let blister = match rng.unit() {
                p if p < 0.50 => 0.0,
                p if p < 0.90 => rng.range(0.0, 100.0),
                _ => rng.range(-20.0, 150.0),
            };
            let pressure_ratio = if rng.unit() < 0.3 { 0.0 } else { rng.range(-0.8, 0.6) };
            let input = TyreModelInput {
                load,
                slip_angle_rad,
                slip_ratio,
                camber_rad,
                speed,
                u,
                tyre_index: self.tyre_index,
                cp_length,
                grain,
                blister,
                pressure_ratio,
                use_simple_model: rng.unit() < 0.1,
            };
            rows.push(Row {
                slip_angle_deg: (slip_angle_rad as f64).to_degrees(),
                camber_deg: (camber_rad as f64).to_degrees(),
                input,
                output: self.sctm.solve(&input),
            });
        }
        rows
    }

    fn lateral(&self, camber_deg: f64) -> Vec<Row> {
        let mut rows = Vec::new();
        for &load in &LOADS {
            for i in 0..=120 {
                rows.push(self.run(load, -15.0 + 0.25 * i as f64, 0.0, camber_deg));
            }
        }
        rows
    }

    fn longitudinal(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for &load in &LOADS {
            for i in -30..=30 {
                rows.push(self.run(load, 0.0, i as f64 / 100.0, 0.0));
            }
        }
        rows
    }

    fn camber(&self) -> Vec<Row> {
        CAMBERS_DEG.iter().flat_map(|&c| self.lateral(c)).collect()
    }

    /// Coarse grid: slip angle -12..12 deg step 2 x slip ratio -0.30..0.30 step 0.05.
    fn combined(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for &load in &COMBINED_LOADS {
            for a in -6..=6 {
                for r in -6..=6 {
                    rows.push(self.run(load, 2.0 * a as f64, 0.05 * r as f64, 0.0));
                }
            }
        }
        rows
    }

    fn csv(&self, rows: &[Row]) -> String {
        let mut s = String::from(
            "compound,axle,load,slip_angle_deg,slip_angle_rad,slip_ratio,camber_deg,camber_rad,\
             speed,u,tyre_index,cp_length,grain,blister,pressure_ratio,use_simple_model,\
             Fy,Fx,Mz,trail,ndSlip,Dy,Dx\n",
        );
        for r in rows {
            let (i, o) = (&r.input, &r.output);
            writeln!(
                s,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                self.compound, self.axle, i.load, r.slip_angle_deg, i.slip_angle_rad, i.slip_ratio,
                r.camber_deg, i.camber_rad, i.speed, i.u, i.tyre_index, i.cp_length, i.grain,
                i.blister, i.pressure_ratio, i.use_simple_model as u8,
                o.fy, o.fx, o.mz, o.trail, o.nd_slip, o.dy, o.dx
            )
            .unwrap();
        }
        s
    }
}

/// Row with the largest value of `key` among rows at `load`.
fn peak<'a>(rows: &'a [Row], load: f32, key: impl Fn(&Row) -> f32) -> &'a Row {
    rows.iter()
        .filter(|r| r.input.load == load)
        .max_by(|a, b| key(a).total_cmp(&key(b)))
        .unwrap()
}

fn print_summary(lateral: &[Row], longitudinal: &[Row]) {
    println!("\nload_N  peakFy_N  Fy/load  at_SA_deg | peakFx_N  Fx/load  at_SR | brakeFx_N  Fx/load  at_SR");
    for &load in &LOADS {
        let fy = peak(lateral, load, |r| r.output.fy);
        let drive = peak(longitudinal, load, |r| r.output.fx);
        let brake = peak(longitudinal, load, |r| -r.output.fx);
        println!(
            "{:6}  {:8.1}  {:7.4}  {:9.2} | {:8.1}  {:7.4}  {:5.2} | {:9.1}  {:7.4}  {:5.2}",
            load, fy.output.fy, fy.output.fy / load, fy.slip_angle_deg,
            drive.output.fx, drive.output.fx / load, drive.input.slip_ratio,
            brake.output.fx, brake.output.fx / load, brake.input.slip_ratio
        );
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let car_name = args
        .car
        .canonicalize()
        .map_err(|e| format!("{}: {e}", args.car.display()))?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("--car has no folder name")?;

    let ini = ini::Ini::load(&args.car.join("tyres.ini"))?;
    let version = ini.section("HEADER").ok_or("tyres.ini has no [HEADER]")?.int("VERSION")?;
    let sec = find_compound(&ini, &args.axle, args.compound.as_deref())?;
    let compound = sec.text("NAME").unwrap_or(&sec.name).to_string();

    let acs = acs::Acs::load(&args.acs)?;
    let (sctm, params) = Sctm::new(&acs, sec, version, &args.overrides)?;

    println!("acs.exe mapped at {:#x} (entry point not run)", acs.base());
    println!("car {car_name}, tyres.ini VERSION {version}, [{}] \"{compound}\"", sec.name);
    for (name, value) in &params.fields {
        println!("  SCTM.{name} = {value}");
    }
    let o = &args.overrides;
    for (name, curve) in
        [("dyLoadCurve", &o.dy_curve), ("dxLoadCurve", &o.dx_curve), ("dCamberCurve", &o.dcamber_lut)]
    {
        if let Some(pairs) = curve {
            println!("  SCTM.{name} = {} points (override)", pairs.len());
        }
    }
    if o.dcamber_smooth {
        println!("  SCTM.useSmoothDCamberCurve = true (override)");
    }
    println!(
        "fixed inputs: speed {} m/s, u {}, pressureRatio {}, grain 0, blister 0, useSimpleModel 0",
        args.speed, args.u, args.pressure_ratio
    );

    let bench = Bench {
        sctm: &sctm,
        compound: &compound,
        axle: &args.axle,
        radius: sec.float("RADIUS")?,
        rate: match sec.float("RATE")? {
            r if r == 0.0 => 220000.0,
            r => r,
        },
        speed: args.speed,
        u: args.u,
        pressure_ratio: args.pressure_ratio,
        // wheel order in the game is FL, FR, RL, RR
        tyre_index: if args.axle == "front" { 0 } else { 2 },
    };

    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let write = |sweep: &str, rows: &[Row]| -> Result<(), String> {
        let tag = args.tag.as_ref().map(|t| format!("_{t}")).unwrap_or_default();
        let path: PathBuf =
            Path::new(&args.out).join(format!("{car_name}_{}_{sweep}{tag}.csv", args.axle));
        std::fs::write(&path, bench.csv(rows)).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{} rows -> {}", rows.len(), path.display());
        Ok(())
    };

    if args.sweep == "random" {
        println!("random sweep: {} rows, seed {}", args.n, args.seed);
        return write("random", &bench.random(args.n, args.seed));
    }
    let wanted = |s: &str| args.sweep == "all" || args.sweep == s;
    let lateral = bench.lateral(0.0);
    let longitudinal = bench.longitudinal();
    if wanted("lateral") {
        write("lateral", &lateral)?;
    }
    if wanted("longitudinal") {
        write("longitudinal", &longitudinal)?;
    }
    if wanted("camber") {
        write("camber", &bench.camber())?;
    }
    if wanted("combined") {
        write("combined", &bench.combined())?;
    }
    print_summary(&lateral, &longitudinal);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
