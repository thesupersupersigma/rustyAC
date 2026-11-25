// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lua for CSP mode (Task C6a): LuaJIT 2.1 as Custom Shaders Patch build 4305 has it.
//!
//! CSP's dwrite.dll carries "LuaJIT 2.1.1791809196", a fork commit on top of upstream v2.1;
//! the build options read from its code are GC64, JIT, FFI, LUAJIT_ENABLE_LUA52COMPAT, no
//! DUALNUM (docs/csp/lua_host.md). The library is built from source by build.rs under the
//! feature `luajit` (x64 Windows only); without it this crate is empty, so Vanilla, Linux and
//! the browser build never link LuaJIT.

#[cfg(feature = "luajit")]
pub mod sys {
    //! The few LuaJIT C functions rustyAC calls (lua.h, lauxlib.h, lualib.h).
    #![allow(non_camel_case_types)]
    use std::ffi::{c_char, c_int, c_void};

    pub enum lua_State {}
    pub type lua_Reader = unsafe extern "C" fn(*mut lua_State, *mut c_void, *mut usize) -> *const c_char;
    /// "C-unwind": a Lua error raised in one unwinds through it.
    pub type lua_CFunction = unsafe extern "C-unwind" fn(*mut lua_State) -> c_int;

    pub const LUA_GLOBALSINDEX: c_int = -10002;
    pub const LUA_ERRSYNTAX: c_int = 3;
    pub const LUA_TNIL: c_int = 0;
    pub const LUA_TBOOLEAN: c_int = 1;
    pub const LUA_TNUMBER: c_int = 3;
    pub const LUA_TSTRING: c_int = 4;
    pub const LUA_TTABLE: c_int = 5;

    extern "C-unwind" {
        pub fn luaL_newstate() -> *mut lua_State;
        pub fn luaL_openlibs(l: *mut lua_State);
        pub fn luaopen_base(l: *mut lua_State) -> c_int;
        pub fn luaopen_math(l: *mut lua_State) -> c_int;
        pub fn luaopen_string(l: *mut lua_State) -> c_int;
        pub fn lua_loadx(l: *mut lua_State, reader: lua_Reader, data: *mut c_void, name: *const c_char, mode: *const c_char) -> c_int;
        pub fn luaL_loadbufferx(l: *mut lua_State, buf: *const c_char, size: usize, name: *const c_char, mode: *const c_char) -> c_int;
        pub fn luaL_loadfilex(l: *mut lua_State, name: *const c_char, mode: *const c_char) -> c_int;
        pub fn luaL_error(l: *mut lua_State, fmt: *const c_char, ...) -> c_int;
        pub fn lua_pcall(l: *mut lua_State, nargs: c_int, nresults: c_int, errfunc: c_int) -> c_int;
        pub fn lua_call(l: *mut lua_State, nargs: c_int, nresults: c_int);
        pub fn lua_gettop(l: *mut lua_State) -> c_int;
        pub fn lua_settop(l: *mut lua_State, idx: c_int);
        pub fn lua_pushvalue(l: *mut lua_State, idx: c_int);
        pub fn lua_type(l: *mut lua_State, idx: c_int) -> c_int;
        pub fn lua_toboolean(l: *mut lua_State, idx: c_int) -> c_int;
        pub fn lua_tonumber(l: *mut lua_State, idx: c_int) -> f64;
        pub fn lua_tolstring(l: *mut lua_State, idx: c_int, len: *mut usize) -> *const c_char;
        pub fn lua_next(l: *mut lua_State, idx: c_int) -> c_int;
        pub fn lua_pushnil(l: *mut lua_State);
        pub fn lua_pushnumber(l: *mut lua_State, n: f64);
        pub fn lua_pushboolean(l: *mut lua_State, b: c_int);
        pub fn lua_pushstring(l: *mut lua_State, s: *const c_char);
        pub fn lua_pushlstring(l: *mut lua_State, s: *const c_char, len: usize);
        pub fn lua_isnumber(l: *mut lua_State, idx: c_int) -> c_int;
        pub fn lua_pushcclosure(l: *mut lua_State, f: lua_CFunction, n: c_int);
        pub fn lua_setfield(l: *mut lua_State, idx: c_int, k: *const c_char);
        pub fn lua_close(l: *mut lua_State);
    }
}

/// A new state with the standard libraries and CSP's JIT parameters, or null if out of memory.
///
/// CSP's jit_init sets maxtrace 8000, maxrecord 16000, minstitch 3, maxmcode 40960 and the
/// fma optimisation (upstream: 1000, 4000, 0, 2048, off); fma only changes arm64/ppc code.
#[cfg(feature = "luajit")]
pub fn new_state() -> *mut sys::lua_State {
    const PARAMS: &[u8] = b"jit.opt.start('maxtrace=8000','maxrecord=16000','minstitch=3','maxmcode=40960','+fma')";
    unsafe {
        let l = sys::luaL_newstate();
        if l.is_null() {
            return l;
        }
        sys::luaL_openlibs(l);
        let ok = sys::luaL_loadbufferx(l, PARAMS.as_ptr().cast(), PARAMS.len(), c"=params".as_ptr(), c"t".as_ptr()) == 0 && sys::lua_pcall(l, 0, 0, 0) == 0;
        assert!(ok, "jit.opt.start failed");
        sys::lua_settop(l, 0);
        l
    }
}

#[cfg(all(test, feature = "luajit"))]
mod tests {
    use super::sys::*;

    #[test]
    fn state_runs_a_chunk() {
        let l = super::new_state();
        assert!(!l.is_null());
        let code = b"assert(rawlen and jit.status() and 2^0.5 > 1.41)";
        unsafe {
            assert_eq!(luaL_loadbufferx(l, code.as_ptr().cast(), code.len(), c"=t".as_ptr(), std::ptr::null()), 0);
            assert_eq!(lua_pcall(l, 0, 0, 0), 0);
            lua_close(l);
        }
    }
}
