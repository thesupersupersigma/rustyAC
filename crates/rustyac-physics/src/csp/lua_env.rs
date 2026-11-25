// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6b: the Lua state CSP runs a car's physics script in, on rustyAC's LuaJIT (feature
//! `lua`). CSP's script host (sub_1813E5BA0) opens its libraries in mode 0 (base, package,
//! table, string, math, debug, bit; ffi preloaded; no io, os or jit, so the interpreter only),
//! sets the global `ffi`, fills `ac`, `__util`, `__script` and `ui` (sub_1813E25E0), then runs
//! CSP's own libraries, LuaJIT bytecode read out of the installed dwrite.dll at run time.
//! rustyAC carries none of that bytecode; it answers the libraries' `ffi.C.lj_*` natives.

use std::ffi::{c_char, c_void, CStr};
use std::path::Path;

use rustyac_lua::sys::*;

/// What stands in for CSP's environment set-up: `ffi` as CSP sets it (`package.loaded.ffi`),
/// the four tables empty. shortcut: CSP's set-up needs its script host object (C6b part 3).
pub const PRELUDE: &str = "ffi = require 'ffi' ac, __util, __script, ui = {}, {}, {}, {}";

/// CSP's libraries of a car physics script, in its order: shared base, then the kind's own.
pub const LIBS: [&str; 2] = ["ac_common_base", "ac_car_cphys"];

/// Called through any `ffi.C.lj_*` name: none is ported yet, so a script reaching one stops.
extern "C" fn unported() {
    eprintln!("CSP Lua: an ffi.C.lj_* native was called; none is ported yet (C6b)");
    std::process::abort();
}

/// `ffi.C` in a CSP state: CSP's own names only (Windows' are not found, as in CSP).
/// shortcut: every `lj_` name resolves, CSP knows only its own; list them when natives are ported.
unsafe extern "C" fn resolve(name: *const c_char) -> *mut c_void {
    if CStr::from_ptr(name).to_bytes().starts_with(b"lj_") { unported as *mut c_void } else { std::ptr::null_mut() }
}

unsafe fn run(l: *mut lua_State, code: &[u8], name: &CStr, mode: &CStr) -> Result<(), String> {
    if luaL_loadbufferx(l, code.as_ptr().cast(), code.len(), name.as_ptr(), mode.as_ptr()) == 0 && lua_pcall(l, 0, 0, 0) == 0 {
        return Ok(());
    }
    let p = lua_tolstring(l, -1, std::ptr::null_mut());
    Err(if p.is_null() { "error".into() } else { CStr::from_ptr(p).to_string_lossy().into_owned() })
}

/// A car physics script's state with CSP's libraries of `dwrite` loaded.
///
/// # Safety
/// Sets LuaJIT's process-wide `ffi.C` resolver.
pub unsafe fn car_physics(dwrite: &Path) -> Result<*mut lua_State, String> {
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
    let mut r = run(l, PRELUDE.as_bytes(), c"=prelude", c"t");
    for name in LIBS {
        if r.is_err() {
            break;
        }
        let Some(mut b) = super::inipp_lua::bundle_entry(dwrite, name) else {
            r = Err(format!("{name} is not in {}", dwrite.display()));
            break;
        };
        b[..4].copy_from_slice(b"\x1bLJ\x02");
        let cname = std::ffi::CString::new(name).unwrap();
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
