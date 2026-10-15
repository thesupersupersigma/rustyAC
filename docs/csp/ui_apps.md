# UI and apps

CSP changes the user interface in three ways at once. (1) It hooks 89 classes of AC's retained-mode GUI (`ksgui`, `SystemApps.lib`,
the HUD objects, the setup editor) with 177 replacement functions and 30 lookups. (2) It brings its own immediate-mode UI layer (an
ImGui build with AC specific back-ends, called "cui"/`imgui_impl_ac_ingame` inside the DLL) that draws a new taskbar, a new
main menu, a new pause menu, a chat, a driver context menu, 18 native C++ apps and every window of a Lua app. (3) It adds a
Python bridge (213 `ac.ext_*` functions, wrapped listeners, a profiler around every Python app). The shipped
documentation is thin (`gui.ini`, `taskbar.ini`, `splashscreen.ini`, `chat_shortcuts.ini`, `car_instruments.ini`, 3 plain Lua apps),
so most of this file comes from strings, RTTI names, the hook decompiles and the owner's own log and state files.

Cross-references (not repeated here): hook mechanism and gate records `attach.md` 1.7; Lua runtime, manifest keys, the 19 SDK
modules, per-kind entry points `lua.md` 3.2 to 3.7; the AC side of every class `docs/atlas/ui.md`, `docs/atlas/python_api.md`,
`docs/port/gui_hud.md`, `docs/port/menus.md`, `docs/port/apps_user.md`, `docs/port/setup.md`.

## 1. What it does

### 1.1 The three layers in plain words

| Layer | What the user sees | Where it lives |
|---|---|---|
| AC's own windows with tweaks | Same AC apps, ESC menu, pause menu, setup screen, quick pit menu, leaderboards, driver tags; but with bug fixes, event hooks for scripts, hide switches, a new look for some, and (when `REPLACE_*` is on) replaced by CSP's ImGui versions | 177 hook functions (`decomp_hooks/`, group `ui_apps`) |
| CSP's immediate-mode UI | New taskbar with grouping, pinning, search and glow notifications; "New main menu" (Drive / Info / Settings / Telemetry / Lap times / Setup / Exit); new pause menu; chat with attachments and admin tools; driver context menu (ballast, kick, penalty, teleport); photo mode, audio, render stats, post-process selector; developer apps; every Lua app window | the DLL's own code (`.text` 0x7d0000..0xa40000 holds most of it), fonts and 28 UI shaders from the embedded archives |
| Script surface | `ui.*` (397 functions), `ac.*` window / app / hotkey calls, 213 Python `ac.ext_*` functions | LuaJIT host (`lua.md`), embedded Python glue |

### 1.2 CSP's app system (verified parts)

* **Lua apps** (`ac_apps` SDK module, 2,077 functions, 1,282 not shared with the other 18 modules): a folder with `manifest.ini` and a
  Lua file; each `[WINDOW_...]` section becomes one ImGui window, the app's `FUNCTION_MAIN` is called every frame while that
  window is visible. Manifest keys and `LAZY` modes are listed in `lua.md` 3.4. New facts from the owner's log
  `[log-csp]`: an app is loaded in three phases ("Async loading Lua script [App: X, p0]" 0.3 to 0.9 ms; "p1" 28 to 45 ms on a worker
  thread; "sync part" on the main thread: 2.1 s for the 15-window RSS Race Lab, 2 to 104 ms for the others); after that "App is
  loaded: <dir>, windows: N". Load order in the log: the apps named in `[LUA_APPS] ACTIVE=` of the user state file first (in reverse
  order of that list), then the rest of `apps/lua`, `extension/internal/lua-apps`. An app that is not allowed (flag
  `DISABLED_OFFLINE` etc.) logs "App is not available" (Mumble in an offline race). There is an uninstall list ("Trying to
  uninstall app: RSS_Extended_Controls" ... "App removal finished"; the folder is recycled, strings at 0x2268410..0x22684c0).
* **Window runtime**: function 0xceb6a0 (7,667 B) turns one `[WINDOW_...]` section into a window: reads `NAME`, `ICON` (default `app.png`),
  `GROUP` (`SETTINGS`, `DEVELOPER`, others), `FLAGS` (the 20 flag names at 0x2267dd8..0x2268000), `PADDING`, `MIN_SIZE`,
  `MAX_SIZE`, `SIZE`, `DEFAULT_POSITION`, `BACKGROUND_COLOR`, `FUNCTION_MENU/ON_SHOW/ON_HIDE/SETTINGS/ICON`, and creates a
  hidden sub-window `###settings_<id>` for `FUNCTION_SETTINGS` (the small cog button). Function 0xcf0800 (11,269 B) parses the
  rest of the manifest (`[ABOUT]`, `REQUIRED_VERSION`, `OPAQUE`, `LAZY_<name>`, `[SERVICE]`, `[AWAKE_TRIGGERS]` with `EVENT`
  / `BUTTON` / `WINDOW` / `TIMEOUT`, `[SIM_CALLBACKS]`, `[UI_CALLBACKS] IN_GAME`, `[RENDER_CALLBACKS] TRANSPARENT`, `PRELOAD_SQLITE`,
  `LIVE_RELOAD`, `WebBrowser`, `IMMEDIATE`). `[dll]` Manifest-only flags that touch AC's setup screen: `SETUP`, `SETUP_HIDDEN`,
  `SETUP_INLINE` and `BACKGROUND_COLOR = SETUP` (used by the owner's RSS Settings "Extra Setup" and VRC Advanced Setup apps):
  the window is embedded in the setup screen instead of floating `[ini][dll]`.
* **Window state**: positions, sizes and open state are kept in `Documents\Assetto Corsa\cfg\extension\state\imgui_settings.ini`
  `[state]`: sections `IMGUI_APP_POS_SIZE_<desktop>` with `IMGUI_LUA_<App name>_<window id>=x,y,w,h[,pinned]` (CSP's own apps use
  `IMGUI_CSP_LUA_<App name>_<window id>` or a plain id such as `AC_CHAT`, `AC_AUDIO`), `[OPENED_WINDOWS]`, `[LUA_APPS] ACTIVE`,
  `[PAUSE] POS`, `[COLUMN_SIZE_n]`, `[DO_NOT_ASK_AGAIN]` (hash=1), `[MOVABLE]`, `[AC_CSPRSTATS]`, `[OBJECTS_INSPECTOR]`,
  `[LUA_APPS_DEBUG]`, `[APP_SERVICES]`. Writer 0x7df7d0 (2,532 B, `IMGUI_APP_POS_SIZE_`), window ids built by 0x7ddf80
  (3,027 B, `IMGUI_CSP_LUA_`) and 0xced8d0 (`IMGUI_LUA_`). Per-app script data (`ac.storage`) goes to
  `state\lua\app\<App>.ini` with 64-bit hash keys `[BOOLEAN] / [NUMBER] / [STRING]`. `[ini][log-csp]`
* **Native ImGui apps** (C++, `apps::imgui_app<T>` with `imgui_app_base / helper / context / factory_base`, RTTI at 0x26151d8..0x2621b78):
  18 types `[dll]`: `app_ac_audio` ("Audio"), `app_ac_camera_picture_mode` ("Photo Mode"), `app_ac_chat` (Chat),
  `app_ac_csprstats` ("Render Stats CSP" / "performance"), `app_ac_post_process_filter_selector` ("Post Process Filter"), `app_car_debug`,
  `app_extrafx_debug`, `app_grassfx_debug`, `app_lightingfx_debug`, `app_particlesfx_debug`, `app_reflectionsfx_debug`,
  `app_weatherfx_debug`, `app_leaderboard`, `app_light_maker`, `app_lua_debug`, `app_objects_inspector`, `app_python_profiler`
  ("Python Apps Debug"), `app_radar`. The display names (17 distinct strings) sit in 55 to 116 byte functions at 0xce28c0..0xce3b60. Each has a
  `gui.ini` switch (3.1). Five of them replace an AC app (hide the original, see 2.4).
* **Tools** (Objects Inspector plug-ins, `apps::tool_base`): `tool_track_area_maps`, `tool_track_conditions`, `tool_track_fake_shadows`,
  `tool_track_occluders`, `tool_extra_vao_samples`, `tool_navigator_setup`, `tool_collisions_monitor`, `tool_car_positioning_helper`,
  `tool_animated_wipers`, `tool_trees_inspector`, `tool_smoke_tweaks`, and `apps::lua_tool` for Lua tools from `extension/lua/tools/*`
  (`lua_tools.ini ENABLED=1`; `TOOL_NAME`, `TOOL_DESCRIPTION`). Shipped Lua tools: 2 (`csp-railworks-tool` "Railroad planner",
  `csp-traffic-tool` traffic paths / intersections / simulation, 12,240 B `tool.lua` plus `src/`, `lib/`).
* **Internal Lua apps** (plain text, `extension/internal/lua-apps`): AppShelf (installs apps from the "App Shelf" list; its
  list of known apps is cached in `state\lua\app\AppShelf.ini`: 22 ids on this PC), CspDebug (two windows: "CSP Debug"
  with car state record / load, light and physics outlines, VAO debug, weather override via `ac.connect` structs
  `weatherFXDebugOverride*`; and "View & Video Settings", group `SETTINGS`), Mumble (voice chat; `DISABLED_OFFLINE`; wraps
  `AcTools.MumbleClient.exe` in `internal/plugins`). The shared Lua helpers for UI sit in `lua-shared/ui/{base,components,replay,virtualizing}.lua`
  and `lua-shared/sim/{chat,cspdebug}.lua`, `utils/appshelf.lua` (embedded as precompiled bytecode in the DLL too).
* **Python bridge** (2.8): AC's Python apps keep working; CSP wraps `PyPlugin::update` with a profiler and the "pause hidden apps"
  logic, adds 213 `ac.ext_*` functions, wraps 13 `ac_*` C entry points and a traceback helper.

### 1.3 The new in-game settings UI and how Content Manager (CM) fits

* The `[ℹ]` sections of every CSP ini (`FULLNAME`, `DESCRIPTION`, `SHORT_DESCRIPTION`, `PREVIEW`, `URL`, `ORDER`, `REQUIRE`,
  `LIVE_SUPPORT`) and the trailing `; text; 1 or 0; only with X` comments are **metadata for CM**: the DLL has no string for
  `FULLNAME` / `SHORT_DESCRIPTION` / `PREVIEW` (grep of `dll_strings.tsv`: none). CM builds its settings pages from the shipped
  `extension/config/*.ini` and writes only changed keys to `Documents\Assetto Corsa\cfg\extension\<module>.ini`; the DLL reads both
  (`attach.md` section 4 of FACTS). `[interp]` (the file watcher `WATCH_FOR_CHANGES=1` applies edits live except where a
  module says `LIVE_SUPPORT=0`, e.g. `splashscreen.ini`). On this PC the user's `gui.ini` is empty (0 bytes), so every default applies.
* **In-game** there is no general "edit every ini key" page in this build. What exists: (a) per-app settings windows (`FUNCTION_SETTINGS`,
  cog button; e.g. "Taskbar Settings" at 0xa35d40, 4,024 B; `%s settings##MainMenuChatS` for the chat; the CspDebug window "View &
  Video Settings" with FPS / V-sync / frame limiter diagnostics and a report export); (b) the new main menu's **Settings**
  tab and "About" tab (0x985a20, 3,661 B: AC version "v1.16.4 (x64)", "Custom Shaders Patch", credits from `data_credits.txt`); (c)
  per-module keys that apps edit with `ac.INIConfig` (script settings `extension/state/lua/<kind>/<name>__settings.ini`, key
  `__PLUGIN_SETTINGS=` in `splashscreen.ini`). The main menu Settings tab (function 0x986e70, 3,185 B, reads `gui.ini`)
  offers: select setup tabs with the mouse wheel, combined sliders, vertical setup tabs, new layout for car status in setup,
  track names for setup groups, lock mirrored items by default, load last setup automatically (setup named `ext_default`
  is loaded when AC starts: first from the track's folder, then the `generic` category), accent colour, "Bright Fluent
  effect", colored logo, use throttle to start the race (`USE_THROTTLE_TO_START`), show disconnected players in the time table. The
  keys are `main_menu.select_setup_tabs_by_scrolling`, `.combined_sliders`, `.vertical_tabs`, `.new_status_style`,
  `.use_track_names`, `.lock_mirrored_by_default`, `.load_last_setup_automatically`, `.show_disconnected` (strings
  0x2249710..0x22498a0); storage not located (probably the `ac.storage`-style state file) `[interp]`.
* "cui": the string `cui::imgui_offscreen` (0x2283400) is a type name registered by function 0x106acc0 ("New instance of ... created"):
  an off-screen render target class that ImGui windows with blur / cut-outs draw into. There is no other `cui::` string; "cui" is the
  DLL's namespace for its UI toolkit `[interp]`.

### 1.4 What changes in AC's own UI (summary; hook detail in section 2)

| Area | Change | Switch |
|---|---|---|
| Apps bar / taskbar | grouping (built-in / yours / developer), pin and hide with right mouse, sorting, slide-in popup, desktop buttons; or the new ImGui taskbar with search, "Hide all apps", glow on notifications, compact button | `taskbar.ini` (51 keys), `gui.ini [NEW_UI] REPLACE_TASKBAR=1` |
| App windows | no more windows lost outside the screen (forms are pulled back by 10 / 20 px), hit-test margin of 4 px around pinned buttons, per-desktop positions in a new file format, "pause hidden apps" for Python apps | always |
| ESC menu (pits menu) | replaced by CSP's "main menu" (ImGui) with session-information lines, rollback prompt, assists editor | `REPLACE_MAIN_MENU=1`, `ASSISTS_IN_MAIN_MENU=1` |
| Pause menu | new pause menu (Resume / Replay / Photo / Restart / Exit), optional camera shift, apps in the pause menu | `NEW_PAUSE_MENU/ENABLED`, `SHIFT_CAMERA`, `APPS_IN_PAUSE_MENU` |
| Race control, time table, leaderboards | sorted by real race position, disconnected players shown, chequered flag early, final lap tweaks, new ImGui time table with deltas and CSV export | `SORT_LEADERBOARD`, `SHOW_CHEQUERED_FLAG_EARLY`, `FINAL_LAP_TWEAK` |
| Setup screen | extra tabs kinds tagged, toe from real geometry, ABS levels count from the car, CSP section in setup files (`__EXT_PATCH`), reset / load / save events, `ext_default` auto-load, setups list with description, CSV export | always (+ main-menu options above) |
| Quick pit menu | custom wing labels, extra header lines, controller (D-pad) control, Lua can disable it or send input | `CONTROLLER_INTEGRATION_PITS`, gate byte 0x18268b14d |
| Chat | Lua / CSP chat app: attachments, emoticons (`data_emoji.txt`), setups sharing, admin sign-in, ballast / kick / ban / teleport, driver tags from CM `Friends.data`, chat shortcuts (`chat_shortcuts.ini`, 20 messages) | `APP_AC_CHAT=1`, `chat_shortcuts.ini ENABLED=0` here |
| HUD messages | system / driver-status / session-status messages can be hidden; pit-limiter and low-fuel icons; pedal-overlap warning; "PLANK WORN PAST LEGAL LIMIT / LAP WILL NOT COUNT" message; wrong-way indicator and proximity indicator wrapped in profiler scopes | `gui.ini [HIDE]`, `[HIDE_ICONS]`, `[POPUP_PEDAL_WARNING]` |
| Driver tags | new tags drawn by CSP (VR-capable, mirror rendering, flags or AI level, size / distance multipliers); shader set `accDriverTag_*` (6) | `[DRIVER_TAGS] ENABLED=1` |
| Fonts / scaling | ImGui fonts built from Segoe UI / Verdana / Consolas + 3 embedded CJK-historical-living TTFs, scale 1..2, DirectWrite text for Lua | `UI_SCALE`, hidden `USE_CJK_FALLBACK2` |
| Loading screen | replaced; keeps pumping Windows messages so the window does not look frozen | `splashscreen.ini` (50 keys) |
| In-car dashboards | AnalogInstruments / DigitalInstruments / DigitalPanels hooks: realistic speedometer and tachometer, digital panels can be disabled per car, LED merge, scriptable displays | `car_instruments.ini` |
| Console | hide and disable (`HIDE_CONSOLE`), `ac.onConsoleInput` / `ac.consoleExecute` | `gui.ini [HIDE]` |
| Speed units | no dedicated unit switch found in the hooks; `ui_use_mph` is a parameter of the navigator / car display code at 0xc388b0 (with `ui_font_scale`, `ui_brightness`, `gps_*`, `map_*`) | per car |

### 1.5 The ImGui layer (what is verified)

* Back-end names: `imgui_impl_ac` (0x227e970), `imgui_impl_ac_ingame` (0x224d508; function 0xa451e0, 1,710 B, message "A draw attempt from
  a secondary thread" -> drawing is main-thread only), `imgui_impl_dx11_csp` (0x227e980). State file `imgui_settings.ini`
  (0x224c218; reader/writer 0xa177b0), font cache `cache\imgui.bin` (5,159,108 B on this PC; reader 0x4a7f00 checks "Version mismatch"
  / "Settings mismatch", writer 0x4a78e0), log "Loading cached IMGUI fonts: 228,528 μs".
* Fonts: function 0x4a85a0 (5,477 B) loads Windows `Fonts\segoeui.ttf`, `segoeuil_0.ttf` (light), `verdana.ttf`, `verdanai.ttf`,
  `consola.ttf` (paths from `SystemRoot` / `WinDir`) and the glyph list `config\data_imgui_symbols.txt` (3 lines of symbols
  and diacritics). Function 0x14ee730 builds the atlas ("Creating fonts atlas for CSP UI", a stage of start-up between
  "Miscellaneous preparation" and "CSP shaders"). Missing system fonts: dialog "Make sure Microsoft Core Fonts component is
  installed" with an offer to download `https://acstuff.club/u/blob/ac-fonts.zip` (0x4a7500, 0x4a9b10). The overlay archive 5 of the DLL
  holds `cjk.ttf` (16,326,844 B, deflated 9,786,705), `historical.ttf` (4,318,068 / 2,073,548) and `living.ttf` (6,007,824 /
  2,776,828): the "CJK fallback" of `gui.ini` (`USE_CJK_FALLBACK2`, commented: "slightly increases loading time and VRAM").
  Other text path: DirectWrite (`ui.dwrite*`, `ui.pushDWriteFont`, `real_DWriteCreateFactory`, custom font collection from
  `content\fonts\*.ttf` and `extension\fonts` at 0x11c52c0 / 0x6d9d20) and the FW1 glyph atlas
  (`FW1FontWrapper::CFW1Factory` hooks 2, `Font::*` hooks); see rendering briefs.
* Shaders (names, `.fxo` in the overlay archive 1): `uiImgui_vs / _ps / _ps_s / _outline_vs`, `uiImgui_point / tonemap /
  subtract / blur / sharpen _ps`, `uiImguiCustom_vs / _ps`, `uiImguiCustomGen_vs`, `accImguiBlur2_{downscale,blur0,blur1,fill,
  color,modal}_{vs,ps}`, `accImguiBlurGlow_ps`, `accImguiBlurGlow0_{ps,blur0_ps,blur1_ps}`, `accImguiShadowTest1_{vs,ps}`, plus states
  `Cull::ext_imgui`, `Blend::ext_imgui_shadow` and `acUITaskbarPreview_ps`, `accDriverTag_*`. They implement the `ui.beginBlurring`,
  `beginTonemapping`, `beginSharpening`, `beginSubtraction`, `beginOutline`, `glowRectFilled`, shadow and modal-dim effects.
* Input gating: global bytes / ints `0x1826a2877` (byte), `0x1826a4e44` (dword), `0x1825e17e9`, `0x1826951dc` are tested by
  `GUI::onMouseDown / Move / Up / onKeyDown`, `Control::render`, `GameScreen::render`, `ACTimeFlow::render`, `SessionLeaderboard`
  and others: AC's own handling and drawing of forms is skipped while they are set. `0x1826a2877` also stops `GameScreen::render`
  from drawing AC's forms, so it behaves like "UI hidden / ImGui owns the screen" (Ctrl+Shift+Alt+H `__EXT_HIDE_UI` or
  `HIDE_UI=1` ends in it) `[interp]`; `0x1826a4e44` is non-zero while a CSP window or modal consumes input `[interp]`.
  In Lua: `ui.captureMouse`, `ui.captureKeyboard`, `ui.mouseBusy`, `ui.passthroughIMGUI`, `ui.windowFocused`, `ac.blockEscapeButton`.
* Rendering order: `ksgui::GUI::render` (hook 0x6129b0) -> CSP's `doRender2D` (0x6127a0) which re-creates AC's blend / depth /
  rasteriser state through the device wrapper at `*0x1826aa890` (vtable +0x118 / +0x158 / +0x120), then runs a CSP routine 0x612480
  (after the original forms) that is the ImGui frame `[interp]`. The frame draws from the 2D pass; in VR AC's
  `preRender3D / render3D / renderVPointer` are wrapped to put it on the VR plane. `gui.ini DRAW_UI_LAST=1` moves CSP's UI above AC HUD objects
  (driver tags, proximity indicator, quick pits menu).
* A per-frame profiler scope list (global vector at `0x182691ec8`, stack top `0x182691ed0`; scope objects created at 0x1810e65f0 /
  0x1807ac930, names "Quick Pits menu", "Python apps update/<app>") feeds the "Render Stats CSP" app; hooks push a named
  scope around their original call (QuickMenuPitstop::renderHUD, PythonForm::render, PyPlugin::update, TrackMap::render,
  OverlayLeaderboard::renderHUD, ProximityIndicator::renderHUD). `[dll]`

## 2. Game parts it replaces or extends

Category letters as in `physics_car_hooks.md`: A = pass-through that records state / fires CSP events, B = wraps the original
and changes inputs or outputs, C = replaces (original not called, or only conditionally), D = lookup. `H` = hook, `L` = lookup.
Slots are `.data` cells of dwrite.dll; the original is called through them. The group has **207 rows: 177 hooks, 30 lookups,
89 classes, 151 distinct replacement functions** (150 real ones plus the 3-byte stub `0x305980` used by 27 rows). Per library:
SystemApps.lib 58 H, ksGUI.lib 38 H + 10 L, 36 object files (`AnalogInstruments.obj` 4 H + 6 L, `QuickMenuPitstop.obj` 6 H + 3 L,
`SetupScreen.obj` 6 H + 1 L, `SplashScreen.obj` 5 H, ...). Decompile volume of the 151 functions: 339,634 bytes, 34 files over
3 KB; all read at least by their shape, the 20 largest read through.

### 2.0 Gating and the event bus

* **Gate records** (`attach.md` 1.7): of the 207 rows, 175 are unconditional. 27 are inside **one** `#if` block (registration
  sites 0x263dce..0x2657f7) whose predicate `0x6dab40` is "`*settings0[0] != 0` or global byte at 0x1826b9917 != 0", where
  `settings0 = 0xef1700()` returns a static start-up options object (`0x1825eb3e0`) whose byte 0 is the **headless** switch:
  the key `HEADLESS_MODE` is read by the settings loader 0x1387350 (together with `SINGLE_THREAD`, `NO_DIRECT_INPUT`,
  `DISABLE_INPUT`, `INCLUDE_APPS`, `NO_SHARED_MEMORY`, `ENABLE_PYTHON`) when the environment variable `AC_TOOL_RUN` is set; log
  strings "Starting tool run" / "Exiting tool run"; `DAT_1826b8a18` receives the same value. Byte 3 of that object is a second
  "special mode" bit (showroom / splitscreen / replay-only: `deserializeForm` skips layout restore, `Car::getSteerFF` returns 0).
  `[dll][interp]` Five more rows carry their own single predicate: `Console::show` (byte 0x1826951e2, 8 B predicate 0x5a6970),
  `PauseMenu::PauseMenu` (byte 0x182695073, 0x681360), `PostProcessFilterSelector::~` (byte 3 of the options object, 0x59dde0),
  `QuickMenuPitstop::itemWing` (byte 0x18268b14d, 0x697df0), `FriendsLeaderboardDisplayer` ctor (two predicates 0x6c7ca0 and
  0x6f4e90: byte 0x182695220). The bytes in the 0x182695xxx block are CSP's own settings flags written by the `gui.ini`
  loaders (0xce56c0, 5,039 B; 0xce6b00, 6,268 B) `[interp]`; I could not map byte to key without those two functions (requests).
* **Event bus**: many hooks iterate a global vector of 40-byte listener records (callable at +8) from the last to the first,
  passing a pointer to a small argument struct, and some stop when a listener sets a "handled" byte (cancelable). RTTI names
  `AC::Event<T>`, `listener_impl ... subscribe`, `ac_function` confirm a typed event class `[dll]`. The vectors found, with the
  hook that fires them (all `.data`, base 0x180000000): `0x182690b40` `Console::parse` (cancelable; Lua `ac.onConsoleInput`
  `[interp]`), `0x182690b50` `DigitalInstruments::initInstruments`, `0x182690b58` `DigitalInstruments::update`, `0x182690b60`
  `DigitalItem::update`, `0x182690b98` `DriverNameDisplayer::renderHUD` (cancelable; `ui.onDriverNameTag` `[interp]`),
  `0x182690ba0/ba8` `EndSessionDisplayer::renderHUD` (before / after), `0x182690bc8` `ESCMenu::setRaceControlScreen`, `0x182690d20`
  `GameScreen::showDesktop`, `0x182690d48` `GUI::render3D`, `0x182690d50` `Taskbar::selectVirtualDesktop`, `0x182690f48`
  `LapInvalidatorAvatar::renderHUD`, `0x182691048` `PauseMenu::onVisibleChanged`, `0x182691050` `PauseMenu::render`,
  `0x182691088` `ReplayInterface::render` (cancelable), `0x182691098` `ScreenCapturer::getPath`, `0x182691100`
  `WrongWayIndicator::update` (cancelable), `0x182697318` `Spinner::setValue` (setup values), `0x182697408` / `0x182697418`
  `SetupScreen::loadSetupAbsolutePath / saveSetupAbsolutePath`, `0x1826974a0` `SplashScreen::loadHints`, `0x1826974a8/b0`
  `SplashScreen::renderHUD` (cancelable pre, plain post), `0x18268ff30` / `0x18268ff38` `AnalogInstruments::updateSpeed / updateRPM`
  (return an override value; see 2.6), `0x1826a4990` (stack read by `RaceControlScreen::render`). Who subscribes (Lua `ac.on*`, the
  ImGui apps) was not traced. `[dll]` for the list, `[interp]` for the Lua names.

### 2.1 `ksgui::GUI` and the form layer (`ksGUI.lib`; 15 + 4 + 3 hooks)

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `GUI::GUI` | 0x140251ac0 | 0x6107e0 | A | original, then stores `this` in `0x1826a4e80` (GUI singleton), zeroes the word at +10, calls 0x614ce0 |
| `GUI::onMouseDown` | 0x140254290 | 0x610810 | C | ignores the click while the UI-hidden byte / capture counter is set; otherwise its own dispatch: drops a captured control, sends vtable +0x50 to the taskbar and to each top-level form, records drag start in `0x1826a4e88/e90`, clears the travel counter `0x1826a4e00`; original never called |
| `GUI::onMouseMove` | 0x140254320 | 0x610e90 | B | same gate; folds coordinates >= 20000 back (-0x999a, virtual-desktop encoding), accumulates travel in `0x1826a4e00` (click vs drag), keeps forms with certain flags from moving by clearing their visible byte during the original (slot 0x18268b2a8), then clamps dragged form to the screen (20 px margin) |
| `GUI::onMouseUp` | 0x1402545c0 | 0x611580 | B | click if travel < 4 px; after a drag pulls the form back inside the screen and logs "GUI::onMouseUp: fixing position from ..."; original (slot 0x18268b2b0) |
| `GUI::onKeyDown` | 0x140254230 | 0x6117e0 | B | gate, bracketed by 0x6100c0 / 0x610070 (CSP key-capture), then original |
| `GUI::render` | 0x140254c10 | 0x6129b0 | C | if no VR target (`gui+0x118 == 0`) saves and sets `gui+8 = 1`, calls CSP's `doRender2D` instead |
| `GUI::doRender2D` | 0x1402539d0 | 0x6127a0 | C | AC's state setup rewritten (blend 0 / 1, depth, rasteriser through the device wrapper), then CSP's frame function 0x612480 when the 3D-GUI plane is absent |
| `GUI::preRender3D` | 0x1402547c0 | 0x612720 | B | VR: binds the GUI render target through device vtable +400, forces `gui+8 = 1` around the original (slot 0x1826a4ec8) |
| `GUI::render3D` | 0x140254900 | 0x611bb0 | B | VR plane; fires event 0x182690d48 (899 B; body only skimmed) |
| `GUI::renderVPointer` | 0x140254c30 | 0x6120c0 | C | VR head pointer re-implemented: fade by distance (`0.1 / 1.1111112`), colour pair from `0x1825e17ec / 17f8`, size grows with distance (`3.125e-7 * d^2`) |
| `GUI::fromIni` | 0x140253b20 | 0x612a00 | C | reads `POSX POSY VISIBLE BLOCKED SCALE` per form from `acos.ini` (AC's keys) |
| `GUI::deserializeForm` / `deserializeFormDesktop` | 0x140252f90 / 0x1402533a0 | 0x614210 / 0x614270 | C | position, scale, visible, blocked of a form come from CSP's per-name / per-desktop cache (0x613490); in "special mode" (options byte 3) the form is simply hidden |
| `GUI::serializeMemoryForm` | 0x1402553e0 | 0x6142c0 | C | stores the form into the same cache (x, y, visible, blocked, scale at +0x184) |
| `GUI::serializeForms` | 0x140254d60 | 0x614310 | C | writes `acos.ini` / `acos_splitscreen.ini` / `acos_showroom.ini` with `[HEADER] VERSION=2`, `DESKTOP_SELECTED=n` and one entry per (form name hash, desktop index); skipped in special mode or when the form list is not the default one |
| `ksgui::isFormInside` | 0x140254070 | 0x610950 | C | rectangle overlap (196 B, pure) |
| `ksgui::isPointInside` | 0x140254190 | 0x2f5bb0 | C | constant false (3-byte stub); AC's point test is disabled, CSP uses `Form::hitTest` |
| `Form::Form` | 0x140245890 | 0x60a990 | B | counts forms (`0x1826a4dc0`); on the first form named `HIDE_APPS` publishes the start-up step "Apps" and raises the progress fraction `0x1825e272c` to 0.9; zeroes +0x18a/+0x18c; pulls a form with x < 0 back (deferred call 0x1811bfc40) |
| `Form::~Form` | 0x1402463c0 | 0x60ada0 | A | counter, tail call |
| `Form::onVisibleChanged` | 0x140246860 | 0x60adb0 | B | if the form is outside the screen minus 10 px, moves it back |
| `Form::hitTest` | 0x1402466a0 | 0x623760 | C | full re-implementation: form rectangle, plus 4 px margin around the title-button rects at +0x198 / +0x1a0 / +0x1a8 (only when the GUI is visible) |
| `GameScreen::GameScreen` | 0x140250bd0 | 0x60da50 | A | singleton capture `0x1826a4de8` |
| `GameScreen::render` | 0x140250de0 | 0x60db50 | C | draws AC's forms only when not hidden (`0x1826a2877`, `0x1825e17e9`); records "visible" in `0x1826a2876` |
| `GameScreen::showDesktop` | 0x140250e70 | 0x60dae0 | A | event 0x182690d20 after original |
| `Control::render` | 0x140244b80 | 0x10dfc20 | B | before original, for controls with children and no capture: CSP helper 0xf47420 (tooltips / drag hints `[interp]`) |
| `Spinner::Spinner` | 0x14024bac0 | 0x10dfda0 | B | custom colour from `0x1826ab830..834` (spinner skin); `-1` index at +0x24c |
| `Spinner::setValue` (2 overloads) | 0x14024dd30 | 0x10dfe20 | B | marks setup dirty `0x1825e07ef`; fires event 0x182697318 with (kind, value); when kind 1 and the setup screen is visible copies +0x3614 to +0x3618 of the focused car's state |
| `Spinner::render` | 0x14024cdd0 | 0x10e0350 | B | CSP helper 0x10dff10 first |
| `TabBar::orderAlphabetically` | 0x140258500 | 0x1133cb0 | C | does not sort now: queues a deferred call (0x1811bfc40) that CSP runs later |
| `Control::Control / onMouseMove / setPosition / setSize / setVisible`, `ListBox::addItem / scrollListBoxToItem`, `TabBar::addTab`, `Taskbar::addDesktop / cycleDekstop` | 0x140244150, 0x1402449b0, 0x1402450a0, 0x140245120, 0x1402451a0, 0x1402495f0, 0x14024a030, 0x140258120, 0x1402599f0, 0x140259df0 | - | D | called by CSP's own code (new apps, desktop switching, the chat list) |

### 2.2 Taskbar (`ksgui::Taskbar`, `TaskBarIcon`)

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `Taskbar::Taskbar` | 0x140258a20 | 0x619510 | B | after the original reads `gui.ini [NEW_UI]`: if `APPS_IN_PAUSE_MENU` and `REPLACE_TASKBAR` are both on, adds a 4th ("pause") desktop (index 4, 10000) |
| `Taskbar::render` | 0x14025a050 | 0x619950 | C when new style | CSP's old-style animated taskbar (slide popup, per-icon alpha, desktop button layout from `taskbar.ini [LAYOUT_*]` in globals around 0x182690d58..0x182690f0c); returns at once when the new ImGui taskbar is active (0x619460) |
| `Taskbar::updateAppIconsPosition` | 0x14025a360 | 0x61a0b0 | C when new style | lays icons in groups (pinned / apps / hidden) with spacing from `ITEM_HEIGHT`; hides icons of closed groups at y = -200 |
| `Taskbar::mouseWheelMoved` | 0x140259eb0 | 0x61a3f0 | C | scroll with `SCROLL_WHEEL_SPEED`, clamped to content height |
| `Taskbar::addForm` | 0x140259c10 | 0x61a420 | C | registers the form in CSP's app list (0x619310) instead of AC's icon row |
| `Taskbar::onMouseMove` | 0x140259f70 | 0x61a6b0 | B | hover logic for the popup (`THRESHOLD_SHOW / HIDE`, delays) |
| `Taskbar::selectVirtualDesktop` | 0x14025a2b0 | 0x61a7e0 | A | original, then event 0x182690d50 (desktop index) |
| `TaskBarIcon::render` | 0x14025c8d0 | 0x61dfa0 | B | context menu text "Pin app in the list" / "Hide app from the list"; icon drawing with `NEW_ICONS_STYLE` |

The new ImGui taskbar is 0xa2bd10, 0xa2cd90, 0xa30580 (2,886 B), 0xa32b70 (7,034 B), 0xa34fd0, 0xa35d40, 0xa36e10 (5,818 B). Strings: "Built-in
apps", "Developer apps", "Your apps", "Lua app: %s", "Python app: %s", "Open the app folder", "Remove this app", "Never pin this
app", "Search for apps", "Hide all apps", "Window %s renamed to %s" (custom names `_TASKBAR_RENAMES`), pinned list
`TASKBAR_APPS_<n>`, `TASKBAR_NEVER_PIN`, "Dynamic desktop icon", settings: show unpin button, notifications glow
(for missed chat messages and notifications from Lua apps), placement bottom / top-left / bottom-left / top-right, compact
apps button, align icons, desktops buttons, "Mark new apps". Toggle hotkey `__EXT_TOGGLE_TASKBAR` (Alt+H). `[dll][ini]`

### 2.3 AC's pre-race and in-race screens

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `ESCMenu::ESCMenu` | 0x1400fcb40 | 0x5cd4a0 | B | when the headless byte is set, sets the global byte `0x1826a945a` during the original (loading shortcut) |
| `ESCMenu::render` | 0x1400fe420 | 0x5cdcf0 | B / C | first: if the "reset setup" request `0x1825ea014` is set, applies default values to every tab; then if the new main menu is off draws AC's menu (original, with a flag set at +0x201 of the tab holder when `0x18268ad98` bit 0) only when the focused car state allows; with the new main menu on CSP's function 0x11be370 draws instead (temporarily forces the car field +0xa40 to 1 and `0x1826fe850` to 1) |
| `ESCMenu::setRaceControlScreen` | 0x1400ff2b0 | 0x5ce310 | B | event 0x182690bc8 around the original; while the CSP flag pair (`0x1826a2c46`, `0x1826950fd`) is set and the focused car is faster than 1 km/h (`speed*3.6 > 1`) nothing is done, so race control does not open while driving [interp] |
| `EscMenuTimeTable::refreshPlayer / refreshSession / render` | 0x140096f60 / 0x1400972f0 / 0x140097660 | - | D | CSP's own time table (0x999600, 0x99ab30, 0x9a6da0, 0x9a8240, 0x9a9ec0) calls AC's data builders |
| `RaceControlScreen::render` | 0x1400b29c0 | 0x6997e0 | B | after original, when a race-control session exists and the screen is up, draws two extra panels: "Assists" (title) and "Chat" (when the chat list is non-empty) at fixed offsets (+30 / +54 px under the form rectangles, 363 px minimum height) |
| `EndSessionDisplayer::displayResults` | 0x14010b260 | 0x5c9f30 | A | tail-call stub |
| `EndSessionDisplayer::renderHUD` | 0x14010b520 | 0x5c9f40 | B | events before / after (0x182690ba0 / ba8); hidden while `0x18268a836` set; temporarily writes a value into `+0xe8` of a sub-form |
| `PauseMenu::PauseMenu` | 0x14011d9b0 | 0x681370 | B | gated by 0x182695073; calls original with the pointer field +0x1c8 of its argument zeroed |
| `PauseMenu::onVisibleChanged` | 0x14011f870 | 0x6813b0 | B | sets `0x1826a6ce9`; when the screen mode is 9 marks the button `PITS`; original; event 0x182691048 |
| `PauseMenu::render` | 0x14011f990 | 0x681500 | B | skipped in headless; labels of buttons `PITS` and `RESTART` get " (not allowed in this race)" when pits teleport / restart are blocked (flags 0x1826950fb, 0x182697ec8); event 0x182691050 |
| `PauseMenu::getButtonByName` | 0x14011f670 | - | D | used by the hook above |
| `ReplayInterface::ReplayInterface` | 0x140148300 | 0x69a040 | B | headless shortcut as ESCMenu; stores `this` |
| `ReplayInterface::render` | 0x14014d970 | 0x69a0e0 | B | cancelable event 0x182691088; replay-rewind / jump-to-time keys via the controller (`CONTROLLER_INTEGRATION_REPLAY`) |
| `ScreenCapturer::*` (4) | 0x140162560, 0x140162be0, 0x1401631a0, 0x140163470 | 0x69a900, 0x69a940, 0x69aa40, 0x69aa50 | B | screenshot path (event 0x182691098), debounce of `takeScreen` (0.5 s), CSP's capture routes (`Nice Screenshots`, `__EXT_SCREENSHOT_ALT`) |
| `SessionLeaderboard::renderHUD` | 0x1400bb3c0 | 0x429090 | B | drawn only when CSP agrees (0x4290c0); copies the session time into `0x1826998a0`; control helper 0xf47420 |
| `OverlayLeaderboard::OverlayLeaderboard` | 0x1401bf440 | 0x518740 | A | singleton `0x1826a2af8` |
| `OverlayLeaderboard::renderHUD` | 0x1401c1350 | 0x518760 | B | profiler scope; suppressed while `0x1826a2b00` is non-zero (Lua `ac.accessOverlayLeaderboardParams` / `app_leaderboard` replaces it) |
| `FriendsLeaderboardDisplayer` ctor | 0x140324fc0 | 0x6f5170 | B | singleton capture; gated |
| `LapInvalidatorAvatar` ctor / `renderHUD` | 0x140111990 / 0x140111c00 | 0x621960 / 0x621ad0 | A / B | event 0x182690f48; messages "PLANK WORN PAST LEGAL LIMIT" and "LAP WILL NOT COUNT" with a 1,400 B layout of its own |
| `LockControlsTimeAvatar::renderHUD` | 0x14009dc50 | 0x622260 | B | countdown text from `lockControlsTime / 1000` with a CSP formatter (0x180622460) while controls are locked |
| `ProximityIndicator::renderHUD` | 0x140123a80 | 0x55c640 | B | profiler scope; skipped when `0x1826a2c00` |
| `WrongWayIndicator::update` | 0x1401d8940 | 0x6bc460 | B | cancelable event 0x182691100; resets +0x78 when `0x1826a85b0` |
| `SystemMessage::SystemMessage / renderHUD / addMessage` | 0x1401b5610 / 0x1401b6520 / 0x1401b61c0 | 0x6a3da0 / 0x6a3de0 / 0x6a3e00 | A / B / B | singleton; hide switch `0x1826a2c44` (`HIDE_SYSTEM_MESSAGES`); messages of type 5..7 (votes) get " (press ...). Press key "Y" or "N" to vote" and a 5 s duration |
| `DriverStatusSystemMessage::render` | 0x14008f800 | 0x5bb000 | B | skipped when `0x1826997ec` (`HIDE_DRIVER_STATUS_MESSAGE`) |
| `SessionStatusSystemMessage::render` | 0x140169ac0 | 0x69d8d0 | B | skipped when the session object byte +0x14 is set |
| `PlayerCurrentTime::render / getSectorSplitsString` | 0x1402da350 / 0x1402da220 | 0x5e65f0 / 0x5e6570 | B | shows the times of the focused or selected car instead of car 0 and adjusts the lap counter field `+0x8c0` by flag `+0xfe5` for the call [interp]; empty split string when no valid lap |
| `MicroSectors::update` | 0x14009fe20 | 0x5e6700 | B | skipped when no lap timing is active |
| `FormChat::FormChat` / `onChatMessage` | 0x140308190 / 0x14030bd90 | 0x5dbbf0 / 0x5dbc80 | C / B | returns 0 in headless; with `APP_AC_CHAT=1` the created form is put into CSP's "replaced form" list `0x1826add20..28` (AC's chat form is hidden, the ImGui chat takes over); incoming chat is offered to CSP (0x574150) first and swallowed when consumed |

### 2.4 AC apps replaced by CSP versions, and developer apps

* **Replaced** (constructor hook pushes the new form pointer into the global vector `0x1826add20` when the replacement is on, so a
  later pass hides it; the original runs anyway): `FormChat` (switch byte `0x1826997ef`, key `APP_AC_CHAT`), `FormVolumes`
  (`0x18269817d`, `APP_AC_AUDIO`; hook 0x42c420, C when headless), `PhotoMode` (`0x1826a2b04`, `APP_AC_CAMERA_PICTURE_MODE`, 0x682b80),
  `PostProcessFilterSelector` (`0x1826a2b06`, `APP_AC_POST_PROCESS_FILTER_SELECTOR`, 0x691c90; dtor hook 0x2db1f0 is the shared
  `_guard_check_icall` no-op), `ksgui::FormRenderStats` (`0x18268ad9d`, `APP_AC_CSPRSTATS`, 0x60d800; render 0x60d890 draws
  "Render Stats (AC)"). `[dll]` switch bytes, `[ini]` keys.
* **Developer / debug apps switched off** in headless tool runs by hook `0x305980` (3-byte `xor eax,eax; ret`, so the constructor
  returns null): 27 constructors, listed here with acs VA: `ACDriverPerformance` 0x140303bd0, `ACEssentials` 0x14031bc60, `ACHelp`
  0x140307900, `ACTimeFlow` 0x14030cb10, `CarPhysicsForm` 0x1402d7680, `FFBForm` 0x140314a10, `FormAI` 0x1402dcf80, `FormBallast`
  0x1402f0ef0, `FormDRSZones` 0x140336210, `FormGameStats` 0x1402e7a10, `FormGear` 0x1402e8280, `FormLogoAC` 0x1402ec650, `FormMGU`
  0x1402eb080, `FormRealTimeTiming` 0x14032b8a0, `FormRunningTime` 0x1402dc760, `FormTelemetry` 0x14032f660, `LeaderboardApp`
  0x140329bf0, `NetMonitorForm` 0x140303400, `PlayerCurrentTime` 0x1402d9660, `RaceFuelMonitor` 0x140338250, `RacePosition`
  0x1402fe0f0, `SetupDebug` 0x140317a80, `TelemetryApp` 0x140302180, `TrackDescription` 0x1403270e0, `TyresApp` 0x140338860,
  `TyreTesterForm` 0x14033b520, `TyreWearingDebug` 0x140319960. Category C (always null when the gate holds). The stub is a trick
  to disable them without a flag in each: the whole block sits inside the single `#if` of 2.0 `[dll][interp]`.
* **Wrapped renders** (B): `ACEssentials::render` 0x14031d670 / 0x6daf80 and `CarLabel::render` 0x1402468e0 / 0x6dab70 write the
  focused car's custom gear label (car state string at +0xc0, one or two characters; `ac.setGearLabel`, Python
  `ac.ext_getCarGearLabel`) into their label and rescale the font (x0.4898, x1.5) `[interp]`; `ACSuspension::render`
  0x1402e4ad0 / 0x580bc0 sets the CSP "current car" global `0x1825e1f68` around the original (restores `0xffffffff`);
  `ACSuspension::renderWireframe` 0x1402e6da0 / 0x580bf0 (898 B, not fully read); `FormGear::render` 0x1402e9ac0 / 0x6db1a0 sets
  `0x1826a90a8 = this` around the original; `FormAI::render` 0x1402e1480 / 0x6d5b70 temporarily overwrites two AI floats
  of the focused car (+0x4c8, +0x4cc) from globals `0x1825e1f74/78` (a scope guard restores them) and sets the car index
  `0x1825e1f70`; `FormAI::renderSplines` 0x1402e3920 / 0x6d5e30 (153 B); `ACTimeFlow::render` 0x14030d270 / 0x11154c0 shows "(x%.1f)" and
  "Time paused" and applies CSP's weather time multiplier; `FormWings::render` 0x1402edff0 / 0x1116030 draws the wing
  gauges for the focused car's wings and the 4 per-wheel items; `CarPedalsForm::render` 0x1402dc4d0 / 0x1095200
  (`INCLUDE_HANDBRAKE_IN_PEDALS_APP`); `TrackMap::TrackMap / render / createMap` 0x1402ee8e0 / 0x1402efd50 / 0x1402eeee0 at
  0x6a7b60 / 0x6a7be0 / 0x6a7c90 (profiler scope, `createMap` only when `0x182695070`, null in headless);
  `FormCamera::FormCamera` 0x1402f1bc0 / 0x5dbb90 (singleton `0x182690be8`); `DriverEyesPositioner` ctor / `saveSettings` /
  `saveToINI` / `onRenderFinished` 0x14030fa60 / 0x140313920 / 0x1403144e0 / 0x140312620 at 0x5b7a90 / 0x5b7ab0 / 0x5b7ed0 /
  0xeef090 (saves `view.ini` keys `ON_BOARD_YAW_ANGLE`, `CAMERA...` with the "saving" flag `0x18268a834` set; the on-board camera
  tuning also exposed to Lua as `ac.getOnboardCameraParams`); `TripleScreenManager` ctor / dtor / `onSpinnerValueChanged`
  0x1402ff920 / 0x140300710 / 0x140301b90 at 0x6a7f70 / 0x6a7f90 (15,722 B, writes `triple_screen.ini` keys `ACTIVE`,
  `TRIPLE_SCREEN`, `LEFT_POINT`, `LEFT_SCREEN`, `CENTER_SCREEN`, `RIGHT_POINT`, `RIGHT_SCREEN`, spinners `SP_MARGIN`,
  `SP_ROTATION`, `SP_DISTANCE`, `SP_WIDTH`) / 0x6a7fa0; `FriendsLeaderboardDisplayer` above.

### 2.5 Setup screen (`SetupScreen.obj` family, 22 rows)

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `SetupScreen::SetupScreen` | 0x14017c010 | 0x10f61c0 | B | two function-local statics (0x182700a9c / ab0), singleton `0x1826abaa8`, extra call 0x7fbd70 when a car exists |
| `SetupScreen::loadINI` | 0x14017d950 | 0x10f6410 | B | sets a thread-local "loading setup.ini" byte (`TLS+0x1a9`) around the original when the car has an extension object; afterwards re-reads `setup.ini` itself for entries of type 1 with keys `QUICK_MENU_WINGS_LABEL`, `_EXTENSION`, `SHOW_CLICKS`, `STEP` and attaches CSP data |
| `SetupScreen::render` | 0x14017f7c0 | 0x10f6a40 | B | CSP helper 0x10f9cb0 first |
| `SetupScreen::resetTabsValueToDefault` | 0x14017f950 | 0x10f6300 | B | original, then per tab (kind != 6) a hash lookup and "dirty" notification, clears tab bit 0 at +0x17a, `0x1825e07ef = 1` |
| `SetupScreen::loadSetupAbsolutePath` | 0x14017f2c0 | 0x10f6a70 | B | writes log "Load setup: <path>"; the loader knows the section `__EXT_PATCH` of a setup file (CSP's own extra setup values); event 0x182697408 |
| `SetupScreen::saveSetupAbsolutePath` | 0x14017f9c0 | 0x10f83c0 | B | same for saving (1,612 B), event 0x182697418 |
| `SetupScreen::getSetupTab` | 0x14017d650 | - | D | lookup of a tab by name |
| `SetupTab::SetupTab` | 0x140183130 | 0x10fc2a0 | B | tab kind byte at +0x179 := 0 (generic) |
| `SetupTab::addItem` | 0x140183850 | 0x10fc2d0 | B | original, then registers the new item in a hash map keyed by the item name (open-addressing table at 0x1825e7e30, 64-bit golden-ratio hash) for script access by name |
| `SetupIOManager` ctor | 0x140174c90 | 0x69f5a0 | B | kind byte +0x179 := 1; when `0x18268b14e` is set a deferred action (0x582420) adds the CSP setups list |
| `SetupGearManager` ctor / `addSetupItem` | 0x140170c80 / 0x140171f60 | 0x69dbc0 / 0x69dc30 | B | kind 2; `addSetupItem` handles ratio files under `/data/` and logs "File is missing: %s" |
| `SetupTyresManager` ctor | 0x140184650 | 0x69fe30 | B | kind 3 |
| `SetupFuelManager` ctor | 0x14016d880 | 0x69db70 | B | kind 4 |
| `SetupElectronics` ctor / `addABS` | 0x14016a020 / 0x14016b900 | 0x69d8f0 / 0x69d940 | B | kind 5; `addABS` temporarily replaces the ABS spinner's level count by the number of entries of the car's CSP ABS level list (`ext+0x998..0x9a0`, `size/4`) and defers a rebuild |
| `SetupPitstopPresets` ctor | 0x1401787d0 | 0x69d920 | B | kind 6 |
| `SetupPitstopPresets::syncSpinners / syncCarPitstop` | 0x14017ac00 / 0x14017aa70 | - | D | called by CSP's quick-pit controller |
| `SetupInformation` ctor / `getTOE` | 0x14017fcf0 / 0x140180580 | 0x69e9c0 / 0x69ea00 | A / C | `getTOE` returns the toe angle computed from the hub vectors of the extended suspension object (degrees, sign by side, NaN-guarded) instead of AC's value |
| `SetupIOManager::refreshSetupListBox` | 0x140177290 | - | D | called by CSP after save / delete from the ImGui setups list |
| `SpinnerData::SpinnerData` | 0x14016a400 | 0x6a2a50 | A | stub tail call |
| `QuickMenuPitstop::*` (6 H, 3 L) | ctor 0x1400aca70, `buildControls` 0x1400ae7b0, `addHeader` 0x1400ae6f0, `itemWing` 0x1400b0750, `renderHUD` 0x1400b1230, `setLabels` 0x1400b16d0; lookups `buildItem` 0x1400aff20, `sendInput` 0x1400b1460, `QuickMenuPitstopItem` ctor 0x1400ad760 (2 overloads) | 0x696f30, 0x6968b0, 0x696c50, 0x697e00, 0x696f60, 0x697340 | A / B | singleton `0x1826a6ee8`; header strings " TYRES", " CHANGE", `EXT_CHANGE_SIDE` (extra tyre side line); `setLabels` asks CSP (0x697230) for a custom label when `0x18268b14d`; `itemWing` replaced when the same byte is set; `renderHUD` in a scope "Quick Pits menu", honours a "suspended" counter `0x1826a6f10`; `ac.disableQuickMenuPitstop`, `ui.base.QuickMenuPitstopInput(button)` (0 = open the menu) drive it |

### 2.6 In-car dashboards (`AnalogInstruments`, `DigitalInstruments`, `DigitalItem`, `DigitalLed`, `DigitalPanels`)

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `AnalogInstruments::AnalogInstruments` | 0x140056480 | 0x58abf0 | B | stores `this` into the per-car CSP object (`*0x1826a9d50+0x18`), zeroes +0x46c / +0x469, original, then CSP's init (0x58aaa0, 0x58b600) |
| `AnalogInstruments::update` | 0x140059770 | 0x58ac40 | B | for the spectated / selected car temporarily swaps `+0x220` of the physics holder so the needles follow that car; sets +0x469 |
| `AnalogInstruments::updateRPM` | 0x140059ba0 | 0x58ace0 | B | event 0x18268ff38 returns an override RPM + flag (impulse tachometers `USE_CORRECT_TACHOMETERS`); writes it to `+0x4ac` around the original; two-pass draw for a second needle (+0x318) |
| `AnalogInstruments::updateSpeed` | 0x140059e30 | 0x58aea0 | B | event 0x18268ff30 returns a speed override (driveshaft-based speedometer `USE_CORRECT_SPEEDOMETERS`), written to `+0x4d8` |
| `AnalogInstruments::initFuel / initRPM / initSpeed / initTurbo / initWater / initPlaceHolders` | 0x1400572e0, 0x140057e70, 0x1400584e0, 0x140058930, 0x140058b40, 0x140057ae0 | - | D | CSP builds extra needles with AC's constructors |
| `DigitalInstruments` ctor / `initInstruments` / `update` / `clearInstruments` | 0x1400eb030 / 0x1400eb510 / 0x1400f0520 / 0x1400eb3f0 | 0x5a9190 / 0x5aa6d0 / 0x5aadf0 / - | B / B / B / D | ctor calls the original through 0x5a91c0 then registers the instance (0x5ab450); `initInstruments` reads the ini group `Tweaks.LEDGroup`, fires event 0x182690b50; `update` fires 0x182690b58, in headless sets `car+0x8c0` to 0 around the draw |
| `DigitalItem::DigitalItem` / `update` / `cleanup` | 0x1400f0690 / 0x1400f3190 / 0x1400f3100 | 0x5ad790 / 0x5ad7c0 / 0x5ada10 | B | clears +0x34; event 0x182690b60; cleanup also releases the two attached nodes (+0x38, +0x48) via their owner |
| `DigitalLed::update` | 0x1400f6200 | 0x5addc0 | B | skipped unless the shift-light intensity is non-zero or the car's light state is 3 (blinking) |
| `DigitalPanels::initPanels` | 0x140082740 | 0x5ade00 | B | honours `DISABLE_DIGITALPANELSINI` (key read from the car's `DATA` ini; a car can opt out of AC's digital panels) |
| `DigitalPanels::update` | 0x140083f70 | 0x5ae270 | B | 106 B guard |
| `DriverNameDisplayer` ctor / `projectPoint` / `renderHUD` / `setActive` | 0x14008c7e0 / 0x14008d550 / 0x14008d900 / 0x14008e070 | 0x5ba8e0 / 0x5ba920 / 0x5baa30 / 0x5bac70 | B / C / B / B | with `[DRIVER_TAGS] ENABLED` (byte 0x1826a4901 / 4902): own point projection (unit vector from the camera, offset globals `0x1825e2f08..10`), renderHUD cancelable event 0x182690b98, name-tag hide distance 5.08 m (`dist^2 < 25.81656`), `setActive(0)` forced when `0x1826a4903` |

### 2.7 Loading screen and console

| Symbol | acs.exe VA | CSP RVA | Cat | What it changes |
|---|---|---|---|---|
| `SplashScreen::SplashScreen` | 0x14019faf0 | 0x1116200 | B | original through 0x1116230, then CSP's setup (0x1116820) |
| `SplashScreen::loadHints` | 0x1401a0b20 | 0x11163d0 | B | original, then event 0x1826974a0 with the hints string |
| `SplashScreen::setServerHints` / `setOnlinePlayerCar` | 0x1401a17e0 / 0x1401a17a0 | 0x1116460 / 0x1116520 | B | after the original, if the calling thread is the loader thread and loading is active, pumps the Windows message queue (`PeekMessageW` / `TranslateMessage` / `DispatchMessageW`), advances the fade fraction `0x1825e2730` with a 15x ease and throttles with `0x1825e2778/80` (ms) |
| `SplashScreen::renderHUD` | 0x1401a0f90 | 0x1116690 | B | re-entrancy guard `0x1826a975c`; cancelable event 0x1826974a8, post event 0x1826974b0; "Skipping a frame!" logged on re-entry |
| `Console::show / parse` / `singleton` | 0x14018c7f0 / 0x14018bf50 / 0x14018c820 | 0x5a6980 / 0x5a6990 / - | B / B / D | `show` always forces hidden (`show(this, false)`) when gated; `parse` fires cancelable event 0x182690b40 first |
| `PyPlugin::PyPlugin / update / shutdown` | 0x140123f50 / 0x140124670 / 0x140124630 | 0x6942e0 / 0x694570 / 0x694cf0 | A / B / B | see 2.8 |

### 2.8 Python bridge

* `PyPlugin::PyPlugin` (0x6942e0) calls the original and registers the object; `PyPlugin::update` (0x694570, 1,916 B): measures
  every Python app's `acUpdate` with QPC / `GetTickCount64`, publishes it to the Python profiler app and to the log
  ("Slow Python app! %s took %d ms", "%s/update, %d ms", scope "Python apps update/%s", "Open folder" button in the
  report), sets the "current Python app" global `0x1826a6ec0` so `ac.ext_*` functions know the caller, and implements
  **pause hidden apps**: if none of the app's forms is visible, `acUpdate` is skipped N times (counter at +0x2c of the
  per-app record), the skipped time is accumulated in +0x30 and handed to the next call (capped at 1 s). The list of
  apps and N is `data_apps_allowed_to_pause.txt` (21 entries, `name : N`), per-app override `ac.ext_pauseWhenHidden(n)`.
  `PyPlugin::shutdown` (0x694cf0) logs "Closing Python app: %s..." / "Shutting down..." into the loading-screen status.
* Neighbouring rows in other groups: `PyEnvironment::PyEnvironment` 0x691d30 (singleton), `PyModule::PyModule` 0x693580,
  `PythonForm::PythonForm / render` 0x10e53a0 / 0x10e58c0 (profiler scope, `0x1826951e7` cleared during original),
  `PythonInterface::PythonInterface` 0x10e6f30 (log step "Initializing Python apps", 623,568 μs on this PC),
  `writeToLog` 0x10ea230 (counter-limited), `printErr`, `getTexture` / `getTextureByIndex` 0x10eaa00 / 0x10ead10 (hash cache of
  textures by path), plus 13 `ac_*` C-API names (`ac_addOnClickedListener`, `ac_addOnValidateListener`,
  `ac_addOnCheckBoxChanged`, `ac_addOnChatMessageListener`, `ac_addOnListBoxSelectionListener`,
  `ac_addOnListBoxDeselectionListener`, `ac_addOnAppActivatedListener`, `ac_addOnAppDismissedListener`,
  `ac_getCarRealTimeLeaderboardPosition`, `ac_getCarTyreCompound`, ...) and `\python33\PyObject_CallFunction` 0x6f4ad0: callbacks
  are wrapped to produce readable tracebacks (`__s_pretty_traceback`, `extension/internal/python/traceback_helper.py`,
  4,284 B: a copy of `traceback` that survives non-UTF-8 source lines), and the dialog "Python app <name> raised an error during
  loading: ... Would you want to continue?" (0x2288a42..). `clearPythonCache` is a registered plain name. `[dll]`
* `extension/internal/python/initialize.py` (128 B) only sets `ARG_LOGGING_ACTIVE = False` and re-enables bytecode caching when
  `ARG_ENABLE_CACHE`.

## 3. Config keys and Lua API

### 3.1 Files, sections, keys (defaults; comment in my words)

**`extension/config/gui.ini`** (86 keys in 11 sections; the user file on this PC is empty). `[BASIC]` (12): `ENABLED=1` (no-op),
`CONTROLLER_INTEGRATION=1` (D-pad moves the mouse in the pause, start and end menus and in replays; other buttons click),
`CONTROLLER_INTEGRATION_KEYBOARD=1`, `CONTROLLER_INTEGRATION_PITS=1` (D-pad drives the quick pit menu), `CONTROLLER_INTEGRATION_REPLAY=0`,
hidden `CONTROLLER_INTEGRATION_DEBUG`, `VR_MODE`, `HIDE_MAIN_MENU`, `NO_LOWERCASE_FOR_EXTRA_SWITCHES`; `HIDE_UI=0`,
`FORCE_START=0` (start races at once; "Back to pits" overrides), `NO_MOUSE=0`. `[HIDE]` (7): `HIDE_PING_TEXT`, `HIDE_DRIVER_STATUS_MESSAGE`
(F3 / F5 camera message), `HIDE_SYSTEM_MESSAGES`, `HIDE_TASKBAR`, `HIDE_CONSOLE`, `HIDE_CPU_OCCUPANCY_MESSAGE`, `HIDE_RACE_FLAGS`
(all 0). `[HIDE_ICONS]` (9): `ALL`, `CTRL_H=1` (hide with the apps), `LOW_FUEL`, `MANUAL_PIT_LIMITER`, `PIT_LIMITER_WARNING`,
`CONNECTIVITY`, `AI`, `LOW_BATTERY`, `OTHERS` (Lua can add icons). `[TWEAKS]` (12): `TIDY_AC_APPS2` (commented; rename / move AC apps),
`DRAW_UI_LAST=1`, `SORT_LEADERBOARD=1`, `LIMIT_FADE_TIME2`, `USE_JSON_CAR_NAMES2`, `YEAR_POSTFIX=1`, `MOUSE_REAPPEAR_THRESHOLD=40` (0..200 px),
`USE_ESCAPE_TO_UNPAUSE2`, `USE_THROTTLE_TO_START=0`, `INCLUDE_HANDBRAKE_IN_PEDALS_APP=0`, `SUSPENSIONS_APP_PRECISION=1` (0..3 digits),
`CENTER_UI=0` (0 no, 1 main menu, 3 main menu and apps bar), `FINAL_LAP_TWEAK=0` (0 "leader on last lap" message no flag; 1 white
flag; 3 no message, white flag), `SHOW_CHEQUERED_FLAG_EARLY=0`, `SETUP_INSTEAD_OF_RACE_CONTROL=0`, `ASSISTS_IN_MAIN_MENU=1`,
`APPLY_MSAA3` (commented). `[NEW_UI]` (7): `UI_SCALE=1` (1..2), `REPLACE_MAIN_MENU=1`, `REPLACE_TASKBAR=1`,
`APPS_IN_PAUSE_MENU=0`, `SIMPLIFIED_RENDER=0` (0 blur scene, 1 simplest, 2 blur and cast shadows, 3 also blur underlying
items), `USE_CJK_FALLBACK2`, `DRAW_AC_APPS_WITH_NEW_UI=0` (blur background behind AC apps), `REDUCED_ANIMATIONS=0`, `IME_SUPPORT2`.
`[NEW_PAUSE_MENU]` (2): `ENABLED=1`, `SHIFT_CAMERA=0`. `[DRIVER_TAGS]` (9): `ENABLED=1`, `ORIGINAL_FONT=1`, `SHOW_FLAGS=1`,
`SHOW_AI_LEVEL_INSTEAD_OF_FLAGS=1`, `RENDER_IN_MIRRORS=1`, `MAIN_SIZE=1` (0..2), `MIRROR_SIZE=4` (0..10), `FONT_SCALE=1`
(0.5..1.5), `DISTANCE_MULT=1` (0.5..2.5). `[POPUP_PEDAL_WARNING]`: `ENABLED=0`, `DURATION=0` (0..1 s). `[NEW_UI_0_HIDDEN]`
(replacements): `APP_AC_CHAT`, `APP_AC_AUDIO`, `APP_AC_CAMERA_PICTURE_MODE`, `APP_AC_CSPRSTATS`, `APP_AC_POST_PROCESS_FILTER_SELECTOR` (all 1).
`[NEW_UI_1_HIDDEN]`: `APP_APPSHELF=1`, `APP_MUMBLE=1`, `APP_RADAR=0`, `APP_CSPDEBUG=1`, `APP_CSPDEV`. `[NEW_UI_2]` developer apps:
`APP_CAR_DEBUG`, `APP_LIGHT_MAKER`, `APP_LUA_DEBUG`, `APP_OBJECTS_INSPECTOR`, `APP_PYTHON_PROFILER` (1); `APP_EXTRAFX_DEBUG`,
`APP_GRASSFX_DEBUG`, `APP_LIGHTINGFX_DEBUG`, `APP_PARTICLESFX_DEBUG` (0); `APP_REFLECTIONSFX_DEBUG`, `APP_WEATHERFX_DEBUG` (1).
Extra DLL-only names in the gui loader (0xce6b00 / 0xce56c0): `NEW_POPUPS`, `LUA_DEBUG`, `APP_TWEAKS`, `APP_AC_CHAT_EMOJI2`,
`__START_IMMEDIATELY`, `OTHERS`.

**`taskbar.ini`** (51 keys, only used with the old-style taskbar: `REQUIRE=! gui.ini:NEW_UI/REPLACE_TASKBAR`): `[BASIC]`
`ENABLED=1`, `NEW_ICONS_STYLE=1`; `[ARRANGEMENT]` `GROUP_APPS`, `USE_PINNED_GROUP`, `USE_HIDDEN_GROUP`, `HIDE_HIDDEN_GROUP`;
`[SORTING]` `SORT_PINNED_APPS`, `SORT_AC_APPS`, `SORT_PYTHON_APPS`, `SORT_DEV_APPS`, `SORT_HIDDEN_APPS`; `[LAYOUT]`
`SHOW_SCROLL_BUTTONS`, `SHOW_DESKTOP_BUTTONS`, `SHOW_SCROLLBAR`, `SHOW_HIDE_ALL_BUTTON=0`, `TASKBAR_BACKGROUND_OPACITY=0.7`,
`TASKBAR_HEADER_DARKER`; `[LAYOUT_DIMENSIONS]` (16): `WIDTH=240`, `ITEM_HEIGHT=32`, `ITEM_ICON_SIZE=27`, text / icon offsets,
`ITEM_FONT_SIZE=13`, `ITEM_PADDING_LEFT=20`, `SECONDARY_ITEM_PADDING_LEFT=40`, `HEADER_HEIGHT=40`, `HEADER_FONT_SIZE=13`,
`SCROLL_PADDING=8`, `SCROLLBAR_WIDTH=4`; `[MOUSE_POPUP]` `USE_MOUSE_POPUP=1`, `DELAY_HIDE=0.1`, `DELAY_SHOW=0.4`,
`THRESHOLD_HIDE=560`, `THRESHOLD_SHOW=10`; `[MOTION]` `POPUP_SPEED=2000`, `SCROLL_LAG=0.8`, `SCROLL_WHEEL_SPEED=2`,
`SCROLL_BUTTONS_STEP=600`, `COLLAPSE_LAG=0.6`. Parser 0x12c8860 (6,957 B), toggle `__EXT_TOGGLE_TASKBAR` 0x12c8630.

**`splashscreen.ini`** (50 keys, `LIVE_SUPPORT=0`): `[BASIC]` `ENABLED=1`, `SMOOTH_FADE_IN=0`; `[CLEANING]` (hidden) `CLEAN_UP_STATUS`,
`CLEAN_UP_SERVER_HINTS`, `CLEAN_UP_CAR_HINTS`, `CLEAN_UP_SERVER_NAME`; `[SCRIPT]` `ENABLED=0`, `IMPLEMENTATION=default`
(looks for `splashscreen.lua` in `extension/lua/loading-screen/<name>`; the shipped "default" is a 5,202 B demo);
`[LAYOUT]` `STATUS_AT_THE_TOP`, `STATUS_HORIZONTAL_OFFSET`, `TRACK_MAP_SCALE=1.6`, `BRAND_BADGE_SCALE=0.8`, `FIT_BACKGROUND=1`,
`BLUR_BACKGROUND=1`, `USE_BLUR=0`, `TRIPLE_DETAILS_IN_CENTER=1`; `[COMPONENTS]` `DISPLAY_VERSION`, `DISPLAY_ANIMATION`,
`DISPLAY_STATUS`, `DISPLAY_CAR_INFO`, `DISPLAY_TRACK_INFO`, `DISPLAY_SERVER_INFO`, `DISPLAY_WARNINGS` (all 1),
`DISPLAY_PRO_TIPS=0`, `DISPLAY_CAR_PREVIEW=0` (0 off, 1 offline, 2 always); `[LOOK_FOR_IMAGES]` (14): Kunos `bcrN.jpg` in track
`ui`, `extension/backgrounds`, `STRICT_SEARCH`, `FAVOURITE_SHOTS_LOCATION`, `USE_FAVOURITE_TRACK_SHOTS`, `..._CAR_SHOTS`,
`..._RANDOM_SHOTS`, `REGULAR_SHOTS_IGNORE_MASK=__custom_showroom_*`, `USE_REGULAR_*`, `FALLBACK_IMAGE`, `FALLBACK_GRAPHICS`,
`FALLBACK_CHANCE=0.2`; `[SLIDESHOW]` `ENABLED=0`, `INTERVAL=15`; `[TASKBAR]` `SHOW_PROGRESS=1` (Windows taskbar progress).
`extension/backgrounds` holds 9 JPEGs (Imola, Nordschleife x4, Red Bull Ring, Magione, Monaco x2, Spa on this PC). The Lua SDK
module `ac_splashscreen` exposes a read-only `loading` table (`status`, `progress`, `carID`, `trackID`, `serverHints`, `proceed`,
`hold`).

**`chat_shortcuts.ini`** (29 keys; `ENABLED=0`): `[MESSAGES] MSG_1..MSG_0` (Hi, Bye, Sorry, My fault, Thanks, No problem, GG, Coming
through, Gotta go to pits, Be careful please), `[EXTRA_MESSAGES] MSG_EXTRA_1..0` (empty); hold the `__EXT_CHAT_SHORTCUTS_BASE`
key (Oemtilde) then a number, `Alt+` for the second set.
**`car_instruments.ini`** (31 keys): `[BASIC]` `USE_CORRECT_SPEEDOMETERS=1`, `USE_CORRECT_TACHOMETERS=1`, `USE_NAVIGATORS=1`,
`USE_BREAKABLE_LIGHTS=1`, `LED_PANEL_MODE=BLANCPAIN` (or `N24`, `NONE`); `[INPUT]` 7 turn-signal / high-beam keys;
`[NAVIGATORS]` 7 hidden map-render keys; `[FUEL_LIGHT_HUD]` 6 keys (position in Lua expressions: `screen.triple and
screen.width / 3 + 50 or 50`). `spectating.ini` and `lua_tools.ini` have one `ENABLED` key each (0 and 1).
**Data files**: `data_controls_props.ini` (84 labels: `[BASIC]` 40 car controls, `[SYSTEM]` 44 incl. `__CM_*` Content Manager
commands), `data_hotkeys_supported.txt` (41 control names a Lua app may query / simulate), `data_apps_allowed_to_pause.txt`
(21), `data_apps_virtual_tyres_fix.txt` (1: `camber-extravaganza`), `data_imgui_symbols.txt`, `data_setup_names.txt` (91 adjectives
for "Randomize name"), `data_emoji.txt` (4 rows of 11 emoji for the chat). `data_manifest.ini [FEATURES]` lists 125 `__EXT_*`
extra bindings (UI ones: `__EXT_HIDE_UI` Ctrl+Shift+Alt+H, `__EXT_TOGGLE_TASKBAR` Alt+H, `__EXT_CHAT_SHORTCUTS_BASE/ALT`,
`__EXT_QMP_SHOW`, `__EXT_SAVE_CLIP`, `__EXT_VIRTUAL_MIRROR`, `__EXT_SIM_PAUSE`, `__EXT_TOGGLE_FULLSCREEN`).

### 3.2 Lua API relevant to this area (counts from the SDK `lib.lua` files `[sdk]`)

* **`ui.*` : 397 names**, present in 18 of the 19 modules (`ac_apps` 396, `ac_tools` 395, `ac_car_scriptable_display` 392,
  `ac_scripted_texture` 389, `ac_splashscreen` 389): the immediate-mode toolkit is common to apps, tools, displays and
  loading screens. My grouping by name (approximate, regex over names): windows and window state 37 (`transparentWindow`,
  `toolWindow`, `beginChild`, `setNextWindow*`, `bringWindowToFront`, `windowPos`, `windowPinned`, ...); popups / modals /
  tooltips / menus 23 (`popup`, `modalDialog`, `modalPrompt`, `beginMenu`, `menuItem`, `toast`, `setTooltip`); draw-list
  primitives and paths 37 (`drawRect*`, `drawLine`, `drawCircle*`, `drawPie`, `drawBezierCurve`, `path*`, `glow*`); draw text /
  images / icons / media 46 (`drawText`, `drawImage*`, `drawIcon`, `drawCarIcon`, `drawDriverIcon`, `drawRaceFlag`,
  `drawVirtualMirror`, `ExtraCanvas`, `GIFPlayer`, `MediaPlayer`, `SharedTexture`, `UserIconsLayer`, `decodeImage`,
  `weatherIcon`); text / fonts / DirectWrite / markup 41 (`text*`, `dwrite*`, `pushDWriteFont`, `measureText`, `html`, `markdown`,
  `inputText`, `copyable`); widgets 37 (`button`, `slider`, `checkbox`, `combo`, `colorPicker`, `treeNode`, `tabBar`,
  `selectable`, `progressBar`, `modernButton`, `shortcut`, `hotkey*`); layout / cursor / columns / scroll / ID stack / style
  80; item state queries 22; post effects and transform stacks 29 (`beginBlurring`, `beginTonemapping`, `beginSharpening`,
  `beginSubtraction`, `beginOutline`, `beginGradientShade`, `beginRotation`, `beginScale`, `beginTransformMatrix`,
  `beginMIPBias`, `forceSimplifiedComposition`); mouse / keyboard 28 (`mouse*`, `keyPressed`, `keyboardButton*`,
  `captureKeyboard`, `captureMouse`, `setMouseCursor`); AC hooks 9 (`onChat`, `mentionDriverInChat`, `onDriverContextMenu`,
  `onDriverNameTag`, `onDriverTooltip`, `onExclusiveHUD`, `onUIFinale`, `setDriverPopup / Tooltip`, `registerOnlineExtra`,
  `setExtraContentMark`); storage and time 7 (`storeBool`, `storeNumber`, `loadStoredBool`, `loadStoredNumber`, `time`, `frameCount`,
  `deltaTime`); enums `ui.Alignment`, `ButtonFlags`, `Icons`, `MouseButton`, `StyleColor`, `StyleVar`, `Font`, `CornerFlags`.
* **`ac_apps` own 1,282 functions** beyond the shared 795; relative to `ac_background` the app module adds 1,193 (ui 396, `ac` 270,
  scene `_ac_SceneReference` 110, `physics` 96, `render` 38, `_ac_GeometryShot` 35, `_ui_MediaPlayer` 33, ...). The app / UI part of
  `ac.*`: windows (`ac.accessAppWindow`, `getAppWindows`, `setAppWindowVisible`, `setWindowOpen`, `isWindowOpen`, `setWindowTitle`,
  `setWindowIcon`, `setWindowBackground`, `setWindowSizeConstraints`, `setWindowNotificationCounter`, `windowFading`,
  `AppWindowAccessor` 15 methods: move / resize / position / visible / pinned / redirectLayer ...), apps (`ac.noticeNewApp`,
  `overrideAppLazyFlag`, `restartApp`, `unloadApp`, `uninstallApp`, `reloadPythonApp`, `isPythonAppActive`, `setAppsHidden`,
  `setCurrentDesktop`, `setAppOpen`), AC UI redirects (`redirectDamageDisplayer`, `redirectFuelWarningIndicator`,
  `redirectVirtualMirror`, `disableQuickMenuPitstop`, `disableExtraHUDElements`, `hideCarLabels`, `hideMouseCursor`,
  `accessOverlayLeaderboardParams`, `blockEscapeButton`, `forceFadingIn`), menus (`tryToOpenRaceMenu`, `tryToPause`,
  `tryToStart`, `tryToRestartSession`, `tryToSkipSession`, `tryToTeleportToPits`, `tryToToggleReplay`, `canCastVote`,
  `castVote`, `getCurrentVoteDetails`, `timeToNextVote`), chat / console (`sendChatMessage`, `onChatMessage`,
  `onOutgoingChatMessage`, `setDriverChatNameColor`, `onConsoleInput`, `consoleExecute`), setups (`loadSetup`, `saveCurrentSetup`,
  `setActiveSetupName`, `refreshSetups`, `resetSetupToDefault`, `setSetupSpinnerValue`, `setPitstopSpinnerValue`,
  `setCurrentQuickPitPreset`), input (`ControlButton` 12 methods, `simulateCustomHotkeyPress`, `trySimKeyPressCommand`,
  `sendKeyboardButtonEvent`, `setMousePosition`, `setMouseLeftButtonDown`, `setMouseWheel`, `accessMouseDelta`).
  `display.*` (4: `text`, `rect`, `image`, `horizontalBar`) is the minimal drawing set of the simplest display scripts.
* **`ac_tools`** adds 8 `script.*` callbacks (`asyncUpdate`, `draw3D`, `frameBegin`, `onHide`, `onReplayStart`,
  `onReplayStop`, `onShow`, `simUpdate`) over `ac_apps`. **`ac_scripted_texture`** (1,558 functions, 389 `ui.*`) is the module of a
  script that draws into a texture; it has no path rule in `rules.json` (loaded through other kinds `[interp]`).
* **Shared Lua libraries about UI** (text in `lua-shared/`, also precompiled in the DLL's archive 4): `shared/ui/base` (1 function
  `QuickMenuPitstopInput`), `shared/ui/components` (`MainMenuModePiece` and `AssistsEditor`: `present / title / height /
  hasSettings / content / settings` so a custom main menu can draw the chat or the assists editor), `shared/ui/replay`,
  `shared/ui/virtualizing` (9,802 B list virtualiser), `shared/sim/chat` (`extras.canChangeCarColor`, `changeCarColor`, teleport
  destinations, ...), `shared/sim/cspdebug`, `shared/utils/appshelf` (`offer({id=...}, cb)`), `shared/web/ui`.
* **Python**: 213 `ac.ext_*` names (`functions.txt`, 5,763 B); my split by name: app / UI / runtime 18 (`patchVersion`,
  `patchVersionCode`, `setStrictMode`, `pauseWhenHidden`, `isAnyAppVisible`, `isAppVisible`, `perfBegin / perfEnd` (profiler
  markers), `setMessage`, `loadIniFileAsJson`, `parseIniAsJson`, `setEmojiActive`, `dirname`, `storeLua / loadLua`,
  `pauseFsWatching`, `setClipboardData`); camera 32; replay 4; mirrors 22 (`ext_mirror*`); GL / render targets / pixel shaders 18
  (`glSetCullMode`, `glSetBlendMode`, `createRenderTarget`, `glLoadPixelShader`, `glFontCreate`, ...); input 9 (buttons, modifiers,
  joystick, G27 thresholds); weather / track / VAO / rain / lights 35; car physics read-outs 75 (`getTyre*`, `getTire*`,
  `getBrake*`, `getSuspensionTravel`, `getDamper*`, `getAero*`, `getCurrentTorque`, `isExtendedPhysics`, `getStateAbs`, ...).

## 4. Files it reads

* `extension/config/{gui,taskbar,splashscreen,chat_shortcuts,car_instruments,spectating,lua_tools}.ini` + the same names under
  `Documents\Assetto Corsa\cfg\extension\`; `data_{controls_props,hotkeys_supported,apps_allowed_to_pause,apps_virtual_tyres_fix,
  imgui_symbols,setup_names,emoji,credits}` and `data_manifest.ini` in `extension/config`.
* Apps: `<AC>\apps\lua\*\manifest.ini` (`LUA_APPS_DIR`), `<AC>\extension\internal\lua-apps\*`, `<AC>\apps\python\*`; tools
  `extension\lua\tools\*`; loading screens `extension\lua\loading-screen\*`; backgrounds `extension\backgrounds\*.jpg`.
  Owner's installed apps on this PC: Lua 7 folders (RSS_FH26_Race_Lab, RSS_Settings, Radar, ext_controls,
  vrc_advanced_setup, vrc_track_zone_editor with manifests; SimHubUDPConnector without a manifest: "windows: 0"), 230 `.lua`
  files, 3,447,925 B; Python 3 (`Chat`, `gMeter`, `system`).
* Fonts: `%WinDir%\Fonts\{segoeui,segoeuil_0,verdana,verdanai,consola}.ttf`, `content\fonts\*.ttf`, `extension\fonts\`; cache
  `<AC>\cache\imgui.bin`, `<AC>\cache\web`, `<AC>\cache\track_logos`, `cache\lua_shaders`.
* State written: `cfg\extension\state\imgui_settings.ini` (1,136 B here), `state\imgui.bin` (72 B here; real cache is under
  `<AC>\cache`), `state\lua\app\*.ini`, `state\consumption.ini` (fuel-estimation history shown in the setup fuel tab),
  `state\odometers.ini`, `window_position.ini` (AC window `[AC_WINDOW_POSITION] REAL_X/Y, X, Y`), `acos*.ini` (form layout, new
  format), `view.ini`, `triple_screen.ini`, `setups\*` and `chat_driver_tags.ini`, `online_passwords.ini` (chat admin
  passwords, section `AC_CHAT`), CM's `Progress\Friends.data` (driver tags).
* Network: `https://ss.acstuff.club/` (setup sharing "Shared ID"), `https://acstuff.club/u/blob/ac-fonts.zip`, `known.dat`
  (shared with Lua), the App Shelf catalogue (service script period 12 h).

## 5. How it is switched on

* Master: `general.ini [BASIC] ENABLED=1`; UI module: `gui.ini` (its own `ENABLED` is a no-op; `ENABLED2` commented, unsupported).
  Everything in 2.1 / 2.3 is unconditional once CSP loaded; the new UI pieces are per key: `REPLACE_MAIN_MENU`, `REPLACE_TASKBAR`,
  `NEW_PAUSE_MENU/ENABLED`, `DRIVER_TAGS/ENABLED`, each `APP_*`, `splashscreen.ini ENABLED`. On this PC all defaults are in force
  (no user `gui.ini`), `chat_shortcuts.ini ENABLED=0`, `taskbar.ini ENABLED=1` (old style unused because `REPLACE_TASKBAR=1`).
* Per app: `LAZY` (manifest), `FLAGS DISABLED_*` / `HIDDEN_*` (offline, online, render mode single / triple / VR / custom),
  `REQUIRED_VERSION` (CSP build: RSS apps ask 3334 and 3465, this build is 4184), Lua `DISABLED_IF`, the `[LUA_APPS] ACTIVE` list
  and per-window open state in `imgui_settings.ini`. Server side: `ALLOW_APPS` and `ALLOW_TOOLS` in the server's extended config
  (strings 0x2248408 / 0x22483b8) can forbid apps / tools online.
* Developer apps show only in the "Developer apps" taskbar group; Lua "SETTINGS" group windows show in "Settings".
* Headless tool runs (`AC_TOOL_RUN`) disable 27 AC apps and several others (2.0).

## 6. Size

| Item | Number |
|---|---|
| Hook rows | 207 (177 H, 30 L), 89 classes, 151 distinct replacement RVAs, 339,634 B of pseudo-C |
| Event listener vectors found | 25 (+ 1 stack at 0x1826a4990) |
| `gui.ini` / `taskbar.ini` / `splashscreen.ini` / `chat_shortcuts.ini` / `car_instruments.ini` | 86 / 51 / 50 / 29 / 31 keys |
| Native ImGui apps / tools | 18 / 11 (+ `lua_tool`) |
| DLL UI code | main menu 0x97b8c0 (9,689 B), pause menu 0xa13a90 (10,762 B), chat 0x96c970 (7,716 B) + 0x964f20 + 0x963b30, driver context menu 0xa1c2f0 (5,437 B) + 0xa1eab0 + 0xa21310, time table 0x9986e1..0x9a9ec0 (7 functions, 3 to 6 KB each), setup pages 0x98a3b0, 0x990c70, 0x9928d0, 0x994120, 0x9b2600 / 0x9b4ca0 (tyre pages), taskbar 0xa2bd10..0xa36e10 (7 functions, 25 KB) |
| Lua | 3 internal apps (9 files, 157,002 B), 7 installed apps (230 files, 3.45 MB; 75 files / 1.7 MB of RSS Race Lab are obfuscated), `ui.*` 397, `ac_apps` 2,077 functions / 95 classes / 109 aliases, precompiled `ac_apps` 195,121 B |
| Python | 213 added functions, 13 wrapped `ac_*` entry points, 3 hooks on `PyPlugin` |
| Fonts | 3 TTF 26.65 MB uncompressed (12.64 MB deflated), `imgui.bin` 5.16 MB |
| UI shaders | about 28 names (`uiImgui*`, `accImgui*`, `accDriverTag*`, `acUITaskbarPreview`) |
| Startup cost seen in the log | "Loading cached IMGUI fonts" 228.5 ms; "Initializing Python apps" 623.6 ms; RSS Race Lab sync part 2.1 s |

## 7. Port difficulty for rustyAC

**Overall XL**, but it splits into independent parts. What rustyAC already has: `crates/rustyac-gui` (20,987 lines) is a port of
AC's retained `ksgui` library (control, form, spinner, taskbar, widgets, 15 app files, ESC menu, pause menu, setup screen and 8 tabs,
leaderboards, end session, replay interface, online HUD), proven against acs.exe by oracle comparisons (`docs/port/gui_hud.md`,
`menus.md`, `apps_user.md`, `setup.md`); a `Painter` trait with a Direct3D implementation and a browser one; a TTF outline
renderer; FW1/DirectWrite text. It has **no** immediate-mode layer, no Lua host, no Python host, no desktops beyond AC's,
no `acos.ini` format v2.

| Part | Rating | Reasoning, reuse, proof |
|---|---|---|
| Small behaviour hooks of 2.1 / 2.3 (form clamping, hit-test margin, `HIDE_*`, system message hiding, pause-menu labels, leaderboard sorting, final-lap tweak, `ext_default` setup, toe from geometry, ABS levels) | S each, M together (about 40 items) | direct edits of existing gui code; prove with the existing render / state oracles against CSP-patched acs.exe; deterministic state and file output |
| Event bus (25 vectors) | S as a mechanism, M for the Lua-visible names | a typed listener list in `rustyac-gui`'s `SimLink` / `AppLink`; required before Lua |
| New taskbar, pause menu, main menu (ImGui versions) | L | optional visual replacements; AC's versions are already ported and proven, so ship those first. Needs the immediate-mode layer and the blur / glow shaders |
| Immediate-mode core (`ui.*` 397) | L to XL | draw list + layout + ID stack + input + fonts; can be built on `egui` or a hand-written ImGui-like core in Rust; text with DirectWrite semantics already exists in `rustyac-render` fw1.rs; image/ GIF / media, `ExtraCanvas`, blur effects need new passes. Proof: screenshot compare of the 4 installed apps and CspDebug / AppShelf (plain text, readable) against CSP |
| Lua app host (manifest, windows, LAZY, services, triggers) | L; XL with the `ac.*` surface | see `lua.md` S7; 396 `ui.*` and 270 `ac.*` app functions; the used subset is 132 `ui.*` names in `apps/lua` (RSS Race Lab not countable: obfuscated) and 110 in internal apps |
| Native apps (18) | M each; Car Debug / Objects Inspector / Render Stats L | developer apps can be dropped; Audio, Photo mode, Chat, Radar first |
| Python bridge | M if Python 3.3 is hosted | rustyAC has no Python; the 213 `ac.ext_*` are thin getters over state the physics already exposes (`docs/atlas/python_api.md` section 6) |
| Loading screen | M | options and image search are plain file logic; the message pump trick is Windows-only and not needed with a native event loop |
| Persistence formats (`imgui_settings.ini`, `acos*.ini` v2, hashed `ac.storage` keys) | S | text formats, byte-exact files are comparable |

What is hard: matching ImGui's exact pixels (CSP's fork, its style, custom fonts, blur passes) is not realistic bit-for-bit;
aim for state / layout equality and visual closeness. What can be proven exactly: hook behaviours that write files or change
state (`acos.ini` v2, `view.ini`, `triple_screen.ini`, setup files with `__EXT_PATCH`, `state\*.ini` layouts), events, and the
counts of drawn items.

## 8. Open questions and not mapped

1. What `0x612480` (called at the end of CSP's `doRender2D`) is: I treat it as the ImGui frame; not decompiled.
2. Byte-to-key map of the 0x182695xxx / 0x18268xxxx flag block (the two `gui.ini` loaders 0xce56c0 and 0xce6b00 were not
   decompiled), so the gate predicates of 2.0 and the `0x1826a2877` / `0x1826a4e44` meaning are inferences.
3. Which listener each event vector in 2.0 has (Lua `ac.on*`, ImGui apps): not traced; names are guesses from the API.
4. Where the new main-menu options are stored (`main_menu.*`).
5. Bodies of `ACSuspension::renderWireframe`, `GUI::render3D`, `AnalogInstruments` init helpers (0x58aaa0, 0x58b600),
   `DigitalInstruments` replacement constructor (0x5a91c0 / 0x5ab450), the 24 not-decompiled UI functions of 1.5.
6. `0xce28c0..0xce3880` returns names for 17 apps; their order and the mapping to `[WINDOW]` ids was not checked.
7. RSS Race Lab (75 files, header "RSSLP3") is obfuscated by its maker: not read, not decoded (rule 2). `patch.lua` /
   `user.lua` in `extension/internal` are also obfuscated (hashes / byte-string loaders): not read.
8. How CM passes settings to a running game (live edits): `WATCH_FOR_CHANGES` and `LIVE_SUPPORT` only seen as names.
9. The `sh_web_ui` / `sh_web_browser` embedded Lua (WebBrowser flag, `ui.html`, `ui.markdown`) were not examined (web brief).
10. Whether `INCLUDE_APPS` in the tool-run config re-enables apps in headless mode.

## Evidence

* Tables: `csp_hook_table.tsv`, `hook_groups.tsv` (group `ui_apps`, 207 rows), `dll_strings.tsv`, `dll_func_strings.tsv`, `dll_funcs.tsv`,
  `dll_zips_all.json` (fonts, Lua libs), `lua_sdk_functions_union.txt`, `lua_sdk_counts.tsv`.
* Decompiles read: `decomp_hooks/` 0x6107e0, 0x610810, 0x610e90, 0x611580, 0x6117e0, 0x6120c0, 0x612720, 0x6127a0, 0x6129b0, 0x612a00,
  0x614210, 0x614270, 0x6142c0, 0x614310, 0x60a990, 0x60adb0, 0x623760, 0x60da50, 0x60db50, 0x60dae0, 0x619510, 0x619950, 0x61a0b0,
  0x61a3f0, 0x61a420, 0x61a7e0, 0x681370, 0x6813b0, 0x681500, 0x5cdcf0, 0x5ce310, 0x6997e0, 0x5c9f40, 0x518760, 0x429090, 0x10f6410,
  0x10f6300, 0x10f61c0, 0x10fc2d0, 0x69ea00, 0x69d940, 0x69f5a0, 0x694570, 0x694cf0, 0x58ace0, 0x58aea0, 0x58ac40, 0x5a9190, 0x5baa30,
  0x5ba920, 0x6dab70, 0x6daf80, 0x1116460, 0x1116520, 0x1116690, 0x11163d0, 0x696f60, 0x697340, 0x69a0e0, 0x69aa50, 0x6bc460,
  0x691c90, 0x682b80, 0x42c420, 0x5dbbf0, 0x60d800, 0x10dfc20, 0x10dfe20, 0x5a6990, `decomp_req1/1387350.c` (HEADLESS_MODE).
* Gate decoding by hand from `dwrite_copy.dll` bytes: registration sites 0x263dce..0x2657f7, predicates 0x6dab40, 0xef1700.
* Files: `extension/config/{gui,taskbar,splashscreen,chat_shortcuts,car_instruments,spectating,lua_tools,data_*}.ini|txt`,
  `extension/internal/{lua-apps,lua-shared,lua-sdk,python}`, `apps/lua/*/manifest.ini`, `Documents\Assetto Corsa\cfg\extension\state\*`,
  `logs\custom_shaders_patch.log` lines 24 to 1160 `[log-csp]`.
* Other briefs / docs: `attach.md` 1.7, `lua.md` 3.2 to 3.7, `docs/atlas/ui.md`, `docs/atlas/python_api.md`, `docs/port/apps_user.md`.

## Decompile requests

1. 0x612480 - the function `doRender2D` calls last: is it the ImGui NewFrame / Render, and what does it read?
2. 0x106acc0 (606 B) - `cui::imgui_offscreen` class: members, render target formats.
3. 0xa451e0 (1,710 B) - `imgui_impl_ac_ingame` draw: how draw lists are converted to AC / D3D11 calls, blend states.
4. 0xcf0800 (11,269 B) - Lua app manifest parser: field order, defaults, flag bit values.
5. 0xceb6a0 (7,667 B) - window creation: flag bit meanings and the `GROUP` table.
6. 0xce56c0 (5,039 B) and 0xce6b00 (6,268 B) - gui.ini loaders: key to global-byte map (gates, 0x1826a2877 etc.).
7. 0xef1700 / 0xef1770 - start-up options object: which command-line / tool-run field is byte 0 and byte 3.
8. 0x97b8c0 (9,689 B) - new main menu: tab logic and what `REPLACE_MAIN_MENU` skips.
9. 0xa13a90 (10,762 B) - new pause menu.
10. 0x11be370, 0x11be400, 0x11be450 - the functions the ESCMenu / FormVolumes hooks call instead of the original.
11. 0xa30580, 0xa32b70, 0xa34fd0, 0xa36e10 - new taskbar logic (pin, hide, search, glow).
12. 0x4a85a0 (5,477 B) and 0x14ee730 (910 B) - font atlas build: ranges, sizes, which TTF for which script.
13. 0x4a7f00 / 0x4a78e0 - `imgui.bin` cache layout (header, version, what is cached).
14. 0x7df7d0 (2,532 B), 0x7ddf80 (3,027 B), 0xa177b0 - imgui_settings.ini sections writer and reader, id hashing.
15. 0x574150, 0x574460 - chat handlers called by `FormChat::onChatMessage` / `ACChat::onChatMessage`.
16. 0xf47420 - helper called from `Control::render`, `ACTimeFlow::render`, `SessionLeaderboard::renderHUD`.
17. 0x1810fc450, 0x1810fc870 - setup tab value update / hash used in `resetTabsValueToDefault`.
18. 0x5a91c0 and 0x5ab450 - DigitalInstruments constructor replacement.
19. 0x58aaa0 and 0x58b600 - AnalogInstruments extension (needles, correct speedometer).
20. 0x611bb0 body (899 B) and 0x580bf0 (898 B) - `GUI::render3D`, `ACSuspension::renderWireframe`.
21. 0x1810e65f0, 0x1807ac930, 0x180b12300 - profiler scope construction (names and registry).
22. 0x613490 - the per-form layout cache used by deserialize / serialize.
23. 0x619310, 0x619460 - old taskbar app list builder and the "new taskbar active" test.
24. 0x986e70 (3,185 B) - new main menu Settings tab: where `main_menu.*` values are stored.
25. 0x5a6970, 0x681360, 0x697df0, 0x6f4e90 - small gate predicates (confirm the globals).
