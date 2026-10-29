// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 44: the game's own `AISplineRecorder` (the one `Track::initAISpline` made for the
//! oracle's track) recording the first car, beside the port's (`multi --ai-record`).
//!
//! `startRecording` 0x140296f00 is called on the game's recorder; `Track::step` then steps
//! it inside `PhysicsEngine::step`, right after the `evOnPreStep` handlers. A handler hung
//! last on `evOnPreStep` gives the port's recorder what the game's reads at that moment, and
//! one on the car's `evOnLapCompleted` the lap events. After two laps both record a pit lane
//! (`beginPitLaneSpline` / `endPitLaneSpline`) for 20 s. At the end the game's `save(false)`
//! 0x140296ae0 writes into a scratch folder (the track's data folder is pointed there first:
//! the game would write into the track's own `ai\` folder) and its files are compared byte
//! for byte with the port's.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use rustyac_physics::track::ai_line::AiSplineRecorder;
use rustyac_physics::track::Track;

use super::ghost::add_handler;
use super::race_ini::write_wstring;
use super::{rd, wr};
use crate::acs::Acs;

const VA_START_RECORDING: usize = 0x1_4029_6f00; // AISplineRecorder::startRecording()
const VA_SAVE: usize = 0x1_4029_6ae0; // AISplineRecorder::save(bool inPlace)
const VA_BEGIN_PIT_LANE: usize = 0x1_4029_5020; // AISplineRecorder::beginPitLaneSpline()
const VA_END_PIT_LANE: usize = 0x1_4029_5110; // AISplineRecorder::endPitLaneSpline()
const VA_POINTS_COUNT: usize = 0x1_402a_96f0; // AISpline::pointsCount()
const VA_CAR_GET_SPEED: usize = 0x1_4027_2160; // Speed Car::getSpeed()

const ENGINE_EV_ON_PRE_STEP: usize = 0x88;
const TRACK_AI_SPLINE_RECORDER: usize = 0x80;
const TRACK_DATA_FOLDER: usize = 0x108;
const REC_SPLINE_CURRENT: usize = 0x10;
const REC_IS_SAVE_NEEDED: usize = 0x39c;
const CAR_EV_ON_LAP_COMPLETED: usize = 0x40;
const CAR_BODY: usize = 0x118;
const CAR_GAS: usize = 0x164;
const CAR_BRAKE: usize = 0x168;
const CAR_PHYSICS_GUID: usize = 0x3238;

/// Steps of the pit lane's recording (20 s).
const PIT_STEPS: u64 = 6667;

struct State {
    acs: usize,
    car: usize,
    recorder: usize,
    port: AiSplineRecorder,
    steps: u64,
    laps: u64,
    pit_from: Option<u64>,
    pit_done: bool,
    first_difference: Option<String>,
}

thread_local! {
    static REC: RefCell<Option<State>> = const { RefCell::new(None) };
}

unsafe fn game_points(acs: &Acs, recorder: *mut u8) -> u32 {
    let count: extern "C" fn(*mut u8) -> u32 = std::mem::transmute(acs.va(VA_POINTS_COUNT));
    count(recorder.add(REC_SPLINE_CURRENT))
}

extern "C" fn on_pre_step(_this: *mut u8, _event: *const u8) {
    REC.with(|r| unsafe {
        let mut r = r.borrow_mut();
        let Some(s) = r.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let car = s.car as *mut u8;
        let body: *mut u8 = rd(car, CAR_BODY);
        let vtable: *const usize = rd(body, 0);
        let get_position: extern "C" fn(*mut u8, *mut [f32; 3], f32) -> *mut [f32; 3] = std::mem::transmute(*vtable.add(0xa0 / 8));
        let mut pos = [0.0f32; 3];
        get_position(body, &mut pos, 0.0);
        let get_speed: extern "C" fn(*mut u8, *mut [f32; 16]) -> *mut f32 = std::mem::transmute(acs.va(VA_CAR_GET_SPEED));
        let mut speed = [0.0f32; 16];
        get_speed(car, &mut speed);
        s.port.step(rd(car, CAR_PHYSICS_GUID), pos, speed[0], rd(car, CAR_GAS), rd(car, CAR_BRAKE));
    });
}

extern "C" fn on_lap(_this: *mut u8, event: *const u8) {
    REC.with(|r| unsafe {
        let mut r = r.borrow_mut();
        let Some(s) = r.as_mut() else { return };
        s.port.on_lap_completed(rd(event, 0), rd(event, 4));
        s.laps += 1;
    });
}

/// Starts both recorders on the first car.
pub unsafe fn install(acs: &Acs, engine: *mut u8, game_track: *mut u8, car: *mut u8, port_track: &Track) -> Result<(), String> {
    let recorder: *mut u8 = rd(game_track, TRACK_AI_SPLINE_RECORDER);
    if recorder.is_null() {
        return Err("the game's track has no AISplineRecorder".to_string());
    }
    let mut port = port_track.ai().cloned().ok_or("the port's track has no AI lines")?;
    let start: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_START_RECORDING));
    start(recorder);
    port.start_recording();
    REC.with(|r| *r.borrow_mut() = Some(State { acs: acs as *const Acs as usize, car: car as usize, recorder: recorder as usize, port, steps: 0, laps: 0, pit_from: None, pit_done: false, first_difference: None }));
    add_handler(acs, engine.add(ENGINE_EV_ON_PRE_STEP), on_pre_step as *const () as usize);
    add_handler(acs, car.add(CAR_EV_ON_LAP_COMPLETED), on_lap as *const () as usize);
    Ok(())
}

/// After every step: the lap being recorded on both sides, and the pit lane's recording.
pub unsafe fn after_step() {
    REC.with(|r| {
        let mut r = r.borrow_mut();
        let Some(s) = r.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let recorder = s.recorder as *mut u8;
        s.steps += 1;
        let (game, port) = (game_points(acs, recorder), s.port.spline_current.point_count() as u32);
        if game != port && s.first_difference.is_none() {
            s.first_difference = Some(format!("step {}: the lap being recorded has {game} points in the game, {port} in the port", s.steps));
        }
        if s.laps >= 2 && s.pit_from.is_none() {
            let begin: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_BEGIN_PIT_LANE));
            begin(recorder);
            s.port.begin_pit_lane_spline();
            s.pit_from = Some(s.steps);
        }
        if !s.pit_done && s.pit_from.is_some_and(|from| s.steps >= from + PIT_STEPS) {
            let end: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_END_PIT_LANE));
            end(recorder);
            s.port.end_pit_lane_spline();
            s.pit_done = true;
        }
    });
}

/// Both recorders save; the lines of the result.
pub unsafe fn finish(repo: &Path, scenario: &str, game_track: *mut u8, port_track: &Arc<Track>) -> Result<Vec<String>, String> {
    let Some(mut s) = REC.with(|r| r.borrow_mut().take()) else { return Ok(Vec::new()) };
    let acs = &*(s.acs as *const Acs);
    let recorder = s.recorder as *mut u8;
    let root = repo.join("re/scratch/task44/ai_record").join(scenario);
    let (game_dir, port_dir) = (root.join("game"), root.join("port"));
    for dir in [&game_dir, &port_dir] {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir.join("ai")).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // the game writes <data folder>/ai/*.ai.candidate: its data folder becomes the scratch one
    write_wstring(game_track.add(TRACK_DATA_FOLDER), &game_dir.display().to_string().replace('\\', "/"));
    let save: extern "C" fn(*mut u8, u8) = std::mem::transmute(acs.va(VA_SAVE));
    save(recorder, 0);
    // (nothing more when the track goes: the destructor saves only when this is set)
    wr(recorder, REC_IS_SAVE_NEEDED, 0u8);
    let files = s.port.save(port_track, &port_dir.join("data"))?;
    let mut lines = Vec::new();
    let mut ok = s.first_difference.is_none();
    lines.push(format!(
        "ai_record | {scenario} | recording | {} steps, {} laps, pit lane from step {:?} for {PIT_STEPS} steps | {}",
        s.steps,
        s.laps,
        s.pit_from,
        s.first_difference.as_deref().unwrap_or("IDENTICAL")
    ));
    for name in ["fast_lane.ai.candidate", "pit_lane.ai.candidate"] {
        let game = std::fs::read(game_dir.join("ai").join(name)).ok();
        let port = files.iter().find(|f| f.0 == name).map(|f| f.1.clone());
        if let Some(port) = &port {
            std::fs::write(port_dir.join("ai").join(name), port).map_err(|e| e.to_string())?;
        }
        let verdict = match (&game, &port) {
            (None, None) => "written by neither".to_string(),
            (Some(g), Some(p)) if g == p => format!("{} bytes, IDENTICAL", g.len()),
            (Some(g), Some(p)) => {
                ok = false;
                let at = g.iter().zip(p).position(|(a, b)| a != b).unwrap_or(g.len().min(p.len()));
                let words = g.chunks(4).zip(p.chunks(4)).filter(|(a, b)| a != b).count();
                format!("game {} bytes, port {} bytes, first difference at byte {at}, {words} words differ: DIFFERENT", g.len(), p.len())
            }
            (g, p) => {
                ok = false;
                format!("written by the game: {}, by the port: {}: DIFFERENT", g.is_some(), p.is_some())
            }
        };
        lines.push(format!("ai_record | {scenario} | {name} | {verdict}"));
    }
    lines.push(format!("ai_record | {scenario} | all | {} (files in {})", if ok { "IDENTICAL" } else { "DIFFERENT" }, root.display()));
    Ok(lines)
}
