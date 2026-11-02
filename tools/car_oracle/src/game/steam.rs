// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47: the game's `SteamInit` @ 0x1401a7950 on a stand-in `steam_api64.dll` that writes
//! down every call and its arguments. Nothing reaches the real Steam.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use super::race_ini::write_wstring;
use super::*;

const VA_STEAM_INIT: usize = 0x1_401a_7950;
/// `KN5IO::dlc_keys` and `FolderEncrypter::keys` (std::vector<unsigned int>: first, last, end).
const VA_KN5_DLC_KEYS: usize = 0x1_4155_a310;
const VA_FOLDER_KEYS: usize = 0x1_4155_a5c8;
/// `SteamInterface::playerGUID` (std::wstring).
const VA_PLAYER_GUID: usize = 0x1_4151_ae30;
/// `CrashGuard::isTracking`.
const VA_CRASH_GUARD_TRACKING: usize = 0x1_4155_a340;

pub static SCENARIO: AtomicU32 = AtomicU32::new(0);
pub static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log(line: String) {
    CALLS.lock().unwrap().push(line);
}

/// What the stand-in Steam answers in a scenario: does `SteamAPI_Init` work, the persona
/// name, the Steam id and which of the 11 DLC app ids are owned (bit i: the i-th asked).
pub fn answers(scenario: u32, seed: u32) -> (bool, &'static str, u64, u32) {
    let names = ["Driver", "Pilota è veloce", "", "a name of thirty-two characters!!"];
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ scenario.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    let owned = match scenario {
        1 => 0,
        2 => 0x7ff,
        _ => x & 0x7ff,
    };
    (scenario != 0, names[scenario as usize % names.len()], 76_561_197_960_265_728 + u64::from(x), owned)
}

static DLC_ASKED: AtomicU32 = AtomicU32::new(0);

extern "C" fn init() -> bool {
    log("SteamAPI_Init".into());
    DLC_ASKED.store(0, Ordering::Relaxed);
    let (ok, ..) = answers(SCENARIO.load(Ordering::Relaxed), SEED.load(Ordering::Relaxed));
    ok
}

static NAME: Mutex<Vec<u8>> = Mutex::new(Vec::new());

extern "C" fn get_persona_name(_this: *mut u8) -> *const u8 {
    log("ISteamFriends::GetPersonaName".into());
    let (_, name, ..) = answers(SCENARIO.load(Ordering::Relaxed), SEED.load(Ordering::Relaxed));
    let mut keep = NAME.lock().unwrap();
    *keep = name.bytes().chain(Some(0)).collect();
    keep.as_ptr()
}

extern "C" fn get_steam_id(_this: *mut u8, out: *mut u64) -> *mut u64 {
    log("ISteamUser::GetSteamID".into());
    let (_, _, id, _) = answers(SCENARIO.load(Ordering::Relaxed), SEED.load(Ordering::Relaxed));
    // SAFETY: the caller's CSteamID
    unsafe { *out = id };
    out
}

extern "C" fn is_dlc_installed(_this: *mut u8, app: u32) -> bool {
    log(format!("ISteamApps::BIsDlcInstalled {app}"));
    let (.., owned) = answers(SCENARIO.load(Ordering::Relaxed), SEED.load(Ordering::Relaxed));
    let i = DLC_ASKED.fetch_add(1, Ordering::Relaxed);
    owned >> i & 1 != 0
}

extern "C" fn unknown_slot(_this: *mut u8) -> usize {
    log("an interface slot the stand-in does not know".into());
    0
}

/// One interface: a pointer to a vtable whose slot `slot` is `f`, the rest `unknown_slot`.
fn interface(slot: usize, f: usize) -> usize {
    let mut vtable = vec![unknown_slot as *const () as usize; 64];
    vtable[slot] = f;
    let vtable = Box::leak(vtable.into_boxed_slice());
    Box::leak(Box::new(vtable.as_ptr() as usize)) as *mut usize as usize
}

static FRIENDS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static USER: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static APPS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

extern "C" fn steam_friends() -> usize {
    log("SteamFriends".into());
    *FRIENDS.get_or_init(|| interface(0, get_persona_name as *const () as usize))
}
extern "C" fn steam_user() -> usize {
    log("SteamUser".into());
    *USER.get_or_init(|| interface(2, get_steam_id as *const () as usize))
}
extern "C" fn steam_apps() -> usize {
    log("SteamApps".into());
    *APPS.get_or_init(|| interface(7, is_dlc_installed as *const () as usize))
}
extern "C" fn shutdown() {
    log("SteamAPI_Shutdown".into());
}
/// (Steam's minidump writer: rustyAC never installs it, so it is left out of the calls)
extern "system" fn set_unhandled_exception_filter(_filter: usize) -> usize {
    0
}

pub fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("steam_api64.dll", "SteamAPI_Init", init as *const () as usize),
        ("steam_api64.dll", "SteamAPI_Shutdown", shutdown as *const () as usize),
        ("steam_api64.dll", "SteamFriends", steam_friends as *const () as usize),
        ("steam_api64.dll", "SteamUser", steam_user as *const () as usize),
        ("steam_api64.dll", "SteamApps", steam_apps as *const () as usize),
        ("kernel32.dll", "SetUnhandledExceptionFilter", set_unhandled_exception_filter as *const () as usize),
    ]
}

static SEED: AtomicU32 = AtomicU32::new(1);

unsafe fn vector_u32(at: *mut u8) -> Vec<u32> {
    let first: *const u32 = rd(at, 0);
    let last: *const u32 = rd(at, 8);
    if first.is_null() {
        return Vec::new();
    }
    std::slice::from_raw_parts(first, last.offset_from(first) as usize).to_vec()
}

unsafe fn wstring(at: *const u8) -> String {
    let size: usize = rd(at, 0x10);
    let cap: usize = rd(at, 0x18);
    let chars: *const u16 = if cap > 7 { rd(at, 0) } else { at.cast() };
    String::from_utf16_lossy(std::slice::from_raw_parts(chars, size))
}

/// Lines: `scenario <n>`, the Steam calls `SteamInit` made, `init <0|1>`, `guid <playerGUID>`,
/// `kn5 <keys>`, `acd <keys>`, then `SteamShutdown`'s calls and `end`. The crash guard is
/// tracking in odd scenarios.
pub unsafe fn micro(acs: &Acs, seed: u32, scenarios: u32) -> Result<String, String> {
    let steam_init: extern "C" fn() -> bool = std::mem::transmute(acs.va(VA_STEAM_INIT));
    let steam_shutdown: extern "C" fn() = std::mem::transmute(acs.va(0x1_401a_8380));
    SEED.store(seed, Ordering::Relaxed);
    let mut tape = String::new();
    for scenario in 0..scenarios {
        SCENARIO.store(scenario, Ordering::Relaxed);
        CALLS.lock().unwrap().clear();
        for at in [VA_KN5_DLC_KEYS, VA_FOLDER_KEYS] {
            let v = acs.va(at) as *mut u8;
            wr::<usize>(v, 8, rd::<usize>(v, 0));
        }
        write_wstring(acs.va(VA_PLAYER_GUID) as *mut u8, "");
        acs.set_global(VA_CRASH_GUARD_TRACKING, (scenario & 1) as u8);
        let ok = steam_init();
        let calls = std::mem::take(&mut *CALLS.lock().unwrap());
        writeln!(tape, "scenario {scenario}").unwrap();
        for call in calls {
            writeln!(tape, "{call}").unwrap();
        }
        let keys = |v: Vec<u32>| v.iter().map(|k| format!("{k:x}")).collect::<Vec<_>>().join(" ");
        writeln!(tape, "init {}", ok as u8).unwrap();
        writeln!(tape, "guid {}", wstring(acs.va(VA_PLAYER_GUID) as *const u8)).unwrap();
        writeln!(tape, "kn5 {}", keys(vector_u32(acs.va(VA_KN5_DLC_KEYS) as *mut u8))).unwrap();
        writeln!(tape, "acd {}", keys(vector_u32(acs.va(VA_FOLDER_KEYS) as *mut u8))).unwrap();
        if ok {
            steam_shutdown();
            for call in std::mem::take(&mut *CALLS.lock().unwrap()) {
                writeln!(tape, "{call}").unwrap();
            }
        }
        tape.push_str("end\n");
    }
    Ok(tape)
}
