# Lua: runtime, script kinds, API surface, sandboxing

CSP embeds its own LuaJIT host in `dwrite.dll` and uses it for everything that is "content logic": Lua apps, new game
modes, Objects-Inspector tools, weather styles and controllers, cameras, FFB / gamepad post-processing, car and track
displays, car physics scripts, post-processing filters, the loading screen, fireworks, server-supplied scripts, and the
expression language inside `ext_config.ini` (INIpp). The public face is a set of 19 EmmyLua definition modules
(`extension/internal/lua-sdk/<module>/lib.lua`); the implementation is a native bridge plus 135 precompiled Lua
libraries embedded in the DLL. The host is **not** attached to `acs.exe` by hooks (0 hooks have a Lua or Script name);
it is CSP's own subsystem, entered from the hooked game loop.

Markers: `[sdk]` Lua SDK text, `[lua]` shipped Lua source, `[ini]` shipped ini, `[dll]` dwrite.dll string / table /
decompile (RVA, image base 0x180000000), `[log-csp]` `custom_shaders_patch.log` of the 2026-10-12 session, `[prior]`
earlier brief (`physics_documented.md`, `physics_car_hooks.md`, `attach.md`), `[scan]` `content_scan.json`,
`[interp]` my inference. Sections of the other briefs are cited by name.

## 1. What it does

* **One Lua VM per script instance.** Every script (an app window set, a mode, a weather style, a car display, a physics
  script, ...) runs in its own `lua_State` created from one of 19 "environments" (the SDK modules). Environment =
  global tables (`ac`, `ui`, `physics`, `render`, `io`, `os`, `web`, `display`, `loading`, `worker`, `script`, `car`,
  `sim`, `ffi`, `JSON`, `stringify`, vector / colour / matrix types, `class`) restricted to what that kind may do
  (3.9). `[sdk]` `[dll]`
* **Scripts are found by naming convention.** A module's ini names an `IMPLEMENTATION=<folder>`; the loader looks for a
  fixed file name in that folder (`camera.lua`, `cockpit.lua`, `ffb.lua`, `assist.lua`, `filter.lua`, `fireworks.lua`,
  `splashscreen.lua`, `weather.lua`, `controller.lua`, `mode.lua`, `tool.lua`, `<App>.lua` + `manifest.ini`). Car and
  track configs name a script per section with `SCRIPT=`. `[ini]` `[dll]` (3.2)
* **Most of the visible API is Lua over a native bridge.** The embedded bundle holds 21 environment libraries
  (`ac_common_base`, `ac_common`, 19 kinds; 2,663,440 bytes of bytecode) + 23 `lib_*` + 89 `sh_*` (the 89 files of
  `extension/internal/lua-shared/`). 27 of the 89 shared libraries are thin shells that call natives by name
  (172 `__util.native` call sites; most are bare `__util.native()` stubs that bind to a C++ function of the same
  name). `[dll]` `[lua]`
* **Lua is also the config language.** `ext_config.ini`, `config/cars/*`, `config/tracks/*` use INIpp: `$" expr "`
  Lua expressions, `[FUNCTION: name] CODE='...'`, `[INCLUDE: ...]`, `@ACTIVE`, `[MIXIN: ...]`. On this PC: 725
  expressions in 43 of the 181 ini files under `extension/config` and the cars' / tracks' `extension` folders; 26
  `[FUNCTION:]` sections in 11 shipped files. `[ini]` `[scan]` (3.8)
* **By default on this PC** (user ini files are empty, so shipped defaults apply): `[log-csp]` shows the internal
  "CSP module" script (twice: before and after sim init), Weather FX style + controller (Lua), and 8 Lua apps
  (3 internal: AppShelf, CspDebug, Mumble [not available offline]; 5 in `apps/lua`). Camera / FFB / gamepad / filter /
  loading-screen scripts are off (`ENABLED=0`); fireworks `ENABLED=1, BEHAVIOUR=2` (holidays only).

## 2. Game parts it replaces or extends

### 2.1 Hooks

`csp_hook_table.tsv` has **no hook whose name contains Lua or Script** (grep `lua|script` over the symbol column: 5
rows; 3 are false hits `FuelLapEvaluator::step`, `TrackDescription::TrackDescription`,
`TrackAvatar::getSectorDescription`; 2 are the same CSP-own type `lua::known_executables::parsed`, kind lookup,
registered at 0xfe4d61 and 0x1dc19f2). `hook_groups.tsv` has no Lua group. The Lua host is driven from places other
briefs own. Known entry points into the Lua core from hooked game code:

| Game function (acs.exe VA) | CSP hook RVA | What it does for Lua | Source |
|---|---|---|---|
| `Car::pollControls` (0x140274e70) | 0x59fe60 (alt. 0x59fdb0) | runs the car physics script every 3 ms step via 0x856ed0 -> 0x1373420 (load) -> 0x1375740 (call) | `physics_car_hooks.md` section 2c [dll] |
| decompiles in `decomp_hooks/` that reference the Lua core (0x136xxxx..0x138xxxx) | only 0x59fdb0 | the other 868 hook decompiles never call the Lua core directly (4 further hits call 0x1385550, a 101-byte random-number-like helper [interp], not Lua) | scan of `decomp_hooks` [dll] |

Everything else (UI frame -> app windows, render passes -> `script.renderSky`, replay -> `ac.onReplay`, sim update ->
`script.update`) goes through CSP's own code that I could not tie to a hook name: see section 8.

### 2.2 Lua host functions found in the DLL (not hooks; from string references)

| RVA | Size | What (from its strings) |
|---|---|---|
| 0x136f620 | 4,691 | script-environment setup: 52 string references, about 40 of them native helper names (`getFolder`, `dirname`, `findFile`, `parseINIppFile`, `readDataFile`, `uniqueMachineKey(Async)`, `loadDLL`, `addDLLDirectory`, `hostNamedPipe`, `openSerialPort`, `openAudioOutput`, `showMessage`, `setCurrentFolder`, `openURL`, ...), sets `package.path` patterns `<dir>/?.lua` and `<AC>/extension/internal/lua-?.lua`, and the per-script fields `__dirname`, `__mode__`, `__allowIO__` |
| 0x1373420 | 2,819 | load / reload a script ("Loading Lua script [", "Reloading Lua script [", "Delaying loading of Lua script ") |
| 0x13743f0 / 0x1374a50 / 0x1374cb0 | 1,627 / 606 / 694 | asynchronous loading in two parts ("Async loading Lua script [... , p0 / p1 / sync part]") |
| 0x1372bd0 | 1,363 | "Failed to initialize preprocessor" (the Lua preprocessor, hidden key `FORCE_LUA_PREPROCESSOR`) |
| 0x1373f30 | 454 | "Delayed initialization: <script>" |
| 0x136d110 | 2,263 | per-kind plug-in factory: `soft_lock`, `chaser_camera`, `pp_filters`, `joypad_assist`, `fireworks`, `cockpit_camera`, `wfx_impl`, `__settings.ini`; prints "Lua: <name> is changed" (file monitoring) |
| 0x136e370 / 0x136e9a0 | 1,572 / 1,129 | "Slow script! %s/%s took %d ms" (profiler hook) / "Failed to process:" |
| 0x136b060 / 0x136bb10 | 701 / (n/a) | "Lua state is damaged ...", "error in error handling (might be a stack overflow ...)" |
| 0x1371b30 | 483 | fatal handler: "Fatal Lua crash from %s: %s", "Check your Lua scripts, disabling one of them should help", button "Disable broken script" |
| 0x137f5f0 | 220 | "Fatal Lua failure: %s" |
| 0x13768c0 | - | references `__monitored` (script viewed in the Lua Debug app) |
| 0x137de10 | 648 | "Failed to load lazy module" |
| 0x11eb6a0 | 1,319 | "Lua syntax error: " / "Lua runtime error: " (INIpp / config side) [interp] |
| 0xf77150 | 655 | background worker thread: name "CSP: Lua background worker (%d)", fields `__input`, `@worker`, "Parent disposed" |
| 0xc50930 | 2,373 | car script loader: "Car script: %s:%d", "Car display: %s:%d [%s]", keys under `extension` |
| 0x530f10 / 0x530a50 | 674 / 1,084 | script-type test: strings `car_script`, `car_scriptable_display`, "Currently, not available to this type of script" |
| 0xcea750, 0xcf8340, 0xcf0800 | 1,610 / 220 / 11,269 | Lua apps: manifest parse, "Loading app: ", windows (the 11 KB function references 30 app strings) |
| 0xfd3190, 0xfd40f0, 0xfd3a00, 0xfd1b60, 0xfd19a0, 0x3027f0 | 372 / 1,578 / 1,770 / 868 / 446 / 1,902 | known-executables gate for `os.runConsoleProcess` / `os.openURL`; 0x3027f0 fetches the rule set `https://acstuff.club/j/known.dat` |
| 0x5f2c50, 0x5f30b0 | - | `data_override` / `extension/data_override` virtual data overlay (cars) |
| 0xe21263 | - | references `reloadScript` |
| 0xe25eb0 | 336 | loads `lua-module\module`, "CSP module script is missing" |

RTTI type names (Microsoft `.?A` names in `.data`) that identify the host: `lua::base_limp_thread`, `lua::limp_thread`,
`lua::limp_physics_thread`, `lua::base_script_data`, `lua::lua_updateable`, `lua::lua_storage`,
`lua::app_script_settings`, `lua::lua_custom_lookup`, `lua::lua_argument_error`, `lua::lua_immediate_return`,
`lua::lua_shader_library`, `lua::car_acd_lookup`, `lua::kn5_loader`, `lua::fake_shadow`, `lua::lua_gif_{gif,webp,impl}`,
`lua_app::{lua_window, lua_app_data, lua_app_service}`, `ac_ext::server_lua_script`,
`ac_ext::car_scriptable_display_impl::limp_impl`, `ac_ext::track_script::limp_impl`,
`ac_ext::track_scriptable_display::limp_impl`, `ac_ext::data_ac_ext_{chaser_camera,gamepad_fx,neck,new_modes,
small_tweaks,weather_fx,splashscreen}::limp_impl` (`weather_fx` as `wfx_limp`), `ac_ext::pp_filters_data::limp_impl`,
`ac_ext::fireworks_state::limp_impl`, `apps::lua_tool::limp_impl`, `apps::app_lua_debug_impl`,
`cphys_script::cphys::impl::cphys_script_limp`. "limp" is the host's name for one Lua instance [interp: "Lua
IMPlementation"]. `[dll]`

## 3. Config keys and Lua API

### 3.1 Runtime keys (`extension/config/general.ini`, defaults) `[ini]`

| Key | Default | Meaning |
|---|---|---|
| `[DEV] WATCH_FOR_CHANGES` | 1 | watch configs (and scripts, 0x136d110: "Lua: ... is changed") for changes |
| `[DEV] USE_PARTIAL_RELOAD` | 1 | partial reload for car / track configs |
| `[DEV] SILENT_LUA` | 0 | stop Lua scripts from logging unless the Lua Debug app was used in a few previous sessions |
| `[DEV] KEEP_BACKGROUND_WORKERS_AROUND` | 0 (0..60 s) | keep finished background workers listed in Lua Debug |
| `[DEV] LUA_APPS_DIR` | `lua` (hidden) | sub-folder of `<AC>/apps` scanned for Lua apps |
| `[DEV] COUNT_LUA_MEMORY` | 0 (hidden) | count Lua memory |
| `[DEV] FORCE_LUA_PREPROCESSOR` | 1 (hidden) | run the Lua preprocessor on every script |
| `[DEV] NANORT_SCENE_RAYCAST` | 2 | accelerated mesh-ray tests used by the Lua API (about 25 MB RAM per 1 M triangles) |
| `[CONFIGURATIONS] BLOCK_UNTRUSTED_NETWORKING` | 0 | block car and track scripts from accessing the web (breaks Android Auto) ([dll]: the key name is not present as a plain string) |
| `[CONTROL_MOUSE_STEERING] USE_SCRIPT`, `IMPLEMENTATION` | 0, `base` | Lua mouse-steering script (`extension/lua/mouse-steering`) |
| `gui.ini APP_LUA_DEBUG` | 1 | the native "Lua Debug" app (`app_lua_debug`) |
| `gui.ini APP_APPSHELF` | 1 | App Shelf (Lua app that installs apps) |
| `lua_tools.ini [BASIC] ENABLED` | 1 | Lua tools in the Objects Inspector |
| `[__DEBUG] DUMP_FLATTEN_INI` | (car/track ini) | writes `<ini>-flatten.txt` with every INIpp expression resolved [dll 0x22b9190..] |

### 3.2 How each kind is found and selected `[ini]` `[dll]` `[lua]`

| Kind (SDK module) | Selected by | Entry file / manifest | Per-implementation settings state |
|---|---|---|---|
| Lua app (`ac_apps`) | folder present | `<AC>/apps/lua/<App>/manifest.ini` + `<App>.lua` (also `<AC>/extension/internal/lua-apps/<App>`) | `cfg/extension/state/lua/app/<App>.ini` |
| New mode (`ac_new_modes`) | `race.ini [RACE] __CM_CUSTOM_MODE` (read by the shipped manifests) [interp] | `extension/lua/new-modes/<mode>/manifest.ini` + `mode.lua` (+ `settings.ini`, `description.txt`) | script settings via `ac.INIConfig.scriptSettings()` |
| Tool (`ac_tools`) | `lua_tools.ini ENABLED` | `extension/lua/tools/<tool>/manifest.ini` + `tool.lua` | - |
| WFX style (`ac_wfx_impl`) | `weather_fx.ini [BASIC] IMPLEMENTATION=base` | `extension/weather/<name>/manifest.ini` + `weather.lua` (+ `settings.ini`, `src/`, `shaders/`) | `state/lua/wfx_impl/<name>__settings.ini` |
| WFX controller (`ac_wfx_controller`) | `weather_fx.ini [BASIC] CONTROLLER=sol` (default names a controller that is not installed here) | `extension/weather-controllers/<name>/manifest.ini` + `controller.lua` | - |
| Chase camera (`ac_chaser_camera`) | `chaser_camera.ini [BASIC]` and `[BASIC_2]` `ENABLED` (0), `IMPLEMENTATION=base` | `extension/lua/chaser-camera/<name>/camera.lua` | `state/lua/chaser_camera{,_2}/<name>__settings.ini` |
| Cockpit camera (`ac_cockpit_camera`) | `neck.ini [SCRIPT] ENABLED=1` but `[BASIC] ENABLED=0`; `IMPLEMENTATION=default` | `extension/lua/cockpit-camera/<name>/cockpit.lua` | `state/lua/cockpit_camera/...` |
| FFB post-process (`ac_ffb_postprocess`) | `ffb_tweaks.ini [POSTPROCESSING_SCRIPT] ENABLED=0`, `IMPLEMENTATION=alternative` | `extension/lua/ffb-postprocess/<name>/ffb.lua` | `state/lua/soft_lock/...` |
| Gamepad / mouse assist (`ac_joypad_assist`) | `gamepad_fx.ini [JOYPAD_ASSIST] ENABLED=0`, `IMPLEMENTATION=gamepad_force`; `general.ini [CONTROL_MOUSE_STEERING] USE_SCRIPT=0`, `IMPLEMENTATION=base` | `extension/lua/joypad-assist/<name>/assist.lua`, `extension/lua/mouse-steering/<name>/assist.lua` | `state/lua/joypad_assist/...`, `mouse_steering/...` |
| PP filter (`ac_pp_filters`) | `graphics_adjustments.ini [SCRIPTABLE_FILTER] ENABLED=0`, `IMPLEMENTATION=tunnel` | `extension/lua/pp-filters/<name>/manifest.ini` + `filter.lua` (+ `filter.fx`) | `state/lua/pp_filters/...` |
| Fireworks (`ac_pfx_fireworks`) | `particles_fx.ini [FIREWORKS] ENABLED=1`, `BEHAVIOUR=2`, `IMPLEMENTATION=holidays` | `extension/lua/fireworks/<name>/fireworks.lua` | `state/lua/fireworks/...` |
| Loading screen (`ac_splashscreen`) | `splashscreen.ini [SCRIPT] ENABLED=0`, `IMPLEMENTATION=default` | `extension/lua/loading-screen/<name>/splashscreen.lua` | `state/lua/loading-screen/...` |
| Car display / car script (`ac_car_scriptable_display`) | car or CSP car config sections `[SCRIPTABLE_DISPLAY_...]` and `[SCRIPT_n]`: keys `ACTIVE`, `ACTIVE_FOR_PLAYER_ONLY`, `ACTIVE_FOR_UNFOCUSED`, `ACTIVE_FOR_LOD`, `ACTIVE_FOR_NEAREST`, `MESHES`/`MATERIALS`, `SCRIPT=` (file relative to `extension/`, or inline `'...'`), `DISPLAY_POS/SIZE`, `RESOLUTION`, `KEEP_BACKGROUND`, `SKIP_FRAMES`, `INTERACTIVE`, `FORCE_UPDATE_AFTER`, `BIND_TO` | `content/cars/<car>/extension/<path>.lua`; shared: `extension/lua/cars/<name>/script.lua` | `state/lua/shared-<carID>` (script has no IO) |
| Car physics script (`ac_car_cphys`) | car `data/script.lua` (+ `script_*.lua`, optional `script_render.lua` via `ac.loadRenderThreadModule`); gate `ALLOW_CPHYS_SCRIPT` [dll 0x2242b28] | in `data.acd` | same shared state |
| Track script / display (`ac_track_script`) | track CSP config `[SCRIPT_n]`, `[SCRIPTABLE_DISPLAY_...]`; physics-affecting parts need `surfaces.ini [_SCRIPTING_PHYSICS] ALLOW_TRACK_SCRIPTS=1` | `content/tracks/<t>/extension/`, `extension/config/tracks/loaded/<t>.ini`, `extension/lua/tracks/*` | - |
| Online script (`ac_online_script`) | supplied by the server (type `ac_ext::server_lua_script`, thread "CSP: updating server script"); rules.json folder `extension/lua/online/*` | not shipped | - |
| Background worker (`ac_background`) | `ac.startBackgroundWorker(script, data, cb)` | file next to the caller (name without `.lua`) or code string | - |
| Physics worker (`ac_background_pw`) | `physics.startPhysicsWorker(script, data, cb)` | same | - |
| Scripted texture (`ac_scripted_texture`) | [dll] strings only: "Creating Make-Script texture", `scripted_texture`, cache `made_textures`, shader `accMakeTexRgba_ps.fxo` | no shipped example, reference syntax not found | `cache/` DDS |

`<AC>` state root for the `state/...` column is `Documents/Assetto Corsa/cfg/extension/` (the `__PLUGIN_SETTINGS=<key>,
extension/state/lua/<kind>/{0}__settings.ini` hidden keys). `[ini]` `[lua]` on this PC: `state/lua/app/{AppShelf,Radar,
vrc_advanced_setup}.ini`, `state/lua/wfx_impl/base.ini`, `state/lua/shared-vrc_formula_alpha_2025_csp`, `...2026_csp`.

### 3.3 Entry points, callbacks, frequency, thread `[sdk]` `[lua]`

| Kind | Entry points (fields of the global `script` table unless noted) | When / how often | Thread |
|---|---|---|---|
| app | manifest-named `FUNCTION_MAIN`, `FUNCTION_SETTINGS`, `FUNCTION_MENU`, `FUNCTION_ON_SHOW`, `FUNCTION_ON_HIDE`, `FUNCTION_ICON` (`script.windowMain` ...), optional `script.update(dt)`, `[SIM_CALLBACKS] WORLD_UPDATE / FRAME_BEGIN / UPDATE / PREPROCESS`, `[UI_CALLBACKS] IN_GAME`, `[RENDER_CALLBACKS] TRANSPARENT`, `[SERVICE] SCRIPT + PERIOD` | window functions each UI frame while the window is visible; `LAZY` NONE/PARTIAL/FULL decides load and unload; `[AWAKE_TRIGGERS] EVENT / BUTTON / WINDOW` wake a lazy app | main (UI) thread; the service script has its own state (`__worker_finalize`) |
| new mode | `prepare(dt)` (returns true when ready; only if `ac.setStartMessage()` was called), `update(dt)`, `draw3D()`, `drawUI()` | each frame | main |
| tool | `update`, `asyncUpdate(dt)`, `simUpdate(dt)`, `frameBegin(dt, gameDT)`, `draw3D`, `onShow`, `onHide`, `onReplayStart`, `onReplayStop` | frame; `asyncUpdate` right after `update` on another thread ("whole AC would pause waiting for it") | main + 1 async |
| track script | `update(dt)` (for `[SCRIPT_n]` at sim update, for displays at display update), `draw3D`, `frameBegin`, `onReplayStart/Stop` | frame | main |
| online script | `update`, `draw3D`, `drawUI`, `frameBegin` | frame | main |
| car display / car script | `update(dt)` (per `[SCRIPTABLE_DISPLAY_...]` / `[SCRIPT_n]`, subject to `ACTIVE_FOR_*`, `SKIP_FRAMES`) | frame; only cars near the camera when `ACTIVE_FOR_LOD / NEAREST` | main (render-thread module via `ac.loadRenderThreadModule`) |
| car physics | `update(dt)`, `reset()` | every physics step, dt about 0.003 s (333 Hz); `ac.pauseScriptUntil(...)` skips updates | physics thread (`limp_physics_thread`) |
| physics worker | `update` via `script.update`, `worker.terminate()`, `worker.input`; "run at 333 frames a second", "very limited physics API" | physics rate | physics thread |
| background worker | runs once; `worker.sleep(t)`, `worker.wait(timeout)`, `worker.result`, `os.sleep` | VM start about 10 ms; worker threads created low priority; not auto-reloaded; reloaded with the parent | own thread "CSP: Lua background worker (n)" |
| chase / cockpit camera | `update(dt)`; `update(dt, mode, turnMix)`; `onCarChanged()` | frame | main |
| FFB post-process | `update(ffbValue, ffbDamper, steerInput, steerInputSpeed, dt) -> (value, damper)` | each physics frame | physics-side [interp] |
| gamepad / mouse assist | `update(dt)` (physics rate, about 0.003 s); `ac.loadRenderThreadModule(name)`; `ac.loadGamepadThreadModule(name)` (works while paused) | physics / gamepad / render | 3 modules |
| PP filter | `update(dt)` must draw the frame (`ui.drawImage('dynamic::screen', ...)`) | each frame | render |
| WFX style | `update`, `asyncUpdate`, `renderSky`, `renderClouds`, `renderTrack`, `renderSceneEnd`, `renderCloudShadows` (enabled by `ac.enableRenderCallback()`), `frameBegin` | frame / render passes | main |
| WFX controller | `update(dt)` | frame | main |
| fireworks | `update(dt)` | frame | main |
| loading screen | `update(dt)` + read-only `loading` table (`status`, `progress`, `carID`, `trackID`, `serverHints`, `proceed`, `hold`, ...) | during loading | own instance (`limp_loading`); thread not mapped [interp] |

Timers `setTimeout` / `setInterval` / `clearInterval`, `ac.onSharedEvent`, `ac.onRelease` and the `ac.on*` events (36)
fire from the same script thread; in a background worker they fire only inside `worker.sleep()` / `os.sleep()`.
Global `car` (`ac.StateCar`) and `sim` (`ac.StateSim`) are provided to car, track, FFB and gamepad scripts. `[sdk]`

### 3.4 Lua app `manifest.ini` keys `[dll]` (strings at 0x2267c28..0x22686f8) and shipped manifests `[ini]`

Keys seen in shipped manifests are certain; the section a DLL-only string belongs to is my assignment `[interp]`.

* `[ABOUT]` `NAME`, `AUTHOR`, `VERSION`, `DESCRIPTION`, `URL`, `REQUIRED_VERSION` (CSP build, e.g. 2744).
* `[CORE]` `LAZY` (NONE/0, PARTIAL/1, FULL/2, also `LAZY_<name>`), `FLAGS` (`DISABLED_OFFLINE`, `DISABLED_ONLINE`,
  `DISABLED_RENDER_SINGLE/TRIPLE/VR/CUSTOM`, `DISABLED_IF`), `DELAYED_INITIALIZATION`, `LIVE_RELOAD`, `PRELOAD_SQLITE`.
* `[WINDOW_...]` (one per window) `ID`, `NAME`, `ICON`, `TAGS`, `GROUP` (`SETTINGS`, `DEVELOPER`, ...), `FLAGS`
  (`SETTINGS`, `FADING`, `FIXED_SIZE`, `AUTO_RESIZE`, `NO_BACKGROUND`, `NO_TITLE_BAR`, `NO_COLLAPSE`, `NO_SCROLLBAR`,
  `NO_SCROLL_WITH_MOUSE`, `NO_ICON_IN_WINDOW`, `FLOATING_TITLE_BAR`, `DARK_HEADER`, `HANDLE_CTRL_TAB`, `SETUP_HIDDEN`,
  `SETUP_INLINE`, `HIDDEN_OFFLINE/ONLINE`, `HIDDEN_RENDER_*`), `MIN_SIZE`, `MAX_SIZE`, `SIZE`, `DEFAULT_POSITION`,
  `PADDING`, `BACKGROUND_COLOR` (`SETUP` ...), `FUNCTION_*`, `TOOL_NAME`, `TOOL_DESCRIPTION`; `FLAGS` may be an
  expression (`$" read('csp/version', 0) >= 2514 and {...} "`).
* `[SERVICE]` `SCRIPT`, `PERIOD` (e.g. `12h`), `[AWAKE_TRIGGERS]` `EVENT`, `BUTTON`, `WINDOW`, `TIMEOUT`,
  `[SIM_CALLBACKS]`, `[UI_CALLBACKS]`, `[RENDER_CALLBACKS]`.
* WFX controller manifests add `FOLLOWS_SELECTED_WEATHER`, `FOLLOWS_SELECTED_TEMPERATURE`, `FOLLOWS_SELECTED_WIND`; WFX
  style manifests add `[CORE] LINEAR_COLOR_SPACE_HINT` and `AWARE_OF_GAUSSIAN_SPLATS`, `[COMPATIBILITY]
  DISABLE_WHITE_REF_POINT_FIX`; new-mode manifests add `[RULES] BASE_MODE, PENALTIES, START_TYPE, AI_LIMIT, AI_LEVEL,
  ALLOWED_TRACKS, ALLOW_PHYSICS_ALTERATIONS`, `[TWEAKS] LOADING_BACKGROUND, LOADING_PROGRESS, IMMEDIATE_START,
  DISABLE_FLAGS, HIDE_PITCREW, FORCE_EXTENDED_PHYSICS, ICE_EVERYWHERE, SEASON_FORCE, ALT_ONLINE_*`, `[MIXIN: ...]`
  with `@ACTIVE`, `[CORE] SESSION_AWARE`. (details of modes belong to the new-modes brief.)

### 3.5 The 19 SDK modules (counted from `lib.lua`, exact)

Method: `grep -c '^function '` = definition lines ("defs", overloads repeat a name); distinct names = unique text
between `function ` and `(`; "core" = names present in all 19 modules (795); "own" = distinct - 795; classes =
distinct `---@class` names (equals the README's `## Class` count); aliases = distinct `---@alias`. Union over the 19
modules = **2,729 distinct function names** (file `lua_sdk_functions_union.txt` has 2,728 lines: one name differs, I did
not chase it). `lib.lua` total 15,135,150 bytes, README.md total 9,483,576 bytes.

| SDK module | defs | distinct | own | classes | aliases | `lib.lua` bytes | Kind | Shipped examples |
|---|---|---|---|---|---|---|---|---|
| ac_apps | 2,092 | 2,077 | 1,282 | 95 | 109 | 1,031,884 | Lua apps (windows, ImGui-like UI) | 3 internal + 4 in `apps/lua` with manifests (+1 stub) |
| ac_background | 890 | 884 | 89 | 58 | 58 | 509,124 | background worker, basic `ac` only | 1 use (`AppShelf/BackgroundInstall.lua`) |
| ac_background_pw | 921 | 914 | 119 | 59 | 59 | 520,935 | physics worker (333 Hz) | 2 (`p2p-1v1/impl_steam_worker`, `CspDebug/CarDebugWorker`) |
| ac_car_cphys | 998 | 992 | 197 | 64 | 60 | 551,602 | car physics script | 4 VRC cars (data scripts closed) + 4 helper libs |
| ac_car_scriptable_display | 1,754 | 1,747 | 952 | 90 | 104 | 922,938 | car display / car script | 1 shared (`lua/cars/android_auto`, 26 files), 3 VRC cars, 1 inline (Red Bull Ring config) |
| ac_chaser_camera | 1,725 | 1,718 | 923 | 82 | 103 | 901,630 | chase camera | 4 |
| ac_cockpit_camera | 1,695 | 1,688 | 893 | 82 | 104 | 900,788 | cockpit camera (NeckFX script) | 1 |
| ac_ffb_postprocess | 911 | 905 | 110 | 61 | 59 | 518,830 | FFB filter | 2 |
| ac_joypad_assist | 962 | 956 | 161 | 63 | 60 | 537,166 | gamepad / mouse steering | 6 (3 + 3) |
| ac_new_modes | 2,082 | 2,068 | 1,273 | 114 | 109 | 1,035,991 | game modes | 10 (+1 dev placeholder) |
| ac_online_script | 1,961 | 1,947 | 1,152 | 108 | 103 | 979,375 | server-supplied script | 0 |
| ac_pfx_fireworks | 1,709 | 1,702 | 907 | 84 | 103 | 902,238 | fireworks | 1 (15 files) |
| ac_pp_filters | 1,706 | 1,699 | 904 | 83 | 103 | 908,343 | post-processing filter | 7 |
| ac_scripted_texture | 1,564 | 1,558 | 763 | 73 | 101 | 824,103 | texture made by a script | 0 |
| ac_splashscreen | 1,298 | 1,294 | 499 | 38 | 99 | 646,068 | loading screen | 1 |
| ac_tools | 2,039 | 2,026 | 1,231 | 94 | 109 | 1,018,814 | Objects Inspector tools | 2 (103 files) |
| ac_track_script | 1,810 | 1,797 | 1,002 | 85 | 103 | 925,615 | track script / display | 0 files, 1 inline |
| ac_wfx_controller | 925 | 919 | 124 | 59 | 59 | 519,501 | weather controller | 3 |
| ac_wfx_impl | 2,089 | 2,082 | 1,287 | 115 | 103 | 980,205 | weather style | 1 (25 files) |

Notes. (1) `rules.json` (VS Code mapping) has 17 keys: it lacks `ac_background`, `ac_background_pw` and
`ac_scripted_texture`, and lists `ac_tfx` (`extension/lua/tfx/*`) for which there is no SDK folder, and it marks
`ac_track_script`, `ac_car_cphys`, `ac_car_scriptable_display` as `"withoutIO": true`. (2) "Own" is large for kinds
that add `ac_scene`, `ac_render`, `ac_ui` ... (the 38 common modules are in every README; each README has 42..75
`# Module` headings, union 88). (3) Functions unique to exactly one module: `ac_wfx_impl` 356 (sky / cloud / fog / light
pollution), `ac_car_cphys` 94, `ac_apps` 38, `ac_chaser_camera` 31, `ac_car_scriptable_display` 24, `ac_splashscreen` 18
(the `loading.*` table), `ac_pfx_fireworks` 15, `ac_new_modes` 12, `ac_ffb_postprocess` 11, `ac_joypad_assist` 9,
`ac_wfx_controller` 9, `ac_tools` 7, `ac_background_pw` 6, `ac_online_script` 3, `ac_cockpit_camera` 1,
`ac_pp_filters` 1 (`ac.setFixedFOV`), `ac_background` / `ac_scripted_texture` / `ac_track_script` 0. (4) `ac_track_script`
exposes `car` and `sim` globals, `ac_apps`/`ac_new_modes`/`ac_tools` do not.

### 3.6 Runtime, FFI and the precompiled bundle `[dll]`

* **VM**: LuaJIT, version string "LuaJIT 2.1.1769366199" (RVA 0x20886d8; build date 2026-01-25) next to "Lua 5.1"
  (0x2087728); the library bundle was compiled with "LuaJIT 2.1.1711381957" (2024-03-25) per the archive's
  `readme.txt`, so the runtime accepts older bytecode. Standard `luaL_openlibs` default path `.\?.lua;!\lua\?.lua;
  !\lua\?\init.lua;` and `LUA_PATH` / `LUA_CPATH` lookups are present. `jit.profile`, `jit.util`, `jit.opt` are linked
  (the Lua Debug profiler reports "JIT-compiled / Interpreted / CSP API / Garbage collector / Time spent compiling").
  Flags: `jit` is on (the two protected loaders `internal/patch.lua` (92,545 B) and `user.lua` (2,155 B), both
  modified at the start of the last session, declare themselves a small performance tweak calling `jit.on()`; not
  opened, per rule); GC64 and other build flags: not determined.
* **Bytecode header**: entries of the embedded archive start with bytes `06 07 08 88 0A ...`, not the stock `1B 4C 4A`
  LuaJIT magic: a customised dump header [interp]; not loadable by a stock LuaJIT. `cache/lua/<64-bit hex>` (17 files,
  431 B .. 349,540 B on this PC) are per-script bytecode caches with a 4-byte version (4) in front. `[dll]` `[scan]`
* **FFI**: available and central. The SDK types it as `ffilibex` ("Custom FFI namespace. Be very careful around
  here."): only 8 members are documented (`cdef` with extra `params`, `istype`, `string`, `typeof`, `cast`,
  `metatype`, `gc`, `copy`), `ffi.C` is `nil`. Internal shared libraries use `ffi.C.sqlite3_*` and `ffi.C.lj_luasteam_*`
  after replacing `ffi.load` (the sqlite helper states "LuaJIT in CSP does not have ffi.load"; symbols come from
  `extension/internal/plugins/libsqlite3.dll` and from CSP's own exports). The API is called on a "fast route"
  (an FFI-call path; the error handler warns "due to fast API route, raising this error is likely going to collapse
  AC"). State objects (`ac.StateCar` 278 fields, `ac.StateSim` 183, `ac.StateWheel` 63, ...) are FFI structs over
  host memory; `ac.GenericList`, `ac.StructItem.*` (35 builders) / `ac.connect()` give scripts shared structs.
* **Native C modules preloaded**: LuaSocket 3.0-rc1 (`socket.core`, `mime.core`), a luaossl-style OpenSSL binding
  (`_openssl.*`, 25 sub-modules in the bundle), `internal.serialize`.
* **Embedded bundle** (4th ZIP of `dwrite.dll`, offset of the central directory 0x465ebab, 135 entries, 2,996,460
  bytes uncompressed / 1,006,935 deflated, plain names, none encrypted):

| Group | n | Uncompressed | Names |
|---|---|---|---|
| `readme.txt`, `ini_std` | 2 | 967 + 17,249 | INIpp expression library (from a separate public project) |
| environments `ac_common_base`, `ac_common` | 2 | 177,569 + 196,386 | base + common API |
| environments per kind `ac_*` | 19 | 2,289,485 | the 19 modules of 3.5 (1,134 B `ac_background` .. 269,707 B `ac_splashscreen`) |
| libraries `lib_*` | 23 | 116,630 | `accessappwindow audio binary carcameradef hashspace jsonparse music numlut numlut_jit onlineevent physics postprocessing replaystream scene shader smoothing social stringify tracklines ui_gif ui_shared vector web` |
| shared `sh_*` = `lua-shared/` | 89 | 198,174 | `dualsense` 2, `info` 1, `ltn12`, `math/cubic`, `mime`, `openssl` 25, `physics` 4 (`automated-manual-transmission`, `automatic-transmission`, `automatic-transmission-utils`, `rnpd-shifting`), `promise`, `sim` 15 (`ai altonline ambience cars chat cspdebug di events fireworks ghost motec plates surfaces traffic weather`), `socket` 8, `ui` 4, `unstable` 2, `utils` 19 (`appshelf binary dbstorage diagnostics ioext kn5 luasteam ntp obs openkneeboard oswindow parcel playing qr radio signing sqlite steam tts`), `vr` 3, `web` 2 (`browser`, `ui`) |

  The 89 shared libraries exist twice: as source in `extension/internal/lua-shared/` (89 .lua, 490,858 bytes) and as
  bytecode in the DLL; `require('shared/<path>')` resolves through the `<AC>/extension/internal/lua-?.lua` pattern.
  The mirror archive (7th ZIP, 166 entries) also holds `internal/lua-module/*.luac`: 41 files, 313,029 bytes, namely
  `module` (70,241 B, "CSP module"), `performance_hints`, `python_prepare`, `weather_style_previews`, `sfp1` /
  `sfp1_render` / `alt_sfp1` / `alt_sfp1_render` (alternative tyre model) and **33 plug-ins** (`ai_tweaks
  ambience_audio background_pause clear_cache custom_drivable_wheel_model custom_driver_models damaged_engine_audio
  dark_suit desaturated_collisions dlss5_settings driver_gloves dualsense_lights extra_buttons extra_physics_buttons
  fmod_logging hdr hide_standing_spectators hotlap mumble_autoconnect obs_integration online_license_plates
  reconnect_devices showroom_previews showroom stalling tts_chat udpcheck ui_pits_limiter unhide_driver_and_steering
  vr_mirror_adjustments vr_rig_remote vr_rig_vr vr_rig`): a good share of CSP features is itself Lua.
* **Preprocessor**: scripts are run through a Lua preprocessor (`[Lua PP]` messages; `FORCE_LUA_PREPROCESSOR=1`);
  one visible feature: `const(value)` "inlines value as constant". Other transformations: not mapped.
* **Debugging**: (a) native **Lua Debug** app (`apps::app_lua_debug_impl`, id `LUA_DEBUG`, `gui.ini APP_LUA_DEBUG=1`):
  tabs for stored values (`ac.store`), `ac.debug()` values (with graphs; `__monitored` is true while viewed), canvas
  textures, performance (time and garbage per script, "Measure GC"), sampling profiler with call tree and hot spots,
  background workers list; `ac.setLogSilent()`, `SILENT_LUA`. (b) the **CSP Debug** Lua app
  (`internal/lua-apps/CspDebug`, `LAZY=PARTIAL`, group DEVELOPER): lights and physics outlines, VAO debugging, "View
  & Video Settings" window, car-state record / load buttons; it starts a physics worker `CarDebugWorker`. (c) the
  Objects Inspector with Lua tools. (d) `ac.log`, `ac.warn`, `ac.error`, `ac.console*`.

### 3.7 API surface (counts reproducible with the stated command)

Counted over the union of the 19 `lib.lua` (`grep '^function '`, distinct names):

| Namespace / owner | Functions | Notes |
|---|---|---|
| `ac.*` (free functions and constructors) | 1,170 | of which 352 `set*`, 281 `get*`, 138 start upper case (constructors / namespaces), 44 `is*`, 36 `on*` (events), 21 `reset*`, 20 `override*`, 14 `add*`, 12 `disable*`, 10 `access*`, 9 `load*`, 8 `try*`, 8 `fix*`, 7 `calculate*`, 7 `find*`, 6 `draw*` |
| `ui.*` | 397 | immediate-mode UI (3.7 group 7) |
| methods `Class:method` / `vec3.method` | 705 | `SceneReference` 110, `GeometryShot` 35, `vec3` 34, `MediaPlayer` 33, `vec4` 30, `vec2` 29, `rgb` 28, `RigidBody` 27, `TrackPaint` 26, `rgbm` 25, `AudioEvent` 23, `ExtraCanvas` 21, `mat4x4` 18, `quat` 17, `AppWindowAccessor` 15, `ColorCorrection` 14, `ControlButton` 12, `StateCar` 12, `INIConfig` 11, `ray` 11, ... (62 owners) |
| `physics.*` | 114 | 107 unique in track / app / tool / online kinds (see `physics_documented.md` 3.3.1), 31 in the physics worker, 13 in car scripts |
| `io.*` / `os.*` | 41 / 24 | CSP additions (no `io.open` documented) |
| `render.*` | 39 | debug draw, GL-style immediate mode, passes, rays |
| `math.*` / `table.*` / `string.*` | 36 / 33 / 29 | extensions of the standard libraries |
| `loading.*` / `script.*` / `web.*` / `ffi.*` / `display.*` | 18 / 11 / 10 / 8 / 6 | |
| globals | 21 | `class`, `const`, `try`, `using`, `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`, `print`, `stringify`, constructors `vec2 vec3 vec4 rgb rgbm hsv quat mat3x3 mat4x4`, `refnumber`, `refbool` |
| `JSON`, `stringify`, `worker`, `class`, `package`, `coroutine` | 2, 8, 2, 3, 2, 2 | |
| **Total distinct** | **2,729** | |

State classes (fields, `ac_apps`): `ac.StateCar` 278, `ac.StateSim` 183, `ac.StateWheel` 63, `ac.StateUi` 33,
`ac.CarControlsInput` 33, `ac.LightSource` 32, `physics.CarControls` 26; car physics adds `ac.StateCphysCar` 52,
`ac.StateCphysWheel` 36, `ac.StateCphysSurface` 13. `---@field` lines per module: 74 (`ac_splashscreen`) .. 1,116
(`ac_wfx_impl`).

**The 20 architecturally most important groups.** Counts in the "n" column for topic groups are an approximation:
first-match-wins regular expressions over the 1,170 `ac.*` names (100 names stay unassigned: window / VR / misc
singletons); namespace counts are exact.

| # | Group | n | What it is |
|---|---|---|---|
| 1 | Sim / session / car state read | about 97 functions + 4 state classes | `ac.getSim()`, `ac.getCar(i)`, `ac.getUI()`, `ac.getSession()`, `ac.iterateCars()`: live FFI structs refreshed at graphics rate, plus about 280 `getCar*/getTyre*/getDriver*` getters. The universal input of every script. |
| 2 | Car physics write (cphys) | about 98 `ac.*` | `ac.overrideGasInput / overrideBrakesTorque / overrideEngineTorque / overrideABS`, `ac.addForce`, `addHubForce`, `ac.replaceClutch`, setters of drivetrain, turbo, ERS, DRS, differentials. Only the car physics script (and a few other kinds) may call them. |
| 3 | Physics world and AI | `physics.*` 114 | rigid bodies, colliders, track surfaces, teleport / force / penalty functions (38 documented lap invalidators) and 27 `physics.setAI*`; gated by `physics.allowed()`. |
| 4 | Scene graph | `SceneReference` 110 methods + `ac.findNodes/findMeshes/...` | find, create, clone nodes and meshes, transform, visibility, materials (`setMaterialProperty/Texture`), shader replacements, animations, KN5 loading, boolean mesh ops, fake shadows. The core of car and track visual scripts. |
| 5 | Offscreen scene shots, paint | `GeometryShot` 35, `TrackPaint` 26, `ac.Simple*`, vertex / index buffers | render the scene or paint decals on the track into textures. |
| 6 | Rendering | `render.*` 39, `ExtraCanvas` 21, shader calls | debug shapes, immediate-mode GL, quads with custom HLSL (`render.shaderedQuad`, `ui.renderShader`), render targets, `render.on*` callbacks; shaders are compiled at run time into `cache/lua_shaders`. |
| 7 | Immediate-mode UI | `ui.*` 397 | windows, child / popup / tab / menu, widgets (`button`, `slider`, `combo`, `inputText`, `colorPicker`, `treeNode` ...), draw lists (`drawRect*`, `path*`, `drawImage*`), DirectWrite text (`dwrite*`, `pushDWriteFont`), icons, transform stacks (`beginRotation`, `beginScale`), post effects (`beginBlurring`, `beginTonemapping`), HTML / markdown. ImGui-like, with CSP additions. |
| 8 | Displays and textures | `display.*` 6, `SharedTexture`, `MediaPlayer` 33, `GIFPlayer` 8 | `[SCRIPTABLE_DISPLAY_...]` canvases bound to materials; video / GIF / webp playback via Media Foundation (`USE_MMF`). |
| 9 | Audio | `AudioEvent` 23 methods, about 34 `ac.*`, `CarAudioTweak` 11 | FMOD events from soundbanks (`ac.loadSoundbank`) or files, DSP, per-car audio tweaks, volume mixes, `os.openAudioOutput`. |
| 10 | Sky, weather, clouds, fog, light | about 252 `ac.*` (356 unique to `ac_wfx_impl`) | the whole WeatherFX engine API: sky V2 parameters, clouds and covers, fog, godrays, ambient, shadows, rain, track conditions (`ac.TrackCondition`, `ConditionsSet`). |
| 11 | Post-processing and colour | about 121 + `ColorCorrection` 21 | tonemapping, exposure, glare, white balance, LDR / HDR matrices, `ac.getPp*/setPp*`, colour-correction stack. |
| 12 | Cameras and mirrors | about 56 | `ac.grabCamera` (4 methods), on-board camera parameters, FOV, real mirrors, virtual mirror, camera def tables (`carcameradef`). |
| 13 | Input and controllers | about 51 + `ControlButton` 12 methods | joysticks, gamepads, DualSense / DualShock output (haptics, adaptive triggers), keyboard / mouse events, new bindable buttons stored in `controls.ini`. |
| 14 | Session, replay, setup control | about 67 | `ac.tryToRestartSession/ToToggleReplay/...`, `ac.endSession`, setups (`getSetupSpinners`, `saveCurrentSetup`), pit-stop presets, timing helpers. |
| 15 | Online and chat | about 30 + `ac.OnlineEvent`, `ac.onSharedEvent`, `web.socket` | server info, votes, chat, custom messages between scripts and clients (TCP / UDP), `shared/sim/altonline` fake-online client. |
| 16 | AI | about 14 `ac.*` + 27 `physics.setAI*` | aggression, lookahead, mistakes, pit requests, AI spline recording. |
| 17 | Storage, config, IPC | about 68 | `ac.INIConfig` (11 methods, `carData/trackData/raceConfig/scriptSettings/...`), `ac.DataLUT11/21`, `ac.storage / store / load`, `ac.connect` (shared structs), `ac.ReplayStream`, memory-mapped files, hashing, `HashSpace`. |
| 18 | File system, OS, processes | `io.*` 41, `os.*` 24 | file read / write / zip, dialogs, `os.runConsoleProcess`, `os.hostNamedPipe`, `os.openSerialPort`, `os.loadDLL`. Most of it is not available to no-IO scripts. |
| 19 | Web | `web.*` 10, `shared/socket`, `openssl`, `web/browser` | HTTP/S via WinHTTP (two concurrent requests), WebSocket, remote assets cache, embedded browser (`shared/web/browser`, litehtml for HTML). |
| 20 | Value types and language helpers | about 200 methods + 21 globals | `vec2..4`, `rgb`, `rgbm`, `hsv`, `quat`, `mat3x3`, `mat4x4`, `ray`, `class`, `stringify`, `JSON`, timers, `table.*` / `string.*` / `math.*` extensions (98 functions). |

### 3.8 Language extensions and INIpp `[sdk]` `[ini]`

* `math` +36 (`lerp`, `clamp`, `saturate`, `smoothstep`, `perlin`, `simplex`, `seededRandom`, `round`, `sign`,
  `isnan/isinf/isfinite`, vector overloads `clampV/saturateN`, `poissonSampler*`, `convertHDR`, ...), `string` +29
  (`split`, `trim`, `startsWith`, `endsWith`, `regfind/regmatch/reggsub`, `url*`, `pack/unpack`, `codePointAt`,
  `versionCompare`, ...; `%` operator formatting `"%d" % n` used in shipped scripts), `table` +33 (`map`, `filter`,
  `reduce`, `clone`, `join`, `slice`, `sum`, `distinct`, ...), `coroutine.sleep`, `package.add`, `class(name, parent)` with
  mixins, `try/using` helpers, `const()`.
* **INIpp**: `$" expr "` evaluated in a small Lua environment with `read(path, default)` (`car/id`, `car/index`,
  `track/id`, `data/car.ini`, `data/tyres.ini`, `settings/<section>`, `csp/version`, `meta/year`, `meta/open_wheeler`,
  `meta/has_rain_tyres`, `race.ini/...`), `def(v, fallback)` (320 uses), `def2..4`, `discard()` (28), `has`, `get`, and
  the base functions `config/cars/common/functions_base.lua`, `config/tracks/common/functions_base.lua`
  (`saturate`, `clamp`, `ParseColor`). `[FUNCTION: name]` takes `ARGUMENTS`, `PRIVATE` ("if set to 1, Lua state will be
  reset before loading next file") and `CODE`. The expression library is the embedded `ini_std` (17,249 B bytecode,
  not read). Calls seen in shipped ini: `vec2/3/4`, `max`, `lerp`, `normalize`, `clamp`, `saturate`, `cross`, `pow`,
  `length`, `string.upper`, plus user functions (`GetExtraSwitch` 16, `Navigator_ResolveSize` 16, `_Material_PBR_*`).

### 3.9 Sandboxing and security (what a script can and cannot do)

| Mechanism | Evidence |
|---|---|
| **Script-kind gating.** Many API functions check the kind at call time and raise "Not available: not for this type of script", "Currently, not available to this type of script", "Not available to run from a worker", "Not available: online", "Not available: not for a local car", "Not available: New Modes module is not active", "Setting currently playing music is not available for this type of script". | `[dll]` 0x21f1fd8..0x21f23d0, 0x2213a60, 0x22511a0 |
| **I/O access flag per script** (`__allowIO__`, `io.available()`). No-IO kinds by `rules.json`: car physics, car display, track. 48 doc comments (46 distinct functions) say "Not available to scripts without I/O access": `io.save*`, `createDir`, `move`, `copyFile`, `deleteFile`, `deleteDir`, `recycle`, zip writers, `io.scanDrives`, `io.getMainExecutable`, `os.execute`, `os.setCurrentFolder`, `os.loadDLL`, `os.addDLLDirectory`, `os.hostNamedPipe`, serial / audio device functions, explorer helpers, `ac.compressTexture`, `ac.encodeTexture`, `ac.onFileChanged`, `ac.onFolderChanged`, `ac.pauseFilesWatching`, `ac.sendKeyboardButtonEvent`, `ac.setMousePosition`, `ac.setMouseLeftButtonDown`, `ac.setMouseWheel`, `ac.debugLights`, `ac.applyLiveConfigEdit`. No-IO scripts may still read (`io.load`) files they can reach; `os.setCurrentFolder` and `io.findFile` are limited to readable folders; `ac.storageSetPath` gets a forced `shared-` prefix and may not contain `.` or slashes; `ac.encodeTexture` output must be a `.dds` inside the script folder. | `[sdk]` |
| **Storage isolation.** No-IO scripts keep state under `cfg/extension/state/lua/shared-<carID>`. | `[sdk]` `[scan]` |
| **FFI restricted.** `ffilibex` exposes 8 members; `ffi.C` nil; `ffi.load` absent; `os.loadDLL` is IO-only and "Loaded DLLs can't interact with LuaJIT FFI". | `[sdk]` `[lua]` |
| **Process launching is allow-listed.** `os.runConsoleProcess` / `os.openURL` consult a rule set downloaded from `https://acstuff.club/j/known.dat` (RVA 0x21eed00; version tag `v%llx`) with `binaries`, `files`, `rules`, `allow_stdin`, `allow_envinroment` [sic]: messages "Not allowed (unknown binary)", "Not allowed (missing rule set)", "Not allowed URL: ", "Raw arguments are not allowed here", "can't use custom environment / stdin / working directory here", "can't use these arguments here". RTTI type `lua::known_executables::{parsed, rules_entry}`. Format of `known.dat`: not read. | `[dll]` 0x2280fb0..0x22812e0 |
| **Physics is opt-in per track and kind.** `physics.allowed()` ("if not, only `physics.raycastTrack()` is available"); `surfaces.ini [_SCRIPTING_PHYSICS]` keys `ALLOW_TRACK_SCRIPTS`, `ALLOW_DISPLAY_SCRIPTS`, `ALLOW_NEW_MODE_SCRIPTS`, `ALLOW_TOOLS`, `ALLOW_APPS`, `ALLOW_CUSTOM_AI_MANIPULATION`, `ALLOW_TRACK_SCRIPTS_STATIC_OBJECTS`, `ALLOW_TRACK_SCRIPTS_DYNAMIC_OBJECTS`, section `_EXTRA_PERMISSIONS`; needs extended physics and double precision engine (see `physics_documented.md` 3.3 and 3.5). | `[sdk]` `[dll]` 0x22483b8..0x2248460 |
| **Lap-invalidating and online-restricted calls are marked.** 38 functions state they invalidate the current lap (`physics.setCarPosition`, `teleportCarTo`, `setEngineRPM` unless below 400, `setCarFuel`, `ac.resetCar`, `ac.takeAStepBack`, `ac.setMessage` ...); 8 say "not available online" (`ac.getCarLeaderboardPosition`, `ac.getCarRealTimeLeaderboardPosition`, `ac.loadCarState`, `ac.resetCar`, `ac.takeAStepBack`, `ac.setTrackCoordinatesDeg(2)`, `ac.setTrackTimezoneOffset`); `ac.getUserSteamID` "available only to online scripts"; fake mouse events "do not work from online scripts". Experimental features are refused online, with special events, SIMRACERS / RSRLIVETIME apps (0x22aa228..). | `[sdk]` `[dll]` |
| **Network limits.** `web.get/post/request`: at most 2 simultaneous requests (configurable up to 32 by `web.timeouts`), default timeouts 4 / 10 / 30 / 30 s; remote assets cached, expire after a couple of weeks; `[CONFIGURATIONS] BLOCK_UNTRUSTED_NETWORKING` blocks car and track scripts from the web; `os.hostNamedPipe` names are forced to `\\.\pipe\AcTools.CSP.Lua.<name>` and `localOnly` defaults to true. | `[sdk]` `[ini]` `[dll]` 0x2281360 |
| **Integrity and signing for online use.** `shared/utils/signing`: `verify(files)` returns an 18-byte fingerprint of files (`:self` = the running script and its requires, `:ac` = surfaces.ini, `:car`, `:track`, `:car/<id>`, `:track/<id>`), `blob(format, data)` signs with RSA (384-byte signature = RSA-3072; the doc shows verification with RSA-SHA1) with substitutions `{ServerIP}`, `{ServerSeed}`, `{UniqueMachineKey}`, `{VerifiedSteamID}`, `{CSPVersion}` ...; natives `ac.verifyIntegrity`, `ac.signBlob`. Server side `[EXTRA_RULES] VERIFY_RAIN_FX`, checksums (see `attach.md` 1.9). | `[lua]` |
| **Containment of failures.** Slow-script detector ("Slow script! %s/%s took %d ms"), fatal Lua error handler that disables the broken script and offers "Check for updates for installed mods", state-damaged detection, "A fatal failure terminates the script to prevent Assetto Corsa from crashing", worker threads at low priority, per-script memory and time counters. | `[dll]` |
| **Safe deserialisation.** `stringify.parse` evaluates only a namespace table you pass (not `_G` unless asked: "might be pretty unsecure"). | `[sdk]` |

What I could not establish: whether `io.open` / `io.popen` / `loadstring` / `debug` of the stock Lua libraries are
removed or replaced for non-IO scripts; the exact rule that gives a script `__allowIO__`; how the server's script is
authenticated (class `server_lua_script`); the format of `known.dat`.

## 4. Files it reads

* **Scripts and manifests**: 3.2 paths; `<AC>/apps/lua` (`LUA_APPS_DIR`), `<AC>/extension/internal/lua-apps`,
  `.../lua-shared`, `.../lua-sdk` (not read at run time), `<AC>/extension/{lua,weather,weather-controllers}`,
  car `extension/**` and `data.acd`, `content/tracks/<t>/extension`, `extension/config/{cars,tracks}/{loaded,generic,common}`.
* **Embedded**: the 4th ZIP (135 libs) and the 7th ZIP `internal/lua-module/**` (41 `.luac`), `internal/shader-tpl/*.fx`
  (14 shader templates for `lua_shaders`), `internal/tzdata`.
* **Caches written** (by CSP, observed): `<AC>/cache/lua/*` (17 files), `<AC>/cache/lua_shaders/4a91a560.zip` (1 file),
  `<AC>/cache/web/*` (hash-named); per-script state under `Documents/Assetto Corsa/cfg/extension/state/lua/**`;
  settings of the plug-in kinds `extension/state/lua/<kind>/<impl>__settings.ini`.
* **Network**: `https://acstuff.club/j/known.dat`, `https://acstuff.club/patch` and plug-in installer (not Lua).
* **Config**: `extension/config/{general,gui,lua_tools,chaser_camera,neck,ffb_tweaks,gamepad_fx,graphics_adjustments,
  particles_fx,splashscreen,weather_fx}.ini` and the owner's overrides (empty on this PC).

## 5. How it is switched on

* Always on once CSP is enabled (`general.ini [BASIC] ENABLED=1`): the internal "CSP module" script and its 33
  plug-ins (each plug-in has its own switch in its module), INIpp evaluation, Lua apps (`apps/lua`), Weather FX
  style + controller (`weather_fx.ini [BASIC] ENABLED=1`), App Shelf, CSP Debug, Lua Debug, fireworks (holidays).
* Opt-in by ini: chase camera, cockpit camera script, FFB script, gamepad / mouse assist, PP filter, loading screen
  (all `ENABLED=0`, see 3.2); Objects-Inspector tools (`lua_tools.ini ENABLED=1`).
* Opt-in by content: car display / physics scripts (per car), track scripts, new modes (selected in CM / quick
  drive), online scripts (server). Physics-affecting Lua on a track needs the track's `[_SCRIPTING_PHYSICS]` keys and
  extended physics.
* This PC (`[log-csp]`, 2026-10-12): the "CSP module" script loaded twice (20,724 and 26,708 microseconds, before and
  after sim init), the Weather FX style once (delayed: 134.8 microseconds queued, 549,145 microseconds to load) and
  the controller (41,664 microseconds); apps load in "p0 / p1 / sync part" steps (Extended Controls: 0.3 ms, 11.8 ms,
  382 ms); of 8 apps `Mumble` reported "App is not available" (offline) and `SimHubUDPConnector` has 0 windows.

## 6. Size

| Item | Count / bytes |
|---|---|
| SDK modules | 19 (15,135,150 B `lib.lua`, 9,483,576 B README), 2,729 distinct functions, 88 `# Module` names |
| Embedded Lua libraries | 135 (2,996,460 B bytecode) + 41 `.luac` in the mirror (313,029 B) |
| Shipped Lua source on this PC | `extension/lua` 207 .lua (1,345,724 B) of 355 files; `internal/lua-apps` 9 (159,335 B) of 24; `lua-shared` 89 (490,858 B); `weather` 25 (194,007 B); `weather-controllers` 8 (42,632 B); `apps/lua` 123 (848,171 B); `config` 2 (1,008 B); `internal` loaders 2 (not read) |
| Script kinds shipped | 10 modes, 2 tools, 7 PP filters, 4 chase cameras, 1 cockpit, 2 FFB, 6 gamepad / mouse, 1 fireworks, 1 loading screen, 3 weather controllers, 1 weather style, 3 internal + 5 user apps |
| Content with Lua | 3 VRC cars with plain `extension` scripts: `alpha_2025_csp` 23 files (177,620 B), `alpha_2026_csp` 28 (205,255 B), `beta_2024_csp` 19 (94,671 B); `lithium_2023_csp` has only a data script; `urd_darche_992_23` 3 files (22,700 B) packed; 1 track inline script (`ks_red_bull_ring` CSP config) |
| Hooks named Lua/Script | 0 |
| Host functions identified | about 30 (2.2) |

## 7. Port difficulty for rustyAC

**Overall: XL** for "what CSP content may use", **M** for a first useful slice (INIpp + core types + state read).

### 7.1 Host

* LuaJIT is the choice: the content uses the FFI struct model (`ac.StateCar` views, `StructItem` layouts,
  `ac.connect`), the `bit` library (used by the VRC scripts), and performance that assumes a tracing JIT. A Rust host
  with `mlua` + vendored LuaJIT gives the FFI. FFI itself is **not** the hard part; the hard parts are (a) the *size of
  the native surface* (829 distinct functions are actually called by the installed content, 2,729 are declared),
  (b) per-kind sandboxes, (c) the UI toolkit behind `ui.*` (397 functions, ImGui-like), (d) the 3D / audio
  subsystems the API drives. CSP-specific language behaviour to reproduce: `"fmt" % args`, `vec3` operator overloads,
  `class()`, `try`/`using`.
* **WASM**: the browser build cannot run LuaJIT (no JIT, no FFI on `wasm32`); a PUC-Lua / Luau fallback would lose
  `ffi`. Plan for either "Lua only natively" or an FFI-free shim of `ac.StateCar` (tables with metatables).
* Reusable from the existing port: INI reading and the `acd` reader (`ac.INIConfig`, `readDataFile`), car / track
  state (`rustyac-physics`, `car_oracle` tapes: source for `ac.StateCar`/`StateCphysCar` values), `rustyac-audio`
  (FMOD-level behaviour: `AudioEvent`), `rustyac-render` (scene graph, passes), `rustyac-gui` (app ports, SimLink /
  AppLink; AC's native `ksgui` widgets, **not** CSP's ImGui), `rustyac-net` (online events), replay tools.
* Not reusable: CSP's native bridge itself, the bytecode bundle (custom header; sources are claimed public by the
  archive readme: "All source code is available" in the `acc-lua-sdk` repository; not fetched here).

### 7.2 What the owner's installed content needs (static, from plain files)

| Source | Files / bytes | Distinct SDK functions called (`ns.func` in source, comments stripped) |
|---|---|---|
| CSP shipped Lua (modes, tools, filters, cameras, weather, controllers, internal apps, shared libs) | 338 `.lua` (207 + 9 + 89 + 25 + 8) | 759 |
| `apps/lua` (Radar, Extended Controls, VRC Advanced Setup, VRC Track Zone Editor) | 123 files | 220 (Radar 37, ext_controls 70, vrc_advanced_setup 171, vrc_track_zone_editor 90) |
| 3 VRC cars, plain `extension` scripts | 70 files | 92 (60 / 62 / 55 per car, 29 common) |
| **Union** | | **829** of 2,729 (1,900 never called); by namespace `ac` 473, `ui` 237, `physics` 31, `render` 29, `io` 24, `loading` 12, `os` 10, `web` 6, `stringify` 3, `display` 2, `JSON` 2 |

Not visible to this count: method calls on objects (`:setVisible()`), physics scripts of the cars (scrambled inside
`data.acd`, not touched), `urd_darche_992_23` (its three `*.sq.lua` files are packed with a decompressor stub; not
read further), aliasing through tables.

### 7.3 The VRC cars (plain scripts)

* All three have a purely **visual / audio / display** Lua side: modules `can` (reads the physics script's
  published signal bus), `audio`, `opti`, `display` (+ pages / popups / instruments), `lights`, `cameras`, `fx`
  (`halo`, `tyres`, `damage`, `particles`, `wobbly_bits`), `installer`. They contain **no physics write** call
  (checked: no `ac.override*`, `ac.addForce`, `physics.*`).
* **They cannot run without the physics script**: `can.lua` waits for a layout string the encrypted physics script
  publishes with `ac.store` and then connects with `ac.connect` / `stringify.parse` / `ac.ReplayStream` /
  `ac.getCarPhysics(i).scriptControllerInputs` / `ac.getCarExtraSwitchState`. A synthetic publisher is needed to test.
* Union of free functions the three cars call (92: `ac` 56, `ui` 30, `io` 3, `display` 1, `stringify` 1, `os` 1;
  29 are called by all three): `ac` (56): `AudioEvent ControlButton DataLUT11 DataLUT21
  INIConfig OnlineEvent ReplayStream accessCarCamera accessCarLight broadcastSharedEvent connect debug decodeBase64
  dirname error findFile findMeshes findNodes getCar getCarColliders getCarExtraSwitchState getCarID getCarPhysics
  getCarSkinID getDriverName getFolder getGapBetweenCars getLastError getPatchVersionCode getPpFilter getScriptDeltaT
  getSession getSim getSunAngle getTrackID getTrackLayout getTyresName getUI isControllerGearDownPressed
  isControllerGearUpPressed iterateCars lapTimeToString load loadSoundbank onClientConnected onReplay onSharedEvent
  onTyresSetChange setExteriorAudioMix setMessage setSystemMessage storage storageChanged storageSetPath store warn`,
  `ui` (30): `SharedTexture beginRotation beginScale decodeImage drawCircleFilled drawLine drawRect drawRectFilled
  drawSimpleLine dwriteText dwriteTextAligned endRotation endScale getCursor getCursorX getCursorY icon image imageSize
  popDWriteFont popFont popTextWrapPosition pushDWriteFont pushFont pushTextWrapPosition setCursor setCursorX
  setCursorY treeNode windowSize`, `io` 3 (`dirExists loadFromZip scanDir`), `os.preciseClock`, `stringify.parse`,
  `display.horizontalBar`, plus enums (`ac.CameraMode`, `FlagType`, `FolderID`,
  `SessionType`, `INIFormat`, `SurfaceExtendedType`, `AudioChannel`, `CarAudioEventID`, `SharedNamespace`,
  `ui.Alignment`, `CornerFlags`, `ui.Icons`, `render.BlendMode`).
* Method families used heavily (counted occurrences of `:name(` over the three cars): SceneReference
  (`findNodes` 43, `findMeshes` 37, `setVisible` 50, `setMaterialProperty` 50, `setMaterialTexture` 29,
  `setAnimation` 25, `ensureUniqueMaterials` 8), AudioEvent (`setParam` 104, `start` 27, `isPlaying` 19,
  `setDistanceMax` 11, `setConeSettings` 7), `INIConfig:get/iterate`, `ControlButton:onPressed`.
* Car configs use `[SCRIPTABLE_DISPLAY_...]` (`RESOLUTION 800x844` or `1024x1024`, `ACTIVE_FOR_LOD`,
  `KEEP_BACKGROUND`) and `[SCRIPT_0]` (beta 2024), `SCRIPT = data_override/script.lua`.

### 7.4 Proposed staged order and how to prove each

| Stage | Content | Difficulty | Proof |
|---|---|---|---|
| S0 | Lua host + `INIpp` evaluator (`$" "`, `[FUNCTION:]`, `[INCLUDE]`, mixins, `def/discard/read`) feeding the existing config loaders | M | 725 expressions in 43 files here; oracle = CSP's own `[__DEBUG] DUMP_FLATTEN_INI` output (`<ini>-flatten.txt`) of the same ini; compare every resolved key |
| S1 | value types (`vec*`, `rgb*`, `hsv`, `quat`, `mat*`, `ray`), `math/string/table` extensions, `class`, `stringify` / `JSON`, timers, `const` | S-M | golden vectors from a Lua app run once inside real CSP (needs AC, a later task) and property tests from SDK doc comments; no game state |
| S2 | state read: `ac.getSim`, `getCar(i)` with the used `car.*` fields (60 seen in the VRC scripts: `index`, `wheels`, `speedKmh`, `rpm`, `gas`, `brake`, `gear`, `drsAvailable`, `isInPitlane`, `lapCount`, ...) and `sim.*` (31 seen), `getCarPhysics`, `iterateCars`, events (`onClientConnected`, `onReplay`, `onSessionStart`, `onTyresSetChange`, `onSharedEvent`) | M | log both sides per frame for the same replay (`car_oracle` tape + a Lua dump app under CSP) and compare |
| S3 | data and IPC: `ac.INIConfig.*` incl. `carData` / `carConfig` / `scriptSettings`, `DataLUT11/21`, `storage/store/load`, `ac.connect` + `StructItem`, `ReplayStream`, `OnlineEvent` / shared events | M | round-trip tests, plus a synthetic physics-script publisher for the VRC `can.lua` |
| S4 | scene access (`ac.findNodes/findMeshes`, `SceneReference` methods used: visibility, material property / texture, animation, transform), `ac.accessCarLight`, `accessCarCamera` | L | `render_oracle` compare of the exterior / cockpit shots with the car's visual scripts active; start with the 12 methods above |
| S5 | display scripts: `[SCRIPTABLE_DISPLAY_...]` canvases, `ui.*` drawing subset (30 functions used by VRC cars), DirectWrite text, `display.*` | L | pixel compare of the display texture vs CSP (existing render oracle on the cockpit view) |
| S6 | audio: `ac.AudioEvent` (23 methods), `loadSoundbank`, `CarAudioTweak`, `setExteriorAudioMix` | M-L | `audio_oracle` compare (the existing FMOD log layer) |
| S7 | Lua apps: manifest loader, windows, `LAZY`, remaining `ui.*` (237 used by installed content), `web`, `io` | XL | screenshot compare of the 4 installed apps with CSP; start with Radar (37 functions) |
| S8 | physics script host (`ac_car_cphys` 95 own + `physics.*` 13, 333 Hz thread): needs the car scripts, which are closed for the VRC cars | XL | prove only on the 4 shared libraries `shared/physics/*` and synthetic scripts; real cars: blocked |
| S9 | weather style / controllers (226 + 16 functions), new modes (249), tools, cameras, filters | XL each | per kind, using shipped scripts as the test corpus |

## 8. Open questions and not mapped

1. How `__allowIO__` is decided and whether `io.open`, `io.popen`, `loadstring`, `debug.*`, `os.exit` are removed in
   no-IO kinds: needs 0x136f620 (4,691 B) read fully.
2. How the game calls into Lua per kind (UI frame, render pass callbacks, sim update, replay): the hooked caller
   functions are not identified except `Car::pollControls`. The host's scheduler is in the 0x136xxxx..0x138xxxx range.
3. The Lua preprocessor: what `const()` and other transformations do; whether `---@ext` (top comment of many VRC
   and CSP scripts, also `internal/lua-apps/Mumble`, `lua-shared/web`) is a preprocessor / loader directive.
4. Server scripts (`ac_ext::server_lua_script`, "CSP: updating server script"): delivery, authentication and
   permissions of kind `ac_online_script`.
5. `known.dat` format and its rule semantics (RVAs 0x3027f0, 0xfd1b60, 0xfd3190, 0xfd40f0).
6. `ac_scripted_texture`: reference syntax and example (no shipped sample).
7. `BLOCK_UNTRUSTED_NETWORKING` is not a plain string in the image: where it is read.
8. The custom bytecode header and the `cache/lua` record layout (header read only: version 4 + hash + 32-bit counts).
9. Whether the 89 shared libraries are loaded from the DLL archive or from disk (both exist).
10. Which of the 760 used functions need the exact CSP numeric semantics (e.g. `math.perlin`, `simplex`, `seededRandom`).
11. CSP's Python side (`ac.ext_*` 213 functions) is out of scope here (`attach.md`).
12. I did not read the protected loaders `internal/patch.lua` and `user.lua` (rule), the packed `urd_darche_992_23`
    scripts, the scrambled car data scripts, the bytecode in `cache/lua` or in the archive (headers only; one
    431-byte cache entry was shown in a hex dump before I recognised what it was: it holds CSP-internal code, not
    mod-maker content, and I used nothing from it).

## Evidence

* `re/scratch/csp1/FACTS.md`, `attach.md` 1.7-1.9 and 2, `physics_documented.md` 3.3-3.5, `physics_car_hooks.md` 2c,
  `lua_sdk_counts.tsv`, `lua_sdk_functions_union.txt` (recounted here), `content_scan.json`, `dll_zips_all.json`
  (4th and 7th archives), `dll_strings.tsv`, `dll_func_strings.tsv`, `dll_func_imports.tsv`, `dll_funcs.tsv`,
  `csp_hook_table.tsv`, `hook_groups.tsv`, `decomp_hooks/` (grep for Lua-core calls).
* `<AC>/extension/internal/lua-sdk/{readme.txt, rules.json, <19>/lib.lua, README.md}`; shipped manifests and entry
  files under `extension/lua/**`, `extension/weather*/**`, `extension/internal/lua-apps/*`, `apps/lua/*`;
  `extension/config/*.ini` (general, gui, lua_tools, chaser_camera, neck, ffb_tweaks, gamepad_fx,
  graphics_adjustments, particles_fx, splashscreen, weather_fx); `extension/config/cars/common/functions_base.lua`.
* `content/cars/vrc_formula_{alpha_2025,alpha_2026,beta_2024}_csp/extension/**` (plain Lua), `.../ext_config.ini`.
* `Documents/Assetto Corsa/logs/custom_shaders_patch.log` lines 126-135, 866-1053; `cache/lua/` directory (names,
  sizes, 4-byte header).
* dwrite.dll strings: 0x20886d8 (LuaJIT), 0x2087728, 0x2087838, 0x2236180.., 0x2266838, 0x2267c28..0x22686f8 (apps),
  0x2269570..0x2269dd8 (tools, modes), 0x227f300, 0x2280fb0..0x2281eb0 (processes, rules), 0x228f4f8 (scripted
  texture), 0x22a8c20..0x22a9650 (loader, errors), 0x22aa228; RTTI at 0x25f8f00.., 0x26229d8.

## Decompile requests

1. `dwrite.dll` 0x136f620 (4,691 B): list every native bound into the Lua environment and show where `__allowIO__`,
   `__mode__` are set (sandbox rules).
2. 0x1373420, 0x13743f0, 0x1374a50, 0x1374cb0: the load pipeline (p0 / p1 / sync), bytecode cache key and the
   preprocessor invocation (0x1372bd0).
3. 0x1375740, 0x1375c60, 0x136d110: how an `update` is invoked (thread, error handling, timing) and the per-kind
   plug-in table.
4. 0x1371b30, 0x137f5f0, 0x136e370: fatal-error policy, "disable broken script" effect, slow-script threshold.
5. 0x3027f0, 0xfd1b60, 0xfd19a0, 0xfd3190, 0xfd3a00, 0xfd40f0: `known.dat` fetch / parse and rule evaluation.
6. 0xcea750, 0xcf8340, 0xcf0800 (11,269 B): app loader and window runtime (manifest keys, `LAZY`, triggers).
7. 0xc50930: car script loader (conditions, `ACTIVE_FOR_*`, LOD, `ALLOW_CPHYS_SCRIPT`), 0x530f10 / 0x530a50
   script-type gating.
8. 0x5f2c50, 0x5f30b0: `data_override` virtual overlay (which names it serves, order vs `data.acd`).
9. 0xf77150: background worker thread (limits, `os.sleep`, shutdown).
10. 0x11eb6a0: "Lua syntax error / runtime error" (INIpp?).
11. 0x31f150: the `shared/sim/altonline` native side ("Not available: fake online client is not configured").
12. 0x115a470, 0x1156650, 0x1156bd0, 0x1156dc0: the `[Lua PP]` preprocessor passes.
