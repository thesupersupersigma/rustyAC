// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the setup editor's proofs: acs.exe's own `SetupScreen` with its tabs, on a
//! real `Car` of the game (its own `PhysicsEngine`, a `Track` without files and `Car::Car`, as
//! `tools/car_oracle` builds them) hung on the stand-in `CarAvatar` of the HUD scripts.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ac::{rd, wr, wstring, Game};

const VA_PHYSICS_ENGINE_CTOR: usize = 0x1_4026_2430;
const VA_TRACK_CTOR: usize = 0x1_4027_7100;
const VA_TRACK_INIT_AI_SPLINE: usize = 0x1_4027_82a0;
const VA_CAR_CTOR: usize = 0x1_4026_bf00;
const PE_SIZE: usize = 0x278;
const TRACK_SIZE: usize = 0x148;
const CAR_SIZE: usize = 0x3ea0;
pub const CAR_SETUP_MANAGER: usize = 0x35d0;
const SETUP_ITEM_SIZE: usize = 0x88;

/// class, size, constructor, table (see `gui_ac::CLASSES`)
pub const CLASSES: [(&str, usize, usize, usize); 10] = [
    ("SetupScreen", 0x1f8, 0x1_4017_c010, 0x1_404d_4ee8),
    ("SetupTab", 0x1c0, 0x1_4018_3130, 0x1_404d_5a20),
    ("SetupIOManager", 0x270, 0x1_4017_4c90, 0x1_404d_3d50),
    ("SetupGearManager", 0x208, 0x1_4017_0c80, 0x1_404d_37f0),
    ("SetupTyresManager", 0x1d8, 0x1_4018_4650, 0x1_404d_5c70),
    ("SetupFuelManager", 0x228, 0x1_4016_d880, 0x1_404d_2f70),
    ("SetupElectronics", 0x1e8, 0x1_4016_a020, 0x1_404d_25f8),
    ("SetupInformation", 0x1d8, 0x1_4017_fcf0, 0x1_404d_5508),
    ("SetupPitstopPresets", 0x228, 0x1_4017_87d0, 0x1_404d_4810),
    ("EscMenuTimeTable", 0x1c0, 0x1_4009_4230, 0x1_404b_5368),
];

static CAR: AtomicUsize = AtomicUsize::new(0);
static ENGINE: AtomicUsize = AtomicUsize::new(0);

/// The game's `Car` of the script (null: the script has none).
pub fn car() -> *mut u8 {
    CAR.load(Ordering::SeqCst) as *mut u8
}


/// The game's own `PhysicsEngine`, `Track` ("flat": no files) and `Car` of that folder name.
pub unsafe fn make_car(game: &Game, name: &str) -> *mut u8 {
    let acs = &game.acs;
    // LocalizationManager's statics, then the locale as `initLocalization` sets it
    for va in [0x1_4001_1060usize, 0x1_4001_1070, 0x1_4001_1090, 0x1_4001_10c0, 0x1_4001_10f0] {
        let init: extern "C" fn() = std::mem::transmute(acs.va(va));
        init();
    }
    let set_locale: extern "C" fn(*mut u8) -> bool = std::mem::transmute(acs.va(0x1_4024_0980));
    assert!(set_locale(wstring(acs, "en")), "acLocaleSetLocale(en)");
    let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
    srand(1);
    let engine = acs.alloc(PE_SIZE);
    let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PHYSICS_ENGINE_CTOR));
    ctor(engine);
    wr(engine, 0x100, 26.0f32);
    wr(engine, 0x104, 30.0f32);
    let track = acs.alloc(TRACK_SIZE);
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_CTOR));
    ctor(track, engine, wstring(acs, "flat"), wstring(acs, ""));
    let init: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_TRACK_INIT_AI_SPLINE));
    init(track);
    let car = acs.alloc(CAR_SIZE);
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_CTOR));
    ctor(car, engine, wstring(acs, name), wstring(acs, ""));
    CAR.store(car as usize, Ordering::SeqCst);
    ENGINE.store(engine as usize, Ordering::SeqCst);
    car
}

/// Every setup item of the game's car: name, `newValue`, attached, the car's value.
pub unsafe fn items(out: &mut String) {
    let car = car();
    if car.is_null() {
        return;
    }
    let manager = car.add(CAR_SETUP_MANAGER);
    let mut item: *mut u8 = rd(manager, 0);
    let end: *mut u8 = rd(manager, 8);
    while item < end {
        let name = crate::gui_ac::read_wstring(item.add(0x8));
        let connected: *const f32 = rd(item, 0x28);
        let value = if connected.is_null() { 0 } else { (*connected).to_bits() };
        out.push_str(&crate::gui_seq_setup::item_line(&name, rd::<f32>(item, 0x54), rd::<u8>(item, 0x58) != 0, f32::from_bits(value)));
        item = item.add(SETUP_ITEM_SIZE);
    }
}

/// In place of `PhysicsAvatar::executeOnPhysicsThread` 0x140121f20: the job is done at once
/// (the physics thread would do it before its next step). The `std::function` comes by
/// address; its `_Do_call` is the third entry of its implementation's table.
unsafe extern "C" fn execute_on_physics_thread(_this: *mut u8, function: *mut u8) {
    let implementation: *mut u8 = rd(function, 0x18);
    if !implementation.is_null() {
        let table: *const usize = rd(implementation, 0);
        let call: extern "C" fn(*mut u8) = std::mem::transmute(*table.add(2));
        call(implementation);
    }
}

fn time_link() -> rustyac_gui::time_table::TimeLink {
    let race = crate::gui_ac_race::race();
    crate::gui_seq_setup::time_link(race.cars.len(), race.current_session_index, 0)
}

/// A `std::vector<Lap>` of the game from the script's laps.
unsafe fn write_laps(out: *mut u8, laps: &[rustyac_gui::time_table::TimeLap]) {
    let acs = crate::gui_ac_hud::acs();
    let block = acs.alloc(laps.len().max(1) * 0x48);
    for (i, lap) in laps.iter().enumerate() {
        let at = block.add(i * 0x48);
        crate::gui_ac_race::write_lap(at, &rustyac_gui::leaderboard::RaceLap { time: lap.time, splits: lap.splits.clone(), is_valid: lap.is_valid });
        wr(at, 0x20, lap.cuts);
        crate::ac::write_wstring(acs, at.add(0x28), &lap.compound);
    }
    wr(out, 0, block);
    wr(out, 8, block.add(laps.len() * 0x48));
    wr(out, 0x10, block.add(laps.len() * 0x48));
}

/// In place of `RaceManager::getLaps` 0x1401386b0: the running session's laps of a car.
unsafe extern "C" fn get_laps(_this: *mut u8, out: *mut u8, car: *mut u8, _replay: bool) -> *mut u8 {
    let index = rd::<i32>(car, 0x1158) as usize;
    write_laps(out, time_link().laps.get(index).map_or(&[][..], |laps| laps.as_slice()));
    out
}

/// In place of `RaceManager::getSessionHistory` 0x140139cf0: an `OnSessionEndEvent` (0xe8
/// bytes) with every car's laps and best lap (an empty one for a session there is none of).
unsafe extern "C" fn get_session_history(_this: *mut u8, out: *mut u8, id: *const i32) -> *mut u8 {
    let acs = crate::gui_ac_hud::acs();
    std::ptr::write_bytes(out, 0, 0xe8);
    for at in [0x18usize, 0x38] {
        wr(out, at + 0x18, 7usize);
    }
    let link = time_link();
    if let Some(history) = usize::try_from(*id).ok().and_then(|i| link.history.get(i)) {
        let cars = history.laps.len();
        let lists = acs.alloc(cars.max(1) * 0x18);
        for (i, laps) in history.laps.iter().enumerate() {
            write_laps(lists.add(i * 0x18), laps);
        }
        wr(out, 0x70, lists);
        wr(out, 0x78, lists.add(cars * 0x18));
        wr(out, 0x80, lists.add(cars * 0x18));
        write_laps(out.add(0x88), &history.best_laps);
    }
    out
}

/// A menu call of the script's that is the setup editor's.
pub unsafe fn call(game: &Game, _gui: *mut u8, h: &crate::gui_ac_hud::HudGame, what: &str, number: f32, text: &str) -> Result<(), String> {
    match what {
        "menu.setupCar" => {
            crate::gui_ac_hud::jump(game, 0x1_4012_1f20, execute_on_physics_thread as *const () as usize);
            let car = make_car(game, text);
            wr(h.cars[0], 0x1168, car);
            // CarAvatar::raceEngineer
            let engineer = game.acs.alloc(0x18);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(game.acs.va(0x1_4027_9bf0));
            ctor(engineer, car);
            wr(h.cars[0], 0xf80, engineer);
            // CarAvatar::physicsInfo, as CarAvatar::initPhysics fills it (here from the port's
            // car of the same data folder); Sim::useMousePitstop
            let port = crate::gui_port_setup::make_car(std::path::Path::new("."), text, number as i32)?;
            use rustyac_gui::setup_car::SetupCar;
            let info = h.cars[0].add(0xe40);
            wr(info, 0x4, number as i32);
            wr(info, 0x88, port.max_fuel());
            let names = port.tyre_compound_names();
            let list = game.acs.alloc(names.len() * 0x20 + 0x20);
            for (i, name) in names.iter().enumerate() {
                crate::ac::write_wstring(&game.acs, list.add(i * 0x20), name);
            }
            wr(info, 0xd8, list);
            wr(info, 0xe0, list.add(names.len() * 0x20));
            wr(info, 0xe8, list.add(names.len() * 0x20));
            wr(info, 0x121, port.has_ers() as u8);
            wr(info, 0x122, port.has_cockpit_ers_recovery() as u8);
            wr(info, 0x123, port.has_cockpit_ers_delivery_profile() as u8);
            wr(info, 0x125, port.has_cockpit_ers_mgu_h_mode() as u8);
            wr(info, 0x128, port.engine_brake_settings_count());
            wr(info, 0x12c, port.ers_power_controller_names().len() as i32);
            wr(info, 0x134, port.has_adjustable_turbo() as u8);
            // the car's own cockpit settings (CarAvatar::CarAvatar and initPhysics), and the
            // boost level of its state
            wr(h.cars[0], 0x127c, port.engine_brake_setting());
            wr(h.cars[0], 0x1280, port.ers_heat_charging() as u8);
            wr(h.cars[0], 0x1284, port.ers_power_index());
            wr(h.cars[0], 0x1288, port.ers_recovery());
            wr(h.cars[0], 0xa18, port.turbo_boost_level());
            wr(h.sim, 0x1e4, 1u8);
        }
        "menu.timeLaps" => {
            crate::gui_seq_setup::set_time_seed(number);
            static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !DONE.swap(true, Ordering::SeqCst) {
                crate::gui_ac_hud::jump(game, 0x1_4013_86b0, get_laps as *const () as usize);
                crate::gui_ac_hud::jump(game, 0x1_4013_9cf0, get_session_history as *const () as usize);
                // TrackAvatar::physicsTrack.sectorsNormalizedPositions: three sectors
                let track: *mut u8 = rd(h.sim, 0x150);
                let sectors = game.acs.alloc(12);
                wr(track, 0x228, sectors);
                wr(track, 0x230, sectors.add(12));
                wr(track, 0x238, sectors.add(12));
            }
        }
        "menu.setupState" => {
            crate::gui_seq_setup::set_car_state(number);
            let state = crate::gui_seq_setup::car_state();
            // CarAvatar::physicsState and physicsInfo.minHeightM
            let ps = h.cars[0].add(0x268);
            for w in 0..4 {
                wr(ps, 0x6dc + w * 4, state.camber_rad[w]);
                crate::gui_ac_hud3::write_matrix(ps.add(0x44 + w * 0x40), &state.suspension_matrix[w]);
                let t = ps.add(0x81c + w * 0xc0);
                wr(t, 0x98, state.tyres[w].dynamic_pressure);
                wr(t, 0x9c, state.tyres[w].static_pressure);
                wr(t, 0xa0, state.tyres[w].last_set_imo);
                wr(t, 0xb0, state.tyres[w].last_grain);
                wr(t, 0xb4, state.tyres[w].last_blister);
            }
            wr(ps, 0xb64, state.caster);
            wr(ps, 0x79c, state.ride_height);
            wr(h.cars[0], 0xf30, state.min_height_m);
        }
        other => return Err(format!("the setup editor has no call {other:?}")),
    }
    Ok(())
}
