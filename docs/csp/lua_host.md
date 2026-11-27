# CSP Lua host (phase 2: C6a LuaJIT + INIpp, C6b host types, C6c car scripts)

Plan: docs/csp/plan.md CSP-3 / CSP-11 / CSP-12. What CSP's Lua side does: docs/csp/lua.md.

## Resume here

C6a and C6b are done (below). C6c part 1 is done: `CSP_ORACLE_FRAMES` runs CSP's own car script in
csp_oracle for the whole drive. C6c part 2 is under way (next section). Ported and proven so far:
`ac.DataLUT21` (`lj_lut2d_*`) and `ac.ControlButton` (`lj_binaryinput_*`, CSP's null buttons). The
Resume questions are answered there, except whether the game also does VRC 2026's second set-up.
Next: RSS 3's remaining natives: `cphys0` accessCarPhysics, `cphys62` getScriptSetupValue and
`cphys66` setEngineRPMLimit. All three read the script host from TLS+0x500 (below). After them come
`lj_getSim`/`lj_getCar_inner` (the 3,280-B state the Lua/States update fills), then the other cars'
natives in order of use: `cphys35`/`27`, `lj_lut_get`, `lj_connect_*`, `setExtraSwitch__carc`,
`cphys9`/`80`/`52`/`46`/`11`/`21`/`50`/`60`, `lj_encodeHalf`, `lj_getAirPressure` and
`lj_preciseClock__os`. `--lua-env` has no TLS script context (the current script at TLS+0x1f8 and the
cphys host at TLS+0x500). A native that reads either crashes there (dwrite+0x489f8d), so it needs the
host stand-in in `--lua-env` or a proof in the `--drive` run.

## C6c part 2: the state natives (dwrite.dll build 4305)

**The Resume questions**

- **Frame count vs steps.** Three places read CSP's frame count (+0x2820e90) during a drive:
  - the sim state fill (+0x10b291e), which writes it to `sim.frame` (+0x98);
  - the per-frame cache stamp (+0x10b33a4, sim +0x1d8 through qword_18281A2A8);
  - the script runner's "start at 10" gate (+0x84c165).

  A write watch on `sim.frame` over 8 script cars found writes only; no script reads it. So the
  count matters to a script only through the gate. Rule, the same in rustyAC and the oracle: one
  frame per physics step (frame = n + step). It needs no tape field, because no step result depends
  on it after the gate.
- **The car of every car.** The car-load finisher now sets context +0xa0 to each car's own list slot,
  not only the last car's (commit a9667a36). In a two-car RSS 3 run both scripts load and both
  limiters read 8300. At step 1000 car 0 is at 3612 rpm and car 1 at 8208 rpm.
- **The RSS 3 hash change is the script.** Each CarAvatar now gets its car's last-step
  CarPhysicsState (+0x268) before the Lua state fill, so `car.rpmLimiter` is the car's 8300 and not 0
  (commit 1d761546). Run lim5 shows the script acting:
  - the throttle map turns full gas into 0.33;
  - the clutch map gives a clutch of about 0 to 0.61;
  - `setEngineRPMLimit` (`cphys66`) writes the limit each step, and the limiter first cuts in at
    step 1586.

  The single-car steps hash (f1f71e4dbd3e04ba) is the same after the every-car fix (run lim6).
- **VRC alpha 2026's second set-up at step 172.** It is deterministic: the same with
  `CSP_ORACLE_FRAMES` 11 or 40, and with or without breakpoints. It follows the script's own update
  count or accumulated dt (about 0.52 s), not the frame count or the wall clock. Whether the game does
  it too needs a game run with that car. This is open.

**Ported**

| Native | CSP | rustyAC |
|---|---|---|
| `lj_lut2d_*` (`ac.DataLUT21`) | parse sub_1814A63E0, grid sub_1814A55E0, bilinear sub_1814A98C0, clamp sub_1814A6100 | csp::lua_natives (bicubic aborts as in CSP) |
| `lj_binaryinput_*` (`ac.ControlButton`) | `new` (+0x489f40) returns null unless the script's car is the car controls provider's (sub_1805DDC70); every other call takes null to 0 / false / no-op | csp::lua_natives: null buttons (`// shortcut:` until rustyAC binds `__EXT_CAR_` keys) |

In the oracle's drive every `pressedphysics` call gets a null button (rcx = 0 in all s_* logs), so
the null port is what CSP does there. Proof: `csp lua env` 123/123 identical under both MXCSR.

**The next three (found, not ported)**: each reads the script host `h` = TLS[TlsIndex]+0x500 and
throws `runtime_error` when it is null.

| Native | What it does |
|---|---|
| `cphys0` accessCarPhysics (+0x84f980) | returns `h + 1336`, the script's CarPhysicsState view |
| `cphys62` getScriptSetupValue(name) (+0x857c00) | hashes the name (sub_181647300, 1 when 0) and looks it up in `h + 4544` (count at +4552, 16-B entries); returns entry+8 or null |
| `cphys66` setEngineRPMLimit(rpm, b) (+0x859620) | car = `*(h + 1320) + 792`. If `(int)rpm` != car+1292, it writes car+1292 = `(int)rpm` and `h + 1468` = rpm. If also b and rpm > 0 and `*(h + 1328) + 88` is set, it calls sub_18059CAE0 |

## C6c part 1: CSP's car scripts running in the oracle (dwrite.dll build 4305)

`CSP_ORACLE_FRAMES=<n>` (csp_oracle `--drive`) makes CSP run the car's physics script for the whole
drive. After the CSP command queue each step it sets CSP's frame count (+0x2820e90) to n + step. In
the game only its frame update (+0x1223740) writes that count, once per graphics frame; the oracle
writes it once per physics step. The script runner (+0x84c110) starts the script once the count is
10 or more, and `n=0` starts like the game. At step 0 it puts in the stand-ins below. Every step it
then calls CSP's per-frame "Lua/States" update (+0x10ad540, argument unused), which fills the sim
state and one 3,280-B state per car that `lj_getSim`/`lj_getCarState` return. Without the flag
the drive does none of this.

The stand-ins are zeroed objects, put only where the pointer is null:

| Where | What | Who reads it |
|---|---|---|
| car list end (+0x28213e0) | begin + 8 × cars (the vector the oracle's car load fills at +0x28213d8) | Lua/States update: the number of car states |
| car context (+0x2817d90) +0xa0 | the car list's slot (last car built) | script runner via +0xf32f00 (builds a fresh slot when unset): natives such as `ac.accessExtraSwitchParams` (+0x489b00) read the runner's car, which in the game is the list's slot |
| slot +0x440 | 4,584 B through CSP's own car data constructor (+0x7eaf10) with the car folder | `INIConfig.carData`, `getCarDataFiles` (+72 `<cars>\<car>`, +104 its data) |
| slot +0xe8, +0x108 | the car folder as wstring / narrow string | `getCarDataFiles` (+0x13ce180), `lj_getCarID` |
| slot +0x470 | the extra-switch object: 20 params of 72 B at +0, availability bytes at +0x5a7, objects at +0x648 and +0x650 | `accessExtraSwitchParams`, `lj_getCarExtraSwitchState` (+0x4d9460), the state fill (+0x10b1778, their +0x148) |
| slot +0x478 | an empty hash map: shift byte +0x26 = 63, then CSP's own sizer (+0x2c4a70) | first state fill's insert (+0x10b4590, +0x2d2120) |
| slot +0x4b0 | CSP's empty callback manager (+0x2c60e0) | the same insert (+0x2d3690) |
| slot +0x548 | an object with +0x8d8 (vec3 at +0x2e8, -1 unset) and per wheel w: +0x520 + 0xe8w = avatar + 940 + 64w, +0x528 + 0xe8w = avatar + 684 + 64w (the rule of CSP's slot constructor +0xb2b9a0) | the state fill (+0x10ab700, +0x10af5ea) |
| slot +0x288, +0x290 | objects with 1.0f fields | state fill tail (+0x10b1fe6) |
| CarAvatar (slot+0) +0x210, +0xe18, +0xfd0, +0xfe8, +0x1008, +0x1010, +0x1270 | objects (+0xfc8 = +0xfd0: an empty vector) | first state fill (+0x10af8f0) and the avatar reads it makes |
| CarAvatar +0x130 | the oracle's Sim | acs RaceManager::getBestLap → ReplayManager::isInReplaymode |
| globals +0x2820640 (+0x470), +0x281a7b8, +0x28197b0, +0x2818870 (+0x88, +0xb0, +0xb8, +0xc0), +0x28196f8 | the game window, camera and other main-thread objects | the sim state builder (+0x10b2730, +0x10b23c0) |
| Sim +0x118, +0x150, +0x168, +0x1b0, +0x1b8, +0x238, +0x260 | objects | the same builder; +0x118 (escMenu) by a car_cphys native (+0x858cc0, its +0x1a8), +0x1b0 by isInReplaymode |

CSP's car_cphys step (+0x85ad10: its own 3,529-B chunk, a table of 82 natives, the real
`ac.accessCarPhysics`) runs when the script's state is made: once per load (28 times for alpha 2025,
whose script makes more states). Its natives show below as `cphys<i>` (table index i).

**The 9 cars, 4,000 steps** (spa lap tape, `CSP_ORACLE_FRAMES=11`, `CSP_ORACLE_RECORD=1`, every
native's address a breakpoint; only calls from the script are counted: LuaJIT's FFI call site
+0x196905, or JIT code). URD's data.acd is read as shipped. No decryption was needed: CSP's own
loader reads every packed script itself.

| Car | Class | CSP's log | Calls / distinct natives | Step 0 | Each later step |
|---|---|---|---|---|---|
| `rss_formula_rss_3_v6_2026` | 1 | loaded and ready | 28,012 / 9 | 19: getSim, getCar, lut2d_loadcardata ×2, binaryinput_new ×5, … | cphys66, binaryinput_pressedphysics ×4, lut2d_get ×2 |
| `rss_formula_2010` | 1 | loaded and ready | 20,015 / 12 | 20 (connect_new ×2, lut_loadcardata, binaryinput_new ×4, …) | cphys35, cphys27, binaryinput_pressedphysics ×3 |
| `pxw_sl_v4` | 1 | loaded and ready | 2 / 2 | getSim, getCar | none (offline: entitled, does nothing) |
| `vrc_formula_alpha_2025_csp` | 2 | loaded and ready | 52,941 / 41 | 954 | cphys80, encodeHalf, lut_get ×4, setExtraSwitch__carc, cphys9 ×4, lut2d_get ×2 |
| `vrc_formula_alpha_2026_csp` | 2 (packed) | loaded and ready | 8,029 / 14 | 13 | preciseClock__os, cphys60 (a second set-up at step 172: connectonline_*, checksumXXH, …) |
| `vrc_formula_beta_2024_csp` | 2 | loaded and ready | 74,513 / 36 | 184 | cphys52, cphys27, cphys46, lut_get ×3, cphys11, getAirPressure, setExtraSwitch__carc ×3-4, cphys21, cphys9 ×4, lut2d_get ×2 (timers at 1977, 3976) |
| `vrc_formula_lithium_2023_csp` | 2 | loaded and ready | 2 / 2 | getSim, getCar | none |
| `rss_formula_hybrid_2026` | 3 as 2 (packed) | loaded and ready | 283,574 / 37 | 261 | cphys35, cphys62 ×3, binaryinput_downphysics ×16-18, lut_get, binaryinput_onpressed ×8, binaryinput_pressedphysics ×31, cphys50, lut2d_get, binaryinput_new ×8, setMGUKDelivery__carc (steps 1-2) |
| `urd_darche_992_23` | 3 as 2 (FolderEncrypter) | failed to load: `std/ac_common:0: bad argument #1 to 'pairs' (table expected, got function)` | 7 / 6 | connect_new, getSim, getCarID, getCar ×2, random_seed, connect_gc | none |

URD fails in the game the same way: the game's own CSP log has a Car physics script failing on that
std/ac_common `pairs` error. The steps hash with the scripts running differs from the plain run's
(RSS 3: 2094b15abe6958a1 with the flag, 08a5a823364991ca without). The script acts on CSP's car
(cphys66 each step), and the breakpoints don't change the hash. The per-step logs are
re/scratch/taskC6c (all.sh, names.sh), scratch only.

## C6b part 1: the car physics script's environment (survey, dwrite.dll build 4305)

The RVAs in docs/csp/lua.md come from an earlier dwrite.dll; this build (md5 33e366945091) has them here:

- **The state**: the script host (sub_1813E5BA0) makes a LuaJIT state with CSP's opener in mode 0 (base,
  package, table, string, math, debug, bit; `ffi` preloaded; no io, os, jit: mode 0 never calls CSP's
  luaopen_jit sub_18177C620, so car scripts run in the interpreter), then the environment set-up, then
  runs the bundle entry `ac_common_base` (read from the zip in dwrite.dll, sub_1811C3870, lua_loadx +
  pcall), the kind's library (`ac_car_cphys`), then the global `__script.__initbase__` when there is one.
- **The set-up** (sub_1813E25E0, 4,691 B): `package.path` = `<script dir>/?.lua;<csp>/internal/lua-?.lua`;
  globals `__dirname`, `__dirname__`, the script's name, `__mode__`, `__allowIO__` (from the host, vtable
  +112); free functions `log warn error debug clearDebug dirname findFile getFolder lapTimeToString
  getCountryName readDataFile parseINIppFile getTrackId getTrackLayout getTrackFullID getTrackName
  getTrackDataFilename getCarTags uniqueMachineKey(Async) getCarDataFiles getCarColliders`; tables `ac`
  (9 slots), `__script`, `ui`, `__util` (`native`, `nativf`, `result`, `parseDate`; `imageState` and the
  io/os extras `loadDLL openSerialPort openURL ...` only for kinds other than 0); then the string
  extensions (sub_18101E2C0: `split`, `trim`, `startsWith`, ...).
- **Two ways to native code**: `ffi.C.lj_*` (CSP's LuaJIT resolves `ffi.C` names from its own table, the
  names are not in the dll as text) and `__util.native(name, ...)` / `__util.nativf(hash, ...)`: name ->
  hash sub_180A4D3D0(name, 0xDEADBEEF) -> a map (qword_1828187F8) filled by 296 static initialisers
  (sub_18101A9E0(hash, fn)); "Unknown function: %s" when missing. Only hashes are in the dll; 250 of the
  296 hashes are constants in the bundle's bytecode.
- **CSP's bytecode runs on rustyAC's LuaJIT** (c6ffc141 built from source): all 135 bytecode entries of
  the bundle load with only the 4-byte magic changed to `LJ`. Two differences of CSP's fork
  needed patches in crates/rustyac-lua/build.rs (lib folder `-csp2`): `table.clone` keeps table values
  (CSP's lj_tab_dup sub_1817AC270 has a flag; upstream makes them nil), and a missing `ffi.C` name says
  "missing declaration for" without the name.

What the car physics kind (`ac_common_base` 182,266 B + `ac_car_cphys` 35,299 B) asks for:

| Group | `ffi.C.lj_*` names | Examples | When |
|---|---|---|---|
| maths and types | 47 | `lj_mat_*` (20), `lj_lut_*`/`lj_lut2d_*` (DataLUT11/21, 19), `lj_rgb(m)_from_string`, `lj_random_seed`, `lj_2d_segments_intersection` | C6b part 4 |
| encoding, noise, misc | 34 | `lj_encode/decodeBase64`, `lj_checksum*`, `lj_(de)compress`, `lj_perlin_*`, `lj_utf8To16`, `lj_structBytes`, `lj_getPatchVersion` | C6b part 4 |
| config and data | 19 | `lj_loadINIppFile`, `lj_binaryinput_*` (11), `lj_stringToFFIStruct_inner`, `lj_parseUInt64`, `lj_memoryBarrier` | C6b part 5 survey; on demand (C6c: need the game's car and input) |
| car, sim and track state, car controls | 63 | `lj_getCarState`, `lj_getCarPhysics`, `lj_getSim`, `lj_getSimState`, `lj_set*__carc` (24), `lj_getTrackDateTime`, `lj_simplex_*` | C6c |
| `__util.nativf` keys (ac.INIConfig, stringify, ...) | 37 hashes | INI group 0x180fec8a0..0x180fed670 (17), 0x180856170..0x1808617d0 (19) | C6b part 5 (parse, carData ported) |

163 `lj_*` names in all (lists: re/scratch/taskC6b/envnames.txt, not kept). Loading the two libraries only
looks names up (the `lj_getTimeZone*`, `lj_binaryinput_*`, `lj_*__carc` ones); none is called before a
script runs. The car scripts were not read (their classes: part 3): what
they call is known only when CSP runs them (C6c: log the natives CSP calls per script in the oracle).

**Plan in sessions** (C6b rest about 3-5, C6c about 4-6):

- C6b part 3 (1-2): CSP's set-up itself instead of the stand-in: the globals, `__util` (native/nativf with
  the hash map), `__script`, the string extensions, `__initbase__`; proof: the same snippets in CSP's own
  set-up, which needs a script host object CSP builds only with the game (find what sub_1813E25E0 reads
  from it, or build one in the oracle).
- C6b part 4 (1-2): the maths/type and encoding natives, each against CSP's function in the oracle.
- C6b part 5 (1, done): what the scripts use: `ac.INIConfig.parse` and `.carData` (below); proof: parse
  snippets against CSP's own; the parser itself on the cars' files is the `csp inipp` case.
- C6c: the state structs over rustyAC's car (ac.StateCar, physics overrides and forces), `update(dt)` /
  `reset()` from the physics step, getting the scripts' bytecode (open question: without decrypting
  anything ourselves), then the RSS 3 row.

## C6b part 4: what the car scripts reach, and the natives ported

**Running the scripts in CSP** (csp_oracle `--lua-env`, scratch only, not a proof case): with
`CSP_ORACLE_LUA_CAR=<car folder>` the oracle runs, after CSP's libraries, the host's next step for a
car_cphys state (sub_18085AD10: CSP's own 3,529-B chunk and its 82 natives, which put the real
`ac.accessCarPhysics` in place of the "Physics API is not available here" one; rustyAC's lua_env lacks
this step, part 5/C6c), then reads the car's data.acd in memory (rustyac_content::acd) and puts each
`*.lua` entry in `package.preload` under its name, through CSP's own lua_load: that is what CSP's file
hook (sub_1813DB7A0) does with a plain source its car_acd_lookup (vtable 0x182301e78, read
sub_180849C70) returns. Nothing is written or printed; the oracle prints names only
(`lua refs ...`: `ac.`, `physics.`, `__util.`, `C.` references counted in the sources).
`CSP_ORACLE_LUA_PRE=<file>` runs a prelude after the set-up; the scratch one wraps every `ffi.C`
name, `__util.native/nativf`, the C functions of `ac`/`__util`/`physics` and their C upvalues, and
counts calls per phase (load = `require 'script'`, update = `script.update(0.003)` x3 with zeroed and
with 0x3f-filled `car`/`sim`/physics state).

Why CSP's own path to the script was not used: `require` gives CSP's hook full paths from
`package.path`, and car_acd_lookup::read returns nothing for a name with `/` or `\`; `loadfile` of a
bare name goes through the read-permission check sub_1813CA370, which needs the game's car list
(qword_1828213D8) and car (+1088 path), and still said "permission denied" with stand-ins.

What a run without the game reached before C6c (C6c part 1 has the runs with CSP's car state;
natives the game must answer were stubbed: `lj_getSim`,
`lj_getCar` return zeroed state_sim/state_car, `lj_getCarID` the folder name; `lj_getSim` crashes in
sub_1810B23C0 without the game's sim):

| Car | Load | Update | Refers to (names, of the API) |
|---|---|---|---|
| `pxw_sl_v4` | runs; no native (offline: entitlement check skipped) | runs, 1 line (entitled: does nothing) | 7: ac.decodeBase64, ac.INIConfig, ac.getCarID, ac.getServerIP, ac.addForce, ac.accessCarPhysics, ac.debug |
| `rss_formula_2010` | `lj_getSim`, then a C++ exception (a native needing the game) | not reached | 15 |
| `rss_formula_rss_3_v6_2026` | crashes in dwrite+0x489f8d after `lj_getSim` | not reached | 8 |
| `rss_formula_hybrid_2026` | `lj_getCarID`, `__util.__si_cdef`, then "RSS protected Lua owning car identity unavailable" | not reached | 0 (packed) |
| `urd_darche_992_23` | not run: CSP's FolderEncrypter (needs CSP's reader sub_18142A660) | | |
| `vrc_formula_alpha_2025_csp` | `lj_getCarID`, `__util.__si_cdef`, then ac_common_base "attempt to index a nil value" | not reached | 62 |
| `vrc_formula_alpha_2026_csp` | the same | not reached | 41 (packed) |
| `vrc_formula_beta_2024_csp` | `ac.getCarDataFiles` (nil without the game's car) | not reached | 45 |
| `vrc_formula_lithium_2023_csp` | `lj_getSim` | runs, 2 lines | 2 |

So a run without the game's car and sim reaches few natives; the names the sources refer to are the
better list. Of the part 4 group they use: `ac.decodeBase64` (4 cars), `ac.DataLUT11`/`DataLUT21`
(3), `ac.StructItem` (3), `ac.getPatchVersionCode` (1); the rest of what they refer to is car state
and controls (C6c: `ac.accessCarPhysics` 5, `ac.getSim` 5, `ac.getCarID` 5, `ac.setEngineRPM` 3,
`ac.overrideGasInput`, `ac.setDRS`, `ac.addHubForce`, ... 70 names in all) and config (part 5:
`ac.INIConfig` 4, `ac.getCarDataFiles` 3, `ac.storage`/`ac.store`, `ac.readDataFile`). The VRC
`physics.<Name>` references are the scripts' own modules, not CSP's. No script names `ffi.C` itself.

**The part 4 natives** (ffi.C name, CSP's function in dwrite.dll 4305, from the address `ffi.C`
gives in CSP's state): strings out are a per-thread MSVC `std::string` (`const struct 187 *`).

| Native | RVA | Ported |
|---|---|---|
| `lj_encodeBase64({begin,end}, no_pad)` | 0x1813d2c00 (coder sub_181433D70) | yes |
| `lj_decodeBase64(const char*)` | 0x1813d2e30 (sub_181435190: stops at `=` or a byte not alnum/`+`/`/`) | yes |
| `lj_lut_*` (add atkey atvalue empty gc get loadcardata parse refresh) | 0x180fedbb0..0x180fee320 | yes (Resume 2, below) |
| `lj_lut_load`, `lj_lut_serialize` | 0x180fee170, 0x180fedc10 | on demand |
| `lj_lut2d_*` (add empty gc get loadcardata parse refresh) | 0x180fee460..0x180feeba0 (parse sub_1814A63E0, grid sub_1814A55E0, bilinear sub_1814A98C0) | yes (C6c part 2; bicubic aborts) |
| `lj_lut2d_load`, `lj_lut2d_serialize` | 0x180fee460.. | on demand |
| `lj_structBytes` | 0x1813d3210 | yes |
| `lj_getPatchVersionCode` | 0x1802fd360 (`(11078 - dword_1827FAB10) >> 1`, 4305 here) | yes (4305) |
| `lj_getPatchVersion` | 0x1813caf80 | on demand |
| `lj_mat_*` (euler inverse(self) look mul(self) multo normalize(self) ortho pack perspective proj=rotation scaling toeuler translation transpose(self) unpack) | 0x1813d05e0..0x1813d1b10, normalizeself 0x181002a70 | |
| `lj_checksumCRC32`, `lj_checksumSHA256`, `lj_checksumXXH` | 0x1813d23f0, 0x1813d2290, 0x1813d23d0 | |
| `lj_compress`, `lj_decompress` | 0x1813d2410, 0x1813d2b30 | |
| `lj_encodeHalf(2)`, `lj_decodeHalf(2)` | 0x1813d04f0, 0x1813d0500, 0x1813d0530, 0x1813d0570 | |
| `lj_perlin_1/2/3` | 0x1813d35a0, 0x1813d37f0, 0x1813d3c10 | |
| `lj_random_seed` | 0x1813d1e60 (xoshiro256** per thread, seeded from random_device + QPC, 0..=2147483646) | yes (a random int in range) |
| `lj_utf8To16`, `lj_2d_segments_intersection` | 0x1813d30a0, 0x1813d2140 | |
| `lj_rgb_from_string`, `lj_rgbm_from_string` | 0x1813cb0a0, 0x1813caf90 | |

**"On demand"**: ported only when a script reaches it. Until then the call stops the game with
`CSP Lua: <name> is not ported yet (docs/csp/lua_host.md)`, the name being the `ffi.C.lj_*` one (each
unported `ffi.C` name resolves to its own stub, `UNPORTED_FFI` in csp::lua_env) or `ac.<name>` /
`__util.<name>` / `string.<name>` (the closure carries its name). Left on demand: the rows above without
"yes" (`lj_mat_*`, checksums, compression, half floats, perlin, rgb strings, utf16, segments, 2D LUTs,
`lj_lut_load`, `lj_lut_serialize`, `lj_getPatchVersion`).

### Resume 2: DataLUT11, StructItem, getPatchVersionCode

Natives a call reaches in CSP's state (the oracle with the logging prelude): `ac.DataLUT11.parse` and
`:get` -> `lj_lut_parse`, `lj_lut_get`; `ac.DataLUT11.carData(0, ...)` -> `lj_lut_loadcardata` (nil
without the game's car list); `ac.DataLUT21.parse` -> `lj_lut2d_parse` (on demand: its grid builder
sub_1814A55E0 and constructor sub_1814A63E0 are 5.5 KB); `ac.StructItem.combine` -> `lj_random_seed`
(the struct's type name); `ac.connect` crashes the stand-in (dwrite+0xfe183f, the game's shared
memory: C6c).

The 1D LUT (csp::lua_natives, `LuaLut` = CSP's `lua_lut` cdef, the points a `[f32; 2]` vector):

- **parse** (sub_1814A3160): trimmed of `

	 `; an `=` anywhere means the inline form
  `(x=y|x=y)` (keys and values through csp::parse_float, a value's first `,` piece, pushed unsorted);
  else a digit, `+`, `-` or a comment means the file form (sub_1814A1B30: lines `x|y`, comments `;`,
  `#`, `//`, the value's first `,` piece; unsorted, repeated keys kept); else empty. NULL gives NULL.
- **get** (sub_1814A2120 linear, sub_1814A21F0 cubic): 0 points 0, 1 point its y; with `extrapolate`
  the end segment's slope past either end (`x > last` / `!(x >= first)`, so NaN extrapolates from the
  first); else the segment by upper-bound binary search (sub_1814A3780), `t` clamped to 0..1 with the
  width at least f32::EPSILON; linear `(y1 - y0) * t + y0`, cubic Hermite with the tangents of
  sub_1814A38A0 in CSP's operation order. **add** keeps keys sorted (sub_1814A2670), **refresh**
  the bounds (sub_1814A23C0), **atkey/atvalue** NaN out of range.
- **loadcardata**: CSP reads the car's data file (its reader sub_18142A660, cached per file) into the
  file-form parser; rustyAC reads `<car>/data/<file>` (data.acd or the folder, crate::data) for the
  host's car only (any other index gives NULL, as CSP does without the car). Not in the snippet proof
  (CSP's stand-in has no car): the file form is the one `parse` proves, the read is a unit test.

Proof: `csp lua env` 98/98 identical under both MXCSR (18 new snippets: inline and file forms, NULL and
empty strings, linear/cubic/extrapolated `get` at and between points, past both ends and NaN, repeated
keys, `add`/`refresh`, `atkey`/`atvalue` out of range, StructItem layout and values, `lj_structBytes`
bytes, `lj_random_seed` range, `getPatchVersionCode`). The struct's ctype number differs by one
(CSP's state has declared one more ctype before; layout and values are the same), so the snippet
compares the layout, not `tostring`.

## C6b part 5: ac.INIConfig (parse, carData)

**What the 9 scripts use** (`INIConfig.`/`INIFormat.`/`DataLUT*.` names counted in their plain sources,
the oracle's `lua refs`; the packed ones are not counted): `INIConfig.carData` (3 cars, 21 places),
`DataLUT11.carData` (3, ported in part 4), `DataLUT21.carData`/`.parse` (3, on demand), `INIConfig.cspModule`
(2), and once each `onlineExtras`, `carConfig`, `controlsConfig`, `load`. `ac.store`, `ac.storage` and
`ac.storageSetPath` are nil in CSP's car physics state (scripts referring to them get nil), so rustyAC
leaves them out too. `ac.ControlButton` (`lj_binaryinput_*`), `ac.readDataFile`, `ac.getFolder`,
`ac.getCarDataFiles` and `lj_loadINIppFile` need the game's car or input state (they crash CSP's
stand-in or give nil): C6c part 1 gives CSP that state in the oracle, part 2 ports them.

**The natives** (`__util.nativf(hash, ...)`, map filled by sub_18101A9E0; the CSP side is the bundle's
own Lua, so `get`, `set`, `serialize`, the typed reads are CSP's code on both sides):

| INIConfig | hash | dwrite 4305 | Ported |
|---|---|---|---|
| `parse(text, format, includeFolders)` | 0x14c2318e | sub_180FECE10 -> sub_180FEAD20 | formats 10/11 without include folders: INIpp Lua off (sub_181588550, csp::inipp::parse); formats < 10 (CSP's AC parser sub_180FE8970) and 11 with folders (sub_1815889E0) on demand |
| `carData(carIndex, fileName)` | 0xe8bebf1b | sub_180FECC40 -> sub_180FEA170 -> sub_181507A10 | yes, for the cars with a physics state (other indexes on demand) |
| `load` | 0x9aa21a8b | sub_180FECFA0 | on demand (reads files through the game's car) |
| the others, `onlineExtras`, `carConfig`, `controlsConfig`, `cspModule` among them (0xf5942b6f, 0xd0c262f7, 0x045b9414, 0x7d025bd2, 0xfd30fc39, 0x639c0401, 0x8c9c67da, 0x22f72a11, 0x624eff7b) | | sub_180FEC8A0..sub_180FED320 (sub_180FEC8B0 is a `REMOTE`/`PASSWORD` singleton) | on demand |

- **Result shape** (sub_1813EED80): `{section = {key = {"v1", "v2", ...}}}`, values as strings; the
  host's `lua_State` is read from host+184 (the oracle sets that cell). Extended splits values at
  commas (quotes keep them), `$"..."` gives no value (Lua off), templates and mixins resolve,
  `[INCLUDE]` is dropped with no folders, a lone CR is not a line break, a key without `=` is dropped.
- **carData**: an index < 0, past the game's cars or without a car gives no sections; the name is
  the part after the last `/` or `\`, lowercased; the text comes from data.acd or the data folder
  (crate::data; CSP's reader sub_1805E6120/sub_181582A00 caches per car and name) and is read by
  INIpp with Lua on (sub_181588210 mode 0, as the config loader; csp::inipp::parse_lua with the
  `ini_std` library from dwrite.dll). `tyres.ini` is special: CSP's tyre generator (sub_180914710:
  `_VIRTUAL_ORIGIN` sections, extra rain tyres unless `[_EXTENSION] GENERATE_EXTRA_TYRES` is false)
  may change it, then `_EFX_` keys are left out. rustyAC gives it only when CSP would not change
  it (generator off and no `_VIRTUAL_ORIGIN`), else stops naming sub_180914710.

Proof: `csp lua env` 111/111 identical under both MXCSR (13 new snippets: plain, Lua, template,
mixin, `[INCLUDE]`, empty, BOM/CRLF/comments/repeated sections, `$` references, quotes and escapes,
lone CR, keys before a section, `:get`). carData has no snippet (CSP's stand-in has no car list):
its parser is the one the `csp inipp` case proves on the cars' extension files, its read and name
rules are a unit test (csp::lua_env car_physics_state, csp::lua_natives car_data_glue).

## C6b part 3: CSP's set-up of the state

**CSP's car physics host** is `cphys::cphys_script::impl::cphys_script_limp` (vtable 0x182301cf8, made
by sub_18084AC70, constructor sub_18084D7C0 over `lua::base_script_data` sub_1810DACE0 and
`lua_updateable` sub_1813DED40). Part 1 read the generic set-up; corrections: the 23 functions go into
`ac` (not the globals; `getTrackID` is a second name of `getTrackId`), there is no script-name global
(the string after `__dirname__` is `__mode__`), and `ui`'s `imageState` and the `io`/`os` extras are
added only when the set-up's last argument is 0, which car physics never passes. In order:

- sub_1813E2260 (host vtable +216): `ffi` = `require 'ffi'` (into `_LOADED` and the global).
- sub_18084A7F0 (vtable +224) = sub_1813E25E0(host, L, host+528, 1), then `__carindex__`.
- sub_1813E25E0: vtable +0 is 1 for this host, so the script folder is the string at host+560: when
  it is not empty, `package.path` gets `;<folder>/?.lua;<AC>\extension/internal/lua-?.lua` appended to
  LuaJIT's own (from `LUA_PATH`, `!` = the AC folder) and `__dirname` = `__dirname__` = the folder;
  `__mode__` = host+1200 (vtable +56); `__allowIO__` = vtable +112 (cached in host+736); tables `ac`
  (record size 0x2ed), `__script` (9), `__util` (0x63: `result native nativf parseDate`), `ui` (0x257,
  empty); then string extensions sub_18101E2C0 (28, in this order: `split numbers findIgnoreCase replace
  codePointToUTF8 codePointAt nextEmoji __jsonFormat pack packsize unpack xor urlEncode urlDecode
  startsWith endsWith alphanumCompare versionCompare urlCheck urlNext trim multiply pad regfind cspmatch
  regmatch reggsub reggmatch`). LuaJIT's `lua_createtable(L, 0, n)` gives the same hash sizes as CSP's.

**The host object, field by field** (what the set-up reads; the oracle builds it, rustyAC's
`csp::lua_env::Host` carries the three values that vary):

| Field | CSP | Stand-in |
|---|---|---|
| +0 | vtable 0x182301cf8 | CSP's own vtable (oracle) |
| +528 | std::string, used only by hosts whose vtable +0 is 0 | empty |
| +560 | std::string, the script's folder; its writer runs only in the game (not in the host's constructor or creator) | `<AC>\content\cars\stand_in\data` (`Host::dir`) |
| +736 | `__allowIO__` cache (-1 = not asked; else the car flag of vtable +112, `*(car+1088)+904` bit 0) | 0 (`Host::allow_io` false) |
| +1200 | std::string `car_cphys` (constructor) | the same |
| +1232 | std::string `ac_car_cphys`, the kind's library (vtable +256) | not read by the set-up |
| +1272 | the car; `__carindex__` = int at car+176, plus 1 | a zero block (`Host::car_index` 0) |
| L+96 | the host (CSP's lua_State extension) | the oracle's block |

`ffi.C`: CSP answers exactly the 1,788 `lj_*` names its libraries' cdefs declare, less `lj_free`
(an old `void lj_free(void *ptr)` cdef in `lib_vector`; not answered), whatever the host. Found by
asking CSP's state for each name of every bundle entry's string constants; the list is
crates/rustyac-physics/src/csp/lua_names.txt.

**The proof**: case `lua env` (same command as part 2), CSP's state is now CSP's own set-up on the
stand-in host (`Vm::csp_car_physics`: CSP's vtable, sub_1813E2260, sub_18084A7F0) and rustyAC's is
`car_physics(dwrite, &Host { .. })`; 17 new chunks: sorted keys and types of `_G`, `ac`, `__util`, `ui`,
`__script`, `string`, `package.loaded`; which of them are C functions; `package.path` / `cpath`;
`__mode__ __allowIO__ __dirname __dirname__ __carindex__`; `ffi == package.loaded.ffi`; and every name of
lua_names.txt plus `lj_free lj_ lj_cshader_ lj_no_such_name_xyz GetCurrentProcessId luaL_newstate
lua_pushnumber` through `ffi.C` (CSP: 1788 found, the 7 not). 77/77 identical under both MXCSR.
`pairs` order is not compared: CSP's own differs between two fresh states (LuaJIT's per-state string
hash seed), so no script can depend on it.

**The 9 installed car physics scripts** (`data/script.lua` in data.acd; classes of the C6b prompt; only
counts were looked at: lines, longest line, keyword counts, never content):

| Car | Stored | Class |
|---|---|---|
| `pxw_sl_v4` | Lua source in data.acd, minified (one 21 KB line) | 1 |
| `rss_formula_2010` | Lua source in data.acd | 1 (RSS: may be read) |
| `rss_formula_rss_3_v6_2026` | Lua source in data.acd | 1 (RSS) |
| `rss_formula_hybrid_2026` | Lua in data.acd, packed (long lines, `load`, hashed module names) | 3, handled as 2 |
| `urd_darche_992_23` | not Lua after AC's decoding: CSP's FolderEncrypter, like all its data | 3, handled as 2 |
| `vrc_formula_alpha_2025_csp` | Lua source in data.acd (36 files) | 2 (VRC) |
| `vrc_formula_alpha_2026_csp` | Lua in data.acd, packed (one 345 KB line, `load`); `script.lua` twice in the archive | 2 (VRC) |
| `vrc_formula_beta_2024_csp` | Lua source in data.acd (32 files) | 2 (VRC) |
| `vrc_formula_lithium_2023_csp` | Lua source in data.acd | 2 (VRC) |

(content_needs.md's "VRC physics scripts are scrambled" holds for alpha 2026 only.)

Not done: `__script.__initbase__` (the libraries define it; CSP's host calls it after the kind's
library, part 4 with the script itself); `ui`'s and `__script`'s keys come from the libraries, not the
set-up; no native is ported.

## C6b part 2: CSP's car physics libraries on rustyAC's LuaJIT

**The proof**: case `lua env` runs `csp_oracle --lua-env tools/csp_oracle/lua_env_snippets.txt`: CSP's
state (its newstate, its opener in mode 0, the stand-in set-up, then `ac_common_base` and `ac_car_cphys`
out of the dwrite.dll) and rustyAC's (`csp::lua_env::car_physics`) run the same 60 chunks (what the
state has: globals, `ac` keys and types, `__script`, `string`, `table`, `math`; errors for an undefined
variable and for a Windows name through `ffi.C`; vec2/3/4, quat, rgb/rgbm/hsv, mat3x3; class, refnumber,
refbool; math.lerp/clamp/saturate/round; table.clone/nkeys/isArray/contains/indexOf/map), results as
bits or text, MXCSR 0x1f80 and 0x9fc0: 60/60 identical.

**The port**: crates/rustyac-physics/src/csp/lua_env.rs (feature `lua`): `car_physics(dwrite)` opens the
libraries as mode 0 does, runs `PRELUDE` and the `LIBS` from the installed dwrite.dll
(`inipp_lua::bundle_entry`, nothing of CSP's is carried). `ffi.C` finds no Windows name (as in CSP);
every `lj_` name resolves to a stop ("none is ported yet").

Not done then (part 3 replaced the first and the last): the stand-in set-up (`PRELUDE`: `ffi` and empty `ac`, `__util`,
`__script`, `ui`, the same in both states); no native is ported, so snippets that reach one (mat4x4 `*`,
DataLUT, ac.INIConfig, stringify) are not in the list yet; `ffi.C` answers every `lj_` name, CSP only
its own.

## C6a-2c: the car and track configs

**The proof**: case `configs` runs `inipp_flat --configs <ac> <dwrite.dll>` (every car with its first
skin and each skin that has its own ext_config.ini, as car 0 and as car 1; every track layout) and
`tools/proofs/csp_configs.py` compares each config with the entry CSP's ConfigsCache wrote for it
(`cache\csp_configs\<id>.zip`, writer sub_181510A00; entry name = low 32 bits of XXH3-64 of the
lower-case path, hex), key by key: 35/35 the same. Entries written under an older CSP install are
skipped and counted: their dependency stamps (types 2-4, the common files) are the install's, not the
files' times, so an entry older than the newest install stamp read older common files. Probes for each
INIpp rule found on the way: re/scratch/taskC6a/q (`dcmp.sh`, all `extension\config` and content
configs with the include folders: `dall.sh cars|tracks`, 155/155 and 23/23).

**The order** (car loader sub_18150E200 / per file sub_18150D840, track loader sub_18141A1A0 / per file
sub_181416320, file search sub_18142BFC0):
- Folders: the user's `Documents\Assetto Corsa\cfg\extension\<kind>`, then for a car the skin's and the
  car's `ext_config.ini`, for a track Content Manager's active track skins
  (`skins\default\cm_skins_active.json` -> `skins\cm_skins\<name>\ext_config.ini`), the layout's and the
  track's `extension\ext_config.ini`; then `extension\config\<kind>`. A folder is searched depth first
  for `<car>__<skin>.ini`, `<car>.ini` (tracks: `<track>__<layout>.ini`, `<track>_<layout>.ini`,
  `<track>.ini`), subfolders by upper-case name, `loaded` after the others; each file once.
- The files are read with index n-1 down to 0 (`PREFIX_...` numbers from 10000 x index); the first file
  outside the user folder gets `extension\config\<kind>\generic\generic.ini` included after it (alone,
  uncached, when there is no such file); `[BASIC] IGNORE_OTHER_CONFIGS` drops later `extension\config`
  files. Include folders: user, `extension\config`, `extension\lua`, each `\<kind>`.
- `read()` answers (car reader sub_18150C4D0): `car/id`, `car/skin`, `car/index`,
  `data/<file>/<section>/<key>` (the car's data, first value), `meta/year` (ui_car.json, else
  `data_car_years.ini`, else 2000), `meta/open_wheeler` (ui tags), `meta/has_rain_tyres`,
  `settings/<section>/<key>` (graphics_adjustments.ini), `fixed_random/...` (XXH3 seeded mt19937);
  tracks `track/id` and settings. A config is cached unless its reads asked for the race (`car/index`,
  `track/data`) or the skin (a skin's own file may).
- `meta/has_rain_tyres`: a FRONT tyre of type rain (sub_18090C9A0: `TYPE_HINT`, else guessed from the
  name and short name), counting the rain tyre CSP makes when it loads tyres.ini (sub_180914710): for a
  car whose car.ini VERSION starts `extended-`/`extendedV2-`, or any car once the player's car is loaded
  (a byte set in sub_1805E32E0), with slicks, or semislicks and no street tyre, unless
  `[_EXTENSION] GENERATE_EXTRA_TYRES=0` or a tyre has `_VIRTUAL_ORIGIN`. The cache shows both: tatuusfa1
  (the player's car) without, rss_formula_rss_3_v6 with. CSP's ini reader also ends lines at a lone CR.

**INIpp rules found by the configs** (crates/rustyac-physics/src/csp/inipp.rs): `[DEFAULTS]` does not
override a variable an enclosing `[INCLUDE: ...]` passed; a section's own earlier keys are variables
for its later ones (over `[DEFAULTS]`); a repeated template key is another line applied in order
(`discard()` skips only its line); a mixin inside a mixin sees the caller's arguments; an `ARG = $"..."`
inside an `@` line stays text for the mixin; a Lua key is not split at commas; `read(path, "")` gives
an empty string, not nil.

**The port**: crates/rustyac-physics/src/csp/config.rs (`car`, `track`, `cache_name`, the readers),
`CspFeatures::add_configs` in crates/rustyac-physics/src/csp.rs, called by crates/rustyac-game/src/sim.rs
(`csp_configs`) for the player's car (index 0) and the offline opponents with `--csp`. rustyac-game
turns on rustyac-physics' `lua` for x64 Windows only: the browser and the other platforms don't link
LuaJIT. The release workflow clones LuaJIT for it (as CI does).

Not done (shortcuts in the code): a car's own `data\csp_config.ini`, `sfp.ini` and CSP's built-in
fallback configs (no installed car has them); `read()` keys no installed config asks for (other `meta/`,
`track/data/`); online players' cars get no configs; urd_darche_992_23's tyres.ini is encrypted by CSP
(only CSP can read it, so its `meta/has_rain_tyres` is unknown: the proof counts its entries apart).

## C6a-2b: INIpp with Lua

**The proof**: case `inipp` now runs `csp_oracle --inipp` with Lua on (CSP's normal mode) against
`inipp_flat <list> <dwrite.dll>` (built `--features lua`): 261/261 the same; with Lua off (no dll
argument) still 261/261. Hand probes of each rule below: re/scratch/taskC6a/q (`LUA=1 probe.sh`).

**The Lua state** (sub_181489730, one per parser): `lua_newstate`, then only base (+coroutine), math and
string; then CSP's `ini_std` chunk as `@std`; then `read`, `has`, `get`, `set`. No `ac_common_base`: that
belongs to the other (script preprocessor) state at dll+0x13e5ba0, not to INIpp. `ini_std` (`def`,
`vec2..4`, `discard`, `clamp`, `__conv_result`, ...) is LuaJIT 2.1 bytecode in a zip inside dwrite.dll
(deflated, 17,249 bytes, "from a different INIpp project" says CSP's readme); it is upstream bytecode
with only the 4-byte magic changed. rustyAC reads it from the installed CSP's dwrite.dll at run time
(`csp::inipp_lua::ini_std`), puts back `1B 4C 4A 02` and loads it: no CSP bytes in the repo.

**What CSP does** (each probed):
- `$"expr"` as an item's whole tail is marked by the splitter (open `5B 01 02 04 01`, close ending
  `01 02 04 5D`); text after the closing quote leaves `$"..."` as written. At the end of expansion
  (sub_18148BB50) the last marked expression runs as `return __conv_result(expr)`, on a syntax error again
  as `return __conv_result((function() expr end)())`; each value gets the text before/after it. Values:
  table entries in `lua_next` order, nil nothing, booleans `1`/`0`, numbers `%.14g`.
- Errors: a syntax error or `discard()` (`__discardError__`) drops the key; any other runtime error is
  logged and the key keeps only the text around the expression (empty if none).
- A variable inside `$" "` becomes a Lua literal (sub_18148C1C0): nothing -> `nil` (a missing `$X` too),
  one value: raw if it is a number by CSP's test (sub_18148DA30: decimal, one `.`, `e±n`, `0x` with decimal
  digits only, blanks around; empty counts), else a quoted Lua string; `:exists` -> `true`/`false`, `:str`
  quoted; 2-4 numbers -> `vecN(...)`, else `{...}` with strings quoted.
- `ACTIVE` false: with Lua, every section other than `[FUNCTION]`/`[USE]`/`[INCLUDE]` keeps only
  `ACTIVE=0` (a plain `ACTIVE = 0` too); those three vanish. `[INCLUDE]` keys are not expanded, so its
  `ACTIVE` runs at the section end.
- `[FUNCTION: NAME]` runs `function NAME(ARGUMENTS)
CODE
end` and vanishes; `[USE: f.lua]` runs the file
  (found as `[INCLUDE]` finds files) once per state and vanishes.
- `read(path, default)`: CSP's reader answers `csp/version` (4305) and gives the default otherwise in
  csp_oracle's set-up; number defaults go through a float. `has` is false and `get` the default there.

**The port**: crates/rustyac-physics/src/csp/inipp.rs (`flatten_lua`, `run_lua`, `literal`,
`csp_number`, `quote`, section end) and crates/rustyac-physics/src/csp/inipp_lua.rs (`ini_std`, `Lua`:
the state, `eval`, `function`, `use_file`, `read`/`has`/`get`/`set`). Feature `lua` of rustyac-physics
pulls rustyac-lua with `luajit` and miniz_oxide; off by default (rustyac.exe on x64 Windows turns it on
since C6a-2c; the browser build never links LuaJIT).

Not done (shortcuts in the code; none reached by the 261 files): `read`/`has`/`get`/`set` answering
from a real car and config (done in C6a-2c), mixin arguments without `=` (CSP sets them to `1`; done in C6a-2c),
`@GENERATOR_STARTING_INDEX` (named in docs/csp/rendering_config.md, in no installed config; our
`@GENERATOR...` prefix match would take it for a generator). `@GENERATOR` is done in C6a-2c (below). Known difference outside the corpus: CSP's LuaJIT fork numbers its builtins differently
(`tostring(print)` is `builtin#0` there, `#30` in upstream), so `pairs` over `_G`/`math`/`string` and
`tostring` of a builtin differ.

## C6a-2a: INIpp without Lua

**The proof** (`tools/proofs/csp.sh` case `inipp`): every `.ini` under `extension/config` and the
installed cars' and tracks' `extension` folders (261) through `csp_oracle --inipp` with
`CSP_ORACLE_INIPP_LUA=0` (the ctor's 4th argument 0) and through `inipp_flat` (crates/rustyac-physics
bin), file by file (tools/proofs/csp_inipp.py): 261/261 the same, 2 s each side. (Since C6a-2b the
case runs Lua on; Lua off is checked by hand the same way.)

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
