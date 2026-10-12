// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 24b: the game's own reading of a race.ini's cars.
//!
//! `RaceManager::initOffline` 0x14013a6c0 is run on a zeroed `RaceManager` with an `INIReader`
//! the game's own code loaded. Everything it reads of the file, the session loop, the model
//! carry-over, the skin test and the random skin (the game's `isSkinAvailable`,
//! `getModelSkins` and `ksRand` on the C runtime's `rand()`), the ballast gate and the grid
//! rule run as they are. What it would hand on to the rest of the game (`Sim::loadTrack`,
//! `Sim::addCar`, the `CarAvatar` setters, the queue of the physics thread, the timing
//! services, the weather) is replaced by small functions here that write down what they were
//! given. The result is one line per car, to be held against `rustyac_physics::session::race_grid`.

use std::cell::RefCell;
use std::path::Path;

use crate::acs::Acs;

const VA_INIT_OFFLINE: usize = 0x1_4013_a6c0; // bool RaceManager::initOffline(INIReaderDocuments&)
const VA_INI_READER_CTOR: usize = 0x1_4023_4010; // INIReader::INIReader()
const VA_INI_READER_LOAD: usize = 0x1_4023_7140; // INIReader::load(const std::wstring&)
const VA_INIT_LIGHTING: usize = 0x1_4013_a3d0; // RaceManager::initLighting(INIReader&)
const VA_SIM_LOAD_TRACK: usize = 0x1_4019_a4c0; // Sim::loadTrack(const wstring&, const wstring&, int)
const VA_GET_TRACK_AVATAR: usize = 0x1_4008_49c0; // GameObject::getGameObject<TrackAvatar>()
const VA_INIT_RESPAWN_SET: usize = 0x1_401c_b8c0; // TrackAvatar::initRespawnPositionSet(const wstring&)
const VA_GET_MAX_SLOTS: usize = 0x1_401c_8090; // int TrackAvatar::getMaxSlotsAvailable(const wstring&)
const VA_CRITICAL_ERROR: usize = 0x1_4002_4c70; // ks_critical_error(const wstring&)
const VA_GET_PHYSICS_AVATAR: usize = 0x1_4003_8ca0; // GameObject::getGameObject<PhysicsAvatar>()
const VA_SIM_ADD_CAR: usize = 0x1_4019_78a0; // CarAvatar* Sim::addCar(const wstring&, const wstring&, const wstring&)
const VA_ARM_FIRST_LAP: usize = 0x1_400d_2300; // CarAvatar::armFirstLap()
const VA_SET_BALLAST: usize = 0x1_400d_9de0; // CarAvatar::setBallastKG(float)
const VA_SET_RESTRICTOR: usize = 0x1_400d_a6b0; // CarAvatar::setRestrictor(float)
const VA_SET_DRIVER_INFO: usize = 0x1_400d_a0b0; // CarAvatar::setDriverInfo(const DriverInfo&)
const VA_AI_DRIVER_DRAG_CTOR: usize = 0x1_402a_b740; // AIDriverDrag::AIDriverDrag(Car*)
const VA_SET_CONTROLS_PROVIDER: usize = 0x1_400d_9f90; // CarAvatar::setControlsProvider(ICarControlsProvider*)
const VA_EXECUTE_ON_PHYSICS_THREAD: usize = 0x1_4012_1f20; // PhysicsAvatar::executeOnPhysicsThread(std::function<void()>)
const VA_SET_SPAWN_POSITION_INDEX: usize = 0x1_400d_a760; // CarAvatar::setSpawnPositionIndex(const wstring&, int)
const VA_RACE_TIMING_CTOR: usize = 0x1_4014_3c60; // RaceTimingServices::RaceTimingServices(Sim*)
const VA_APPLY_CUSTOM_WEATHER: usize = 0x1_4019_8010; // Sim::applyCustomWeather(const wstring&)
const VA_SET_CURRENT_SESSION: usize = 0x1_4014_0df0; // RaceManager::setCurrentSession(int)
const VA_SET_DYNAMIC_TEMP: usize = 0x1_4014_1210; // PhysicsEngine::setDynamicTempData(const DynamicTempData&)
const VA_RESET_CURRENT_LAPS: usize = 0x1_4014_0300; // RaceManager::resetCurrentLaps()
const VA_RESET_MANDATORY_PIT: usize = 0x1_4014_0600; // RaceManager::resetMandatoryPit()
const VA_AI_LAMBDA_VTABLE: usize = 0x1_404c_bd38; // the std::function implementation of the lambda that makes a car's AIDriver
const VA_CARS_TO_BE_LOADED: usize = 0x1_4155_9b00; // static int RaceManager::carsToBeLoaded
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized

/// One car as the game's code handed it on.
#[derive(Clone, Debug, Default)]
pub struct GameCar {
    pub model: String,
    pub config: String,
    pub skin: String,
    pub ballast_kg: f32,
    pub restrictor: f32,
    pub driver_name: String,
    pub nation_code: String,
    /// the AI lambda's captures: the car's level, the race's level, the aggression (`None`:
    /// the car got no AI)
    pub ai: Option<(f32, f32, f32)>,
    /// the last `setSpawnPositionIndex("START", n)` (`Sim::addCar` sets the car's number first)
    pub start_index: i32,
    pub armed_first_lap: bool,
    avatar: usize,
}

#[derive(Default)]
struct Seen {
    cars: Vec<GameCar>,
    notes: Vec<String>,
    track_avatar: usize,
    physics_avatar: usize,
    sim: usize,
    pit_slots: i32,
    ai_vtable: usize,
}

thread_local! {
    static SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

pub unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    std::ptr::read_unaligned(base.add(offset) as *const T)
}

pub unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    std::ptr::write_unaligned(base.add(offset) as *mut T, value);
}

/// An MSVC `std::wstring` as text.
pub unsafe fn text(s: *const u8) -> String {
    if s.is_null() {
        return String::new();
    }
    let (size, capacity): (usize, usize) = (rd(s, 0x10), rd(s, 0x18));
    let data: *const u16 = if capacity >= 8 { rd(s, 0) } else { s.cast() };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, size))
}

pub unsafe fn write_wstring(at: *mut u8, value: &str) {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() < 8 {
        std::ptr::write_bytes(at, 0, 0x10);
        std::ptr::copy_nonoverlapping(units.as_ptr(), at as *mut u16, units.len());
        wr(at, 0x10, units.len());
        wr(at, 0x18, 7usize);
    } else {
        let buffer = Box::leak(vec![0u16; units.len() + 1].into_boxed_slice());
        buffer[..units.len()].copy_from_slice(&units);
        wr(at, 0, buffer.as_ptr());
        wr(at, 0x10, units.len());
        wr(at, 0x18, units.len());
    }
}

pub fn zeroed(size: usize) -> *mut u8 {
    Box::leak(vec![0u64; size.div_ceil(8)].into_boxed_slice()).as_mut_ptr().cast()
}

fn car_of(seen: &mut Seen, avatar: *mut u8) -> Option<&mut GameCar> {
    seen.cars.iter_mut().find(|c| c.avatar == avatar as usize)
}

extern "C" fn nothing() {}

extern "C" fn load_track(_sim: *mut u8, track: *const u8, config: *const u8, _flag: i32) {
    let line = unsafe { format!("Sim::loadTrack({:?}, {:?})", text(track), text(config)) };
    SEEN.with(|s| s.borrow_mut().notes.push(line));
}

extern "C" fn get_track_avatar(_sim: *mut u8) -> *mut u8 {
    SEEN.with(|s| s.borrow().track_avatar as *mut u8)
}

extern "C" fn get_physics_avatar(_sim: *mut u8) -> *mut u8 {
    SEEN.with(|s| s.borrow().physics_avatar as *mut u8)
}

extern "C" fn init_respawn_set(_track: *mut u8, set: *const u8) {
    let line = unsafe { format!("TrackAvatar::initRespawnPositionSet({:?})", text(set)) };
    SEEN.with(|s| s.borrow_mut().notes.push(line));
}

extern "C" fn get_max_slots(_track: *mut u8, _set: *const u8) -> i32 {
    SEEN.with(|s| s.borrow().pit_slots)
}

extern "C" fn critical_error(message: *const u8) {
    let line = unsafe { format!("ks_critical_error({:?})", text(message)) };
    SEEN.with(|s| s.borrow_mut().notes.push(line));
}

extern "C" fn add_car(sim: *mut u8, model: *const u8, config: *const u8, skin: *const u8) -> *mut u8 {
    unsafe {
        // a CarAvatar of which the caller reads the guid, the name and the power class, and
        // whose `physics` stays null
        let avatar = zeroed(0x12a8);
        let model = text(model);
        write_wstring(avatar.add(0x138), &model);
        wr(avatar, 0xf80, zeroed(16));
        SEEN.with(|s| {
            let mut seen = s.borrow_mut();
            let index = seen.cars.len();
            wr(avatar, 0x1158, index as i32);
            // Sim::cars.push_back
            let list: *mut u8 = rd(sim, 0x208);
            wr(list, 8 * index, avatar);
            wr(sim, 0x210, list.add(8 * (index + 1)));
            seen.cars.push(GameCar { model, config: text(config), skin: text(skin), start_index: index as i32, avatar: avatar as usize, ..GameCar::default() });
        });
        avatar
    }
}

extern "C" fn arm_first_lap(avatar: *mut u8) {
    SEEN.with(|s| {
        if let Some(car) = car_of(&mut s.borrow_mut(), avatar) {
            car.armed_first_lap = true;
        }
    });
}

extern "C" fn set_ballast(avatar: *mut u8, kg: f32) {
    SEEN.with(|s| {
        if let Some(car) = car_of(&mut s.borrow_mut(), avatar) {
            car.ballast_kg = kg;
        }
    });
}

extern "C" fn set_restrictor(avatar: *mut u8, value: f32) {
    SEEN.with(|s| {
        if let Some(car) = car_of(&mut s.borrow_mut(), avatar) {
            car.restrictor = value;
        }
    });
}

extern "C" fn set_driver_info(avatar: *mut u8, info: *const u8) {
    let (name, nation) = unsafe { (text(info), text(info.add(0x60))) };
    SEEN.with(|s| {
        if let Some(car) = car_of(&mut s.borrow_mut(), avatar) {
            car.driver_name = name;
            car.nation_code = nation;
        }
    });
}

extern "C" fn ai_driver_drag(memory: *mut u8, _car: *mut u8) -> *mut u8 {
    memory
}

extern "C" fn set_controls_provider(avatar: *mut u8, provider: *const u8) {
    // (the drag driver: its `push` is the car's AI_LEVEL as read)
    let push: f32 = unsafe { rd(provider, 0x18) };
    SEEN.with(|s| {
        if let Some(car) = car_of(&mut s.borrow_mut(), avatar) {
            car.ai = Some((push, f32::NAN, f32::NAN));
        }
    });
}

extern "C" fn execute_on_physics_thread(_physics_avatar: *mut u8, function: *const u8) {
    unsafe {
        let implementation: *const u8 = rd(function, 0x18);
        if implementation.is_null() {
            return;
        }
        SEEN.with(|s| {
            let mut seen = s.borrow_mut();
            if rd::<usize>(implementation, 0) == seen.ai_vtable {
                let (car_ai, race_ai, avatar, aggression): (f32, f32, *mut u8, f32) = (rd(implementation, 8), rd(implementation, 0xc), rd(implementation, 0x10), rd(implementation, 0x18));
                if let Some(car) = car_of(&mut seen, avatar) {
                    car.ai = Some((car_ai, race_ai, aggression));
                }
            }
        });
    }
}

extern "C" fn set_spawn_position_index(avatar: *mut u8, set: *const u8, index: i32) {
    let set = unsafe { text(set) };
    SEEN.with(|s| {
        let mut seen = s.borrow_mut();
        if set == "START" {
            if let Some(car) = car_of(&mut seen, avatar) {
                car.start_index = index;
            }
        } else {
            seen.notes.push(format!("setSpawnPositionIndex({set:?}, {index})"));
        }
    });
}

extern "C" fn race_timing(memory: *mut u8, _sim: *mut u8) -> *mut u8 {
    unsafe { std::ptr::write_bytes(memory, 0, 0xc0) };
    memory
}

extern "C" fn apply_custom_weather(_sim: *mut u8, name: *const u8) {
    let line = unsafe { format!("Sim::applyCustomWeather({:?})", text(name)) };
    SEEN.with(|s| s.borrow_mut().notes.push(line));
}

extern "C" fn set_current_session(_manager: *mut u8, index: i32) {
    SEEN.with(|s| s.borrow_mut().notes.push(format!("RaceManager::setCurrentSession({index})")));
}

/// What the game's own `initOffline` made of a race.ini.
pub struct GameReading {
    pub cars_to_be_loaded: i32,
    pub cars: Vec<GameCar>,
    pub notes: Vec<String>,
    /// the sessions it listed: (type, forced position)
    pub sessions: Vec<(i32, i32)>,
}

/// Runs the game's `RaceManager::initOffline` on `ini` (a path the game's code can open). The
/// current directory must hold `content/cars/<model>/skins/<skin>` folders for the cars of the
/// file (empty folders do). `seed`: the C runtime's `srand` right before the call.
pub unsafe fn read(acs: &Acs, ini: &Path, pit_slots: i32, seed: u32) -> Result<GameReading, String> {
    acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);
    // jump to a function of ours: mov rax, imm64; jmp rax
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_INIT_LIGHTING, nothing as *const () as usize);
    stub(VA_SIM_LOAD_TRACK, load_track as *const () as usize);
    stub(VA_GET_TRACK_AVATAR, get_track_avatar as *const () as usize);
    stub(VA_INIT_RESPAWN_SET, init_respawn_set as *const () as usize);
    stub(VA_GET_MAX_SLOTS, get_max_slots as *const () as usize);
    stub(VA_CRITICAL_ERROR, critical_error as *const () as usize);
    stub(VA_GET_PHYSICS_AVATAR, get_physics_avatar as *const () as usize);
    stub(VA_SIM_ADD_CAR, add_car as *const () as usize);
    stub(VA_ARM_FIRST_LAP, arm_first_lap as *const () as usize);
    stub(VA_SET_BALLAST, set_ballast as *const () as usize);
    stub(VA_SET_RESTRICTOR, set_restrictor as *const () as usize);
    stub(VA_SET_DRIVER_INFO, set_driver_info as *const () as usize);
    stub(VA_AI_DRIVER_DRAG_CTOR, ai_driver_drag as *const () as usize);
    stub(VA_SET_CONTROLS_PROVIDER, set_controls_provider as *const () as usize);
    stub(VA_EXECUTE_ON_PHYSICS_THREAD, execute_on_physics_thread as *const () as usize);
    stub(VA_SET_SPAWN_POSITION_INDEX, set_spawn_position_index as *const () as usize);
    stub(VA_RACE_TIMING_CTOR, race_timing as *const () as usize);
    stub(VA_APPLY_CUSTOM_WEATHER, apply_custom_weather as *const () as usize);
    stub(VA_SET_CURRENT_SESSION, set_current_session as *const () as usize);
    stub(VA_SET_DYNAMIC_TEMP, nothing as *const () as usize);
    stub(VA_RESET_CURRENT_LAPS, nothing as *const () as usize);
    stub(VA_RESET_MANDATORY_PIT, nothing as *const () as usize);

    let manager = zeroed(0x218);
    let sim = zeroed(0x2d0);
    let physics_avatar = zeroed(0x498);
    wr(manager, 0x168, sim);
    wr(sim, 0x1b8, physics_avatar);
    // Sim::cars: room for more cars than any track has boxes
    let list = zeroed(8 * 256);
    wr(sim, 0x208, list);
    wr(sim, 0x210, list);
    wr(sim, 0x218, list.add(8 * 256));
    SEEN.with(|s| {
        *s.borrow_mut() = Seen { track_avatar: zeroed(0x400) as usize, physics_avatar: physics_avatar as usize, sim: sim as usize, pit_slots, ai_vtable: acs.va(VA_AI_LAMBDA_VTABLE), ..Seen::default() };
    });

    // the file, read by the game's own INIReader
    let reader = zeroed(0x68);
    let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_INI_READER_CTOR));
    ctor(reader);
    let path = zeroed(0x20);
    write_wstring(path, &ini.to_string_lossy());
    let load: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_INI_READER_LOAD));
    load(reader, path);
    if rd::<u8>(reader, 8) == 0 {
        return Err(format!("the game's INIReader could not read {}", ini.display()));
    }
    let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
    srand(seed);
    let init_offline: extern "C" fn(*mut u8, *mut u8) -> u8 = std::mem::transmute(acs.va(VA_INIT_OFFLINE));
    init_offline(manager, reader);

    // RaceManager::sessions (+0x188): records of 0x70 bytes, the type at +0, forcedPosition at +0x60
    let (begin, end): (*const u8, *const u8) = (rd(manager, 0x188), rd(manager, 0x190));
    let mut sessions = Vec::new();
    let mut at = begin;
    while !at.is_null() && at < end {
        sessions.push((rd::<i32>(at, 0), rd::<i32>(at, 0x60)));
        at = at.add(0x70);
    }
    let seen = SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let _ = seen.sim;
    Ok(GameReading { cars_to_be_loaded: acs.global::<i32>(VA_CARS_TO_BE_LOADED), cars: seen.cars, notes: seen.notes, sessions })
}
