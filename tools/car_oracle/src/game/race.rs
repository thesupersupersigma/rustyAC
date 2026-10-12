// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28: the game's own referee, in-process.
//!
//! `RaceManager` is a main-thread object tied to `Sim`, so it is built by hand here: a
//! zeroed block with the members its constructor and `loadSessions` would set, a session list
//! of records made by the game's own `Session::Session`, and around it just enough of `Sim`
//! for the manager's code to run: a `Game` with the clock, the car list, a `TrackAvatar` with
//! the sector list, a camera manager and an end-of-session panel that are only their flags.
//! `RaceTimingServices`, `CarRaceInfo` (each car's grid slot) and `JsonOutputFile` are made by
//! the game's own constructors. Every car is a `CarAvatar`-sized block holding what the
//! manager reads of one.
//!
//! What then runs is the game's own code, by address: `setCurrentSession`, `startRace`,
//! `update` (with `resetInvalidStateCarsToPits`, `updateCarsRealTimePositions`,
//! `updateCarsEndRace`, `isRaceOver`), `onLapCompleted`, `RaceTimingServices::onLapCompleted`
//! / `onSectorSplit` / `updateLeaderboard`, `skipCurrentSession`, `restartCurrentSession`,
//! `finalizeCurrentSession` with the whole `SessionResultProcessor`, the fade lambdas, the
//! session-end handler of `CarRaceInfo`, `JsonOutputFile::endSession` and `writeOutputFile`.
//! What the manager hands on to the rest of the game (the camera's fade, the system message,
//! the results panel, a car's teleport, the queue of the physics thread) is written down.

use std::cell::RefCell;

use rustyac_physics::race::manager::{filter_speed, Action, CarView, SessionsIni, SESSION_RACE};
use rustyac_physics::session::MsvcRand;
use rustyac_physics::track::timing::{OnLapCompletedEvent, OnSectorSplitEvent};

use super::race_ini::{rd, text, wr, write_wstring, zeroed};
use super::*;
use crate::acs::Acs;

const VA_RM_VTABLE: usize = 0x1_404c_af48;
const VA_CAR_AVATAR_VTABLE: usize = 0x1_404b_e5e8;
const VA_SIM_VTABLE: usize = 0x1_404d_8010;
const VA_SESSION_CTOR: usize = 0x1_4009_4f70; // Session::Session()
const VA_RM_SET_CURRENT_SESSION: usize = 0x1_4014_0df0;
const VA_RM_START_RACE: usize = 0x1_4014_1470; // void RaceManager::startRace(float)
const VA_RM_UPDATE: usize = 0x1_4014_1550; // void RaceManager::update(float)
const VA_RM_ON_LAP_COMPLETED: usize = 0x1_4013_f380;
const VA_RM_SKIP: usize = 0x1_4014_1350;
const VA_RM_RESTART: usize = 0x1_4014_0d00;
const VA_RM_RESET_CURRENT_LAPS: usize = 0x1_4014_0300;
const VA_RM_RESET_MANDATORY_PIT: usize = 0x1_4014_0600;
const VA_RM_SHUTDOWN: usize = 0x1_4014_1320;
const VA_RTS_CTOR: usize = 0x1_4014_3c60; // RaceTimingServices::RaceTimingServices(Sim*)
const VA_RTS_ON_SECTOR_SPLIT: usize = 0x1_4014_5940;
const VA_RTS_UPDATE_LEADERBOARD: usize = 0x1_4014_6660;
const VA_CAR_RACE_INFO_CTOR: usize = 0x1_400e_78e0;
const VA_CAR_RACE_INFO_INIT: usize = 0x1_400e_8070; // CarRaceInfo::init(CarAvatar*)
const VA_CAR_RACE_INFO_SET: usize = 0x1_400e_84a0; // setSpawnPositionIndex(const wstring&, int)
const VA_CAR_GET_SPAWN_INDEX: usize = 0x1_400e_8050; // int CarRaceInfo::getSpawnPositionIndex(const wstring&)
const VA_CAR_SET_CONTROLS_LOCK: usize = 0x1_400d_9f00; // CarAvatar::setControlsLock(bool)
const VA_JSON_CTOR: usize = 0x1_4010_c0e0; // JsonOutputFile::JsonOutputFile(Sim*)
const VA_JSON_WRITE: usize = 0x1_4010_ecc0; // JsonOutputFile::writeOutputFile()
// replaced by functions of this file
const VA_KS_GET_TIME: usize = 0x1_4023_8a30;
const VA_REGISTER_TO_FADE_IN: usize = 0x1_4003_3ce0; // ACCameraManager::registerToFadeInEvent(function, void*)
const VA_FADE_IN: usize = 0x1_4003_3690; // ACCameraManager::fadeIn(const FadeModeDef&)
const VA_ADD_MESSAGE: usize = 0x1_401b_61c0; // SystemMessage::addMessage(wstring, wstring, type)
const VA_DISPLAY_RESULTS: usize = 0x1_4010_b260;
const VA_HIDE_RESULTS: usize = 0x1_4010_b380;
const VA_GET_COMPOUND: usize = 0x1_400d_3200; // wstring CarAvatar::getCurrentCompoundShortName()
const VA_EXECUTE_ON_PHYSICS_THREAD: usize = 0x1_4012_1f20;
const VA_GO_TO_SPAWN: usize = 0x1_400d_3ae0; // CarAvatar::goToSpawnPosition(const wstring&)

// offsets (acs.pdb)
const RM_SIM: usize = 0x168;
const RM_RTS: usize = 0x170;
const RM_SESSIONS: usize = 0x188;
const RM_REAL_TIME: usize = 0x1a0;
const RM_RACE_STATUS: usize = 0x1b8;
const RM_LEADERBOARD_HISTORY: usize = 0x1e8;
const RM_SESSION_HISTORY: usize = 0x200;
const RM_CURRENT_SESSION: usize = 0xe8;
const RM_EV_ON_SESSION_END: usize = 0x70;
const SIM_EV_ON_NEW_SESSION: usize = 0xc0;
const SIM_OPTIONS_MAP: usize = 0xf8;
const SIM_TRACK: usize = 0x150;
const SIM_CAMERA_MANAGER: usize = 0x188;
const SIM_SYSTEM_MESSAGE: usize = 0x198;
const SIM_RACE_MANAGER: usize = 0x1a8;
const SIM_REPLAY_MANAGER: usize = 0x1b0;
const SIM_PHYSICS_AVATAR: usize = 0x1b8;
const SIM_CARS: usize = 0x208;
const SIM_END_SESSION_DISPLAYER: usize = 0x260;
const CM_EV_ON_FADE_IN_FINISHED: usize = 0x178;
const TA_LENGTH: usize = 0x78;
const TA_CONFIG: usize = 0xa0;
const TA_SECTORS: usize = 0xf8 + 0x130;
const PA_PENALTY_MODE: usize = 0x12c;
const CA_SIM: usize = 0x130;
const CA_UNIX_NAME: usize = 0x138;
const CA_GUI_NAME: usize = 0x178;
const CA_NET: usize = 0x210; // what isConnected() asks
const CA_POSITION: usize = 0x29c;
const CA_SPEED: usize = 0x4d8;
const CA_NPOS: usize = 0x984;
const CA_STATE_RETIRED: usize = 0xa0c;
const CA_RACE_ENGINEER: usize = 0xf80;
const CA_IS_BLACK_FLAGGED: usize = 0x1000;
const CA_PIT_POSITION: usize = 0x1078;
const CA_DRIVER_NAME: usize = 0x10b8;
const CA_CURRENT_SKIN: usize = 0x1138;
const CA_GUID: usize = 0x1158;
const CA_PHYSICS: usize = 0x1168;
const CA_RACE_INFO: usize = 0x11c8;
const CA_FILTERED_SPEED: usize = 0x1278;
const CAR_IS_RETIRED: usize = 0xf0;

/// What the stand-ins for the rest of the game were told, and the clock they answer with.
#[derive(Default)]
struct Told {
    now: f64,
    log: Vec<String>,
    jobs: Vec<usize>,
    avatars: Vec<usize>,
    compounds: Vec<String>,
    /// the callable of a fade that was registered (a `std::function`'s `_Func_impl`, copied)
    fade_callable: Option<[u64; 3]>,
    fade_left: Option<f32>,
    camera: usize,
    image_base: usize,
    new_session: bool,
    set_controls_lock: usize,
    /// whole sessions: the posted jobs are kept to be run before the next physics step
    keep_jobs: bool,
    kept: Vec<Vec<u64>>,
    /// whole sessions: `goToSpawnPosition` calls, (car, set)
    spawns: Vec<(usize, String)>,
    /// whole sessions: the spawn sets' places and the game's functions the stand-ins call
    whole: Option<WholeContext>,
    /// whole sessions: the lap and sector events the cars raised
    laps: Vec<OnLapCompletedEvent>,
    splits: Vec<OnSectorSplitEvent>,
}

/// What the stand-ins need to act on the game's real cars.
#[derive(Clone, Default)]
struct WholeContext {
    /// a spawn set's slots as `mat44f`
    spawn_sets: Vec<(String, Vec<[f32; 16]>)>,
    get_spawn_index: usize,
    force_position: usize,
    on_new_session: usize,
}

thread_local! {
    static TOLD: RefCell<Told> = RefCell::new(Told::default());
}

fn car_index(avatar: *const u8) -> usize {
    TOLD.with(|t| t.borrow().avatars.iter().position(|a| *a == avatar as usize).unwrap_or(usize::MAX))
}

extern "C" fn ks_get_time() -> f64 {
    TOLD.with(|t| t.borrow().now)
}

extern "C" fn register_to_fade_in(_camera: *mut u8, function: *const u8, _owner: *mut u8) {
    unsafe {
        // std::function: three pointers of room, then the implementation's address
        let implementation: *const u64 = rd(function, 0x18);
        if implementation.is_null() {
            return;
        }
        let copy = [*implementation, *implementation.add(1), *implementation.add(2)];
        TOLD.with(|t| t.borrow_mut().fade_callable = Some(copy));
    }
}

extern "C" fn fade_in(camera: *mut u8, def: *const u8) {
    unsafe {
        let seconds: f32 = rd(def, 8);
        // the handler list is not empty while a fade runs
        wr(camera, CM_EV_ON_FADE_IN_FINISHED, 0x1000usize);
        wr(camera, CM_EV_ON_FADE_IN_FINISHED + 8, 0x1028usize);
        TOLD.with(|t| {
            let mut t = t.borrow_mut();
            t.fade_left = Some(seconds);
            t.log.push("FadeStarted".to_string());
        });
    }
}

extern "C" fn add_message(_messages: *mut u8, message: *const u8, description: *const u8, _kind: i32) {
    let line = unsafe { format!("Message({} | {})", text(message), text(description)) };
    TOLD.with(|t| t.borrow_mut().log.push(line));
}

extern "C" fn display_results(panel: *mut u8) {
    unsafe {
        if rd::<u8>(panel, 0x30) != 0 {
            return;
        }
        wr(panel, 0x30, 1u8);
    }
    let (first, lock) = TOLD.with(|t| {
        let mut t = t.borrow_mut();
        t.log.push("DisplayResults".to_string());
        (t.avatars.first().copied(), t.set_controls_lock)
    });
    if let Some(avatar) = first {
        let set_controls_lock: extern "C" fn(*mut u8, u8) = unsafe { std::mem::transmute(lock) };
        set_controls_lock(avatar as *mut u8, 1);
    }
}

extern "C" fn hide_results(panel: *mut u8) {
    unsafe {
        if rd::<u8>(panel, 0x30) == 0 {
            return;
        }
        wr(panel, 0x30, 0u8);
    }
    let (first, lock) = TOLD.with(|t| {
        let mut t = t.borrow_mut();
        t.log.push("HideResults".to_string());
        (t.avatars.first().copied(), t.set_controls_lock)
    });
    if let Some(avatar) = first {
        let set_controls_lock: extern "C" fn(*mut u8, u8) = unsafe { std::mem::transmute(lock) };
        set_controls_lock(avatar as *mut u8, 0);
    }
}

extern "C" fn get_compound(avatar: *const u8, out: *mut u8) -> *mut u8 {
    let index = car_index(avatar);
    let name = TOLD.with(|t| t.borrow().compounds.get(index).cloned().unwrap_or_default());
    unsafe { write_wstring(out, &name) };
    out
}

extern "C" fn execute_on_physics_thread(_physics_avatar: *mut u8, function: *const u8) {
    unsafe {
        let implementation: *const u64 = rd(function, 0x18);
        if implementation.is_null() {
            return;
        }
        let vtable = *implementation as usize;
        TOLD.with(|t| {
            let mut t = t.borrow_mut();
            let rva = vtable.wrapping_sub(t.image_base);
            t.jobs.push(rva);
            if t.keep_jobs {
                // (the callables of these jobs are at most six words)
                t.kept.push((0..6).map(|i| *implementation.add(i)).collect());
            }
        });
    }
}

extern "C" fn go_to_spawn(avatar: *mut u8, set: *const u8) {
    let index = car_index(avatar);
    let name = unsafe { text(set) };
    let whole = TOLD.with(|t| {
        let mut t = t.borrow_mut();
        t.log.push(format!("GoToSpawn({index}, {name})"));
        t.spawns.push((index, name.clone()));
        t.whole.clone()
    });
    // whole sessions: CarAvatar::goToSpawnPosition itself, with the port's reading of the
    // track's spawn nodes for TrackAvatar::getSpawnPosition
    if let Some(whole) = whole {
        unsafe {
            let get_spawn_index: extern "C" fn(*mut u8, *const u8) -> i32 = std::mem::transmute(whole.get_spawn_index);
            let slot = get_spawn_index(avatar.add(CA_RACE_INFO), set);
            let Some(matrix) = whole.spawn_sets.iter().find(|(set, _)| *set == name).and_then(|(_, slots)| slots.get(slot.max(0) as usize)) else {
                return;
            };
            let car: *mut u8 = rd(avatar, CA_PHYSICS);
            // FuelLapEvaluator::setIgnoreLap(true)
            wr(car, 0x3af8 + 0x40, 1u8);
            let force_position: extern "C" fn(*mut u8, *const [f32; 16]) = std::mem::transmute(whole.force_position);
            force_position(avatar, matrix);
        }
    }
}

/// `Car::evOnLapCompleted` and `Car::evOnSectorSplit`, heard by this file (whole sessions).
extern "C" fn on_car_lap(_this: *mut u8, event: *const u8) {
    let event = unsafe { OnLapCompletedEvent { car_index: rd(event, 0), lap_time: rd(event, 4), lap_count: rd(event, 8), splits: Vec::new(), event_time: rd(event, 0x28), is_valid: rd::<u8>(event, 0x30) != 0, cuts: rd(event, 0x34) } };
    TOLD.with(|t| t.borrow_mut().laps.push(event));
}

extern "C" fn on_car_split(_this: *mut u8, event: *const u8) {
    let event = unsafe { OnSectorSplitEvent { car_index: rd(event, 0), sector_index: rd(event, 4), sector_time: rd(event, 8), cuts: rd(event, 0xc) } };
    TOLD.with(|t| t.borrow_mut().splits.push(event));
}

/// `Sim::evOnNewSession`, heard by this file.
extern "C" fn on_new_session(_this: *mut u8, event: *const u8) {
    let line = unsafe { format!("NewSession({}, type {}, {:?})", rd::<i16>(event, 0x70), rd::<i32>(event, 0), text(event.add(0x38))) };
    let whole = TOLD.with(|t| {
        let mut t = t.borrow_mut();
        t.log.push(line);
        t.new_session = true;
        if t.whole.is_some() {
            // PhysicsAvatar's handler: a job for the physics thread with the session's
            // numbers (lambda @ 0x1401213f0); the first word 0 marks it among the kept jobs
            let (kind, start, minutes, laps, index): (i32, f64, f32, i32, i16) = unsafe { (rd(event, 0), rd(event, 0x58), rd(event, 0xc), rd(event, 8), rd(event, 0x70)) };
            let seconds = (minutes * 60.0) as f64;
            t.kept.push(vec![0, kind as u32 as u64, start.to_bits(), seconds.to_bits(), laps as u32 as u64, index as i32 as u32 as u64]);
        }
        t.whole.clone().map(|whole| (whole, t.avatars.clone()))
    });
    // every car's own handler (CarAvatar::onNewSession): to its place, the lap timer reset
    if let Some((whole, avatars)) = whole {
        let car_on_new_session: extern "C" fn(*mut u8) = unsafe { std::mem::transmute(whole.on_new_session) };
        for avatar in avatars {
            car_on_new_session(avatar as *mut u8);
        }
    }
}

unsafe fn vec_i32(at: *const u8) -> Vec<i32> {
    let (begin, end): (*const i32, *const i32) = (rd(at, 0), rd(at, 8));
    if begin.is_null() {
        return Vec::new();
    }
    std::slice::from_raw_parts(begin, end.offset_from(begin) as usize).to_vec()
}

unsafe fn vec_u32(at: *const u8) -> Vec<u32> {
    vec_i32(at).into_iter().map(|v| v as u32).collect()
}

/// A `Lap` (0x48 bytes) as text.
unsafe fn lap_text(lap: *const u8) -> String {
    format!("{}/{:?}/{}/{}/{}", rd::<u32>(lap, 0), vec_u32(lap.add(8)), rd::<u32>(lap, 0x20), rd::<u8>(lap, 0x24), text(lap.add(0x28)))
}

unsafe fn laps_text(at: *const u8) -> String {
    let (begin, end): (*const u8, *const u8) = (rd(at, 0), rd(at, 8));
    let mut out = Vec::new();
    let mut lap = begin;
    while !lap.is_null() && lap < end {
        out.push(lap_text(lap));
        lap = lap.add(0x48);
    }
    out.join(" ")
}

/// `RaceManager::evOnSessionEnd`, heard by this file (after the game's own handlers).
extern "C" fn on_session_end(_this: *mut u8, event: *const u8) {
    let line = unsafe { format!("SessionEnd({})", session_end_text(event)) };
    TOLD.with(|t| t.borrow_mut().log.push(line));
}

unsafe fn session_end_text(event: *const u8) -> String {
    let result = event.add(0x70);
    let (begin, end): (*const u8, *const u8) = (rd(result, 0), rd(result, 8));
    let mut laps = Vec::new();
    let mut car = begin;
    while !car.is_null() && car < end {
        laps.push(format!("[{}]", laps_text(car)));
        car = car.add(0x18);
    }
    let total: Vec<String> = vec_u32(result.add(0x60)).iter().map(|bits| format!("{bits:08x}")).collect();
    format!("type {} positions {:?} lapCount {:?} total {:?} best [{}] laps {}", rd::<i32>(event, 0), vec_i32(result.add(0x30)), vec_i32(result.add(0x48)), total, laps_text(result.add(0x18)), laps.join(""))
}

/// The game's referee and what it stands in.
pub struct GameReferee<'a> {
    pub acs: &'a Acs,
    pub game: *mut u8,
    pub sim: *mut u8,
    pub manager: *mut u8,
    pub timing: *mut u8,
    pub avatars: Vec<*mut u8>,
    /// the physics car of every avatar (a zeroed block unless real cars are given)
    pub cars: Vec<*mut u8>,
    camera: *mut u8,
    panel: *mut u8,
    physics_avatar: *mut u8,
    json: *mut u8,
    start_key: *mut u8,
}

/// One car as the referee's harness is told it.
#[derive(Clone, Debug)]
pub struct CarSetup {
    pub name: String,
    pub car: String,
    pub skin: String,
    pub compound: String,
    /// its slot of the grid at the beginning
    pub start_index: i32,
}

impl<'a> GameReferee<'a> {
    /// `real_cars`: the game's `Car` of every car (whole sessions), else blocks of zeros.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn build(acs: &'a Acs, ini: &SessionsIni, cars: &[CarSetup], track_name: &str, track_config: &str, track_length: f32, sectors: usize, over_time_mult: f32, now: f64, real_cars: Option<(&[*mut u8], &rustyac_physics::track::Track)>) -> GameReferee<'a> {
        let stub = |va: usize, target: usize| {
            let mut code = vec![0x48, 0xb8];
            code.extend_from_slice(&(target as u64).to_le_bytes());
            code.extend_from_slice(&[0xff, 0xe0]);
            acs.patch(acs.va(va), &code);
        };
        stub(VA_KS_GET_TIME, ks_get_time as *const () as usize);
        stub(VA_REGISTER_TO_FADE_IN, register_to_fade_in as *const () as usize);
        stub(VA_FADE_IN, fade_in as *const () as usize);
        stub(VA_ADD_MESSAGE, add_message as *const () as usize);
        stub(VA_DISPLAY_RESULTS, display_results as *const () as usize);
        stub(VA_HIDE_RESULTS, hide_results as *const () as usize);
        stub(VA_GET_COMPOUND, get_compound as *const () as usize);
        stub(VA_EXECUTE_ON_PHYSICS_THREAD, execute_on_physics_thread as *const () as usize);
        stub(VA_GO_TO_SPAWN, go_to_spawn as *const () as usize);
        let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(acs.crt_function(c"??2@YAPEAX_K@Z"));

        let game = zeroed(0x278);
        wr(game, 0x18, now);
        let sim = zeroed(0x2d0);
        wr(sim, 0, acs.va(VA_SIM_VTABLE));
        wr(sim, 8, game);
        let manager = zeroed(0x218);
        wr(manager, 0, acs.va(VA_RM_VTABLE));
        wr(manager, 8, game);
        wr(manager, 0x30, 1u8);
        let track = zeroed(0x308);
        write_wstring(track.add(0x10), track_name);
        write_wstring(track.add(TA_CONFIG), track_config);
        wr(track, TA_LENGTH, track_length);
        let sector_list = Box::leak(vec![0f32; sectors.max(1)].into_boxed_slice()).as_mut_ptr();
        wr(track, TA_SECTORS, sector_list);
        wr(track, TA_SECTORS + 8, sector_list.add(sectors));
        wr(track, TA_SECTORS + 16, sector_list.add(sectors));
        let camera = zeroed(0x198);
        let panel = zeroed(0x130);
        let physics_avatar = zeroed(0x8000);
        wr(sim, SIM_TRACK, track);
        wr(sim, SIM_CAMERA_MANAGER, camera);
        wr(sim, SIM_SYSTEM_MESSAGE, zeroed(0x100));
        wr(sim, SIM_RACE_MANAGER, manager);
        wr(sim, SIM_REPLAY_MANAGER, zeroed(0x198));
        wr(sim, SIM_PHYSICS_AVATAR, physics_avatar);
        wr(sim, SIM_END_SESSION_DISPLAYER, panel);
        // OptionsManager::options: an empty std::map (its head node, from the game's heap)
        let head = new(0x48);
        std::ptr::write_bytes(head, 0, 0x48);
        wr(head, 0, head);
        wr(head, 8, head);
        wr(head, 0x10, head);
        wr(head, 0x18, 1u8);
        wr(head, 0x19, 1u8);
        wr(sim, SIM_OPTIONS_MAP, head);

        // the manager as its constructor and loadSessions leave it
        wr(manager, 0xa0, ini.penalties_enabled as u8);
        wr(manager, 0xa2, ini.fixed_setup as u8);
        wr(manager, 0xa4, -1i32);
        wr(manager, 0xa8, over_time_mult);
        wr(manager, 0xae, ini.has_additional_lap as u8);
        wr(manager, RM_SIM, sim);
        let session_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SESSION_CTOR));
        session_ctor(manager.add(RM_CURRENT_SESSION));
        let count = ini.sessions.len();
        let records = zeroed(0x70 * count);
        for (i, session) in ini.sessions.iter().enumerate() {
            let record = records.add(0x70 * i);
            session_ctor(record);
            wr(record, 0, session.session_type);
            wr(record, 4, session.is_timed_race as u8);
            wr(record, 5, session.has_additional_lap as u8);
            wr(record, 8, session.laps);
            wr(record, 0xc, session.duration_minutes);
            wr(record, 0x10, session.overtime_ms);
            write_wstring(record.add(0x18), &session.spawn_set);
            write_wstring(record.add(0x38), &session.name);
            wr(record, 0x58, session.start_time);
            wr(record, 0x60, session.forced_position);
        }
        wr(manager, RM_SESSIONS, records);
        wr(manager, RM_SESSIONS + 8, records.add(0x70 * count));
        wr(manager, RM_SESSIONS + 16, records.add(0x70 * count));

        // the cars
        let n = cars.len();
        let list = zeroed(8 * n);
        wr(sim, SIM_CARS, list);
        wr(sim, SIM_CARS + 8, list.add(8 * n));
        wr(sim, SIM_CARS + 16, list.add(8 * n));
        let start_key = zeroed(0x20);
        write_wstring(start_key, "START");
        let info_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_RACE_INFO_CTOR));
        let info_init: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_CAR_RACE_INFO_INIT));
        let info_set: extern "C" fn(*mut u8, *const u8, i32) = std::mem::transmute(acs.va(VA_CAR_RACE_INFO_SET));
        let mut avatars = Vec::new();
        let mut physics = Vec::new();
        let connected = zeroed(0x100);
        wr(connected, 0xd8, 1u8);
        for (i, setup) in cars.iter().enumerate() {
            let avatar = zeroed(0x12a8);
            wr(avatar, 0, acs.va(VA_CAR_AVATAR_VTABLE));
            wr(avatar, 8, game);
            wr(avatar, CA_SIM, sim);
            wr(avatar, CA_GUID, i as i32);
            wr(avatar, CA_NET, connected);
            write_wstring(avatar.add(CA_UNIX_NAME), &setup.car);
            write_wstring(avatar.add(CA_GUI_NAME), &setup.car);
            write_wstring(avatar.add(CA_DRIVER_NAME), &setup.name);
            write_wstring(avatar.add(CA_CURRENT_SKIN), &setup.skin);
            let car = match real_cars {
                Some((real, track)) => {
                    // the car's pit box (CarAvatar::pitPosition: its slot of the PIT set)
                    if let Some(node) = track.spawn_positions.get("PIT").and_then(|slots| slots.get(i)) {
                        wr(avatar, CA_PIT_POSITION, flat(&track.helper_nodes[*node].local.m));
                    }
                    real[i]
                }
                None => zeroed(0x4800),
            };
            let engineer = zeroed(0x40);
            wr(engineer, 8, car);
            wr(avatar, CA_RACE_ENGINEER, engineer);
            wr(avatar, CA_PHYSICS, car);
            wr(list, 8 * i, avatar);
            avatars.push(avatar);
            physics.push(car);
        }
        TOLD.with(|t| {
            *t.borrow_mut() = Told {
                now,
                avatars: avatars.iter().map(|a| *a as usize).collect(),
                compounds: cars.iter().map(|c| c.compound.clone()).collect(),
                camera: camera as usize,
                image_base: acs.va(0x1_4000_0000),
                set_controls_lock: acs.va(VA_CAR_SET_CONTROLS_LOCK),
                keep_jobs: real_cars.is_some(),
                whole: real_cars.map(|(_, track)| WholeContext {
                    spawn_sets: track.spawn_positions.iter().map(|(set, slots)| (set.clone(), slots.iter().map(|node| flat(&track.helper_nodes[*node].local.m)).collect())).collect(),
                    get_spawn_index: acs.va(VA_CAR_GET_SPAWN_INDEX),
                    force_position: acs.va(VA_CAR_AVATAR_FORCE_POSITION),
                    on_new_session: acs.va(VA_CAR_AVATAR_ON_NEW_SESSION),
                }),
                ..Told::default()
            };
        });
        for (i, avatar) in avatars.iter().enumerate() {
            let info = avatar.add(CA_RACE_INFO);
            info_ctor(info);
            info_init(info, *avatar);
            info_set(info, start_key, cars[i].start_index);
            // (Sim::addCar: the car's own number in the pits)
            let pit_key = zeroed(0x20);
            write_wstring(pit_key, "PIT");
            info_set(info, pit_key, i as i32);
        }
        // loadSessions: one record per car in both lists, one leaderboard per session
        let status = zeroed(0x18 * n);
        let real_time = zeroed(0x10 * n);
        for (i, avatar) in avatars.iter().enumerate() {
            let s = status.add(0x18 * i);
            wr(s, 0, *avatar);
            wr(s, 8, 5.0f32);
            wr(s, 0x14, 5.0f32);
            let r = real_time.add(0x10 * i);
            wr(r, 0, *avatar);
            wr(r, 0xc, -1.0f32);
        }
        wr(manager, RM_RACE_STATUS, status);
        wr(manager, RM_RACE_STATUS + 8, status.add(0x18 * n));
        wr(manager, RM_RACE_STATUS + 16, status.add(0x18 * n));
        wr(manager, RM_REAL_TIME, real_time);
        wr(manager, RM_REAL_TIME + 8, real_time.add(0x10 * n));
        wr(manager, RM_REAL_TIME + 16, real_time.add(0x10 * n));
        let history = zeroed(0x18 * count);
        wr(manager, RM_LEADERBOARD_HISTORY, history);
        wr(manager, RM_LEADERBOARD_HISTORY + 8, history.add(0x18 * count));
        wr(manager, RM_LEADERBOARD_HISTORY + 16, history.add(0x18 * count));

        // initOffline from the timing services on
        let timing = new(0xc0);
        std::ptr::write_bytes(timing, 0, 0xc0);
        let timing_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_RTS_CTOR));
        timing_ctor(timing, sim);
        wr(manager, RM_RTS, timing);
        let json = new(0xc0);
        std::ptr::write_bytes(json, 0, 0xc0);
        let json_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_JSON_CTOR));
        json_ctor(json, sim);
        let referee = GameReferee { acs, game, sim, manager, timing, avatars, cars: physics, camera, panel, physics_avatar, json, start_key };
        referee.add_handler(sim.add(SIM_EV_ON_NEW_SESSION), on_new_session as *const () as usize);
        referee.add_handler(manager.add(RM_EV_ON_SESSION_END), on_session_end as *const () as usize);
        let set_current_session: extern "C" fn(*mut u8, u32) = std::mem::transmute(acs.va(VA_RM_SET_CURRENT_SESSION));
        set_current_session(manager, 0);
        let reset_laps: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_RM_RESET_CURRENT_LAPS));
        reset_laps(manager);
        let reset_pit: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_RM_RESET_MANDATORY_PIT));
        reset_pit(manager);
        referee
    }

    /// One more handler at the end of an `Event<T>` (a vector of 0x28-byte records: an owner
    /// and a `std::function`, whose implementation's `_Do_call` is the third virtual).
    unsafe fn add_handler(&self, event: *mut u8, handler: usize) {
        let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(self.acs.crt_function(c"??2@YAPEAX_K@Z"));
        let delete: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.crt_function(c"??3@YAXPEAX@Z"));
        let (begin, end): (*mut u8, *mut u8) = (rd(event, 0), rd(event, 8));
        let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / 0x28 };
        let buffer = new(0x28 * (count + 4));
        std::ptr::write_bytes(buffer, 0, 0x28 * (count + 4));
        for i in 0..count {
            let (from, to) = (begin.add(0x28 * i), buffer.add(0x28 * i));
            std::ptr::copy_nonoverlapping(from, to, 0x28);
            // an implementation that lives in the function's own room moves with it
            if rd::<usize>(from, 0x20) == from as usize + 8 {
                wr(to, 0x20, to as usize + 8);
            }
        }
        let table = Box::leak(vec![0usize; 6].into_boxed_slice());
        table[2] = handler;
        let implementation = Box::leak(vec![table.as_ptr() as usize; 2].into_boxed_slice());
        let record = buffer.add(0x28 * count);
        wr(record, 0x20, implementation.as_ptr());
        if !begin.is_null() {
            delete(begin);
        }
        wr(event, 0, buffer);
        wr(event, 8, buffer.add(0x28 * (count + 1)));
        wr(event, 16, buffer.add(0x28 * (count + 4)));
    }

    pub unsafe fn set_now(&self, now: f64) {
        wr(self.game, 0x18, now);
        TOLD.with(|t| t.borrow_mut().now = now);
    }

    pub unsafe fn start_race(&self, delay_ms: f32) {
        let start_race: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_RM_START_RACE));
        start_race(self.manager, delay_ms);
    }

    /// What the manager reads of a car, written into its avatar.
    pub unsafe fn set_car(&self, index: usize, view: &CarView, speed: f32) {
        let avatar = self.avatars[index];
        wr(avatar, CA_NPOS, view.normalized_spline_position);
        // CarAvatar::isInPit: the distance to the pit box (the origin here)
        wr(avatar, CA_POSITION, [if view.is_in_pit { 0.0f32 } else { 100.0 }, 0.0, 0.0]);
        wr(avatar, CA_PIT_POSITION + 0x30, [0.0f32; 3]);
        wr(avatar, CA_SPEED, speed);
        wr(avatar, CA_FILTERED_SPEED, view.filtered_speed);
        wr(avatar, CA_STATE_RETIRED, view.is_retired.unwrap_or(false) as u8);
        wr(avatar, CA_IS_BLACK_FLAGGED, view.is_black_flagged as u8);
    }

    /// The same for a made-up car: its "physics" is retired or not.
    pub unsafe fn set_fake_car(&self, index: usize, view: &CarView) {
        wr(self.cars[index], CAR_IS_RETIRED, view.is_retired.unwrap_or(false) as u8);
    }

    pub unsafe fn sector_split(&self, event: &OnSectorSplitEvent) {
        let raw = [event.car_index, event.sector_index as u32, event.sector_time, event.cuts as u32];
        let on_sector_split: extern "C" fn(*mut u8, *const u32) = std::mem::transmute(self.acs.va(VA_RTS_ON_SECTOR_SPLIT));
        on_sector_split(self.timing, raw.as_ptr());
    }

    pub unsafe fn lap_completed(&self, event: &OnLapCompletedEvent) {
        let raw = zeroed(0x38);
        wr(raw, 0, event.car_index);
        wr(raw, 4, event.lap_time);
        wr(raw, 8, event.lap_count);
        wr(raw, 0x28, event.event_time);
        wr(raw, 0x30, event.is_valid as u8);
        wr(raw, 0x34, event.cuts);
        let on_lap_completed: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(self.acs.va(VA_RM_ON_LAP_COMPLETED));
        on_lap_completed(self.manager, raw);
    }

    /// One frame of the main thread: the camera's fade (counted here as the port counts
    /// it), `RaceTimingServices::update`, `RaceManager::update`.
    pub unsafe fn update(&self, dt: f32) {
        let due = TOLD.with(|t| {
            let mut t = t.borrow_mut();
            match t.fade_left {
                Some(left) if left - dt <= 0.0 => {
                    t.fade_left = None;
                    t.fade_callable.take()
                }
                Some(left) => {
                    t.fade_left = Some(left - dt);
                    None
                }
                None => None,
            }
        });
        if let Some(mut callable) = due {
            wr(self.camera, CM_EV_ON_FADE_IN_FINISHED, 0usize);
            wr(self.camera, CM_EV_ON_FADE_IN_FINISHED + 8, 0usize);
            let vtable = callable[0] as *const usize;
            let do_call: extern "C" fn(*mut u64, *const u8) = std::mem::transmute(*vtable.add(2));
            let flag = 0u8;
            do_call(callable.as_mut_ptr(), &flag);
        }
        let update_leaderboard: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_RTS_UPDATE_LEADERBOARD));
        update_leaderboard(self.timing);
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_RM_UPDATE));
        update(self.manager, dt);
    }

    pub unsafe fn skip(&self) {
        let skip: extern "C" fn(*mut u8) -> u8 = std::mem::transmute(self.acs.va(VA_RM_SKIP));
        skip(self.manager);
    }

    pub unsafe fn restart(&self) {
        let restart: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_RM_RESTART));
        restart(self.manager);
    }

    /// `RaceManager::shutdown` (the last session's results) and `Sim::writeOutputJson`: the
    /// bytes of the `race_out.json` the game's own writer leaves in the scratch root.
    pub unsafe fn shutdown(&self) -> Result<Vec<u8>, String> {
        let shutdown: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_RM_SHUTDOWN));
        shutdown(self.manager);
        let folder = std::path::Path::new("oracle_no_documents/Assetto Corsa/out");
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let file = folder.join("race_out.json");
        let _ = std::fs::remove_file(&file);
        let write: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_JSON_WRITE));
        write(self.json);
        std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// What the stand-ins were told since the last call, and the posted jobs (the address of
    /// each job's implementation table).
    pub fn take_told(&self) -> (Vec<String>, Vec<usize>, bool) {
        TOLD.with(|t| {
            let mut t = t.borrow_mut();
            (std::mem::take(&mut t.log), std::mem::take(&mut t.jobs), std::mem::take(&mut t.new_session))
        })
    }

    /// Whole sessions: the jobs posted for the physics thread, and the cars sent to a spawn set.
    pub fn take_work(&self) -> (Vec<Vec<u64>>, Vec<(usize, String)>) {
        TOLD.with(|t| {
            let mut t = t.borrow_mut();
            (std::mem::take(&mut t.kept), std::mem::take(&mut t.spawns))
        })
    }

    pub fn physics_avatar(&self) -> *mut u8 {
        self.physics_avatar
    }

    pub unsafe fn results_displayed(&self) -> bool {
        rd::<u8>(self.panel, 0x30) != 0
    }

    unsafe fn guid(&self, avatar: *const u8) -> i32 {
        if avatar.is_null() {
            -1
        } else {
            rd(avatar, CA_GUID)
        }
    }

    /// Every field of the referee's state, named.
    pub unsafe fn dump(&self) -> Vec<(String, String)> {
        let m = self.manager;
        let mut out = Vec::new();
        let mut put = |key: &str, value: String| out.push((key.to_string(), value));
        put("currentSessionIndex", rd::<i32>(m, 0xa4).to_string());
        put("flags", format!("{} {} {}", rd::<u8>(m, 0xac), rd::<u8>(m, 0xad), rd::<u8>(m, 0xae)));
        put("pitWindow", format!("{:08x} {:08x}", rd::<u32>(m, 0xb0), rd::<u32>(m, 0xb4)));
        let s = m.add(RM_CURRENT_SESSION);
        put(
            "session",
            format!(
                "type {} timed {} laps {} duration {:08x} overtime {:08x} spawn {:?} name {:?} start {:016x} forced {} over {} leaderLaps {}",
                rd::<i32>(s, 0),
                rd::<u8>(s, 4),
                rd::<i32>(s, 8),
                rd::<u32>(s, 0xc),
                rd::<u32>(s, 0x10),
                text(s.add(0x18)),
                text(s.add(0x38)),
                rd::<u64>(s, 0x58),
                rd::<i32>(s, 0x60),
                rd::<u8>(s, 0x64),
                rd::<u32>(s, 0x68)
            ),
        );
        put("penaltyMode", rd::<i32>(self.physics_avatar, PA_PENALTY_MODE).to_string());
        let (begin, end): (*const u8, *const u8) = (rd(m, RM_REAL_TIME), rd(m, RM_REAL_TIME + 8));
        let mut list = Vec::new();
        let mut at = begin;
        while at < end {
            list.push(format!("{}:{}:{:08x}", self.guid(rd(at, 0)), rd::<u8>(at, 8), rd::<u32>(at, 0xc)));
            at = at.add(0x10);
        }
        put("realTime", list.join(" "));
        let (begin, end): (*const u8, *const u8) = (rd(m, RM_RACE_STATUS), rd(m, RM_RACE_STATUS + 8));
        let mut list = Vec::new();
        let mut at = begin;
        while at < end {
            list.push(format!("{}:{:08x}:{}:{:08x}", self.guid(rd(at, 0)), rd::<u32>(at, 8), rd::<u8>(at, 0xc), rd::<u32>(at, 0x10)));
            at = at.add(0x18);
        }
        put("raceStatus", list.join(" "));
        let t = self.timing;
        let (begin, end): (*const *const u8, *const *const u8) = (rd(t, 0x58), rd(t, 0x60));
        let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize };
        for i in 0..count {
            let db = *begin.add(i);
            let car = self.guid(rd(db, 0x68));
            put(
                &format!("lapDB {car}"),
                format!(
                    "total {:016x} best {} lastStart {} current {:?} personal {:?} bestLapSplit {} completed {} laps {}",
                    rd::<u64>(db, 0),
                    lap_text(db.add(0x20)),
                    rd::<u32>(db, 0x70),
                    vec_u32(db.add(0x78)),
                    vec_u32(db.add(0x90)),
                    rd::<i32>(db, 0xa8),
                    rd::<u8>(db, 0xac),
                    laps_text(db.add(8))
                ),
            );
        }
        put("bestSplits", format!("{:?} global {}", vec_u32(t.add(0xa0)), rd::<u32>(t, 0xb8)));
        let (begin, end): (*const u8, *const u8) = (rd(t, 0x70), rd(t, 0x78));
        let mut list = Vec::new();
        let mut at = begin;
        while !at.is_null() && at < end {
            list.push(format!("{}:{:016x}:{:016x}:{}:{}{}{}", self.guid(rd(at, 0)), rd::<u64>(at, 8), rd::<u64>(at, 0x10), rd::<i32>(at, 0x18), rd::<u8>(at, 0x1c), rd::<u8>(at, 0x1d), rd::<u8>(at, 0x1e)));
            at = at.add(0x20);
        }
        put("leaderboard", list.join(" "));
        let get_spawn_index: extern "C" fn(*mut u8, *const u8) -> i32 = std::mem::transmute(self.acs.va(VA_CAR_GET_SPAWN_INDEX));
        put("grid", self.avatars.iter().map(|avatar| get_spawn_index(avatar.add(CA_RACE_INFO), self.start_key).to_string()).collect::<Vec<_>>().join(" "));
        put("resultsDisplayed", (rd::<u8>(self.panel, 0x30)).to_string());
        let (begin, end): (*const u8, *const u8) = (rd(m, RM_SESSION_HISTORY), rd(m, RM_SESSION_HISTORY + 8));
        let mut at = begin;
        let mut i = 0;
        while !at.is_null() && at < end {
            put(&format!("history {i}"), session_end_text(at));
            at = at.add(0xe8);
            i += 1;
        }
        let (begin, end): (*const u8, *const u8) = (rd(m, RM_LEADERBOARD_HISTORY), rd(m, RM_LEADERBOARD_HISTORY + 8));
        let mut board = begin;
        let mut i = 0;
        while !board.is_null() && board < end {
            let (first, last): (*const u8, *const u8) = (rd(board, 0), rd(board, 8));
            let mut list = Vec::new();
            let mut at = first;
            while !at.is_null() && at < last {
                list.push(format!("{}:{:016x}:{:016x}:{}:{}{}{}", self.guid(rd(at, 0)), rd::<u64>(at, 8), rd::<u64>(at, 0x10), rd::<i32>(at, 0x18), rd::<u8>(at, 0x1c), rd::<u8>(at, 0x1d), rd::<u8>(at, 0x1e)));
                at = at.add(0x20);
            }
            put(&format!("leaderboardHistory {i}"), list.join(" "));
            board = board.add(0x18);
            i += 1;
        }
        out
    }
}

fn flat(m: &[[f32; 4]; 4]) -> [f32; 16] {
    let mut out = [0f32; 16];
    for r in 0..4 {
        out[4 * r..4 * r + 4].copy_from_slice(&m[r]);
    }
    out
}

const VA_CAR_AVATAR_FORCE_POSITION: usize = 0x1_400d_2f30; // CarAvatar::forcePosition(const mat44f&)
const VA_CAR_AVATAR_ON_NEW_SESSION: usize = 0x1_400d_91c0; // CarAvatar::onNewSession()
const VA_PE_SET_SESSION_START_TIME: usize = 0x1_4026_4590; // PhysicsEngine::setCurrentSessionStartTime(double)
const VA_PE_SET_SESSION_INFO: usize = 0x1_4026_4560; // PhysicsEngine::setSessionInfo(const SessionInfo&)
/// the implementation tables of two jobs that act on the engine inside `PhysicsAvatar`
const VA_JOB_SESSION_START_TIME: usize = 0x1_404c_bf78; // lambda @ 0x140135220
const CAR_EV_ON_LAP_COMPLETED: usize = 0x40;
const CAR_EV_ON_SECTOR_SPLIT: usize = 0x58;
const PE_EV_ON_NEW_SESSION_PHYSICS: usize = 0xa0;
const PE_JUMP_START_PENALTY_MODE: usize = 0xd8;
const PE_LOCK_GEARBOX_MS: usize = 0xc0;

std::thread_local! {
    /// The referee's own `rand()` (the game's main thread has one of its own; here both
    /// threads' code runs on one, so the game's `rand` import is answered from this stream
    /// while the referee's code runs).
    static REFEREE_RAND: std::cell::Cell<Option<MsvcRand>> = const { std::cell::Cell::new(None) };
    static REFEREE_RAND_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CRT_RAND: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where the C runtime's own `rand` is: asked once the game is loaded, before any of its
/// code runs.
pub fn init_rand(acs: &Acs) {
    CRT_RAND.with(|c| c.set(acs.crt_function(c"rand")));
}

/// The game's `rand` import in a whole session (see [`REFEREE_RAND`]).
pub extern "C" fn game_rand() -> i32 {
    if REFEREE_RAND_ON.with(|on| on.get()) {
        if let Some(mut rand) = REFEREE_RAND.with(|r| r.get()) {
            let value = rand.next();
            REFEREE_RAND.with(|r| r.set(Some(rand)));
            return value;
        }
    }
    let crt: extern "C" fn() -> i32 = unsafe { std::mem::transmute(CRT_RAND.with(|c| c.get())) };
    crt()
}

/// The game's referee among the game's real cars.
pub struct WholeReferee<'a> {
    pub game: GameReferee<'a>,
    engine: *mut u8,
    step_in_frame: u32,
    filtered: Vec<f32>,
    next_start_delay_ms: f32,
    /// the hash of the referee's state after its last frame
    pub state_hash: u64,
    pub state: Vec<(String, String)>,
    pub frames: u64,
    /// what the stand-ins were told, frame by frame (for a look)
    pub log: Vec<(u64, String)>,
    /// what the player's car went through, in frames: a time penalty running, a drive-through
    /// pending, controls locked by a time, black-flagged; and the most cuts in a lap
    pub seen: [u64; 5],
    /// Task 28b: the game's pit stop of the first car
    pub pit: Option<super::pit::GamePit<'a>>,
    /// frames in which the stop's countdown ran, and in which the mandatory stop was done
    pub pit_seen: [u64; 2],
}

impl<'a> WholeReferee<'a> {
    /// The referee of `setup` for the cars of `engine`, as `RaceManager::initOffline` and
    /// `Sim::Sim` leave it. The first jobs run before the first step.
    pub unsafe fn build(acs: &'a Acs, engine: *mut u8, cars: &[*mut u8], track: &rustyac_physics::track::Track, setup: &rustyac_physics::race::RefereeSetup, compounds: &[String], now: f64) -> WholeReferee<'a> {
        CRT_RAND.with(|c| c.set(acs.crt_function(c"rand")));
        REFEREE_RAND.with(|r| r.set(Some(MsvcRand(setup.seed))));
        // the rules race.ini gives the engine
        wr(engine, PE_ALLOWED_TYRES_OUT, setup.sessions.allowed_tyres_out());
        if let Some(mode) = setup.sessions.jump_start_penalty_mode {
            wr(engine, PE_JUMP_START_PENALTY_MODE, mode);
        }
        let car_setups: Vec<CarSetup> = setup.players.iter().enumerate().map(|(i, p)| CarSetup { name: p.name.clone(), car: p.car.clone(), skin: p.skin.clone(), compound: compounds.get(i).cloned().unwrap_or_default(), start_index: setup.start_index[i] }).collect();
        let length = track.ai_spline.as_ref().map_or(0.0, |spline| spline.length());
        REFEREE_RAND_ON.with(|on| on.set(true));
        let game = GameReferee::build(acs, &setup.sessions, &car_setups, &track.name, &setup.layout, length, track.sectors_normalized_positions.len(), setup.over_time_mult, now, Some((cars, track)));
        // every car's lap and sector events come to this file
        for car in cars {
            game.add_handler(car.add(CAR_EV_ON_LAP_COMPLETED), on_car_lap as *const () as usize);
            game.add_handler(car.add(CAR_EV_ON_SECTOR_SPLIT), on_car_split as *const () as usize);
        }
        game.start_race(setup.first_start_delay_ms);
        REFEREE_RAND_ON.with(|on| on.set(false));
        let mut referee = WholeReferee { game, engine, step_in_frame: 0, filtered: vec![0.0; cars.len()], next_start_delay_ms: setup.next_start_delay_ms, state_hash: 0, state: Vec::new(), frames: 0, log: Vec::new(), seen: [0; 5], pit: None, pit_seen: [0; 2] };
        referee.note();
        referee
    }

    /// Task 28b: the game's `PitStop` and `QuickMenuPitstop` join, for the first car's pit
    /// box, with the menu as `menu` has it and this mandatory pit window.
    pub unsafe fn enable_pit(&mut self, pit_position: [f32; 3], menu: &rustyac_physics::race::pitstop::QuickMenu, window: (u16, u16)) {
        let pit = super::pit::GamePit::build(self.game.acs, self.game.sim, self.game.game, self.game.manager, self.game.avatars[0], self.game.cars[0], pit_position, menu, window);
        self.game.add_handler(self.game.sim.add(SIM_EV_ON_NEW_SESSION), super::pit::GamePit::new_session_handler());
        self.pit = Some(pit);
    }

    fn note(&mut self) {
        let (log, _, _) = self.game.take_told();
        let frame = self.frames;
        self.log.extend(log.into_iter().map(|line| (frame, line)));
    }

    /// The jobs the main thread posted, at the start of a physics step, in their order.
    pub unsafe fn before_step(&mut self) {
        let (jobs, _) = self.game.take_work();
        let acs = self.game.acs;
        for mut job in jobs {
            if job[0] == 0 {
                // the new session: PhysicsEngine::setSessionInfo, the engine's handlers of
                // its new-session event, and no contacts for 250 steps
                let mut info = [0u8; 32];
                info[0..4].copy_from_slice(&(job[1] as u32).to_le_bytes());
                info[8..16].copy_from_slice(&job[2].to_le_bytes());
                info[16..24].copy_from_slice(&job[3].to_le_bytes());
                info[24..28].copy_from_slice(&(job[4] as u32).to_le_bytes());
                info[28..32].copy_from_slice(&(job[5] as u32).to_le_bytes());
                let set_session_info: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_PE_SET_SESSION_INFO));
                set_session_info(self.engine, info.as_ptr());
                let begin: *const u8 = rd(self.engine, PE_EV_ON_NEW_SESSION_PHYSICS);
                let end: *const u8 = rd(self.engine, PE_EV_ON_NEW_SESSION_PHYSICS + 8);
                let mut handler = begin;
                while handler < end {
                    let function: *mut u8 = rd(handler, 0x20);
                    if !function.is_null() {
                        let vtable: *const usize = rd(function, 0);
                        let do_call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*vtable.add(2));
                        do_call(function, info.as_ptr());
                    }
                    handler = handler.add(0x28);
                }
                let core: *mut u8 = rd(self.engine, PE_CORE);
                let vtable: *const usize = rd(core, 0);
                let set_no_collision: extern "C" fn(*mut u8, i32) = std::mem::transmute(*vtable.add(0x98 / 8));
                set_no_collision(core, 0xfa);
            } else if job[0] as usize == acs.va(VA_JOB_SESSION_START_TIME) {
                // (its own body would write into the stand-in for PhysicsAvatar's engine)
                let start: f64 = rd(self.game.manager, RM_CURRENT_SESSION + 0x58);
                let set_start: extern "C" fn(*mut u8, f64) = std::mem::transmute(acs.va(VA_PE_SET_SESSION_START_TIME));
                set_start(self.engine, start);
            } else {
                let vtable = job[0] as *const usize;
                let do_call: extern "C" fn(*mut u64) = std::mem::transmute(*vtable.add(2));
                do_call(job.as_mut_ptr());
            }
        }
        // RaceManager::setCurrentSession writes the mode straight into the engine
        wr(self.engine, PE_PENALTY_MODE, rd::<i32>(self.game.physics_avatar(), PA_PENALTY_MODE));
    }

    /// After a physics step at clock `now`: every third step a frame of the main thread.
    pub unsafe fn after_step(&mut self, now: f64) {
        self.step_in_frame += 1;
        if self.step_in_frame < 3 {
            return;
        }
        self.step_in_frame = 0;
        self.frames += 1;
        const DT: f32 = 0.009;
        let acs = self.game.acs;
        self.game.set_now(now);
        let get_physics_state: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_CAR_GET_PHYSICS_STATE));
        for (index, avatar) in self.game.avatars.iter().enumerate() {
            let car = self.game.cars[index];
            // CarAvatar::update: the car's state, its filtered speed
            get_physics_state(car, avatar.add(0x268));
            let speed: f32 = rd(*avatar, CA_SPEED);
            self.filtered[index] = filter_speed(self.filtered[index], speed, DT);
            wr(*avatar, CA_FILTERED_SPEED, self.filtered[index]);
            if index == 0 {
                // (FlagManager's handler of the engine's flag event, for the player's car)
                wr(*avatar, CA_IS_BLACK_FLAGGED, rd::<u8>(car, 0x3d20));
            }
        }
        if let Some(pit) = &self.pit {
            // (CarAvatar::getCurrentCompoundShortName, after a stop's new tyres)
            if let Some(name) = super::pit::current_compound(self.game.cars[0]) {
                TOLD.with(|t| {
                    if let Some(slot) = t.borrow_mut().compounds.get_mut(0) {
                        *slot = name;
                    }
                });
            }
            pit.before_manager(DT);
        }
        let (laps, splits) = TOLD.with(|t| {
            let mut t = t.borrow_mut();
            (std::mem::take(&mut t.laps), std::mem::take(&mut t.splits))
        });
        REFEREE_RAND_ON.with(|on| on.set(true));
        for split in &splits {
            self.game.sector_split(split);
        }
        for lap in &laps {
            self.game.lap_completed(lap);
        }
        self.game.update(DT);
        let (log, _, new_session) = self.game.take_told();
        let frame = self.frames;
        self.log.extend(log.into_iter().map(|line| (frame, line)));
        if new_session && rd::<i32>(self.game.manager, RM_CURRENT_SESSION) == SESSION_RACE {
            self.game.start_race(self.next_start_delay_ms);
        }
        if let Some(pit) = &self.pit {
            pit.update(DT, now);
            self.pit_seen[0] += (rd::<i32>(pit.pit, 0x120) == 3) as u64;
            self.pit_seen[1] += rd::<u8>(self.game.avatars[0], 0x1295) as u64;
        }
        REFEREE_RAND_ON.with(|on| on.set(false));
        self.note();
        if let Some(car) = self.game.cars.first() {
            self.seen[0] += (rd::<f64>(*car, 0x3d30) > 0.0) as u64;
            self.seen[1] += (rd::<i32>(*car, 0x3a38 + 0x20) == 5 && rd::<i16>(*car, 0x3a38 + 0x26) > 0) as u64;
            self.seen[2] += (rd::<f64>(*car, 0x3d18) > now) as u64;
            self.seen[3] += rd::<u8>(*car, 0x3d20) as u64;
            self.seen[4] = self.seen[4].max(rd::<i32>(car.add(CAR_TRANSPONDER), 0x84).max(0) as u64);
        }
        // (RaceManager::setCurrentSession writes the penalty mode straight into the engine,
        // from the main thread: at once)
        wr(self.engine, PE_PENALTY_MODE, rd::<i32>(self.game.physics_avatar(), PA_PENALTY_MODE));
        self.state = self.dump();
        self.state_hash = rustyac_physics::race::referee::dump_hash(&self.state);
    }

    /// The state as the port's `Referee::dump` words it: the manager, then every car.
    pub unsafe fn dump(&self) -> Vec<(String, String)> {
        let mut out = self.game.dump();
        // (the port's penalty mode is the engine's)
        if let Some(entry) = out.iter_mut().find(|(key, _)| key == "penaltyMode") {
            entry.1 = rd::<i32>(self.engine, PE_PENALTY_MODE).to_string();
        }
        for (index, car) in self.game.cars.iter().enumerate() {
            let p = car.add(0x3a38);
            out.push((
                format!("car {index}"),
                format!(
                    "lockUntil {:016x} locked {} black {} retired {} penalty {} {}{} {} {:08x} cuts {} time {:016x} {:016x}",
                    rd::<u64>(*car, 0x3d18),
                    rd::<u8>(*car, 0x3d14),
                    rd::<u8>(*car, 0x3d20),
                    rd::<u8>(*car, CAR_IS_RETIRED),
                    rd::<i32>(p, 0x20),
                    rd::<u8>(p, 0x24),
                    rd::<u8>(p, 0x25),
                    rd::<i16>(p, 0x26),
                    rd::<u32>(p, 0x44),
                    rd::<i32>(car.add(CAR_TRANSPONDER), 0x84),
                    rd::<u64>(*car, 0x3d30),
                    rd::<u64>(*car, 0x3d28)
                ),
            ));
        }
        if let Some(pit) = &self.pit {
            out.extend(pit.dump());
        }
        out
    }

    /// The end: the last session's results and the game's own `race_out.json`.
    pub unsafe fn shutdown(&mut self) -> Result<Vec<u8>, String> {
        REFEREE_RAND_ON.with(|on| on.set(true));
        let file = self.game.shutdown();
        REFEREE_RAND_ON.with(|on| on.set(false));
        self.note();
        file
    }

    pub fn lock_gearbox_ms(&self) -> f64 {
        unsafe { rd(self.engine, PE_LOCK_GEARBOX_MS) }
    }
}

const VA_FLAGS_UPDATE: usize = 0x1_4009_d140; // void FlagManager::update(float)
const VA_FLAGS_SET_FLAG: usize = 0x1_4009_cf00; // void FlagManager::setFlag(FlagType)
const VA_FLAGS_CLEAR_FLAG: usize = 0x1_4009_cb60; // void FlagManager::clearFlag()
const VA_GET_ROAD_DIRECTION: usize = 0x1_400d_3630; // vec3f CarAvatar::getRoadDirection()

extern "C" fn control_virtual(_this: *mut u8, _value: u8) {}

/// `CarAvatar::getRoadDirection`: along +z by as much as the harness wrote into the avatar's
/// (unused) mirror position.
extern "C" fn get_road_direction(avatar: *const u8, out: *mut f32) -> *mut f32 {
    unsafe {
        *out = 0.0;
        *out.add(1) = 0.0;
        *out.add(2) = rd::<f32>(avatar, 0x120);
    }
    out
}

/// The game's `FlagManager` on the cars of a [`GameReferee`]: a zeroed block with the members
/// its constructor sets and six controls that are only their "visible" flag. `update`,
/// `checkForBlue`, `checkForYellow`, `checkforPenalty`, `setFlag` and `clearFlag` are the
/// game's own.
pub struct GameFlags<'a> {
    acs: &'a Acs,
    flags: *mut u8,
    controls: Vec<*mut u8>,
    cars: usize,
}

impl<'a> GameFlags<'a> {
    pub unsafe fn build(acs: &'a Acs, referee: &GameReferee, yellow_flag_distance: f32) -> GameFlags<'a> {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(get_road_direction as *const () as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(VA_GET_ROAD_DIRECTION), &code);
        let flags = zeroed(0x108);
        wr(flags, 8, referee.game);
        wr(flags, 0xa8, referee.sim);
        wr(flags, 0xc8, rd::<*mut u8>(referee.sim, SIM_TRACK));
        wr(flags, 0xd0, yellow_flag_distance);
        wr(flags, 0xd4, 1u8);
        let n = referee.avatars.len();
        let last = Box::leak(vec![-1.0f32; n].into_boxed_slice()).as_mut_ptr();
        wr(flags, 0x78, last);
        wr(flags, 0x80, last.add(n));
        wr(flags, 0x88, last.add(n));
        let table = Box::leak(vec![control_virtual as *const () as usize; 32].into_boxed_slice());
        let mut controls = Vec::new();
        for offset in [0xd8usize, 0xe0, 0xe8, 0xf0, 0xf8, 0x100] {
            let control = zeroed(0x400);
            wr(control, 0, table.as_ptr());
            wr(flags, offset, control);
            controls.push(control);
        }
        // the menus the flags hide behind: there, and not shown
        wr(referee.sim, 0x110, zeroed(0x400));
        wr(referee.sim, 0x118, zeroed(0x400));
        GameFlags { acs, flags, controls, cars: n }
    }

    /// What the flags read of a car beyond the manager's view, written into its avatar.
    pub unsafe fn set_car(&self, referee: &GameReferee, index: usize, car: &rustyac_physics::race::flags::FlagCar) {
        let avatar = referee.avatars[index];
        wr(avatar, 0x129c, car.is_in_pitlane as u8);
        for tyre in 0..4usize {
            wr(avatar, 0x268 + 0x328 + tyre * 0xc8 + 0xa0, (tyre as u32 >= car.tyres_off_track) as u8);
        }
        wr(avatar, 0x268 + 0x26c, car.gear);
        // the body's forward axis is -z here: the road's direction dot the backward axis is
        // what the stand-in for getRoadDirection answers along z
        wr(avatar, 0x268 + 4 + 0x20, [0.0f32, 0.0, -1.0]);
        wr(avatar, 0x120, car.against_road);
    }

    /// The player's penalties (`Car::penaltyTime`, the penalty manager's laps).
    pub unsafe fn set_penalty(&self, referee: &GameReferee, penalty: &rustyac_physics::race::flags::PlayerPenalty) {
        let car = referee.cars[0];
        wr(car, 0x3d30, penalty.penalty_time);
        wr(car, 0x3d28, penalty.penalty_time);
        wr(car, 0x3a38 + 0x26, penalty.pit_penalty_laps);
    }

    pub unsafe fn black_flag(&self, flag: bool) {
        if flag {
            let set_flag: extern "C" fn(*mut u8, i32) = std::mem::transmute(self.acs.va(VA_FLAGS_SET_FLAG));
            set_flag(self.flags, 2);
        } else {
            let clear_flag: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_FLAGS_CLEAR_FLAG));
            clear_flag(self.flags);
        }
    }

    pub unsafe fn new_session(&self) {
        let clear_flag: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_FLAGS_CLEAR_FLAG));
        clear_flag(self.flags);
    }

    pub unsafe fn update(&self, dt: f32) {
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_FLAGS_UPDATE));
        update(self.flags, dt);
    }

    /// The flag, its timer, every car's last place, and what is shown (blue, black,
    /// chequered, yellow, penalty).
    pub unsafe fn text(&self) -> String {
        let last: *const u32 = rd(self.flags, 0x78);
        let places: Vec<String> = (0..self.cars).map(|i| format!("{:08x}", *last.add(i))).collect();
        let shown: Vec<String> = self.controls[..5].iter().map(|control| rd::<u8>(*control, 0x150).to_string()).collect();
        format!("{} {:08x} [{}] {}", rd::<i32>(self.flags, 0xb0), rd::<u32>(self.flags, 0x70), places.join(" "), shown.join(""))
    }
}

/// The port's actions as the stand-ins of the game's side write them down.
pub fn told_by_port(actions: &[Action]) -> Vec<String> {
    let mut out = Vec::new();
    for action in actions {
        match action {
            Action::NewSession { session, index } => out.push(format!("NewSession({index}, type {}, {:?})", session.session_type, session.name)),
            Action::GoToPit(car) => out.push(format!("GoToSpawn({car}, PIT)")),
            Action::SessionEnd { event, .. } => out.push(format!("SessionEnd({})", rustyac_physics::race::referee::session_end_text(event))),
            Action::Message { text, description } => out.push(format!("Message({text} | {description})")),
            Action::DisplayResults => out.push("DisplayResults".to_string()),
            Action::HideResults => out.push("HideResults".to_string()),
            Action::FadeStarted => out.push("FadeStarted".to_string()),
            _ => {}
        }
    }
    out
}

/// The jobs for the physics thread that the port's actions stand for, by what they are.
pub fn jobs_by_port(actions: &[Action]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for action in actions {
        match action {
            Action::ResetCar(_) => out.extend(["lockControlsUntilTime", "resetPenalty"]),
            Action::ControlsLock { .. } => out.push("setControlsLock"),
            Action::SessionStartTime(_) => out.push("setCurrentSessionStartTime"),
            Action::GenerateWind => out.push("generateWind"),
            // (the results panel locks and frees the player's controls)
            Action::DisplayResults | Action::HideResults => out.push("setControlsLock"),
            _ => {}
        }
    }
    out
}

/// What a made-up drive found.
pub struct MicroReport {
    pub frames: usize,
    pub laps: usize,
    pub sessions_ended: usize,
    pub values: usize,
    pub jobs_seen: Vec<(usize, &'static str)>,
    pub json_bytes: usize,
    pub mismatch: Option<String>,
    /// frames with each flag as the current one (none, blue, black, chequered, yellow, white, penalty)
    pub flag_frames: [usize; 7],
    /// server sessions through `convertRemoteSession`, all the same
    pub remote_sessions: usize,
    /// one hash over the state after every frame, and the hash of `race_out.json`: what
    /// `crates/rustyac-physics/tests/race_rules.rs` holds the port alone against
    pub state_hash: u64,
    pub json_hash: u64,
}

/// Where two values part: the words around the first one that differs.
fn first_difference(key: &str, game: &str, port: &str) -> String {
    let (g, p): (Vec<&str>, Vec<&str>) = (game.split(' ').collect(), port.split(' ').collect());
    let at = g.iter().zip(&p).position(|(x, y)| x != y).unwrap_or(g.len().min(p.len()));
    let from = at.saturating_sub(3);
    format!("{key}, word {at}\n  the game: {}\n  the port: {}", g[from.min(g.len())..(at + 6).min(g.len())].join(" "), p[from.min(p.len())..(at + 6).min(p.len())].join(" "))
}

const VA_RM_CONVERT_REMOTE_SESSION: usize = 0x1_4013_6a50; // Session RaceManager::convertRemoteSession(const RemoteSession&)

/// `count` made-up server sessions through the game's `convertRemoteSession` and the port's:
/// how many came out the same.
pub unsafe fn remote_sessions(acs: &Acs, manager: *mut u8, seed: u32, count: usize) -> Result<usize, String> {
    use rustyac_physics::race::{convert_remote_session, RemoteSession};
    let convert: extern "C" fn(*mut u8, *mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(VA_RM_CONVERT_REMOTE_SESSION));
    let mut rand = MsvcRand(seed ^ 0x0e28);
    let names = ["Practice", "Qualify", "Race", "Q", "", "A much longer session name"];
    for n in 0..count {
        let remote = RemoteSession {
            name: names[rand.next() as usize % names.len()].to_string(),
            session_type: rand.next() % 5,
            index: rand.next() % 4,
            time: rand.next() % 90,
            laps: rand.next() % 40,
            start_time: rand.next() as f64 * 37.5 - 1000.0,
            is_spectator: rand.next() % 4 == 0,
            is_timed_race: rand.next() % 2 == 0,
            has_extra_lap: rand.next() % 2 == 0,
        };
        let raw = zeroed(0x40);
        write_wstring(raw, &remote.name);
        wr(raw, 0x20, remote.session_type);
        wr(raw, 0x24, remote.index);
        wr(raw, 0x28, remote.time);
        wr(raw, 0x2c, remote.laps);
        wr(raw, 0x30, remote.start_time);
        wr(raw, 0x38, remote.is_spectator as u8);
        wr(raw, 0x39, remote.is_timed_race as u8);
        wr(raw, 0x3a, remote.has_extra_lap as u8);
        let out = zeroed(0x70);
        let s = convert(manager, out, raw);
        let game = format!(
            "type {} timed {} extra {} laps {} duration {:08x} overtime {:08x} spawn {:?} name {:?} start {:016x} forced {} over {} leaderLaps {}",
            rd::<i32>(s, 0),
            rd::<u8>(s, 4),
            rd::<u8>(s, 5),
            rd::<i32>(s, 8),
            rd::<u32>(s, 0xc),
            rd::<u32>(s, 0x10),
            text(s.add(0x18)),
            text(s.add(0x38)),
            rd::<u64>(s, 0x58),
            rd::<i32>(s, 0x60),
            rd::<u8>(s, 0x64),
            rd::<u32>(s, 0x68)
        );
        let p = convert_remote_session(&remote);
        let port = format!(
            "type {} timed {} extra {} laps {} duration {:08x} overtime {:08x} spawn {:?} name {:?} start {:016x} forced {} over {} leaderLaps {}",
            p.session_type,
            p.is_timed_race as u8,
            p.has_additional_lap as u8,
            p.laps,
            p.duration_minutes.to_bits(),
            p.overtime_ms.to_bits(),
            p.spawn_set,
            p.name,
            p.start_time.to_bits(),
            p.forced_position,
            p.is_over as u8,
            p.leader_completed_laps
        );
        if game != port {
            return Err(format!("remote session {n} ({remote:?})\n  the game: {game}\n  the port: {port}"));
        }
    }
    Ok(count)
}

/// One made-up drive (`rustyac_physics::race::toy`): `frames` frames of the game's referee
/// and flags and of the port's, fed the same cars, compared after every frame; the results
/// and `race_out.json` at the end.
pub unsafe fn micro(acs: &Acs, seed: u32, variant: usize, car_count: usize, frames: usize, fault: bool) -> Result<MicroReport, String> {
    use rustyac_physics::race::toy::{self, Button, PortDrive};
    let ini = toy::sessions(variant, car_count);
    let setups: Vec<CarSetup> = toy::players(variant, car_count).into_iter().enumerate().map(|(i, (player, compound))| CarSetup { name: player.name, car: player.car, skin: player.skin, compound, start_index: i as i32 }).collect();
    let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
    srand(seed);
    let game = GameReferee::build(acs, &ini, &setups, "toy_track", toy::layout(variant), toy::TRACK_LENGTH, toy::SECTORS, toy::OVER_TIME_MULT, 100_000.0, None);
    // (online the manager's session is a copy of the server's: the conversion alone)
    let converted = remote_sessions(acs, game.manager, seed, 300)?;
    let (mut drive, start_actions) = PortDrive::new(seed, variant, car_count);
    drive.fault = fault;
    game.start_race(20000.0);
    let (mut game_log, mut game_jobs, _) = game.take_told();
    let mut port_log = told_by_port(&start_actions);
    let mut port_jobs = jobs_by_port(&start_actions);
    let game_flags = GameFlags::build(acs, &game, 700.0);
    let mut report = MicroReport { frames: 0, laps: 0, sessions_ended: 0, values: 0, jobs_seen: Vec::new(), json_bytes: 0, mismatch: None, flag_frames: [0; 7], remote_sessions: converted, state_hash: 0xcbf2_9ce4_8422_2325, json_hash: 0 };
    let mut job_names: std::collections::BTreeMap<usize, &'static str> = std::collections::BTreeMap::new();
    // (the first session is a new session too)
    let mut last_new_session = true;

    for frame_index in 0..frames {
        if last_new_session {
            // (FlagManager's handler of the new-session event)
            game_flags.new_session();
        }
        let frame = drive.begin_frame();
        game.set_now(drive.now);
        for (index, view) in frame.views.iter().enumerate() {
            game.set_car(index, view, frame.speeds[index]);
            game.set_fake_car(index, view);
            game_flags.set_car(&game, index, &frame.flag_cars[index]);
        }
        game_flags.set_penalty(&game, &frame.penalty);
        if let Some(flag) = frame.black_flag {
            game_flags.black_flag(flag);
        }
        // the frame, the game's and the port's
        for split in &frame.splits {
            game.sector_split(split);
        }
        for lap in &frame.laps {
            game.lap_completed(lap);
        }
        game.update(toy::DT);
        let (log, jobs, new_session) = game.take_told();
        game_log.extend(log);
        game_jobs.extend(jobs);
        let port = drive.finish_frame(frame);
        if new_session != port.new_session {
            report.mismatch = Some(format!("frame {frame_index}: a new session on one side only (the game: {new_session})"));
            break;
        }
        last_new_session = port.new_session;
        if port.new_session && drive.manager.current_session.session_type == SESSION_RACE {
            game.start_race(10000.0);
        }
        // the player's buttons, now and then
        match port.button {
            Button::NextSession => game.skip(),
            Button::RestartSession => game.restart(),
            Button::None => {}
        }
        game_flags.update(toy::DT);
        let (log, jobs, _) = game.take_told();
        game_log.extend(log);
        game_jobs.extend(jobs);
        port_log.extend(told_by_port(&port.actions));
        port_jobs.extend(jobs_by_port(&port.actions));

        if game_log != port_log {
            let at = game_log.iter().zip(&port_log).position(|(a, b)| a != b).unwrap_or(game_log.len().min(port_log.len()));
            report.mismatch = Some(format!("frame {frame_index}: told differently\n  the game: {:?}\n  the port: {:?}", game_log.get(at), port_log.get(at)));
            break;
        }
        game_log.clear();
        port_log.clear();
        if game_jobs.len() != port_jobs.len() {
            report.mismatch = Some(format!("frame {frame_index}: {} jobs for the physics thread in the game, {} in the port ({:x?} / {:?})", game_jobs.len(), port_jobs.len(), game_jobs, port_jobs));
            break;
        }
        for (address, name) in game_jobs.drain(..).zip(port_jobs.drain(..)) {
            match job_names.get(&address) {
                Some(known) if *known != name => {
                    report.mismatch = Some(format!("frame {frame_index}: the job at {address:#x} is {known} and {name}"));
                }
                Some(_) => {}
                None => {
                    if job_names.values().any(|known| *known == name) {
                        report.mismatch = Some(format!("frame {frame_index}: the job {name} has two addresses"));
                    }
                    job_names.insert(address, name);
                }
            }
        }
        if report.mismatch.is_some() {
            break;
        }
        let mut a = game.dump();
        a.push(("flag".to_string(), game_flags.text()));
        let b = drive.dump();
        report.values += a.len();
        if a != b {
            let line = a.iter().zip(&b).find(|(x, y)| x != y).map(|(x, y)| first_difference(&x.0, &x.1, &y.1)).unwrap_or_else(|| format!("{} values in the game, {} in the port", a.len(), b.len()));
            report.mismatch = Some(format!("frame {frame_index}: {line}"));
            break;
        }
        report.state_hash = (report.state_hash ^ rustyac_physics::race::referee::dump_hash(&b)).wrapping_mul(0x0000_0100_0000_01b3);
        report.frames += 1;
    }
    report.jobs_seen = job_names.into_iter().collect();
    report.flag_frames = drive.flag_frames;
    report.laps = drive.laps;
    report.sessions_ended = drive.sessions_ended;
    if report.mismatch.is_some() {
        return Ok(report);
    }
    // the end of the program: the last session's results, the file
    let game_file = game.shutdown()?;
    let (actions, port_file) = drive.shutdown();
    report.sessions_ended = drive.sessions_ended;
    let (log, _, _) = game.take_told();
    let port_log = told_by_port(&actions);
    if log != port_log {
        report.mismatch = Some(format!("the end: told differently\n  the game: {log:?}\n  the port: {port_log:?}"));
        return Ok(report);
    }
    let mut a = game.dump();
    a.push(("flag".to_string(), game_flags.text()));
    let b = drive.dump();
    if a != b {
        let line = a.iter().zip(&b).find(|(x, y)| x != y).map(|(x, y)| first_difference(&x.0, &x.1, &y.1)).unwrap_or_default();
        report.mismatch = Some(format!("the end: {line}"));
        return Ok(report);
    }
    report.state_hash = (report.state_hash ^ rustyac_physics::race::referee::dump_hash(&b)).wrapping_mul(0x0000_0100_0000_01b3);
    report.json_bytes = game_file.len();
    report.json_hash = game_file.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, byte| (h ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3));
    if game_file != port_file {
        let at = game_file.iter().zip(&port_file).position(|(x, y)| x != y).unwrap_or(game_file.len().min(port_file.len()));
        let from = at.saturating_sub(60);
        report.mismatch = Some(format!(
            "race_out.json differs at byte {at} ({} bytes in the game's, {} in the port's)\n  the game: {:?}\n  the port: {:?}",
            game_file.len(),
            port_file.len(),
            String::from_utf8_lossy(&game_file[from..(at + 60).min(game_file.len())]),
            String::from_utf8_lossy(&port_file[from..(at + 60).min(port_file.len())])
        ));
    }
    Ok(report)
}
