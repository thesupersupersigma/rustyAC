// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Steam: the game's `SteamInit` @ 0x1401a7950 and `SteamShutdown` @ 0x1401a8380
//! (`SteamInterface.obj`), through the `steam_api64.dll` of the player's own Assetto Corsa
//! folder. Nothing of Steam is shipped with rustyAC.
//!
//! Only when Steam is running and logged in to an account that owns Assetto Corsa
//! (`SteamAPI_Init` says so): then the persona name, the Steam id (the online GUID) and the
//! owned DLCs' content keys come from Steam, and Steam stays on (its overlay) until the
//! program ends. Without Steam nothing changes: no keys are checked and everything loads.
//!
//! Left out on purpose: the game's `SetUnhandledExceptionFilter(MiniDumpFunction)`, which sends
//! crash dumps through Steam to the game's makers; a crash of rustyAC is not theirs.

use std::ffi::c_void;
use std::path::Path;
use std::sync::Mutex;

/// `SteamInit`'s DLC checks in its order: the DLC's app id (`ISteamApps::BIsDlcInstalled`) and
/// the content key it unlocks (`KN5IO::addDLCKey`, `FolderEncrypter::keys`).
pub const DLC_KEYS: [(u32, u32); 11] = [
    (347990, 0xa4691),
    (404430, 0xdeafd),
    (423630, 0xbebb1),
    (467980, 0x3b787),
    (493190, 0xb5735),
    (509350, 0x4c791),
    (526060, 0xe09a1),
    (540710, 0xcc03f),
    (540711, 0x905f2),
    (619680, 0x7e4bf),
    (675590, 0xdebda),
];

/// An interface of Steamworks: a pointer to its vtable.
pub type Interface = *mut *const usize;

/// The flat functions of `steam_api64.dll` the game imports.
#[derive(Clone, Copy)]
pub struct SteamApi {
    pub init: unsafe extern "C" fn() -> bool,
    pub shutdown: unsafe extern "C" fn(),
    pub run_callbacks: unsafe extern "C" fn(),
    pub friends: unsafe extern "C" fn() -> Interface,
    pub user: unsafe extern "C" fn() -> Interface,
    pub apps: unsafe extern "C" fn() -> Interface,
    pub user_stats: unsafe extern "C" fn() -> Interface,
}

/// A vtable slot of an interface (the game: `call [r8 + 8 * slot]`).
///
/// # Safety
/// `instance` is a live Steamworks interface with that slot.
pub unsafe fn slot(instance: Interface, index: usize) -> usize {
    *(*instance).add(index)
}

/// A Steam session (`SteamInit` worked); `SteamShutdown` when dropped.
pub struct Steam {
    pub api: SteamApi,
    /// `ISteamFriends::GetPersonaName`
    pub name: String,
    /// `ISteamUser::GetSteamID`: the player's id, the online GUID (`SteamInterface::playerGUID`)
    pub id: u64,
    /// the content keys of the owned DLCs, in `DLC_KEYS` order
    pub dlc_keys: Vec<u32>,
}

// SAFETY: plain function pointers into the DLL, which stays loaded
unsafe impl Send for Steam {}

impl Steam {
    /// `SteamInit` @ 0x1401a7950: `None` when `SteamAPI_Init` fails (the game then does not
    /// start; rustyAC goes on without Steam).
    ///
    /// # Safety
    /// `api` holds Steamworks' functions (or stand-ins with their signatures).
    pub unsafe fn init(api: SteamApi) -> Option<Steam> {
        if !(api.init)() {
            return None;
        }
        let friends = (api.friends)();
        let get_persona_name = std::mem::transmute::<usize, unsafe extern "C" fn(Interface) -> *const std::ffi::c_char>(slot(friends, 0));
        let name = std::ffi::CStr::from_ptr(get_persona_name(friends)).to_string_lossy().into_owned();
        // ISteamUser::GetSteamID, slot 2, its CSteamID through a hidden pointer; asked twice
        // as the game does (the printed "Steam ID", then playerGUID)
        let steam_id = || {
            let user = (api.user)();
            let get = std::mem::transmute::<usize, unsafe extern "C" fn(Interface, *mut u64) -> *mut u64>(slot(user, 2));
            let mut id = 0u64;
            get(user, &mut id);
            id
        };
        steam_id();
        let id = steam_id();
        let mut dlc_keys = Vec::new();
        for (app, key) in DLC_KEYS {
            let apps = (api.apps)();
            let is_dlc_installed = std::mem::transmute::<usize, unsafe extern "C" fn(Interface, u32) -> bool>(slot(apps, 7));
            if is_dlc_installed(apps, app) {
                dlc_keys.push(key);
            }
        }
        Some(Steam { api, name, id, dlc_keys })
    }
}

impl Drop for Steam {
    /// `SteamShutdown` @ 0x1401a8380.
    fn drop(&mut self) {
        // SAFETY: the session's own API
        unsafe { (self.api.shutdown)() }
    }
}

/// Assetto Corsa's Steam application id (the game's own `steam_appid.txt`).
const AC_APP_ID: &str = "244210";

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

/// The `steam_api64.dll` of the Assetto Corsa folder, when it is there and Steam is running
/// (`SteamAPI_IsSteamRunning`).
pub fn load(ac_root: &Path) -> Option<SteamApi> {
    use std::os::windows::ffi::OsStrExt;
    let path = ac_root.join("steam_api64.dll");
    if !path.is_file() {
        return None;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a terminated path; the exports are Steamworks' flat C API, the game's imports
    unsafe {
        let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x8);
        if module.is_null() {
            return None;
        }
        let symbol = |name: &str| -> Option<usize> {
            let c = std::ffi::CString::new(name).ok()?;
            let p = GetProcAddress(module, c.as_ptr().cast());
            (!p.is_null()).then_some(p as usize)
        };
        let is_running = std::mem::transmute::<usize, unsafe extern "C" fn() -> bool>(symbol("SteamAPI_IsSteamRunning")?);
        let api = SteamApi {
            init: std::mem::transmute::<usize, unsafe extern "C" fn() -> bool>(symbol("SteamAPI_Init")?),
            shutdown: std::mem::transmute::<usize, unsafe extern "C" fn()>(symbol("SteamAPI_Shutdown")?),
            run_callbacks: std::mem::transmute::<usize, unsafe extern "C" fn()>(symbol("SteamAPI_RunCallbacks")?),
            friends: std::mem::transmute::<usize, unsafe extern "C" fn() -> Interface>(symbol("SteamFriends")?),
            user: std::mem::transmute::<usize, unsafe extern "C" fn() -> Interface>(symbol("SteamUser")?),
            apps: std::mem::transmute::<usize, unsafe extern "C" fn() -> Interface>(symbol("SteamApps")?),
            user_stats: std::mem::transmute::<usize, unsafe extern "C" fn() -> Interface>(symbol("SteamUserStats")?),
        };
        if !is_running() {
            return None;
        }
        // the game is started by Steam with these set; outside Steam the API takes the
        // application from them (as from the game folder's steam_appid.txt)
        for name in ["SteamAppId", "SteamGameId"] {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, AC_APP_ID);
            }
        }
        Some(api)
    }
}

/// The program's Steam session, when [`start`] found Steam.
static SESSION: Mutex<Option<Steam>> = Mutex::new(None);

/// Starts Steam for the whole run (`SteamInit`) when it is running and owns Assetto Corsa:
/// the content readers then refuse the DLCs the account does not own, as the game does.
/// Returns whether Steam is on.
pub fn start(ac_root: &Path) -> bool {
    // SAFETY: the DLL's own functions
    let Some(steam) = load(ac_root).and_then(|api| unsafe { Steam::init(api) }) else {
        return false;
    };
    rustyac_content::kn5::set_dlc_keys(Some(steam.dlc_keys.clone()));
    *SESSION.lock().unwrap() = Some(steam);
    true
}

/// Ends the session (`SteamShutdown`); the content readers stop checking keys.
pub fn stop() {
    SESSION.lock().unwrap().take();
    rustyac_content::kn5::set_dlc_keys(None);
}

/// `SteamAPI_RunCallbacks`, each frame (`SteamInterface::update` @ 0x1401accd0).
pub fn run_callbacks() {
    if crate::achievements::LIVE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(steam) = SESSION.lock().unwrap().as_ref() {
        // SAFETY: the session's own API
        unsafe { (steam.api.run_callbacks)() }
    }
}

/// The session's Steam id and persona name, when Steam is on.
pub fn session() -> Option<(u64, String)> {
    SESSION.lock().unwrap().as_ref().map(|steam| (steam.id, steam.name.clone()))
}
