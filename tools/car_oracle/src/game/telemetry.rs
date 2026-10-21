// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28c: the game's own `ComparativeTelemetryRecorder` beside the port's, on the first
//! car of a session of the game's cars (`multi --telemetry`).
//!
//! The game's recorder is laid out by hand as its constructor 0x1400731b0 leaves it (the
//! constructor wants a whole `Sim`); its queue is made by the runtime's own constructor. Two
//! handlers are hung on the real car's events and call the game's own two closures: the
//! sample of every step (0x140073820) and the lap's end (0x1400736e0). The game's `update`
//! 0x140076850 runs after every sixth step (a picture at about 55 a second); its own call of
//! `RaceManager::getLapCount` is sent to a stand-in that reads the car's own lap count (Task 38d:
//! before, the whole function was replaced, which changed the referee's values). The port gets the same
//! car values at the same moments. At the end: the three laps' slots, the saved file
//! (`saveBest` 0x140075d50, into the scratch root's stand-in Documents) and the file loaded
//! again (`loadBest` 0x140074d30).

use std::cell::RefCell;

use rustyac_physics::replay::telemetry::{ComparativeTelemetryRecorder, CtData, TelemetryNames, SLOTS};

use super::ghost::{add_handler, read_wstring};
use super::race_ini::write_wstring;
use super::{rd, wr};
use crate::acs::Acs;

const VA_SAMPLE: usize = 0x1_4007_3820; // the closure of Car::evOnStepComplete
const VA_LAP: usize = 0x1_4007_36e0; // the closure of Car::evOnLapCompleted
const VA_UPDATE: usize = 0x1_4007_6850; // ComparativeTelemetryRecorder::update(float)
const VA_SAVE_BEST: usize = 0x1_4007_5d50; // saveBest()
const VA_LOAD_BEST: usize = 0x1_4007_4d30; // loadBest()
const VA_UPDATE_CALL_LAP_COUNT: usize = 0x1_4007_68c2; // update's `call RaceManager::getLapCount`
const VA_UPDATE_PADDING: usize = 0x1_4007_69c2; // 14 bytes of int3 after update's ret
const VA_QUEUE_CTOR_IMPORT: usize = 0x1_404a_46c8; // MSVCP120's _Concurrent_queue_base_v4(size_t)
const VA_QUEUE_VFTABLE: usize = 0x1_404b_02a8; // concurrent_queue<CTData>::`vftable'
const VA_GET_DOCUMENT_PATH: usize = 0x1_4023_0890;

const CAR_EV_ON_STEP_COMPLETE: usize = 0x10;
const CAR_EV_ON_LAP_COMPLETED: usize = 0x40;
const CAR_LAP_COUNT: usize = 0x1f8 + 0xc;

struct State {
    acs: usize,
    car: usize,
    recorder: usize,
    avatar: usize,
    closure: usize,
    lap_count: usize,
    port: ComparativeTelemetryRecorder,
    differences: Vec<String>,
    steps: u64,
    laps: u64,
    bests: u64,
    dropped: u64,
}

thread_local! {
    static TELEMETRY: RefCell<Option<State>> = const { RefCell::new(None) };
}

unsafe fn slots(recorder: *const u8, offset: usize) -> Vec<u8> {
    let begin: *const u8 = rd(recorder, offset);
    std::slice::from_raw_parts(begin, rd::<usize>(recorder, offset + 8) - begin as usize).to_vec()
}

fn port_slots(data: &[CtData]) -> Vec<u8> {
    let mut out = Vec::new();
    for d in data {
        d.to_bytes(&mut out);
    }
    out
}

/// A `ComparativeTelemetryRecorder` as its constructor leaves it (before `loadBest`).
unsafe fn make_recorder(acs: &Acs, car: *mut u8, avatar: *mut u8, names: &TelemetryNames) -> *mut u8 {
    let r = acs.alloc(0x170);
    write_wstring(r.add(0x10), "COMPARATIVE_TELEMETRY_RECORDER");
    wr(r, 0x58, avatar);
    wr(r, 0x60, car);
    let queue: extern "C" fn(*mut u8, usize) -> *mut u8 = std::mem::transmute(rd::<usize>(acs.va(VA_QUEUE_CTOR_IMPORT) as *const u8, 0));
    queue(r.add(0x68), 0x14);
    wr(r, 0x68, acs.va(VA_QUEUE_VFTABLE));
    for offset in [0x90, 0xa8, 0xc0] {
        let buffer = acs.alloc(SLOTS * 0x14);
        wr(r, offset, buffer);
        wr(r, offset + 8, buffer.add(SLOTS * 0x14));
        wr(r, offset + 0x10, buffer.add(SLOTS * 0x14));
    }
    write_wstring(r.add(0xd8), &names.car);
    write_wstring(r.add(0xf8), &names.track);
    write_wstring(r.add(0x118), &names.driver);
    write_wstring(r.add(0x138), &names.config);
    wr(r, 0x158, u32::MAX);
    wr(r, 0x15c, u32::MAX);
    wr(r, 0x168, 0.00025f32);
    r
}

unsafe fn compare(s: &mut State, at: &str) {
    let r = s.recorder as *const u8;
    let game = (rd::<u32>(r, 0x158), rd::<u32>(r, 0x15c), rd::<u8>(r, 0x160) != 0, rd::<u32>(r, 0x164));
    let p = &s.port;
    let port = (p.best_time, p.last_time, p.need_to_save, p.last_recording_index);
    if game != port && s.differences.len() < 10 {
        s.differences.push(format!("{at} (step {}): game {game:?}, port {port:?}", s.steps));
    }
}

extern "C" fn on_lap(_this: *mut u8, event: *const u8) {
    TELEMETRY.with(|t| unsafe {
        let mut t = t.borrow_mut();
        let Some(s) = t.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let call: extern "C" fn(*const usize, *const u8) = std::mem::transmute(acs.va(VA_LAP));
        call(s.closure as *const usize, event);
        if s.port.on_lap_completed(rd(event, 4), rd(event, 8)) {
            s.bests += 1;
        }
        s.laps += 1;
        compare(s, "after the lap's handler");
        // the whole lap's slots after each lap
        let r = s.recorder as *const u8;
        if slots(r, 0x90) != port_slots(&s.port.current) || slots(r, 0xa8) != port_slots(&s.port.last) || slots(r, 0xc0) != port_slots(&s.port.best) {
            s.differences.push(format!("the slots after lap {} differ", s.laps));
        }
    });
}

extern "C" fn on_step(_this: *mut u8, event: *const u8) {
    TELEMETRY.with(|t| unsafe {
        let mut t = t.borrow_mut();
        let Some(s) = t.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let car = s.car as *const u8;
        let call: extern "C" fn(*const usize, *const u8) = std::mem::transmute(acs.va(VA_SAMPLE));
        call(s.closure as *const usize, event);
        s.port.on_step_complete(rd(car, 0x894), rd(car, 0x3ad0), rd(car, 0x3e90), rd(car, 0x164), rd(car, 0x168));
        s.steps += 1;
        if s.steps % 6 == 0 {
            let lap_count: u32 = rd(car, CAR_LAP_COUNT);
            *(s.lap_count as *mut u32) = lap_count;
            if lap_count == 0 {
                s.dropped += s.port.queue.len() as u64;
            }
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE));
            update(s.recorder as *mut u8, 0.018);
            s.port.update(lap_count);
            compare(s, "after update");
        }
    });
}

/// Hangs the game's recorder and the port's on the first car of the session.
pub unsafe fn install(acs: &Acs, car: *mut u8, car_name: &str, track: &str, config: &str) {
    // only the recorder's own call of RaceManager::getLapCount reads the number this file keeps:
    // a stand-in (mov rax, imm64; mov eax, [rax]; ret) in the int3 padding after `update`; the rest
    // of the game (referee, standings) keeps every car's own lap count
    let lap_count = Box::leak(Box::new(0u32)) as *mut u32 as usize;
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&(lap_count as u64).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x00, 0xc3]);
    let stub = acs.va(VA_UPDATE_PADDING);
    assert!(std::slice::from_raw_parts(stub as *const u8, code.len()).iter().all(|&b| b == 0xcc), "no int3 padding after update");
    acs.patch(stub, &code);
    let call = acs.va(VA_UPDATE_CALL_LAP_COUNT);
    assert_eq!(*(call as *const u8), 0xe8);
    let mut code = vec![0xe8];
    code.extend_from_slice(&((stub as i64 - (call as i64 + 5)) as i32).to_le_bytes());
    acs.patch(call, &code);
    let names = TelemetryNames { driver: "Telemetry Driver".to_string(), track: track.to_string(), car: car_name.to_string(), config: config.to_string() };
    let sim = acs.alloc(0x800);
    let track_avatar = acs.alloc(0x400);
    write_wstring(track_avatar.add(0x10), track);
    write_wstring(track_avatar.add(0xa0), config);
    wr(sim, 0x150, track_avatar);
    let avatar = acs.alloc(0x12a8);
    wr(avatar, 0x130, sim);
    write_wstring(avatar.add(0x138), car_name);
    write_wstring(avatar.add(0x10b8), &names.driver);
    let recorder = make_recorder(acs, car, avatar, &names);
    let closure = Box::leak(Box::new(recorder as usize)) as *const usize as usize;
    TELEMETRY.with(|t| *t.borrow_mut() = Some(State { acs: acs as *const Acs as usize, car: car as usize, recorder: recorder as usize, avatar: avatar as usize, closure, lap_count, port: ComparativeTelemetryRecorder::new(names), differences: Vec::new(), steps: 0, laps: 0, bests: 0, dropped: 0 }));
    add_handler(acs, car.add(CAR_EV_ON_STEP_COMPLETE), on_step as *const () as usize);
    add_handler(acs, car.add(CAR_EV_ON_LAP_COMPLETED), on_lap as *const () as usize);
}

/// The end of the session: the lines of the result.
pub unsafe fn finish(repo: &std::path::Path, scenario: &str) -> Result<Vec<String>, String> {
    let Some(mut s) = TELEMETRY.with(|t| t.borrow_mut().take()) else { return Ok(Vec::new()) };
    let acs = &*(s.acs as *const Acs);
    let recorder = s.recorder as *mut u8;
    let mut lines = Vec::new();
    let verdict = |differences: &[String]| if differences.is_empty() { "IDENTICAL".to_string() } else { format!("DIFFERENT ({})", differences.len()) };
    for (name, offset, port) in [("running", 0x90, &s.port.current), ("last", 0xa8, &s.port.last), ("best", 0xc0, &s.port.best)] {
        if slots(recorder, offset) != port_slots(port) {
            s.differences.push(format!("the {name} lap's slots differ"));
        }
    }
    let filled = s.port.best.iter().filter(|d| d.speed != 0.0).count();
    lines.push(format!("telemetry | {scenario} | recording | {} samples ({} of the lap out dropped), {} laps, {} of them a new best (the last of {} ms; {} of {SLOTS} slots with a speed); the three laps' slots after every lap and at the end | {}", s.steps, s.dropped, s.laps, s.bests, s.port.best_time, filled, verdict(&s.differences)));
    for line in &s.differences {
        lines.push(format!("    {line}"));
    }
    if !s.port.need_to_save {
        lines.push(format!("telemetry | {scenario} | all | no lap was finished: nothing to save"));
        return Ok(lines);
    }
    // the file: the game's own save, into the stand-in Documents of the scratch root
    let get: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GET_DOCUMENT_PATH));
    let text = acs.alloc(0x20);
    get(text);
    let documents = read_wstring(text);
    let folder = std::path::absolute(std::path::Path::new(&documents)).map_err(|e| e.to_string())?;
    if documents != "oracle_no_documents" || !folder.starts_with(repo) {
        return Err(format!("the game's Documents folder is {documents:?}: nothing is saved"));
    }
    std::fs::create_dir_all(folder.join("Assetto Corsa")).map_err(|e| e.to_string())?;
    let name = ComparativeTelemetryRecorder::file_name(&s.port.names);
    let file = folder.join("Assetto Corsa/ctelemetry").join(&name);
    let _ = std::fs::remove_file(&file);
    let save: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_SAVE_BEST));
    save(recorder);
    let game_file = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let port_file = s.port.save_bytes().unwrap_or_default();
    let mut file_differences = Vec::new();
    if game_file != port_file {
        let at = game_file.iter().zip(&port_file).position(|(a, b)| a != b).unwrap_or(game_file.len().min(port_file.len()));
        file_differences.push(format!("the files differ ({} and {} bytes), first at {at:#x}", game_file.len(), port_file.len()));
    }
    if slots(recorder, 0xc0) != port_slots(&s.port.best) {
        file_differences.push("the best lap in memory after the save differs".to_string());
    }
    lines.push(format!("telemetry | {scenario} | file {name} | {} bytes of the game, {} of the port | {}", game_file.len(), port_file.len(), verdict(&file_differences)));
    lines.extend(file_differences.iter().map(|line| format!("    {line}")));
    // the file loaded by a second recorder of each side; another driver's is refused by both
    let mut load_differences = Vec::new();
    let loaded = make_recorder(acs, s.car as *mut u8, s.avatar as *mut u8, &s.port.names);
    let load: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_LOAD_BEST));
    load(loaded);
    let mut port = ComparativeTelemetryRecorder::new(s.port.names.clone());
    let port_ok = port.load_bytes(&port_file);
    if !port_ok || rd::<u32>(loaded, 0x158) != port.best_time || slots(loaded, 0xc0) != port_slots(&port.best) {
        load_differences.push(format!("the loaded laps differ (the port's load answered {port_ok}; best {} and {})", rd::<u32>(loaded, 0x158), port.best_time));
    }
    let other = TelemetryNames { driver: "Somebody Else".to_string(), ..s.port.names.clone() };
    write_wstring((s.avatar as *mut u8).add(0x10b8), &other.driver);
    // (the game looks for the other driver's file: the same bytes under that name)
    let other_file = folder.join("Assetto Corsa/ctelemetry").join(ComparativeTelemetryRecorder::file_name(&other));
    std::fs::write(&other_file, &port_file).map_err(|e| e.to_string())?;
    let refused = make_recorder(acs, s.car as *mut u8, s.avatar as *mut u8, &other);
    load(refused);
    let mut port_refused = ComparativeTelemetryRecorder::new(other);
    let answered = port_refused.load_bytes(&port_file);
    let _ = std::fs::remove_file(&other_file);
    write_wstring((s.avatar as *mut u8).add(0x10b8), &s.port.names.driver);
    if answered || rd::<u32>(refused, 0x158) != u32::MAX || port_refused.best_time != u32::MAX || slots(refused, 0xc0) != port_slots(&port_refused.best) {
        load_differences.push(format!("another driver's file: the game's best time {}, the port's {} (answer {answered})", rd::<u32>(refused, 0x158), port_refused.best_time));
    }
    lines.push(format!("telemetry | {scenario} | load | the file loaded by both ({SLOTS} slots, best {} ms), another driver's file refused by both | {}", port.best_time, verdict(&load_differences)));
    lines.extend(load_differences.iter().map(|line| format!("    {line}")));
    let all = s.differences.is_empty() && file_differences.is_empty() && load_differences.is_empty();
    lines.push(format!("telemetry | {scenario} | all | {}", if all { "IDENTICAL" } else { "DIFFERENT" }));
    Ok(lines)
}
