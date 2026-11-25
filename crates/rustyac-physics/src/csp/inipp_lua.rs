// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2b: the Lua side of CSP's INIpp on rustyAC's LuaJIT (feature `lua`): `$"..."` values,
//! `[FUNCTION]` and `[USE]` sections. One state per parser as CSP has (sub_181489730): base,
//! math and string only, CSP's `ini_std` library, then `read`, `has` and `get`.

use std::ffi::{c_int, CStr};
use std::path::Path;

use rustyac_lua::sys::*;

type B = Vec<u8>;

/// CSP's `ini_std` (the expression library: `def`, `vec2`, `__conv_result`, ...) out of the
/// installed dwrite.dll, with LuaJIT's own magic.
pub fn ini_std(dwrite: &Path) -> Option<B> {
    let mut out = bundle_entry(dwrite, "ini_std")?;
    out[..4].copy_from_slice(b"\x1bLJ\x02");
    Some(out)
}

/// One library of the zip inside the installed dwrite.dll (`ac_common_base`, `lib_physics`, ...):
/// a deflated entry, LuaJIT 2.1 bytecode with CSP's own 4-byte magic, as stored.
pub fn bundle_entry(dwrite: &Path, name: &str) -> Option<B> {
    let d = std::fs::read(dwrite).ok()?;
    let u16at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]) as usize;
    let u32at = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
    let mut from = 0;
    while let Some(p) = d[from..].windows(name.len()).position(|w| w == name.as_bytes()).map(|p| p + from) {
        from = p + 1;
        let h = match p.checked_sub(30) {
            Some(h) if d[h..h + 4] == *b"PK\x03\x04" && u16at(h + 26) == name.len() => h,
            _ => continue,
        };
        let (method, csize, usize_) = (u16at(h + 8), u32at(h + 18), u32at(h + 22));
        let start = h + 30 + name.len() + u16at(h + 28);
        let raw = d.get(start..start + csize)?;
        let out = if method == 8 { miniz_oxide::inflate::decompress_to_vec(raw).ok()? } else { raw.to_vec() };
        return (out.len() == usize_ && out.len() >= 4).then_some(out);
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

/// What `read`, `has` and `get` answer from while a parser runs Lua (C6a-2c).
pub(crate) trait Host {
    /// `read`'s value for a path from the loader's data provider (car reader sub_18150C4D0); `None`: the default stays.
    fn read(&self, path: &[u8]) -> Option<B>;
    /// The values of the first key matching `key` (and with a value matching `val`) in the first
    /// finished section matching `sec` that has one (parser +24, in order); `key` `None`: `Some(empty)`
    /// for a matching section.
    fn find(&self, sec: &Pat, key: Option<&Pat>, val: Option<&Pat>) -> Option<Vec<B>>;
}

thread_local! {
    static HOST: std::cell::Cell<Option<*const (dyn Host + 'static)>> = const { std::cell::Cell::new(None) };
}

/// Run `f` (Lua calls) with `h` answering `read`, `has` and `get`.
pub(crate) fn with_host<R>(h: &dyn Host, f: impl FnOnce() -> R) -> R {
    // SAFETY: only dereferenced by the callbacks while `f` runs, `h` outlives it; restored after
    let p: *const (dyn Host + 'static) = unsafe { std::mem::transmute(h as *const dyn Host) };
    let old = HOST.with(|c| c.replace(Some(p)));
    let r = f();
    HOST.with(|c| c.set(old));
    r
}

fn host<R>(f: impl FnOnce(&dyn Host) -> R) -> Option<R> {
    HOST.with(|c| c.get()).map(|p| f(unsafe { &*p }))
}

/// A name pattern of `has`/`get` (sub_181487670): `X?` starts with, `?X` ends with, `?X?` contains,
/// `?` alone (or no argument) anything; another `?` inside matches any run.
pub(crate) struct Pat(Option<(B, u8)>);

impl Pat {
    fn new(p: Option<B>) -> Pat {
        let Some(p) = p else { return Pat(None) };
        if p == b"?" {
            return Pat(None);
        }
        let n = p.len();
        let mut mode = 0;
        for (i, &c) in p.iter().enumerate() {
            if c == b'?' {
                mode = if i == 0 { 2 } else if i + 1 < n || mode == 4 { 4 } else if mode == 2 { 3 } else { 1 };
            }
        }
        let core = if mode == 4 { p } else { p.into_iter().filter(|&c| c != b'?').collect() };
        Pat(Some((core, mode)))
    }

    pub(crate) fn any(&self) -> bool {
        self.0.is_none()
    }

    pub(crate) fn hit(&self, s: &[u8]) -> bool {
        let Some((p, mode)) = &self.0 else { return true };
        match mode {
            0 => s == &p[..],
            1 => s.starts_with(p),
            2 => s.ends_with(p),
            3 => s.windows(p.len().max(1)).any(|w| w == &p[..]) || p.is_empty(),
            // shortcut: `?` inside taken as any run (sub_181487AE0 not decompiled, no config uses one)
            _ => glob(p, s),
        }
    }
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.iter().position(|&c| c == b'?') {
        None => p == s,
        Some(i) => s.starts_with(&p[..i]) && (i..=s.len()).any(|j| glob(&p[i + 1..], &s[j..])),
    }
}

unsafe fn arg(l: *mut lua_State, i: c_int) -> Option<B> {
    matches!(lua_type(l, i), LUA_TSTRING | LUA_TNUMBER).then(|| string_at(l, i))
}

unsafe fn push_str(l: *mut lua_State, s: &[u8]) {
    lua_pushlstring(l, s.as_ptr().cast(), s.len());
}

/// `read(path[, default])` (sub_181487CF0): the provider answers by the default's type: none or
/// nil a string (empty: nil), a number a float, a boolean a boolean, a string that string (empty too).
unsafe extern "C-unwind" fn read(l: *mut lua_State) -> c_int {
    let n = lua_gettop(l);
    if !(1..=2).contains(&n) {
        return luaL_error(l, c"read: needs path and default value".as_ptr());
    }
    if lua_type(l, 1) != LUA_TSTRING {
        return luaL_error(l, c"read: path is not a string".as_ptr());
    }
    let path = string_at(l, 1);
    let got = host(|h| h.read(&path)).flatten();
    let ty = if n == 1 { LUA_TNIL } else { lua_type(l, 2) };
    if ty == LUA_TNIL {
        match got {
            Some(v) if !v.is_empty() => push_str(l, &v),
            _ => lua_pushnil(l),
        }
    } else if lua_isnumber(l, 2) != 0 {
        // the reader's numbers are floats; csp/version is the base reader's (sub_1805E53C0)
        let x = if path == b"csp/version" { Some(4305.0) } else { got.and_then(|v| float(&v)) };
        lua_pushnumber(l, f64::from(x.unwrap_or(lua_tonumber(l, 2) as f32)));
    } else if ty == LUA_TBOOLEAN {
        lua_pushboolean(l, got.map_or(lua_toboolean(l, 2), |v| super::inipp::truthy(Some(&v)) as c_int));
    } else if ty == LUA_TSTRING {
        push_str(l, &got.unwrap_or_else(|| string_at(l, 2)));
    } else {
        lua_pushnil(l);
    }
    1
}

/// A float as CSP's readers parse one (leading blanks, the longest number prefix).
fn float(v: &[u8]) -> Option<f32> {
    let s = std::str::from_utf8(v).ok()?.trim();
    (1..=s.len()).rev().find_map(|i| s.get(..i)?.parse::<f32>().ok())
}

/// `has(section[, key[, value]])` (sub_181488490): any finished section of the config being loaded.
unsafe extern "C-unwind" fn has(l: *mut lua_State) -> c_int {
    let n = lua_gettop(l);
    if !(1..=3).contains(&n) {
        return luaL_error(l, c"has: needs section name, section name and key or section name, key and value".as_ptr());
    }
    let (sec, key, val) = (Pat::new(arg(l, 1)), Pat::new(arg(l, 2)), (n > 2).then(|| Pat::new(arg(l, 3))).filter(|p| !p.any()));
    let key = (!key.any() || val.is_some()).then_some(&key);
    lua_pushboolean(l, host(|h| h.find(&sec, key, val.as_ref()).is_some()).unwrap_or(false) as c_int);
    1
}

/// `get(section, key[, default])` (sub_181488920): the key's values (else the default's) as the
/// default's type wants (sub_181487200).
unsafe extern "C-unwind" fn get(l: *mut lua_State) -> c_int {
    let n = lua_gettop(l);
    if !(2..=3).contains(&n) {
        return luaL_error(l, c"get: needs section name and key".as_ptr());
    }
    let (sec, key) = (Pat::new(arg(l, 1)), Pat::new(arg(l, 2)));
    let vals = match host(|h| h.find(&sec, Some(&key), None)).flatten() {
        Some(v) => v,
        None if n == 3 => {
            let mut v = Vec::new();
            conv(l, 3, b"", b"", &mut v);
            v
        }
        None => Vec::new(),
    };
    let first = vals.first().cloned().unwrap_or_default();
    match if n == 3 { lua_type(l, 3) } else { LUA_TNIL } {
        LUA_TBOOLEAN => lua_pushboolean(l, super::inipp::truthy(Some(&first)) as c_int),
        LUA_TNUMBER => lua_pushnumber(l, std::str::from_utf8(&first).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0)),
        LUA_TSTRING => push_str(l, &first),
        // shortcut: a table default and nil take each value as a number if it reads as one, else a string (no config uses either)
        _ => {
            for v in &vals {
                match std::str::from_utf8(v).ok().and_then(|s| s.parse::<f64>().ok()) {
                    _ if v.is_empty() => lua_pushnil(l),
                    Some(x) => lua_pushnumber(l, x),
                    None => push_str(l, v),
                }
            }
            return vals.len() as c_int;
        }
    }
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
