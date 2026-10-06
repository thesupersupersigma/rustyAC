# Configuration: ini reader, settings files, race.ini, localisation, small utilities

Source: `re/decomp/` pseudo-C (folders `INIReader`, `INIReaderDocuments`, `OptionsManager`, `Path`,
`LocalizationManager`, `JSONWriter`, `TagDocumentReader`, `StringUtility`, `MD5CheckSum`, `FileChangeObserver`,
`ksSecurity`, `acs`), `tools/re_query.py uses "\.ini"` grouped by file name (raw list: `re/scratch/atlas_core/ini_by_file.txt`),
and a read-only look at `system/cfg/` in the game folder. Nothing was modified.

## 1. What it is

Almost every setting in the game is a Windows-style ini file read through one class, `INIReader`.
There is no central settings object: each feature opens the file it needs in its own constructor, asks for
`[SECTION] KEY` values, and throws the reader away (about 190 distinct ini names are referenced from about 390 functions).
Files live in three places: `system/cfg/*.ini` in the game folder (engine defaults shipped with the game),
`Documents\Assetto Corsa\cfg\*.ini` (user settings written by the launcher: video, controls, audio, gameplay,
assists, and the session description `race.ini`), and each car's and track's `data/` folder.
`INIReaderDocuments` is the same reader with the path made relative to the Documents folder.
For car data the reader is transparent about packing: if a `data.acd` archive sits next to the `data/` folder it
decrypts the file from the archive instead (see content_loading.md).
`INIReader` only reads. Settings are written back with the Win32 call `WritePrivateProfileStringW` or with plain
file streams, directly from the feature that owns the value (app positions, camera settings, FF gain, volumes).
`race.ini` is the contract between launcher and game: it names the track, the cars, the sessions, weather and the
online/replay/benchmark switches; `acs.exe` reads it in many places and writes only the `[RESTART]` flag.
Localisation is a flat key-to-text table loaded from `system/locales/<lang>.ini`; `acTranslate` is called from
about 170 places. The rest of this area is small helpers: paths, strings/UTF-8, MD5, JSON output, a file-change
watcher for developer hot-reload, and the `ksSecurity` key maker used for the `.acd` key.

## 2. Main classes

| Class / unit | Source file (library) | Size | Role |
|---|---|---|---|
| `INIReader` | INIReader.obj (ksCommon.lib) | 0x68 | Parses an ini into `map<section, INISection>`; typed getters; static cache of parsed files (`INIReader::cache`, `useCache`), `crashAtError`, `openedFiles`. |
| `INISection` | same | - | One section's key/value map. |
| `INIReaderDocuments` | INIReaderDocuments.obj (ksCommon.lib) | 0x68 | `INIReader` whose path is under `Documents/<PROGRAM_NAME>/`; static `basePath`, `programName`. |
| `OptionsManager` | OptionsManager.obj | 0x18 | `map<name, float>` of four developer options owned by `Sim`. |
| `Path` | Path.obj (ksCommon.lib) | static | File helpers: exists (also inside `.acd`), list files/folders, read all text, Documents path, name/extension split. |
| localisation (`acLocale*`, `acTranslate`) | LocalizationManager.obj (ksGame.lib) | static | Current locale, string table, setup help and tutorial text. |
| `TagDocumentReader` | TagDocumentReader.obj (ksCommon.lib) | 0x10 | Reads `.tag` text documents split into named blocks (setup help, tutorial, in-game help). |
| `jsonwriter::*` | JSONWriter.obj (ksCommon.lib) | - | Small JSON DOM + writer (Object / Array / Value / FileWriter). Output only. |
| string helpers | StringUtility.obj (ksCommon.lib) | static | `ksSplitString`, `ksTrim`, `ksDecodeUtf8` / `ksEncodeUtf8`, `ksReplaceStringW`, JSON escaping. |
| `MD5CheckSum`, `md5`, `md5Folder` | MD5CheckSum.obj (ksCommon.lib) | 0x6c | MD5 of a buffer / file / folder (multiplayer content check). |
| `FileChangeObserver` | FileChangeObserver.obj (ksCommon.lib) | 0x30 | Remembers a file's modification time; `hasChanged` drives live reload of a few car ini files. |
| `ksSecurity` | ksSecurity.obj (ksSecurity.lib) | 0x20 | MAC address, "request code", `keyFromString` (the `.acd` key), `validate`. |

## 3. Entry points

### INIReader (all confirmed from pseudo-C / callee lists)

| Function | Address | Notes |
|---|---|---|
| `INIReader::INIReader(filename)` | 0x1402340a0 | If `useCache` and the name is in `INIReader::cache`, copies the cached sections. Otherwise: if `<folder of file>.acd` exists (`Path::fileExists`) -> `loadEncrypt`, else `load`. Stores the result in the cache when caching is on. |
| `INIReader::load` | 0x140237140 | Reads the plain file, then `parse`. |
| `INIReader::loadEncrypt` | 0x140237570 | `FolderEncrypter::decryptFile` 0x14023bdd0 -> `ksDecodeUtf8` -> `parse`; `ready = (text != "NOT FOUND")`. |
| `INIReader::parse` | 0x140237680 | `[section]`, `key=value`, `;` comments. A repeated key keeps the first value and reports "MULTIPLE KEY FOUND ... STILL USING OLD VALUE". |
| `getInt` / `getFloat` / `getString` / `getHex` | 0x140235c70 / 0x1402358c0 / 0x1402360f0 / 0x140235a70 | Typed getters by (section, key). |
| `getFloat2/3/4`, `getVector2/3/4` | 0x140235760, 0x1402357a0, 0x140235860, 0x140236300, 0x140236540, 0x140236ad0 | Comma-separated tuples. |
| `INIReader::getCurve` | 0x140235040 | Either an inline `x=y|x=y|...` list or the name of a `.lut` file next to the ini ("Loading curve from:"). |
| `hasSection` / `hasKey` / `getSections` / `getKeyes` | 0x1402370b0 / 0x140236f00 / 0x140235fc0 / 0x140235e10 | Enumeration; `"CAR_" + n` style loops use `hasSection`. |
| `INIReader::errorReport` | 0x140234e70 | Prints "INIReader Error"; with `crashAtError` (`WARNINGS_AS_ERRORS`) it is fatal (interpretation of the flag name). |
| `INIReader::clearCache` | 0x140234910 | Called by `Sim::Sim` and by the hot-reload users below. |
| `INIReaderDocuments::INIReaderDocuments(name, bool)` | 0x140238350 | First use reads `system/cfg/inireaderdocuments.ini [FOLDER] PROGRAM_NAME / BYPASS_DOCUMENT_FOLDER / CUSTOM_PATH` and sets `basePath = <Documents>/<PROGRAM_NAME>/` (or empty / custom). Then loads `basePath + name`. |

Other units: `OptionsManager::OptionsManager` 0x14018d470 registers `PROVIEW_MODE`, `AUTOFLIP_RECOVERY`, `RENDER_SPLINE`,
`DISABLE_SHOW_MODE`; `OptionsManager::loadOptions` 0x14018da20 reads them from `system/cfg/options.ini [OPTIONS]`.
`Path::fileExists` 0x1402305e0 (falls through to `FolderEncrypter::fileExists` 0x14023c390 for packed car data),
`Path::getDocumentPath` 0x140230890, `Path::getFiles` 0x140230b00, `Path::getFolders` 0x140231260, `Path::readAllText` 0x140231c90.
Localisation: `initLocalization` 0x1400c15c0 (from `wWinMain`) -> `acLocaleSetLocale` 0x140240980 -> `loadMainINI` 0x140240ee0 /
`loadSetupINI` 0x1402414e0; lookup with `acTranslate` 0x140240cf0, `acLocaleGetString` 0x1402404a0, `acLocaleGetSetupHelpText` 0x140240410.
`TagDocumentReader::load` 0x14044e030 / `getText` 0x14044df90. `md5Folder` 0x14023a450 (caller: `ACClient::getCarMD5` 0x140046070).
`FileChangeObserver::observe` 0x14023b470 / `hasChanged` 0x14023b440, used by `DigitalInstruments`, `DigitalPanels`, `CarBrakeLights`,
`Flames`, `CarColliderManager` to re-read their ini while the game runs. `ksSecurity::keyFromString` 0x1402cfe00,
`ksSecurity::getRequestCode` 0x1402cfd40 (only for the `-request` switch), `ksSecurity::validate` 0x1402d01f0 (called by the three
car-control classes; purpose not read). JSON: `jsonwriter::FileWriter::WriteFile` is called by `JsonOutputFile::writeOutputFile`
0x14010ecc0 (race result file) and `WarningReporter::shutdown` 0x1401d70c0.

Hook into the main loop: none. Everything here is called on demand, mostly during `Sim::Sim` loading on the main thread;
the physics thread reads ini only inside car/track construction.

## 4. What it reads from disk

Path prefixes as they appear in code: `system/cfg/...` is relative to the game folder (plain `INIReader`);
`cfg/...` is given to `INIReaderDocuments` (so `Documents/Assetto Corsa/cfg/...`); strings starting with
`/Assetto Corsa/...` are appended to `Path::getDocumentPath()` and are almost always **writes**.

### 4.1 Settings file -> reader -> what it controls

| File | Reader function (address) | Controls |
|---|---|---|
| `system/cfg/assetto_corsa.ini` | `wWinMain` 0x1400c29c0; `DebugLog::DebugLog` 0x140217ef0; `PhysicsDriveThread::PhysicsDriveThread` 0x140122a60; `PhysicsEngine::PhysicsEngine` 0x140262430; `PhysicsEngine::initLowSpeedFF` 0x140263c90; `Sim::Sim` 0x140192070; `RaceManager::RaceManager` 0x140132db0; `ACClient::ACClient` 0x14003a2c0; `DICarControl::DICarControl` 0x14007ddd0; `JoypadCarControl::JoypadCarControl` 0x1401d9280; `DriverModel::DriverModel` 0x1400f76d0; `MirrorTextureRenderer::MirrorTextureRenderer` 0x140113c40; `Telemetry::init` 0x1402bc010; `ksgui::Form::Form` 0x140245890; others | Engine switches: sleep time, warnings-as-errors, timer mode, log suppression, threading, FF skip steps and experimental FF, low-speed FF, dev apps, race over-time, mirrors, driver hiding, free camera, gamepad legacy code. |
| `system/cfg/inireaderdocuments.ini` | `INIReaderDocuments::INIReaderDocuments` 0x140238350 | Where the Documents folder is. |
| `system/cfg/options.ini` | `OptionsManager::loadOptions` 0x14018da20; `ACClient::ACClient` 0x14003a2c0 | Developer options (pro view, autoflip recovery, spline rendering, show mode). |
| `system/cfg/graphics.ini` | `createDeviceAndSwapChain` 0x14001a1d0; `GraphicsManager::GraphicsManager` 0x140201620; `GraphicsManager::initRenderFlags` 0x140202f60; `SkyBox::SkyBox` 0x14021c790 | D3D device options, render flags, skybox reflection gain. |
| `system/cfg/lighting.ini`, `hdr.ini`, `colorCurves.ini` | `Sim::initHDRLevels` 0x140199b00; `CameraShadowMapped::CameraShadowMapped` 0x14020bdd0; `HDR::HDR` 0x14022d430; `GraphicsManager::loadLightingSettings` 0x1402042b0 | Exposure levels, shadow cascades, colour curves. |
| `system/cfg/ppfilters/*.ini` (and `Documents/.../cfg/ppfilters/`) | `PostProcessFilterSelector::PostProcessFilterSelector` 0x140329360; `YebisPP::readPPSet` 0x14002cd60; `CameraForwardYebis::readPPSetOptions` 0x140026150 | Post-processing filter presets. |
| `system/cfg/audio_engine.ini` | `Game::Game` 0x140241880; `AudioEngine::AudioEngine` 0x1401f6bf0; `Sim::Sim` | Audio on/off and FMOD settings. |
| `system/cfg/camera_*.ini`, `chase_cam.ini`, `random_camera.ini` | `CameraDrivableManager::CameraDrivableManager` 0x1400c4a80; `CameraOnBoardFree::CameraOnBoardFree` 0x140115100; `CameraTrack::CameraTrack` 0x1400cab20; `CameraDrivableManager::initChaseCamFromINI` 0x1400c61c0; `ACCameraManager::ACCameraManager` 0x140031270 | Camera behaviour defaults. |
| `system/cfg/fades.ini` | `SplashScreen::SplashScreen` 0x14019faf0; `ESCMenu::ESCMenu` 0x1400fcb40; `PauseMenu::PauseMenu` 0x14011d9b0; `Sim::Sim` | Fade times. |
| `system/cfg/pitstop.ini`, `messages.ini`, `session_info.ini`, `name_displayer.ini`, `damage_displayer.ini`, `proximity_indicator.ini`, `mouse_hider.ini`, `map.ini`, `tyres_app.ini`, `chat_app.ini`, `endSessionDisplayer.ini`, `driver_performances.ini`, `telemetry_presets.ini` | constructor of the class with the matching name, e.g. `QuickMenuPitstop::QuickMenuPitstop` 0x1400aca70, `SystemMessage::SystemMessage` 0x1401b5610, `DriverNameDisplayer::DriverNameDisplayer` 0x14008c7e0, `ProximityIndicator::ProximityIndicator` 0x140123880, `TrackMap::initParameters` 0x1402ef150 | HUD / app layout and behaviour. |
| `system/cfg/skidmarks.ini`, `tyre_smoke.ini`, `tyre_smoke_grass.ini`, `tyre_pieces_grass.ini`, `engine_smoke.ini` | `SkidMarkBuffer::SkidMarkBuffer` 0x14018f2d0; `TyreSmoke::TyreSmoke` 0x1401d0000; `EngineSmoke::EngineSmoke` 0x140092de0 | Visual effects. |
| `system/cfg/ghost_car.ini`, `fanatec.ini`, `track_skins.ini` | `GhostCar::loadGhostCarParameters` 0x140105fd0; `FanatecUpdater::FanatecUpdater` 0x140099f70; `TrackAvatar::init3D` 0x1401c8740 | Ghost car look, wheel display, track skins. |
| `system/data/surfaces.ini` | `SurfacesManager::SurfacesManager` 0x1401ae9b0 | Default track surface types (track.md / tyre.md). |
| `cfg/video.ini` | `loadVideoSettings` 0x1400c18d0; `Sim::Sim`; `Sim::initCubemaps` 0x1401997a0; `PostProcessEffectsUpdater::PostProcessEffectsUpdater` 0x1400ab880; `MirrorTextureRenderer::MirrorTextureRenderer`; `TyreSmoke::TyreSmoke`; `BenchmarkMode::generateResultString` 0x14005b370; `StereoCameraVive::StereoCameraVive` 0x140220b80 | `[VIDEO]` size/AA/vsync/`FPS_CAP_MS`, `[REFRESH]`, `[ASSETTOCORSA] WORLD_DETAIL`, `[POST_PROCESS]`, `[EFFECTS]`, cubemap and mirror quality. |
| `cfg/controls.ini` | `DICarControl::DICarControl` 0x14007ddd0; `DICommandManager::DICommandManager` 0x140081150; `KeyboardCarControl::readFromIni` 0x14010ff60; `JoypadCarControl::JoypadCarControl`; `CommandManager::CommandManager` 0x1400e8b70; `CarAvatar::initControls` 0x1400d6f30; `PhysicsDriveThread::PhysicsDriveThread` | Input method, axes, buttons, FF gain, keyboard shortcuts. |
| `cfg/assists.ini` | `DrivingAssistManager::DrivingAssistManager` 0x1400fbd90; `IdealLine::IdealLine` 0x140109d10; `SetupElectronics::checkDrivingAssist` 0x14016cb40; `CarHintLoader::CarHintLoader` 0x140068860 | Driving aids (`[ASSISTS]`, e.g. `AUTO_BLIP`), ideal line. |
| `cfg/gameplay.ini` | `Sim::Sim`; `ksgui::GUI::GUI` 0x140251ac0; `CarAvatar::initCommon` 0x1400d56e0; `CarAvatar::initPhysics` 0x1400d7660; `PitStop::PitStop` 0x1400a55d0; `OverlayLeaderboard::OverlayLeaderboard` 0x1401bf440; `VirtualMirrorRenderer::VirtualMirrorRenderer` 0x1401d1ab0; `TimeAttack::TimeAttack` 0x1401b7e20; several displayers | Units, HUD options, mirror, pit menu, overlays. |
| `cfg/audio.ini` | `Game::Game`; `AudioEngine::AudioEngine`; `MixVolumes::MixVolumes` 0x140064a40; `SkidParams::SkidParams` 0x140065110 | Volumes, skid sound thresholds. |
| `cfg/camera_manager.ini`, `camera_onboard.ini`, `video_editing.ini`, `triple_screen.ini`, `oculus.ini`, `openvr.ini`, `trackir.ini` | `ACCameraManager::ACCameraManager`; `CameraOnBoard::CameraOnBoard` 0x1400c84e0; `CinematicFreeCamera::loadSettingsFromINI` 0x14006cf60; `CameraTripleScreen::CameraTripleScreen` 0x1402248e0; `OculusManager::OculusManager` 0x1400a01c0; `StereoCameraVive::configureOpenVR` 0x140222c20; `TrackIR::TrackIR` 0x1401cdfe0 | Camera, triple-screen and VR/head-tracking settings. |
| `cfg/acos.ini` | `ksgui::GUI::GUI`; `ksgui::GUI::fromIni` 0x140253b20; written by `ksgui::GUI::serializeForms` 0x140254d60 | In-game app window positions and visibility. |
| `cfg/replay.ini`, `cfg/client.ini`, `cfg/python.ini`, `cfg/ff_post_process.ini`, `cfg/user_ff.ini`, `cfg/warning_reporter.ini`, `cfg/time_limited_test.ini`, `cfg/telemetry_user.ini` | `ReplayManager::ReplayManager` 0x140153130; `ACClient::ACClient`; `PythonInterface::PythonInterface` 0x140124920; `FFPostProcessor::FFPostProcessor` 0x140279560; `CarAvatar::initCommonPostPhysics` 0x1400d6190; `WarningReporter::WarningReporter` 0x1401d64f0; `TimeLimitedTest::TimeLimitedTest` 0x1401becd0; `FormTelemetry::FormTelemetry` 0x14032f660 | Replay size/quality, client network options, enabled Python apps, FF curve, per-car FF gain. |
| `cfg/launcher.ini` | `initLocalization` 0x1400c15c0 | `[WINDOW] LANGUAGE` only. |
| `cfg/cars/<car>/view.ini` (Documents) | `CarAvatar::init3D` 0x1400d3b90; `CameraOnBoard::CameraOnBoard` | Per-car seat position. |
| `plugins/plugins.ini`, `redirect.ini`, `content/gui/skin.ini` | `PluginManager::loadPlugins` 0x14018de70; `checkRedirect` 0x1400c1430; `ksgui::GUI::GUI` | DLL plugins, working-dir redirect, GUI skin. |
| `system/locales/<lang>.ini`, `system/locales/setup/<lang>.tag`, `system/locales/tutorial/...`, `system/locales/help/...` | `acLocaleSetLocale` 0x140240980; `ACHelp::ACHelp` 0x140307900 | Translations (`[STRINGS]`), setup help, tutorial and help text. |

Files present in `system/cfg/` with no reference found in acs.exe by file-name search: `camera_free.ini`, `camera_start_mode.ini`,
`chat.ini`, `ig_config.ini`, `physics.ini`, `replay_steer_driver.ini`, `scene_dimmer.ini`, `sparks.ini`, `temp_blister.ini`, `vr.ini`,
`weather.ini` (other executables, leftovers, or names built from pieces; not checked further).

### 4.2 race.ini (`Documents/Assetto Corsa/cfg/race.ini`, written by the launcher)

| Section | Keys seen in code | Read by |
|---|---|---|
| `[HEADER]` | `VERSION` | `RaceManager::initOffline` 0x14013a6c0 |
| `[RACE]` | `TRACK`, `CONFIG_TRACK`, `MODEL`, `MODEL_CONFIG`, `SKIN`, `CARS`, `AI_LEVEL`, `FIXED_SETUP`, `PENALTIES`, `JUMP_START_PENALTY` | `checkRaceINI` 0x1400c09b0, `initOffline`, `RaceEvent::RaceEvent` 0x1401303c0, `Sim::createCamera` 0x1401982e0, `SteamInterface::SteamInterface` 0x1401a5d70, `WrongWayIndicator::WrongWayIndicator` 0x1401d83d0, `SplashScreen::initHints` 0x1401a0830 |
| `[CAR_n]` | `MODEL`, `MODEL_CONFIG`, `SKIN`, `DRIVER_NAME`, `NATION_CODE`, `AI_LEVEL`, `AI_AGGRESSION`, `BALLAST`, `RESTRICTOR` | `checkRaceINI`, `initOffline`; `CAR_0` also `ACClient::handshakeTCP` 0x140048b50 |
| `[SESSION_n]` | `NAME`, `TYPE`, `LAPS`, `DURATION_MINUTES`, `SPAWN_SET`, `STARTING_POSITION` | `initOffline`; `SESSION_0` also `SharedMemoryWriter::SharedMemoryWriter` 0x140185430, `DragMode::DragMode` 0x140084a20 |
| `[TEMPERATURE]`, `[WIND]`, `[WEATHER]` | `AMBIENT`, `ROAD`; `SPEED_KMH_MIN`, `SPEED_KMH_MAX`, `DIRECTION_DEG`; `NAME` | `initOffline`; `SkyBox::SkyBox` 0x14021c790 |
| `[LIGHTING]` | `SUN_ANGLE` and others | `RaceManager::initLighting` 0x14013a3d0 |
| `[DYNAMIC_TRACK]`, `[GROOVE]` | `SESSION_START`, `RANDOMNESS`, `LAP_GAIN`, `SESSION_TRANSFER` | `Track::initDynamicTrack` 0x140278300; `DynamicTrackManager::DynamicTrackManager` 0x1401ccc20 |
| `[GHOST_CAR]` | `LOAD`, `FILE`, `SECONDS_ADVANTAGE` | `GhostCarRecorder::loadGhostCarRecorderCustomParameters` 0x140108880, `RaceManager::initGhostcar` 0x14013a210 |
| `[REPLAY]` | `ACTIVE`, `FILENAME` | `RaceManager::loadSessions` 0x14013ecb0, `RaceManager::initReplay` 0x14013e820, `ReplayManager::ReplayManager` |
| `[REMOTE]` | `ACTIVE`, `SERVER_IP`, `SERVER_PORT`, `SERVER_HTTP_PORT`, `SERVER_NAME`, `NAME`, `TEAM`, `PASSWORD`, `REQUESTED_CAR` | `loadSessions`, `ACClient::handshakeTCP` |
| `[BENCHMARK]` | `ACTIVE` | `loadSessions` |
| `[SPECIAL_EVENT]` | `GUID` | `initOffline`, `GameMode::GameMode` 0x1400bc810 (then decrypts `system/data/se/SPECIAL_EVENT_<guid>.acd` with `SimpleFileEncrypter`) |
| `[RESTART]` | `ACTIVE` | written by `ac_restart` 0x14012cec0 |

`loadSessions` picks the mode in the order visible in its strings: benchmark flag, then replay, then remote (online), else offline.
`initOffline` prints the whole file to the log ("PRINTING RACE INI"). Sections present in a real launcher-written file but with no reader
found by string search: `[LAP_INVALIDATOR]`, `[OPTIONS]`, `[__PREVIEW_GENERATION]` (the last is from a third-party launcher).

## 5. Size

`python tools/re_query.py size` on the stems below: **153 hand-written functions, 74,395 code bytes**.

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| INIReader | 33 | 26,453 | | JSONWriter | 42 | 9,078 |
| Path | 16 | 8,771 | | LocalizationManager | 20 | 6,284 |
| MD5CheckSum | 8 | 6,263 | | TagDocumentReader | 5 | 5,937 |
| StringUtility | 8 | 3,688 | | ksSecurity | 7 | 3,293 |
| INIReaderDocuments | 6 | 2,156 | | OptionsManager | 4 | 2,017 |
| FileChangeObserver | 4 | 455 | | | | |

Not counted here (left to other areas): `Console*.obj` and `CommandManager.obj` (developer console and key-binding commands:
UI / input, not settings storage), `JsonOutputFile.obj` (race results, session logic), `ACINIReader.obj` (plugin SDK wrapper),
`acs.obj` helpers `loadVideoSettings` / `checkRaceINI` (counted in main_loop.md), `FolderEncrypter` / `SimpleFileEncrypter` (content_loading.md).

## 6. Port difficulty: **S**

- The reader is a simple line parser plus typed getters. The details that matter for bit-exact physics are few and worth
  copying exactly: first-value-wins on duplicate keys, how a missing key is reported and what it returns, number parsing of
  values with trailing comments/units (the car inis contain many), and `getCurve` (inline list vs `.lut` file).
- Off-the-shelf: `std::path` / `dirs` (Documents folder), `md5` crate, `serde_json` (output only), `encoding_rs` or std for
  UTF-8/UTF-16. A generic ini crate is risky because of the quirks above; a 200-line own parser is safer.
- Design choice for the rewrite: keep "each component reads its own file", but pass a virtual file system (plain folder or
  unpacked `.acd`) instead of the hidden `.acd` switch inside the reader.
- `ksSecurity::getMacAddress` / request code is licence-era code with no use in a rewrite; `keyFromString` is already
  reimplemented in `tools/acd_extract.py`.
- Depends on: nothing but the OS. Everything else depends on it.

## 7. Open questions

- Return values of the getters on a missing section/key (0, -1, empty string?) and the exact float parser (`stof` on the
  raw text, per the error strings in `getCurve`) were not read; this matters for the physics port and should be pinned with a
  small test against the real function.
- `INIReaderDocuments`'s second argument (bool): it changes whether the file is looked up under Documents only or also by the
  plain relative path; the branch was skimmed, not decoded.
- `useCache` default and who toggles it were not traced (the cache is cleared by `Sim::Sim` and by the hot-reload users).
- The third `INIReader` constructor at 0x140234310 and the default one at 0x140234010 were not read.
- `ksSecurity::validate` is called from `DICarControl::validate`, `KeyboardCarControl::validate`, `JoypadCarControl::validate`: what it validates is unknown.
- `[OPTIONS]` is read from several files by `Sim::Sim`, `TyresApp`, `FormChat`; whether any of them is `race.ini [OPTIONS]` was not checked.
- Where `[CAMERA] MODE` (single / triple / Oculus / OpenVR, used by `Sim::createCamera`) comes from: the key names are in
  `createCamera` together with `cfg/race.ini`; the launcher writes it in `video.ini`. Not resolved which reader object is passed in.
- Choice made: Console and CommandManager are treated as UI / input and not counted in this area.
