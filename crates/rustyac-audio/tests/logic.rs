// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the sound that need neither Assetto Corsa nor FMOD: the small rules around the
//! FMOD calls (smoothing, volumes from `audio.ini`, the backfire trigger, the track's scene
//! searches) and the files of the oracle (a `CarPhysicsState`'s bytes, the golden file).

use std::path::Path;

use rustyac_audio::car::{CarFrame, MixVolumes, SkidParams, SmoothValue};
use rustyac_audio::engine::AudioReverb;
use rustyac_audio::golden::{Golden, AUDIO_INI};
use rustyac_audio::sim::BackfireParams;
use rustyac_audio::track::{Scene, IDENTITY};
use rustyac_physics::data::ini::IniReader;

fn ini(text: &str) -> IniReader {
    IniReader::from_text(Path::new("test.ini"), text)
}

#[test]
fn a_smooth_value_follows_its_target_as_the_game_has_it() {
    // alpha 0 (the player's throttle): straight through
    let mut v = SmoothValue { alpha: 0.0, value: 0.25 };
    v.update(0.75, 0.016);
    assert_eq!(v.value, 0.75);
    // alpha * dt >= 1: straight through
    let mut v = SmoothValue { alpha: 60.0, value: 0.0 };
    v.update(1.0, 1.0 / 60.0 + 0.001);
    assert_eq!(v.value, 1.0);
    // else ((target - value) * alpha) * dt + value, in that order
    let (alpha, dt, target, start) = (3.2f32, 1.0f32 / 60.0, 0.9f32, 0.1f32);
    let mut v = SmoothValue { alpha, value: start };
    v.update(target, dt);
    assert_eq!(v.value.to_bits(), ((((target - start) * alpha) * dt) + start).to_bits());
    // a target that is not finite leaves the value alone
    let before = v.value;
    v.update(f32::NAN, dt);
    v.update(f32::INFINITY, dt);
    assert_eq!(v.value, before);
}

#[test]
fn the_levels_of_audio_ini() {
    let v = MixVolumes::new(&ini(AUDIO_INI));
    assert_eq!((v.wind, v.tyres, v.surfaces, v.dirt, v.engine, v.opponents), (0.3, 1.0, 0.9, 1.0, 1.0, 0.8));
    // the transmission's level is three times its slider
    assert_eq!(v.transmission, 0.333_333_34f32 * 3.0);
    assert_eq!((v.tyres_mult, v.engine_mult), (1.5, 1.5));
    // no TRANSMISSION key: the engine's level; values are clamped to 0..1
    let v = MixVolumes::new(&ini("[LEVELS]\nENGINE=0.7\nWIND=3\nTYRES=-1\n"));
    assert_eq!((v.transmission, v.engine, v.wind, v.tyres), (0.7, 0.7, 1.0, 0.0));
    // no file at all: everything 1, the transmission's level left at -1
    let v = MixVolumes::new(&IniReader::default());
    assert_eq!((v.engine, v.transmission), (1.0, -1.0));
}

#[test]
fn the_skid_entry_point_is_half_a_percent_of_the_slider() {
    let p = SkidParams::new(&ini(AUDIO_INI));
    assert_eq!(p.entry_point, (100.0f32 * 0.5) * 0.01);
    let p = SkidParams::new(&IniReader::default());
    assert_eq!((p.entry_point, p.pitch_base, p.pitch_gain, p.volume_gain), (0.5, 0.75, 0.8, 2.5));
    assert_eq!(p.smooth_alpha.to_bits(), 0x426f_ffff);
}

fn frame(gas: f32, rpm: f32) -> CarFrame {
    let mut f = CarFrame::from_physics_state(&vec![0u8; 0xb70]);
    f.gas = gas;
    f.engine_rpm = rpm;
    f.fuel = 30.0;
    f.engine_life_left = 1000.0;
    f
}

#[test]
fn a_backfire_needs_hard_throttle_then_a_lift_at_revs_with_fuel_in_the_exhaust() {
    let mut params = BackfireParams::new(&ini("[BACKFIRE]\nMAXGAS=0.25\nMINRPM=5000\nMAXRPM=20000\nTRIGGERGAS=0.8\n")).unwrap();
    let mut fuel_in_exhaust = 0.0f32;
    let dt = 1.0 / 60.0;
    // full throttle: the trigger arms, the exhaust fills (dt a frame, up to 10)
    for _ in 0..120 {
        assert!(!params.check(&frame(1.0, 9000.0), &mut fuel_in_exhaust, false, dt));
    }
    assert!(fuel_in_exhaust > 1.9 && fuel_in_exhaust < 2.1);
    // a lift to a tenth of the throttle at 9000 rpm: once
    assert!(params.check(&frame(0.1, 9000.0), &mut fuel_in_exhaust, false, dt));
    assert!(!params.check(&frame(0.1, 9000.0), &mut fuel_in_exhaust, false, dt));
    // below the revs, or with the engine dead, never
    let mut params = BackfireParams::new(&ini("[BACKFIRE]\nMAXGAS=0.25\nMINRPM=5000\nMAXRPM=20000\nTRIGGERGAS=0.8\n")).unwrap();
    let mut fuel_in_exhaust = 5.0f32;
    params.check(&frame(1.0, 3000.0), &mut fuel_in_exhaust, false, dt);
    assert!(!params.check(&frame(0.1, 3000.0), &mut fuel_in_exhaust, false, dt));
    let mut dead = frame(0.1, 9000.0);
    dead.engine_life_left = 0.0;
    assert!(!params.check(&dead, &mut fuel_in_exhaust, false, dt));
    // MAXGAS is at most 0.3
    assert_eq!(BackfireParams::new(&ini("[BACKFIRE]\nMAXGAS=0.9\n")).unwrap().max_gas, 0.3);
}

#[test]
fn the_scene_is_searched_as_the_game_searches_its_nodes() {
    let mut scene = Scene::new();
    let model = scene.add(0, "track.kn5", IDENTITY, false, None);
    let mut moved = IDENTITY;
    moved[12] = 10.0;
    moved[13] = 2.0;
    let group = scene.add(model, "AC_AUDIO_PITS", moved, false, None);
    let child = scene.add(group, "AC_AUDIO_INNER", moved, false, None);
    let mesh = scene.add(model, "AC_AUDIO_MESH", IDENTITY, true, None);
    let pit = scene.add(group, "AC_PIT_0", moved, false, None);
    // depth first, a node before its children, meshes included (the caller leaves them out)
    let mut found = Vec::new();
    scene.find_children_by_prefix(0, "AC_AUDIO_", &mut found);
    assert_eq!(found, vec![group, child, mesh]);
    assert_eq!(scene.find_child_by_name(0, "AC_PIT_0"), Some(pit));
    assert_eq!(scene.find_child_by_name(0, "TRACK_ROOT"), None);
    // a node's world matrix is its own times its parents'
    let w = scene.world_matrix(pit);
    assert_eq!((w[12], w[13], w[14]), (20.0, 4.0, 0.0));
}

#[test]
fn a_reverb_preset_is_found_by_its_exact_name() {
    assert_eq!(AudioReverb::preset_from_name("HANGAR"), 11);
    assert_eq!(AudioReverb::preset_from_name("hangar"), 0);
    assert_eq!(AudioReverb::preset_from_name(""), 0);
}

#[test]
fn a_physics_state_is_read_at_the_offsets_of_the_game() {
    let mut b = vec![0u8; 0xb70];
    let put = |b: &mut Vec<u8>, at: usize, v: f32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut b, 0x244, 7250.0); // engineRPM
    put(&mut b, 0x270, 61.5); // speed, m/s
    put(&mut b, 0x2e4 + 8, 3100.0); // load[2]
    put(&mut b, 0x328 + 0xc8 + 0x90, 0.6); // tyreSurfaceDef[1].gripMod
    b[0x26c] = 4; // gear
    b[0x744] = 1; // isGearGrinding
    for (i, unit) in "grass.wav".encode_utf16().enumerate() {
        b[0x328 + 0xc8 + 2 * i..0x328 + 0xc8 + 2 * i + 2].copy_from_slice(&unit.to_le_bytes());
    }
    // what follows the terminator is not part of the name
    b[0x328 + 0xc8 + 22] = 0x41;
    let f = CarFrame::from_physics_state(&b);
    assert_eq!((f.engine_rpm, f.speed, f.load[2], f.surface_grip_mod[1], f.gear, f.is_gear_grinding), (7250.0, 61.5, 3100.0, 0.6, 4, true));
    assert_eq!(f.surface_wav, [String::new(), "grass.wav".to_string(), String::new(), String::new()]);
}

#[test]
fn the_golden_file_reads_back_as_it_was_written() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/f2004_spa_launch_cockpit_240.augold");
    let golden = Golden::read(&path).unwrap();
    assert_eq!((golden.car.as_str(), golden.track.as_str(), golden.frames.len()), ("ks_ferrari_f2004", "spa", 240));
    let copy = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden_copy.augold");
    golden.write(&copy).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&copy).unwrap());
}
