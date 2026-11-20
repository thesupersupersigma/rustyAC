// SPDX-License-Identifier: GPL-3.0-or-later

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
// car_oracle's real track (Task C5b): the game's Track, SurfacesManager and collision meshes
#[path = "../../car_oracle/src/track.rs"]
#[allow(dead_code)]
mod track;
// Task C5c: rustyac-ode's f64 functions against CSP's ODE (--ode)
mod btl;
mod island;
mod ode;
mod reduce;
mod solver;
mod space;
#[path = "../../car_oracle/src/setup_screen.rs"]
mod setup_screen;
/// What track.rs takes from car_oracle's game.rs.
mod game {
    pub unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
        base.add(offset).cast::<T>().write_unaligned(value);
    }
    pub unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
        base.add(offset).cast::<T>().read_unaligned()
    }
}

/// The repository (acs.rs reads the function table from it).
pub fn repo_root() -> std::path::PathBuf {
    // RUSTYAC_REPO: a copy of the repository the exe was not built in (the proof farm, docs/proof_farm.md)
    std::env::var_os("RUSTYAC_REPO").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(std::path::Path::parent).expect("the repository folder").to_path_buf()
    })
}

use acs::Acs;
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
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
const INIREADER_GET_INT: usize = 0x1_4023_5c70;
const INIREADER_GET_FLOAT: usize = 0x1_4023_58c0;

// the car load of car_oracle (tools/car_oracle/src/game.rs, new_engine and World::build's flat track)
const CRASH_GUARD_CTOR: usize = 0x1_4021_8800; // CrashGuard::CrashGuard(const wstring&), wWinMain's first object
const CRASH_GUARD_SIZE: usize = 0x10;
const VA_PHYSICS_ENGINE_CTOR: usize = 0x1_4026_2430;
const VA_TRACK_CTOR: usize = 0x1_4027_7100;
const VA_TRACK_INIT_AI_SPLINE: usize = 0x1_4027_82a0;
const VA_CAR_CTOR: usize = 0x1_4026_bf00;
const VA_CAR_INIT_COLLIDER_MESH: usize = 0x1_4027_3b20;
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588;
const VA_IS_USING_QPT: usize = 0x1_4151_d140;
const VA_TIMER_START: usize = 0x1_4155_a590;
const VA_TIMER_FREQUENCY: usize = 0x1_4155_a598;
const VA_IS_TEST_MODE: usize = 0x1_4155_a770;
const PE_SIZE: usize = 0x278;
const PE_PHYSICS_TIME: usize = 0x20;
const PE_GAME_TIME: usize = 0xf8;
const PE_AMBIENT_TEMPERATURE: usize = 0x100;
const PE_ROAD_TEMPERATURE: usize = 0x104;
const TRACK_SIZE: usize = 0x148;
const CAR_SIZE: usize = 0x3ea0;
const CAR_PHYSICS_GUID: usize = 0x3238;

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
// CSP's job pools (worker start dwrite+0x572660); +696 counts the jobs queued or running
static POOLS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
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

/// The game's physics thread, for CSP: CSP tells it from its main thread (the one that loaded it), and
/// queues calls made on the main thread for it (docs/csp/core.md, C5b). Runs one job at a time while
/// the caller waits.
struct PhysicsThread {
    jobs: std::sync::mpsc::Sender<Job>,
    done: std::sync::mpsc::Receiver<()>,
}

struct Job(Box<dyn FnOnce()>);
// the game's memory is used by one thread at a time: the caller waits for each job
unsafe impl Send for Job {}

impl PhysicsThread {
    fn start() -> PhysicsThread {
        let (jobs, receive) = std::sync::mpsc::channel::<Job>();
        let (finished, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for job in receive {
                (job.0)();
                finished.send(()).unwrap();
            }
        });
        PhysicsThread { jobs, done }
    }

    fn run<'a>(&self, job: impl FnOnce() + 'a) {
        let job: Box<dyn FnOnce() + 'a> = Box::new(job);
        // the borrows outlive the job: run waits until it has finished, as a scoped thread does
        let job: Box<dyn FnOnce()> = unsafe { std::mem::transmute(job) };
        self.jobs.send(Job(job)).unwrap();
        self.done.recv().expect("the physics thread finishes its job");
    }
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

// CSP's static CRT looks RtlGenRandom up by name (SystemFunction036) for std::random_device, which
// seeds its per-thread xoshiro (RVA 0x13fae20; e.g. the random wheel start angle of its Tyre::init
// detour): it gets a fixed byte stream per thread, like the clock (rule 18)
thread_local! {
    static RANDOM: Cell<u64> = const { Cell::new(0) };
}

unsafe extern "system" fn system_function_036(buffer: *mut u8, length: u32) -> u8 {
    for i in 0..length as usize {
        let n = RANDOM.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        *buffer.add(i) = (n.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 56) as u8;
    }
    1
}

unsafe extern "system" fn get_proc_address(module: Handle, name: *const u8) -> *mut c_void {
    if name as usize > 0xffff && from_ansi(name) == "SystemFunction036" {
        return system_function_036 as *const () as *mut c_void;
    }
    GetProcAddress(module, name)
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
        if place(start).ends_with("dwrite.dll+0x572660") {
            // a worker of CSP's job pool: its parameter starts with the pool
            let pool = *(parameter as *const usize);
            let mut pools = POOLS.lock().unwrap();
            if !pools.contains(&pool) {
                pools.push(pool);
            }
        }
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
        "GetProcAddress" => get_proc_address as *const () as usize,
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

/// The call log of one game function: its entry, then each jump (`e9 rel32`, `ff 25 [rip]`) a
/// patch put there, down to the code that runs.
unsafe fn through(address: usize) -> String {
    let mut hops = vec![place(address)];
    let mut at = address;
    for _ in 0..4 {
        at = match (read::<u8>(at), read::<u8>(at + 1)) {
            (0xe9, _) => (at as isize + 5 + read::<i32>(at + 1) as isize) as usize,
            (0xff, 0x25) => read::<usize>((at as isize + 6 + read::<i32>(at + 2) as isize) as usize),
            _ => break,
        };
        hops.push(place(at));
    }
    hops.join(" -> ")
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

// ---- the breakpoint log: CSP_ORACLE_BREAK=rva,rva,... (CSP DLL offsets, hex) -----------------
// each listed instruction gets an int3; a hit logs (step, thread, rva, caller, rcx rdx r8 r9; an rva
// ending in "d" also the 12 doubles at rdx; ending in "g" the ODE geoms at rcx and rdx: 8 qwords from +0
// and the 3 doubles at the position pointer +0x28, the body's 6 qwords; ending in "b" the dxBody at
// rcx as `dxStepBody` reads it: h from xmm1, the flags, then ode::BODY_FIELDS; "i" / "w" the island
// recorder, "a" / "z" the world recorder, island.rs), puts the byte back,
// single-steps it and re-arms. shortcut: another thread passing during that one instruction is not
// logged, upgrade if a count looks short

static BREAKS: Mutex<Vec<(usize, u8, char)>> = Mutex::new(Vec::new());
static BREAK_LOG: Mutex<String> = Mutex::new(String::new());
static BREAK_STEP: AtomicUsize = AtomicUsize::new(usize::MAX);
thread_local!(static REARM: Cell<usize> = const { Cell::new(0) });

unsafe fn arm_breaks(dll: usize) {
    let Ok(list) = std::env::var("CSP_ORACLE_BREAK") else { return };
    let mut breaks = BREAKS.lock().unwrap();
    for rva in list.split(',').filter(|rva| !rva.is_empty()) {
        let rva = rva.trim();
        let (rva, dump) = match rva.chars().last() {
            Some(kind @ ('c' | 'r' | 'h' | 'd' | 'm' | 'g' | 'b' | 'i' | 'w' | 'l' | 'a' | 'z')) => (&rva[..rva.len() - 1], kind),
            _ => (rva, ' '),
        };
        let address = dll + usize::from_str_radix(rva.trim_start_matches("0x"), 16).expect("CSP_ORACLE_BREAK takes hex offsets");
        breaks.push((address, read::<u8>(address), dump));
        let mut old = 0;
        // left writable: hits on two threads must not race a protection change back to read-only
        VirtualProtect(address as *mut c_void, 1, 0x40, &mut old);
        set_byte(address, 0xcc);
    }
    let add_handler: extern "system" fn(u32, usize) -> *mut c_void = std::mem::transmute(GetProcAddress(GetModuleHandleA(c"kernel32.dll".as_ptr().cast()), c"AddVectoredExceptionHandler".as_ptr().cast()));
    add_handler(1, break_hit as *const () as usize);
    println!("{} breakpoints armed", breaks.len());
}

unsafe fn set_byte(address: usize, byte: u8) {
    *(address as *mut u8) = byte;
    FlushInstructionCache(GetCurrentProcess(), address as *const c_void, 1);
}

unsafe extern "system" fn break_hit(pointers: *const *const usize) -> i32 {
    let record = *pointers as usize;
    let context = *pointers.add(1) as usize;
    let (code, at) = (read::<u32>(record), read::<usize>(record + 0x10));
    let flags = (context + 0x44) as *mut u32; // EFlags; Rip at 0xf8, Rsp at 0x98
    if code == 0x8000_0004 {
        let rearm = REARM.with(|cell| cell.replace(0));
        if rearm == 0 {
            return 0;
        }
        set_byte(rearm, 0xcc);
        *flags &= !0x100;
        return -1; // EXCEPTION_CONTINUE_EXECUTION
    }
    let (original, dump) = match BREAKS.lock().unwrap().iter().find(|(address, ..)| *address == at) {
        Some(&(_, byte, dump)) if code == 0x8000_0003 => (byte, dump),
        _ => return 0,
    };
    let step = BREAK_STEP.load(Ordering::Relaxed);
    if step != usize::MAX {
        let caller = read::<usize>(read::<usize>(context + 0x98));
        let [rcx, rdx, r8, r9] = [0x80, 0x88, 0xb8, 0xc0].map(|offset| read::<usize>(context + offset));
        let mut line = format!("{step} {} {:#x} {} {rcx:#x} {rdx:#x} {r8:#x} {r9:#x}", tag_of(GetCurrentThreadId()), at - DLL_BASE.load(Ordering::Relaxed), place(caller));
        if dump == 'd' {
            (0..12).for_each(|i| line.push_str(&format!(" {}", read::<f64>(rdx + i * 8))));
        }
        if dump == 'b' {
            line.push_str(&format!(" {} {}", read::<f64>(context + 0x1b0), read::<u32>(rcx + 0x38)));
            crate::ode::BODY_FIELDS.iter().for_each(|&(offset, n)| (0..n).for_each(|i| line.push_str(&format!(" {}", read::<f64>(rcx + offset + i * 8)))));
        }
        if dump == 'i' {
            crate::island::enter(rcx, step);
        }
        if dump == 'w' {
            // the world step's return site: a break of its own, kind 'x'
            let ret = read::<usize>(read::<usize>(context + 0x98));
            let mut breaks = BREAKS.lock().unwrap();
            if !breaks.iter().any(|(address, ..)| *address == ret) {
                let mut old = 0;
                VirtualProtect(ret as *mut c_void, 1, 0x40, &mut old);
                breaks.push((ret, read::<u8>(ret), 'x'));
                set_byte(ret, 0xcc);
            }
        }
        if dump == 'x' {
            crate::island::leave(step);
        }
        if dump == 'a' {
            crate::island::world_enter(rcx, read::<f64>(context + 0x1c0), step);
        }
        if dump == 'z' {
            crate::island::world_leave(step);
        }
        if dump == 'l' {
            crate::island::lcp(rdx, r8, r9, read::<usize>(context + 0x98));
        }
        if dump == 'h' {
            crate::space::hit(rcx, r8, at, original, DLL_BASE.load(Ordering::Relaxed), &mut line);
        }
        if dump == 'c' {
            crate::btl::hit(rcx, rdx, r8 as u32, at, original, DLL_BASE.load(Ordering::Relaxed), &mut line);
        }
        if dump == 'r' {
            crate::reduce::hit(rcx, rdx as i32, r9, at, original, DLL_BASE.load(Ordering::Relaxed), &mut line);
        }
        if dump == 'm' {
            // raw words of rcx's object, for reading struct layouts
            line.push_str(" |");
            (0..64).for_each(|i| line.push_str(&format!(" {:x}", read::<usize>(rcx + i * 8))));
        }
        if dump == 'g' {
            for geom in [rcx, rdx] {
                line.push_str(" |");
                (0..8).for_each(|i| line.push_str(&format!(" {:x}", read::<usize>(geom + i * 8))));
                let pos = read::<usize>(geom + 0x28);
                if pos != 0 {
                    (0..3).for_each(|i| line.push_str(&format!(" {}", read::<f64>(pos + i * 8))));
                }
                let body = read::<usize>(geom + 0x18);
                if body != 0 {
                    line.push_str(" body");
                    (0..6).for_each(|i| line.push_str(&format!(" {:x}", read::<usize>(body + i * 8))));
                }
            }
        }
        line.push('\n');
        BREAK_LOG.lock().unwrap().push_str(&line);
    }
    set_byte(at, original);
    *((context + 0xf8) as *mut usize) = at;
    *flags |= 0x100; // trap after this one instruction, then re-arm
    REARM.with(|cell| cell.set(at));
    -1
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

unsafe fn put<T>(base: *mut u8, offset: usize, value: T) {
    std::ptr::write_unaligned(base.add(offset) as *mut T, value);
}

/// car_oracle's car load (game.rs `new_engine`, the flat track, `Car::Car`, the box collider),
/// with the game's own functions, so CSP's patches in them run.
/// `track_folder`: a real track (car_oracle's track.rs, ghost: only the tyres' rays meet it, as in
/// car_oracle's track runs without --collide) in place of the flat road; `car_root` was prepared
/// for it (track::prepare_root).
unsafe fn load_car(acs: &Acs, car_name: &str, car_root: &str, csp: Option<usize>, count: usize, track_folder: Option<&std::path::Path>) -> (*mut u8, Vec<*mut u8>) {
    std::env::set_current_dir(car_root).expect("the car root can be entered");
    let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
    srand(1);
    acs::reset_clock();
    assert_eq!(acs.global::<u8>(VA_IS_USING_QPT), 1, "ksTimer is not in QueryPerformanceCounter mode");
    assert_eq!(acs.global::<u8>(VA_IS_TEST_MODE), 0, "PhysicsEngine::isTestMode is set");
    acs.set_global(VA_TIMER_START, 0i64);
    acs.set_global(VA_TIMER_FREQUENCY, acs::CLOCK_FREQUENCY as i64);
    acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);

    let engine = acs.alloc(PE_SIZE);
    let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PHYSICS_ENGINE_CTOR));
    ctor(engine);
    put(engine, PE_PHYSICS_TIME, 60_000.0f64);
    put(engine, PE_GAME_TIME, 60_000.0f64);
    put(engine, PE_AMBIENT_TEMPERATURE, 26.0f32);
    put(engine, PE_ROAD_TEMPERATURE, 30.0f32);
    println!("PhysicsEngine built");

    if let Some(dll) = csp {
        // CSP's car config reads its RaceManager global (RVA 0x281a270, set by its RaceManager::RaceManager
        // detour) for the lights' session rule: a zeroed RaceManager whose sim (+0x168) and GameObject
        // game pointer (+8) are zeroed stand-ins
        // shortcut: stand-ins instead of the game's Sim / RaceManager, upgrade if a physics seam reads them
        let race_manager = acs.alloc(0x218);
        put(race_manager, 8, acs.alloc(0x100));
        put(race_manager, 0x168, acs.alloc(0x2d0));
        *((dll + 0x281_a270) as *mut *mut u8) = race_manager;
        // CSP's Sim::Sim detour stores the Sim in global RVA 0x2820e60; its player car's pre-step (RVA
        // 0x58a8d0) reads escMenu (+0x118) and pauseMenu (+0x110) there, its clocks off the thread read
        // GameObject::game (+8) → gameTime.now: the zeroed Sim stand-in with a zeroed Game (0x278)
        let sim = read::<*mut u8>(race_manager as usize + 0x168);
        put(sim, 8, acs.alloc(0x278));
        *((dll + 0x282_0e60) as *mut *mut u8) = sim;
        // the same pre-step reads byte +0x2fb of the object global RVA 0x28213d8 points to (whose first
        // word other code follows to a CarAvatar-sized object, +0x1168 → Car): a zeroed object whose first
        // word is a zeroed 0x12a8 stand-in
        // shortcut: CSP's own object is made by set-up code the oracle does not run, upgrade if a physics
        // seam reads more of it
        // one per car GUID: the car collision hook (dwrite+0x587cc0) follows slot+0x440 to a box it
        // reads at +0x90..+0xa4 and the slot's first word to a CarAvatar (+0xe18 audio), all zero here
        let state = acs.alloc(0x1000);
        for guid in 0..count {
            let slot = acs.alloc(0x1000);
            put(slot, 0, acs.alloc(0x12a8));
            put(slot, 0x440, acs.alloc(0x100));
            put(state, 8 * guid, slot);
        }
        *((dll + 0x282_13d8) as *mut *mut u8) = state;
        if let Some(folder) = track_folder {
            // CSP's AI-line detour (dwrite+0x5f2920) builds a lookup grid of the AI line and caches it as
            // ai_grids\{track}_{layout}_{hash}.bin in its cache folder (the run's own local folder here),
            // naming it from the track-info global RVA 0x2821430: track (+0x68) and layout (+0x88) wstrings
            // shortcut: the rest of the track info is zero, upgrade if a physics seam reads more of it
            let info = acs.alloc(0x200);
            let name = folder.file_name().expect("the track folder has a name").to_string_lossy();
            std::ptr::copy_nonoverlapping(wstring(acs, &name), info.add(0x68), 32);
            std::ptr::copy_nonoverlapping(wstring(acs, ""), info.add(0x88), 32);
            *((dll + 0x282_1430) as *mut *mut u8) = info;
        }
    }
    // the Track sits in its TrackAvatar (0x308) at +0xf8 (physicsTrack): CSP's TrackAvatar global
    // (RVA 0x2820268) is read in the car step for the track's ray cast (+0xf8 vtable +8) and timeLines (+0x120)
    // after the Sim stand-in: CSP's AI-line detour (dwrite+0x5f1d60, reached from AISpline::loadFast)
    // reads the Sim global's +0x128 (the track's scene: none here, so it reverses nothing)
    let track_avatar = if let Some(folder) = track_folder { real_track(acs, engine, folder) } else { flat_track(acs, engine) };
    if let Some(dll) = csp {
        *((dll + 0x282_0268) as *mut *mut u8) = track_avatar;
    }
    let cars: Vec<*mut u8> = (0..count).map(|guid| build_car(acs, engine, car_name, csp, guid as u32)).collect();

    // the loaded car's memory, word by word, for run-to-run and Vanilla-to-CSP compares; a word that
    // reads as a readable address is a pointer (heap addresses move between runs) and is written as "ptr"
    let (words, pointers) = words_of(cars[0], CAR_SIZE, &mut HashMap::new());
    std::fs::write("car_words.txt", &words).expect("car_words.txt can be written");
    println!("car memory: {} words, {pointers} pointers, hash {:016x}", CAR_SIZE / 8, fnv(&words));
    (engine, cars)
}

/// car_oracle's flat road (game.rs track_ray_cast) in a zeroed TrackAvatar: one surface, the track
/// answers the tyres' rays itself through a copy of its vtable (RTTI pointer in front) with rayCast and
/// createRayCaster ours.
/// shortcut: the rest of the TrackAvatar is zero, upgrade if a physics seam reads more of it
unsafe fn flat_track(acs: &Acs, engine: *mut u8) -> *mut u8 {
    let track_avatar = acs.alloc(0x308);
    let track = track_avatar.add(0xf8);
    const { assert!(TRACK_SIZE <= 0x308 - 0xf8) };
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_CTOR));
    ctor(track, engine, wstring(acs, "flat"), wstring(acs, ""));
    let init_ai_spline: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_TRACK_INIT_AI_SPLINE));
    init_ai_spline(track);
    let surface = acs.alloc(0xc8);
    put(surface, 0x90, 1.0f32); // gripMod
    put(surface, 0x9c, 1u32); // collisionCategory
    put(surface, 0xa0, 1u8); // isValidTrack
    SURFACE.store(surface as usize, Ordering::Relaxed);
    let original = read::<*const usize>(track as usize);
    let mut table: Vec<usize> = (0..5).map(|i| *original.offset(i - 1)).collect();
    table[2] = track_ray_cast as *const () as usize;
    table[4] = track_create_ray_caster as *const () as usize;
    put(track, 0, Box::leak(table.into_boxed_slice()).as_mut_ptr().add(1));
    println!("Track built");
    track_avatar
}

/// A real track as car_oracle's track runs build it (game.rs World::build without --collide): the game's
/// Track inside a TrackAvatar stand-in with its name, layout and SurfacesManager (track.rs), every mesh
/// through Track::addSurface with collision category 0 (ghost: only the tyres' rays meet it), the time
/// lines from the AC_TIME_n nodes, penalties on with two tyres allowed out. The tyres' rays are the game's
/// own Track::createRayCaster / rayCast.
/// shortcut: the TrackAvatar's other members are zero (no AI, no timing, no track objects), upgrade if a
/// physics seam reads them
unsafe fn real_track(acs: &Acs, engine: *mut u8, folder: &std::path::Path) -> *mut u8 {
    let plan = track::BuildPlan { in_avatar: true, clean_surfaces: true, ..Default::default() };
    let built = track::GameTrack::build_with(acs, engine, folder, "", true, &plan).unwrap_or_else(|e| stop(2, &format!("the track: {e}")));
    if !built.surface_mismatches.is_empty() {
        stop(2, &format!("the game's surfaces differ from the port's: {:?}", built.surface_mismatches));
    }
    let place = |name: &str| -> Option<[f32; 3]> {
        let m = built.rust.helper_nodes.iter().find(|h| h.name == name)?.local.m;
        Some([m[3][0], m[3][1], m[3][2]])
    };
    let add_time_line: extern "C" fn(*mut u8, *const [f32; 3], *const [f32; 3], i32) = std::mem::transmute(acs.va(0x1_4027_8040));
    let mut lines = 0;
    while let (Some(left), Some(right)) = (place(&format!("AC_TIME_{lines}_L")), place(&format!("AC_TIME_{lines}_R"))) {
        add_time_line(built.track, &left, &right, 0);
        lines += 1;
    }
    if let [Some(sl), Some(sr), Some(fl), Some(fr)] = ["AC_AB_START_L", "AC_AB_START_R", "AC_AB_FINISH_L", "AC_AB_FINISH_R"].map(place) {
        add_time_line(built.track, &sl, &sr, 1);
        add_time_line(built.track, &fl, &fr, 2);
        lines += 2;
    }
    if let (Some(left), Some(right)) = (place("AC_OPEN_FINISH_L"), place("AC_OPEN_FINISH_R")) {
        add_time_line(built.track, &left, &right, 0);
        lines += 1;
    }
    assert_eq!(lines, built.rust.time_lines.len(), "the port found other timing lines");
    put(engine, 0xd4, 1i32); // penaltyMode
    put(engine, 0xd0, 2i32); // allowedTyresOut
    println!("Track built: {} ({} meshes, {lines} time lines, ghost)", folder.display(), built.objects.len());
    let avatar = built.track.sub(track::TA_PHYSICS_TRACK); // the track sits inside the avatar (BuildPlan::in_avatar)
    Box::leak(Box::new(built));
    avatar
}

/// One `Car::Car` on `engine` (physics GUID `guid`) with CSP's per-car set-up around it.
unsafe fn build_car(acs: &Acs, engine: *mut u8, car_name: &str, csp: Option<usize>, guid: u32) -> *mut u8 {
    if let Some(dll) = csp {
        // CSP's CarAvatar::CarAvatar detour makes the car's extension context (current-context
        // global at RVA 0x2817d90) before the game builds the car; its Car detours read it unchecked.
        // shortcut: a zeroed CarAvatar with only unixName (+0x138) set, upgrade if a detour reads more of it
        let avatar = acs.alloc(0x12a8);
        std::ptr::copy_nonoverlapping(wstring(acs, car_name), avatar.add(0x138), 32);
        let push_context: extern "C" fn(*mut u8) = std::mem::transmute(dll + 0xf3_2f90);
        push_context(avatar);
        println!("CSP car context made: {}", read::<usize>(dll + 0x281_7d90) != 0);
    }

    // CarAvatar::initCommon's car.ini [HEADER] VERSION read: CSP wraps initCommon (arm byte RVA 0x2815087)
    // and its INIReader::getInt detour (RVA 0x5e3590) turns "extended-N" into N and builds the car's
    // physics extension at context+0xe0, which its Car::Car detour then uses
    let reader = ini_reader(acs, &format!("content/cars/{car_name}/data/car.ini"));
    if let Some(dll) = csp {
        *((dll + 0x281_5087) as *mut u8) = 1;
    }
    let get_int: extern "system" fn(*mut u8, *const u8, *const u8) -> i32 = std::mem::transmute(acs.va(INIREADER_GET_INT));
    // Vanilla's getInt is std::stoi: "extended-N" throws std::invalid_argument, which ends this process
    println!("reading car.ini [HEADER] VERSION with INIReader::getInt: {}", through(acs.va(INIREADER_GET_INT)));
    let version = get_int(reader, wstring(acs, "HEADER"), wstring(acs, "VERSION"));
    if let Some(dll) = csp {
        *((dll + 0x281_5087) as *mut u8) = 0;
        let context = read::<usize>(dll + 0x281_7d90);
        println!("car.ini VERSION read as {version}, CSP car physics extension made: {}", read::<usize>(context + 0xe0) != 0);
        // the game builds cars while CSP's loading flag (RVA 0x26bad10) is up; Car::Car's detour reads
        // the current context only then
        *((dll + 0x26b_ad10) as *mut u8) = 1;
    } else {
        println!("car.ini VERSION read as {version}");
    }

    // the wing rule: CSP's hasSection detour (RVA 0x5dfd20) hides a DYNAMIC_CONTROLLER_ section whose
    // WING_n is missing, armed only inside its NetCarStateProvider::initWings wrapper (RVA 0x61e6f0,
    // byte RVA 0x27f9201); the oracle arms the byte the way that wrapper does around its call
    std::fs::write("wing_rule.ini", "[DYNAMIC_CONTROLLER_0]\nWING=3\n[DYNAMIC_CONTROLLER_1]\nWING=0\n[WING_0]\nNAME=X\n").expect("wing_rule.ini can be written");
    let reader = ini_reader(acs, "wing_rule.ini");
    println!("hasSection: {}", through(acs.va(INIREADER_HAS_SECTION)));
    if let Some(dll) = csp {
        *((dll + 0x27f_9201) as *mut u8) = 1;
    }
    let (missing, present) = (has_section(acs, reader, "DYNAMIC_CONTROLLER_0"), has_section(acs, reader, "DYNAMIC_CONTROLLER_1"));
    if let Some(dll) = csp {
        *((dll + 0x27f_9201) as *mut u8) = 0;
    }
    println!("wing rule: hasSection(DYNAMIC_CONTROLLER_0, WING=3 missing) = {missing}, hasSection(DYNAMIC_CONTROLLER_1, WING=0) = {present}");

    println!("Car::Car: {}", through(acs.va(VA_CAR_CTOR)));
    let car = acs.alloc(CAR_SIZE);
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_CTOR));
    ctor(car, engine, wstring(acs, car_name), wstring(acs, ""));
    assert_eq!(read::<u32>(car as usize + CAR_PHYSICS_GUID), guid, "the car has another physics GUID");
    // rule 18: colliderManager.observer.lastFileTime (Car+0x2b0) is the game's uninitialised stack when
    // the watched file is missing (FileChangeObserver::observe: CreateFileW fails with -1, not 0, so
    // GetFileTime fails and its output local is copied); zeroed here
    put(car, 0x2b0, 0u64);
    println!("Car built");

    // a mesh collider is mandatory (Car::updateColliderStatus reads mesh 0 unchecked): car_oracle's box
    let mesh = acs.alloc(0x168);
    // Node padding +0xdc: CSP's Node::Node detour keeps its extended-data index there, -1 = none
    put(mesh, 0xdc, -1i32);
    let mut vertices = vec![0f32; 8 * 11];
    for i in 0..8 {
        vertices[i * 11] = if i & 1 == 0 { -0.3 } else { 0.3 };
        vertices[i * 11 + 1] = if i & 2 == 0 { 0.3 } else { 0.5 };
        vertices[i * 11 + 2] = if i & 4 == 0 { -0.5 } else { 0.5 };
    }
    let indices: Vec<u16> = vec![0, 1, 2, 1, 3, 2, 4, 6, 5, 5, 6, 7, 0, 4, 1, 1, 4, 5, 2, 3, 6, 3, 7, 6, 0, 2, 4, 2, 6, 4, 1, 5, 3, 3, 5, 7];
    let vp = vertices.leak().as_mut_ptr().cast::<u8>();
    put(mesh, 0x108, vp);
    put(mesh, 0x110, vp.add(8 * 0x2c));
    put(mesh, 0x118, vp.add(8 * 0x2c));
    let ip = indices.leak().as_mut_ptr().cast::<u8>();
    put(mesh, 0x120, ip);
    put(mesh, 0x128, ip.add(36 * 2));
    put(mesh, 0x130, ip.add(36 * 2));
    let identity: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    let init_mesh: extern "C" fn(*mut u8, *mut u8, *const [f32; 16]) = std::mem::transmute(acs.va(VA_CAR_INIT_COLLIDER_MESH));
    init_mesh(car, mesh, &identity);
    println!("collider mesh set");
    if let Some(dll) = csp {
        *((dll + 0x26b_ad10) as *mut u8) = 0;
    }
    car
}

/// `size` bytes at `base` as "offset value" lines; a word that reads as a readable address is a
/// pointer (heap addresses move between runs) and is written as "ptr". `pages` caches the probe
/// per 4 KB page.
unsafe fn words_of(base: *const u8, size: usize, pages: &mut HashMap<u64, bool>) -> (String, usize) {
    let mut words = String::with_capacity(size * 3);
    let mut pointers = 0;
    for offset in (0..size).step_by(8) {
        let value = read::<u64>(base as usize + offset);
        let readable = value != 0
            && *pages.entry(value >> 12).or_insert_with(|| {
                let mut probe = 0u8;
                ReadProcessMemory(GetCurrentProcess(), value as usize, &mut probe, 1, std::ptr::null_mut()) != 0
            });
        if readable {
            pointers += 1;
            words.push_str(&format!("{offset:#06x} ptr\n"));
        } else {
            words.push_str(&format!("{offset:#06x} {value:016x}\n"));
        }
    }
    (words, pointers)
}

/// AC's `RayCastResult`.
#[repr(C)]
struct RayCastResult {
    surface_def: *mut u8,
    pos: [f32; 3],
    normal: [f32; 3],
    has_hit: u8,
    collision_object: *mut u8,
}
const _: () = assert!(std::mem::size_of::<RayCastResult>() == 0x30);

static SURFACE: AtomicUsize = AtomicUsize::new(0);

/// car_oracle's `track_ray_cast` on its flat road: height 0, the game's 3 m ray.
extern "C" fn track_ray_cast(_this: *mut u8, org: *const [f32; 3], _dir: *const [f32; 3], result: *mut RayCastResult, _length: f32) -> u8 {
    let org = unsafe { *org };
    let height = 0.0f32;
    let hit = org[1] >= height && org[1] - height <= 3.0;
    unsafe {
        (*result).has_hit = hit as u8;
        if hit {
            (*result).surface_def = SURFACE.load(Ordering::Relaxed) as *mut u8;
            (*result).pos = [org[0], height, org[2]];
            (*result).normal = [0.0, 1.0, 0.0];
            (*result).collision_object = std::ptr::null_mut();
        }
    }
    hit as u8
}

extern "C" fn track_create_ray_caster(_this: *mut u8, _length: f32) -> *mut u8 {
    std::ptr::null_mut()
}

// ---- Task C4a Part A: car_oracle's drive (game.rs World::build and World::step, without its
// recording hooks), every car's memory and getPhysicsState output hashed after every step ----

/// The current step's 40-byte .ryin record (crates/rustyac-game/src/input_file.rs StepInput), read by
/// every car's controls device: with workers the cars poll on CSP's worker threads.
static RECORD: Mutex<[u8; 40]> = Mutex::new([0; 40]);

fn record_word(k: usize) -> u32 {
    record_bytes_word(&*RECORD.lock().unwrap(), k)
}

extern "C" fn device_destructor(_this: *mut u8, _flags: u32) -> *mut u8 {
    std::ptr::null_mut()
}

/// car_oracle's `device_acquire_controls` for a scenario that is not a whole-car one (no DRS/KERS).
extern "C" fn device_acquire_controls(_this: *mut u8, controls: *mut u8, _dt: f32, _input: *const f32) {
    let flags = record_word(6);
    unsafe {
        put(controls, 0, (flags & 1) as u8); // gearUp
        put(controls, 1, (flags >> 1 & 1) as u8); // gearDn
        put(controls, 0x10, f32::from_bits(record_word(4))); // handBrake
        put(controls, 8, record_word(5) as i32); // requestedGearIndex
        put(controls, 0x24, f32::from_bits(record_word(0))); // gas
        put(controls, 0x28, f32::from_bits(record_word(1))); // brake
        put(controls, 0x2c, f32::from_bits(record_word(2))); // steer
        put(controls, 0x30, f32::from_bits(record_word(3))); // clutch
    }
}

extern "C" fn device_get_action(_this: *mut u8, _action: i32) -> u8 {
    0
}
extern "C" fn device_send_ff(_this: *mut u8, _force: f32, _damper: f32, _gain: f32) {}
extern "C" fn device_get_ff_global_gain(_this: *mut u8) -> f32 {
    1.0
}
extern "C" fn device_get_name(_this: *mut u8) -> *const u8 {
    c"csp_oracle script".as_ptr().cast()
}
extern "C" fn device_on_auto_shifter_changed(_this: *mut u8, _on: u8) {}
extern "C" fn device_false(_this: *mut u8) -> u8 {
    0
}
extern "C" fn device_set_vibrations(_this: *mut u8, _def: *const u8) {}
extern "C" fn device_set_engine_rpm(_this: *mut u8, _rpm: f32, _limiter: f32) {}

/// Drives `cars` with the records of `ryin` (car_oracle's flat-road scenarios), `steps` of them at
/// most, and writes steps.txt: per step and car the hash of the car's words and of its state.
/// `still`: every record's pedals and wheel zero. `air` (CSP's dll, `track:ambient:road:wind`): per
/// step CSP's own air density of car 0 against rustyac_physics::csp::air (see AirCheck).
/// `track`: the real track load_car built (its name): the tape's spawn, armed first lap and the session
/// start's setup as car_oracle's track runs, and (without CSP) body0.txt, the chassis body after each step.
#[allow(clippy::too_many_arguments)]
unsafe fn drive(acs: &Acs, engine: *mut u8, cars: &[*mut u8], ryin: &str, steps: usize, still: bool, csp: Option<usize>, air: Option<&str>, track: Option<(&str, &str)>) {
    let bytes = std::fs::read(ryin).expect("the .ryin file can be read");
    let end = bytes.windows(5).position(|w| w == b"\nend\n").expect("the .ryin file has an end line") + 5;
    let header: BTreeMap<String, String> = std::str::from_utf8(&bytes[..end])
        .expect("the .ryin header is text")
        .lines()
        .filter_map(|line| line.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    // what load_car built; shortcut: the other header keys (consumption rates, blankets …) are the
    // engine's defaults as in car_oracle's flat scenarios, upgrade for a scenario that sets them
    for (key, value) in [("dt", "0.003"), ("clock_start_ms", "60000.0"), ("ambient_temperature", "26.0"), ("road_temperature", "30.0"), ("oracle_ground", "flat"), ("oracle_pitlane", "0"), ("oracle_stability_gain", "0.0"), ("oracle_wind_speed", "0.0")] {
        if header.get(key).map(String::as_str) != Some(value) {
            stop(2, &format!("{ryin}: {key} is {:?}, csp_oracle drives only {value}", header.get(key)));
        }
    }
    let records = &bytes[end..];
    assert_eq!(records.len() % 40, 0, "the .ryin records are not 40 bytes each");
    let tape_track = header.get("oracle_track_folder").and_then(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned());
    if tape_track.as_deref() != track.map(|t| t.0) {
        stop(2, &format!("{ryin}: the tape's track is {tape_track:?}, csp_oracle drives {:?}", track.map(|t| t.0)));
    }
    // the tape's spawn: f32 bits, three per vector
    let vector = |key: &str| -> [f32; 3] {
        let bits: Vec<u32> = header.get(key).map(|v| v.split(',').map(|w| u32::from_str_radix(w, 16).expect("a spawn word")).collect()).unwrap_or_default();
        <[u32; 3]>::try_from(bits).unwrap_or_else(|_| stop(2, &format!("{ryin}: {key} is not three words"))).map(f32::from_bits)
    };

    let keyboard = acs.va(0x1_404c_5d50) as *const usize; // KeyboardCarControl's vtable
    let table: Vec<usize> = vec![
        *keyboard.offset(-1),
        device_destructor as *const () as usize,
        device_acquire_controls as *const () as usize,
        device_get_action as *const () as usize,
        device_send_ff as *const () as usize,
        device_get_name as *const () as usize,
        device_get_ff_global_gain as *const () as usize,
        device_false as *const () as usize,
        device_on_auto_shifter_changed as *const () as usize,
        device_false as *const () as usize,
        device_set_vibrations as *const () as usize,
        device_set_engine_rpm as *const () as usize,
        device_false as *const () as usize,
    ];
    let table = Box::leak(table.into_boxed_slice()).as_mut_ptr().add(1);
    let force_rotation: extern "C" fn(*mut u8, *const [f32; 3]) = std::mem::transmute(acs.va(0x1_4027_0040));
    let force_position: extern "C" fn(*mut u8, *const [f32; 3], u8) = std::mem::transmute(acs.va(0x1_4026_fe10));
    let set_damage_level: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(0x1_4027_5b20));
    let reset_suspension_damage: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(0x1_4027_5970));
    for (i, &car) in cars.iter().enumerate() {
        let device = acs.alloc(0x120);
        put(device, 0, table);
        put(car, 0x3d80, device); // controlsProvider
        put(car, 0x3560, 0u8); // performanceMeter.isEnabled
        put(car, 0x3258, 0u8); // telemetry.isEnabled
        let auto_clutch = header.get("auto_clutch").map(String::as_str) == Some("1");
        put(car, 0x3090 + 0xc, auto_clutch as u8); // autoClutch.useAutoOnStart
        if auto_clutch {
            put(car, 0x3090 + 0xd, 1u8); // autoClutch.useAutoOnChange
        }
        if header.get("auto_shifter").map(String::as_str) == Some("1") {
            put(car, 0x33e8, 1u8); // autoShifter.isActive
        }
        if track.is_some() {
            if header.get("oracle_armed").map(String::as_str) == Some("1") {
                put(car, 0x1f8 + 0x80, 1u8); // CarAvatar::armFirstLap (transponder)
            }
            // more cars (Task C5e): 20 m apart along the spawn's heading, each driven by the same tape
            let (tail, at) = (vector("oracle_spawn_tail"), vector("oracle_spawn_position"));
            let n = (tail[0] * tail[0] + tail[1] * tail[1] + tail[2] * tail[2]).sqrt();
            force_rotation(car, &tail);
            force_position(car, &std::array::from_fn(|k| at[k] + tail[k] / n * 20.0 * i as f32), 1);
        } else {
            // side by side 4 m apart, nose towards +z
            force_rotation(car, &[0.0, 0.0, -1.0]);
            force_position(car, &[20.0 * i as f32, 0.0, 0.0], 1);
        }
        set_damage_level(car, 0.0);
        reset_suspension_damage(car);
        if let Some((_, car_name)) = track {
            // car_oracle's session_start_setup: the default compound (Tyre::setCompound), then the setup screen
            use rustyac_physics::data::ini::IniReader;
            let data = std::path::Path::new("content/cars").join(car_name).join("data");
            let compound = IniReader::load(&data.join("tyres.ini")).expect("the car's tyres.ini").get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
            let set_compound: extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(acs.va(0x1_4028_34e0));
            for wheel in 0..4 {
                set_compound(car.add(0xb28 + wheel * 0x858), compound);
            }
            if let Ok(setup) = IniReader::load(&data.join("setup.ini")) {
                let changes = setup_screen::apply_setup_screen_defaults(car, &setup);
                println!("session start setup: compound {compound}, {} changes {changes:?}", changes.len());
            }
        }
        // shortcut: no session-start setup round trip on the flat road (car_oracle's session_start_setup),
        // upgrade when a flat csp_oracle drive is compared with a car_oracle or rustyAC one
    }

    let step: extern "C" fn(*mut u8, f32, f64, f64) = std::mem::transmute(acs.va(0x1_4026_4760));
    let get_physics_state: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(0x1_4027_0d70));
    // one state per car, as each CarAvatar has: CSP's getPhysicsState hook (0x590220) smooths the wheels'
    // angular speed against what the buffer held before
    let states: Vec<*mut u8> = cars.iter().map(|_| acs.alloc(0x2000)).collect();
    let mut pages = HashMap::new();
    let mut lines = String::new();
    let mut body0 = String::new();
    let count = steps.min(records.len() / 40);
    let mut air = air.map(|spec| AirCheck::new(acs, csp.expect("--air needs CSP"), spec, cars[0]));
    for (i, record) in records.chunks_exact(40).take(count).enumerate() {
        if record_bytes_word(record, 7) != 0 || record_bytes_word(record, 8) != 0 {
            stop(2, &format!("step {i}: events or brake-bias clicks, which csp_oracle does not drive"));
        }
        *RECORD.lock().unwrap() = record.try_into().unwrap();
        if still {
            RECORD.lock().unwrap()[..20].fill(0); // gas, brake, steer, clutch, hand brake
        }
        if let Some(air) = &mut air {
            air.before(i);
        }
        BREAK_STEP.store(i, Ordering::Relaxed);
        if i == 0 && !BREAKS.lock().unwrap().is_empty() {
            // names the bodies in the log's geom dumps
            for (k, &car) in cars.iter().enumerate() {
                BREAK_LOG.lock().unwrap().push_str(&format!("car {k} at {car:?}, +0x118 {:x}\n", read::<usize>(car as usize + 0x118)));
            }
        }
        if let Some(dll) = csp {
            // CSP queues calls made on its main thread, Car::forcePosition among them (hook 0x58d7d0),
            // and runs them in its hook of the game's PhysicsAvatar::stepCommandQueue (0x676470), which the
            // physics thread calls each step; this oracle has no PhysicsAvatar, so it runs that queue itself
            let run_queue: extern "C" fn(usize) = std::mem::transmute(dll + 0x15d_1ee0);
            run_queue(read::<usize>(read::<usize>(dll + 0x281_4d78)));
        }
        let time = 60_000.0 + (i + 1) as f64 * 3.0;
        step(engine, 0.003, time, time);
        if let Some(dll) = csp {
            // CSP's PhysicsEngine::step leaves the world step to its drive thread while this byte is set
            // (0x67ca40): the thread clears it, steps the core with 0.003 after the cars, sets it again
            let late = (dll + 0x280_3210) as *mut u8;
            if *late != 0 {
                *late = 0;
                let core = read::<*mut u8>(engine as usize + 0x190);
                let world: extern "C" fn(*mut u8, f32) = std::mem::transmute(read::<usize>(read::<usize>(core as usize) + 0x10));
                world(core, 0.003);
                *late = 1;
            }
        }
        if let Some(air) = &mut air {
            air.after(i);
        }
        lines.push_str(&i.to_string());
        for (k, &car) in cars.iter().enumerate() {
            let state = states[k];
            get_physics_state(car, state);
            if i == 0 || i + 1 == steps {
                let at = |n: usize| read::<f32>(state as usize + 0x34 + 4 * n); // worldMatrix's translation
                println!("step {i}: car {k} at {} {} {}", at(0), at(1), at(2));
            }
            let (car_words, _) = words_of(car, CAR_SIZE, &mut pages);
            let (state_words, _) = words_of(state, 0xb70, &mut pages);
            lines.push_str(&format!(" {:016x} {:016x}", fnv(&car_words), fnv(&state_words)));
        }
        lines.push('\n');
        if track.is_some() && csp.is_none() {
            // Vanilla's dxBody (f32): pos +0xc0, q +0x100, lvel +0x110, avel +0x120, as car_oracle's body.post.*
            let body = read::<usize>(read::<usize>(cars[0] as usize + 0x118) + 0x8);
            let words = |at: usize, n: usize| (0..n).map(|k| format!(" {:08x}", read::<u32>(body + at + 4 * k))).collect::<String>();
            body0.push_str(&format!("{i}{}{}{}{}\n", words(0xc0, 3), words(0x100, 4), words(0x110, 3), words(0x120, 3)));
        }
        if (i + 1) % 2000 == 0 {
            println!("step {}: {}", i + 1, lines.lines().last().unwrap_or(""));
        }
    }
    std::fs::write("steps.txt", &lines).expect("steps.txt can be written");
    if let Some(dll) = csp {
        // after the drive: which ray path the tyres took (docs/csp/core.md, switches): Embree when this byte is 1, which
        // CSP sets from ODE_BATCH_RAYCAST=0 in general.ini or from the forcing flag
        let flags = read::<usize>(dll + 0x282_1078);
        let forcing = if flags == 0 { "no flags object".to_string() } else { read::<u8>(flags + 0x16).to_string() };
        println!("Embree switch {} (BYTE3 of +0x28031fc, set when ODE_BATCH_RAYCAST is 0), forcing flag [+0x2821078]+0x16: {forcing}", read::<u8>(dll + 0x280_31ff));
    }
    if !body0.is_empty() {
        std::fs::write("body0.txt", &body0).expect("body0.txt can be written");
    }
    BREAK_STEP.store(usize::MAX, Ordering::Relaxed);
    island::save();
    let btl = btl::finish() + &space::finish() + &reduce::finish();
    print!("{btl}");
    BREAK_LOG.lock().unwrap().push_str(&btl);
    if !BREAKS.lock().unwrap().is_empty() {
        std::fs::write("breaks.txt", &*BREAK_LOG.lock().unwrap()).expect("breaks.txt can be written");
    }
    for (i, &car) in cars.iter().enumerate() {
        std::fs::write(format!("car{i}_words_end.txt"), words_of(car, CAR_SIZE, &mut pages).0).expect("the end words can be written");
    }
    println!("drove {count} steps, {} cars, steps hash {:016x}", cars.len(), fnv(&lines));
    if let Some(air) = air {
        air.finish();
    }
}

type V3 = rustyac_physics::vecmath::Vec3f;

/// Task C4a part C: CSP's air density of an extended car (docs/csp/port_c4.md), held against the
/// port in rustyac_physics::csp::air with CSP's own inputs. The car steps as a plain car (an
/// extended one's step needs CSP's track objects); around each step the oracle does what CSP's
/// Car::step wrapper (dwrite+0x58afc0) does for an extended car's record: the body's position
/// before the step into record+0x418, the tyres' ground plane after it into record+0x424
/// (dwrite+0x1150520). Before the step it calls the law (dwrite+0xb10410, the physics' flag 1)
/// with the record in CSP's record vector and the track's altitude and conditions in place of the
/// engine's, for that call only, and the near-ground temperature (dwrite+0xb0fcc0) on its own.
/// Every value is compared bit for bit; nothing of the drive changes.
struct AirCheck {
    dll: usize,
    car: *mut u8,
    pe: *mut u8,
    record: *mut u8,
    slot: *mut usize,
    altitude: f32,
    ambient: f32,
    road: f32,
    wind: f32,
    plane: [f32; 4],
    lines: String,
    differ: [usize; 4],
    first: Vec<String>,
}

impl AirCheck {
    unsafe fn new(acs: &Acs, dll: usize, spec: &str, car: *mut u8) -> AirCheck {
        let parts: Vec<&str> = spec.split(':').collect();
        let [track, ambient, road, wind] = parts[..] else { stop(2, "--air takes track:ambient:road:wind") };
        let number = |text: &str| text.parse::<f32>().unwrap_or_else(|_| stop(2, "--air takes numbers after the track"));
        // flat road: no AC_PIT_0 node, so Y 0 (dwrite+0x118e6c0)
        let known = table(dll, track, "");
        let csp_altitude = if known == rustyac_physics::csp::air::UNKNOWN_ALTITUDE { 294.0 } else { known };
        let altitude = rustyac_physics::csp::air::track_altitude(None, track, "", None);
        if csp_altitude.to_bits() != altitude.to_bits() {
            stop(3, &format!("air: {track}: CSP's altitude {csp_altitude}, the port's {altitude}"));
        }
        // the whole table: its names, then each guessed word (and pair) in a made-up folder name, as
        // written, in capitals and with a pair the wrong way round
        let mut names: Vec<(String, &str)> = ["ks_barcelona", "ks_black_cat_county", "ks_brands_hatch", "ks_drag", "drift", "ks_highlands", "imola", "ks_laguna_seca", "magione", "monza", "ks_monza66", "mugello", "ks_nordschleife", "ks_nurburgring", "ks_red_bull_ring", "ks_silverstone", "ks_silverstone1967", "spa", "trento-bondone", "ks_vallelunga", "ks_zandvoort", "india", "grandsambuc", "forliring", "greatwall", "korea", "la_canyons", "auto24ring", "monaco_1966", "hong_kong", "nowhere", "", "Spa"]
            .iter().map(|id| (id.to_string(), "")).collect();
        names.push(("ks_nordschleife".into(), "touristenfahrten"));
        for (a, b, _) in rustyac_physics::csp::air::GUESSED_WORDS {
            names.push((format!("x_{a}_{b}"), ""));
            names.push((format!("X_{}_{}", a.to_uppercase(), b.to_uppercase()), ""));
            if !b.is_empty() {
                names.push((format!("x_{b}_{a}"), ""));
            }
        }
        for (id, layout) in &names {
            let (csp, port) = (table(dll, id, layout), rustyac_physics::csp::air::known_altitude(id, layout));
            if csp.to_bits() != port.to_bits() {
                stop(3, &format!("air: altitude table: {id}/{layout}: CSP {csp}, the port {port}"));
            }
        }
        println!("air: altitude table: {} names, all as CSP's", names.len());
        let pe = read::<*mut u8>(dll + 0x281_4e40);
        assert!(!pe.is_null(), "CSP's physics engine global is set");
        grid(dll, pe);
        let record = acs.alloc(0x6f0);
        let avatar = acs.alloc(0x1000);
        let engineer = acs.alloc(0x40);
        put(engineer, 8, car); // RaceEngineer::car, whose GUID CSP's ground-height hook looks up
        put(avatar, 0xf80, engineer);
        put(record, 0x320, avatar);
        let slot = acs.alloc(16) as *mut usize;
        *slot = record as usize;
        assert_eq!(read::<u32>(car as usize + 0x3e9c), 0, "car 0 has CSP's GUID 0");
        println!("air: {track} altitude {altitude} m, ambient {ambient}, road {road}, wind {wind}");
        AirCheck { dll, car, pe, record, slot, altitude, ambient: number(ambient), road: number(road), wind: number(wind), plane: [0.0; 4], lines: String::new(), differ: [0; 4], first: Vec::new() }
    }

    unsafe fn before(&mut self, i: usize) {
        let body = read::<*mut u8>(self.car as usize + 0x118);
        // the body's position (vtable+0xa0, interpolation 0), as the wrapper reads it
        let get_position: extern "C" fn(*mut u8, *mut [f32; 3], f32) -> *mut [f32; 3] = std::mem::transmute(read::<usize>(read::<usize>(body as usize) + 0xa0));
        let mut pos = [0.0f32; 3];
        get_position(body, &mut pos, 0.0);
        put(self.record, 0x418, pos);
        let dll = self.dll;
        let pe = self.pe as usize;
        let saved_vector = (read::<usize>(dll + 0x282_13f0), read::<usize>(dll + 0x282_13f8));
        let saved_pe = (read::<f32>(pe + 0x100), read::<f32>(pe + 0x104), read::<f32>(pe + 0x164));
        let saved_altitude = read::<f32>(dll + 0x27f_a4e0);
        *((dll + 0x282_13f0) as *mut usize) = self.slot as usize;
        *((dll + 0x282_13f8) as *mut usize) = self.slot.add(1) as usize;
        put(self.pe, 0x100, self.ambient);
        put(self.pe, 0x104, self.road);
        put(self.pe, 0x164, self.wind);
        *((dll + 0x27f_a4e0) as *mut f32) = self.altitude;
        let law: extern "C" fn(*const [f32; 3], *mut u8, u8) -> f32 = std::mem::transmute(dll + 0xb1_0410);
        let height_hook: extern "C" fn(*mut u8, *const [f32; 3]) -> f32 = std::mem::transmute(dll + 0x115_0ae0);
        let csp_density = law(self.record.add(0x418) as *const [f32; 3], self.record, 1);
        let csp_height = height_hook(read::<*mut u8>(read::<usize>(self.record as usize + 0x320) + 0xf80), &pos);
        *((dll + 0x282_13f0) as *mut usize) = saved_vector.0;
        *((dll + 0x282_13f8) as *mut usize) = saved_vector.1;
        put(self.pe, 0x100, saved_pe.0);
        put(self.pe, 0x104, saved_pe.1);
        put(self.pe, 0x164, saved_pe.2);
        *((dll + 0x27f_a4e0) as *mut f32) = saved_altitude;
        let z: extern "C" fn(f32, f32, f32, f32) -> f32 = std::mem::transmute(dll + 0xb0_fcc0);
        let csp_temperature = z(self.road, self.ambient, self.wind.abs(), csp_height);

        use rustyac_physics::csp::air;
        let p = V3::new(pos[0], pos[1], pos[2]);
        let height = air::plane_height(&self.plane, &p);
        let temperature = air::near_ground_temperature(self.road, self.ambient, self.wind.abs(), height);
        let density = air::air_density(self.altitude, p.y, self.ambient, self.road, self.wind, height);
        self.lines.push_str(&format!(
            "{i} y {:08x} height {:08x} temperature {:08x} density {:08x}\n",
            pos[1].to_bits(),
            csp_height.to_bits(),
            csp_temperature.to_bits(),
            csp_density.to_bits()
        ));
        for (k, (name, csp, port)) in [("height", csp_height, height), ("temperature", csp_temperature, temperature), ("density", csp_density, density)].into_iter().enumerate() {
            self.compare(k + 1, i, name, &[csp], &[port]);
        }
    }

    unsafe fn after(&mut self, i: usize) {
        let plane: extern "C" fn(*mut u8, *mut [f32; 4]) = std::mem::transmute(self.dll + 0x115_0520);
        plane(self.car, self.record.add(0x424) as *mut [f32; 4]);
        let csp = read::<[f32; 4]>(self.record as usize + 0x424);
        // the four tyres' contact points (Car+0xb28 tyres, 0x858 apart, contactPoint at +0x3f4)
        let contact = |at: usize| {
            let v = read::<[f32; 3]>(self.car as usize + at);
            V3::new(v[0], v[1], v[2])
        };
        self.plane = rustyac_physics::csp::air::ground_plane([contact(0xf1c), contact(0x1774), contact(0x1fcc), contact(0x2824)]);
        let port = self.plane;
        self.compare(0, i, "plane", &csp, &port);
    }

    fn compare(&mut self, k: usize, i: usize, name: &str, csp: &[f32], port: &[f32]) {
        if csp.iter().map(|v| v.to_bits()).ne(port.iter().map(|v| v.to_bits())) {
            self.differ[k] += 1;
            if self.first.len() < 8 {
                self.first.push(format!("step {i} {name}: CSP {csp:?}, port {port:?}"));
            }
        }
    }

    fn finish(self) {
        std::fs::write("air.txt", &self.lines).expect("air.txt can be written");
        for line in &self.first {
            println!("air: {line}");
        }
        let [plane, height, temperature, density] = self.differ;
        println!(
            "air: {} steps, differ: plane {plane}, height {height}, temperature {temperature}, density {density}, air hash {:016x}",
            self.lines.lines().count(),
            fnv(&self.lines)
        );
    }
}

/// CSP's altitude table (dwrite+0x1237c50) for a track folder and layout, given as MSVC wstrings.
/// CSP's near-ground temperature and its law (without a car: base altitude dwrite+0x27fa488, 0.5 m
/// above the ground) over a grid of every input, held against the port's.
unsafe fn grid(dll: usize, pe: *mut u8) {
    use rustyac_physics::csp::air::{air_density, near_ground_temperature};
    let z: extern "C" fn(f32, f32, f32, f32) -> f32 = std::mem::transmute(dll + 0xb0_fcc0);
    let law: extern "C" fn(*const [f32; 3], *mut u8, u8) -> f32 = std::mem::transmute(dll + 0xb1_0410);
    let temperatures: Vec<f32> = (0..=29).map(|i| -15.0 + 2.5 * i as f32).chain([20.0 + 1e-7, 20.0 + 3e-7]).collect();
    let winds: Vec<f32> = (0..=20).map(|i| i as f32).chain([-3.0, 0.25, 1e-8]).collect();
    let heights = [-1.0f32, 0.0, 1e-5, 1e-4, 0.001, 0.003, 0.01, 0.05, 0.1, 0.3, 0.5, 1.0, 2.0, 5.0, 40.0];
    let (mut count, mut differ) = (0usize, 0usize);
    for &road in &temperatures {
        for &ambient in &temperatures {
            for &wind in &winds {
                for &height in &heights {
                    let (csp, port) = (z(road, ambient, wind, height), near_ground_temperature(road, ambient, wind, height));
                    count += 1;
                    if csp.to_bits() != port.to_bits() {
                        if differ < 8 {
                            println!("air: grid: temperature({road}, {ambient}, {wind}, {height}): CSP {csp} {:08x}, port {port} {:08x}", csp.to_bits(), port.to_bits());
                        }
                        differ += 1;
                    }
                }
            }
        }
    }
    println!("air: grid: near-ground temperature, {count} inputs, {differ} differ");
    let saved: [u32; 3] = [read(pe as usize + 0x100), read(pe as usize + 0x104), read(pe as usize + 0x164)];
    let base = (dll + 0x27f_a488) as *mut f32;
    let saved_base = *base;
    let (mut count, mut differ) = (0usize, 0usize);
    for altitude in [-400.0f32, 0.0, 5.0, 294.0, 395.011, 418.0, 1250.0, 2863.0, 9000.0, 44000.0, 60000.0, f32::NAN] {
        for y in [-200.0f32, -1.0, 0.0, 0.3, 1.0, 22.9886, 800.0] {
            for &road in temperatures.iter().step_by(3) {
                for &ambient in temperatures.iter().step_by(3) {
                    for wind in [0.0f32, 2.0, -7.0] {
                        *base = altitude;
                        put(pe, 0x100, ambient);
                        put(pe, 0x104, road);
                        put(pe, 0x164, wind);
                        let csp = law(&[0.0, y, 0.0], std::ptr::null_mut(), 0);
                        let port = air_density(if altitude.is_nan() { 0.0 } else { altitude }, if altitude.is_nan() { 0.0 } else { y }, ambient, road, wind, 0.5);
                        count += 1;
                        if csp.to_bits() != port.to_bits() {
                            if differ < 8 {
                                println!("air: grid: density({altitude}, {y}, {ambient}, {road}, {wind}): CSP {csp} {:08x}, port {port} {:08x}", csp.to_bits(), port.to_bits());
                            }
                            differ += 1;
                        }
                    }
                }
            }
        }
    }
    *base = saved_base;
    put(pe, 0x100, saved[0]);
    put(pe, 0x104, saved[1]);
    put(pe, 0x164, saved[2]);
    println!("air: grid: density, {count} inputs, {differ} differ");
}

unsafe fn table(dll: usize, id: &str, layout: &str) -> f32 {
    let wstring = |text: &str| {
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let length = wide.len() - 1;
        let heap = Box::leak(wide.into_boxed_slice()).as_ptr() as usize;
        // capacity over 7 characters: the first word is the heap pointer
        [heap, 0, length, length.max(8)]
    };
    let (id, layout) = (wstring(id), wstring(layout));
    let table: extern "C" fn(*const [usize; 4], *const [usize; 4]) -> f32 = std::mem::transmute(dll + 0x123_7c50);
    table(&id, &layout)
}

fn record_bytes_word(record: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(record[k * 4..k * 4 + 4].try_into().unwrap())
}

/// `SurfacesManager::loadSurfaceDefinitions`' `WAV_PITCH` reads of a track's surfaces.ini, through
/// the game's `INIReader::getFloat` (CSP's detour at RVA 0x5e4180 reads `extended-N` as N; Vanilla's
/// `std::stof` throws, which ends this process). The path is the game's own form
/// (`content/tracks/<track>/<layout>/data/surfaces.ini`): the detour looks at its end.
unsafe fn wav_pitch(acs: &Acs, track_root: &str, csp: Option<usize>) {
    std::env::set_current_dir(track_root).expect("the track root can be entered");
    if let Some(dll) = csp {
        // the detour's gate: CSP's state object (RVA 0x2821078), its +0x3b8 and that one's +0x60
        // shortcut: +0x3b8 is taken to be the TrackAvatar being built (+0x60 = trackObjects, empty while
        // its SurfacesManager loads) and is a zeroed stand-in; upgrade when the writer of +0x3b8 is read
        let state = read::<usize>(dll + 0x282_1078);
        assert!(state != 0, "CSP's state object is not made");
        if read::<usize>(state + 0x3b8) == 0 {
            put(state as *mut u8, 0x3b8, acs.alloc(0x308));
        }
        println!("CSP state +0x3b8 set: {}", read::<usize>(read::<usize>(state + 0x3b8) + 0x60) == 0);
    }
    let reader = ini_reader(acs, "content/tracks/t/l/data/surfaces.ini");
    let get_float: extern "system" fn(*mut u8, *const u8, *const u8) -> f32 = std::mem::transmute(acs.va(INIREADER_GET_FLOAT));
    println!("reading WAV_PITCH with INIReader::getFloat: {}", through(acs.va(INIREADER_GET_FLOAT)));
    for n in 0.. {
        let section = format!("SURFACE_{n}");
        if !has_section(acs, reader, &section) {
            break;
        }
        let value = get_float(reader, wstring(acs, &section), wstring(acs, "WAV_PITCH"));
        println!("{section} WAV_PITCH {:08x}", value.to_bits());
    }
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

/// Task C4a: every f32 through the C runtime functions CSP's air density calls (its own static
/// copies) and through rustyac_math's pure ports and MSVCR120's, run with the calling thread's
/// MXCSR; prints the number of inputs whose result bits differ.
unsafe fn maths(dll: usize) {
    type F1 = extern "C" fn(f32) -> f32;
    let csr = mxcsr() & 0xffc0;
    let one = |name: &str, csp: Box<dyn Fn(f32) -> f32 + Sync>, pure: fn(f32) -> f32, dll_fn: fn(f32) -> f32| {
        let (mut bad_pure, mut bad_dll, mut first) = (0u64, 0u64, Vec::new());
        let parts: Vec<(u64, u64, Vec<String>)> = std::thread::scope(|scope| {
            let chunks: Vec<_> = (0..16u64)
                .map(|part| {
                    let (csp, pure, dll_fn) = (&csp, pure, dll_fn);
                    scope.spawn(move || {
                        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
                        let (mut a, mut b, mut list) = (0u64, 0u64, Vec::new());
                        for bits in (part << 28)..((part + 1) << 28) {
                            let x = f32::from_bits(bits as u32);
                            let want = csp(x).to_bits();
                            let (p, d) = (pure(x).to_bits(), dll_fn(x).to_bits());
                            if p != want {
                                a += 1;
                                if list.len() < 3 {
                                    list.push(format!("{:#010x}: csp {want:#010x} pure {p:#010x}", bits));
                                }
                            }
                            if d != want {
                                b += 1;
                            }
                        }
                        (a, b, list)
                    })
                })
                .collect();
            chunks.into_iter().map(|c| c.join().unwrap()).collect()
        });
        for (a, b, list) in parts {
            bad_pure += a;
            bad_dll += b;
            first.extend(list);
        }
        first.truncate(6);
        println!("maths {name}: 2^32 inputs, {bad_pure} differ from rustyac_math::pure, {bad_dll} from MSVCR120 {first:?}");
    };
    let f = |rva: usize| std::mem::transmute::<usize, F1>(dll + rva);
    let (expf, atanf, logf) = (f(0x1db_8350), f(0x1dc_7150), f(0x1dc_9b50));
    let powf: extern "C" fn(f32, f32) -> f32 = std::mem::transmute(dll + 0x1db_47a0);
    println!("maths: MXCSR {csr:#06x}, expf(1) {} atanf(1) {} logf(2) {} powf(2, 3) {}", expf(1.0), atanf(1.0), logf(2.0), powf(2.0, 3.0));
    one("expf", Box::new(move |x| expf(x)), rustyac_math::pure::expf, rustyac_math::expf);
    one("atanf", Box::new(move |x| atanf(x)), rustyac_math::pure::atanf, rustyac_math::atanf);
    one("logf", Box::new(move |x| logf(x)), rustyac_math::pure::logf, rustyac_math::logf);
    // the law's only exponent (0x238ba88)
    const Y: f32 = f32::from_bits(0x40a8_2f53);
    one("powf(x, 5.2557769)", Box::new(move |x| powf(x, Y)), |x| rustyac_math::pure::powf(x, Y), |x| rustyac_math::powf(x, Y));
}

fn main() {
    let mut arguments: Vec<String> = std::env::args().collect();
    // --islands <file>: recorded CSP islands stepped by rustyac-ode's csp_step (Task C5d), no AC
    if let Some(i) = arguments.iter().position(|a| a == "--world") {
        return island::check_world(&arguments[i + 1]);
    }
    if let Some(i) = arguments.iter().position(|a| a == "--islands") {
        return island::check(&arguments[i + 1]);
    }
    // --maths: CSP's C runtime functions of the air density against rustyac_math (Task C4a)
    let maths_check = arguments.iter().position(|a| a == "--maths").map(|i| arguments.remove(i)).is_some();
    // --ode <breaks.txt>: CSP's ODE functions against rustyac-ode's f64 ones on that drive's inputs (Task C5c)
    let ode_check = arguments.iter().position(|a| a == "--ode").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    // --solver <breaks.txt>: CSP's solver kernels against rustyac-ode's csp_matrix (Task C5d)
    let solver_check = arguments.iter().position(|a| a == "--solver").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    // --car <name>: load that car after CSP's start-up; --vanilla: the same without CSP
    let car_name = arguments.iter().position(|a| a == "--car").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    let vanilla = arguments.iter().position(|a| a == "--vanilla").map(|i| arguments.remove(i)).is_some();
    // --car-data <folder>: the car's plain data files (default cardata/<name>), as car_oracle reads them
    let car_data = arguments.iter().position(|a| a == "--car-data").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    // --surfaces <file>: read that track surfaces.ini's WAV_PITCH values (in place of a car)
    let surfaces = arguments.iter().position(|a| a == "--surfaces").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    // --drive <file.ryin> [--steps n] [--cars n] [--workers n]: after the car load, step the physics with
    // that tape's records (Task C4a); --workers sets CSP's worker count (dword RVA 0x267b338), 0 by default
    let mut number = |flag: &str| {
        arguments.iter().position(|a| a == flag).map(|i| {
            arguments.remove(i);
            arguments.remove(i).parse::<usize>().expect("a number after the flag")
        })
    };
    let (steps, cars, workers) = (number("--steps").unwrap_or(usize::MAX), number("--cars").unwrap_or(1), number("--workers").unwrap_or(0));
    let drive_tape = arguments.iter().position(|a| a == "--drive").map(|i| {
        arguments.remove(i);
        full_path(&arguments.remove(i))
    });
    // --still: zero pedals and wheel; --air track:ambient:road:wind: CSP's air density each step (part C)
    let still = arguments.iter().position(|a| a == "--still").map(|i| arguments.remove(i)).is_some();
    // --track <folder>: the car on that real track (Task C5b, car_oracle's track.rs), ghost, in place of the flat road
    let track_folder = arguments.iter().position(|a| a == "--track").map(|i| {
        arguments.remove(i);
        std::path::PathBuf::from(arguments.remove(i))
    });
    let track_name = track_folder.as_ref().map(|f| f.file_name().expect("the track folder has a name").to_string_lossy().into_owned());
    let drive_track = track_name.as_deref().zip(car_name.as_deref());
    let air_spec = arguments.iter().position(|a| a == "--air").map(|i| {
        arguments.remove(i);
        arguments.remove(i)
    });
    if arguments.len() != 3 && arguments.len() != 4 {
        eprintln!("usage: csp_oracle <ac root> <run folder> [cfg folder] [--car <name> [--drive <file.ryin> [--steps n] [--cars n] [--workers n]] | --surfaces <file>] [--vanilla]");
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
    // the car's root, as car_oracle's prepare_root: plain data files and a physics without a thread pool
    let car_root = format!("{run}\\car_root");
    if let Some(car) = &car_name {
        let from = car_data.clone().unwrap_or(format!("cardata/{car}"));
        copy_tree(std::path::Path::new(&from), std::path::Path::new(&format!("{car_root}\\content\\cars\\{car}\\data"))).expect("the car's data can be copied");
        std::fs::create_dir_all(format!("{car_root}\\system\\cfg")).unwrap();
        std::fs::write(
            format!("{car_root}\\system\\cfg\\assetto_corsa.ini"),
            "[PHYSICS_THREADING]\nTHREADS=0\n[FF_EXPERIMENTAL]\nENABLE_GYRO=0\nDAMPER_MIN_LEVEL=0\nDAMPER_GAIN=1\n[LOW_SPEED_FF]\nSPEED_KMH=3\nMIN_VALUE=0.01\n",
        )
        .unwrap();
        if let Some(folder) = &track_folder {
            track::prepare_root(std::path::Path::new(&car_root), folder, "").unwrap_or_else(|e| stop(2, &format!("the track's root: {e}")));
        }
    }
    let track_root = format!("{run}\\track_root");
    if let Some(file) = &surfaces {
        std::fs::create_dir_all(format!("{track_root}\\content\\tracks\\t\\l\\data")).unwrap();
        std::fs::copy(file, format!("{track_root}\\content\\tracks\\t\\l\\data\\surfaces.ini")).expect("the surfaces file can be copied");
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
        if let (Some(car), true) = (&car_name, vanilla) {
            let (engine, cars) = load_car(&acs, car, &car_root, None, cars, track_folder.as_deref());
            if let Some(tape) = &drive_tape {
                drive(&acs, engine, &cars, tape, steps, still, None, None, drive_track);
            }
            stop(0, "end of the run");
        }
        if let (Some(_), true) = (&surfaces, vanilla) {
            wav_pitch(&acs, &track_root, None);
            stop(0, "end of the run");
        }

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
        arm_breaks(dll as usize);
        checkpoint("loaded", &before, &functions);

        ask("with CSP, reader built before CSP", reader);
        ask("with CSP, reader built now", build());
        checkpoint("called", &before, &functions);
        if maths_check {
            maths(dll as usize);
            stop(0, "end of the run");
        }
        if let Some(breaks) = &ode_check {
            ode::run(dll as usize, breaks);
            stop(0, "end of the run");
        }
        if let Some(breaks) = &solver_check {
            solver::run(dll as usize, breaks);
            stop(0, "end of the run");
        }

        if let Some(car) = &car_name {
            // wWinMain's first object: CSP's detour of its constructor does the set-up the game's
            // log shows after "Starting loading…" (race configuration, core modules)
            let guard = acs.alloc(CRASH_GUARD_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(CRASH_GUARD_CTOR));
            ctor(guard, wstring(&acs, ""));
            println!("CrashGuard built");
            checkpoint("crash guard", &before, &functions);
            let dll = dll as usize;
            // what CSP's PhysicsDriveThread::run (RVA 0x67d840) does on the game's physics thread before
            // its loop: its one-time set-up (RVA 0x67d6a0) names the thread through RVA 0x15d8e20, which
            // sets FTZ and DAZ (RVA 0x15d9c70), and stores the thread's id (dword RVA 0x2814e50); its first
            // job (RVA 0x67d8a0) clears three state bytes
            // shortcut: the set-up's thread name, priority, start time and its +122 one-time call on the
            // thread object are left out, upgrade if a physics seam reads them
            let physics = PhysicsThread::start();
            physics.run(|| {
                THREADS.lock().unwrap().push(GetCurrentThreadId());
                let csr = mxcsr() | 0x8040;
                std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
                *((dll + 0x281_4e50) as *mut u32) = GetCurrentThreadId();
                let first_job: extern "C" fn() = std::mem::transmute(dll + 0x67_d8a0);
                first_job();
                say(&format!("physics thread set up, MXCSR {:#06x}", mxcsr()));
            });
            println!("CSP worker count {} set to {workers}", read::<u32>(dll + 0x267_b338));
            *((dll + 0x267_b338) as *mut u32) = workers as u32;
            let (engine, cars) = load_car(&acs, car, &car_root, Some(dll), cars, track_folder.as_deref());
            checkpoint("car", &before, &functions);
            if let Some(tape) = &drive_tape {
                // CSP's loading jobs (AI grids, AI payloads) run on its job pool and change the car's
                // physics when they land; the game loads for seconds before a session, so wait for
                // them as the game would have, not for a clock (worker decrement: dwrite+0x572d50)
                let since = std::time::Instant::now();
                while POOLS.lock().unwrap().iter().any(|&pool| read::<i32>(pool + 696) != 0) {
                    if since.elapsed().as_secs() > 120 {
                        stop(2, "CSP's job pool is still busy after 120 s");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                say(&format!("CSP's job pools idle after {} ms", since.elapsed().as_millis()));
                physics.run(|| drive(&acs, engine, &cars, tape, steps, still, Some(dll), air_spec.as_deref(), drive_track));
                checkpoint("drive", &before, &functions);
            }
            stop(0, "end of the run");
        }
        if surfaces.is_some() {
            let guard = acs.alloc(CRASH_GUARD_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(CRASH_GUARD_CTOR));
            ctor(guard, wstring(&acs, ""));
            wav_pitch(&acs, &track_root, Some(dll as usize));
            stop(0, "end of the run");
        }

        // real time for CSP's own threads, to see what they do when left alone
        Sleep(1500);
        checkpoint("settled", &before, &functions);
    }
    stop(0, "end of the run");
}
