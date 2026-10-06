# AI drivers

Source: `re/decomp/` pseudo-C (folders `AIDriver`, `AIDriverDrag`, `AISpline`, `AISplineRecorder`, `RaceEngineer`,
`SplineLocator`, plus the callers in `CarAvatar`, `RaceManager`, `sim`, `Track`, `FormAI`), `re/types/*.txt`,
`tools/re_query.py`, `tools/ini_survey.py ai.ini`, the commented `formula_k/data/ai.ini` in the SDK and
`content/tracks/spa/data/ai_hints.ini` (all read-only). Nothing in the game folder, the Ghidra project or git was
changed. This is a structure and data-flow map; the control maths was not studied. "Confirmed" = read in the
pseudo-C or the string index; "interpretation" is marked.

## 1. What it is

An AI driver is not a separate simulation: it is just another source of pedal, steering and gear inputs for a normal
physics `Car`. `AIDriver` implements the same `ICarControlsProvider` interface as the wheel, keyboard and gamepad
classes, and the car asks it for controls at the start of every physics step.
The driver follows a pre-recorded racing line stored in the track folder (`ai/fast_lane.ai`), and a second line for
the pit lane (`ai/pit_lane.ai`). Each point of the line carries a payload recorded from a real lap: speed, gas, brake,
corner radius, distance to the track edges, camber, direction and grade.
Every fifth physics step (66.6 Hz) the driver locates itself on the line, looks ahead, picks a lateral offset
(overtaking, defending, avoiding slower cars, staying inside the track limits), computes a steering target with a
PID controller, a target speed from the line radius and a look-ahead braking calculation, and chooses gears from RPM
thresholds. Between those updates the last requests are held, and a cheap per-step pass applies ABS-like and
traction-control-like corrections.
Skill is one number, `basePush` (0..1) from `AI_LEVEL` in `race.ini`, plus `aggression`; per-car tuning is the car's
`data/ai.ini`; per-track tuning is `data/ai_hints.ini`.
The AI also decides fuel and tyre strategy, requests pit stops, changes a few setup items (wings, final ratio) and
uses KERS / DRS / push-to-pass / turbo boost where the car has them.
AI cars get help the player does not: a tyre grip multiplier (`Tyre::aiMult`) and extra forces on the body
(`useMagicForces`).
`RaceEngineer` is a calculator attached to a car that answers "what can this car do" questions (grip, loads, braking
force, top speed per gear, fuel per lap); the AI uses it, and so do several non-AI systems.

## 2. Main classes

| Class | Source file (library) | Size | Role |
|---|---|---|---|
| `ICarControlsProvider` | (interface) | 0x18 | Vtable +0x08 `acquireControls(CarControls*, dt, ...)`; see `docs/map/car_step.md`. |
| `AIDriver` | AIDriver.obj (ksPhysicsAC.lib) | 0x3e8 | The circuit AI. Members worth knowing: `basePush`, `aggression`, `genome[20]`, `pidSteer`, `currentSpline`, `currentPayload`, `currentOffset` / `desiredOffset`, `gasRequest` / `brakeRequest` / `steerRequest`, `opponentData`, `brakeObstacles`, `brakeProfiler`, `humanize`, `kersAreas`, `aiSetup`, `engineer`, `freqCounter`, `requestPitStop`, `ultraGrip`, `useMagicForces`. |
| `AIDriverDrag` | AIDriverDrag.obj (ksPhysicsAC.lib) | 0x118 | Simple straight-line AI for drag races: three PID controllers (steer, gas, brake), launch with a slip-ratio limit, shift by RPM. |
| `AISpline` | AISpline.obj (ksPhysicsAC.lib) | 0xe0 | A line: `InterpolatingSpline spline` + `vector<AISplinePayload> payloads` + `straights`, `lapTime`, `version`. Loader, saver and geometry queries (radius, sides, distance to corner). |
| `AISplinePayload` | (struct) | 0x54 | Per point: `speedMS`, `radius`, `sides[2]`, `camber`, `direction`, `normal`, `forwardVector`, `length`, `gas`, `brake`, `grade`, `grip`, `distFromCorner`, `distFromNextCorner`, `isPitlane`, `compression`. |
| `AISplineRecorder` | AISplineRecorder.obj (ksPhysicsAC.lib) | 0x448 | Owned by `Track`. Holds `splineCurrent` (being recorded), `bestLapSpline` (= fast lane), `pitLaneSpline`, `leftSpline` / `rightSpline` (track edges), the four hint lists; loads, records and saves them. |
| `SplineLocator` | SplineLocator.obj (ksPhysicsAC.lib) | 0x30 | Per car, every physics step: `normalizedPos` on the fast lane, signed lateral `offset`, `isOutsideLimits`. Used by AI, timing, DRS, penalties. |
| `RaceEngineer` | RaceEngineer.obj (ksPhysicsAC.lib) | 0x18 | `car`, `fuelPerLapEvaluated`. About 50 small estimators (section 3.4). |
| `FormAI` | SystemApps.lib | - | Developer app "AI": records lines, shows splines, edits `ai.ini` values, hands the player car to an `AIDriver`. Mention only (UI area). |

## 3. Entry points

### 3.1 Attaching a driver to a car

- `CarAvatar::initControls` 0x1400d6f30 reads `cfg/controls.ini` `[HEADER] INPUT_METHOD` (`WHEEL`, `X360`,
  `KEYBOARD`, `AI`); for `AI` it creates `AIDriver::AIDriver` 0x140298c60 for the player car.
- Opponents: a `RaceManager` closure at 0x140133e00 (run on the physics thread through the command queue) creates the
  `AIDriver`, sets `basePush` and `aggression`, and calls `CarAvatar::setControlsProvider` 0x1400d9f90.
  `basePush = clamp(level, 0, 1)` with `level = [RACE] AI_LEVEL / 100`, multiplied by the per-car
  `[CAR_n] AI_LEVEL / 100` when that is not 0. `aggression = saturate([CAR_n] AI_AGGRESSION / 100) * 0.95`
  (read in `RaceManager::initOffline` 0x14013a6c0; 0 when the key is absent).
- Two more closures hand the player car to the AI: in `sim` at 0x140195b40 and in `FormAI` at 0x1402e05b0 (the latter
  also sets the physics time scale to 5).
- Drag sessions: `RaceManager::initOffline` creates `AIDriverDrag::AIDriverDrag` 0x1402ab740.

### 3.2 When it runs

Physics thread (or a physics worker thread, cars are stepped in parallel), once per 3 ms step:
`PhysicsEngine::step` 0x140264760 -> `Car::step` 0x140275da0 -> `Car::pollControls` 0x140274e70 ->
`controlsProvider->acquireControls` = `AIDriver::acquireControls` 0x14029baa0.

Order inside `acquireControls` (confirmed from call order):

1. Every step: `AIDriver::stepPushVariations` 0x1402a1f20, `AIDriver::stepSetup` 0x1402a2020, the fuel / stint check
   that sets `requestPitStop` (uses `FuelLapEvaluator::getFuelLaps` 0x14028d260), `AIDriver::stepHumanizer`
   0x1402a19e0, `AIDriver::stepSplineSelection` 0x1402a2240 (fast lane or pit lane), `AIDriver::stepPitLaneMode`
   0x1402a1d00, `AIDriver::stepSinOffset` 0x1402a2170.
2. Every 5th step (`freqCounter`, dt passed as 5 * 0.003 s): danger-zone factor from the hints,
   `AIDriver::getDistanceToNextCorner` 0x14029d250, `AIDriver::updateCurrentSplinePos` 0x1402a3200,
   `AIDriver::stepCarsAwareness2` 0x14029f8d0 (opponents: fills `opponentData` / `brakeObstacles`, chooses the lateral
   offset, 4 kB), `AIDriver::stepSteer` 0x1402a2630, `AIDriver::stepGears` 0x1402a1710, `AIDriver::stepGasBrake`
   0x1402a08e0 (target speed, look-ahead braking via `AIDriver::getLABraking` 0x14029d650, start procedure via
   `AIDriver::stepGasBrakeStart` 0x1402a1580, tyre choice via `AIDriver::updateTyres` 0x1402a32f0),
   `AIDriver::stepTurbo` 0x1402a30c0, `AIDriver::stepDRS` 0x1402a0850, `AIDriver::stepKers` 0x1402a1bb0, the
   retire / teleport-to-pit branch (`Car::forcePosition` 0x14026fe10), `AIDriver::stepP2P` 0x1402a1c20.
3. Every step: clamp and copy `steerRequest`, `gasRequest`, `brakeRequest` into `CarControls`, `clutch = 1`, then
   `AIDriver::stepSuperhuman` 0x1402a2a60 (cuts gas / brake when tyre `ndSlip` > 1, and for AI cars adds a local force
   on the body through `IRigidBody` +0xd8 `addLocalForce`), then publishes `Car::isRequestingPitStop` and
   `Car::aiLapsToComplete`.

Connections to the tyre model: `stepGasBrake` writes `Tyre::aiMult` on all four tyres (`ultraGrip`, set to 1.2 by the
constructor, or 1.001 in one branch). This answers open question 7 of `docs/map/tyre.md` for `aiMult`.

Event handlers registered by the constructor: `Car::evOnLapCompleted`, `Car::evOnForcedPositionCompleted`,
`PhysicsEngine::evOnNewSessionPhysics` (-> `AIDriver::onNewSession` 0x14029f140 -> `AIDriver::generatePitStopStrategy`
0x14029cb70), `Car::evOnCollisionEvent`.

### 3.3 The line: loading, recording, locating

- `Track::initAISpline` 0x1402782a0 -> `AISplineRecorder::AISplineRecorder` 0x140293bf0 ->
  `AISplineRecorder::load` 0x1402952c0 (fast lane; if the file version is below 7 it is re-saved as 7; side splines
  from `data/side_l.csv` / `side_r.csv` if present; then `AISplineRecorder::loadPitLaneSpline` 0x140296200 ->
  `AISpline::initPitlane` 0x1402a7f70) and `AISplineRecorder::loadHints` 0x140295670.
- `AISpline::load` 0x1402a8390 = `AISpline::loadFast` 0x1402a83c0 (reads the version, then
  `AISpline::loadVersion6` 0x1402a85b0 or `AISpline::loadVersion7` 0x1402a8b80) + `AISpline::calculateRadius`
  0x1402a6420. `AISpline::save` 0x1402a98d0 always writes version 7.
- Recording: `Track::step` 0x140278d20 -> `AISplineRecorder::step` 0x140296f60 each physics step while `isActive`:
  when the car has moved `recordingStep` metres it adds a point with a payload (speed, gas, brake from the car).
  `AISplineRecorder::startRecording` 0x140296f00 is triggered from `FormAI`;
  `AISplineRecorder::onLapCompleted` 0x140296760 promotes a finished lap to `bestLapSpline`;
  `AISplineRecorder::save` 0x140296ae0 (also from the destructor) recomputes radius, normals and sides
  (`AISpline::buildSides` 0x1402a5630 ray-casts left / right up to 100 m to find the track edges) and writes
  `ai/fast_lane.ai.candidate` / `ai/pit_lane.ai.candidate` or the real files.
- Locating: `Car::stepComponents` 0x1402764d0 -> `SplineLocator::step` 0x1402ab5c0 for every car (AI or not):
  `InterpolatingSpline` closest-point search (restricted to `Track::startingBounds` until the first valid position),
  signed distance from the line, and `isOutsideLimits` from the payload `sides` and the car width.

### 3.4 RaceEngineer

A member of `AIDriver` (`engineer`) and a short-lived helper in `Car`, `AutoShifter`, `SpeedLimiter`,
`DriftModeComponent`, `SetupManager`, `KeyboardCarControl`, `Wing` and the Python API (callers confirmed). It estimates, from the car's current state and data:
tyre loads and lateral grip, lateral G from loads, front / rear ride height, available and optimal brake, maximum
braking force, acceleration profile, top speed per gear, wing drag / lift projections, forward force at full gas,
torque-to-grip ratio, natural frequencies and damping ratios, anti-squat, caster and KPI, track widths, weight
distribution, driven tyres and their slip. `RaceEngineer::evaluateFuelPerLapFromTrackSpline` 0x14027ac60 loads
`data/ideal_line.ai` and integrates fuel use over one lap; `RaceEngineer::findTyreCompound` 0x14027b3a0 picks a
compound for the AI.

## 4. What it reads from disk

| Path | Reader | Content |
|---|---|---|
| `content/tracks/<track>[/<layout>]/ai/fast_lane.ai`, `ai/pit_lane.ai` | `AISplineRecorder::load`, `loadPitLaneSpline` | Binary lines, format below. Written back as `.candidate` files or in place. |
| `.../data/ai_hints.ini` | `AISplineRecorder::loadHints` | `[HINT_n]`, `[BRAKEHINT_n]`, `[MAXSPEED_n]`: `START`, `END` (lap fractions), `VALUE`. `[DANGER_n]`: `START`, `END`, `LEFT`, `RIGHT`. |
| `.../data/side_l.csv`, `side_r.csv` | `AISplineRecorder::recomputeSidesFromCsv` 0x140296870 | Optional track-edge polylines (`Spline::loadFromCSV` 0x1401ee3c0); sides are recomputed on several threads. |
| `.../data/ideal_line.ai` | `RaceEngineer`, `evaluateTimeFromTrackSpline` 0x1401bab80 | Optional line for fuel / lap-time estimates. |
| `content/cars/<car>/data/ai.ini` | `AIDriver::AIDriver`, `AIDriverDrag::AIDriverDrag`, `AutoShifter::loadINI` 0x1402ba050 | See below. |
| `content/cars/<car>/data/ai_tyres.ini` | `AIDriver::initTyreStrategies` 0x14029e600 | One section per compound short name, `MAX_KM` (25 of 113 cars have it). |
| `content/cars/<car>/data/tyres.ini` `[COMPOUND_DEFAULT] INDEX`, `data/setup.ini` (`[FINAL_GEAR_RATIO] RATIOS`, `[WING_n] MIN, MAX`) | `AIDriver::AIDriver` | Preferred compound; which setup items the AI may change. |
| `Documents/Assetto Corsa/setups/<car>/.../ai_default.ini`, `system/setups/...` | `AIDriver::initSetup` 0x14029e250 | Optional AI setup files. |
| `cfg/race.ini` `[RACE] AI_LEVEL`, `[CAR_n] AI_LEVEL`, `[CAR_n] AI_AGGRESSION` | `RaceManager::initOffline` | Skill and aggression. |
| `cfg/controls.ini` `[HEADER] INPUT_METHOD=AI` | `CarAvatar::initControls` | Player car driven by AI. |

`ai.ini` keys read by `AIDriver::AIDriver` (section, key; confirmed pairs): `[HEADER] VERSION`; `[GEARS] UP`, `DOWN`;
`[STEER] STEER_GAIN`; `[LOOKAHEAD] BASE`, `GAS_BRAKE_LOOKAHEAD`; `[UNDERSTEER] HINT`; `[TYRES] HINT`;
`[PEDALS] BRAKE_HINT`; `[PHYSICS_HINTS] AERO_HINT`; `[BRAKE_BIAS] FRONT_BIAS` / `VALUE`; `[WING_VARIATION] VALUE`.
`AIDriverDrag` adds `[DRAG] SLIP_RATIO_LIMIT`. `[GEARS] SLIP_THRESHOLD` and `GAS_CUTOFF_TIME` are read by
`AutoShifter` (the automatic-gearbox assist), not by the AI. Present in the data but with no reader found in
`AIDriver`: `[PEDALS] GASGAIN`, `TRAIL_HINT`, `[ULTRA_GRIP] VALUE`, `[LOOKAHEAD] SPEED_GAIN` (`TRAIL_HINT` and
`ULTRA_GRIP` appear only in a `FormAI` closure).

**`.ai` file, version 7** (confirmed from `loadFast` + `loadVersion7`; little-endian, no padding):

| Part | Bytes | Fields |
|---|---|---|
| Header | 16 | int32 `version`, int32 point count, uint32 `lapTime` (ms), int32 unused |
| Points, count x 20 | 20 | vec3f position, float `pointLength`, int32 `tag` |
| Payload count | 4 | int32 |
| Payloads, count x 72 | 72 | float `speedMS`, `gas`, `brake`, (one float skipped), `radius`, `sides[0]`, `sides[1]`, `camber`, `direction`, vec3f `normal`, float `length`, vec3f `forwardVector`, (one float skipped), float `grade` |
| Grid | variable | `InterpolatingSpline::loadGrid` 0x1401f31a0: the 2D lookup grid for closest-point search |

After loading, `forwardVector` is recomputed from consecutive points; `grip`, `distFromCorner`,
`distFromNextCorner`, `isPitlane`, `compression` are not in the file (filled later or left at defaults).
Version 6 and older use a different per-point layout (`loadVersion6`, not decoded) and are upgraded on load.

## 5. Size

`python tools/re_query.py size AIDriver AIDriverDrag AISpline AISplineRecorder RaceEngineer SplineLocator`:
**226 hand-written functions, 114,475 code bytes.**

| Object (ksPhysicsAC.lib) | Functions | Code bytes |
|---|---|---|
| AIDriver.obj | 65 | 51,615 |
| AISpline.obj | 42 | 25,201 |
| AISplineRecorder.obj | 30 | 17,163 |
| RaceEngineer.obj | 51 | 13,356 |
| AIDriverDrag.obj | 24 | 5,184 |
| SplineLocator.obj | 14 | 1,956 |

Not counted here: FormAI.obj (SystemApps.lib, 47 functions, 32,523 bytes: UI area), the `kml.lib` splines
(`track.md`), `FuelLapEvaluator`, `AutoShifter` (car physics maps).

## 6. Port difficulty: **L**

- The framework is easy: one interface method, a rate divider, file loaders with a simple binary format.
- The bulk is tuned heuristics: `AIDriver::AIDriver` (8 kB), `stepCarsAwareness2` (4 kB), `stepGasBrake` (3.2 kB),
  `getLABraking`, `stepSuperhuman`. They read many car internals (tyre slips, loads, engine, wings, ERS) and write a
  few (`Tyre::aiMult`, body forces, setup items, compound), so the AI can only be finished after the car physics.
- Bit-exact parity is needed if the recorded AI lap (`f2004_spa_ai.csv`) is to be reproduced: the AI closes the loop
  through the physics, so any difference in the controller diverges within seconds. `ksRand` use in the humanizer and
  start-time selection means the random stream must match too.
- `RaceEngineer` and `SplineLocator` are small, shared with non-AI systems and should be ported early with the car.
- The recorder (`AISplineRecorder::step` / `save`, `buildSides`, CSV sides) is a developer tool: skip at first and
  only load existing `.ai` files.
- No third-party SDK. No crate replaces it; `byteorder` or plain `from_le_bytes` covers the file format.
- Depends on: car physics (all of it), `kml.lib` splines (`track.md`), `Track` ray cast, session info
  (`SessionInfo`, laps, start time), pit-stop logic.

## 7. Open questions

- `genome[20]` is printed (`AI GENOME:`) in the constructor; how it is generated and what each gene scales was not read.
- The exact `initSetup` search path (order of car / track parts between `setups/` and `ai_default.ini`) was not decoded.
- `[ULTRA_GRIP] VALUE` in `ai.ini`: the constructor calls `setUltraGrip(1.2)` with a constant; whether the `FormAI`
  closure at 0x1402df8b0 reads or writes the ini key was not checked.
- The two skipped floats in the version-7 payload are discarded by the loader; their meaning in the writer
  (`AISpline::save`) was not read.
- `stepSuperhuman`'s body force: condition is `physicsGUID != 0 || useMagicForces`; magnitude and purpose
  (interpretation: stability / braking help for AI cars) were not studied.
- Online: whether an `AIDriver` can exist on a client (server-side AI is not a stock feature) was not checked.
- Which thread runs the `RaceManager` closure that installs the driver is taken from the `executeOnPhysicsThread`
  pattern described in `main_loop.md`, not traced here.
- Choice made: `SplineLocator` and `RaceEngineer` are counted here although they also serve non-AI code; FormAI is
  left to the UI area.
