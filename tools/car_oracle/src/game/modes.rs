//! Task 45: the game's own special-event goals against the port's
//! (`rustyac_physics::race::game_mode`), in-process:
//!
//! * `SimpleFileEncrypter::decryptFile` on every `system/data/se/SPECIAL_EVENT_*.acd` of the
//!   install against `rustyac_content::acd::decrypt_simple`, compared in memory (nothing
//!   decrypted is written anywhere);
//! * `GameMode::GameMode` reading a race.ini (the oracle's own `cfg/race.ini`, and through it
//!   every official event) against `GameMode::new`;
//! * `HotlapMode`'s lap handler on made-up laps against `Mode::on_lap_completed`;
//! * `TimeAttack::update` (with its penalties and goals) on made-up frames against
//!   `rustyac_physics::race::time_attack`, field by field every frame;
//! * `DriftModeComponent::step` on made-up drives against `rustyac_physics::car::drift`,
//!   every member every physics step;
//! * `DragMode` on made-up matches against `rustyac_physics::race::drag` (`modes/drag.rs`).

mod drag;

use std::cell::RefCell;
use std::path::Path;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::game_mode::{Condition, GameMode, Mode};
use rustyac_physics::race::manager::SESSION_HOTLAP;
use rustyac_physics::race::time_attack::{TaFrame, TaMode, TimeAttack};
use rustyac_physics::track::timing::OnLapCompletedEvent;

use super::race_ini::{rd, text, wr, write_wstring, zeroed};
use crate::acs::Acs;

const VA_DECRYPT_FILE: usize = 0x1_4023_b650; // vector<char> SimpleFileEncrypter::decryptFile(const wstring&, const wstring&)
const VA_GAME_MODE_CTOR: usize = 0x1_400b_c810; // GameMode::GameMode(const wstring&, Game*)
const VA_HOTLAP_LAP: usize = 0x1_400b_eb80; // HotlapMode's lambda on OnLapCompletedEvent
const VA_ADD_MESSAGE: usize = 0x1_401b_61c0; // SystemMessage::addMessage(wstring, wstring, type)
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588;
const VA_INIREADERDOCUMENTS_BASE_PATH: usize = 0x1_4151_d100;
const GM_GUID: usize = 0x88;
const GM_CONDITIONS: usize = 0xa8;
const GM_TIER: usize = 0xc4;
const HOTLAP_SIZE: usize = 0xd8;
const HOTLAP_SIM: usize = 0xd0;

thread_local! {
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn add_message(_messages: *mut u8, message: *const u8, description: *const u8, _kind: i32) {
    let line = unsafe { format!("Message({} | {})", text(message), text(description)) };
    SAID.with(|s| s.borrow_mut().push(line));
}

#[derive(Default)]
pub struct Report {
    pub files: usize,
    pub bytes: usize,
    pub inis: usize,
    pub conditions: usize,
    pub laps: usize,
    pub messages: usize,
    pub json_bytes: usize,
    pub ta_sessions: usize,
    pub ta_frames: usize,
    pub ta_checkpoints: usize,
    pub ta_messages: usize,
    pub drift_steps: usize,
    pub drifts: usize,
    pub drift_combos: usize,
    pub drift_points: f64,
    pub drag_sessions: usize,
    pub drag_frames: usize,
    pub drag_matches: usize,
    pub drag_penalties: usize,
    pub drag_messages: usize,
    pub mismatch: Option<String>,
}

/// The game's goals of a `GameMode` block.
unsafe fn game_side(mode: *const u8) -> (String, Vec<Condition>, i32) {
    let (begin, end): (usize, usize) = (rd(mode, GM_CONDITIONS), rd(mode, GM_CONDITIONS + 8));
    let conditions = (begin..end)
        .step_by(0x48)
        .map(|c| {
            let c = c as *const u8;
            Condition { kind: text(c), name: text(c.add(0x20)), objective: rd(c, 0x40), achieved: rd::<u8>(c, 0x44) != 0 }
        })
        .collect();
    (text(mode.add(GM_GUID)), conditions, rd(mode, GM_TIER))
}

/// `root`: the oracle's scratch folder (its `cfg/race.ini` is written there); `ac`: the
/// install, the game's working folder while it reads (it only reads).
pub unsafe fn micro(acs: &Acs, root: &Path, ac: &Path, seed: u32, laps_per_case: usize) -> Result<Report, String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);
    write_wstring(acs.va(VA_INIREADERDOCUMENTS_BASE_PATH) as *mut u8, &format!("{}/", root.to_string_lossy().replace('\\', "/")));
    let mut report = Report::default();
    let differ = |report: &mut Report, what: String| {
        if report.mismatch.is_none() {
            report.mismatch = Some(what);
        }
    };

    // 1. every official event, decrypted by both
    let se = ac.join("system/data/se");
    let mut guids = Vec::new();
    let decrypt: extern "C" fn(*mut [usize; 3], *const u8, *const u8) -> *mut [usize; 3] = std::mem::transmute(acs.va(VA_DECRYPT_FILE));
    for entry in std::fs::read_dir(&se).map_err(|e| format!("{}: {e}", se.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let Some(guid) = stem.strip_prefix("SPECIAL_EVENT_").and_then(|n| n.parse::<i32>().ok()) else { continue };
        let key = format!("ARUMBA_{stem}");
        let (path_w, key_w) = (zeroed(0x20), zeroed(0x20));
        write_wstring(path_w, &path.to_string_lossy());
        write_wstring(key_w, &key);
        let mut out = [0usize; 3];
        decrypt(&mut out, path_w, key_w);
        let game = std::slice::from_raw_parts(out[0] as *const u8, out[1] - out[0]);
        let file = std::fs::read(&path).map_err(|e| e.to_string())?;
        let port = rustyac_content::acd::decrypt_simple(&file, &key);
        if port.as_deref() != Some(game) {
            differ(&mut report, format!("{stem}: decrypted {} bytes by the game, {:?} by the port", game.len(), port.map(|p| p.len())));
        }
        report.files += 1;
        report.bytes += game.len();
        guids.push(guid);
    }
    guids.sort();

    // 2. race.ini files: none, a mod's event (no file), one that names a missing file, every official one
    let mut cases = vec![
        "[RACE]\nTRACK=magione\n".to_string(),
        "[SPECIAL_EVENT]\nGUID=999999\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=90000\nACHIEVED=1\n[CONDITION_1]\nTYPE=TIME\nOBJECTIVE=87000\n[CONDITION_2]\nTYPE=POINTS\nOBJECTIVE=85000\nACHIEVED=1\n[CONDITION_3]\nTYPE=TIME\nOBJECTIVE=84000\n[CONDITION_4]\nOBJECTIVE=-5\nACHIEVED=0\n".to_string(),
        // GUID 0 names no file, so race.ini stays (a GUID that is not a number throws in the game)
        "[SPECIAL_EVENT]\nGUID=0\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=60000\n".to_string(),
    ];
    cases.extend(guids.iter().enumerate().map(|(i, guid)| format!("[SPECIAL_EVENT]\nGUID={guid}\n[CONDITION_{}]\nACHIEVED=1\n", i % 4)));
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GAME_MODE_CTOR));
    let lap_handler: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_HOTLAP_LAP));
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let file = root.join("cfg/race.ini");
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    let mut kept = None;
    let mut rand = seed;
    let mut next = move || {
        rand = rand.wrapping_mul(214_013).wrapping_add(2_531_011);
        rand >> 16 & 0x7fff
    };
    for (case, ini) in cases.iter().enumerate() {
        std::fs::write(&file, ini).map_err(|e| e.to_string())?;
        let mode = zeroed(HOTLAP_SIZE);
        let name = zeroed(0x20);
        write_wstring(name, "HOTLAP_MODE");
        std::env::set_current_dir(ac).map_err(|e| e.to_string())?;
        ctor(mode, name, zeroed(0x278));
        std::env::set_current_dir(&here).map_err(|e| e.to_string())?;
        let documents = IniReader::load(&file)?;
        let official = documents.official_special_event();
        let mut port = GameMode::new(official.as_ref().unwrap_or(&documents), &documents, official.is_some());
        let game = game_side(mode);
        if (&port.guid, &port.conditions, port.current_tier) != (&game.0, &game.1, game.2) {
            differ(&mut report, format!("race.ini case {case}: the game read {game:?}, the port {:?}", (&port.guid, &port.conditions, port.current_tier)));
            continue;
        }
        report.inis += 1;
        report.conditions += port.conditions.len();

        // 3. the hotlap judge on made-up laps
        wr(mode, HOTLAP_SIM, zeroed(0x2d0));
        let lambda = zeroed(8);
        wr(lambda, 0, mode);
        port.official = false;
        let mut port_mode = Mode::of(SESSION_HOTLAP, &port).ok_or("no hotlap mode")?;
        let objectives: Vec<i32> = port.conditions.iter().map(|c| c.objective).collect();
        for lap in 0..laps_per_case {
            let base = objectives.get(next() as usize % objectives.len().max(1)).copied().unwrap_or(60_000);
            let event = OnLapCompletedEvent {
                lap_time: if next() % 16 == 0 { 0 } else { (base as i64 + next() as i64 % 4001 - 2000).max(0) as u32 },
                is_valid: next() % 8 != 0,
                cuts: (next() % 8 == 0) as i32,
                ..OnLapCompletedEvent::default()
            };
            let block = zeroed(0x40);
            wr(block, 4, event.lap_time);
            wr(block, 0x30, event.is_valid as u8);
            wr(block, 0x34, event.cuts);
            SAID.with(|s| s.borrow_mut().clear());
            lap_handler(lambda, block);
            let game_said = SAID.with(|s| std::mem::take(&mut *s.borrow_mut()));
            let mut port_said = Vec::new();
            port_mode.on_lap_completed(&event, &mut |a, b| port_said.push(format!("Message({a} | {b})")));
            let game = game_side(mode);
            let base = port_mode.base();
            if game_said != port_said || (&base.conditions, base.current_tier) != (&game.1, game.2) {
                differ(&mut report, format!("race.ini case {case} lap {lap} {event:?}: the game said {game_said:?} tier {}, the port {port_said:?} tier {}", game.2, base.current_tier));
                break;
            }
            report.laps += 1;
            report.messages += game_said.len();
        }
        if case == 1 {
            kept = Some((mode, port_mode.extras()));
        }
    }
    if report.mismatch.is_some() {
        return Ok(report);
    }

    // 4. race_out.json with the mod event's mode among the game objects (one drive per process)
    let race = super::race::micro(acs, seed, 0, 2, 20_000, false, kept)?;
    report.json_bytes = race.json_bytes;
    if let Some(what) = race.mismatch {
        differ(&mut report, format!("the race with a mode: {what}"));
        return Ok(report);
    }

    // 5. Time Attack (after the race: its stand-ins replace the race manager's lap getters)
    std::fs::write(&file, &cases[1]).map_err(|e| e.to_string())?;
    let documents = IniReader::load(&file)?;
    let base = GameMode::new(&documents, &documents, false);
    std::env::set_current_dir(ac).map_err(|e| e.to_string())?;
    let done = time_attack(acs, &base, &mut next, laps_per_case * 100, &mut report);
    std::env::set_current_dir(&here).map_err(|e| e.to_string())?;
    if let Err(what) = done {
        differ(&mut report, what);
    }
    // 6. the drift points of the physics
    if let Err(what) = drift_component(acs, &mut next, laps_per_case * 2000, &mut report) {
        differ(&mut report, what);
    }
    // 7. drag matches (last: its stand-ins replace the others')
    std::env::set_current_dir(ac).map_err(|e| e.to_string())?;
    let done = drag::drag(acs, &base, seed, &mut next, laps_per_case * 400, &mut report);
    std::env::set_current_dir(&here).map_err(|e| e.to_string())?;
    if let Err(what) = done {
        differ(&mut report, what);
    }
    Ok(report)
}

const VA_DRIFT_STEP: usize = 0x1_402b_ff20; // DriftModeComponent::step(float)
const CAR_BODY: usize = 0x118;
const CAR_GEAR: usize = 0x894;
const CAR_TYRES: usize = 0xb28; // Tyre[4], 0x858 each: surfaceDef* +0x410, TyreStatus +0x2e8
const CAR_DAMAGE: usize = 0x3de0;
const CAR_DRIFT: usize = 0x3510;
const CAR_SPEED: usize = 0x3e90;

thread_local! {
    static LOCAL_VELOCITY: std::cell::Cell<[f32; 3]> = const { std::cell::Cell::new([0.0; 3]) };
}

extern "C" fn local_velocity(_body: *mut u8, out: *mut [f32; 3]) -> *mut [f32; 3] {
    unsafe { *out = LOCAL_VELOCITY.with(|v| v.get()) };
    out
}

/// The game's `DriftModeComponent` against the port's, on drives made of slides one way and
/// the other, straights, slow parts, dirt, knocks and neutral.
unsafe fn drift_component(acs: &Acs, next: &mut dyn FnMut() -> u32, steps: usize, report: &mut Report) -> Result<(), String> {
    use rustyac_physics::car::drift::{DriftInput, DriftModeComponent, DriftTyre};
    let car = zeroed(0x4000);
    let vtable = zeroed(0x100);
    wr(vtable, 0xb8, local_velocity as *const () as usize);
    let body = zeroed(0x40);
    wr(body, 0, vtable);
    wr(car, CAR_BODY, body);
    let surfaces = [zeroed(0x100), zeroed(0x100), zeroed(0x100), zeroed(0x100)];
    let game = car.add(CAR_DRIFT);
    wr(game, 0, car);
    let step: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_DRIFT_STEP));
    let mut port = DriftModeComponent::default();
    let mut unit = || (next() % 10_000) as f32 / 10_000.0;
    let (mut speed, mut angle, mut side, mut gear) = (30.0f32, 0.0f32, 1.0f32, 3);
    let mut f = DriftInput::default();
    let (mut was_drifting, mut combo) = (false, 0);
    for n in 0..steps {
        // a new part of the drive now and then
        if n % 600 == 0 {
            let r = unit();
            angle = if r < 0.15 { 0.0 } else if r < 0.25 { 0.1 } else { 0.15 + unit() * 0.9 };
            speed = if unit() < 0.1 { 4.0 } else { 6.0 + unit() * 40.0 };
            gear = if unit() < 0.05 { 0 } else { 2 + (unit() * 4.0) as i32 };
        }
        if n % 200 == 0 && unit() < 0.4 {
            side = -side; // a flick the other way: a combo
        }
        speed = (speed + (unit() - 0.5) * 0.2).max(0.0);
        let a = side * (angle + (unit() - 0.5) * 0.02);
        f.local_velocity = [speed * a.sin(), (unit() - 0.5) * 0.1, speed * a.cos()];
        f.speed = speed;
        f.gear = gear;
        if unit() < 0.0008 {
            f.damage_zone_level[(unit() * 5.0) as usize] += unit() * 10.0;
        }
        for (k, t) in f.tyres.iter_mut().enumerate() {
            let dirt = if unit() < 0.002 { unit() * 0.01 } else if unit() < 0.01 { 0.0 } else { t.surface.map_or(0.0, |s| s.1) };
            t.surface = if n % 5000 < 30 && k == 3 { None } else { Some((if unit() < 0.01 { 0.85 } else { 1.0 }, dirt)) };
            t.load = if unit() < 0.02 { 5.0 } else { 1000.0 + unit() * 3000.0 };
            t.slip_ratio = (unit() - 0.3) * 1.6 * angle;
            t.angular_velocity = (unit() - 0.1) * 120.0;
        }
        let dt = if n % 7 == 0 { 0.003 } else { 0.0030003 };
        // the game's car
        LOCAL_VELOCITY.with(|v| v.set(f.local_velocity));
        wr(car, CAR_SPEED, f.speed);
        wr(car, CAR_GEAR, f.gear);
        for i in 0..5 {
            wr(car, CAR_DAMAGE + 4 * i, f.damage_zone_level[i]);
        }
        for (k, t) in f.tyres.iter().enumerate() {
            let tyre = car.add(CAR_TYRES + 0x858 * k);
            match t.surface {
                Some((grip, dirt)) => {
                    wr(surfaces[k], 0x90, grip);
                    wr(surfaces[k], 0x98, dirt);
                    wr(tyre, 0x410, surfaces[k]);
                }
                None => wr(tyre, 0x410, 0usize),
            }
            wr(tyre, 0x2e8 + 4, t.load);
            wr(tyre, 0x2e8 + 0x10, t.slip_ratio);
            wr(tyre, 0x2e8 + 0x14, t.angular_velocity);
        }
        step(game, dt);
        port.step(dt, &f);
        let theirs = DriftModeComponent {
            drift_straight_timer: rd(game, 0x8),
            points: rd(game, 0x10),
            instant_drift: rd(game, 0x18),
            current_speed_multiplier: rd(game, 0x20),
            current_drift_angle: rd(game, 0x28),
            combo_counter: rd(game, 0x30),
            last_drift_direction: rd(game, 0x34),
            extreme_drifting: rd::<u8>(game, 0x38) != 0,
            drifting: rd::<u8>(game, 0x39) != 0,
            invalid: rd::<u8>(game, 0x3a) != 0,
            old_damage_zones: [rd(game, 0x3c), rd(game, 0x40), rd(game, 0x44), rd(game, 0x48), rd(game, 0x4c)],
        };
        let bits = |c: &DriftModeComponent| format!("{:08x} {:016x} {:016x} {:016x} {:016x} {} {} {} {} {} {:?}", c.drift_straight_timer.to_bits(), c.points.to_bits(), c.instant_drift.to_bits(), c.current_speed_multiplier.to_bits(), c.current_drift_angle.to_bits(), c.combo_counter, c.last_drift_direction, c.extreme_drifting, c.drifting, c.invalid, c.old_damage_zones.map(f32::to_bits));
        if bits(&theirs) != bits(&port) {
            return Err(format!("drift step {n}: the game {} and the port {} (input {f:?})", bits(&theirs), bits(&port)));
        }
        report.drift_steps += 1;
        report.drifts += (port.drifting && !was_drifting) as usize;
        report.drift_combos += (port.combo_counter > combo && combo > 0) as usize;
        (was_drifting, combo) = (port.drifting, port.combo_counter);
        report.drift_points = port.points;
    }
    Ok(())
}

const VA_TA_VTABLE: usize = 0x1_404d_c990;
const VA_TA_UPDATE: usize = 0x1_401b_e0b0; // void TimeAttack::update(float)
const VA_TA_TARGET_RESET: usize = 0x1_401b_ddc0; // TimeAttackTarget::reset(const vector<float>&)
const VA_TA_STATUS_RESET: usize = 0x1_401b_dcd0; // TimeAttackGameStatus::reset(int)
const VA_TA_CHECKPOINT_BREAK: usize = 0x1_401b_c4c0; // TimeAttack::renderGuiCheckpointBreak(float, float, float)
const VA_IS_IN_REPLAY: usize = 0x1_4015_7670;
const VA_SET_GENTLE_STOP: usize = 0x1_400d_a280;
const VA_GET_CURRENT_LAP: usize = 0x1_4013_80b0;
const VA_GET_LAP_COUNT: usize = 0x1_4013_8570;
const VA_GET_MAX_SPEED_FROM_GEAR: usize = 0x1_4027_c240;
const VA_CONSOLE_SINGLETON: usize = 0x1_4018_c820;
const VA_CONSOLE_SHIFTS: [usize; 3] = [0x1_4018_94b0, 0x1_4018_90e0, 0x1_4018_8f40];
const VA_REGISTER_TO_FADE_IN: usize = 0x1_4003_3ce0;
const VA_FADE_IN: usize = 0x1_4003_3690;
const TA_SIZE: usize = 0x2e0;
const TA_CAR: usize = 0x240;
const TA_TARGET: usize = 0x248;
const TA_INCREMENTS: usize = 0x260;
const TA_STATUS: usize = 0x278;
const TA_SCORE_LABEL_TIMER: usize = 0x2c4;
const TA_PENALTY_POINTS: usize = 0x2c8;
const CA_SIM: usize = 0x130;
const CA_POSITION: usize = 0x254;
const CA_SPEED: usize = 0x4d8;
const CA_DAMAGE: usize = 0x9c4;
const CA_VALID_TRACK: [usize; 4] = [0x630, 0x6f8, 0x7c0, 0x888];
const CA_RACE_ENGINEER: usize = 0xf80;
const SIM_PAUSE_MENU: usize = 0x110;
const SIM_ESC_MENU: usize = 0x118;
const SIM_TRACK: usize = 0x150;
const SIM_CAMERA_MANAGER: usize = 0x188;
const SIM_SYSTEM_MESSAGE: usize = 0x198;
const SIM_RACE_MANAGER: usize = 0x1a8;
const SIM_REPLAY_MANAGER: usize = 0x1b0;
const TRACK_SECTORS: usize = 0xf8 + 0x130;
const FIRST_GEAR_MAX_SPEED: f32 = 14.7;

/// What the stand-ins hand the game, and what they were told.
#[derive(Default)]
struct TaWorld {
    replay: bool,
    splits: usize,
    lap_count: u32,
    gentle_stop: Option<bool>,
    fade: bool,
    checkpoint: Option<(f32, f32)>,
    operator_new: usize,
}

thread_local! {
    static TA: RefCell<TaWorld> = RefCell::new(TaWorld::default());
}

extern "C" fn is_in_replay(_replay: *mut u8) -> bool {
    TA.with(|t| t.borrow().replay)
}
extern "C" fn set_gentle_stop(_car: *mut u8, on: bool) {
    TA.with(|t| t.borrow_mut().gentle_stop = Some(on));
}
extern "C" fn get_current_lap(_manager: *mut u8, out: *mut u8, _car: *mut u8) -> *mut u8 {
    let (splits, new) = TA.with(|t| (t.borrow().splits, t.borrow().operator_new));
    unsafe {
        std::ptr::write_bytes(out, 0, 0x48);
        if splits > 0 {
            // (the game frees the vector: its own allocator)
            let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(new);
            let buffer = new(splits * 4);
            std::ptr::write_bytes(buffer, 0, splits * 4);
            wr(out, 8, buffer);
            wr(out, 0x10, buffer.add(splits * 4));
            wr(out, 0x18, buffer.add(splits * 4));
        }
        wr(out, 0x40, 7usize);
    }
    out
}
extern "C" fn get_lap_count(_manager: *mut u8, _car: *mut u8) -> u32 {
    TA.with(|t| t.borrow().lap_count)
}
extern "C" fn get_max_speed_from_gear(_engineer: *mut u8, out: *mut f32, _gear: i32) -> *mut f32 {
    unsafe { *out = FIRST_GEAR_MAX_SPEED };
    out
}
extern "C" fn checkpoint_break(_ta: *mut u8, _dt: f32, gained_points: f32, gained_seconds: f32) -> bool {
    TA.with(|t| t.borrow_mut().checkpoint = Some((gained_points, gained_seconds)));
    false
}
extern "C" fn fade_in(_cameras: *mut u8, _fade: *const u8) {
    TA.with(|t| t.borrow_mut().fade = true);
}
extern "C" fn first_argument(this: *mut u8) -> *mut u8 {
    this
}

type TaState = (Vec<f32>, Vec<f32>, [u32; 7], [f32; 3], Vec<f32>, f32);

/// The game's `TimeAttack` state in the port's shape.
unsafe fn ta_game_side(ta: *const u8) -> TaState {
    let floats = |at: usize| {
        let (begin, end): (usize, usize) = (rd(ta, at), rd(ta, at + 8));
        (begin..end).step_by(4).map(|p| *(p as *const f32)).collect::<Vec<f32>>()
    };
    let st = TA_STATUS;
    let scalars = [4, 8, 0xc, 0x10, 0x14, 0x18, 0x40].map(|at| rd::<u32>(ta, st + at));
    (floats(TA_TARGET), floats(TA_INCREMENTS), scalars, [rd(ta, st + 0x1c), rd(ta, st + 0x20), rd(ta, st + 0x24)], floats(st + 0x28), rd(ta, TA_PENALTY_POINTS))
}

fn ta_port_side(ta: &TimeAttack) -> TaState {
    let scalars = [ta.sector_index as u32, ta.time_left.to_bits(), ta.points.to_bits(), ta.car_damage.to_bits(), ta.game_over_time.to_bits(), ta.slow_penalty_timer.to_bits(), ta.mode as u32];
    (ta.sectors.clone(), ta.sector_increments.clone(), scalars, ta.reset_position, ta.sector_times.clone(), ta.penalty_points_accumulated)
}

/// Sessions of made-up frames: the game's `TimeAttack::update` on a hand-built block and car
/// (its budget from the same estimate, through its own resets) against the port's. Runs
/// with the install as the working folder (the goals' constructor reads the events there).
unsafe fn time_attack(acs: &Acs, base: &GameMode, next: &mut dyn FnMut() -> u32, frames: usize, report: &mut Report) -> Result<(), String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    stub(VA_IS_IN_REPLAY, is_in_replay as *const () as usize);
    stub(VA_SET_GENTLE_STOP, set_gentle_stop as *const () as usize);
    stub(VA_GET_CURRENT_LAP, get_current_lap as *const () as usize);
    stub(VA_GET_LAP_COUNT, get_lap_count as *const () as usize);
    stub(VA_GET_MAX_SPEED_FROM_GEAR, get_max_speed_from_gear as *const () as usize);
    stub(VA_TA_CHECKPOINT_BREAK, checkpoint_break as *const () as usize);
    stub(VA_FADE_IN, fade_in as *const () as usize);
    stub(VA_REGISTER_TO_FADE_IN, first_argument as *const () as usize);
    stub(VA_CONSOLE_SINGLETON, first_argument as *const () as usize);
    for va in VA_CONSOLE_SHIFTS {
        stub(va, first_argument as *const () as usize);
    }
    TA.with(|t| t.borrow_mut().operator_new = acs.crt_function(c"??2@YAPEAX_K@Z"));
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GAME_MODE_CTOR));
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_TA_UPDATE));
    let target_reset: extern "C" fn(*mut u8, *const [usize; 3]) = std::mem::transmute(acs.va(VA_TA_TARGET_RESET));
    let status_reset: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_TA_STATUS_RESET));
    for session in 0..24 {
        let sectors = 1 + next() as usize % 4;
        let time_mult = [1.0f32, 0.5, 0.25, 0.0, 1.5][session % 5];
        // the mode with its goals, then TimeAttack's members
        let ta = zeroed(TA_SIZE);
        let name = zeroed(0x20);
        write_wstring(name, "TIME_ATTACK");
        ctor(ta, name, zeroed(0x278));
        wr(ta, 0, acs.va(VA_TA_VTABLE));
        let (sim, car, track, pause, esc) = (zeroed(0x2d0), zeroed(0x1300), zeroed(TRACK_SECTORS + 0x18), zeroed(0x160), zeroed(0x160));
        let positions = Box::leak(vec![0.0f32; sectors].into_boxed_slice()).as_ptr();
        wr(track, TRACK_SECTORS, positions);
        wr(track, TRACK_SECTORS + 8, positions.add(sectors));
        wr(track, TRACK_SECTORS + 0x10, positions.add(sectors));
        for (at, block) in [(SIM_TRACK, track), (SIM_PAUSE_MENU, pause), (SIM_ESC_MENU, esc), (SIM_CAMERA_MANAGER, zeroed(0x200)), (SIM_RACE_MANAGER, zeroed(0x300)), (SIM_REPLAY_MANAGER, zeroed(0x200)), (SIM_SYSTEM_MESSAGE, zeroed(0x100))] {
            wr(sim, at, block);
        }
        wr(car, CA_SIM, sim);
        wr(car, CA_RACE_ENGINEER, zeroed(0x100));
        wr(ta, TA_CAR, car);
        // the budget (the constructor's and a new session's sums) through the game's resets
        let estimate: Vec<f32> = (0..sectors).map(|_| (next() % 4000) as f32 * 0.01 + 3.0).collect();
        let budget: Vec<f32> = estimate.iter().map(|v| (v + 10.0) * time_mult).collect();
        let end = budget.as_ptr().add(budget.len()) as usize;
        target_reset(ta.add(TA_TARGET), &[budget.as_ptr() as usize, end, end]);
        wr(ta, TA_STATUS + 0x1c, 7.0f32); // a resetPosition of the session before: the reset keeps it
        status_reset(ta.add(TA_STATUS), sectors as i32);
        wr(ta, TA_STATUS + 0x18, 10.0f32);
        wr(ta, TA_SCORE_LABEL_TIMER, 1000.0f32);
        let mut port = TimeAttack::new(base.clone(), time_mult);
        port.reset_position[0] = 7.0;
        port.new_session(&estimate, sectors);

        let mut f = TaFrame { track_sectors: sectors, first_gear_max_speed: FIRST_GEAR_MAX_SPEED, esc_open: session % 3 == 0, ..TaFrame::default() };
        let mut speed_phase = 0usize;
        for frame in 0..frames {
            let dt = (5 + next() % 30) as f32 * 0.001;
            // the made-up drive
            if f.esc_open && next() % 50 == 0 {
                f.esc_open = false;
            }
            f.paused = next() % 200 == 0;
            f.replay = next() % 300 == 0;
            if next() % 60 == 0 {
                speed_phase = next() as usize % 4;
            }
            f.speed = [3.0, 9.0, 25.0, 60.0][speed_phase] + (next() % 100) as f32 * 0.01;
            if !f.esc_open {
                f.position[2] += f.speed * dt;
                f.position[0] += (next() % 3) as f32 * 0.01;
            }
            if next() % 400 == 0 {
                f.damage_zone_level[next() as usize % 5] += (next() % 50) as f32 * 0.1;
            }
            f.tyres_off = if next() % 20 == 0 { next() as usize % 5 } else { 0 };
            if next() % 250 == 0 {
                f.splits += 1;
                if f.splits >= sectors {
                    f.splits = 0;
                    f.lap_count += 1;
                }
            }
            // the game
            *esc.add(0x150) = f.esc_open as u8;
            *pause.add(0x150) = f.paused as u8;
            wr(car, CA_POSITION, f.position);
            wr(car, CA_SPEED, f.speed);
            wr(car, CA_DAMAGE, f.damage_zone_level);
            for (k, at) in CA_VALID_TRACK.iter().enumerate() {
                *car.add(*at) = (k >= f.tyres_off) as u8;
            }
            TA.with(|t| {
                let mut t = t.borrow_mut();
                (t.replay, t.splits, t.lap_count, t.gentle_stop, t.fade, t.checkpoint) = (f.replay, f.splits, f.lap_count, None, false, None);
            });
            SAID.with(|s| s.borrow_mut().clear());
            update(ta, dt);
            let game_said = SAID.with(|s| std::mem::take(&mut *s.borrow_mut()));
            let (gentle, fade, checkpoint) = TA.with(|t| (t.borrow().gentle_stop, t.borrow().fade, t.borrow().checkpoint));
            // the port (a checkpoint moves the sector on)
            let mut port_said = Vec::new();
            let sector_before = port.sector_index;
            let out = port.update(dt, &f, &mut |a, b| port_said.push(format!("Message({a} | {b})")));
            let port_checkpoint = (port.sector_index != sector_before).then_some((port.last_gained_points, port.last_gained_seconds));
            let (game_state, port_state) = (ta_game_side(ta), ta_port_side(&port));
            let (goals, tier) = { let g = game_side(ta); (g.1, g.2) };
            if game_state != port_state || game_said != port_said || gentle != out.gentle_stop || fade != out.fade_to_results || checkpoint != port_checkpoint || (&goals, tier) != (&port.base.conditions, port.base.current_tier) {
                return Err(format!(
                    "time attack session {session} frame {frame} dt {dt} {f:?}: the game {game_state:?} said {game_said:?} gentle {gentle:?} fade {fade} checkpoint {checkpoint:?} tier {tier}, the port {port_state:?} said {port_said:?} gentle {:?} fade {} checkpoint {port_checkpoint:?} tier {}",
                    out.gentle_stop, out.fade_to_results, port.base.current_tier
                ));
            }
            report.ta_frames += 1;
            report.ta_messages += game_said.len();
            report.ta_checkpoints += checkpoint.is_some() as usize;
            if port.mode == TaMode::Over && port.game_over_time > 1.0 {
                break;
            }
        }
        report.ta_sessions += 1;
    }
    Ok(())
}
