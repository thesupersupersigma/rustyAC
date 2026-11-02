// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Head tracking: the game's TrackIR interface (`TrackIR.obj`, `NPClientWraps.obj`).
//!
//! The game loads NaturalPoint's `NPClient64.dll` from the folder the registry names (FreeTrack
//! and opentrack install a DLL of that name too), checks its signature, and asks it each frame
//! for the head's angles and place; the cockpit camera turns and moves by them
//! (`DrivingCamera::head`). Nothing is shipped with rustyAC: without that DLL there is no head
//! tracking, as in the game.

use rustyac_physics::data::ini::IniReader;

/// `tagTrackIRData`, as `NP_GetData` fills it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackIrData {
    /// `wNPStatus`: 0 while the head is tracked
    pub status: u16,
    pub frame_signature: u16,
    pub io_data: u32,
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// raw, delta and smoothed x, y, z (unused)
    pub rest: [f32; 9],
}

/// `TrackIR::getOffsets` 0x1401ce530 after a poll: the turn (x pitch, y heading, z roll, in
/// degrees) and the move (metres, each within ±0.1) of the head. `division`: trackir.ini's
/// `[TRACK_IR] VALUE` (500 without it). Both zero while the head is not tracked.
pub fn offsets(data: &TrackIrData, division: f32) -> ([f32; 3], [f32; 3]) {
    if data.status != 0 {
        return ([0.0; 3], [0.0; 3]);
    }
    // (a NaN ends at -0.1, as the game's comparisons leave it)
    let clamp = |v: f32| {
        let v = v * 1.0e-5;
        if v > 0.1 {
            0.1
        } else if v >= -0.1 {
            v
        } else {
            -0.1
        }
    };
    ([-(data.pitch / division), data.yaw / division, data.roll / division], [clamp(data.x), clamp(data.y), clamp(data.z)])
}

/// `NPClient_Init` 0x1401ce740's two strings `NP_GetSignature` must give.
pub const DLL_SIGNATURE: &str = "precise head tracking\n put your head into the game\n now go look around\n\n Copyright EyeControl Technologies";
pub const APP_SIGNATURE: &str = "hardware camera\n software processing data\n track user movement\n\n Copyright EyeControl Technologies";

/// The functions `NPClient_Init` asks the DLL for after its signature, in its order.
pub const FUNCTIONS: [&str; 11] = [
    "NP_RegisterWindowHandle",
    "NP_UnregisterWindowHandle",
    "NP_RegisterProgramProfileID",
    "NP_QueryVersion",
    "NP_RequestData",
    "NP_GetData",
    "NP_StartCursor",
    "NP_StopCursor",
    "NP_ReCenter",
    "NP_StartDataTransmission",
    "NP_StopDataTransmission",
];

/// `NPClient_Init`'s DLL: the folder of the registry's `NPClient Location` (or "Error" without
/// one), a backslash, `NPClient64.dll`.
pub fn dll_path(location: &str) -> String {
    if location.is_empty() {
        "NPClient64.dll".into()
    } else {
        format!("{location}\\NPClient64.dll")
    }
}

/// `getDllLocation`'s registry key (under `HKEY_CURRENT_USER`) and the access it asks for.
pub const REGISTRY_KEY: &str = "Software\\NaturalPoint\\NATURALPOINT\\NPClient Location";
pub const REGISTRY_ACCESS: u32 = 0x20019;

/// `NP_ERR_DLL_NOT_FOUND`: what a wrapper answers for a function the DLL does not have.
const NOT_FOUND: i32 = 4;

/// The DLL's functions (`gpfNP_*`), 0 where it has none.
pub struct NpClient {
    functions: [usize; 11],
}

type Plain = unsafe extern "C" fn() -> i32;
type WithShort = unsafe extern "C" fn(u16) -> i32;

impl NpClient {
    /// `NPClient_Init` after `LoadLibraryA`: `symbol` is the DLL's `GetProcAddress`. `Err`: the
    /// message the game prints.
    ///
    /// # Safety
    /// `symbol` gives functions with the signatures of NaturalPoint's NPClient API, or 0.
    pub unsafe fn from_symbols(symbol: &mut dyn FnMut(&str) -> usize) -> Result<NpClient, &'static str> {
        let get_signature = symbol("NP_GetSignature");
        if get_signature == 0 {
            return Err("Signature2 ver failed\n");
        }
        // tagTrackIRSignature: two char[200]
        let mut signature = [0u8; 400];
        let get_signature: unsafe extern "C" fn(*mut u8) -> i32 = std::mem::transmute(get_signature);
        if get_signature(signature.as_mut_ptr()) != 0 {
            return Err("Signature2 ver failed\n");
        }
        // (strcmp: up to the first zero)
        let text = |at: usize| {
            let field = &signature[at..at + 200];
            field[..field.iter().position(|b| *b == 0).unwrap_or(200)].to_vec()
        };
        if text(0) != DLL_SIGNATURE.as_bytes() || text(200) != APP_SIGNATURE.as_bytes() {
            return Err("Signature ver failed\n");
        }
        Ok(NpClient { functions: FUNCTIONS.map(symbol) })
    }

    fn function(&self, name: &str) -> usize {
        self.functions[FUNCTIONS.iter().position(|n| *n == name).expect("an NPClient function")]
    }

    unsafe fn plain(&self, name: &str) -> i32 {
        match self.function(name) {
            0 => NOT_FOUND,
            f => std::mem::transmute::<usize, Plain>(f)(),
        }
    }

    unsafe fn with_short(&self, name: &str, value: u16) -> i32 {
        match self.function(name) {
            0 => NOT_FOUND,
            f => std::mem::transmute::<usize, WithShort>(f)(value),
        }
    }
}

/// `TrackIR`: a DLL whose version query answered. Dropping it stops the data and lets the
/// window go (`TrackIR::~TrackIR` 0x1401ce2a0).
pub struct TrackIr {
    np: NpClient,
    /// `divisionValue`
    pub division: f32,
}

impl TrackIr {
    /// `TrackIR::TrackIR` 0x1401cdfe0 after a found DLL: the window, the version, data mask
    /// 0x77, profile 1002, the cursor off, the data on, then trackir.ini (`ini`: the file of the
    /// Documents folder, when it could be read). `None`: the version query failed, and the
    /// game has no head tracking then either.
    ///
    /// # Safety
    /// As [`NpClient::from_symbols`]; `window` is the game's window.
    pub unsafe fn start(np: NpClient, window: isize, ini: Option<&IniReader>) -> Option<TrackIr> {
        println!("TrackIR found");
        match np.function("NP_RegisterWindowHandle") {
            0 => {}
            f => {
                std::mem::transmute::<usize, unsafe extern "C" fn(isize) -> i32>(f)(window);
            }
        }
        let mut version = 0u16;
        let query = np.function("NP_QueryVersion");
        if query == 0 || std::mem::transmute::<usize, unsafe extern "C" fn(*mut u16) -> i32>(query)(&mut version) != 0 {
            return None;
        }
        println!("NaturalPoint TrackIR software version is {}.{:02}", version >> 8, version & 0xff);
        np.with_short("NP_RequestData", 0x77);
        np.with_short("NP_RegisterProgramProfileID", 1002);
        println!("{}", if np.plain("NP_StopCursor") == 0 { "Cursor stopped" } else { "NPCient : Error stopping cursor" });
        println!("{}", if np.plain("NP_StartDataTransmission") == 0 { "Data transmission started" } else { "NPCient : Error starting data transmission" });
        let mut division = 500.0;
        if let Some(ini) = ini.filter(|ini| ini.has_section("TRACK_IR")) {
            // (text that is no number: the game throws; here the start value stays)
            division = ini.get_float("TRACK_IR", "VALUE").unwrap_or(division);
        }
        Some(TrackIr { np, division })
    }

    /// `TrackIR::getOffsets` 0x1401ce530: one poll, then [`offsets`].
    pub fn poll(&self) -> ([f32; 3], [f32; 3]) {
        let mut data = TrackIrData::default();
        let f = self.np.function("NP_GetData");
        // SAFETY: the DLL's NP_GetData fills a tagTrackIRData.
        let result = if f == 0 { NOT_FOUND } else { unsafe { std::mem::transmute::<usize, unsafe extern "C" fn(*mut TrackIrData) -> i32>(f)(&mut data) } };
        if result != 0 {
            println!("TrackIR poll failed");
        }
        offsets(&data, self.division)
    }
}

impl Drop for TrackIr {
    fn drop(&mut self) {
        println!("Releasing TrackIR interface");
        // SAFETY: as in start
        unsafe {
            self.np.plain("NP_StopDataTransmission");
            self.np.plain("NP_UnregisterWindowHandle");
        }
    }
}

/// The game's whole start: the registry's folder, the DLL, [`NpClient::from_symbols`],
/// [`TrackIr::start`]. `documents`: the Documents `Assetto Corsa` folder.
#[cfg(windows)]
pub fn open(window: isize, documents: Option<&std::path::Path>) -> Option<TrackIr> {
    use std::ffi::{c_void, CString};
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    println!("Initializing TrackIR interface");
    let location = dll_location();
    let path = CString::new(dll_path(&location)).ok()?;
    // SAFETY: a terminated name; the DLL's exports are NaturalPoint's NPClient API.
    unsafe {
        let module = LoadLibraryA(path.as_ptr().cast());
        if module.is_null() {
            println!("LoadLibrary failed\nTrackIR not found at {location}");
            return None;
        }
        let mut symbol = |name: &str| CString::new(name).map_or(0, |c| GetProcAddress(module, c.as_ptr().cast()) as usize);
        let np = match NpClient::from_symbols(&mut symbol) {
            Ok(np) => np,
            Err(message) => {
                print!("{message}");
                println!("TrackIR not found at {location}");
                return None;
            }
        };
        let ini = documents.and_then(|d| IniReader::load(&d.join("cfg/trackir.ini")).ok());
        TrackIr::start(np, window, ini.as_ref())
    }
}

/// `getDllLocation` 0x1401ce340: `HKCU\Software\NaturalPoint\NATURALPOINT\NPClient Location`'s
/// `Path` (up to its first zero), "Error" without it.
#[cfg(windows)]
fn dll_location() -> String {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExA(key: isize, sub: *const u8, options: u32, access: u32, out: *mut isize) -> i32;
        fn RegQueryValueExA(key: isize, name: *const u8, reserved: *mut u32, kind: *mut u32, data: *mut u8, size: *mut u32) -> i32;
        fn RegCloseKey(key: isize) -> i32;
    }
    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    let mut key = 0isize;
    // SAFETY: terminated names, buffers of the size given.
    unsafe {
        let sub = format!("{REGISTRY_KEY}\0");
        if RegOpenKeyExA(HKEY_CURRENT_USER, sub.as_ptr(), 0, REGISTRY_ACCESS, &mut key) != 0 {
            println!("ERROR: TrackIR DLL Location key not present");
            return "Error".into();
        }
        let mut size = 0u32;
        let null = std::ptr::null_mut::<c_void>().cast();
        let mut location = "Error".to_string();
        let path = c"Path".as_ptr().cast();
        if RegQueryValueExA(key, path, null, null, std::ptr::null_mut(), &mut size) == 0 {
            let mut data = vec![0u8; size as usize];
            if RegQueryValueExA(key, path, null, null, data.as_mut_ptr(), &mut size) == 0 {
                let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
                location = String::from_utf8_lossy(&data[..end]).into_owned();
            } else {
                println!("Error: reading TrackIR location key!");
            }
        }
        RegCloseKey(key);
        location
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_turn_and_clamp() {
        let data = TrackIrData { roll: 50.0, pitch: -100.0, yaw: 250.0, x: 20_000.0, y: -5000.0, z: f32::NAN, ..Default::default() };
        let (turn, place) = offsets(&data, 500.0);
        assert_eq!(turn, [0.2, 0.5, 0.1]);
        assert_eq!(place, [0.1, -5000.0 * 1.0e-5, -0.1]);
        assert_eq!(offsets(&TrackIrData { status: 1, ..data }, 500.0), ([0.0; 3], [0.0; 3]));
    }
}
