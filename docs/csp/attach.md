# CSP attach and hook mechanism

How Custom Shaders Patch (CSP 0.3.0-preview649, build 4184, `dwrite.dll` 139,567,496 bytes, image base `0x180000000`) gets
into `acs.exe` 1.16.4 and how it patches the game. CSP is a DLL proxy for DirectWrite: Windows loads it because `acs.exe`
imports `DWrite.dll`, all start-up work runs inside `DllMain` on the main thread, the game's functions are found **by name
through `acs.pdb`** (or a cache of it), and each replaced function is detoured with an embedded MinHook-style library.
This file is the core of the CSP atlas (see the README). Addresses: `dwrite.dll RVA 0x...` (add `0x180000000` for a VA);
`acs.exe` addresses are `0x14....` VAs. Markers: `[dll]` seen in dwrite.dll, `[log-csp]`, `[ini]`, `[prior]`, `[interp]` inference.

> **Correction (applied after the tyre / suspension area was read).** The table this file was
> written from classed 934 registration rows as hooks and 507 as lookups. For 92 rows the compiler
> emitted `lea rdi,[replacement]` *before* the name load, and the first scan only looked after it, so
> those hooks were counted as lookups (verified on `SCTM::solve`, replacement RVA `0x69d700`, stub at
> `0x2572c1`). The corrected totals are **1,026 hooks and 415 lookups** (1,441 rows, 1,368 distinct
> names unchanged). Per-class tables below still show the old split and a few per-class figures that the area
> files corrected (`AIDriver` is 19 hooks + 2 lookups, `RaceEngineer` 11 + 2, `Sim` 20 + 3, `NetCarStateProvider` 9 hooks); the per-area files use the
> corrected `csp_hook_table.tsv` where they say so, and `physics_tyres_suspension.md` lists the first 76.

## 1. What it does

### 1.1 How dwrite.dll gets into acs.exe

* `acs.exe` has **DWrite.dll in its static import table** (verified by parsing the PE: 25 imports, among them
  `DWrite.dll`, `fmod64.dll`, `fmodstudio64.dll`, `d3d11.dll`, `dxgi.dll`, `DINPUT8.dll`, `XINPUT1_3.dll`,
  `d3dx11_43.dll`, `D3DCOMPILER_43.dll`, `steam_api64.dll`, `MSVCR120.dll`, `WSOCK32.dll`; PE timestamp 2018-01-10). `[dll]`
* The Windows loader looks in the application folder first, finds the 139 MB `dwrite.dll` next to `acs.exe` and maps it
  instead of the system DirectWrite. There is no injector, no `LoadLibrary` from outside, no process-name test for
  loading. The DLL is a **64-bit-only** build (messages say "v1.16.3 or v1.16.4, 64-bit"). `acs_x86.exe` cannot load it. `[dll][interp]`
* **Entry point** `RVA 0x16da528` (decomp `decomp_infra/16da528.c`) is the normal MSVC `_DllMainCRTStartup`: on
  `DLL_PROCESS_ATTACH` it runs the security-cookie init (`0x16da834`), then the CRT initialisers (`0x16da214`, which run
  every C++ static initialiser, see 1.5: **the registry of hooks is filled here, before `DllMain` proper**), then the
  dispatcher `0x16da400`, which calls the real user `DllMain` `0x14f81d0`. `[dll]`
* **User `DllMain` = `0x14f81d0`** (52 bytes, hand-decoded from the bytes): for `DLL_PROCESS_ATTACH` it calls
  `0x14f7810` ("starting_loading", 2,486 bytes) and returns TRUE; for `DLL_PROCESS_DETACH` it calls a clean-up at
  `0x1cf8755`; nothing for thread attach/detach. `0x14f7810` itself returns at once if the global `0x1826b44f8`
  (orchestrator pointer) is already set, so it runs once. `[dll]`
* **Which thread / when:** the thread that loads the DLL during loader initialisation of the process, i.e. the **main
  thread before `acs.exe`'s own entry point runs**. `0x14f7810` stores `GetCurrentThreadId()` (global `0x1825e27b8`) and
  the log header prints it ("main thread ID: 10040"; every later log line tagged `M` is that thread). Everything below
  (log, settings, symbol resolution, hook installation) happens inside this `DllMain`, under the loader lock. `[dll][log-csp]`
* TLS: the DLL has a TLS index used for the compiler's thread-safe static guards (e.g. the first-call guard in
  `DWriteCreateFactory`); the TLS callbacks listed by Ghidra belong to statically linked third-party CRT code and do nothing
  CSP-specific that was found. `[dll][interp]`
* Process names: **no allow-list**. The only name tests found are cosmetic or file-name derivation:
  `0x150cb30` ("acs_pro", string `0x22b9220`) just picks the emoji printed in front of the log header (🔋 normally, other
  emojis by date or build kind); `0x22789d8` holds `acs\0`, compared with the exe stem to choose between the Steam wording
  ("verify Assetto Corsa installation using Steam") and the generic wording of the missing-PDB message; the PDB file name is
  `<exe stem>.pdb` so a renamed `acs_pro.exe` would look for `acs_pro.pdb`. The product strings `Assetto Corsa v1.16.4` /
  `Assetto Corsa Pro v1.16.4` (function `0xeb5630`) are chosen by a Steam/Pro probe (`0xeb5710` uses the string
  `SteamInit`; `0xeb57c0` uses `GetModuleHandleW` + `GetProcAddress`, probably the Wine probe [interp]). `[dll]`

### 1.2 The DWriteCreateFactory proxy

* Exports (31): `DWriteCreateFactory` (the proxy), `NvOptimusEnablement`, `AmdPowerXpressRequestHighPerformance`, 28
  `ffxFsr2*`/`ffxFsr3Upscaler*` (AMD FSR statically linked; the exports let the engine find them). `[dll]`
* `DWriteCreateFactory` = `0x14fd180` (`decomp_infra/14fd180.c`). On the **first call only** (thread-safe static guard):
  1. `LoadLibraryW(L"nvapi64.dll")` + `GetProcAddress("NvAPI_Initialize")` and call it if present (string `0x22b8e30` /
     `0x22b8e18`);
  2. build `<path kind 2 = system folder>\dwrite.dll` (path helper `0x15047f0(2)`, literal "dwrite.dll" `0x22b8e60`),
     `LoadLibraryW` it; if that fails the process is terminated through the fatal-exit helper `0x6d0a20(1)`;
  3. keep the module handle in `0x1826ae2f8`, resolve the real `DWriteCreateFactory` (`0x22b8e48`) into `0x182708cd8`.
  Every call (first or later) is then forwarded with its three arguments unchanged. `[dll]`
* CSP also **uses DirectWrite itself**: `0x6d9660` and `0x6d97f0` ("real_dwrite_a/b") call the proxy with
  `DWRITE_FACTORY_TYPE_SHARED` (0) and build text layouts; they are called from font/text code (`0x371220`, `0x9735c0`,
  `0x137b040`, `0x137b110`). `[dll]`

### 1.3 Time line (one start-up, `custom_shaders_patch.log`, first lines)

Log columns: elapsed ms, thread id (`M` = main thread), message. `[log-csp]`

| t (ms) | Thread | Log line | Printed by (dwrite.dll RVA) |
|---|---|---|---|
| (before) | M | CRT initialisers run: ~1,640 registration records + 142 gate records pushed into the hook registry | static initialisers (`0x22cbd6` ...) |
| 0 | M | `🔋 CSP v0.3.0-preview649/4184, enabled, date & time: ..., main thread ID: N` + "Do not use this log file, because its format will change." | log opened by `0x14f7810` -> `0x150c630`; strings `0x22b96ac`, `0x22b9658`, `0x22b9678`, `0x22b9608` (the "disabled" variant is `0x22b96b8`) |
| 3493 | M | `Extrapolate state: 4`, `Hotlap mode: no` | `0x138ded0` (reads `general.ini` and `race.ini`) |
| 3494 | M | `Extension DXGI Tweaks disabled: ...` | `0x11e81a0` (see 1.5) |
| 3494 | M | `Setting DPI-aware flag: already set before us, final state: per-monitor-aware v1` | `0xc73560` |
| 3494 | M | `DXGI tweaks are prepared` | `0x14db8f0` |
| 3524 | M | `  All 1542 functions were found successfully` | `0x14fa3a0` (after `0x14fafb0` installation-path check) |
| 3548 | M | `Base settings are ready` | `0x14db8f0` |
| 3548 | M | `Orchestrator is ready` | `0x14f0580` |
| 3548 | M | `Starting loading...` | `0x14f7810` (after `0x14f0580` returns) |
| ~4166 | M | `Race configuration is checked` | `0x14dc880` (reached from the CrashGuard hook, see 1.4) |
| 4167 | various | `Cores: 16, physical: 8, perf.: 8, workers: 7, cfg: 0`; `New thread: CSP: threadpool for threadpools / COM async / background threadpool (size: 4) / files monitoring` | `0x1556e10`, `0x1557a00` |
| 4569 | M | `Core modules are created`; `Extension New Modes disabled: not selected` | `0x14dc880`; `0x11e81a0` |
| 4704 | M | `Additional modules are created` | `0x14dc880` |
| 4729 | M | `Extra hotkeys are configured` | `0x14dc880` |
| 4729 | M | `Setting up AC logging... Done!` | `0x150dde0` (installs the redirect of AC's own `log.txt`) |
| 4731 | M | `Importing DLL: python33.dll`, `Importing DLL: openvr_api.dll` | `0x6ccc90`, the hook of `__HrLoadAllImportsForDll` (see 1.7) |
| 4823+ | M and workers | DualShock scan, monitors, window placement (`0x3f5ed0`, `0x3f30b0`), hardware report, FMOD advanced settings, `Sim initialization started` (`0x11bb510`), `Loading configs for <track>` (`0x13a5140`, `0x14900d0`), WeatherFX, ... | per module |

`[interp]` Reading of the sequence: **stage A** (before the game does anything) = log + settings + symbols + hooks, all in
`DllMain`; **stage B** = the first time the game constructs its `CrashGuard` object (very early in `wWinMain`), the hook
`CrashGuard::CrashGuard` (`0x6c8b70`) runs the module factory `0x14dc880`; **stage C** = per-module work driven by the
hooks as the game loads a session.

### 1.4 The staged start-up ("orchestrator")

| Stage | Function | What it does | Reads / writes |
|---|---|---|---|
| 0 `starting_loading` | `0x14f7810` | QueryPerformanceFrequency/Counter set-up, `MXCSR |= 0x8040` (flush-to-zero + denormals-are-zero for the whole process), `GetCommandLineW` -> parser `0x14f87f0`, `GetModuleFileNameW`, `SetCurrentDirectoryW(exe folder)`, then a check that tests for a file in the exe folder and for the master switch `[BASIC] ENABLED` (literals obfuscated, see 1.9). If enabled: creates the 0x580-byte orchestrator object | command line, exe path, `extension\config\general.ini` + `Documents\Assetto Corsa\cfg\extension\general.ini` `[interp]`, writes `Documents\Assetto Corsa\logs\custom_shaders_patch.log` (path kind 7, literal `0x22af5e8`) |
| 1 `orchestrator_ready` | `0x14f0580` | `0x14db470`; `new` 0x580 bytes; calls `0x14db8f0`; stores the object in `0x1826b44f0`; registers ~30 module callbacks through static function-pointer slots `0x1825f4188..0x1825f42c8` (each an init thunk `0x14f5610..0x14f5840` that forwards to a module's registration); log "Orchestrator is ready" | none directly |
| 1a `base_settings_ready` | `0x14db8f0` | zero-initialises the 0x580-byte settings object (`param+8` = settings container ctor `0x11e82b0`), `0x1387350` loads the base ini set, `0x14dfca0` registers the first module, log "DXGI tweaks are prepared", then `0x14fafb0` (installation path) -> `0x14fa3a0` (symbols + hooks) -> `0x14e0470`, "Base settings are ready"; ends with a block that, if a global is set, shows an obfuscated message box and aborts (see 1.9) | `graphics_adjustments.ini`, `general.ini`, `dxgi_tweaks.ini`, `vr_tweaks.ini`, `small_tweaks.ini`, `gui.ini`, `GUIDs.txt`, sections `[BASIC]`, `[CONFIGURATIONS]`, `[PHYSICS_EXPERIMENTS]`, `[OTHER_EXPERIMENTS]`, `[CONTROL]`, `[NEW_KN5_LOADER]`, `race.ini [REMOTE]`/`[SESSION_0]`/`[RACE]` (strings of `0x1387350`, `0x138ded0`) `[dll]` |
| 2 `core_modules_created` | `0x14dc880` | (a) `0x14ddd60` arms the file-change watcher with the glob `{ extension\? & ! ?\real_mirrors\? & ! ?\state\? & (?.ini|?.lut)}`; (b) `0x14dc510` handles `shaders-cache.txt`; (c) log "Race configuration is checked"; (d) constructs ~30 core module objects with `operator new(size)` + `ctor(this, settings+8)` + register (`0x14dfca0`), some only when two globals (`0x1826b8a18`, `0x18269506f`) and the "is replay/hotlap/headless" byte at `[0x180ef1700()+3]` are clear; sizes 0x100..0x298 bytes; log "Core modules are created"; (e) walks the **additional module map** (`0x14dab50`): for each entry whose name does not start with `_`, forms `<config dir>\<name>.ini` and `<user cfg dir>\<name>.ini` (path kinds 0x11 / 0x12), and if the module is enabled (`ENABLED`, literal `0x21ee468`) calls its factory (`entry+0x48`) and appends the object to the module list at `settings+0x460`; log "Additional modules are created"; (f) installs 4 Windows-message/hotkey handlers (`0x180ee5220`, `0x180ee5670`, `0x180ee5690`, `0x180ee10d0`) and the two virtual keys `__EXT_SAVE_CLIP` / `__EXT_VIRTUAL_MIRROR`; log "Extra hotkeys are configured" | the whole `extension\` tree (watch), `<module>.ini` files |
| 3 AC logging | `0x150dde0` | redirects AC's own `log.txt` writes into the CSP log | `logs\log.txt` |
| 4 DLL import hook | `0x6ccc90` | see 1.7 | - |

### 1.5 `Extension X disabled: reason`, and the extension names

* Every "module" derives from a small extension base: byte at `+8` = active, `std::string` display name at `+0xd8`.
  `0x11e81a0(ext, reason)` prints ``Extension `<name>` disabled: <reason>`` (strings `0x22b...` "Extension `" and
  "` disabled: " at `0x2297840`) once and clears `+8`. `0x12a4500` is the opposite lazy path for a singleton extension
  (``Extension `<name>` enabled: requested`` then sets a set of per-feature bytes). `[dll]`
* The reason is chosen by the module's own constructor. Reasons seen in this PC's log `[log-csp]`: DXGI Tweaks "not available
  with MSAA without post-processing"; New Modes "not selected"; Walking Out "not available in this mode"; Custom Rendering
  Modes "custom mode is not selected"; Mumble Plugin "available in online race only". No other extension produced a line,
  so all other modules stayed enabled.
* Extension names are the `[ℹ] FULLNAME=` of the shipped `extension/config/*.ini` files (57 ini files, 45 not `data_*`):
  General settings; Cars: Instruments; Camera: Chase; UI: Chat shortcuts; Display mode: custom; Information; Other: DXGI;
  Graphics: Extra FX; Input: Wheel (FFB Tweaks); Camera: Free; Input: Logitech G27/29; Input: Gamepad FX; Graphics:
  Adjustments; Track: Grass; UI: General; Graphics: Lighting FX; Other: Music; Camera: Cockpit (Neck FX); Simulation: AI
  (New Behavior); Other: Screenshots; Graphics: Particles FX; Simulation: Rain FX; Graphics: Reflections FX; Track:
  Skidmarks; Other: Small tweaks; Cars: Mirrors; Graphics: Smart Shadows; UI: Loading; Simulation: Surfaces FX; UI:
  Taskbar; Track: Miscellaneous; Display mode: Triple; Cars: Visual; Display mode: VR; Graphics: Weather FX; Cars:
  Windscreen FX. Without FULLNAME (internal/hidden): brakedisc_fx, colorful_shadowing, fake_shadows_fx, lua_tools,
  module_lw, mumble_plugin, shadowed_wheels, spectating, walking_out, yebisest. `[ini]`
  The module-level `ENABLED=` key of those files is, for most, "this option doesn't do anything" (always on); real
  switches: car_instruments 0, chaser_camera 0, chat_shortcuts 0, extra_fx 0, neck 0, surfaces_fx 0, module_lw 0,
  spectating 0 by default. `[ini]`

### 1.6 Symbol resolution

* **Loader:** `0x155a870` ("dbghelp_load", `decomp_infra/155a870.c`) loads `<system dir>\dbghelp.dll` (falls back to bare
  `dbghelp.dll`, string `0x210d1d0`), resolves the DbgHelp exports by name (strings `0x22bd250..0x22bd320`:
  `SymInitializeW`, `SymSetOptions`, `SymLoadModuleExW`, `SymFromAddrW`, `SymGetModuleBase64`,
  `SymGetLineFromAddrW64`, `SymFunctionTableAccess64`; `MiniDumpWriteDump` in the crash code), opens its own process with
  `OpenProcess(PROCESS_ALL_ACCESS)` and calls `SymInitializeW` with a **search path** built from: AC root (path kind 4) +
  `;` + `_NT_SYMBOL_PATH` + `;` + `_NT_ALTERNATE_SYMBOL_PATH` (the two env vars are only read when the flag byte
  `0x18268b39b` is clear; probably the Wine switch [interp]) + another env var (`0x22bd2e0`, read twice, the second time
  with `\system32` appended). `SymInitializeW` is called with `fInvadeProcess = FALSE`. Then `SymSetOptions(0x80212)` =
  `SYMOPT_UNDNAME | SYMOPT_LOAD_LINES | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS` (`0x80202`, without
  `LOAD_LINES`, when that flag byte is set). `[dll]`
* **Symbol subsystem** is the cluster `0xeb07d0..0xeb7165` (`0xeb1ff0` mapping init, `0xeb25f0` version pick,
  `0xeb3060` init/`SymLoadModuleEx`, `0xeb3a3b` `SymFromName`, `0xeb39c2`/`0xeb4820` `SymEnumSymbols`, `0xeb4e90` module-
  qualified lookups, `0xeb6020` resolve-by-name, `0xeb66e0` create-hook). Logic that can be stated `[dll]`:
  * PDB requirement: the file `<AC root>\<exe stem>.pdb` must exist, otherwise the message box (`0x14f9db0`)
    "Required file acs.pdb is missing. Please verify the integrity of your version of Assetto Corsa." (or the Steam wording
    "...verify Assetto Corsa installation using Steam") and, when the missing list is the 3.3 Python function, the VC++ 2010
    runtime hint ("Make sure you have Microsoft Visual C++ 2010 Redistributable Package (x64) installed"). The version hint
    is "Please make sure you're using correct AC version (v1.16.3 or v1.16.4, 64-bit) and have original acs.pdb in AC root
    folder." Product strings `Assetto Corsa v1.16.4` / `Assetto Corsa Pro v1.16.4` at `0x2278e20/0x2278e38`.
    **There is no hash of `acs.exe`**: a different build simply fails because named functions are missing or have the
    wrong size (below).
  * Installation path check `0x14fafb0`: scans the AC root path bytes; any byte > 0x80 (non-ASCII, "non-latin symbols") logs
    "Installation path: ..." and shows "Failed to tweak Assetto Corsa: its installation path contains non-latin symbols.
    Please consider moving it to a different folder. Would you want to continue without Custom Shaders Patch?" (Yes/No box;
    No = exit code 1; Yes = an internal "Invalid installation path" error is raised [interp]). Otherwise it calls the
    resolver.
  * **Direct map / cache.** `general.ini [DEV] DIRECT_MAP_INIT=0` (hidden). The log shows the whole stage took ~30 ms on
    this PC, far too fast for loading a 69,218,304-byte PDB, because CSP uses a **cached map**:
    `<AC>\cache\memory_layout.bin` (524,921 bytes, created 5 Oct and not touched since; strings `0x2278dc0`,
    "Failed to parse AC cached map: " / "Failed to load AC cached map: " at `0x2278a60`/`0x2278a80`, loader near
    `0x1dbb430`, "Caching failed to detect any items, switching to straightforward approach" in `0xeb4820`). Format (read
    here, parsed to the exact end of file): 8-byte header (`e61f2dfb19dfc443`, probably a hash/GUID of the PDB `[interp]`),
    then records `[u8 name_len][name][u8 n][n x (u32 RVA, u32 function size)]`: **6,280 records, 29,142 (RVA, size) pairs**
    (names are demangled `Class::member`, mangled `?name@..` or free names; `n` > 1 are overloads). `[dll]` + data read
  * If the cache is absent or stale CSP falls back to `SymEnumSymbols` over the loaded PDB (`SymLoadModuleEx` with the exe
    path from `GetModuleFileNameW`; "Initializing mapping for <module>" `0x22789b0`) and rebuilds the cache `[interp]`.
    A debugging branch dumps every PDB symbol as `%08X %4u %s` ("Dumping everything:"), gated by a flag. `[dll]`
  * **Overload choice by function size.** A registration record carries the *expected function size* (dword at +0x20;
    `0xFFFFFFFF` marks a data symbol). 92 of ~1,650 records have it. Verified by hand: `StringBlitter3D::blitString`
    expects 0x295 = 661 bytes (acs.exe has two overloads of 661 and 1,481 bytes), `Car::setDamageLevel` 0x29 = 41 bytes
    (overloads of 41 and 13 bytes), `AudioOccluder::AudioOccluder` 0x53 = 83 bytes. `[dll]`
  * **`extension/config/data_alt_mapping.ini`** (shipped, readable) is the escape hatch for other builds: `[OPTIONAL]` (6
    names that may be missing: `CameraShadowMapped::shadowMapPass`, `PythonInterface::writeToLog`,
    `\d3dx11_43\D3DX11SaveTextureToFileW`, `\MSVCP120\...setprecision...`, `\MSVCR120\printf`, `CBuffer::release`;
    `\python33\PyObject_CallFunction` is commented out), `[NAMES_ALT]` (58 lines such as
    `StringBlitter3D::blitString/0x295=0x251`: "when expecting size 0x295, accept 0x251", i.e. the size the other supported
    build (1.16.3 vs 1.16.4) has; plus lambda aliases and `vr::VRCompositor` renames), `[NAMES_WINE]` (22 data symbols
    given by their MSVC-mangled names for Wine/Proton: `device`, `immediateContext`, `swapChain`, `screenRenderTarget`,
    `videoSettings`, `INIReader::cache`, `FolderEncrypter::keys`, `RaceManager::carsToBeLoaded` ...). `[ini]`
* **What happens when symbols are missing** (`0x14fa3a0`, `0x14f9db0`): the walker collects the unresolved names into a
  vector (`0x1826a9cc8`). If it is empty -> enable all hooks, run post-resolution callbacks, log "All N functions were found
  successfully". If it holds exactly one entry equal to `CameraShadowMapped::shadowMapPass` -> tolerated (also
  optional in the ini). Otherwise log "Failed to initialize", (debug flag: dump), then build a message of up to 11 names
  ("... and N more"; if more than 800 names: "Failed to tweak Assetto Corsa, no symbols were found.") followed by "Would you
  want to continue without Custom Shaders Patch?" (Yes/No box, style 0x34 = warning + Yes/No). **No** -> `ExitProcess`-style
  exit through `0x6d09f0(1)`; **Yes** -> return **without enabling any hook** (hooks were created but stay
  disabled) so the game runs vanilla while the rest of CSP is loaded `[interp]`. A failed resolution is not cached: the
  only cache is `memory_layout.bin` of successful lookups.
* **Reconciling 1,542 / 1,441 / 1,368** (see 6): `csp_hook_table.tsv` has only registrations whose name contains `::`
  and that an earlier scan could match to a 40-byte record shape (1,441 sites, 1,368 distinct names, 934 hook + 507 lookup).
  A second scan of the same registration code shape (`mov word [rec+0x24], len` after `lea name`) finds **1,638 named
  records, 1,556 distinct names**, and the union of both scans is **1,716 distinct registered name strings**: 1,386
  `Class::member` (17 more than the table: lambdas, `operator<<`, `GameObject::addGameObject<GhostCar>`, ...), 152
  module-qualified (`\fmod64\FMOD_*` ...), 120 plain (free functions and variables: `kgl*`, `ovr_*`, `ac_*`, `ks*`,
  `device`, `swapChain`, `wWinMain`, ...), 58 mangled (`?...@IPfxBaseContext@PPFX@@...`, FMOD C++ API). Of these 154
  distinct `::` names are CSP's **own** type names (cbuffer structs, `mz::`, `utils::`, `efx::`...) that are not
  `acs.exe` symbols. 1,716 - 154 = 1,562 names to find; the log says 1,542, i.e. 20 fewer. Candidates (not proven): 7
  deferred module names (`\python33\PyObject_CallFunction` + 6 `\openvr_api\...`, resolved after those DLLs load, see
  1.7), up to 6 `[OPTIONAL]` names, and gate-skipped groups (VR / Oculus / Wine, see 1.7) whose count differs per
  machine. The number `1542` therefore is "names resolved in this configuration", **not** "number of hooks". `[dll][interp]`

### 1.7 Hook installation

**Registry.** Each registration is a tiny static-initialiser function (`0x22cbd6`..`0x1ddb6f1`, one per name or per
group) that appends a **40-byte record** to a global fixed vector. The vector object is created lazily (`operator new(0x28)`
at the first registration, ctor `0x1099910`; global `0x1826ff4d0`): fields `[0]` data pointer, `[8]` dword count, `[0xc]`
dword capacity, `[0x10]` condition stack (byte vector), `[0x18]` its size, `[0x1c]` its capacity, `[0x20]` byte "gate
active". Record layout (hand-decoded from the record writer at `0x22cbd6` and from the walker `0x1099a70`/`0x1099a9e`;
the names of the gate operations 5 and 6 are an interpretation of the decoded control flow): `[dll][interp]`

| Offset | Size | Field | Meaning |
|---|---|---|---|
| +0x00 | 8 | name pointer | C string in `.rdata` (`Class::member`, `\module\\function`, `?mangled`, plain name) |
| +0x08 | 8 | detour / predicate | hook: CSP replacement function. lookup: 0. **gate record: predicate function** |
| +0x10 | 8 | slot pointer | `.data` cell that receives the result (see below) |
| +0x18 | 8 | `bool*` "missing" flag | optional; walker sets the byte to 1 when a lookup yields 0 (0 for most records) |
| +0x20 | 4 | expected size | function size in bytes to pick the overload; `0xFFFFFFFF` = data symbol; 0 = any |
| +0x24 | 2 | name length | **the last word** the project note mentions (`mov word [rec+0x24], len`) |
| +0x26 | 1 | kind / gate op | 0 normal; 1,2 = "evaluate predicate, store the bool in a global" (6 and 14 uses); 3 = `#if pred` (52); 5 = `#elif pred` (17); 6 = `#else` (1); 4 = `#endif` (52) |

* `kind 1/2` records set two global flags from a predicate; `3/5/6/4` form a preprocessor-like stack (a record byte per
  nesting level; bit0 = "this branch active", bit1 = "a branch was already taken"; the byte `[+0x20]` of the registry is the
  AND of all levels). Normal records inside an inactive branch are skipped. 48 distinct predicate functions; typical
  ones are 3-18 bytes: "return true", "return global byte", "return `[getter()+3]`". `[dll]` Interpretation: these are build/
  OS/mode gates (VR, Wine, new GUI, ...). `[interp]`
* **Walker:** `0x1099a70` (46 bytes, hand-decoded) loops the records; per record `0x1099a9e` handles the gate op or, if
  the gate is open: calls the resolver `0xeb6020(name_view, size)`; for a **lookup** (+0x08 == 0) it stores the
  address in `*(+0x10)` and sets `*(+0x18)` if zero; for a **hook** it calls `0xeb66e0(name_view, size, target,
  detour, slot)` which wraps the hook library's create call. The walker is called once from `0x14fa3a0`. `[dll]`
* **Hook library = embedded MinHook** `[interp]` (no MinHook/Detours strings, identified by shape): `0x1cd1d70` apply/
  enable-all (calls the freezer `0x1cd1be0`, the patcher `0x1cd1ad0` twice (enable/undo), the unfreezer part), `0x1cd1be0`
  suspend all other threads (`OpenThread` + `SuspendThread`, thread-context fix-up via `GetThreadContext`/`SetThreadContext`
  at `0x1cd2390`/`0x1cd23f3`, so an instruction pointer inside a patched prologue is moved), `0x1cd1ad0` patch bytes
  (`VirtualProtect` RWX, write, `FlushInstructionCache`, restore), `0x1cd2003` create-hook (allocates a trampoline in a
  buffer near the target via `0x1cd264d` `VirtualAlloc`/`VirtualQuery`, builds it with a length disassembler,
  `0x1cd2870`), `0x1cd2250`/`0x1cd22f0` init/uninit (private heap `HeapCreate`), `0x1cd1f90` leave/unlock. 14fa3a0 calls
  `0x1cd1d70(1)` once when the name list is complete; `Sim::Sim` (`0x11bb510`) calls it again after adding one more hook.
  The detour is a **jump over the function prologue** (inline hook), so any other caller of the function is covered, and
  `*slot` receives the **trampoline** (callable original). Byte length (5-byte rel jump vs absolute) unknown. `[dll][interp]`
* **Second mechanism, vtable patch.** Only one hook function (`Sim::Sim`, `0x11bb510`) touches page protection itself: after
  calling the original constructor through its slot `0x1826add10` it takes the object's vtable, `VirtualProtect`s the
  page range (`0x40` = RWX), saves the entry at vtable offset `0x48` into `0x1826aa8f8`, writes `0x107cd30` and
  restores the protection. It also calls `0xeb66e0` directly to hook the function at vtable offset `0x1c8` of the global
  object `*0x1826aa890` with detour `0x107bc90`. So virtual calls on that object are intercepted by data patch, not by
  code patch. `[dll]`
* **Deferred module names.** `0x6ccc90` is the detour of `__HrLoadAllImportsForDll(const char*)` (acs.exe delay-load
  helper; string `0x22244d0`, slot `0x1826a8f00`). For `python33.dll` and `openvr_api.dll` it logs "Importing DLL:
  ...", calls the original, then runs a closure (`0x6d75a0`) that resolves the `\python33\\...` and `\openvr_api\\...`
  names now that those DLLs are in memory. `[dll]`
* **Per-hook "gate".** There is no per-hook enable at run time; the hook always runs and decides internally (see the
  examples). Run-time on/off is by flags the hooks read (`0x1826b8a18`, `0x1826b8a1d`, `0x18269506f`, per-car objects).

**Examples (patterns, no pseudo-C):**

* `ABS::step` (hook `0x563dc0`, original through the slot at RVA `0x268a850`, i.e. `0x18268a850`): reads the car index from the
  car object (`car+0x3e9c`), finds CSP's **per-car extension object** in a vector of 8-byte pointers (`0x1826ae290`..
  `0x1826ae298`; an override pointer `0x1826a9d50`+0xe0 when a single-car debug flag `0x1825e2728` is set). If the object
  has its own ABS controller (`ext+0x1d8` non-null) CSP runs its **own 4-wheel ABS implementation** (look-up tables, rate
  limits, per-wheel brake modulation) and **does not call the original**; if the object exists but has no controller it
  calls the original and then blends four values; if no object, it just calls the original. So the "extended" test is the
  **existence of the per-car extension object / feature pointer**, not a bool on the car. `[dll]`
* `Car::step` (hook `0x5996f0`, original through slot `0x268aa98`): optional per-car timing record (256 entries at
  `0x1826a3fb0`, thread id + QPC, feeds the `ac.ext_perfBegin/End` profiler); for car index 0 first calls helper
  `0x599000`; fills extension state from the car's contact patches; then **always calls the original**
  (`slot(car, dt)`) between its own pre and post work (`0x597f00`, `0x10eb8f0`), and finally accumulates elapsed time.
  So CSP code wraps rather than replaces the step. `[dll]`
* `ksgui::GUI::render` (hook `0x6129b0`, 72 bytes) saves/overrides a flag at `gui+8` and calls CSP's render
  `0x6127a0` instead of the original; `ksgui::GUI::GUI` (`0x6107e0`) and `ksgui::GameScreen::GameScreen` (`0x60da50`) call the
  original then store `this` in a global (singleton capture, 45 bytes); `ksgui::GUI::onMouseDown` (`0x610810`) ignores
  the click when CSP's ImGui layer wants the mouse (`0x1826a2877`, `0x1826a4e44`), otherwise replays AC's own dispatch
  loop; `ksgui::Form::hitTest` (`0x623760`) is a full re-implementation with CSP's "hit area" margins. Pattern: **singleton
  capture, input gating, full replacement** (original not called). `[dll]`
* Other recurring patterns: `FolderEncrypter::decryptFile` (`0x5d7490`) serves files from CSP's virtual file layer and
  falls back to a "NOT FOUND" result; `CrashGuard::CrashGuard` (`0x6c8b70`) deletes old `CrashDumps\acs.exe.*.dmp`,
  starts the module factory and logs "Launched from: ..." and "Env.: ..."; `ksSecurity::validate` (`0x428cd0`) is a
  pass-through that calls the original (nothing is bypassed there). `[dll]`

### 1.8 Other attachment points not covered by the `Class::method` table

All names below are registrations in the **module-qualified / plain** families (152 + 120 + 58). Counts of the
module-qualified family by DLL (hook/lookup by a code-shape heuristic, ±2): `[dll][interp]`

| DLL | Names | Mostly | What CSP does |
|---|---|---|---|
| `fmod64` | 69 | lookups | calls the FMOD core C API (channels, groups, DSPs, sounds, system) directly; 1 hook |
| `fmodstudio64` | 28 | 21 hooks | hooks the FMOD Studio event/instance/system functions to mirror or extend audio |
| `steam_api64` | 16 | 11 hooks | Steam interfaces (`SteamApps`, `SteamFriends`, `SteamUser`, `SteamUserStats`, `SteamUtils`, `SteamScreenshots`, `SteamController`, `SteamAPI_*` callbacks); 'Failed to hook to Steam screenshots', 'Failed to get ticket' |
| `USER32` | 9 | hooks | `GetWindowRect`, `RegisterClassW` (window class), `GetAsyncKeyState`, `GetCursorPos`, `SetCursorPos`, `ShowCursor`, `SetCursor`, `MessageBoxA/W` |
| `ntdll` | 6 | hooks | `NtCreateFile`, `NtOpenFile`, `RtlAllocateHeap`, `RtlFreeHeap`, `RtlReAllocateHeap`, `RtlSizeHeap` (CSP replaces AC's heap with its own allocator) |
| `openvr_api` | 6 | hooks | `VR_InitInternal`, `VR_ShutdownInternal`, `VR_GetGenericInterface`, `VR_IsInterfaceVersionValid`, `VR_GetInitToken`, `VR_GetVRInitErrorAsEnglishDescription` |
| `KERNEL32` | 6 | hooks | `CreateFileA/W` (file redirection), `WritePrivateProfileStringW`, `DeviceIoControl` (trigger vibration), `VirtualAlloc`/`VirtualFree` |
| `d3dx11_43` | 4 | 1 hook | `D3DX11SaveTextureToFileW/ToMemory`, `CreateShaderResourceViewFromMemory`, `GetImageInfoFromMemory` |
| `MSVCR120` | 2 | hooks | `exit` ("Assetto Corsa requested an exit: ..."), `printf` (AC's debug print is captured into the log) |
| `WSOCK32` | 2 | hooks | `bind`, `connect` (server-IP binding: "Socket is bound to target IP") |
| `XINPUT1_3` | 2 | 1 hook | `XInputGetState`, `XInputSetState` |
| `D3D11` | 1 | hook | `D3D11CreateDeviceAndSwapChain` (`0x6c5170`) |
| `MSVCP120` | 1 | hook | `std::setprecision` (log formatting) |
| `python33` | 1 | hook | `PyObject_CallFunction` (`0x6f4ad0`), deferred (1.7) |

* **Direct3D 11.** `D3D11CreateDeviceAndSwapChain` is hooked (`0x6c5170`, strings "Failed to create device and swap chain:",
  "No redirect for this device", "Pre-initializing MMF: creating offscreen DirectX device"). CSP also creates its own
  devices with `D3D11CreateDevice` (3 callers: `0x6c3f50`, `0x6c5170`, `0xbafcd0` media-foundation helper). acs.exe's own
  D3D globals (`device`, `immediateContext`, `swapChain`, `activeRenderTargetView`, `activeDepthStencilView`,
  `blendStates`, `cullStates`, `depthStates`, `currentLayout`, `screenRenderTarget`, `screenDepthTarget`,
  `videoSettings`, `frameInformation`, `formCurrentOffset`) are **data-symbol lookups**, not vtable patches: CSP reads
  the game's own device and then drives it itself. The renderer entry points `kglInit`, `kglOpenWindow`, `kglSwapBuffers`,
  `kglDraw`, `kglDrawIndexed`, `kglSetShader`, `kglSetTexture`, `kglSetRenderTargets`, `kglResolveRenderTarget`,
  `kglResizeBuffers`, `kglSaveScreenCapture`, ... (28 `kgl*` names) are hooked/looked up; `initDX11` and
  `createDeviceAndSwapChain` as well. There is **no generic D3D11 context vtable patch** found (only `ID3D11DeviceContext::ResolveSubresource`
  as a CSP-own lookup name). Imports of the DLL itself: `d3d11!D3D11CreateDevice`, `dxgi!CreateDXGIFactory/1`, delay
  `D3DCOMPILER_47`, `libxess_dx11.dll` (Intel XeSS, 7 functions), `ffx_backend_dx11_x64.dll` (FSR 3.1, 5 functions),
  `MFPlat.dll` (video playback). `[dll]`
* **Window procedure / messages.** acs.exe functions `WndProc`, `onKeyDown`, `onKeyUp`, `onChar`, `onMouseDown/Up/Move/
  Wheel` are registered by name; CSP hooks `RegisterClassW` and also creates its own windows (own class). It reads raw
  input (`GetRawInputData`, `RegisterRawInputDevices`; `0xf4c1b0` "csp_raw_input"), `RegisterHotKey`
  (`0x1386fc0` "csp_crash_on_f12"), IME (`Imm*`), `keybd_event`, clipboard. No `SetWindowsHookEx` import. `WRAP_HOOKS=1` in
  `general.ini` refers to the Lua keyboard/mouse hook API. `[dll][ini]`
* **DirectInput / XInput / HID.** acs.exe `DirectInput::*` (6 hooks, 1 lookup), `InputDevice::*` (6/1), `X360Joypad::*`,
  `JoypadCarControl::*`, `KeyboardCarControl::*`, `EnumJoysticksCallbackDI` are hooked; XInput through the module-
  qualified pair; the DLL itself imports `HID.DLL` (`HidD_GetFeature`, ... DualShock/DualSense support, `0x6bc840`),
  `SETUPAPI`. No DirectInput COM vtable patch found. `[dll]`
* **Python.** Hooked: `PyEnvironment::PyEnvironment`, `PyModule::PyModule`, `PyPlugin::PyPlugin/update/shutdown`,
  `PythonInterface::*` (writeToLog, printErr, getTexture), `PythonForm::PythonForm`, 13 `ac_*` API function names
  (`ac_newApp`, `ac_addOnClickedListener`, ...), and `\python33\PyObject_CallFunction`. `extension/internal/python/
  initialize.py` + `traceback_helper.py` run inside AC's interpreter; `functions.txt` lists the **213 `ac.ext_*` Python
  functions** CSP adds to the `ac` module. `[dll][ini]`
* **FMOD.** `AudioEngine::*` (17 hooks), `AudioEvent`, `AudioReverb`, `AudioOccluder`, `CarAudioFMOD::*` (11) hooked, plus the
  98 FMOD C/Studio names above and 6 C++-mangled FMOD names (`setSoftwareChannels`, `setAdvancedSettings`,
  `Studio::System::initialize` ...). Log "[FMOD] Advanced settings", "[FMOD] System: 2048 channels". `[dll][log-csp]`
* **Steam.** 11 hooks on `SteamInterface::*` + 16 `\steam_api64\` names; `SteamInit` / `SteamShutdown` names. `[dll]`
* **OpenVR / Oculus.** `OculusManager::update`, `ViveManager`, `StereoCameraVive` (11 hooks), `ovr_*` (14 names: LibOVR stub
  inside acs.exe), `\openvr_api\` (6), OpenXR (`openxr_loader.dll` in `internal/plugins`; strings `xrCreateActionSpace`...). `[dll]`
* **File access redirection.** `\KERNEL32\CreateFileW/A` hooks (`0x6cb0f0`, `0x6cae80`) treat `\AppData\`,
  `Cookies`, `system\cfg\fades.ini`, `changelog.txt`, `_splitscreen.ini`, `_showroom.ini`, `_headless.ini`; `\ntdll\NtCreateFile`
  / `NtOpenFile` (`0x6cad60`, `0x6cadf0`); `Path::getLastModificationTime/fileExists/getFolders/getDocumentPath`,
  `FolderEncrypter::decryptFile/fileExists`, `INIReader::*` (the whole settings reader, 12 hooks) so that AC's reads of its
  own cfg files can be answered from CSP's embedded copies (50 `ac/*.ini` in embedded zip #2) or from CSP's merged config.
  Hidden key `FILE_OP_HOOK=<dll>, <export>` lets an external DLL take over file operations. `[dll][ini][interp]`
* **Crash handling.** `wWinMain` is a registered name; its replacement is `0x13d0ac0` (10,575 bytes: race.ini / controls.ini
  / video.ini / python.ini, "Forced crash requested", "Out of memory", "unhandled exception", "NVIDIA", GPU/driver data)
  = CSP's crash reporter around the game's main. Helpers: `0x13d0490` `MiniDumpWriteDump` ("crash-%s.dmp", "Minidump has
  been written: ", "Minidump exceeded its time"), `0x6c9d90` `AddVectoredExceptionHandler` + `SetUnhandledExceptionFilter`
  + `SetErrorMode`, `0x13d02c0` classifies the faulting module by name (`ntdll`, `d3d11`, `kernel`, `kernel32`,
  `kernelbase`, `user32`, `dwrite`, `dinput8`), `0x3ee3d0` freezes threads with the label "Custom Shaders Patch code",
  `0x13d05f0` resolves mapped names (`\Device\Mup`). `CrashGuard::CrashGuard` hook cleans `CrashDumps\acs.exe.*.dmp`;
  `[BASIC]`-level keys `CHECK_FOR_RAPID_CRASHES` ("Did Assetto Corsa just crash?" box, `0x22b94a8`), `CRASH_ON_F12`
  (Ctrl+Shift+Alt+Win+F10), `CRASH_PLUGIN` (separate process `plugins/AcTools.ErrorReporter.exe` shows the message),
  `DETAILED_CRASHES`, `SAFE_EXIT`. `[dll][ini]`
* **Networking helpers of CSP itself:** WinHTTP incl. WebSocket (34 imports), WS2_32 (161 delay imports; LAN beacon
  `0x38f960`), `libgns.dll` plus `libopus`, `librnnoise`, `libspeexdsp`, `libsqlite3` in `internal/plugins`. `[dll]`

### 1.9 Safety, integrity, encryption

* **AC version differs / PDB missing:** see 1.6; the checks are "PDB present", "every non-optional name resolves", sizes; user
  gets a Yes/No box and may continue without CSP. No version number comparison was found beyond the message text.
* **CSP disabled by the user:** `general.ini [BASIC] ENABLED=0` ("set to 0 to disable everything completely"). Log header
  becomes "CSP v..., disabled, date & time: ..." (`0x22b96b8`) and the orchestrator is not constructed: no settings, no
  symbol lookup, no hooks. The proxy keeps forwarding `DWriteCreateFactory`, so the game starts as vanilla (a 139 MB
  DLL still loaded). `[dll][ini][interp]`
* **Online verification.** Server-side `[EXTRA_RULES] VERIFY_RAIN_FX=1` forces clients to load the *verified*
  RainFX configs shipped in the DLL: embedded zip #3 (101 per-track ini files) has the archive comment "RainFX configs for
  AC CSP, packed for online verification ... Use `[EXTRA_RULES] VERIFY_RAIN_FX=1` in extended config of your online server
  to force clients to load verified configs instead. Integrity: <base64 SHA-256>". Related strings: `_VERIFY_INTEGRITY`,
  `CHECKSUM`, `REQUIRED_VERSION`, "modified or damaged" (`0x94dcd0`, track/surfaces checks), "Checksum failed. You have
  been kicked out of the race" (`0x572e80`, the join path comparing `system/data/surfaces.ini`), "PROBLEMS DURING
  CHECKSUM" (`0x12ff540`), "Invalid packed flags (checksum failed)" (`0xed9310`, car tweaks), "Penalty message:
  verification failed" (`0xee10d0`), "Integrity check failed" / "Failed to calculate checksum" (`0x15c9db0`: the external
  custom-physics/raycast module loader, exports `csp_Initialize_v1`, `csp_Raycast_v1`, `csp_GetLastError_v1`).
  `[dll][ini]`
* **Signature checks:** `WinVerifyTrust` (`0x1582b06`), Authenticode via `imagehlp` (`0x1cd3884`) and a CryptoAPI
  signer-name check ("Logitech Inc", `0x1cd3974`; Logitech wheel DLL), `NP_GetSignature` for TrackIR
  (`0x112b260`). An RSA-3072-looking public key (base64 `MIIBoDANBgkq...` at `0x22b9bf0`) sits among the Crypto++ strings. `[dll]`
* **Encryption.** `FolderEncrypter::decryptFile` / `fileExists` (hooks `0x5d7490`, `0x5da110`), the data symbol
  `FolderEncrypter::keys`, `KN5IO::addDLCKey`, `KN5IO::dlc_keys` are **AC's own** content-protection layer (Kunos
  encrypts some stock folders); CSP wraps it so that the same file API also serves CSP's own virtual files (a table keyed
  by path, with a "NOT FOUND" fallback). Nothing was decrypted, no key material was looked at or extracted. Content that
  a mod maker encrypted for CSP is outside this brief: **stop here**. `[dll]`
* **Obfuscated literals.** A handful of strings in the start-up path (`0x14f7810`, `0x14db8f0`, `0x6c8b70`) are stored
  XOR/rotate-obfuscated with a global byte key and decoded at run time (the check of the master switch/file, the final
  block of `0x14db8f0`, part of the crash code). `0x14db8f0` ends with a condition that, when it holds, builds an
  obfuscated message and aborts the process with a breakpoint. It looks like an anti-tamper / licence-style guard. Not
  decoded (rule 2); guarded: master-switch lookup, a final integrity-type condition, a crash message. `[dll][interp]`

## 2. Game parts it replaces or extends

Source of every number: `csp_hook_table.tsv` (1,441 registration sites; 934 hook + 507 lookup; 387 classes). Hook = replacement
function (`hook_fn_rva` non-empty); lookup = address (or data symbol) only. The replacement-function RVAs and slot RVAs of each
row are in the table; this section gives the per-class totals. **Arithmetic:** hooks 136 + 24 + 165 + 82 + 76 + 38 + 117 + 31
+ 36 + 31 + 47 + 28 + 30 + 12 + 80 + 1 + 0 + 0 = 934; lookups 67 + 15 + 59 + 24 + 14 + 3 + 57 + 12 + 12 + 16 + 6 + 11 + 10 +
3 + 28 + 117 + 1 + 52 = 507; 934 + 507 = 1,441 sites; classes 51 + 3 + 60 + 24 + 23 + 14 + 74 + 6 + 9 + 12 + 8 + 6 + 13 +
5 + 25 + 42 + 1 + 11 = 387. Same names registered more than once (overloads / several registrations): 55 names with 73
extra rows -> 1,441 - 73 = **1,368 distinct names** (matches FACTS). Rows without an `acs.exe` match: 186 (3 hooks +
183 lookups; 154 distinct names; CSP's own types plus 2 destructors `CBuffer::~CBuffer`,
`AnimationPlayer::~AnimationPlayer` that the PDB spells differently); with a match: 1,255 sites (931 hooks + 324 lookups),
1,214 distinct names.

### 2.1 By acs.exe library / object (hooks / lookups)

| Library or object | Hooks | Lookups | Total | Atlas area |
|---|---|---|---|---|
| ksPhysicsAC.lib | 155 | 82 | 237 | physics, ai_drivers, misc |
| KunosSim.lib | 156 | 57 | 213 | renderer, camera, content_loading |
| (not in acs.exe: CSP-own names) | 3 | 183 | 186 | - |
| SystemApps.lib | 53 | 5 | 58 | ui (built-in apps), camera, track |
| ksGUI.lib | 29 | 19 | 48 | ui |
| CarAvatar.obj | 30 | 16 | 46 | content_loading |
| ksAudioFMOD.lib | 34 | 4 | 38 | audio |
| RaceManager.obj | 18 | 17 | 35 | misc (race logic) |
| ksCommon.lib | 25 | 1 | 26 | config |
| sim.obj | 14 | 10 | 24 | main_loop |
| ksCameraYebis.lib | 19 | 3 | 22 | camera / renderer (YEBIS) |
| kml.lib | 13 | 5 | 18 | renderer (splines, maths) |
| ReplayManager.obj | 16 | 2 | 18 | replay |
| ksGame.lib | 12 | 2 | 14 | main_loop, input |
| ACClient.obj | 12 | 1 | 13 | multiplayer |
| ksDI.lib | 11 | 2 | 13 | input_ffb |
| kgl.lib | 10 | 3 | 13 | renderer |
| TrackAvatar.obj | 11 | 2 | 13 | track |
| CarAudioFMOD.obj | 12 | 0 | 12 | audio |
| SteamInterface.obj | 11 | 0 | 11 | misc |
| AnalogInstruments.obj | 4 | 6 | 10 | ui |
| 125 further objects (<= 9 each) | 286 | 87 | 373 | various |
| **Total** | **934** | **507** | **1,441** | |

Check: 645 + 3 + 286 = 934 hooks; 237 + 183 + 87 = 507 lookups. The 125 small objects are in the per-class lists below
(`Camera*.obj`, `Setup*.obj`, `Digital*.obj`, `Form*.obj`, `Pit*.obj`, `Flames.obj`, ...).

### 2.2 Per class (hooks/lookups), grouped by atlas area

`Class h/l` = number of hook / lookup registrations. Library in front of each group.

**physics: 51 classes, 136 hooks, 67 lookups (203)**
- ksPhysicsAC.lib: ABS 2/0; AeroMap 0/3; AntirollBar 1/0; AutoBlip 0/1; AutoShifter 0/1; BrakeSystem 2/1; BrushSlipProvider 0/1; Car 26/7; CarColliderManager 1/1; DRS 1/0; DRSManager 1/0; Damper 1/0; DriftModeComponent 0/1; Drivetrain 6/3; DynamicController 3/1; DynamicControllerStage 0/1; DynamicWingController 2/0; EDL 0/1; ERS 0/2; Engine 4/5; FuelLapEvaluator 1/0; HeaveSpring 1/0; Kers 0/1; PhysicsEngine 4/5; PhysicsObject 2/0; RaceEngineer 11/2; SetupItem 0/1; SetupManager 3/1; SlipStream 0/1; SpeedLimiter 2/0; SplineLocator 0/3; StabilityControl 0/1; SteeringSystem 0/1; Suspension 5/1; SuspensionAxle 3/0; SuspensionML 3/0; SuspensionStrut 4/1; Track 0/3; TractionControl 1/0; Turbo 3/0; Tyre 17/7; TyreThermalModel 8/2; Wing 4/2
- ksPhysicsCoreODE.lib: CollisionMeshODE 1/0; PhysicsCore 0/1; ksPhysicsCoreODEFactory 1/0
- PhysicsAvatar.obj: PhysicsAvatar 8/0 - PhysicsDriveThread.obj: PhysicsDriveThread 4/0 - ksTyreModel.lib: SCTM 0/1 - SuspensionAnimator.obj: 0/3 - SuspensionAvatar.obj: 0/1

**ai_drivers: 3 classes, 24 hooks, 15 lookups (39)** - ksPhysicsAC.lib: AIDriver 12/9; AISpline 5/2; AISplineRecorder 7/4

**renderer: 60 classes, 165 hooks, 59 lookups (224)**
- KunosSim.lib: CBuffer 7/2; CubeMap 2/3; CubeMapRenderer 1/0; Curve 4/2; DynamicBuffer 0/1; Font 2/2; GLRenderer 4/1; GraphicsManager 22/14; HDR 0/1; Material 8/1; MaterialFilter 1/1; MaterialFilterSM 1/1; OculusTexture 1/0; RenderTarget 1/0; Shader 3/0; ShaderManager 2/0; ShaderVariable 2/0; SkyBox 3/0; SplineStripBuilder 3/0; StereoCameraForward 9/2; StereoCameraVive 11/3; StringBlitter3D 0/2; TextNode 1/1; sphere 0/1; vr 1/1
- kgl.lib: CCollectionLoader 0/1; KGLCBuffer 1/0; KGLIndexBuffer 1/0; KGLRenderTarget 1/0; KGLShader 3/2; KGLTexture 3/0; KGLVertexBuffer 1/0
- ksCameraYebis.lib: KGLYebisParticleManager 2/0; KGLYebisParticleSet 1/0; YebisPP 6/2; mat44f 1/0
- kml.lib: InterpolatingSpline 11/3; Spline 2/2 - ksParticleSystem.lib: ParticleSystem 1/3; StaticParticleSystem 1/3
- (CSP-only names): FW1FontWrapper::CFW1GlyphRenderStates 0/1; ID3D11DeviceContext 0/1 - FW1FontWrapper.lib: CFW1Factory 2/0
- objects: BrakeDiscGraphics 2/0; MaterialVar 1/0; BlurredObjects 2/0; CarBrakeLights 2/1; CarFakeShadow 3/0; CarMirrorManager 2/0; DynamicCarEffects 2/0; Flames 8/0; FuelLight 2/0; MirrorTextureRenderer 2/0; OculusManager 1/0; ScreenCapturer 4/0; Texture 0/1; TyreBlur 2/0; TyreSmoke 2/0; VirtualMirrorRenderer 3/0; ViveManager 1/0

**content_loading: 24 classes, 82 hooks, 24 lookups (106)**
- KunosSim.lib: Animation 1/0; AnimationBlender 1/0; AnimationPlayer 4/0; Mesh 5/0; Model 2/0; Node 4/3; NodeBoundingSphere 1/1; NodeEvent 1/0; Renderable 2/0; ResourceStore 5/0; SkinnedMesh 4/3; WorldMatrixTraverser 1/0
- CarAvatar.obj: BackfireParams 1/0; BrakeBiasNotifier 1/0; CarAvatar 28/13; MGUHModeNotifier 0/1
- other objects: CarAnimation 1/0; CarAnimations 3/0; CarHintLoader 2/0; CarLodManager 4/2; CarNodeSorter 2/0; DamageDisplayer 2/0; DriverModel 4/0; VisualDamageManager 3/1

**camera: 23 classes, 76 hooks, 14 lookups (90)**
- KunosSim.lib: BoundingFrustum 2/1; Camera 7/4; CameraForward 7/0; CameraMeshFilter 1/1; CameraMirror 2/0; CameraMouseControlBase 2/1; CameraShadowMapped 5/1; CameraTripleScreen 3/0
- SystemApps.lib: DriverEyesPositioner 4/0; FormCamera 1/0; TripleScreenManager 3/0
- objects/libs: ACCameraManager 6/2; CameraCarManager 1/1; CameraDrivableManager 7/0; CameraFacing 1/0; CameraForwardYebis 9/1; CameraManager 1/0 (ksACCamera.lib); CameraModeStart 1/0; CameraOnBoard 3/1; CameraOnBoardFree 3/0; CameraTrack 3/1; CinematicFreeCamera 1/0; TrackIR 3/0

**track: 14 classes, 38 hooks, 3 lookups (41)** - TrackAvatar 11/2; TrackObject 3/0; DynamicTrackManager 2/0; IdealLine 1/0; StartingLights 3/0; StartingTrackSemaphore 1/0; SurfacesManager 2/0; SunAnimator 4/0; TrackDescription 1/0; TrackMap 3/0; WeatherManager 2/0; SkidMarkBuffer 1/1; RotatingObjects 2/0; AnimatedLights 2/0

**ui: 74 classes, 117 hooks, 57 lookups (174)**
- SystemApps.lib (built-in apps/forms): ACDriverPerformance 1/0; ACEssentials 2/0; ACHelp 1/0; ACSuspension 1/1; ACTimeFlow 1/1; CarPedalsForm 0/1; CarPhysicsForm 1/0; FormAI 3/0; FormBallast 1/0; FormChat 1/1; FormDRSZones 1/0; FormGameStats 1/0; FormGear 2/0; FormLogoAC 1/0; FormMGU 1/0; FormRealTimeTiming 1/0; FormRunningTime 1/0; FormTelemetry 1/0; FormVolumes 1/0; FormWings 0/1; FriendsLeaderboardDisplayer 1/0; LeaderboardApp 1/0; PhotoMode 1/0; PlayerCurrentTime 3/0; PostProcessFilterSelector 2/0; RaceFuelMonitor 1/0; RacePosition 1/0; SetupDebug 1/0; TelemetryApp 1/0; TyreTesterForm 1/0; TyreWearingDebug 1/0; TyresApp 1/0; ksgui::FormRenderStats 2/0
- ksGUI.lib: CarLabel 1/0; TabBar 1/1; ksgui 2/0; ksgui::Control 0/6; ksgui::Form 4/0; ksgui::GUI 15/0; ksgui::GameScreen 3/0; ksgui::ListBox 0/2; ksgui::Spinner 1/2; ksgui::TaskBarIcon 0/1; ksgui::Taskbar 2/7
- objects: QuickMenuPitstop 6/2; QuickMenuPitstopItem 0/1; SetupElectronics 2/0; SpinnerData 1/0; AnalogInstruments 4/6; DigitalInstruments 3/1; DigitalItem 3/0; DigitalLed 1/0; DigitalPanels 2/0; DisplayNode 2/1; DriverNameDisplayer 4/0; DriverStatusSystemMessage 0/1; ESCMenu 1/2; EndSessionDisplayer 2/1; EscMenuTimeTable 0/3; OverlayLeaderboard 2/0; PauseMenu 1/3; ProximityIndicator 1/0; SessionLeaderboard 1/0; SessionStatusSystemMessage 1/0; SetupFuelManager 1/0; SetupGearManager 1/1; SetupIOManager 1/1; SetupInformation 1/1; SetupPitstopPresets 1/2; SetupScreen 2/5; SetupTab 1/1; SetupTyresManager 1/0; SystemMessage 3/0; ksgui::ListBoxRowData 0/1 (ComparativeTelemetry.obj)

**main_loop: 6 classes, 31 hooks, 12 lookups (43)** - Game 8/1; GameObject 2/0 (ksGame.lib); GameTime 1/0; Sim 13/10 (sim.obj); SplashScreen 5/0; PostProcessEffectsUpdater 2/1

**config: 9 classes, 36 hooks, 12 lookups (48)** - ksCommon.lib: FileChangeObserver 3/0; FolderEncrypter 2/1; INIReader 12/4; INIReaderDocuments 1/0; Path 8/0 - KunosSim.lib: CrashGuard 1/0; DebugLog 2/1; KN5IO 5/5 - Console.obj: Console 2/1

**input_ffb: 12 classes, 31 hooks, 16 lookups (47)** - ksDI.lib: DirectInput 5/1; InputDevice 6/1 - ksGame.lib: X360Joypad 2/1 - X360CarControl.obj: JoypadCarControl 4/3; SecondaryJoypadButton 1/0 - DICarControl 4/2; DICommandManager 1/0; DIControlAxis 0/1; DIControlButton 1/3; KeyboardCarControl 2/4; MouseHider 4/0; FFBForm 1/0 (SystemApps.lib)

**audio: 8 classes, 47 hooks, 6 lookups (53)** - ksAudioFMOD.lib: AudioEngine 17/0; AudioEvent 7/1; AudioOccluder 1/3; AudioReverb 9/0 - CarAudioFMOD.obj: AudioBankRef 1/0; CarAudioFMOD 11/0 - TrackAudio 1/0 - (CSP-only) AC::FMOD 0/2

**replay: 6 classes, 28 hooks, 11 lookups (39)** - ReplayManager 16/2; ReplayRecorder 1/7; ReplayInterface 1/1; GhostCar 5/0; GhostCarRecorder 4/0; ComparativeTelemetryRecorder 1/1

**multiplayer: 13 classes, 30 hooks, 10 lookups (40)** - ACClient 12/1; ACChat 0/2; NetCarStateProvider 6/3; TCPSocket 1/0; UDPPacket 4/0 (ksNetwork.lib); UDPCommandListener 2/0; RemoteTelemetryUDP 2/0; QOSOverlay 1/1; RaceControlScreen 1/0; RaceControlVoting 0/1; NetMonitorForm 1/0; CommandManager 0/1; (CSP-only) AC::socket_configure_qos 0/1

**python_api: 5 classes, 12 hooks, 3 lookups (15)** - PyEnvironment 1/1; PyModule 1/0 (ksPython.lib); PyPlugin 3/0; PythonForm 1/1; PythonInterface 6/1

**misc: 25 classes, 80 hooks, 28 lookups (108)** - ksPhysicsAC.lib: LapInvalidator 1/0; PenaltyManager 1/1; PerformanceMeter 2/1; Telemetry 0/1; TimeTransponder 5/3 - RaceManager 18/18; RaceTimingServices 3/2; PitStop 8/0; PitCrew 3/0; LollipopCrew 2/0; FlagManager 6/0; SteamInterface 11/1; SharedMemoryWriter 8/0; SessionResultProcessor 1/0; TimeAttack 1/0; DragMode 1/0; LapInvalidatorAvatar 2/0; JsonOutputFile 1/0; MicroSectors 1/0; ksSecurity 1/0; PIDController 0/1; NKUtils 1/0; WrongWayIndicator 1/0; LockControlsTimeAvatar 1/0; ACClientVotingManager 1/0

**CSP-own names (not acs.exe symbols): 54 classes, 1 hook, 170 lookups (171)**: renderer-side `ac_ext::*` (cbuffer layouts of CSP's own shaders; 0/40 + 33 sub-structs), `dx::*`, `efx::*` (8), `pp::*` (6), `final`, `mirror`, `dynamic`, `special` (3), `splashscreen` (5): 42 classes 1/117; physics `hooks::car_flex_data::impl` 0/1; misc `utils::*` (25), `mz::*` (12 = miniz result codes), `data::*` (6: music-reader types), `lua::known_executables` (2), `cui`, `AC`, `Concurrency`, `Speed`, `hooks::chunks_optimizer`: 11 classes 0/52. These are **registered type/struct names** (hash/log/reflection), not game functions. `[interp]`

**Plain-name family (not in the table; 120 names):** `kgl*` (28), `ovr_*` (14), `ac_*` (13 Python API entry points), `ks*` (ksDecodeUtf8, ksRandomize, ksGetNumberOfProcessors, ksGenerateCrash, ks_critical_error, ksSetCurrentThreadAffinityMask, ksCalcCamberRAD, ksTimeFromAngle), `ac*Locale*`, `dGeomSetOffsetPosition`, `dWorldSetGravity`, `onKeyDown/Up/Char/Mouse*`, `WndProc`, `wWinMain`, `string2wstring`, `checkRaceINI`, `clearPythonCache`, `loadDriverBasePos`, D3D globals (14), `g_pPfxContext`, `hLibOVR`, `startTime`, `pyString`. `[dll]`

## 3. Config keys and Lua API

* **Master switch:** `extension/config/general.ini [BASIC] ENABLED=1` "Active (set to 0 to disable everything completely)". `[ini]`
* **Keys that touch attach/safety** (`general.ini`): `[CONFIGURATIONS] CHECK_FOR_RAPID_CRASHES=1`, `CRASH_ON_F12=1`
  (hotkey Ctrl+Shift+Alt+Win+F10 crashes AC on purpose to get a dump), `CRASH_PLUGIN=0` (separate crash-message process),
  `BLOCK_UNTRUSTED_NETWORKING=0`, hidden `FILE_OP_HOOK=<dll>, <export>` (commented, developer sample), `CLEAR_CSP_CACHE=1`,
  `CLEAR_SHADERS_CACHE2=0`; `[DEV] DETAILED_CRASHES=0`, `SAFE_EXIT=1`, `SILENT_LUA=0`, `DIRECT_MAP_INIT=0` (hidden; bypass the
  PDB search, use the cached map directly), `GRACEFUL_START=3`, `D3D_DEBUG=0`, `PRINT_OUT_ENV=0` (log environment: the "Env.:"
  lines), `DISABLE_STEAM_INTERFACE2=0`, `WATCH_FOR_CHANGES=1`, `PROFILE_LOADING=0`. `[ini]`
* **Mapping file:** `extension/config/data_alt_mapping.ini` (`[OPTIONAL]` 6, `[NAMES_ALT]` 58, `[NAMES_WINE]` 22). `[ini]`
* **Lua API:** none belongs to this area. The only scripting surface the attach layer creates is the 213 Python functions
  `ac.ext_*` (`extension/internal/python/functions.txt`), and the Lua SDK (19 modules) is covered by other briefs.

## 4. Files it reads

* `acs.pdb` (69,218,304 bytes, AC root), `acs.exe` (22,890,776 bytes; header only), `dbghelp.dll` (system folder),
  `nvapi64.dll`, system `dwrite.dll`. `[dll]`
* `<AC>\cache\memory_layout.bin` (symbol cache, 524,921 bytes); other files in `<AC>\cache\`: `clouds.bin` (44,564,640),
  `imgui.bin` (5,159,108), folders `ai_grids`, `ai_payloads`, `cars_data`, `csp_configs`, `lua`, `lua_shaders`,
  `meshes_metadata`, `tracks_data`, `track_logos`, `trees_alignment`, `web`. `[dll]` + listing
* `extension\config\*.ini` (57 files) + `config\cars\{loaded,generic,common}`, `config\tracks\...`, `Documents\Assetto Corsa\cfg\
  extension\*.ini` (13 user ini files + `state\`), `race.ini`, `cfg\video.ini`, `cfg\controls.ini`, `python.ini`.
* Writes: `Documents\Assetto Corsa\logs\custom_shaders_patch.log`, AC's `log.txt` (redirected), crash dumps under
  `CrashDumps` and `crash_logs`, cache files. `[dll][log-csp]`

## 5. How it is switched on

* On by default (`ENABLED=1`); loads on **every** start of a 64-bit `acs.exe` that sits next to `dwrite.dll`. No per-car or
  per-track gate at this level: the per-car extended physics is decided later by the hooks (per-car extension object, see
  1.7 examples). The only user-facing off switches are `general.ini [BASIC] ENABLED=0` (CSP inert) and removing/renaming
  `dwrite.dll` (vanilla). On this PC: enabled; symbol cache present; 5 extensions disabled by their own conditions
  (1.5). `[ini][log-csp]`

## 6. Size

| Item | Number |
|---|---|
| `dwrite.dll` | 139,567,496 bytes; image ~41 MB (`.text` 31 MB, `.rdata` 7 MB, `.data` 2.2 MB); 77,058 functions in `.pdata`; overlay 7 embedded zips |
| Entry/proxy/start-up | 17 infra functions decompiled (`decomp_infra`), ~120 KB of pseudo-C |
| Hook registry | about 1,650 regular records + 142 gate records (kinds 1:6, 2:14, 3:52, 4:52, 5:17, 6:1); 1,716 distinct names |
| Hook table (this brief) | 1,441 sites, 934 hooks, 507 lookups, 1,368 distinct names, 387 classes, 146 libs/objs |
| Hook replacement functions | 869 distinct RVAs decompiled (`decomp_hooks`) |
| Hook library | ~12 KB (`0x1cd0000..0x1cd3c00`), MinHook-style |
| Symbol cache | 524,921 bytes, 6,280 names, 29,142 (RVA, size) pairs |
| Names resolved this run | 1,542 ("All 1542 functions were found successfully") |
| Python API added | 213 `ac.ext_*` functions |
| Threads created by CSP | CSP: threadpool for threadpools, COM async, background threadpool (4), files monitoring, continuous loading, loading threadpool (16), logging; plus AC: main / physics / worker tags |

## 7. Port difficulty for rustyAC

**S for the mechanism, L-XL for the seams.** `[interp]`

* There is **nothing to port of the mechanism**: no PDB reading, no inline detours, no loader tricks. rustyAC owns
  the engine, so "hooking" becomes ordinary extension points in our own code. What is needed is the *contract* CSP
  hooks assume: every one of the ~1,250 acs.exe symbols in section 2 is a call site where CSP either replaces the
  behaviour, wraps it (pre/post) or only reads a variable. The port is therefore "implement each CSP behaviour natively inside
  the corresponding rustyAC module" (cost sits in the area briefs: physics, renderer, UI, Lua), plus a thin **seam
  layer** so that vanilla behaviour stays the default.
* Reusable from the existing port: all of `docs/atlas`/`docs/map` (the hooked classes have Rust twins:
  `Car::step`, `ABS`, `Tyre`, `AIDriver`, `RaceManager`, `INIReader`, `Game`, `Sim`, kgl/GraphicsManager, ksgui widgets,
  audio engine). The per-car "extension object" corresponds to a per-car optional struct on the Rust car.
* Hard parts: (1) ordering/identity: CSP assumes acs.exe call order and re-entrancy (e.g. `Sim::Sim` vtable patch,
  CrashGuard as early-init point); (2) the **function-size overload matching** shows which acs.exe overload each hook
  replaces; a native port must map each of the 92 sized records to the right Rust function; (3) module-qualified
  names (FMOD, Steam, OpenVR, WinHTTP) become our own audio/network back-ends; (4) CSP-own registries (`ac_ext::*`)
  are shader struct layouts that will be implied by the renderer port.
* What could be proven and how: (a) the **seam list** itself: replay the CSP log of a session and compare the names
  resolved with the Rust seam table (counts 1,542 vs ours); (b) per seam, an A/B test with the existing oracles:
  run acs.exe + CSP in a harness (needs the real process for `DllMain`), or acs.exe alone with each CSP-extended content
  item and compare against rustyAC with the same CSP content using the existing `car_oracle`/`render_oracle`/
  `audio_oracle` flows; (c) the symbol cache (`memory_layout.bin`) is a free, exact (RVA, size) map for every name CSP
  uses and can be diffed against `acs_index/functions.tsv` to prove the name -> function mapping used here.

## How the pieces fit

1. `acs.exe` imports `DWrite.dll`; CSP's `dwrite.dll` in the game folder is mapped at process start. Its CRT initialisers
   fill a registry of about 1,650 `{name, detour, slot, size, gate}` records (the "what do I touch" table).
2. `DllMain` (main thread, loader lock) opens the log, reads the master switch, builds settings, finds every named
   function through `memory_layout.bin` / `acs.pdb`, creates the detours and enables them in one thread-freeze pass.
3. The game then runs normally; each hooked function enters CSP code that either calls the original (wrapper), replaces it
   (full re-implementation) or only reads a variable. State shared across hooks is a per-car extension object vector
   plus singleton globals captured by constructor hooks (`ksgui::GUI::GUI`, `Sim::Sim`, ...).
4. The module factory (`0x14dc880`) runs from the `CrashGuard::CrashGuard` hook; modules read `extension/config/*.ini`
   and the user's overrides and switch themselves off with a stated reason.
5. DLLs the game loads late (`python33`, `openvr_api`) are hooked via `__HrLoadAllImportsForDll`.

What a native replacement must expose (a **symbol-less host** with the same seams):
* An event/extension API on: process start (settings + log), `CrashGuard`-time module creation, `Sim` construction, `Game`
  loop (`run`, `update`, `render`, `renderHUD`, `onIdle`), `Car::step` and the 40+ physics sub-steps (ABS, TC, Tyre, Drivetrain,
  Suspension*, Engine, Wing, Turbo, ERS...), per-car extension storage, `CarAvatar` construction/animation, `KN5IO` load, `INIReader`
  parsing and file open (so ext files are visible), `GraphicsManager`/kgl render passes, `ksgui` form tree + input, FMOD
  engine/event, input devices, network client, Python bridge (for `ac.ext_*`), crash handler.
* Critical seams (highest leverage, in order): `Car::step` + `PhysicsEngine::step`/`PhysicsDriveThread::step`;
  `INIReader::*` and `Path::*`/`FolderEncrypter::*` (config + virtual files); `Sim::Sim`, `Game::*`; `CarAvatar::CarAvatar`
  + `KN5IO::*` + `Material::*` (content); `GraphicsManager::*` and `kgl*` (renderer); `ksgui::GUI::*`
  (UI input/render); `AudioEngine::*`/`CarAudioFMOD::*`; `RaceManager::*`.

## 8. Open questions and not mapped

1. Exact record count and per-record kind of the 330 non-`::` registrations (the shipped `csp_hook_table.tsv` only has
   `::` names); hook/lookup split for those is a heuristic (1,161 hooks / 395 lookups over 1,556 names).
2. Why 1,562 candidates become 1,542 (20 names): proof needs the walker's (`0x1099a9e`) and resolver's (`0xeb6020`) decompiles.
3. Byte pattern of the detour (relative 5-byte vs 14-byte absolute) and whether the trampoline relocates RIP-relative
   code: needs `0x1cd2003`, `0x1cd2870`, `0x1cd1ad0` decompiles.
4. How the AC version (1.16.3 vs 1.16.4) is detected: only sizes + `[NAMES_ALT]` and a product-string function (`0xeb5630`)
   were seen; the header of `memory_layout.bin` (`e61f2dfb19dfc443`) is probably the PDB/exe identity.
5. What the obfuscated literal checks in `0x14f7810` / `0x14db8f0` test (not decoded on purpose).
6. Whether CSP can be hosted in a process where `acs.exe` is not the main module (an oracle harness): `GetModuleFileNameW`
   and `GetModuleHandle(NULL)` are used; untested.
7. Which acs.exe functions CSP wraps *only because of Wine*: `NAMES_WINE` shows 22 data symbols but gate predicates were not
   evaluated.
8. `0x13d02c0` classification list, `0x1cd3884`/`0x1cd3974` signature checks: callers are indirect (no direct call found).
9. Pro build (`acs_pro.exe`): string evidence only; no log of such a run.

## Evidence

* `re/scratch/csp1/decomp_infra/{16da528,14fd180,6d9660,6d97f0,14f7810,14f0580,14db8f0,14dc880,14fa3a0,14f9db0,14fafb0,
  155a870,150cb30,6ccc90,11e81a0,12a4500,c73560}.c`; `decomp_hooks/{563dc0,5996f0,6129b0,610810,6107e0,60da50,623760,
  60db50,11bb510,6c8b70,428cd0,5d7490,5da110}.c`.
* Hand-decoded bytes (no decompile available): `0x14f81d0`, `0x16da400`, `0x1099a70`, `0x1099a9e`, `0x22cbd6` (record
  writer), predicates `0x59dde0`, `0x6ba320`, `0x2f4d70`, `0x5d0a70`, `0x60a480`, `0x69fe80`.
* Call-graph scan by `E8`/`E9` targets over `.text` (callers of `0x14f7810`, `0x14fa3a0`, `0x1cd1d70`, `0x6d97f0`,
  `0xeb57c0`, ...). Registration scans: `mov word [rec+0x24], len` (1,642 sites), `mov byte [rec+0x26], k`, `mov dword
  [rec+0x20], size` (92).
* `dll_strings.tsv`, `dll_func_strings.tsv`, `dll_func_imports.tsv`, `dll_funcs.tsv`, `csp_hook_table.tsv`, `acs_index/functions.tsv`.
* Shipped files: `extension/config/general.ini`, `data_alt_mapping.ini`, all `config/*.ini` `FULLNAME` keys,
  `extension/internal/python/{initialize.py,functions.txt}`; `<AC>/cache/memory_layout.bin` (read-only parse);
  `acs.exe` import table (read-only parse); embedded zip comment (RainFX); `custom_shaders_patch.log` first 130 lines.

## Decompile requests

1. `0x1099a9e` per-record walker: confirm gate semantics, error-list push, "missing" flag.
2. `0xeb6020` resolve-by-name: cache vs `SymFromName` order, overload by size, `\module\\name` form.
3. `0xeb66e0` create-hook wrapper: arguments, naming, error text.
4. `0xeb3060` symbol subsystem init: Wine branch, `DIRECT_MAP_INIT`, cache write.
5. `0xeb1ff0` mapping init (`NAMES_WINE`/`NAMES_ALT` loader); `0xeb25f0` "No version of `...` is available".
6. `0x1dbb430` cached-map loader and its file path/validity check.
7. `0xeb4e90` module-qualified lookups (`GetModuleHandleA`/`LoadLibraryA`, "Module `X` has not been found").
8. `0xeb4820` `SymEnumSymbols` cache builder.
9. `0xeb57c0`, `0xeb5710`, `0xeb5630`: Wine/Steam/Pro probes and product string.
10. `0x1cd2003`, `0x1cd2870`, `0x1cd1ad0`, `0x1cd1d70`: confirm MinHook shape, detour bytes.
11. `0x14f87f0` command-line parser (which switches are accepted).
12. `0x14db470`, `0x14e0470`, `0x14dfca0`, `0x14dab50`, `0x14ddd60`, `0x14dc510`, `0x14f0f00`: stage helpers.
13. `0x1387350` and `0x138ded0`: settings load and the "Extrapolate state / Hotlap mode" lines.
14. `0x6d75a0`: deferred closure after `__HrLoadAllImportsForDll`.
15. `0x13d0ac0`: the `wWinMain` replacement (crash reporter).
16. `0x6c5170`: `D3D11CreateDeviceAndSwapChain` detour; `0x6c3f50`.
17. `0x15c9db0`: external custom-physics module loader and checksum.
18. `0x94dcd0`, `0x572e80`, `0x12ff540`: online track/surfaces checksum and join checks.
19. `0x1582b06`, `0x1cd3884`, `0x1cd3974`, `0x15830d0`: signature checks and callers.
20. `0x14db8f0` tail and `0x14f7810` obfuscated blocks: only to name what they *guard* (not to decode).
21. `0x150c630`: log file open ("enabled/disabled" header, file replace).
22. `0x1814dfca0` = `0x14dfca0`: module registration into the settings object.
23. `0x6d09f0`, `0x6d0a20`: fatal-exit helpers.
24. `0x3ee3d0`, `0x13d02c0`: thread freeze with "Custom Shaders Patch code" label and module classification.
25. `0x1099910`: registry constructor (confirm field offsets 0x10..0x20).
