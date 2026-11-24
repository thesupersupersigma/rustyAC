// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a: Lua snippets run by CSP's own LuaJIT (a fresh state made by its `lua_newstate`,
//! its libraries opened by its own opener) and by rustyAC's (crates/rustyac-lua), each result
//! printed as the raw 64-bit value LuaJIT keeps (GC64: the same layout in both) and compared.

use std::ffi::c_void;

type Reader = extern "C" fn(*mut c_void, *mut c_void, *mut usize) -> *const u8;
type Load = extern "C" fn(*mut c_void, Reader, *mut c_void, *const u8, *const u8) -> i32;
// CSP's pcall wrapper takes (L, nargs, nresults) and ignores the fourth (lua_pcall's errfunc, 0)
type Pcall = extern "C" fn(*mut c_void, i32, i32, i32) -> i32;

pub struct Vm {
    l: *mut c_void,
    load: Load,
    pcall: Pcall,
}

/// The chunk handed to `lua_load` once, then the end.
extern "C" fn reader(_: *mut c_void, data: *mut c_void, size: *mut usize) -> *const u8 {
    unsafe {
        let chunk = &mut *(data as *mut &[u8]);
        *size = chunk.len();
        let start = chunk.as_ptr();
        *chunk = &[];
        start
    }
}

impl Vm {
    /// CSP's: `lua_newstate` (dwrite+0x1753460, LJ_ALLOCF_INTERNAL), then the libraries the way
    /// its INIpp / script states open them (dwrite+0x13dc780; mode 1 adds io, os and jit).
    pub unsafe fn csp(dll: usize) -> Vm {
        let newstate: extern "C" fn(usize, usize) -> *mut c_void = std::mem::transmute(dll + 0x175_3460);
        let open: extern "C" fn(*mut c_void, i32) = std::mem::transmute(dll + 0x13d_c780);
        let l = newstate(19792, 0);
        assert!(!l.is_null(), "CSP's lua_newstate returned NULL");
        open(l, 1);
        Vm { l, load: std::mem::transmute::<usize, Load>(dll + 0x176_10b0), pcall: std::mem::transmute::<usize, Pcall>(dll + 0x176_0ec0) }
    }

    /// rustyAC's.
    pub fn rustyac() -> Vm {
        let l = rustyac_lua::new_state();
        assert!(!l.is_null(), "rustyAC's lua_newstate returned NULL");
        unsafe { Vm { l: l.cast(), load: std::mem::transmute::<*const (), Load>(rustyac_lua::sys::lua_loadx as *const ()), pcall: std::mem::transmute::<*const (), Pcall>(rustyac_lua::sys::lua_pcall as *const ()) } }
    }

    /// One chunk's first result: `num <bits>`, `str "<text>"`, `bool`, `nil` or `error "<message>"`.
    pub fn run(&self, chunk: &str) -> String {
        let mut data: &[u8] = chunk.as_bytes();
        let status = (self.load)(self.l, reader, &mut data as *mut &[u8] as *mut c_void, c"=snippet".as_ptr().cast(), std::ptr::null());
        let status = if status == 0 { (self.pcall)(self.l, 0, 1, 0) } else { status };
        unsafe {
            // lua_State (GC64): top at +40; a TValue is 8 bytes, its type in the top 17 bits
            let top = (self.l as *mut u8).add(40) as *mut *mut u64;
            *top = (*top).sub(1);
            let value = **top;
            let tag = ((value as i64) >> 47) as u32;
            let text = || {
                let s = (value & ((1 << 47) - 1)) as *const u8;
                let len = *(s.add(20) as *const u32) as usize;
                String::from_utf8_lossy(std::slice::from_raw_parts(s.add(24), len)).into_owned()
            };
            match (status, tag) {
                (0, t) if t <= 0xffff_fff2 => format!("num {value:#018x}"),
                (0, 0xffff_ffff) => "nil".into(),
                (0, 0xffff_fffe) => "false".into(),
                (0, 0xffff_fffd) => "true".into(),
                (0, 0xffff_fffb) => format!("str {:?}", text()),
                (0, t) => format!("type {:#x}", !t),
                (_, 0xffff_fffb) => format!("error {status} {:?}", text()),
                (_, _) => format!("error {status}"),
            }
        }
    }
}

/// `--lua <file>`: every non-empty line not starting with `--` is a chunk; run by both VMs under
/// MXCSR 0x1f80 and with FTZ / DAZ (0x9fc0, CSP's physics thread). Prints each differing line.
pub unsafe fn run(dll: usize, file: &str) {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let chunks: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("--")).collect();
    let only_csp = std::env::var_os("CSP_ORACLE_LUA_CSP").is_some();
    for csr in [0x1f80u32, 0x9fc0] {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        // fresh states for each MXCSR: no trace compiled under the other one is reused
        let csp = Vm::csp(dll);
        let ours = if only_csp { None } else { Some(Vm::rustyac()) };
        let mut differ = 0;
        for (i, chunk) in chunks.iter().enumerate() {
            let want = csp.run(chunk);
            match &ours {
                None => println!("lua {csr:#06x} {i}: {want}  <- {chunk}"),
                Some(vm) => {
                    let got = vm.run(chunk);
                    if got != want {
                        differ += 1;
                        println!("lua {csr:#06x} {i}: csp {want} rustyac {got}  <- {chunk}");
                    }
                }
            }
        }
        if ours.is_some() {
            println!("lua MXCSR {csr:#06x}: {} snippets, {differ} differ", chunks.len());
        }
    }
}
