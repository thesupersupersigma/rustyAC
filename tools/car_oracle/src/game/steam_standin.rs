// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47d: a stand-in for `steam_api64.dll`'s stats side (user stats, achievements,
//! leaderboards, the player's avatar, callbacks and call results) that writes down every call
//! with its arguments and answers from a scripted [`Answers`]. Nothing reaches the real Steam.
//! Shared by `car_oracle achievements-micro` (as the game's imports) and the port's check
//! (`crates/rustyac-game/examples/achievements_check.rs`, as its `SteamApi`), so both sides
//! meet the same answers and leave the same log. Std only.

#![allow(dead_code)]

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

/// What the stand-in answers in a case.
#[derive(Clone, Copy, Debug, Default)]
pub struct Answers {
    pub seed: u32,
    pub steam_id: u64,
    /// `GetUserStat` finds the stat
    pub stats_ok: bool,
    /// `SetStat` works
    pub set_stat_ok: bool,
    /// `StoreStats` works (and its callback says OK, else result 8)
    pub store_ok: bool,
    /// `FindOrCreateLeaderboard`'s result finds one
    pub found: bool,
    /// `UploadLeaderboardScore`'s result: success and score changed
    pub upload_ok: bool,
    pub changed: bool,
    /// rows of a download
    pub entries: i32,
    /// `GetLargeFriendAvatar`'s image handle (0: none) and does `GetImageRGBA` work
    pub avatar: i32,
    pub image_ok: bool,
}

pub const AVATAR_BYTES: usize = 0x21100;

enum Pending {
    Callback(i32, Vec<u8>),
    Result(i32, u64, Vec<u8>),
}

struct State {
    answers: Answers,
    calls: Vec<String>,
    callbacks: Vec<(usize, i32)>,
    results: Vec<(u64, usize)>,
    pending: Vec<Pending>,
    next_call: u64,
    next_board: u64,
    strings: Vec<CString>,
}

static STATE: Mutex<State> = Mutex::new(State {
    answers: Answers { seed: 0, steam_id: 0, stats_ok: false, set_stat_ok: false, store_ok: false, found: false, upload_ok: false, changed: false, entries: 0, avatar: 0, image_ok: false },
    calls: Vec::new(),
    callbacks: Vec::new(),
    results: Vec::new(),
    pending: Vec::new(),
    next_call: 0,
    next_board: 0,
    strings: Vec::new(),
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn log(line: String) {
    state().calls.push(line);
}

/// A new case: its answers, nothing pending, nothing registered, handles from the start.
pub fn reset(answers: Answers) {
    let mut s = state();
    s.answers = answers;
    s.calls.clear();
    s.callbacks.clear();
    s.results.clear();
    s.pending.clear();
    s.next_call = 0x100;
    s.next_board = 0x7000;
}

/// The calls written down since the last take.
pub fn take_calls() -> Vec<String> {
    std::mem::take(&mut state().calls)
}

fn hash(text: &[u8], seed: u32) -> u32 {
    let mut h = 0x811c_9dc5u32 ^ seed;
    for &b in text {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a of an image, as written down for a texture.
pub fn image_hash(bytes: &[u8]) -> u32 {
    hash(bytes, 0)
}

unsafe fn name(p: *const c_char) -> String {
    if p.is_null() {
        return "(null)".into();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

fn keep(text: String) -> *const c_char {
    let mut s = state();
    s.strings.push(CString::new(text).unwrap_or_default());
    s.strings.last().map_or(std::ptr::null(), |c| c.as_ptr())
}

fn schedule_result(id: i32, data: impl FnOnce(u64) -> Vec<u8>) -> u64 {
    let mut s = state();
    s.next_call += 1;
    let h = s.next_call;
    s.pending.push(Pending::Result(id, h, data(h)));
    h
}

fn bytes<const N: usize>(fields: &[(usize, &[u8])]) -> Vec<u8> {
    let mut v = vec![0u8; N];
    for (at, b) in fields {
        v[*at..*at + b.len()].copy_from_slice(b);
    }
    v
}

// ---- the flat API ----

pub extern "C" fn run_callbacks() {
    log("SteamAPI_RunCallbacks".into());
    let pending = std::mem::take(&mut state().pending);
    for p in pending {
        match p {
            Pending::Callback(id, mut data) => {
                let targets: Vec<usize> = state().callbacks.iter().filter(|c| c.1 == id).map(|c| c.0).collect();
                log(format!("  callback {id:#x} to {}", targets.len()));
                for cb in targets {
                    // SAFETY: a registered CCallback: vtable slot 1 is Run(void*)
                    unsafe {
                        let vt = *(cb as *const *const usize);
                        let run: extern "C" fn(usize, *mut u8) = std::mem::transmute(*vt.add(1));
                        run(cb, data.as_mut_ptr());
                    }
                }
            }
            Pending::Result(id, h, mut data) => {
                let found = {
                    let mut s = state();
                    s.results.iter().position(|r| r.0 == h).map(|i| s.results.remove(i).1)
                };
                log(format!("  result {id:#x} {h} {}", if found.is_some() { "delivered" } else { "dropped" }));
                if let Some(cr) = found {
                    // SAFETY: a registered CCallResult: vtable slot 0 is Run(void*, bool, SteamAPICall_t)
                    unsafe {
                        let vt = *(cr as *const *const usize);
                        let run: extern "C" fn(usize, *mut u8, bool, u64) = std::mem::transmute(*vt);
                        run(cr, data.as_mut_ptr(), false, h);
                    }
                }
            }
        }
    }
}

pub extern "C" fn register_callback(cb: *mut u8, id: i32) {
    log(format!("SteamAPI_RegisterCallback {id:#x}"));
    // SAFETY: a CCallbackBase {vtable, u8 flags, i32 id}
    unsafe {
        *cb.add(8) |= 1;
        (cb.add(0xc) as *mut i32).write_unaligned(id);
    }
    state().callbacks.push((cb as usize, id));
}

pub extern "C" fn unregister_callback(cb: *mut u8) {
    // SAFETY: as above
    let id = unsafe {
        *cb.add(8) &= !1;
        (cb.add(0xc) as *const i32).read_unaligned()
    };
    log(format!("SteamAPI_UnregisterCallback {id:#x}"));
    state().callbacks.retain(|c| c.0 != cb as usize);
}

pub extern "C" fn register_call_result(cr: *mut u8, h: u64) {
    // SAFETY: as above
    let id = unsafe { (cr.add(0xc) as *const i32).read_unaligned() };
    log(format!("SteamAPI_RegisterCallResult {id:#x} {h}"));
    state().results.push((h, cr as usize));
}

pub extern "C" fn unregister_call_result(cr: *mut u8, h: u64) {
    // SAFETY: as above
    let id = unsafe { (cr.add(0xc) as *const i32).read_unaligned() };
    log(format!("SteamAPI_UnregisterCallResult {id:#x} {h}"));
    state().results.retain(|r| !(r.0 == h && r.1 == cr as usize));
}

// ---- ISteamUserStats ----

extern "C" fn request_current_stats(_this: usize) -> bool {
    log("ISteamUserStats::RequestCurrentStats".into());
    let id = state().answers.steam_id;
    // UserStatsReceived_t {u64 gameId, i32 result @8, CSteamID user @0x10}
    let data = bytes::<0x18>(&[(0, &244_210u64.to_le_bytes()), (8, &1i32.to_le_bytes()), (0x10, &id.to_le_bytes())]);
    state().pending.push(Pending::Callback(0x44d, data));
    true
}

extern "C" fn set_stat_int(_this: usize, stat: *const c_char, value: i32) -> bool {
    log(format!("ISteamUserStats::SetStat {} {value}", unsafe { name(stat) }));
    state().answers.set_stat_ok
}

/// (exists, achieved) of an achievement
fn achievement(text: &str) -> (bool, bool) {
    let h = hash(text.as_bytes(), state().answers.seed);
    (!h.is_multiple_of(7), h.is_multiple_of(3))
}

extern "C" fn get_achievement(_this: usize, ach: *const c_char, achieved: *mut bool) -> bool {
    let text = unsafe { name(ach) };
    let (exists, done) = achievement(&text);
    if exists {
        log(format!("ISteamUserStats::GetAchievement {text} -> {}", done as u8));
        // SAFETY: the caller's bool
        unsafe { *achieved = done };
    } else {
        log(format!("ISteamUserStats::GetAchievement {text} -> none"));
    }
    exists
}

extern "C" fn set_achievement(_this: usize, ach: *const c_char) -> bool {
    log(format!("ISteamUserStats::SetAchievement {}", unsafe { name(ach) }));
    true
}

extern "C" fn clear_achievement(_this: usize, ach: *const c_char) -> bool {
    log(format!("ISteamUserStats::ClearAchievement {}", unsafe { name(ach) }));
    true
}

extern "C" fn store_stats(_this: usize) -> bool {
    log("ISteamUserStats::StoreStats".into());
    let ok = state().answers.store_ok;
    // UserStatsStored_t {u64 gameId, i32 result @8}
    let data = bytes::<0x10>(&[(0, &244_210u64.to_le_bytes()), (8, &(if ok { 1i32 } else { 8 }).to_le_bytes())]);
    state().pending.push(Pending::Callback(0x44e, data));
    ok
}

extern "C" fn get_achievement_display_attribute(_this: usize, ach: *const c_char, key: *const c_char) -> *const c_char {
    let (ach, key) = unsafe { (name(ach), name(key)) };
    log(format!("ISteamUserStats::GetAchievementDisplayAttribute {ach} {key}"));
    keep(format!("{ach} {key}"))
}

extern "C" fn get_user_stat(_this: usize, user: u64, stat: *const c_char, value: *mut i32) -> bool {
    let text = unsafe { name(stat) };
    let (ok, seed) = {
        let s = state();
        (s.answers.stats_ok, s.answers.seed)
    };
    log(format!("ISteamUserStats::GetUserStat {user} {text}"));
    if ok {
        // SAFETY: the caller's int
        unsafe { *value = (hash(text.as_bytes(), seed) % 50) as i32 };
    }
    ok
}

extern "C" fn find_or_create_leaderboard(_this: usize, board: *const c_char, sort: i32, display: i32) -> u64 {
    log(format!("ISteamUserStats::FindOrCreateLeaderboard {} {sort} {display}", unsafe { name(board) }));
    let (found, lb) = {
        let mut s = state();
        s.next_board += 1;
        (s.answers.found, s.next_board)
    };
    // LeaderboardFindResult_t {u64 board, u8 found @8}
    schedule_result(0x450, |_| bytes::<0x10>(&[(0, &(if found { lb } else { 0 }).to_le_bytes()), (8, &[found as u8])]))
}

extern "C" fn download_leaderboard_entries(_this: usize, board: u64, kind: i32, start: i32, end: i32) -> u64 {
    log(format!("ISteamUserStats::DownloadLeaderboardEntries {board} {kind} {start} {end}"));
    let count = state().answers.entries;
    // LeaderboardScoresDownloaded_t {u64 board, u64 entries @8, i32 count @0x10}
    schedule_result(0x451, |h| bytes::<0x18>(&[(0, &board.to_le_bytes()), (8, &(h | 0xe000_0000).to_le_bytes()), (0x10, &count.to_le_bytes())]))
}

/// The score of a downloaded row: the two the game skips among them
pub fn row_score(index: i32, seed: u32) -> i32 {
    match index % 5 {
        3 => 0,
        4 => 0x4e72_9fff,
        _ => 55_000 + (hash(&index.to_le_bytes(), seed) % 9_000) as i32,
    }
}

/// The Steam id of a downloaded row (row 1 is the player)
pub fn row_id(index: i32, answers: &Answers) -> u64 {
    if index == 1 {
        answers.steam_id
    } else {
        76_561_197_960_000_000 + index as u64 * 17
    }
}

extern "C" fn get_downloaded_leaderboard_entry(_this: usize, entries: u64, index: i32, entry: *mut u8, details: *mut i32, max: i32) -> bool {
    log(format!("ISteamUserStats::GetDownloadedLeaderboardEntry {entries} {index} {} {max}", details.is_null() as u8));
    let answers = state().answers;
    // LeaderboardEntry_t {CSteamID user, i32 rank @8, i32 score @0xc, i32 details @0x10, u64 ugc @0x18}
    let row = bytes::<0x20>(&[(0, &row_id(index, &answers).to_le_bytes()), (8, &(index + 1).to_le_bytes()), (0xc, &row_score(index, answers.seed).to_le_bytes())]);
    // SAFETY: the caller's LeaderboardEntry_t
    unsafe { std::ptr::copy_nonoverlapping(row.as_ptr(), entry, row.len()) };
    true
}

extern "C" fn upload_leaderboard_score(_this: usize, board: u64, method: i32, score: i32, details: *const i32, count: i32) -> u64 {
    log(format!("ISteamUserStats::UploadLeaderboardScore {board} {method} {score} {} {count}", details.is_null() as u8));
    let (ok, changed) = {
        let s = state();
        (s.answers.upload_ok, s.answers.changed)
    };
    // LeaderboardScoreUploaded_t {u8 success, u64 board @8, i32 score @0x10, u8 changed @0x14, i32 new rank @0x18, i32 old @0x1c}
    schedule_result(0x452, |_| bytes::<0x20>(&[(0, &[ok as u8]), (8, &board.to_le_bytes()), (0x10, &score.to_le_bytes()), (0x14, &[changed as u8]), (0x18, &3i32.to_le_bytes()), (0x1c, &5i32.to_le_bytes())]))
}

// ---- ISteamUser, ISteamFriends, ISteamUtils ----

extern "C" fn get_steam_id(_this: usize, out: *mut u64) -> *mut u64 {
    log("ISteamUser::GetSteamID".into());
    // SAFETY: the caller's CSteamID
    unsafe { *out = state().answers.steam_id };
    out
}

/// The persona name of a Steam id (some longer than the 25 characters a row shows)
pub fn persona(id: u64) -> String {
    if id.is_multiple_of(3) {
        format!("a driver with a long name number {}", id % 1000)
    } else {
        format!("Friend {}", id % 1000)
    }
}

extern "C" fn get_friend_persona_name(_this: usize, id: u64) -> *const c_char {
    log(format!("ISteamFriends::GetFriendPersonaName {id}"));
    keep(persona(id))
}

extern "C" fn get_large_friend_avatar(_this: usize, id: u64) -> i32 {
    log(format!("ISteamFriends::GetLargeFriendAvatar {id}"));
    state().answers.avatar
}

/// The avatar's pixels
pub fn avatar_pixel(i: usize, seed: u32) -> u8 {
    (i as u32).wrapping_mul(31).wrapping_add(seed) as u8
}

extern "C" fn get_image_rgba(_this: usize, image: i32, out: *mut u8, size: i32) -> bool {
    log(format!("ISteamUtils::GetImageRGBA {image} {size}"));
    let answers = state().answers;
    if answers.image_ok {
        for i in 0..size.max(0) as usize {
            // SAFETY: the caller's buffer of `size` bytes
            unsafe { *out.add(i) = avatar_pixel(i, answers.seed) };
        }
    }
    answers.image_ok
}

extern "C" fn unknown_slot(_this: usize) -> usize {
    log("an interface slot the stand-in does not know".into());
    0
}

fn interface(slots: &[(usize, usize)]) -> usize {
    let mut vtable = vec![unknown_slot as *const () as usize; 64];
    for &(offset, f) in slots {
        vtable[offset / 8] = f;
    }
    let vtable = Box::leak(vtable.into_boxed_slice());
    Box::leak(Box::new(vtable.as_ptr() as usize)) as *mut usize as usize
}

static USER_STATS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static USER: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static FRIENDS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static UTILS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

// The interface getters are not written down: they change nothing.
pub extern "C" fn steam_user_stats() -> usize {
    *USER_STATS.get_or_init(|| {
        interface(&[
            (0x00, request_current_stats as *const () as usize),
            (0x20, set_stat_int as *const () as usize),
            (0x30, get_achievement as *const () as usize),
            (0x38, set_achievement as *const () as usize),
            (0x40, clear_achievement as *const () as usize),
            (0x50, store_stats as *const () as usize),
            (0x60, get_achievement_display_attribute as *const () as usize),
            (0x90, get_user_stat as *const () as usize),
            (0xb0, find_or_create_leaderboard as *const () as usize),
            (0xe0, download_leaderboard_entries as *const () as usize),
            (0xf0, get_downloaded_leaderboard_entry as *const () as usize),
            (0xf8, upload_leaderboard_score as *const () as usize),
        ])
    })
}
pub extern "C" fn steam_user() -> usize {
    *USER.get_or_init(|| interface(&[(0x10, get_steam_id as *const () as usize)]))
}
pub extern "C" fn steam_friends() -> usize {
    *FRIENDS.get_or_init(|| interface(&[(0x38, get_friend_persona_name as *const () as usize), (0x120, get_large_friend_avatar as *const () as usize)]))
}
pub extern "C" fn steam_utils() -> usize {
    *UTILS.get_or_init(|| interface(&[(0x30, get_image_rgba as *const () as usize)]))
}

/// The game's imports this stand-in answers.
pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    let dll = "steam_api64.dll";
    vec![
        (dll, "SteamAPI_RunCallbacks", run_callbacks as *const () as usize),
        (dll, "SteamAPI_RegisterCallback", register_callback as *const () as usize),
        (dll, "SteamAPI_UnregisterCallback", unregister_callback as *const () as usize),
        (dll, "SteamAPI_RegisterCallResult", register_call_result as *const () as usize),
        (dll, "SteamAPI_UnregisterCallResult", unregister_call_result as *const () as usize),
        (dll, "SteamUserStats", steam_user_stats as *const () as usize),
        (dll, "SteamUser", steam_user as *const () as usize),
        (dll, "SteamFriends", steam_friends as *const () as usize),
        (dll, "SteamUtils", steam_utils as *const () as usize),
    ]
}
