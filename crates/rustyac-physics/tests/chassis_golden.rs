// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The Rust car against Assetto Corsa's own: five excerpts of the whole-car recordings of
//! `tools/car_oracle` (the game's own code driving a car on a flat road).
//!
//! The rolling chassis alone, with brakes, engine, drivetrain, wings and aids fed from the
//! recording (the F2004, 300 steps each):
//!
//! * `slalom`, steps 3000 to 3299: 100 km/h with the steering swinging at 0.5 Hz;
//! * `kerb`, steps 2684 to 2983: 100 km/h straight, the left wheels climb a 2 cm strip.
//!
//! The **whole car** ([`VanillaCar`](rustyac_physics::car::VanillaCar) with a scripted
//! device): nothing is fed but what the driver did (pedals, wheel, paddles, handbrake,
//! cockpit brake-bias clicks) and the session's settings. Brakes, engine, clutch, gearbox,
//! differential, shift helpers, wings with their controllers, traction control, ABS and the
//! telemetry page are all computed:
//!
//! * the F2004, `launch_autoclutch_off`, steps 522 to 981: a standing start by hand clutch
//!   (the clutch comes up, wheelspin in first gear on the rev limiter, traction control cuts
//!   in above 40 km/h);
//! * the F2004, `brake`, steps 2807 to 3206: full braking from 250 km/h with the first
//!   down-shifts (clutch profile, throttle blip, down-shift protection);
//! * the 488 GT3, `wc_stops`, steps 1845 to 2244: flat out, then the full brake pedal with
//!   the ABS releasing and re-applying the front brakes (automatic gearbox and clutch).
//!
//! Each file (`golden/*.chgold`, written by `tools/chassis_compare excerpt`) holds the car's
//! state at the start, per step what the car is given, and per step the game's answer:
//! position, rotation and velocities of the six bodies in full, and one hash over all compared
//! values (2,009 of the chassis: body and joint states, joint forces, suspension travel and
//! damper speeds, steer torques, every tyre value, the steering signal, the force-feedback
//! number; in the whole-car excerpts about 270 more: the applied controls, brakes, engine and
//! drivetrain, air density and every wing's angle of attack, coefficients, height and forces,
//! the aids' switches and outputs, and the 148 values of the `acpmf_physics` telemetry page)
//! and the whole force tape.
//!
//! The cars' parameters are not in the files: they are loaded from `cardata/<car>` (extracted
//! game data, not in git) by the ported loaders. Without a car's folder its replay tests print
//! a notice and pass without testing anything.

use std::path::PathBuf;

use rustyac_physics::car::replay::{Golden, Ground};
use rustyac_physics::car::suspension::Damper;
use rustyac_physics::math::{self, Backend};

const GOLDEN: [(&str, &[u8]); 5] = [
    ("slalom", include_bytes!("golden/chassis_slalom_3000_300.chgold")),
    ("kerb", include_bytes!("golden/chassis_kerb_2684_300.chgold")),
    ("launch_autoclutch_off", include_bytes!("golden/whole_launch_autoclutch_off_522_460.chgold")),
    ("brake", include_bytes!("golden/whole_brake_2807_400.chgold")),
    ("wc_stops", include_bytes!("golden/whole_488_gt3_wc_stops_1845_400.chgold")),
];

/// The data folder of a golden file's car, if it is there.
fn car_data(car: &str) -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata").join(car);
    if data.join("suspensions.ini").is_file() {
        Some(data)
    } else {
        eprintln!(
            "NOT TESTED: {} is missing (the car's extracted data files); the golden replay of that car needs them",
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
        // the first two are the chassis alone, the others whole cars
        let whole = !matches!(name, "slalom" | "kerb");
        let steps = match name {
            "launch_autoclutch_off" => 460,
            "brake" | "wc_stops" => 400,
            _ => 300,
        };
        assert_eq!(golden.steps.len(), steps, "{name}");
        assert_eq!(golden.setup.is_whole(), whole, "{name}");
        assert_eq!(golden.setup.telemetry, whole, "{name}");
        assert_eq!(golden.car, if name == "wc_stops" { "ks_ferrari_488_gt3" } else { "ks_ferrari_f2004" });
        assert!(!golden.state.is_empty(), "{name}: the excerpt starts in the middle of a run");
        assert_eq!(golden.to_bytes(), bytes, "{name}: parse and write do not round-trip");
    }
    let kerb = Golden::parse(GOLDEN[1].1).unwrap();
    assert!(matches!(kerb.setup.ground, Ground::Step { .. }));
}

/// A whole car's file holds nothing but the driver: every other field of every step is empty.
#[test]
fn whole_car_files_hold_only_the_driver() {
    for (name, bytes) in &GOLDEN[2..] {
        let golden = Golden::parse(bytes).unwrap();
        for (index, step) in golden.steps.iter().enumerate() {
            assert_eq!(step.feed, step.feed.driver_only(), "{name} step {index}");
            assert!(step.feed.aero.is_empty(), "{name} step {index}: wing forces in the file");
        }
        // and the drive is not trivial
        assert!(golden.steps.iter().any(|step| step.feed.controls.gas > 0.0 || step.feed.controls.brake > 0.0), "{name}");
    }
}

#[test]
fn golden_steps_are_ac_s() {
    for (name, bytes) in GOLDEN {
        let golden = Golden::parse(bytes).unwrap();
        let Some(data) = car_data(&golden.car) else { continue };
        if let Err(e) = golden.check(&data) {
            panic!("{name}: {e} (maths and number parsing: {})", backend());
        }
    }
}

/// The replay must be able to fail: one input changed in its last bit has to show.
#[test]
fn a_changed_input_is_noticed() {
    let Some(data) = car_data("ks_ferrari_f2004") else { return };
    // the steering input of one step, one bit up
    let mut golden = Golden::parse(GOLDEN[0].1).unwrap();
    let steer = &mut golden.steps[10].feed.controls.steer;
    *steer = f32::from_bits(steer.to_bits() + 1);
    let error = golden.check(&data).expect_err("a changed steering input went unnoticed");
    assert!(error.contains("step 3010"), "{error}");
    // a wing force of one step, one bit up (this chassis is fed the wings)
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

/// The same for the whole car: a pedal or a paddle changed in one step has to show, and
/// nothing but the driver's controls may be read.
#[test]
fn a_changed_driver_input_is_noticed() {
    let Some(data) = car_data("ks_ferrari_f2004") else { return };
    // the throttle pedal of one step of the launch, one bit down
    let mut golden = Golden::parse(GOLDEN[2].1).unwrap();
    let step = golden.steps.iter().position(|step| step.feed.controls.gas == 1.0).expect("a step at full throttle");
    let gas = &mut golden.steps[step].feed.controls.gas;
    *gas = f32::from_bits(gas.to_bits() - 1);
    let error = golden.check(&data).expect_err("a changed throttle went unnoticed");
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
    // what used to be fed (traction control's cut, wing forces, brake torques) is not read:
    // nonsense in those fields changes nothing
    let mut golden = Golden::parse(GOLDEN[2].1).unwrap();
    for step in &mut golden.steps {
        step.feed.engine_electronic_override = 0.5;
        step.feed.brake_electronic_override = 0.25;
        step.feed.clutch = 0.3;
        step.feed.gear = 4;
        for wheel in &mut step.feed.wheels {
            wheel.brake_torque = 123.0;
            wheel.abs_override = 0.0;
        }
    }
    golden.check(&data).expect("a whole car read something that is not the driver's");
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

/// The 488 GT3's excerpt has the ABS at work; easing the brake pedal by one bit shows at once.
#[test]
fn a_changed_brake_pedal_is_noticed_on_the_488() {
    let Some(data) = car_data("ks_ferrari_488_gt3") else { return };
    let mut golden = Golden::parse(GOLDEN[4].1).unwrap();
    let step = golden.steps.iter().position(|step| step.feed.controls.brake == 1.0).expect("a step at full brake");
    let brake = &mut golden.steps[step].feed.controls.brake;
    *brake = f32::from_bits(brake.to_bits() - 1);
    let error = golden.check(&data).expect_err("a changed brake pedal went unnoticed");
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

// --- the car on a real track (Task 12) -------------------------------------------------------

/// Two excerpts of `tools/car_oracle run --track spa` (the game's own car on the game's own
/// Spa, the body a ghost, only the tyres' rays meeting the track), the whole car in Rust on
/// the Rust track, nothing fed but the driver:
///
/// * `spa_kerbs`, steps 2417 to 2716: into the Bus Stop chicane over its kerbs and off the
///   track with more than two tyres until the lap is cut (surfaces, the lap invalidator);
/// * `spa_launch`, steps 5427 to 5626: over the start line at speed (the armed first crossing
///   of a hot-lap start, which starts the lap timer anew).
///
/// On top of the whole car's values the hash holds every tyre's ray (hit, point, normal, mesh),
/// the surface under every tyre, the lap timer, the lap invalidator and the place along the
/// AI line. The track is not in the files: it is loaded from the game's folder.
const TRACK_GOLDEN: [(&str, &[u8]); 2] = [
    ("spa_kerbs", include_bytes!("golden/track_spa_kerbs_2417_300.chgold")),
    ("spa_launch", include_bytes!("golden/track_spa_launch_5427_200.chgold")),
];

/// A track's folder in Assetto Corsa's own folder (`AC_ROOT`, else Steam's usual place), if
/// it is there.
fn track_folder(name: &str) -> Option<PathBuf> {
    let root = match std::env::var_os("AC_ROOT") {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"),
    };
    let folder = root.join("content").join("tracks").join(name);
    if folder.is_dir() {
        Some(folder)
    } else {
        eprintln!(
            "NOT TESTED: {} is missing (Assetto Corsa's own track folder; set AC_ROOT if the game is elsewhere); the golden replay on that track needs it",
            folder.display()
        );
        None
    }
}

#[test]
fn track_golden_files_read_back() {
    for (name, bytes) in TRACK_GOLDEN {
        let golden = Golden::parse(bytes).unwrap();
        assert_eq!(golden.setup.scenario, name);
        let track = golden.track.as_ref().expect("a track excerpt names its track");
        assert_eq!((track.name.as_str(), track.allowed_tyres_out, track.armed), ("spa", 2, name == "spa_launch"));
        assert_eq!(golden.setup.env.penalty_mode, 1, "{name}: leaving the track costs the lap");
        assert!(golden.setup.is_whole() && !golden.state.is_empty());
        assert_eq!(golden.to_bytes(), bytes, "{name}: parse and write do not round-trip");
        // without its track an excerpt cannot run, and says so
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
        assert!(golden.check(&data).unwrap_err().contains("needs its track"));
    }
}

#[test]
fn the_car_on_spa_matches_the_game() {
    let Some(data) = car_data("ks_ferrari_f2004") else { return };
    let Some(folder) = track_folder("spa") else { return };
    let (track, _) = rustyac_physics::track::load_track(&folder, "").expect("Spa loads");
    let track = std::sync::Arc::new(track);
    for (name, bytes) in TRACK_GOLDEN {
        let mut golden = Golden::parse(bytes).unwrap();
        golden.attach_track(std::sync::Arc::clone(&track));
        golden.check(&data).unwrap_or_else(|e| panic!("{name} ({}): {e}", backend()));
    }
    // the steering wheel one bit off in one step of the chicane is noticed in that step
    let mut golden = Golden::parse(TRACK_GOLDEN[0].1).unwrap();
    golden.attach_track(std::sync::Arc::clone(&track));
    let step = golden.steps.iter().position(|step| step.feed.controls.steer != 0.0).expect("a step with the wheel turned");
    let steer = &mut golden.steps[step].feed.controls.steer;
    *steer = f32::from_bits(steer.to_bits() ^ 1);
    let error = golden.check(&data).expect_err("a changed steering input went unnoticed");
    assert!(error.contains(&format!("step {}", golden.first + step)), "{error}");
}
