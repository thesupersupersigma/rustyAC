# CSP Lua host (phase 2: C6a LuaJIT + INIpp, C6b host types, C6c car scripts)

Plan: docs/csp/plan.md CSP-3 / CSP-11 / CSP-12. What CSP's Lua side does: docs/csp/lua.md.

## Resume here

C6a part 1 is done (below). Part 2 (INIpp: `$" expr "`, functions, includes, mixins, templates,
conditions, the file order, against CSP's own flatten) starts next.

## C6a part 1: which LuaJIT

**The version.** Build 4305's dwrite.dll (143,148,080 B) holds one version string, "LuaJIT
2.1.1791809196" (RVA 0x2384068, next to "Lua 5.1" at 0x2383cc8). Rolling 2.1 releases are numbered by
the build commit's unix time: 1791809196 = 2026-10-12 12:46:36 UTC. No upstream LuaJIT commit has that
time (author or commit): the upstream v2.1 commits around it are c6ffc141 (2026-09-08, "PPC: Fix
string.* fast functions with P64 set") and 0218af7e (2026-10-13). So CSP builds its own fork commit on
top of upstream v2.1 as of c6ffc141 or a little older. Its bytecode header is its own too:
`string.dump` gives `06 07 08 88 ..` where upstream gives `1B 4C 4A 02 ..` (CSP loads only its own
bytecode; rustyAC runs Lua source, so this does not matter until something loads CSP's compiled files).

**The build options, read from CSP's code** (luaopen_jit at dwrite+0x177c620, lua_newstate at
+0x1753460, the library opener at +0x13dc780) and confirmed in process:

| Option | CSP | How it shows |
|---|---|---|
| x64, Windows | yes | `jit.arch`, `jit.os` |
| GC64 | on | strings pushed as `ptr | 0xFFFD800000000000`; `ffi.abi("gc64")` true |
| DUALNUM | off | `jit.version_num` pushed as a double |
| JIT | on | `jit.status()` true, hot loops get traces (`jit.util.traceinfo`) |
| FFI | on | `ffi` in `package.preload`, `ffi.abi`, `ffi.new` work |
| LUAJIT_ENABLE_LUA52COMPAT | on | `rawlen` exists (lib_base.c only has it `#if LJ_52`), `;;` parses |
| CPU features | detected, like upstream | `jit.status()`: SSE3 SSE4.1 BMI2 on this PC |
| JIT parameters | changed | maxtrace 8000, maxrecord 16000, minstitch 3, maxmcode 40960 (upstream 1000, 4000, 0, 2048); the rest as upstream |
| Optimisations | all 11 on | `fma` on as well (upstream off); fma is only used by the arm64 and ppc back ends |

The libraries CSP opens (opener mode 1, its script states): base, package, table, string, math,
debug, bit, io, os, jit; mode 0 (the INIpp preprocessor's state, +0x13e5ba0) leaves out io, os and
jit. Its `_PRELOAD` adds ffi, socket.core, mime.core, internal.serialize and many `_openssl.*` modules:
that is host work (C6b), not LuaJIT's.

**rustyAC's build.** `crates/rustyac-lua` (MIT OR Apache-2.0), feature `luajit`: build.rs builds
upstream commit c6ffc141 from the clone in `ignored/luajit` (or `LUAJIT_DIR`) with `git archive`
and `msvcbuild.bat lua52compat static` (GC64 is the x64 default; static compiles with cl's default /MT,
like Embree's /MT build of C5i-c) into `target/luajit-c6ffc141-mt/lua51.lib` (or `RUSTYAC_LUAJIT_LIB`).
x64 Windows only; the build refuses without +crt-static. `rustyac_lua::new_state()` opens the
libraries and sets CSP's JIT parameters with `jit.opt.start`. Without the feature the crate is empty:
Vanilla, Linux and the browser build never link LuaJIT. Only `tools/csp_oracle` turns it on so far;
CSP mode in rustyac.exe will in C6b. CI clones LuaJIT for the tools step. Licence: LICENSING.md,
THIRD_PARTY_NOTICES.txt (tools/notices.py).

**Numeric proof** (`csp_oracle --lua tools/csp_oracle/lua_snippets.txt`, proof case `csp lua`). In
the oracle process, CSP's own LuaJIT (a fresh state from its `lua_newstate`, its own library opener,
its `lua_loadx` and pcall) and rustyAC's run the same 141 chunks: arithmetic edge cases (denormals,
-0, inf, mod), every math.* function, bit.*, tostring / tonumber / string.format / string <-> number
coercion, 25 hot loops of 5000 iterations that run as compiled traces, and build-option probes. Each
first result is compared as its raw 64-bit value (strings by their text). Under MXCSR 0x1f80 and
0x9fc0 (FTZ/DAZ, CSP's physics thread), with fresh states for each: **141 snippets, 0 differ**.
The only differences found by probing, by design: `jit.version` (CSP's fork commit time against
c6ffc141's 1788856981), the bytecode header, and `package.path` / `cpath` (made from the exe's
folder; the host sets them in C6b). `CSP_ORACLE_LUA_CSP=1` prints CSP's results alone.

Upstream fixes after c6ffc141 that CSP's fork does not show either: `math.floor("x")`'s error,
`for` loops with a -0 step and `tonumber` with a base give the same results in both.

**Browser and Linux (later).** LuaJIT's JIT cannot run in WASM, and its interpreter is assembly
(DynASM) for each CPU: no wasm target. Suggestion: on Linux x64 build the same commit with its
Makefile (`XCFLAGS=-DLUAJIT_ENABLE_LUA52COMPAT`, GC64 default), the same numeric proof against a
Windows run's results file. In the browser, CSP mode runs without Lua scripts (the scripts' effects
are host features, absent), or PUC-Lua 5.1 compiled to WASM with LuaJIT's extensions (bit, ffi
subset) emulated: numerics agree for + - * / and most of math.*, but not for every libm call nor
`tostring`'s %.14g; any such port would get its own proof against the Windows results.
