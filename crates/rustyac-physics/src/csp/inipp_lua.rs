// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2b: the Lua side of CSP's INIpp on rustyAC's LuaJIT (feature `lua`): `$"..."` values,
//! `[FUNCTION]` and `[USE]` sections. One state per parser as CSP has (sub_181489730): base,
//! math and string only, CSP's `ini_std` library, then `read`, `has` and `get`.

use std::ffi::{c_int, CStr};
use std::path::Path;

use rustyac_lua::sys::*;

type B = Vec<u8>;

/// CSP's `ini_std` (the expression library: `def`, `vec2`, `__conv_result`, ...) out of the
/// installed dwrite.dll: a deflated zip entry, LuaJIT 2.1 bytecode with its own 4-byte magic.
pub fn ini_std(dwrite: &Path) -> Option<B> {
    let d = std::fs::read(dwrite).ok()?;
    let u16at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]) as usize;
    let u32at = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
    let mut from = 0;
    while let Some(p) = d[from..].windows(7).position(|w| w == b"ini_std").map(|p| p + from) {
        from = p + 1;
        let h = match p.checked_sub(30) {
            Some(h) if d[h..h + 4] == *b"PK\x03\x04" && u16at(h + 26) == 7 => h,
            _ => continue,
        };
        let (method, csize, usize_) = (u16at(h + 8), u32at(h + 18), u32at(h + 22));
        let start = h + 30 + 7 + u16at(h + 28);
        let raw = d.get(start..start + csize)?;
        let mut out = if method == 8 { miniz_oxide::inflate::decompress_to_vec(raw).ok()? } else { raw.to_vec() };
        if out.len() != usize_ || out.len() < 4 {
            return None;
        }
        out[..4].copy_from_slice(b"\x1bLJ\x02");
        return Some(out);
    }
    None
}

pub struct Lua {
    l: *mut lua_State,
    /// `[USE]` files already run, lower case (sub_18148B060)
    used: std::cell::RefCell<Vec<String>>,
}

impl Drop for Lua {
    fn drop(&mut self) {
        unsafe { lua_close(self.l) };
    }
}

fn cstr(b: &[u8]) -> B {
    [b, b"\0"].concat()
}

unsafe fn load(l: *mut lua_State, code: &[u8]) -> c_int {
    // the chunk's name is the code itself, as CSP's (only error messages show it)
    let name = cstr(code);
    luaL_loadbufferx(l, code.as_ptr().cast(), code.len(), name.as_ptr().cast(), std::ptr::null())
}

unsafe fn string_at(l: *mut lua_State, i: c_int) -> B {
    let mut n = 0;
    let p = lua_tolstring(l, i, &mut n);
    if p.is_null() { Vec::new() } else { std::slice::from_raw_parts(p.cast::<u8>(), n).to_vec() }
}

/// `read(path[, default])` (sub_181487CF0).
// shortcut: the reader knows `csp/version` only, as csp_oracle's (CSP's car/track readers are C6a-2c)
unsafe extern "C-unwind" fn read(l: *mut lua_State) -> c_int {
    let n = lua_gettop(l);
    if !(1..=2).contains(&n) {
        return luaL_error(l, c"read: needs path and default value".as_ptr());
    }
    if lua_type(l, 1) != LUA_TSTRING {
        return luaL_error(l, c"read: path is not a string".as_ptr());
    }
    match if n == 1 { LUA_TNIL } else { lua_type(l, 2) } {
        LUA_TSTRING | LUA_TBOOLEAN => lua_pushvalue(l, 2),
        LUA_TNUMBER if string_at(l, 1) == b"csp/version" => lua_pushnumber(l, 4305.0),
        // the reader's numbers are floats
        LUA_TNUMBER => lua_pushnumber(l, f64::from(lua_tonumber(l, 2) as f32)),
        _ => lua_pushnil(l),
    }
    1
}

/// `has(section[, key[, value]])` (sub_181488490); no sections to look at yet.
// shortcut: always false as in csp_oracle (the config being loaded is C6a-2c)
unsafe extern "C-unwind" fn has(l: *mut lua_State) -> c_int {
    if !(1..=3).contains(&lua_gettop(l)) {
        return luaL_error(l, c"has: needs section name, section name and key or section name, key and value".as_ptr());
    }
    lua_pushboolean(l, 0);
    1
}

/// `get(section, key[, default])` (sub_181488920): the default, as `has`.
unsafe extern "C-unwind" fn get(l: *mut lua_State) -> c_int {
    lua_settop(l, 3);
    1
}

/// `set(section, key, value)` (sub_181488C90) writes into the config being loaded.
// shortcut: no config here, nothing written (no corpus file calls it); C6a-2c with `has`/`get`
unsafe extern "C-unwind" fn set(l: *mut lua_State) -> c_int {
    if lua_gettop(l) != 3 {
        return luaL_error(l, c"set: needs section name, key and value".as_ptr());
    }
    0
}

impl Lua {
    /// A state with `std` (from [`ini_std`]) loaded, `None` if it fails.
    pub fn new(std: &[u8]) -> Option<Lua> {
        unsafe {
            let l = luaL_newstate();
            if l.is_null() {
                return None;
            }
            let lua = Lua { l, used: Default::default() };
            let libs: [(lua_CFunction, &CStr); 3] = [(luaopen_base, c""), (luaopen_math, c"math"), (luaopen_string, c"string")];
            for (f, name) in libs {
                lua_pushcclosure(l, f, 0);
                lua_pushstring(l, name.as_ptr());
                lua_call(l, 1, 0);
            }
            if luaL_loadbufferx(l, std.as_ptr().cast(), std.len(), c"@std".as_ptr(), std::ptr::null()) != 0 || lua_pcall(l, 0, 0, 0) != 0 {
                return None;
            }
            let fns: [(lua_CFunction, &CStr); 4] = [(read, c"read"), (has, c"has"), (get, c"get"), (set, c"set")];
            for (f, name) in fns {
                lua_pushcclosure(l, f, 0);
                lua_setfield(l, LUA_GLOBALSINDEX, name.as_ptr());
            }
            Some(lua)
        }
    }

    /// One `$"..."` expression (sub_181489B30): its values, each `prefix + value + suffix`, into
    /// `out`; false drops the key (a syntax error, `discard()`).
    pub fn eval(&self, expr: &[u8], prefix: &[u8], suffix: &[u8], out: &mut Vec<B>) -> bool {
        let l = self.l;
        unsafe {
            let top = lua_gettop(l);
            let mut r = load(l, &[b"return __conv_result(", expr, b")"].concat());
            if r == LUA_ERRSYNTAX {
                lua_settop(l, top);
                r = load(l, &[b"return __conv_result((function() ", expr, b" end)())"].concat());
            }
            let ok = match if r == 0 { lua_pcall(l, 0, 1, 0) } else { r } {
                0 => {
                    conv(l, lua_gettop(l), prefix, suffix, out);
                    true
                }
                // a runtime error: logged, the key keeps its text around the expression
                2 if find(&string_at(l, -1), b"__discardError__").is_none() => {
                    if !prefix.is_empty() || !suffix.is_empty() {
                        out.push([prefix, suffix].concat());
                    }
                    true
                }
                _ => false,
            };
            lua_settop(l, top);
            ok
        }
    }

    /// `[FUNCTION: NAME]` (sub_18148A810): `function NAME(ARGUMENTS)\nCODE\nend` run once.
    pub fn function(&self, name: &[u8], args: &[B], code: &[u8]) {
        let text = [b"function ", name, b"(", &args.join(&b','), b")\n", code, b"\nend"].concat();
        self.run(|l| unsafe { load(l, &text) });
    }

    /// `[USE: file.lua]` (sub_18148B060): the file run once per state.
    pub fn use_file(&self, path: &Path) {
        let key = path.to_string_lossy().to_lowercase();
        if self.used.borrow().contains(&key) {
            return;
        }
        self.used.borrow_mut().push(key);
        let p = cstr(path.to_string_lossy().as_bytes());
        self.run(|l| unsafe { luaL_loadfilex(l, p.as_ptr().cast(), std::ptr::null()) });
    }

    /// Load with `f`, then call; errors only go to CSP's log.
    fn run(&self, f: impl FnOnce(*mut lua_State) -> c_int) {
        unsafe {
            let top = lua_gettop(self.l);
            if f(self.l) == 0 {
                lua_pcall(self.l, 0, 0, 0);
            }
            lua_settop(self.l, top);
        }
    }
}

/// A result's values (sub_181486C40): tables in `lua_next` order, nil nothing, booleans 1/0.
unsafe fn conv(l: *mut lua_State, i: c_int, prefix: &[u8], suffix: &[u8], out: &mut Vec<B>) {
    match lua_type(l, i) {
        LUA_TTABLE => {
            lua_pushnil(l);
            while lua_next(l, i) != 0 {
                conv(l, lua_gettop(l), prefix, suffix, out);
                lua_settop(l, -2);
            }
        }
        LUA_TNIL => {}
        LUA_TBOOLEAN => out.push([prefix, if lua_toboolean(l, i) != 0 { b"1" } else { b"0" }, suffix].concat()),
        _ => out.push([prefix, &string_at(l, i), suffix].concat()),
    }
}

fn find(s: &[u8], pat: &[u8]) -> Option<usize> {
    s.windows(pat.len()).position(|w| w == pat)
}
