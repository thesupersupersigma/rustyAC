// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

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
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
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

// --- Task 13: the car's body touches things ------------------------------------------------
//
// Three excerpts of recordings made with collisions on (`car_oracle run --track spa --collide`),
// each starting at a moment without a contact joint, a little before the car's collider mesh
// first touches something:
//
// * `spa_wall_low`, steps 3993 to 4192: straight on at La Source, into the barrier at 60 km/h
//   (170 steps with contact joints: floor boxes on the kerb, then the mesh against the wall);
// * `spa_wall_high`, steps 4213 to 4412: off the road before Blanchimont and into the barrier
//   at 283 km/h (the engine is blown up, the damage zones fill);
// * `spa_rollover`, steps 745 to 1084: put down upside down, the car lands on its roof,
//   bounces twice and comes to rest there.
//
// On top of what the other excerpts hash, each step's hash here holds the contact joints (how
// many, a hash over all of them, the first six in full: position, normal, depth, the two
// geoms, triangle numbers, the material), the collision pass's counter and parity, the
// collision clocks, the five damage zones, the four suspensions' damage and the collision
// events the car pushed on the engine's queue (how many, a hash over all their fields, the
// closing speed and the other shape's group of the first and the last). The track and the
// car's collider mesh are read from the game's own folder; without it the tests print a notice
// and pass without testing anything.

const COLLIDE_GOLDEN: [(&str, &[u8]); 3] = [
    ("spa_wall_low", include_bytes!("golden/collide_spa_wall_low_3993_200.chgold")),
    ("spa_wall_high", include_bytes!("golden/collide_spa_wall_high_4213_200.chgold")),
    ("spa_rollover", include_bytes!("golden/collide_spa_rollover_745_340.chgold")),
];

/// The car's collider mesh out of the game's folder, if it is there.
fn collider_mesh(car: &str, data: &std::path::Path, track_folder: &std::path::Path) -> Option<rustyac_physics::car::colliders::ColliderMesh> {
    let root = rustyac_physics::track::loader::game_root(track_folder)?;
    match rustyac_physics::car::colliders::load(data, Some(&root), car) {
        Ok(colliders) if colliders.mesh.is_some() => colliders.mesh,
        other => {
            eprintln!(
                "NOT TESTED: no collider mesh for {car} in {} ({}); the golden replay of a crash needs it",
                root.display(),
                other.err().unwrap_or_else(|| "content/cars/<car>/collider.kn5 is missing".to_string())
            );
            None
        }
    }
}

#[test]
fn collision_golden_files_read_back() {
    for (name, bytes) in COLLIDE_GOLDEN {
        let golden = Golden::parse(bytes).unwrap();
        assert_eq!(golden.setup.scenario, name);
        assert_eq!(golden.track.as_ref().map(|track| track.name.as_str()), Some("spa"));
        assert!(golden.setup.collide.on && golden.collider_mesh && !golden.setup.collide.floor, "{name}: collisions with the car's own mesh");
        assert!(golden.setup.collide.mesh.is_none(), "{name}: the mesh is not in the file");
        assert!(golden.setup.is_whole() && !golden.state.is_empty());
        assert_eq!(golden.to_bytes(), bytes, "{name}: parse and write do not round-trip");
        // the oldest files have no collision keys and read as "no collisions"
        assert!(!Golden::parse(GOLDEN[0].1).unwrap().setup.collide.on);
    }
}

#[test]
fn the_car_hits_spa_s_walls_and_lands_on_its_roof_like_the_game() {
    let Some(data) = car_data("ks_ferrari_f2004") else { return };
    let Some(folder) = track_folder("spa") else { return };
    let Some(mesh) = collider_mesh("ks_ferrari_f2004", &data, &folder) else { return };
    let (track, _) = rustyac_physics::track::load_track(&folder, "").expect("Spa loads");
    let track = std::sync::Arc::new(track);
    let attach = |golden: &mut Golden| {
        golden.attach_track(std::sync::Arc::clone(&track));
        golden.attach_collider_mesh(mesh.clone());
    };
    for (name, bytes) in COLLIDE_GOLDEN {
        let mut golden = Golden::parse(bytes).unwrap();
        // without the mesh an excerpt cannot run, and says so
        golden.attach_track(std::sync::Arc::clone(&track));
        assert!(golden.check(&data).unwrap_err().contains("collider mesh"));
        attach(&mut golden);
        golden.check(&data).unwrap_or_else(|e| panic!("{name} ({}): {e}", backend()));
    }

    // a wall that is not there is noticed: the same excerpt with the body a ghost again
    let mut golden = Golden::parse(COLLIDE_GOLDEN[0].1).unwrap();
    attach(&mut golden);
    let mut ghost = golden.clone();
    ghost.setup.collide.on = false;
    assert!(ghost.check(&data).is_err(), "a car that passes through the barrier went unnoticed");

    // and so is a mesh that is a millimetre longer at the nose (one bit would be lost: Spa's
    // coordinates are hundreds of metres, where a float's step is 0.03 mm)
    let mut moved = golden.clone();
    if let Some(mesh) = &mut moved.setup.collide.mesh {
        for vertex in &mut mesh.vertices {
            vertex[2] += 0.001;
        }
    }
    let error = moved.check(&data).expect_err("a changed collider mesh went unnoticed");
    assert!(error.contains("spa_wall_low step"), "{error}");
}
