// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! `VanillaSctm` against rows of AC's own `SCTM::solve` output (sampled from the oracle CSVs
//! by `tools/make_sctm_golden.py`). Every output field of every row must match bit for bit.

use std::path::Path;

use rustyac_physics::curve::Curve;
use rustyac_physics::data::tyres_ini::{load_sctm, Axle};
use rustyac_physics::math::{self, Backend};
use rustyac_physics::tyre::oracle_csv::{self, OUTPUT_FIELDS};
use rustyac_physics::tyre::{TyreModel, VanillaSctm};

const GOLDEN: &str = include_str!("golden/sctm_f2004.csv");
const DY_CURVE: &str = include_str!("golden/synthetic_dy_curve.lut");
const DX_CURVE: &str = include_str!("golden/synthetic_dx_curve.lut");
const DCAMBER_LUT: &str = include_str!("golden/synthetic_dcamber.lut");

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// The F2004 "Slick Soft" SCTM parameters as the game computes them from tyres.ini, as bit
/// patterns, so the test does not need the (not checked in) car data.
fn f2004_soft(axle: &str) -> VanillaSctm {
    let common = VanillaSctm {
        asy: f(0x3f666666),              // 0.9
        falloff_speed: f(0x40e00000),    // 7
        dcamber0: f(0x3fb33333),         // 1.4
        dcamber1: f(0xc1500000),         // -13
        cf_x_mult: f(0x3f800000),        // 1
        pressure_cf_gain: f(0x3e99999a), // 0.3
        brake_dx_mod: f(0x3f851eb8),     // 1.04
        combined_factor: 0.0,
        ..VanillaSctm::default()
    };
    match axle {
        "front" => VanillaSctm {
            ls_mult_y: f(0x4113b3cf),         // 9.231399
            ls_exp_y: f(0x3f4ccccd),          // 0.8
            ls_mult_x: f(0x4113b3cf),         // 9.231399
            ls_exp_x: f(0x3f4ccccd),          // 0.8
            fz0: f(0x4557b000),               // 3451
            max_slip0: f(0x3de218f5),         // 0.110399164
            max_slip1: f(0x3de8b5b7),         // 0.11362784
            speed_sensitivity: f(0x3b538cda), // 0.003228
            camber_gain: f(0x3e5a1cac),       // 0.213
            ..common
        },
        "rear" => VanillaSctm {
            ls_mult_y: f(0x41ae4539),         // 21.7838
            ls_exp_y: f(0x3f333333),          // 0.7
            ls_mult_x: f(0x41a066ca),         // 20.05019
            ls_exp_x: f(0x3f35c28f),          // 0.71
            fz0: f(0x4579b000),               // 3995
            max_slip0: f(0x3dd00656),         // 0.101574585
            max_slip1: f(0x3dd609af),         // 0.10451066
            speed_sensitivity: f(0x3b3ef8cf), // 0.002914
            camber_gain: f(0x3e851eb8),       // 0.26
            ..common
        },
        other => panic!("unknown axle {other}"),
    }
}

/// The parameter set a golden row was produced with (see tools/make_sctm_golden.py).
fn sctm_for(case: &str, axle: &str) -> VanillaSctm {
    let mut sctm = f2004_soft(axle);
    let with_curves = |sctm: &mut VanillaSctm| {
        sctm.dy_load_curve = Curve::from_lut_text(DY_CURVE).unwrap();
        sctm.dx_load_curve = Curve::from_lut_text(DX_CURVE).unwrap();
        sctm.d_camber_curve = Curve::from_lut_text(DCAMBER_LUT).unwrap();
    };
    match case {
        "base" => {}
        "cf15" => sctm.combined_factor = 1.5,
        "curves" => with_curves(&mut sctm),
        "curves_smooth" => {
            with_curves(&mut sctm);
            sctm.use_smooth_d_camber_curve = true;
            sctm.combined_factor = 3.0;
        }
        other => panic!("unknown golden case {other}"),
    }
    sctm
}

#[test]
fn golden_rows_are_bit_exact() {
    let rows = oracle_csv::parse(GOLDEN).unwrap();
    assert!(
        rows.len() >= 200,
        "golden file has only {} rows",
        rows.len()
    );
    let mut failures = Vec::new();
    for row in &rows {
        let got = oracle_csv::fields(&sctm_for(&row.case, &row.axle).solve(&row.input));
        let expected = oracle_csv::fields(&row.expected);
        for i in 0..7 {
            if got[i].to_bits() != expected[i].to_bits() {
                failures.push(format!(
                    "line {} ({} {}) {}: AC {} ({:#010x}), port {} ({:#010x})",
                    row.line,
                    row.case,
                    row.axle,
                    OUTPUT_FIELDS[i],
                    expected[i],
                    expected[i].to_bits(),
                    got[i],
                    got[i].to_bits()
                ));
            }
        }
    }
    let backend = match math::backend() {
        Backend::Msvcr120 => "MSVCR120.dll",
        Backend::Std => "Rust std; MSVCR120.dll (Visual C++ 2013 runtime) was not found",
    };
    assert!(
        failures.is_empty(),
        "{} field(s) differ from AC (maths: {backend}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The golden rows reach every branch the test is meant to pin down.
#[test]
fn golden_rows_cover_the_branches() {
    let rows = oracle_csv::parse(GOLDEN).unwrap();
    let count =
        |pred: &dyn Fn(&oracle_csv::OracleRow) -> bool| rows.iter().filter(|r| pred(r)).count();
    for case in ["base", "cf15", "curves", "curves_smooth"] {
        for axle in ["front", "rear"] {
            assert!(
                count(&|r| r.case == case && r.axle == axle) > 0,
                "no {case} {axle} rows"
            );
        }
    }
    assert!(
        count(&|r| r.expected.nd_slip > 1.0) > 0,
        "no row past the peak"
    );
    assert!(
        count(&|r| r.expected.nd_slip > 0.0 && r.expected.nd_slip < 1.0) > 0,
        "no row before the peak"
    );
    assert!(count(&|r| r.input.slip_ratio < 0.0) > 0, "no braking row");
    assert!(
        count(&|r| r.input.use_simple_model) > 0,
        "no simple-model row"
    );
    assert!(count(&|r| r.input.camber_rad != 0.0) > 0, "no cambered row");
    assert!(count(&|r| r.input.blister > 0.0) > 0, "no blistered row");
    assert!(count(&|r| r.input.grain > 0.0) > 0, "no grained row");
    assert!(count(&|r| r.input.speed < 1.0) > 0, "no row below 1 m/s");
}

/// When the extracted F2004 data is present (it is not checked in), loading its tyres.ini
/// must give exactly the parameters the golden rows were produced with.
#[test]
fn tyres_ini_loader_reproduces_the_golden_parameters() {
    let car = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
    if !car.join("tyres.ini").exists() {
        eprintln!("cardata/ks_ferrari_f2004/tyres.ini not present, skipping");
        return;
    }
    for axle in [Axle::Front, Axle::Rear] {
        let loaded = load_sctm(&car, axle, Some("Slick Soft")).unwrap().sctm;
        let expected = f2004_soft(axle.name());
        let bits = |s: &VanillaSctm| {
            [
                s.ls_mult_y,
                s.ls_exp_y,
                s.ls_mult_x,
                s.ls_exp_x,
                s.fz0,
                s.max_slip0,
                s.max_slip1,
                s.asy,
                s.falloff_speed,
                s.speed_sensitivity,
                s.camber_gain,
                s.dcamber0,
                s.dcamber1,
                s.cf_x_mult,
                s.pressure_cf_gain,
                s.brake_dx_mod,
                s.d_camber_blend,
                s.combined_factor,
            ]
            .map(f32::to_bits)
        };
        assert_eq!(bits(&loaded), bits(&expected), "{} axle", axle.name());
        assert_eq!(loaded.dy_load_curve.get_count(), 0);
        assert_eq!(loaded.dx_load_curve.get_count(), 0);
        assert_eq!(loaded.d_camber_curve.get_count(), 0);
        assert!(!loaded.use_smooth_d_camber_curve);
    }
}
