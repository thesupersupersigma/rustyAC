# CSP Lua host (phase 2: C6a LuaJIT + INIpp, C6b host types, C6c car scripts)

Plan: docs/csp/plan.md CSP-3 / CSP-11 / CSP-12. What CSP's Lua side does: docs/csp/lua.md.

## Resume here

C6a part 1 stopped before the build: the LuaJIT source is not in `ignored/` yet. Super runs, from the repo root:

    git clone https://github.com/LuaJIT/LuaJIT.git ignored/luajit

Then part 1 goes on: find the commit with `git -C ignored/luajit log --all --format="%H %ct %s" | grep 1791809196`,
read CSP's build options, build it behind a feature, numeric proof against CSP's own VM in process.
Part 2 (INIpp) waits for part 1, since `$" expr "` is Lua.

## C6a part 1: which LuaJIT

* Build 4305's dwrite.dll (143,148,080 B) holds exactly one version string, "LuaJIT 2.1.1791809196" (RVA 0x2384068,
  next to "Lua 5.1" at 0x2383cc8 and "jit.profile" after it). Rolling 2.1 releases are numbered by the commit's
  unix time: 1791809196 = 2026-10-12 12:46:36 UTC.
* Build options (GC64, DUALNUM, LUAJIT_ENABLE_LUA52COMPAT, JIT, FFI): not read yet.
