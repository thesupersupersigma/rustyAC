// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SteamInterface` (constructor @ 0x1401a5d70, `update` @ 0x1401accd0): the game's Steam
//! achievements, stats and Friends Leaderboard. Off unless `--steam-achievements on` (or
//! `[STEAM] ACHIEVEMENTS=1` in rustyAC's own cfg): with it off none of this is made and no
//! Steam call of it is made.
//!
//! Every Steam call goes through the flat API ([`Flat`]) and the interfaces' vtables, so a
//! stand-in `steam_api64.dll` sees the same calls, arguments and order as the game makes
//! (`examples/achievements_check.rs` against `car_oracle achievements-micro`).

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

use rustyac_physics::data::ini::IniReader;

use crate::steam::{slot, Interface};

/// The flat API functions `SteamInterface` uses.
#[derive(Clone, Copy)]
pub struct Flat {
    pub run_callbacks: unsafe extern "C" fn(),
    pub register_callback: unsafe extern "C" fn(*mut u8, i32),
    pub unregister_callback: unsafe extern "C" fn(*mut u8),
    pub register_call_result: unsafe extern "C" fn(*mut u8, u64),
    pub unregister_call_result: unsafe extern "C" fn(*mut u8, u64),
    pub user_stats: unsafe extern "C" fn() -> Interface,
    pub user: unsafe extern "C" fn() -> Interface,
    pub friends: unsafe extern "C" fn() -> Interface,
    pub utils: unsafe extern "C" fn() -> Interface,
}

impl Flat {
    /// The functions of the `steam_api64.dll` [`crate::steam::start`] loaded.
    #[allow(clippy::missing_transmute_annotations)] // each field's type is the annotation
    pub fn loaded() -> Option<Flat> {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
            fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
        }
        let wide: Vec<u16> = "steam_api64.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: Win32 lookups; each symbol has the Steamworks signature it is cast to
        unsafe {
            let module = GetModuleHandleW(wide.as_ptr());
            if module.is_null() {
                return None;
            }
            let symbol = |name: &str| -> Option<usize> {
                let c = CString::new(name).ok()?;
                let p = GetProcAddress(module, c.as_ptr().cast());
                (!p.is_null()).then_some(p as usize)
            };
            use std::mem::transmute as t;
            Some(Flat {
                run_callbacks: t::<usize, _>(symbol("SteamAPI_RunCallbacks")?),
                register_callback: t::<usize, _>(symbol("SteamAPI_RegisterCallback")?),
                unregister_callback: t::<usize, _>(symbol("SteamAPI_UnregisterCallback")?),
                register_call_result: t::<usize, _>(symbol("SteamAPI_RegisterCallResult")?),
                unregister_call_result: t::<usize, _>(symbol("SteamAPI_UnregisterCallResult")?),
                user_stats: t::<usize, _>(symbol("SteamUserStats")?),
                user: t::<usize, _>(symbol("SteamUser")?),
                friends: t::<usize, _>(symbol("SteamFriends")?),
                utils: t::<usize, _>(symbol("SteamUtils")?),
            })
        }
    }
}

/// What the game's side gets back: its `printf`, `SystemMessage::addMessage`, the avatar's
/// texture and the Friends Leaderboard's rows (`evOnLeaderboardUpdated`).
pub trait Host {
    fn printf(&mut self, line: &str);
    fn message(&mut self, text: &str, description: &str);
    /// `kglCreateTexture(184, 184, format 1, rgba)`; 0: none
    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> u64;
    fn destroy_texture(&mut self, id: u64);
    fn rows(&mut self, rows: &[Row]);
}

/// `SteamLeaderboardEntry`
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub score: i32,
    pub rank: i32,
    pub is_player: bool,
}

/// The player's lap as `getLastLap` gives it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lap {
    pub time: u32,
    pub valid: bool,
    pub cuts: i32,
    /// its sector splits
    pub splits: usize,
}

/// What `update` reads of the player's car and the session, once a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// m/s
    pub speed: f32,
    pub pos: [f32; 3],
    pub laps: u32,
    pub combo: i32,
    pub damage: [f32; 5],
    pub abs: bool,
    pub tc: bool,
    pub auto_shifter: bool,
    pub auto_blip: bool,
    /// the drift mode's `currentLevel`; None: no drift mode
    pub drift_level: Option<i32>,
    /// `Replay::isInReplaymode`
    pub replay: bool,
    /// `Replay::hasLoadedReplay`
    pub replay_loaded: bool,
    pub lap: Lap,
}

/// The session `SteamInterface` is made in.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub session_type: i32,
    /// `RaceManager` +0xa0: the penalties are on
    pub penalties: bool,
    /// `RaceManager::isOfficialSpecialEvent`
    pub official: bool,
    /// the player car's folder name
    pub car: String,
    pub track: String,
    pub track_config: String,
    pub sectors: usize,
    /// the game mode's GUID; None: no game mode object
    pub game_mode: Option<String>,
    /// `Documents\Assetto Corsa`, where `INIReaderDocuments` reads
    pub documents: PathBuf,
}

const SESSION_PRACTICE: i32 = 1;
const SESSION_HOTLAP: i32 = 4;
const SESSION_TIME_ATTACK: i32 = 5;
const SESSION_DRIFT: i32 = 6;
const SESSION_DRAG: i32 = 7;
/// the hotlap board's score for "no time"
const NO_TIME: i32 = 0x4e72_9fff;
const AVATAR_BYTES: usize = 0x21100;

#[derive(Clone, Debug, Default)]
struct Achievement {
    display_name: String,
    display_description: String,
    achieved: bool,
    added: bool,
}

/// `CarTrackComb`: the times to beat of the four Steam lap achievements.
const COMBS: [(&str, &str, &str, f32, f32); 4] =
    [("ferrari_458", "imola", "", 116.6, 114.0), ("lotus_49", "m", "r", 90.0, 88.5), ("tatuusfa1", "v", "e", 94.0, 93.0), ("bmw_m3_gt2", "imola", "", 103.0, 102.0)];

const CALLBACK_STATS_RECEIVED: i32 = 0x44d;
const CALLBACK_STATS_STORED: i32 = 0x44e;
const RESULT_FIND: i32 = 0x450;
const RESULT_DOWNLOAD: i32 = 0x451;
const RESULT_UPLOAD: i32 = 0x452;

/// `CCallbackBase` as Steam reads it ({vtable, u8 flags, i32 id}), then ours.
#[repr(C)]
struct Callback {
    vtable: *const usize,
    flags: u8,
    id: i32,
    owner: *mut Inner,
    /// a call result's pending call (`m_hAPICall`)
    call: u64,
}

impl Callback {
    fn new(id: i32) -> Callback {
        Callback { vtable: (&VTABLE as *const Vtable).cast(), flags: 0, id, owner: std::ptr::null_mut(), call: 0 }
    }
}

/// MSVC orders the overloads backwards: slot 0 `Run(void*, bool, SteamAPICall_t)`, slot 1
/// `Run(void*)`, slot 2 `GetCallbackSizeBytes`.
#[repr(C)]
struct Vtable {
    run_result: unsafe extern "C" fn(*mut Callback, *mut u8, bool, u64),
    run_callback: unsafe extern "C" fn(*mut Callback, *mut u8),
    size_bytes: unsafe extern "C" fn(*mut Callback) -> i32,
}
static VTABLE: Vtable = Vtable { run_result, run_callback, size_bytes };

unsafe extern "C" fn run_callback(cb: *mut Callback, data: *mut u8) {
    let owner = (*cb).owner;
    match (*cb).id {
        CALLBACK_STATS_RECEIVED => (*owner).on_user_stats_received(),
        CALLBACK_STATS_STORED => (*owner).on_user_stats_stored(data.add(8).cast::<i32>().read_unaligned()),
        _ => {}
    }
}

unsafe extern "C" fn run_result(cb: *mut Callback, data: *mut u8, io_failure: bool, call: u64) {
    if (*cb).id == CALLBACK_STATS_RECEIVED || (*cb).id == CALLBACK_STATS_STORED {
        run_callback(cb, data);
        return;
    }
    // CCallResult::Run
    if call != (*cb).call {
        return;
    }
    (*cb).call = 0;
    let owner = (*cb).owner;
    match (*cb).id {
        RESULT_FIND => (*owner).on_find_leaderboard(data.cast::<u64>().read_unaligned(), *data.add(8) != 0, io_failure),
        RESULT_DOWNLOAD => (*owner).on_download_score(data.add(8).cast::<u64>().read_unaligned(), data.add(0x10).cast::<i32>().read_unaligned(), io_failure),
        RESULT_UPLOAD => (*owner).on_upload_score(*data != 0, *data.add(0x14) != 0, io_failure),
        _ => {}
    }
}

unsafe extern "C" fn size_bytes(cb: *mut Callback) -> i32 {
    match (*cb).id {
        CALLBACK_STATS_RECEIVED | RESULT_DOWNLOAD => 0x18,
        RESULT_UPLOAD => 0x20,
        _ => 0x10,
    }
}

struct Inner {
    api: Flat,
    host: Box<dyn Host>,
    session: Session,
    frame: Frame,
    stats_received: Callback,
    stats_stored: Callback,
    find: Callback,
    upload: Callback,
    download: Callback,
    achievements: BTreeMap<String, Achievement>,
    avatar: u64,
    car_position: [f32; 3],
    current_leaderboard: u64,
    max_damage: f32,
    distance: f32,
    std_time: u32,
    pro_time: u32,
    time_attack_on: bool,
    hotlap_on: bool,
    /// the drift mode's level once one was found (the game keeps the object)
    drift_level: Option<i32>,
    /// `GameMode::forceConditionAchieved` calls, for the caller's game mode
    forced: Vec<(usize, bool)>,
}

fn narrow(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

macro_rules! call {
    ($interface:expr, $offset:expr, fn($($t:ty),*) $(-> $r:ty)?, $($arg:expr),*) => {{
        let i = $interface;
        let f = std::mem::transmute::<usize, unsafe extern "C" fn(Interface, $($t),*) $(-> $r)?>(slot(i, $offset / 8));
        f(i, $($arg),*)
    }};
}

impl Inner {
    unsafe fn steam_id(&self) -> u64 {
        let mut id = 0u64;
        let out = call!((self.api.user)(), 0x10, fn(*mut u64) -> *mut u64, &mut id);
        *out
    }

    /// `CCallResult::Set`
    unsafe fn set(&mut self, which: i32, call: u64) {
        let api = self.api;
        let cb = match which {
            RESULT_FIND => &mut self.find,
            RESULT_DOWNLOAD => &mut self.download,
            _ => &mut self.upload,
        };
        if cb.call != 0 {
            (api.unregister_call_result)((cb as *mut Callback).cast(), cb.call);
        }
        cb.call = call;
        if call != 0 {
            (api.register_call_result)((cb as *mut Callback).cast(), call);
        }
    }

    /// `fetchPlayerImage`
    unsafe fn fetch_player_image(&mut self) {
        let id = self.steam_id();
        let image = call!((self.api.friends)(), 0x120, fn(u64) -> i32, id);
        if image > 0 {
            let mut pixels = vec![0u8; AVATAR_BYTES];
            if call!((self.api.utils)(), 0x30, fn(i32, *mut u8, i32) -> bool, image, pixels.as_mut_ptr(), AVATAR_BYTES as i32) {
                self.avatar = self.host.create_texture(184, 184, &pixels);
            } else {
                self.host.printf("WARNING: SteamInterface::Couldn't get steam image");
            }
        }
    }

    /// `createLeaderboards`
    unsafe fn create_leaderboards(&mut self, model: &str, track: &str, config: &str) {
        if !self.session.penalties {
            return;
        }
        let (prefix, sort) = match self.session.session_type {
            SESSION_HOTLAP => ("_lb_HOTLAP_", 1),
            SESSION_TIME_ATTACK => ("_lb_TIME_ATTACK_", 2),
            SESSION_DRIFT => ("_lb_DRIFT_", 2),
            _ => return,
        };
        let mut name = format!("{prefix}{model}_{track}");
        if !config.is_empty() {
            name += &format!("_{config}");
        }
        let n = narrow(&name);
        let call = call!((self.api.user_stats)(), 0xb0, fn(*const c_char, i32, i32) -> u64, n.as_ptr(), sort, 1);
        self.host.printf(&format!("CREATING STEAM LEADERBOARD {name}"));
        self.set(RESULT_FIND, call);
    }

    unsafe fn download(&mut self) {
        let call = call!((self.api.user_stats)(), 0xe0, fn(u64, i32, i32, i32) -> u64, self.current_leaderboard, 2, -2, 2);
        self.set(RESULT_DOWNLOAD, call);
    }

    unsafe fn on_find_leaderboard(&mut self, board: u64, found: bool, io_failure: bool) {
        if found && !io_failure {
            self.current_leaderboard = board;
            self.download();
        } else {
            self.host.printf("ERROR: STEAM INTERFACE LEADERBOARD ERROR");
        }
    }

    unsafe fn on_upload_score(&mut self, success: bool, changed: bool, io_failure: bool) {
        if success && !io_failure {
            if changed {
                self.host.printf("STEAM INTERFACE Score changed");
                self.download();
            }
        } else {
            self.host.printf("STEAM INTERFACE ERROR OnUploadScore");
        }
    }

    unsafe fn on_download_score(&mut self, entries: u64, count: i32, io_failure: bool) {
        if entries == 0 || io_failure {
            self.host.printf("ERROR: SteamInterface::OnDownloadScore ERROR");
            return;
        }
        let mut rows = Vec::new();
        for i in 0..count.max(0) {
            // LeaderboardEntry_t {CSteamID user, i32 rank @8, i32 score @0xc, i32 details, UGCHandle_t @0x18}
            let mut entry = [0u8; 0x20];
            call!((self.api.user_stats)(), 0xf0, fn(u64, i32, *mut u8, *mut i32, i32) -> bool, entries, i, entry.as_mut_ptr(), std::ptr::null_mut(), 0);
            let user = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let rank = i32::from_le_bytes(entry[8..12].try_into().unwrap());
            let score = i32::from_le_bytes(entry[12..16].try_into().unwrap());
            if score == 0 || score == NO_TIME {
                continue;
            }
            let name = text(call!((self.api.friends)(), 0x38, fn(u64) -> *const c_char, user));
            let is_player = user == self.steam_id();
            rows.push(Row { name, score, rank, is_player });
        }
        self.host.rows(&rows);
    }

    /// `incrementStat`
    unsafe fn increment_stat(&mut self, name: &str) {
        let stats = (self.api.user_stats)();
        let id = self.steam_id();
        let n = narrow(name);
        let mut value = 0i32;
        if !call!(stats, 0x90, fn(u64, *const c_char, *mut i32) -> bool, id, n.as_ptr(), &mut value) {
            self.host.printf(&format!("ERROR: NO STAT FOR {name}"));
        } else if !call!(stats, 0x20, fn(*const c_char, i32) -> bool, n.as_ptr(), value + 1) {
            self.host.printf("ERROR: Steam SET STAT FAILED");
        } else if !call!(stats, 0x50, fn() -> bool,) {
            self.host.printf("ERROR: Steam StoreStats FAILED");
        }
    }

    /// `updateStats`
    unsafe fn update_stats(&mut self) {
        self.increment_stat("runs");
        if !self.frame.replay_loaded {
            self.increment_stat("games");
            let mut car = self.session.car.clone();
            for needle in ["_step1", "_step2", "_step3", "_drift", "_S1", "_S2", "_S3", "_s1", "_s2", "_s3"] {
                if let Some(at) = car.find(needle) {
                    car.truncate(at);
                }
            }
            self.increment_stat(&car);
            let track = self.session.track.clone();
            self.increment_stat(&track);
            match self.session.session_type {
                SESSION_DRIFT => self.increment_stat("drift_mode"),
                SESSION_TIME_ATTACK => self.increment_stat("time_attack_mode"),
                _ => {}
            }
        } else {
            self.increment_stat("replay_mode");
        }
        if let Ok(controls) = IniReader::load(&self.session.documents.join("cfg/controls.ini")) {
            if controls.ready {
                let stat = match controls.get_string("HEADER", "INPUT_METHOD").as_str() {
                    "X360" => "controls_x360",
                    "KEYBOARD" => "controls_keyboard",
                    _ => "controls_wheel",
                };
                self.increment_stat(stat);
            }
        }
    }

    /// `addAchievement`
    unsafe fn add_achievement(&mut self, name: &str) {
        let stats = (self.api.user_stats)();
        let n = narrow(name);
        let mut achieved = false;
        if !call!(stats, 0x30, fn(*const c_char, *mut bool) -> bool, n.as_ptr(), &mut achieved) {
            return;
        }
        let attribute = |key: &CStr| text(call!(stats, 0x60, fn(*const c_char, *const c_char) -> *const c_char, n.as_ptr(), key.as_ptr()));
        let mut display_name = attribute(c"name");
        if display_name.is_empty() {
            display_name = name.to_string();
        }
        let display_description = attribute(c"desc");
        self.achievements.insert(name.to_string(), Achievement { display_name, display_description, achieved, added: true });
    }

    /// `addAchievements`
    unsafe fn add_achievements(&mut self) {
        for name in [
            "AC_ONEHUNDRED", "AC_TWOHUNDRED", "AC_THREEHUNDRED", "AC_CLEANDRIVE", "AC_HOTLAPPER", "AC_TIMEATTACK_3", "AC_TIMEATTACK_5", "AC_TIMEATTACK_10", "AC_CRASH",
            "AC_PRACTICE_10", "AC_NOASSISTS_PRACTICE", "AC_DISTANCE_50", "AC_DISTANCE_100", "AC_DISTANCE_250", "AC_COMBO_10", "AC_COMBO_25", "AC_COMBO_50", "DRIFT_LEVEL_10",
            "DRIFT_LEVEL_25", "DRIFT_LEVEL_50", "DRIFT_LEVEL_100",
        ] {
            self.add_achievement(name);
        }
        for i in 0..400 {
            for j in 0..4 {
                self.add_achievement(&format!("SPECIAL_EVENT_{i}_{j}"));
            }
        }
        if let Some(guid) = self.session.game_mode.clone() {
            let stats = (self.api.user_stats)();
            for k in 0..4 {
                let n = narrow(&format!("SPECIAL_EVENT_{guid}_{k}"));
                let mut achieved = false;
                if call!(stats, 0x30, fn(*const c_char, *mut bool) -> bool, n.as_ptr(), &mut achieved) {
                    self.forced.push((k, achieved));
                }
            }
        }
        for (car, track, config, _, _) in COMBS {
            let name = if config.is_empty() { format!("{car}_{track}") } else { format!("{car}_{track}_{config}") };
            self.add_achievement(&name);
            self.add_achievement(&format!("{name}_pro"));
        }
    }

    /// `achievementCompleted`
    unsafe fn achievement_completed(&mut self, name: &str) {
        let a = self.achievements.entry(name.to_string()).or_default();
        if a.achieved || !a.added || !self.session.penalties {
            return;
        }
        self.host.message(&a.display_name, &a.display_description);
        a.achieved = true;
        let n = narrow(name);
        let stats = (self.api.user_stats)();
        call!(stats, 0x38, fn(*const c_char) -> bool, n.as_ptr());
        call!(stats, 0x50, fn() -> bool,);
    }

    unsafe fn on_user_stats_received(&mut self) {
        self.add_achievements();
        self.update_stats();
    }

    unsafe fn on_user_stats_stored(&mut self, result: i32) {
        if result == 8 {
            self.host.printf("ERROR: FAILED Steam UserStatStored");
        }
    }

    /// `generalAchievements`
    unsafe fn general(&mut self) {
        let kmh = self.frame.speed * 3.6;
        for (limit, name) in [(100.0, "AC_ONEHUNDRED"), (200.0, "AC_TWOHUNDRED"), (300.0, "AC_THREEHUNDRED")] {
            if kmh > limit {
                self.achievement_completed(name);
            }
        }
        let lap = self.frame.lap;
        let (car, track, config) = (self.session.car.clone(), self.session.track.clone(), self.session.track_config.clone());
        for (time, suffix) in [(self.std_time, ""), (self.pro_time, "_pro")] {
            if time != 0 && lap.valid && lap.time != 0 && lap.time < time && lap.splits == self.session.sectors {
                let name = if config.is_empty() {
                    format!("{car}_{track}{suffix}")
                } else if car == "tatuusfa1" && track == "ks_vallelunga" && config == "extended_circuit" {
                    format!("{car}_v_e{suffix}")
                } else if car == "lotus_49" && track == "ks_monza66" && config == "road" {
                    format!("{car}_m_r{suffix}")
                } else {
                    format!("{car}_{track}_{config}{suffix}")
                };
                self.achievement_completed(&name);
            }
        }
        let (c, p) = (self.car_position, self.frame.pos);
        let x = (c[0] - p[0]) * (c[0] - p[0]);
        let y = (c[1] - p[1]) * (c[1] - p[1]);
        let d2 = x + y + (c[2] - p[2]) * (c[2] - p[2]);
        let d = if d2 == 0.0 { 0.0 } else { d2.sqrt() };
        self.distance += d;
        self.car_position = p;
        for (limit, name) in [(50_000.0, "AC_DISTANCE_50"), (100_000.0, "AC_DISTANCE_100"), (250_000.0, "AC_DISTANCE_250")] {
            if self.distance > limit {
                self.achievement_completed(name);
            }
        }
    }

    /// `damageRelatedAchievements`
    unsafe fn damage(&mut self) {
        let mut sum = 0.0f32;
        for d in self.frame.damage {
            sum += d;
            self.max_damage = if self.max_damage < d { d } else { self.max_damage };
        }
        if self.frame.laps >= 5 && self.max_damage == 0.0 && self.session.session_type != SESSION_DRAG {
            self.achievement_completed("AC_CLEANDRIVE");
        }
        if sum > 200.0 {
            self.achievement_completed("AC_CRASH");
        }
    }

    /// `achievementCheck`
    unsafe fn achievement_check(&mut self) {
        self.general();
        self.damage();
        let laps = self.frame.laps;
        if self.hotlap_on && laps > 9 {
            self.achievement_completed("AC_HOTLAPPER");
        }
        if self.session.session_type == SESSION_PRACTICE {
            if laps > 9 {
                self.achievement_completed("AC_PRACTICE_10");
            }
            let f = self.frame;
            if laps > 9 && !f.abs && !f.tc && !f.auto_shifter && !f.auto_blip {
                self.achievement_completed("AC_NOASSISTS_PRACTICE");
            }
        }
        if self.time_attack_on {
            for (n, name) in [(2, "AC_TIMEATTACK_3"), (4, "AC_TIMEATTACK_5"), (9, "AC_TIMEATTACK_10")] {
                if laps > n {
                    self.achievement_completed(name);
                }
            }
        }
        if self.session.session_type == SESSION_DRIFT {
            for (n, name) in [(9, "AC_COMBO_10"), (24, "AC_COMBO_25"), (49, "AC_COMBO_50")] {
                if self.frame.combo > n {
                    self.achievement_completed(name);
                }
            }
            if self.drift_level.is_none() {
                self.drift_level = self.frame.drift_level;
            } else if let Some(level) = self.frame.drift_level {
                self.drift_level = Some(level);
            }
            let Some(level) = self.drift_level else { return };
            for (n, name) in [(8, "DRIFT_LEVEL_10"), (23, "DRIFT_LEVEL_25"), (48, "DRIFT_LEVEL_50"), (98, "DRIFT_LEVEL_100")] {
                if level > n {
                    self.achievement_completed(name);
                }
            }
        }
    }
}

/// The game's `SteamInterface` object.
pub struct Achievements {
    inner: *mut Inner,
}

impl Achievements {
    /// The constructor, with the session's first frame of the car (`update` comes later).
    ///
    /// # Safety
    /// `api` must be Steamworks' functions (or stand-ins with the same signatures).
    pub unsafe fn new(api: Flat, host: Box<dyn Host>, session: Session) -> Achievements {
        let inner = Box::into_raw(Box::new(Inner {
            api,
            host,
            session,
            frame: Frame::default(),
            stats_received: Callback::new(CALLBACK_STATS_RECEIVED),
            stats_stored: Callback::new(CALLBACK_STATS_STORED),
            find: Callback::new(RESULT_FIND),
            upload: Callback::new(RESULT_UPLOAD),
            download: Callback::new(RESULT_DOWNLOAD),
            achievements: BTreeMap::new(),
            avatar: 0,
            car_position: [0.0; 3],
            current_leaderboard: 0,
            max_damage: 0.0,
            distance: 0.0,
            std_time: 0,
            pro_time: 0,
            time_attack_on: false,
            hotlap_on: false,
            drift_level: None,
            forced: Vec::new(),
        }));
        let s = &mut *inner;
        for cb in [&mut s.stats_received, &mut s.stats_stored, &mut s.find, &mut s.upload, &mut s.download] {
            cb.owner = inner;
        }
        (api.register_callback)((&mut s.stats_received as *mut Callback).cast(), CALLBACK_STATS_RECEIVED);
        (api.register_callback)((&mut s.stats_stored as *mut Callback).cast(), CALLBACK_STATS_STORED);
        let race = IniReader::load(&s.session.documents.join("cfg/race.ini")).unwrap_or_default();
        let (model, track, config) = (race.get_string("RACE", "MODEL"), race.get_string("RACE", "TRACK"), race.get_string("RACE", "CONFIG_TRACK"));
        for (car, t, c, std, pro) in COMBS {
            let matched = (model == "tatuusfa1" && track == "ks_vallelunga" && config == "extended_circuit" && t == "v" && c == "e")
                || (model == "lotus_49" && track == "ks_monza66" && config == "road" && t == "m" && c == "r")
                || (car == model && t == track && c == config);
            if matched {
                s.std_time = (std * 1000.0) as i64 as u32;
                s.pro_time = (pro * 1000.0) as i64 as u32;
            }
        }
        if s.std_time == 0 || s.pro_time == 0 {
            s.host.printf("Steam Interface :: COULDN't FIND A TIME TO BEAT!");
        }
        s.fetch_player_image();
        match s.session.session_type {
            SESSION_TIME_ATTACK => s.time_attack_on = true,
            SESSION_HOTLAP => s.hotlap_on = true,
            _ => {}
        }
        call!((api.user_stats)(), 0x00, fn() -> bool,);
        s.create_leaderboards(&model, &track, &config);
        Achievements { inner }
    }

    /// `update`: Steam's callbacks, then the checks.
    pub fn update(&mut self, frame: Frame) {
        // SAFETY: no reference into `inner` lives across RunCallbacks, which calls back in
        unsafe {
            (*self.inner).frame = frame;
            ((*self.inner).api.run_callbacks)();
            let s = &mut *self.inner;
            if !frame.replay && s.session.penalties {
                s.achievement_check();
            }
        }
    }

    /// The game mode's `evOnTierCompleted` handler: an official event's goal reached.
    pub fn tier(&mut self, tier: i32) {
        // SAFETY: as in update
        let s = unsafe { &mut *self.inner };
        if s.session.official && s.session.penalties {
            let name = format!("SPECIAL_EVENT_{}_{tier}", s.session.game_mode.clone().unwrap_or_default());
            s.host.printf(&format!("STEAM :: SETTING ACHIEVEMENT {name}"));
            unsafe { s.achievement_completed(&name) };
        }
    }

    /// The game mode's `evOnLeaderboardScore` handler.
    pub fn score(&mut self, score: f32) {
        // SAFETY: as in update
        let s = unsafe { &mut *self.inner };
        if s.current_leaderboard != 0 {
            unsafe {
                let call = call!((s.api.user_stats)(), 0xf8, fn(u64, i32, i32, *const i32, i32) -> u64, s.current_leaderboard, 1, score as i32, std::ptr::null(), 0);
                s.set(RESULT_UPLOAD, call);
            }
        }
    }

    /// `refreshPlayerLeaderboard`
    pub fn refresh(&mut self) {
        // SAFETY: as in update
        unsafe { (*self.inner).download() }
    }

    /// `resetPlayerLeaderboard`
    pub fn reset(&mut self) {
        // SAFETY: as in update
        let s = unsafe { &mut *self.inner };
        let score = if s.session.session_type == SESSION_HOTLAP { NO_TIME } else { 0 };
        unsafe {
            let call = call!((s.api.user_stats)(), 0xf8, fn(u64, i32, i32, *const i32, i32) -> u64, s.current_leaderboard, 2, score, std::ptr::null(), 0);
            s.set(RESULT_UPLOAD, call);
        }
    }

    /// `resetAchievements` (the console's command)
    pub fn reset_achievements(&mut self) {
        // SAFETY: as in update
        let s = unsafe { &mut *self.inner };
        s.host.printf("RESETTING STEAM ACHIEVEMENTS");
        unsafe {
            let stats = (s.api.user_stats)();
            for name in s.achievements.keys() {
                let n = narrow(name);
                call!(stats, 0x40, fn(*const c_char) -> bool, n.as_ptr());
            }
            call!(stats, 0x50, fn() -> bool,);
        }
    }

    /// `GameMode::forceConditionAchieved` calls made since the last take: (condition, achieved).
    pub fn take_forced(&mut self) -> Vec<(usize, bool)> {
        // SAFETY: as in update
        std::mem::take(unsafe { &mut (*self.inner).forced })
    }
}

impl Drop for Achievements {
    fn drop(&mut self) {
        // SAFETY: made by Box::into_raw in new
        unsafe {
            let s = &mut *self.inner;
            if s.avatar != 0 {
                s.host.destroy_texture(s.avatar);
            }
            let api = s.api;
            for cb in [&mut s.download, &mut s.upload, &mut s.find] {
                if cb.call != 0 {
                    (api.unregister_call_result)((cb as *mut Callback).cast(), cb.call);
                }
            }
            for cb in [&mut s.stats_stored, &mut s.stats_received] {
                (api.unregister_callback)((cb as *mut Callback).cast());
            }
            drop(Box::from_raw(self.inner));
        }
    }
}


/// What [`Live`] tells the window: the game's GUI messages (title, description).
pub type Messages = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;

struct LiveHost(Messages);

impl Host for LiveHost {
    fn printf(&mut self, line: &str) {
        println!("{line}");
    }
    fn message(&mut self, text: &str, description: &str) {
        self.0.lock().unwrap().push((text.to_string(), description.to_string()));
    }
    // shortcut: no avatar texture and no Friends Leaderboard rows on screen (the calls are
    // made as the game makes them), until the Friends app is wired to the GUI
    fn create_texture(&mut self, _width: u32, _height: u32, _rgba: &[u8]) -> u64 {
        0
    }
    fn destroy_texture(&mut self, _id: u64) {}
    fn rows(&mut self, _rows: &[Row]) {}
}

/// A [`Live`] runs Steam's callbacks itself.
pub static LIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The port fed from a drive (on the physics thread): the game's per-frame `update`, and its
/// game mode's tier and leaderboard-score events.
pub struct Live {
    port: std::mem::ManuallyDrop<Achievements>,
    gm: bool,
    replay: bool,
    achieved: Vec<bool>,
    laps: usize,
    drift_over: bool,
}

impl Live {
    /// Made when the drive starts; None without Steam's DLL loaded or a referee.
    pub fn new(sim: &crate::sim::GameSim, messages: Messages, replay: bool) -> Option<Live> {
        let api = Flat::loaded()?;
        let referee = sim.referee.as_ref()?;
        let session_type = referee.manager.current_session.session_type;
        // the game's SteamInterface finds a GameMode only in hotlap, time attack and drift
        let gm = matches!(session_type, 4..=6);
        let mode = referee.mode.as_ref().filter(|_| gm).map(|m| m.base());
        let session = Session {
            session_type,
            penalties: referee.manager.penalties_enabled,
            official: mode.is_some_and(|m| m.official),
            car: sim.setup_car_name(),
            track: sim.track.as_ref().map_or(String::new(), |t| t.name.clone()),
            track_config: sim.track.as_ref().map_or(String::new(), |t| t.config.clone()),
            sectors: sim.lap_db.sector_count,
            game_mode: mode.map(|m| m.guid.clone()),
            documents: crate::conditions::documents_folder().unwrap_or_default(),
        };
        // Steam's callbacks run on this thread from now on (the window's run_callbacks waits for
        // this, then stops calling it): they call into the port
        crate::steam::session()?;
        LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
        // SAFETY: Flat::loaded's functions are Steamworks' own
        let port = unsafe { Achievements::new(api, Box::new(LiveHost(messages)), session) };
        Some(Live { port: std::mem::ManuallyDrop::new(port), gm, replay, achieved: Self::achieved(sim), laps: sim.lap_db.laps.len(), drift_over: false })
    }

    fn achieved(sim: &crate::sim::GameSim) -> Vec<bool> {
        let mode = sim.referee.as_ref().and_then(|r| r.mode.as_ref());
        mode.map_or(Vec::new(), |m| m.base().conditions.iter().map(|c| c.achieved).collect())
    }

    /// Once a drawn frame: `SteamInterface::update`.
    // shortcut: runs only where frames are drawn (the window, or headless with --drawn-frames),
    // until headless achievements are asked for
    pub fn frame(&mut self, sim: &crate::sim::GameSim) {
        use rustyac_physics::race::game_mode::Mode;
        let car = &sim.car.car;
        let p = car.core.get_position(car.body);
        let aids = car.aids.as_ref().map(|a| a.base());
        let lap = sim.lap_db.last_lap();
        let drift_level = match sim.referee.as_ref().and_then(|r| r.mode.as_ref()) {
            Some(Mode::Drift(d)) => Some(d.current_level),
            _ => None,
        };
        self.port.update(Frame {
            speed: car.speed,
            pos: [p.x, p.y, p.z],
            laps: sim.lap_db.laps.len() as u32,
            combo: car.drift.combo_counter,
            damage: car.damage_zone_level,
            abs: aids.is_some_and(|a| a.abs.is_present && a.abs.is_active),
            tc: aids.is_some_and(|a| a.traction_control.is_present && a.traction_control.is_active),
            auto_shifter: car.auto_shifter.is_active,
            auto_blip: car.auto_blip.is_active,
            drift_level,
            replay: self.replay,
            replay_loaded: self.replay,
            lap: Lap { time: lap.time, valid: lap.is_valid, cuts: lap.cuts, splits: lap.splits.len() },
        });
    }

    /// After the steps and frames of a loop: the game mode's `setTier` calls (its conditions
    /// newly achieved, in order), then its leaderboard scores (a hotlap's valid clean laps, the
    /// drift run's points at its end), then the achievements Steam already holds.
    pub fn events(&mut self, sim: &mut crate::sim::GameSim) {
        use rustyac_physics::race::drift::DriftStatus;
        use rustyac_physics::race::game_mode::Mode;
        if !self.gm {
            return;
        }
        let now = Self::achieved(sim);
        for i in (0..now.len()).filter(|&i| now[i] && !self.achieved.get(i).copied().unwrap_or(false)) {
            self.port.tier(i as i32);
        }
        let laps = &sim.lap_db.laps;
        let mode = sim.referee.as_ref().and_then(|r| r.mode.as_ref());
        if matches!(mode, Some(Mode::Hotlap(_))) {
            for lap in laps.iter().skip(self.laps) {
                if lap.time > 0 && lap.is_valid && lap.cuts == 0 {
                    self.port.score(lap.time as i64 as f32);
                }
            }
        }
        self.laps = laps.len();
        if let Some(Mode::Drift(d)) = mode {
            let over = d.mode == DriftStatus::Over;
            if over && !self.drift_over {
                self.port.score(d.max_points as f32);
            }
            self.drift_over = over;
        }
        if let Some(mode) = sim.referee.as_mut().and_then(|r| r.mode.as_mut()) {
            let conditions = &mut mode.base_mut().conditions;
            for (k, achieved) in self.port.take_forced() {
                if let Some(c) = conditions.get_mut(k) {
                    c.achieved = achieved;
                }
            }
        }
        self.achieved = Self::achieved(sim);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        // the port (its callbacks) goes before the window may run Steam's callbacks again
        // SAFETY: dropped once, here
        unsafe { std::mem::ManuallyDrop::drop(&mut self.port) };
        LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
