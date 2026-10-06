//! The Rust car against Assetto Corsa's own: four excerpts of the whole-car recordings of
//! `tools/car_oracle` (the game's F2004 on a flat road).
//!
//! The rolling chassis alone, with brakes, engine and drivetrain fed from the recording
//! (300 steps each):
//!
//! * `slalom`, steps 3000 to 3299: 100 km/h with the steering swinging at 0.5 Hz;
//! * `kerb`, steps 2684 to 2983: 100 km/h straight, the left wheels climb a 2 cm strip.
//!
//! With brakes, engine, clutch, gearbox, differential and the shift helpers computed in Rust
//! (460 and 400 steps); only the driver's controls, the wings and the traction control's cut are
//! fed:
//!
//! * `launch_autoclutch_off`, steps 522 to 981: a standing start by hand clutch (the clutch
//!   comes up, wheelspin in first gear on the rev limiter, traction control cuts in);
//! * `brake`, steps 2807 to 3206: full braking from 250 km/h with the first down-shifts
//!   (clutch profile, throttle blip, down-shift protection).
//!
//! Each file (`golden/*.chgold`, written by `tools/chassis_compare excerpt`) holds the car's
//! state at the start, per step what the systems that are not ported hand to it, and per step
//! the game's answer: position, rotation and velocities of the six bodies in full, and one
//! hash over all compared values (2,009 of the chassis: body and joint states, joint forces,
//! suspension travel and damper speeds, steer torques, every tyre value, the steering signal,
//! the force-feedback number; and, in the two powertrain excerpts, 51 more: the applied
//! controls, brake bias and power, gear, engine and shaft speeds, clutch state and torque,
//! shift request, differential settings, engine torque, limiter, fuel use, water temperature)
//! and the whole force tape.
//!
//! The car's parameters are not in the files: they are loaded from `cardata/ks_ferrari_f2004`
//! (extracted game data, not in git) by the ported loaders. Without that folder the replay
//! tests print a notice and pass without testing anything.

use std::path::PathBuf;

use rustyac_physics::car::replay::{Golden, Ground};
use rustyac_physics::car::suspension::Damper;
use rustyac_physics::math::{self, Backend};

const GOLDEN: [(&str, &[u8]); 4] = [
    ("slalom", include_bytes!("golden/chassis_slalom_3000_300.chgold")),
    ("kerb", include_bytes!("golden/chassis_kerb_2684_300.chgold")),
    ("launch_autoclutch_off", include_bytes!("golden/powertrain_launch_autoclutch_off_522_460.chgold")),
    ("brake", include_bytes!("golden/powertrain_brake_2807_400.chgold")),
];

fn car_data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
    if data.join("suspensions.ini").is_file() {
        Some(data)
    } else {
        eprintln!(
            "NOT TESTED: {} is missing (the F2004's extracted data files); the chassis golden replay needs them",
            data.display()
        );
        None
    }
}

fn backend() -> &'static str {
    match math::backend() {
        Backend::Msvcr120 => "MSVCR120.dll",
        Backend::Std => "Rust std; MSVCR120.dll (Visual C++ 2013 runtime) was not found",
    }
}

#[test]
fn golden_files_read_back() {
    for (name, bytes) in GOLDEN {
        let golden = Golden::parse(bytes).unwrap();
        assert_eq!(golden.setup.scenario, name);
        // the first two are the chassis alone, the last two have the powertrain in Rust
        let powertrain = name == "launch_autoclutch_off" || name == "brake";
        let steps = match name {
            "launch_autoclutch_off" => 460,
            "brake" => 400,
            _ => 300,
        };
        assert_eq!(golden.steps.len(), steps, "{name}");
        assert_eq!((golden.setup.rust_brakes, golden.setup.rust_drivetrain), (powertrain, powertrain), "{name}");
        assert!(!golden.state.is_empty(), "{name}: the excerpt starts in the middle of a run");
        assert_eq!(golden.to_bytes(), bytes, "{name}: parse and write do not round-trip");
    }
    let kerb = Golden::parse(GOLDEN[1].1).unwrap();
    assert!(matches!(kerb.setup.ground, Ground::Step { .. }));
}

#[test]
fn golden_steps_are_ac_s() {
    let Some(data) = car_data() else { return };
    for (name, bytes) in GOLDEN {
        let golden = Golden::parse(bytes).unwrap();
        if let Err(e) = golden.check(&data) {
            panic!("{name}: {e} (maths and number parsing: {})", backend());
        }
    }
}

/// The replay must be able to fail: one input changed in its last bit has to show.
#[test]
fn a_changed_input_is_noticed() {
    let Some(data) = car_data() else { return };
    // the steering input of one step, one bit up
    let mut golden = Golden::parse(GOLDEN[0].1).unwrap();
    let steer = &mut golden.steps[10].feed.controls.steer;
    *steer = f32::from_bits(steer.to_bits() + 1);
    let error = golden.check(&data).expect_err("a changed steering input went unnoticed");
    assert!(error.contains("step 3010"), "{error}");
    // a wing force of one step, one bit up
    let mut golden = Golden::parse(GOLDEN[1].1).unwrap();
    let force = &mut golden.steps[20].feed.aero[3].a[1];
    *force = f32::from_bits(force.to_bits() + 1);
    let error = golden.check(&data).expect_err("a changed wing force went unnoticed");
    assert!(error.contains("step 2704"), "{error}");
    // the start state: the left front hub one bit higher
    let mut golden = Golden::parse(GOLDEN[1].1).unwrap();
    // bodies are saved first, 29 words each (position 3, quaternion 4, rotation 12, velocities 6,
    // mass and inertia 4); hub_lf is the third, its height the second word
    golden.state[2 * 29 + 1] += 1;
    let error = golden.check(&data).expect_err("a changed start state went unnoticed");
    assert!(error.contains("step 2684"), "{error}");
}

/// The same for the excerpts with brakes, engine and drivetrain in Rust: a pedal, a paddle or
/// the traction control's cut changed in one step has to show.
#[test]
fn a_changed_powertrain_input_is_noticed() {
    let Some(data) = car_data() else { return };
    // the throttle pedal of one step of the launch, one bit down
    let mut golden = Golden::parse(GOLDEN[2].1).unwrap();
    let step = golden.steps.iter().position(|step| step.feed.controls.gas == 1.0).expect("a step at full throttle");
    let gas = &mut golden.steps[step].feed.controls.gas;
    *gas = f32::from_bits(gas.to_bits() - 1);
    let error = golden.check(&data).expect_err("a changed throttle went unnoticed");
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
    // traction control's cut left out in one step
    let mut golden = Golden::parse(GOLDEN[2].1).unwrap();
    let step = golden
        .steps
        .iter()
        .position(|step| step.feed.engine_electronic_override == 0.0)
        .expect("a step in which traction control cuts the throttle");
    golden.steps[step].feed.engine_electronic_override = 1.0;
    let error = golden.check(&data).expect_err("a missing traction control cut went unnoticed");
    // the cut itself is one of the compared values (it acts on the engine of the next step)
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
    // the brake pedal of one step, one bit down
    let mut golden = Golden::parse(GOLDEN[3].1).unwrap();
    let step = golden.steps.iter().position(|step| step.feed.controls.brake == 1.0).expect("a step at full brake");
    let brake = &mut golden.steps[step].feed.controls.brake;
    *brake = f32::from_bits(brake.to_bits() - 1);
    let error = golden.check(&data).expect_err("a changed brake pedal went unnoticed");
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
    // a down-shift paddle press removed: the press lasts ten steps, the gearbox sees the first
    let mut golden = Golden::parse(GOLDEN[3].1).unwrap();
    let step = golden.steps.iter().position(|step| step.feed.controls.gear_dn).expect("a down-shift request");
    for feed in golden.steps[step..].iter_mut().take_while(|step| step.feed.controls.gear_dn) {
        feed.feed.controls.gear_dn = false;
    }
    let error = golden.check(&data).expect_err("a missing down-shift went unnoticed");
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
}

/// `Damper::getForce` @ 0x1402b3280 on the F2004's front damper: the four branches.
#[test]
fn damper_branches() {
    let damper = Damper {
        rebound_slow: 3850.0,
        rebound_fast: 1460.0,
        bump_slow: 2140.0,
        bump_fast: 1100.0,
        fast_threshold_bump: 0.06,
        fast_threshold_rebound: 0.14,
    };
    assert_eq!(damper.get_force(0.05), -(0.05f32 * 2140.0));
    assert_eq!(damper.get_force(0.10), -((0.10f32 - 0.06) * 1100.0 + 0.06f32 * 2140.0));
    assert_eq!(damper.get_force(-0.10), -(-0.10f32 * 3850.0));
    assert_eq!(damper.get_force(-0.20), 0.14f32 * 3850.0 - (0.14f32 + -0.20) * 1460.0);
    assert_eq!(damper.get_force(0.0), -0.0);
}
