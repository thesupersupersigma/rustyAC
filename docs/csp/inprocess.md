# Running CSP inside an oracle

Can Custom Shaders Patch 0.3.2-preview2 (build 4305, the frozen build of
[`version.md`](version.md)) be loaded next to an `acs.exe` image that an oracle mapped by hand,
without the game ever running as a program? Part 1 is what the DLL needs from its host between
being loaded and its "functions were found" log line, read from the disassembly only. Part 2 is
what happened when it was tried, and the verdict.

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
