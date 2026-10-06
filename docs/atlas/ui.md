# UI: widget library, screens, HUD, in-car instruments, built-in apps, console

Source: `re/decomp/` pseudo-C (folders of every object listed in section 5, plus `sim/Sim__Sim`, `Game/Game__onIdle`, `Font`, `LocalizationManager`), `re/types/*.txt`, `tools/re_query.py` (find / callers / strings / vtable / size), `tools/disasm.py` (one check: the `Form::devApp` stores), `tools/ini_survey.py setup.ini`, and the plain-text config files under `system/cfg` (read only). Nothing in the game folder, the Ghidra project or git was modified. "Confirmed" means read in the pseudo-C or disassembly; "interpretation" is marked.

## 1. What it is

Everything the player sees on top of the 3D scene inside `acs.exe` is drawn by a small retained-mode widget library written by Kunos, `ksGUI.lib` (namespace `ksgui`). The launcher (the menu where car, track and session are chosen) is a separate program and is only mentioned here: `acs.exe` starts straight into a session described by `cfg/race.ini`. A widget is a `ksgui::Control`: a rectangle with colours, an optional texture, a text, a font, a list of child controls and a set of `Event<...>` members that hold `std::function` handlers. `ksgui::GUI` owns the top-level controls, turns window mouse / keyboard events into virtual calls on the control under the cursor, and draws the tree once per frame with immediate-mode quads (`GLRenderer`) and DirectWrite text (`Font` on top of FW1FontWrapper). A `ksgui::Form` is a movable, pinnable, scalable window with a title bar; every "app" on the right-hand app bar is a `Form`, and the app bar itself is `ksgui::Taskbar` with several virtual desktops. Form positions are saved in `Documents\Assetto Corsa\cfg\acos.ini`.

On top of that library sit: the full-screen menus (`ESCMenu` with its Drive / Setup / Time / Telemetry / Race control pages, `PauseMenu`, the end-of-session panel), the car setup editor (`SetupScreen` and its tabs, driven by the car's `data/setup.ini`, saving to `Documents\Assetto Corsa\setups`), 56 built-in apps (`SystemApps.lib`), a set of HUD objects that are plain `GameObject`s drawing in `renderHUD` (messages, flags, name tags, damage, proximity arrows, fuel light), the in-car dashboard instruments (3D nodes and text driven by `digital_instruments.ini` / `analog_instruments.ini`), a developer console, and a string-table localisation layer. None of this feeds back into physics except through the same public setters a key press would use (setup values, electronics, pit stop requests).

## 2. Main classes

| Class | Source file / library | Size | Role |
|---|---|---|---|
| `ksgui::GUI` | GUI.obj (ksGUI.lib) | 0x190 | Root: top-level `controls`, `taskbar`, default `font`, skin reader, input routing, 2D and VR ("3D") rendering, form layout save / load. |
| `ksgui::Control` | Control.obj | 0x178 | Base widget. Vtable: +0x08 `hitTest`, +0x10 `addControl`, +0x18 `render(dt)`, +0x38 `setSize`, +0x40 `onVisibleChanged`, +0x50..+0x68 mouse down / up / move / wheel, +0x70 `onKeyChar`, +0x78 `onKeyDown`, +0x88 `setText`, +0x98 `scaleByMult`. |
| `ksgui::Form` | Form.obj | 0x1b8 | Window: title, icon, pin / zoom buttons, `devApp`, `autohide`, `blocked`, scale. Adds +0xa8 `onTitleClicked`, +0xb0 `shutdown`. |
| `ksgui::GameScreen` | GameScreen.obj | 0x1b0 | Full-screen container (`SIM_GAME_SCREEN`) that holds all forms; +0xa8 `addControl(control, showInTaskbar)`. |
| `ksgui::Taskbar`, `TaskBarIcon` | Taskbar.obj, TaskBarIcon.obj | 0x268 / 0x218 | App bar with icons, scrolling, virtual desktops. |
| `Label`, `TextBox`, `TextInput`, `ActiveButton`, `CheckBox`, `Spinner`, `CustomSpinner`, `Slider`, `ScrollBar`, `ListBox`, `ListBoxRow`, `Graph`, `ProgressBar`, `PopOver`, `ConnectedLabel`, `TabBar` | ksGUI.lib | 0x180-0x270 | The widget set. `Graph` plots value series; `PopOver` is the help bubble; `TabBar` (global namespace) is the setup tab strip. |
| `DefaultSystemMessage` | DefaultSystemMessage.obj (ksGUI.lib) | 0x1b0 | The centre-screen message bar used by `SystemMessage`. |
| `Font` | Font.obj (KunosSim.lib, counted in renderer.md) | 0x20 | Family name + size + bold / italic; draws through `kglBlitStringWide`. |
| `SimScreen` | SimScreen.obj | 0x60 | `GameObject` whose `render` calls `Sim::renderScene` (the 3D scene pass). |
| `ESCMenu` | ESCMenu.obj | 0x260 | In-pit menu with buttons drive / setup / time / telemetry / racecontrol / exit. |
| `PauseMenu` | PauseMenu.obj | 0x200 | Resume, Back to pits, Skip session, Restart session, Replay, Exit. |
| `SetupScreen` + `SetupTab`, `SetupIOManager`, `SetupInformation`, `SetupElectronics`, `SetupFuelManager`, `SetupGearManager`, `SetupTyresManager`, `SetupPitstopPresets` | one .obj each | 0x1c0-0x270 | Car setup editor: generic spinner tabs built from `setup.ini`, plus special tabs (tyres, fuel, gears, electronics, pit strategy, save / load). |
| `QuickMenuPitstop` | QuickMenuPitstop.obj | 0x190 | The on-track pit stop menu (fuel, compound, pressures, wings, repairs). |
| `RaceControlScreen` + `RaceControlSessionInfo`, `RaceControlSessionServer`, `RaceControlSPSession`, `RaceControlVoting` | one .obj each | 0x1a0-0x2b8 | "Race control" page: session info, weather, server rules, single-player skip / restart, online voting. |
| `EscMenuTimeTable` | EscMenuTimeTable.obj | 0x360 | "Time" page: lap table per session and driver. |
| `EndSessionDisplayer`, `EndSessionCommonGui` | one .obj each | 0x130 / 0x1c0 | End-of-session results with Exit / Restart / Replay. |
| `SessionLeaderboard`, `OverlayLeaderboard` | one .obj each | 0x60 / 0xf0 | Leaderboard shown on a key, and the optional always-on top / vertical overlay. |
| `SystemMessage` (+ `DriverStatusSystemMessage`, `SessionStatusSystemMessage`), `SystemNotification` | one .obj each | 0x98 / 0x228 / 0x58 / 0xa8 | Message queue (ABS / TC / turbo / brake bias changes, joins, kicks, votes) and icon notifications (downshift protection, wheel not centred). |
| `DriverNameDisplayer`, `CarLabel` | one .obj each | 0x138 / 0x230 | Name tags over cars (world point projected to the screen). |
| `ProximityIndicator`, `WrongWayIndicator`, `DamageDisplayer`, `FlagManager`, `FuelLight` | one .obj each | 0x60-0x108 | HUD elements; each is a `GameObject` with `renderHUD` and / or `update`. |
| `DigitalInstruments`, `DigitalItem`, `DigitalLed`, `DigitalPanels`, `DisplayNode`, `AnalogInstruments` | one .obj each | 0xc8 / 0xd0 / 0x78 / 0xd8 / 0x168 / 0x490 | Dashboard: text items, LED bars, texture panels and needles attached to nodes of the car model. |
| `Console`, `ConsoleSet`, `ConsoleCommandLambda`, `ConsoleRefreshInstruments`, `ConsoleCrash` | Console*.obj | 0x128 / 0x30 / 0x70 / 0x30 | Developer console: command table, variable table, text input, scroll-back. |
| `Desktop`, `QuickMenu`, `SetupGenericCarSettings`, `TimeDifferenceDisplayer`, `PitstopOverlay`, `ScenePauseDimmer` | one .obj each | 0xc0 / 0x88 / 0x1d0 / 0x78 / 0x60 / 0xd0 | Dead in this build: the types exist in the PDB but each object file contains only a static initialiser (7 functions, 469 bytes), no methods and no constructor call. |

## 3. Entry points

Creation (all on the main thread; `Sim::Sim` 0x140192070 is the one long constructor described in main_loop.md):
- `Game::Game` 0x140241880 creates `ksgui::GUI::GUI` 0x140251ac0 and calls `GUI::attachToRenderWindowEvents` 0x140252eb0 (subscribes the GUI to the window's mouse events; the GUI is also an `IKeyEventListener`).
- `Sim::Sim` creates, in this order: `Console::Console` 0x1401888c0 (which runs `Console::initCommands` 0x14018aa80), the `GameScreen` (`ksgui::GameScreen::GameScreen` 0x140250bd0, added with `GUI::addControl` 0x140252e50), `SystemMessage::SystemMessage` 0x1401b5610, `SystemNotification::SystemNotification` 0x1401b68a0, `PauseMenu::PauseMenu` 0x14011d9b0, then the apps (each `new XxxApp(...)` followed by `Sim::addForm` 0x140197ae0), `ESCMenu::ESCMenu` 0x1400fcb40, `SimScreen::SimScreen` 0x140188540, `WrongWayIndicator`, `ProximityIndicator::ProximityIndicator` 0x140123880, `DriverNameDisplayer::DriverNameDisplayer` 0x14008c7e0, `DamageDisplayer::DamageDisplayer` 0x140078c10, `EndSessionDisplayer::EndSessionDisplayer` 0x14010a650, `OverlayLeaderboard::OverlayLeaderboard` 0x1401bf440 (guarded by a count > 1 and "session type is not Drag"; interpretation: more than one car), `SessionLeaderboard::SessionLeaderboard` 0x1400baf60, `FlagManager::FlagManager` 0x14009ab60, `FuelLight::FuelLight` 0x1400bb510, and finally `ksgui::GameScreen::showDesktop` 0x140250e70.
- `ESCMenu::ESCMenu` creates `SetupScreen::SetupScreen` 0x14017c010, `EscMenuTimeTable::EscMenuTimeTable` 0x140094230, `RaceControlScreen::RaceControlScreen` 0x1400b2370 and the comparative-telemetry page. `SetupScreen::SetupScreen` creates the special tabs and `QuickMenuPitstop::QuickMenuPitstop` 0x1400aca70, then `SetupScreen::loadINI` 0x14017d950 builds the generic tabs.
- Per car: `CarAvatar::initCommonPostPhysics` 0x1400d6190 creates `AnalogInstruments::AnalogInstruments` 0x140056480, `DigitalInstruments::DigitalInstruments` 0x1400eb030 (-> `initInstruments` 0x1400eb510) and `DigitalPanels::DigitalPanels` 0x1400820f0 (-> `initPanels` 0x140082740).

Per frame (render thread = main thread; see `Game::onIdle` 0x140242730):
```
Game::onIdle
  update pass over the GameObject tree      -> FlagManager::update 0x14009d140, SystemMessage::update 0x1401b66b0,
                                               DigitalInstruments::update 0x1400f0520 -> DigitalItem::update 0x1400f3190 / DigitalLed::update 0x1400f6200,
                                               DigitalPanels::update 0x140083f70, AnalogInstruments::update 0x140059770
  render pass                               -> SimScreen::render 0x140188610 -> Sim::renderScene 0x14019e570 (3D scene; DisplayNode::render 0x1400f74a0 draws dashboard panels)
  ksgui::GUI::render 0x140254c10            -> reset states, depth off, alpha blend; for each visible top-level control: vtable +0x18 render(dt); then the taskbar
        ksgui::Control::render 0x140244b80  -> background quad, texture quad, border line strip (GLRenderer begin / color4f / vertex3f / end / quad), Font::setColor + Font::blitString 0x140200e20, then children
        apps override render(dt): it is both their per-frame update and their draw call
  renderHUD pass over the GameObject tree   -> SystemMessage::renderHUD 0x1401b6520, DamageDisplayer::renderHUD 0x14007ae80, DriverNameDisplayer::renderHUD 0x14008d900,
                                               ProximityIndicator::renderHUD 0x140123a80, WrongWayIndicator::renderHUD 0x1401d87a0, FuelLight::renderHUD 0x1400bc030,
                                               OverlayLeaderboard::renderHUD 0x1401c1350, EndSessionDisplayer::renderHUD 0x14010b520, QuickMenuPitstop::renderHUD 0x1400b1230,
                                               Console::renderHUD 0x14018c550
```
So forms are drawn first and HUD objects on top (confirmed from the call order in `Game::onIdle`). The physics thread never touches UI objects; apps read the per-car state copy (`CarAvatar::physicsState`).

Input: `GUI::onMouseDown` 0x140254290 / `onMouseMove` 0x140254320 / `onMouseUp` 0x1402545c0 / `onMouseWheelMoved` 0x140254720 hit-test the control list and forward to the control's virtual handler, which fires its `Event<OnControlClicked>` etc.; `GUI::onKeyDown` 0x140254230 / `onKeyChar` 0x1402541e0 go to the focused control. `Form::onTitleClicked` 0x140246830 -> `GUI::setCapturedForm` 0x1402556c0 starts a drag. Game keys (menus, console toggle, app shortcuts) are handled in `Sim::onKeyDown` 0x14019a940, which calls `Console::show` 0x14018c7f0.

VR: `Sim::Sim` calls `GUI::init3DMode` 0x140253f10; the GUI is then drawn into a render target by `GUI::preRender3D` 0x1402547c0 (-> `doRender2D` 0x1402539d0) from `Sim::renderScene`, shown as a quad in the scene by `GUI::render3D` 0x140254900, with a head-tracked pointer `GUI::renderVPointer` 0x140254c30.

Layout persistence: `Form::Form` 0x140245890 -> `GUI::deserializeForm` 0x140252f90 -> `GUI::fromIni` 0x140253b20 (keys `POSX POSY VISIBLE BLOCKED SCALE`); `Sim::~Sim` -> `GUI::serializeForms` 0x140254d60 writes `acos.ini` (`[HEADER] VERSION=2`, `DESKTOP_SELECTED`). Desktops: `Taskbar::addDesktop` 0x1402599f0, `selectVirtualDesktop` 0x14025a2b0, `cycleDekstop` 0x140259df0.

Fonts: `Font::Font` 0x140200a60 / 0x140200bb0 (default families `Segoe UI`, `Consolas`); `Font::getFontWrapper` 0x1402010d0 creates one FW1FontWrapper per family (+ ` Bold` / ` Italic`); `Font::blitString` -> `kglBlitStringWide` 0x140019aa0. The FW1 font collection loader enumerates `content/fonts/*.ttf`. Dashboard text does not use this path: `DigitalItem` uses `StringBlitter3D` / `TextNode` with bitmap fonts from `content/fonts/` (renderer.md).

Setup editor: `SetupScreen::loadINI` reads each section of `setup.ini` (`TAB NAME POS_X POS_Y MIN MAX STEP HELP`, `[DISPLAY_METHOD] SHOW_CLICKS`, `[GEARS] USE_GEARSET`, gear ratio files) and calls `SetupTab::addItem` 0x140183850 to place a spinner on the named tab; help text comes from `acLocaleGetSetupHelpText` 0x140240410. Saving: `SetupIOManager::sendSaveMessage` 0x140177f20 -> `SetupScreen::saveSetupAbsolutePath` 0x14017f9c0, which concatenates every tab's `getStringToSave` (`SetupTab::getStringToSave` 0x140183e90: `[ITEM]` + `VALUE=n`; fixed sections `[TYRES]`, `[FUEL]`, `[GEARSET]`) plus `[CAR] MODEL=<car>`; pit presets go to a sibling `.sp` file (`SetupPitstopPresets::savePresets` 0x14017a8b0, sections `[PRESET_n]`). Loading: `SetupScreen::loadSetupAbsolutePath` 0x14017f2c0 -> each tab's `loadFromINI` (`SetupTab::loadFromINI` 0x140184120). `ESCMenu::onShutdown` 0x1400fdf80 auto-saves `setups\<car>\generic\last.ini`. The values themselves are applied to the car by `SetupManager` (physics side, docs/map).

Console: `Console::onKeyChar` 0x14018b480 -> `Console::parse` 0x14018bf50 looks the first word up in the command table (`Console::addCommand` 0x14018a530). Commands found: `help`, `exit` (built into `parse`), `set <var> <value>` (`ConsoleSet::execute` 0x14018c8f0), `refresh_instruments` (`ConsoleRefreshInstruments::execute` 0x1400eae80, reloads `digital_instruments.ini`), `testCrash` (`ConsoleCrash::execute` 0x14018a9d0), `saveff` (lambda, `ConsoleCommandLambda::execute` 0x140076bd0), `getSteerAssist` (CarAvatar), `vote_next` / `vote_restart` / `vote_kick` (online voting), `rSsA` (SteamInterface; purpose not read). Variables for `set`, registered with `Console::addVar` 0x14018a6c0 / `addVarLambda` 0x14018a830: `fov aniso driverEye.x/y/z graphicsOffset.x/y/z graphicsPitchRotation bias0 bias1 bias2 ffMult observeDigital observeFlames observePanel observeLights maxLayer mirrorpos.x/y/z shadowBias exposure minExposure hdrHPLevel hdrBloomLevel hdrBloomBaseLevel bodyDamageLevel engineDamageLevel debugDamage lodDebug hideHead headPitch headRoll headYaw headFilter headMaxG freeCamFov freeCamExp freeCamDof freeCamDofFocus aiLookAhead aiLookAheadSpeed aiPush aiSteerGain steerAssist remoteSpring remoteDamper remoteFactor` and the analog-instrument calibration values (`RPM_ZERO RPM_STEP SPEED_ZERO SPEED_STEP FUEL_ZERO FUEL_STEP FUEL_MIN WATER_ZERO WATER_MIN TURBO_ZERO TURBO_STEP`).

Localisation (counted in config.md): `wWinMain` -> `initLocalization` 0x1400c15c0 (language from `cfg/launcher.ini [WINDOW] LANGUAGE`) -> `acLocaleSetLocale` 0x140240980 -> `loadMainINI` 0x140240ee0 (`[STRINGS]` key = text) and `loadSetupINI` 0x1402414e0. UI code wraps literals in `acTranslate` 0x140240cf0 (about 170 call sites); an unknown string is returned unchanged.

Developer apps switch: `Form::Form` sets `devApp = true`; user-facing apps clear it in their constructor. `Sim::Sim` reads `system/cfg/assetto_corsa.ini [AC_APPS] ENABLE_DEV_APPS` and passes it to `Sim::addForm(form, enabled)`: when it is 0 and `form->devApp` is set, the form is added to the `GameScreen` without a taskbar icon and hidden; otherwise it gets an icon. The shipped file has `ENABLE_DEV_APPS=0`.

### The 56 SystemApps.lib source files

"user" / "dev" is the value the constructor leaves in `Form::devApp` (confirmed by disassembly; `user*` = the store writes a register that is zero by context, interpretation). Title = the string passed as the form title.

| Source file | Constructor | Fn / bytes | Kind | What it is |
|---|---|---|---|---|
| **Timing and race info** | | | | |
| ACEssentials | 0x14031bc60 | 23 / 15,245 | user | "Essentials": gear, speed, rpm and fuel bars, position, lap times. |
| PlayerCurrentTime | 0x1402d9660 | 13 / 11,261 | user* | "Laps": current / best / last lap with sector splits. |
| PerformanceMeterApp | 0x140302570 | 10 / 4,427 | user | "Performance Delta": delta bar against the best lap, estimated lap. |
| FormRealTimeTiming | 0x14032b8a0 | 13 / 13,676 | user | "Realtime": gaps to the cars ahead / behind, track status, wind. |
| RacePosition | 0x1402fe0f0 | 10 / 7,225 | user | "Race Position": position and lap counter. |
| LeaderboardApp | 0x140329bf0 | 9 / 1,059 | user | "Leaderboard": hosts an `ACStandardLeaderboard`. |
| ACStandardLeaderboard | 0x1402d10f0 | 29 / 25,665 | widget | Shared leaderboard table (entry list / time table per session, kick button); also used by RaceControlScreen, SessionLeaderboard, EndSessionDisplayer. |
| FriendsLeaderboardDisplayer | 0x140324fc0 | 20 / 10,087 | user* | "Friends Leaderboard" (Steam scores; reset / refresh). |
| TrackMap, TrackMapHandler, TrackMapSupport | 0x1402ee8e0, 0x14033c420 | 17+17+3 / 17,177 | user | "Track map": track `map.png` + `data\map.ini`, or a map drawn from the AI spline into a render target; car dots. |
| TrackDescription | 0x1403270e0 | 10 / 1,413 | user | "Track Description": name of the current track section. |
| FormRunningTime | 0x1402dc760 | 10 / 1,668 | user | "Running Time". |
| ACTimeFlow | 0x14030cb10 | 12 / 5,321 | user | "Time of the day": time, session time, time multiplier (sun animator). |
| RaceFuelMonitor | 0x140338250 | 10 / 2,142 | dev | "Race Fuel Monitor". |
| FormChat | 0x140308190 | 36 / 22,123 | user | "Chat": online chat with fade and export. |
| **Car and telemetry** | | | | |
| TyresApp | 0x140338860 | 11 / 12,647 | user | "Tyres": inner / middle / outer and core temperature, pressure, wear, compound. |
| FormGear | 0x1402e8280 | 25 / 11,349 | user | "Gears": gear, speed, rpm arc, turbo and KERS / ERS gauges. |
| CarPedalsForm | 0x1402dbc80 | 10 / 3,492 | user | "Pedals": gas / brake / clutch / steer / force-feedback bars. |
| ElectronicAidsForm | 0x1402ea500 | 10 / 3,735 | user | "Electronics": ABS / TC / EDL / DRS icons. |
| FormMGU | 0x1402eb080 | 10 / 6,514 | user | "ERS Management": MGU-K recovery / delivery, MGU-H mode, engine brake. |
| FormBallast | 0x1402f0ef0 | 10 / 1,763 | user* | "Ballast" spinner. |
| ACDriverPerformance | 0x140303bd0 | 18 / 17,595 | dev | "Driver performances": 0-50-0 ... 0-160-0 km/h test timer with saved records. |
| FormTelemetry | 0x14032f660 | 69 / 34,206 | dev | "Telemetry": live graph of named channels (physics or graphics side), presets. |
| TelemetryApp | 0x140302180 | 3 / 1,045 | dev | "SusTelemetry": suspension travel graph. |
| ACSuspension | 0x1402e3fd0 | 19 / 16,550 | dev | "Suspensions": wireframe of the suspension geometry, camber, tyre temperature run. |
| CarPhysicsForm | 0x1402d7680 | 11 / 10,754 | dev | "Car physics": CG height, tyre Dy, loads, slip angles and similar raw values. |
| ACDCarEngineer | 0x14031a0f0 | 10 / 5,860 | dev | "Car engineer": sprung / unsprung mass, ride frequency, damping ratios. |
| FormWings | 0x1402ec890 | 14 / 9,124 | dev | "Wings": per-wing aero values drawn on the car. |
| TyreTesterForm | 0x14033b520 | 11 / 4,626 | dev | "Tyre tester": slip curve graph from the car's tyre data. |
| TyreWearingDebug | 0x140319960 | 10 / 2,607 | dev | "Tyre debug". |
| SetupDebug | 0x140317a80 | 12 / 9,161 | dev | "Setup debug": dump of setup item values. |
| **Dev tools for content makers** | | | | |
| FormAI | 0x1402dcf80 | 47 / 32,523 | dev | "AI": AI line recorder (fast lane / pit lane), AI hints, fuel consumption estimate. |
| FormDRSZones | 0x140336210 | 14 / 5,068 | dev | "DRS Zones": edit and save track DRS zones. |
| FormCamera | 0x1402f1bc0 | 48 / 34,360 | dev | "Camera editor": edit the car's onboard cameras. |
| ChaseCamEditor | 0x1403372a0 | 17 / 4,771 | dev | "ChaseCam Editor": distance / height / pitch of the chase cameras. |
| FormDriverModel | 0x14031f0e0 | 22 / 12,303 | dev | "Driver Model Utilities": driver head movement parameters. |
| FormHDR | 0x140329e20 | 17 / 6,220 | dev | "HDR Settings": onboard / outboard exposure, save to the car. |
| PhotoMode | 0x140321bc0 | 33 / 14,790 | dev | "Photo mode": free camera with exposure / azimuth controls. |
| NetMonitorForm | 0x140303400 | 10 / 2,541 | dev | "Net monitor": ping, velocity and position error graphs (online only). |
| KeyboardControlsDebug | 0x14030dbe0 | 13 / 7,321 | dev | "Keyboard debug": keyboard steering parameters. |
| X360ControllerDebug | 0x1403276d0 | 24 / 9,196 | dev | "X360 Controller debug": gamepad steering parameters, save. |
| FormGameStats | 0x1402e7a10 | 10 / 2,890 | dev | "Graphics stats": update / render / HUD / physics / audio times. |
| GPUProfilerApp | 0x14031b320 | 3 / 2,578 | dev | "GPU profiler": per-pass GPU timer queries. |
| FormLogoAC | 0x1402ec650 | 2 / 614 | dev | "AC Logo" watermark form. |
| **Settings and utilities** | | | | |
| FormRenderStats | 0x1402d69d0 | 3 / 3,825 | user | "Render Stats": FPS, frame time, draw calls, triangles, physics time, FMOD usage. |
| FormVolumes | 0x1402f8fd0 | 36 / 18,402 | user* | "Audio": volume sliders. |
| FFBForm | 0x140314a10 | 34 / 14,238 | user* | "Force Feedback Controller": per-car FFB gain. |
| DriverEyesPositioner | 0x14030fa60 | 37 / 23,094 | user | "Onboard settings": seat position, FOV, pitch. |
| TripleScreenManager, VirtualDisplay, LateralVirtualDisplay | 0x1402ff920, 0x14033d980, 0x14033e3a0 | 19+8+2 / 14,353 | user | "Triple screen": screen angle / width / distance / bezel editor. |
| FormOpenVR | 0x1402fd030 | 16 / 5,029 | user* | "OpenVR": supersampling and IPD (created conditionally; interpretation: only in OpenVR mode). |
| PostProcessFilterSelector | 0x140329360 | 12 / 2,840 | user* | "Post Process Filter": pick a filter file. |
| ACHelp | 0x140307900 | 9 / 1,623 | user | "Help": key help text. |
| ACHideAll | 0x14030f3e0 | 12 / 1,567 | user | "Hide all Apps". |

## 4. What it reads from disk

Paths as built in code. `cfg/...` opened through `INIReaderDocuments` and all `/Assetto Corsa/...` paths are under the user's Documents folder; everything else is relative to the game folder.

| Who | Files |
|---|---|
| ksGUI | `content/gui/skin.ini` (`[CONTROL] [FORM] [TASKBAR]` `BACK BORDER FORE` colours), `cfg/acos.ini` (form layout), `cfg/gameplay.ini [GUI] ALLOW_OVERLAPPING_FORMS`, `system/cfg/assetto_corsa.ini [AC_APPS] SCALE_STEP / ENABLE_DEV_APPS`, `content/gui/controls/...` icons (form pin and zoom, taskbar scroll, spinner arrows), `content/gui/inapp_aclogo.png`, `content/gui/oculus_pointer.png`, `content/fonts/*.ttf` |
| Menus | `content/gui/inGameMenu/*`, `logo_esc_menu.png`, `pauseMenuLogo.png`, `system/cfg/fades.ini` (`ESC_MENU_FADE`, `FADE` in ms), `content/gui/raceControl/*`, `content/gui/endRace/*`, `system/cfg/endSessionDisplayer.ini`, `content/cars/<car>/skins/<skin>/livery.png`, `content/gui/NationFlags/*.png` |
| Setup | `content/cars/<car>/data/setup.ini`, `data/tyres.ini`, gear ratio files named by `setup.ini` (from `data/` or `data.acd`), `cfg/assists.ini`, `Documents\Assetto Corsa\setups\<car>\<track or generic>\<name>.ini` and `.sp`, `content/tracks/*.*` (track list), `content/gui/setupIO/*`, `content/gui/setupApps/carView.png`, `system/locales/setup/<lang>.tag` |
| Pit menu | `system/cfg/pitstop.ini [SETTINGS]` (sizes, colours, `PRESETS_COUNT`, `STAY_IN_CAR`, `USE_MOUSE_PITSTOP`) |
| HUD | `system/cfg/messages.ini`, `session_info.ini`, `name_displayer.ini`, `proximity_indicator.ini`, `damage_displayer.ini`, `mouse_hider.ini`; `cfg/gameplay.ini` (`[OVERLAY_LEADERBOARD] ACTIVE / VERTICAL`, name-tag `IS_ACTIVE`, notification toggles; several objects write it back in `shutdown`); `content/texture/damage/*.png`, `content/texture/wrongway.png`, `content/gui/flags/<name>.png`, `content/tracks/<track>/<layout>/data/overlays.ini`, `content/gui/fuelIndicator/*.png` + `data/car.ini`, `content/gui/driverNameDisplayer/*.png`, `content/gui/systemMessage/bar.png` |
| Instruments | `content/cars/<car>/data/digital_instruments.ini` (sections `ITEM_n`, `LED_n`, `RPM_SERIE_n`, `SPEED_SERIE_n`, `FUEL_n`, `KERS_*`, `DRS_*`, `TURBO_BOOST_*`, `GFORCE_SERIE_n`, `TYRE_LOCK_SLIP_n`, `PERF_LED_n` ...; item `TYPE` values `RPM SPEED GEAR FUEL LAPTIME BEST_LAP LAST_LAP DELTA_DISPLAY TC_LEVEL ABS_LEVEL TURBO_BOOST KERS_CHARGE WATER_TEMP PRESSURE CLOCK TEXT ...`), `data/digital_panels.ini` + `texture/display_panel/*.dds`, `data/analog_instruments.ini` (`RPM_INDICATOR SPEED_INDICATOR FUEL_INDICATOR WATER_TEMP TURBO_INDICATOR_n PLACE_HOLDER_n`), `data/analog_speed_curve.lut`; both digital files are watched with `FileChangeObserver` for live reload |
| Apps | `system/cfg/tyres_app.ini`, `chat_app.ini`, `map.ini`, `chase_cam.ini`, `telemetry_presets.ini`, `driver_performances.ini`, `ppfilters/*.ini`; `cfg/telemetry_user.ini`, `controls.ini`, `audio.ini`, `camera_onboard.ini`, `triple_screen.ini`, `openvr.ini`, `video.ini`, `ppfilters\*.ini`, `cfg/cars/<car>/view.ini`; `Documents\Assetto Corsa\savedData\driverPerformances\`, `out\chat*.txt`, `out\test_result.txt`; car files `data/car.ini`, `engine.ini`, `cameras.ini`, `dash_cam.ini`, `driver3d.ini`, `ai.ini`, `fuel_cons.ini`; track `map.png`, `data\map.ini`, `data/drs_zones.ini.candidate`; `system/locales/help/<lang>.tag`; `content/gui/<app>/...` and `content/texture/*.png` icons |
| Console | `Documents\Assetto Corsa\cfg\console.ini` |
| Localisation | `cfg/launcher.ini`, `system/locales/<lang>.ini`, `system/locales/setup/<lang>.tag`, `system/locales/tutorial/`, `Documents\Assetto Corsa\dblocales\` (`en.ini`, `templates/template.ini`: dumps of the default strings) |

## 5. Size

`python tools/re_query.py size <stems>`: **129 object files, 2,141 hand-written functions, 1,083,959 code bytes.**

| Group | Object files counted | Functions | Code bytes |
|---|---|---|---|
| ksGUI.lib (23) | ActiveButton, CheckBox, ConnectedLabel, Control, CustomSpinner, DefaultSystemMessage, Form, GameScreen, Graph, GUI, Label, ListBox, ListBoxRow, PopOver, ProgressBar, ScrollBar, Slider, Spinner, TabBar, Taskbar, TaskBarIcon, TextBox, TextInput | 305 | 108,054 |
| SystemApps.lib (56) | the files in the table of section 3 | 943 | 509,663 |
| Screens and menus (16) | SimScreen, Desktop, ESCMenu, PauseMenu, QuickMenu, QuickMenuPitstop, RaceControlScreen, RaceControlSessionInfo, RaceControlSessionServer, RaceControlSPSession, RaceControlVoting, EndSessionCommonGui, EndSessionDisplayer, EscMenuTimeTable, SessionLeaderboard, OverlayLeaderboard | 283 | 145,192 |
| Setup editor (10) | SetupScreen, SetupTab, SetupIOManager, SetupInformation, SetupElectronics, SetupFuelManager, SetupGearManager, SetupGenericCarSettings, SetupPitstopPresets, SetupTyresManager | 240 | 124,381 |
| HUD (14) | SystemMessage, SystemNotification, DriverStatusSystemMessage, SessionStatusSystemMessage, DriverNameDisplayer, CarLabel, ProximityIndicator, WrongWayIndicator, TimeDifferenceDisplayer, PitstopOverlay, DamageDisplayer, FlagManager, FuelLight, ScenePauseDimmer | 209 | 85,204 |
| Instruments (6) | DigitalInstruments, DigitalItem, DigitalLed, DigitalPanels, AnalogInstruments, DisplayNode | 108 | 88,442 |
| Console (4) | Console, ConsoleSet, ConsoleCommandLambda, ConsoleRefreshInstruments | 53 | 23,023 |

Not counted here: `LocalizationManager` (config.md, 20 / 6,284), `Font`, `StringBlitter3D`, `TextNode`, `GLRenderer` (renderer.md), `SetupManager` / `SetupItem` (physics, docs/map), `PythonInterface` and the plugin SDK forms (`PythonForm`, `AppForm`: python_api.md), `FormVolumes` and `FormAI` are counted here although audio.md and ai_drivers.md describe them. UI-flavoured objects left for the areas that own their logic (sizes for whoever picks them up): `ComparativeTelemetry` (33 / 23,029; the ESC menu "telemetry" page) and `ComparativeTelemetryRecorder` (32 / 17,016), `ACChat` (27 / 9,057) and `QOSOverlay` (3 / 728) (multiplayer), `ReplayInterface` (71 / 38,738; replay), `PitStop` (57 / 28,371), `MouseHider` (17 / 2,694), `WarningReporter` (14 / 8,248). Third-party: FW1FontWrapper is outside the hand-written set.

## 6. Port difficulty

**L** overall, by volume only (the largest non-physics area: about 1.08 MB of code), with low technical risk and no effect on physics correctness. Nothing here needs to be bit-exact.
- ksGUI itself: **do not port, replace** (S). An immediate-mode crate such as `egui` (or `iced`) covers every widget in the set, including plots (`egui_plot`) and docking / movable windows; text through `glyphon` / `cosmic-text` instead of DirectWrite + FW1FontWrapper. Keep only the `acos.ini` layout format if app positions should carry over.
- Built-in apps: **L** as a whole but fully incremental: each app is an independent window reading the per-car state copy. A useful first set is Essentials, Laps, Tyres, Pedals, Performance Delta, Realtime, Track map; the dev apps are optional (several write car data files and matter only for modding).
- Setup editor: **M**. The file formats matter (`setup.ini` item definitions, the saved setup `.ini` / `.sp`), and it depends on the physics-side `SetupManager` and on the rules for fixed setups online.
- Menus and HUD: **M**; they depend on session logic (RaceManager), multiplayer (voting, kicks) and replay.
- In-car instruments: **M**; depends on the renderer port (kn5 node lookup, emissive material variables, dynamic textures, 3D text) and on the two ini formats, which every car ships.
- Console: **S** (a command map and a variable map; could be an `egui` window).
- Localisation: **S** (key = text ini files; any i18n crate or a `HashMap`).
- Third-party involved: FW1FontWrapper + DirectWrite (fonts), Steam (friends leaderboard through SteamInterface). No SDK is needed if the widget library is replaced.

## 7. Open questions

- `user*` rows: the `devApp` store uses a register whose value was not traced (it is the usual zero register of those constructors). Rows marked "dev" with an explicit store of 1 (RaceFuelMonitor, TelemetryApp, ACDCarEngineer, FormDriverModel, PhotoMode, FormHDR) may sit on a conditional path that was not read; PhotoMode in particular is user-visible in practice, so treat its flag as unverified.
- The exact section naming in `acos.ini` (the `DESK_` / `_FORM_` pieces in `GUI::deserializeForm` and `deserializeFormDesktop` 0x1402533a0) and the number of virtual desktops were not read.
- Which key opens what (`Sim::onKeyDown` is 14 KB) was not mapped; input_ffb.md / main_loop.md may cover the key table.
- `Desktop`, `QuickMenu`, `SetupGenericCarSettings`, `TimeDifferenceDisplayer`, `PitstopOverlay`, `ScenePauseDimmer` are counted (469 bytes each) but are dead code in this build; choice made: list them, port nothing.
- Console command `rSsA` (registered by SteamInterface) and `saveff` were not read; guess: Steam stats reset and saving the force-feedback multiplier.
- `ComparativeTelemetry*`, `MouseHider` and `WarningReporter` were deliberately left uncounted so that the areas owning their logic (or misc.md) can count them; if nobody does, they belong here.
- `GUI::render` returns early when a GUI render target exists (VR mode); whether HUD `renderHUD` objects are also redirected into that target was not checked.
