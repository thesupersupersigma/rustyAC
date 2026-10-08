// SPDX-License-Identifier: GPL-3.0-or-later

//! Check 1 of Task 11: the game loop does not change the physics.
//!
//! A drive is written as an input file and replayed by `rustyac.exe --replay --headless
//! --dump-states`. The same drive is stepped here on a `VanillaCar` with the physics crate
//! alone (its scripted device, its own functions for every command). Every value of the car
//! (bodies, joints, tyres, counters, brakes, engine, drivetrain, wings, aids, the telemetry
//! page) must be the same bit pattern after every step.
//!
//! Needs `cardata/ks_ferrari_f2004` (extracted game data, not in git); without it the tests
//! print a notice and pass without testing anything.

use std::path::PathBuf;
use std::process::Command;

use rustyac_game::dump::{self, StepDump};
use rustyac_game::input_file::{event, InputFile, SimSetup, StepInput};
use rustyac_physics::car::replay::Ground;
use rustyac_physics::car::{CarControls, ChassisEnvironment, ScriptedDevice, VanillaCar};
use rustyac_physics::vecmath::Vec3f;

fn car_data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
    if data.join("suspensions.ini").is_file() {
        Some(data)
    } else {
        eprintln!("NOT TESTED: {} is missing (the F2004's extracted data files)", data.display());
        None
    }
}

/// A drive with everything in it: the rest after the spawn, a start, shifts, steering, hard
/// braking, the headlight switch, aid keys, brake-bias clicks, a reset and a new car.
fn drive() -> Vec<StepInput> {
    let mut steps = Vec::new();
    let mut gear_timer = 0;
    for n in 0..5200u32 {
        let t = n as f32 * 0.003;
        let mut c = CarControls { clutch: 1.0, ..CarControls::default() };
        let mut s = StepInput::default();
        // phase time: the drive starts again after the reset (step 2600) and the new car (step 3900)
        let (phase, local) = match n {
            0..=2599 => (0, n),
            2600..=3899 => (1, n - 2600),
            _ => (2, n - 3900),
        };
        if n == 2600 {
            s.events |= event::RESET;
        }
        if n == 3900 {
            s.events |= event::REBUILD;
        }
        if local >= 400 {
            // first gear, then flat out with up-shifts every 0.9 s
            if local < 410 {
                c.gear_up = true;
            } else {
                let since = local - 410;
                c.gas = (since as f32 * 0.004).min(1.0) * if phase == 1 { 0.6 } else { 1.0 };
                if since > 500 && since % 300 < 12 {
                    c.gear_up = true;
                    gear_timer += 1;
                }
                c.steer = 0.12 * (t * 3.1).sin() * ((since as f32) * 0.002).min(1.0);
                // the last 1.2 s of a phase: off the throttle, on the brakes, down a gear
                if local >= 1300 - 400 + 410 + 300 {
                    c.gas = 0.0;
                    c.brake = ((local - 1610) as f32 * 0.01).min(0.9);
                    c.gear_dn = (local / 60) % 2 == 0;
                }
            }
        }
        c.drs = phase == 0 && (1500..1700).contains(&n);
        c.hand_brake = if (2400..2500).contains(&n) { 0.7 } else { 0.0 };
        s.headlights = (700..720).contains(&n) || (3000..3003).contains(&n);
        match n {
            900 | 4100 => s.events |= event::TC_UP,
            950 => s.events |= event::TC_DN,
            1000 => s.events |= event::ABS_DN,
            1050 => s.events |= event::ABS_UP,
            1100 => s.bias_clicks = 2,
            1200 => s.bias_clicks = -1,
            4300 => s.events |= event::AUTO_SHIFTER,
            _ => {}
        }
        s.controls = c;
        steps.push(s);
    }
    assert!(gear_timer > 0);
    steps
}

fn spawn(setup: &SimSetup, data: &std::path::Path, clock: f64) -> VanillaCar<ScriptedDevice> {
    let mut car =
        VanillaCar::new(data, ChassisEnvironment::default(), Box::new(Ground::Flat), setup.seed, clock, ScriptedDevice::default()).unwrap();
    car.car.autoclutch.use_auto_on_start = true;
    car.car.autoclutch.use_auto_on_change = true;
    car.car.auto_shifter.is_active = false;
    // as the game does: the collider mesh when the game's folder has one, and no contacts
    // during the first 250 steps of a session
    if let Some(root) = rustyac_game::sim::ac_root() {
        let _ = car.car.load_collider_mesh(&root);
    }
    car.car.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
    car.car.force_position(&Vec3f::new(0.0, 0.0, 0.0));
    car.car.session_start().unwrap();
    car.car.reset_collisions_for_new_session();
    car
}

/// The drive on a `VanillaCar` stepped directly, with the physics crate's own scripted device.
fn stepped_directly(setup: &SimSetup, steps: &[StepInput], data: &std::path::Path) -> Vec<StepDump> {
    let clock = |n: usize| setup.clock_start_ms + (n as f64 + 1.0) * 3.0;
    let mut car = spawn(setup, data, setup.clock_start_ms);
    let mut out = Vec::new();
    for (n, step) in steps.iter().enumerate() {
        if step.events & event::REBUILD != 0 {
            car = spawn(setup, data, clock(n - 1));
        }
        if step.events & event::RESET != 0 {
            car.car.queue(|c| {
                c.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
                c.force_position(&Vec3f::new(0.0, 0.0, 0.0));
                // the teleport repairs the car
                c.set_damage_level(0.0);
                c.reset_suspension_damage_level();
            });
        }
        if step.bias_clicks != 0 {
            car.car.brake_system.as_mut().unwrap().set_manual_front_bias(step.bias_clicks);
        }
        let aids = car.car.aids.as_mut().unwrap().base_mut();
        if step.events & event::TC_UP != 0 {
            aids.traction_control.cycle_mode(1);
        }
        if step.events & event::TC_DN != 0 {
            aids.traction_control.cycle_mode(-1);
        }
        if step.events & event::ABS_UP != 0 {
            aids.abs.cycle_mode(1);
        }
        if step.events & event::ABS_DN != 0 {
            aids.abs.cycle_mode(-1);
        }
        if step.events & event::AUTO_SHIFTER != 0 {
            car.car.auto_shifter.is_active = !car.car.auto_shifter.is_active;
        }
        car.device.controls = step.controls;
        car.device.headlights = step.headlights;
        car.step(0.003, clock(n));
        out.push(StepDump::capture(&car.car));
    }
    out
}

fn assert_same(direct: &[StepDump], game: &[StepDump], what: &str) {
    assert_eq!(direct.len(), game.len(), "{what}: number of steps");
    for (n, (d, g)) in direct.iter().zip(game).enumerate() {
        if d == g {
            continue;
        }
        // name the first value that differs
        for (k, (a, b)) in d.state.iter().zip(&g.state).enumerate() {
            assert_eq!(a, b, "{what}: step {n}, state word {k}");
        }
        for (a, b) in d.trace.iter().zip(&g.trace) {
            assert_eq!(a, b, "{what}: step {n}, {}", a.name);
        }
        panic!("{what}: step {n} differs in size ({} / {} state words, {} / {} traced values)", d.state.len(), g.state.len(), d.trace.len(), g.trace.len());
    }
}

#[test]
fn a_replay_by_the_game_is_the_car_stepped_directly() {
    let Some(data) = car_data() else { return };
    let setup = SimSetup::default();
    let steps = drive();
    let direct = stepped_directly(&setup, &steps, &data);

    // the drive really drives: the car gets going, shifts up, the aids' keys and the reset work
    let speed = |dump: &StepDump| f32::from_bits(dump.trace.iter().find(|v| v.name == "page.speedKmh").unwrap().word as u32);
    let gear = |dump: &StepDump| dump.trace.iter().find(|v| v.name == "page.gear").unwrap().word as i32;
    assert!(speed(&direct[1600]) > 60.0, "{} km/h after 3.6 s of throttle", speed(&direct[1600]));
    assert!(gear(&direct[1600]) >= 3, "gear {}", gear(&direct[1600]));
    assert!(speed(&direct[2100]) < speed(&direct[1600]) - 30.0, "the brakes work: {} km/h", speed(&direct[2100]));
    assert!(speed(&direct[2999]) < 1.0, "after the reset the car stands: {} km/h", speed(&direct[2999]));
    assert!(speed(&direct[5000]) > 30.0, "the new car drives too: {} km/h", speed(&direct[5000]));

    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let input = folder.join("replay_test.ryin");
    let states = folder.join("replay_test.rystate");
    InputFile { setup: setup.clone(), steps: steps.clone() }.write(&input).unwrap();
    assert_eq!(InputFile::read(&input).unwrap().steps, steps, "the input file reads back as written");

    // through the library
    let count = rustyac_game::run_replay_headless(&input, Some(&states)).unwrap();
    assert_eq!(count as usize, steps.len());
    assert_same(&direct, &dump::read(&states).unwrap(), "run_replay_headless");
    std::fs::remove_file(&states).unwrap();

    // through the program itself
    let output = Command::new(env!("CARGO_BIN_EXE_rustyac"))
        .args(["--replay", input.to_str().unwrap(), "--headless", "--no-shm", "--dump-states", states.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "rustyac failed: {}", String::from_utf8_lossy(&output.stderr));
    let game = dump::read(&states).unwrap();
    assert_same(&direct, &game, "rustyac.exe --replay");
    let values: usize = game.iter().map(|g| g.words().len()).sum();
    println!("{} steps, {values} values compared, all bit-identical", game.len());
}

#[test]
fn a_changed_input_is_noticed() {
    // the comparison can fail: one step's steering a little different, and the cars part ways
    let Some(data) = car_data() else { return };
    let setup = SimSetup::default();
    let steps: Vec<StepInput> = drive().into_iter().take(1200).collect();
    let direct = stepped_directly(&setup, &steps, &data);
    let mut changed = steps.clone();
    changed[800].controls.steer += 0.01;
    let other = stepped_directly(&setup, &changed, &data);
    assert_eq!(direct[799], other[799]);
    assert_ne!(direct[800].words(), other[800].words());
    assert_ne!(direct[1199].words(), other[1199].words());
}

#[test]
fn an_unsupported_car_is_refused_with_the_physics_message() {
    // a car the plain game cannot load either (its suspension type is Custom Shaders Patch's
    // own): the program repeats the physics crate's message
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata");
    let Some(car) = ["vrc_formula_alpha_2026_csp"].into_iter().find(|c| root.join(c).join("car.ini").is_file()) else {
        eprintln!("NOT TESTED: no car in cardata/ that is still refused (vrc_formula_alpha_2026_csp)");
        return;
    };
    let setup = SimSetup { car: car.to_string(), ..SimSetup::default() };
    let direct = match VanillaCar::new(&root.join(car), ChassisEnvironment::default(), Box::new(Ground::Flat), 1, 60_000.0, ScriptedDevice::default()) {
        Err(message) => message,
        Ok(_) => {
            eprintln!("NOT TESTED: {car} is supported by now");
            return;
        }
    };
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let input = folder.join("refused.ryin");
    InputFile { setup, steps: vec![StepInput::default()] }.write(&input).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rustyac")).args(["--replay", input.to_str().unwrap(), "--headless", "--no-shm"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // the same message, wherever the car's files were found (the program reads the game's own
    // data.acd, this test the extracted folder)
    let message = direct.rsplit_once(".ini").unwrap().1;
    assert!(stderr.contains(message), "stderr {stderr:?} does not hold {message:?}");
}

// ---------------------------------------------------------------------------------------------
// A live drive, written down by the recorder, replays to the same car.

use rustyac_game::input::keyboard::KeyboardCarControl;
use rustyac_game::input::{CarProbe, Extra, DEVICE_KEYBOARD};
use rustyac_game::sim::{DriverSource, GameSim, ReplaySource, SpawnSequence, DEVICE_SPAWN};
use rustyac_physics::car::CarControlsInput;

/// AC's keyboard class with keys pressed by a script: a live device as far as the game loop
/// can tell. Like the real keyboard it looks at the car before every step (tyre slip, the
/// brake pedal the tyres can take).
struct ScriptedKeyboard {
    keyboard: KeyboardCarControl,
    probe: CarProbe,
    step: u32,
}

impl DriverSource for ScriptedKeyboard {
    fn wants_probe(&self) -> bool {
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        let n = self.step;
        self.step += 1;
        // Up = gas, Down = brake, Left / Right, Space = gear up, L = headlights
        let key_down = move |key: i32| match key {
            0x26 => (30..1300).contains(&n),
            0x28 => (1300..1700).contains(&n),
            0x27 => (500..700).contains(&n),
            0x25 => (800..950).contains(&n),
            0x20 => (400..410).contains(&n) || (700..710).contains(&n),
            _ => false,
        };
        let mut extra = Extra::default();
        self.keyboard.acquire_controls(controls, &mut extra, dt, input, &self.probe, &key_down);
    }

    fn headlights(&mut self) -> bool {
        (200..220).contains(&self.step)
    }

    fn device_id(&self) -> u32 {
        DEVICE_KEYBOARD
    }
}

#[test]
fn a_recorded_live_drive_replays_to_the_same_car() {
    if car_data().is_none() {
        return;
    }
    let ini = rustyac_game::input::bindings::default_ini();
    let keyboard = KeyboardCarControl::from_ini(&rustyac_game::input::bindings::effective(&ini));
    let source = SpawnSequence::new(ScriptedKeyboard { keyboard, probe: CarProbe::default(), step: 0 }, true);
    let setup = SimSetup::default();
    let mut live = GameSim::new(setup.clone(), Box::new(source)).unwrap();
    let mut records = Vec::new();
    let mut dumps = Vec::new();
    for n in 0..2600 {
        if n == 2300 {
            // the game itself puts a car back (as when it has fallen over)
            live.car.device.source.request(event::RESET);
        }
        records.push(live.step().unwrap());
        dumps.push(StepDump::capture(&live.car.car));
    }
    // the spawn sequence: 400 steps of rest, the paddle, a moment for the gearbox: first gear
    assert!(records[..470].iter().all(|r| r.device == DEVICE_SPAWN));
    assert!(records[400..410].iter().all(|r| r.controls.gear_up) && !records[399].controls.gear_up && !records[410].controls.gear_up);
    assert!(records[470..2300].iter().all(|r| r.device == DEVICE_KEYBOARD));
    let gear = |dump: &StepDump| dump.trace.iter().find(|v| v.name == "drivetrain.currentGear").unwrap().word as i32;
    let speed = |dump: &StepDump| f32::from_bits(dump.trace.iter().find(|v| v.name == "page.speedKmh").unwrap().word as u32);
    let rpm = |dump: &StepDump| dump.trace.iter().find(|v| v.name == "page.rpms").unwrap().word as i32;
    assert_eq!(gear(&dumps[469]), 2, "first gear when the driver gets the car");
    assert!(speed(&dumps[469]) < 1.0 && rpm(&dumps[469]) > 1000, "standing, engine running: {} km/h, {} rpm", speed(&dumps[469]), rpm(&dumps[469]));
    // the keyboard drives: throttle ramp, steering, the brake straight to the optimal pedal
    assert!(records[470 + 100].controls.gas > 0.5 && records[470 + 600].controls.steer > 0.05 && records[470 + 900].controls.steer < -0.05);
    assert!(speed(&dumps[470 + 1290]) > 80.0, "{} km/h", speed(&dumps[470 + 1290]));
    let braking = records[470 + 1400].controls.brake;
    assert!(braking > 0.2 && braking <= 1.0, "brake {braking}");
    assert!(records[470 + 210].headlights && records[2300].events & event::RESET != 0);
    // after the game's reset the sequence runs again
    assert!(records[2301..2600].iter().all(|r| r.device == DEVICE_SPAWN));
    assert!(speed(&dumps[2599]) < 1.0);

    // the same drive from the records alone
    let mut replay = GameSim::new(setup.clone(), Box::new(ReplaySource::default())).unwrap();
    let mut replayed = Vec::new();
    for record in &records {
        replay.step_recorded(record).unwrap();
        replayed.push(StepDump::capture(&replay.car.car));
    }
    assert_same(&dumps, &replayed, "replay of a recorded live drive");

    // and through the file
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let (input, states) = (folder.join("live_drive.ryin"), folder.join("live_drive.rystate"));
    InputFile { setup, steps: records }.write(&input).unwrap();
    rustyac_game::run_replay_headless(&input, Some(&states)).unwrap();
    assert_same(&dumps, &dump::read(&states).unwrap(), "replay of the written file");
}

// ---------------------------------------------------------------------------------------------
// The clutch button with the automatic clutch aid on.

use rustyac_game::input::pad::{JoypadCarControl, PadState};

/// AC's pad class with a scripted pad; like the live driver it tells the game when the
/// driver holds the clutch himself.
struct ScriptedPad {
    pad: JoypadCarControl,
    step: u32,
    pending: u32,
}

impl DriverSource for ScriptedPad {
    fn take_events(&mut self) -> (u32, i32) {
        (std::mem::take(&mut self.pending), 0)
    }

    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, input: &CarControlsInput) {
        let n = self.step;
        self.step += 1;
        // full throttle from the start; A (the clutch) held for the first 600 steps
        let state = PadState { right_trigger: 255, buttons: if n < 600 { 0x1000 } else { 0 }, ..PadState::default() };
        let mut extra = Extra::default();
        self.pad.acquire_controls(&state, controls, &mut extra, input, &|_| false);
        if extra.clutch_pressed {
            self.pending |= event::MANUAL_CLUTCH;
        }
    }
}

#[test]
fn the_clutch_button_holds_the_car_although_the_automatic_clutch_is_on() {
    if car_data().is_none() {
        return;
    }
    let ini = rustyac_game::input::bindings::effective(&rustyac_game::input::bindings::default_ini());
    let pad = JoypadCarControl::from_ini(&ini, false);
    let source = SpawnSequence::new(ScriptedPad { pad, step: 0, pending: 0 }, true);
    let mut sim = GameSim::new(SimSetup::default(), Box::new(source)).unwrap();
    let mut records = Vec::new();
    let mut dumps = Vec::new();
    for _ in 0..470 + 1500 {
        records.push(sim.step().unwrap());
        dumps.push(StepDump::capture(&sim.car.car));
    }
    let speed = |dump: &StepDump| f32::from_bits(dump.trace.iter().find(|v| v.name == "page.speedKmh").unwrap().word as u32);
    let rpm = |dump: &StepDump| dump.trace.iter().find(|v| v.name == "page.rpms").unwrap().word as i32;
    // first gear, flat out, the clutch held: the engine revs, the car stands
    // (the very first step of the button is still the aid's: it hears of it one step later)
    assert!(records[472..470 + 600].iter().all(|r| r.events & event::MANUAL_CLUTCH != 0));
    assert!(speed(&dumps[470 + 590]) < 2.0, "{} km/h with the clutch held", speed(&dumps[470 + 590]));
    assert!(rpm(&dumps[470 + 590]) > 10_000, "{} rpm", rpm(&dumps[470 + 590]));
    // let go: the aid has the clutch again and the car drives off
    assert!(records[470 + 602..].iter().all(|r| r.events & event::MANUAL_CLUTCH == 0));
    assert!(speed(&dumps[470 + 1490]) > 40.0, "{} km/h after the clutch was let go", speed(&dumps[470 + 1490]));
    // and all of it replays
    let mut replay = GameSim::new(SimSetup::default(), Box::new(ReplaySource::default())).unwrap();
    let mut replayed = Vec::new();
    for record in &records {
        replay.step_recorded(record).unwrap();
        replayed.push(StepDump::capture(&replay.car.car));
    }
    assert_same(&dumps, &replayed, "replay of the clutch drive");
}

// ---------------------------------------------------------------------------------------------
// Task 15: the session's conditions are part of a recorded drive.

#[test]
fn the_session_travels_in_the_file() {
    use rustyac_game::input_file::Session;
    use rustyac_physics::track::DynamicTrack;
    let session = Session {
        wind_speed: 3.25,
        wind_direction_deg: -12.5,
        dynamic_track: Some(DynamicTrack::from_race_ini(96.0, 2.0, 5.0, 80.0, 12345)),
        ballast_kg: 20.0,
        restrictor: 35.0,
        penalties: false,
        assists: Some((1, 2, 35.0)),
        setup_file: Some(PathBuf::from(r"C:\some folder\setups\car\spa\a name, with = signs.ini")),
        session_type: Some(1),
        arm_first_lap: Some(true),
    };
    let setup = SimSetup { session: session.clone(), auto_blip: Some(false), session_starts_at_spawn: true, drs_zones: true, ..SimSetup::default() };
    let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("session.ryin");
    InputFile { setup: setup.clone(), steps: vec![StepInput::default()] }.write(&file).unwrap();
    let back = InputFile::read(&file).unwrap();
    assert_eq!(back.setup, setup);
    assert_eq!(back.setup.session, session);
    // a drive without any of it writes none of the new lines: older programs read it
    let plain = SimSetup::default().header();
    for key in ["wind=", "dynamic_track=", "ballast_kg=", "restrictor=", "penalties=", "assists=", "setup_file=", "session_type=", "arm_first_lap="] {
        assert!(!plain.contains(key), "{key} in a plain header");
    }
}

// ---------------------------------------------------------------------------------------------
// Task 16: hybrids.

/// The static shared-memory page holds the car as it was built (`CarAvatar::initPhysics`): the
/// SF15-T's turbo has a controller that gives no boost at a standing engine, and from the
/// first step on the turbo's own number is that controller's.
#[test]
fn the_static_page_holds_the_hybrid_as_built() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata");
    if !root.join("ks_ferrari_sf15t/ers.ini").is_file() {
        eprintln!("NOT TESTED: cardata/ks_ferrari_sf15t is missing (the SF15-T's extracted data files)");
        return;
    }
    let setup = SimSetup { car: "ks_ferrari_sf15t".to_string(), ..SimSetup::default() };
    let mut sim = GameSim::new(setup, Box::new(ReplaySource::default())).unwrap();
    let built = rustyac_game::shm::ShmSink::static_page(&sim);
    for _ in 0..5 {
        sim.step_recorded(&StepInput::default()).unwrap();
    }
    let page = rustyac_game::shm::ShmSink::static_page(&sim);
    // [TURBO_0] MAX_BOOST=3.5
    assert_eq!(page.get_f("maxTurboBoost"), 3.5);
    assert_eq!(page.get_f("maxPower"), built.get_f("maxPower"));
    assert!(page.get_f("maxPower") > 0.0);
    assert_eq!((page.get_i("hasERS"), page.get_i("hasKERS")), (1, 0));
    // six delivery profiles, thirteen engine-brake settings, 4000 kJ a lap
    assert_eq!((page.get_i("ersPowerControllerCount"), page.get_i("engineBrakeSettingsCount")), (6, 13));
    assert_eq!(page.get_f("ersMaxJ"), 4_000_000.0);
}

/// A saved setup's hybrid knobs are stepped to by the cockpit's cyclers.
#[test]
fn a_saved_setup_sets_the_hybrid_knobs() {
    use rustyac_physics::data::ini::IniReader;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_sf15t");
    if !data.join("ers.ini").is_file() {
        eprintln!("NOT TESTED: cardata/ks_ferrari_sf15t is missing (the SF15-T's extracted data files)");
        return;
    }
    let mut car = VanillaCar::new(&data, ChassisEnvironment::default(), Box::new(Ground::Flat), 1, 60_000.0, ScriptedDevice::default()).unwrap();
    let chassis = &mut car.car;
    assert_eq!((chassis.cockpit.ers_power_index, chassis.cockpit.ers_recovery, chassis.cockpit.ers_heat_charging), (1, 5, true));
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let file = folder.join("hybrid_setup.ini");
    std::fs::write(&file, "[MGUK_DELIVERY]\nVALUE=4\n\n[MGUK_RECOVERY]\nVALUE=8\n\n[MGUH_MODE]\nVALUE=0\n\n[BRAKE_ENGINE]\nVALUE=3\n").unwrap();
    let saved = IniReader::load(&file).unwrap();
    let setup = IniReader::load(&data.join("setup.ini")).unwrap();
    let mut manager = std::mem::take(&mut chassis.setup_manager);
    let log = manager.load_setup_file(chassis, &setup, &saved).unwrap();
    chassis.setup_manager = manager;
    assert_eq!((chassis.cockpit.ers_power_index, chassis.cockpit.ers_recovery, chassis.cockpit.ers_heat_charging), (4, 8, false));
    let ers = chassis.ers.as_ref().unwrap();
    assert_eq!((ers.kinetic_recovery, ers.is_heat_charging_battery), (8.0f32 * 0.1, false));
    assert!(log.iter().any(|line| line.starts_with("MGUK_DELIVERY = profile 4")), "{log:?}");
    assert!(log.iter().any(|line| line.starts_with("BRAKE_ENGINE = 3: not applied")), "{log:?}");
}
