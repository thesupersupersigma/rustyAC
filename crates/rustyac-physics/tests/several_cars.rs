// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 24: several cars in one world.
//!
//! * `golden_rear_end`: an excerpt of a recording of the game's own `PhysicsEngine` with two
//!   F2004s on Spa, one driving flat out into the back of the other (`tools/car_oracle multi
//!   --scenario mc_rear_end`), replayed from step 0 with the recorded controls and held
//!   against the game's values of every step, with no worker thread and with three. Needs
//!   Assetto Corsa's folder (the track, the car's collider mesh): NOT TESTED without it.
//! * `one_car_in_the_engine_is_the_car_alone`: a car stepped by the engine gives the bits of
//!   the same car stepping itself.

use std::path::PathBuf;
use std::sync::Arc;

use rustyac_physics::car::{ChassisEnvironment, RollingChassis, ScriptedDevice, VanillaCar};
use rustyac_physics::engine::record::{Golden, ReplayPaths};
use rustyac_physics::engine::{CarSlot, DeviceRef, PhysicsEngine};
use rustyac_physics::vecmath::Vec3f;

fn cardata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata")
}

fn game_root() -> Option<PathBuf> {
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    root.join("content").join("tracks").join("spa").is_dir().then_some(root)
}

#[test]
fn golden_rear_end() {
    let golden = Golden::from_bytes(include_bytes!("golden/multi_spa_rear_end_1900.mcgold")).expect("the golden file");
    if !cardata().join("ks_ferrari_f2004").join("suspensions.ini").is_file() {
        eprintln!("NOT TESTED: cardata/ks_ferrari_f2004 is missing (the car's extracted data files)");
        return;
    }
    let Some(root) = game_root() else {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the several-car golden replay needs the track spa and the car's collider.kn5");
        return;
    };
    let paths = ReplayPaths { cardata: cardata(), game_root: Some(root) };
    let recording = golden.recording().expect("the header");
    let track = match rustyac_physics::engine::record::load_track(&recording, &paths) {
        Ok(track) => track,
        Err(message) => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
    };
    assert!(golden.hashes.len() >= 1800, "the excerpt is too short to reach the hit");
    for threads in [0, 3] {
        let result = golden.replay(&paths, Arc::clone(&track), threads).expect("the replay");
        assert_eq!(result, None, "with {threads} worker threads the port leaves the game's values at this step");
    }
    // the comparison can fail: one input bit of one step
    let mut spoiled = golden.clone();
    spoiled.inputs[900][0] ^= 1;
    let result = spoiled.replay(&paths, Arc::clone(&track), 0).expect("the replay");
    assert!(result.is_some(), "a changed throttle bit went unnoticed");
}

#[test]
fn one_car_in_the_engine_is_the_car_alone() {
    let data = cardata().join("ks_ferrari_f2004");
    if !data.join("suspensions.ini").is_file() {
        eprintln!("NOT TESTED: cardata/ks_ferrari_f2004 is missing (the car's extracted data files)");
        return;
    }
    let ground = || Box::new(rustyac_physics::car::replay::Ground::Flat);
    let env = ChassisEnvironment::default();
    let place = |car: &mut RollingChassis| {
        car.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
        car.force_position(&Vec3f::new(0.0, 0.0, 0.0));
        car.session_start().expect("the session start");
        car.auto_shifter.is_active = true;
    };
    // alone
    let mut alone = VanillaCar::new(&data, env, ground(), 7, 60_000.0, ScriptedDevice::default()).expect("the car");
    place(&mut alone.car);
    // in an engine, with two worker threads that have nothing to do
    let mut engine = PhysicsEngine::new(env, 2);
    let car_env = engine.car_env();
    let mut car = engine
        .build_car(|core, guid| {
            let mut car = RollingChassis::new_in_core(core, guid, &data, car_env, ground(), 7, 60_000.0)?;
            car.install_aero()?;
            car.install_brakes()?;
            car.install_drivetrain()?;
            car.install_aids()?;
            car.install_telemetry()?;
            Ok(car)
        })
        .expect("the car in the engine");
    place(&mut car);
    let mut device = ScriptedDevice::default();
    for step in 0..2500u32 {
        let t = step as f32 * 0.003;
        let mut controls = alone.device.controls;
        controls.clutch = 1.0;
        controls.gas = if t < 5.0 { (0.2 + 0.2 * t).min(1.0) } else { 0.0 };
        controls.brake = if t >= 5.5 { 0.8 } else { 0.0 };
        controls.steer = if (2.0..5.0).contains(&t) { 0.05 * (3.0 * t).sin() } else { 0.0 };
        alone.device.controls = controls;
        device.controls = controls;
        let time = 60_000.0 + (step as f64 + 1.0) * 3.0;
        alone.step(0.003, time);
        let mut slots = [CarSlot { car: &mut car, device: DeviceRef::Anywhere(&mut device) }];
        engine.step(0.003, time, &mut slots);
        let (a, b) = (alone.car.save_state(), car.save_state());
        if a != b {
            let first = a.iter().zip(&b).position(|(x, y)| x != y);
            let i = first.unwrap_or(0);
            panic!("step {step}: the car in the engine is not the car alone (word {first:?} of {} / {}): {:x?} against {:x?}", a.len(), b.len(), &a[i.saturating_sub(4)..(i + 12).min(a.len())], &b[i.saturating_sub(4)..(i + 12).min(b.len())]);
        }
    }
    assert!(alone.car.speed > 1.0 || alone.car.core.get_position(alone.car.body).z.abs() > 20.0, "the car never moved");
}
