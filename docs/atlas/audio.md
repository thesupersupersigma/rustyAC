# Audio (FMOD Studio)

> Ported in Task 19: `docs/port/audio.md`. That report's section 3 lists what this survey, written from
> pseudo-C, got wrong (the version check, the `.strings.bank`, `impact_speed`, the listener distance, the
> skid's load factor, the ambience emitters, the reverb's wet level).

Source: `re/decomp/` pseudo-C (folders `AudioEngine`, `fmod_distance_filter`, `fmod_gain`, `CarAudioFMOD`,
`TrackAudio`, `FormVolumes`), `re/types/*.txt`, `re/index/calls.tsv` (FMOD imports), `tools/re_query.py`
(callers / callees / strings / vtable), `tools/ini_survey.py sounds.ini`, and the game's own
`system/cfg/audio_engine.ini`, `content/sfx/` listing and one track `audio_sources.ini` (all read-only; nothing was
modified). "Confirmed" means read in the pseudo-C; "interpretation" is marked as such.

## 1. What it is

All sound in `acs.exe` goes through FMOD Studio, loaded as two DLLs (`fmodstudio64.dll`, `fmod64.dll`); the game
checks at start-up that the DLL reports version `0x00010812` (FMOD 1.08.12) and refuses to run otherwise.
`AudioEngine` (library `ksAudioFMOD.lib`) is a thin wrapper: it creates the Studio system, registers two small
custom DSP plug-ins, reads text files that map event paths to GUIDs, loads sound banks with reference counting,
and each frame pushes volumes, occlusion and `Studio::System::update`.
Sounds are never played as raw files by the game: a sound is an FMOD *event* inside a `.bank`, addressed by a path
such as `event:/cars/<car>/engine_ext`, wrapped in an `AudioEvent` (description + instance + base volume + base
pitch). The game only starts/stops events, places them in 3D and sets named parameters; all the mixing, pitch
curves, cross-fades between rpm samples and so on live inside the bank and were authored in the FMOD Studio tool.
Each car owns one `CarAudioFMOD` game object. Once per rendered frame it reads the render-side copy of the car's
physics state (`CarAvatar::physicsState`, a `CarPhysicsState`) and writes about fifteen parameters (rpm, throttle,
speed, boost, slip-derived volumes ...) into its events. It runs on the main thread, not the physics thread.
The track owns one `TrackAudio`: ambience emitters on named scene nodes, reverb zones and occlusion meshes from the
track's `audio_sources.ini`.
The 3D listener is set by whichever camera is active. Volumes come from `Documents\Assetto Corsa\cfg\audio.ini`
and can be changed in game with the `FormVolumes` app.

## 2. Main classes

| Class / unit | Source file (library) | Size | Role |
|---|---|---|---|
| `AudioEngine` | AudioEngine.obj (ksAudioFMOD.lib) | 0x78 | Owns `FMOD::Studio::System* system`, `FMOD::System* lowLevelSystem`, `masterVolume` / `targetVolume` (ramped), `registeredGuids` (path -> GUID map), `loadedBanks` (ref-counted list), `registeredEvents`, `cachedEvents` / `freeEvents` (event pool for surface sounds), `reverbValue`, `playing`. |
| `AudioEvent` | AudioEngine.obj | 0x48 | One playable sound: `engine`, `description`, `instance`, `baseVolume`, `basePitch`, `path`, `reverbResponse`. Methods: start, stop, resume, isPlaying, isPaused, isWithinRange, setParameter(name, value), set3DAttributes, setBaseVolume, setBasePitch. |
| `AudioReverb` | AudioEngine.obj | - | Wrapper of `FMOD::Reverb3D` (a sphere with min/max distance and a reverb preset or custom properties). |
| `AudioOccluder` | AudioEngine.obj | - | Wrapper of `FMOD::Geometry` (triangles that muffle sounds behind them). |
| `AudioBankRef` | CarAudioFMOD.obj | 0x28 | RAII holder: `addBankRef` in the constructor, `removeBankRef` in the destructor. |
| `FMODDistanceFilterState`, `FMODGainState` | fmod_distance_filter.obj, fmod_gain.obj (ksAudioFMOD.lib) | - | Two custom FMOD DSP plug-ins ("distance filter": a band-pass that narrows with distance, parameters `Max Dist`, `Frequency`, `3D Attributes`; "gain": `Gain` in dB, `Invert`). Interpretation: these are the example plug-ins shipped with the FMOD SDK, compiled into the exe so that the banks can use them. |
| `CarAudioFMOD` | CarAudioFMOD.obj | 0x4b8 | Per-car sounds. Members are the events themselves: `engineExtEvent`, `engineIntEvent`, `gearExtEvent`, `gearIntEvent`, `bodyworkEvent`, `windEvent`, `dirtEvent`, `hitEvents[3]`, `scrapeEvents[3]`, optional `downShiftEvent`, `hornEvent`, `gearGrindEvent`, `backfireExt/IntEvent`, `tractionControlExt/IntEvent`, `transmissionEvent`, `limiterEvent`, `turboEvent`, vectors `wheelEvents`, `skidExtEvents`, `skidIntEvents`, `surfaceEvents[4]`; plus `mixVolumes`, `skidParams`, smoothing state. |
| `MixVolumes` | CarAudioFMOD.obj | 0x24 | Category volumes read from `cfg/audio.ini [LEVELS]`: wind, tyres, surfaces, dirt, engine, transmission, opponents. |
| `SkidParams` | CarAudioFMOD.obj | 0x14 | `entryPoint`, `pitchBase`, `pitchGain`, `volumeGain`, smoothing. Hard-coded defaults (0.5, 0.75, 0.8, 2.5); only `entryPoint` is read from `cfg/audio.ini [SKIDS] ENTRY_POINT` (percent, scaled by 0.005). |
| `TrackAudio` | TrackAudio.obj | 0xb0 | `events` (ambience), `reverbs`, `occluders`, `started`. |
| `FormVolumes` | FormVolumes.obj (SystemApps.lib) | - | In-game "Audio" app: sliders per category, output-device list, saves `audio.ini`. |

## 3. Entry points

Creation
- `AudioEngine::AudioEngine` 0x1401f6bf0, called by `Game::Game` 0x140241880 (only when
  `audio_engine.ini [SETTINGS] ENABLE_AUDIO` is set). In order: `Studio::System::create`, advanced settings
  (command queue 0x80000), `setSoftwareChannels(MAX_CHANNELS)`, `Studio::System::initialize` (live-update flag from
  `LIVE_UPDATE`), `registerPlugin` twice (`FMODGetDSPDescription_DistanceFilter` 0x1401fc650,
  `FMODGetDSPDescription_Gain` 0x1401fd240), `setGeometrySettings(5000)`, `listDrivers` 0x1401fa320, device-lost
  callback `fmodCallback` 0x1401f9580, version check, `parseGUIDs("content/sfx/GUIDs.txt")` 0x1401fa8c0,
  `addBankRef("content/sfx/common.bank")` 0x1401f8220, then `setDriver` 0x1401fb7f0 with the device named in
  `audio.ini [SETTINGS] DRIVER_NAME`.
- `CarAudioFMOD::CarAudioFMOD` 0x140062830, called by `CarAvatar::initCommonPostPhysics` 0x1400d6190 for every
  car (player, AI, remote). That caller first parses `content/cars/<car>/sfx/GUIDs.txt` if it exists; the
  constructor then takes a bank reference on `content/cars/<car>/sfx/<car>.bank`, creates all events (optional
  ones only when `AudioEngine::hasEvent` 0x1401f9c60 finds them), reads `engine.ini` (`TURBO_n MAX_BOOST`) and
  `sounds.ini [ENGINE] POSITION`, and subscribes to gear-change events.
- `TrackAudio::TrackAudio` 0x1401c24c0, called by `TrackAvatar::TrackAvatar` 0x1401c5250.
- Surface sounds are pre-cached by `Sim::loadTrack` 0x14019a4c0 (`AudioEngine::addCache` 0x1401f8730 for
  `event:/surfaces/<wav name of each surface>`). `PitCrew::PitCrew` 0x1400a0b80 creates
  `event:/common/screw` / `unscrew`.

Per frame (main thread; see `docs/atlas/main_loop.md`)
- `Game::onIdle` 0x140242730 -> object tree `renderAudio(dt)` (vtable +0x20) -> `CarAudioFMOD::renderAudio`
  0x1400669b0 per car -> `startStopEvents` 0x1400678c0, `setEventVolumes` 0x140067330, `computeTransforms`
  0x140065f20, then parameters, `updateWheels` 0x140068730, `updateSkids` 0x140067f30, `updateSurfaces`
  0x140068450.
- `TrackAudio::render` 0x1401c42a0 (vtable +0x10): starts the ambience once, then stores the largest
  `AudioReverb::hearValue` 0x1401f9dd0 into `AudioEngine::reverbValue`.
- `AudioEngine::update` 0x1401fc080, called by `Game::onIdle`: master volume ramp (`rampVolume` 0x1401fafa0), for
  every registered event `getGeometryOcclusion` between listener and event and `setVolume((1 - occlusion) *
  baseVolume)`, then `Studio::System::update` and `checkOutputSanity` 0x1401f8e00.
- Listener: `AudioEngine::setListener` 0x1401fbb30 (matrix + velocity -> `setListenerAttributes`) is called by the
  active camera's update (`CameraDrivableManager::update*`, `CameraOnBoard::update`, `CameraTrack::update`,
  `CameraHelicopter::update`, `CameraCarManager::update`, free cameras ...). `AudioEngine::setDistanceScale`
  0x1401fb790 is set by `ACCameraManager::setAudioDistanceScale` 0x140033e20 and the track / free cameras.

Callbacks and events
- `CarAudioFMOD::onCarHit` 0x140066480 from `Sim::stepPhysicsEvent` 0x14019ebd0 (collision events queued by the
  physics thread): picks `event:/collisions/{car,track,object}/hit` or `/scrape`, parameter `impact_speed` (km/h).
- `CarAudioFMOD::onGearChanged` 0x140066670 (gear event handler): gear-shift event with parameter `state`
  (0 = down-shift, 1 = up-shift).
- `AudioEngine::start` 0x1401fbec0 / `stop` 0x1401fbfc0: pause menu and replay mode. `AudioEngine::setVolume`
  0x1401fbdf0: splash fade-in, replay shutdown, `FormVolumes`.

FMOD parameters fed from the game (confirmed in `renderAudio` and helpers; source is `CarAvatar::physicsState`)

| Event | Parameter = source |
|---|---|
| engine ext / int | `rpms` = max(engineRPM, 1); `throttle` = smoothed gas |
| backfire ext / int | `throttle`; started by an event handler subscribed in the constructor (lambda at 0x140065750). The thresholds (`BackfireParams`, `sounds.ini [BACKFIRE]`) are owned and evaluated by `CarAvatar`, not by this class |
| transmission | `drivetrain_speed` = drivetrainSpeed; `throttle` |
| turbo | `boost` = turboBoost / maxTurboBoost; `bov` = turboBov; `bov_decay` = seconds since blow-off |
| limiter, traction control | `decay` = seconds since `isEngineLimiterOn` / `CarAvatar::isTcInAction` |
| wind | `speed` (km/h); `air_pressure` = airDensity (1.2102 if zero) |
| bodywork | `susp_travel_speed` = smoothed, clamped `bodyWorkVolume` |
| dirt | `dirtiness` = clamp(0.1 * sum of tyreDirtyLevel); `speed` |
| wheel (per wheel) | `speed`, `brake`, `inflation` (tyreInflation), `suspension_damage` |
| scrape | `decay`, `speed` |
| surface (per wheel, pooled) | `speed`; the event is `event:/surfaces/<SurfaceDef.wavString>` of the surface under the wheel |
| skid ext / int (per wheel) | no parameter: `setBaseVolume` and `setBasePitch` are computed in code (below) |

Skid sound (confirmed structure, offsets resolved by hand): only on surfaces with `gripMod >= 0.9`. If the wheel's
`|slipRatio| > 0.8` while moving (locked or spinning wheel), volume = clamp(0.05 * speed km/h) and pitch = 1.2.
Otherwise, above 1 m/s and when `ndSlip >= entryPoint`: t = clamp((ndSlip - entryPoint) / 6), volume =
clamp(t * volumeGain * load factor), pitch = clamp(pitchBase + t * pitchGain, pitchBase, 1.2). Both are low-pass
filtered, multiplied by the tyres volume and, for other cars, by the opponents volume. Interior or exterior variants
are chosen by `shouldUseInternalSounds` 0x140067810.

FMOD API actually imported (from `re/index/calls.tsv`)
- `fmodstudio64.dll`: `Studio::System::{create, initialize, release, update, flushCommands, setAdvancedSettings,
  getLowLevelSystem, loadBankFile, registerPlugin, getEventByID, setListenerAttributes, getListenerAttributes,
  getCPUUsage, getBufferUsage}`, `Studio::Bank::unload`, `Studio::EventDescription::{createInstance,
  getMaximumDistance, loadSampleData, unloadSampleData}`, `Studio::EventInstance::{start, stop, release, setPaused,
  getPaused, getPlaybackState, setVolume, setPitch, setParameterValue, set3DAttributes, get3DAttributes,
  setTimelinePosition, getChannelGroup}`.
- `fmod64.dll`: `System::{getVersion, getNumDrivers, getDriver, getDriverInfo, setDriver, getOutput,
  getSoftwareFormat, setSoftwareFormat, setSoftwareChannels, getDSPBufferSize, setAdvancedSettings, setCallback,
  get3DSettings, set3DSettings, setGeometrySettings, getGeometryOcclusion, createGeometry, createReverb3D,
  getMasterChannelGroup, getChannelsPlaying}`, `ChannelControl::{setVolume, setReverbProperties}`,
  `Geometry::{addPolygon, getNumPolygons, setPolygonAttributes, setActive, release}`, `Reverb3D::{setProperties,
  set3DAttributes, get3DAttributes, setActive, release}`, and the C function `FMOD_System_GetDriver`.

## 4. What it reads from disk

| Path (as built in code) | Format | Read by | Content |
|---|---|---|---|
| `system/cfg/audio_engine.ini` | ini | `Game::Game`, `AudioEngine::AudioEngine`, `Sim::Sim` | `[SETTINGS] ENABLE_AUDIO`, `MAX_CHANNELS`, `LIVE_UPDATE` (the file also has `UPDATE_INTERVAL`, `ENABLE_PERFORMANCE_SPEW`). |
| `cfg/audio.ini` (Documents) | ini | `Game::Game`, `AudioEngine::AudioEngine`, `MixVolumes::MixVolumes` 0x140064a40, `SkidParams::SkidParams` 0x140065110, `FormVolumes` | `[LEVELS] MASTER, WIND, TYRES, SURFACES, DIRT_BOTTOM, OPPONENTS, ENGINE, TRANSMISSION`; `[SETTINGS] DRIVER_NAME`; `[SKIDS] ENTRY_POINT`. Written back by `FormVolumes::saveVolume` 0x1402fcb30. |
| `content/sfx/GUIDs.txt` | text, one `{guid} path` per line (about 3,200 lines: `bank:/...`, `event:/cars/...`, `event:/surfaces/...`, `event:/common/...`, `event:/collisions/...`) | `AudioEngine::parseGUIDs` | path -> GUID table. |
| `content/sfx/common.bank` (+ `common.strings.bank` next to it) | FMOD Studio bank | `AudioEngine::addBankRef` | Shared events: surfaces, collisions, dirt, ambience, pit crew. |
| `content/cars/<car>/sfx/GUIDs.txt`, `content/cars/<car>/sfx/<car>.bank` | text, bank | `CarAvatar::initCommonPostPhysics`, `CarAudioFMOD::CarAudioFMOD` | Per-car events `event:/cars/<car>/{engine_ext, engine_int, gear_ext, gear_int, bodywork, wind, horn, gear_grind, backfire_ext, backfire_int, tractioncontrol_ext, tractioncontrol_int, transmission, limiter, turbo, wheel, skid_ext, skid_int}`. |
| `content/cars/<car>/data/sounds.ini` | ini (inside `data.acd`) | `CarAudioFMOD::CarAudioFMOD` (`[ENGINE] POSITION` = `front` / `rear`), `BackfireParams::BackfireParams` 0x1400cccb0 (`[BACKFIRE] MAXGAS, MINRPM, MAXRPM, TRIGGERGAS`) | The other sections present in every car (`[SKIDS]`, `[WIND]`, `[TYRE_ROLLING]`, `[BODY_WORK]`, `[TURBO]`) are not read by any function in this build (confirmed with `re_query.py uses`): they are leftovers of the pre-FMOD sound system. |
| `content/cars/<car>/data/engine.ini` | ini | `CarAudioFMOD::CarAudioFMOD` | `[TURBO_n] MAX_BOOST` to normalise the `boost` parameter. |
| `content/tracks/<track>/data/audio_sources.ini` | ini | `TrackAudio::TrackAudio` | `[REVERB_n] ENABLED, NODE, MINDISTANCE, MAXDISTANCE, PRESET` (a preset name from the table `presetNames`, or `CUSTOM` with `DECAY_TIME, EARLY_DELAY, LATE_DELAY, HF_REFERENCE, HF_DECAY_RATIO, DIFFUSION, DENSITY, LOW_SHELF_FREQUENCY, LOW_SHELF_GAIN, HIGH_CUT, EARLY_LATE_MIX, WET_LEVEL`); `[OCCLUDER_n] MESH, VOLUME_OCCLUSION, DOUBLESIDED`. |
| track `.kn5` scene nodes named `AC_AUDIO_*` | scene graph | `TrackAudio::TrackAudio` | Each such node gets an `event:/common/ambience` emitter at its position (interpretation of the prefix search plus the event creation that follows it). |

`camera_click.wav`, `gui_click.wav` and `showroom_default.mp3` in `content/sfx` are not referenced by `acs.exe`
(no string use found); they belong to the launcher / showroom.

## 5. Size

`python tools/re_query.py size AudioEngine fmod_distance_filter fmod_gain CarAudioFMOD TrackAudio`

| Object file | Hand-written functions | Code bytes |
|---|---|---|
| AudioEngine.obj (ksAudioFMOD.lib) | 90 | 29,687 |
| fmod_distance_filter.obj (ksAudioFMOD.lib) | 13 | 3,085 |
| fmod_gain.obj (ksAudioFMOD.lib) | 12 | 1,422 |
| CarAudioFMOD.obj | 40 | 29,512 |
| TrackAudio.obj | 13 | 9,487 |
| **Total** | **168** | **73,193** |

Described here but not counted: `FormVolumes.obj` (SystemApps.lib, 36 functions, 18,402 bytes) belongs with the
other in-game apps (`ui.md`). The FMOD import thunks (`fmod64_vc.lib`, `fmodstudio64_vc.lib`, 65 symbols, 390
bytes) are not game code. `PitCrew.obj`, the cameras and `CarAvatar.obj` only call into this area.

## 6. Port difficulty

**M.** The game-side code is small and simple (wrappers, a parameter table, a few clamps and low-pass filters).
What makes it non-trivial is the data: every sound is an FMOD Studio 1.08 bank with its logic inside.
- Depends on: the render-side `CarPhysicsState` snapshot (rpm, gas, speeds, slips, loads, surface under each
  wheel, damage, turbo), the camera system (listener), the scene graph (emitter nodes, occluder meshes), ini
  reader, collision event queue.
- Option A (closest to the original): keep FMOD and call it through a Rust binding (`libfmod` or own FFI to
  `fmodstudio`); then the port is a 1:1 translation of `AudioEngine`, `CarAudioFMOD`, `TrackAudio`. Needs the FMOD
  runtime (proprietary, free for small projects) of a version that can still load 1.08 banks, and the two custom
  DSPs (port to Rust as FMOD plug-ins or load the FMOD example DLLs).
- Option B (pure Rust: `kira`, `rodio`, `cpal`): the banks cannot be used; the samples and the per-event logic
  would have to be re-authored. Much more work and a different sound.
- Nothing here needs to be bit-exact, and nothing feeds back into physics.

## 7. Open questions

- The FMOD header version for 1.08.12 and whether a current FMOD runtime still loads these banks was not checked.
- `updateSkids` scales the volume by a "load factor" (wheel value / (0.125 * sum of loads), floor 0.2); the
  pseudo-C shows the same offset as `ndSlip` for the numerator, which is probably a decompiler artefact for
  `load[i]`. Check in the disassembly before porting.
- `setEventVolumes` (interior / exterior mix, opponents attenuation 0.6, focus-car logic) and `startStopEvents`
  (which events run for far-away cars: `listenerPriority`, `listenerDistance`) were only skimmed.
- `AudioEvent::reverbResponse` and how `AudioEngine::reverbValue` is applied to events
  (`ChannelControl::setReverbProperties` in `AudioEngine::updateProperties` 0x1401fc310) were not traced.
- Weather / rain and UI click sounds: no code found in `acs.exe`; the `audio.ini` keys `MUSIC`, `UISOUNDS` and the
  section `[LEVELS_EXT]` are not read by this executable (confirmed with `re_query.py uses`).
- Choice made: `FormVolumes` is described here but left to the UI area for counting, because it is one of the
  SystemApps forms.
