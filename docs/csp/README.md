# Custom Shaders Patch (CSP) 0.3.0-preview649 — atlas

A map of CSP, the third-party mod that most of today's Assetto Corsa content is built for, written
for rustyAC. It is a **map, not a port**: what CSP is, how it attaches to `acs.exe`, which game parts
it replaces, which keys and Lua functions it exposes, which of the owner's installed content needs
it, and in which order and how it could be rebuilt and proven. Style and depth follow
[`docs/atlas/`](../atlas/README.md).

Read-only work: nothing in the game folder, `Documents\Assetto Corsa`, the Ghidra project of
`acs.exe` or a running game was changed. `dwrite.dll` was studied on a copy.

## Resume here

> Updated after each commit. Branch `csp` (worktree `rustyAC-csp`), docs only.

| Step | State |
|---|---|
| README, attach and hook mechanism | done: [`attach.md`](attach.md) |
| Physics extensions | done: overview + 4 detail files |
| `content_needs.md` | done (15 cars with `VERSION=extended-N`, 6 extended track layouts, 1 encrypted car); re-check after the rendering files |
| Lua | done: [`lua.md`](lua.md) |
| Rendering | in progress (config, core hooks, content hooks being written) |
| UI / apps, online, audio, input / FFB, VR / screens, misc | in progress |
| `plan.md` | not started |

Scratch (not committed, gitignored): `re/scratch/csp1/` — index tables of `dwrite.dll`, decompiles of
its replacement functions, the briefs the area files were written from.

## 1. What CSP is

- A **DLL proxy**: the package puts `dwrite.dll` (139,567,496 bytes) next to `acs.exe`. Windows
  loads it instead of the system DirectWrite DLL, because `acs.exe` imports `DWriteCreateFactory`.
  The DLL exports the same function and forwards it to the real system DLL; everything else it does
  happens inside the game process.
- An **`extension/` folder** beside it: default configs, Lua scripts and libraries, weather and
  weather controllers, audio banks, textures, track "VAO" patches, and the Lua SDK definitions.
- Build **0.3.0-preview649, build 4184** (the one rustyAC pins; older packages for comparison are in
  `ignored/CSP`: 0.2.11 and 0.3.0-preview212). Its export table names it `accRenderingAdv.release.dll`.
- Closed source for the engine; **public** are the Lua SDK definitions, the ini defaults with their
  comments, the weather/controller Lua, the shader sources (a public repository the embedded shader
  archive points at) and the embedded RainFX configs.

## 2. How it attaches to `acs.exe` (summary; details in [`attach.md`](attach.md))

- It resolves `acs.exe` functions **by name from `acs.pdb`** (the Kunos symbol file that ships in the
  game folder) through `dbghelp.dll`. It refuses to start on an `acs.exe` other than 1.16.3 / 1.16.4
  (64-bit) or without `acs.pdb`. The log line is "All 1542 functions were found successfully".
- 1,441 registration records name **1,368 distinct acs.exe symbols**: **1,026 are hooked** (CSP puts
  a replacement in front; it calls the original through a saved pointer when it wants to) and **415
  are looked up** (CSP only calls or reads them). The first extraction counted 934 / 507: 92
  registration stubs load the replacement into a register *before* the name, which the first scan
  missed; the correction is in [`attach.md`](attach.md). 1,215 of the 1,377 `Class::method`-shaped
  strings in the DLL exist in `acs.exe`; the others are CSP's own type names.
- The hooked functions cover physics (`ksPhysicsAC.lib` 155), the renderer and scene graph
  (`KunosSim.lib` 156, `kgl.lib`, `CarAvatar`, `TrackAvatar`, cameras), apps and GUI (`SystemApps.lib`
  53, `ksGUI.lib` 29), audio (`ksAudioFMOD.lib` 34, `CarAudioFMOD`), network (`ACClient`), input
  (`ksDI.lib`), replay, race logic, Python, Steam, shared memory and more.

## 3. What is inside the 139 MB

| Part | Where | Size / count |
|---|---|---|
| Code (`.text`) | image | 31 MB, 77,058 functions (from the exception table) |
| Read-only data, data, resources | image | `.rdata` 7 MB, `.data` 2.2 MB |
| **Shaders archive** (precompiled DXBC, hashed names) | overlay ZIP 1 | 6,683 files, 123 MB unpacked, 31 MB packed; the source is public (the archive comment names the repository) |
| Default AC cfg ini files (`ac/*.ini`) | overlay ZIP 2 | 50 entries |
| RainFX per-track configs ("packed for online verification") | overlay ZIP 3 | 101 entries |
| Lua libraries, precompiled with LuaJIT 2.1.1711381957 | overlay ZIP 4 | 135 entries, 3.0 MB unpacked |
| Fonts (CJK, historical, living) | overlay ZIP 5 | 3 TTF, 26.7 MB unpacked |
| Cover images | overlay ZIP 6 | 7 |
| Mirror of the `extension/` defaults | overlay ZIP 7 | 166 entries, 117 MB unpacked |
| Third-party code linked in | image | OpenSSL, Intel Embree (BVH ray casting), an ODE build with libccd, LuaJIT, AMD FSR 2 / 3 (28 exports), NVIDIA NGX / DLSS, an immediate-mode GUI, FXAA |

None of the ZIP members is flagged as encrypted. `extension/shaders.zip` and `extension/internal/lua.zip`
on disk are 172-byte placeholders ("actual content has been moved to dwrite.dll"). The two Lua files
`extension/internal/patch.lua` and `user.lua` are obfuscated loaders; they were **not** analysed.

## 4. Folder layout (`<AC>/extension/`)

| Folder | Files / size | What |
|---|---|---|
| `config/*.ini` | 57 files (66 top-level files incl. data lists), ~400 kB | default settings per module (`general.ini` 44 kB, `graphics_adjustments.ini`, `vr_tweaks.ini`, `lighting_fx.ini`, `weather_fx.ini`, `rain_fx.ini`, `tyres_fx.ini`, `surfaces_fx.ini`, `new_behaviour.ini`, `gui.ini`, ...). The `;` comments on each key are the documentation |
| `config/cars/{generic,common,loaded}` | 3 + 28 + 8 ini (+ 1 Lua) | per-car configs: `generic.ini` for every car, shared includes (materials, lights, tyre damage, navigators), and the ones Content Manager downloaded for installed cars |
| `config/tracks/{generic,common,loaded}` | 1 + 12 + 11 ini (+ 3 lut, 1 Lua, 23 kn5) | same for tracks, plus extra light / glass models |
| `internal/lua-sdk/` | 19 modules, 41 files, 24 MB | EmmyLua definitions + generated README of the Lua API per script kind |
| `internal/lua-shared/`, `lua-apps/` | 89 + 24 files | shared libraries, CSP's own apps (CSP Debug, AppShelf, Mumble) |
| `internal/plugins/` | 14 files, 8.8 MB | helper executables and libraries (gamepad server, Mumble client, Wooting, OpenRGB, text-to-speech, error reporter, libsqlite3, libopus, openxr_loader, libgns) |
| `lua/` | 355 files, 6.7 MB | `cars` 60, `new-modes` 102, `tools` 111, `pp-filters` 21, `fireworks` 18, `joypad-assist` 15, `chaser-camera` 9, `mouse-steering` 9, `ffb-postprocess` 5, `cockpit-camera` 3, `loading-screen` 2 |
| `weather/`, `weather-controllers/` | 57 + 13 files | WeatherFX types and the controller Lua (`base`, `base-live`, `base-schedule`) |
| `sfx/` | 7 files, 23 MB | FMOD banks `csp_base`, `csp_extras`, `csp_extras_rain`, `csp_extras_sfp`, `csp_surface_gravel`, `csp_surface_ice` + `GUIDs.txt` |
| `vao-patches/`, `vao-patches-cars/` | 11 + 8 files, 31 MB | precomputed ambient-occlusion patches for tracks and cars |
| `textures/`, `backgrounds/` | 35 + 11 files | UI and effect textures; launcher backgrounds |
| `installed.log`, `shaders.zip` | | install manifest; placeholder |

## 5. How settings flow

1. **Defaults**: `extension/config/<module>.ini` (also embedded in the DLL).
2. **User overrides**: `Documents\Assetto Corsa\cfg\extension\<module>.ini`, key by key. A missing
   key or file means the default; Content Manager writes only keys that differ. The owner's
   files change only visual keys (earlier survey).
3. **Per car**: `extension/config/cars/generic/generic.ini` (all cars), then
   `config/cars/loaded/<car>.ini`, then `content/cars/<car>/extension/ext_config.ini`.
4. **Per track**: `config/tracks/generic/generic.ini`, `config/tracks/loaded/<track>.ini`,
   `content/tracks/<track>/extension/ext_config.ini`.
5. **Physics switches** live in the content's own data files: `car.ini [HEADER] VERSION=extended-N`,
   `surfaces.ini WAV_PITCH=extended-N`, `[_EXTENSION]` sections of `suspensions.ini` / `brakes.ini`,
   `TYPE=COSMIC`, `data/script.lua` — see [`physics.md`](physics.md).
6. **Launcher side channel**: `race.ini` keys written by Content Manager (`__CM_*`, `[__EXT_*]`),
   `data_manifest.ini` lists them.
7. **State**: `Documents\Assetto Corsa\cfg\extension\state\` (odometers, app windows).
8. CSP writes `Documents\Assetto Corsa\logs\custom_shaders_patch.log` every session (format "will
   change"); it is the best free debug output.

## 6. Index of the areas

Filled in as the area files are written; sizes are hooked functions (H) / looked-up (L) in `acs.exe`,
config files, Lua files and functions; difficulty S (smallest) to XL, a relative guess.

| Area | File | Status |
|---|---|---|
| Attach and hook mechanism | [`attach.md`](attach.md) | done |
| Physics extensions | [`physics.md`](physics.md) (+ 4 detail files) | done |
| Lua | [`lua.md`](lua.md) | done |
| Rendering | [`rendering.md`](rendering.md) | pending |
| UI and apps | [`ui_apps.md`](ui_apps.md) | pending |
| Online | [`online.md`](online.md) | pending |
| Audio | [`audio.md`](audio.md) | pending |
| Input and FFB | [`input_ffb.md`](input_ffb.md) | pending |
| VR and screens | [`vr_screens.md`](vr_screens.md) | pending |
| Misc | [`misc.md`](misc.md) | pending |
| Content needs | [`content_needs.md`](content_needs.md) | done |
| Plan | [`plan.md`](plan.md) | pending |
