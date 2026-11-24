# CSP Lua host (phase 2: C6a LuaJIT + INIpp, C6b host types, C6c car scripts)

Plan: docs/csp/plan.md CSP-3 / CSP-11 / CSP-12. What CSP's Lua side does: docs/csp/lua.md.

## Resume here

C6a part 1 and C6a-2a are done (below): rustyAC's INIpp without Lua (`rustyac_physics::csp::inipp`)
flattens all 261 files as CSP does with Lua off (proof case `csp inipp`). Next: **C6a-2b**, the Lua part
(`$" "`, `[FUNCTION]`, `@GENERATOR`, `CONDITION`) on rustyac-lua, proven on the same 261 files with Lua on.

## C6a-2a: INIpp without Lua

**The proof** (`tools/proofs/csp.sh` case `inipp`): every `.ini` under `extension/config` and the
installed cars' and tracks' `extension` folders (261) through `csp_oracle --inipp` with
`CSP_ORACLE_INIPP_LUA=0` (the ctor's 4th argument 0) and through `inipp_flat` (crates/rustyac-physics
bin), file by file (tools/proofs/csp_inipp.py): 261/261 the same, 2 s each side.

**The port**, crates/rustyac-physics/src/csp/inipp.rs, `flatten(path) -> Option<String>`; each function
names the dwrite.dll function it ports (imagebase 0x180000000): scanner sub_181499CE0, value splitter
sub_1814965D0, key commit sub_1814973D0, section header sub_181498920, section end sub_181495400,
templates sub_181494190 / sub_181490D20, mixins sub_1814952E0 / sub_181495130, variables
sub_18148FEB0 / sub_18148D630, `_...` keys sub_181498310 / sub_18149A860, `_...` sections and merging
sub_18149B1C0, writer sub_18149CEB0 (natural sort sub_181433790).

**What CSP does that is easy to get wrong** (each one probed in CSP's own parser):
- Writer: sections and keys in natural order (digit runs as numbers), the unnamed section first without a
  header, items joined by `,`, an item quoted `'...'` (with `\'`) when it holds anything but
  `[A-Za-z0-9_.-]`.
- A `$" "` item is no item at all with Lua off (`X = $"..", 10` is `X=10`); an `ACTIVE = $" "` drops a
  `[FUNCTION]` / `[USE]` / `[INCLUDE...]` section, other sections keep `ACTIVE=` as written.
- `[INCLUDE...]` is a prefix match (`[INCLUDE_...]` with an `INCLUDE=` key includes); a missing file is
  skipped silently; the included file sees the section's other keys as variables, the includer does not
  see the included file's `[DEFAULTS]`.
- Header `[P: A, B]`: templates are looked up last to first, each with its parents depth first; any
  template that is not `earlyresolve` makes ONE section named `P` (`[A]` alone: the unnamed section), else
  one section per name. Template keys never overwrite the section's own (unless the same template wrote
  them), `@OUTPUT` names an unnamed section, `@ACTIVE` / `@DISCARD` skip the template.
- Mixins (`@ = M, ARG = value`): applied at that line, overwrite the section's keys; arguments come before
  every other variable; a mixin's `EXTENDS` parents are not applied; repeated `@` and `PREFIX_...` lines
  in a template or mixin all stay (each application numbers its `PREFIX_...` keys anew).
- Variables: `${L:2}`, `${L:2:2}`, `${L:-1}`, `${L:1::2}`, `:count`, `:length`, `:exists`, `:or=`, `:vec3`,
  `:bool`, `:num`; a missing `${X}` is nothing, a missing `$X` stays `'$X'`, `${X:?}` drops the key;
  `p${L}s` and `${L}-${L}` multiply out.

Shortcuts left (marked `shortcut:` in the code, none reached by the 261 files): a template's captured
variables are a copy (CSP keeps the live scope), finished sections are not finalized a second time at
the end, mixin arguments without `=` are skipped.

## C6a part 2: CSP's INIpp, found and measured

**The oracle** (`csp_oracle --inipp <list>`, tools/csp_oracle/src/inipp.rs). In process, CSP builds a
`utils::ini_file` the way its own config loaders do: ctor dll+0x1588210 (mode 0, path as a UTF-8
std::string, 4th argument 1, no extra search folders, no pre/post files), which loads, resolves
includes from the file's own folders and evaluates; then dll+0x149ceb0 writes the flattened text, the
same text `[__DEBUG] DUMP_FLATTEN_INI` writes next to a file (sections sorted, values quoted where
needed when the first option word is 1). The ctor's 4th argument turns Lua on: with 0, `$" "` values
come out empty (54 of the 261 files change). All 261 files take 2 s.

**The corpus**: every `.ini` under `extension/config` and under the installed cars' and tracks'
`extension` folders: 30 folders, 261 files, 4,340 sections and 35,949 keys after flattening; 750
`$" "` expressions in 51 files (the plan's 725 in 43 was an older content set), 32 `[FUNCTION]`s,
352 `[INCLUDE]` sections, 187 templates, 185 mixins, 59 `@GENERATOR`s. 25 files flatten to nothing on
their own (libraries of templates and mixins, e.g. `cars/common/materials_carpaint.ini`). 207 files
flatten the same with Lua off.

**Where it is in dwrite.dll** (for the port): parser object 272 B, ctor +0x1490f80; parse text
+0x1499ce0; load a file +0x149a770; `[INCLUDE]` handler +0x1495400; resolve (templates, mixins,
expressions) +0x149b1c0; flatten +0x149ceb0; JSON form +0x149d260 (what Lua's `parseINIppFile` returns,
+0x13cd970 on text). The functions between +0x147f000 and +0x14a0000 hold about 33,600 instructions.
Lua side: the preprocessor state (+0x13e5ba0) opens base/package/table/string/math/debug/bit, no
io/os/jit, and runs the embedded chunk `ac_common_base` (`?__initbase__`). Loaders: the track one
(+0x141a1a0, `tracks\generic\generic.ini`, `extension\ext_config.ini`, `skins\...\cm_skins`, `loaded`)
calls the per-file loader +0x1416320; the car one is not located yet.

**Split** (replaces "part 2", about 1.5 sessions, with about 5-6):

| Step | What | Proof | Size |
|---|---|---|---|
| C6a-2a | INIpp without Lua: reading and quoting values, sections and `...` numbering, `[INCLUDE]` / `INCLUDE=` with variables, `$variables`, templates / `EXTENDS` / `[A : B]`, `ACTIVE`, the flatten writer; in `rustyac-physics::csp` | all 261 files, ours vs CSP's flatten with the ctor's 4th argument 0, key by key | 2-3 sessions |
| C6a-2b | the Lua part on rustyAC's LuaJIT: `$" "`, `[FUNCTION]`, `CONDITION`, `@GENERATOR`, `read()` / `get()` / `has()`, `ac_common_base` | all 261 files with Lua on, proof case `csp inipp` turned to Lua on | 2 sessions |
| C6a-2c | file order for a car and a track (generic / common / loaded / own `ext_config.ini`), into `CspFeatures` | each installed car and track: CSP's loader result vs ours | 1 session |

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
