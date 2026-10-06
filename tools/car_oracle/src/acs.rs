//! Maps acs.exe into this process as a plain image and prepares it so that the game's own
//! physics code can be called. The game's entry point is never run and nothing in the file on
//! disk is touched; everything written goes to our private copy-on-write mapping:
//! the import table (C/C++ runtime and kernel32 are bound, the clock functions are replaced by
//! a deterministic clock, every other import gets a stub that reports the call and stops), a
//! handful of globals, and the entry bytes of the functions the recorder wraps.

use std::ffi::{c_void, CString, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// `static bool INIReader::useCache` (initially true). The cache itself is a static
/// `std::map` built by a start-up initialiser that never runs here, so it is switched off:
/// every `INIReader` then reads its file, which gives the same values.
const RVA_INIREADER_USE_CACHE: usize = 0x151d0f9;
/// PE TimeDateStamp of the build all addresses in this crate belong to (1.16.4).
const EXPECTED_TIMESTAMP: u32 = 0x5a55e7a8;
/// Ghidra's image base: addresses in the maps and in this crate are written as `0x14…`.
pub const GHIDRA_BASE: usize = 0x1_4000_0000;

const DONT_RESOLVE_DLL_REFERENCES: u32 = 0x1;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const MEM_COMMIT_RESERVE: u32 = 0x3000;

/// Imports of these DLLs are bound for real. Everything else the image imports (Direct3D,
/// FMOD, Steam, sockets, input, the shell …) is never loaded.
const BOUND_DLLS: [&str; 3] = ["msvcr120.dll", "msvcp120.dll", "kernel32.dll"];

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
    fn GetLastError() -> u32;
    fn GetStdHandle(which: u32) -> *mut c_void;
    fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn DuplicateHandle(
        source_process: *mut c_void,
        source: *mut c_void,
        target_process: *mut c_void,
        target: *mut *mut c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut c_void;
    fn SetUnhandledExceptionFilter(
        handler: extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut c_void;
    fn SetErrorMode(mode: u32) -> u32;
}

#[repr(C)]
struct ExceptionPointers {
    record: *const ExceptionRecord,
    context: *const u8,
}

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *const ExceptionRecord,
    address: usize,
    parameter_count: u32,
    information: [usize; 15],
}

static IMAGE_BASE: AtomicUsize = AtomicUsize::new(0);
static IMAGE_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Function names for crash reports, from the local index if it is there (`re/index` is not
/// part of the repository; without it only addresses are printed).
fn function_name(ghidra: usize) -> String {
    static TABLE: std::sync::OnceLock<Vec<(usize, usize, String)>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../re/index/functions.tsv");
        let mut rows = Vec::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines().skip(1) {
                let mut cells = line.split('\t');
                let (Some(va), Some(size)) = (cells.next(), cells.next()) else {
                    continue;
                };
                let name = cells.nth(2).unwrap_or("");
                if let (Ok(va), Ok(size)) = (usize::from_str_radix(va, 16), size.parse::<usize>()) {
                    rows.push((va, size, name.to_string()));
                }
            }
            rows.sort();
        }
        rows
    });
    let i = table.partition_point(|row| row.0 <= ghidra);
    if i > 0 {
        let (va, size, name) = &table[i - 1];
        if ghidra < va + size.max(&1) {
            return format!("{name} +{:#x}", ghidra - va);
        }
    }
    String::new()
}

/// `address` as a Ghidra address if it lies in the mapped image.
pub fn ghidra_of(address: usize) -> Option<usize> {
    let offset = address.wrapping_sub(IMAGE_BASE.load(Ordering::Relaxed));
    (offset < IMAGE_SIZE.load(Ordering::Relaxed)).then_some(GHIDRA_BASE + offset)
}

/// Prints the stack slots that point into acs.exe: a rough call stack.
unsafe fn print_stack(rsp: *const usize, limit: usize) {
    let mut shown = 0;
    for i in 0..1200 {
        let value = *rsp.add(i);
        if let Some(ghidra) = ghidra_of(value) {
            if ghidra < GHIDRA_BASE + 0x4a4000 && ghidra > GHIDRA_BASE + 0x1000 {
                eprintln!("  stack[{i}]: acs.exe {ghidra:#x}  {}", function_name(ghidra));
                shown += 1;
                if shown == limit {
                    break;
                }
            }
        }
    }
}

/// A crash inside the game's code would otherwise just end the process: say where it was,
/// as addresses that can be looked up in Ghidra / `tools/disasm.py who`.
extern "system" fn report_crash(pointers: *mut ExceptionPointers) -> i32 {
    unsafe {
        let record = &*(*pointers).record;
        // only hard faults; C++ exceptions and debugger notifications pass through (they reach
        // `report_unhandled` if nothing catches them)
        if record.code >> 28 != 0xc {
            return 0;
        }
        describe_crash(pointers)
    }
}

/// Anything nobody handled: the game's own "critical error" (code 0x29a, raised after it has
/// printed the reason) or a C++ exception (0xe06d7363; the physics code has no handlers).
extern "system" fn report_unhandled(pointers: *mut ExceptionPointers) -> i32 {
    unsafe { describe_crash(pointers) }
}

unsafe fn describe_crash(pointers: *mut ExceptionPointers) -> ! {
    {
        let record = &*(*pointers).record;
        // the game's own messages first: its stdout is buffered
        if let Some(flush) = FFLUSH.get() {
            flush(std::ptr::null_mut());
        }
        match record.code {
            0x29a => eprintln!("the game's code raised its own critical error (see its message above)"),
            0xe06d_7363 => eprintln!("the game's code threw a C++ exception that nothing catches"),
            _ => {}
        }
        eprintln!(
            "the game's code crashed: exception {:#x} at {:#x}",
            record.code, record.address
        );
        if let Some(ghidra) = ghidra_of(record.address) {
            eprintln!("  = acs.exe {ghidra:#x}  {}", function_name(ghidra));
        }
        if record.code == 0xc000_0005 {
            eprintln!(
                "  access violation {} address {:#x}",
                match record.information[0] {
                    0 => "reading",
                    1 => "writing",
                    _ => "executing",
                },
                record.information[1]
            );
        }
        // CONTEXT: Rax 0x78, Rcx 0x80, Rdx 0x88, Rbx 0x90, Rsp 0x98, Rbp 0xa0, Rsi 0xa8,
        // Rdi 0xb0, R8.. 0xb8
        let context = (*pointers).context;
        let reg = |offset: usize| *(context.add(offset) as *const usize);
        eprintln!(
            "  rax {:#x} rcx {:#x} rdx {:#x} rbx {:#x} rsi {:#x} rdi {:#x} r8 {:#x} r9 {:#x}",
            reg(0x78),
            reg(0x80),
            reg(0x88),
            reg(0x90),
            reg(0xa8),
            reg(0xb0),
            reg(0xb8),
            reg(0xc0)
        );
        print_stack(reg(0x98) as *const usize, 24);
        std::process::exit(3);
    }
}

/// msvcr120 `fflush`, for the crash reports.
static FFLUSH: std::sync::OnceLock<extern "C" fn(*mut u8) -> i32> = std::sync::OnceLock::new();

// --- stand-ins for imports -------------------------------------------------------------------

/// Names of the imports that were not bound, by stub number.
static UNBOUND: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

extern "C" fn unbound_import(index: u32) -> ! {
    let name = UNBOUND
        .get()
        .and_then(|names| names.get(index as usize).cloned())
        .unwrap_or_default();
    eprintln!("the game's code called an import the oracle does not provide: {name}");
    let marker = 0usize;
    unsafe { print_stack(&marker as *const usize, 16) };
    std::process::exit(4);
}

extern "C" fn game_exit(code: i32) -> ! {
    eprintln!("the game's code called exit({code})");
    let marker = 0usize;
    unsafe { print_stack(&marker as *const usize, 16) };
    std::process::exit(5);
}

extern "C" fn game_abort() -> ! {
    eprintln!("the game's code called abort()");
    let marker = 0usize;
    unsafe { print_stack(&marker as *const usize, 16) };
    std::process::exit(5);
}

/// No key is ever down (the game asks in two places: an engine restart key and an AI-line tool).
extern "system" fn fake_get_async_key_state(_key: i32) -> i16 {
    0
}

/// ODE reports an internal error with a message box and then aborts: print it instead.
extern "system" fn fake_message_box_a(_window: usize, text: *const i8, caption: *const i8, _kind: u32) -> i32 {
    let read = |p: *const i8| {
        if p.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    };
    eprintln!("the game's code opened a message box: {} / {}", read(caption), read(text));
    1
}

/// The oracle's clock: every query moves it on by a fixed amount, so nothing the game derives
/// from wall-clock time can differ between two runs.
static CLOCK_TICKS: AtomicU64 = AtomicU64::new(0);
pub const CLOCK_FREQUENCY: u64 = 10_000_000;
const CLOCK_STEP: u64 = 1_000; // 0.1 ms per query

fn clock_tick() -> u64 {
    CLOCK_TICKS.fetch_add(CLOCK_STEP, Ordering::Relaxed) + CLOCK_STEP
}

/// Restarts the fake clock (called before every scenario).
pub fn reset_clock() {
    CLOCK_TICKS.store(0, Ordering::Relaxed);
}

extern "system" fn fake_query_performance_counter(out: *mut i64) -> i32 {
    unsafe { *out = clock_tick() as i64 };
    1
}

extern "system" fn fake_query_performance_frequency(out: *mut i64) -> i32 {
    unsafe { *out = CLOCK_FREQUENCY as i64 };
    1
}

extern "system" fn fake_time_get_time() -> u32 {
    (clock_tick() / (CLOCK_FREQUENCY / 1000)) as u32
}

extern "system" fn fake_get_tick_count() -> u32 {
    (clock_tick() / (CLOCK_FREQUENCY / 1000)) as u32
}

extern "system" fn fake_get_tick_count64() -> u64 {
    clock_tick() / (CLOCK_FREQUENCY / 1000)
}

/// A fixed date (2020-01-01) plus the fake clock, as a FILETIME.
extern "system" fn fake_get_system_time_as_file_time(out: *mut u64) {
    unsafe { out.write_unaligned(132_223_104_000_000_000 + clock_tick()) };
}

/// Imports that are replaced instead of bound: (dll, function, stand-in).
fn overrides() -> Vec<(&'static str, &'static str, usize)> {
    vec![
        ("msvcr120.dll", "exit", game_exit as *const () as usize),
        ("msvcr120.dll", "_exit", game_exit as *const () as usize),
        ("msvcr120.dll", "abort", game_abort as *const () as usize),
        (
            "kernel32.dll",
            "QueryPerformanceCounter",
            fake_query_performance_counter as *const () as usize,
        ),
        (
            "kernel32.dll",
            "QueryPerformanceFrequency",
            fake_query_performance_frequency as *const () as usize,
        ),
        ("kernel32.dll", "GetTickCount", fake_get_tick_count as *const () as usize),
        ("kernel32.dll", "GetTickCount64", fake_get_tick_count64 as *const () as usize),
        (
            "kernel32.dll",
            "GetSystemTimeAsFileTime",
            fake_get_system_time_as_file_time as *const () as usize,
        ),
        ("winmm.dll", "timeGetTime", fake_time_get_time as *const () as usize),
        ("user32.dll", "GetAsyncKeyState", fake_get_async_key_state as *const () as usize),
        ("user32.dll", "MessageBoxA", fake_message_box_a as *const () as usize),
    ]
}

// --- executable scratch memory ---------------------------------------------------------------

/// A block of executable memory for import stubs and detour trampolines.
pub struct ExecMem {
    base: *mut u8,
    used: usize,
    size: usize,
}

impl ExecMem {
    fn new(size: usize) -> Result<ExecMem, String> {
        let base = unsafe {
            VirtualAlloc(std::ptr::null_mut(), size, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE)
        } as *mut u8;
        if base.is_null() {
            return Err(format!("VirtualAlloc failed, error {}", unsafe { GetLastError() }));
        }
        Ok(ExecMem { base, used: 0, size })
    }

    /// Copies `code` in and returns its address.
    fn push(&mut self, code: &[u8]) -> usize {
        assert!(self.used + code.len() <= self.size, "out of stub memory");
        let at = unsafe { self.base.add(self.used) };
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), at, code.len()) };
        self.used += code.len().next_multiple_of(16);
        at as usize
    }
}

pub struct Acs {
    base: usize,
    /// MSVCR120 `operator new(size_t)`: memory the game may later `operator delete`.
    operator_new: extern "C" fn(usize) -> *mut u8,
    crt: *mut c_void,
    exec: std::cell::RefCell<ExecMem>,
}

impl Acs {
    pub fn load(path: &Path) -> Result<Acs, String> {
        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        // Mapped as an image with relocations applied; no imports resolved, no code run.
        let handle = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), DONT_RESOLVE_DLL_REFERENCES)
        };
        if handle.is_null() {
            return Err(format!(
                "LoadLibraryExW({}) failed, error {}",
                path.display(),
                unsafe { GetLastError() }
            ));
        }
        let crt = unsafe { LoadLibraryA(c"msvcr120.dll".as_ptr().cast()) };
        if crt.is_null() {
            return Err(
                "msvcr120.dll not found (Visual C++ 2013 x64 runtime, which the game needs)".into(),
            );
        }
        let operator_new = unsafe { GetProcAddress(crt, c"??2@YAPEAX_K@Z".as_ptr().cast()) };
        if operator_new.is_null() {
            return Err("msvcr120.dll has no operator new".into());
        }
        let acs = Acs {
            base: handle as usize,
            operator_new: unsafe {
                std::mem::transmute::<*mut c_void, extern "C" fn(usize) -> *mut u8>(operator_new)
            },
            crt,
            exec: std::cell::RefCell::new(ExecMem::new(0x20000)?),
        };
        IMAGE_BASE.store(acs.base, Ordering::Relaxed);
        unsafe {
            let size_of_image = acs.read::<u32>(acs.nt_headers() + 24 + 56) as usize;
            IMAGE_SIZE.store(size_of_image, Ordering::Relaxed);
            AddVectoredExceptionHandler(1, report_crash);
            SetUnhandledExceptionFilter(report_unhandled);
            // a failing C runtime must say so on stderr and end, not wait on a dialog box
            const SEM_FAILCRITICALERRORS_NOGPFAULTERRORBOX: u32 = 0x1 | 0x2;
            SetErrorMode(SEM_FAILCRITICALERRORS_NOGPFAULTERRORBOX);
            let set_app_type: extern "C" fn(i32) = std::mem::transmute(acs.crt_function(c"__set_app_type"));
            set_app_type(1); // console
            let set_abort_behavior: extern "C" fn(u32, u32) -> u32 =
                std::mem::transmute(acs.crt_function(c"_set_abort_behavior"));
            set_abort_behavior(0, 3);
            let set_error_mode: extern "C" fn(i32) -> i32 = std::mem::transmute(acs.crt_function(c"_set_error_mode"));
            set_error_mode(1); // to stderr
            let _ = FFLUSH.set(std::mem::transmute::<usize, extern "C" fn(*mut u8) -> i32>(acs.crt_function(c"fflush")));
            acs.check_build()?;
            acs.bind_imports()?;
            if acs.read::<u8>(RVA_INIREADER_USE_CACHE) != 1 {
                return Err("INIReader::useCache is not where it is expected".into());
            }
            // .data is mapped copy-on-write: this changes our private copy only
            ((acs.base + RVA_INIREADER_USE_CACHE) as *mut u8).write(0);
        }
        Ok(acs)
    }

    /// Address in this process of a Ghidra address (`0x14…`).
    pub fn va(&self, ghidra: usize) -> usize {
        debug_assert!(ghidra >= GHIDRA_BASE);
        self.base + (ghidra - GHIDRA_BASE)
    }

    /// Memory from the game's own allocator, zeroed.
    pub fn alloc(&self, size: usize) -> *mut u8 {
        let p = (self.operator_new)(size);
        unsafe { p.write_bytes(0, size) };
        p
    }

    /// A function of the C runtime the game uses (same heap, same `rand` state).
    pub fn crt_function(&self, name: &std::ffi::CStr) -> usize {
        let f = unsafe { GetProcAddress(self.crt, name.as_ptr().cast()) };
        assert!(!f.is_null(), "msvcr120.dll has no {name:?}");
        f as usize
    }

    /// Makes the game's own `printf` output appear at once (it is fully buffered when piped).
    pub fn unbuffer_game_stdout(&self) {
        unsafe {
            let iob: extern "C" fn() -> *mut u8 = std::mem::transmute(self.crt_function(c"__iob_func"));
            let setvbuf: extern "C" fn(*mut u8, *mut u8, i32, usize) -> i32 =
                std::mem::transmute(self.crt_function(c"setvbuf"));
            const IONBF: i32 = 4;
            setvbuf(iob().add(48), std::ptr::null_mut(), IONBF, 0);
        }
    }

    /// Sends the game's own `printf` output (it reports every key it loads) to NUL.
    pub fn silence_game_stdout(&self) {
        unsafe {
            let iob: extern "C" fn() -> *mut u8 = std::mem::transmute(self.crt_function(c"__iob_func"));
            let freopen: extern "C" fn(*const u8, *const u8, *mut u8) -> *mut u8 =
                std::mem::transmute(self.crt_function(c"freopen"));
            // freopen closes the process's stdout handle: keep a duplicate for our own output
            const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
            const DUPLICATE_SAME_ACCESS: u32 = 2;
            let mut ours = std::ptr::null_mut();
            let process = GetCurrentProcess();
            if DuplicateHandle(
                process,
                GetStdHandle(STD_OUTPUT_HANDLE),
                process,
                &mut ours,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            ) == 0
            {
                return;
            }
            // FILE is 48 bytes in this runtime; stdout is the second entry
            freopen(c"NUL".as_ptr().cast(), c"w".as_ptr().cast(), iob().add(48));
            SetStdHandle(STD_OUTPUT_HANDLE, ours);
        }
    }

    /// Overwrites bytes of the mapped image (our private copy), whatever the page protection.
    pub unsafe fn patch(&self, address: usize, bytes: &[u8]) {
        let mut old = 0u32;
        let p = address as *mut c_void;
        assert!(
            VirtualProtect(p, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) != 0,
            "VirtualProtect failed, error {}",
            GetLastError()
        );
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        let mut ignored = 0u32;
        VirtualProtect(p, bytes.len(), old, &mut ignored);
        FlushInstructionCache(GetCurrentProcess(), p, bytes.len());
    }

    /// Wraps a game function: its first `prologue` bytes (whole instructions, none of them
    /// position-dependent) are replaced by a jump to `hook`. Returns the address through which
    /// the original function can still be called.
    pub unsafe fn detour(&self, ghidra: usize, prologue: usize, expected: &[u8], hook: usize) -> usize {
        self.detour_with(ghidra, prologue, expected, &expected[..prologue], hook)
    }

    /// As [`Acs::detour`], for a prologue that cannot be copied as it is: `relocated` is code
    /// that does the same as the first `prologue` bytes when run from anywhere.
    pub unsafe fn detour_with(
        &self,
        ghidra: usize,
        prologue: usize,
        expected: &[u8],
        relocated: &[u8],
        hook: usize,
    ) -> usize {
        assert!(prologue >= 12 && expected.len() >= prologue, "a detour needs 12 known bytes");
        let target = self.va(ghidra);
        let original = std::slice::from_raw_parts(target as *const u8, expected.len());
        assert!(original == expected, "{ghidra:#x}: the function does not start with the expected bytes");
        // trampoline: the displaced instructions, then jmp [rip+0] -> target + prologue
        let mut code = relocated.to_vec();
        code.extend([0xff, 0x25, 0, 0, 0, 0]);
        code.extend(((target + prologue) as u64).to_le_bytes());
        let trampoline = self.exec.borrow_mut().push(&code);
        // entry: mov rax, hook; jmp rax (rax is free at a function's entry), padded with int3
        let mut entry = vec![0x48, 0xb8];
        entry.extend((hook as u64).to_le_bytes());
        entry.extend([0xff, 0xe0]);
        entry.resize(prologue, 0xcc);
        self.patch(target, &entry);
        trampoline
    }

    /// Reads a global of the mapped image.
    pub unsafe fn global<T: Copy>(&self, ghidra: usize) -> T {
        std::ptr::read_unaligned(self.va(ghidra) as *const T)
    }

    /// Writes a global of the mapped image (.data is copy-on-write: our copy only).
    pub unsafe fn set_global<T>(&self, ghidra: usize, value: T) {
        std::ptr::write_unaligned(self.va(ghidra) as *mut T, value);
    }

    unsafe fn read<T: Copy>(&self, rva: usize) -> T {
        std::ptr::read_unaligned((self.base + rva) as *const T)
    }

    unsafe fn nt_headers(&self) -> usize {
        self.read::<u32>(0x3c) as usize
    }

    unsafe fn check_build(&self) -> Result<(), String> {
        let nt = self.nt_headers();
        let stamp = self.read::<u32>(nt + 8);
        if stamp != EXPECTED_TIMESTAMP {
            return Err(format!(
                "acs.exe build timestamp {stamp:#x} != expected {EXPECTED_TIMESTAMP:#x}; \
                 the hard-coded addresses do not apply to this build"
            ));
        }
        Ok(())
    }

    unsafe fn cstr(&self, rva: usize) -> String {
        let mut bytes = Vec::new();
        let mut p = (self.base + rva) as *const u8;
        while *p != 0 {
            bytes.push(*p);
            p = p.add(1);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Fills the import address table: real functions for the bound DLLs (with the stand-ins
    /// of `overrides`), a reporting stub for everything else.
    unsafe fn bind_imports(&self) -> Result<(), String> {
        let opt = self.nt_headers() + 24;
        let import_rva = self.read::<u32>(opt + 112 + 8) as usize;
        let overrides = overrides();
        let mut unbound = Vec::new();
        let mut desc = import_rva;
        let mut bound = 0;
        loop {
            let ilt = self.read::<u32>(desc) as usize;
            let name_rva = self.read::<u32>(desc + 12) as usize;
            let iat = self.read::<u32>(desc + 16) as usize;
            if name_rva == 0 {
                break;
            }
            let dll = self.cstr(name_rva).to_ascii_lowercase();
            let real = BOUND_DLLS.contains(&dll.as_str());
            self.bind_one(&dll, if ilt != 0 { ilt } else { iat }, iat, real, &overrides, &mut unbound)?;
            bound += real as usize;
            desc += 20;
        }
        if bound != BOUND_DLLS.len() {
            return Err("acs.exe does not import the expected runtime DLLs".into());
        }
        let _ = UNBOUND.set(unbound);
        Ok(())
    }

    unsafe fn bind_one(
        &self,
        dll: &str,
        ilt: usize,
        iat: usize,
        real: bool,
        overrides: &[(&str, &str, usize)],
        unbound: &mut Vec<String>,
    ) -> Result<(), String> {
        let module = if real {
            let cdll = CString::new(dll).unwrap();
            let module = LoadLibraryA(cdll.as_ptr() as *const u8);
            if module.is_null() {
                return Err(format!("{dll} not found (the game itself needs it)"));
            }
            module
        } else {
            std::ptr::null_mut()
        };
        let mut count = 0;
        while self.read::<u64>(ilt + count * 8) != 0 {
            count += 1;
        }
        let table = (self.base + iat) as *mut c_void;
        let mut old = 0u32;
        if VirtualProtect(table, count * 8, PAGE_READWRITE, &mut old) == 0 {
            return Err(format!("VirtualProtect failed, error {}", GetLastError()));
        }
        for i in 0..count {
            let entry = self.read::<u64>(ilt + i * 8);
            let by_ordinal = entry >> 63 != 0;
            let name = if by_ordinal {
                format!("#{}", entry & 0xffff)
            } else {
                // IMAGE_IMPORT_BY_NAME: u16 hint, then the name
                self.cstr(entry as usize + 2)
            };
            // a stand-in wins whether or not the DLL itself is bound
            let replaced = overrides
                .iter()
                .find(|(d, f, _)| *d == dll && *f == name)
                .map(|o| o.2);
            let target = if let Some(stand_in) = replaced {
                stand_in
            } else if real {
                let found = if by_ordinal {
                    GetProcAddress(module, (entry & 0xffff) as usize as *const u8)
                } else {
                    let cname = CString::new(name.clone()).unwrap();
                    GetProcAddress(module, cname.as_ptr() as *const u8)
                };
                if found.is_null() {
                    return Err(format!("{dll}: import {name} could not be resolved"));
                }
                found as usize
            } else {
                // mov ecx, index; mov rax, unbound_import; jmp rax
                let mut code = vec![0xb9];
                code.extend((unbound.len() as u32).to_le_bytes());
                code.extend([0x48, 0xb8]);
                code.extend((unbound_import as *const () as u64).to_le_bytes());
                code.extend([0xff, 0xe0]);
                unbound.push(format!("{dll}!{name}"));
                self.exec.borrow_mut().push(&code)
            };
            std::ptr::write_unaligned((self.base + iat + i * 8) as *mut u64, target as u64);
        }
        let mut ignored = 0u32;
        VirtualProtect(table, count * 8, old, &mut ignored);
        Ok(())
    }
}
