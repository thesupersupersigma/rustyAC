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
pub const APPS: [(&str, usize, usize, usize); 44] = [
    ("carengineer", 0x1f0, 0x1_4031_a0f0, 0x1_4050_bf80),
    ("telemetry", 0x2b8, 0x1_4032_f660, 0x1_4050_fbe8),
    ("tyredebug", 0x1c0, 0x1_4031_9960, 0x1_4050_bd90),
    ("setupdebug", 0x1c8, 0x1_4031_7a80, 0x1_4050_b8c8),
    ("carphysics", 0x1f8, 0x1_402d_7680, 0x1_4050_1a10),
    ("sustelemetry", 0x1e0, 0x1_4030_2180, 0x1_4050_7a38),
    ("driverperf", 0x2a8, 0x1_4030_3bd0, 0x1_4050_81b8),
    ("fuelmonitor", 0x1c0, 0x1_4033_8250, 0x1_4051_1b70),
    ("rstats", 0x1f0, 0x1_402d_69d0, 0x1_4050_17b8),
    ("gamestats", 0x1e8, 0x1_402e_7a10, 0x1_4050_3ba0),
    ("friends", 0x270, 0x1_4032_4fc0, 0x1_4050_d7b8),
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
    ("chat", 0x3e0, 0x1_4030_8190, 0x1_4050_8ca0),
    // Task 43c
    ("wings", 0x1c0, 0x1_402e_c890, 0x1_4050_4aa0),
    ("tyretester", 0x1f8, 0x1_4033_b520, 0x1_4051_20e8),
    ("suspension", 0x288, 0x1_402e_3fd0, 0x1_4050_37f8),
    ("gpuprofiler", 0x1c0, 0x1_4031_b320, 0x1_4050_c230),
    // Task 44
    ("drszones", 0x210, 0x1_4033_6210, 0x1_4051_1390),
    ("chasecameditor", 0x1f0, 0x1_4033_72a0, 0x1_4051_16e0),
    ("netmonitor", 0x1e0, 0x1_4030_3400, 0x1_4050_7e60),
    ("keyboard", 0x1f8, 0x1_4030_dbe0, 0x1_4050_9a20),
    ("x360", 0x1d8, 0x1_4032_76d0, 0x1_4050_e090),
    ("cameraeditor", 0x2d0, 0x1_402f_1bc0, 0x1_4050_5638),
    ("photomode", 0x250, 0x1_4032_1bc0, 0x1_4050_cff0),
    ("ppfilter", 0x1c8, 0x1_4032_9360, 0x1_4050_e610),
];
/// the apps whose constructor takes the player's `CarAvatar` (the others take `Sim`)
/// The Net monitor's stand-in client and car 1's net provider.
static NET_MONITOR: Mutex<Option<(usize, usize)>> = Mutex::new(None);
/// Keyboard debug's `KeyboardCarControl` stand-in
static KEYBOARD: Mutex<Option<usize>> = Mutex::new(None);
/// X360 Controller debug's `JoypadCarControl` stand-in
static PAD: Mutex<Option<usize>> = Mutex::new(None);
/// what `getControlsProvider` answers while Keyboard debug or X360 Controller debug is made
static CONTROLS_CTOR: Mutex<Option<usize>> = Mutex::new(None);

/// The Camera editor's staging: car 0, the Sim, YEBIS's camera, the camera manager's calls,
/// the last line of `effects_game.txt`.
struct CameraGame {
    car: usize,
    sim: usize,
    yebis: usize,
    log: Vec<String>,
    last: String,
}

static CAMERA_EDITOR: Mutex<Option<CameraGame>> = Mutex::new(None);

fn camera_log(line: String) {
    if let Some(c) = CAMERA_EDITOR.lock().unwrap().as_mut() {
        c.log.push(line);
    }
}

/// `ACCameraManager::setMode(mode, bool, bool)` 0x1400340d0: the call, written down.
unsafe extern "C" fn camera_set_mode(_this: *mut u8, mode: i32, _a: u8, _b: u8) {
    camera_log(format!("setMode {mode}"));
}

/// `ACCameraManager::setDrivableCarIndex` 0x140033fd0: written down.
unsafe extern "C" fn camera_set_drivable(_this: *mut u8, index: i32) {
    camera_log(format!("setDrivableCarIndex {index}"));
}

/// `ACCameraManager::setCameraCarIndex` 0x140033fb0: written down.
unsafe extern "C" fn camera_set_car(_this: *mut u8, index: i32) {
    camera_log(format!("setCameraCarIndex {index}"));
}

/// The Camera editor's staged game, read back as the port's link.
unsafe fn camera_editor_link(c: &CameraGame) -> rustyac_gui::app_camera_editor::CameraEditorLink {
    use rustyac_physics::vecmath::Mat44f;
    let (car, sim) = (c.car as *mut u8, c.sim as *mut u8);
    let begin: *mut u8 = rd(car, 0xfc8);
    let end: *mut u8 = rd(car, 0xfd0);
    let cameras = (0..(end as usize - begin as usize) / 0x4c)
        .map(|i| {
            let at = begin.add(i * 0x4c);
            rustyac_gui::app_camera_editor::EditorCamera { matrix: Mat44f { m: rd(at, 0) }, fov: rd(at, 0x40), exposure: rd(at, 0x44), external_sound: rd::<u8>(at, 0x48) != 0 }
        })
        .collect();
    let manager: *mut u8 = rd(sim, 0x188);
    let def: *mut u8 = rd(rd::<*mut u8>(manager, 0xc0), 0x88);
    let node: *mut u8 = rd(car, 0x1b8);
    rustyac_gui::app_camera_editor::CameraEditorLink {
        unix_name: crate::gui_ac::read_wstring(car.add(0x138)),
        on_board_exposure: rd(car, 0xfb8),
        out_board_exposure: rd(car, 0xfbc),
        dash_board_exposure: rd(car, 0xfc0),
        cameras,
        eyes: rd(car, 0x218),
        onboard_pitch: rd(rd::<*mut u8>(rd::<*mut u8>(manager, 0x88), 0xc8), 0),
        bumper_pos: rd(def, 0),
        bumper_pitch: rd(def, 0xc),
        bonnet_pos: rd(def, 0x10),
        bonnet_pitch: rd(def, 0x1c),
        scene_camera: Mat44f { m: rd(rd::<*mut u8>(sim, 0x238), 0xc) },
        body: Mat44f { m: rd(node, 8) },
        auto_exposure: rd(c.yebis as *mut u8, 0x7b4),
        use_auto_exposure_limits: rd::<u8>(c.yebis as *mut u8, 0x7a1) != 0,
    }
}

/// `PostProcessEffectsUpdater::loadConfigurationFile` 0x1400abb70 for the filter picker: the
/// name written down (the wstring by value is left to leak).
unsafe extern "C" fn ppf_load(_this: *mut u8, name: *mut u8) {
    let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), vec![format!("load {:?}", crate::gui_ac::read_wstring(name))], "", &mut String::new());
}

/// The Photo mode's staged blocks: the scene camera, the graphics manager, the sun animator
/// and its graphics block, the camera manager; the calls written down, the last line.
struct PhotoGame {
    cam: usize,
    gm: usize,
    sa: usize,
    sun_gm: usize,
    manager: usize,
    log: Vec<String>,
    last: String,
}

static PHOTO: Mutex<Option<PhotoGame>> = Mutex::new(None);
/// the real `GetAsyncKeyState`
static REAL_KEYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn photo_log(line: String) {
    if let Some(p) = PHOTO.lock().unwrap().as_mut() {
        p.log.push(line);
    }
}

/// `GetAsyncKeyState` while the Photo mode is staged: the right and middle buttons from the
/// script, END never down, the others the real one's.
unsafe extern "system" fn photo_keys(vk: i32) -> i16 {
    let [right, middle] = crate::gui_seq_apps::script(|s| s.photo_keys);
    let down = |b: bool| if b { i16::MIN } else { 0 };
    match vk {
        2 => down(right),
        4 => down(middle),
        0x23 => 0,
        _ => {
            let real: extern "system" fn(i32) -> i16 = std::mem::transmute(REAL_KEYS.load(std::sync::atomic::Ordering::SeqCst));
            real(vk)
        }
    }
}

/// `ACCameraManager::setMode` for the Photo mode: the mode kept (+0x120), the call written down.
unsafe extern "C" fn photo_set_mode(this: *mut u8, mode: i32, _a: u8, _b: u8) {
    wr(this, 0x120, mode);
    photo_log(format!("setMode {mode}"));
}

/// `PhysicsAvatar::pausePhysics` 0x140122300: written down.
unsafe extern "C" fn photo_pause(_this: *mut u8, paused: u8) {
    photo_log(format!("pausePhysics {paused}"));
}

/// `ScreenCapturer::takeScreen` 0x140163470: written down.
unsafe extern "C" fn photo_take_screen(_this: *mut u8, _path: *mut u8) {
    photo_log("takeScreen".to_string());
}

/// The Photo mode's staged game, read back as the port's link.
unsafe fn photo_link(p: &PhotoGame) -> (rustyac_gui::app_photo_mode::PhotoLink, bool) {
    use rustyac_physics::vecmath::{Mat44f, Vec3f};
    let (cam, sun_gm) = (p.cam as *mut u8, p.sun_gm as *mut u8);
    let sun: [f32; 3] = rd(sun_gm, 0x388);
    let link = rustyac_gui::app_photo_mode::PhotoLink {
        matrix: Mat44f { m: rd(cam, 0xc) },
        fov: rd(cam, 8),
        dof_factor: rd(cam, 0x1b0),
        dof_focus: rd(cam, 0x1b4),
        exposure_multiplier: rd(p.gm as *mut u8, 0xc),
        custom_sun: (rd::<u8>(sun_gm, 8) != 0).then_some(Vec3f::new(sun[0], sun[1], sun[2])),
        camera_mode: rd(p.manager as *mut u8, 0x120),
        ..Default::default()
    };
    (link, rd::<u8>(p.sa as *mut u8, 0x30) != 0)
}

/// The ChaseCam Editor's camera manager block (0: none staged).
static CHASE_CAMERA: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const TAKES_CAR: [&str; 17] = ["keyboard", "drszones", "suspension", "wings", "carphysics", "telemetry", "sustelemetry", "driverperf", "delta", "tyres", "pedals", "electronics", "gears", "ffb", "mgu", "ballast", "eyes"];
/// `const DICarControl::vftable'`
const VA_DICARCONTROL_VFTABLE: usize = 0x1_404b_2098;
const VA_SPEED_USE_MPH: usize = 0x1_4155_a5ae;
/// `static std::wstring INIReaderDocuments::basePath`
const VA_INIREADERDOCUMENTS_BASE_PATH: usize = 0x1_4151_d100;

/// The apps' tables for `gui_ac::class_of`: every app's form is a "Form" in the dumps.
pub fn classes() -> Vec<(&'static str, usize, usize, usize)> {
    let mut out: Vec<_> = APPS.iter().filter(|a| a.3 != 0).map(|a| ("Form", a.1, a.2, a.3)).collect();
    // Task 33: ComparativeTelemetry, the ESC menu's page "Telemetry"
    out.push(("Form", 0x258, 0x1_4006_dff0, 0x1_404a_f980));
    // Task 42: PythonForm, a Python app's window
    out.push(("Form", 0x1d8, 0x1_4012_4850, 0x1_404c_8da8));
    out
}

struct AppGame {
    game_block: usize,
    sim: usize,
    cars: Vec<usize>,
    replay: usize,
    made: Vec<(String, usize)>,
}

static GAME: Mutex<Option<AppGame>> = Mutex::new(None);

/// Task 42: the stand-in `Game` block and `Sim` (none before `app.create`).
pub fn blocks() -> Option<(usize, usize)> {
    GAME.lock().unwrap().as_ref().map(|g| (g.game_block, g.sim))
}

/// Task 42: the stand-in cars.
pub fn avatars() -> Vec<usize> {
    GAME.lock().unwrap().as_ref().map(|g| g.cars.clone()).unwrap_or_default()
}

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
pub unsafe extern "C" fn execute_on_physics_thread(_this: *mut u8, f: *mut u8) {
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
    } else {
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), vec![line], "", &mut String::new());
    }
}

/// `AudioEngine::setVolume` 0x1401fbdf0: the call, written down.
unsafe extern "C" fn set_volume(_this: *mut u8, v: f32) {
    if let Some(s) = SOUND.lock().unwrap().as_mut() {
        s.log.push(format!("master {:08x}", v.to_bits()));
    }
}

/// A Steam request of the Friends leaderboard's buttons: nothing (no Steam).
unsafe extern "C" fn steam_nothing(_this: *mut u8) {}

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
    if let Some(key) = *CONTROLS_CTOR.lock().unwrap() {
        return key as *mut u8;
    }
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

/// Task 42: the cars' RaceManager getters again (a HUD field made after the apps takes them; its
/// session getters stay the HUD's).
pub unsafe fn install_car_getters(game: &Game) {
    for (va, to) in [
        (0x1_4013_80b0, get_current_lap as *const () as usize),
        (0x1_4013_7c00, get_best_lap as *const () as usize),
        (0x1_4013_87c0, get_last_lap as *const () as usize),
        (0x1_4013_7ee0, get_car_leaderboard_position as *const () as usize),
        (0x1_4013_7f90, get_car_real_time_position as *const () as usize),
        (0x1_4013_8570, get_lap_count as *const () as usize),
    ] {
        crate::gui_ac_hud::jump(game, va, to);
    }
}

unsafe fn install(game: &Game) {
    // the static `gears` of ACEssentials.obj ("R", "N", "1" …)
    let init: extern "C" fn() = std::mem::transmute(game.acs.va(0x1_4001_5240));
    init();
    // the language (acLocaleGetCurrentLocale's `currentLocale`): ACHelp reads help/<it>.tag
    crate::ac::write_wstring(&game.acs, game.acs.va(0x1_4151_d190) as *mut u8, &crate::gui_seq::language_or_en());
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
        (0x1_401f_bdf0, set_volume as *const () as usize),
        (0x1_4012_1f20, execute_on_physics_thread as *const () as usize),
        (0x1_401b_61c0, add_message as *const () as usize),
        // SteamInterface::resetPlayerLeaderboard / refreshPlayerLeaderboard: Steam requests
        // that never answer here
        (0x1_401a_ca60, steam_nothing as *const () as usize),
        (0x1_401a_c7c0, steam_nothing as *const () as usize),
        (0x1_401f_9830, get_driver_num as *const () as usize),
        (0x1_401f_96d0, get_driver_info as *const () as usize),
    ] {
        jump(game, va, to);
    }
    if crate::gui_seq::language().is_none() {
        jump(game, 0x1_4024_0cf0, crate::gui_ac_hud::ac_translate as *const () as usize);
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

/// Task 33: the game's `FormRenderStats` (0: none).
static RSTATS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Task 33: the game's `MicroSectors` of the sequence (0: none).
/// FormTelemetry: the raw values its channels read (in `app_telemetry::channels`' order) and
/// the physics avatar's `evOnStepCompleted`
static TELEMETRY: Mutex<Option<(usize, usize)>> = Mutex::new(None);
/// TyreWearingDebug's `Car` (PhysicsEngine::cars[0])
static TYRE_DEBUG: Mutex<Option<usize>> = Mutex::new(None);
/// SetupDebug's `Car` (every avatar's `raceEngineer->car`)
static SETUP_DEBUG: Mutex<Option<(usize, usize)>> = Mutex::new(None);
/// CarPhysicsForm's and ACDCarEngineer's `Car` (the player's avatar's `raceEngineer->car` and
/// physics), its body and four suspensions (stand-ins whose values sit at +0x180), its engineer,
/// which app
static CAR_PHYSICS: Mutex<Option<(usize, usize, [usize; 4], usize, &'static str)>> = Mutex::new(None);
static MICRO: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Once a frame before the GUI (`Game::update`): `MicroSectors::update` 0x14009fe20.
pub unsafe fn frame(game: &Game) {
    // Sim::update's statistics block on FormRenderStats' members (+0x1b8 cpuTime, +0x1bc
    // simulationOccupancy, +0x1c0 renderAudioTime, +0x1c4 physicsLate, +0x1c8 fmodUpdateUsage)
    let form = RSTATS.load(std::sync::atomic::Ordering::SeqCst) as *mut u8;
    if !form.is_null() {
        let mut s = rustyac_gui::app_stats::SimStats { cpu_time: rd(form, 0x1b8), simulation_occupancy: rd(form, 0x1bc), render_audio_time: rd(form, 0x1c0), physics_late: rd(form, 0x1c4), fmod_update_usage: rd(form, 0x1c8) };
        s.update(crate::gui_seq_apps::DT, &link().stats);
        wr(form, 0x1b8, s.cpu_time);
        wr(form, 0x1bc, s.simulation_occupancy);
        wr(form, 0x1c0, s.render_audio_time);
        wr(form, 0x1c4, s.physics_late);
        wr(form, 0x1c8, s.fmod_update_usage);
    }
    let ms = MICRO.load(std::sync::atomic::Ordering::SeqCst);
    if ms == 0 {
        return;
    }
    let table: *const usize = rd(ms as *const u8, 0);
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(*table.add(1));
    let _ = game;
    update(ms as *mut u8, crate::gui_seq_apps::DT);
}

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
        // MicroSectors (0x90): switched off, or (Task 33) the game's own as SHOW_SECTORS=1
        // makes it (enabled, 24 overall bests of FLT_MAX and no car)
        if crate::gui_seq_apps::script(|s| s.sectors) {
            let ms = a.alloc(0x90);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_4009_e9d0));
            ctor(ms, sim);
            wr(ms, 0x88, 1u8);
            let best = a.alloc(24 * 8);
            for k in 0..24 {
                wr(best, k * 8, f32::MAX);
                wr(best, k * 8 + 4, -1i32);
            }
            wr(ms, 0x70, best);
            wr(ms, 0x78, best.add(24 * 8));
            wr(ms, 0x80, best.add(24 * 8));
            wr(sim, 0x1f0, ms);
            MICRO.store(ms as usize, std::sync::atomic::Ordering::SeqCst);
        } else {
            wr(sim, 0x1f0, a.alloc(0x90));
        }
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
            if text == "friends" {
                // a SteamInterface among the Sim's game objects (the class's own table, for the
                // app's dynamic cast; its evOnLeaderboardUpdated at +0x58)
                let steam = a.alloc(0x400);
                wr(steam, 0, a.va(0x1_404d_a7b8) as usize);
                let list = a.alloc(0x10);
                wr(list, 0, steam);
                wr(sim, 0x40, list);
                wr(sim, 0x48, list.add(8));
                wr(sim, 0x50, list.add(8));
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
            if text == "telemetry" {
                telemetry_car(a, first_car);
            }
            if text == "chat" {
                // FormChat (Task 34): the clock it stamps the lines with, the script's; online
                // `Sim::client` the HUD's stand-in client (its handler of evOnChatMessage)
                chat_clock(game);
                if !crate::gui_seq_menu::script().online.net_cars.is_empty() {
                    crate::gui_ac_esc::apps_client(game, sim);
                }
                wr(sim, 0x1c8, crate::gui_ac_esc::client());
            }
            if text == "tyredebug" {
                tyre_debug_car(a, sim);
            }
            if text == "setupdebug" {
                setup_debug_car(a);
            }
            if text == "wings" {
                // FormWings: the render-finished node event its constructor adds a handler to
                // (Sim+0x170, evOnRender at +0xe0); the wings' blocks
                if rd::<usize>(sim, 0x170) == 0 {
                    wr(sim, 0x170, a.alloc(0x200));
                }
                wings_car(a, first_car);
            }
            if text == "ppfilter" {
                // PostProcessFilterSelector(Sim*): the game objects (Sim+0x40) one
                // PostProcessEffectsUpdater (its table for the dynamic cast, defaultFilter +0x58);
                // loadConfigurationFile written down
                let ppeu = a.alloc(0x88);
                wr(ppeu, 0, a.va(0x1_404b_8730) as usize);
                crate::ac::write_wstring(&game.acs, ppeu.add(0x58), "default");
                let list = a.alloc(0x10);
                wr(list, 0, ppeu);
                wr(sim, 0x40, list);
                wr(sim, 0x48, list.add(8));
                wr(sim, 0x50, list.add(8));
                crate::gui_ac_hud::jump(game, 0x1_400a_bb70, ppf_load as *const () as usize);
            }
            if text == "photomode" {
                // PhotoMode(Sim*): Sim::getSceneCamera (+0x238) a CameraForward (its vftable for
                // the dynamic cast: matrix +0xc, fov +8, dofFactor +0x1b0, dofFocus +0x1b4);
                // Sim::cameraManager (+0x188, mode +0x120); the game objects (+0x40) one
                // SunAnimator whose game (+8) has its own graphics block at +0x138 (the sun's
                // flag +8, direction +0x388); the GUI's graphics manager's exposure multiplier
                // (+0xc). setMode, pausePhysics and takeScreen are written down, not made;
                // GetAsyncKeyState answers the script's buttons.
                let l = crate::gui_seq_apps::photo_start();
                let scene = a.alloc(0x200);
                wr(scene, 0, a.va(0x1_404e_d0a8) as usize);
                wr(scene, 8, l.fov);
                wr(scene, 0xc, l.matrix.m);
                wr(scene, 0x1b0, l.dof_factor);
                wr(scene, 0x1b4, l.dof_focus);
                wr(sim, 0x238, scene);
                let manager = a.alloc(0x200);
                wr(manager, 0x120, l.camera_mode);
                wr(sim, 0x188, manager);
                let sun_game = a.alloc(0x200);
                let sun_gm = a.alloc(0x400);
                wr(sun_game, 0x138, sun_gm);
                let sa = a.alloc(0x80);
                wr(sa, 0, a.va(0x1_404d_b280) as usize);
                wr(sa, 8, sun_game);
                wr(sa, 0x30, 1u8);
                let list = a.alloc(0x10);
                wr(list, 0, sa);
                wr(sim, 0x40, list);
                wr(sim, 0x48, list.add(8));
                wr(sim, 0x50, list.add(8));
                let gm: *mut u8 = rd(gui, 0x10);
                wr(gm, 0xc, l.exposure_multiplier);
                for (va, to) in [
                    (0x1_4003_40d0, photo_set_mode as *const () as usize),
                    (0x1_4012_2300, photo_pause as *const () as usize),
                    (0x1_4016_3470, photo_take_screen as *const () as usize),
                ] {
                    crate::gui_ac_hud::jump(game, va, to);
                }
                let slot = a.va(0x1_404a_4fa8) as *mut u8;
                REAL_KEYS.store(rd::<usize>(slot, 0), std::sync::atomic::Ordering::SeqCst);
                game.acs.patch(slot as usize, &(photo_keys as *const () as usize).to_le_bytes());
                *PHOTO.lock().unwrap() = Some(PhotoGame { cam: scene as usize, gm: gm as usize, sa: sa as usize, sun_gm: sun_gm as usize, manager: manager as usize, log: Vec::new(), last: String::new() });
            }
            if text == "chasecameditor" {
                // ChaseCamEditor(Sim*): Sim::cameraManager (+0x188) -> mode (+0x120),
                // cameraDrivable (+0xc0) -> currentMode (+0x58), chaseCamData (+0x5c, the
                // shipped ini's), chaseIndex (+0x128); update() switches them with the script
                if rd::<usize>(sim, 0x188) == 0 {
                    let camera = a.alloc(0x200);
                    wr(camera, 0xc0, a.alloc(0x200));
                    wr(sim, 0x188, camera);
                }
                let drivable: *mut u8 = rd(rd::<*mut u8>(sim, 0x188), 0xc0);
                let (d, h, p) = crate::gui_seq_apps::CHASE_0;
                wr(drivable, 0x5c, [d, h, p, 3.9f32, 1.9, -0.017_453]);
                CHASE_CAMERA.store(rd::<usize>(sim, 0x188), std::sync::atomic::Ordering::Relaxed);
            }
            if text == "drszones" {
                // FormDRSZones' save: TrackAvatar::getDataFolder (TrackAvatar+0xf8 physicsTrack,
                // Track+0x108 dataFolder), relative to the oracle's root
                let (name, config) = crate::gui_seq_apps::script(|s| s.track.clone());
                let folder = if config.is_empty() { format!("content/tracks/{name}") } else { format!("content/tracks/{name}/{config}") };
                crate::ac::write_wstring(a, rd::<*mut u8>(sim, 0x150).add(0xf8 + 0x108), &folder);
            }
            if text == "suspension" {
                // ACSuspension: the same render-finished event; the car's body node and its
                // suspension avatar's wheel nodes (Node::getWorldMatrix reads them)
                if rd::<usize>(sim, 0x170) == 0 {
                    wr(sim, 0x170, a.alloc(0x200));
                }
                suspension_car(a, first_car);
            }
            if text == "carphysics" || text == "carengineer" {
                car_physics_car(a, first_car, if text == "carphysics" { "carphysics" } else { "carengineer" });
            }
            if text == "rstats" {
                // FormRenderStats(GUI*, GameTime*): Game::gameTime (Game+0x10)
                let (game_block,) = GAME.lock().unwrap().as_ref().map(|g| (g.game_block,)).unwrap_or_default();
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_402d_69d0));
                ctor(form, gui, (game_block as *mut u8).add(0x10));
                RSTATS.store(form as usize, std::sync::atomic::Ordering::SeqCst);
            } else if text == "gamestats" {
                let (game_block,) = GAME.lock().unwrap().as_ref().map(|g| (g.game_block,)).unwrap_or_default();
                let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_402e_7a10));
                ctor(form, game_block as *mut u8);
                // (its render looks PhysicsAvatar up among the game objects by their classes
                // and never uses it: the hand-laid Sim has no classes, the member is filled)
                wr(form, 0x1c0, a.alloc(0x400));
            } else if text == "trackmap" {
                // TrackMap::TrackMap(GUI*, Sim*, bool)
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(a.va(0x1_402e_e8e0));
                ctor(form, gui, sim, false);
            } else if text == "gpuprofiler" {
                // GPUProfilerApp(GraphicsManager*, GUI*): its rows go into the real GPUProfiler
                // (GraphicsManager+0x138), the names laid once (update() writes the times)
                let (game_block,) = GAME.lock().unwrap().as_ref().map(|g| (g.game_block,)).unwrap_or_default();
                let graphics: *mut u8 = rd(game_block as *mut u8, 0x138);
                let rows = a.alloc(crate::gui_seq_apps::GPU_NAMES.len() * 0x38);
                for (i, name) in crate::gui_seq_apps::GPU_NAMES.iter().enumerate() {
                    crate::ac::write_wstring(a, rows.add(i * 0x38 + 0x18), name);
                }
                *GPU_PROFILER.lock().unwrap() = Some((rd::<usize>(graphics, 0x138), rows as usize));
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_4031_b320));
                ctor(form, graphics, gui);
            } else if text == "help" {
                ctor(form, gui);
            } else if text == "cameraeditor" {
                // FormCamera(Sim*, CameraForwardYebis*): car 0's exposures (+0xfb8 / +0xfbc /
                // +0xfc0), its cameras (+0xfc8, room for six: the constructor copies the first
                // into the others in place), unixName (+0x138), eye point (+0x218), body node
                // (+0x1b8, matrix at +8); Sim::cameraManager (+0x188: cameraOnBoard +0x88 ->
                // customCameraSettings +0xc8, cameraDrivable +0xc0 -> drivableCameraPositions
                // +0x88), the focused car (+0x220), the scene camera (+0x238, matrix at +0xc);
                // YEBIS's camera (currentAutoExposure +0x7b4, useAutoExposureLimits +0x7a1).
                // The camera manager's mode calls are written down, not made.
                let l = crate::gui_seq_apps::camera_editor_start();
                crate::ac::write_wstring(a, first_car.add(0x138), &l.unix_name);
                wr(first_car, 0xfb8, [l.on_board_exposure, l.out_board_exposure, l.dash_board_exposure]);
                let cams = a.alloc(6 * 0x4c);
                for (i, c) in l.cameras.iter().enumerate() {
                    let at = cams.add(i * 0x4c);
                    wr(at, 0, c.matrix.m);
                    wr(at, 0x40, c.fov);
                    wr(at, 0x44, c.exposure);
                    wr(at, 0x48, c.external_sound as u8);
                }
                wr(first_car, 0xfc8, cams);
                wr(first_car, 0xfd0, cams.add(l.cameras.len() * 0x4c));
                wr(first_car, 0xfd8, cams.add(6 * 0x4c));
                wr(first_car, 0x218, l.eyes);
                wr(rd::<*mut u8>(first_car, 0x1b8), 8, l.body.m);
                let cars = GAME.lock().unwrap().as_ref().map(|g| g.cars.len()).unwrap_or(1).max(1);
                let manager = a.alloc(0x200);
                let cob = a.alloc(0x200);
                let settings = a.alloc(cars * 8);
                wr(settings, 0, l.onboard_pitch);
                wr(cob, 0xc8, settings);
                wr(cob, 0xd0, settings.add(cars * 8));
                wr(cob, 0xd8, settings.add(cars * 8));
                wr(manager, 0x88, cob);
                let drivable = a.alloc(0x200);
                let defs = a.alloc(cars * 0x38);
                wr(defs, 0, l.bumper_pos);
                wr(defs, 0xc, l.bumper_pitch);
                wr(defs, 0x10, l.bonnet_pos);
                wr(defs, 0x1c, l.bonnet_pitch);
                wr(drivable, 0x88, defs);
                wr(drivable, 0x90, defs.add(cars * 0x38));
                wr(drivable, 0x98, defs.add(cars * 0x38));
                wr(manager, 0xc0, drivable);
                wr(sim, 0x188, manager);
                wr(sim, 0x220, 0i32);
                let scene = a.alloc(0x100);
                wr(scene, 0xc, l.scene_camera.m);
                wr(sim, 0x238, scene);
                let yebis = a.alloc(0x800);
                wr(yebis, 0x7a1, 1u8);
                wr(yebis, 0x7b4, crate::gui_seq_apps::script(|s| s.auto_exposure));
                for (va, to) in [
                    (0x1_4003_40d0, camera_set_mode as *const () as usize),
                    (0x1_4003_3fd0, camera_set_drivable as *const () as usize),
                    (0x1_4003_3fb0, camera_set_car as *const () as usize),
                ] {
                    crate::gui_ac_hud::jump(game, va, to);
                }
                *CAMERA_EDITOR.lock().unwrap() = Some(CameraGame { car: first_car as usize, sim: sim as usize, yebis: yebis as usize, log: Vec::new(), last: String::new() });
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_402f_1bc0));
                ctor(form, sim, yebis);
            } else if text == "tyretester" {
                // TyreTesterForm(CarAvatar*, GUI*): its testers read content/cars/<unixName>/data/
                // from the working folder (the oracle's root, where the staging put tyres.ini)
                let unix = crate::gui_seq_apps::script(|s| s.unix.clone());
                crate::ac::write_wstring(a, first_car.add(0x138), &unix);
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_4033_b520));
                ctor(form, first_car, gui);
                *TYRE_TESTER.lock().unwrap() = Some(first_car as usize);
            } else {
                if text == "keyboard" {
                    *CONTROLS_CTOR.lock().unwrap() = Some(keyboard_class(a));
                }
                if text == "x360" {
                    *CONTROLS_CTOR.lock().unwrap() = Some(pad_class(a));
                }
                ctor(form, if TAKES_CAR.contains(&text) { first_car } else { sim });
                *CONTROLS_CTOR.lock().unwrap() = None;
            }
            if text == "telemetry" {
                telemetry_channels(a, form, sim)?;
            }
            if text == "netmonitor" {
                // the constructor finds no ACClient among the hand-laid Sim's game objects:
                // NetMonitorForm::client (+0x1c0) a stand-in whose avatar (+0x408) is car 0, so
                // getPing(car 0) answers sok.ping (+0x3a0 + 0x28); car 1's physicsStateProvider
                // (+0x11b8) a NetCarStateProvider's ICarPhysicsStateProvider part (+0x58, its
                // vftable for the render's dynamic cast), errorVector at +0x100
                let cars = GAME.lock().unwrap().as_ref().map(|g| g.cars.clone()).unwrap_or_default();
                let client = a.alloc(0x500);
                wr(client, 0x408, first_car);
                wr(form, 0x1c0, client);
                let net = a.alloc(0x200);
                wr(net, 0x58, a.va(0x1_404c_6e80));
                if let Some(car1) = cars.get(1) {
                    wr(*car1 as *mut u8, 0x11b8, net.add(0x58));
                }
                *NET_MONITOR.lock().unwrap() = Some((client as usize, net as usize));
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
        "app.shutdown" if text == "ppfilter" => {
            // ~PostProcessFilterSelector 0x1403299d0: video.ini's FILTER (the game's end)
            let destroy: extern "C" fn(*mut u8) = std::mem::transmute(a.va(0x1_4032_99d0));
            crate::gui_ac::DESTROYING.store(true, std::sync::atomic::Ordering::SeqCst);
            destroy(app("ppfilter")?);
        }
        "app.shutdown" if text == "chat" => {
            // ~FormChat 0x14030a2c0: its export (the game's end)
            let destroy: extern "C" fn(*mut u8) = std::mem::transmute(a.va(0x1_4030_a2c0));
            crate::gui_ac::DESTROYING.store(true, std::sync::atomic::Ordering::SeqCst);
            destroy(app("chat")?);
        }
        "app.shutdown" => {
            // ~ACDriverPerformance's saveDriverRecords 0x140306df0 (the game's end)
            let save: extern "C" fn(*mut u8) -> bool = std::mem::transmute(a.va(0x1_4030_6df0));
            save(app("driverperf")?);
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

/// FormChat's `_time64` / `_localtime64` (their import slots 0x1404a4a90 / 0x1404a4a88): the
/// script's clock, the same the port's chat app reads.
unsafe fn chat_clock(game: &Game) {
    unsafe extern "C" fn time64(out: *mut i64) -> i64 {
        if !out.is_null() {
            *out = 1;
        }
        1
    }
    unsafe extern "C" fn localtime64(_t: *const i64) -> *mut i32 {
        static mut TM: [i32; 9] = [0; 9];
        let c = crate::gui_seq_apps::chat_clock();
        let tm = std::ptr::addr_of_mut!(TM) as *mut i32;
        for (k, v) in [c[5], c[4], c[3], c[2], c[1] - 1, c[0] - 1900, 0, 0, 0].into_iter().enumerate() {
            *tm.add(k) = v;
        }
        tm
    }
    game.acs.patch(game.acs.va(0x1_404a_4a90), &(time64 as *const () as usize).to_le_bytes());
    game.acs.patch(game.acs.va(0x1_404a_4a88), &(localtime64 as *const () as usize).to_le_bytes());
}

/// The session's state where the game's apps read it themselves: every car's
/// `physicsState`, `Sim::focusedCarIndex`, `Sim::connectedCarsCount`, the replay manager's
/// "in replay", the game's clock.
pub unsafe fn update() {
    crate::gui_ac_esc::apps_client_sync();
    let guard = GAME.lock().unwrap();
    let Some(g) = guard.as_ref() else { return };
    let l = link();
    let camera = CHASE_CAMERA.load(std::sync::atomic::Ordering::Relaxed) as *mut u8;
    if !camera.is_null() {
        // ACCameraManager::mode eDrivable (2) or eCockpit (0); CameraDrivableManager eChase, index 0
        wr(camera, 0x120, if l.chase_cam_active { 2i32 } else { 0 });
        let drivable: *mut u8 = rd(camera, 0xc0);
        wr(drivable, 0x58, 0i32);
        wr(drivable, 0x128, 0i32);
    }
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
    // FormTelemetry: the physics steps since the last frame (each heard by the step handler),
    // then the values its render reads
    if let Some((raw, event)) = *TELEMETRY.lock().unwrap() {
        let raw = raw as *mut f32;
        let mut argument = [0u8; 0x40];
        for (pt, values) in &l.telemetry.steps {
            std::ptr::copy_nonoverlapping(values.as_ptr(), raw, values.len());
            argument[..8].copy_from_slice(&pt.to_le_bytes());
            crate::gui_ac_replay::fire(event as *const u8, argument.as_ptr());
        }
        std::ptr::copy_nonoverlapping(l.telemetry.graphics.as_ptr(), raw, l.telemetry.graphics.len());
    }
    // KeyboardControlsDebug: what its spinners do not change (they change the game's own)
    if let (Some(key), Some(k)) = (*KEYBOARD.lock().unwrap(), l.keyboard) {
        for (off, v) in [0x38, 0x3c, 0x34, 0x40, 0x44, 0x18].into_iter().zip(&k[5..]) {
            wr(key as *mut u8, off, *v);
        }
    }
    // X360ControllerDebug: lastMaxSteer, lastRaw, lastNormalized (its buttons change the rest)
    if let (Some(pad), Some(p)) = (*PAD.lock().unwrap(), l.pad) {
        wr(pad as *mut u8, 0x2c, [p[5], p[6], p[7]]);
    }
    // NetMonitorForm: the client's ping (u16), car 1's error vector
    if let Some((client, net)) = *NET_MONITOR.lock().unwrap() {
        wr(client as *mut u8, 0x3c8, l.ping as u16);
        wr(net as *mut u8, 0x100, l.net_error_vector);
    }
    // FormWings: the wings' states
    if let Some(status) = *WINGS.lock().unwrap() {
        for (i, row) in l.wings.iter().enumerate() {
            write_wing_state((status as *mut u8).add(i * 0x44), &row.state);
        }
    }
    // ACSuspension: physicsInfo caster (+0xe48), kpi (+0xf44, +0xf48), susBasePos (+0xee8);
    // the wheel nodes' places
    if let (Some((car, nodes)), Some(s)) = (*SUSPENSION.lock().unwrap(), l.suspension) {
        let car = car as *mut u8;
        wr(car, 0xe48, s.facts.caster);
        wr(car, 0xf44, [s.facts.kpi_rad, s.facts.scrub]);
        wr(car, 0xee8, s.facts.sus_base_pos);
        for (i, p) in s.wheel_nodes.iter().enumerate() {
            wr(nodes[i] as *mut u8, 8 + 0x30, *p);
        }
    }
    // GPUProfilerApp: its profiler's rows (QueryData 0x38: lastResult +8, lastTS +0x10), kept
    // off (isActive +0x28, waitCount +0x20: no query of the oracle's frames is measured)
    if let Some((profiler, rows)) = *GPU_PROFILER.lock().unwrap() {
        let (profiler, rows) = (profiler as *mut u8, rows as *mut u8);
        for (i, row) in l.gpu_profile.iter().enumerate() {
            wr(rows.add(i * 0x38), 8, [row.last_result, row.last_ts]);
        }
        wr(profiler, 8, [rows as usize, rows as usize + l.gpu_profile.len() * 0x38, rows as usize + crate::gui_seq_apps::GPU_NAMES.len() * 0x38]);
        wr(profiler, 0x20, 0i32);
        wr(profiler, 0x28, 0u8);
    }
    // TyreTesterForm: its car's physicsState.speed (+0x4d8) and load[4] (+0x54c),
    // currentTyreCompoundIndex[4] (+0x1260)
    if let (Some(car), Some(c)) = (*TYRE_TESTER.lock().unwrap(), l.tyre_tester) {
        wr(car as *mut u8, 0x4d8, c.speed);
        wr(car as *mut u8, 0x54c, c.load);
        wr(car as *mut u8, 0x1260, c.compound);
    }
    // TyreWearingDebug: its car's tyres (Car+0xb28, 0x858 each: status.virtualKM +0x350,
    // thermalModel.coreTemp +0x45c, isActive +0x4e0)
    if let (Some(car), Some(rows)) = (*TYRE_DEBUG.lock().unwrap(), l.tyre_debug) {
        for (i, t) in rows.iter().enumerate() {
            let tyre = (car as *mut u8).add(0xb28 + i * 0x858);
            wr(tyre, 0x350, t.virtual_km);
            wr(tyre, 0x45c, t.core_temp);
            wr(tyre, 0x4e0, t.active as u8);
        }
    }
    // SetupDebug: the Car's tyres' pressureStatic (+0x344), wings' status.angle (+0x25c),
    // suspensions' rodLength (+0x24), gears' ratios, finalRatio (+0x314); physicsInfo.maxGear
    if let (Some((car, engineer)), Some(c)) = (*SETUP_DEBUG.lock().unwrap(), l.setup_debug.as_ref()) {
        let car = car as *mut u8;
        // (the cars without a force feedback gain lost theirs above)
        for avatar in &g.cars {
            wr(*avatar as *mut u8, 0xf80, engineer);
        }
        for i in 0..4 {
            wr(car, 0xb28 + i * 0x858 + 0x344, c.pressure_static[i]);
            wr(rd::<*mut u8>(rd::<*mut u8>(car, 0x2c88), i * 8), 0x24, c.rod_lengths[i]);
        }
        let wings: *mut u8 = rd(car, 0xaf0);
        for (w, angle) in c.wing_angles.iter().enumerate() {
            wr(wings, w * 0x310 + 0x25c, *angle);
        }
        let gears: *mut u8 = rd(car, 0x310 + 0x98);
        for (k, ratio) in c.gear_ratios.iter().enumerate() {
            wr(gears, k * 0x28, *ratio);
        }
        wr(car, 0x314, c.final_ratio);
        for avatar in &g.cars {
            wr(*avatar as *mut u8, 0xe44, c.max_gear);
        }
    }
    // CarPhysicsForm: getDynamicIndex's inputs, the fuel evaluator (Car+0x3af8: totalM +0x20,
    // totalLiters +0x28), CarAvatar::filteredSpeed (+0x1278)
    if let Some((car, body, suspensions, engineer, "carengineer")) = *CAR_PHYSICS.lock().unwrap() {
        // ACDCarEngineer: getK (+0x188), getDamper (+0x190: reboundSlow, bumpSlow +8), the front
        // heave spring (Car+0x3b40: isPresent, k +0xc, reboundSlow +0x24, bumpSlow +0x2c);
        // physicsInfo's totalMass (+0xe6c), bodyMass, unsprungWeights
        let e = crate::gui_seq_apps::car_engineer(crate::gui_seq_apps::script(|s| s.frame));
        let (car, body) = (car as *mut u8, body as *mut u8);
        for (j, k) in [0, 2].into_iter().enumerate() {
            let p = suspensions[k] as *mut u8;
            wr(p, 0x180, e.z[j]);
            wr(p, 0x188, e.k[j]);
            wr(p, 0x190, e.dampers[j][0]);
            wr(p, 0x198, e.dampers[j][1]);
        }
        wr(body, 0x180, e.mass);
        let h = e.heave.unwrap_or_default();
        wr(car, 0x3b40, e.heave.is_some() as u8);
        wr(car, 0x3b4c, h[0]);
        wr(car, 0x3b64, h[1]);
        wr(car, 0x3b6c, h[2]);
        if let Some(avatar) = g.cars.first() {
            let avatar = *avatar as *mut u8;
            wr(avatar, 0xf80, engineer);
            wr(avatar, 0xe6c, e.total_mass);
            wr(avatar, 0xe70, e.body_mass);
            for i in 0..4 {
                wr(avatar, 0xe74 + i * 4, e.unsprung[i]);
            }
        }
    }
    if let Some((car, body, suspensions, _, "carphysics")) = *CAR_PHYSICS.lock().unwrap() {
        let (z, mass, iyy, total_m, total_liters) = crate::gui_seq_apps::car_physics_raw(crate::gui_seq_apps::script(|s| s.frame));
        wr(suspensions[0] as *mut u8, 0x180, z[0]);
        wr(suspensions[2] as *mut u8, 0x180, z[1]);
        wr(body as *mut u8, 0x180, mass);
        wr(body as *mut u8, 0x184, iyy);
        wr(car as *mut u8, 0x3b18, total_m);
        wr(car as *mut u8, 0x3b20, total_liters);
        if let Some(avatar) = g.cars.first() {
            wr(*avatar as *mut u8, 0x1278, l.car_physics.filtered_speed);
        }
    }
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
    // Task 33: GameTime::fps, Game::stats, GraphicsManager::stats (after beginScene's reset)
    wr(g.game_block as *mut u8, 0x28, l.stats.fps);
    for (at, v) in [(0x150, l.stats.cpu_time), (0x158, l.stats.update_time), (0x160, l.stats.render_hud_time), (0x168, l.stats.render_time), (0x170, l.stats.render_audio_time)] {
        wr(g.game_block as *mut u8, at, v);
    }
    let graphics: *mut u8 = rd(g.game_block as *mut u8, 0x138);
    if !graphics.is_null() && RSTATS.load(std::sync::atomic::Ordering::SeqCst) != 0 {
        wr(graphics, 0x60, l.stats.dip_calls);
        wr(graphics, 0x68, l.stats.triangles);
        wr(graphics, 0x6c, l.stats.scene_triangles);
    }
    // RaceManager::penaltiesEnabled
    wr(rd::<*mut u8>(sim, 0x1a8), 0xa0, l.penalties_enabled as u8);
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
    if let Some(c) = CAMERA_EDITOR.lock().unwrap().as_mut() {
        wr(c.yebis as *mut u8, 0x7b4, crate::gui_seq_apps::script(|s| s.auto_exposure));
        let line = crate::gui_port_apps::camera_line(&camera_editor_link(c));
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), std::mem::take(&mut c.log), &line, &mut c.last);
    }
    if let Some(p) = PHOTO.lock().unwrap().as_mut() {
        let (link, animator) = photo_link(p);
        let line = crate::gui_port_apps::photo_line(&link, animator);
        let root = crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::gui_port_apps::append_effects(&root.join("effects_game.txt"), std::mem::take(&mut p.log), &line, &mut p.last);
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

/// FormTelemetry's `Car` (CarAvatar+0x1168), what `initChannels` reads of it: the engine
/// (Car+0x3f0, its table for `getLimiterRPM`: `(int)((f32)limiter * mult)`), `clutchMaxTorque`
/// (+0x7e0), `tractionType` (+0x890), the dynamic anti-roll bars' `isReady` (+0x3478, +0x34c0;
/// the byte at +0x20); and INIReaderDocuments::basePath for `cfg/telemetry_user.ini`.
unsafe fn telemetry_car(a: &crate::acs::Acs, avatar: *mut u8) {
    let c = crate::gui_seq_apps::TELEMETRY_CAR;
    let car = a.alloc(0x4000);
    wr(car, 0x3f0, a.va(0x1_404f_8fd8) as usize);
    wr(car, 0x3f0 + 0x11c, c.limiter_rpm);
    wr(car, 0x3f0 + 0x14c, 1.0f32);
    wr(car, 0x7e0, c.clutch_max_torque);
    wr(car, 0x890, if c.awd2 { 3i32 } else { 0 });
    wr(car, 0x3478 + 0x20, c.arb_ready[0] as u8);
    wr(car, 0x34c0 + 0x20, c.arb_ready[1] as u8);
    wr(avatar, 0x1168, car);
    let base = format!("{}/Assetto Corsa/", crate::root::documents().to_string_lossy());
    crate::ac::write_wstring(a, a.va(VA_INIREADERDOCUMENTS_BASE_PATH) as *mut u8, &base);
}

/// TyreWearingDebug's car: `PhysicsEngine::cars` (PhysicsAvatar+0x58+8) holds one `Car` whose
/// tyres have the game's own wear `Curve` (`Curve::Curve` 0x1402058b0, `addValue` 0x140205ae0);
/// every avatar gets a `physicsStateProvider` (+0x11b8), which the app only tests.
/// FormWings' car: `physicsInfo.wingData` (CarAvatar+0xe88, WingData 0x250: name, chord +0x20,
/// span +0x24, position +0x28, isVertical +0x24c) and `wingsStatus` (+0xfa0, WingState 0x44),
/// laid once here; [`update`] writes the states every frame.
unsafe fn wings_car(a: &crate::acs::Acs, avatar: *mut u8) {
    let rows = crate::gui_seq_apps::wing_rows(0);
    let data = a.alloc(rows.len() * 0x250 + 8);
    let status = a.alloc(rows.len() * 0x44 + 8);
    for (i, r) in rows.iter().enumerate() {
        let d = data.add(i * 0x250);
        crate::ac::write_wstring(a, d, &r.name);
        wr(d, 0x20, r.chord);
        wr(d, 0x24, r.span);
        wr(d, 0x28, r.position);
        wr(d, 0x24c, r.is_vertical as u8);
    }
    wr(avatar, 0xe88, data);
    wr(avatar, 0xe90, data.add(rows.len() * 0x250));
    wr(avatar, 0xfa0, status);
    wr(avatar, 0xfa8, status.add(rows.len() * 0x44));
    *WINGS.lock().unwrap() = Some(status as usize);
}

static WINGS: Mutex<Option<usize>> = Mutex::new(None);
static TYRE_TESTER: Mutex<Option<usize>> = Mutex::new(None);
/// the game's GPUProfiler, the rows laid for it
static GPU_PROFILER: Mutex<Option<(usize, usize)>> = Mutex::new(None);
/// ACSuspension's car and its four wheel nodes
static SUSPENSION: Mutex<Option<(usize, [usize; 4])>> = Mutex::new(None);

/// `ISuspensionAvatar::getWheelTransform(i)` (vtable +0x10) of the stand-in: the i-th node
extern "C" fn get_wheel_transform(this: *mut u8, i: i32) -> *mut u8 {
    // SAFETY: the stand-in holds four node pointers after its table
    unsafe { rd(this, 8 + 8 * (i as usize & 3)) }
}

/// The car's body node and a suspension avatar (+0xff8) with four wheel nodes, all game
/// `Node`s (vtable 0x1404eb558: getWorldMatrix copies +8 when there is no parent at +0xa8).
unsafe fn suspension_car(a: &crate::acs::Acs, avatar: *mut u8) {
    const VA_NODE_VTABLE: usize = 0x1_404e_b558;
    let body: *mut u8 = rd(avatar, 0x1b8);
    wr(body, 0, a.va(VA_NODE_VTABLE));
    let node = || {
        let n = a.alloc(0x100);
        wr(n, 0, a.va(VA_NODE_VTABLE));
        wr(n, 8, rustyac_physics::vecmath::Mat44f::IDENTITY.m);
        n as usize
    };
    let nodes = [node(), node(), node(), node()];
    let table = a.alloc(0x100);
    for slot in 0..32 {
        wr(table, slot * 8, get_wheel_transform as *const () as usize);
    }
    let sus = a.alloc(0x40);
    wr(sus, 0, table);
    for (i, n) in nodes.iter().enumerate() {
        wr(sus, 8 + 8 * i, *n);
    }
    wr(avatar, 0xff8, sus);
    *SUSPENSION.lock().unwrap() = Some((avatar as usize, nodes));
}

/// A `WingState` as the game lays it (0x44 bytes).
unsafe fn write_wing_state(at: *mut u8, s: &rustyac_physics::car::aero::WingState) {
    for (o, v) in [(0x0, s.aoa), (0x4, s.cd), (0x8, s.cl), (0xc, s.angle), (0x10, s.input_angle), (0x14, s.ground_height), (0x18, s.front_share), (0x1c, s.drag_kg), (0x20, s.lift_kg), (0x24, s.angle_mult), (0x28, s.ground_effect_lift), (0x2c, s.ground_effect_drag), (0x30, s.yaw_angle)] {
        wr(at, o, v);
    }
    wr(at, 0x34, s.is_vertical as u8);
    wr(at, 0x38, [s.lift_vector.x, s.lift_vector.y, s.lift_vector.z]);
}

unsafe fn tyre_debug_car(a: &crate::acs::Acs, sim: *mut u8) {
    let car = a.alloc(0x4000);
    let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(a.va(0x1_4020_58b0));
    let add: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(a.va(0x1_4020_5ae0));
    for i in 0..4 {
        let curve = car.add(0xb28 + i * 0x858 + 0xb0);
        ctor(curve);
        for (x, y) in crate::gui_seq_apps::TYRE_WEAR_CURVE {
            add(curve, x, y);
        }
    }
    let cars = a.alloc(16);
    wr(cars, 0, car);
    let physics: *mut u8 = rd(sim, 0x1b8);
    wr(physics, 0x60, cars);
    wr(physics, 0x68, cars.add(8));
    wr(physics, 0x70, cars.add(8));
    for avatar in GAME.lock().unwrap().as_ref().map(|g| g.cars.clone()).unwrap_or_default() {
        wr(avatar as *mut u8, 0x11b8, a.alloc(0x100));
    }
    *TYRE_DEBUG.lock().unwrap() = Some(car as usize);
}

/// SetupDebug's car, every avatar's `raceEngineer->car` (one engineer for all): its wings (`aeroMap.wings`, 0x310
/// each, at +0xaf0), suspensions (pointers at +0x2c88), gears (`drivetrain.gears`, 0x28 each, at
/// +0x310+0x98) as many as the script's; `physicsInfo.tyreCompounds` (+0xf18) holds the
/// compound's name, `currentTyreCompoundIndex[0]` (+0x1260) is 0.
unsafe fn setup_debug_car(a: &crate::acs::Acs) {
    let c = crate::gui_seq_apps::setup_debug_car(0);
    let car = a.alloc(0x4000);
    let wings = a.alloc(c.wing_angles.len().max(1) * 0x310);
    wr(car, 0xaf0, wings);
    wr(car, 0xaf8, wings.add(c.wing_angles.len() * 0x310));
    let suspensions = a.alloc(32);
    for i in 0..4 {
        wr(suspensions, i * 8, a.alloc(0x100));
    }
    wr(car, 0x2c88, suspensions);
    wr(car, 0x2c90, suspensions.add(32));
    let gears = a.alloc(c.gear_ratios.len().max(1) * 0x28);
    wr(car, 0x310 + 0x98, gears);
    wr(car, 0x310 + 0xa0, gears.add(c.gear_ratios.len() * 0x28));
    let names = a.alloc(0x20);
    empty_wstring(names);
    crate::ac::write_wstring(a, names, &c.tyre_compound);
    let engineer = a.alloc(0x40);
    wr(engineer, 8, car);
    for avatar in GAME.lock().unwrap().as_ref().map(|g| g.cars.clone()).unwrap_or_default() {
        let avatar = avatar as *mut u8;
        wr(avatar, 0xf80, engineer);
        wr(avatar, 0xf18, names);
        wr(avatar, 0x1260, 0i32);
    }
    *SETUP_DEBUG.lock().unwrap() = Some((car as usize, engineer as usize));
}

/// `ISuspension::getBasePosition` (+0x50) of the stand-in: (0, 0, z)
unsafe extern "C" fn stand_in_base_position(this: *mut u8, out: *mut f32) -> *mut f32 {
    *out = 0.0;
    *out.add(1) = 0.0;
    *out.add(2) = rd(this, 0x180);
    out
}

/// `ISuspension::getK` (+0x58) of the stand-in
unsafe extern "C" fn stand_in_k(this: *mut u8) -> f32 {
    rd(this, 0x188)
}

/// `ISuspension::getDamper` (+0x60) of the stand-in
unsafe extern "C" fn stand_in_damper(this: *mut u8) -> *mut u8 {
    this.add(0x190)
}

/// `IRigidBody::getMass` (+0x28) of the stand-in
unsafe extern "C" fn stand_in_mass(this: *mut u8) -> f32 {
    rd(this, 0x180)
}

/// `IRigidBody::getLocalInertia` (+0x40) of the stand-in: (0, iyy, 0)
unsafe extern "C" fn stand_in_inertia(this: *mut u8, out: *mut f32) -> *mut f32 {
    *out = 0.0;
    *out.add(1) = rd(this, 0x184);
    *out.add(2) = 0.0;
    out
}

/// CarPhysicsForm's car: the player's avatar's `raceEngineer` (+0xf80, read by the
/// constructor) and `physics` (+0x1168) are one `Car` with a body (+0x118) and suspensions
/// (+0x2c88) that answer `getDynamicIndex`.
unsafe fn car_physics_car(a: &crate::acs::Acs, avatar: *mut u8, app: &'static str) {
    let car = a.alloc(0x4000);
    let table = a.alloc(0x100);
    wr(table, 0x28, stand_in_mass as *const () as usize);
    wr(table, 0x40, stand_in_inertia as *const () as usize);
    wr(table, 0x50, stand_in_base_position as *const () as usize);
    wr(table, 0x58, stand_in_k as *const () as usize);
    wr(table, 0x60, stand_in_damper as *const () as usize);
    let body = a.alloc(0x200);
    wr(body, 0, table);
    wr(car, 0x118, body);
    let list = a.alloc(32);
    let mut suspensions = [0usize; 4];
    for (i, s) in suspensions.iter_mut().enumerate() {
        let p = a.alloc(0x200);
        wr(p, 0, table);
        wr(list, i * 8, p);
        *s = p as usize;
    }
    wr(car, 0x2c88, list);
    wr(car, 0x2c90, list.add(32));
    let engineer = a.alloc(0x40);
    wr(engineer, 8, car);
    wr(avatar, 0xf80, engineer);
    wr(avatar, 0x1168, car);
    // (the constructor's getDynamicIndex divides zeros: render overwrites it before drawing)
    *CAR_PHYSICS.lock().unwrap() = Some((car as usize, body as usize, suspensions, engineer as usize, app));
}

/// FormTelemetry's `channels` map (+0x1d8: head, size; the nodes' key at +0x20, the
/// `TelemetryChannelDef` at +0x40) against `app_telemetry::channels`: the same names, limits,
/// scales, threads, skips and auto ranges. Then every channel reads its raw value from one
/// float of ours (`inputValue`, the lambda emptied: `scale * value` either way).
unsafe fn telemetry_channels(a: &crate::acs::Acs, form: *mut u8, sim: *mut u8) -> Result<(), String> {
    let port = rustyac_gui::app_telemetry::channels(&crate::gui_seq_apps::TELEMETRY_CAR);
    let head: *mut u8 = rd(form, 0x1d8);
    let size: usize = rd(form, 0x1e0);
    if size != port.len() {
        return Err(format!("FormTelemetry has {size} channels, the port {}", port.len()));
    }
    let raw = a.alloc(port.len() * 4);
    let mut stack = vec![rd::<*mut u8>(head, 8)];
    while let Some(node) = stack.pop() {
        if rd::<u8>(node, 0x19) != 0 {
            continue;
        }
        stack.push(rd(node, 0));
        stack.push(rd(node, 0x10));
        let name = crate::gui_ac::read_wstring(node.add(0x20));
        let def = node.add(0x40);
        let i = port.iter().position(|d| d.name == name).ok_or(format!("the port has no channel {name:?}"))?;
        let p = &port[i];
        let game = (rd::<f32>(def, 0x28).to_bits(), rd::<f32>(def, 0x2c).to_bits(), rd::<f32>(def, 0x30).to_bits(), rd::<u8>(def, 0x34) != 0, rd::<i32>(def, 0x38), rd::<u8>(def, 0x40) != 0);
        if game != (p.min.to_bits(), p.max.to_bits(), p.scale.to_bits(), p.physics, p.jump, p.auto_range) {
            return Err(format!("channel {name}: the game's {game:?}, the port's {p:?}"));
        }
        wr(def, 0, raw.add(i * 4));
        wr(def, 0x20, 0usize);
    }
    let physics_avatar: *mut u8 = rd(sim, 0x1b8);
    *TELEMETRY.lock().unwrap() = Some((raw as usize, physics_avatar.add(0x2d0) as usize));
    Ok(())
}

/// KeyboardControlsDebug's controls while it is made (the constructor alone asks for them,
/// through the `getControlsProvider` stand-in): a `KeyboardCarControl` (its vftable for the
/// constructor's dynamic cast) when the script has one, else none (`KEYBOARD CONTROL NOT
/// SELECTED`).
unsafe fn keyboard_class(a: &crate::acs::Acs) -> usize {
    let Some(k) = crate::gui_seq_apps::script(|s| s.keyboard) else { return 0 };
    let key = a.alloc(0x100);
    wr(key, 0, a.va(0x1_404c_5d50));
    // steerSpeed, steerOppositeDirectionFactor, steerGain, steerResetFactor, lookAhead,
    // speedDeceleringFactor, turnDeceleringFactor, steerDecelerator, intGas, intSteer, minSteering
    for (off, v) in [0x1c, 0x20, 0x24, 0x28, 0x2c, 0x38, 0x3c, 0x34, 0x40, 0x44, 0x18].into_iter().zip(k) {
        wr(key, off, v);
    }
    *KEYBOARD.lock().unwrap() = Some(key as usize);
    key as usize
}

/// X360ControllerDebug's controls while it is made: a `JoypadCarControl` (its vftable for the
/// constructor's dynamic cast, its fields from +0x18 on) when the script has one, else none.
unsafe fn pad_class(a: &crate::acs::Acs) -> usize {
    let Some(p) = crate::gui_seq_apps::script(|s| s.pad) else { return 0 };
    let pad = a.alloc(0x200);
    wr(pad, 0, a.va(0x1_404e_0d38));
    wr(pad, 0x18, p);
    *PAD.lock().unwrap() = Some(pad as usize);
    pad as usize
}
