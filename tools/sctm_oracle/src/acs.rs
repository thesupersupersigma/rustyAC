//! Maps acs.exe into this process as a plain image and exposes the handful of functions the
//! oracle calls. The game's entry point is never run and nothing in the file on disk is touched;
//! the only memory written is the import table of our private mapping (CRT imports only).

use std::ffi::{c_void, CString, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// Addresses are RVAs (Ghidra address - 0x140000000), names from acs.pdb.
pub const RVA_SCTM_CTOR: usize = 0x44b8c0; // SCTM::SCTM
pub const RVA_SCTM_SOLVE: usize = 0x44bc20; // SCTM::solve
pub const RVA_SCTM_VFTABLE: usize = 0x1416580; // SCTM::`vftable' (slot 1 = solve)
pub const RVA_CALC_LOAD_SENS_MULT: usize = 0x27f770; // calcLoadSensMult(float, float, float)
pub const RVA_TAN_FLOAT: usize = 0xbf630; // float tan(float), as used by Tyre::initCompounds
pub const RVA_DEG2RAD_CONST: usize = 0x14196dc; // the 0.017453f literal initCompounds multiplies by

/// PE TimeDateStamp of the build the RVAs above belong to.
const EXPECTED_TIMESTAMP: u32 = 0x5a55e7a8;

const DONT_RESOLVE_DLL_REFERENCES: u32 = 0x1;
const PAGE_READWRITE: u32 = 0x04;
/// Only these imports are bound: the tyre maths needs sinf/cosf/tanf/powf/sqrtf and operator new.
const BOUND_DLLS: [&str; 2] = ["msvcr120.dll", "msvcp120.dll"];

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn GetLastError() -> u32;
}

pub struct Acs {
    base: usize,
}

impl Acs {
    pub fn load(path: &Path) -> Result<Acs, String> {
        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        // Mapped as an image with relocations applied; no imports resolved, no code run.
        let handle = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), DONT_RESOLVE_DLL_REFERENCES)
        };
        if handle.is_null() {
            return Err(format!("LoadLibraryExW({}) failed, error {}", path.display(), unsafe {
                GetLastError()
            }));
        }
        let acs = Acs { base: handle as usize };
        unsafe {
            acs.check_build()?;
            acs.bind_crt_imports()?;
        }
        Ok(acs)
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn addr(&self, rva: usize) -> usize {
        self.base + rva
    }

    unsafe fn read<T: Copy>(&self, rva: usize) -> T {
        std::ptr::read_unaligned((self.base + rva) as *const T)
    }

    pub fn read_f32(&self, rva: usize) -> f32 {
        unsafe { self.read::<f32>(rva) }
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

    /// Fill the import address table entries of the CRT DLLs, the way the loader would have.
    unsafe fn bind_crt_imports(&self) -> Result<(), String> {
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
            return Err("acs.exe does not import MSVCR120.dll / MSVCP120.dll as expected".into());
        }
        Ok(())
    }

    unsafe fn bind_one(&self, dll: &str, ilt: usize, iat: usize) -> Result<(), String> {
        let cdll = CString::new(dll).unwrap();
        let module = LoadLibraryA(cdll.as_ptr() as *const u8);
        if module.is_null() {
            return Err(format!(
                "{dll} not found (Visual C++ 2013 x64 runtime, which the game itself needs)"
            ));
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

    /// `float tan(float)` exactly as Tyre::initCompounds calls it.
    pub fn tan(&self, x: f32) -> f32 {
        let f: extern "C" fn(f32) -> f32 = unsafe { std::mem::transmute(self.addr(RVA_TAN_FLOAT)) };
        f(x)
    }

    /// `calcLoadSensMult(dRef, fz0, exponent)` = dRef * fz0 / fz0^exponent.
    pub fn calc_load_sens_mult(&self, d_ref: f32, fz0: f32, exp: f32) -> f32 {
        let f: extern "C" fn(f32, f32, f32) -> f32 =
            unsafe { std::mem::transmute(self.addr(RVA_CALC_LOAD_SENS_MULT)) };
        f(d_ref, fz0, exp)
    }
}
