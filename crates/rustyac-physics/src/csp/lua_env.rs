// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6b: the Lua state CSP runs a car's physics script in, on rustyAC's LuaJIT (feature
//! `lua`). CSP's script host (sub_1813E5BA0) opens its libraries in mode 0 (base, package,
//! table, string, math, debug, bit; ffi preloaded; no io, os or jit, so the interpreter only),
//! sets the global `ffi` (sub_1813E2260), runs its set-up (the car physics host's
//! sub_18084A7F0 over sub_1813E25E0), then CSP's own libraries, LuaJIT bytecode read out of the
//! installed dwrite.dll at run time. rustyAC carries none of that bytecode; the set-up's C
//! functions and the libraries' `ffi.C.lj_*` natives are named here; csp::lua_natives has the ported ones.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

use rustyac_lua::sys::*;

/// CSP's libraries of a car physics script, in its order: shared base, then the kind's own.
pub const LIBS: [&str; 2] = ["ac_common_base", "ac_car_cphys"];

/// What the set-up reads from CSP's car physics script host (`cphys_script_limp`); the running
/// game fills it (docs/csp/lua_host.md, part 3).
pub struct Host<'a> {
    /// The script's folder (host+560): `package.path`, `__dirname`; empty adds neither.
    pub dir: &'a str,
    /// The car's index (car+176), `__carindex__` is one more.
    pub car_index: i32,
    /// `__allowIO__` (host vtable+112: a flag of the car).
    pub allow_io: bool,
}

/// The set-up's `ac` functions (sub_1813E25E0, its order; kinds other than car physics add more).
const AC: [&CStr; 23] = [
    c"log", c"warn", c"error", c"debug", c"clearDebug", c"dirname", c"findFile", c"getFolder",
    c"lapTimeToString", c"getCountryName", c"readDataFile", c"parseINIppFile", c"getTrackId",
    c"getTrackID", c"getTrackLayout", c"getTrackFullID", c"getTrackName", c"getTrackDataFilename",
    c"getCarTags", c"uniqueMachineKey", c"uniqueMachineKeyAsync", c"getCarDataFiles", c"getCarColliders",
];

/// `__util`'s.
const UTIL: [&CStr; 4] = [c"result", c"native", c"nativf", c"parseDate"];

/// CSP's string extensions (sub_18101E2C0, its order).
const STRING: [&CStr; 28] = [
    c"split", c"numbers", c"findIgnoreCase", c"replace", c"codePointToUTF8", c"codePointAt",
    c"nextEmoji", c"__jsonFormat", c"pack", c"packsize", c"unpack", c"xor", c"urlEncode",
    c"urlDecode", c"startsWith", c"endsWith", c"alphanumCompare", c"versionCompare", c"urlCheck",
    c"urlNext", c"trim", c"multiply", c"pad", c"regfind", c"cspmatch", c"regmatch", c"reggsub", c"reggmatch",
];

/// The names CSP's `ffi.C` answers in a car physics state: those its libraries declare, less
/// `lj_free` (declared by an old cdef, not answered); found by asking CSP for each (part 3).
const FFI_NAMES: &str = include_str!("lua_names.txt");

/// Called through any of the names above not ported yet (csp::lua_natives): a script reaching one stops.
unsafe extern "C-unwind" fn unported(_: *mut lua_State) -> c_int {
    eprintln!("CSP Lua: a CSP native not ported yet was called (C6b)");
    std::process::abort();
}

/// `ffi.C` in a CSP state: CSP's own names only (Windows' are not found, as in CSP).
unsafe extern "C" fn resolve(name: *const c_char) -> *mut c_void {
    let name = CStr::from_ptr(name).to_bytes();
    if !FFI_NAMES.lines().any(|n| n.as_bytes() == name) {
        return std::ptr::null_mut();
    }
    super::lua_natives::native(name).unwrap_or(unported as *mut c_void)
}

unsafe fn run(l: *mut lua_State, code: &[u8], name: &CStr, mode: &CStr) -> Result<(), String> {
    if luaL_loadbufferx(l, code.as_ptr().cast(), code.len(), name.as_ptr(), mode.as_ptr()) == 0 && lua_pcall(l, 0, 0, 0) == 0 {
        return Ok(());
    }
    let p = lua_tolstring(l, -1, std::ptr::null_mut());
    Err(if p.is_null() { "error".into() } else { CStr::from_ptr(p).to_string_lossy().into_owned() })
}

/// `t[key] = value`, `idx` as seen after the value is pushed.
unsafe fn set_string(l: *mut lua_State, idx: c_int, key: &CStr, value: &str) {
    lua_pushlstring(l, value.as_ptr().cast(), value.len());
    lua_setfield(l, idx, key.as_ptr());
}

/// A table of `unported` under `names`, sized as CSP's (LuaJIT's hash size of `nrec`).
unsafe fn table(l: *mut lua_State, nrec: c_int, names: &[&CStr]) {
    lua_createtable(l, 0, nrec);
    for name in names {
        lua_pushcclosure(l, unported, 0);
        lua_setfield(l, -2, name.as_ptr());
    }
}

/// LuaJIT's `setpath` with `!` the AC folder (CSP's progdir: the folder of dwrite.dll).
fn lua_path(var: &str, default: &str, root: &str) -> String {
    let path = std::env::var(var).map_or(default.to_string(), |p| p.replace(";;", ";\u{1};").replace('\u{1}', default));
    path.replace('!', root)
}

/// CSP's set-up of a car physics state, after its opener (sub_1813E2260, sub_18084A7F0).
unsafe fn set_up(l: *mut lua_State, root: &str, host: &Host) -> Result<(), String> {
    run(l, b"ffi = require 'ffi'", c"=ffi", c"t")?;
    lua_getfield(l, LUA_GLOBALSINDEX, c"package".as_ptr());
    let mut path = lua_path("LUA_PATH", ".\\?.lua;!\\lua\\?.lua;!\\lua\\?\\init.lua;", root);
    if !host.dir.is_empty() {
        path = format!("{path};{}/?.lua;{root}\\extension/internal/lua-?.lua", host.dir);
    }
    set_string(l, -2, c"path", &path);
    set_string(l, -2, c"cpath", &lua_path("LUA_CPATH", ".\\?.dll;!\\?.dll;!\\loadall.dll", root));
    lua_settop(l, 0);
    if !host.dir.is_empty() {
        set_string(l, LUA_GLOBALSINDEX, c"__dirname", host.dir);
        set_string(l, LUA_GLOBALSINDEX, c"__dirname__", host.dir);
    }
    set_string(l, LUA_GLOBALSINDEX, c"__mode__", "car_cphys");
    lua_pushboolean(l, host.allow_io as c_int);
    lua_setfield(l, LUA_GLOBALSINDEX, c"__allowIO__".as_ptr());
    for (name, nrec, names) in [(c"ac", 0x2ed, &AC[..]), (c"__script", 9, &[]), (c"__util", 0x63, &UTIL), (c"ui", 0x257, &[])] {
        table(l, nrec, names);
        lua_setfield(l, LUA_GLOBALSINDEX, name.as_ptr());
    }
    lua_getfield(l, LUA_GLOBALSINDEX, c"string".as_ptr());
    for name in STRING {
        lua_pushcclosure(l, unported, 0);
        lua_setfield(l, -2, name.as_ptr());
    }
    lua_settop(l, 0);
    lua_pushnumber(l, (host.car_index + 1) as f64);
    lua_setfield(l, LUA_GLOBALSINDEX, c"__carindex__".as_ptr());
    Ok(())
}

/// A car physics script's state: CSP's set-up for `host`, then CSP's libraries of `dwrite`.
///
/// # Safety
/// Sets LuaJIT's process-wide `ffi.C` resolver.
pub unsafe fn car_physics(dwrite: &Path, host: &Host) -> Result<*mut lua_State, String> {
    let l = luaL_newstate();
    if l.is_null() {
        return Err("out of memory".into());
    }
    let libs: [(lua_CFunction, &CStr); 7] = [
        (luaopen_base, c""),
        (luaopen_package, c"package"),
        (luaopen_table, c"table"),
        (luaopen_string, c"string"),
        (luaopen_math, c"math"),
        (luaopen_debug, c"debug"),
        (luaopen_bit, c"bit"),
    ];
    for (f, name) in libs {
        lua_pushcclosure(l, f, 0);
        lua_pushstring(l, name.as_ptr());
        lua_call(l, 1, 0);
    }
    lua_getfield(l, LUA_GLOBALSINDEX, c"package".as_ptr());
    lua_getfield(l, -1, c"preload".as_ptr());
    lua_pushcclosure(l, luaopen_ffi, 0);
    lua_setfield(l, -2, c"ffi".as_ptr());
    lua_settop(l, 0);
    lj_clib_resolver = Some(resolve);
    let root = dwrite.parent().unwrap_or(Path::new("")).to_string_lossy();
    let mut r = set_up(l, &root, host);
    for name in LIBS {
        if r.is_err() {
            break;
        }
        let Some(mut b) = super::inipp_lua::bundle_entry(dwrite, name) else {
            r = Err(format!("{name} is not in {}", dwrite.display()));
            break;
        };
        b[..4].copy_from_slice(b"\x1bLJ\x02");
        let cname = CString::new(name).unwrap();
        r = run(l, &b, &cname, c"b").map_err(|e| format!("{name}: {e}"));
    }
    match r {
        Ok(()) => Ok(l),
        Err(e) => {
            lua_close(l);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state builds from the stand-in AC folder's dwrite.dll, when there is one (csp proof area otherwise).
    #[test]
    fn car_physics_state() {
        let dwrite = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/csp/fakeac/dwrite.dll");
        if !dwrite.exists() {
            return;
        }
        unsafe {
            let l = car_physics(&dwrite, &Host { dir: "x", car_index: 2, allow_io: true }).unwrap();
            run(l, b"assert(__carindex__ == 3 and __allowIO__ and __dirname == 'x' and __mode__ == 'car_cphys')", c"=t", c"t").unwrap();
            lua_close(l);
        }
    }
}
