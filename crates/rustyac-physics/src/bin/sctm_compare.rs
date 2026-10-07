// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Runs `VanillaSctm` over the inputs of an oracle CSV (AC's own `SCTM::solve` output) and
//! reports, per output field, how closely the port matches.
//!
//! sctm_compare --car <extracted car data dir> --axle front|rear --csv <file>
//!              [--compound <NAME or SHORT_NAME>]
//!              [--combined-factor <x>] [--dy-curve <lut>] [--dx-curve <lut>]
//!              [--dcamber-lut <lut>] [--dcamber-smooth]
//!
//! The optional overrides must be the ones the oracle was run with for that CSV.
//! Exit code: 0 if every field of every row is bit-identical, 1 otherwise, 2 on usage errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rustyac_physics::curve::Curve;
use rustyac_physics::data::tyres_ini::{load_sctm, Axle};
use rustyac_physics::math::{self, Backend};
use rustyac_physics::tyre::oracle_csv::{self, OracleRow, OUTPUT_FIELDS};
use rustyac_physics::tyre::TyreModel;

const USAGE: &str = "usage: sctm_compare --car <dir> --axle front|rear --csv <file> \
[--compound <name>] [--combined-factor <x>] [--dy-curve <lut>] [--dx-curve <lut>] \
[--dcamber-lut <lut>] [--dcamber-smooth]";

struct Args {
    car: PathBuf,
    axle: Axle,
    csv: PathBuf,
    compound: Option<String>,
    combined_factor: Option<f32>,
    dy_curve: Option<PathBuf>,
    dx_curve: Option<PathBuf>,
    dcamber_lut: Option<PathBuf>,
    dcamber_smooth: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut car = None;
    let mut axle = None;
    let mut csv = None;
    let mut a = Args {
        car: PathBuf::new(),
        axle: Axle::Front,
        csv: PathBuf::new(),
        compound: None,
        combined_factor: None,
        dy_curve: None,
        dx_curve: None,
        dcamber_lut: None,
        dcamber_smooth: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--car" => car = Some(PathBuf::from(value()?)),
            "--axle" => axle = Some(Axle::parse(&value()?)?),
            "--csv" => csv = Some(PathBuf::from(value()?)),
            "--compound" => a.compound = Some(value()?),
            "--combined-factor" => {
                let v = value()?;
                a.combined_factor = Some(v.parse().map_err(|_| format!("{v}: not a number"))?);
            }
            "--dy-curve" => a.dy_curve = Some(PathBuf::from(value()?)),
            "--dx-curve" => a.dx_curve = Some(PathBuf::from(value()?)),
            "--dcamber-lut" => a.dcamber_lut = Some(PathBuf::from(value()?)),
            "--dcamber-smooth" => a.dcamber_smooth = true,
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    a.car = car.ok_or("--car is required")?;
    a.axle = axle.ok_or("--axle is required")?;
    a.csv = csv.ok_or("--csv is required")?;
    Ok(a)
}

fn load_lut(path: &Path) -> Result<Curve, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Curve::from_lut_text(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Default)]
struct FieldStats {
    exact: usize,
    max_abs: f64,
    max_ulp: u32,
    /// (index into rows, got) of the row with the largest ULP error
    worst: Option<(usize, f32)>,
}

fn describe(row: &OracleRow) -> String {
    let i = &row.input;
    format!(
        "line {}: load {} slipAngleRAD {} slipRatio {} camberRAD {} speed {} u {} cpLength {} \
         grain {} blister {} pressureRatio {} useSimpleModel {}",
        row.line,
        i.load,
        i.slip_angle_rad,
        i.slip_ratio,
        i.camber_rad,
        i.speed,
        i.u,
        i.cp_length,
        i.grain,
        i.blister,
        i.pressure_ratio,
        i.use_simple_model as u8
    )
}

fn run() -> Result<bool, String> {
    let args = parse_args().map_err(|e| format!("{e}\n{USAGE}"))?;
    let compound = load_sctm(&args.car, args.axle, args.compound.as_deref())?;
    let mut sctm = compound.sctm;
    if let Some(v) = args.combined_factor {
        sctm.combined_factor = v;
    }
    if let Some(p) = &args.dy_curve {
        sctm.dy_load_curve = load_lut(p)?;
    }
    if let Some(p) = &args.dx_curve {
        sctm.dx_load_curve = load_lut(p)?;
    }
    if let Some(p) = &args.dcamber_lut {
        sctm.d_camber_curve = load_lut(p)?;
    }
    // an override only: the car's own DCAMBER_LUT_SMOOTH is kept otherwise
    if args.dcamber_smooth {
        sctm.use_smooth_d_camber_curve = true;
    }

    let text =
        std::fs::read_to_string(&args.csv).map_err(|e| format!("{}: {e}", args.csv.display()))?;
    let rows = oracle_csv::parse(&text).map_err(|e| format!("{}: {e}", args.csv.display()))?;
    if let Some(row) = rows.iter().find(|r| r.axle != args.axle.name()) {
        return Err(format!(
            "{} line {} is for the {} axle, not {}",
            args.csv.display(),
            row.line,
            row.axle,
            args.axle.name()
        ));
    }

    println!("csv      {}", args.csv.display());
    println!(
        "tyre     [{}] \"{}\", {} axle",
        compound.section,
        compound.name,
        args.axle.name()
    );
    println!(
        "maths    {}",
        match math::backend() {
            Backend::Msvcr120 => "MSVCR120.dll (the runtime acs.exe uses)",
            Backend::Std => "Rust std (not the runtime acs.exe uses)",
        }
    );
    for (name, value) in [
        ("lsMultY", sctm.ls_mult_y),
        ("lsExpY", sctm.ls_exp_y),
        ("lsMultX", sctm.ls_mult_x),
        ("lsExpX", sctm.ls_exp_x),
        ("Fz0", sctm.fz0),
        ("maxSlip0", sctm.max_slip0),
        ("maxSlip1", sctm.max_slip1),
        ("asy", sctm.asy),
        ("falloffSpeed", sctm.falloff_speed),
        ("speedSensitivity", sctm.speed_sensitivity),
        ("camberGain", sctm.camber_gain),
        ("dcamber0", sctm.dcamber0),
        ("dcamber1", sctm.dcamber1),
        ("cfXmult", sctm.cf_x_mult),
        ("pressureCfGain", sctm.pressure_cf_gain),
        ("brakeDXMod", sctm.brake_dx_mod),
        ("dCamberBlend", sctm.d_camber_blend),
        ("combinedFactor", sctm.combined_factor),
    ] {
        println!("  SCTM.{name} = {value} ({:#010x})", value.to_bits());
    }

    let mut stats: [FieldStats; 7] = Default::default();
    let mut exact_rows = 0;
    for (index, row) in rows.iter().enumerate() {
        let got = oracle_csv::fields(&sctm.solve(&row.input));
        let expected = oracle_csv::fields(&row.expected);
        let mut row_exact = true;
        for f in 0..7 {
            let ulp = oracle_csv::ulp_distance(got[f], expected[f]);
            let s = &mut stats[f];
            if ulp == 0 {
                s.exact += 1;
                continue;
            }
            row_exact = false;
            let abs = (got[f] as f64 - expected[f] as f64).abs();
            if abs > s.max_abs {
                s.max_abs = abs;
            }
            if ulp > s.max_ulp || s.worst.is_none() {
                s.max_ulp = ulp;
                s.worst = Some((index, got[f]));
            }
        }
        exact_rows += row_exact as usize;
    }

    println!(
        "\n{:<7} {:>8} {:>10} {:>9} {:>13} {:>8}",
        "field", "rows", "bit-exact", "%", "max abs err", "max ULP"
    );
    for (name, s) in OUTPUT_FIELDS.iter().zip(&stats) {
        let pct = if rows.is_empty() {
            100.0
        } else {
            100.0 * s.exact as f64 / rows.len() as f64
        };
        let ulp = if s.max_ulp == u32::MAX {
            "NaN".to_string()
        } else {
            s.max_ulp.to_string()
        };
        println!(
            "{:<7} {:>8} {:>10} {:>8.4}% {:>13e} {:>8}",
            name,
            rows.len(),
            s.exact,
            pct,
            s.max_abs,
            ulp
        );
    }
    for (f, (name, s)) in OUTPUT_FIELDS.iter().zip(&stats).enumerate() {
        if let Some((index, got)) = s.worst {
            let row = &rows[index];
            let expected = oracle_csv::fields(&row.expected)[f];
            println!(
                "worst {name}: AC {expected} ({:#010x}) vs port {got} ({:#010x})\n    {}",
                expected.to_bits(),
                got.to_bits(),
                describe(row)
            );
        }
    }
    let all = exact_rows == rows.len();
    println!(
        "\n{exact_rows} of {} rows bit-exact in every field: {}",
        rows.len(),
        if all { "MATCH" } else { "MISMATCH" }
    );
    Ok(all)
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
