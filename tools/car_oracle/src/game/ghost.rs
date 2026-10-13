// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28c: the game's own `GhostCarRecorder` beside the port's, on the first car of a
//! session of the game's cars (`multi --ghost`).
//!
//! The game's recorder is laid out by hand as its constructor 0x140106b90 leaves it (the
//! constructor wants a whole `Sim`). Two handlers are hung on the real car's own events, as
//! the constructor does: `Car::evOnLapCompleted` calls the game's `onLapCompleted`
//! 0x1401091e0, `Car::evOnStepComplete` the game's `onStepComplete` 0x1401093f0, at the
//! moments the game's physics raises them. The port is given what the car's transponder says
//! at that moment and the matrices the game's `Car` gives. After every call the two
//! recorders' states are compared; at the end the buffers, the saved file
//! (`saveGhostCarRecording` 0x1401097b0, into the scratch root's stand-in Documents), the
//! file loaded again (`loadGhostCarRecording` 0x140108c10) and the playback's getters.

use std::cell::RefCell;

use rustyac_physics::replay::ghost::{GhostCarFrame, GhostCarRecorder, GhostNames, GhostTransponder};
use rustyac_physics::vecmath::Mat44f;

use super::race_ini::write_wstring;
use super::{rd, wr};
use crate::acs::Acs;

const VA_ON_STEP_COMPLETE: usize = 0x1_4010_93f0; // GhostCarRecorder::onStepComplete(const OnStepCompleteEvent&)
const VA_ON_LAP_COMPLETED: usize = 0x1_4010_91e0; // GhostCarRecorder::onLapCompleted(const OnLapCompletedEvent&)
const VA_SAVE: usize = 0x1_4010_97b0; // bool GhostCarRecorder::saveGhostCarRecording()
const VA_LOAD: usize = 0x1_4010_8c10; // bool GhostCarRecorder::loadGhostCarRecording(const std::wstring&)
const VA_SET_CURRENT_FRAME: usize = 0x1_4010_9c30; // setCurrentFrame(unsigned)
const VA_SHOULD_DISPLAY: usize = 0x1_4010_9ca0; // bool shouldDisplayGhostCar()
const VA_GET_BODY_MATRIX: usize = 0x1_4010_7cd0; // mat44f getBodyMatrix()
const VA_GET_TYRE_MATRIX: usize = 0x1_4010_8590; // mat44f getTyreMatrix(int)
const VA_GET_SUSPENSION_MATRIX: usize = 0x1_4010_8520; // mat44f getSuspensionMatrix(int)
const VA_CAR_GET_TYRE_MATRIX: usize = 0x1_4027_26a0; // mat44f Car::getTyreMatrix(int)
const VA_CAR_GET_SUSPENSION_MATRIX: usize = 0x1_4027_24f0; // mat44f Car::getSuspensionMatrix(int)
const VA_EXECUTE_ON_MAIN_THREAD: usize = 0x1_4019_9240; // Sim::executeOnMainThread(std::function<void()>)
const VA_GET_DOCUMENT_PATH: usize = 0x1_4023_0890;

const CAR_EV_ON_STEP_COMPLETE: usize = 0x10;
const CAR_EV_ON_LAP_COMPLETED: usize = 0x40;
const CAR_BODY: usize = 0x118;
const CAR_TRANSPONDER: usize = 0x1f8;

struct State {
    acs: usize,
    car: usize,
    recorder: usize,
    avatar: usize,
    port: GhostCarRecorder,
    names: GhostNames,
    differences: Vec<String>,
    steps: u64,
    kept: u64,
    laps: u64,
    bests: u64,
    /// the lap times (ms) at which a frame was kept, for the playback's check
    seen: u64,
}

thread_local! {
    static GHOST: RefCell<Option<State>> = const { RefCell::new(None) };
}

unsafe fn transponder(car: *const u8) -> GhostTransponder {
    let t = car.add(CAR_TRANSPONDER);
    GhostTransponder { t: rd(t, 0), last_lap: rd(t, 4), lap_count: rd(t, 0xc), finish_line_passed: rd::<u8>(t, 0x10) != 0, was_last_lap_valid: rd::<u8>(t, 0x11) != 0, cuts: rd(t, 0x84) }
}

pub(super) unsafe fn read_wstring(string: *const u8) -> String {
    let len: usize = rd(string, 0x10);
    let cap: usize = rd(string, 0x18);
    let data: *const u16 = if cap > 7 { rd(string, 0) } else { string.cast() };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, len))
}

/// The game's `Car` matrices of this step, as `onStepComplete` takes them.
unsafe fn frame_of(acs: &Acs, car: *mut u8) -> GhostCarFrame {
    let body: *mut u8 = rd(car, CAR_BODY);
    let vtable: *const usize = rd(body, 0);
    let get_world: extern "C" fn(*mut u8, *mut Mat44f, f32) -> *mut Mat44f = std::mem::transmute(*vtable.add(9));
    let tyre: extern "C" fn(*mut u8, *mut Mat44f, i32) -> *mut Mat44f = std::mem::transmute(acs.va(VA_CAR_GET_TYRE_MATRIX));
    let suspension: extern "C" fn(*mut u8, *mut Mat44f, i32) -> *mut Mat44f = std::mem::transmute(acs.va(VA_CAR_GET_SUSPENSION_MATRIX));
    let mut f = GhostCarFrame { body: Mat44f::IDENTITY, wheels: [Mat44f::IDENTITY; 4], suspensions: [Mat44f::IDENTITY; 4] };
    get_world(body, &mut f.body, 0.0);
    for i in 0..4 {
        suspension(car, &mut f.suspensions[i], i as i32);
        tyre(car, &mut f.wheels[i], i as i32);
    }
    f
}

/// The game's recorder against the port's: what the code branches on.
unsafe fn compare(s: &mut State, at: &str) {
    let r = s.recorder as *const u8;
    let count = |offset: usize| (rd::<usize>(r, offset + 8) - rd::<usize>(r, offset)) / 0x240;
    let game = (count(0x78), count(0x90), rd::<u32>(r, 0x130), rd::<u8>(r, 0x134) != 0, rd::<u8>(r, 0x135) != 0, rd::<u32>(r, 0x138), rd::<f64>(r, 0x140).to_bits(), rd::<u8>(r, 0x150) != 0, rd::<i32>(r, 0x154), rd::<i32>(r, 0x158));
    let p = &s.port;
    let port = (p.recording_buffer.len(), p.playing_buffer.len(), p.ghost_car_lap, p.line_passed_for_the_first_time, p.is_ghost_car_lap_valid, p.current_frame, p.last_record_time.to_bits(), p.need_to_save, p.current_lap, p.version);
    if game != port && s.differences.len() < 10 {
        s.differences.push(format!("{at} (step {}): game {game:?}, port {port:?}", s.steps));
    }
}

extern "C" fn on_lap(_this: *mut u8, event: *const u8) {
    GHOST.with(|g| unsafe {
        let mut g = g.borrow_mut();
        let Some(s) = g.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let t = transponder(s.car as *const u8);
        let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_ON_LAP_COMPLETED));
        call(s.recorder as *mut u8, event);
        let names = s.names.clone();
        if s.port.on_lap_completed(&t, &names) {
            s.bests += 1;
        }
        s.laps += 1;
        compare(s, "after onLapCompleted");
    });
}

extern "C" fn on_step(_this: *mut u8, event: *const u8) {
    GHOST.with(|g| unsafe {
        let mut g = g.borrow_mut();
        let Some(s) = g.as_mut() else { return };
        let acs = &*(s.acs as *const Acs);
        let car = s.car as *mut u8;
        let t = transponder(car);
        let physics_time: f64 = rd(event, 8);
        let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_ON_STEP_COMPLETE));
        call(s.recorder as *mut u8, event);
        if s.port.on_step_complete(physics_time, &t, &mut || frame_of(acs, car)) {
            s.kept += 1;
        }
        s.steps += 1;
        s.seen = s.seen.max(t.t as u64);
        compare(s, "after onStepComplete");
    });
}

/// One more handler at the end of an `Event<T>` (as `GameReferee::add_handler`).
pub(super) unsafe fn add_handler(acs: &Acs, event: *mut u8, handler: usize) {
    let (begin, end): (*mut u8, *mut u8) = (rd(event, 0), rd(event, 8));
    let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / 0x28 };
    let buffer = acs.alloc(0x28 * (count + 4));
    for i in 0..count {
        let (from, to) = (begin.add(0x28 * i), buffer.add(0x28 * i));
        std::ptr::copy_nonoverlapping(from, to, 0x28);
        if rd::<usize>(from, 0x20) == from as usize + 8 {
            wr(to, 0x20, to as usize + 8);
        }
    }
    let table = Box::leak(vec![0usize; 6].into_boxed_slice());
    table[2] = handler;
    let implementation = Box::leak(vec![table.as_ptr() as usize; 2].into_boxed_slice());
    wr(buffer.add(0x28 * count), 0x20, implementation.as_ptr());
    wr(event, 0, buffer);
    wr(event, 8, buffer.add(0x28 * (count + 1)));
    wr(event, 0x10, buffer.add(0x28 * (count + 4)));
}

/// A `GhostCarRecorder` as its constructor leaves it, recording and playing.
unsafe fn make_recorder(acs: &Acs, car: *mut u8, avatar: *mut u8, driver: &str) -> *mut u8 {
    let r = acs.alloc(0x160);
    write_wstring(r.add(0x10), "GHOST_CAR_RECORDER");
    wr(r, 0x68, car);
    wr(r, 0x70, avatar);
    write_wstring(r.add(0xa8), driver);
    for offset in [0xc8, 0xe8, 0x108] {
        write_wstring(r.add(offset), "");
    }
    wr(r, 0x130, 0xffff_ffffu32);
    wr(r, 0x135, 1u8);
    wr(r, 0x151, 1u8);
    wr(r, 0x152, 1u8);
    wr(r, 0x15c, 1i32);
    r
}

/// Hangs the game's recorder and the port's on the first car of the session.
pub unsafe fn install(acs: &Acs, car: *mut u8, car_name: &str, track: &str, config: &str) {
    // the "New Ghost Car best" message goes to the main thread's queue of a Sim: not here
    acs.patch(acs.va(VA_EXECUTE_ON_MAIN_THREAD), &[0xc3]);
    let driver = "Ghost Driver";
    let sim = acs.alloc(0x800);
    let track_avatar = acs.alloc(0x400);
    write_wstring(track_avatar.add(0x10), track);
    write_wstring(track_avatar.add(0xa0), config);
    wr(sim, 0x150, track_avatar);
    let avatar = acs.alloc(0x12a8);
    wr(avatar, 0x130, sim);
    write_wstring(avatar.add(0x138), car_name);
    write_wstring(avatar.add(0x10b8), driver);
    let recorder = make_recorder(acs, car, avatar, driver);
    let names = GhostNames { driver: driver.to_string(), track: track.to_string(), config: config.to_string(), car: car_name.to_string() };
    GHOST.with(|g| *g.borrow_mut() = Some(State { acs: acs as *const Acs as usize, car: car as usize, recorder: recorder as usize, avatar: avatar as usize, port: GhostCarRecorder::new(driver, true, true), names, differences: Vec::new(), steps: 0, kept: 0, laps: 0, bests: 0, seen: 0 }));
    // (the constructor's order: the step's handler, then the lap's)
    add_handler(acs, car.add(CAR_EV_ON_STEP_COMPLETE), on_step as *const () as usize);
    add_handler(acs, car.add(CAR_EV_ON_LAP_COMPLETED), on_lap as *const () as usize);
}

unsafe fn buffer_bytes(recorder: *const u8, offset: usize) -> Vec<u8> {
    let begin: *const u8 = rd(recorder, offset);
    if begin.is_null() {
        return Vec::new();
    }
    std::slice::from_raw_parts(begin, rd::<usize>(recorder, offset + 8) - begin as usize).to_vec()
}

fn frames_bytes(frames: &[GhostCarFrame]) -> Vec<u8> {
    let mut out = Vec::new();
    for frame in frames {
        frame.to_bytes(&mut out);
    }
    out
}

/// The end of the session: the lines of the result.
pub unsafe fn finish(repo: &std::path::Path, scenario: &str) -> Result<Vec<String>, String> {
    let Some(mut s) = GHOST.with(|g| g.borrow_mut().take()) else { return Ok(Vec::new()) };
    let acs = &*(s.acs as *const Acs);
    let recorder = s.recorder as *mut u8;
    let mut lines = Vec::new();
    let verdict = |differences: &[String]| if differences.is_empty() { "IDENTICAL".to_string() } else { format!("DIFFERENT ({})", differences.len()) };
    // the two buffers and the names
    if buffer_bytes(recorder, 0x78) != frames_bytes(&s.port.recording_buffer) {
        s.differences.push("the recording buffers differ".to_string());
    }
    if buffer_bytes(recorder, 0x90) != frames_bytes(&s.port.playing_buffer) {
        s.differences.push("the playing buffers differ".to_string());
    }
    let game_names = GhostNames { driver: read_wstring(recorder.add(0xa8)), track: read_wstring(recorder.add(0xc8)), car: read_wstring(recorder.add(0xe8)), config: read_wstring(recorder.add(0x108)) };
    if s.port.need_to_save && game_names != s.port.names {
        s.differences.push(format!("the ghost's names: game {game_names:?}, port {:?}", s.port.names));
    }
    lines.push(format!("ghost | {scenario} | recording | {} steps, {} frames kept, {} laps, {} of them a new ghost (the last of {} ms, {} frames) | {}", s.steps, s.kept, s.laps, s.bests, s.port.ghost_car_lap, s.port.playing_buffer.len(), verdict(&s.differences)));
    for line in &s.differences {
        lines.push(format!("    {line}"));
    }
    if s.port.playing_buffer.is_empty() {
        lines.push(format!("ghost | {scenario} | all | no lap became a ghost: nothing to save"));
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
    let file = folder.join("Assetto Corsa/GhostCar").join(GhostCarRecorder::file_name(&s.port.names));
    let _ = std::fs::remove_file(&file);
    let save: extern "C" fn(*mut u8) -> bool = std::mem::transmute(acs.va(VA_SAVE));
    save(recorder);
    let game_file = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let port_file = s.port.save_bytes();
    let mut file_differences = Vec::new();
    if game_file != port_file {
        let at = game_file.iter().zip(&port_file).position(|(a, b)| a != b).unwrap_or(game_file.len().min(port_file.len()));
        file_differences.push(format!("the files differ ({} and {} bytes), first at {at:#x}", game_file.len(), port_file.len()));
    }
    lines.push(format!("ghost | {scenario} | file {} | {} bytes of the game, {} of the port | {}", GhostCarRecorder::file_name(&s.port.names), game_file.len(), port_file.len(), verdict(&file_differences)));
    lines.extend(file_differences.iter().map(|line| format!("    {line}")));
    // the file loaded by a second recorder of each side
    let loaded = make_recorder(acs, s.car as *mut u8, s.avatar as *mut u8, "Somebody Else");
    let path = acs.alloc(0x20);
    write_wstring(path, &format!("oracle_no_documents/Assetto Corsa/GhostCar/{}", GhostCarRecorder::file_name(&s.port.names)));
    let load: extern "C" fn(*mut u8, *const u8) -> bool = std::mem::transmute(acs.va(VA_LOAD));
    let game_ok = load(loaded, path);
    let mut port = GhostCarRecorder::new("Somebody Else", true, true);
    let port_ok = port.load_bytes(&port_file, &s.names);
    let mut play = Vec::new();
    if game_ok != port_ok || !port_ok {
        play.push(format!("the load answered {game_ok} in the game, {port_ok} in the port"));
    }
    if buffer_bytes(loaded, 0x90) != frames_bytes(&port.playing_buffer) || rd::<u32>(loaded, 0x130) != port.ghost_car_lap || rd::<i32>(loaded, 0x158) != port.version || rd::<i32>(loaded, 0x15c) != port.suspension_wheels_enabled {
        play.push("the loaded ghosts differ".to_string());
    }
    // a file of another car is refused by both
    let other = GhostNames { car: "another_car".to_string(), ..s.names.clone() };
    write_wstring((s.avatar as *mut u8).add(0x138), "another_car");
    let refused = make_recorder(acs, s.car as *mut u8, s.avatar as *mut u8, "x");
    let game_refused = !load(refused, path);
    let port_refused = !GhostCarRecorder::new("x", true, true).load_bytes(&port_file, &other);
    write_wstring((s.avatar as *mut u8).add(0x138), &s.names.car);
    if !game_refused || !port_refused {
        play.push(format!("another car's file: refused by the game {game_refused}, by the port {port_refused}"));
    }
    // the playback: the line was crossed; lap times through the whole ghost and past its end
    wr(loaded, 0x134, 1u8);
    port.line_passed_for_the_first_time = true;
    let set_frame: extern "C" fn(*mut u8, u32) = std::mem::transmute(acs.va(VA_SET_CURRENT_FRAME));
    let should: extern "C" fn(*mut u8) -> bool = std::mem::transmute(acs.va(VA_SHOULD_DISPLAY));
    let body: extern "C" fn(*mut u8, *mut Mat44f) -> *mut Mat44f = std::mem::transmute(acs.va(VA_GET_BODY_MATRIX));
    let tyre: extern "C" fn(*mut u8, *mut Mat44f, i32) -> *mut Mat44f = std::mem::transmute(acs.va(VA_GET_TYRE_MATRIX));
    let suspension: extern "C" fn(*mut u8, *mut Mat44f, i32) -> *mut Mat44f = std::mem::transmute(acs.va(VA_GET_SUSPENSION_MATRIX));
    let (mut shown, mut matrices, mut bad) = (0u64, 0u64, 0u64);
    let end = port.ghost_car_lap.saturating_add(2000);
    let mut lap_time = 0u32;
    let mut k = 0u32;
    while lap_time < end {
        // GhostCar::update: while the ghost shows, the frame and the blend of the lap time (the
        // port's arithmetic: the game's is inside GhostCar, which needs the picture's objects),
        // then the matrices
        let (game_shows, port_shows) = (should(loaded), port.should_display_ghost_car());
        if game_shows != port_shows {
            bad += 1;
        }
        if port_shows {
            port.set_lap_time(lap_time);
            let x = (1.0 / 129.0) * lap_time as f64;
            set_frame(loaded, x as i32 as u32);
            wr(loaded, 0x58, port.blend);
            if rd::<u32>(loaded, 0x138) != port.current_frame {
                bad += 1;
            }
            shown += 1;
            let mut m = Mat44f::IDENTITY;
            let same = |a: &Mat44f, b: Option<Mat44f>| b.is_some_and(|b| a.m.iter().flatten().zip(b.m.iter().flatten()).all(|(x, y)| x.to_bits() == y.to_bits()));
            body(loaded, &mut m);
            matrices += 1;
            if !same(&m, port.get_body_matrix()) {
                bad += 1;
            }
            for i in 0..4 {
                tyre(loaded, &mut m, i as i32);
                matrices += 1;
                if !same(&m, port.get_tyre_matrix(i)) {
                    bad += 1;
                }
                suspension(loaded, &mut m, i as i32);
                matrices += 1;
                if !same(&m, port.get_suspension_matrix(i)) {
                    bad += 1;
                }
            }
        }
        // picture frames of 60, 144 and 20 a second, mixed
        lap_time += [17, 16, 7, 50][k as usize % 4];
        k += 1;
    }
    if bad != 0 {
        play.push(format!("{bad} answers of the playback differ"));
    }
    lines.push(format!("ghost | {scenario} | load and playback | loaded by both, another car's file refused by both; {k} lap times to {end} ms: shown at {shown}, {matrices} matrices compared, {bad} differ | {}", verdict(&play)));
    lines.extend(play.iter().map(|line| format!("    {line}")));
    let ok = s.differences.is_empty() && file_differences.is_empty() && play.is_empty();
    lines.push(format!("ghost | {scenario} | all | {}", if ok { "IDENTICAL" } else { "DIFFERENT" }));
    Ok(lines)
}
