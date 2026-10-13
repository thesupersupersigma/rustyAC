// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's replays (`rustyac_physics::replay`) without the game: made-up drives recorded, saved,
//! loaded and played. The port itself is proven against the game's own code by
//! `tools/car_oracle` (`multi --replay`, `replay-file`; see docs/port/replays.md); the hashes
//! here are of that proven code, so that a later change of it shows.

use rustyac_physics::car_physics_state::CarPhysicsState;
use rustyac_physics::replay::track_env::NoTrack;
use rustyac_physics::replay::*;
use rustyac_physics::vecmath::create_from_euler;

/// The game's own sine and cosine (the same bits on every machine).
trait Exact {
    fn s(self) -> f32;
    fn c(self) -> f32;
}

impl Exact for f32 {
    fn s(self) -> f32 {
        rustyac_math::sinf(self)
    }
    fn c(self) -> f32 {
        rustyac_math::cosf(self)
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// A car that drives a circle, with everything the recorder reads changing.
fn state(car: usize, step: usize) -> CarPhysicsState {
    let t = step as f32 * 0.03 + car as f32;
    let mut s = CarPhysicsState::at_origin();
    let pose = |dx: f32, dy: f32| create_from_euler([t * 0.2 + dx, (t * 0.7).s() * 0.05, (t * 0.9).c() * 0.03], [t.c() * 60.0 + dx, 1.0 + dy, t.s() * 60.0 + car as f32 * 4.0]);
    s.world_matrix = pose(0.0, 0.0);
    for i in 0..4 {
        s.suspension_matrix[i] = pose(i as f32 * 0.3, -0.4);
        s.tyre_matrix[i] = pose(i as f32 * 0.3, -0.5);
        s.wheel_angular_speed[i] = 40.0 + t + i as f32;
        s.tyre_slip[i] = (t + i as f32).s();
        s.slip_ratio[i] = (t * 1.3 + i as f32).c() * 0.2;
        s.nd_slip[i] = 0.5 + (t * 0.4).s() * 0.5;
        s.load[i] = 3000.0 + 500.0 * (t + i as f32).s();
        s.tyre_dirty_level[i] = (step % 40) as f32 * 0.5;
        s.damage_zone_level[i] = (step % 300) as f32;
    }
    s.damage_zone_level[4] = 7.0;
    s.engine_rpm = 3000.0 + 2500.0 * (t * 0.5).s();
    s.gas = (t.s() * 0.5 + 0.5).clamp(0.0, 1.0);
    s.brake = (t.c() * 0.5).max(0.0);
    s.steer = t.s() * 90.0;
    s.gear = 1 + (step / 50 % 5) as i32;
    s.velocity = [t.c() * 30.0, 0.1, -t.s() * 30.0];
    s.body_work_volume = 0.3;
    s.drivetrain_speed = 100.0 + t;
    s.lap_time = (step as u32 % 400) * 30;
    s.last_lap = 12_345;
    s.best_lap = 12_000;
    s.lap_count = (step / 400) as u32;
    s.fuel = 30.0 - step as f32 * 0.01;
    s.fuel_laps = 7.6;
    s.turbo_boost = 1.2 + t.s() * 0.3;
    s.engine_life_left = 990.0;
    s.is_gear_grinding = step % 97 == 0;
    s.is_drift_valid = step % 2 == 0;
    s.actions_state = if step % 60 < 3 { 0x40 | 0x100 } else { 0 };
    s.status_bytes = if step % 120 < 30 { 4 } else { 1 };
    s.kers_is_charging = step % 5 == 0;
    s
}

fn record(cars: usize, steps: usize, max_frames: i32) -> (ReplayManager, SaveInfo) {
    let recorders = (0..cars).map(|i| ReplayRecorder::new(7000 + i as i32, 2)).collect();
    let mut manager = ReplayManager::new(None, 30.0, recorders, Some(1), 3);
    manager.max_frames = max_frames;
    manager.size_ring(Some(1));
    for step in 0..steps {
        let pt = 1000.0 + step as f64 * 3.0;
        let states: Vec<CarPhysicsState> = (0..cars).map(|car| state(car, step)).collect();
        let wings = [[1.5f32, 20.0 + (step % 30) as f32], [0.0, 99.0]];
        let avatars: Vec<AvatarRecord> = (0..cars)
            .map(|car| AvatarRecord {
                actions_state: states[car].actions_state as u32,
                status_bytes: states[car].status_bytes,
                kers_is_charging: states[car].kers_is_charging,
                max_fuel: 60.0,
                car_dirt: Some(step as f32 * 0.0005),
                connected: step % 500 != 499,
                wing_angles: &wings[car % 2],
            })
            .collect();
        let object = create_from_euler([0.1, 0.0, step as f32 * 0.001], [5.0, 0.0, step as f32 * 0.01]);
        if manager.wants_first_leaderboard() {
            manager.on_lap_leaderboard(vec![LeaderboardEntry { car: 0, total_time: 0.0, best_lap: 0.0, laps: 0, is_race_mode: true, is_black_flagged: false, has_completed_last_lap: false }; cars]);
        }
        if step > 0 && step % 1200 == 0 {
            for car in 0..cars {
                manager.on_lap_completed(car, 36_000 + car as u32, (step / 1200) as u32, car != 1);
            }
        }
        manager.on_step_completed(pt, &states, &avatars, 12.0 + step as f32 * 0.001, &[Some(object)]);
    }
    let info = SaveInfo {
        weather: "3_clear".to_string(),
        track: "a_track".to_string(),
        layout: "short".to_string(),
        cars: (0..cars).map(|i| CarHeader { unix_name: format!("car_{i}"), driver_name: format!("Driver {i}"), nation_code: "ITA".to_string(), team: String::new(), skin: "red".to_string() }).collect(),
        wings: vec![2; cars],
        guids: (0..cars as i32).collect(),
    };
    (manager, info)
}

struct Loader;

impl ReplayLoader for Loader {
    fn load_track(&mut self, track: &str, layout: &str) {
        assert_eq!((track, layout), ("a_track", "short"));
    }
    fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder {
        ReplayRecorder::new(7000 + car.unix_name[4..].parse::<i32>().unwrap(), 2)
    }
    fn apply_custom_weather(&mut self, weather: &str) {
        assert_eq!(weather, "3_clear");
    }
}

fn play(manager: &mut ReplayManager, frames: usize) -> u64 {
    let cars = manager.recorders.len();
    let mut outs = vec![CarPhysicsState::at_origin(); cars];
    let mut fx = vec![PlaybackAvatar { max_fuel: 60.0, has_dynamic_effects: true, dirt_level: None, car_node_active: true }; cars];
    let mut hash = 0u64;
    for frame in 0..frames {
        match frame {
            60 => manager.slow_motion(4.0),
            140 => manager.fast_forward(2, false),
            220 => manager.rewind(1, false),
            300 => manager.pause(),
            305 => manager.next_frame(),
            320 => manager.set_normalized_position(0.6),
            321 => manager.play(),
            _ => {}
        }
        let objects = manager.update_play_state([1.0 / 60.0, 1.0 / 144.0, 0.05][frame % 3]);
        for car in 0..cars {
            manager.get_physics_state(car, &mut outs[car], &mut fx[car], &NoTrack);
            hash = fnv(&[&hash.to_le_bytes()[..], &outs[car].to_game_bytes()].concat());
        }
        for object in objects {
            hash = fnv(&[&hash.to_le_bytes()[..], &object.m.iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()].concat());
        }
        hash ^= manager.current_frame as u64;
    }
    hash
}

/// Of the code as the oracle proved it (v0.28.2).
const FILE_HASH: u64 = 0xf320_daf2_c755_60ca;
const FILE_LEN: usize = 347_740;
const PLAYBACK_HASH: u64 = 0x8c5c_2c3b_2b60_1a9f;

#[test]
fn a_recording_saves_loads_and_plays_the_same() {
    // 3 cars, 13500 steps = 1350 frames into a ring of 400: the ring has gone round three times
    let (mut manager, info) = record(3, 13500, 400);
    assert_eq!((manager.recorded_frames, manager.current_recording_index), (400, 150));
    let file = manager.save_bytes(&info).unwrap();
    assert_eq!(i32::from_le_bytes(file[0..4].try_into().unwrap()), 16);
    let mut loaded = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    loaded.load(&file, &mut Loader).unwrap();
    assert!(loaded.has_loaded_replay);
    assert_eq!(loaded.recorders.len(), 3);
    assert_eq!(loaded.recorded_frames, 400);
    assert_eq!(loaded.replay_laps[1].len(), 11);
    assert_eq!(loaded.replay_laps[1][0], ReplayLap { time: 36_001, is_valid: false });
    // the oldest frame of the ring is the first frame of the file
    assert_eq!(loaded.recorders[0].frames[0], manager.recorders[0].frames[150]);
    // a loaded replay records nothing
    assert!(!loaded.on_step_completed(1.0e9, &[], &[], 0.0, &[]));
    loaded.start_replay_mode(0);
    assert_eq!((loaded.cut_in, loaded.cut_out, loaded.current_frame), (0, 399, 400));
    let first = play(&mut loaded, 400);
    // the same file played again gives the same states, bit for bit
    let mut second = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    second.load(&file, &mut Loader).unwrap();
    second.start_replay_mode(0);
    assert_eq!(play(&mut second, 400), first);
    println!("file {:#018x} ({} bytes), playback {first:#018x}", fnv(&file), file.len());
    assert_eq!((fnv(&file), file.len()), (FILE_HASH, FILE_LEN), "the saved file changed");
    assert_eq!(first, PLAYBACK_HASH, "the playback changed");
}

#[test]
fn a_cut_saves_the_frames_between_the_cuts() {
    let (mut manager, info) = record(1, 9000, 1000);
    manager.start_replay_mode(0);
    manager.set_cut_in_perc(0.25);
    manager.set_cut_out_perc(0.75);
    let file = manager.save_bytes(&info).unwrap();
    let mut loaded = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    loaded.load(&file, &mut Loader).unwrap();
    assert_eq!(loaded.recorded_frames, manager.cut_out - manager.cut_in);
    assert_eq!(loaded.recorders[0].frames[0], manager.recorders[0].frames[manager.cut_in as usize]);
}

#[test]
fn a_version_below_15_is_refused_and_15_is_converted() {
    let (mut manager, info) = record(1, 1800, 100);
    let mut file = manager.save_bytes(&info).unwrap();
    file[0..4].copy_from_slice(&14i32.to_le_bytes());
    let mut loaded = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    assert!(loaded.load(&file, &mut Loader).unwrap_err().contains("no longer supported"));
    // a version 15 frame: the drivetrain speed two bytes further on, twelve flags
    let mut v15 = [0u8; FRAME_SIZE];
    v15[0xec..0xee].copy_from_slice(&0x1111u16.to_le_bytes());
    v15[0xee..0xf0].copy_from_slice(&0x3c00u16.to_le_bytes());
    v15[0x10c..0x110].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
    v15[0x108] = 200;
    let frame = convert_replay_format(&v15);
    assert_eq!(&frame.0[0xec..0xf0], &[0x00, 0x3c, 0, 0]);
    assert_eq!(frame.status(), 0xfff);
    assert_eq!(frame.0[0x108], 200);
}

#[test]
fn the_file_names_and_the_autosave_rules() {
    let t = LocalTime { day: 6, month: 10, year: 26, hour: 12, minute: 47, second: 7 };
    assert_eq!(get_replay_file_name(&t, "R", "lotus_exos_125_s1", "spa", ""), "AC_061026-124707_R_lotus_exos_125_s1_spa_");
    assert_eq!(get_auto_file_name(&t, "abarth500", "imola", "gp"), "abarth500_imola_gp_061026-124707");
    assert!(has_illegal_char("a:b") && !has_illegal_char("AC_061026-124707_R_x_spa_"));
    assert!(autosave_pattern_matches("AC_061026-124707_R_lotus_exos_125_s1_spa_.acreplay", "R"));
    assert!(!autosave_pattern_matches("AC_061026-124707_Q_lotus_exos_125_s1_spa_.acreplay", "O"));
    let files = vec!["AC_071026-1_R_b.x".to_string(), "AC_061026-1_R_a.x".to_string(), "AC_081026-1_R_c.x".to_string()];
    assert_eq!(clear_saved(files.clone(), 3), Some("AC_061026-1_R_a.x".to_string()));
    assert_eq!(clear_saved(files.clone(), 4), None);
    assert_eq!(clear_saved(files, -1), None);
    // replay.ini: the rate, the ring's size for the cars, the autosave table
    let ini = rustyac_physics::data::ini::IniReader::from_text(std::path::Path::new("replay.ini"), "[QUALITY]\nLEVEL=4\n[REPLAY]\nMAX_SIZE_MB=26\n[AUTOSAVE]\nENABLED=1\nRACE=3\nQUALIFY=2\nOTHERS=1\nMIN_TIME_SECONDS=30\n");
    let manager = ReplayManager::new(Some(&ini), 30.0, vec![ReplayRecorder::new(9999, 0); 24], Some(0), 3);
    assert_eq!((manager.record_interval_ms, manager.max_frames, manager.min_frames), (15.0, 3982, 2000));
    assert_eq!(manager.auto_save_data.iter().find(|(session, _)| *session == 3).map(|(_, data)| (data.size, data.short_name.as_str())), Some((3, "R")));
    assert_eq!(ReplayManager::new(None, 30.0, Vec::new(), None, 0).max_frames, 18000);
}
