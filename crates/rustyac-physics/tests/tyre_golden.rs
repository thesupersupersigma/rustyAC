//! `VanillaTyre` against AC's own `Tyre::step`, on a made-up tyre (`golden/synthetic_car`,
//! not car data): two scenarios of a few hundred 0.003 s steps each, recorded by
//! `tools/tyre_oracle run --golden`. After every step the whole recorded state (forces and
//! torques handed to the hub, wheel speed, slip, every temperature, pressure, wear, grain,
//! blister, flat spot, ...) must hash to what the game produced.
//!
//! The tyre is loaded from `golden/synthetic_car/tyres.ini` by the ported loader, so this
//! also pins the loader (curves in files and inline, defaults, the parser's quirks).

use std::path::PathBuf;

use rustyac_physics::data::tyres_ini::init_compounds;
use rustyac_physics::math::{self, Backend};
use rustyac_physics::tyre::rig::Golden;

const GOLDEN: [(&str, &str); 2] = [
    (
        "golden_mix front",
        include_str!("golden/synthetic_car_front_golden_mix.golden"),
    ),
    (
        "liftoff rear",
        include_str!("golden/synthetic_car_rear_liftoff.golden"),
    ),
];

fn synthetic_car() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/synthetic_car")
}

fn backend() -> &'static str {
    match math::backend() {
        Backend::Msvcr120 => "MSVCR120.dll",
        Backend::Std => "Rust std; MSVCR120.dll (Visual C++ 2013 runtime) was not found",
    }
}

#[test]
fn golden_steps_are_ac_s() {
    for (name, text) in GOLDEN {
        let golden = Golden::parse(text).unwrap();
        assert!(
            golden.inputs.len() >= 300,
            "{name}: only {} steps",
            golden.inputs.len()
        );
        if let Err(e) = golden.check(&synthetic_car()) {
            panic!("{e} (maths and number parsing: {})", backend());
        }
    }
}

/// The synthetic tyre file loads the way the game loaded it (values the oracle's
/// `coverage` command checked against the game's memory, pinned here as bit patterns).
#[test]
fn synthetic_car_loads_like_the_game() {
    let front = init_compounds(&synthetic_car(), 0).unwrap();
    let rear = init_compounds(&synthetic_car(), 2).unwrap();
    assert_eq!(front.version, 10);
    assert_eq!(front.explosion_temperature, Some(260.0));
    assert_eq!(front.blanket_temperature, Some(75.0));
    assert_eq!(front.use_load_for_vkm, Some(true));
    assert_eq!(front.compound_defs.len(), 2);
    assert_eq!(rear.compound_defs.len(), 2);

    let soft = &front.compound_defs[0];
    assert_eq!(soft.name, "Synthetic Soft (SS)");
    assert_eq!(soft.model_data.brake_dx_mod, 1.05);
    assert_eq!(
        soft.data.radius_raise_k.to_bits(),
        (0.021f32 * 0.001).to_bits()
    );
    assert_eq!(soft.model_data.wear_curve.get_count(), 7);
    assert_eq!(soft.data.grain_threshold, 75.0);
    assert_eq!(soft.data.blister_threshold, 105.0);

    let curves = &front.compound_defs[1];
    // a repeated key keeps its first value; zeros fall back to the defaults
    assert_eq!(curves.data.radius, 0.312);
    assert_eq!(curves.data.angular_inertia, 1.2);
    assert_eq!(curves.data.d, 520.0);
    assert_eq!(curves.data.k, 215000.0);
    assert_eq!(curves.pressure_static, 26.0);
    assert_eq!(curves.model_data.pressure_spring_gain, 1000.0);
    assert_eq!(curves.model_data.ideal_pressure, 26.0);
    assert_eq!(
        (curves.model_data.dcamber0, curves.model_data.dcamber1),
        (0.1, -0.8)
    );
    // CX_MULT is missing: 0, not the constructor's 1
    assert_eq!(curves.model_data.cf_x_mult, 0.0);
    assert_eq!(curves.model_data.dy_load_curve.get_count(), 10);
    assert_eq!(curves.model_data.dx_load_curve.get_count(), 7);
    assert_eq!(curves.model_data.d_camber_curve.get_count(), 9);
    assert!(curves.model_data.use_smooth_d_camber_curve);
    assert_eq!(curves.model_data.combined_factor, 2.2);

    let rear_curves = &rear.compound_defs[1];
    // missing files give empty curves; no thermal section: defaults, and zero grain gains
    assert_eq!(rear_curves.model_data.wear_curve.get_count(), 0);
    assert_eq!(rear_curves.model_data.dx_load_curve.get_count(), 0);
    assert_eq!(rear_curves.model_data.d_camber_curve.get_count(), 9);
    assert_eq!(rear_curves.thermal_patch_data.surface_transfer, 0.3);
    assert_eq!(rear_curves.data.grain_gamma, 0.0);
    assert_eq!(rear_curves.thermal_performance_curve.get_count(), 0);
}
