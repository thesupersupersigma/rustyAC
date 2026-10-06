# Main loop: from process start to the per-frame loop

Source: `re/decomp/` pseudo-C (folders `acs`, `Game`, `GameObject`, `GameTime`, `ksTimer`, `kssystem`, `sim`, `SplashScreen`, `PhysicsDriveThread`, `PhysicsAvatar`, `ThreadPool`, `TimerProcess`, `CrashGuard`, `Win32FaultHandler`, `StackWalker`, `ACErrorHandler`, `DebugLog`, `BenchmarkMode`), `re/types/*.txt`, and `tools/re_query.py` (callers / callees / strings / vtable). Read-only: nothing in the game folder, the Ghidra project or git was changed. "Confirmed" below means read in the pseudo-C; "interpretation" is marked as such.

## 1. What it is

`acs.exe` is started by the launcher with a finished session description on disk (`Documents\Assetto Corsa\cfg\race.ini`); it has no menu of its own. `wWinMain` parses a handful of command-line switches, installs the log and the crash handler, starts Steam, reads the video settings, checks that the track and cars named in `race.ini` exist, and builds one `Game` object. `Game` owns the window, the D3D11 graphics manager, the audio engine, the GUI, a frame timer and a tree of `GameObject`s; every game feature is a `GameObject` with four virtual hooks (update, render, renderHUD, renderAudio) plus shutdown. The first object put in the tree is the `SplashScreen`. After it has drawn one frame it constructs `Sim` on the main thread; the `Sim` constructor is one very long function that builds the whole session (physics, track, cars, replay, apps, cameras, game modes) while the splash redraws itself after each progress message. From then on `Game::run` loops: pump Windows messages, measure frame time, walk the tree four times (update, render, HUD, audio), present. Physics does not run in that loop: `PhysicsAvatar` owns a `PhysicsDriveThread` that steps the physics engine in fixed 3 ms steps on its own thread, catching up with the wall clock, and the render side only copies the latest car states across under a lock. Shutdown is the reverse: a flag stops the loop, every object gets `shutdown()`, the tree is deleted, and `wWinMain` closes network, COM and Steam and prints `CLEAN EXIT`.

## 2. Main classes

| Class / unit | Source file (library) | Size | Role |
|---|---|---|---|
| `wWinMain` and helpers | acs.obj | - | Entry point, command line, pre-flight checks. |
| `Game` | Game.obj (ksGame.lib) | 0x278 | Window + graphics + audio + GUI + timer + root of the object tree; owns the loop. |
| `GameObject` | GameObject.obj (ksGame.lib) | 0x58 | Tree node: `game`, `name`, `isActive`, `parent`, `gameObjects` (children). Vtable: +0x00 dtor, +0x08 `update(dt)`, +0x10 `render(dt)`, +0x18 `renderHUD(dt)`, +0x20 `renderAudio(dt)`, +0x28 `shutdown()`. |
| `GameTime` | GameTime.obj (KunosSim.lib) | 0x38 | Frame clock: `now` (ms), `deltaT` (s), `smoothDeltaT`, `fps`, `cappedFPS` (frame limiter, ms), `useStabilizer`. |
| `ksTimer` functions | ksTimer.obj (ksCommon.lib) | - | Millisecond clock over `QueryPerformanceCounter`, with fallback to `timeGetTime`. |
| `kssystem` functions | kssystem.obj (ksCommon.lib) | - | CPU count, thread affinity, OS version string, deliberate crash (`ksGenerateCrash`). |
| `Sim` | sim.obj | 0x2d0 | The session. A `GameObject` and key listener; owns scene-graph roots, `cars`, track, managers, a main-thread job queue (`chFunctions`). |
| `SplashScreen` | SplashScreen.obj | 0x1c8 | Loading screen; creates `Sim`; shows car logo and hints. |
| `PhysicsAvatar` | PhysicsAvatar.obj | 0x498 | `GameObject` that embeds the `PhysicsEngine` (0x278) and the `PhysicsDriveThread`; bridge between threads (state copy, command queue). |
| `PhysicsDriveThread` | PhysicsDriveThread.obj | 0xb8 | The physics thread: fixed-step loop, input polling, pause, time scale. |
| `ThreadPool` / `WorkerThread` | ThreadPool.obj (ksThreads.lib) | 0x68 | Worker threads used only by `PhysicsEngine::step` to step cars in parallel. |
| `TimerProcess` | TimerProcess.obj (ksThreads.lib) | 0x38 | Alternative physics driver: a 3 ms Windows multimedia timer (`timeSetEvent`) instead of a thread. |
| `CrashGuard` / `Win32FaultHandler` / `StackWalker` | KunosSim.lib | 0x8 / - / 0x30 | Unhandled-exception filter that prints a symbolised call stack (StackWalker is a well-known open-source helper built on dbghelp). |
| `ACErrorHandler` | ACErrorHandler.obj | 0x28 | Collects error/warning lines during loading; writes `logs/errors.txt`. |
| `DebugLog` | DebugLog.obj (KunosSim.lib) | 0x18 | Redirects stdout to `logs/log.txt`; all `printf` output is the game log. |
| `DebugTimer` | DebugTimer.obj (KunosSim.lib) | 0x30 | Scope timer that prints `DEBUG TIMER: name:ms`. |
| `BenchmarkMode` | BenchmarkMode.obj | 0xb8 | Collects FPS statistics and writes the benchmark result file. |
| `Trigger`, `ErrorBlob` | ksCommon.lib | 0xc / 0x20 | Tiny helpers (edge detector; error-string holder). |

## 3. Entry points

### 3.1 Process start: `wWinMain` 0x1400c29c0 (called by the CRT start-up)

In order (confirmed):
1. `SetDllDirectoryW("system/x64")`.
2. Command line (the whole command-line string must equal the switch exactly, so only one switch can be given): `-request` -> `printRequestCode` 0x1400c2740 and exit; `-video_modes` -> `printVideoModes` 0x1400c27d0 and exit; `-test`, `-get_car_range`, `-get_car_bestlap` set `PhysicsEngine::isTestMode` with a test mode (AI / CarRange / BestLap); `-autodrive` sets `PhysicsEngine::autoDrive`; `-online` sets `Sim::forceOnline`.
3. `DebugLog::DebugLog` 0x140217ef0, then `CrashGuard::CrashGuard` 0x140218800 with the name `acs.pdb`.
4. `checkRedirect` 0x1400c1430: optional `redirect.ini` `[REDIRECT] PATH` -> `SetCurrentDirectoryW`.
5. Delay-loads `python33.dll` and `openvr_api.dll` (message box and `exit(2)` on failure), then `SteamInit` (if Steam fails the function just unwinds and returns).
6. Reads the first line of `changelog.txt` into `Sim::acVersionString`; `CoInitializeEx`.
7. `loadVideoSettings` 0x1400c18d0 (`cfg/video.ini` -> `VideoSettings`), `InitKSNetwork`.
8. `checkRaceINI` 0x1400c09b0: `cfg/race.ini` must name an existing `content/tracks/<TRACK>/...kn5` and, for each `[CAR_n]`, a car for which `checkIfCarExists` 0x1400c02c0 finds every `LOD_n FILE` of `data/lods.ini`. On failure: message box `ERROR` and exit.
9. `initLocalization` 0x1400c15c0 (language from `cfg/launcher.ini`).
10. `Game::Game` 0x140241880 with the title "Assetto Corsa" and the video settings.
11. `system/cfg/assetto_corsa.ini`: `[ASSETTO_CORSA] SLEEP_TIME` -> `Game::sleepTime`, `WARNINGS_AS_ERRORS` -> `INIReader::crashAtError`, `[TIMER] FORCE_TGT` -> `ksTimerForceTGT` 0x140238bc0.
12. Creates `SplashScreen` 0x14019faf0 (0x1c8 bytes), adds it under `game.root`, calls `Game::run` 0x140242eb0.
13. After the loop: `Game::~Game` 0x140241eb0, `ShutdownKSNetwork`, `CoUninitialize`, `SteamShutdown`, prints `CLEAN EXIT`.

### 3.2 `Game::Game` 0x140241880 (confirmed from callees and strings)

`GameTime`, `RenderWindow::RenderWindow` 0x1401fd640, `KeyboardManager`, `JoypadManager`, `ksInitTimer` 0x140238aa0 (`timeBeginPeriod(1)`, QPC frequency and start values), `GraphicsManager`, `ksgui::GUI`, `AudioEngine` (enabled by `system/cfg/audio_engine.ini [SETTINGS] ENABLE_AUDIO`, master volume from `cfg/audio.ini [LEVELS] MASTER`), root `GameObject` named `ROOT`, handlers for window-closed (sets `isClosing`) and window-resize, GUI attached to window events. `gameTime.cappedFPS = videoSettings.fpsCapMS`.

### 3.3 The frame: `Game::run` 0x140242eb0 -> `Game::onIdle` 0x140242730

```
Game::run
  while RenderWindow::step (0x1401fdda0 -> kglWindowStep 0x140030ea0: PeekMessage/Translate/Dispatch until empty; false on WM_QUIT)
    if isClosing: fire evOnShutdownRequested; if still closing -> leave loop
    Game::onIdle
      GameTime::update 0x14044c250           frame limiter + dt
      evOnBeginFrame handlers
      if deltaT > 0:
        Game::update(root, dt) 0x140243010   depth-first, vtable +0x08, skips inactive subtrees
        evOnPostUpdate
        GraphicsManager::beginScene 0x140202580
        Game::render(root, dt) 0x140242d50   vtable +0x10
        GraphicsManager::setScreenSpaceMode; evOnPreGUI
        if isRenderingGui: ksgui::GUI::render; Game::renderHUD(root, dt) 0x140242e30 (vtable +0x18)
        evOnPostGui
        Game::renderAudio(root, dt) 0x140242dc0 (vtable +0x20); AudioEngine::update
        if isSwappingBuffer: GraphicsManager::endScene 0x140202850 -> kglSwapBuffers 0x140019080 (IDXGISwapChain::Present, sync interval 1 if vSync)
        else: evOnRenderFinished
        if sleepTime != -1: ksSleep(sleepTime)
      evOnEndFrame handlers
  AudioEngine::stop; isClosing = true; Game::shutdownObject 0x140242fc0 for each child of root (vtable +0x28); root->shutdown()
```

`GameStats` (updateTime, renderTime, renderHUDTime, renderAudioTime, cpuTime) is filled from `ksGetQPTTime` 0x1402389d0 around each phase. The 3D scene itself is not drawn by `Game::render`: `Sim::render` 0x14019e4b0 only stores `lastDT`; the scene is drawn from the GUI pass through `SimScreen::render` 0x140188610 -> `Sim::renderScene` 0x14019e570 (scene-graph traversal, mirrors, 3D GUI). See renderer.md for the passes.

### 3.4 Loading: `SplashScreen` -> `Sim::Sim` 0x140192070

`SplashScreen::SplashScreen` registers `SplashScreen::postRender` 0x1401a0e50 on `Game::evOnEndFrame`. Once one splash frame has been drawn (`screenCount > 0`) it mutes audio, does `new Sim(game)` (blocking, on the main thread), adds it to the root, restores the saved vsync flag, calls `Sim::onPostLoad` 0x14019e020, and prints `TIME TO INIT`. During the blocking constructor, `Sim::setSplashMessage` 0x14019e860 -> `SplashScreen::setCurrentMessage` 0x1401a1730 draws the splash HUD and presents a frame directly, which is how the loading screen keeps moving.

What `Sim::Sim` creates, in call-site order (confirmed from the callee list; grouped, not every form listed):
1. `CommandManager`, `OptionsManager`, `INIReader::clearCache`, settings from `system/cfg/fades.ini`, `cfg/video.ini`, `cfg/gameplay.ini`.
2. `Sim::createCamera` 0x1401982e0 (plain / Yebis post-processing / Oculus / OpenVR / triple-screen scene camera).
3. `Console`, `ACErrorHandler`, `SharedFileOut`, **`PhysicsAvatar::PhysicsAvatar` 0x140120930** (constructs `PhysicsEngine` 0x140262430 and the drive thread object, not yet running), `SharedMemoryWriter`.
4. `ksgui::GameScreen`, `FormRenderStats`, `SkyBox`, `Sim::initSceneGraph` 0x140199d70, `WarningReporter`, `MirrorTextureRenderer`, `ScreenCapturer`, `SystemMessage`, `SystemNotification`, `WeatherManager`, `MicroSectors`.
5. **`RaceManager::RaceManager` 0x140132db0 + `RaceManager::loadSessions` 0x14013ecb0**: picks `initReplay` 0x14013e820, `initOnline` 0x14013d160 or `initOffline` 0x14013a6c0. These call `Sim::loadTrack` 0x14019a4c0 (new `TrackAvatar`) and `Sim::addCar` 0x1401978a0 / `Sim::addNetCar` 0x140197c30 (new `CarAvatar`, see content_loading.md). Track and cars are loaded here.
6. `PushToPassManager`, `ReplayManager`, `ACHideAll`, `PauseMenu`, then about 45 in-game apps/forms (`ACEssentials`, `FormChat`, `TyresApp`, `LeaderboardApp`, `FormTelemetry`, `TrackMap`, `FFBForm`, ...), `TimeLimitedTest`, `PythonInterface`.
7. `ComparativeTelemetryRecorder`, `ESCMenu`, one game mode (`DragMode` / `DriftMode` / `TimeAttack` / `HotlapMode` / `RaceEvent`), `TrackAvatar::initPitCrew`, `PluginManager`, `SimScreen`, `Sim::initCubemaps` 0x1401997a0, `RemoteTelemetryUDP`.
8. `MouseHider`, `WrongWayIndicator`, `ACOperatingSystem`, `ACCameraManager`, `ChaseCamEditor`, `StartingLights`, `StartingTrackSemaphore`, `UDPCommandListener`, `JsonOutputFile`, penalty/lap-invalidation avatars, `ProximityIndicator`, `PitStop`, `DriverNameDisplayer`, `DamageDisplayer`, `FanatecUpdater`, `DebugVisualizer`, `DrivingAssistManager`, `Sim::initHDRLevels` 0x140199b00.
9. `RaceManager::startRace` 0x140141470, `EndSessionDisplayer`, `OverlayLeaderboard`, `SteamInterface`, `FriendsLeaderboardDisplayer`, `SessionLeaderboard`, `VirtualMirrorRenderer`.
10. **`PhysicsAvatar::startPhysics` 0x1401223a0** (physics thread starts), `Sim::unloadMeshResources` 0x14019edd0, window focus, `FlagManager`, `FuelLight`, `Sim::startGame` 0x14019eb00 (camera fade-in), `BenchmarkMode` 0x14005b210, GUI 3D mode.

Per frame, `Sim::update` 0x14019ef90 drains the `executeOnMainThread` queue (`Sim::executeOnMainThread` 0x140199240 is called from network, physics-completed and plugin code), forwards physics collision events (`Sim::stepPhysicsEvent` 0x14019ebd0), sorts cars, raises the "CPU OCCUPANCY > 99%" warning and logs audio statistics. Keyboard shortcuts are in `Sim::onKeyDown` 0x14019a940 (14 kB).

### 3.5 Threads

| Thread | Created by | What runs on it |
|---|---|---|
| Main / render | process | Message pump, all `GameObject` hooks, D3D11, GUI, FMOD `update`, network message dispatch to game objects (through `executeOnMainThread`), loading. |
| Physics | `PhysicsDriveThread::start` 0x140123480 (std::thread running `PhysicsDriveThread::run` 0x140123150) | DirectInput polling + `DICommandManager::step`, `PhysicsEngine::step` 0x140264760 (dt = 0.003), `evPhysicsStepCompleted`. docs/map/car_step.md covers the inside. |
| Physics timer (alternative) | same function when `useTimerProcess`: `TimerProcess::TimerProcess` 0x1402d0230, `timeSetEvent(3 ms)` -> `timer2_callback` 0x1402d0380 -> `PhysicsDriveThread::step` 0x140123560 | Same work, driven by the multimedia timer. `[THREADING] USE_TIMER_PROCESS` (-1 = automatic: only for dual-core CPUs, per the ini comment). |
| Physics workers | `ThreadPool::ThreadPool` 0x1402d04a0, called only from `PhysicsEngine::PhysicsEngine` | `WorkerThread::operator()` 0x1402d0a00 runs tasks queued by `ThreadPool::addTask` 0x1402d0f30 (only caller: `PhysicsEngine::step`). Count from `[PHYSICS_THREADING] THREADS`; disabled below 4 logical CPUs. |
| Fanatec SDK | `FWThreadBase::Start` 0x1401e46f0 | Wheel LED/FFB command threads of the Fanatec SDK (interpretation from the `FW*` class names; input_ffb.md). |
| One-off | `AISplineRecorder::recomputeSidesFromCsv` 0x140296870 | Tool thread (ai_drivers.md). |

No other thread creation exists in hand-written code (searched the pseudo-C for `std::thread` launch, `_beginthreadex`, `CreateThread`, `timeSetEvent`). Audio, Steam, Python, networking sockets and replay have no hand-written thread: FMOD and Steam use their own internal threads (interpretation), sockets are polled.

Cross-thread traffic: `PhysicsAvatar::executeOnPhysicsThread` 0x140121f20 queues closures (a concurrent queue) that `PhysicsAvatar::stepCommandQueue` 0x1401223e0 runs on the physics side; `PhysicsAvatar::onPhysicsStepCompleted` 0x1401221a0 copies car states under a critical section; `PhysicsAvatar::update` 0x140122480 (main thread) hands them to each `CarAvatar::setNewPhysicsState`, optionally time-corrected ("matrix smoothing").

### 3.6 Timing and pause

- Clock: `ksGetTime` 0x140238a30 returns milliseconds since start from QPC; `ksTimeSyncAndCheck` 0x140238b30 compares QPC with `timeGetTime` each frame and permanently switches to the 1 ms timer if they drift more than 500 ms.
- Frame limiter: `GameTime::update` busy-waits (no sleep) until `cappedFPS` ms have passed since the last frame (`cfg/video.ini [VIDEO] FPS_CAP_MS`). `deltaT` is clamped to 0.2 s; `smoothDeltaT` is a 99/1 then 90/10 low-pass; `useStabilizer` replaces `deltaT` by the smoothed value.
- Vsync: `kglSwapBuffers` passes `videoSettings.vSync` as the Present sync interval.
- Physics clock: `PhysicsDriveThread::run` computes `ksGetTime() * timeScale` and steps `currentTime += 3.0` ms until it catches up; a gap above 1000 ms resets the clock ("RESET PHYSICS TIMER"); more than one step per wake-up prints "WAS LATE" and counts `physicsLateLoops`. `occupancy` = step CPU time as a percentage of 3 ms. `timeScale` is 1.0, 20.0 in test mode, 5.0 from a developer button in `FormAI`; `ksSleep(0)` between wake-ups.
- Pause: `Sim::setPauseMode` 0x14019e6d0 -> `PhysicsAvatar::pausePhysics` 0x140122300 sets `driveThread.isPaused`; the thread then only polls input, calls `PhysicsEngine::stepPaused` and sleeps 10 ms. The render loop keeps running.

### 3.7 Shutdown and crash handling

- Exit: anything that calls `Game::shutdown` 0x140242fb0 or closes the window sets `isClosing`. `Sim::shutdown` 0x14019e9e0 shuts the GUI, saves the ESC-menu state, writes the result JSON (`Sim::writeOutputJson` 0x14019f950) and the user FF gain to `cfg/user_ff.ini`. `PhysicsAvatar::shutdown` 0x140122360 -> `PhysicsDriveThread::stop` 0x140123820 and DirectInput cleanup. `Game::deleteGameObjectRec` 0x1402426b0 then deletes the tree; `Sim::~Sim` 0x1401956d0 serialises app window positions.
- `ac_restart` 0x14012cec0 (exposed to Python) writes `[RESTART] ACTIVE=1` into `Documents/.../cfg/race.ini`.
- Crash: `InstallFaultHandler` 0x14022d180 = `SetUnhandledExceptionFilter(Win32FaultHandler)`. `Win32FaultHandler` 0x14022d190 prints `CRASH in:` and a call stack via `StackWalker::ShowCallstack` 0x14022fa80. Fatal game errors call `ks_critical_error` 0x140024c70 -> `ksGenerateCrash` 0x1402399a0, which prints the message and raises exception 0x29a so the same handler runs.
- `ACErrorHandler::writeLine` 0x1400560d0 / `writeWarning` 0x1400561e0 collect lines; `ACErrorHandler::~ACErrorHandler` 0x140055df0 writes them to `logs/errors.txt`.
- `BenchmarkMode::renderHUD` 0x14005c0c0 gathers frame statistics; `BenchmarkMode::generateResultString` 0x14005b370 writes `Documents/Assetto Corsa/benchmarks/acbench_*.txt` (enabled by `race.ini [BENCHMARK] ACTIVE`, read in `RaceManager::loadSessions`).

## 4. What it reads from disk

| Path (as built in code) | Reader | Purpose |
|---|---|---|
| `redirect.ini` (working dir) | `checkRedirect` | Optional change of working directory. |
| `changelog.txt` | `wWinMain`, `SplashScreen::SplashScreen` | First line = version string. |
| `cfg/video.ini` (Documents) | `loadVideoSettings` | Resolution, AA, vsync, FPS cap, post-processing flags. |
| `cfg/race.ini` (Documents) | `checkRaceINI`, `SplashScreen::initHints`, `RaceManager::loadSessions` | Session description (config.md). |
| `content/tracks/<track>/...kn5`, `content/cars/<car>/data/lods.ini` + LOD files | `checkRaceINI`, `checkIfCarExists` | Existence checks only. |
| `cfg/launcher.ini` (Documents), `system/locales/` | `initLocalization` | Language. |
| `system/cfg/assetto_corsa.ini` | `wWinMain`, `DebugLog`, `PhysicsDriveThread`, `PhysicsEngine`, `Sim::Sim` | `[ASSETTO_CORSA]`, `[TIMER]`, `[LOG] SUPPRESS`, `[THREADING]`, `[PHYSICS_THREADING]`. |
| `cfg/controls.ini` (Documents) | `PhysicsDriveThread::PhysicsDriveThread` | `[HEADER] INPUT_METHOD` (whether the physics thread polls DirectInput). |
| `system/cfg/audio_engine.ini`, `cfg/audio.ini` | `Game::Game` | Audio on/off, master volume. |
| `system/cfg/fades.ini`, `content/gui/splash.png`, `content/cars/<car>/logo.png` | `SplashScreen` | Loading screen. |
| `acs.pdb` | `CrashGuard` | Symbols for the crash call stack. |
| Written: `Documents/Assetto Corsa/logs/log.txt`, `logs/errors.txt`, `out/`, `benchmarks/acbench_*.txt`, `cfg/user_ff.ini` | `DebugLog`, `ACErrorHandler`, `BenchmarkMode`, `Sim::shutdown` | Outputs. |

## 5. Size

`python tools/re_query.py size` on the stems below: **313 hand-written functions, 144,612 code bytes** in total.

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| sim.obj | 114 | 68,084 | | StackWalker | 18 | 8,397 |
| acs.obj | 17 | 16,277 | | BenchmarkMode | 12 | 7,798 |
| PhysicsAvatar | 50 | 12,641 | | Game | 19 | 7,270 |
| SplashScreen | 22 | 9,115 | | PhysicsDriveThread | 8 | 3,858 |
| ThreadPool | 6 | 3,577 | | DebugLog | 4 | 2,643 |
| ACErrorHandler | 5 | 1,850 | | kssystem | 5 | 589 |
| ksTimer | 8 | 457 | | GameTime | 4 | 452 |
| GameObject | 3 | 409 | | TimerProcess | 4 | 368 |
| DebugTimer | 3 | 276 | | Win32FaultHandler | 2 | 187 |
| CrashGuard | 3 | 149 | | ErrorBlob / Trigger | 3 / 3 | 110 / 105 |

Not counted here: RenderWindow / kglWindow / GraphicsManager (renderer), AudioEngine (audio), RaceManager (session logic), KeyboardManager / JoypadManager / DirectInput (input), PhysicsEngine (docs/map). `CarHintLoader::getCarName` / `getHints` are compiled into SplashScreen.obj but belong to content loading.

## 6. Port difficulty: **M**

- The loop itself, the object tree, the timer, the physics thread and the cross-thread hand-off are small and plain (about 20 kB without sim.obj). In Rust: `winit` for the window and message pump, `std::time::Instant` for the clock, `std::thread` + channels for the physics thread and the command queues, `rayon` or a small pool for the car workers.
- The hard part is `Sim::Sim` and `Sim::onKeyDown` (28 kB together): they are wiring for about 150 other classes, so they cannot be finished before those areas exist. The port will grow with the atlas, not be done in one go.
- Bit-exact physics only needs the fixed 3 ms stepping and the car order; the catch-up policy, occupancy counters and time scale are free to reimplement.
- Do not port: StackWalker and the fault handler (use a Rust panic hook / `backtrace`), DebugLog (`log` crate), ksTimer's QPC/TGT fallback, TimerProcess (dual-core workaround), the request-code switch.
- Third-party touched here: Steam API (`SteamInit`), Python 3.3 and OpenVR are delay-loaded before anything else; COM is initialised for DirectInput/XAudio. All can be stubbed for a first headless build.
- Depends on: config (INIReader), renderer (window, present), audio, GUI, session logic (RaceManager), every feature area through `Sim::Sim`.

## 7. Open questions

- `Sim::Sim` order above is the order of first call sites in the binary (`re_query.py callees`). Branches (online vs offline, VR, dev apps enabled) were not followed, so which objects are conditional is not recorded.
- `USE_TIMER_PROCESS = -1` being "timer for dual cores only" is taken from the comment in `assetto_corsa.ini` and the `ksGetNumberOfProcessors` call in the constructor; the exact threshold was not read.
- The pool size rule for `[PHYSICS_THREADING] THREADS = -1` was not decoded (the constant 3 and a `< 4 cores` cut-off are visible).
- "FMOD/Steam run their own threads" and "sockets are polled" are interpretations from the absence of thread creation in hand-written code; the network area should confirm where `UDPSocket::receive` is called from.
- Who calls `Game::shutdown` was not resolved by the caller index (probably called through lambdas in ESC/pause menus).
- `GameTime::useStabilizer`: no writer other than the constructor was found by name; it may be set through a raw offset.
- Whether `Game::isSwappingBuffer == false` (fire `evOnRenderFinished` instead of presenting) is the VR path was not checked.
- Choice made: PhysicsAvatar is counted here (thread bridge) rather than in the car-physics maps; RenderWindow is left to renderer.md.
