// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47: `steam_check <steam.tape> <seed>`: the port's `SteamInit` / `SteamShutdown`
//! (`rustyac_game::steam`) on the same stand-in Steam as the game's run
//! (`car_oracle steam-micro`, tools/car_oracle/src/game/steam.rs), every call and its
//! arguments in order, the GUID and the DLC keys compared line by line.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use rustyac_game::steam::{Interface, Steam, SteamApi};

static SCENARIO: AtomicU32 = AtomicU32::new(0);
static SEED: AtomicU32 = AtomicU32::new(1);
static DLC_ASKED: AtomicU32 = AtomicU32::new(0);
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static NAME: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn log(line: String) {
    CALLS.lock().unwrap().push(line);
}

/// The same answers as the game's stand-in (tools/car_oracle/src/game/steam.rs `answers`).
fn answers(scenario: u32, seed: u32) -> (bool, &'static str, u64, u32) {
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

fn now() -> (bool, &'static str, u64, u32) {
    answers(SCENARIO.load(Ordering::Relaxed), SEED.load(Ordering::Relaxed))
}

unsafe extern "C" fn init() -> bool {
    log("SteamAPI_Init".into());
    DLC_ASKED.store(0, Ordering::Relaxed);
    now().0
}
unsafe extern "C" fn shutdown() {
    log("SteamAPI_Shutdown".into());
}
unsafe extern "C" fn run_callbacks() {
    log("SteamAPI_RunCallbacks".into());
}
unsafe extern "C" fn get_persona_name(_this: Interface) -> *const u8 {
    log("ISteamFriends::GetPersonaName".into());
    let mut keep = NAME.lock().unwrap();
    *keep = now().1.bytes().chain(Some(0)).collect();
    keep.as_ptr()
}
unsafe extern "C" fn get_steam_id(_this: Interface, out: *mut u64) -> *mut u64 {
    log("ISteamUser::GetSteamID".into());
    *out = now().2;
    out
}
unsafe extern "C" fn is_dlc_installed(_this: Interface, app: u32) -> bool {
    log(format!("ISteamApps::BIsDlcInstalled {app}"));
    let i = DLC_ASKED.fetch_add(1, Ordering::Relaxed);
    now().3 >> i & 1 != 0
}
unsafe extern "C" fn unknown_slot(_this: Interface) -> usize {
    log("an interface slot the stand-in does not know".into());
    0
}

fn interface(slot: usize, f: usize) -> Interface {
    let mut vtable = vec![unknown_slot as *const () as usize; 64];
    vtable[slot] = f;
    let vtable: &'static [usize] = Box::leak(vtable.into_boxed_slice());
    Box::leak(Box::new(vtable.as_ptr())) as *mut *const usize
}

static FRIENDS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static USER: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static APPS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

unsafe extern "C" fn friends() -> Interface {
    log("SteamFriends".into());
    *FRIENDS.get_or_init(|| interface(0, get_persona_name as *const () as usize) as usize) as Interface
}
unsafe extern "C" fn user() -> Interface {
    log("SteamUser".into());
    *USER.get_or_init(|| interface(2, get_steam_id as *const () as usize) as usize) as Interface
}
unsafe extern "C" fn apps() -> Interface {
    log("SteamApps".into());
    *APPS.get_or_init(|| interface(7, is_dlc_installed as *const () as usize) as usize) as Interface
}
unsafe extern "C" fn user_stats() -> Interface {
    log("SteamUserStats".into());
    std::ptr::null_mut()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("steam_check <steam.tape> <seed>");
        std::process::exit(2);
    }
    let game = std::fs::read_to_string(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    SEED.store(args[2].parse().expect("seed"), Ordering::Relaxed);
    let scenarios = game.lines().filter(|l| l.starts_with("scenario ")).count() as u32;
    let api = SteamApi { init, shutdown, run_callbacks, friends, user, apps, user_stats };
    let mut port = String::new();
    for scenario in 0..scenarios {
        SCENARIO.store(scenario, Ordering::Relaxed);
        CALLS.lock().unwrap().clear();
        // SAFETY: stand-ins with Steamworks' signatures
        let steam = unsafe { Steam::init(api) };
        port += &format!("scenario {scenario}\n");
        for call in std::mem::take(&mut *CALLS.lock().unwrap()) {
            port += &format!("{call}\n");
        }
        let keys = steam.as_ref().map(|s| s.dlc_keys.iter().map(|k| format!("{k:x}")).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        port += &format!("init {}\n", steam.is_some() as u8);
        port += &format!("guid {}\n", steam.as_ref().map(|s| s.id.to_string()).unwrap_or_default());
        port += &format!("kn5 {keys}\nacd {keys}\n");
        drop(steam);
        for call in std::mem::take(&mut *CALLS.lock().unwrap()) {
            port += &format!("{call}\n");
        }
        port += "end\n";
    }
    let calls = game.lines().filter(|l| l.starts_with("Steam") || l.starts_with("ISteam")).count();
    match game.lines().zip(port.lines()).position(|(a, b)| a != b) {
        None if game.lines().count() == port.lines().count() => {
            println!("steam start: IDENTICAL ({} lines of the game, {scenarios} cases, {calls} Steam calls)", game.lines().count());
        }
        at => {
            let at = at.unwrap_or(game.lines().count().min(port.lines().count()));
            println!("steam start: DIFFERENT at line {}: game {:?} port {:?}", at + 1, game.lines().nth(at), port.lines().nth(at));
            std::process::exit(1);
        }
    }
}
