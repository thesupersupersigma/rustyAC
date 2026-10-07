//! The whole car's branches that no recording of the game reaches: they were ported from the
//! disassembly and read again by reviewers (`docs/port/whole_car.md` section 6.3). These tests
//! pin what that reading says. They are not comparisons with the game; those are in
//! `chassis_golden.rs` and `tools/chassis_compare`.
//!
//! The tests that drive a car need `cardata/ks_ferrari_f2004` (extracted game data, not in
//! git); without it they print a notice and pass without testing anything.

use std::path::PathBuf;

use rustyac_physics::car::replay::Ground;
use rustyac_physics::car::telemetry::{PAGE_FIELDS, PAGE_SIZE};
use rustyac_physics::car::{
    Abs, AidsBase, ChassisEnvironment, ForceSource, ScriptedDevice, SlipStream, TractionControl, VanillaCar,
};
use rustyac_physics::curve::Curve;
use rustyac_physics::vecmath::Vec3f;

const DT: f32 = 0.003;
const CLOCK: f64 = 60_000.0;

fn car_data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004");
    if data.join("suspensions.ini").is_file() {
        Some(data)
    } else {
        eprintln!("NOT TESTED: {} is missing (the F2004's extracted data files)", data.display());
        None
    }
}

/// An F2004 on a flat road, spawned at the origin facing +z, with the game's automatic clutch
/// and gearbox so that a test only needs the pedals.
fn car(env: ChassisEnvironment) -> Option<VanillaCar<ScriptedDevice>> {
    let data = car_data()?;
    let mut car = VanillaCar::new(&data, env, Box::new(Ground::Flat), 1, CLOCK, ScriptedDevice::default()).unwrap();
    car.car.autoclutch.use_auto_on_start = true;
    car.car.autoclutch.use_auto_on_change = true;
    car.car.auto_shifter.is_active = true;
    car.car.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
    car.car.force_position(&Vec3f::new(0.0, 0.0, 0.0));
    car.car.session_start().unwrap();
    car.device.controls.clutch = 1.0;
    Some(car)
}

/// Steps `count` times from step number `from`; returns the next step number.
fn run(car: &mut VanillaCar<ScriptedDevice>, from: usize, count: usize) -> usize {
    for step in from..from + count {
        car.step(DT, CLOCK + (step as f64 + 1.0) * 3.0);
    }
    from + count
}

#[test]
fn the_page_has_the_game_s_size() {
    let values: usize = PAGE_FIELDS.iter().map(|(_, _, count)| count).sum();
    assert_eq!(values, 148);
    assert_eq!(values * 4, PAGE_SIZE);
    assert_eq!(PAGE_SIZE, 592);
}

#[test]
fn the_page_warms_up_like_the_game_s() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    // the game's writer lets 300 steps pass before its first page; that one has packetId 0
    let next = run(&mut car, 0, 300);
    assert!(car.physics_page().is_none());
    run(&mut car, next, 2);
    let page = car.physics_page().unwrap();
    assert_eq!(page.get("packetId"), Some(1));
    assert_eq!(page.to_bytes().len(), PAGE_SIZE);
    // a standing F2004: neutral, engine idling, on its wheels
    assert_eq!(page.get("gear"), Some(1));
    assert!(page.get("rpms").unwrap() as i32 > 1000);
    assert!(f32::from_bits(page.get("wheelLoad.0").unwrap()) > 500.0);
    assert_eq!(page.get("numberOfTyresOut"), Some(0));
    assert_eq!(f32::from_bits(page.get("airTemp").unwrap()), 26.0);
}

#[test]
fn level_keys_of_traction_control_and_abs() {
    // with a table of three levels: up goes 1, 2, 3, off; the limit follows the table
    let mut tc = TractionControl {
        value_curve: Curve::from_pairs(&[(0.0, 0.05), (1.0, 0.08), (2.0, 0.12)]),
        slip_ratio_limit: 0.08,
        current_mode: 1,
        ..TractionControl::default()
    };
    assert_eq!(tc.get_current_mode(), (2, 3));
    tc.cycle_mode(1);
    assert_eq!((tc.get_current_mode(), tc.slip_ratio_limit), ((3, 3), 0.12));
    tc.cycle_mode(1);
    // wrapped: switched off, the limit keeps its last value
    assert_eq!((tc.is_active, tc.get_current_mode(), tc.slip_ratio_limit), (false, (0, 0), 0.12));
    // switched on again upwards starts at the first level, downwards at the last
    tc.cycle_mode(1);
    assert_eq!((tc.is_active, tc.get_current_mode(), tc.slip_ratio_limit), (true, (1, 3), 0.05));
    tc.cycle_mode(-1);
    assert!(!tc.is_active, "down from the first level is off");
    tc.cycle_mode(-1);
    assert_eq!((tc.is_active, tc.get_current_mode(), tc.slip_ratio_limit), (true, (3, 3), 0.12));
    // an aid the car does not have ignores the key
    tc.is_present = false;
    tc.cycle_mode(1);
    assert_eq!(tc.get_current_mode(), (3, 3));

    // without a table: a plain switch, one level
    let mut abs = Abs::default();
    assert_eq!(abs.get_current_mode(), (1, 1));
    abs.cycle_mode(1);
    assert_eq!((abs.is_active, abs.get_current_mode()), (false, (0, 0)));
    abs.cycle_mode(-1);
    assert_eq!((abs.is_active, abs.get_current_mode()), (true, (1, 1)));
}

#[test]
fn the_game_s_assist_options() {
    let mut aids = AidsBase::default();
    aids.abs.is_present = false;
    // "factory": active, present as the car says
    aids.apply_driving_assists(1, 1, 0.0);
    assert_eq!((aids.abs.is_active, aids.abs.is_present), (true, false));
    assert_eq!((aids.traction_control.is_active, aids.traction_control.is_present), (true, true));
    // "on": forced onto a car that does not have it; "off": neither
    aids.apply_driving_assists(2, 0, 75.0);
    assert_eq!((aids.abs.is_active, aids.abs.is_present), (true, true));
    assert_eq!((aids.traction_control.is_active, aids.traction_control.is_present), (false, false));
    assert_eq!(aids.stability_control.gain, 0.75);
    aids.apply_driving_assists(0, 0, -5.0);
    assert_eq!(aids.stability_control.gain, 0.0);
}

#[test]
fn wind_setter_and_gusts() {
    let mut env = ChassisEnvironment::default();
    // straight along +z: every zero component is +0.0, as the game's matrix product leaves it
    env.set_wind(3.0, 0.0);
    assert_eq!([env.wind.x.to_bits(), env.wind.y.to_bits()], [0, 0]);
    assert_eq!((env.wind.z, env.wind_speed, env.wind_direction_deg), (3.0, 3.0, 0.0));
    // the gusts change the strength by at most a tenth and never the direction
    env.set_wind(10.0, 90.0);
    let before = env.wind;
    for step in 0..40_000 {
        env.step_wind(CLOCK + step as f64 * 3.0);
        let strength = (env.wind.x * env.wind.x + env.wind.z * env.wind.z).sqrt();
        assert!((9.0 - 1e-3..=11.0 + 1e-3).contains(&strength), "{strength}");
        assert!(env.wind.x * before.x > 0.0 && (env.wind.z / env.wind.x - before.z / before.x).abs() < 1e-4);
    }
    // below 1 cm/s the wind stands still
    let mut calm = ChassisEnvironment::default();
    calm.wind = Vec3f::new(0.001, 0.0, 0.002);
    calm.wind_speed = 0.005;
    calm.step_wind(CLOCK);
    assert_eq!((calm.wind.x, calm.wind.z), (0.001, 0.002));
}

#[test]
fn a_wake_is_a_cone_behind_the_car() {
    let mut wake = SlipStream::default();
    // a car at z = 100 doing 50 m/s along +z leaves a wake 25 m long pointing back
    wake.set_position(&Vec3f::new(0.0, 0.0, 100.0), &Vec3f::new(0.0, 0.0, 50.0));
    assert_eq!((wake.length, wake.dir.z), (25.0, -1.0));
    let effect = |wake: &SlipStream, x: f32, z: f32| wake.get_slip_effect(&Vec3f::new(x, 0.0, z));
    // right behind: (1 - distance / length) * (cos - 0.7) * 3.33
    let close = effect(&wake, 0.0, 90.0);
    assert!((close - 0.6).abs() < 1e-5, "{close}");
    assert!(effect(&wake, 0.0, 80.0) < close, "weaker further back");
    assert_eq!(effect(&wake, 0.0, 74.0), 0.0, "beyond its length");
    assert_eq!(effect(&wake, 0.0, 110.0), 0.0, "in front of the car");
    assert_eq!(effect(&wake, 9.0, 95.0), 0.0, "outside the cone");
    // a standing car has no wake
    wake.set_position(&Vec3f::new(0.0, 0.0, 100.0), &Vec3f::new(0.0, 0.0, 0.0));
    assert_eq!((wake.length, effect(&wake, 0.0, 99.0)), (0.0, 0.0));
}

#[test]
fn another_car_s_wake_thins_the_air() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    run(&mut car, 0, 1);
    let alone = car.car.air_density;
    assert_eq!(alone, 1.2922 - 26.0 * 0.0041, "a car alone breathes the engine's air");
    // a car 10 m ahead at 50 m/s
    let mut wake = SlipStream::default();
    wake.set_position(&Vec3f::new(0.0, car.car.core.get_position(car.car.body).y, 10.0), &Vec3f::new(0.0, 0.0, 50.0));
    let effect = wake.get_slip_effect(&car.car.core.get_position(car.car.body));
    assert!(effect > 0.5, "{effect}");
    car.car.other_wakes = vec![wake];
    run(&mut car, 1, 1);
    let left = 1.0 - effect;
    let expected = (alone - left * alone) * 0.75 + left * alone;
    assert_eq!(car.car.air_density, expected);
    assert_eq!(car.car.aero.as_ref().unwrap().base().air_density, expected);
    assert!(expected < alone && expected > alone * 0.75);
}

#[test]
fn locked_controls() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    car.device.controls.gas = 1.0;
    car.device.controls.steer = 0.5;
    // locked outright: the device is not asked, the car stands on its brakes
    car.car.lock_controls(true);
    let next = run(&mut car, 0, 5);
    assert_eq!(car.device.polls, 0);
    let c = car.car.controls;
    assert_eq!((c.gas, c.brake, c.steer, c.clutch), (0.0, 1.0, 0.0, 0.0));
    // locked for 30 ms from the next step's clock: the device is asked, the pedals are overridden
    car.car.lock_controls(false);
    car.car.queue(|c| c.lock_controls_until(30.0, c.physics_time));
    let next = run(&mut car, next, 10);
    assert_eq!(car.device.polls, 10);
    assert_eq!((car.car.controls.gas, car.car.controls.brake), (0.0, 1.0));
    // the eleventh step is free again
    run(&mut car, next, 1);
    assert_eq!((car.car.controls.brake, car.car.controls.steer), (0.0, 0.5));
    // a gentle stop: no throttle, a fifth of the brake
    car.car.is_gentle_stopping = true;
    run(&mut car, next + 1, 1);
    assert_eq!((car.car.controls.gas, car.car.controls.brake), (0.0, 0.2));
}

#[test]
fn headlight_switch_toggles_on_a_press() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    car.device.headlights = true;
    let next = run(&mut car, 0, 3);
    assert!(car.car.lights_on, "held: toggled once");
    car.device.headlights = false;
    let next = run(&mut car, next, 2);
    assert!(car.car.lights_on);
    car.device.headlights = true;
    run(&mut car, next, 1);
    assert!(!car.car.lights_on);
}

#[test]
fn a_black_flag_puts_the_car_into_its_pit_box() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    // a pit box 40 m to the left and 30 m ahead, facing -z
    car.car.pit_position.m = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [40.0, 0.0, 30.0, 1.0]];
    car.car.fuel = 10.0;
    car.device.controls.gas = 1.0;
    car.car.queue(|c| c.set_black_flag(true));
    run(&mut car, 0, 1);
    let p = car.car.core.get_position(car.car.body);
    assert!((p.x - 40.0).abs() < 0.01 && (p.z - 30.0).abs() < 0.01, "{p:?}");
    assert!(car.car.is_in_pits());
    // the body's forward axis is the pit matrix's third row
    let forward = car.car.core.get_world_matrix(car.car.body).m[2];
    assert!(forward[0].abs() < 1e-6 && forward[2] < -0.999_999, "{forward:?}");
    // the teleport is a reset: the tank is full again; the controls are dead
    assert_eq!(car.car.fuel, car.car.requested_fuel as f64);
    assert_eq!((car.device.polls, car.car.controls.gas), (0, 0.0));
    // a new session lifts the flag
    car.car.on_new_session();
    run(&mut car, 1, 1);
    assert_eq!(car.device.polls, 1);
}

#[test]
fn penalty_timers() {
    // "cut gas" rules
    let env = ChassisEnvironment { penalty_mode: 0, ..ChassisEnvironment::default() };
    let Some(mut car) = car(env) else { return };
    // below 35 km/h a first penalty is over at once
    car.car.add_penalty(5.0);
    assert_eq!(car.car.get_penalty_time(), 5.0);
    let next = run(&mut car, 0, 1);
    assert_eq!((car.car.penalty_time, car.car.get_penalty_time()), (0.0, 0.0));
    // a second penalty on top of a running one is not forgiven for being slow; it runs down
    // only while the driver is off the throttle
    car.car.add_penalty(5.0);
    car.car.add_penalty(2.0);
    assert!(car.car.disable_min_speed_penalty_clear);
    let next = run(&mut car, next, 2);
    assert_eq!(car.car.penalty_time_accumulator, 7.0 - DT as f64 - DT as f64);
    car.device.controls.gas = 0.5;
    let next = run(&mut car, next, 3);
    assert_eq!(car.car.penalty_time_accumulator, 7.0, "on the throttle the count starts again");
    car.car.clear_penalty();
    assert_eq!((car.car.penalty_time, car.car.disable_min_speed_penalty_clear), (0.0, false));
    // the default rules ("nothing") leave a penalty alone
    car.car.env.penalty_mode = 3;
    car.car.add_penalty(4.0);
    run(&mut car, next, 3);
    assert_eq!(car.car.get_penalty_time(), 4.0);
}

#[test]
fn a_jump_start_sends_the_car_to_the_pits() {
    // the session starts 3 s from now and the grid is watched for the 5 s before it
    let env = ChassisEnvironment {
        session_start_time_ms: CLOCK + 3000.0,
        lock_gearbox_at_start_time_ms: 5000.0,
        ..ChassisEnvironment::default()
    };
    let Some(mut car) = car(env) else { return };
    car.car.pit_position.m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-20.0, 0.0, 5.0, 1.0]];
    // the first step after the spawn only notes where the car stands
    let next = run(&mut car, 0, 1);
    assert!(car.car.has_grid_position);
    assert_eq!(car.car.jump_start_events, 0);
    let next = run(&mut car, next, 20);
    assert_eq!(car.car.jump_start_events, 0, "standing still is no jump start");
    // pushed half a metre forward
    let mut p = car.car.core.get_position(car.car.body);
    p.z += 0.5;
    car.car.core.set_position(car.car.body, &p);
    run(&mut car, next, 1);
    assert_eq!(car.car.jump_start_events, 1);
    let p = car.car.core.get_position(car.car.body);
    assert!((p.x + 20.0).abs() < 0.01 && (p.z - 5.0).abs() < 0.01, "{p:?}");
    // controls dead until 20 s after the start; the teleport cleared the grid position
    assert_eq!(car.car.lock_controls_time, 20_000.0 + CLOCK + 3000.0);
    assert!(!car.car.has_grid_position);
    // once the session has started nothing is watched any more
    car.car.env.session_start_time_ms = 0.0;
    assert!(car.car.has_session_started(0.0));
}

#[test]
fn the_ai_s_stability_aid_makes_a_torque_call() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    let aids = car.car.aids.as_mut().unwrap().base_mut();
    aids.stability_control.gain = 2.0;
    aids.stability_control.use_beta = true;
    car.car.core.tape = Some(Vec::new());
    car.device.controls.gas = 1.0;
    // standing: no call (the forward speed must be above 5 m/s)
    let mut next = run(&mut car, 0, 200);
    let calls = |car: &VanillaCar<ScriptedDevice>| {
        car.car.core.tape.as_ref().unwrap().iter().filter(|call| call.source == ForceSource::Stability).count()
    };
    assert_eq!(calls(&car), 0);
    // under way: one local torque per step, about the body's up axis only
    while car.car.speed < 12.0 && next < 4000 {
        next = run(&mut car, next, 50);
    }
    assert!(car.car.speed >= 12.0, "the car did not get going ({} m/s)", car.car.speed);
    assert_eq!(calls(&car), 1);
    let call = car.car.core.tape.as_ref().unwrap().iter().find(|call| call.source == ForceSource::Stability).copied().unwrap();
    assert_eq!((call.a[0], call.a[2]), (0.0, 0.0));
}

#[test]
fn a_car_on_its_side_collides_with_the_road() {
    let Some(mut car) = car(ChassisEnvironment::default()) else { return };
    run(&mut car, 0, 1);
    assert_eq!(car.car.mesh_collide_mask, 0x1e);
    // another car within 6 m changes nothing unless this car came from the pit lane
    car.car.other_car_positions = vec![Vec3f::new(2.0, 0.3, 0.0)];
    car.car.is_collision_off_for_pits = true;
    run(&mut car, 1, 1);
    assert_eq!(car.car.mesh_collide_mask, 0x1a, "still a ghost while another car is near");
    car.car.other_car_positions = vec![Vec3f::new(20.0, 0.3, 0.0)];
    run(&mut car, 2, 1);
    assert_eq!((car.car.is_collision_off_for_pits, car.car.mesh_collide_mask), (false, 0x1e));
    // the spectator car collides with nothing but walls
    car.car.unix_name = "spectator".to_string();
    run(&mut car, 3, 1);
    assert_eq!(car.car.mesh_collide_mask, 2);
}
