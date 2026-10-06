# Python app API and plugin interfaces

Source: `re/decomp/` pseudo-C (folders `PythonInterface`, `PyPlugin`, `PyInterpreter`, `PyModule`, `PyEnvironment`, `PluginManager`, `ACPlugin`, `ACOperatingSystem`, `AppForm`, `ACControl`, `ACGL`, `ACFont`, `ACTexture`, `ACINIReader`, plus `sim/Sim__Sim` and `acs/wWinMain`), `re/types/*.txt`, `tools/re_query.py` (strings / callers / callees / vtable / size), `tools/disasm.py dis 0x140124920` (to read the handler address of every registered Python function), and the plain-text `apps/python/system/acsys.py`, `plugins/plugins.ini`, `Documents\Assetto Corsa\cfg\python.ini` (read only). Nothing in the game folder, the Ghidra project or git was modified. "Confirmed" means read in the pseudo-C or disassembly; "interpretation" is marked.

## 1. What it is

`acs.exe` embeds a CPython 3.3 interpreter so that small Python scripts ("apps") can open windows in the in-game GUI and read live car and session data. The interpreter lives in `python33.dll`, which `wWinMain` delay-loads right after `SetDllDirectoryW("system/x64")`; the game exits with "Failed to load Python, exiting" if that fails. When `[PYTHON] ENABLE_PYTHON` in the user's `cfg/gameplay.ini` is on, `Sim::Sim` creates one `PythonInterface` object. Its constructor builds a built-in module called `ac` from 116 C functions, starts the interpreter, and imports every app that is switched on in `cfg/python.ini`: an app is a folder `apps/python/<name>/` containing `<name>.py`. Each imported app becomes a `PyPlugin`, a normal game object in the main object tree: the game calls the script's `acMain` once, then `acUpdate(deltaT)` every rendered frame and `acShutdown()` at exit. Windows created by a script are ordinary `ksgui::Form`s (class `PythonForm`); controls are addressed by small integer handles; a script can also register a render callback and draw with a tiny immediate-mode `ac.gl*` API. Everything runs on the main (render) thread and reads the render-side copy of the car state, so Python never touches the physics thread directly.

Two older C++ routes sit next to it. `acSDK.lib` is a C++ app SDK: DLLs in `apps/` get an `IACOperatingSystem` interface to create forms, textures and read car data. `PluginManager` / `ACPlugin` load DLLs from `plugins/` that are listed in `plugins/plugins.ini`; such a plugin receives a car state struct every frame and may replace the player's controls. The third integration route, used by most external tools, is the shared-memory writer (`SharedMemoryWriter`, pages `acpmf_physics` / `acpmf_graphics` / `acpmf_static`): see docs/map/telemetry.md.

## 2. Main classes

| Class | Source file / library | Size | Role |
|---|---|---|---|
| `PythonInterface` | PythonInterface.obj | 0x1d8 | `GameObject` (no update of its own). Owns the interpreter, the list of `PyPlugin`s, the handle tables `controls` (vector of `ksgui::Control*`) and textures, the log stream, `evOnSetupAppCreated`. A global `pyi` points at it; every `ac_*` handler goes through `pyi`. |
| `PythonForm` | PythonInterface.obj | - | `ksgui::Form` subclass with a vector `listeners` of Python callables (render callbacks). |
| `PyPlugin` | PyPlugin.obj | 0x80 | `GameObject` per app: `module`, `acUpdateFunc`, `acShutdownFunc`; vtable +0x08 `update`, +0x28 `shutdown`. |
| `PyEnvironment` | PyEnvironment.obj (ksPython.lib) | - | Module name + `std::vector<PyMethodDef>`; `addFunc(name, fn)`, `addTerminator()`. |
| `PyInterpreter` | PyInterpreter.obj (ksPython.lib) | 0x28 | Registers the module and starts / stops CPython; `correctlyInitialized`. |
| `PyModule` | PyModule.obj (ksPython.lib) | 0x30 | One imported script (`PyImport_Import`), with its name and error flag. |
| `ACOperatingSystem` | ACOperatingSystem.obj (acSDK.lib) | 0xd0 | `GameObject` + `IACOperatingSystem`: host object handed to C++ app DLLs. |
| `AppForm`, `ACControl` | acSDK.lib | 0x1e0 / 0x180 | SDK wrappers over `ksgui::Form` / `ksgui::Control`. |
| `ACGL`, `ACFont`, `ACTexture`, `ACINIReader` | acSDK.lib | 0x20 / 0x18 / 0x38 / 0x10 | SDK wrappers over `GLRenderer`, `Font`, `Texture`, `INIReader`. |
| `PluginManager` | PluginManager.obj | 0x88 | `GameObject`: loads plugin DLLs, calls them each frame. |
| `ACPlugin` | ACPlugin.obj | 0x78 | One plugin DLL: `hModule`, the six bound entry points, and the `IACPPluginHost` interface it exposes back to the DLL. |
| `ACCarState`, `ACCarInfo`, `ACCarData`, `ACTrackData`, `ACRaceData` | types only | 0x148 / 0x80 / 0xc / 0x400 / 0x4 | Plain structs passed across the DLL boundary. |

## 3. Entry points

### 3.1 Start-up

- `wWinMain` 0x1400c29c0: `SetDllDirectoryW("system/x64")`, then the delay-load of `python33.dll` is forced; failure -> message box and `exit(2)`. `python33.zip` (the standard library) is never named in `acs.exe`: CPython finds it next to its DLL, and the file is present as `system/x64/Python33.zip` (interpretation of the mechanism; presence confirmed).
- `Sim::Sim` 0x140192070: if `cfg/gameplay.ini` (Documents) has `[PYTHON] ENABLE_PYTHON` > 0 -> `PythonInterface::PythonInterface` 0x140124920, added as a child of `Sim`. Later in the same constructor: `PluginManager::PluginManager` 0x14018dbe0 and `ACOperatingSystem::ACOperatingSystem` 0x1401eb310.
- `PythonInterface::PythonInterface`, in order (confirmed):
  1. opens `Documents/Assetto Corsa/logs/py_log.txt`, sets the global `pyi`;
  2. `PyEnvironment::PyEnvironment` 0x1402cef20 with the name `ac`, 116 calls to `PyEnvironment::addFunc` 0x1402cf0a0, `PyEnvironment::addTerminator` 0x1402cf160;
  3. `clearPythonCache` 0x14012f710 on `apps/python/system/` (deletes `__pycache__`);
  4. `PyInterpreter::PyInterpreter` 0x1402cf260: `PyImport_AppendInittab("ac", PyInit_emb)` then `Py_Initialize()`; `PyInit_emb` 0x1402cf4a0 is `PyModule_Create2(&EmbModule, 0x3f5)` (module API 1013 = Python 3.3);
  5. `PyRun_SimpleString`: `import sys`, `sys.dont_write_bytecode = 1`, `sys.path.append('apps/python/system')`;
  6. reads the sections of `Documents/Assetto Corsa/cfg/python.ini` and the folder list `apps/python/*.*`; for each section whose lower-cased name matches a folder: requires `apps/python/<name>/<name>.py`, skips it when `[<NAME>] ACTIVE` is 0, clears that folder's `__pycache__`, runs `sys.path.append('apps/python/<name>')`, creates `PyModule::PyModule` 0x1402cf4c0 (`PyImport_Import`) and, if the import worked, `PyPlugin::PyPlugin` 0x140123f50, added with `GameObject::addGameObject` (parent not traced: `PythonInterface` or `Sim`; either way it is in the main object tree). A missing file is reported to the log and the console unless the folder is `system`.
- `PyPlugin::PyPlugin`: `acMain = getattr(module, "acMain")`, called once as `acMain(1.0)` (format `"d"`; the argument is the API version, the return value is read as the app's name); then `acUpdate` and `acShutdown` are looked up and kept if present.

### 3.2 Callbacks an app can define, and who calls them

| Python callable | Called from | When / thread | Arguments (format string) |
|---|---|---|---|
| `acMain(ac_version)` | `PyPlugin::PyPlugin` 0x140123f50 | once, while the session loads (main thread) | `"d"` = 1.0 |
| `acUpdate(deltaT)` | `PyPlugin::update` 0x140124670, vtable slot +0x08 of a `GameObject` | every rendered frame, in the update pass of `Game::onIdle` (main thread; not the 333 Hz physics thread) | `"d"` frame time in seconds |
| `acShutdown()` | `PyPlugin::shutdown` 0x140124630, vtable slot +0x28 | when the object tree is shut down | none |
| render callback (usually named `onFormRender`) | `PythonForm::render` 0x140130290, after the form has drawn itself | every frame the form is visible, inside `ksgui::GUI::render`; it sets the global `formCurrentOffset` so `ac.gl*` coordinates are form-relative | `"f"` deltaT |
| click listener | lambda registered by `PythonInterface::addPythonOnClickedListener` 0x14012f4c0 | on the control's `OnControlClicked` event | `"ii"` local x, y (a second variant passes one stored `"i"`) |
| spinner value listener | `PythonInterface::addPythonOnValueChangedListener` 0x14012f650 | `OnSpinnerValueChanged` | `"i"` value |
| check box listener | `PythonInterface::addPythonOnCheckBoxChanged` 0x14012f400 | `OnCheckBoxChanged` | `"Si"` name, value |
| text input validate listener | `PythonInterface::addPythonOnValidatedListener` 0x14012f550 | Enter in a `TextInput`; the box is cleared afterwards | `"S"` text |
| list box selection / deselection | `PythonInterface::addPythonListBoxElementListener` 0x14012f070 / `addPythonListBoxDeselectionElementListener` 0x14012efc0 | list box item events | not read |
| app activated / dismissed | `PythonInterface::addPythonOnAppActivatedListener` 0x14012f120 / `addPythonOnAppDismissedListener` 0x14012f240 | hooked to the form's taskbar icon / visibility events | not read |
| chat listener | `PythonInterface::addPythonOnChatMessageListener` 0x14012f360 | `OnChatMessageEvent` from the network client | `"SS"` message, author (`SERVER` for id 255) |

After every call into Python the game runs `PythonInterface::printErr` 0x14012fcb0: a pending exception is fetched and written as `[PY ERROR]: ...` to `py_log.txt` and the in-game console; the app keeps running. There is no use of the GIL / thread-state API in `acs.exe` (the C-API calls used are `PyArg_ParseTuple`, `PyObject_CallFunction`, `PyLong_FromLong`, `PyFloat_FromDouble`, `PyTuple_*`, `PyUnicode_*`, `PyImport_*`, `PyRun_SimpleString`, `PyErr_*`), so all callbacks are serialised on the main thread. Shutdown: `PythonInterface::~PythonInterface` 0x140126490 -> `PyInterpreter::~PyInterpreter` 0x1402cf320 -> `Py_Finalize`.

### 3.3 The `ac` module: 116 names, 111 handlers

`PythonInterface.obj` has 151 hand-written functions; 111 of them are `ac_*` handlers with the CPython signature `PyObject* f(PyObject* self, PyObject* args)`. Names are the string literals passed to `addFunc`; addresses were read from the `lea r8` before each call. Handles: `newApp` and every `add*` return a 1-based index into `pyi->controls`; a bad index prints `[PYI ERROR]: Control index %d out of range`. Most functions return -1 on a parse error.

| Python name | Handler | Python name | Handler | Python name | Handler |
|---|---|---|---|---|---|
| **App window (18)** | | | | | |
| `newApp` | 0x14012cb00 | `setTitle` | 0x14012d220 | `setSize` | 0x14012e770 |
| `getSize` | 0x14012bbb0 | `setPosition` | 0x14012e4a0 | `getPosition` | 0x14012b830 |
| `setIconPosition` | 0x14012e3d0 | `setTitlePosition` | 0x14012ea50 | `setVisible` | 0x14012ed40 |
| `setBackgroundColor` | 0x14012d330 | `setBackgroundOpacity` | 0x14012d440 | `drawBackground` | 0x140128e70 |
| `drawBorder` | 0x140128f20 | `setBackgroundTexture` | 0x14012d4f0 | `addRenderCallback` | 0x140128450 |
| `addOnAppActivatedListener` | 0x140127ef0 | `addOnAppDismissedListener` | 0x140127f50 | `setFocus` | 0x14012dda0 |
| **Controls (31)** | | | | | |
| `addLabel` | 0x140127c40 | `addButton` | 0x140127600 | `addCheckBox` | 0x1401277c0 |
| `addTextBox` | 0x140128890 | `addTextInput` | 0x140128a50 | `addSpinner` | 0x1401286d0 |
| `addProgressBar` | 0x140128240 | `addListBox` | 0x140127e00 | `addGraph` | 0x1401279b0 |
| `addSerieToGraph` | 0x1401285a0 | `addValueToGraph` | 0x140128c40 | `addItem` | 0x140127ba0 |
| `removeItem` | 0x14012d180 | `getItemCount` | 0x14012b920 | `setItemNumberPerPage` | 0x14012d180 |
| `highlightListBoxItem` | 0x14012d180 | `setAllowDeselection` | 0x14012d0b0 | `setAllowMultiSelection` | 0x14012d180 |
| `getSelectedItems` | 0x14012b920 | `setText` | 0x14012e8e0 | `getText` | 0x14012bcb0 |
| `setValue` | 0x14012eb20 | `getValue` | 0x14012bf60 | `setRange` | 0x14012e550 |
| `setStep` | 0x14012e820 | `addOnClickedListener` | 0x140128060 | `addOnValidateListener` | 0x140128180 |
| `addOnValueChangeListener` | 0x1401281e0 | `addOnCheckBoxChanged` | 0x140128000 | `addOnListBoxSelectionListener` | 0x140128120 |
| `addOnListBoxDeselectionListener` | 0x1401280c0 |  |  |  |  |
| **Fonts and drawing (14)** | | | | | |
| `initFont` | 0x14012c4d0 | `setFont` | 0x14012de60 | `setCustomFont` | 0x14012d900 |
| `setFontColor` | 0x14012e0f0 | `setFontSize` | 0x14012e210 | `setFontAlignment` | 0x14012de70 |
| `glBegin` | 0x14012c0f0 | `glEnd` | 0x14012c250 | `glVertex2f` | 0x14012c460 |
| `glColor3f` | 0x14012c150 | `glColor4f` | 0x14012c1c0 | `glQuad` | 0x14012c280 |
| `glQuadTextured` | 0x14012c330 | `newTexture` | 0x14012cdf0 |  |  |
| **Car state (18)** | | | | | |
| `getCarState` | 0x140129af0 | `getCarMinHeight` | 0x1401296d0 | `getCarName` | 0x140129760 |
| `getCarSkin` | 0x1401299f0 | `getCarBallast` | 0x140129460 | `getCarRestrictor` | 0x140129910 |
| `getCarTyreCompound` | 0x14012ac80 | `getCarEngineBrakeCount` | 0x140129540 | `getCarPowerControllerCount` | 0x1401297d0 |
| `getCarFFB` | 0x140129610 | `setCarFFB` | 0x14012d890 | `isCarInPitline` | 0x14012c8b0 |
| `isCarInPitlane` | 0x14012c8b0 | `isCarInPit` | 0x14012c7d0 | `isConnected` | 0x14012c990 |
| `isAIControlled` | 0x14012c640 | `getDriverName` | 0x14012b200 | `getDriverNationCode` | 0x14012b340 |
| **Session, timing, track, server (17)** | | | | | |
| `getCarsCount` | 0x14012ae50 | `getCarLeaderboardPosition` | 0x140129660 | `getCarRealTimeLeaderboardPosition` | 0x1401298a0 |
| `getLastSplits` | 0x14012b4b0 | `getCurrentSplits` | 0x14012ae80 | `getTrackName` | 0x14012bee0 |
| `getTrackConfiguration` | 0x14012bd50 | `getTrackLength` | 0x14012be70 | `getWindSpeed` | 0x14012c0c0 |
| `getWindDirection` | 0x14012c090 | `isAcLive` | 0x14012c700 | `getServerName` | 0x14012bab0 |
| `getServerIP` | 0x14012b9e0 | `getServerHttpPort` | 0x14012b9b0 | `getServerSlotsCount` | 0x14012bb80 |
| `sendChatMessage` | 0x14012cfb0 | `addOnChatMessageListener` | 0x140127fb0 |  |  |
| **Camera (14)** | | | | | |
| `setCameraMode` | 0x14012d7d0 | `getCameraMode` | 0x140129440 | `isCameraOnBoard` | 0x14012c760 |
| `getCameraCarCount` | 0x140129380 | `setCameraCar` | 0x14012d680 | `focusCar` | 0x140128fd0 |
| `getFocusedCar` | 0x14012b480 | `freeCameraSetClearColor` | 0x1401292e0 | `freeCameraMoveForward` | 0x140129040 |
| `freeCameraMoveRight` | 0x1401290b0 | `freeCameraMoveUpWorld` | 0x140129120 | `freeCameraRotatePitch` | 0x140129200 |
| `freeCameraRotateHeading` | 0x140129190 | `freeCameraRotateRoll` | 0x140129270 |  |  |
| **Misc (4)** | | | | | |
| `log` | 0x14012ca70 | `console` | 0x140128d40 | `shutdown` | 0x14012ee00 |
| `restart` | 0x14012cec0 |  |  |  |  |

Notes (confirmed): `isCarInPitline` and `isCarInPitlane` are the same handler. `removeItem`, `setItemNumberPerPage`, `highlightListBoxItem` and `setAllowMultiSelection` share 0x14012d180, and `getItemCount` / `getSelectedItems` share 0x14012b920: both bodies only look the control up and return -1, so these six names do nothing in this build. `setFont` returns 1 and does nothing. `shutdown` calls a virtual on the `Game` object (interpretation: request exit); `restart` writes a value into an ini file under Documents with `WritePrivateProfileStringW` (file and key not read). `log` -> `PythonInterface::writeToLog` 0x140130340 (`py_log.txt`); `console` -> the in-game `Console`. `newApp` -> `PythonForm::PythonForm` 0x140124850, registered with the game screen and the app taskbar (`PythonInterface::addFormToTaskBar` 0x14012ee40). `glBegin` ... `glQuadTextured` forward to `GLRenderer`; `newTexture` loads an image file by path and returns a texture handle (`PythonInterface::getTextureByIndex` 0x14012fbe0). Camera functions call `ACCameraManager` (`setMode`, `setCameraCarIndex`) and the free camera; `focusCar` -> `Sim::setFocusedCarIndex`; `getCarFFB` / `setCarFFB` -> `CarAvatar::getUserFFGain` / `modifyUserFFGain`; `isAcLive` is "not in replay" (`ReplayManager::isInReplaymode`); `sendChatMessage` -> `ACClient::sendChat`.

### 3.4 `ac.getCarState(carId, id [, index])` and the `acsys.CS` constants

The names live in `apps/python/system/acsys.py` (`class CS: ... = range(73)`), not in the executable; `ac_getCarState` 0x140129af0 is a switch over the same numbers (argument format `"ll|l"`). `ps` = `CarAvatar::physicsState` (the render-side copy of the physics state), `pi` = `CarAvatar::physicsInfo`. "x4" = tuple of four wheel values, "vec3" = tuple of three.

| Id | CS name | Returned from | Id | CS name | Returned from |
|---|---|---|---|---|---|
| 0 | SpeedMS | `ps.speed` | 37 | Steer | `ps.steer` |
| 1 | SpeedMPH | `ps.speed` * 2.2369363 | 38 | SuspensionTravel | `ps.suspensionTravel` x4 |
| 2 | SpeedKMH | `ps.speed` * 3.6 | 39 | TurboBoost | `ps.turboBoost` |
| 3 | Gas | `ps.gas` | 40 | TyreDirtyLevel | `ps.tyreDirtyLevel` x4 |
| 4 | Brake | `ps.brake` | 41 | TyreContactNormal | `ps.tyreContactNormal[index]` vec3 |
| 5 | Clutch | `ps.clutch` | 42 | TyreContactPoint | `ps.tyreContactPoint[index]` vec3 |
| 6 | Gear | `ps.gear` | 43 | TyreHeadingVector | no case: returns -1 |
| 7 | Aero | `ps.aero.CD` / `CL_Front` / `CL_Rear` by index | 44 | TyreLoadedRadius | `ps.tyreLoadedRadius` x4 |
| 8 | BestLap | `RaceManager::getBestLap` (ms) | 45 | TyreRadius | `ps.tyreRadius` x4 |
| 9 | CamberRad | `ps.camberRAD` x4 | 46 | TyreRightVector | no case: returns -1 |
| 10 | AccG | `ps.accG` vec3 | 47 | TyreSlip | `ps.tyreSlip` x4 |
| 11 | CGHeight | `ps.cgHeight` | 48 | TyreSurfaceDef | no case: returns -1 |
| 12, 13 | DriftBestLap, DriftLastLap | constant 0.0 | 49 | TyreVelocity | no case: returns -1 |
| 14 | DriftPoints | `ps.driftPoints` | 50 | Velocity | `ps.velocity` vec3 |
| 15 | DriveTrainSpeed | `ps.drivetrainSpeed` | 51 | WheelAngularSpeed | `ps.wheelAngularSpeed` x4 |
| 16 | DY | `ps.Dy` x4 | 52 | WorldPosition | translation of `ps.worldMatrix` vec3 |
| 17 | RPM | `ps.engineRPM` | 53 | Caster | `pi.caster` |
| 18 | Load | `ps.load` x4 | 54 | CurrentTyresCoreTemp | `ps.tyreThermalStates[i].coreTemp` x4 |
| 19 | InstantDrift | `ps.instantDrift` | 55 | LastTyresTemp | `ps.tyreThermalStates[index].lastSetIMO` x3 |
| 20 | IsDriftInvalid | `!ps.isDriftValid` | 56 | DynamicPressure | `ps.tyreThermalStates[i].dynamicPressure` x4 |
| 21 | IsEngineLimiterOn | `ps.isEngineLimiterOn` | 57 | RideHeight | `ps.rideHeight[0..1]` |
| 22 | LapCount | `RaceManager::getLapCount` | 58 | ToeInDeg | from `ps.suspensionMatrix[index]` and the body transform, in degrees |
| 23 | LapInvalidated | constant 0 | 59 | CamberDeg | `ps.camberRAD` * 57.29578 x4 |
| 24 | LapTime | `RaceManager::getCurrentLap` (ms) | 60 | KersCharge | `ps.kersCharge` |
| 25 | LastFF | abs(`ps.lastFF_Pure`) of the focused car * a controls-provider value * a per-car gain | 61 | KersInput | `ps.kersInput` |
| 26 | LastLap | `RaceManager::getLastLap` (ms) | 62 | DrsAvailable | bit 1 of `ps.statusBytes` |
| 27 | LocalAngularVelocity | `ps.localAngularVelocity` vec3 | 63 | DrsEnabled | bit 2 of `ps.statusBytes` |
| 28 | LocalVelocity | `ps.localVelocity` vec3 | 64 | EngineBrake | `CarAvatar::currentEngineBrakeSetting` |
| 29 | Mz | `ps.Mz` x4 | 65 | ERSRecovery | `CarAvatar::currentERSRecovery` (0 without ERS) |
| 30 | NdSlip | `ps.ndSlip` x4 | 66 | ERSDelivery | `CarAvatar::getERSPower().first` (0 without ERS) |
| 31 | NormalizedSplinePosition | `ps.normalizedSplinePosition` | 67 | ERSHeatCharging | `CarAvatar::isHeatChargingBatteries` (0 without ERS) |
| 32 | PerformanceMeter | `ps.performanceMeter` | 68 | ERSCurrentKJ | `ps.kersCurrentKJ` |
| 33 | SlipAngle | `ps.slipAngle` x4 | 69 | ERSMaxJ | `pi.ersMaxJ`, or `pi.kersMaxJ` for KERS cars |
| 34 | SlipAngleContactPatch | four zeros | 70 | RaceFinished | `RaceManager::getHasCompletedFlag` |
| 35 | SlipRatio | `ps.slipRatio` x4 | 71 | P2PStatus | `ps.p2pStatus` |
| 36 | SpeedTotal | (km/h, mph, m/s) tuple | 72 | P2PActivations | `ps.p2pActivations` |

Rows 25 and 58 are read less closely than the rest. Rows 64 to 67 were checked in the disassembly (offsets +0x127c, +0x1288, +0x1280 of `CarAvatar`, and `physicsInfo.hasERS` at +0xf61) because the pseudo-C prints the wrong member names for them: see section 7. Other `acsys` classes: `GL` (Lines, LineStrip, Triangles, Quads = 0..3 for `glBegin`), `WHEELS` (FL, FR, RL, RR), `AERO` (CD, CL_Front, CL_Rear), `CM` camera modes (Cockpit, Car, Drivable, Track, Helicopter, OnBoardFree, Free, Random, ImageGeneratorCamera, Start = 0..9).

### 3.5 C++ app SDK (`acSDK.lib`) and plugin DLLs

- `ACOperatingSystem::ACOperatingSystem` -> `ACOperatingSystem::initApps` 0x1401ebbc0: for every `apps/*.dll`, `LoadLibraryW`, `GetProcAddress("acAppInit")` and call it with the host object (argument list not read); the destructor 0x1401eb420 calls each DLL's `acAppShutdown`. Host interface `IACOperatingSystem` (vtable order): `createForm` 0x1401eb6f0 (icon from `apps/<name>/icon.png`), `createTexture` 0x1401eb860, `releaseTexture` 0x1401ebcc0, `getCarInfo` 0x1401eb990, `getCarData` 0x1401eb950, `getTrackData` 0x1401ebb20, `getRaceData` 0x1401ebb00, `getINIReader` 0x1401ebaa0. `AppForm::render` 0x1401ec430 calls the DLL's listener each frame with `ACGL` / `ACFont` helpers; `AppForm::addControl` 0x1401ec2e0 creates `ACControl`s. No such DLL ships with the game (the `apps` folder holds only script folders).
- `PluginManager::loadPlugins` 0x14018de70: reads `plugins/plugins.ini`, enumerates `plugins/*.dll`, and loads a DLL only when `[PLUGINS] <file name>` is non-zero ("Ignoring plugin" otherwise). `ACPlugin::ACPlugin` 0x1400bf6a0 binds the exports `acpGetName`, `acpInit(IACPPluginHost*)`, `acpShutdown`, `acpUpdate`, `acpOnGui`, `acpGetControls`; when `acpGetControls` exists the plugin overrides the player's controls provider ("Ovverriding controls provider"). Per frame: `PluginManager::update` 0x14018e380 fills an `ACCarState` from the player car's `physicsState` and calls `ACPlugin::update` 0x1400bfe00 (-> `acpUpdate`); `PluginManager::renderHUD` 0x14018e260 -> `ACPlugin::onGui` 0x1400bfb20 (-> `acpOnGui`); `PluginManager::shutdown` 0x14018e2d0. The host interface offered to the DLL: `getHwnd`, `getHInstance`, `setABS`, `setTC`, `setStabilityControl`, `setIdealLine`, `setAutoShift`, `setBrakeBias`, `setSystemMessage`. The shipped `plugins.ini` lists one motion-platform DLL, disabled.

## 4. What it reads from disk

| Path (as built in code) | Use |
|---|---|
| `system/x64/python33.dll` (delay-load after `SetDllDirectoryW`), `system/x64/Python33.zip` | Interpreter and standard library. |
| `Documents/Assetto Corsa/cfg/gameplay.ini` `[PYTHON] ENABLE_PYTHON` | Master switch. |
| `Documents/Assetto Corsa/cfg/python.ini` `[<APPNAME>] ACTIVE` | One section per enabled app; the section name is matched case-insensitively against the folder name. |
| `apps/python/*.*` (folders), `apps/python/<name>/<name>.py` | The apps. `apps/python/system/` is on `sys.path` and holds `acsys.py`; each app folder is appended to `sys.path` too. `__pycache__` folders are deleted at start and `sys.dont_write_bytecode` is set. |
| `Documents/Assetto Corsa/logs/py_log.txt` | Written by `ac.log` and by the error printer. |
| image paths given by scripts (`ac.newTexture`, `ac.setBackgroundTexture`), font family names (`ac.initFont`, `ac.setCustomFont`) | Loaded through the normal texture / font code. |
| `apps/*.dll`, `apps/<name>/icon.png` | C++ SDK apps. |
| `plugins/plugins.ini` `[PLUGINS] <dll>=0/1`, `plugins/*.dll` | C++ plugins. |

## 5. Size

`python tools/re_query.py size PythonInterface PyPlugin PyEnvironment PyInterpreter PyModule ACControl ACFont ACGL ACINIReader ACOperatingSystem ACTexture AppForm PluginManager ACPlugin`: **14 object files, 273 hand-written functions, 66,781 code bytes.**

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| PythonInterface | 151 | 49,518 | | ACOperatingSystem | 19 | 3,464 |
| PyPlugin | 14 | 2,502 | | AppForm | 9 | 1,776 |
| PyEnvironment | 3 | 848 | | ACINIReader | 7 | 455 |
| PyInterpreter | 3 | 804 | | ACTexture | 6 | 338 |
| PyModule | 1 | 252 | | ACGL | 10 | 318 |
| PluginManager | 13 | 3,609 | | ACControl | 7 | 302 |
| ACPlugin | 24 | 2,424 | | ACFont | 6 | 171 |

Not counted here: `SharedMemoryWriter`, `SharedFileOut`, `RemoteTelemetryUDP` (docs/map/telemetry.md), `ImageGeneratorDLLManager` (renderer.md), the `ksgui` widgets that the API wraps (ui.md). `python33.dll` itself is a stock third-party DLL.

## 6. Port difficulty

**M** for the Python route, **S** for the two C++ routes (or drop them).
- The glue is thin: 111 small handlers, most of them a single getter on `CarAvatar::physicsState`, `RaceManager`, `ACCameraManager` or a `ksgui` control. The work is in what they depend on: a GUI with integer control handles, per-form render callbacks with an immediate-mode quad / line API, textures and fonts by handle, the session / timing model, the camera manager, the chat client.
- Embedding: use `pyo3` and register a native module named `ac` before interpreter start, the same pattern as `PyImport_AppendInittab`. Existing community apps are written for Python 3.3 and expect the bundled standard library plus `acsys.py` and, very often, `ctypes` access to the shared-memory pages (`sim_info.py`); running them on a modern CPython mostly works but is not guaranteed, so shipping the same 3.3 runtime is the compatible choice. `RustPython` is an option for a pure-Rust build but would break apps that use C extension modules.
- Compatibility target: keep the 116 names, the argument formats (`PyArg_ParseTuple` strings), the -1 error convention, the 73 `CS` ids and their units, the `acMain` / `acUpdate` / `acShutdown` contract and per-frame (not per-physics-step) timing.
- If the UI is rebuilt on `egui` (see ui.md), the `ac.add*` / `ac.gl*` calls need an adapter layer that records controls and draw commands per app window; that adapter is the bulk of the effort.
- C++ SDK apps (`acAppInit`) and plugins (`acp*`) pass C++ objects with MSVC vtables across the DLL boundary; supporting them from Rust means hand-written vtable shims. No stock content uses them, so the reasonable choice is to skip both and keep shared memory as the supported external interface.

## 7. Open questions

- The parent object of each `PyPlugin` (the `GameObject::addGameObject` receiver in the constructor) was not traced; it only matters for update order relative to the other apps.
- `ac.restart` (which ini file / key it writes) and `ac.shutdown` (which `Game` virtual) were not read.
- Argument lists of the list-box and app-activated / dismissed listeners, and of `acAppInit`, were not read.
- Caution for every reader of `re/decomp`: in `ac_getCarState` the pseudo-C labels the `CarAvatar` members at +0x127c / +0x1280 / +0x1288 as `currentERSPowerIndex` / `currentERSRecovery` / `currentERSNormalizedRecharge`, while `re/types/CarAvatar.txt` (and the meaning of the Python ids) says they are `currentEngineBrakeSetting` / `isHeatChargingBatteries` / `currentERSRecovery`. Ghidra's `CarAvatar` layout is shifted by one member in that tail region (after `filteredSpeed`, +0x1278); `physicsState` and `physicsInfo` members were cross-checked and are correct. Trust the offsets in the disassembly over pseudo-C names for late `CarAvatar` members.
- `getCarState` rows 25 and 58: the expressions were skimmed, not decoded; ids 43, 46, 48, 49 fall to the default branch, which appears to return -1 (the decompiler shows the value as a float NaN bit pattern converted with `PyLong_FromLong`).
- Whether `python33.dll` is told where `Python33.zip` is (no `Py_SetPath` / `Py_SetPythonHome` call was seen in the C-API list) or simply finds it beside the DLL: stated as the latter, unverified.
- The `evOnSetupAppCreated` event on `PythonInterface` (fired from `ac_newApp`) suggests the setup screen reacts to Python apps; the listener was not looked up.
- Choice made: the user's install contains extra folders that the stock executable never reads (for example `apps/lua`); they come from third-party patches and are ignored here.
