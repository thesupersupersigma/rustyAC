# Misc B: config and content-loading seam, sim, cameras, shared memory, Steam, Python bridge and the rest

CSP 0.3.0-preview649 (build 4184). Brief for `docs/csp/misc.md` (second half). This is "everything else CSP touches" once
physics, rendering, UI, audio, input, online and VR are taken out: the 337 rows of group `misc_core` in `hook_groups.tsv`
(151 rows that name an acs.exe symbol, 186 rows that name no acs.exe symbol), plus the plain-name families `ks*` / `ac_*`,
plus the documented side (small ini modules, camera Lua scripts, the 213 `ac.ext_*` Python functions, cache folder,
network facts). The centre of gravity is the **config-and-content-loading seam**: `INIReader`, `INIReaderDocuments`, `Path`,
`FolderEncrypter`, `FileChangeObserver`, with `Sim` and `Game` as the main-loop seam. Nothing was decrypted; no key material
was looked at (section 2.3).

Markers: `[ini]` shipped ini comment, `[sdk]` Lua SDK, `[lua]` shipped Lua, `[dll]` seen in dwrite.dll (RVA, image base
0x180000000; a `FUN_18xxxxxxx` is base + RVA), `[dll-bytes]` x86-64 bytes decoded by hand, `[log-csp]` owner's CSP log,
`[prior]` earlier briefs (`attach.md`, `physics_documented.md`, `physics_car_hooks.md`, `lua.md`, `ui_apps.md`,
`rendering_content.md`, `online.md`), `[interp]` inference. acs.exe addresses are `0x14....`; "CSP RVA" is the replacement
function (`hook_fn_rva`); `*` after a name = lookup only (no replacement).

---

## 1. What it does

1. **It sits between acs.exe and every ini file it reads.** All 12 `INIReader` / `INIReaderDocuments` hooks are real
   replacements (about 55.7 KB of CSP code in `0x5ec030..0x5f99ae`). They (a) accept `extended-N` / `extendedV2-N` where AC
   expects a number (the extended-physics switch, already covered in `physics_documented.md` 2.2), (b) force a handful of
   values AC would have read from its files (VR camera mode, tyre-smoke gate, FFB multiplier, steer assist, replay size,
   Python switch, mirror quality), (c) answer missing / empty values from CSP's own settings (`[REMOTE] NAME`,
   `[CAR_n] DRIVER_NAME`, `[INFO] SHORT_NAME` / `SCREEN_NAME`), (d) let an environment variable replace the Documents folder
   name (`AC_CFG_PROGRAM_NAME`) or the race.ini name (`AC_CFG_RACE_INI`), (e) run a **registered-handler chain** after every
   `getString` so other CSP modules can override any (file, section, key), (f) make malformed vectors / numbers non-fatal
   with a log line instead of an exception, and (g) feed the loading screen while AC is busy parsing.
2. **It makes AC's file helpers CSP-aware.** `Path::fileExists`, `getFiles`, `getFolders`, `getLastModificationTime`,
   `getDocumentPath`, `getPlatformSpecificPath`, `doesFilenameContainsIllegalChar` and the two `FolderEncrypter` entry
   points answer from CSP's own tables for some paths (a virtual car `content/cars/nul/`, the extension fonts folder, a
   per-folder table of already-unpacked car data). `Path::getFolders` is also the moment CSP injects
   `python/initialize.py` into AC's Python interpreter.
3. **It replaces the top of the main loop.** `Game::update`, `Game::render`, `Game::renderHUD`, `Game::renderAudio`,
   `Game::run`, `Game::~Game`, `GameTime::update` (now a trap) and 20 `Sim` functions are replaced or wrapped. The
   mechanism is an **event bus**: global vectors of 40-byte listener records, iterated last-to-first, some
   cancelable. CSP's own modules subscribe; acs.exe's behaviour is the fall-through.
4. **It adds, around the cameras, a script seam.** The ACCameraManager family keeps AC's camera classes but wraps
   their `update` calls with cancelable event lists and head-turn / shake corrections. The Lua chaser-camera and
   cockpit-camera ("NeckFX") scripts and the C++ freer-camera module sit on that seam.
5. **It leaves AC's shared memory almost untouched.** `SharedMemoryWriter` gets one tweak (G-forces fade out below
   6 km/h, `[SHARED_MEMORY] REDUCE_GFORCES_WHEN_SLOW`) and an after-update event; in headless tool runs the writer is
   switched off. No new page or field was found.
6. **It turns most of `SteamInterface` into no-ops in a gated branch and filters achievements in the normal one**
   (section 2.9). CSP itself uses the Steam interfaces (user, friends, utils, apps, screenshots, networking sockets)
   through its own Lua `steam.*` layer.
7. **It extends the Python bridge** with 213 `ac.ext_*` functions, an error-handling layer around every Python callback
   and traceback printing (section 2.10).
8. **It accelerates and caches AI splines** (grid cache in `<AC>/cache/ai_grids`, `Spline::wrapIndex`,
   `computeSplineLength`, `getLastIndexFromNorm` memoisation).
9. **It registers 175 type names of its own** (cbuffer layouts, `utils::*`, `mz::*` ...) in scattered static
   initialisers; they are not hooks (section 2.13).

---

## 2. Game parts it replaces or extends

### 2.0 Counts for the group (from `hook_groups.tsv`, group `misc_core`)

| Item | Number |
|---|---|
| rows | **337** = 134 hooks + 203 lookups |
| rows with an acs.exe symbol | **151** = 131 hooks + 20 lookups, in 34 classes |
| rows without an acs.exe function | **186** = 3 hooks + 183 lookups (175 type-name registrations in 18 namespaces + 11 acs.exe *data* symbols / destructors the index does not hold) |
| distinct replacement functions for the 131 hooks | 119 (13 rows use the 1-byte no-op `0x2db1f0`; `0x2f4d70` is the 3-byte "return true") |
| code size of those replacement functions | about 45 KB (sum of `dll_funcs.tsv` sizes) |
| libraries (rows hook/lookup) | ksCommon.lib 26 (25/1), sim.obj 24 (21/3), ksGame.lib 14 (12/2), kml.lib 18 (13/5), SteamInterface.obj 11 (11/0), SharedMemoryWriter.obj 8 (8/0), ACCameraManager.obj 9 (6/3, incl. `Texture::Texture`), CameraDrivableManager.obj 7, CameraTrack.obj 4 (3/1), CameraOnBoard.obj 4 (3/1), CameraOnBoardFree.obj 3, PythonInterface.obj 9 (8/1), ksPython.lib 3 (2/1), ksSecurity.lib 1, NKUtils.obj 1, ksACCamera.lib 1, CameraCarManager.obj 2 (1/1), CameraFacing.obj 1, CameraModeStart.obj 1, CarHintLoader.obj 2, CinematicFreeCamera.obj 1, CommandManager.obj 1 (0/1) |

Note: `attach.md` 2.2 lists `Sim 13/10`; the table gives **`Sim` 20 hooks / 3 lookups** (the 10 in `attach.md` is not
reproducible from `csp_hook_table.tsv`). Hooks per class: INIReader 11 (+ INIReaderDocuments 1), Path 8, FolderEncrypter 2,
FileChangeObserver 3, Sim 20, Game 8, GameObject 2, InterpolatingSpline 11 (+3 lookups), Spline 2 (+2), SharedMemoryWriter 8,
SteamInterface 11, PythonInterface 6, PythonForm 2, PyEnvironment 1, PyModule 1, ACCameraManager 6, CameraDrivableManager 7,
CameraOnBoard 3, CameraOnBoardFree 3, CameraTrack 3, CameraCarManager 1, CameraFacing 1, CameraManager 1, CameraModeStart 1,
CinematicFreeCamera 1, CarHintLoader 2, X360Joypad 2, MouseHider 1, NKUtils 1, ksSecurity 1.

### 2.1 Seam table A: INIReader and INIReaderDocuments (the settings seam)

acs.exe side: `docs/atlas/config.md` (INIReader 0x140234xxx..0x14023xxxx; everything reads its own file through it).
rustyAC already ports the reader 1:1 (`crates/rustyac-physics/src/data/ini.rs`, "Not ported: the static file cache and the
error pop-ups").

Common preamble. `getString`, `parse`, `Spline::closestPointIndicesFlat`, `SteamInterface::addAchievement` and 15 others
start with the **loading-screen pump** (section 2.6).

| Function (acs VA) | CSP RVA | What CSP changes | Original called? |
|---|---|---|---|
| `INIReader::getString` 0x1402360f0 | 0x5ef300 (3,696 B) | After the pump, special cases by (file, section, key), all `[dll]`: **(1)** section `LOD_0`, key `FILE`, only while a per-thread flag (TLS +0x1b3) is set: answered by CSP function `0x5eec40` (which knows `LOD_1` / `LOD_2`; effect `[interp]`: a lower LOD is served as LOD_0 for far or optimised cars). **(2)** `[CAMERA] MODE` of a reader whose path is `cfg/video.ini` (predicate `0x5ed680`): CSP returns the text `OCULUS` when its VR byte (settings +0x20) is set, `OPENVR` when settings +0x21 is set. This is how CSP forces VR without editing `video.ini` (answers `docs/atlas/config.md` 7: where `[CAMERA] MODE` comes from). **(3)** `[INFO] SHORT_NAME` / `SCREEN_NAME` (car-side `[INFO]` section): with CSP flag byte `0x26951e0` clear, an empty `SHORT_NAME` is re-read with key `SCREEN_NAME` (a fallback AC does not have); with the flag set, `SCREEN_NAME` is replaced by a text derived from the car's folder name (path parent chain, prettified by helper `0x1813b2dd0` with mode 1) and an empty `SHORT_NAME` gets the same derivation (mode 0). **(4)** `[REMOTE] NAME` of race.ini: if the value is empty, CSP fills it from its own settings table (`settings+0x38` map, sections `REMOTE` / `NAME`); the same table answers `[CAR_n] DRIVER_NAME` when the value in `cfg/race.ini` is empty. **(5)** `[FOLDER] PROGRAM_NAME` (inireaderdocuments.ini): if the environment variable `AC_CFG_PROGRAM_NAME` is set, its value replaces the program name, i.e. the Documents sub-folder (`Documents\Assetto Corsa` by default). **(6)** everything else: original, then the **handler chain**: the global vector at `0x182690c58` (40-byte records, callable at +8, walked last-registered-first, stops when a handler sets the "handled" byte of the context `{reader, section, key, out string}`). | yes (except case 1) |
| `INIReader::getInt` 0x140235c70 | 0x5f0650 (1,990 B) | Inside a car being loaded (`_DAT_1826a9d50` non-null, byte `0x26a756c` set around `CarAvatar::initCommon`): `[HEADER] VERSION` is read as text with the original `getString`; prefix `extended-` (9 chars) or `extendedV2-` (11 chars) flags the car as extended (setter `0x5f0170`) and the number after the prefix is parsed (full story: `physics_documented.md` 2.2). Other rules: `[PYTHON] ENABLE_PYTHON` in a file whose path ends `/gameplay.ini` is answered without the original when settings byte `0x268b1a8` is set (returns 0 `[interp]`; this is the tool-run `ENABLE_PYTHON` switch, `ui_apps.md` 2.0); `[MIRROR] HQ` is answered by CSP while a quality byte (`*(0x1826ad4e8+0x10)`) is < 2; `[HEADER] DESKTOP_SELECTED` (acos.ini) is passed straight to the original (exempt from handlers); `[EFFECTS] SMOKE` of `video.ini` is forced when the `particles_fx.ini [SMOKE] ENABLED` byte `0x269514c` is clear (stops AC's own tyre smoke, `rendering_content.md` 3.1). On the main thread with flag `0x26998bc` set it lazily reads the CSP setting `[VR] ENABLE_CAMERA_SHAKE` once into `0x26c71c0` `[interp]`. Everything else goes through the handler dispatch `0x5f45f0`. | partly (HEADER/VERSION via getString; rest in dispatch) |
| `INIReader::getFloat` 0x1402358c0 | 0x5f1110 (1,219 B) | `[SURFACE_n] WAV_PITCH` in `/data/surfaces.ini` while the track physics is not created: `extended-` prefix flags the track (`physics_documented.md` 2.2). `[REPLAY] MAX_SIZE_MB` is answered by CSP when launch flag `*0xef1700()` or `0x26b8a18` (headless) is set. `[CONTROLS] FFMULT` and `STEER_ASSIST` are replaced by the two global floats `0x268b1ac` / `0x268b1b0` when non-zero (`general.ini [INPUTS_REMAP] OVERRIDE_CAR_FFMULT` / `OVERRIDE_CAR_STEERASSIST`). | yes |
| `INIReader::getVector2/3/4` 0x140236300 / 0x140236540 / 0x140236ad0 | 0x5f1930 / 0x5f1cf0 / 0x5f1eb0 | Zero the outputs first, read the string through the (hooked) `getString`, then split with CSP's own tuple parser (`0x5f4770`, `0x5f4820`, `0x5f48f0`). Its helpers log "Malformed value: ..., a crash has been prevented" and "invalid stof argument" instead of throwing (`0x5f1ba0`, `0x5f1d60`, `0x5f2200`). | no |
| `INIReader::hasSection` 0x1402370b0 | 0x5ed6d0 (726 B) | (a) A section whose name starts `DYNAMIC_CONTROLLER_` is reported present only if the section `WING_<n>` named by its `WING` key also exists (stops a controller pointing to a missing wing; `physics`). (b) section `THREADING` in `system/cfg/assetto_corsa.ini` is special-cased (effect not visible `[interp]`). | yes |
| `INIReader::parse` 0x140237680 | 0x5edd90 (3,518 B) | Pump; then either the original parser or CSP's own pass (`0x5ed9b0`, 909 B, builds comma-joined lists from repeated keys `[interp]`). After a parse of any `content/cars/.../data/lods.ini` CSP rebuilds the `LOD_n` sections: it collects the `FILE` values, fires the event list at `0x182690c40` (listeners may edit the vector) and writes the sections back renumbered. | yes / own |
| `INIReader::errorReport` 0x140234e70 | 0x5eeb50 (44 B) | Drops the report silently when the message contains the 33-char text "KEY_NOT_FOUND: [FUEL] CONSUMPTION" while an extended car is loading (`ctx[0xe0] != 0`): extended cars may omit `[FUEL] CONSUMPTION` in car.ini. Otherwise original. | conditional |
| `INIReader::clearCache` 0x140234910 | 0x5ec420 | Clears the static parsed-file cache (map pointed to by `INIReader::cache`, default object at `0x182690c10`). Same effect as the original; logs "Clear INI reader cache". | no |
| `INIReader::~INIReader` 0x140234400 | 0x5ecc00 | When flag `0x26998bc` is set and the current thread is the main thread (`_DAT_1825e27b8`), the destructor is skipped (reader leaked on purpose; the flag is set around `CameraOnBoard::update`'s original call) `[interp]`. | conditional |
| `INIReaderDocuments::INIReaderDocuments` 0x140238350 | 0x5f9090 (2,334 B) | Name `cfg/race.ini`: if env var `AC_CFG_RACE_INI` is set, the file name becomes `cfg/<value>`. Name `cfg/video.ini`: normal path (handler lists `0x182690c60/68/70` still run). Other names: if launch flag `*0xef1700()+3` is set (CSP special mode), CSP looks the name up in its **embedded defaults** (zip #2, 50 `ac/*.ini`, key = name without `cfg/`, suffix `.ini`) and fills the reader's text and parses it without touching the disk. Three handler vectors (`0x182690c60` before, `0x182690c68` content provider, `0x182690c70` after). | yes |
| `INIReader::cache` / `useCache` | lookups (data symbols `0x26a4ba0`, `0x26a4b98`) | read / written by CSP. | - |

Per-car data overrides that sit next to this seam but are not hooks (own functions, `[dll]`, not decompiled):
`0x5f2c50` / `0x5f30b0` resolve names against `skins` / `data_override` / `extension/data_override` / `content/cars`
(virtual data overlay, `lua.md` already lists it as open), `0x5f34f0` and `0x5f56a0` name the car data files the overlay
may replace: `blurred_objects.ini`, `cameras.ini`, `ambient_shadows.ini`, `analog_instruments.ini`, `fuel_cons.ini`,
`escmode.ini`, `digital_instruments.ini`, `mirrors.ini`, `extra_animations.ini`, `wing_animations.ini`,
`proview_nodes.ini` (11 names; `driver3d.ini`, `camera_facing.ini` at `0x5ec170`). `0x5f01f0` is the pop-up shown for a
bad / missing value: "OFFER:" with choices "Reinstall the car", "Reinstall the track", "Try a different car / track",
"Verify integrity of your Assetto Corsa" (`steam://validate/244210`, "Would you like to run validation now?").
`0x5f0e20` / `0x5f15e0` log "Invalid int value: X, parsed as Y" / "Invalid float value" (lenient parsing).

### 2.2 Seam table B: Path

acs.exe: `Path.obj` (ksCommon.lib), `docs/atlas/config.md` 3. Eight hooks:

| Function (acs VA) | CSP RVA | What CSP changes |
|---|---|---|
| `Path::fileExists` 0x1402305e0 (two overloads) | 0x5da470, 0x5da7d0 | First asks CSP's virtual-file rules `0x5da2a0` (special names: `driver_base_pos.knh`, `collider.knh`, `data/driver3d.ini`, `data/car.ini`, `content/cars/nul/`, `data.acd`, `sfx/nul.bank`, i.e. a **placeholder car "nul"** that always "exists"); else original-equivalent `0x5d9c60` (checks `content/cars/<car>/...`, `/sfx/`, `.bank`, `sfx/GUIDs.txt`). The first overload keeps a **hash-keyed cache** (up to 9 jobs, `_DAT_1826add30 < 10`, key = string hash + flag) of answers. |
| `Path::getFiles` 0x140230b00 | 0x11c52c0 | Original, then: pattern `content/fonts/*.ttf` also lists `../../extension/fonts/*.ttf` (CSP's fonts appear in AC's font enumeration); pattern `content/texture/clouds/*.dds` (28 chars) resets a counter (`+0x4c`) and refreshes the cloud list `[interp]`. |
| `Path::getFolders` 0x140231260 | 0x6cba40 | **First call** while `PythonInterface::PythonInterface` runs (flag `0x26a3787`): builds the Python snippet `ARG_ENABLE_CACHE = True/False` (from `[APPS] ENABLE_CACHE`) plus `python/initialize.py` under the extension folder and runs it in AC's interpreter ("Running Python insert: "). Pattern `content/tracks/*.*` (18 chars) returns an **empty list** when flag `0x26a3794` is set (AC never scans all tracks; `[interp]` loading speed). |
| `Path::getLastModificationTime` 0x140231b40 | 0x5da1c0 | Own implementation: `CreateFileW` read, then once more with `FILE_FLAG_BACKUP_SEMANTICS` (works for folders), `GetFileTime` last-write; 0 on failure. |
| `Path::getDocumentPath` 0x140230890 | 0x6c8810 | Returns a function-local static string from CSP's known-folder table (index 1), so the Documents location is CSP's, computed once. |
| `Path::getPlatformSpecificPath` 0x140231c50 | 0x6c87d0 | Original, then an optional global callback (`0x182691140`, the Wine / path translator hook `[interp]`). |
| `Path::doesFilenameContainsIllegalChar` 0x140230520 | 0x11a21c0 | Returns "none" when global byte `0x26a994a` is set; else original (CSP's own loaders pass unusual names). |

### 2.3 Seam table C: FolderEncrypter and FileChangeObserver

`FolderEncrypter` is AC's own container reader for stock `data.acd` (decrypted by AC with its built-in key; rustyAC:
`crates/rustyac-content/src/acd.rs`, "FolderEncrypter::decryptFile 0x14023bdd0"). **Rule 2 applies: what CSP does with it
is documented, nothing was decrypted.** Content that a mod maker encrypted for CSP is out of scope.

| Function (acs VA) | CSP RVA | What CSP changes |
|---|---|---|
| `FolderEncrypter::decryptFile` 0x14023bdd0 | 0x5d7490 (8.8 KB pseudo-C) | **Re-implemented**; the original is not called. The folder path is looked up in a CSP table `0x180529b40` (map folder -> map lower-cased file name -> text), a miss returns the 9-byte text `NOT FOUND` (AC's own contract, `INIReader::loadEncrypt` tests it). A special folder name `content/cars/nul/data.acd` (25 chars) is handled. If the stored text starts with `;` it is passed through `0x180a55c10(1, ...)` together with the folder name minus its 4-char extension (`.acd`): a CSP preprocessing step `[interp]` (the INIpp / `data_override` layer). The table is filled by CSP's own car loader; the readers of stock cars still use AC's code path through CSP's loader (not visible here). |
| `FolderEncrypter::fileExists` 0x14023c390 | 0x5da110 | Calls the table's "known folder" test `0x5d7a60` (names categories `content/tracks/`, `/skins/`, `content/objects3D/`, `content/driver/`, `content/fonts/`, `content/texture/`); when the folder is unknown it registers (`0x180529b40(folder)` then `0x180529930(file)`) the file in the table. |
| `FolderEncrypter::keys` | lookup (data symbol `0x26a91f0`) | AC's key table; CSP only holds the pointer. |
| `KN5IO::dlc_keys` | lookup (`0x26ab820`) | same (DLC key table of the KN5 loader); `KN5IO::addDLCKey` is a lookup in the rendering group. |
| `FileChangeObserver::observe` 0x14023b470 | 0x55ae00 | Stores the watched path (+8) only when no car is being loaded and TLS +0x1ac is 0; sets state -1 ("register on first query"). |
| `FileChangeObserver::hasChanged` 0x14023b440 | 0x55ae70 | On the first query registers the file with CSP's **files-monitoring thread** (log: "New thread: CSP: files monitoring"); afterwards the state flips to 1 when the file changes and the call consumes it. Replaces AC's modification-time polling of `DigitalInstruments`, `DigitalPanels`, `CarBrakeLights`, `Flames`, `CarColliderManager` (`docs/atlas/config.md`). Switch: `general.ini [DEV] WATCH_FOR_CHANGES=1`, hidden `[CONFIGURATIONS] CUSTOM_FILE_WATCHING=1`. |
| `FileChangeObserver::reset` 0x14023b530 | 0x2db1f0 | no-op stub. |

### 2.4 Seam table D: Sim (20 hooks, 3 lookups)

acs.exe: `docs/atlas/main_loop.md`. `Sim` is created once in `wWinMain`; CSP keeps a pointer in `_DAT_1826add00` and
fires event lists (vectors at the RVAs below, `.data`, base 0x180000000) around the original.

| Function (acs VA) | CSP RVA | What CSP changes | Event vector |
|---|---|---|---|
| `Sim::Sim` 0x140192070 | 0x11bb510 | Stores `this`; QPC start; calls original; then **patches the D3D11 immediate context**: the pointer `*_DAT_1826aa890` is an `ID3D11DeviceContext` (vtable offsets 0x48 = `PSSetShader` [index 9], 0x88 = `IASetInputLayout`, 0xc0 = `IASetPrimitiveTopology` called with 4 = triangle list, 0x1c8 = `ResolveSubresource` [index 57]); CSP inline-hooks `ResolveSubresource` (detour `0x107bc90`, name string "ID3D11DeviceContext::ResolveSubresource") and data-patches vtable slot +0x48 (saved to `0x26aa8f8`, new `0x107cd30`); applies hooks again (`0x1cd1d70(1)`). Logs "Sim initialization started", "Sim initialized: %.2f s, %.2f s", "Waiting for jobs to finish", "All jobs finished", "Final steps" (owner's log: 20.03 s and 1.02 s). This **corrects `attach.md` 1.7**, which called the object "the vtable of a global object". `[dll][log-csp]` | - |
| `Sim::update` 0x14019ef90 | 0x11bd380 | Runs `0x181550bf0` (job pump, `*_DAT_1826add08`), then CSP's own per-frame pipeline `0x1811bcf00(sim, dt)` (not decompiled; carries the "CPU OCCUPANCY > 99%" warning), then fires the update vector. The original is not called here (the CSP pipeline wraps it `[interp]`). | 0x26978e8 |
| `Sim::renderHUD` 0x14019e4f0 | 0x11bd420 | If launch flag `*0xef1700()+3` is clear: original, then the HUD vector, then the pause-menu / escape sub-form show-hide logic (iterates children of the GUI form, toggles bit 0x10 of their flags). | 0x26978c0 |
| `Sim::renderScene` 0x14019e570 | 0x11bc930 | Scene preparation: zone names "Scene preparation", "Frame shadows", "Rear mirror", "Main camera", "Initial preparation"; "Switching to free camera to prevent crashing rendering" / "Camera glitched out" when the camera matrix is NaN. Three vectors. | 0x2697878, 0x2697880, 0x2697890 |
| `Sim::loadTrack` 0x14019a4c0 | 0x11bb9e0 | Copies the two strings, fires the "before" vector, calls original, fires the "after" vector (arg = track object `+0x150`). | 0x2697898 / 0x26978e0 |
| `Sim::startGame` 0x14019eb00 | 0x11be370 | When session type (`**(sim+0x208)+0xa40`) == 1, fires a **cancelable** vector first (listener may stop the start). | 0x26978d0 |
| `Sim::setPauseMode` 0x14019e6d0 | 0x11be250 | cancelable vector, then original. | 0x26978f0 |
| `Sim::onKeyDown` 0x14019a940 | 0x11beae0 | **Re-implemented** key handler: first a cancelable vector; Escape handling with CSP flags (`IGNORE_ESCAPE_IN_REPLAYS`); digit keys set byte `0x268bcb0` (`UNBIND_TURBO_DIGITS`); instant-replay key blocked while the car is faster than 1 km/h with the messages "Stop your car to start instant replay" / "Stop for Instant Replay" (a CSP rule, offline); autopilot key calls `0x11be520` ("Autopilot is not available in this session", "AI Driver", "Activated"/"Deactivated"; `ALTERNATIVE_AI_DRIVER`, `AI_DRIVER_KEEP_SETTINGS`); other keys fall to AC's table (per-key table at `0x1826a8670..` stride 0x200). | 0x26978d8 |
| `Sim::onReplayModeChanged` 0x14019e0c0 | 0x11bfa00 | Passes a sleep time (500 ms; 20 ms when `[REPLAY] SPEED_UP_REPLAY_ACTIVATION`, byte `0x25a2e21`) through a vector (listeners may change it), calls original only for the default 500, otherwise does a shortened switch itself with `Sleep(n)`. | 0x26978a8 |
| `Sim::activateEscMenu` 0x140197780 | 0x11bd5f0 | Online with the server rule `ENFORCE_BACK_TO_PITS_PENALTY_STOP` (byte `0x26950fd`) the ESC menu does not open while the car moves faster than 1 km/h `[interp]`; offline resets `0x268ad9a`. Then original. | - |
| `Sim::activateGameGui` 0x1401977e0 / `activatePauseMenu` 0x140197840 | 0x11bd660 / 0x11bd730 | Show / hide GUI children and fire the on-hide / on-show calls of the remembered page (`iRam1825e8d30`). | - |
| `Sim::addForm` 0x140197ae0 | 0x11be4a0 | Adds a form only if it is not already in CSP's list (`0x1826add18` vector): de-duplicates. | - |
| `Sim::executeOnMainThread` 0x140199240 | 0x11bce70 | Moves AC's queued closure into CSP's job scheduler (`0x181550870(_DAT_1826add08, ...)`). | - |
| `Sim::getNationFlag` 0x140199350 | 0x11bfcd0 | Caches nation-flag entries by 32-bit hash of the code in CSP's map (`0x182697a20`). | - |
| `Sim::applyCustomWeather` 0x140198010 | 0x11bfe30 | If CSP's custom weather controller is active (byte `0x268b336`) the original is skipped and the name is written straight into the weather object (`+0x60`); the name is kept in CSP global `0x1825e29d0`. | - |
| `Sim::initStaticCubemap` 0x14019a2a0 | 0x11bd8b0 | Static reflections logic ("Static reflections: need to load cubemap model", "WeatherFX is present, no need to render cubemap", ...); `rendering_*` briefs. | 0x26978b0 |
| `Sim::setSplashMessage` 0x14019e860 | 0x11bd820 | Message "PROBLEMS DURING CHECKSUM" (24 chars) aborts through `0x18056cfa0` (breakpoint); otherwise forwards the text to CSP's splash screen while loading is active. | - |
| `Sim::shutdown` 0x14019e9e0 | 0x11bf830 | Logs "Sim::shutdown (CSP), called: ..." and "Sim::shutdown (AC), called: ..." (call-state tracing) around the original. | - |
| `Sim::unloadMeshResources` 0x14019edd0 | 0x2db1f0 | no-op stub (AC's per-session mesh unload is disabled; CSP keeps meshes). | - |
| lookups | `addCar` 0x1401978a0, `getCar` 0x1401992e0, `applyCustomWeather` (second row) | CSP calls them. | - |

### 2.5 Seam table E: Game and GameObject

acs.exe: `Game` (ksGame.lib 0x140241880..), the owner of the `GameObject` lists and `Game::run`'s loop.

| Function (acs VA) | CSP RVA | What CSP changes |
|---|---|---|
| `Game::Game` 0x140241880 | 0x1111e00 | Stores `this` (`0x1826abf68`), copies settings byte `+0x22` to the constructor's third argument `+0x3a`, original; if the debug level byte `0x2695077` == 2 calls `0x1814329c0` `[interp]`. |
| `Game::~Game` 0x140241eb0 | 0x11127c0 | **Shutdown path**: starts a watchdog thread (`0x6d0830`; named object `AcTools.CSP.ShutdownProgress.v0` for Content Manager), reads `general.ini [DEV] SAFE_EXIT` (default 1); clears 20 members of the Game; calls the original; hands the captured data to `0x181114360`. |
| `Game::run` 0x140242eb0 | 0x11141d0 | **Re-implemented** loop: `while (!quit) { frame(0x181113720); if main thread: pump Win32 messages (PeekMessage / TranslateMessage / DispatchMessage) with a nesting counter }`; at the end `Sleep(300000)` when the crash flag `0x25e9428` is set (keeps the crash reporter alive). The frame body `0x181113720` carries the profiler zones "Start", "Frame", "Preparation", "Precompute", "Wrapping up", "Present resulting frame", "Audio playback". |
| `Game::onIdle` 0x140242730 | 0x11141c0 | Pass-through (7-byte jump). |
| `Game::update` 0x140243010 | 0x1113350 | Pre-pipeline `0x1811bc4a0(sim, dt)`, update vector (dt), scoped job `AC entities` over Game's object list `0x1826abfd0` (calls `update` at vtable +8 for active objects), post vector (`0x2697470`), a per-frame helper `0x18109a2a0`, a replay / cinematic branch for session-mode `_DAT_1826add30 > 9` and `0x180eeeb70` above 100 `[interp]`; a profiler scope named "AC entities" wraps the object loop. |
| `Game::render` 0x140242d50 | 0x1112e60 | Render list loop with scope "Track audio" and the render vector (`0x2697478`). |
| `Game::renderHUD` 0x140242e30 | 0x11130f0 | Resets the context's input layout and topology when CSP's cached values differ (`0x1825e2a30` vs `0x1825e2a20`, topology 4), runs the HUD object lists (`0x1826abfa0`, `0x1826abfb0`), AC's own ksgui GUI draw (`0x180612480`) when byte `0x26951e5` is set, then fires `0x2697480` (cancel byte cleared first). |
| `Game::renderAudio` 0x140242dc0 | 0x11132d0 | Only for the current scene (`param == *(game+0x178)`): runs the audio object list `0x1826abfc0` through vtable +0x20. |
| `Game::renderHUDOnDemand` | lookup | CSP calls it. |
| `GameObject::GameObject` 0x14023fc30 / `~GameObject` 0x14023fce0 | 0x117a740 / 0x117a900 | Original, then set byte `0x268b337` = 1 ("game-object list changed") and register / unregister with CSP (`0x18117a920`). |
| `GameTime::update` 0x14044c250 | 0x5dc7c0 (rendering group) | **Trap**: CSP logs "Should not be called anymore" and breaks. The frame limiter lives in CSP's own functions `0x5dc4e0` ("Failed to create the timer", waitable timers) and `0x5dc910` ("Frame limiter"), driven by `[THREAD_TWEAKS] RENDER_TIMING` / `PHYSICS_TIMING` (0 spin ... 4 high-resolution timer). |

### 2.6 The loading-screen pump (a hidden cooperative seam)

19 replacement functions begin with the same test: loading is active (`DAT_1825e2728`), the call comes from the main thread
(`_DAT_1825e27b8`) and `GetTickCount64() - _DAT_1825e2778 >= _DAT_1825e2780` (a few tens of ms); then `0x180b9daa0` /
`0x180b503b0(elapsed)` redraw CSP's loading screen and keep the window alive. The 19: `INIReader::getString`,
`INIReader::parse`, `Spline::closestPointIndicesFlat`, `InterpolatingSpline::getLastIndexFromNorm` (2), `filterPointsTooClose`,
`AISpline::load`, `AISpline::loadFast`, `KN5IO::load*` (3), `KN5IO::getSkinOverridenTexturePath`, `KN5IO::loadTexture`,
`Material::createCBuffers`, `Mesh::compile`, `CBuffer::init`, `InputDevice::initFF`, `InputDevice::InputDevice`,
`PhysicsAvatar::startPhysics`, `SteamInterface::addAchievement` `[dll]` (grep of `decomp_hooks` for `0x180b503b0`).
A port with a threaded loader does not need this; a single-threaded one needs a progress callback at the same places.

### 2.7 kml: InterpolatingSpline and Spline (AI splines, ideal line, track map)

acs.exe: `kml.lib` (0x1401ed900..0x1401f3c00). 11 + 2 hooks, 5 lookups.

| Function (acs VA) | CSP RVA | What CSP changes |
|---|---|---|
| `InterpolatingSpline::buildGrid` 0x1401ef980, `loadGrid` 0x1401f31a0 (2), `saveGrid` 0x1401f3750, `~` 0x1401eed40 | 0x5ffcc0, 0x5ffcd0 / 0x5ffe70, 0x5ffcf0, 0x5ffc60 | **Grid cache**: the spatial grid for closest-point queries is stored under `<AC>/cache/ai_grids/` and loaded back ("Cached grid for `content/tracks/<t>/ai/fast_lane.ai` has been loaded successfully", "Loading cached payloads for fast_lane"; 36 files, 753,024 bytes there; payloads in `ai_payloads`, 41 files, 5.1 MB). The cache is skipped for paths that end `/data/ideal_line.ai`. A grid object carries the magic float 1234.5677 at `+0x18`. |
| `InterpolatingSpline::closestPointIndexGrid` 0x1401f0da0 (2) | 0x5ffd70, 0x5ffe40 | Uses the CSP grid when present (exact nearest index through `0x180b2e1f0`, distance written to the optional out-float). |
| `InterpolatingSpline::computeSplineLength` 0x1401f1db0 | 0x600530 | Own implementation of the segment lengths and total length (helper `0x1806007b0`, 4-point windows, closed vs open end handling). |
| `InterpolatingSpline::filterPointsTooClose` 0x1401f2800 | 0x600a20 | Own implementation (with the loading pump). |
| `InterpolatingSpline::getLastIndexFromNorm` 0x1401f2b90 (2) | 0x5fe700, 0x5fe8c0 | **Per-thread one-entry memo** (last normalised position -> index, TLS +0x1e0..+0x1ec) and an optional faster lookup `0x5fe470` (flag `0x268b1f3`). |
| `Spline::wrapIndex` 0x1401eead0 | 0x5fe9d0 | Closed spline: modulo; open: clamp to `0..n-1`. |
| `Spline::closestPointIndicesFlat` 0x1401edb00 | 0x6a2a90 | pump + original. |
| lookups | `Spline::closestPointIndex`, `getPointSmooth`, `InterpolatingSpline::splineToWorld`, `worldToSpline`, `buildGrid` (2nd row) | called by CSP code and by `ac.ext_splineToWorld` / `worldToSpline`. |
| (CSP function `0x5feb30`) | - | "Reversing spline" / "Spline reversed" with the names `AC_START_0`, `AC_AB_FINISH_L`: CSP can reverse an AI spline (reversed layouts). |

### 2.8 SharedMemoryWriter (8 hooks)

acs.exe: `docs/map/telemetry.md` (pages `Local\acpmf_static` 684 B, `_graphics` 304 B, `_physics` 592 B). `[dll]`

| Function (acs VA) | Branch A (no-op) | Branch B (active) |
|---|---|---|
| `initSharedMemory` 0x140185f70 | 0x2f4d70 = `return true` (3 bytes `b0 01 c3`): the pages are never created | original |
| `shutdown` 0x140186140 | 0x2db1f0 | 0x6a0160: swaps the saved writer pointer (`_DAT_1826a6fa8`) to 0 and calls the original only if it was set (no double shutdown) |
| `update` 0x140186230 | 0x2db1f0 | 0x6a0000: unless the quit flag, stores `this` in `_DAT_1826a6fa8`, runs a scoped callback list, calls the original |
| `updatePhysics` 0x140186ef0 | 0x2db1f0 | 0x69feb0: if `[SHARED_MEMORY] REDUCE_GFORCES_WHEN_SLOW` (byte `0x26a2c05`): multiplies the three `accG` floats of the physics state (`+0x1e8..+0x1f0`) by `clamp(v * 1.8 - 2.0, 0, 1)` with `v` the car speed in m/s (0 below 4 km/h, 1 above 6 km/h) for the duration of the write, original, event vector `0x26910b8` (arg: the page pointer `+0xb8`), values restored afterwards |
| `writeStatic` 0x1401877c0 | 0x2db1f0 | original (no replacement row in this branch) |

Branch A is selected by predicate `0x69fe80` (hand-decoded): "byte `0x26b8a1b` (`[HEADLESS_MODE] NO_SHARED_MEMORY`) or
`*0xef1700()[0]` (headless)". So **CSP adds no field to any AC page**; in headless tool runs there is no shared memory.
CSP's own named objects for Content Manager are `Local\AcTools.CSP.LoadingState.v0`, `AcTools.CSP.ShutdownProgress.v0`,
`AcTools.CSP.SteamInviteHandledMark.v0`, `Local\AcTools.CurrentlyPlaying.v2` and the pipes
`\\.\pipe\AcTools.CSP.Lua.%s`, `\\.\pipe\AcTools.CSP.NewBehaviour.CustomAI.Helpers` (strings `0x2250740`, `0x228aa10`,
`0x21f9630`, `0x2251120`, `0x2281360`, `0x2269a70`). `[APPS] FIX_SHARED_MEMORY2` (commented, default 0) "replaces
sim_info.py by its latest version" in Python apps. Compare with the owner's `ac_telemetry.py`: it reads only the
three standard pages (`docs/port/conditions.md` 301), so it sees nothing CSP-specific.

### 2.9 SteamInterface (11 hooks)

acs.exe: `SteamInterface.obj` (0x1401a5d70..), `docs/atlas/multiplayer.md`.

| Function (acs VA) | CSP RVA | What |
|---|---|---|
| `SteamInterface::SteamInterface` 0x1401a5d70 | 0x6f5100 | Appends `this` to a vector of live instances (`0x1826add20`), calls original. |
| `addAchievement` 0x1401a9050 | 0x6f5210 | Names that start `SPECIAL_EVENT_` (14 chars) are dropped when settings byte `+0x11` is clear; otherwise pump + original. |
| `addAchievements` 0x1401a9410 | 0x6f51c0 | Shows "Registering Steam achievements" on the loading screen, then original (tail jump). |
| `update` 0x1401accd0 | 0x6d5660 (gate B) or 0x2db1f0 (gate A) | B: if byte `0x269506a` clear, sets `0x26a3797` around the original. |
| `fetchPlayerImage` 0x1401ab490, `incrementStat` 0x1401ac190, `refreshPlayerLeaderboard` 0x1401ac7c0, `resetAchievements` 0x1401ac850, `updateStats` 0x1401acd20, `~SteamInterface` 0x1401a6ec0, `update` | 0x2db1f0 | **7 no-ops** (gate A). Gate B predicate `0x6d5630` (hand-decoded) = "function `0xed5710` true and byte `0x2695222` clear". Switch candidate: `general.ini [DEV] DISABLE_STEAM_INTERFACE2` ("Limit Steam integration, not recommended, not available online") `[interp]`. |
| `SteamInterface::playerGUID` | lookup (data) | read by CSP (online identity). |

CSP's own use of Steam (strings): interface versions `SteamClient023`, `SteamUser023`, `SteamUtils010`, `SteamFriends018`,
`STEAMAPPS_INTERFACE_VERSION008`, `STEAMUSERSTATS_INTERFACE_VERSION013`, `STEAMSCREENSHOTS_INTERFACE_VERSION003`,
`SteamNetworkingSockets012` (GNS, `libgns.dll`), "Steam interface is not available: ", a Lua `steam.*` module with
leaderboards (`uploadLeaderboardScore`), friends, overlay (`activateGameOverlayInviteDialog`), `getPlayerSteamLevel`.
Steam screenshot hotkey: `nice_screenshots.ini [BASIC] STEAM_INTEGRATION`, `STEAM_GALLERY`. [dll][ini]

### 2.10 Python bridge (PythonInterface, PyEnvironment, PyModule, PythonForm)

acs.exe: `docs/atlas/python_api.md`. `[dll]`

| Function (acs VA) | CSP RVA | What |
|---|---|---|
| `PythonInterface::PythonInterface` 0x140124920 | 0x10e6f30 | Opens the profiler zone "Initializing Python apps" (owner's log: 623,568.8 us), sets `0x26a3787` so the first `Path::getFolders` injects `initialize.py`, calls the original, then `0x10ead20`. |
| `PythonInterface::writeToLog` 0x140130340 / `printErr` 0x14012fcb0 | 0x10ea230 / 0x10ea860 | Python's stdout / stderr lines go to CSP's log with a level tag ("?exception?", "?error?", "?warn?"); with `[APPS] CUSTOM_ERRORS_HANDLING` the traceback is formatted by `python/traceback_helper.py` (`__s_pretty_traceback`), strings "Syntax error: ", ", line ", ", col. ", "Python app X raised an error during loading: ... Would you want to continue?" |
| `PythonInterface::getTexture` 0x14012fad0, `getTextureByIndex` 0x14012fbe0 | 0x10eaa00, 0x10ead10 | `getTexture` keeps a string-hash keyed cache of name -> texture (static table `0x182700918`) in front of the original; the by-index one is a thunk to the original. |
| `PythonInterface::~PythonInterface` 0x140126490 | 0x10e7040 | clears the app vector (`+0x1a8..+0x1b0`), original. |
| `PyEnvironment::PyEnvironment` 0x1402cef20, `PyModule::PyModule` 0x1402cf4c0, `PythonForm::PythonForm` 0x140124850 | 0x691d30, 0x693580, 0x10e53a0 | Constructor wrappers: allocate CSP's side object and register it (`0x692740`, `0x693ce0`, `0x10e60f0`); this is where the 213 `ac.ext_*` functions are attached (`PyEnvironment::addFunc` is a lookup). |
| `PythonForm::render` 0x140130290 | 0x10e58c0 | Profiling wrapper: pushes a scope named after the app ("Python apps/%s"), clears byte `0x26951e7` for the duration of the original, and records the elapsed time per app (the Python timings of CSP's debug app). |
| 13 plain-name entry points (`ac_*`, section 2.14) | - | wrap listener registration. |
| `PythonInterface::addFormToTaskBar` | lookup | called by CSP. |

Files: `extension/internal/python/initialize.py` (128 B: sets `ARG_LOGGING_ACTIVE`, applies `ARG_ENABLE_CACHE` to
`sys.dont_write_bytecode`), `traceback_helper.py` (4,284 B), `functions.txt` (5,763 B, header + 213 lines).
**The 213 `ac.ext_*` functions grouped by name** (regex split, a few borderline names): car state and physics getters 57
(`getFuel`, `getRideHeight`, `getSuspensionTravel`, `getDamperForce`, `getDrag`, `getDownforce`, `getAirDensity`,
`isExtendedPhysics`, `getStateAbs/Tc`, ...), camera 32 (get/set position, direction, matrix, fov, dof, clip planes, current
drivable / track camera, `isTripleScreenCamera`, `chaserCameraDebugText`), tyre 21 (`getTyre*`, `getTire*`), mirror (virtual /
real mirror control) 18, render-target and GL helpers 19 (`createRenderTarget`, `glSetTexture`, `glLoadPixelShader`,
`glFontCreate` ...), weather / rain / wiper 12, lights 11, input 11 (`isButtonPressed`, `isJoystickButtonPressed`,
`getJoystickAxisValue`, `setClipboardData`, `isG27Available`), perf / log / patch 9 (`patchVersion`, `patchVersionCode`,
`perfBegin`, `perfEnd`, `setStrictMode`, `pauseWhenHidden`, `setMessage`), Lua / ini / fs 6 (`storeLua`, `loadLua`,
`loadIniFileAsJson`, `parseIniAsJson`, `pauseFsWatching`, `resumeFsWatching`), VAO 6, spline / world 5, replay 4, audio 2.

### 2.11 Cameras

acs.exe: `docs/atlas/camera.md` (one scene camera; `ACCameraManager` owns the mode objects). Common patterns: **singleton
capture**, **cancelable event vector before the original**, **head-turn / shake correction**, **headless short-circuit**
(launch flag `*0xef1700()[0]`, `ui_apps.md` 2.0).

| Class / function (acs VA) | CSP RVA | What |
|---|---|---|
| `ACCameraManager::ACCameraManager` 0x140031270 | 0x107b160 | Stores `this` (`0x1826aa830`); in headless mode sets byte `0x26a945a` for the duration of the original; then `0x107b500` (registers CSP's camera modes). |
| `ACCameraManager::~ACCameraManager` 0x140032310 | 0x107b210 | Skips the destructor in headless mode. |
| `ACCameraManager::fadeIn` 0x140033690 | 0x107b240 | Copies the fade record (colour + time) into the manager (`+0x128..+0x13c`). Not headless: a CSP override time (`0x26fe854`, set by scripts; FLT_MAX = none) replaces the fade time; else, with CSP flag byte `0x26951e8` set, any fade longer than 0.1 s is clamped to 0.1 s unless it is exactly 2.5 s and settings byte `+0x13` is set. Headless: alpha and time forced to at most 0.001. `[interp]` snappier camera switches. |
| `ACCameraManager::setMode` 0x1400340d0 | 0x107b310 | Mode 9 (random / "previous") in headless becomes mode 5; mode 3 (track cameras) with an empty set falls back to 0; after switching, clears bit 0x10 of the "hidden by camera" flags on the car-mesh lists (`+0x1c8`). |
| `ACCameraManager::update` 0x1400345e0 | 0x107b4a0 | Mode 9 resolves to the saved mode (`+0x124`) while `0x268ad9b` is set; mode 0 -> 2 when `0x26a85e4`; marks `0x2699589` (camera changed). |
| `ACCameraManager::loadPreviousSessionUserCamera` 0x1400337e0 | 0x107b470 | headless: forces mode 5. |
| `ACCameraManager::stepCameraRandom`, `isVirtualMirrorOn` | lookups | |
| `CameraDrivableManager::CameraDrivableManager` 0x1400c4a80 | 0x108f450 | Stores `this`, original, `0x108fdf0`. |
| `CameraDrivableManager::update` 0x1400c6a30 | 0x108f870 | Fires the cancelable vector `0x2695f38` (the script-camera listeners: chaser camera Lua); if a listener marks the frame, ORs flags 0x1 / 0x2 / 0x200 into the focused car's camera flags (`car+0xa3c`), scales the current mode's offset by the car's look-back value (`+0x280`), calls the original with the modified state and restores it. |
| `updateChase` 0x1400c7120, `updateLook` 0x1400c7de0 | 0x108fa40, 0x108fb90 | `updateChase` fires cancelable vector `0x2695f40` first; both add a **head-turn** correction: with `look` = float at `+0x18` of the object returned by `0x180ed0130` (CSP's look-axis source; FLT_MAX = none, 0.5 = straight ahead) and `f` the configured side-glance angle (`+0x110`, degrees), the camera yaw (`+0x10c` / `+0x118`) becomes `f * (1 - 2 * look)` in radians; applied only when `look` is set and either `|look - 0.5| > 0.1` or the car's camera flags `& 0x203` are clear. |
| `updateBonnet` 0x1400c6ba0, `updateBumper` 0x1400c6df0 | 0x108fd50, 0x108fcb0 | Original, then override FOV (`0x25a2ddc` = `[CAR_CAMERAS] FOV_BONNET` / `FOV_BUMPER`) and optional shaking offsets (`SHAKING_BONNET`, `SHAKING_BUMPER`, `0x25a2de0/de4`). |
| `CameraDrivableManager::nextMode` 0x1400c6790 | 0x108fc80 | Increments the mode (`+0x58`), wraps after 4 to 0, then re-evaluates CSP's allow flags (`0x108f480`: `ALLOW_BUMPER`, `ALLOW_BONNET`, `ALLOW_COCKPIT`, `ALLOW_DASHBOARD`, `ALLOW_CHASE_1`, `ALLOW_CHASE_2`) and returns "wrapped to 0". |
| `CameraOnBoard::CameraOnBoard` 0x1400c84e0 | 0x10a22d0 | Stores, original, registers neck / cockpit scripts. |
| `CameraOnBoard::update` 0x1400c9ea0 | 0x10a2750 | Cancelable vector `0x2696058` (cockpit-camera Lua listeners), keeps the camera FOV (`+0x19c`), optionally disables AC's built-in head shake (byte `+0x164`) for the original call unless a non-zero shake configuration is present (cell `0x26ff59c`); sets `0x26998bc` = 1 around the original (also read by the INIReader hooks, section 2.1). |
| `CameraOnBoard::lookLeftRight` 0x1400c9b80 | 0x10a21d0 | Same head-turn correction as `updateLook`. |
| `CameraOnBoardFree::update` 0x140115db0, `forceThetaPhi` 0x140115b90, `updateSphericalCoord` 0x1401167e0 | 0x594560, 0x5949f0, 0x594370 | Orbit (F5) camera: captured-mouse look (`[ORBIT_CAMERA] CAPTURE_MOUSE`, `RMB_FOR_ROTATING`, `FIX_SPEED`), raw input (`[MISC_CAMERAS] RAW_INPUT`), optional fixed theta / phi / radius (`0x25a2cc0..cc8`) when AC asks for the default (0.1, 0.1, 5.0); reads the car's `crew.ini` sections `HEADER` / `SIDE` for the driver-head position `[interp]`. |
| `CameraTrack::CameraTrack` 0x1400cab20 / `update` 0x1400cbe30 / `nextSet` 0x1400cbc90 | 0x1090480 / 0x1090e20 / 0x1090e60 | Capture; `update` clears the camera hold (`+0xb8`) when `[TRACK_CAMERAS] FORCE_DISABLE_WOBBLE`, calls original, then `0x109af10` and marks `0x26a9177`; `nextSet` returns the 5-char text for an empty set list instead of crashing. |
| `CameraCarManager::CameraCarManager` 0x1400c4470 | 0x58f5c0 | Capture, `0x58f600`. |
| `CameraManager::getActiveCamera` 0x1401f4820 | 0x58fab0 | Caches the result in `0x26b6af0` (re-fetched when `DAT_18268b6a4`). |
| `CameraModeStart::updateCar` 0x140060470 | 0x592630 | Original, then optional CSP start-camera trajectory (`USE_TRAJECTORIES_FOR_START_CAMERA`, helper `0x591e20`) or a position offset + smoothstep blend with the car (fields `+0xb0..+0xc4`). |
| `CameraFacing::CameraFacing` 0x14005e640 | 0x58f620 | Capture + `0x58fa90`. (Not a camera: crowd billboards, `docs/atlas/camera.md`.) |
| `CinematicFreeCamera::getMouseLook` 0x14006ca60 | 0x5a6700 | Wraps the free-camera look: sets `0x2697ecd` during the call, uses the captured-mouse input state (`RAW_INPUT`, `RESTORE_POSITION`) and an optional gain `0x25a2cb8`; zeroes the look when the GUI wants the mouse. |
| `CarHintLoader::CarHintLoader` 0x140068860 / `~` 0x140069770 | 0x5a5c80 / 0x5a5d20 | Event vector `0x2690b38` after construction (assist hints); the destructor fires the clear path `0x5a6070`. |

### 2.12 Small ones (each one hook or lookup)

| Function (acs VA) | CSP RVA | What |
|---|---|---|
| `Console::show` 0x14018c7f0 / `Console::parse` 0x14018bf50 (ui group) | 0x5a6980 / 0x5a6990 | `show` always calls the original with `false` (the developer console cannot be opened; gate byte `0x26951e2`); `parse` fires the cancelable vector `0x2690b40` (Lua console-input listeners) first. |
| `CommandManager::CommandManager` 0x1400e8b70 | lookup | CSP creates / reads it. |
| `X360Joypad::acquire` 0x140243ff0 / `setVibrations` 0x140244110 / `getButtonMask` * 0x140244010 | 0x6bf270 / 0x6bf260 | `acquire`: reads XInput state `0`, ORs a CSP virtual-button mask (`0x26a85c8`, filled by the Lua gamepad scripts) and clears the 4 D-pad bits when `[CONTROL] RELEASE_DPAD` (byte `0x26a2c47`); `setVibrations`: clamps both motors to 0..1 and scales to 0..65535, sets a TLS flag (+0x1af) during the call when the vibration filter says so. See `input_ffb.md`. |
| `MouseHider::forceMouseHide` 0x1401992d0 | 0x6238b0 | Stores the flag also in CSP's global `0x268adcd`. |
| `NKUtils::getSectorID` 0x14018d2b0 | 0x6ccae0 | When CSP's check `0x6cc7d0` (reads the track's `data\surfaces.ini`) says "extended", uses CSP's parser `0x6cc6c0` for the sector name; else original `[interp]`. |
| `ksSecurity::validate` 0x1402d01f0 | 0x428cd0 | Pass-through (jump to the original). Nothing is bypassed here. |
| `PIDController::eval` 0x14044e720 | lookup | CSP uses AC's PID. |
| `Texture::Texture` 0x1400322f0 (5 overloads) | lookup | CSP constructs AC textures. |
| `CrashGuard::CrashGuard` 0x140218800 (rendering group) | 0x6c8b70 | Deletes old `CrashDumps\acs.exe.*.dmp`, starts the module factory, logs "Launched from: " and "Env.: " (`attach.md` 1.7). |
| `DebugLog::DebugLog` 0x140217ef0 (rendering group) | 0x6cf630 | Zero-fills the 3 words (AC's log object does nothing) and `~DebugLog` is a no-op; AC's own prints are captured at `MSVCR120!printf` instead. |
| `PostProcessEffectsUpdater::PostProcessEffectsUpdater` 0x1400ab880 / `update` 0x1400abc20 (rendering group) | 0x6915a0 / 0x6915c0 | Capture + own update (rendering brief). |
| `KN5IO` (rendering group): `dlc_keys` lookup | - | see 2.3. |

### 2.13 The 186 rows without an acs.exe function: what they are

| Class of row | Rows (distinct) | What they are |
|---|---|---|
| `ac_ext::*` | 89 (88) | **HLSL constant-buffer / struct layouts of CSP's own shaders**: `cbuffer_*` / `*_cbuffer_draw` / `*_simulate_cbuffer` (GPU particle systems: cardrops, fireworks, flames, marbles, miragedisruption, oilspill, pieces, skidwet, smoke, sparks, traces, windscreen; `track_emitter` structs), `rain_*` (drops, maps, puddles, screen drops, windscreen, streaks), `skidmarks_renderer` (4), `data_ac_ext_*` (extra_fx, fake_shadows, grass, gui / kiosk blend, nice_screenshots, rain_fx, reflections_fx, windscreen_fx), sky / moon / cloud data, post-processing (`pp_ffxcas`, `pp_upscaler_fsr1` x2, `pp_upscaler_fsr_old`, `gradj_data_fxaa3`, `gradj_data_mlaa`, `base_gradj_ppaa_data`), interior masking (3), `bending_shader` (2), `navigator_impl::cbuffer_map_layers` (the only one of the three "hook" rows that is not a destructor), `refracting_headlight_data`, `hs_car_holder`, `animated_wiper_impl`, `sprite_fx_ext_data`, `track_area_heightmap_impl`, `data_track_display`. |
| `utils::*` | 25 (5) | Library type names: `blob` (3), `http::http_reply` (2), `mz_result` (17, miniz result wrapper), `path` (1), `winhelper::run_reply` (1). |
| `mz::*` | 12 (12) | miniz result codes `OK`, `stream_end`, `need_dict`, `errno`, `stream_error`, `data_error`, `mem_error`, `buf_error`, `version_error`, `param_error`, `limit_exceed_1/2`. |
| `efx::*` / `pp::*` / `final` / `mirror` / `dynamic` / `special` / `splashscreen` / `dx` / `cui` | 8 + 6 + 1 + 1 + 1 + 3 + 5 + 4 + 1 | Names of render targets / channels of CSP's frame graph (`efx::depth`, `motion`, `normals`, `stencil`; `pp::colorGrading3D`, `depth`, `hdr`; `final::clean`; `mirror::raw`; `special::driver`, `livery`, `theme`; `splashscreen::background`, `badge`, `logo`, `preview`, `track`; `dx::dx_perlinworley::pw_settings`, `fake_shadows_common::ps_data` / `vs_data`, `vr_mirror_helper_data::cbuffer_data`; `cui::imgui_offscreen`). `[interp]` |
| `data::*` | 6 (3) | Music-reader backends: `music_reader_aimp`, `music_reader_win10_direct`, `music_reader_win10_exe` (see `music.ini`). |
| `AC::*` | 6 (6) | Enum / struct names used with FFI-like reflection: `DataCameraExtended`, `DepthMode`, `EVRInitError`, `FMOD::FMOD_DSP_RESAMPLER`, `FMOD::FMOD_RESULT`, `socket_configure_qos::_QOS_TRAFFIC_TYPE` (`[QOS_TWEAKS]`). |
| `lua::known_executables::parsed`, `hooks::chunks_optimizer::mode`, `hooks::car_flex_data::impl::flex_cbuffer`, `Concurrency::concurrent_queue`, `FW1FontWrapper::CFW1GlyphRenderStates::ShaderConstants`, `ID3D11DeviceContext::ResolveSubresource` | 2+2+1+1+1 | Type names; the last is the string used to name the context hook (section 2.4). |
| acs.exe class names without a function VA | 11 | **Data symbols**, all lookups: `INIReader::cache`, `INIReader::useCache`, `FolderEncrypter::keys`, `KN5IO::dlc_keys`, `Speed::useMPH` (the units flag), `SteamInterface::playerGUID`, `ReplayRecorder::recordIntervalMS` (`docs/atlas/replay.md` 88: 120, 90, 60, 30, 15 ms), `RaceManager::carsToBeLoaded` (the smoke-cap input, `docs/port/draw_hear_cars.md`), `CarLodManager::lodDistDivisor`; plus 2 destructors the PDB spells differently (`CBuffer::~CBuffer` 0x10b3680, `AnimationPlayer::~AnimationPlayer` 0x10e3790; the only real hooks of the group without an acs VA). |

Evidence for "not hooks": 175 of the 186 rows have registration sites scattered over `0x49732d..0x1ddb6f1` (inside the
rendering / Lua modules), not in the contiguous hook block `0x22cbd6..`; the rows have no slot and no function. `[interp]`:
they are names registered for type-checking / logging (the cbuffer ones are probably compared with shader reflection).

### 2.14 Plain-name families `ks*` / `ac_*` (attach.md 2.2, not in `csp_hook_table.tsv`)

| Name (acs VA) | What it is |
|---|---|
| `ksGenerateCrash` 0x1402399a0 (wstring) / 0x140239a10 (void), `ks_critical_error` 0x140024c70 | Crash helpers (CSP's `CRASH_ON_F12`, forced-crash path, YEBIS critical errors). |
| `ksGetNumberOfProcessors` 0x140239a40, `ksSetCurrentThreadAffinityMask` 0x140239bc0 | Thread / affinity helpers (`[THREAD_TWEAKS]`). |
| `ksRandomize` 0x14004b290, `ksTimeFromAngle` 0x1400b3eb0, `ksCalcCamberRAD` 0x1402cafe0, `ksDecodeUtf8` 0x140238dc0 / 0x140238df0 | Small utility functions CSP calls. |
| `acLocaleGetString`, `acTranslate`, `acLocaleSetLocale`, `acLocaleGetSetupHelpText`, `acShutdown` | Localisation lookups (CSP's UI translates its own strings through them) and shutdown. |
| 13 `ac_*` Python C entry points | `ac_newApp` 0x14012cb00, `ac_addRenderCallback` 0x140128450, `ac_addOnValueChangeListener`, `ac_addOnClickedListener`, `ac_addOnListBoxSelectionListener`, `ac_addOnListBoxDeselectionListener`, `ac_addOnCheckBoxChanged`, `ac_addOnValidateListener`, `ac_addOnChatMessageListener`, `ac_addOnAppActivatedListener`, `ac_addOnAppDismissedListener`, plus the two getters `ac_getCarRealTimeLeaderboardPosition`, `ac_getCarTyreCompound`: CSP wraps the callback registrations (error handling, `SORT_PYTHON_APPS`, app naming) and overrides two getters. `[dll][interp]` |
| `checkRaceINI` 0x1400c09b0, `loadDriverBasePos` 0x1400fb0a0, `clearPythonCache` 0x14012f710, `string2wstring` (0x14001eaa0, 0x140232090) | acs.exe free functions CSP looks up (`checkRaceINI` is part of the race.ini contract; `clearPythonCache` pairs with `ENABLE_CACHE`). |

---

## 3. Config keys and Lua API

Counts are uncommented `KEY=` lines in `extension/config/*.ini`; the rest of `general.ini` belongs to other briefs.

| File | Sections (keys) | This file's keys (default; own words) |
|---|---|---|
| `general.ini` (35 sections, 264 keys) | `BASIC` (1), `CONFIGURATIONS` (10), `CONTROL` (8), `CAR_CAMERAS` (10), `ORBIT_CAMERA` (3), `TRACK_CAMERAS` (1), `MISC_CAMERAS` (3), `SHARED_MEMORY` (1), `DEV` (38), `THREAD_TWEAKS` (6), `QOS_TWEAKS` (5), `APPS` (4), `LAP_TIMES` (3), `REPLAY` (5) = 98 | `ENABLED=1` master; `BLOCK_UNTRUSTED_NETWORKING=0`; `USE_MMF=1`; `CRASH_PLUGIN=0`; `CLEAR_SHADERS_CACHE2=0`; `CLEAR_CSP_CACHE=1`; `HTTP_DISABLE_FLAGS=TCP_FAST_OPEN, HTTP3`; `ADAPTER_NAME=`; `LOOPBACK_SERVERS=0`; `CHECK_FOR_RAPID_CRASHES=1`; `CRASH_ON_F12=1`; hidden `FILE_OP_HOOK`; **`[CONTROL]`** `LOOK_OUT_FOR_MISSING=0`, `RELEASE_DPAD=0`, `UNBIND_TURBO_DIGITS=0`, `UNBIND_GAMEPAD_SHORTCUT=1`, `ALTERNATIVE_AI_DRIVER=1`, `AI_DRIVER_KEEP_SETTINGS=1`, `IGNORE_ESCAPE_IN_REPLAYS=0`, `INACTIVE_ONLY_WHEN_COVERED2=2`; **`[CAR_CAMERAS]`** `ALLOW_BUMPER/BONNET/COCKPIT/DASHBOARD/CHASE_1/CHASE_2=1`, `FOV_BUMPER=60`, `SHAKING_BUMPER=0,0`, `FOV_BONNET=60`, `SHAKING_BONNET=0,0`; **`[ORBIT_CAMERA]`** `FIX_SPEED=1`, `CAPTURE_MOUSE=0`, `RMB_FOR_ROTATING=0`; `[TRACK_CAMERAS] FORCE_DISABLE_WOBBLE=0`; `[MISC_CAMERAS]` `RAW_INPUT=0`, `RAW_INPUT_SPEED=2`, `RESTORE_POSITION=1`; **`[SHARED_MEMORY] REDUCE_GFORCES_WHEN_SLOW=0`**; **`[DEV]`** `WATCH_FOR_CHANGES=1`, `USE_PARTIAL_RELOAD=1`, `DETAILED_CRASHES=0`, `SILENT_LUA=0`, `SAFE_EXIT=1`, `DISABLE_STEAM_INTERFACE2=0`, `GRACEFUL_START=3`, `PROFILE_LOADING=0`, `PROFILE_MEMORY=0`, `PRINT_OUT_ENV=0`, `DIRECT_MAP_INIT=0`, `D3D_DEBUG=0`, `HANDLE_ALT_COMMANDS=1`, `PREVENT_SLEEP=2`, `CONSIDER_OCCLUDED_FLAG=1`, `NANORT_SCENE_RAYCAST=2`; **`[THREAD_TWEAKS]`** `RENDER_TIMING=4`, `PHYSICS_TIMING=4`, `PROCESS_PRIORITY=1`, `RENDER_HIGHEST_PRIORITY=0`, `MMCSS_PRIORITY_RENDER=0`, `MMCSS_PRIORITY_PHYSICS=1`; `[QOS_TWEAKS]` `BASE_TOS_FLAG=0`, `QWAVE_CFG=0`, `QWAVE_TYPE=5`, `QWAVE_NON_ADAPTIVE=1`, `QWAVE_DSCP_VALUE=46`; **`[APPS]`** `CUSTOM_ERRORS_HANDLING=1`, `CUSTOM_ERRORS_HANDLING_MESSAGES=0`, `ENABLE_CACHE=0`, `WRAP_HOOKS=1`, (commented) `FIX_SHARED_MEMORY2`; `[LAP_TIMES]` `KEEP_AI_LAPS=0`, `KEEP_ALTERED_LAPS=0`; `[REPLAY]` `CLIP_DURATION=30`, `LOOP_INSTEAD_OF_PAUSE=0`, `KEEP_CAMERA=1`, `ALLOW_REPLAYS_ONLINE=1`, `ACCOUNT_FOR_PING=1`, (commented) `SPEED_UP_REPLAY_ACTIVATION`, `EXTRA_LARGE`, `JUMP_FIX2` |
| `chaser_camera.ini` (6 sections, 15 keys) | `BASIC` (`ENABLED=0`, `IMPLEMENTATION=base`), `BASIC_2` (second camera), `POSTPROCESSING_SCRIPT_SETTINGS(_2)`, `LOOK` | `XBOX_STICK=1`, `XBOX_STICK_DEADZONE=0.03`, `XBOX_STICK_EXPONENT=1.8`; chosen script's settings in `extension/state/lua/chaser_camera/<impl>__settings.ini` |
| `freer_camera.ini` (7 / 48) | `BASIC` 6, `MAIN` 11, `ALTERNATIVE` 12, `PEDAL_ADJUSTMENTS` 5, `SLOW_MOTION` 5, `POINTS` 3 | `ENABLED=1`, `SWITCH_MESSAGE=1`, `VERTICAL_MOVEMENT=1`, `ALTERNATIVE_CONTROLS=0`, `DISABLE_DOF=1`, `CAPTURE_MOUSE=1`; move / look / FOV speeds and lags for two sets (Scroll Lock switches); pedals change speed (off); slow motion (hold LMB, rate 4, on); stored positions (`Ctrl+Alt+digit`, off). Native C++ module (no Lua). |
| `neck.ini` (15 / 100) | `BASIC` (`ENABLED=0`, `ENABLED_IN_BASE_MODE`, `_IN_REPLAY`, `_IN_VR`, `_WITH_TRACKIR`), `SCRIPT` (`ENABLED=1`, `IMPLEMENTATION=default`), `LOOKAHEAD`, `ALIGNMENT_BASE` / `_VR` (18 each), `LOOK`, `PAN`, `GFORCES_BASE` / `_VR`, `EFFECTS_BASE` / `_VR`, `HELMET`, `DEV` | "Camera: Cockpit (Neck FX)": look at apexes, follow steering and velocity, stabilised shake, tilt with G, free look on RMB; **off by default** (`BASIC/ENABLED=0`) |
| `spectating.ini` (1 / 1) | `BASIC ENABLED=0` | hidden spectator mode (render modes `SPECTATING_AWARE/SHADOWS/GBUFFER/MAIN` in the dll) |
| `small_tweaks.ini` (15 / 73) | `MISCELLANEOUS` 27, `SHOWROOM` 2, `RULES` 2, `MUMBLE` 2, `OBS` 4, `TRACK_IR` 6, `AUDIO_VOLUME_BUTTONS` 8, `FFB_ADJUSTMENTS` 3, `ONBOARD_HOTKEYS` 4, `REALMIRROR_HOTKEYS` 2, `CUSTOM_DRIVER_MODELS` 2, `WALKING_OUT_PERSON` 5, `AUDIO_PEAK_FILTERS` 2 | `ALWAYS_ALLOW_FREE_CAMERA=1`, `TURN_HEAD_WITH_FREE_CAMERA=0`, `PYTHON_APPS_EXTENSION=1`, `FIX_SETUP_SORTING=1`, `PAUSE_IN_BACKGROUND=0`, `CHECK_INTEL_BUG=1`, `DISABLE_TELEPORT_TO_PITS=0`, `DISABLE_SESSION_RESTART=0`, `ALT_IMPLEMENTATION=1` (TrackIR), ... |
| `taskbar.ini` (8 / 51) | `BASIC` 3, `ARRANGEMENT` 4, `SORTING` 5, `LAYOUT` 6, `LAYOUT_DIMENSIONS` 16, `MOUSE_POPUP` 5, `MOTION` 5 | `ENABLED=1`; `REQUIRE ! gui.ini:NEW_UI/REPLACE_TASKBAR` (the new GUI module can replace it; `ui_apps.md`) |
| `nice_screenshots.ini` (10 / 59) | `BASIC` 5, `NAME_FORMAT` 9, `WINDOWS_IMAGING_COMPONENT2` 3, `ACCUMULATION_AA` 17, `BRIGHTNESS_BOOST` 5, `ACCUMULATION_COMPONENTS` 3, `ACCUMULATION_DOF` 2, `ACCUMULATION_BLUR` 7, `ACCUMULATION_INTERMEDIATE_SAVING` 3 | `ENABLED=1`, `ACCUMULATION_AA/ENABLED=1`, `ITERATIONS=24`, `RESOLUTION_MULTIPLIER=1`, `SHARPENING=0.05` (FidelityFX CAS), `NAME_FORMAT/MODE=ORIGINAL`, `STEAM_INTEGRATION=1`, WIC JPEG quality 99 with EXIF; accumulation shaders are the `ac_ext::data_ac_ext_nice_screenshots` buffer. Rendering side: `rendering_config.md`. |
| `music.ini` (5 / 23) | `PEAK_METER_MONITOR`, `WIN10_LISTENER` 8, `WIN10_LISTENER_TWEAKS` 5, `AIMP_TRACK_INFO_PLUGIN` 4 | currently-playing track for displays; `plugins/AcTools.CurrentlyPlaying.exe` + `libcurrentlyplaying.dll`; `LIVE_SUPPORT=0` |
| `module_lw.ini` (1 / 1) | `BASIC ENABLED=0` | hidden module with no description; types `ac_ext_module_lw`, `ac_ext_module_lw_settings` exist (purpose unknown) |
| `lua_tools.ini` (1 / 1) | `BASIC ENABLED=1` | "Create new Objects Inspector tools with Lua" (tools folder, below) |
| `data_alt_mapping.ini` | `[OPTIONAL]` 6, `[NAMES_ALT]` 58, `[NAMES_WINE]` 22 | symbol alias table (`attach.md`) |

**Owner's overrides** (`Documents\Assetto Corsa\cfg\extension\`, 15 files; `general.ini` and `gui.ini` and `weather_fx.ini`
are 0 bytes): nothing here touches this area. So on this PC: Chaser camera off, NeckFX off, freer camera on, spectating off,
module_lw off, taskbar on, nice screenshots on, music default, all general.ini defaults. `[log-csp]`

**Lua**: no `ac.*` API of this file exists beyond the 213 Python names (those are Python, not Lua). The Lua kinds that
use this seam, `[lua]`:

| Kind | Folder | Files | Entry | What it is |
|---|---|---|---|---|
| chaser camera | `extension/lua/chaser-camera/<name>/` | 4 scripts (`base`, `arcade-mode`, `arcade-mode-2`, `drone`), 9 files, 17,122 B | `script.update(dt)`; objects `ac.Camera`, `ac.getCameraParameters()`, `ac.getDesiredLookDirection()`, car fields `car.velocity`, `car.look`, `car.acceleration`; settings via `ac.INIConfig.scriptSettings()` + `settings.ini` | Replaces the chase camera. Selected by `chaser_camera.ini [BASIC] IMPLEMENTATION` (and a second one with `BASIC_2`). Hooked at `CameraDrivableManager::update` / `updateChase` (cancelable vectors `0x2695f38/40`). SDK module `ac_chaser_camera`: 1,718 functions (923 own), 82 classes. |
| cockpit camera | `extension/lua/cockpit-camera/default/` | 1 script, 3 files, 6,527 B | `script.update(dt)`; `ac.CockpitCameraMode`, `car.groundNormal`, `car.splinePosition`, `ac.trackProgressToWorldCoordinate` | "NeckFX" implementation (align to ground, steering and slide following, G tilt, shake). `neck.ini [SCRIPT] IMPLEMENTATION=default`. Hooked at `CameraOnBoard::update` (vector `0x2696058`). SDK `ac_cockpit_camera`: 1,688 functions (893 own). |
| loading screen | `extension/lua/loading-screen/default/` | 1 script, 2 files, 5,330 B | `script.update(dt)` with the `ui.*` drawing API | Splash screen implementation (`splashscreen.ini [SCRIPT]`; `ui_apps.md`). SDK `ac_splashscreen`: 1,294 functions (499 own). |
| tools | `extension/lua/tools/` | 2 tools (railroad editor, traffic tool), 111 files, 579,906 B | `tool.lua` + `manifest.ini` | Objects-Inspector-style developer tools (`lua_tools.ini`); SDK `ac_tools`. |

(Other kinds in `extension/lua/`: `cars`, `ffb-postprocess`, `fireworks`, `joypad-assist`, `mouse-steering`, `new-modes`,
`pp-filters`: see `lua.md`, `input_ffb.md`, `physics_*`.)

---

## 4. Files it reads

| What | Path / source | Read by |
|---|---|---|
| every AC ini | `system/cfg/*.ini`, `Documents\Assetto Corsa\cfg\*.ini`, `content/cars/<car>/data/*.ini` (+ `data.acd`) | AC through the hooked `INIReader` |
| embedded defaults | 50 `ac/*.ini` (zip #2, e.g. `ac/video_showroom.ini`); `acos_showroom.ini` / `acos_splitscreen.ini` / `video_showroom.ini` names in the dll | `INIReaderDocuments` hook when flag `*0xef1700()+3` is set |
| env vars | `AC_CFG_PROGRAM_NAME`, `AC_CFG_RACE_INI`, `AC_TOOL_RUN` (tool-run mode, keys `HEADLESS_MODE`, `SINGLE_THREAD`, `NO_DIRECT_INPUT`, `NO_SHARED_MEMORY`, `INCLUDE_APPS`, `SIMPLIFY_COLLISIONS`, `DISABLE_INPUT`, `ENABLE_PYTHON`, `SHOW_PROGRESS`, `TASKBAR`; log "Starting tool run") | getString / INIReaderDocuments hooks, settings loader `0x1387350` |
| CSP settings | `extension/config/*.ini` (57), `config/cars/{loaded,generic,common}`, `config/tracks/...`, `content/{cars,tracks}/*/extension/ext_config.ini`, owner's `cfg/extension/*.ini` | CSP's config module; the getString handler chain |
| file watching | `extension/` glob `{extension\? & ! ?\real_mirrors\? & ! ?\state\? & (?.ini|?.lut)}` (`attach.md` 1.4 (a)) | files-monitoring thread + `FileChangeObserver` |
| splines | `content/tracks/<t>/ai/fast_lane.ai`, `pit_lane.ai`, `data/ideal_line.ai`; `<AC>/cache/ai_grids/*.bin`, `ai_payloads/*` | `InterpolatingSpline` hooks, AI |
| Python | `extension/internal/python/{initialize.py,traceback_helper.py,functions.txt}` | `Path::getFolders` insert, `PythonInterface::printErr` |
| fonts | `content/fonts/*.ttf`, `extension/fonts/*.ttf` | `Path::getFiles` |
| cache folder `<AC>/cache/` (owner's PC, 11 sub-folders + 3 files) | `ai_grids` 36 files 753,024 B; `ai_payloads` 41 / 5,117,891 B; `cars_data` 24 / 6,964,000 B (per-car zips: ambient shadows, rain occlusion); `csp_configs` 31 / 236,889 B (`ConfigsCache` of merged configs); `lua` 28 / 5,286,013 B; `lua_shaders` 1; `meshes_metadata` 46 / 1,795,904 B; `track_logos` 17 / 6,568,568 B; `tracks_data` 13 / 130,444 B; `trees_alignment` 1 / 1,159,936 B; `web` 24 / 2,501,454 B; `clouds.bin` 44,564,640 B; `imgui.bin` 5,159,108 B; `memory_layout.bin` 524,921 B (symbol cache, `attach.md`) | CSP; cleared by `[CONFIGURATIONS] CLEAR_CSP_CACHE=1` when the CSP version changes |
| Steam | `steam_api64.dll` interfaces (section 2.9) | CSP, SteamInterface hooks |
| crash / dumps | `CrashDumps\acs.exe.*.dmp` (deleted at start), `crash_logs`, `plugins/AcTools.ErrorReporter.exe` | `CrashGuard` hook, `0x13cea80` |

**Network facts (only from strings and decompiles; nothing was captured).** No analytics, usage-statistics, user-id or
phone-home string was found (searched `analytic`, `statistic`, `sentry`, `user-agent`, `anonym`, `consent`, `privacy`, `opt-out`).
The outgoing endpoints in the image are: `https://acstuff.club/j/known.dat` (fetch, `0x3027f0`: the allow-list for
`os.runConsoleProcess` / `os.openURL`, `lua.md`), `https://assets.acstuff.club/shared-soundbank/%s` (download of a
missing shared soundbank into `cache/shared_audio/<id>/{GUIDs.txt,audio.bank}`, strings `0x221ba80`, `0x2284280`),
`https://ss.acstuff.club/` (setup sharing, `online.md`), `http://<ip>:<port>/api/details` and `/INFO` (AC server, `online.md`),
a clip-upload URL taken from the server (`UPLOAD_URL`, `__CLIPS_UPLOAD_URL`, "Clip upload URL is not set"). User-opened
pages: `https://acstuff.club/patch`, `/patch/?r=%d`, `/patch/?r=latest` (message "Newer version of CSP is required",
`0x1405310`, `0x14054b0`), `https://acstuff.club/u/blob/ac-fonts.zip` (opened with `ShellExecuteW`, `0x4a7500`),
Google search link for errors, Steam store page, `acstuff.club/s/redirect/vs2010`. **Update check**: no automatic
check of CSP's own version was found; version problems are detected against the server / car / track `REQUIRED_VERSION` and
the message links the patch page. The "Check for updates for installed mods" string (`0x22a92e0`, function `0x1371b30`)
is part of a Lua-crash message that points the user to Content Manager. **Crash reports**: `0x13cea80` shows "Would you
like to share the crash report for further analysis?" and opens the reporter plug-in with `ShellExecuteW`; the
destination of "share" is not in this function (request). General HTTP client: WinHTTP incl. WebSocket (`attach.md`).
`[CONFIGURATIONS] BLOCK_UNTRUSTED_NETWORKING` and `ADAPTER_NAME` / `HTTP_DISABLE_FLAGS` control it.

---

## 5. How it is switched on

* Everything in 2.1 to 2.14 is **always on** once CSP loads (`[BASIC] ENABLED=1`); there is no per-hook switch (`attach.md`
  1.7). Branch selection is by gate predicates evaluated at start: headless / tool-run (`AC_TOOL_RUN` or
  `[HEADLESS_MODE]` keys, flags `0x26b8a18..1e`, launch struct byte 0), CM special modes (showroom, splitscreen,
  launch struct byte 3), `DISABLE_STEAM_INTERFACE2`.
* Per car: `[HEADER] VERSION=extended-N | extendedV2-N` (only while a car loads, inside `CarAvatar::initCommon`); per
  track: `WAV_PITCH=extended-N`; `lods.ini` post-processing and the `data_override` overlay apply to every car.
* User switches in this file: `WATCH_FOR_CHANGES`, `USE_PARTIAL_RELOAD`, `SAFE_EXIT`, `RENDER_TIMING` /
  `PHYSICS_TIMING`, `REDUCE_GFORCES_WHEN_SLOW`, `RELEASE_DPAD`, `ALLOW_*` camera keys, `IGNORE_ESCAPE_IN_REPLAYS`,
  `ALTERNATIVE_AI_DRIVER`, `CUSTOM_ERRORS_HANDLING`, `ENABLE_CACHE`, `[CHASER/FREER/NECK]` module switches.
* On this PC: all defaults; chaser camera and NeckFX off, freer camera and taskbar on, no tool-run, no showroom. The
  owner's log shows "Sim initialized: 20.03 s, 1.02 s", "Initializing Python apps: 623568.80 us", "Cached grid for
  `...ai/fast_lane.ai` has been loaded successfully", "[ConfigsCache] Loaded cached version of ...monaco_2017.ini".
  `[log-csp]`

---

## 6. Size

| Item | Number |
|---|---|
| rows of the group | 337 (151 with an acs.exe symbol, 186 without) |
| real hooks | 131 on acs.exe symbols (119 distinct functions, about 45 KB) + 2 destructors |
| INIReader block | 12 hooks, `0x5ec030..0x5f99ae` about 55.7 KB, 150 functions incl. helpers |
| event vectors identified in this group | 27 (Sim 12, Game 4, camera 3, Console 1, CarHintLoader 1, SharedMemoryWriter 1, INIReader 2 (`0x182690c40` lods, `0x182690c58` getString), INIReaderDocuments 3); Python none |
| config files / keys | 11 ini files (general share 98 uncommented keys of 264), chaser 15, freer 48, neck 100, small_tweaks 73, taskbar 51, nice_screenshots 59, music 23, spectating 1, module_lw 1, lua_tools 1 |
| Lua | 4 chaser scripts, 1 cockpit script, 1 loading-screen script, 2 tools (111 files, 579,906 B) |
| Python API | 213 `ac.ext_*` functions, 13 wrapped `ac_*` entry points, 2 Python files (4,412 B) |
| cache folder | 11 sub-folders (about 30.5 MB in files) + 3 big files (about 50 MB) |
| embedded zips involved | zip #2 (50 `ac/*.ini`), #5 fonts (3 TTF) |

---

## 7. Port difficulty for rustyAC

Overall **M for what rustyAC needs, L for a faithful copy**. The seams exist only because acs.exe is closed; rustyAC owns
the code and needs the **behaviours**, not the hooks.

| Piece | Difficulty | Reuse | What is hard | Proof |
|---|---|---|---|---|
| INIReader value rules (extended-N, REPLAY / FFMULT / STEER_ASSIST overrides, `[INFO] SHORT_NAME` / `SCREEN_NAME` fallback, lenient number / vector parsing, `KEY_NOT_FOUND [FUEL] CONSUMPTION` waiver, `DYNAMIC_CONTROLLER_` -> `WING_n` check) | **S** | `rustyac-physics/src/data/ini.rs` (1:1 reader already) | Which rules matter for stock content: only the lenient parsing and the SHORT_NAME / DRIVER_NAME fallbacks change stock behaviour; the rest need extended content | unit tests on text inputs; run the same car.ini through CSP-on and CSP-off game (`general.ini [BASIC] ENABLED=0`) and compare the getter results logged by `car_oracle` |
| Environment overrides `AC_CFG_PROGRAM_NAME`, `AC_CFG_RACE_INI` | **S** | `rustyac.exe --cfg-dir`, `--out-dir` already select folders | none | launch with the variable and observe which file is opened |
| Handler chain (a place where mods override any key) | **M** | none | designing the registry (file-suffix, section, key) -> value, plus partial reload | golden: a handler that returns a fixed value for one key changes exactly that read |
| `Path` / `FolderEncrypter` seam (virtual files, nul car, fonts folder, mtime of folders, `content/tracks/*.*` skip) | **S** | `crates/rustyac-content/src/vfs.rs` (virtual file set, used by the browser), `acd.rs` | the "nul" placeholder car semantics and the `;`-preprocessing (unknown) | compare `fileExists` answers for the listed names with CSP on (Process Monitor log or CSP's own log) |
| Files monitoring / hot reload (`FileChangeObserver`) | **S** | `notify` crate | none | touch a car ini and see the instrument file re-read |
| Sim / Game event bus (before / after hooks, cancelable events, main-thread job queue, frame limiter) | **M** | `crates/rustyac-game/src/sim.rs`, `timer.rs` | choosing which 20 Sim events are public; frame pacing with `RENDER_TIMING` | order-of-calls test; frame-time histogram versus CSP with `RENDER_TIMING=4` |
| Loading-screen pump | **S** | the loader is separate | nothing if loading is already asynchronous | n/a |
| Shared memory tweak `REDUCE_GFORCES_WHEN_SLOW` | **S** | `rustyac-game/src/shm.rs` | none: factor `clamp(v*1.8-2, 0, 1)`, v in m/s, applied to the three `accG` values of the page only | read `acpmf_physics` with `ac_telemetry.py` while crawling below 6 km/h, CSP on vs off (note: the engine's own `accG` is unchanged) |
| Headless: no shared memory, no Python, single thread | **S** | `--no-gui` | flags map | n/a |
| Steam | **S** (skip) | `docs/port/apps_user.md` ("without Steam, proven") | nothing | n/a |
| Python bridge (213 `ac.ext_*`, traceback helper, callback wrapping) | **XL** | none (rustyAC has no Python) | embedding Python 3.3 or re-hosting apps in Lua | skip; the Lua SDK covers new apps (`lua.md`) |
| Camera seam (event vectors, head-turn correction on `updateChase` / `updateLook` / `lookLeftRight`, fade clamp, allow flags, FOV bonnet / bumper, orbit capture, start trajectories) | **M** for the C++ side; **XL** for the Lua scripts | cockpit / chase camera port exists inside the scene renderer (`docs/port/renderer_scene.md`) | head-turn input source (TrackIR / VR), the fade rules, the mode cycle (`nextMode` 0..4) | camera-matrix goldens per mode with the same replay (the `car_oracle` + `render_oracle` pair), scripts excluded |
| Freer camera module (native) | **M** | `CameraMouseControl` port | two settings sets, slow motion, stored points | feed a recorded mouse / key tape, compare camera matrices |
| Spline grid cache and `getLastIndexFromNorm` memo | **S** | AI port (`docs/port/ai.md`) | cache file format (`ai_grids/*.bin`, not read) is CSP's; rustyAC can use its own | indices equal to the original `closestPointIndex` for random points on `fast_lane.ai` |
| `lods.ini` renumbering event | **S** | `crates/rustyac-content` | none | compare LOD lists |
| CSP-own type-name rows (175) | **n/a** | - | no behaviour | - |

How to prove CSP-specific behaviour without CSP's source: run the real game twice (CSP enabled and `ENABLED=0`,
`general.ini [BASIC]`) with the same `race.ini` and record (a) the CSP log and (b) the three shared-memory pages; compare
only the fields the brief says change. Everything else in this file is "CSP does not change acs.exe's result".

---

## 8. Open questions and not mapped

1. **What the `INIReader::parse` custom pass really does** (`0x5ed9b0`, 909 B) and how the `data_override` / `skins` overlay
   (`0x5f2c50`, `0x5f30b0`, 1.1 KB each) resolves names (order versus `data.acd`).
2. **Who registers into the INIReader / INIReaderDocuments handler vectors** (`0x182690c40`, `c58`, `c60`, `c68`, `c70`)
   and what they return; I only saw the dispatch.
3. **Launch-struct byte 3 and the `0xef1700` object** (constructor `0xef1770`): byte 0 = headless (`ui_apps.md`), byte 3
   gates `Sim::renderHUD`, the embedded-default lookup and `Car::getSteerFF`; its source (CM special mode?) is unproven.
4. **`Game::run`'s frame body `0x181113720`, `Sim::update`'s `0x1811bcf00`, `Game::update`'s `0x1811bc4a0`**: not decompiled;
   the order of CSP's own steps inside the frame is not mapped.
5. **`[LOD_0] FILE` rule** (`0x5eec40`) and the exact condition of TLS byte `+0x1b3`.
6. **`SteamInterface` gate A versus B**: which switch picks the no-op branch (only the predicate `0x6d5630` was decoded).
7. **`NKUtils::getSectorID` replacement** (`0x6cc6c0`, `0x6cc7d0`): what "extended sectors" mean.
8. **`INIReader::hasSection("THREADING")` special case** and the `[PYTHON] ENABLE_PYTHON` return value (register not
   visible in the pseudo-C).
9. **Crash-report destination** after the "share" answer; `0x13cea80` only opens the reporter plug-in.
10. **`module_lw`**: no description string, only the type names.
11. The `;`-prefixed text branch of `FolderEncrypter::decryptFile` (`0x180a55c10`).
12. **`CameraOnBoardFree::update` reading `crew.ini`** (sections `HEADER`, `SIDE`): intent unclear.
13. The mode integers of `ACCameraManager::setMode` (9, 5, 3) and the `_DAT_1826add30` session-mode number (>9 = replay /
    cinematic) are inferred from use, not from a table.

## Evidence

* `re/scratch/csp1/hook_groups.tsv` (group `misc_core`, 337 rows), `csp_hook_table_raw.tsv`, `dll_strings.tsv`,
  `dll_func_strings.tsv`, `dll_funcs.tsv`, `dll_func_imports.tsv`, `acs_index/functions.tsv`.
* `decomp_hooks/{5ec420,5ecc00,5ed6d0,5edd90,5eeb50,5ef300,5f0650,5f1110,5f1930,5f1cf0,5f1eb0,5f9090}.c` (INIReader),
  `{5da1c0,5da110,5da470,5da7d0,5d7490,6c87d0,6c8810,6cba40,11a21c0,11c52c0,55ae00,55ae70}.c` (Path, FolderEncrypter,
  FileChangeObserver), `{11bb510,11bb9e0,11bd380,11bd420,11bd5f0,11bd660,11bd730,11bd820,11be250,11be370,11be4a0,11bce70,11bfa00,
  11bfcd0,11bfe30,11beae0,11bc930,11bd8b0,11bf830}.c` (Sim), `{1111e00,11127c0,1112e60,11130f0,11132d0,11141c0,11141d0,
  1113350,117a740,117a900,5dc7c0}.c` (Game, GameObject, GameTime), `{5fe700,5fe8c0,5fe9d0,5ffc60,5ffcc0,5ffcd0,5ffcf0,
  5ffd70,5ffe40,600530,600a20,6a2a90}.c` (kml), `{2f4d70,69feb0,6a0000,6a0160}.c` (shared memory), `{6f5100,6f5210,6f51c0,
  6d5660}.c` (Steam), `{10e6f30,10ea230,10ea860,10e7040,691d30,693580,10e53a0}.c` (Python), `{107b160,107b210,107b240,
  107b310,107b470,107b4a0,108f450,108f870,108fa40,108fb90,108fc80,108fd50,10a21d0,10a2750,594370,594560,5949f0,1090480,
  1090e20,1090e60,58f5c0,58f620,58fab0,592630,5a6700}.c` (cameras), `{5a5c80,5a5d20,5a6980,5a6990,6bf260,6bf270,6238b0,
  6ccae0,428cd0,6cf630,6915a0}.c`; `decomp_req1/{1387350,5f0170}.c` (settings loader, extended flag setter).
* Hand-decoded bytes of `dwrite_copy.dll`: predicates `0x69fe80`, `0x6d5630`, `0x2f4d70`, the static getter `0xef1700`,
  wide strings at `0x221c5d8`, `0x221c600`, `0x2212088`, `0x221c610`, `0x221c6a0`, `0x221c738`, `0x221c790`, `0x221c7e8`,
  `0x221c8e0`, `0x221c900`, `0x221c958`, `0x221c980`, `0x221cd98`, `0x222d660`, `0x222d6c0`, `0x221bdb0`, `0x221b940`,
  `0x221d438`, `0x2225930`.
* Shipped files: `extension/config/{general,chaser_camera,freer_camera,neck,spectating,small_tweaks,taskbar,nice_screenshots,
  music,module_lw,lua_tools,data_alt_mapping}.ini`, `extension/lua/{chaser-camera,cockpit-camera,loading-screen,tools}`,
  `extension/internal/python/{initialize.py,functions.txt,traceback_helper.py}`, `extension/internal/lua-sdk/{ac_chaser_camera,
  ac_cockpit_camera,ac_splashscreen}`, `<AC>/cache/` listing, owner's `Documents\Assetto Corsa\cfg\extension\*.ini`,
  `logs\custom_shaders_patch.log`.
* Earlier briefs: `attach.md` 1.7, 1.8, 2; `physics_documented.md` 2.2; `physics_car_hooks.md`; `lua.md`; `ui_apps.md` 2.0;
  `rendering_content.md` 3.1; `online.md`. acs.exe side: `docs/atlas/{config,camera,main_loop,python_api,misc}.md`,
  `docs/map/telemetry.md`, `docs/atlas/replay.md`.

## Decompile requests

1. `0x1811bcf00` (Sim::update CSP pipeline): which CSP steps run per frame and where the original `Sim::update` is called.
2. `0x181113720` (frame body of `Game::run`): order of the profiler zones, where the frame limiter (`0x5dc910`) is applied.
3. `0x1811bc4a0` (Game::update pre-pipeline).
4. `0x1805f45f0`, `0x1805f4770`, `0x1805f4820`, `0x1805f48f0`: getInt / getFloat / getVector dispatch tails (handler lists, original call).
5. `0x1805ed9b0` (INIReader::parse custom pass) and `0x1805f3db0`.
6. `0x1805eec40` (`LOD_0` / `FILE` rule) and `0x1805eebf0` (empty-string test).
7. `0x1805f2c50`, `0x1805f30b0` (`data_override`), `0x1805f34f0`, `0x1805f56a0` (data-file name table).
8. `0x1805d7a60`, `0x1805da2a0`, `0x1805d9c60`, `0x180529b40`, `0x180529930` (virtual folder table, "nul" car).
9. `0x180a55c10` (the `;`-text preprocessing in `decryptFile`).
10. `0x1805f01f0` (bad-value pop-up: "OFFER:", `steam://validate/244210`).
11. `0x180ef1770` (launch-options constructor behind `0xef1700`): which fields are bytes 0..5.
12. `0x1806cc6c0`, `0x1806cc7d0` (`NKUtils::getSectorID` replacement).
13. `0x1806d0830` (shutdown watchdog thread, `AcTools.CSP.ShutdownProgress.v0`).
14. `0x18055b060` and closure `0x18055b1e0` (files-monitoring registration).
15. `0x1805dc4e0`, `0x1805dc910` (frame limiter, waitable timers).
16. `0x18107cd30`, `0x18107bc90` (detours of `PSSetShader` and `ResolveSubresource` on the immediate context).
17. `0x180b503b0`, `0x180b9daa0` (loading-screen pump).
18. `0x18107b500`, `0x18108fdf0`, `0x1810a2750`'s helper `0x18112bf90` (camera registration and shake configuration).
19. `0x1805fe470` (alternative spline index lookup), `0x1805ff6f0` (grid build / load with cache path), `0x1805feb30` (spline reversal).
20. `0x18139bec0` neighbour and `0x180594560` internals reading `crew.ini` in `CameraOnBoardFree::update`.
21. `0x180ed5710` (predicate of the Steam gate), `0x1806cc6c0`.
22. `0x13cea80` tail (where the crash report is sent after "share").
23. `0x1805ed680` (video.ini path predicate) and `0x1810ead20` (post-constructor of PythonInterface).
