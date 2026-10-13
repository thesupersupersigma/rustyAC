// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28c: the game's own replay code beside the port's, in a session of the game's cars.
//!
//! The game's side: a `ReplayRecorder` made by the game's constructor 0x14015c9e0 for every
//! car (on a stand-in `CarAvatar` whose state provider is the game's `PhysicsCarStateProvider`
//! of the real `Car`), and a `ReplayManager` laid out by hand as its constructor leaves it (the
//! constructor itself needs the game's GUI). The game's `ReplayManager::onStepCompleted`
//! 0x140158b50 records, `onLapCompleted` 0x140158a00 keeps the laps, `save` 0x140159d10
//! writes the file (into the oracle's stand-in Documents folder, which is a folder of the
//! scratch root: see `multi::sh_get_folder_path_w`), `load` 0x140157680 reads it back into a
//! second manager (with `Sim::loadTrack` and `Sim::addCar` replaced by the harness), and
//! `updatePlayState` 0x14015b9a0 with the transport functions plays it, each car's state
//! coming out of the game's `ReplayRecorder::getPhysicsState` 0x14015d850.
//!
//! The port's side (`rustyac_physics::replay`) is fed the same `CarPhysicsState`s (the game's
//! own, from `Car::getPhysicsState`) and the same calls; everything is compared byte by byte.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rustyac_physics::car_physics_state::CarPhysicsState;
use rustyac_physics::replay::{AvatarRecord, CarHeader, LeaderboardEntry, PlaybackAvatar, ReplayLoader, ReplayManager, ReplayRecorder, SaveInfo};
use rustyac_physics::track::Track;
use rustyac_physics::vecmath::Mat44f;

use super::race_ini::write_wstring;
use super::{rd, wr};
use crate::acs::Acs;

const VA_RECORDER_CTOR: usize = 0x1_4015_c9e0; // ReplayRecorder::ReplayRecorder(CarAvatar*, ICarPhysicsStateProvider*)
const VA_RECORDER_SET_FRAME_SIZE: usize = 0x1_4016_2420; // ReplayRecorder::setFrameSize(int)
const VA_ON_STEP_COMPLETED: usize = 0x1_4015_8b50; // ReplayManager::onStepCompleted(const OnPhysicsStepCompleted&)
const VA_ON_LAP_COMPLETED: usize = 0x1_4015_8a00; // ReplayManager::onLapCompleted(const OnLapCompletedEvent&)
const VA_SAVE: usize = 0x1_4015_9d10; // bool ReplayManager::save(std::wstring&, eSaveReplayDir)
const VA_LOAD: usize = 0x1_4015_7680; // bool ReplayManager::load(const std::wstring&)
const VA_UPDATE_PLAY_STATE: usize = 0x1_4015_b9a0; // ReplayManager::updatePlayState(float)
const VA_PLAY: usize = 0x1_4015_91f0;
const VA_PAUSE: usize = 0x1_4015_9070;
const VA_STOP: usize = 0x1_4015_b6f0;
const VA_REWIND: usize = 0x1_4015_9b40; // rewind(int, bool)
const VA_FAST_FORWARD: usize = 0x1_4015_6780; // fastForward(int, bool)
const VA_SLOW_MOTION: usize = 0x1_4015_b3b0; // slowMotion(float)
const VA_NEXT_FRAME: usize = 0x1_4015_8900;
const VA_NEXT_LAP: usize = 0x1_4015_8930;
const VA_PREVIOUS_LAP: usize = 0x1_4015_92a0;
const VA_SET_NORMALIZED_POSITION: usize = 0x1_4015_b0b0; // setNormalizedPosition(float)
const VA_SET_CUT_IN_PERC: usize = 0x1_4015_af30; // float setCutInPerc(float)
const VA_SET_CUT_OUT_PERC: usize = 0x1_4015_aff0; // float setCutOutPerc(float)
const VA_RECORD_INTERVAL_MS: usize = 0x1_4151_9400; // static double ReplayRecorder::recordIntervalMS
const VA_FRAME_INFORMATION: usize = 0x1_4155_9b38; // std::vector<FrameInformation> frameInformation
const VA_REPLAY_LEADERBOARDS: usize = 0x1_4155_9b20; // std::vector<std::vector<LeaderboardEntry>> replayLeaderboards
const VA_PROVIDER_VTABLE: usize = 0x1_404c_7ed8; // PhysicsCarStateProvider
const VA_WEATHER_MANAGER_VTABLE: usize = 0x1_404e_0960;
const VA_CAR_AVATAR_VTABLE: usize = 0x1_404b_e5e8;
const VA_GET_DOCUMENT_PATH: usize = 0x1_4023_0890; // static std::wstring Path::getDocumentPath()
const VA_SIM_LOAD_TRACK: usize = 0x1_4019_a4c0;
const VA_SIM_ADD_CAR: usize = 0x1_4019_78a0; // CarAvatar* Sim::addCar(const wstring&, const wstring&, const wstring&)
const VA_SIM_APPLY_CUSTOM_WEATHER: usize = 0x1_4019_8010;
const VA_PAUSE_PHYSICS: usize = 0x1_4012_2300;
const VA_AUDIO_START: usize = 0x1_401f_bec0;
const VA_AUDIO_STOP: usize = 0x1_401f_bfc0;
const VA_SET_DIRT_LEVEL: usize = 0x1_4009_1a10; // DynamicCarEffects::setDirtLevel(float)
const VA_PHYSICS_OBJECT_GET_WORLD_MATRIX: usize = 0x1_402a_cb70;

const CAR_MAX_FUEL: usize = 0x3dc8;
const STATE: usize = 0xb70;
const AVATAR_STATE: usize = 0x268;

/// The game's real `Track`, for the stand-in track's ray cast.
static REAL_TRACK: AtomicUsize = AtomicUsize::new(0);

/// `Track::rayCast` of the stand-in `TrackAvatar::physicsTrack`: the real track's.
extern "C" fn fake_ray_cast(_this: *mut u8, origin: *const f32, direction: *const f32, result: *mut u8, length: f32) -> usize {
    unsafe {
        let track = REAL_TRACK.load(Ordering::Relaxed) as *mut u8;
        let vtable: *const usize = rd(track, 0);
        let ray_cast: extern "C" fn(*mut u8, *const f32, *const f32, *mut u8, f32) -> usize = std::mem::transmute(*vtable.add(1));
        ray_cast(track, origin, direction, result, length)
    }
}

/// The game's `operator new`, for what the game's code will free.
static OPERATOR_NEW: AtomicUsize = AtomicUsize::new(0);

/// `ICarPhysicsStateProvider::getWingState` of a car that is only a replay's (a file of the
/// player's: there is no physics car): as many wings as the file has for it, all at rest.
extern "C" fn fake_get_wing_state(this: *mut u8, out: *mut [usize; 3]) -> *mut [usize; 3] {
    unsafe {
        let count: usize = rd(this, 8);
        let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(OPERATOR_NEW.load(Ordering::Relaxed));
        let data = new((count * 0x44).max(8));
        std::ptr::write_bytes(data, 0, (count * 0x44).max(8));
        *out = [data as usize, data as usize + count * 0x44, data as usize + count * 0x44];
        out
    }
}

/// What `Sim::addCar` of a load makes: the harness's context.
struct AddCar {
    acs: usize,
    game: usize,
    sim: usize,
    /// the game's `Car` of each car of the session, in order
    cars: Vec<usize>,
    /// a file of the player's: each car's wing count (there are no physics cars)
    wings: Option<Vec<usize>>,
    dirt: bool,
    made: Vec<(usize, usize)>,
}

thread_local! {
    static ADD_CAR: std::cell::RefCell<Option<AddCar>> = const { std::cell::RefCell::new(None) };
}

unsafe fn read_wstring(string: *const u8) -> String {
    let len: usize = rd(string, 0x10);
    let cap: usize = rd(string, 0x18);
    let data: *const u16 = if cap > 7 { rd(string, 0) } else { string.cast() };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, len))
}

unsafe fn new_wstring(acs: &Acs, text: &str) -> *mut u8 {
    let s = acs.alloc(0x20);
    write_wstring(s, text);
    s
}

/// A `std::vector` of `count` elements of `size` bytes, zeroed, in the game's own heap.
unsafe fn write_vector(acs: &Acs, at: *mut u8, count: usize, size: usize) -> *mut u8 {
    let data = acs.alloc((count * size).max(8));
    wr(at, 0, data);
    wr(at, 8, data.add(count * size));
    wr(at, 0x10, data.add(count * size));
    data
}

/// A stand-in `CarAvatar` with the game's own `ReplayRecorder` as its child.
unsafe fn make_avatar(acs: &Acs, game: *mut u8, sim: *mut u8, car: *mut u8, wings: Option<usize>, name: &str, guid: i32, dirt: bool) -> (*mut u8, *mut u8) {
    let avatar = acs.alloc(0x12a8);
    wr(avatar, 0, acs.va(VA_CAR_AVATAR_VTABLE));
    wr(avatar, 0x8, game);
    write_wstring(avatar.add(0x10), "CAR");
    wr(avatar, 0x130, sim);
    write_wstring(avatar.add(0x138), name);
    write_wstring(avatar.add(0x158), "");
    let car_node = acs.alloc(0xe0);
    wr(car_node, 0xd8, 1u8);
    wr(avatar, 0x210, car_node);
    wr(avatar, 0xec8, if car.is_null() { 100.0f64 } else { rd::<f64>(car, CAR_MAX_FUEL) });
    for offset in [0x10b8, 0x10d8, 0x10f8, 0x1118, 0x1138] {
        write_wstring(avatar.add(offset), "");
    }
    wr(avatar, 0x1158, guid);
    // (`CarAvatar::physics` stays null: with a car, `setDriverInfo` would hand the name on to
    // the physics thread)
    let provider = acs.alloc(0x10);
    match wings {
        Some(count) => {
            OPERATOR_NEW.store(acs.crt_function(c"??2@YAPEAX_K@Z"), Ordering::Relaxed);
            let table = acs.alloc(0x20);
            wr(table, 0x10, fake_get_wing_state as *const () as usize);
            wr(provider, 0, table);
            wr(provider, 8, count);
        }
        None => {
            wr(provider, 0, acs.va(VA_PROVIDER_VTABLE));
            wr(provider, 8, car);
        }
    }
    wr(avatar, 0x11b8, provider);
    let recorder = acs.alloc(0x110);
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_RECORDER_CTOR));
    ctor(recorder, avatar, provider);
    // GameObject::addGameObject(recorder): the car's only child
    let children = write_vector(acs, avatar.add(0x40), 1, 8);
    wr(children, 0, recorder);
    // the avatar's wing list, as long as the provider's
    let wings = wing_angles(provider).len();
    write_vector(acs, avatar.add(0xfa0), wings, 0x44);
    if dirt {
        // a `DynamicCarEffects` of which only `currentDirtLevel` (+0x78) is touched
        wr(recorder, 0x70, acs.alloc(0x100));
    }
    (avatar, recorder)
}

/// `ICarPhysicsStateProvider::getWingState`: every wing's angle.
unsafe fn wing_angles(provider: *mut u8) -> Vec<f32> {
    let vtable: *const usize = rd(provider, 0);
    let get: extern "C" fn(*mut u8, *mut [usize; 3]) -> *mut [usize; 3] = std::mem::transmute(*vtable.add(2));
    let mut out = [0usize; 3];
    get(provider, &mut out);
    let count = (out[1] - out[0]) / 0x44;
    (0..count).map(|i| rd::<f32>(out[0] as *const u8, 0x44 * i + 0xc)).collect()
}

/// The harness's `Sim::addCar(model, config, skin)` while a replay is loaded.
extern "C" fn add_car(_sim: *mut u8, model: *const u8, _config: *const u8, skin: *const u8) -> *mut u8 {
    ADD_CAR.with(|context| unsafe {
        let mut context = context.borrow_mut();
        let c = context.as_mut().expect("a load is running");
        let acs = &*(c.acs as *const Acs);
        let index = c.made.len();
        let (car, wings) = match &c.wings {
            Some(wings) => (std::ptr::null_mut(), Some(wings.get(index).copied().unwrap_or(0))),
            None => (c.cars[index.min(c.cars.len() - 1)] as *mut u8, None),
        };
        let (avatar, recorder) = make_avatar(acs, c.game as *mut u8, c.sim as *mut u8, car, wings, &read_wstring(model), index as i32, c.dirt);
        write_wstring(avatar.add(0x1138), &read_wstring(skin));
        // Sim::cars.push_back(avatar)
        let sim = c.sim as *mut u8;
        let begin: *mut usize = rd(sim, 0x208);
        *begin.add(index) = avatar as usize;
        wr(sim, 0x210, begin.add(index + 1));
        c.made.push((avatar as usize, recorder as usize));
        avatar
    })
}

/// One step of a playback script.
#[derive(Clone, Copy, Debug)]
pub enum Act {
    Play,
    Pause,
    Stop,
    Rewind(i32, bool),
    FastForward(i32, bool),
    SlowMotion(f32),
    NextFrame,
    NextLap,
    PreviousLap,
    Position(f32),
    CutIn(f32),
    CutOut(f32),
}

/// The stand-in session of one manager.
struct Session {
    manager: *mut u8,
    #[allow(dead_code)]
    sim: *mut u8,
    avatars: Vec<*mut u8>,
    recorders: Vec<*mut u8>,
}

pub struct ReplayProof<'a> {
    acs: &'a Acs,
    game: *mut u8,
    graphics: *mut u8,
    track_avatar: *mut u8,
    track_objects: Vec<*mut u8>,
    physics_objects: Vec<*mut u8>,
    live: Session,
    cars: Vec<*mut u8>,
    names: Vec<String>,
    states: *mut u8,
    states_vector: *mut u8,
    port: ReplayManager,
    port_track: Arc<Track>,
    max_fuel: Vec<f64>,
    wings: Vec<usize>,
    limiters: Vec<i32>,
    lap_counts: Vec<u32>,
    first_board: bool,
    dirt: bool,
    max_frames: i32,
    track_name: String,
    layout: String,
    pub recorded: u64,
    pub laps: u64,
    /// the next playback's car states are kept: by car, the game's rebuilt `CarPhysicsState` of
    /// every picture frame (for the picture's and the sound's oracles)
    pub keep_tape: bool,
    pub tapes: Vec<Vec<u8>>,
}

/// A hand-made `std::map` head (`_Tree_node` with `_Isnil`): an empty map.
unsafe fn empty_map(acs: &Acs, at: *mut u8) {
    let head = acs.alloc(0x40);
    wr(head, 0, head);
    wr(head, 8, head);
    wr(head, 0x10, head);
    wr(head, 0x18, 1u8);
    wr(head, 0x19, 1u8);
    wr(at, 0, head);
    wr(at, 8, 0usize);
}

/// A `ReplayManager` as its constructor 0x140153130 leaves it.
unsafe fn make_manager(acs: &Acs, game: *mut u8, sim: *mut u8, max_frames: i32) -> *mut u8 {
    let m = acs.alloc(0x198);
    wr(m, 0x8, game);
    write_wstring(m.add(0x10), "REPLAY_MANAGER");
    wr(m, 0xbc, 2i32);
    write_wstring(m.add(0xc0), "");
    empty_map(acs, m.add(0xf8));
    wr(m, 0x108, 2i32);
    wr(m, 0x110, acs.alloc(0x330));
    wr(m, 0x128, sim);
    wr(m, 0x130, 1.0f32);
    wr(m, 0x138, 2.0f32);
    wr(m, 0x160, max_frames);
    wr(m, 0x170, 10.0f32);
    empty_map(acs, m.add(0x180));
    m
}

unsafe fn make_sim(acs: &Acs, game: *mut u8, track_avatar: *mut u8, car_slots: usize) -> *mut u8 {
    let sim = acs.alloc(0x800);
    wr(sim, 0x8, game);
    // the Sim's children: a WeatherManager (found by a dynamic cast, so with the game's vtable)
    let weather = acs.alloc(0x80);
    wr(weather, 0, acs.va(VA_WEATHER_MANAGER_VTABLE));
    write_wstring(weather.add(0x10), "WEATHER_MANAGER");
    write_wstring(weather.add(0x60), "3_clear");
    let children = write_vector(acs, sim.add(0x40), 1, 8);
    wr(children, 0, weather);
    wr(sim, 0x150, track_avatar);
    wr(sim, 0x1b8, acs.alloc(0x400));
    // Sim::cars with room for every car, none in it yet
    let cars = acs.alloc(8 * car_slots.max(1));
    wr(sim, 0x208, cars);
    wr(sim, 0x210, cars);
    wr(sim, 0x218, cars.add(8 * car_slots.max(1)));
    sim
}

impl<'a> ReplayProof<'a> {
    /// `cars`: the game's `Car`s; `interval_ms`: the recording interval; `max_frames`: the
    /// ring's size (the port works it out of `replay.ini` when this is `None`).
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn build(acs: &'a Acs, cars: &[*mut u8], names: &[String], game_track: *mut u8, port_track: Arc<Track>, physics_objects: &[*mut u8], file_objects: usize, ini: Option<&rustyac_physics::data::ini::IniReader>, max_frames: Option<i32>, dirt: bool) -> ReplayProof<'a> {
        REAL_TRACK.store(game_track as usize, Ordering::Relaxed);
        // the sound, the pause of the physics and the car's dirt are not part of this
        acs.patch(acs.va(VA_AUDIO_START), &[0xc3]);
        acs.patch(acs.va(VA_AUDIO_STOP), &[0xc3]);
        acs.patch(acs.va(VA_PAUSE_PHYSICS), &[0xc3]);
        acs.patch(acs.va(VA_SIM_LOAD_TRACK), &[0xc3]);
        acs.patch(acs.va(VA_SIM_APPLY_CUSTOM_WEATHER), &[0xc3]);
        // movss [rcx+0x78], xmm1 ; ret: the dirt level is kept, no material is touched
        acs.patch(acs.va(VA_SET_DIRT_LEVEL), &[0xf3, 0x0f, 0x11, 0x49, 0x78, 0xc3]);
        // mov rax, add_car ; jmp rax
        let mut jump = vec![0x48, 0xb8];
        jump.extend_from_slice(&(add_car as *const () as usize).to_le_bytes());
        jump.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(VA_SIM_ADD_CAR), &jump);

        let game = acs.alloc(0x400);
        let graphics = acs.alloc(0x400);
        wr(game, 0x138, graphics);
        wr(game, 0x148, acs.alloc(0x100));
        // the track's avatar: its name, its layout, its loose objects, and a `Track` whose ray
        // cast and AI line are the real track's
        let track_avatar = acs.alloc(0x400);
        write_wstring(track_avatar.add(0x10), &port_track.name);
        write_wstring(track_avatar.add(0xa0), &port_track.config);
        let table = acs.alloc(0x40);
        wr(table, 8, fake_ray_cast as *const () as usize);
        wr(track_avatar, 0xf8, table);
        wr(track_avatar, 0xf8 + 0x80, rd::<usize>(game_track, 0x80));
        // (a file of the player's brings its own count of loose objects: stand-ins without a body)
        let object_list: Vec<*mut u8> = if physics_objects.is_empty() { vec![std::ptr::null_mut(); file_objects] } else { physics_objects.to_vec() };
        let objects = write_vector(acs, track_avatar.add(0x60), object_list.len(), 8);
        let mut track_objects = Vec::new();
        for (k, physics) in object_list.iter().enumerate() {
            let object = acs.alloc(0xe0);
            wr(object, 0x58, *physics);
            wr(object, 0x60, acs.alloc(0xe0));
            wr(objects, 8 * k, object);
            track_objects.push(object);
        }
        let sim = make_sim(acs, game, track_avatar, cars.len());
        let mut avatars = Vec::new();
        let mut recorders = Vec::new();
        let (mut max_fuel, mut wings, mut limiters) = (Vec::new(), Vec::new(), Vec::new());
        let mut port_recorders = Vec::new();
        for (i, (&car, name)) in cars.iter().zip(names).enumerate() {
            let (avatar, recorder) = make_avatar(acs, game, sim, car, None, name, i as i32, dirt);
            write_wstring(avatar.add(0x10b8), &format!("Driver {i}"));
            write_wstring(avatar.add(0x10d8), &format!("Team {}", i % 3));
            write_wstring(avatar.add(0x1118), ["ITA", "GBR", "ZZZZ"][i % 3]);
            write_wstring(avatar.add(0x1138), &format!("skin_{i}"));
            let begin: *mut usize = rd(sim, 0x208);
            *begin.add(i) = avatar as usize;
            wr(sim, 0x210, begin.add(i + 1));
            max_fuel.push(rd::<f64>(car, CAR_MAX_FUEL));
            wings.push(wing_angles(rd(avatar, 0x11b8)).len());
            limiters.push(rd::<i32>(recorder, 0x10c));
            port_recorders.push(ReplayRecorder::new(rd::<i32>(recorder, 0x10c), wings[i]));
            avatars.push(avatar);
            recorders.push(recorder);
        }
        // the port's manager reads replay.ini; the game's is given what the port worked out
        // (its constructor cannot run here), or the ring size of the run's own choice
        let interval: f64 = rd(acs.va(VA_RECORD_INTERVAL_MS) as *const u8, 0);
        let mut port = ReplayManager::new(ini, interval, port_recorders, Some(physics_objects.len()), 1);
        if let Some(frames) = max_frames {
            port.max_frames = frames;
            port.size_ring(Some(physics_objects.len()));
        }
        wr(acs.va(VA_RECORD_INTERVAL_MS) as *mut u8, 0, port.record_interval_ms);
        let max_frames = port.max_frames;
        let manager = make_manager(acs, game, sim, max_frames);
        wr(manager, 0x164, port.min_frames);
        wr(manager, 0x170, port.max_size_mb);
        wr(manager, 0x178, port.current_session_type);
        let list = write_vector(acs, manager.add(0xe0), recorders.len(), 8);
        let set_frame_size: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_RECORDER_SET_FRAME_SIZE));
        for (i, &recorder) in recorders.iter().enumerate() {
            set_frame_size(recorder, max_frames);
            // the game never writes six bytes of a frame (the gaps at 0x12, 0x26 and 0xee, and
            // `boh` at 0x10b), and its vector leaves them as the heap had them: the ring's
            // memory is cleared first, so that what is compared is what the game wrote
            let frames: *mut u8 = rd(recorder, 0x80);
            std::ptr::write_bytes(frames, 0, rd::<usize>(recorder, 0x88) - frames as usize);
            // (the same for the three bytes after every wing's angle)
            let wings: *const u8 = rd(recorder, 0xb0);
            for f in 0..max_frames as usize {
                let inner: *mut u8 = rd(wings, 0x18 * f);
                if !inner.is_null() {
                    std::ptr::write_bytes(inner, 0, rd::<usize>(wings, 0x18 * f + 8) - inner as usize);
                }
            }
            wr(list, 8 * i, recorder);
        }
        // the global `frameInformation`: max_frames elements, each with room for every object
        let info = write_vector(acs, acs.va(VA_FRAME_INFORMATION) as *mut u8, max_frames as usize, 0x20);
        for f in 0..max_frames as usize {
            write_vector(acs, info.add(0x20 * f + 8), physics_objects.len(), 12);
        }
        write_vector(acs, acs.va(VA_REPLAY_LEADERBOARDS) as *mut u8, 0, 0x18);
        let states = acs.alloc(STATE * cars.len());
        let states_vector = acs.alloc(0x18);
        wr(states_vector, 0, states);
        wr(states_vector, 8, states.add(STATE * cars.len()));
        wr(states_vector, 0x10, states.add(STATE * cars.len()));
        ReplayProof {
            acs,
            game,
            graphics,
            track_avatar,
            track_objects,
            physics_objects: physics_objects.to_vec(),
            live: Session { manager, sim, avatars, recorders },
            cars: cars.to_vec(),
            names: names.to_vec(),
            states,
            states_vector,
            port,
            track_name: port_track.name.clone(),
            layout: port_track.config.clone(),
            port_track,
            max_fuel,
            wings,
            limiters,
            lap_counts: vec![0; cars.len()],
            first_board: false,
            dirt,
            max_frames,
            recorded: 0,
            laps: 0,
            keep_tape: false,
            tapes: Vec::new(),
        }
    }

    /// The dirt of a car's `DynamicCarEffects` at a state (made up: a quarter of the first
    /// tyre's dirt level).
    fn dirt_of(state: &CarPhysicsState) -> f32 {
        state.tyre_dirty_level[0] * 0.25
    }

    unsafe fn push_leaderboard(&mut self, states: &[CarPhysicsState], pt: f64) {
        let acs = self.acs;
        // the port's, and the same entries in the game's global vector
        let mut board = Vec::new();
        let entries = acs.alloc(0x20 * states.len().max(1));
        for (i, s) in states.iter().enumerate() {
            let entry = LeaderboardEntry { car: i as i32, total_time: pt * 0.5 + i as f64, best_lap: s.best_lap as f64, laps: s.lap_count as i32, is_race_mode: i % 2 == 0, is_black_flagged: false, has_completed_last_lap: false };
            let e = entries.add(0x20 * i);
            wr(e, 0, self.live.avatars[i]);
            wr(e, 8, entry.total_time);
            wr(e, 0x10, entry.best_lap);
            wr(e, 0x18, entry.laps);
            wr(e, 0x1c, entry.is_race_mode as u8);
            board.push(entry);
        }
        let global = acs.va(VA_REPLAY_LEADERBOARDS) as *mut u8;
        let (begin, end): (*mut u8, *mut u8) = (rd(global, 0), rd(global, 8));
        let old = end.offset_from(begin) as usize;
        let data = acs.alloc(old + 0x18);
        std::ptr::copy_nonoverlapping(begin, data, old);
        wr(data, old, entries);
        wr(data, old + 8, entries.add(0x20 * states.len()));
        wr(data, old + 0x10, entries.add(0x20 * states.len()));
        wr(global, 0, data);
        wr(global, 8, data.add(old + 0x18));
        wr(global, 0x10, data.add(old + 0x18));
        self.port.on_lap_leaderboard(board);
    }

    /// After a physics step at the time `pt` (ms). `state_of`: the game's own
    /// `CarPhysicsState` of a car (`Car::getPhysicsState`).
    pub unsafe fn after_step(&mut self, pt: f64, state_of: &dyn Fn(usize) -> Vec<u8>) -> Result<(), String> {
        let acs = self.acs;
        let n = self.cars.len();
        let due = pt - self.port.last_record_time >= self.port.record_interval_ms;
        let mut states: Vec<CarPhysicsState> = Vec::new();
        let mut avatar_states: Vec<CarPhysicsState> = Vec::new();
        if due {
            for i in 0..n {
                // the avatar's own copy is the state of the frame recorded before this one
                let at = self.states.add(STATE * i);
                let avatar = self.live.avatars[i].add(AVATAR_STATE);
                std::ptr::copy_nonoverlapping(at, avatar, STATE);
                avatar_states.push(CarPhysicsState::from_game_bytes(std::slice::from_raw_parts(avatar, STATE))?);
                let bytes = state_of(i);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, STATE);
                states.push(CarPhysicsState::from_game_bytes(&bytes)?);
            }
            if !self.first_board {
                // the handler of Game::evOnPostUpdate: the first snapshot
                self.first_board = true;
                self.push_leaderboard(&states, pt);
            }
            // laps: a car's lap count went up
            for i in 0..n {
                let count = states[i].lap_count;
                if count > self.lap_counts[i] {
                    self.lap_counts[i] = count;
                    self.laps += 1;
                    let valid = (i as u32 + count) % 3 != 0;
                    let mut event = [0u8; 0x38];
                    event[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    event[4..8].copy_from_slice(&states[i].last_lap.to_le_bytes());
                    event[8..12].copy_from_slice(&count.to_le_bytes());
                    event[0x30] = valid as u8;
                    let on_lap: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_ON_LAP_COMPLETED));
                    on_lap(self.live.manager, event.as_ptr());
                    self.port.on_lap_completed(i, states[i].last_lap, count, valid);
                    self.push_leaderboard(&states, pt);
                }
            }
            if self.dirt {
                for i in 0..n {
                    wr(rd::<*mut u8>(self.live.recorders[i], 0x70), 0x78, Self::dirt_of(&states[i]));
                }
            }
        }
        // the sun moves (a made-up angle of the time)
        let sun = ((pt * 0.00002) as f32).sin() * 40.0;
        wr(self.graphics, 0xd4, sun);
        let mut event = [0u8; 16];
        event[0..8].copy_from_slice(&pt.to_le_bytes());
        event[8..16].copy_from_slice(&(self.states_vector as usize).to_le_bytes());
        let on_step: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_ON_STEP_COMPLETED));
        on_step(self.live.manager, event.as_ptr());
        if due {
            let angles: Vec<Vec<f32>> = (0..n).map(|i| wing_angles(rd(self.live.avatars[i], 0x11b8))).collect();
            let avatars: Vec<AvatarRecord> = (0..n)
                .map(|i| AvatarRecord {
                    actions_state: avatar_states[i].actions_state as u32,
                    status_bytes: avatar_states[i].status_bytes,
                    kers_is_charging: avatar_states[i].kers_is_charging,
                    max_fuel: self.max_fuel[i],
                    car_dirt: self.dirt.then(|| Self::dirt_of(&states[i])),
                    connected: true,
                    wing_angles: &angles[i],
                })
                .collect();
            let get: extern "C" fn(*mut u8, *mut Mat44f) -> *mut Mat44f = std::mem::transmute(acs.va(VA_PHYSICS_OBJECT_GET_WORLD_MATRIX));
            let objects: Vec<Option<Mat44f>> = self
                .physics_objects
                .iter()
                .map(|&object| {
                    let mut m = Mat44f::IDENTITY;
                    get(object, &mut m);
                    Some(m)
                })
                .collect();
            if !self.port.on_step_completed(pt, &states, &avatars, sun, &objects) {
                return Err(format!("the port did not record at {pt}"));
            }
            self.recorded += 1;
        } else if self.port.on_step_completed(pt, &[], &[], sun, &[]) {
            return Err(format!("the port recorded at {pt} without being due"));
        }
        Ok(())
    }

    /// The game's recorder of a car against the port's: every frame of the ring and what
    /// goes with it. The answer: what differs (nothing, when it is empty).
    unsafe fn compare_recorder(&self, game: *mut u8, port: &ReplayRecorder, label: &str, out: &mut Vec<String>) {
        let frames: *const u8 = rd(game, 0x80);
        let count = (rd::<usize>(game, 0x88) - frames as usize) / 0x110;
        if count != port.frames.len() {
            out.push(format!("{label}: {count} frames of the game, {} of the port", port.frames.len()));
            return;
        }
        let info: *const u8 = rd(game, 0x98);
        let wings: *const u8 = rd(game, 0xb0);
        let mut bad = 0;
        for f in 0..count {
            let g = std::slice::from_raw_parts(frames.add(0x110 * f), 0x110);
            if g != port.frames[f].0 {
                if bad == 0 {
                    let at = g.iter().zip(&port.frames[f].0).position(|(a, b)| a != b).unwrap();
                    out.push(format!("{label}: frame {f} differs first at byte {at:#x}: game {:02x} port {:02x}", g[at], port.frames[f].0[at]));
                }
                bad += 1;
            }
            let gi = std::slice::from_raw_parts(info.add(4 * f), 4);
            if gi != port.frames_additional_info[f] {
                if bad == 0 {
                    out.push(format!("{label}: frame {f}'s additional info: game {gi:02x?} port {:02x?}", port.frames_additional_info[f]));
                }
                bad += 1;
            }
            let inner: *const u8 = rd(wings, 0x18 * f);
            let inner_count = (rd::<usize>(wings, 0x18 * f + 8) - inner as usize) / 4;
            let gw = std::slice::from_raw_parts(inner, 4 * inner_count);
            let pw: Vec<u8> = port.frames_wings[f].iter().flatten().copied().collect();
            if gw != pw {
                if bad == 0 {
                    out.push(format!("{label}: frame {f}'s wings: game {gw:02x?} port {pw:02x?}"));
                }
                bad += 1;
            }
        }
        if bad != 0 {
            out.push(format!("{label}: {bad} frames differ"));
        }
        for (name, offset, value) in [("recordedFrames", 0xe0, port.recorded_frames), ("lapCacheIndex", 0x100, port.lap_cache_index), ("currentRecordingCue", 0x108, port.current_recording_cue), ("stdEngineLimiter", 0x10c, port.std_engine_limiter), ("version", 0xc8, port.version)] {
            let g: i32 = rd(game, offset);
            if g != value {
                out.push(format!("{label}: {name} {g} in the game, {value} in the port"));
            }
        }
        let cache: *const u32 = rd(game, 0xe8);
        let cache_count = (rd::<usize>(game, 0xf0) - cache as usize) / 4;
        if std::slice::from_raw_parts(cache, cache_count) != port.lap_frame_index_cache.as_slice() {
            out.push(format!("{label}: the lap frame cache differs"));
        }
    }

    unsafe fn compare_scene(&self, port: &ReplayManager, label: &str, out: &mut Vec<String>) {
        let global = self.acs.va(VA_FRAME_INFORMATION) as *const u8;
        let begin: *const u8 = rd(global, 0);
        let count = (rd::<usize>(global, 8) - begin as usize) / 0x20;
        if count != port.frame_information.len() {
            out.push(format!("{label}: {count} scene frames in the game, {} in the port", port.frame_information.len()));
            return;
        }
        let mut bad = 0;
        for f in 0..count {
            let e = begin.add(0x20 * f);
            let p = &port.frame_information[f];
            let objects: *const u16 = rd(e, 8);
            let object_count = (rd::<usize>(e, 0x10) - objects as usize) / 12;
            let game_objects = std::slice::from_raw_parts(objects, 6 * object_count);
            let port_objects: Vec<u16> = p.track_objects.iter().flatten().copied().collect();
            if rd::<u16>(e, 0) != p.sun_angle || rd::<i16>(e, 2) != p.lb_index || game_objects != port_objects {
                if bad == 0 {
                    out.push(format!("{label}: scene frame {f}: sun {:04x} / {:04x}, leaderboard {} / {}, objects {}", rd::<u16>(e, 0), p.sun_angle, rd::<i16>(e, 2), p.lb_index, if game_objects == port_objects { "the same" } else { "differ" }));
                }
                bad += 1;
            }
        }
        if bad != 0 {
            out.push(format!("{label}: {bad} scene frames differ"));
        }
    }

    unsafe fn compare_manager(&self, game: *mut u8, port: &ReplayManager, label: &str, out: &mut Vec<String>) {
        let ints = [("status", 0xbc, port.status), ("oldStatus", 0x108, port.old_status), ("recordedFrames", 0x14c, port.recorded_frames), ("currentRecordingIndex", 0x150, port.current_recording_index), ("maxFrames", 0x160, port.max_frames), ("cutOut", 0x168, port.cut_out), ("cutIn", 0x16c, port.cut_in), ("currentFrame", 0x174, port.current_frame)];
        for (name, offset, value) in ints {
            let g: i32 = rd(game, offset);
            if g != value {
                out.push(format!("{label}: {name} {g} in the game, {value} in the port"));
            }
        }
        let floats = [("timeMult", 0x130, port.time_mult), ("slowMoCounter", 0x134, port.slow_mo_counter), ("slowMoLevel", 0x138, port.slow_mo_level)];
        for (name, offset, value) in floats {
            let g: f32 = rd(game, offset);
            if g.to_bits() != value.to_bits() {
                out.push(format!("{label}: {name} {g} in the game, {value} in the port"));
            }
        }
        for (name, offset, value) in [("playCounter", 0x140, port.play_counter), ("lastRecordTime", 0x158, port.last_record_time)] {
            let g: f64 = rd(game, offset);
            if g.to_bits() != value.to_bits() {
                out.push(format!("{label}: {name} {g} in the game, {value} in the port"));
            }
        }
    }

    /// What was recorded, the game's against the port's. The answer: (frames compared, what
    /// differs).
    pub unsafe fn compare_recording(&self) -> (u64, Vec<String>) {
        let mut out = Vec::new();
        for (i, &recorder) in self.live.recorders.iter().enumerate() {
            self.compare_recorder(recorder, &self.port.recorders[i], &format!("car {i}"), &mut out);
        }
        self.compare_scene(&self.port, "recording", &mut out);
        self.compare_manager(self.live.manager, &self.port, "recording", &mut out);
        (self.max_frames as u64 * self.cars.len() as u64, out)
    }

    /// The cuts of the recording's manager on both sides: as `startReplayMode` leaves them
    /// (`entered`), then as `setCutInPerc` / `setCutOutPerc` set them.
    pub unsafe fn set_cuts(&mut self, cut: Option<(f32, f32)>, entered: bool) {
        let manager = self.live.manager;
        if entered {
            let last = rd::<i32>(manager, 0x14c) - 1;
            wr(manager, 0x16c, 0i32);
            wr(manager, 0x174, 0i32);
            wr(manager, 0x168, last);
            self.port.cut_in = 0;
            self.port.current_frame = 0;
            self.port.cut_out = self.port.recorded_frames - 1;
        }
        if let Some((cut_in, cut_out)) = cut {
            // (the cursor is past neither cut, so no recorder is asked for a state)
            let set_in: extern "C" fn(*mut u8, f32) -> f32 = std::mem::transmute(self.acs.va(VA_SET_CUT_IN_PERC));
            let set_out: extern "C" fn(*mut u8, f32) -> f32 = std::mem::transmute(self.acs.va(VA_SET_CUT_OUT_PERC));
            wr(manager, 0x174, ((rd::<i32>(manager, 0x14c) - 1) as f32 * 0.5) as i32);
            self.port.current_frame = ((self.port.recorded_frames - 1) as f32 * 0.5) as i32;
            set_in(manager, cut_in);
            set_out(manager, cut_out);
            self.port.set_cut_in_perc(cut_in);
            self.port.set_cut_out_perc(cut_out);
        }
    }

    /// The game's `Path::getDocumentPath`: where its files would go.
    unsafe fn document_path(&self) -> String {
        let get: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_GET_DOCUMENT_PATH));
        let text = self.acs.alloc(0x20);
        get(text);
        read_wstring(text)
    }

    fn save_info(&self) -> SaveInfo {
        let n = self.cars.len();
        SaveInfo {
            weather: "3_clear".to_string(),
            track: self.track_name.clone(),
            layout: self.layout.clone(),
            cars: (0..n).map(|i| CarHeader { unix_name: self.names[i].clone(), driver_name: format!("Driver {i}"), nation_code: ["ITA", "GBR", "ZZZZ"][i % 3].to_string(), team: format!("Team {}", i % 3), skin: format!("skin_{i}") }).collect(),
            wings: self.wings.iter().map(|w| *w as u32).collect(),
            guids: (0..n as i32).collect(),
        }
    }

    /// The game's `ReplayManager::save` and the port's: the two files (the game's is read
    /// back from the scratch root's stand-in Documents folder). `repo`: the repository, which
    /// must hold the folder the game writes into.
    pub unsafe fn save(&mut self, name: &str, repo: &std::path::Path) -> Result<(Vec<u8>, Vec<u8>), String> {
        // before the game opens a file: its Documents folder is the stand-in, a folder of the
        // scratch root, and nothing of the user's
        let documents = self.document_path();
        if documents != "oracle_no_documents" {
            return Err(format!("the game's Documents folder is {documents:?}, not the oracle's stand-in: nothing is saved"));
        }
        let folder = std::path::absolute(std::path::Path::new(&documents)).map_err(|e| e.to_string())?;
        if !folder.starts_with(repo) || folder.to_string_lossy().contains("Documents") {
            return Err(format!("{} is not inside the repository's scratch folders: nothing is saved", folder.display()));
        }
        // (the game makes `replay` and `replay/temp` itself, in a Documents folder that has an
        // `Assetto Corsa` folder)
        std::fs::create_dir_all(folder.join("Assetto Corsa")).map_err(|e| e.to_string())?;
        let file = folder.join("Assetto Corsa/replay").join(name);
        let _ = std::fs::remove_file(&file);
        let text = new_wstring(self.acs, name);
        let save: extern "C" fn(*mut u8, *mut u8, i32) -> bool = std::mem::transmute(self.acs.va(VA_SAVE));
        if !save(self.live.manager, text, 1) {
            return Err("the game's save said no".to_string());
        }
        let game = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        self.port.save_name(name, false).ok_or("the port refused the name")?;
        let info = self.save_info();
        let port = self.port.save_bytes(&info)?;
        Ok((game, port))
    }

    /// Two files byte by byte, but for the bytes the game never sets.
    pub fn compare_files(&self, game: &[u8], port: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        if game.len() != port.len() {
            out.push(format!("the game's file has {} bytes, the port's {}", game.len(), port.len()));
        }
        let skip: std::collections::HashSet<usize> = self.port.save_undefined.iter().copied().collect();
        let bad: Vec<usize> = (0..game.len().min(port.len())).filter(|at| game[*at] != port[*at] && !skip.contains(at)).collect();
        if let Some(first) = bad.first() {
            out.push(format!("{} bytes differ, the first at {first:#x}: game {:02x} port {:02x}", bad.len(), game[*first], port[*first]));
        }
        out
    }

    /// The saved file loaded by the game's `ReplayManager::load` into a second manager, and by
    /// the port's; then the recorders and the scene compared, and a playback run.
    pub unsafe fn load_and_play(&mut self, game_path: &str, port_file: &[u8], script: &[(Act, usize)], file_wings: Option<&[usize]>) -> Result<PlaybackReport, String> {
        let acs = self.acs;
        let n = file_wings.map_or(self.cars.len(), |wings| wings.len());
        // a new session with no car, as RaceManager::initReplay has it; the leaderboards of the
        // recording are not this session's
        let global = acs.va(VA_REPLAY_LEADERBOARDS) as *mut u8;
        wr(global, 8, rd::<usize>(global, 0));
        let sim = make_sim(acs, self.game, self.track_avatar, n);
        let manager = make_manager(acs, self.game, sim, self.max_frames);
        ADD_CAR.with(|c| *c.borrow_mut() = Some(AddCar { acs: acs as *const Acs as usize, game: self.game as usize, sim: sim as usize, cars: self.cars.iter().map(|c| *c as usize).collect(), wings: file_wings.map(|wings| wings.to_vec()), dirt: self.dirt, made: Vec::new() }));
        let path = new_wstring(acs, game_path);
        let load: extern "C" fn(*mut u8, *const u8) -> bool = std::mem::transmute(acs.va(VA_LOAD));
        let ok = load(manager, path);
        let made = ADD_CAR.with(|c| c.borrow_mut().take()).map(|c| c.made).unwrap_or_default();
        if !ok {
            return Err("the game's load said no".to_string());
        }
        let loaded = Session { manager, sim, avatars: made.iter().map(|m| m.0 as *mut u8).collect(), recorders: made.iter().map(|m| m.1 as *mut u8).collect() };
        // the port's
        struct Loader<'b> {
            limiters: &'b [i32],
            wings: &'b [usize],
            made: usize,
            told: Vec<String>,
        }
        impl ReplayLoader for Loader<'_> {
            fn load_track(&mut self, track: &str, layout: &str) {
                self.told.push(format!("track {track} layout {layout:?}"));
            }
            fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder {
                let i = self.made.min(self.limiters.len() - 1);
                self.made += 1;
                self.told.push(format!("car {} driver {:?} nation {:?} team {:?} skin {:?}", car.unix_name, car.driver_name, car.nation_code, car.team, car.skin));
                ReplayRecorder::new(self.limiters[i], self.wings[i])
            }
            fn apply_custom_weather(&mut self, weather: &str) {
                self.told.push(format!("weather {weather}"));
            }
        }
        // each loaded car's limiter (read by the game's recorder constructor), wing count and
        // tank, as the game's side has them
        let limiters: Vec<i32> = loaded.recorders.iter().map(|recorder| rd::<i32>(*recorder, 0x10c)).collect();
        let wings: Vec<usize> = loaded.avatars.iter().map(|avatar| wing_angles(rd(*avatar, 0x11b8)).len()).collect();
        let max_fuel: Vec<f64> = loaded.avatars.iter().map(|avatar| rd::<f64>(*avatar, 0xec8)).collect();
        if limiters.is_empty() {
            return Err("the game loaded no car".to_string());
        }
        let mut loader = Loader { limiters: &limiters, wings: &wings, made: 0, told: Vec::new() };
        let mut port = ReplayManager::new(None, self.port.record_interval_ms, Vec::new(), None, 0);
        port.max_frames = self.max_frames;
        port.load(port_file, &mut loader)?;
        let mut report = PlaybackReport { told: loader.told, ..PlaybackReport::default() };
        if loaded.recorders.len() != port.recorders.len() {
            return Err(format!("the game loaded {} cars, the port {}", loaded.recorders.len(), port.recorders.len()));
        }
        // what the game's load told its stand-in cars
        for (i, &avatar) in loaded.avatars.iter().enumerate() {
            report.told.push(format!("the game's car {i}: {} driver {:?} nation {:?} team {:?} skin {:?}", read_wstring(avatar.add(0x138)), read_wstring(avatar.add(0x10b8)), read_wstring(avatar.add(0x1118)), read_wstring(avatar.add(0x10d8)), read_wstring(avatar.add(0x1138))));
        }
        for (i, &recorder) in loaded.recorders.iter().enumerate() {
            self.compare_recorder(recorder, &port.recorders[i], &format!("loaded car {i}"), &mut report.differences);
        }
        self.compare_scene(&port, "loaded", &mut report.differences);
        if rd::<u8>(manager, 0xb8) != port.has_loaded_replay as u8 {
            report.differences.push("hasLoadedReplay differs".to_string());
        }
        // ReplayManager::startReplayMode, the manager's own part (the cameras, the GUI and the
        // sound are not here): the game's fields are set by hand as the function sets them
        port.start_replay_mode(0);
        wr(manager, 0x30, 1u8);
        wr(manager, 0x16c, 0i32);
        // (with no seconds to go back the cursor is put at the frame count, which is the
        // first frame again once it moves: 0x14015b562)
        wr(manager, 0x174, rd::<i32>(manager, 0x14c));
        wr(manager, 0x168, rd::<i32>(manager, 0x14c) - 1);
        wr(manager, 0x148, 1u8);
        wr(manager, 0xbc, 0i32);
        wr(manager, 0x130, 1.0f32);
        self.compare_manager(manager, &port, "after the start of the replay", &mut report.differences);
        let cars = loaded.recorders.len();
        let mut outs: Vec<CarPhysicsState> = (0..cars).map(|_| CarPhysicsState::from_game_bytes(&vec![0u8; STATE])).collect::<Result<_, _>>()?;
        let mut fx: Vec<PlaybackAvatar> = (0..cars).map(|i| PlaybackAvatar { max_fuel: max_fuel[i], has_dynamic_effects: self.dirt, dirt_level: None, car_node_active: true }).collect();
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE_PLAY_STATE));
        let call0 = |va: usize| {
            let f: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(va));
            f(manager);
        };
        let mut frame_number = 0usize;
        for &(act, frames) in script {
            match act {
                Act::Play => {
                    call0(VA_PLAY);
                    port.play();
                }
                Act::Pause => {
                    call0(VA_PAUSE);
                    port.pause();
                }
                Act::Stop => {
                    call0(VA_STOP);
                    port.stop();
                }
                Act::Rewind(k, step) => {
                    let f: extern "C" fn(*mut u8, i32, bool) = std::mem::transmute(acs.va(VA_REWIND));
                    f(manager, k, step);
                    port.rewind(k, step);
                }
                Act::FastForward(k, step) => {
                    let f: extern "C" fn(*mut u8, i32, bool) = std::mem::transmute(acs.va(VA_FAST_FORWARD));
                    f(manager, k, step);
                    port.fast_forward(k, step);
                }
                Act::SlowMotion(level) => {
                    let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SLOW_MOTION));
                    f(manager, level);
                    port.slow_motion(level);
                }
                Act::NextFrame => {
                    call0(VA_NEXT_FRAME);
                    port.next_frame();
                }
                Act::NextLap => {
                    call0(VA_NEXT_LAP);
                    port.next_lap(0);
                }
                Act::PreviousLap => {
                    call0(VA_PREVIOUS_LAP);
                    port.previous_lap(0);
                }
                Act::Position(p) => {
                    let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SET_NORMALIZED_POSITION));
                    f(manager, p);
                    port.set_normalized_position(p);
                }
                Act::CutIn(p) => {
                    let f: extern "C" fn(*mut u8, f32) -> f32 = std::mem::transmute(acs.va(VA_SET_CUT_IN_PERC));
                    let g = f(manager, p);
                    let q = port.set_cut_in_perc(p);
                    if g.to_bits() != q.to_bits() {
                        report.differences.push(format!("setCutInPerc({p}) answered {g} in the game, {q} in the port"));
                    }
                }
                Act::CutOut(p) => {
                    let f: extern "C" fn(*mut u8, f32) -> f32 = std::mem::transmute(acs.va(VA_SET_CUT_OUT_PERC));
                    let g = f(manager, p);
                    let q = port.set_cut_out_perc(p);
                    if g.to_bits() != q.to_bits() {
                        report.differences.push(format!("setCutOutPerc({p}) answered {g} in the game, {q} in the port"));
                    }
                }
            }
            for _ in 0..frames {
                // frame times of 60, 144 and 20 pictures a second, mixed
                let dt = [1.0f32 / 60.0, 1.0 / 60.0, 1.0 / 144.0, 0.05][frame_number % 4];
                frame_number += 1;
                update(manager, dt);
                let objects = port.update_play_state(dt);
                self.compare_manager(manager, &port, &format!("frame {frame_number} ({act:?})"), &mut report.differences);
                for i in 0..cars {
                    port.get_physics_state(i, &mut outs[i], &mut fx[i], &*self.port_track);
                    let avatar = loaded.avatars[i];
                    // (the port's track does not keep a surface's sector and collision
                    // category: they are left out of the comparison)
                    let mut game = CarPhysicsState::from_game_bytes(std::slice::from_raw_parts(avatar.add(AVATAR_STATE), STATE))?;
                    for wheel in 0..4 {
                        game.tyre_surface_def[wheel].sector_id = 0;
                        game.tyre_surface_def[wheel].collision_category = 0;
                        // (the game copies the surface's whole name buffer: what follows the
                        // name's end in it is left over from the track's loading)
                        let name = &mut game.tyre_surface_def[wheel].wav_string;
                        if let Some(end) = name.iter().position(|c| *c == 0) {
                            name[end..].fill(0);
                        }
                    }
                    if self.keep_tape {
                        if self.tapes.len() <= i {
                            self.tapes.resize(i + 1, Vec::new());
                        }
                        self.tapes[i].extend_from_slice(std::slice::from_raw_parts(avatar.add(AVATAR_STATE), STATE));
                    }
                    let (g, p) = (game.to_game_bytes(), outs[i].to_game_bytes());
                    report.states += 1;
                    if g != p {
                        report.bad_states += 1;
                        if report.differences.len() < 12 {
                            let at = g.iter().zip(&p).position(|(a, b)| a != b).unwrap();
                            report.differences.push(format!("frame {frame_number} ({act:?}) car {i}: the state differs first at {at:#x}: game {:02x?} port {:02x?} (cursor {}, blend {})", &g[at & !3..(at & !3) + 4], &p[at & !3..(at & !3) + 4], port.current_frame, port.recorders[i].frame_blend));
                        }
                    }
                    // the wings, the car's node, the dirt
                    let wings_begin: *const u8 = rd(avatar, 0xfa0);
                    let wing_count = (rd::<usize>(avatar, 0xfa8) - wings_begin as usize) / 0x44;
                    let game_wings: Vec<u32> = (0..wing_count).map(|w| rd::<u32>(wings_begin, 0x44 * w + 0xc)).collect();
                    let port_wings: Vec<u32> = port.recorders[i].get_wing_state().iter().map(|a| a.to_bits()).collect();
                    let node_active = rd::<u8>(rd::<*const u8>(avatar, 0x210), 0xd8) != 0;
                    let dirt_ok = !self.dirt || fx[i].dirt_level.map(f32::to_bits) == Some(rd::<u32>(rd::<*const u8>(loaded.recorders[i], 0x70), 0x78));
                    if game_wings != port_wings || node_active != fx[i].car_node_active || !dirt_ok {
                        report.bad_states += 1;
                        if report.differences.len() < 12 {
                            report.differences.push(format!("frame {frame_number} ({act:?}) car {i}: wings {game_wings:x?} / {port_wings:x?}, node {node_active} / {}, dirt ok {dirt_ok}", fx[i].car_node_active));
                        }
                    }
                }
                // the loose objects' nodes
                for (k, &object) in self.track_objects.iter().enumerate() {
                    let node: *const u8 = rd(object, 0x60);
                    let game: [u32; 16] = rd(node, 8);
                    let port_matrix: Vec<u32> = objects.get(k).map(|m| m.m.iter().flatten().map(|v| v.to_bits()).collect()).unwrap_or_default();
                    report.objects += 1;
                    if game.as_slice() != port_matrix.as_slice() {
                        report.bad_objects += 1;
                    }
                }
                report.frames += 1;
            }
        }
        report.last_frame = port.current_frame;
        report.recorded = port.recorded_frames;
        Ok(report)
    }
}

/// What a playback run found.
#[derive(Default)]
pub struct PlaybackReport {
    pub frames: u64,
    pub states: u64,
    pub bad_states: u64,
    pub objects: u64,
    pub bad_objects: u64,
    pub differences: Vec<String>,
    pub told: Vec<String>,
    pub last_frame: i32,
    pub recorded: i32,
}
