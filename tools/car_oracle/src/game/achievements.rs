// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47d: the game's `SteamInterface` (constructor 0x1401a5d70, `update` 0x1401accd0, the
//! destructor 0x1401a6ec0) on scripted drives, every Steam call it makes answered and written
//! down by the stand-in `steam_api64.dll` ([`super::steam_standin`]), with its own `printf`
//! lines, system messages, the avatar texture and the Friends Leaderboard rows in the same log.
//!
//! The tape interleaves the inputs (`> ` lines) with what each input made the game do; the
//! port's check (`crates/rustyac-game/examples/achievements_check.rs`) replays the inputs and
//! must write the same tape.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::race_ini::{rd, text, wr, write_wstring, zeroed};
use super::steam_standin::{self as standin, log, Answers};
use crate::acs::Acs;

const VA_CTOR: usize = 0x1_401a_5d70;
const VA_DTOR: usize = 0x1_401a_6ec0;
const VA_UPDATE: usize = 0x1_401a_ccd0;
const VA_REFRESH: usize = 0x1_401a_c7c0;
const VA_RESET: usize = 0x1_401a_ca60;
const VA_RESET_ACHIEVEMENTS: usize = 0x1_401a_c850;
const VA_ADD_MESSAGE: usize = 0x1_401b_61c0;
const VA_CONSOLE_SINGLETON: usize = 0x1_4018_c820;
const VA_CONSOLE_ADD_COMMAND: usize = 0x1_4018_a530;
const VA_KGL_CREATE_TEXTURE: usize = 0x1_4001_91a0;
const VA_KGL_DESTROY_TEXTURE: usize = 0x1_4001_9260;
const VA_GET_DRIFT_MODE: usize = 0x1_4010_a590;
const VA_GET_LAST_LAP: usize = 0x1_4013_87c0;
/// `__RTDynamicCast` of the sim's game objects to `GameMode` (the constructor's and
/// `addAchievements`' loops): the oracle's sim holds only the game mode, so `mov rax, rcx`.
const VA_CASTS: [usize; 2] = [0x1_401a_6038, 0x1_401a_9bca];
/// The `CarTrackComb` list the constructor appends its four combinations to (a global vector).
const VA_COMBS_BEGIN: usize = 0x1_4155_9c28;
const VA_COMBS_END: usize = 0x1_4155_9c30;
const VA_GAME_MODE_CTOR: usize = 0x1_400b_c810;
const VA_HOTLAP_LAP: usize = 0x1_400b_eb80;
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588;
const VA_INIREADERDOCUMENTS_BASE_PATH: usize = 0x1_4151_d100;

static MALLOC: AtomicUsize = AtomicUsize::new(0);
static DRIFT_MODE: AtomicUsize = AtomicUsize::new(0);
static LAP: Mutex<(u32, bool, i32, usize)> = Mutex::new((0, false, 0, 0));

extern "C" fn printf(format: *const u8, arg: *const u16) -> i32 {
    // SAFETY: the game's format string, and its one wide-string argument when it has %S
    let format = unsafe { std::ffi::CStr::from_ptr(format.cast()) }.to_string_lossy().into_owned();
    if format.to_lowercase().contains("steam") || format.starts_with("ERROR: NO STAT FOR") {
        let line = if format.contains("%S") {
            let mut n = 0;
            // SAFETY: a NUL-terminated wide string
            unsafe {
                while *arg.add(n) != 0 {
                    n += 1;
                }
                format.replace("%S", &String::from_utf16_lossy(std::slice::from_raw_parts(arg, n)))
            }
        } else {
            format
        };
        log(format!("printf {}", line.trim_end_matches('\n')));
    }
    0
}

extern "C" fn add_message(_messages: *mut u8, message: *const u8, description: *const u8, kind: i32) {
    log(unsafe { format!("Message({} | {} | {kind})", text(message), text(description)) });
}

extern "C" fn console_singleton() -> usize {
    static CONSOLE: [u64; 64] = [0; 64];
    CONSOLE.as_ptr() as usize
}

extern "C" fn console_add_command(_console: usize, _name: usize, _f: usize) {}

extern "C" fn create_texture(buffer: *const u8, format: i32, width: i32, height: i32) -> u64 {
    // SAFETY: the game's RGBA buffer of width × height pixels
    let pixels = unsafe { std::slice::from_raw_parts(buffer, (width * height * 4) as usize) };
    log(format!("Texture {width}x{height} format {format} {:08x}", standin::image_hash(pixels)));
    0x51
}

extern "C" fn destroy_texture(id: u64) {
    log(format!("DestroyTexture {id:#x}"));
}

extern "C" fn get_drift_mode(_object: usize) -> usize {
    DRIFT_MODE.load(Ordering::Relaxed)
}

/// `RaceManager::getLastLap(Lap* out, CarAvatar*)`: the frame's lap (its splits from the
/// game's heap: the caller frees them).
extern "C" fn get_last_lap(_rm: usize, out: *mut u8, _car: usize) -> *mut u8 {
    let (time, valid, cuts, splits) = *LAP.lock().unwrap();
    // SAFETY: the caller's Lap (0x48 bytes)
    unsafe {
        out.write_bytes(0, 0x48);
        wr(out, 0, time);
        if splits > 0 {
            let malloc: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(MALLOC.load(Ordering::Relaxed));
            let begin = malloc(splits * 4);
            begin.write_bytes(0, splits * 4);
            wr(out, 8, begin);
            wr(out, 0x10, begin.add(splits * 4));
            wr(out, 0x18, begin.add(splits * 4));
        }
        wr(out, 0x20, cuts);
        wr(out, 0x24, valid as u8);
        wr(out, 0x40, 7usize);
    }
    out
}

/// The Friends Leaderboard's listener on `evOnLeaderboardUpdated`: every row it is given.
extern "C" fn rows(_impl: usize, list: *const u8) {
    // SAFETY: a vector<SteamLeaderboardEntry> (0x30 bytes each)
    unsafe {
        let (mut at, end): (*const u8, *const u8) = (rd(list, 0), rd(list, 8));
        log(format!("rows {}", (end as usize - at as usize) / 0x30));
        while at < end {
            log(format!("row {} | {} | {} | {}", text(at), rd::<i32>(at, 0x20), rd::<i32>(at, 0x24), rd::<u8>(at, 0x28)));
            at = at.add(0x30);
        }
    }
}

extern "C" fn nothing(_a: usize, _b: usize) -> usize {
    0
}

/// One frame of the player's car, as the achievement checks read it.
#[derive(Clone, Debug)]
pub struct Frame {
    pub speed: f32,
    pub pos: [f32; 3],
    pub laps: u32,
    pub combo: i32,
    pub damage: [f32; 5],
    pub abs: bool,
    pub tc: bool,
    pub shifter: bool,
    pub blip: bool,
    /// the drift mode's level; None: no drift mode found
    pub level: Option<i32>,
    /// `ReplayManager::isInReplaymode` and the byte `updateStats` reads
    pub replay: bool,
    pub replay_stats: bool,
    /// the last lap: time, valid, cuts, splits
    pub lap: (u32, bool, i32, usize),
}

impl Default for Frame {
    fn default() -> Self {
        Frame { speed: 0.0, pos: [0.0; 3], laps: 0, combo: 0, damage: [0.0; 5], abs: true, tc: true, shifter: true, blip: true, level: None, replay: false, replay_stats: false, lap: (0, false, 0, 0) }
    }
}

impl Frame {
    pub fn line(&self) -> String {
        let b = |v: bool| v as u8;
        let d = &self.damage;
        format!(
            "> frame speed {} pos {} {} {} laps {} combo {} damage {} {} {} {} {} abs {} tc {} shifter {} blip {} level {} replay {} {} lap {} {} {} {}",
            self.speed, self.pos[0], self.pos[1], self.pos[2], self.laps, self.combo, d[0], d[1], d[2], d[3], d[4],
            b(self.abs), b(self.tc), b(self.shifter), b(self.blip),
            self.level.map_or("-".to_string(), |l| l.to_string()), b(self.replay), b(self.replay_stats),
            self.lap.0, b(self.lap.1), self.lap.2, self.lap.3
        )
    }
}

pub enum Input {
    Frame(Frame),
    Hotlap(u32, bool, i32),
    Score(f32),
    Refresh,
    Reset,
    ResetAchievements,
}

pub struct Case {
    pub session: i32,
    pub penalties: bool,
    pub official: bool,
    pub car: &'static str,
    pub track: &'static str,
    pub config: &'static str,
    pub sectors: usize,
    pub answers: Answers,
    /// cfg/race.ini's text after [RACE]'s three keys; cfg/controls.ini's (None: no file)
    pub race_extra: String,
    pub controls: Option<&'static str>,
    pub inputs: Vec<Input>,
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

/// The scripted drives. `guid`: an official special event's GUID.
pub fn cases(seed: u32, guid: i32) -> Vec<Case> {
    let answers = |n: u32| Answers { seed: seed.wrapping_add(n), steam_id: 76_561_198_000_000_000 + u64::from(n), stats_ok: true, set_stat_ok: true, store_ok: true, found: true, upload_ok: true, changed: true, entries: 8, avatar: 7, image_ok: true };
    let f = Frame::default;
    let drive = |n: usize, step: f32| -> Vec<Input> { (0..n).map(|i| Input::Frame(Frame { speed: 20.0 + i as f32 * 9.0, pos: [i as f32 * step, 1.5, -(i as f32) * step * 0.5], laps: i as u32, ..f() })).collect() };
    let se = "[SPECIAL_EVENT]\nGUID=999999\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=92000\n[CONDITION_1]\nTYPE=TIME\nOBJECTIVE=90000\n[CONDITION_2]\nTYPE=TIME\nOBJECTIVE=88000\n";
    let mut out = Vec::new();

    // 1. practice: speeds, distance, both times to beat, ten laps without aids
    let mut inputs = drive(4, 30_000.0);
    inputs.push(Input::Frame(Frame { speed: 84.0, pos: [200_000.0, 0.0, 0.0], laps: 4, lap: (117_000, true, 0, 3), ..f() }));
    inputs.push(Input::Frame(Frame { speed: 70.0, pos: [200_000.0, 0.0, 0.0], laps: 5, lap: (116_000, true, 0, 2), ..f() }));
    inputs.push(Input::Frame(Frame { speed: 70.0, pos: [200_010.0, 0.0, 0.0], laps: 6, lap: (115_000, true, 0, 3), ..f() }));
    inputs.push(Input::Frame(Frame { speed: 70.0, pos: [200_020.0, 0.0, 0.0], laps: 10, lap: (113_500, true, 0, 3), ..f() }));
    inputs.push(Input::Frame(Frame { speed: 70.0, pos: [200_020.0, 0.0, 0.0], laps: 10, abs: false, tc: false, shifter: false, blip: false, lap: (113_500, false, 0, 3), ..f() }));
    inputs.push(Input::ResetAchievements);
    out.push(Case { session: 1, penalties: true, official: false, car: "ferrari_458", track: "imola", config: "", sectors: 3, answers: answers(1), race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=X360\n"), inputs });

    // 2. penalties off: no checks, no leaderboard
    let mut inputs = drive(4, 90_000.0);
    inputs.push(Input::Frame(Frame { speed: 99.0, laps: 12, damage: [300.0, 0.0, 0.0, 0.0, 0.0], ..f() }));
    out.push(Case { session: 1, penalties: false, official: false, car: "ks_mazda_mx5_cup_step2", track: "magione", config: "", sectors: 2, answers: answers(2), race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=KEYBOARD\n"), inputs });

    // 3. hotlap on a mod's event (not official): the board, its rows, laps and their scores
    let mut inputs = drive(3, 10.0);
    inputs.push(Input::Hotlap(91_000, true, 0));
    inputs.push(Input::Frame(f()));
    inputs.push(Input::Frame(f()));
    inputs.push(Input::Hotlap(89_000, true, 0));
    inputs.push(Input::Hotlap(87_000, false, 0));
    inputs.push(Input::Hotlap(87_000, true, 1));
    inputs.push(Input::Frame(Frame { laps: 10, lap: (89_000, true, 0, 2), ..f() }));
    inputs.push(Input::Frame(Frame { laps: 11, lap: (88_000, true, 0, 2), ..f() }));
    inputs.push(Input::Refresh);
    inputs.push(Input::Frame(f()));
    inputs.push(Input::Reset);
    inputs.push(Input::Frame(f()));
    inputs.push(Input::Frame(f()));
    out.push(Case { session: 4, penalties: true, official: false, car: "lotus_49", track: "ks_monza66", config: "road", sectors: 2, answers: answers(3), race_extra: se.into(), controls: Some("[HEADER]\nINPUT_METHOD=WHEEL\n"), inputs });

    // 4. hotlap, an official event, the tiers taken; the upload fails
    let mut inputs = drive(2, 10.0);
    inputs.push(Input::Frame(f()));
    for time in [95_000, 93_500, 92_500, 80_000, 60_000] {
        inputs.push(Input::Hotlap(time, true, 0));
        inputs.push(Input::Frame(Frame { laps: 3, lap: (time, true, 0, 3), ..f() }));
    }
    inputs.push(Input::Frame(f()));
    inputs.push(Input::ResetAchievements);
    out.push(Case { session: 4, penalties: true, official: true, car: "tatuusfa1", track: "ks_vallelunga", config: "extended_circuit", sectors: 3, answers: Answers { upload_ok: false, ..answers(4) }, race_extra: format!("[SPECIAL_EVENT]\nGUID={guid}\n"), controls: None, inputs });

    // 5. time attack, no board found: laps and a score that goes nowhere
    let mut inputs = drive(2, 10.0);
    for laps in [3, 5, 10] {
        inputs.push(Input::Frame(Frame { laps, lap: (102_500, true, 0, 3), ..f() }));
    }
    inputs.push(Input::Score(1234.5));
    inputs.push(Input::Frame(f()));
    out.push(Case { session: 5, penalties: true, official: false, car: "bmw_m3_gt2", track: "imola", config: "", sectors: 3, answers: Answers { found: false, ..answers(5) }, race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=X360\n"), inputs });

    // 6. drift: combos, no drift mode at first, then its levels; a score
    let mut inputs = drive(2, 10.0);
    for (combo, level) in [(10, None), (25, Some(9)), (24, Some(24)), (50, Some(49)), (3, Some(99))] {
        inputs.push(Input::Frame(Frame { combo, level, ..f() }));
    }
    inputs.push(Input::Score(3456.7));
    inputs.push(Input::Frame(f()));
    inputs.push(Input::Frame(f()));
    out.push(Case { session: 6, penalties: true, official: false, car: "ks_toyota_ae86_drift", track: "drift", config: "", sectors: 1, answers: answers(6), race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=WHEEL\n"), inputs });

    // 7. race: a crash
    let mut inputs = drive(2, 10.0);
    inputs.push(Input::Frame(Frame { laps: 6, damage: [10.0, 0.0, 0.0, 0.0, 0.0], ..f() }));
    inputs.push(Input::Frame(Frame { laps: 6, damage: [90.0, 50.0, 40.0, 30.0, 0.5], ..f() }));
    out.push(Case { session: 3, penalties: true, official: false, car: "ks_bmw_m235i_racing_S2", track: "spa", config: "", sectors: 3, answers: answers(7), race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=X360\n"), inputs });

    // 8. drag: a clean drive that is not one, and Steam failing everywhere
    let mut inputs = drive(2, 10.0);
    inputs.push(Input::Frame(Frame { laps: 6, ..f() }));
    inputs.push(Input::Frame(f()));
    let failing = Answers { stats_ok: false, set_stat_ok: false, store_ok: false, image_ok: false, avatar: 5, ..answers(8) };
    out.push(Case { session: 7, penalties: true, official: false, car: "abarth500", track: "drag", config: "", sectors: 1, answers: failing, race_extra: String::new(), controls: Some("[HEADER]\nINPUT_METHOD=WHEEL\n"), inputs });

    // 9. a replay: no checks, the replay stat, no avatar; stats that set but do not store
    let mut inputs = vec![Input::Frame(Frame { replay: true, replay_stats: true, speed: 90.0, ..f() })];
    inputs.push(Input::Frame(Frame { replay: true, replay_stats: true, speed: 90.0, laps: 10, ..f() }));
    inputs.push(Input::Frame(Frame { replay: true, replay_stats: true, speed: 90.0, ..f() }));
    let replay = Answers { avatar: 0, set_stat_ok: true, store_ok: false, ..answers(9) };
    out.push(Case { session: 1, penalties: true, official: false, car: "ferrari_458", track: "imola", config: "", sectors: 3, answers: replay, race_extra: String::new(), controls: None, inputs });
    out
}

/// The header lines of a case, as the tape writes them.
pub fn header(n: usize, case: &Case) -> Vec<String> {
    let a = &case.answers;
    let b = |v: bool| v as u8;
    let race = format!("[RACE]\nMODEL={}\nTRACK={}\nCONFIG_TRACK={}\n{}", case.car, case.track, case.config, case.race_extra);
    vec![
        format!(
            "> case {n} type {} penalties {} official {} car {} track {} config {} sectors {}",
            case.session, b(case.penalties), b(case.official), case.car, case.track, if case.config.is_empty() { "-" } else { case.config }, case.sectors
        ),
        format!(
            "> answers seed {} id {} stats {} set {} store {} found {} upload {} changed {} entries {} avatar {} image {}",
            a.seed, a.steam_id, b(a.stats_ok), b(a.set_stat_ok), b(a.store_ok), b(a.found), b(a.upload_ok), b(a.changed), a.entries, a.avatar, b(a.image_ok)
        ),
        format!("> file cfg/race.ini {}", escape(&race)),
        match case.controls {
            Some(text) => format!("> file cfg/controls.ini {}", escape(text)),
            None => "> nofile cfg/controls.ini".into(),
        },
        "> new".into(),
    ]
}

/// Fires an `Event<T>`'s handlers (a vector of {id, std::function}, 0x28 bytes each).
unsafe fn fire(event: *mut u8, arg: *mut u8) {
    let (mut at, end): (*mut u8, *mut u8) = (rd(event, 0), rd(event, 8));
    while at < end {
        let f: *mut u8 = rd(at, 0x20);
        let vtable: *const usize = rd(f, 0);
        let call: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(2));
        call(f, arg);
        at = at.add(0x28);
    }
}

/// Runs every case; `root`: the oracle's scratch folder (its cfg/ is the game's Documents
/// cfg), `ac`: the install (the game's working folder while a game mode reads its event).
pub unsafe fn micro(acs: &Acs, root: &Path, ac: &Path, seed: u32) -> Result<String, String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    stub(VA_CONSOLE_SINGLETON, console_singleton as *const () as usize);
    stub(VA_CONSOLE_ADD_COMMAND, console_add_command as *const () as usize);
    stub(VA_KGL_CREATE_TEXTURE, create_texture as *const () as usize);
    stub(VA_KGL_DESTROY_TEXTURE, destroy_texture as *const () as usize);
    stub(VA_GET_DRIFT_MODE, get_drift_mode as *const () as usize);
    stub(VA_GET_LAST_LAP, get_last_lap as *const () as usize);
    for va in VA_CASTS {
        acs.patch(acs.va(va), &[0x48, 0x8b, 0xc1, 0x90, 0x90]);
    }
    MALLOC.store(acs.crt_function(c"malloc"), Ordering::Relaxed);
    acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);
    write_wstring(acs.va(VA_INIREADERDOCUMENTS_BASE_PATH) as *mut u8, &format!("{}/", root.to_string_lossy().replace('\\', "/")));
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;

    let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CTOR));
    let dtor: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_DTOR));
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_UPDATE));
    let refresh: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_REFRESH));
    let reset: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_RESET));
    let reset_achievements: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_RESET_ACHIEVEMENTS));
    let mode_ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GAME_MODE_CTOR));
    let lap_handler: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_HOTLAP_LAP));
    let here = std::env::current_dir().map_err(|e| e.to_string())?;

    let se = ac.join("system/data/se");
    let guid = std::fs::read_dir(&se)
        .map_err(|e| format!("{}: {e}", se.display()))?
        .filter_map(|e| e.ok()?.path().file_stem()?.to_str()?.strip_prefix("SPECIAL_EVENT_")?.parse::<i32>().ok())
        .min()
        .ok_or("no official special event")?;

    let mut tape = String::new();
    let flush = |tape: &mut String| {
        for line in standin::take_calls() {
            writeln!(tape, "{line}").unwrap();
        }
    };
    for (n, case) in cases(seed, guid).into_iter().enumerate() {
        standin::reset(case.answers);
        let lines = header(n, &case);
        for line in &lines[..4] {
            writeln!(tape, "{line}").unwrap();
        }
        let race = format!("[RACE]\nMODEL={}\nTRACK={}\nCONFIG_TRACK={}\n{}", case.car, case.track, case.config, case.race_extra);
        std::fs::write(root.join("cfg/race.ini"), race).map_err(|e| e.to_string())?;
        let controls = root.join("cfg/controls.ini");
        match case.controls {
            Some(text) => std::fs::write(&controls, text).map_err(|e| e.to_string())?,
            None => {
                let _ = std::fs::remove_file(&controls);
            }
        }

        // the world the interface reads
        let sim = acs.alloc(0x2d0);
        wr(sim, 8, acs.alloc(0x400));
        let track = acs.alloc(0x240);
        write_wstring(track.add(0x10), case.track);
        write_wstring(track.add(0xa0), case.config);
        let sectors = zeroed(case.sectors * 4 + 4);
        wr(track, 0x228, sectors);
        wr(track, 0x230, sectors.add(case.sectors * 4));
        wr(sim, 0x150, track);
        let rm = acs.alloc(0x200);
        wr(rm, 0xa0, case.penalties as u8);
        wr(rm, 0xa1, case.official as u8);
        wr(rm, 0xe8, case.session);
        write_wstring(rm.add(0x100), "");
        write_wstring(rm.add(0x120), "");
        wr(rm, 0x168, zeroed(0x200));
        wr(sim, 0x1a8, rm);
        let replay = acs.alloc(0x200);
        wr(sim, 0x1b0, replay);
        let car = acs.alloc(0x1200);
        write_wstring(car.add(0x138), case.car);
        let phys = acs.alloc(0x3400);
        wr(car, 0x1168, phys);
        let cars = zeroed(8);
        wr(cars, 0, car);
        wr(sim, 0x208, cars);
        wr(sim, 0x210, cars.add(8));
        let drift_mode = acs.alloc(0x300);
        DRIFT_MODE.store(0, Ordering::Relaxed);

        // the session's game mode (Hotlap, Time Attack, Drift), built by the game from race.ini
        let mode = if matches!(case.session, 4..=6) {
            let mode = zeroed(0xd8);
            let name = zeroed(0x20);
            write_wstring(name, "HOTLAP_MODE");
            std::env::set_current_dir(ac).map_err(|e| e.to_string())?;
            mode_ctor(mode, name, zeroed(0x278));
            std::env::set_current_dir(&here).map_err(|e| e.to_string())?;
            wr(mode, 0xd0, sim);
            let objects = zeroed(8);
            wr(objects, 0, mode);
            wr(sim, 0x40, objects);
            wr(sim, 0x48, objects.add(8));
            Some(mode)
        } else {
            None
        };
        acs.set_global(VA_COMBS_END, acs.global::<usize>(VA_COMBS_BEGIN));
        standin::take_calls();

        writeln!(tape, "{}", lines[4]).unwrap();
        let si = ctor(acs.alloc(0x1f8), sim);
        flush(&mut tape);
        // the Friends Leaderboard's listener
        let vtable = Box::leak(vec![nothing as *const () as usize; 8].into_boxed_slice());
        vtable[2] = rows as *const () as usize;
        let listener = acs.alloc(0x40);
        wr(listener, 0, vtable.as_ptr());
        let handlers = acs.alloc(0x28);
        wr(handlers, 0x20, listener);
        wr(si, 0x58, handlers);
        wr(si, 0x60, handlers.add(0x28));
        wr(si, 0x68, handlers.add(0x28));

        for input in &case.inputs {
            match input {
                Input::Frame(frame) => {
                    writeln!(tape, "{}", frame.line()).unwrap();
                    wr(car, 0x4d8, frame.speed);
                    for (i, v) in frame.pos.iter().enumerate() {
                        wr(car, 0x254 + 4 * i, *v);
                    }
                    wr(car, 0x8cc, frame.laps);
                    wr(car, 0x994, frame.combo);
                    for (i, v) in frame.damage.iter().enumerate() {
                        wr(car, 0x9c4 + 4 * i, *v);
                    }
                    for (at, on) in [(0x958, frame.abs), (0x959, frame.abs), (0xa00, frame.tc), (0xa01, frame.tc), (0x33e8, frame.shifter), (0x3340, frame.blip)] {
                        wr(phys, at, on as u8);
                    }
                    match frame.level {
                        Some(level) => {
                            wr(drift_mode, 0x28c, level);
                            DRIFT_MODE.store(drift_mode as usize, Ordering::Relaxed);
                        }
                        None => DRIFT_MODE.store(0, Ordering::Relaxed),
                    }
                    wr(replay, 0x148, frame.replay as u8);
                    wr(replay, 0xb8, frame.replay_stats as u8);
                    *LAP.lock().unwrap() = frame.lap;
                    update(si, 0.016);
                }
                Input::Hotlap(time, valid, cuts) => {
                    writeln!(tape, "> hotlap {time} {} {cuts}", *valid as u8).unwrap();
                    let mode = mode.ok_or("a hotlap lap without a game mode")?;
                    let lambda = zeroed(8);
                    wr(lambda, 0, mode);
                    let block = zeroed(0x40);
                    wr(block, 4, *time);
                    wr(block, 0x30, *valid as u8);
                    wr(block, 0x34, *cuts);
                    lap_handler(lambda, block);
                }
                Input::Score(score) => {
                    writeln!(tape, "> score {score}").unwrap();
                    let mode = mode.ok_or("a score without a game mode")?;
                    let mut value = *score;
                    fire(mode.add(0x70), (&mut value as *mut f32).cast());
                }
                Input::Refresh => {
                    tape.push_str("> refresh\n");
                    refresh(si);
                }
                Input::Reset => {
                    tape.push_str("> reset\n");
                    reset(si);
                }
                Input::ResetAchievements => {
                    tape.push_str("> resetach\n");
                    reset_achievements(si);
                }
            }
            flush(&mut tape);
        }
        tape.push_str("> end\n");
        dtor(si);
        flush(&mut tape);
    }
    Ok(tape)
}

/// The import stand-ins: the Steam DLL's and the game's `printf`.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    let mut list = standin::overrides();
    list.push(("msvcr120.dll", "printf", printf as *const () as usize));
    list
}
