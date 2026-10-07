// SPDX-License-Identifier: MIT OR Apache-2.0

//! Maps acs.exe into this process as a plain image and exposes the functions the oracle
//! calls. The game's entry point is never run and nothing in the file on disk is touched; the
//! only memory written is the import table of our private mapping (C/C++ runtime and
//! kernel32 imports only, no game DLL is loaded).

use std::ffi::{c_void, CString, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// Addresses are RVAs (Ghidra address - 0x140000000), names from acs.pdb.
pub const RVA_TYRE_CTOR: usize = 0x26dbd0; // Tyre::Tyre
pub const RVA_TYRE_INIT: usize = 0x280650; // Tyre::init
pub const RVA_TYRE_SET_COMPOUND: usize = 0x2834e0; // Tyre::setCompound
pub const RVA_TYRE_STEP: usize = 0x283800; // Tyre::step
pub const RVA_SCTM_SOLVE: usize = 0x44bc20; // SCTM::solve
pub const RVA_SCTM_VFTABLE: usize = 0x1416580; // SCTM::`vftable' (slot 1 = solve)
/// `static bool INIReader::useCache` (initially true). The cache itself is a static
/// `std::map` built by a start-up initialiser that never runs here, so it is switched off:
/// every `INIReader` then reads its file, which gives the same values.
const RVA_INIREADER_USE_CACHE: usize = 0x151d0f9;

/// PE TimeDateStamp of the build the RVAs above belong to.
const EXPECTED_TIMESTAMP: u32 = 0x5a55e7a8;

const DONT_RESOLVE_DLL_REFERENCES: u32 = 0x1;
const PAGE_READWRITE: u32 = 0x04;
/// Only these imports are bound: what `Tyre::init` and `Tyre::step` reach is the C runtime
/// (maths, memory, printf, number parsing), the C++ runtime (strings, file streams) and
/// kernel32 (file attributes).
const BOUND_DLLS: [&str; 3] = ["msvcr120.dll", "msvcp120.dll", "kernel32.dll"];

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
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

static IMAGE_BASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A crash inside the game's code would otherwise just end the process: say where it was,
/// as addresses that can be looked up in Ghidra / `tools/disasm.py who`.
extern "system" fn report_crash(pointers: *mut ExceptionPointers) -> i32 {
    unsafe {
        let record = &*(*pointers).record;
        // only hard faults; C++ exceptions and debugger notifications pass through
        if record.code >> 28 != 0xc {
            return 0;
        }
        let base = IMAGE_BASE.load(std::sync::atomic::Ordering::Relaxed);
        let ghidra = |address: usize| 0x1_4000_0000 + address.wrapping_sub(base);
        eprintln!(
            "the game's code crashed: exception {:#x} at {:#x}",
            record.code, record.address
        );
        if record.address.wrapping_sub(base) < 0x200_0000 {
            eprintln!("  = acs.exe {:#x} (Ghidra address)", ghidra(record.address));
        }
        if record.code == 0xc000_0005 {
            eprintln!(
                "  access violation {} address {:#x}",
                if record.information[0] == 0 {
                    "reading"
                } else {
                    "writing"
                },
                record.information[1]
            );
        }
        // CONTEXT.Rsp is at 0x98: list the stack slots that point into acs.exe
        let rsp = *((*pointers).context.add(0x98) as *const usize) as *const usize;
        let mut shown = 0;
        for i in 0..400 {
            let value = *rsp.add(i);
            if value.wrapping_sub(base) < 0x200_0000 && value.wrapping_sub(base) > 0x1000 {
                eprintln!("  stack[{i}]: acs.exe {:#x}", ghidra(value));
                shown += 1;
                if shown == 12 {
                    break;
                }
            }
        }
        std::process::exit(3);
    }
}

pub struct Acs {
    base: usize,
    /// MSVCR120 `operator new(size_t)`: memory the game may later `operator delete`.
    operator_new: extern "C" fn(usize) -> *mut u8,
}

impl Acs {
    pub fn load(path: &Path) -> Result<Acs, String> {
        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        // Mapped as an image with relocations applied; no imports resolved, no code run.
        let handle = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                DONT_RESOLVE_DLL_REFERENCES,
            )
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
        };
        IMAGE_BASE.store(acs.base, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            AddVectoredExceptionHandler(1, report_crash);
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

    pub fn addr(&self, rva: usize) -> usize {
        self.base + rva
    }

    /// Zeroed memory from the game's own allocator.
    pub fn alloc(&self, size: usize) -> *mut u8 {
        let p = (self.operator_new)(size);
        unsafe { p.write_bytes(0, size) };
        p
    }

    /// Sends the game's own `printf` output (it reports every key it loads) to NUL.
    pub fn silence_game_stdout(&self) {
        unsafe {
            let crt = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            let iob = GetProcAddress(crt, c"__iob_func".as_ptr().cast());
            let freopen = GetProcAddress(crt, c"freopen".as_ptr().cast());
            if iob.is_null() || freopen.is_null() {
                return;
            }
            let iob: extern "C" fn() -> *mut u8 = std::mem::transmute(iob);
            let freopen: extern "C" fn(*const u8, *const u8, *mut u8) -> *mut u8 =
                std::mem::transmute(freopen);
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
                 the hard-coded RVAs do not apply to this build"
            ));
        }
        let slot = self.read::<u64>(RVA_SCTM_VFTABLE + 8) as usize;
        if slot != self.addr(RVA_SCTM_SOLVE) {
            return Err(format!(
                "SCTM vftable slot 1 is {slot:#x}, expected SCTM::solve at {:#x}",
                self.addr(RVA_SCTM_SOLVE)
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

    /// Fill the import address table entries of the bound DLLs, the way the loader would have.
    unsafe fn bind_imports(&self) -> Result<(), String> {
        let opt = self.nt_headers() + 24;
        let import_rva = self.read::<u32>(opt + 112 + 8) as usize;
        let mut desc = import_rva;
        let mut bound = 0;
        loop {
            let ilt = self.read::<u32>(desc) as usize;
            let name_rva = self.read::<u32>(desc + 12) as usize;
            let iat = self.read::<u32>(desc + 16) as usize;
            if name_rva == 0 {
                break;
            }
            let dll = self.cstr(name_rva);
            if BOUND_DLLS.contains(&dll.to_ascii_lowercase().as_str()) {
                self.bind_one(&dll, if ilt != 0 { ilt } else { iat }, iat)?;
                bound += 1;
            }
            desc += 20;
        }
        if bound != BOUND_DLLS.len() {
            return Err(
                "acs.exe does not import MSVCR120 / MSVCP120 / KERNEL32 as expected".into(),
            );
        }
        Ok(())
    }

    unsafe fn bind_one(&self, dll: &str, ilt: usize, iat: usize) -> Result<(), String> {
        let cdll = CString::new(dll).unwrap();
        let module = LoadLibraryA(cdll.as_ptr() as *const u8);
        if module.is_null() {
            return Err(format!("{dll} not found (the game itself needs it)"));
        }
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
            let target = if entry >> 63 != 0 {
                GetProcAddress(module, (entry & 0xffff) as usize as *const u8)
            } else {
                // IMAGE_IMPORT_BY_NAME: u16 hint, then the name
                let name = CString::new(self.cstr(entry as usize + 2)).unwrap();
                GetProcAddress(module, name.as_ptr() as *const u8)
            };
            if target.is_null() {
                return Err(format!("{dll}: import #{i} could not be resolved"));
            }
            std::ptr::write_unaligned((self.base + iat + i * 8) as *mut u64, target as u64);
        }
        let mut ignored = 0u32;
        VirtualProtect(table, count * 8, old, &mut ignored);
        Ok(())
    }
}
