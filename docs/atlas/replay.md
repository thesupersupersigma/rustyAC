# Replays, ghost cars and comparative telemetry

Source: `re/decomp/` pseudo-C (folders `ReplayManager`, `ReplayRecorder`, `ReplayInterface`, `GhostCar`,
`GhostCarRecorder`, `ComparativeTelemetry`, `ComparativeTelemetryRecorder`, plus callers in `CarAvatar`, `RaceManager`,
`sim`, `PhysicsAvatar`), `re/types/*.txt` (`ReplayFrame`, `ReplayFrameAdditionalInfo`, `ReplayFrameWingStatus`,
`FrameInformation`, `CompressedTransform`, `GhostCarFrame`, `CTData` ...), `tools/re_query.py`, and the shipped
`cfg/replay.ini` and `system/cfg/ghost_car.ini` (read-only). Nothing in the game folder, the Ghidra project or git was
changed. "Confirmed" = read in the pseudo-C, the type records or the string index; "interpretation" is marked.

## 1. What it is

The replay system is a ring buffer of compact per-car snapshots, filled from the physics thread at a fixed rate chosen
in `replay.ini` (8 to 67 Hz, default 33 Hz) and capped by a memory budget in megabytes.
Every car has its own `ReplayRecorder` holding a vector of 272-byte `ReplayFrame`s: positions as floats, orientations
and most other values as 16-bit half floats, pedals and levels as bytes, flags as a bit set.
`ReplayManager` owns the timeline: it decides when to record, stores per-frame scene data (sun angle, leaderboard
index, moved track objects), and during playback advances a frame cursor with a time multiplier (pause, slow motion,
rewind, fast forward, single frame, cut in / cut out).
Playback does not run the physics. Each `ReplayRecorder` is also an `ICarPhysicsStateProvider`: when replay mode starts
the car's visual side (`CarAvatar`) is switched from the live physics state to the recorder, which rebuilds a full
`CarPhysicsState` by blending two neighbouring frames. Offline the physics thread is paused meanwhile; online it keeps
running.
Replays are saved to `Documents/Assetto Corsa/replay/` as one binary file (format version 16) containing a header,
the scene frames, then one block per car; autosave keeps the last few sessions in `replay/temp/`.
A saved replay can be opened as its own session: the file names the track and cars, and the loader builds them.
The ghost car is a separate, much simpler recorder: full matrices of the body, wheels and suspension every 129 ms
for one lap, kept in a `.ghost` file per car / track and drawn as a transparent car.
Comparative telemetry is a third lap recorder (gear, speed, gas, brake against lap position) saved as `.tc` files
and shown as graphs by an app.

## 2. Main classes

| Class | Source file | Size | Role |
|---|---|---|---|
| `ReplayManager` | ReplayManager.obj | 0x198 | `GameObject`. Timeline and state machine (`status`: play, pause, rewind, slow motion, single frame ...), `recorders` (one per car), ring indices (`currentRecordingIndex`, `recordedFrames`, `maxFrames`), cuts, `timeMult`, `playCounter`, autosave table, events `evOnReplayStarted` / `Stopped` / `Rewind` / `evDidFinishedSavingReplay`. Save and load of the file. |
| `ReplayRecorder` | ReplayRecorder.obj | 0x110 | `GameObject` + `ICarPhysicsStateProvider`. Per car: `frames`, `framesAdditionalInfo`, `frames_wings`, `frameBlend`, `playingPos`, lap index cache. Records, saves, loads and plays back one car. Static `recordIntervalMS`. |
| `ReplayFrame` | (struct) | 0x110 | One car snapshot, see 3.2. |
| `ReplayFrameAdditionalInfo` | (struct) | 0x4 | `carDirt`, `engineLife`, `turboBoost`, `connected` (bytes). |
| `ReplayFrameWingStatus` | (struct) | 0x4 | `angle` (byte) per wing per frame. |
| `FrameInformation` | (struct, global vector `frameInformation`) | 0x20 | Per frame for the whole scene: `sunAngle` (half), `lbIndex` (int16, which leaderboard snapshot), `trackObjects` (vector of 12-byte `CompressedTransform`: half euler + half translation). |
| `ReplayLap`, `ReplayAutosave` | (structs) | 0x8 / 0x28 | Lap time + valid flag per car; autosave slot count + short name per session type. |
| `ReplayFrameV5`, `V6`, `V15` | (structs) | 0x558 / 0x1a0 / 0x110 | Older frame layouts kept for the loader. |
| `ReplayInterface` | ReplayInterface.obj | 0x330 | The replay GUI: transport buttons, position slider with cut markers, slow-motion level, camera / car selectors, save-name box, photo mode (exposure, FOV, depth of field, roll, sun azimuth / zenith). |
| `GhostCarRecorder` | GhostCarRecorder.obj | 0x160 | Per player car: `recordingBuffer` / `playingBuffer` of `GhostCarFrame`, best ghost lap time, load / save. |
| `GhostCarFrame` | (struct) | 0x240 | `mat44f body`, `mat44f wheels[4]`, `mat44f suspensions[4]`. |
| `GhostCar` | GhostCar.obj | 0x330 | The transparent car model (LOD 0 of the same car) driven from `GhostCarRecorder`; distance-based opacity. |
| `ComparativeTelemetryRecorder` | ComparativeTelemetryRecorder.obj | 0x170 | Records `CTData` (gear, normalizedPosition, speed, gas, brake; 20 bytes) for the current lap; keeps last and best lap; `.tc` files. |
| `ComparativeTelemetry` | ComparativeTelemetry.obj | - | App window that plots the player's laps against saved opponents. |

## 3. Entry points

### 3.1 Creation

- `Sim::Sim` 0x140192070 -> `ReplayManager::ReplayManager` 0x140153130 for a normal session, or
  `RaceManager::initReplay` 0x14013e820 when `race.ini` has `[REPLAY] FILENAME` (also the benchmark file
  `content/benchmark/benchmark.acr`): constructor + `ReplayManager::load` 0x140157680.
- The constructor reads `cfg/replay.ini`, sizes every recorder (`ReplayRecorder::setFrameSize` 0x140162420), deletes
  leftover files in `replay/temp/`, creates `ReplayInterface::ReplayInterface` 0x140148300 and subscribes to
  `PhysicsAvatar::evOnStepCompleted`, each car's `evOnLapCompleted` and `Sim::evOnNewSession`.
- Per car: `CarAvatar::initCommonPostPhysics` 0x1400d6190 -> `ReplayRecorder::ReplayRecorder` 0x14015c9e0 (reads the
  car's `engine.ini` `[ENGINE_DATA] LIMITER`).
- Ghost: `RaceManager::initOffline` 0x14013a6c0 -> `RaceManager::initGhostcar` 0x14013a210 (`race.ini`
  `[GHOST_CAR] RECORDING, PLAYING`) -> `CarAvatar::initGhostCar` 0x1400d7510 ->
  `GhostCarRecorder::GhostCarRecorder` 0x140106b90 + `GhostCar::GhostCar` 0x140104fe0.
- `Sim::Sim` -> `ComparativeTelemetryRecorder::ComparativeTelemetryRecorder` 0x1400731b0;
  `ESCMenu::ESCMenu` -> `ComparativeTelemetry::ComparativeTelemetry` 0x14006dff0.

### 3.2 Recording (physics thread)

`PhysicsEngine::step` 0x140264760 raises `evOnStepCompleted` -> `PhysicsAvatar::onPhysicsStepCompleted`
0x1401221a0 (copies all `CarPhysicsState`s under a lock, then raises `PhysicsAvatar::evOnStepCompleted` with the
physics time `pt` and the state vector) -> `ReplayManager::onStepCompleted` 0x140158b50.

When `pt - lastRecordTime >= ReplayRecorder::recordIntervalMS` it:
1. calls `ReplayRecorder::recordFrame` 0x1401609d0 (frame index, that car's `CarPhysicsState`) for every car;
2. fills `frameInformation[index]`: sun angle, current leaderboard snapshot index, and for every `TrackObject` its
   `PhysicsObject::getWorldMatrix` 0x1402acb70 packed as a `CompressedTransform`;
3. advances the ring: `recordedFrames` grows up to `maxFrames`, `currentRecordingIndex` wraps.

`ReplayFrame` contents (confirmed from the type record): `bodyTranslation` (vec3f) + `bodyOrientation` (3 halves,
euler); `worldTranslation` / `worldOrientation`; for each wheel `susTranslation`, `susOrientation`, `tyreTranslation`,
`tyreOrientation`; `velocity` (3 halves); `engineRPM`; per wheel `wheelAngularSpeed`, `tyreSlipAngle`, `slipRatio`,
`ndSlip`, `load` (halves); `steer`, `bodyworkVolume`, `drivetrainSpeed` (halves); `lapTime`, `lastLap`, `bestLap`
(uint32); bytes `fuel`, `fuelLaps`, `gear`, `tyreDirtyLevel[4]`, `damageZoneLevel[5]`, `gas`, `brake`, `lapCount`;
a 14-bit `status` set (lights, limiter and similar flags; bits not decoded). `recordFrame` converts matrices to euler
angles (`mat44f::toEuler`) and also samples dirt and brake lights from the visual side.

Rate: `[QUALITY] LEVEL` 0..4 -> `recordIntervalMS` 120, 90, 60, 30, 15 (8.3, 11.1, 16.7, 33.3, 66.7 Hz); any other
value keeps the default and prints "using default 30Hz". Budget: `maxFrames = MAX_SIZE_MB * 1e6 / (cars * 272)`
(before the ini is read: 10 minutes).

Other recorders:
- `GhostCarRecorder::onStepComplete` 0x1401093f0 (same physics event): every 129 ms, after the start line has been
  crossed once, pushes a `GhostCarFrame`; `GhostCarRecorder::onLapCompleted` 0x1401091e0 keeps the lap as the new
  ghost if it beats `ghostCarLap`, and flags `needToSave`.
- `ComparativeTelemetryRecorder::update` 0x140076850 (main thread, `GameObject` update) drains a concurrent queue
  filled from the physics side and bins samples by `recordingInterval` along the lap.

### 3.3 Playback (main thread)

- Enter: pause-menu / end-of-session / key command closures -> `ReplayManager::startReplayMode` 0x14015b480 (also
  `Sim::onPostLoad` 0x14019e020 for a loaded replay): shows the GUI, resets cuts, switches the camera manager mode,
  `ReplayManager::play` 0x1401591f0, raises `evOnReplayStarted`.
- `CarAvatar::onStartReplay` 0x1400d9400 stores `nonReplayPhysicsStateProvider` and points `physicsStateProvider` at
  the car's `ReplayRecorder`; it also zeroes controller vibration and force feedback.
  `CarAvatar::onStopReplay` 0x1400d9520 restores it. `Sim::onReplayModeChanged` 0x14019e0c0 pauses the physics thread
  (`PhysicsAvatar::pausePhysics` 0x140122300) only when there is no network client.
- Each frame: `Sim::update` 0x14019ef90 -> `ReplayManager::updatePlayState` 0x14015b9a0: `timeMult` from the status
  (slow motion eases in over 0.4 s to `1 / slowMoLevel`; clamp to +-5), `playCounter += |timeMult| * dt * 1000`; each
  time it passes `recordIntervalMS` the cursor `currentFrame` moves one frame forward or back (modulo the ring) and
  stops at `cutIn` / `cutOut`. Then `ReplayManager::updateReplayRecorders` 0x14015bc90 sets every recorder's
  `frameBlend = playCounter / recordIntervalMS` and `playingPos`, and interpolates (`lerp`) the track objects into
  `TrackObject::setMatrix` 0x1401cf6f0.
- Rendering then pulls `ReplayRecorder::getPhysicsState` 0x14015d850 (9.9 kB): blends frame `playingPos` and the
  next one into a `CarPhysicsState` (matrices rebuilt with `mat44f::createFromEulerSafe`, lap position recomputed
  with `InterpolatingSpline::worldToSpline` 0x1401f3bf0 on the AI line, dirt level pushed to `DynamicCarEffects`),
  and `ReplayRecorder::getWingState` 0x14015ff00.
- Transport: `ReplayManager::pause` 0x140159070, `ReplayManager::rewind` 0x140159b40,
  `ReplayManager::fastForward` 0x140156780, `ReplayManager::slowMotion` 0x14015b3b0,
  `ReplayManager::nextFrame` 0x140158900, `ReplayManager::nextLap` 0x140158930 /
  `ReplayManager::previousLap` 0x1401592a0 (through `ReplayRecorder::fetchLapFrames` 0x14015d760),
  `ReplayManager::setNormalizedPosition` 0x14015b0b0, `ReplayManager::setCutInPerc` 0x14015af30 /
  `ReplayManager::setCutOutPerc` 0x14015aff0, `ReplayManager::photoMode` 0x140159120; each ends in
  `ReplayManager::updateReplayAudio` 0x14015bc00. Exit: `ReplayManager::stopReplayMode` 0x14015b800.
- GUI: `ReplayInterface::render` 0x14014d970.
- Ghost: `GhostCar::update` 0x140106380 asks `GhostCarRecorder` for blended body / tyre / suspension matrices, hides
  itself in replay mode and sets the material colour / opacity from distance.

Bypassed during playback (interpretation from the provider swap): everything in `Car::step`, input, AI, collisions.
Sound, skid marks, lights, driver animation, cameras and HUD apps keep working because they read `CarPhysicsState`.

### 3.4 Saving and loading

- Manual save from the GUI and autosave both call `ReplayManager::save` 0x140159d10 (name, target directory kind).
  Autosave: a closure at 0x1401546d0 run on each new session: if autosave is on, the replay is not a loaded one and
  `recordedFrames >= minFrames`, it trims old files of that session type (`ReplayManager::clearSaved` 0x1401559f0),
  saves to `replay/temp/` under `ReplayManager::getAutoFileName` 0x1401568b0, then
  `ReplayManager::resetReplay` 0x1401597d0. `ReplayManager::shutdown` 0x14015b1c0 does the same on exit.
- `ReplayManager::load` 0x140157680 pauses physics, reads the header, calls `Sim::loadTrack` 0x14019a4c0 and
  `Sim::addCar` 0x1401978a0 for every car in the file, then `ReplayRecorder::load` 0x140160220 per car.

File layout written by `save` (confirmed order; little-endian; strings are int32 byte length + UTF-8):

| Part | Fields |
|---|---|
| Header | int32 `version` = 16; double `recordIntervalMS`; string weather name; string track name; string track layout; int32 car count; int32 `currentRecordingIndex`; int32 frame count; int32 track-object count |
| Scene frames, per frame | half `sunAngle`, int16 `lbIndex`, then 12 bytes per track object |
| Per car | strings: car name, driver name, nation code, team, skin; then the car block from `ReplayRecorder::save` 0x1401622a0: for each frame from cut-in to cut-out, `ReplayFrame` (272 bytes) + `ReplayFrameAdditionalInfo` (4) + 4 bytes per wing; then the car's lap list (int32 count + 8 bytes each) |
| Trailer | int32 leaderboard-snapshot count, then each snapshot (entries of four int32) |

Versioning: the writer always emits 16. `ReplayManager::load` accepts older headers (below version 13 it assumes a
32 ms interval); `ReplayRecorder::load` refuses anything below 15 ("no longer supported") and converts version 15
frames (`ReplayFrameV15`) to the current layout. Code for V5 / V6 additional info remains in the loader.

## 4. What it reads from disk

| Path | Reader | Content |
|---|---|---|
| `cfg/replay.ini` (Documents; listed in `system/cfg/inireaderdocuments.ini`) | `ReplayManager::ReplayManager` | `[QUALITY] LEVEL`; `[REPLAY] MAX_SIZE_MB`; `[AUTOSAVE] ENABLED, RACE, QUALIFY, OTHERS` (slots per session type, short names `R`, `Q`, `O`), `MIN_TIME_SECONDS`. |
| `Documents/Assetto Corsa/replay/<name>` and `replay/temp/AC_...` | `ReplayManager::save`, `load`, `clearSaved`, `RaceManager::initReplay` | Replay files. Names from `ReplayManager::getReplayFileName` 0x140157260 (`AC_` + date / track / car parts). |
| `cfg/race.ini` `[REPLAY] FILENAME` | `RaceManager::initReplay`, `RaceManager::loadSessions` | Start the game as a replay viewer. |
| `content/benchmark/benchmark.acr` | `RaceManager::initReplay` | Benchmark replay. |
| `content/cars/<car>/data/engine.ini`, `content/cars/<car>/skins/` | `ReplayRecorder::ReplayRecorder`, `ReplayManager::save` | Limiter RPM; skin name check. |
| `content/gui/replay/*.png`, `content/gui/photoMode/viewFinder.png` | `ReplayInterface` | GUI art. |
| `system/cfg/ghost_car.ini` `[GHOST_CAR] MIN_DISTANCE, MAX_DISTANCE, MAX_OPACITY, COLOR` | `GhostCar::loadGhostCarParameters` 0x140105fd0 | Ghost look. |
| `cfg/race.ini` `[GHOST_CAR] RECORDING, PLAYING, LOAD, FILE, SECONDS_ADVANTAGE` | `RaceManager::initGhostcar`, `GhostCarRecorder::loadGhostCarRecorderCustomParameters` 0x140108880 | Ghost on / off, custom ghost file, head start. |
| `Documents/Assetto Corsa/GhostCar/.../GHOST_CAR_<...>.ghost` | `GhostCarRecorder::getGhostCarFileName` 0x140107d20, `GhostCarRecorder::loadGhostCarRecording` 0x140108c10, `GhostCarRecorder::saveGhostCarRecording` 0x1401097b0 | Two int32 (second = 1), strings driver / track / layout / car, then `GhostCarFrame`s (576 bytes each). |
| `content/cars/<car>/data/lods.ini`, `suspensions.ini` | `GhostCar::init3D` 0x140105520 | Model and wheel offsets for the ghost. |
| `Documents/Assetto Corsa/ctelemetry/player/<...>.tc`, `ctelemetry/opponents/*` | `ComparativeTelemetryRecorder::loadBest` 0x140074d30, `ComparativeTelemetryRecorder::saveBest` 0x140075d50, `ComparativeTelemetryRecorder::getCTOpponents` 0x1400741a0 | Best-lap traces. |

## 5. Size

`python tools/re_query.py size ReplayManager ReplayRecorder ReplayInterface GhostCar GhostCarRecorder
ComparativeTelemetry ComparativeTelemetryRecorder`: **283 hand-written functions, 177,671 code bytes.**

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| ReplayManager.obj | 67 | 50,557 | | ReplayInterface.obj | 71 | 38,738 |
| ReplayRecorder.obj | 22 | 25,688 | | ComparativeTelemetry.obj | 33 | 23,029 |
| ComparativeTelemetryRecorder.obj | 32 | 17,016 | | GhostCarRecorder.obj | 34 | 15,279 |
| GhostCar.obj | 24 | 7,364 | | | | |

Not counted here: PhysicsAvatar.obj (main loop), `CarAvatar` replay hooks (content / car visuals), camera classes used
in replay mode (camera area), the `half_float` library (templates, no object file of its own).

## 6. Port difficulty: **M**

- Recording and playback are plain data shuffling; no physics. A new implementation that only has to replay its own
  files is small (ring buffer of structs, lerp between two frames).
- Reading original files needs the exact `ReplayFrame` layout (272 bytes, packed fields at odd offsets such as
  `tyreDirtyLevel` at 0xff), half-float decoding, euler conventions of `mat44f::toEuler` / `createFromEulerSafe`,
  and the V15 conversion. The `half` crate covers half floats; `bytemuck` or manual parsing covers the layout.
- `ReplayRecorder::getPhysicsState` is the costly part to port faithfully (9.9 kB): it re-derives many
  `CarPhysicsState` fields that the renderer, audio and apps expect.
- `ReplayInterface` (39 kB) is GUI work tied to the ksgui toolkit: redo in whatever UI the port uses.
- Ghost car and comparative telemetry are independent and small (S each); they need only matrices or five numbers
  per sample.
- Recording is a useful test asset: a replay file is a ready-made stream of car poses to compare a Rust car against.
- Third-party: none. Depends on: main loop / physics hand-off, `CarPhysicsState`, track objects, cameras, GUI,
  session logic (leaderboards, laps), config.

## 7. Open questions

- File extension: no `.acreplay` literal exists in `acs.exe`; saved names have no extension added by this code, so the
  extension seen in the launcher is assumed to be added there (interpretation).
- The 14 `status` bits of `ReplayFrame` and the meaning of `boh` fields were not decoded.
- Exact name patterns of `getReplayFileName` / `getAutoFileName` / `getGhostCarFileName` (order of date, track, car,
  driver parts) were read from string literals only.
- First int32 of the `.ghost` file (probably the recorder `version`) and the block between the names and the frames
  were not decoded; `MAX_MINUTES_RECORDING`, `TIME_DIFF_ENABLED`, `PLAYER_NAME_ENABLED` in `ghost_car.ini` have no
  reader in the string index (`re_query.py uses`), so they look unused by `acs.exe`.
- Leaderboard snapshots (`replayLeaderboards`, entries of 0x18 bytes in memory, four int32 on disk): writer side seen,
  producer not traced.
- Whether `ReplayManager::load` for versions 13 and 14 can succeed at all given the recorder refuses versions below 15
  was not followed.
- "Which systems are bypassed" is inferred from the provider swap and the physics pause, not from reading every
  consumer; online the physics keeps stepping while the local view shows the replay.
- Choice made: ComparativeTelemetry (app) and ComparativeTelemetryRecorder are counted here as lap recorders; the UI
  area should not count them again.
