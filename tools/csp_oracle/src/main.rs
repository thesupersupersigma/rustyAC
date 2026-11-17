// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike oracle for Custom Shaders Patch (Task C1): can CSP's `dwrite.dll` be loaded next to an
//! `acs.exe` image that was mapped by hand, without the game ever running as a program?
//!
//! csp_oracle <ac root> <run folder> [cfg folder]
//!     `<ac root>` holds `acs.exe`, `acs.pdb`, `dwrite.dll` and `extension/` (the frozen build:
//!     `tools/csp/fakeac_csp.ps1`). `<run folder>` must not exist; it becomes the Documents
//!     folder, the temp folder and the place of every report file. `[cfg folder]` is copied in
//!     as the user's `cfg` folder (CSP's own settings are in its `extension` folder); the tool
//!     writes a small `race.ini` and `video.ini` when it has none. Run from the repo root to get
//!     function names (`re/index/functions.tsv`).
//!
//! `acs.exe` is mapped by car_oracle's loader. The DLL is loaded by Windows from the AC root, so
//! its TLS, unwind tables and place in the module list are as in the game; its entry point is
//! caught before it runs and every static import is pointed at a counting thunk, with the host's
//! own answer where one is needed: the main module is `acs.exe`, Documents is the run folder,
//! the clock is fixed, network DLLs do not load, and writes outside the run folder and
//! `<ac root>\cache` are refused. Exit codes: 6 a device or window was asked for, 7 a message
//! box, 8 CSP ended the process, 3 / 4 / 5 as car_oracle (crash, unbound game import, game exit).

#[path = "../../car_oracle/src/acs.rs"]
#[allow(dead_code)]
mod acs;

use acs::Acs;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

type Handle = *mut c_void;

extern "system" {
    fn GetFullPathNameW(name: *const u16, size: u32, buffer: *mut u16, file: *mut *mut u16) -> u32;
    fn GetModuleFileNameW(module: Handle, buffer: *mut u16, size: u32) -> u32;
    fn GetModuleFileNameA(module: Handle, buffer: *mut u8, size: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetModuleHandleA(name: *const u8) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut Handle) -> i32;
    fn GetModuleHandleExA(flags: u32, name: *const u8, module: *mut Handle) -> i32;
    fn LoadLibraryA(name: *const u8) -> Handle;
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn LoadLibraryExA(name: *const u8, file: Handle, flags: u32) -> Handle;
    fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle;
    fn CreateFileA(name: *const u8, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle;
    fn CreateDirectoryW(name: *const u16, security: *mut c_void) -> i32;
    fn DeleteFileW(name: *const u16) -> i32;
    fn RemoveDirectoryW(name: *const u16) -> i32;
    fn SetFileAttributesW(name: *const u16, attributes: u32) -> i32;
    fn MoveFileW(from: *const u16, to: *const u16) -> i32;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    fn ReplaceFileW(replaced: *const u16, replacement: *const u16, backup: *const u16, flags: u32, exclude: *mut c_void, reserved: *mut c_void) -> i32;
    fn CopyFileW(from: *const u16, to: *const u16, fail_if_exists: i32) -> i32;
    fn CreateHardLinkW(name: *const u16, existing: *const u16, security: *mut c_void) -> i32;
    fn WritePrivateProfileStringW(section: *const u16, key: *const u16, value: *const u16, file: *const u16) -> i32;
    fn CreateThread(security: *mut c_void, stack: usize, start: usize, parameter: *mut c_void, flags: u32, id: *mut u32) -> Handle;
    fn ResumeThread(thread: Handle) -> u32;
    fn GetThreadId(thread: Handle) -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentProcess() -> Handle;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut u32) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut u32) -> i32;
    fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut u16) -> i32;
    fn VirtualProtect(address: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    fn FlushInstructionCache(process: Handle, address: *const c_void, size: usize) -> i32;
    fn SetLastError(code: u32);
    fn ReadProcessMemory(process: Handle, address: usize, buffer: *mut u8, size: usize, read: *mut usize) -> i32;
    fn GetLastError() -> u32;
    fn Sleep(milliseconds: u32);
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemAlloc(size: usize) -> *mut c_void;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationThread(thread: Handle, class: u32, out: *mut c_void, size: u32, used: *mut u32) -> i32;
}

/// The Documents sub-folder CSP uses when `AC_CFG_PROGRAM_NAME` is not set.
const USER_FOLDER: &str = "Assetto Corsa";
const GAME_INI: &str = "system/cfg/assetto_corsa.ini";

/// One car, one practice session, on content every install has.
const RACE_INI: &str = "[HEADER]
VERSION=1

[RACE]
TRACK=magione
CONFIG_TRACK=
MODEL=bmw_m3_e30
MODEL_CONFIG=
SKIN=Black
CARS=1
AI_LEVEL=100
FIXED_SETUP=0
PENALTIES=0
DRIFT_MODE=0
RACE_LAPS=0

[CAR_0]
MODEL=-
MODEL_CONFIG=
SKIN=Black
DRIVER_NAME=Driver
NATIONALITY=
SETUP=

[SESSION_0]
NAME=Practice
TYPE=1
DURATION_MINUTES=30
SPAWN_SET=PIT

[GHOST_CAR]
RECORDING=0
PLAYING=0
SECONDS_ADVANTAGE=0
LOAD=0
FILE=

[REPLAY]
ACTIVE=0
FILENAME=

[REMOTE]
ACTIVE=0

[BENCHMARK]
ACTIVE=0

[LIGHTING]
SUN_ANGLE=16
TIME_MULT=1
CLOUD_SPEED=0.2

[TEMPERATURE]
AMBIENT=26
ROAD=36

[WEATHER]
NAME=3_clear

[DYNAMIC_TRACK]
PRESET=5
SESSION_START=100
RANDOMNESS=0
LAP_GAIN=1
SESSION_TRANSFER=100

[WIND]
SPEED_KMH_MIN=0
SPEED_KMH_MAX=0
DIRECTION_DEG=0

[OPTIONS]
USE_MPH=0
";

/// One window, no fullscreen, no headset.
const VIDEO_INI: &str = "[VIDEO]
FULLSCREEN=0
WIDTH=1280
HEIGHT=720
REFRESH=60
VSYNC=0
AASAMPLES=1
ANISOTROPIC=1
SHADOW_MAP_SIZE=1024
FPS_CAP_MS=0

[CAMERA]
MODE=DEFAULT

[EFFECTS]
MOTION_BLUR=0
FXAA=0

[POST_PROCESS]
ENABLED=0
QUALITY=0
FILTER=default
";
const INIREADER_CTOR: usize = 0x1_4023_40a0;
const INIREADER_HAS_SECTION: usize = 0x1_4023_70b0;
const INIREADER_SIZE: usize = 0x68;

struct Host {
    root: String,
    run: String,
    exe: String,
    command_line_wide: Vec<u16>,
    command_line_ansi: Vec<u8>,
}

static HOST: OnceLock<Host> = OnceLock::new();
static ACS_BASE: AtomicUsize = AtomicUsize::new(0);
static DLL_BASE: AtomicUsize = AtomicUsize::new(0);
static DLL_SIZE: AtomicUsize = AtomicUsize::new(0);
static ENTRY_BYTES: OnceLock<[u8; 12]> = OnceLock::new();
/// Thread ids in the order the host met them: 0 is the main thread, the rest `CreateThread` calls.
static THREADS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static IMPORTS: OnceLock<Vec<(String, &'static AtomicU64)>> = OnceLock::new();

fn host() -> &'static Host {
    HOST.get().expect("host answers are set before any CSP code runs")
}

fn tag_of(id: u32) -> String {
    match THREADS.lock().unwrap().iter().position(|known| *known == id) {
        Some(index) => format!("T{index}"),
        None => "T?".to_string(),
    }
}

/// One line of the run's record, tagged with the thread it happened on.
fn say(text: &str) {
    println!("{} {text}", tag_of(unsafe { GetCurrentThreadId() }));
}

fn stop(code: u32, why: &str) -> ! {
    say(&format!("STOP ({code}): {why}"));
    let _ = std::io::stdout().flush();
    // not exit(): CSP's detach code must not run
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::abort()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn from_wide(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut length = 0;
    while *text.add(length) != 0 {
        length += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
}

unsafe fn from_ansi(text: *const u8) -> String {
    if text.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(text.cast()).to_bytes().iter().map(|byte| *byte as char).collect()
}

fn full_path(path: &str) -> String {
    let mut buffer = [0u16; 2048];
    let length = unsafe { GetFullPathNameW(wide(path).as_ptr(), 2048, buffer.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    let full = String::from_utf16_lossy(&buffer[..length.min(2047)]);
    full.strip_prefix(r"\\?\").unwrap_or(&full).to_string()
}

/// The write guard: only the run folder and the AC root's own `cache` folder may change. The AC
/// root made by `fakeac_csp.ps1` has links into the real install, so nothing else under it may.
fn allowed(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let path = full_path(path).to_lowercase();
    [host().run.to_lowercase(), format!("{}\\cache", host().root.to_lowercase())]
        .iter()
        .any(|folder| path == *folder || path.starts_with(&format!("{folder}\\")))
}

fn guard(what: &str, path: &str) -> bool {
    let ok = allowed(path);
    say(&format!("{what} {path}: {}", if ok { "write allowed" } else { "REFUSED (outside the run folder)" }));
    if !ok {
        unsafe { SetLastError(5) };
    }
    ok
}

// ---- the host's answers ---------------------------------------------------------------------

unsafe fn copy_wide(text: &str, buffer: *mut u16, size: u32) -> u32 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let length = units.len().min((size as usize).saturating_sub(1));
    std::ptr::copy_nonoverlapping(units.as_ptr(), buffer, length);
    *buffer.add(length) = 0;
    length as u32
}

unsafe extern "system" fn get_module_file_name_w(module: Handle, buffer: *mut u16, size: u32) -> u32 {
    if module.is_null() {
        return copy_wide(&host().exe, buffer, size);
    }
    GetModuleFileNameW(module, buffer, size)
}

unsafe extern "system" fn get_module_file_name_a(module: Handle, buffer: *mut u8, size: u32) -> u32 {
    if module.is_null() {
        let length = host().exe.len().min((size as usize).saturating_sub(1));
        std::ptr::copy_nonoverlapping(host().exe.as_ptr(), buffer, length);
        *buffer.add(length) = 0;
        return length as u32;
    }
    GetModuleFileNameA(module, buffer, size)
}

unsafe extern "system" fn get_module_handle_w(name: *const u16) -> Handle {
    if name.is_null() {
        return ACS_BASE.load(Ordering::Relaxed) as Handle;
    }
    GetModuleHandleW(name)
}

unsafe extern "system" fn get_module_handle_a(name: *const u8) -> Handle {
    if name.is_null() {
        return ACS_BASE.load(Ordering::Relaxed) as Handle;
    }
    GetModuleHandleA(name)
}

// flag 4 = the "name" is an address inside the module
unsafe extern "system" fn get_module_handle_ex_w(flags: u32, name: *const u16, module: *mut Handle) -> i32 {
    if name.is_null() && flags & 4 == 0 {
        *module = ACS_BASE.load(Ordering::Relaxed) as Handle;
        return 1;
    }
    GetModuleHandleExW(flags, name, module)
}

unsafe extern "system" fn get_module_handle_ex_a(flags: u32, name: *const u8, module: *mut Handle) -> i32 {
    if name.is_null() && flags & 4 == 0 {
        *module = ACS_BASE.load(Ordering::Relaxed) as Handle;
        return 1;
    }
    GetModuleHandleExA(flags, name, module)
}

unsafe extern "system" fn query_full_process_image_name_w(_process: Handle, _flags: u32, buffer: *mut u16, size: *mut u32) -> i32 {
    *size = copy_wide(&host().exe, buffer, *size);
    1
}

extern "system" fn get_command_line_w() -> *const u16 {
    host().command_line_wide.as_ptr()
}

extern "system" fn get_command_line_a() -> *const u8 {
    host().command_line_ansi.as_ptr()
}

unsafe extern "system" fn sh_get_folder_path_w(_window: Handle, folder: i32, _token: Handle, _flags: u32, out: *mut u16) -> i32 {
    let id = folder & 0xff;
    let path = if id == 5 { format!("{}\\Documents", host().run) } else { format!("{}\\shell\\{id}", host().run) };
    let _ = std::fs::create_dir_all(&path);
    say(&format!("SHGetFolderPathW {id} -> {path}"));
    copy_wide(&path, out, 260);
    0
}

unsafe extern "system" fn sh_get_known_folder_path(id: *const u32, _flags: u32, _token: Handle, out: *mut *mut u16) -> i32 {
    let path = format!("{}\\known\\{:08x}", host().run, *id);
    let _ = std::fs::create_dir_all(&path);
    say(&format!("SHGetKnownFolderPath -> {path}"));
    let units = wide(&path);
    let memory = CoTaskMemAlloc(units.len() * 2) as *mut u16;
    std::ptr::copy_nonoverlapping(units.as_ptr(), memory, units.len());
    *out = memory;
    0
}

// The clock: one counter per thread, 0.1 ms per question, so a thread's answers do not depend on
// what the other threads asked. 1 January 2020 as car_oracle.
thread_local! {
    static TICKS: Cell<u64> = const { Cell::new(0) };
}

fn tick() -> u64 {
    TICKS.with(|ticks| {
        ticks.set(ticks.get() + 1_000);
        ticks.get()
    })
}

unsafe extern "system" fn query_performance_counter(out: *mut u64) -> i32 {
    *out = tick();
    1
}

unsafe extern "system" fn query_performance_frequency(out: *mut u64) -> i32 {
    *out = acs::CLOCK_FREQUENCY;
    1
}

extern "system" fn get_tick_count_64() -> u64 {
    tick() / 10_000
}

unsafe extern "system" fn get_system_time_as_file_time(out: *mut u64) {
    *out = 132_223_104_000_000_000 + tick();
}

unsafe extern "system" fn get_system_time(out: *mut u16) {
    FileTimeToSystemTime(&(132_223_104_000_000_000 + tick()), out);
}

fn refused_library(name: &str) -> bool {
    let name = name.to_lowercase();
    ["winhttp", "ws2_32", "wininet"].iter().any(|network| name.contains(network))
}

fn library(name: &str, load: impl FnOnce() -> Handle) -> Handle {
    if refused_library(name) {
        say(&format!("LoadLibrary {name}: REFUSED (network)"));
        unsafe { SetLastError(126) };
        return std::ptr::null_mut();
    }
    let module = load();
    say(&format!("LoadLibrary {name}: {}", if module.is_null() { "not found" } else { "loaded" }));
    module
}

unsafe extern "system" fn load_library_a(name: *const u8) -> Handle {
    library(&from_ansi(name), || LoadLibraryA(name))
}

unsafe extern "system" fn load_library_w(name: *const u16) -> Handle {
    library(&from_wide(name), || LoadLibraryW(name))
}

unsafe extern "system" fn load_library_ex_a(name: *const u8, file: Handle, flags: u32) -> Handle {
    library(&from_ansi(name), || LoadLibraryExA(name, file, flags))
}

unsafe extern "system" fn load_library_ex_w(name: *const u16, file: Handle, flags: u32) -> Handle {
    library(&from_wide(name), || LoadLibraryExW(name, file, flags))
}

unsafe extern "system" fn create_thread(security: *mut c_void, stack: usize, start: usize, parameter: *mut c_void, flags: u32, id: *mut u32) -> Handle {
    // started suspended, so the thread has its number before it can print
    let thread = CreateThread(security, stack, start, parameter, flags | 4, id);
    if !thread.is_null() {
        let index = {
            let mut threads = THREADS.lock().unwrap();
            threads.push(GetThreadId(thread));
            threads.len() - 1
        };
        say(&format!("CreateThread: T{index} starts at {}", place(start)));
        if flags & 4 == 0 {
            ResumeThread(thread);
        }
    }
    thread
}

macro_rules! stops {
    ($($function:ident $name:literal $code:literal),*) => {
        $(extern "system" fn $function() -> usize {
            stop($code, concat!("CSP called ", $name))
        })*
    };
}

stops!(
    d3d11_create_device "D3D11CreateDevice" 6,
    create_dxgi_factory "CreateDXGIFactory" 6,
    create_dxgi_factory_1 "CreateDXGIFactory1" 6,
    exit_process "ExitProcess" 8,
    terminate_process "TerminateProcess" 8
);

/// CSP's hot-key thread asks for a message-only window at start-up: refused, so the thread ends and
/// no system-wide hot key is taken on this PC. A window that would show is a stop.
#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn create_window_ex_w(_ex: u32, class: *const u16, _name: *const u16, _style: u32, _x: i32, _y: i32, _w: i32, _h: i32, parent: isize, _menu: Handle, _instance: Handle, _param: Handle) -> Handle {
    if parent != -3 {
        stop(6, "CSP called CreateWindowExW");
    }
    say(&format!("CreateWindowExW message-only window {}: REFUSED", from_wide(class)));
    SetLastError(5);
    std::ptr::null_mut()
}

unsafe extern "system" fn message_box_w(_window: Handle, text: *const u16, caption: *const u16, _kind: u32) -> i32 {
    stop(7, &format!("CSP opened a message box: {} / {}", from_wide(caption), from_wide(text)))
}

unsafe extern "system" fn message_box_a(_window: Handle, text: *const u8, caption: *const u8, _kind: u32) -> i32 {
    stop(7, &format!("CSP opened a message box: {} / {}", from_ansi(caption), from_ansi(text)))
}

macro_rules! refusals {
    ($($function:ident $name:literal $answer:literal),*) => {
        $(extern "system" fn $function() -> usize {
            say(concat!($name, ": REFUSED"));
            unsafe { SetLastError(5) };
            $answer
        })*
    };
}

// ShellExecuteW answers 32 or less for a failure, SHFileOperationW non-zero
refusals!(create_process_w "CreateProcessW" 0, create_process_a "CreateProcessA" 0, shell_execute_w "ShellExecuteW" 5, sh_file_operation_w "SHFileOperationW" 5);

/// Any access right that can change a file, its attributes or its security.
const WRITE_ACCESS: u32 = 0x4000_0000 | 0x1000_0000 | 0x0200_0000 | 0x000d_0000 | 0x0116;

fn opens_for_writing(access: u32, disposition: u32, flags: u32) -> bool {
    // 3 = OPEN_EXISTING, the only disposition that never creates or truncates; 0x0400_0000 = delete on close
    access & WRITE_ACCESS != 0 || disposition != 3 || flags & 0x0400_0000 != 0
}

unsafe extern "system" fn create_file_w(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle {
    if opens_for_writing(access, disposition, flags) && !guard("CreateFileW", &from_wide(name)) {
        return usize::MAX as Handle;
    }
    CreateFileW(name, access, share, security, disposition, flags, template)
}

unsafe extern "system" fn create_file_a(name: *const u8, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle {
    if opens_for_writing(access, disposition, flags) && !guard("CreateFileA", &from_ansi(name)) {
        return usize::MAX as Handle;
    }
    CreateFileA(name, access, share, security, disposition, flags, template)
}

unsafe extern "system" fn create_directory_w(name: *const u16, security: *mut c_void) -> i32 {
    if !guard("CreateDirectoryW", &from_wide(name)) {
        return 0;
    }
    CreateDirectoryW(name, security)
}

unsafe extern "system" fn delete_file_w(name: *const u16) -> i32 {
    if !guard("DeleteFileW", &from_wide(name)) {
        return 0;
    }
    DeleteFileW(name)
}

unsafe extern "system" fn remove_directory_w(name: *const u16) -> i32 {
    if !guard("RemoveDirectoryW", &from_wide(name)) {
        return 0;
    }
    RemoveDirectoryW(name)
}

unsafe extern "system" fn set_file_attributes_w(name: *const u16, attributes: u32) -> i32 {
    if !guard("SetFileAttributesW", &from_wide(name)) {
        return 0;
    }
    SetFileAttributesW(name, attributes)
}

unsafe extern "system" fn move_file_w(from: *const u16, to: *const u16) -> i32 {
    if !guard("MoveFileW from", &from_wide(from)) || !guard("MoveFileW to", &from_wide(to)) {
        return 0;
    }
    MoveFileW(from, to)
}

unsafe extern "system" fn move_file_ex_w(from: *const u16, to: *const u16, flags: u32) -> i32 {
    if !guard("MoveFileExW from", &from_wide(from)) || (!to.is_null() && !guard("MoveFileExW to", &from_wide(to))) {
        return 0;
    }
    MoveFileExW(from, to, flags)
}

unsafe extern "system" fn replace_file_w(replaced: *const u16, replacement: *const u16, backup: *const u16, flags: u32, exclude: *mut c_void, reserved: *mut c_void) -> i32 {
    if !guard("ReplaceFileW replaced", &from_wide(replaced))
        || !guard("ReplaceFileW replacement", &from_wide(replacement))
        || (!backup.is_null() && !guard("ReplaceFileW backup", &from_wide(backup)))
    {
        return 0;
    }
    ReplaceFileW(replaced, replacement, backup, flags, exclude, reserved)
}

unsafe extern "system" fn copy_file_w(from: *const u16, to: *const u16, fail_if_exists: i32) -> i32 {
    if !guard("CopyFileW to", &from_wide(to)) {
        return 0;
    }
    CopyFileW(from, to, fail_if_exists)
}

unsafe extern "system" fn create_hard_link_w(name: *const u16, existing: *const u16, security: *mut c_void) -> i32 {
    if !guard("CreateHardLinkW", &from_wide(name)) {
        return 0;
    }
    CreateHardLinkW(name, existing, security)
}

unsafe extern "system" fn write_private_profile_string_w(section: *const u16, key: *const u16, value: *const u16, file: *const u16) -> i32 {
    let path = from_wide(file);
    // a bare file name means the Windows folder to this function, not the current one
    if !path.contains('\\') && !path.contains('/') {
        say(&format!("WritePrivateProfileStringW {path}: REFUSED (no folder)"));
        SetLastError(5);
        return 0;
    }
    if !guard("WritePrivateProfileStringW", &path) {
        return 0;
    }
    WritePrivateProfileStringW(section, key, value, file)
}

fn answer_for(name: &str) -> Option<usize> {
    Some(match name {
        "GetModuleFileNameW" => get_module_file_name_w as *const () as usize,
        "GetModuleFileNameA" => get_module_file_name_a as *const () as usize,
        "GetModuleHandleW" => get_module_handle_w as *const () as usize,
        "GetModuleHandleA" => get_module_handle_a as *const () as usize,
        "GetModuleHandleExW" => get_module_handle_ex_w as *const () as usize,
        "GetModuleHandleExA" => get_module_handle_ex_a as *const () as usize,
        "QueryFullProcessImageNameW" => query_full_process_image_name_w as *const () as usize,
        "GetCommandLineW" => get_command_line_w as *const () as usize,
        "GetCommandLineA" => get_command_line_a as *const () as usize,
        "SHGetFolderPathW" => sh_get_folder_path_w as *const () as usize,
        "SHGetKnownFolderPath" => sh_get_known_folder_path as *const () as usize,
        "QueryPerformanceCounter" => query_performance_counter as *const () as usize,
        "QueryPerformanceFrequency" => query_performance_frequency as *const () as usize,
        "GetTickCount64" => get_tick_count_64 as *const () as usize,
        "GetSystemTimeAsFileTime" => get_system_time_as_file_time as *const () as usize,
        // the fixed clock is its own time zone
        "GetSystemTime" | "GetLocalTime" => get_system_time as *const () as usize,
        "LoadLibraryA" => load_library_a as *const () as usize,
        "LoadLibraryW" => load_library_w as *const () as usize,
        "LoadLibraryExA" => load_library_ex_a as *const () as usize,
        "LoadLibraryExW" => load_library_ex_w as *const () as usize,
        "CreateThread" => create_thread as *const () as usize,
        "D3D11CreateDevice" => d3d11_create_device as *const () as usize,
        "CreateDXGIFactory" => create_dxgi_factory as *const () as usize,
        "CreateDXGIFactory1" => create_dxgi_factory_1 as *const () as usize,
        "CreateWindowExW" => create_window_ex_w as *const () as usize,
        "ExitProcess" => exit_process as *const () as usize,
        "TerminateProcess" => terminate_process as *const () as usize,
        "MessageBoxW" => message_box_w as *const () as usize,
        "MessageBoxA" => message_box_a as *const () as usize,
        "CreateProcessW" => create_process_w as *const () as usize,
        "CreateProcessA" => create_process_a as *const () as usize,
        "ShellExecuteW" => shell_execute_w as *const () as usize,
        "SHFileOperationW" => sh_file_operation_w as *const () as usize,
        "CreateFileW" => create_file_w as *const () as usize,
        "CreateFileA" => create_file_a as *const () as usize,
        "CreateDirectoryW" => create_directory_w as *const () as usize,
        "DeleteFileW" => delete_file_w as *const () as usize,
        "RemoveDirectoryW" => remove_directory_w as *const () as usize,
        "SetFileAttributesW" => set_file_attributes_w as *const () as usize,
        "MoveFileW" => move_file_w as *const () as usize,
        "MoveFileExW" => move_file_ex_w as *const () as usize,
        "ReplaceFileW" => replace_file_w as *const () as usize,
        "CopyFileW" => copy_file_w as *const () as usize,
        "CreateHardLinkW" => create_hard_link_w as *const () as usize,
        "WritePrivateProfileStringW" => write_private_profile_string_w as *const () as usize,
        _ => return None,
    })
}

// ---- getting in before the DLL's first instruction ------------------------------------------

unsafe fn read<T: Copy>(address: usize) -> T {
    std::ptr::read_unaligned(address as *const T)
}

unsafe fn poke(address: usize, bytes: &[u8]) {
    let mut old = 0;
    VirtualProtect(address as *mut c_void, bytes.len(), 0x40, &mut old);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
    VirtualProtect(address as *mut c_void, bytes.len(), old, &mut old);
    FlushInstructionCache(GetCurrentProcess(), address as *const c_void, bytes.len());
}

fn mxcsr() -> u32 {
    let mut value = 0u32;
    unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut value as *mut u32, options(nostack)) };
    value
}

unsafe fn entry_of(base: usize) -> usize {
    base + read::<u32>(base + read::<u32>(base + 0x3c) as usize + 24 + 16) as usize
}

/// Windows calls this when a DLL has been mapped, before any of its code runs. The entry point
/// of CSP's DLL gets a jump to `entry_hook`.
unsafe extern "system" fn dll_mapped(reason: u32, data: *const usize, _context: *mut c_void) {
    // data: flags, full name, base name, base, size; a name is { u16 bytes, u16 room, pointer }
    let name = *data.add(1) as *const usize;
    let bytes = *(name as *const u16) as usize;
    let full = String::from_utf16_lossy(std::slice::from_raw_parts(*name.add(1) as *const u16, bytes / 2)).to_lowercase();
    // the only other file of this name is Windows' own
    if reason != 1 || !full.ends_with("\\dwrite.dll") || full.contains("\\windows\\") {
        return;
    }
    let base = *data.add(3);
    DLL_BASE.store(base, Ordering::Relaxed);
    DLL_SIZE.store(*data.add(4) & 0xffff_ffff, Ordering::Relaxed);
    let _ = ENTRY_BYTES.set(read(entry_of(base)));
    let mut jump = vec![0x48, 0xb8];
    jump.extend((entry_hook as *const () as usize).to_le_bytes());
    jump.extend([0xff, 0xe0]);
    poke(entry_of(base), &jump);
}

unsafe extern "system" fn entry_hook(module: Handle, reason: u32, reserved: *mut c_void) -> i32 {
    let base = DLL_BASE.load(Ordering::Relaxed);
    poke(entry_of(base), ENTRY_BYTES.get().unwrap());
    hook_imports(base);
    say(&format!("CSP entry point, reason {reason}, MXCSR {:#06x}", mxcsr()));
    let real: extern "system" fn(Handle, u32, *mut c_void) -> i32 = std::mem::transmute(entry_of(base));
    let answer = real(module, reason, reserved);
    say(&format!("CSP entry point returned {answer}, MXCSR {:#06x}", mxcsr()));
    answer
}

/// Points every static import of the DLL at a thunk that counts the call and jumps on, to the
/// host's answer when there is one, else to the function Windows bound.
unsafe fn hook_imports(base: usize) {
    let optional = base + read::<u32>(base + 0x3c) as usize + 24;
    let mut descriptor = base + read::<u32>(optional + 112 + 8) as usize;
    let mut slots = Vec::new();
    while read::<u32>(descriptor + 12) != 0 {
        let mut lookup = base + read::<u32>(descriptor) as usize;
        let mut slot = base + read::<u32>(descriptor + 16) as usize;
        while read::<u64>(lookup) != 0 {
            let entry = read::<u64>(lookup);
            // bit 63: by number, not by name
            let name = if entry >> 63 != 0 { format!("#{}", entry & 0xffff) } else { from_ansi((base + entry as usize + 2) as *const u8) };
            slots.push((name, slot));
            lookup += 8;
            slot += 8;
        }
        descriptor += 20;
    }
    let counters: &'static [AtomicU64] = (0..slots.len()).map(|_| AtomicU64::new(0)).collect::<Vec<_>>().leak();
    let thunks = VirtualAlloc(std::ptr::null_mut(), slots.len() * 32, 0x3000, 0x40) as usize;
    assert!(thunks != 0, "no memory for the import thunks");
    let mut imports = Vec::new();
    for (index, (name, slot)) in slots.into_iter().enumerate() {
        let target = answer_for(&name).unwrap_or(read::<usize>(slot));
        // mov r11, counter; lock inc qword [r11]; mov r11, target; jmp r11
        let mut code = vec![0x49, 0xbb];
        code.extend((&counters[index] as *const AtomicU64 as usize).to_le_bytes());
        code.extend([0xf0, 0x49, 0xff, 0x03, 0x49, 0xbb]);
        code.extend(target.to_le_bytes());
        code.extend([0x41, 0xff, 0xe3]);
        let thunk = thunks + index * 32;
        std::ptr::copy_nonoverlapping(code.as_ptr(), thunk as *mut u8, code.len());
        poke(slot, &thunk.to_le_bytes());
        imports.push((name, &counters[index]));
    }
    let _ = IMPORTS.set(imports);
}

// ---- what the run reports -------------------------------------------------------------------

/// `module+0xoffset` for an address inside a loaded module.
fn place(address: usize) -> String {
    let mut module: Handle = std::ptr::null_mut();
    let mut name = [0u16; 512];
    // 4 = the name is an address, 2 = do not hold the module
    if unsafe { GetModuleHandleExW(4 | 2, address as *const u16, &mut module) } == 0 {
        return "outside every module".to_string();
    }
    let length = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 512) } as usize;
    let path = String::from_utf16_lossy(&name[..length]);
    format!("{}+{:#x}", path.rsplit('\\').next().unwrap_or("?").to_lowercase(), address - module as usize)
}

/// Says where a fault happened and which code addresses are on the stack above it, then lets
/// the loader's own handler report the fault and end the run.
unsafe extern "system" fn fault_places(pointers: *const *const usize) -> i32 {
    static ONCE: AtomicUsize = AtomicUsize::new(0);
    let record = *pointers as *const u32;
    if *record >> 28 != 0xc || ONCE.swap(1, Ordering::Relaxed) != 0 {
        return 0;
    }
    // the processor state: rsp at 0x98, rip at 0xf8
    let context = *pointers.add(1) as usize;
    let (rsp, rip) = (read::<usize>(context + 0x98), read::<usize>(context + 0xf8));
    say(&format!("fault {:#x} at {}", *record, place(rip)));
    let on_stack: Vec<String> = (0..96).map(|slot| place(read::<usize>(rsp + slot * 8))).filter(|place| place.contains('+')).take(16).collect();
    say(&format!("code addresses on the stack: {}", on_stack.join(", ")));
    // rcx, rdx, r8: when one points at text, it is usually the message CSP was about to print
    for register in [0x80, 0x88, 0xb8] {
        let mut text = [0u8; 400];
        ReadProcessMemory(GetCurrentProcess(), read::<usize>(context + register), text.as_mut_ptr(), 399, std::ptr::null_mut());
        let length = text.iter().position(|byte| *byte < 0x20 && *byte != 10).unwrap_or(0);
        if length >= 8 {
            say(&format!("text at a register: {}", String::from_utf8_lossy(&text[..length]).trim_end()));
        }
    }
    0
}

struct Section {
    name: String,
    start: usize,
    executable: bool,
    bytes: Vec<u8>,
}

/// A copy of every section of the mapped `acs.exe`.
unsafe fn snapshot(base: usize) -> Vec<Section> {
    let pe = base + read::<u32>(base + 0x3c) as usize;
    let first = pe + 24 + read::<u16>(pe + 20) as usize;
    (0..read::<u16>(pe + 6) as usize)
        .map(|index| {
            let header = first + index * 40;
            let name: [u8; 8] = read(header);
            let start = base + read::<u32>(header + 12) as usize;
            Section {
                name: String::from_utf8_lossy(&name).trim_end_matches('\0').to_string(),
                start,
                executable: read::<u32>(header + 36) & 0x2000_0000 != 0,
                bytes: std::slice::from_raw_parts(start as *const u8, read::<u32>(header + 8) as usize).to_vec(),
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ")
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3))
}

/// Everything the run can say at one moment: what changed in `acs.exe` since `before`, how far
/// CSP's log got, the threads, the imports CSP has called.
unsafe fn checkpoint(label: &str, before: &[Section], functions: &[(usize, usize, String)]) {
    let base = ACS_BASE.load(Ordering::Relaxed);
    // function address (or the changed address when no function is known) -> changed bytes
    let mut patched: BTreeMap<usize, usize> = BTreeMap::new();
    for section in before {
        let now = std::slice::from_raw_parts(section.start as *const u8, section.bytes.len());
        if !section.executable {
            // counted in 8-byte words: how many bytes of a pointer differ is chance
            let words = now.chunks(8).zip(section.bytes.chunks(8)).filter(|(new, old)| new != old).count();
            if words != 0 {
                println!("{label}: section {}: {words} words changed", section.name);
            }
            continue;
        }
        for index in (0..now.len()).filter(|index| now[*index] != section.bytes[*index]) {
            let address = acs::GHIDRA_BASE + section.start - base + index;
            let next = functions.partition_point(|function| function.0 <= address);
            // a change in the padding above a function belongs to that function's hook
            let owner = match functions.get(next.wrapping_sub(1)) {
                Some(function) if address < function.0 + function.1 => function.0,
                _ => functions.get(next).map_or(address, |function| function.0),
            };
            *patched.entry(owner).or_default() += 1;
        }
    }
    let old = |address: usize| {
        let mapped = address - acs::GHIDRA_BASE + base;
        let section = before.iter().find(|section| (section.start..section.start + section.bytes.len()).contains(&mapped));
        section.map_or(String::new(), |section| hex(&section.bytes[mapped - section.start..][..16]))
    };
    let name = |address: usize| functions.binary_search_by_key(&address, |function| function.0).map_or("?", |index| functions[index].2.as_str());
    let line = |address: usize| {
        let now = std::slice::from_raw_parts((address - acs::GHIDRA_BASE + base) as *const u8, 16);
        format!("{address:#x} {} | {} bytes | {} -> {}", name(address), patched[&address], old(address), hex(now))
    };
    let list: String = patched.keys().map(|address| line(*address) + "\n").collect();
    let _ = std::fs::write(format!("{}\\patched_{label}.txt", host().run), &list);
    let names: String = patched.keys().map(|address| format!("{address:#x} {}\n", name(*address))).collect();
    println!("{label}: {} acs.exe functions patched, name list hash {:016x}", patched.len(), fnv(&names));
    let shown = [patched.keys().next(), patched.get_key_value(&INIREADER_HAS_SECTION).map(|pair| pair.0), patched.keys().next_back()];
    for address in shown.into_iter().flatten() {
        println!("{label}:   {}", line(*address));
    }

    let log = std::fs::read(format!("{}\\Documents\\{USER_FOLDER}\\logs\\custom_shaders_patch.log", host().run)).unwrap_or_default();
    let log = String::from_utf8_lossy(&log);
    match log.lines().find(|line| line.contains("functions were found")) {
        Some(found) => println!("{label}: CSP log ({} lines): {found}", log.lines().count()),
        None => println!("{label}: CSP log ({} lines) has no \"functions were found\" line", log.lines().count()),
    }

    let snapshot = CreateToolhelp32Snapshot(4, 0);
    // entry: size, usage, thread id, process id, priority, delta, flags
    let mut entry = [28u32, 0, 0, 0, 0, 0, 0];
    let mut threads = Vec::new();
    let mut more = Thread32First(snapshot, entry.as_mut_ptr());
    while more != 0 {
        if entry[3] == GetCurrentProcessId() {
            let thread = OpenThread(0x40, 0, entry[2]);
            let mut start = 0usize;
            // 9 = the address the thread was started at
            NtQueryInformationThread(thread, 9, &mut start as *mut usize as *mut c_void, 8, std::ptr::null_mut());
            CloseHandle(thread);
            threads.push(format!("{} started at {}", tag_of(entry[2]), place(start)));
        }
        more = Thread32Next(snapshot, entry.as_mut_ptr());
    }
    CloseHandle(snapshot);
    threads.sort();
    println!("{label}: {} threads", threads.len());
    for thread in threads {
        println!("{label}:   {thread}");
    }

    if let Some(imports) = IMPORTS.get() {
        let called: Vec<String> = imports.iter().filter(|import| import.1.load(Ordering::Relaxed) != 0).map(|import| format!("{} {}\n", import.0, import.1.load(Ordering::Relaxed))).collect();
        println!("{label}: CSP has called {} of its {} static imports", called.len(), imports.len());
        let _ = std::fs::write(format!("{}\\imports_{label}.txt", host().run), called.concat());
    }
    println!("{label}: MXCSR {:#06x}", mxcsr());
}

// ---- the game side ---------------------------------------------------------------------------

/// A `std::wstring` as the game's compiler lays it out (32 bytes: text or pointer, length, room).
unsafe fn wstring(acs: &Acs, text: &str) -> *mut u8 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let string = acs.alloc(32);
    let data = if units.len() < 8 {
        string
    } else {
        let data = acs.alloc((units.len() + 1) * 2);
        *(string as *mut *mut u8) = data;
        data
    };
    std::ptr::copy_nonoverlapping(units.as_ptr(), data as *mut u16, units.len());
    *(string.add(0x10) as *mut usize) = units.len();
    *(string.add(0x18) as *mut usize) = units.len().max(7);
    string
}

/// The game's `INIReader` built on a file by the game's own constructor.
unsafe fn ini_reader(acs: &Acs, file: &str) -> *mut u8 {
    let constructor: extern "system" fn(*mut u8, *const u8) -> *mut u8 = std::mem::transmute(acs.va(INIREADER_CTOR));
    constructor(acs.alloc(INIREADER_SIZE), wstring(acs, file))
}

unsafe fn has_section(acs: &Acs, reader: *mut u8, section: &str) -> bool {
    let function: extern "system" fn(*mut u8, *const u8) -> u8 = std::mem::transmute(acs.va(INIREADER_HAS_SECTION));
    function(reader, wstring(acs, section)) & 1 != 0
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 3 && arguments.len() != 4 {
        eprintln!("usage: csp_oracle <ac root> <run folder> [cfg folder]");
        std::process::exit(2);
    }
    let root = full_path(&arguments[1]);
    let run = full_path(&arguments[2]);
    if std::path::Path::new(&run).exists() {
        eprintln!("the run folder {run} exists already: every run gets a new one");
        std::process::exit(2);
    }
    // the functions index: address, size, library, object file, name
    let mut functions: Vec<(usize, usize, String)> = std::fs::read_to_string("re/index/functions.tsv")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            let name = cells.get(4)?.split('(').next()?.to_string();
            Some((usize::from_str_radix(cells[0], 16).ok()?, cells[1].parse().ok()?, name))
        })
        .collect();
    functions.sort();
    functions.dedup_by_key(|function| function.0);

    let exe = format!("{root}\\acs.exe");
    for folder in ["local", "roaming", "temp"] {
        std::fs::create_dir_all(format!("{run}\\{folder}")).expect("the run folder can be made");
    }
    for folder in ["cfg", "logs"] {
        std::fs::create_dir_all(format!("{run}\\Documents\\{USER_FOLDER}\\{folder}")).expect("the run folder can be made");
    }
    for (name, value) in [("LOCALAPPDATA", "local"), ("APPDATA", "roaming"), ("TEMP", "temp"), ("TMP", "temp")] {
        std::env::set_var(name, format!("{run}\\{value}"));
    }
    std::env::set_var("USERPROFILE", &run);
    // CSP reads the race configuration before it looks for the game's functions, and stops without one
    let cfg = format!("{run}\\Documents\\{USER_FOLDER}\\cfg");
    if let Some(from) = arguments.get(3) {
        copy_tree(std::path::Path::new(from), std::path::Path::new(&cfg)).expect("the cfg folder can be copied");
    }
    for (name, text) in [("race.ini", RACE_INI), ("video.ini", VIDEO_INI)] {
        if !std::path::Path::new(&format!("{cfg}\\{name}")).exists() {
            std::fs::write(format!("{cfg}\\{name}"), text.replace('\n', "\r\n")).unwrap();
        }
    }
    for name in ["AC_CFG_PROGRAM_NAME", "AC_CFG_RACE_INI", "_NT_SYMBOL_PATH", "_NT_ALT_SYMBOL_PATH"] {
        std::env::remove_var(name);
    }
    let command_line = format!("\"{exe}\"");
    let _ = HOST.set(Host { command_line_wide: wide(&command_line), command_line_ansi: (command_line + "\0").into_bytes(), root: root.clone(), run: run.clone(), exe: exe.clone() });
    THREADS.lock().unwrap().push(unsafe { GetCurrentThreadId() });

    // the call through: a controller section whose wing section is missing, and one whose wing is there
    let acs = match Acs::load(std::path::Path::new(&exe)) {
        Ok(acs) => acs,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    acs.unbuffer_game_stdout();
    let base = acs.va(acs::GHIDRA_BASE);
    ACS_BASE.store(base, Ordering::Relaxed);
    println!("acs.exe mapped at {base:#x}, {} function names, MXCSR {:#06x}", functions.len(), mxcsr());

    unsafe {
        // CSP's INIReader::hasSection hides [THREADING] of exactly this file name; the game reads it
        // relative to the AC root, where CSP moves the current directory anyway
        std::env::set_current_dir(&root).expect("the AC root can be entered");
        let ask = |when: &str, reader: *mut u8| {
            println!(
                "{when}: {GAME_INI}: ready {}, hasSection(THREADING) = {}, hasSection(PHYSICS_THREADING) = {}",
                *reader.add(8),
                has_section(&acs, reader, "THREADING"),
                has_section(&acs, reader, "PHYSICS_THREADING")
            );
        };
        let build = || ini_reader(&acs, GAME_INI);
        let reader = build();
        ask("Vanilla", reader);
        let before = snapshot(base);

        // asked for by name: the loader declares it with its own handler type
        let add_handler: extern "system" fn(u32, usize) -> *mut c_void = std::mem::transmute(GetProcAddress(GetModuleHandleA(c"kernel32.dll".as_ptr().cast()), c"AddVectoredExceptionHandler".as_ptr().cast()));
        add_handler(1, fault_places as *const () as usize);
        let mut cookie = std::ptr::null_mut();
        // not in the import library, so asked for by name
        let register = GetProcAddress(GetModuleHandleA(c"ntdll.dll".as_ptr().cast()), c"LdrRegisterDllNotification".as_ptr().cast());
        assert!(!register.is_null(), "ntdll has no LdrRegisterDllNotification");
        let register: extern "system" fn(u32, usize, *mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(register);
        let status = register(0, dll_mapped as *const () as usize, std::ptr::null_mut(), &mut cookie);
        assert!(status == 0, "LdrRegisterDllNotification failed: {status:#x}");
        // acs.exe's own DLLs: in the game they are loaded before CSP, which looks functions up in them
        for name in [format!("{root}\\fmod64.dll"), format!("{root}\\fmodstudio64.dll"), format!("{root}\\steam_api64.dll"), "wsock32.dll".into(), "d3dx11_43.dll".into(), "xinput1_3.dll".into()] {
            if LoadLibraryW(wide(&name).as_ptr()).is_null() {
                stop(2, &format!("Windows did not load {name}: error {}", GetLastError()));
            }
        }
        let dll = LoadLibraryW(wide(&format!("{root}\\dwrite.dll")).as_ptr());
        if IMPORTS.get().is_none() {
            stop(2, "CSP's entry point was not caught: its imports were not under the host's control");
        }
        if dll.is_null() {
            stop(2, &format!("Windows did not load {root}\\dwrite.dll: error {}", GetLastError()));
        }
        println!("CSP's DLL loaded at {:#x}, size {:#x}", dll as usize, DLL_SIZE.load(Ordering::Relaxed));
        checkpoint("loaded", &before, &functions);

        ask("with CSP, reader built before CSP", reader);
        ask("with CSP, reader built now", build());
        checkpoint("called", &before, &functions);

        // real time for CSP's own threads, to see what they do when left alone
        Sleep(1500);
        checkpoint("settled", &before, &functions);
    }
    stop(0, "end of the run");
}
