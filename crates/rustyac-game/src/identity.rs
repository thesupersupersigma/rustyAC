// SPDX-License-Identifier: GPL-3.0-or-later

//! Who the player is to a server: the GUID of the handshake and the driver's name.
//!
//! Assetto Corsa sends the player's Steam community id as the GUID (`SteamInit`
//! @ 0x1401a7950: `SteamUser()->GetSteamID()`, printed as a decimal number). rustyAC does
//! the same when Steam is running, asking the `steam_api64.dll` of the player's own Assetto
//! Corsa folder (nothing of Steam is shipped with rustyAC). Without Steam it uses an id of
//! its own, made once from random bits and kept in `rustyac_net.ini` next to the program.
//! That id is a 17-digit number starting with 8, which no Steam account has (a player's
//! Steam id starts with 7656119...), so it can never be somebody else's.

use std::ffi::{c_void, CString, OsStr};
use std::hash::{BuildHasher, Hasher};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Assetto Corsa's Steam application id (the game's own `steam_appid.txt`).
const AC_APP_ID: &str = "244210";

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;

/// Where the GUID came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuidSource {
    Steam,
    /// rustyAC's own id, from this file.
    Own(PathBuf),
    /// Given on the command line.
    Given,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub guid: String,
    pub source: GuidSource,
}

/// The player's Steam id, when Steam is running and logged in. `ac_root`: the Assetto Corsa
/// folder (its `steam_api64.dll` is loaded, as the game loads it).
///
/// Steam is asked the way the game asks: `SteamAPI_Init`, `SteamUser()->GetSteamID()`, and
/// then `SteamAPI_Shutdown` at once, so rustyAC is no Steam client while it runs.
pub fn steam_id(ac_root: &Path) -> Option<u64> {
    let path = ac_root.join("steam_api64.dll");
    if !path.is_file() {
        return None;
    }
    let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
    // SAFETY: a terminated path; the DLL's exports are called with the signatures of
    // Steamworks' flat C API, which the game's own imports use.
    unsafe {
        let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH);
        if module.is_null() {
            return None;
        }
        let symbol = |name: &str| -> Option<usize> {
            let c = CString::new(name).ok()?;
            let p = GetProcAddress(module, c.as_ptr().cast());
            (!p.is_null()).then_some(p as usize)
        };
        let is_running = std::mem::transmute::<usize, unsafe extern "C" fn() -> bool>(symbol("SteamAPI_IsSteamRunning")?);
        let init = std::mem::transmute::<usize, unsafe extern "C" fn() -> bool>(symbol("SteamAPI_Init")?);
        let shutdown = std::mem::transmute::<usize, unsafe extern "C" fn()>(symbol("SteamAPI_Shutdown")?);
        let user = std::mem::transmute::<usize, unsafe extern "C" fn() -> *mut *const usize>(symbol("SteamUser")?);
        if !is_running() {
            return None;
        }
        // the game is started by Steam with these set; outside Steam the API takes the
        // application from them (as from the game folder's steam_appid.txt)
        if std::env::var_os("SteamAppId").is_none() {
            std::env::set_var("SteamAppId", AC_APP_ID);
        }
        if std::env::var_os("SteamGameId").is_none() {
            std::env::set_var("SteamGameId", AC_APP_ID);
        }
        if !init() {
            return None;
        }
        let mut id = 0u64;
        let instance = user();
        if !instance.is_null() {
            // ISteamUser::GetSteamID, vtable slot 2 (the game: `call [r8 + 0x10]`); the
            // CSteamID comes back through a hidden pointer
            let vtable = *instance;
            let get_steam_id = std::mem::transmute::<usize, unsafe extern "C" fn(*mut *const usize, *mut u64) -> *mut u64>(*vtable.add(2));
            get_steam_id(instance, &mut id);
        }
        shutdown();
        (id != 0).then_some(id)
    }
}

/// `rustyac_net.ini` next to the program.
pub fn own_file() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("rustyac_net.ini"))
}

fn random_u64() -> u64 {
    // std's hasher keys are random per process (the system's generator); the clock and
    // the process id are mixed in on top
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    hasher.write_u32(std::process::id());
    hasher.finish()
}

fn is_own_guid(text: &str) -> bool {
    text.len() == 17 && text.starts_with('8') && text.bytes().all(|b| b.is_ascii_digit())
}

/// rustyAC's own id from `file` (`[IDENTITY] GUID=`), made and written there when the file
/// has none yet.
pub fn own_guid(file: &Path) -> Result<String, String> {
    if file.exists() {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("GUID=") {
                if is_own_guid(value.trim()) {
                    return Ok(value.trim().to_string());
                }
            }
        }
        // (a file that is there is never replaced: it may be another file, or an id by hand)
        return Err(format!("{}: no GUID= line with one of rustyAC's own player ids (8 and sixteen digits); delete the file to have a new one made, or name another with --net-identity", file.display()));
    }
    let guid = format!("8{:016}", random_u64() % 10_000_000_000_000_000);
    let text = format!("; rustyAC's own player id for online servers, made once from random bits.\n; It is used when Steam is not running. Delete this file to get a new one.\n[IDENTITY]\nGUID={guid}\n");
    rustyac_content::guard::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(guid)
}

/// The player's identity: `given` (the command line), else Steam's when `use_steam` and
/// Steam is running, else rustyAC's own from `file` (default: next to the program).
pub fn resolve(given: Option<&str>, use_steam: bool, ac_root: Option<&Path>, file: Option<&Path>) -> Result<Identity, String> {
    if let Some(guid) = given {
        if guid.is_empty() || guid.len() > 64 || !guid.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("the player id {guid:?} is not letters and digits"));
        }
        return Ok(Identity { guid: guid.to_string(), source: GuidSource::Given });
    }
    if use_steam && file.is_none() {
        if let Some(id) = ac_root.and_then(steam_id) {
            return Ok(Identity { guid: id.to_string(), source: GuidSource::Steam });
        }
    }
    let file = match file {
        Some(file) => file.to_path_buf(),
        None => own_file().ok_or("no place for rustyac_net.ini")?,
    };
    Ok(Identity { guid: own_guid(&file)?, source: GuidSource::Own(file) })
}

/// The driver's name when none is given: the Windows user name, else "rustyAC driver".
pub fn default_name() -> String {
    match std::env::var("USERNAME") {
        Ok(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => "rustyAC driver".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_guid_is_made_once_and_is_no_steam_id() {
        // (next to the test program, inside the build folder)
        let file = std::env::current_exe().unwrap().parent().unwrap().join(format!("rustyac_identity_test_{}.ini", std::process::id()));
        let _ = rustyac_content::guard::remove_file(&file);
        let first = own_guid(&file).unwrap();
        assert!(is_own_guid(&first), "{first}");
        assert_eq!(own_guid(&file).unwrap(), first);
        let _ = rustyac_content::guard::remove_file(&file);
        assert!(resolve(Some("not ok!"), false, None, None).is_err());
    }
}
