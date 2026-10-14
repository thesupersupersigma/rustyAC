// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the apps' proofs: acs.exe's own app forms (`SystemApps.lib`), each made by
//! its own constructor and put on the game screen as `Sim::addForm` 0x140197ae0 does.
//!
//! What stands in for the game around them: a zeroed `Game` with its graphics, its GUI and its
//! clock (`Game+0x18`); a zeroed `Sim` with the members the apps read (the game screen, the
//! cars, the focused car, the connected cars, a replay manager's "in replay" byte, a
//! `RaceManager` block); zeroed `CarAvatar` blocks whose `physicsState` (+0x268) holds the
//! tapes' bytes as they are; and the `RaceManager` getters the apps call, replaced by ones
//! that answer from `gui_seq_apps::link()`, which the port reads too.

use std::sync::Mutex;

use rustyac_gui::apps::AppLink;
use rustyac_gui::leaderboard::RaceLap;

use crate::ac::{rd, wr, wstring, Game};
use crate::gui_ac_hud::jump;

/// name, the class's size, constructor, table
pub const APPS: [(&str, usize, usize, usize); 20] = [
    ("eyes", 0x290, 0x1_4030_fa60, 0x1_4050_a450),
    ("mgu", 0x1e0, 0x1_402e_b080, 0x1_4050_45b0),
    ("ballast", 0x1c0, 0x1_402f_0ef0, 0x1_4050_50b0),
    ("ffb", 0x268, 0x1_4031_4a10, 0x1_4050_b110),
    ("volumes", 0x258, 0x1_402f_8fd0, 0x1_4050_65d0),
    ("help", 0x1c0, 0x1_4030_7900, 0x1_4050_87c8),
    ("hideall", 0x1d0, 0x1_4030_f3e0, 0x1_4050_9f38),
    ("runningtime", 0x1d0, 0x1_402d_c760, 0x1_4050_24c0),
    ("timeflow", 0x290, 0x1_4030_cb10, 0x1_4050_9678),
    ("trackdescription", 0x1c8, 0x1_4032_70e0, 0x1_4050_dde8),
    ("trackmap", 0x368, 0x1_402e_e8e0, 0x1_4050_4db0),
    ("gears", 0x3d8, 0x1_402e_8280, 0x1_4050_3dd0),
    ("tyres", 0x4f0, 0x1_4033_8860, 0x1_4051_1d38),
    ("pedals", 0x1f0, 0x1_402d_bc80, 0x1_4050_2340),
    ("electronics", 0x1e0, 0x1_402e_a500, 0x1_4050_42e8),
    ("realtime", 0x318, 0x1_4032_b8a0, 0x1_4050_efb8),
    ("laps", 0x2f8, 0x1_402d_9660, 0x1_4050_1e50),
    ("position", 0x200, 0x1_402f_e0f0, 0x1_4050_7100),
    ("essentials", 0x2a0, 0x1_4031_bc60, 0x1_4050_c430),
    ("delta", 0x200, 0x1_4030_2570, 0x1_4050_7c10),
];
/// the apps whose constructor takes the player's `CarAvatar` (the others take `Sim`)
const TAKES_CAR: [&str; 9] = ["delta", "tyres", "pedals", "electronics", "gears", "ffb", "mgu", "ballast", "eyes"];
/// `const DICarControl::vftable'`
const VA_DICARCONTROL_VFTABLE: usize = 0x1_404b_2098;
const VA_SPEED_USE_MPH: usize = 0x1_4155_a5ae;
/// `static std::wstring INIReaderDocuments::basePath`
const VA_INIREADERDOCUMENTS_BASE_PATH: usize = 0x1_4151_d100;

/// The apps' tables for `gui_ac::class_of`: every app's form is a "Form" in the dumps.
pub fn classes() -> Vec<(&'static str, usize, usize, usize)> {
    APPS.iter().filter(|a| a.3 != 0).map(|a| ("Form", a.1, a.2, a.3)).collect()
}

struct AppGame {
    game_block: usize,
    sim: usize,
    cars: Vec<usize>,
    replay: usize,
    made: Vec<(String, usize)>,
}

static GAME: Mutex<Option<AppGame>> = Mutex::new(None);

/// The Audio app's session: `CarAudioFMOD` blocks (their `mixVolumes` at +0x118), the
/// drivable cameras' definitions (0x38 a car), the master level's calls, the last line of
/// `effects_game.txt`.
#[derive(Default)]
struct Sound {
    audio: Vec<usize>,
    defs: usize,
    log: Vec<String>,
    last: String,
}

static SOUND: Mutex<Option<Sound>> = Mutex::new(None);

/// The force feedback app's session: the player's `Car` block (`CarAvatar+0x1168`), its
/// `DICarControl` (a block with the class's own table, for the app's `__RTDynamicCast`), the
/// avatar; the messages; the last line of `effects_game.txt`.
#[derive(Default)]
struct Ffb {
    car: usize,
    dic: usize,
    avatar: usize,
    log: Vec<String>,
    last: String,
}

static FFB: Mutex<Option<Ffb>> = Mutex::new(None);

/// Onboard settings' session: the avatar, `CameraOnBoard`, the messages, the last line.
#[derive(Default)]
struct EyesGame {
    avatar: usize,
    cob: usize,
    log: Vec<String>,
    last: String,
}

static EYES: Mutex<Option<EyesGame>> = Mutex::new(None);

/// The player's `Car` block of the MGU and ballast apps, the last line of `effects_game.txt`.
static BALLAST: Mutex<Option<(usize, String)>> = Mutex::new(None);

/// `PhysicsAvatar::executeOnPhysicsThread(std::function<void()>)` 0x140121f20: the job done at
/// once (the oracle has no physics thread), the function given by value destroyed as the
/// game's does it (`_Do_call` +0x10, `_Delete_this` +0x20 of its implementation at +0x18).
unsafe extern "C" fn execute_on_physics_thread(_this: *mut u8, f: *mut u8) {
    let imp: *mut u8 = rd(f, 0x18);
    if imp.is_null() {
        return;
    }
    let table: *const usize = rd(imp, 0);
    let call: extern "C" fn(*mut u8) = std::mem::transmute(*table.add(2));
    call(imp);
    let delete: extern "C" fn(*mut u8, bool) = std::mem::transmute(*table.add(4));
    delete(imp, imp != f);
    wr(f, 0x18, 0usize);
}

/// `SystemMessage::addMessage(std::wstring, std::wstring, eMessageType)` 0x1401b61c0: written
/// down.
unsafe extern "C" fn add_message(_this: *mut u8, title: *const u8, text: *const u8, _kind: i32) {
    let line = format!("message {}|{}", crate::gui_ac::read_wstring(title), crate::gui_ac::read_wstring(text));
    if let Some(f) = FFB.lock().unwrap().as_mut() {
        f.log.push(line);
    } else if let Some(e) = EYES.lock().unwrap().as_mut() {
        e.log.push(line);
    }
}

/// `AudioEngine::setVolume` 0x1401fbdf0: the call, written down.
unsafe extern "C" fn set_volume(_this: *mut u8, v: f32) {
    if let Some(s) = SOUND.lock().unwrap().as_mut() {
        s.log.push(format!("master {:08x}", v.to_bits()));
    }
}

/// `AudioEngine::getDriverNum` 0x1401f9830: the script's sound cards.
unsafe extern "C" fn get_driver_num(_this: *mut u8) -> i32 {
    crate::gui_seq_apps::script(|s| s.drivers.len() as i32)
}

/// `AudioEngine::getDriverInfo(int)` 0x1401f96d0.
unsafe extern "C" fn get_driver_info(_this: *mut u8, out: *mut u8, i: i32) -> *mut u8 {
    let name = crate::gui_seq_apps::script(|s| s.drivers.get(i as usize).cloned().unwrap_or_default());
    empty_wstring(out);
    crate::ac::write_wstring(crate::gui_ac_hud::acs(), out, &name);
    out
}
/// where `Speed::useMPH` is in the loaded game
static GAME_ACS: Mutex<Option<(usize,)>> = Mutex::new(None);

fn link() -> AppLink {
    crate::gui_seq_apps::link()
}

unsafe fn car_index(car: *const u8) -> usize {
    GAME.lock().unwrap().as_ref().and_then(|g| g.cars.iter().position(|c| *c == car as usize)).unwrap_or(0)
}

unsafe fn with_car<T: Default>(car: *const u8, f: impl FnOnce(&rustyac_gui::apps::AppCar) -> T) -> T {
    let index = car_index(car);
    link().cars.get(index).map(f).unwrap_or_default()
}

unsafe fn empty_wstring(at: *mut u8) {
    std::ptr::write_bytes(at, 0, 0x20);
    wr(at, 0x18, 7usize);
}

/// A `Session` (0x70 bytes) of the running session.
unsafe fn write_session(out: *mut u8, l: &AppLink) {
    std::ptr::write_bytes(out, 0, 0x70);
    wr(out, 0, l.session_type);
    wr(out, 4, l.is_timed_race as u8);
    wr(out, 0x8, l.session_laps);
    empty_wstring(out.add(0x18));
    empty_wstring(out.add(0x38));
}

unsafe extern "C" fn get_current_session(_this: *mut u8, out: *mut u8) -> *mut u8 {
    write_session(out, &link());
    out
}

unsafe extern "C" fn get_current_session_type(_this: *mut u8) -> i32 {
    link().session_type
}

unsafe extern "C" fn get_time_to_session_start(_this: *mut u8) -> f64 {
    link().time_to_session_start
}

unsafe extern "C" fn get_session_time_left(_this: *mut u8) -> f64 {
    link().session_time_left
}

unsafe extern "C" fn get_current_lap(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    crate::gui_ac_race::write_lap(out, &with_car(car, |c| c.current_lap.clone()));
    out
}

unsafe extern "C" fn get_best_lap(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    crate::gui_ac_race::write_lap(out, &with_car(car, |c| c.best_lap.clone()));
    out
}

unsafe extern "C" fn get_last_lap(_this: *mut u8, out: *mut u8, car: *mut u8) -> *mut u8 {
    crate::gui_ac_race::write_lap(out, &with_car::<RaceLap>(car, |c| c.last_lap.clone()));
    out
}

unsafe extern "C" fn get_split(_this: *mut u8, car: *mut u8) -> i32 {
    with_car(car, |c| c.split)
}

unsafe extern "C" fn is_current_lap_valid(this: *mut u8) -> bool {
    with_car(this, |c| c.current_lap_valid)
}

unsafe extern "C" fn get_car_leaderboard_position(_this: *mut u8, car: *mut u8) -> i32 {
    with_car(car, |c| c.leaderboard_position)
}

unsafe extern "C" fn get_car_real_time_position(_this: *mut u8, car: *mut u8) -> i32 {
    with_car(car, |c| c.realtime_position)
}

unsafe extern "C" fn get_lap_count(_this: *mut u8, car: *mut u8) -> u32 {
    with_car(car, |c| c.lap_count)
}

unsafe extern "C" fn is_abs_enabled(this: *mut u8) -> bool {
    with_car(this, |c| c.abs_enabled)
}

unsafe extern "C" fn is_abs_in_action(this: *mut u8) -> bool {
    with_car(this, |c| c.abs_in_action)
}

unsafe extern "C" fn is_tc_enabled(this: *mut u8) -> bool {
    with_car(this, |c| c.tc_enabled)
}

unsafe extern "C" fn is_tc_in_action(this: *mut u8) -> bool {
    with_car(this, |c| c.tc_in_action)
}

unsafe extern "C" fn get_session_count(_this: *mut u8) -> u32 {
    link().session_count
}

unsafe extern "C" fn get_time_to_wait(_this: *mut u8) -> f64 {
    link().time_to_wait
}

unsafe extern "C" fn get_current_compound_short_name(this: *mut u8, out: *mut u8) -> *mut u8 {
    empty_wstring(out);
    crate::ac::write_wstring(crate::gui_ac_hud::acs(), out, &with_car(this, |c| c.compound_short_name.clone()));
    out
}

unsafe extern "C" fn get_current_compound_ideal_pressure(this: *mut u8, tyre: u16) -> i32 {
    with_car(this, |c| c.ideal_pressure.get(tyre as usize).copied().unwrap_or(0))
}

unsafe extern "C" fn is_edl_enabled(this: *mut u8) -> bool {
    with_car(this, |c| c.edl_enabled)
}

unsafe extern "C" fn get_edl_out_level(this: *mut u8) -> f32 {
    with_car(this, |c| c.edl_out_level)
}

/// The controls provider of the cars that have one: an object whose table's slot +0x28
/// (`getFFGlobalGain`) answers the script's gain.
static PROVIDERS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

unsafe extern "C" fn ff_global_gain(this: *mut u8) -> f32 {
    let index = PROVIDERS.lock().unwrap().iter().position(|p| *p == this as usize).unwrap_or(0);
    link().cars.get(index).and_then(|c| c.ff_global_gain).unwrap_or(0.0)
}

unsafe extern "C" fn get_controls_provider(this: *mut u8) -> *mut u8 {
    let index = car_index(this);
    if link().cars.get(index).is_some_and(|c| c.ff_global_gain.is_some()) {
        PROVIDERS.lock().unwrap().get(index).copied().unwrap_or(0) as *mut u8
    } else {
        std::ptr::null_mut()
    }
}

/// `TrackAvatar::getAISpline` 0x1401c8010: the fake track's recorder's best lap spline.
static RECORDER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn get_ai_spline(_this: *mut u8) -> *mut u8 {
    (RECORDER.load(std::sync::atomic::Ordering::SeqCst) as *mut u8).add(0xf0)
}

unsafe fn install(game: &Game) {
    // the static `gears` of ACEssentials.obj ("R", "N", "1" …)
    let init: extern "C" fn() = std::mem::transmute(game.acs.va(0x1_4001_5240));
    init();
    // the language (acLocaleGetCurrentLocale's `currentLocale`): ACHelp reads help/<it>.tag
    crate::ac::write_wstring(&game.acs, game.acs.va(0x1_4151_d190) as *mut u8, "en");
    // FormGear.obj's own `gears` ("R", "N", "1" … "10")
    let init: extern "C" fn() = std::mem::transmute(game.acs.va(0x1_4001_2b60));
    init();
    for (va, to) in [
        (0x1_400d_8ab0, is_abs_enabled as *const () as usize),
        (0x1_400d_8ae0, is_abs_in_action as *const () as usize),
        (0x1_400d_8db0, is_tc_enabled as *const () as usize),
        (0x1_400d_8de0, is_tc_in_action as *const () as usize),
        (0x1_4013_9ca0, get_session_count as *const () as usize),
        (0x1_400d_3200, get_current_compound_short_name as *const () as usize),
        (0x1_400d_3180, get_current_compound_ideal_pressure as *const () as usize),
        (0x1_400d_8ba0, is_edl_enabled as *const () as usize),
        (0x1_400d_3470, get_edl_out_level as *const () as usize),
        (0x1_400d_3160, get_controls_provider as *const () as usize),
        (0x1_401c_8010, get_ai_spline as *const () as usize),
        (0x1_4013_a130, get_time_to_wait as *const () as usize),
        (0x1_4013_8220, get_current_session as *const () as usize),
        (0x1_4013_8300, get_current_session_type as *const () as usize),
        (0x1_4013_a0a0, get_time_to_session_start as *const () as usize),
        (0x1_4013_9e60, get_session_time_left as *const () as usize),
        (0x1_4013_80b0, get_current_lap as *const () as usize),
        (0x1_4013_7c00, get_best_lap as *const () as usize),
        (0x1_4013_87c0, get_last_lap as *const () as usize),
        (0x1_4013_a010, get_split as *const () as usize),
        (0x1_400d_8b50, is_current_lap_valid as *const () as usize),
        (0x1_4013_7ee0, get_car_leaderboard_position as *const () as usize),
        (0x1_4013_7f90, get_car_real_time_position as *const () as usize),
        (0x1_4013_8570, get_lap_count as *const () as usize),
        (0x1_4024_0cf0, crate::gui_ac_hud::ac_translate as *const () as usize),
        (0x1_401f_bdf0, set_volume as *const () as usize),
        (0x1_4012_1f20, execute_on_physics_thread as *const () as usize),
        (0x1_401b_61c0, add_message as *const () as usize),
        (0x1_401f_9830, get_driver_num as *const () as usize),
        (0x1_401f_96d0, get_driver_info as *const () as usize),
    ] {
        jump(game, va, to);
    }
}

fn app(name: &str) -> Result<*mut u8, String> {
    GAME.lock().unwrap().as_ref().and_then(|g| g.made.iter().find(|a| a.0 == name).map(|a| a.1 as *mut u8)).ok_or(format!("no app {name:?} was made"))
}

/// `RaceManager::evOnLapCompleted` for the laps the script's last move finished: the game's
/// handlers of the apps that listen (`FormRealTimeTiming`'s lambda 0x14032c7b0, its object
/// {form, sim}).
pub unsafe fn announce_laps(game: &Game) {
    let laps = crate::gui_seq_apps::take_laps();
    let made: Vec<(String, usize)> = GAME.lock().unwrap().as_ref().map(|g| g.made.clone()).unwrap_or_default();
    let sim = GAME.lock().unwrap().as_ref().map(|g| g.sim).unwrap_or(0);
    for (car, time) in laps {
        for (name, form) in &made {
            if name == "ffb" {
                // FFBForm's lap handler 0x1403167d0, its object {form}
                let lambda = game.acs.alloc(0x10);
                wr(lambda, 0, *form);
                let event = game.acs.alloc(0x8);
                wr(event, 0, car);
                wr(event, 4, time);
                let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(game.acs.va(0x1_4031_67d0));
                f(lambda, event);
            }
            if name == "realtime" {
                let lambda = game.acs.alloc(0x10);
                wr(lambda, 0, *form);
                wr(lambda, 8, sim);
                let event = game.acs.alloc(0x8);
                wr(event, 0, car);
                wr(event, 4, time);
                let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(game.acs.va(0x1_4032_c7b0));
                f(lambda, event);
            }
        }
    }
}

static SUN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The sun's angle an app script set, in `GraphicsManager::lightingSettings.angle` around the
/// GUI's pass only (ACTimeFlow reads it there; the scene's lighting, made at the frame's
/// start, keeps its own).
pub unsafe fn sun(game: &Game, on: bool) {
    let Some(angle) = crate::gui_seq_apps::sun() else { return };
    if on {
        SUN.store(rd::<u32>(game.graphics, 0xd4), std::sync::atomic::Ordering::SeqCst);
        wr(game.graphics, 0xd4, angle);
    } else {
        wr(game.graphics, 0xd4, SUN.load(std::sync::atomic::Ordering::SeqCst));
    }
}

pub unsafe fn call(game: &Game, gui: *mut u8, screen: *mut u8, what: &str, number: f32, text: &str) -> Result<(), String> {
    let a = &game.acs;
    if what == "app.create" {
        crate::gui_ac_hud::set_acs(&game.acs);
        install(game);
        *GAME_ACS.lock().unwrap() = Some((a.va(VA_SPEED_USE_MPH) as usize,));
        let game_block = a.alloc(0x300);
        wr(game_block, 0x138, game.graphics);
        wr(game_block, 0x180, gui);
        let sim = a.alloc(0x2d0);
        wr(sim, 0x8, game_block);
        wr(sim, 0x108, screen);
        wr(sim, 0x1a8, a.alloc(0x400));
        // MicroSectors (0x90), switched off: its strip is not ported
        wr(sim, 0x1f0, a.alloc(0x90));
        // the track: its AI spline (TrackAvatar+0x178 -> AISplineRecorder, getBestLapSpline = +0xf0,
        // its Spline at +0x50: points begin / end, length at +0x20)
        let track = a.alloc(0x400);
        let recorder = a.alloc(0x200);
        wr(track, 0x178, recorder);
        RECORDER.store(recorder as usize, std::sync::atomic::Ordering::SeqCst);
        empty_wstring(track.add(0x10));
        empty_wstring(track.add(0xa0));
        wr(recorder, 0x140 + 8, 1usize);
        wr(recorder, 0x140 + 0x10, 2usize);
        wr(sim, 0x150, track);
        wr(sim, 0x1b8, a.alloc(0x500));
        let replay = a.alloc(0x200);
        wr(sim, 0x1b0, replay);
        let count = number as usize;
        let list = a.alloc(count * 8 + 8);
        let mut cars = Vec::new();
        for i in 0..count {
            let avatar = a.alloc(0x2000);
            wr(avatar, 0x8, game_block);
            wr(avatar, 0x130, sim);
            wr(avatar, 0x1158, i as i32);
            for at in [0x10b8, 0x178, 0x138, 0x198, 0x1138] {
                empty_wstring(avatar.add(at));
            }
            crate::ac::write_wstring(a, avatar.add(0x10b8), &crate::gui_seq_apps::driver_name(i));
            // the car's node: "connected" at +0xd8; its body's node (bodyTransform, matrix at +8)
            wr(avatar, 0x210, a.alloc(0x100));
            wr(avatar, 0x1b8, a.alloc(0x100));
            // a controls provider (its table: getFFGlobalGain at +0x28), a race engineer (+0xf80)
            // whose car (+8) has the user's gain at +0xec
            let table = a.alloc(0x100);
            for slot in 0..32 {
                wr(table, slot * 8, ff_global_gain as *const () as usize);
            }
            let provider = a.alloc(0x40);
            wr(provider, 0, table);
            PROVIDERS.lock().unwrap().push(provider as usize);
            let engineer = a.alloc(0x40);
            wr(engineer, 8, a.alloc(0x200));
            wr(avatar, 0xf80, engineer);
            wr(list, i * 8, avatar);
            cars.push(avatar as usize);
        }
        wr(sim, 0x208, list);
        wr(sim, 0x210, list.add(count * 8));
        wr(sim, 0x218, list.add(count * 8));
        *GAME.lock().unwrap() = Some(AppGame { game_block: game_block as usize, sim: sim as usize, cars, replay: replay as usize, made: Vec::new() });
        update();
        return Ok(());
    }
    update();
    let sim = GAME.lock().unwrap().as_ref().map(|g| g.sim).ok_or("no session for the apps (app.create first)")? as *mut u8;
    match what {
        "app.new" => {
            let (_, size, ctor, _) = APPS.iter().find(|x| x.0 == text).ok_or(format!("no app {text:?} in the oracle's table"))?;
            let form = a.alloc(*size);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(*ctor));
            let first_car = GAME.lock().unwrap().as_ref().and_then(|g| g.cars.first().copied()).unwrap_or(0) as *mut u8;
            if text == "runningtime" {
                // a TimeLimitedTest among the Sim's game objects (its ctor reads the Documents
                // cfg/time_limited_test.ini); FormRunningTime finds it by its type
                let test = a.alloc(0x80);
                let tctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_401b_ecd0));
                tctor(test, sim);
                // (its read of the Documents' cfg/time_limited_test.ini finds nothing in the
                // oracle: the test the port reads, 3 minutes, is laid in its members)
                wr(test, 0x30, 1u8);
                wr(test, 0x58, 3i32);
                let list = a.alloc(0x10);
                wr(list, 0, test);
                wr(sim, 0x40, list);
                wr(sim, 0x48, list.add(8));
                wr(sim, 0x50, list.add(8));
            }
            if text == "volumes" {
                // Game::audioEngine (its calls stand in), every car's CarAudioFMOD
                // (CarAvatar+0xe18), Sim::cameraManager (+0x188) -> cameraDrivable (+0xc0) ->
                // drivableCameraPositions (+0x88) with the script's views
                let (game_block, cars) = GAME.lock().unwrap().as_ref().map(|g| (g.game_block, g.cars.clone())).unwrap_or_default();
                wr(game_block as *mut u8, 0x148, a.alloc(0x400));
                let mut audio = Vec::new();
                for car in &cars {
                    let block = a.alloc(0x200);
                    wr(*car as *mut u8, 0xe18, block);
                    audio.push(block as usize);
                }
                let camera = a.alloc(0x200);
                let drivable = a.alloc(0x200);
                let defs = a.alloc(cars.len().max(1) * 0x38);
                let views = crate::gui_seq_apps::script(|s| s.camera.clone());
                for (i, v) in views.iter().enumerate().take(cars.len()) {
                    wr(defs, i * 0x38 + 0x35, v[0] as u8);
                    wr(defs, i * 0x38 + 0x34, v[1] as u8);
                }
                wr(drivable, 0x88, defs);
                wr(drivable, 0x90, defs.add(cars.len() * 0x38));
                wr(drivable, 0x98, defs.add(cars.len() * 0x38));
                wr(camera, 0xc0, drivable);
                wr(sim, 0x188, camera);
                *SOUND.lock().unwrap() = Some(Sound { audio, defs: defs as usize, ..Sound::default() });
            }
            if text == "ffb" {
                // the player's Car (userFFGain +0xec, ffMult +0x1e4, steerAssist +0x3c70) and
                // the avatar's copy of the gain (+0x105c); a DICarControl as the controls
                let (s_mult, s_assist, upgrades) = crate::gui_seq_apps::script(|s| (s.ff_mult, s.steer_assist, s.ff_upgrades));
                let car = a.alloc(0x4000);
                wr(car, 0xec, 1.0f32);
                wr(car, 0x1e4, s_mult);
                wr(car, 0x3c70, s_assist);
                wr(first_car, 0x1168, car);
                wr(first_car, 0x105c, 1.0f32);
                let mut dic = 0usize;
                if let Some(u) = upgrades {
                    let block = a.alloc(0x1000);
                    wr(block, 0, a.va(VA_DICARCONTROL_VFTABLE) as usize);
                    wr(block, 0xa68, u);
                    if let Some(p) = PROVIDERS.lock().unwrap().get_mut(0) {
                        *p = block as usize;
                    }
                    dic = block as usize;
                }
                *FFB.lock().unwrap() = Some(Ffb { car: car as usize, dic, avatar: first_car as usize, ..Ffb::default() });
            }
            if (text == "mgu" || text == "ballast") && crate::gui_seq_apps::script(|s| !s.ers_names.is_empty()) && rd::<usize>(first_car, 0x1168) == 0 {
                // the player's Car: its ERS power controllers (Car+0x3790, 0x48-byte elements
                // with the name first), its ballast (+0x3e30)
                let names = crate::gui_seq_apps::script(|s| s.ers_names.clone());
                let car = a.alloc(0x4000);
                let list = a.alloc(names.len().max(1) * 0x48);
                for (i, name) in names.iter().enumerate() {
                    empty_wstring(list.add(i * 0x48));
                    crate::ac::write_wstring(a, list.add(i * 0x48), name);
                }
                wr(car, 0x3790, list);
                wr(car, 0x3798, list.add(names.len() * 0x48));
                wr(car, 0x37a0, list.add(names.len() * 0x48));
                wr(first_car, 0x1168, car);
                *BALLAST.lock().unwrap() = Some((car as usize, String::new()));
            }
            if text == "eyes" {
                // Sim: the render-finished node event (+0x170, its evOnRender at +0xe0), the
                // camera manager (+0x188: cameraOnBoard +0x88 with onBoardFov +0x90 and one
                // customCameraSettings {pitch, yaw} per car at +0xc8; sim +0x110; mode +0x120 =
                // 1, not on board), the system message (+0x198); the avatar's eye point (+0x218)
                let (e, fov, pitch) = crate::gui_seq_apps::script(|s| s.eyes).ok_or("app.eyes first")?;
                let cars = GAME.lock().unwrap().as_ref().map(|g| g.cars.len()).unwrap_or(1);
                wr(sim, 0x170, a.alloc(0x200));
                if rd::<usize>(sim, 0x188) == 0 {
                    wr(sim, 0x188, a.alloc(0x200));
                }
                let camera: *mut u8 = rd(sim, 0x188);
                let cob = a.alloc(0x200);
                wr(cob, 0x90, fov);
                let settings = a.alloc(cars.max(1) * 8);
                wr(settings, 0, pitch);
                wr(cob, 0xc8, settings);
                wr(cob, 0xd0, settings.add(cars * 8));
                wr(cob, 0xd8, settings.add(cars * 8));
                wr(camera, 0x88, cob);
                wr(camera, 0x110, sim);
                wr(camera, 0x120, 1i32);
                wr(sim, 0x198, a.alloc(0x100));
                wr(first_car, 0x218, e);
                *EYES.lock().unwrap() = Some(EyesGame { avatar: first_car as usize, cob: cob as usize, ..EyesGame::default() });
            }
            if text == "trackmap" {
                // TrackMap::TrackMap(GUI*, Sim*, bool)
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(a.va(0x1_402e_e8e0));
                ctor(form, gui, sim, false);
            } else if text == "help" {
                ctor(form, gui);
            } else {
                ctor(form, if TAKES_CAR.contains(&text) { first_car } else { sim });
            }
            if text == "volumes" {
                // INIReaderDocuments::basePath (the oracle skips its making, which leaves it
                // empty): the stand-in Documents' folder, where the app's Win32 writes go too
                let base = format!("{}/Assetto Corsa/", crate::root::documents().to_string_lossy());
                crate::ac::write_wstring(a, a.va(VA_INIREADERDOCUMENTS_BASE_PATH) as *mut u8, &base);
            }
            // Sim::addForm: GameScreen::addControl(form, true)
            let screen: *mut u8 = rd(sim, 0x108);
            let table: *const usize = rd(screen, 0);
            let add: extern "C" fn(*mut u8, *mut u8, bool) = std::mem::transmute(*table.add(0xa8 / 8));
            add(screen, form, number == 0.0);
            if let Some(g) = GAME.lock().unwrap().as_mut() {
                g.made.push((text.to_string(), form as usize));
            }
        }
        "app.visible" => {
            let set: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(0x1_4024_51a0));
            set(app(text)?, number != 0.0);
        }
        other => return Err(format!("the game's side knows no {other:?}")),
    }
    let _ = wstring;
    Ok(())
}

/// The session's state where the game's apps read it themselves: every car's
/// `physicsState`, `Sim::focusedCarIndex`, `Sim::connectedCarsCount`, the replay manager's
/// "in replay", the game's clock.
pub unsafe fn update() {
    let guard = GAME.lock().unwrap();
    let Some(g) = guard.as_ref() else { return };
    let l = link();
    crate::gui_seq_apps::script(|s| {
        for (i, avatar) in g.cars.iter().enumerate() {
            if let Some(bytes) = crate::gui_seq_apps::raw_state(s, i) {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), (*avatar as *mut u8).add(0x268), bytes.len());
            }
            // physicsInfo (+0xe40): maxFuel, engineDamageRPM
            wr(*avatar as *mut u8, 0xec8, s.max_fuel);
            wr(*avatar as *mut u8, 0xf70, s.damage_rpm);
            if let Some(c) = l.cars.get(i) {
                // physicsInfo: kersMaxJ, ersMaxJ, hasKERS, hasERS; the car's folder
                wr(*avatar as *mut u8, 0xf54, c.kers_max_j);
                wr(*avatar as *mut u8, 0xf58, c.ers_max_j);
                wr(*avatar as *mut u8, 0xf60, c.has_kers as u8);
                wr(*avatar as *mut u8, 0xf61, c.has_ers as u8);
                if crate::gui_ac::read_wstring((*avatar as *mut u8).add(0x138)) != c.unix_name {
                    crate::ac::write_wstring(crate::gui_ac_hud::acs(), (*avatar as *mut u8).add(0x138), &c.unix_name);
                }
                wr(rd::<*mut u8>(*avatar as *mut u8, 0x210), 0xd8, c.connected as u8);
                wr(rd::<*mut u8>(*avatar as *mut u8, 0x1b8), 8, c.body_matrix.m);
                wr(*avatar as *mut u8, 0x129c, c.in_pitlane as u8);
                wr(*avatar as *mut u8, 0xf66, c.has_drs as u8);
                match c.user_ff_gain {
                    Some(gain) => {
                        let engineer: *mut u8 = rd(*avatar as *mut u8, 0xf80);
                        let engineer = if engineer.is_null() { std::ptr::null_mut() } else { engineer };
                        if !engineer.is_null() {
                            wr(rd::<*mut u8>(engineer, 8), 0xec, gain);
                        }
                    }
                    None => wr(*avatar as *mut u8, 0xf80, 0usize),
                }
            }
        }
    });
    let sim = g.sim as *mut u8;
    wr(sim, 0x220, l.focused_car as u32);
    let track: *mut u8 = rd(sim, 0x150);
    wr(rd::<*mut u8>(track, 0x178), 0x140 + 0x20, l.ai_spline_length);
    let (name, config) = crate::gui_seq_apps::script(|s| s.track.clone());
    // the track's sections (TrackAvatar+0x2f0: 0x28-byte {in, out, text})
    let sections = crate::gui_seq_apps::script(|s| s.sections.clone());
    if !sections.is_empty() && rd::<usize>(track, 0x2f8).wrapping_sub(rd::<usize>(track, 0x2f0)) != sections.len() * 0x28 {
        let block = crate::gui_ac_hud::acs().alloc(sections.len() * 0x28);
        for (i, (i_n, o_ut, text)) in sections.iter().enumerate() {
            let e = block.add(i * 0x28);
            wr(e, 0, *i_n);
            wr(e, 4, *o_ut);
            empty_wstring(e.add(8));
            crate::ac::write_wstring(crate::gui_ac_hud::acs(), e.add(8), text);
        }
        wr(track, 0x2f0, block);
        wr(track, 0x2f8, block.add(sections.len() * 0x28));
        wr(track, 0x300, block.add(sections.len() * 0x28));
    }
    // the sun's angle (GraphicsManager::lightingSettings.angle), the game's start
    wr(g.game_block as *mut u8, 0x38, l.game_start_time);
    // ACTimeFlow's `active`, as its handler of the replay's status leaves it
    for (n, form) in &g.made {
        if n == "timeflow" {
            wr(*form as *mut u8, 0x270, !l.in_replay as u8);
        }
    }
    // the AI line's points (20-byte SplinePoints: x, y, z, length, tag) in the spline's vector
    let points = crate::gui_seq_apps::script(|s| s.ai_points.clone());
    let spline = rd::<*mut u8>(track, 0x178).add(0x140);
    if !points.is_empty() && rd::<usize>(spline, 0x10).wrapping_sub(rd::<usize>(spline, 8)) != points.len() * 20 {
        let block = crate::gui_ac_hud::acs().alloc(points.len() * 20);
        for (i, p) in points.iter().enumerate() {
            wr(block, i * 20, *p);
        }
        wr(spline, 8, block);
        wr(spline, 0x10, block.add(points.len() * 20));
        wr(spline, 0x18, block.add(points.len() * 20));
    }
    if crate::gui_ac::read_wstring(track.add(0x10)) != name {
        crate::ac::write_wstring(crate::gui_ac_hud::acs(), track.add(0x10), &name);
    }
    if crate::gui_ac::read_wstring(track.add(0xa0)) != config {
        crate::ac::write_wstring(crate::gui_ac_hud::acs(), track.add(0xa0), &config);
    }
    let physics: *mut u8 = rd(sim, 0x1b8);
    wr(physics, 0x310, l.wind_speed);
    wr(physics, 0x314, l.wind_direction_deg);
    wr(physics, 0x408, [l.ambient_temperature, l.road_temperature]);
    wr(physics, 0x470 + 4, l.grip);
    wr(sim, 0x1fc, l.connected_cars_count);
    wr(g.replay as *mut u8, 0x148, l.in_replay as u8);
    wr(g.game_block as *mut u8, 0x18, l.game_time);
    if let Some(game) = GAME_ACS.lock().unwrap().as_ref() {
        wr(game.0 as *mut u8, 0, l.use_mph as u8);
    }
    // the force feedback app's: the gains, the car's FF mult, the wheel's effects, the messages
    if let Some(f) = FFB.lock().unwrap().as_mut() {
        if f.dic != 0 {
            wr(f.dic as *mut u8, 0xa50, l.cars.first().and_then(|c| c.ff_global_gain).unwrap_or(0.0));
        }
        let upgrades = (f.dic != 0).then(|| rd::<[f32; 4]>(f.dic as *mut u8, 0xa68));
        let line = crate::gui_port_apps::ffb_line(rd(f.avatar as *mut u8, 0x105c), rd(f.car as *mut u8, 0xec), rd(f.car as *mut u8, 0x1e4), upgrades);
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), std::mem::take(&mut f.log), &line, &mut f.last);
    }
    wr(g.game_block as *mut u8, 0x20, l.game_dt);
    // the player's ERS cockpit (CarAvatar: power index +0x1284, recovery +0x1288, engine brake
    // +0x127c, heat charging +0x1280); the ballast app's effect (Car+0x3e30)
    if let (Some(avatar), Some(c)) = (g.cars.first(), l.cars.first()) {
        let s = crate::gui_seq_apps::script(|s| s.ers_state);
        wr(*avatar as *mut u8, 0x1284, s[0]);
        wr(*avatar as *mut u8, 0x1288, c.ers_recovery);
        wr(*avatar as *mut u8, 0x127c, c.engine_brake_setting);
        wr(*avatar as *mut u8, 0x1280, c.ers_heat_charging as u8);
    }
    if let Some(e) = EYES.lock().unwrap().as_mut() {
        let eyes: [f32; 3] = rd(e.avatar as *mut u8, 0x218);
        let line = crate::gui_port_apps::eyes_line(eyes, rd(e.cob as *mut u8, 0x90), rd(rd::<*mut u8>(e.cob as *mut u8, 0xc8), 0));
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), std::mem::take(&mut e.log), &line, &mut e.last);
    }
    if let Some((car, last)) = BALLAST.lock().unwrap().as_mut() {
        let line = format!("ballast {:08x}", rd::<f32>(*car as *mut u8, 0x3e30).to_bits());
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), Vec::new(), &line, last);
    }
    // the Audio app's effects, as the port's side writes them
    if let Some(s) = SOUND.lock().unwrap().as_mut() {
        let mix: Vec<[f32; 7]> = s.audio.iter().map(|b| rd::<[f32; 7]>(*b as *mut u8, 0x118)).collect();
        let camera: Vec<[bool; 2]> = (0..s.audio.len()).map(|i| [rd::<u8>(s.defs as *mut u8, i * 0x38 + 0x35) != 0, rd::<u8>(s.defs as *mut u8, i * 0x38 + 0x34) != 0]).collect();
        let line = crate::gui_port_apps::sound_line(&mix, &camera);
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), std::mem::take(&mut s.log), &line, &mut s.last);
    }
}
