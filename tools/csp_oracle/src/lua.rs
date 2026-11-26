// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a: Lua snippets run by CSP's own LuaJIT (a fresh state made by its `lua_newstate`,
//! its libraries opened by its own opener) and by rustyAC's (crates/rustyac-lua), each result
//! printed as the raw 64-bit value LuaJIT keeps (GC64: the same layout in both) and compared.

use rustyac_physics::csp::{inipp_lua, lua_env};
use std::ffi::{c_void, CStr};
use std::path::Path;

type Reader = extern "C" fn(*mut c_void, *mut c_void, *mut usize) -> *const u8;
type Load = extern "C" fn(*mut c_void, Reader, *mut c_void, *const u8, *const u8) -> i32;
// CSP's pcall wrapper takes (L, nargs, nresults) and ignores the fourth (lua_pcall's errfunc, 0)
type Pcall = extern "C" fn(*mut c_void, i32, i32, i32) -> i32;

pub struct Vm {
    l: *mut c_void,
    load: Load,
    pcall: Pcall,
}

/// The script folder the stand-in host carries: `<AC>\content\cars\stand_in\data`.
pub fn stand_in_dir(dwrite: &Path) -> String {
    dwrite.with_file_name(r"content\cars\stand_in\data").to_string_lossy().into_owned()
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
        Vm::csp_mode(dll, 1)
    }

    unsafe fn csp_mode(dll: usize, mode: i32) -> Vm {
        let newstate: extern "C" fn(usize, usize) -> *mut c_void = std::mem::transmute(dll + 0x175_3460);
        let open: extern "C" fn(*mut c_void, i32) = std::mem::transmute(dll + 0x13d_c780);
        let l = newstate(19792, 0);
        assert!(!l.is_null(), "CSP's lua_newstate returned NULL");
        open(l, mode);
        Vm { l, load: std::mem::transmute::<usize, Load>(dll + 0x176_10b0), pcall: std::mem::transmute::<usize, Pcall>(dll + 0x176_0ec0) }
    }

    /// Task C6b: CSP's car physics state: its opener in mode 0, CSP's own set-up on a stand-in
    /// of its car physics script host (docs/csp/lua_host.md, part 3), then CSP's libraries as
    /// stored in the dll (CSP's magic), loaded by its own `lua_load`.
    pub unsafe fn csp_car_physics(dll: usize, dwrite: &Path) -> Vm {
        let vm = Vm::csp_mode(dll, 0);
        // stand-in cphys_script_limp: only the fields the set-up reads, the rest zero
        let host = Box::leak(vec![0u8; 0x1400].into_boxed_slice()).as_mut_ptr();
        let car = Box::leak(vec![0u8; 0x100].into_boxed_slice()).as_mut_ptr();
        let empty = |at: usize| *(host.add(at + 24) as *mut u64) = 15; // MSVC std::string, inline, empty
        *(host as *mut usize) = dll + 0x230_1cf8; // the real vtable
        empty(528);
        let dir = stand_in_dir(dwrite);
        *(host.add(560) as *mut *const u8) = std::ffi::CString::new(dir.clone()).unwrap().into_raw().cast(); // script folder (filled by the game)
        *(host.add(576) as *mut usize) = dir.len();
        *(host.add(584) as *mut usize) = dir.len().max(16);
        *host.add(736) = 0; // __allowIO__ asked already: false (the game asks the car)
        empty(1200); // __mode__, set by the host's constructor
        std::ptr::copy_nonoverlapping(b"car_cphys".as_ptr(), host.add(1200), 9);
        *(host.add(1216) as *mut u64) = 9;
        *(host.add(1272) as *mut *mut u8) = car; // car index (car+176) 0
        *((vm.l as *mut u8).add(96) as *mut *mut u8) = host;
        // host+184: a cell holding the state, where the INIConfig natives push their table (sub_1813EED80)
        *(host.add(184) as *mut *mut *mut c_void) = Box::leak(Box::new(vm.l.cast::<c_void>()));
        let require_ffi: extern "C" fn(*mut u8, *mut c_void) = std::mem::transmute(dll + 0x13e_2260);
        let set_up: extern "C" fn(*mut u8, *mut c_void, *mut u8, u8) = std::mem::transmute(dll + 0x84_a7f0);
        require_ffi(host, vm.l);
        set_up(host, vm.l, host.add(528), 1);
        if let Some(pre) = std::env::var_os("CSP_ORACLE_LUA_PRE") {
            // a chunk run between CSP's set-up and its libraries (part 4: wrappers logging the natives)
            let pre = std::fs::read(&pre).unwrap_or_else(|e| panic!("CSP_ORACLE_LUA_PRE: {e}"));
            assert_eq!(vm.exec(&pre, c"=pre"), "nil", "CSP_ORACLE_LUA_PRE");
        }
        for name in lua_env::LIBS {
            let b = inipp_lua::bundle_entry(dwrite, name).unwrap_or_else(|| panic!("{name} not in {}", dwrite.display()));
            let cname = std::ffi::CString::new(name).unwrap();
            assert_eq!(vm.exec(&b, &cname), "nil", "CSP's {name}");
        }
        if let Some(folder) = std::env::var_os("CSP_ORACLE_LUA_CAR") {
            // part 4: the car's scripts from its data.acd, read in memory only (classes 1 and 2 of the
            // C6b prompt), each in package.preload under its name; the first of a repeated name wins
            // the host's next step for a car_cphys state (dwrite+0x13e5ba0): CSP's own extra chunk and its 82 natives
            // (dwrite+0x85ad10), which put the real ac.accessCarPhysics in place of the "not available" one
            let cphys: extern "C" fn(*mut c_void) -> u8 = std::mem::transmute(dll + 0x85_ad10);
            cphys(vm.l);
            let acd = rustyac_content::acd::Acd::open(&Path::new(&folder).join("data.acd")).unwrap_or_else(|e| panic!("CSP_ORACLE_LUA_CAR: {e}"));
            let mut seen = std::collections::HashSet::new();
            let mut refs = std::collections::BTreeMap::new();
            for (name, bytes) in acd.entries() {
                let Some(module) = name.strip_suffix(".lua") else { continue };
                // CSP's sealed form goes through dwrite+0x13d8040 in its file hook: not met in the 8 cars read so far
                assert!(!bytes.starts_with(b"@!lua3:"), "{name}: CSP's sealed form");
                if seen.insert(module.to_ascii_lowercase()) {
                    api_refs(bytes, &mut refs);
                    assert_eq!(vm.preload(module, bytes), "nil", "{name}");
                }
            }
            // names only, never the source: what the scripts can reach beyond what a run with stand-ins reaches
            println!("lua refs {}", refs.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" "));
        }
        vm
    }

    /// rustyAC's car physics state (csp::lua_env).
    pub unsafe fn rustyac_car_physics(dwrite: &Path) -> Vm {
        let l = lua_env::car_physics(dwrite, &lua_env::Host { dir: &stand_in_dir(dwrite), car_index: 0, allow_io: false }).unwrap_or_else(|e| panic!("rustyAC's car physics state: {e}"));
        Vm { l: l.cast(), ..Vm::rustyac_fns() }
    }

    /// rustyAC's.
    pub fn rustyac() -> Vm {
        let l = rustyac_lua::new_state();
        assert!(!l.is_null(), "rustyAC's lua_newstate returned NULL");
        Vm { l: l.cast(), ..Vm::rustyac_fns() }
    }

    fn rustyac_fns() -> Vm {
        unsafe { Vm { l: std::ptr::null_mut(), load: std::mem::transmute::<*const (), Load>(rustyac_lua::sys::lua_loadx as *const ()), pcall: std::mem::transmute::<*const (), Pcall>(rustyac_lua::sys::lua_pcall as *const ()) } }
    }

    /// One chunk's first result: `num <bits>`, `str "<text>"`, `bool`, `nil` or `error "<message>"`.
    pub fn run(&self, chunk: &str) -> String {
        self.exec(chunk.as_bytes(), c"=snippet")
    }

    fn exec(&self, chunk: &[u8], name: &CStr) -> String {
        let status = self.load(chunk, name);
        self.result(if status == 0 { (self.pcall)(self.l, 0, 1, 0) } else { status })
    }

    fn load(&self, chunk: &[u8], name: &CStr) -> i32 {
        let mut data: &[u8] = chunk;
        (self.load)(self.l, reader, &mut data as *mut &[u8] as *mut c_void, name.as_ptr().cast(), std::ptr::null())
    }

    /// `package.preload[module]` = the chunk `source`, compiled by this VM's `lua_load` (as CSP's file
    /// hook does for a plain source) without running it.
    fn preload(&self, module: &str, source: &[u8]) -> String {
        let setter = format!("package.preload[{module:?}] = ...");
        let name = std::ffi::CString::new(format!("={module}")).unwrap();
        let status = self.load(setter.as_bytes(), c"=preload");
        let status = if status == 0 { self.load(source, &name) } else { status };
        // a failed load leaves its message above the setter: the setter is left on the stack (unused)
        self.result(if status == 0 { (self.pcall)(self.l, 1, 1, 0) } else { status })
    }

    fn result(&self, status: i32) -> String {
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
/// `--lua-env <file>` (Task C6b, `dwrite` given): the same in the car physics states.
pub unsafe fn run(dll: usize, file: &str, dwrite: Option<&Path>) {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let chunks: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("--")).collect();
    let only_csp = std::env::var_os("CSP_ORACLE_LUA_CSP").is_some();
    for csr in [0x1f80u32, 0x9fc0] {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        // fresh states for each MXCSR: no trace compiled under the other one is reused
        let csp = match dwrite { Some(d) => Vm::csp_car_physics(dll, d), None => Vm::csp(dll) };
        let ours = match (only_csp, dwrite) { (true, _) => None, (_, Some(d)) => Some(Vm::rustyac_car_physics(d)), _ => Some(Vm::rustyac()) };
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

/// Counts `ac.<name>`, `physics.<name>`, `__util.<name>`, `C.<name>` (ffi.C) and `DataLUT11/DataLUT21/StructItem/INIConfig/INIFormat.<name>`
/// references in a Lua source, names only.
fn api_refs(src: &[u8], refs: &mut std::collections::BTreeMap<String, u32>) {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for table in [&b"ac."[..], b"physics.", b"__util.", b"C.", b"DataLUT11.", b"DataLUT21.", b"StructItem.", b"INIConfig.", b"INIFormat."] {
        let mut i = 0;
        while let Some(at) = src[i..].windows(table.len()).position(|w| w == table) {
            let start = i + at;
            let name_at = start + table.len();
            let end = name_at + src[name_at..].iter().take_while(|&&c| ident(c)).count();
            if (start == 0 || !ident(src[start - 1])) && end > name_at {
                let key = format!("{}{}", String::from_utf8_lossy(table), String::from_utf8_lossy(&src[name_at..end]));
                *refs.entry(key).or_insert(0) += 1;
            }
            i = end.max(name_at);
        }
    }
}

#[test]
fn api_refs_counts_names() {
    let mut r = std::collections::BTreeMap::new();
    api_refs(b"local s=ac.getSim() vac.x=ac.getSim().t ffi.C.lj_foo(1) physics.setGentleStop(0)", &mut r);
    assert_eq!(r.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" "), "C.lj_foo=1 ac.getSim=2 physics.setGentleStop=1");
}
