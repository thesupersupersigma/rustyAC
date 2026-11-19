# Running CSP inside an oracle

Can Custom Shaders Patch 0.3.2-preview2 (build 4305, the frozen build of
[`version.md`](version.md)) be loaded next to an `acs.exe` image that an oracle mapped by hand,
without the game ever running as a program? Part 1 is what the DLL needs from its host between
being loaded and its "functions were found" log line, read from the disassembly only. Part 2 is
what happened when it was tried, and the verdict. Part 3 loads a car with CSP live (Task C2).

Addresses: `dwrite.dll RVA 0x...` (add `0x180000000` for a VA). Markers: `[dll]` seen in
`dwrite.dll`, `[pe]` read from the file's headers, `[log-csp]` seen in a CSP log of a normal game
run, `[prior]` from [`attach.md`](attach.md) (build 4184), `[interp]` inference. Some of the
DLL's strings are stored scrambled; they are not decoded here, and a name that is only known in
that form is described by what the code does with it.

## Part 1. What the DLL needs from its host (static)

### 1.1 The file

- 64-bit DLL, preferred base `0x180000000`, image size `0x29c1000`, entry point RVA `0x181b818`.
  It has relocations, an exception directory (`0xe8638` bytes of unwind entries: the code throws
  C++ exceptions) and a TLS directory. `[pe]`
- TLS directory at RVA `0x2396200`: a `0x770`-byte template (RVA `0x23f9e80` to `0x23fa5f0`), the
  index variable at RVA `0x27a68d0`, and two TLS callbacks (RVA `0x181acec`, `0x181ad64`). A host
  that maps the file without the Windows loader's help has to give the image a TLS slot on every
  thread that runs its code, and call the callbacks. `[pe]` `[interp]`
- Static imports: 455 functions from 13 DLLs (KERNEL32 278, USER32 87, ADVAPI32 32, GDI32 24,
  ole32 9, SHELL32 8, IMM32 6, OLEAUT32 4, dxgi 2, POWRPROF 2, d3d11 1, d3d12 1, COMDLG32 1). All
  are system DLLs. `[pe]`
- Delay-loaded imports, 19 DLLs, each loaded only when first called: dbghelp 7, WINHTTP 20,
  WS2_32 37, CRYPT32 21, SETUPAPI 11, MFPlat 7, libxess_dx11 7, HID 6, ffx_backend_dx11_x64 5,
  D3DCOMPILER_47 3, VERSION 3, WINTRUST 3, imagehlp 3, AVRT 2, PROPSYS 2, IPHLPAPI 1, CFGMGR32 1,
  SHLWAPI 1, bcrypt 1. Network (WINHTTP, WS2_32) is in this group, so a host can refuse those two
  DLLs and nothing at load time breaks. `[pe]` `[interp]`
- `acs.exe` has no TLS directory and no exports. It imports one function from `DWrite.dll`,
  which is how the game loads CSP. `[pe]` `[prior]`

### 1.2 From the entry point to "functions were found"

1. Entry point (RVA `0x181b818`) is the compiler's DLL start-up: C++ static initialisers run
   first and fill the registry of wanted functions, then the DLL's own `DllMain`
   (RVA `0x15783a0`) is called. `[dll]` `[prior]`
2. `DllMain` on process attach calls the start routine (RVA `0x1577950`) once. In order it:
   reads the high-resolution clock and the wall clock; stores the calling thread's id as "the
   main thread"; **turns on flush-to-zero and denormals-are-zero in MXCSR on the calling
   thread**; looks for one other module in the process (RVA `0xf12ca0`); reads the command line;
   asks for the DLL's own file name and **makes the DLL's folder the current directory**; builds
   the path table (1.3); tests that one file under a scrambled relative name exists and returns
   silently when it does not; reads the "enabled" setting; opens
   `<user folder>\logs\custom_shaders_patch.log` (RVA `0x158cf60`); calls the orchestrator
   set-up (RVA `0x15706b0`); logs "Starting loading". `[dll]`
3. The orchestrator set-up first installs 15 detours of its own and applies them
   (RVA `0x155ab30`, through `0x3d55a0`), then runs the base settings (RVA `0x155afb0`), then
   constructs about 22 modules. `[dll]`
4. Base settings, in order: read the configuration; a large settings routine
   (RVA `0x13fcd80`) that also switches the address cache on or off and may start one thread
   (1.6); log "DXGI tweaks are prepared"; the install-path check (RVA `0x157b3d0`); the hook
   set-up (RVA `0x157a7c0`); log "Base settings are ready". `[dll]` `[log-csp]`
5. The install-path check refuses any install path with a non-ASCII byte: a message box, then
   exit or an "Invalid installation path" exception. `[dll]`
6. The hook set-up walks the registry once (RVA `0x10fc360`), resolving every name and queuing a
   detour for every entry that has one, then applies all queued detours in one batch
   (RVA `0x1d8eb10`), then logs `All <n> functions were found successfully`. If any name is
   missing it logs "Failed to initialize", shows a message box and base settings throw. `[dll]`

The count `<n>` is a counter incremented by the resolver (RVA `0xf13570`) on every name that
resolved: detoured functions plus plain look-ups, not the number of detours. A normal game run of
this build logs 1557. `[dll]` `[log-csp]`

### 1.3 Folders

The path table is built by RVA `0x1583660` from the folder the DLL was loaded from. `[dll]`

- **Install root = the folder `dwrite.dll` was loaded from**, not the folder of the process's
  exe. Under it: `cache`, `system\cfg\ppfilters`, `content\cars|driver|tracks|fonts`,
  `extension` and its sub-folders, `apps`, `apps\python`.
- **User folder = the Documents folder from `SHGetFolderPathW` (CSIDL 5) plus a fixed 13-character
  folder name**, or plus the value of the environment variable `AC_CFG_PROGRAM_NAME` when that is
  set (CSP then creates `cfg`, `cfg\extension`, `logs`, `out`, `screens`, `replay` and `setups`
  under it). Under it: `cfg`, `cfg\extension`, `cfg\extension\state`, `setups`, `logs`,
  `screens`, `replay`, `out`. The log file is in `logs`.
- Other folders are asked for on demand (RVA `0x1584950`): AppData, LocalAppData and its `Temp`,
  Program Files, the system and Windows directories.

For a host this means: the DLL must really be loaded from the AC root it should use, and the
Documents answer has to be replaced **before** `DllMain`, because the log is opened and folders
are created under it during start-up. `[interp]`

### 1.4 How `acs.exe`'s functions are found

- The symbol set-up (RVA `0xf10450`) runs once: it opens the current process, asks for the
  **file name of the process's main module** (`GetModuleFileNameW(NULL)`), and keeps its stem and
  file name. Symbol names are asked for as `<stem>!<name>`. An identity key is made from the file
  name and the exe file's times (RVA `0x1432c60`: the later of creation and last write, in
  seconds). `[dll]`
- With the cache on, the cache file is `<install root>\cache\memory_layout.bin`
  (RVA `0xf14350`, loader `0xf102f0`). A hit with the same identity key gives every address
  without dbghelp. The file holds offsets from **`GetModuleHandleW(NULL)`** (writer
  RVA `0xf118e0`), so that call must return the base of the `acs.exe` image. `[dll]`
- On a miss, dbghelp is delay-loaded and initialised with **the current directory as the symbol
  search path** and "invade process" on, so dbghelp looks at every module the Windows loader
  knows and finds `acs.pdb` next to the DLL. Names are then looked up one by one
  (`SymFromName`, or `SymEnumSymbols` with a pattern; RVA `0xf10c60`), or all symbols of the
  module are listed once into a table that is saved as the cache (RVA `0xf11c00`). After the
  hook set-up a queued job calls `SymCleanup`. `[dll]`
- Registry names of the form `\module\export` are other DLLs' exports (RVA `0xf12270`):
  `GetModuleHandleA`, and when the module is not in the process CSP loads it itself with
  `LoadLibraryA` and logs "Module ... loaded" or "Failed to load ...", then `GetProcAddress`.
  `[dll]`

For a host: the main-module questions (`GetModuleFileNameW(NULL)`, `GetModuleHandleW(NULL)`,
`GetModuleHandleA(NULL)`) must be answered with the hand-mapped `acs.exe`, and dbghelp must be
able to tie the name `acs` to that image. A hand-mapped image that the Windows loader lists
under another identity is the main risk here; part 2 measures it. `[interp]`

### 1.5 Detours: at start-up or later

- The detour code is shaped like a well-known open-source hook library: create
  (RVA `0x1d8ed30`) saves the first 5 bytes of the target and builds a trampoline near it (or
  saves 7 bytes starting 5 bytes above the target, for a short target with padding above it);
  apply-queued (RVA `0x1d8eb10`) writes the jumps. `[dll]`
- At start-up apply-queued is called in its "no thread freeze" form: other threads are not
  suspended while the jumps are written. The form that suspends every other thread exists and is
  used by other callers. `[dll]`
- **All registry detours go in at start-up**, in the one walk of 1.2 step 6. The registry has
  if / else / endif rows that switch whole groups on a setting, so which detours exist depends
  on the configuration, but nothing in the walk waits for first use. `[dll]`
- Before that walk, 15 detours of the orchestrator are already in place (1.2 step 3). `[dll]`
- Later installs exist: the registry installer (RVA `0xf13c30`) has one more caller
  (RVA `0x12226d0`), and create has two more (RVA `0x157d3b0`, `0x16620b0`). When these run was
  not read; part 2 compares the patched bytes at "functions were found" and after the call
  through. `[dll]` `[interp]`

### 1.6 Threads, network, devices, the registry

- Before the hook set-up there is one conditional `CreateThread` (in RVA `0x13fcd80`, skipped by
  a setting). No other routine on the start-up path calls a thread or network import directly;
  the DLL has 26 `CreateThread` call sites in all. A normal game run logs its named threads
  ("threadpool for threadpools", "COM async", "background threadpool", "files monitoring") after
  "functions were found". `[dll]` `[log-csp]`
- The routine that suspends the other threads "for Custom Shaders Patch code"
  (RVA `0x3da710`) is only stored as a callback; start-up does not call it. `[dll]`
- Network: WinHTTP is opened in one place (RVA `0x1613f40`), not on the start-up path. `[dll]`
- Direct3D device creation has three callers (RVA `0x6b8070`, `0x6b9370`, `0xbc94b0`) and window
  creation five; none is called directly by the routines of 1.2. `[dll]`
- The Windows registry is read by nine routines; none is a direct callee of the routines of 1.2.
  `[dll]`
- `IsDebuggerPresent` is used only by the compiler's run-time code. `[dll]`

"Not called directly" is as far as reading goes: the settings routine and the module
constructors are large. Part 2's host stops on any import it was not told about, which turns the
rest of this list into a measurement. `[interp]`

### 1.7 What a host has to answer

| Dependency | Why | Host's answer |
|---|---|---|
| Main module file name | symbol prefix, identity key | path of the `acs.exe` in the oracle's AC root |
| Main module handle | cache offsets, exports of the exe | base of the hand-mapped `acs.exe` |
| DLL's own file name | install root, current directory | true (the DLL is loaded from the AC root) |
| Documents folder | log, user settings, folders created | the run's own folder |
| Install path | must be ASCII only | an ASCII path |
| `acs.pdb` | names to addresses | next to the DLL; dbghelp allowed to load |
| Clock | start time, log time stamps | a fixed clock |
| TLS slot, TLS callbacks, entry point | hand-mapped image | done by the host, in loader order |
| Unwind tables | C++ exceptions | registered by the host if the loader did not |
| MXCSR | start-up sets FTZ and DAZ on its thread | recorded; restored when Vanilla code is compared |
| WINHTTP, WS2_32 | network | refused |
| Other modules by name (sound, Steam, Direct3D) | registry rows naming their exports | present, or logged by CSP as failed |
| Threads | pools started after the hook set-up | listed, checked against the result |

## Part 2. What happened when it was tried, and the verdict

Extra markers: `[run]` measured in the spike host on 2026-11-17, `[cfg]` the comment next to a key
in CSP's own `extension/config/general.ini`.

**Verdict: GO with conditions.** CSP build 4305 starts next to a hand-mapped `acs.exe`, finds
the same 1557 functions as in the game, patches them, and answers a call through a patched game
function, the same way on every run. The conditions are in 2.8.

### 2.1 The host

`tools/csp_oracle/` (`csp_oracle <ac root> <run folder> [cfg folder]`), run against
`re/csp/fakeac/`. It reuses car_oracle's loader as a shared module (`#[path]`, no copied code).

- `acs.exe` is mapped by that loader, as in every oracle. It never runs as a program.
- **The DLL is loaded by Windows, not mapped by hand.** This is the one departure from the plan
  of part 1 (1.7 rows "TLS slot…" and "Unwind tables"). With the Windows loader the TLS slot
  exists on every thread, the unwind tables are registered, and the DLL is in the module list
  that dbghelp walks. The host still answers on purpose: a loader notification patches the DLL's
  entry point before it runs, and the hook points every one of the 455 static imports at a
  counting thunk, with the host's own function where an answer is needed. The price: the DLL's
  two TLS callbacks run before the hook (the start-up then went through with no fault `[run]`),
  and delay-loaded imports (dbghelp) are not counted.
- The run folder is new for each run and becomes Documents, the temp folder and the local /
  roaming application-data folders. `[cfg folder]` is copied in as the user's `cfg`.
- Writes are refused outside the run folder and `<ac root>\cache` (most folders of
  `re/csp/fakeac/` are links into the real game folder). Network DLLs do not load. A Direct3D
  device, a DXGI factory, a message box or a process exit stops the run with its own exit code.
- 1.6 said the host "stops on any import it was not told about". It does not: it **counts**
  every import and stops only on the ones just named. The counts are the measurement.

### 2.2 What start-up needed

| Dependency | Host's answer | Measured `[run]` |
|---|---|---|
| Main module file name / handle | the fake root's `acs.exe` / the mapped image | asked 4 + 7 times; accepted |
| DLL's own folder | true | current directory set to it once |
| Documents | the run folder (`SHGetFolderPathW`, folder 5) | asked once; `SHGetKnownFolderPath` never |
| `acs.pdb` | next to the exe; dbghelp loads | all names found; the layout cache was **not** written (`cache\` stayed empty) |
| The game's own DLLs | `fmod64`, `fmodstudio64`, `steam_api64` (copied into the fake root), `wsock32`, `d3dx11_43`, `xinput1_3` (Windows) loaded before the DLL | **needed**: the function table also names functions these DLLs export; without them CSP shows "Failed to tweak Assetto Corsa" with 109+ names |
| `race.ini`, `video.ini` in the user's `cfg` | small ones written by the host | **needed**: the race-configuration reader (RVA `0x124f650`) refuses to go on without them |
| Clock | fixed, per thread | 14 + 2 + 2 + 1 + 1 queries; log dated 2020.01.01 |
| MXCSR | recorded | `0x1f80` before the entry point, `0x9fe0` after |
| Network | DLLs refused | never asked for |
| Registry, Direct3D, DXGI, message box, new process | stop or refuse | never asked for |
| Window | message-only window refused | asked once, by the hot-key thread (2.5) |
| Faults | car_oracle's crash reporter left on | 7 C++ exceptions thrown and caught inside CSP; no fault |

CSP called 80 of its 455 static imports. Its only writes were the folder
`cfg\extension\state` and its log. `[run]`

Two things cost a run each and are worth knowing:

- Until "functions were found", CSP reports its own errors through a game function it has not
  looked up yet. An early error therefore shows as a jump to address 0, not as a message. `[run]`
- **The function count depends on the settings.** No user settings: 1558. The player's CSP
  settings only: 1556. The player's game settings only: 1559. Both: 1557, the game's number.
  A reference run is only comparable with the same `cfg` folder. `[run]`

The task's note that the fake root has no fmod / Steam / MSVC DLLs "so that should not matter":
the first two do matter (table above); `tools/csp/fakeac_csp.ps1` now copies them. The MSVC
run-time DLLs come from Windows, as for car_oracle.

### 2.3 Go criteria

| Criterion | Result | Evidence `[run]` |
|---|---|---|
| Log reaches "functions were found" with the game's count | yes | "All 1557 functions were found successfully", then "Base settings are ready", "Orchestrator is ready", "Starting loading…", as in the game's log |
| Detours really in `acs.exe` | yes | 1055 functions patched. `kglInitFonts` `48 83 ec 38 48` → `e9 84 89 f5 ff`; `INIReader::hasSection` `48 89 5c 24 10` → `e9 de 95 d8 ff`; `GameTime::update` `40 53 48 83` → `e9 ff 48 b7` |
| One call through | yes | `INIReader::hasSection("THREADING")` on `system/cfg/assetto_corsa.ini`: `true` before CSP, `false` after (the file has the section). `PHYSICS_THREADING`: `true` both times |
| Repeatable | yes | three runs: report files identical; standard output identical but for the DLL's load address (Windows moved it once); CSP's log identical but for line 1 |
| Threads | yes | one thread started, it ends at once and touches nothing the call reads (2.5) |

Notes:

- The call through is CSP's `hasSection` hook (RVA `0x5dfd20`): it hides the section
  `THREADING` of exactly that file. The two calls suggested in the task (`VERSION=extended-N`,
  the wing controller rule) only act while the game is inside one particular call (a flag set
  and cleared by RVA `0x61e6f0`), so a bare reader cannot show them. `[dll]` (Corrected in
  3.1: 0x61e6f0 arms only the wing rule; Part 3 shows both through car loading.)
- Nothing had to be zeroed for repeatability. What differs between runs: line 1 of CSP's log (a
  random symbol and the main thread's ID) and the DLL's load address. The jump bytes do not
  depend on that address (the trampolines are placed near `acs.exe`). `[run]`
- The host's log differs from the game's in two lines: an extra "CSP version changed from 0 to
  4305" (the run folder is new) and the DPI line (in the game it was already set before CSP).
  The game's log goes on ("Race configuration is checked", "Cores: …", the pool threads) with
  things that happen once the game itself runs. `[run]` `[log-csp]`

### 2.4 Part 1's open points

- 1.4, the identity of a hand-mapped image: no problem. CSP took the mapped image as the main
  module and found every name. `[run]`
- 1.5, later detour installs: none in the spike. The patched functions are the same at
  "functions were found", after the call through and 1.5 s later. The later installers of 1.5
  need the game to run. `[run]` `[interp]`
- 1.6, the conditional thread: it is the hot-key thread of 2.5. `[run]` `[dll]`

### 2.5 Threads

- **T1, the only thread CSP starts** (routine RVA `0x13fc9b0`, from the settings routine):
  it registers a window class, makes a message-only window, registers F10 and F12 as
  system-wide hot keys with every modifier, and pumps messages until shutdown. The host refuses
  the window (system-wide hot keys on the owner's PC are not acceptable), and the thread then
  returns at once. It reads and writes nothing the call through uses. `[dll]` `[run]`
- At all three check points the process has 4 threads: the host's own and three workers of the
  Windows loader. No CSP pool exists yet. `[run]`
- In tool-run mode (2.6) T1 is not started at all. `[run]`

### 2.6 How the physics uses threads (static)

Read in build 4305; nothing here was run.

**The worker count** (RVA `0x15d8110`, once): 0 when the single-thread flag is set or the PC
has 2 logical processors or fewer; otherwise performance cores minus 1 (minus 2 with
`[OPTIMIZATIONS_CPU] HYPERTHREADING_FIX=1` on a PC with hyper-threading), at least 2. The log
line "Cores: 16, physical: 8, perf.: 8, workers: 7" is this routine. `[dll]` `[log-csp]`

**The pool** (dispatcher RVA `0x5864f0`): threads named "LLTP (n)" are made the first time they
are needed, never more than the worker count. A dispatch has a number of work items; the
calling thread and the woken workers each take the next item until none is left, so which
thread runs which item is not fixed. The caller then waits, first spinning with `Sleep(0)`,
then with timed waits. With no work items for the pool, everything runs on the calling thread.
`[dll]`

**Cars** (RVA `0x67e2e0`): with more than one car and a worker count above 0, one work item
per car goes to the pool. Otherwise the cars are stepped one after another in list order. A
one-car session is serial here. `[dll]`

**The rigid-body step** (RVA `0x92e4a0`) has two paths: `[dll]`

- Parallel, when `[OTHER_EXPERIMENTS] MT_ODE_STEP` is on (default 1, "Parallel joints solver"
  `[cfg]`) and the worker count is above 0: CSP builds the island list itself
  (RVA `0x168d1c0`), then gives the islands to the pool. **The number of threads it asks for
  depends on measured time**: at most the worker count and the island count, and further
  limited by a running average of how long earlier steps took on the wall clock.
- Otherwise the island processing of the ODE library itself (RVA `0x16c6d00`), through ODE's
  threading interface.

**Contacts**: `[PHYSICS_EXPERIMENTS_DONE] PARALLELIZED_CONTACTS` (default 1) lets the contact
search run while the cars are being computed `[cfg]`. Its code was not read.

**Forcing one thread**:

- The environment variable `AC_TOOL_RUN` (`true` or a non-zero number) turns on "tool run":
  headless, single thread, worker count 0. Tried in the host: CSP logs "Starting tool run",
  finds 1604 functions (1084 patched) and does not start T1. So it works, but it is **a
  different configuration from the game's** (47 more functions looked up, other switches set
  with it). `[dll]` `[run]`
- `[HEADLESS_MODE] ENABLED` and `SINGLE_THREAD` are read by the same routine
  (RVA `0x13fcd80`); the condition that leads there was not read. `[dll]`
- `MT_ODE_STEP=0` alone removes the timing-dependent path but leaves the cars on the pool.
- A host can also report 2 logical processors, which gives worker count 0 with no other
  setting changed. Not tried. `[interp]`

**Compared with Vanilla.** Vanilla's pool gives the same result whatever the timing, and
rustyAC matches it bit for bit. In CSP the pool's schedule is open: which worker takes which
car or island, and how many threads an island step gets, depend on timing. Whether the
**numbers** then differ was not shown either way: islands are independent by construction and
a car's step mostly writes its own state `[interp]`, but the parallel solver's tasks
(RVA `0x938c50`) and the parallel contact search were not read. With a worker count of 0 every
dispatch runs in list order on one thread; that is the only mode the code shows to be
repeatable.

### 2.7 What cannot be bit-exact

From the code, not a guess:

1. **CSP against Vanilla.** CSP replaces the rigid-body core with a double-precision one;
   `acs.exe`'s is single precision `[prior]`. A CSP run is compared with a CSP oracle, never
   with the Vanilla one.
2. **A multi-thread CSP run against another one**, until measured: the thread count of the
   island step follows wall-clock time, and work items are taken in no fixed order (2.6). The
   worker count also depends on the PC's processor.
3. **Timing statistics** kept by the step routines (smoothed wall-clock durations). They feed
   item 2 and nothing else that was read.
4. **Line 1 of the log**, and load addresses.

Everything on the start-up path, and the one call through, is bit-exact between runs.

### 2.8 Conditions, and the estimate

Conditions of the GO:

1. The DLL is loaded by Windows with its entry point caught, not mapped by hand (2.1).
2. The `cfg` folder is an input of every run; a reference run uses the same one (2.2).
3. The game's own DLLs are in the AC root (2.2).
4. The hot-key window stays refused, and the write guard stays on (2.1, 2.5).
5. Physics runs with a worker count of 0 until a measurement shows that 7 workers give the same
   numbers. If they do not, the multi-thread physics is the "statistical" part and nothing
   else is.

What the spike does not cover: everything after "Starting loading…". In the game, CSP's later
set-up runs from inside patched game functions as the game starts (the lines after it in the
game's log). An oracle that builds a car or steps the physics has to drive those game
functions itself, in order, with CSP's patches in them. That is the unknown in both estimates.

- **C3** (`extended-N` reading): 2 to 3 sessions. The reader rules only act inside the game's
  car loading, so the oracle has to load a car through the patched game code, which car_oracle
  already does without CSP.
- **C4** (the physics baseline): 5 to 8 sessions. CSP replaces the physics engine's set-up and
  step, uses thread-local state, and needs the worker count settled (condition 5) and the
  1-worker against 7-worker measurement.

## Part 3. A car with CSP live (Task C2)

`tools/csp_oracle` now builds one car through the patched game code: physics engine, flat track,
`Car::Car` and the collider mesh, with CSP's detours in them, and compares that with the same
run without CSP (`--vanilla`). Run: `csp_oracle.exe re/csp/fakeac <run folder> <cfg folder>
--car <name> [--car-data <folder>] [--vanilla]` from the repo root; the car's data is copied
into `<run>/car_root/content/cars/<car>/data` (default source `cardata/<car>`), which is the
working directory while the car loads. Scratch (archived on the server: `taskC2.tar`, see docs/disk.md): `re/scratch/taskC2/` (`final.sh` = the runs
below, `py/rematch.py` + `py/annotate.py` = Part C).

### Resume here

Task C2 done: Part A (car stage, three call-throughs, repeatable), Part B static reading only
(stepping is the next step, 3.6), Part C (106 of 116 addresses re-matched, 3.7). Next: the lead
chat's choice between C3 and the B measurement (3.8).

### 3.1 What the game runs between "Starting loading…" and a ready car

In the game, CSP's car set-up happens inside the game functions it patches, in this order
(`[dll]`, build 4305 RVAs; acs.exe VAs from the pdb):

| # | Game function | CSP detour (RVA) | What CSP does there | Oracle |
|---|---|---|---|---|
| 1 | `CrashGuard::CrashGuard` 0x140218800 | 0x6bcc30 | crash dumps; logs "Race configuration is checked", "Core modules are created" | driven (C1) |
| 2 | `RaceManager::RaceManager` | 0x1151720 | stores the race manager in global 0x281a270 (CSP finds the Sim through it: +8 → +0x168) | stand-in objects written to 0x281a270 |
| 3 | `Sim::Sim` 0x140192070 | several | the whole race set-up, cameras, apps | skipped: only the pointers CSP reads are given |
| 4 | `PhysicsEngine::PhysicsEngine` 0x140262430 | registrar 0x23c900 | engine-side set-up | driven |
| 5 | track loading (`Track::Track` 0x140277100, `initAISpline`) | (surfaces, Embree) | `surfaces.ini` extension, raycaster | flat track as in car_oracle; KN5 / surfaces / Embree skipped |
| 6 | `CarAvatar::CarAvatar` 0x1400cd1a0 / 0x1400cddc0 | 0x10e040 / 0x10e480 | **pushes the "current car" context** (0xf32f90 → global 0x2817d90) | stand-in avatar (0x12a8 bytes, unix name at +0x138) + call 0xf32f90 |
| 7 | `CarAvatar::initCommon` 0x1400d56e0 | 0x10e630 | sets byte 0x2815087 around the original: **arms the `extended-N` reading** | driven as one `getInt` call with the byte set (3.3) |
| 8 | loading flag | 0xb687d0 | byte 0x26bad10 = 1 while loading ("Loading is finished in %.3f s" when cleared) | set around `Car::Car` and the collider mesh |
| 9 | `Car::Car` 0x14026bf00 | 0x587180 → 0x5871b0 | with an extension (context +0xe0, or list 0x28213f0[Car+0x3e9c] after loading): `suspensions.ini [_EXTENSION]`, custom suspension types (COSMIC) | driven |
| 10 | inside 9: `Tyre::init` 0x140280650 | 0x69e2f0 | compound value into Tyre+0x40c, random wheel start angle (3.4) | runs |
| 11 | `Car::initColliderMesh` 0x140273b20 | 0x58e720 | `colliders.ini [_EXTENSION]` through the context's config (no null check: faults at dwrite+0xf32f0c without step 6) | driven; the mesh's CSP node index (+0xdc) set to -1 |
| 12 | `CarAvatar::initPhysics` 0x1400d7660, visuals, sound | many | graphics and sound of the car | skipped (not physics) |
| 13 | `PhysicsAvatar::startPhysics` 0x1401223a0, "New thread AC: physics" | 0x676560 | physics thread, thread-local state | skipped: no stepping yet (3.6) |

The stand-in avatar gives one harmless CSP log line, "type must be string, but is null" (a json
field of the real avatar), and the -1 node index one "Accessing extended data of disposed node"
warning. Neither changes the car.

Correction to 2.3: RVA 0x61e6f0 (`NetCarStateProvider::initWings` wrapper) arms only the wing
rule. `extended-N` is armed by the `CarAvatar::initCommon` wrapper 0x10e630 (row 7).

### 3.2 The cars

| Car | `VERSION` | Vanilla | CSP |
|---|---|---|---|
| `ks_ferrari_f2004` (cardata) | 1 | built | built, no extension |
| `vrc_formula_lithium_2023_csp` (unpacked from its `data.acd` into the scratch folder) | extended-2 | refused (3.3) | built, extension made |
| `vrc_formula_alpha_2026_csp` (cardata, suspensions `TYPE=COSMIC`) | extended-2 | refused (3.3) | built, extension made, COSMIC accepted |

### 3.3 Call-throughs

Each line of the oracle names the chain from the acs.exe function to where it ends (jumps
followed, `-> outside every module ->` is CSP's trampoline).

| Seam | Vanilla | CSP | Evidence |
|---|---|---|---|
| `car.ini [HEADER] VERSION=extended-2` | `INIReader::getInt` (acs.exe+0x235c70) uses `std::stoi`: "invalid stoi argument" exception, the run aborts (exit 127; in the game it leaves `CarAvatar::initCommon`) | `getInt` → dwrite+0x5e3590 → **2**, and the CSP car physics extension is made (0x5e32e0, context +0xe0) | `final/lith_*.log`, `final/alpha_*.log` |
| same, `VERSION=1` (F2004) | 1 | 1, no extension | `final/f2004_*.log` |
| wing rule: `hasSection(DYNAMIC_CONTROLLER_0)` whose `WING=3` has no `[WING_3]`; `DYNAMIC_CONTROLLER_1` with `WING=0` present | true / true | `hasSection` → dwrite+0x5dfd20 → **false** / true (armed by byte 0x27f9201, set by the `initWings` wrapper 0x61e6f0) | `wing rule:` line of every log |
| custom suspension type (`Car::Car`, `physics_car_core.md` seam) | "ERROR: Cannot create suspension type: COSMIC for wheel:0", `exit(1)` (run made before the `VERSION` read was added) | `Car::Car` → dwrite+0x587180: car built, collider mesh set | `re/scratch/taskC2/a_van.log` l.52-53, `final/alpha_csp.log` |

### 3.4 Repeatability

`final.sh` runs every car twice and compares stdout (run name removed) and the car's memory
(2004 words of `Car`, words that are addresses written as "ptr"): **all same**, Vanilla and CSP.
Two things had to be fixed (rule 18):

1. **CSP's random numbers.** CSP's `Tyre::init` detour gives every wheel a random start angle
   (sets Tyre+0x2fc angular velocity to `rand*100+1`, steps `Tyre::stepRotationMatrix` once,
   restores: Tyre+0x5b8 `localWheelRotation` differs). The random source is a per-thread
   xoshiro256 (0x13faf90 / 0x13fae20) seeded from `std::random_device` (`RtlGenRandom`, which
   CSP's static CRT looks up by the name `SystemFunction036`) XOR the performance counter. The
   oracle answers that `GetProcAddress` with a fixed per-thread byte stream; the counter is
   already the oracle's clock.
2. **Car+0x2b0** (`colliderManager.observer.lastFileTime`) is the game's uninitialised stack,
   in Vanilla too: `FileChangeObserver::observe` checks `CreateFileW` against 0, not -1, so
   with no file `GetFileTime` fails and its output local is copied. Zeroed after `Car::Car`.

What still differs run to run and is not a result: thread IDs in CSP's log, and call counts in
`imports_*.txt` (e.g. `HeapAlloc` ±14, `VirtualQuery` ±3, `ReadDirectoryChangesW` ±1), from
CSP's background threads (file monitoring, thread pool).

CSP's own writes in the car (F2004, against Vanilla): Car+0x278 = 1, the high dword of Car+0x2d8
= -1, per tyre Tyre+0x40c = compound value (padding in acs's layout) and Tyre+0x64e (int16
compound index), plus the wheel angle. CSP keeps per-car data in acs's padding, so a car
compare has to name those words.

### 3.5 Part B: the multi-thread physics (static)

Stepping needs what car_oracle builds around a car (controls device with its RTTI, controls
provider at Car+0x3d80, aids; `game.rs` 1737-1760): without it Vanilla's `Car::step` faults at
+0x1df. That is more than Part A (rule 32), so B.2/B.3 were not run; the reading:

- **World step** 0x92e4a0 (behind `PhysicsEngine::step` → 0x67ede0). With `MT_ODE_STEP` (byte
  0x2803222) and workers (dword 0x267b338) ≠ 0 it uses ODE's island builder 0x168d1c0
  (`ode-fresh/ode/src/ode.cpp`): a list of islands, 20 bytes each (body start, joint start, body
  count, joint count, quickstep flag). The **thread count** is min(workers, islands, the measured
  solve time of earlier steps / a fixed unit): it depends on the clock, so it changes run to run.
- **Island task** 0x938c50 → 0x92ebd0: each task takes the next island from an atomic counter
  and solves it in its own worker's arena (reset per island). An island is solved with
  `dInternalStepIsland` 0x16c24b0 (the `dWorldStep` solver, the one AC uses:
  `PhysicsCore::step` calls `dWorldStep`), or with quickstep 0x16bd3f0 when every body of the
  island has body flag 0x200 (not a stock ODE flag; who sets it was not found).
- **Shared state found: one.** Quickstep reorders constraints at iterations 8, 16, …
  (0x16c0900) with ODE's process-wide seed (dword 0x284dc20, updated by compare-exchange):
  islands on different threads interleave their draws, so the order each island gets depends on
  timing. Only quickstep islands use it. The `dWorldStep` path shares nothing between islands
  (no common bodies or joints; scratch memory per worker); the only accumulators are timing sums
  (feeding the thread count).
- **Cars step** 0x67e2e0 (pool 0x5864f0 with per-car tasks 0x67eb80): cars step in parallel
  when there are 2+ cars and workers ≠ 0. With `PARALLELIZED_CONTACTS` (byte 0x27f8df5) and 10+
  cars, the pool's "before" task 0x67e6d0 runs the collision step on the calling thread at the
  same time as the car tasks. Whether car tasks read other cars (slipstream, AI) during the step
  was not read.
- **The third pool user** 0xbb2d40 is texture work (`missing_mips\{:x}.bin`), not physics.

Verdict from reading only: **identical with a fixed rule** for car islands solved by
`dWorldStep` (any worker count, if per-car tasks do not read each other mid-step), **order
dependent** wherever a quickstep island exists (flag 0x200) with more than one thread. To be
measured (B.2): 3 cars in contact at workers 0 / 1 / 7, 10 runs at 7, every body every step.

### 3.6 What stepping needs

The controls device and aids from car_oracle, then `PhysicsEngine::step` with CSP live (it goes
to 0x67ede0), the physics-thread set-up CSP does in `PhysicsDriveThread::run` (0x67d840, one-time
init 0x67d6a0, thread-local air density) done on the oracle's thread, and the state read through
`getPhysicsState` (→ 0x590220). The worker count dword 0x267b338 is set by 0x15d8110 from the CPU
count the host reports (2.6).

### 3.7 Part C: addresses re-matched

The rows C3 and C4 need (extended-N parsing, feature flags, `surfaces.ini` / `WAV_PITCH`,
air density, ABS, brake temperatures, surfaces, transponder, AI sleep, `getPhysicsState`) now
read `0x<4184> → 0x<4305>` in `physics.md`, `physics_car_core.md`, `physics_config_and_api.md`,
`physics_powertrain_aero.md`, `misc_core.md`, `misc_ai_race.md`, `rendering_content.md` and
`plan.md`. How: the 4184 instructions with their rip-relative and branch fields masked, grown
until they occur once in 4305 (54); when the code changed, the function between the two nearest
uniquely matched neighbours in `.pdata` order (8); data through the instructions that reference
it (44). Checked against IDA where known (`0x918c80 → 0x90d9f0`, `0x32c650 → 0x31b710`,
`0x5f0650 → 0x5e3590`, `0x59e920 → 0x590220`, `0x26ae290 → 0x28213f0`, the transponder byte
`0x268b680 → 0x27f96d8` read by the settings reader 0x1402bd0). Table:
`re/scratch/taskC2/rematch.tsv`.

Not found (10, all data): `0x228bac8`, `0x268aaa0`, `0x268aad0`, `0x268b411`, `0x268b688`,
`0x269508b`, `0x269514c`, `0x26951ca`, `0x26a6de8`, `0x26a74c0` (mostly hook slots of hooks that
are "never called", referenced only by their registrar).

### 3.8 Estimates now

- **C3**: 2 sessions (was 2-3). The car side is solved: the oracle reads `VERSION` through CSP's
  hook and builds extended cars. Left: the same call-through for `surfaces.ini WAV_PITCH`
  (`getFloat` → 0x5e4180, track side), feature flags, the port and its tables.
- **B measurement**: 1.5-2 sessions (controls device + 3 cars in contact under CSP).
- **C4**: 5-7 sessions (was 5-8), the B measurement included as its first part.
