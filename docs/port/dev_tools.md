# AC's developer windows and shared memory (Task 43c)

## Resume here

State after the last commit (update after every commit):

- **Step 1, shared memory, done and proven:** the graphics page is the game's
  `SharedMemoryWriter::update` (0x140186230) ported; the static page is
  `SharedMemoryWriter::writeStatic` (0x1401877c0) ported; the physics page was ported and proven
  before (Tasks 9-28, `car_oracle --table telemetry`). The car oracle compares the game's writer
  with the port's under `CAR_ORACLE_SHM=1`: the static page once, the graphics page after every
  frame of the referee (`sh tools/proofs/run_all.sh fresh` with `FRESH_PARTS=shm`, the list
  `tools/proofs/shm_list.txt`).
- **Step 2, the PerformanceMeter's other readers, done and proven:** the physics page's
  `performanceMeter`, the online car update's delta and the "recover time" penalty read the
  meter (`RollingChassis::performance_split`); recover time's start (`LapInvalidator::onEnterPenaltyZone`)
  was missing and is ported. Proofs: `rc_recover` with `CAR_ORACLE_PENALTY_MODE=2
  CAR_ORACLE_PERF_METER=1` (`tools/proofs/recover_list.txt`), the online `net_drive` with the meter on.
- **Step 3, the Telemetry app's 20 channels and Setup debug's focus, done:** every channel has
  its game source; the 12 that the game reads through a function of the car are proven in the car
  oracle (`CAR_ORACLE_TELEMETRY=1`, `tools/proofs/telem_list.txt`); the 8 skid channels come from
  the car's sound (already proven by the sound oracle); Setup debug reads the focused car.
- **Step 4, the developers' 3D windows:** Wings (`FormWings`) done and proven: its table in the
  GUI oracle (`app_wings`), its outlines and lift vectors in the scene oracle (`r43c_01`).
  Tyre tester (`TyreTesterForm` + `TyreTester`) done and proven in the GUI oracle
  (`app_tyretester`). Suspensions (`ACSuspension`) done and proven: the suspensions' debug
  lines, KPI and caster in the car oracle (`telem` runs), the window in the GUI oracle
  (`app_suspension`), its 3D lines and keys in the scene oracle (`r43c_02`).
- **Step 5, the readouts and the console:** the GPU profiler (`GPUProfiler` + `GPUProfilerApp`)
  done and proven (the window in the GUI oracle, `app_gpuprofiler`; its queries live in
  rustyac.exe); Render Stats' audio time and FMOD usage done. The console variables done: what
  each one changes in the game now changes in rustyAC (the scene's in the scene oracle, `r43c_03`
  to `r43c_06`; the physics ones as jobs of the physics thread).
- **Version:** v0.43.2 (tag `v0.43.2`). Leftovers go to Task 43d (see "Not done").
- **Checks at the release:** `sh tools/proofs/run_all.sh --quick`: 140 cases in 13 areas, all
  IDENTICAL (physics 9, fresh 51, sound 4, render 12, gui 19, python 1, replays 5, net 16,
  browser 9, ai_page 3, shaders 4, launcher 1, dumps 6; 43 min). Clippy native, wasm and wgpu
  clean.

## In plain words

Assetto Corsa publishes three blocks of shared memory that other programs read while you drive:
`acpmf_physics` (the car, every physics step), `acpmf_graphics` (laps, times, flags, position,
every drawn frame) and `acpmf_static` (the car and the session, once). SimHub, Crew Chief,
Content Manager's live apps, RaceLab, Z1 Dashboard, motion rigs and most telemetry tools read
them. rustyAC now writes all three the way the game does: the same layout, the same values and
the same timing (the graphics page once per drawn frame, after a 30-frame warm-up, the packet
counter going 1, 3, 5 ...). Every tool that reads AC's shared memory reads rustyAC's.

## Step 1: shared memory

### What the port does

- `crates/rustyac-physics/src/race/shm.rs` (new, platform free): the page layouts (moved from
  `rustyac-game`), `GraphicsWriter::update` (the game's update: the warm-up recording the car's
  position, the page cleared each write, `status` / `session` / lap strings / `split` / laps /
  `position` / `sessionTimeLeft` / `distanceTraveled` / `isInPit` (1.5 m from the pit box) /
  sector index and time / `tyreCompound` (the compound's long name) / car position and
  coordinates / `penaltyTime` / `flag` (from the first page on) / ideal line / pit lane / grip /
  wind (km/h) / time limit / ESC menu), `referee_frame` (what the referee knows of the player),
  and `static_page` with `StaticFacts` (writeStatic: `changelog.txt`'s first line, sessions,
  cars, names cut to 30 characters in all three name fields, track and layout, AI line length,
  sectors, penalties, the car's numbers taken when it is built, the online-only numbers only
  online).
- `rustyac.exe` (`crates/rustyac-game/src/shm.rs`): the graphics page is written once per
  referee frame (every third step), not every step; the static page from the sim's facts.
  Fixed on the way: the distance had a 2 m teleport cap the game does not have; `position`,
  `sessionTimeLeft`, `numberOfSessions`, `numCars`, `acVersion`, the three names, the layout,
  the skin, `penaltiesEnabled` and the online-only fields were constants.
- `EngineModel::get_max_torque_nm` (the game's `Engine::getMaxTorqueNM`): the static page no
  longer reads `engine.ini` again.

### The proof

The car oracle's game side builds a real `SharedMemoryWriter` on the referee's sim
(`tools/car_oracle/src/game/race.rs` `GameShm`) and calls the game's own `writeStatic` and
`update`; the port's recorder (`crates/rustyac-physics/src/engine/record.rs`) runs the port's.

- Stand-ins (what a running game would have and the oracle does not): the avatar's
  `physicsInfo` filled from the game's car as `CarAvatar::initPhysics` copies it (the game's own
  `Engine::getMaxPowerW` / `getMaxTurboBoost` / `Curve::getCount`), the drawn body matrix copied
  from the physics state each frame, the physics avatar's wind and session rates copied from the
  engine, the AI line loaded by the game's `AISpline::load`, `TrackAvatar::getMaxSlotsAvailable`
  patched to 0 (a map the track stand-in lacks; the static page does not use it), menus /
  drive aids / replay manager as a fresh game leaves them.
- The whole run has no game flag manager: the oracle's writer shows no flag, and the mapping of
  the 7 flags is checked separately (the game's writer on each flag 0..=6, header key
  `shm_flags`, against `Flag::shared_memory`).
- The static page (header key `shm_static`) must be the port's byte for byte, field by field.
- The graphics page: 76 words per recorded step (`world.shm.<field>+<byte>` columns).

| run (magione) | cars | steps | result |
|---|---|---|---|
| pit_box | 3 × bmw_m3_e30 | 30,000 | IDENTICAL, static page identical |
| pit_window_served (a 3-lap race with a pit window) | 3 × bmw_m3_e30 | 135,000 | IDENTICAL, race_out identical |
| rc_cut (time penalties, black flag) | 3 × bmw_m3_e30 | 100,000 | IDENTICAL; `penaltyTime` took 265 values |
| pit_box_wing | ks_ferrari_f2004 + e30 | 2,000 | IDENTICAL, static page identical |
| pit_box_wing | ks_ferrari_488_gt3 + e30 | 2,000 | IDENTICAL, static page identical |
| pit_box_wing | ks_audi_r8_plus + e30 | 2,000 | IDENTICAL, static page identical |

Fields seen changing in the race: packet id, status, session, the three lap strings, split,
completed laps, position, the three lap times, session time left, distance, pit flags, sector
index and time, laps, compound, car position, coordinates, penalty, mandatory pit done.

Rerun: `FRESH_PARTS=shm bash tools/proofs/run_all.sh fresh` (the quick sample:
`PROOF_QUICK=1`).

## Step 2: the PerformanceMeter's other readers

The game's `PerformanceMeter::getCurrentSplit` 0x14026b700 (the delta to the best lap) has five
callers: the Performance Delta app (Task 33), `Car::getPhysicsState` (Task 33), and the three
this step connects, which in rustyAC read a field that stayed 0:

- `SharedMemoryWriter::updatePhysics` 0x140186ef0: the physics page's `performanceMeter`;
- `ACClient::sendCarPosition` 0x140052230: the delta in every online car update (ms, ±30 s);
- `Car::step` 0x140275da0 in penalty mode 2, "recover time": `penaltyTime = penaltyPerfTarget - split`.

All now read `RollingChassis::performance_split()` (the meter, nothing on an open track that has
not started). The start of a recover-time penalty, `LapInvalidator::onEnterPenaltyZone`
0x1402c03d0's mode-2 branch, was missing: with a best lap in the meter, `Car::addPenalty(5)` and
`penaltyPerfTarget` = the split + 5 s (`InvalidatorAction::RecoverTime`).

Recover time is a mode the game never picks itself (`RaceManager::setCurrentSession` picks cut
gas for a race and lap invalidation otherwise; online also cut detection), so it can only be
reached by a mod or a patched game; it is ported because the code is there.

The car oracle's game side switches the meter off unless `CAR_ORACLE_PERF_METER=1`; the port's
replays of its recordings now do the same (`RunSetup::build`, `ReplayWorld::build` without the
header key `meter`), so every earlier recording replays as before.

| run | what it shows | result |
|---|---|---|
| `rc_recover` magione, 3 × e30, 140,000 steps, `CAR_ORACLE_PENALTY_MODE=2 CAR_ORACLE_PERF_METER=1 CAR_ORACLE_SHM=1` | a clean first lap (2:00.922) gives the meter a best lap; the cuts after 150 s give a recover-time penalty (5 s, page `penaltyTime` 5.0 at step 77,000), served and cleared; the meter's delta, every car's state, the referee and the graphics page at every step | IDENTICAL |
| `net_drive` on the tape `laguna_a`, 66,000 steps, `CAR_ORACLE_PERF_METER=1` | the game's `sendCarPosition` with its meter on against rustyAC's car updates, and the meter at every step | IDENTICAL (3,866 outgoing messages) |

New oracle pieces: `CAR_ORACLE_PENALTY_MODE=<n>` (header key `penalty_force`) writes a penalty
mode wherever the game writes one; the scenario `rc_recover` (rc_cut's race of 4 laps, cutting
from 150 s on).

## Step 3: the Telemetry app's 20 channels, Setup debug's focus

`FormTelemetry::initChannels` 0x140332ff0 reads these 20 through a function or a lambda of its
own; rustyAC showed 0 for them. Now (`crates/rustyac-game/src/view.rs` `telemetry_values`):

| channels | the game's source | rustyAC |
|---|---|---|
| `AI_PUSH`, `AI_LIVE_OFFSET`, `AI_DIST_CORNER` | the car's controls provider as an `AIDriver`: `getLateralPush() * 100`, `liveOffset` (+0x13c), `offsetDistFromCorner` (+0x140); 0 without one | the player's car's AI driver while it drives (`AiDriver::get_lateral_push` now public) |
| `ERS_TORQUE` | `ERS::getOutputTorque` through the ERS's torque-generator table | `Ers::get_output_torque` |
| `OVERSTEER_FACTOR` | `DynamicController::getOversteerFactor` | the same, already ported |
| `STEER_DEG_LF/RF/LR/RR` | `Car::getWheelSterAngleDEG` 0x140272960: the tyre's −z axis in the body's frame, `asinf(x / |(x, z)|)` in degrees | `RollingChassis::get_wheel_steer_angle_deg` (new) |
| `FF_FINAL` | `CarPhysicsState::lastFF_Final` = the device's `getFFGlobalGain` × `userFFGain` × `lastFF` | `RollingChassis::last_ff_final` (new) |
| `RIDE_HEIGHT_F`, `RIDE_HEIGHT_R` | `CarPhysicsState::rideHeight`: `Car::computeRideHeight` over the plane of three tyres, or a standing car with a minimum-height rule the `RaceEngineer` estimate | `RollingChassis::ride_heights` (the physics page's code, shared) |
| `SKID_VOL_*`, `SKID_PITCH_*` | the car's `CarAudioFMOD::skidVolumes` / `skidPitches` (+0x45c / +0x47c) | the player's car's sound (`CarAudio::skids`), handed to the app on the display thread (`GameGui::telemetry_skids`) |

The proof: under `CAR_ORACLE_TELEMETRY=1` (header key `telem`) every multi-car oracle row ends with
the 12 physics-side channels, the game's from its own functions on its car (`AIDriver::getLateralPush`,
`DynamicController::getOversteerFactor`, `Car::getWheelSterAngleDEG`, the ERS's table,
`Car::getPhysicsState`'s `lastFF_Final` and `rideHeight`), the port's from rustyAC's.

| run (magione) | channels that moved | result |
|---|---|---|
| `ai_handover`, ks_ferrari_sf15t, 20,000 steps (the AI takes the car at 7.2 s) | ERS torque 0..77 Nm, the AI's three, oversteer, steer angles, FF final, ride heights | IDENTICAL |
| `ai_handover`, ks_ferrari_f2004, 20,000 steps | AI push 0/100, live offset −0.44..0.78, distance to corner 0..347, oversteer −5.5..2.9, steer −9..11°, FF final −1.3..1.4, ride heights | IDENTICAL |
| `rc_cut`, 3 × bmw_m3_e30, 30,000 steps | oversteer, steer angles, FF final, ride heights (no AI on the player's car: 0 as in the game) | IDENTICAL |
| `pit_box_wing`, ks_ferrari_488_gt3 + e30, 6,000 steps | | IDENTICAL |

The skid channels are the sound's own values; the sound is proven by the sound oracle (Task 19
on), so only their hand-over to the app is new.

**Setup debug** (`SetupDebug::render` reads the focused car's `Car`): the physics thread now builds
`view::setup_debug(sim, focus)` from the car the display focuses (`Shared::setup_debug_focus`,
stored each frame), not always the player's. Test:
`crates/rustyac-game/tests/opponents.rs` `the_setup_debug_app_reads_the_focused_car` (an e30 with a
parked F2004: each focus shows its own car's gears and final ratio). The app itself was proven
against the game's in Task 33b (`app_setupdebug`, the focus moving to car 2 at frame 900).

## Step 4: the developers' 3D windows

### Wings (`FormWings`, 0x1c0)

The game makes it in `Sim::Sim` after `TyreTesterForm` and `FormLogoAC` (a developers' app, in the
Ctrl+M list). Two halves:

- **The table** (`FormWings::render` 0x1402edff0): a row per wing of the player's car (name, angle of
  attack + angle, ground height in mm, CL and CD areas, angle, yaw; fins in red), then the totals
  (CL area, the front's share of the lift in %, CD area, efficiency, downforce and drag in kg).
  `crates/rustyac-gui/src/app_wings.rs` (new); the wings' names and sizes are read once
  (`view::wing_rows`, `CarInfo::wings`, `GuiSetup::wings`), their states every frame
  (`CarView::wing_states`, at most 12: `view::MAX_WINGS`).
- **The 3D lines** (`FormWings::onRenderNode` 0x1402edc30, a handler of the render-finished node):
  while the app shows, every wing's outline (green; a fin yellow) and its lift vector (purple,
  `liftVector * 0.001` long) over `physicsState.worldMatrix`, depth test off.
  `crates/rustyac-render/src/wings.rs` (new); `AcRenderer` adds the handler with the GUI and feeds
  it each frame.

| proof | what | result |
|---|---|---|
| GUI oracle `app_wings` (`tools/proofs/gui_list.txt`) | the game's `FormWings` (its constructor and `render`) and the port's, four wings (three level, a fin; no lift for 30 frames: the share and the efficiency divide by zero), 228 frames, hidden and shown | command log, pixels, state, layout IDENTICAL |
| scene oracle `r43c_01` (`tools/proofs/render_list.txt`) | magione, chase view, the e30's lap tape, 30 frames: the game's `onRenderNode` through its own lambda added with `Event<OnNodeRenderEvent>::addHandler` to a stand-in form (visible byte, GUI → GraphicsManager, car), eight test wings whose lift turns each frame; shown, hidden at 20, shown at 25 | command log and pixels IDENTICAL in all 29 frames |

### Tyre tester (`TyreTesterForm`, 0x1f8, and `TyreTester`, 0x128)

Made in `Sim::Sim` before FormLogoAC and FormWings. Its constructor makes four `TyreTester`s, one
per tyre of the player's car, from `content/cars/<car>/data/`; each holds a tyre of its own on a
stand-in hub (turned by the slip angle, moving along z) and a stand-in ground (where the tyre's
spring holds the load). Every frame (not in a replay) the tyre the spinner names takes the car's
compound and speed (15 m/s below 30) and is swept three times, 200 tyre steps each: slip angle
-20..20 degrees, slip ratio -1..1, slip ratio at 9 degrees; the graph shows Fy, Fx, Mz, DY over
load, Fy at 9 degrees and the cambered DY; the line under it the peaks; a cyan line the friction
circle at 9 degrees.

- `crates/rustyac-physics/src/tyre/tester.rs` (new): `TyreTester` with `TesterHub`,
  `TesterGround` (the tyre's values snapshotted before the step: the step writes
  `pressureDynamic` only after its ray cast), the three sweeps; `VanillaTyre::get_cambered_dy`
  is now public. Test `crates/rustyac-physics/tests/tyre_tester.rs` (the F2004 from data.acd).
- `crates/rustyac-gui/src/app_tyre_tester.rs` (new): the form and `render`; its feed
  `AppLink::tyre_tester` (speed, the four loads, `CarView::compound_index`). rustyac.exe and the
  page make it from the player's data folder; no app when its tyres.ini cannot be read.

| proof | what | result |
|---|---|---|
| GUI oracle `app_tyretester` (`tools/proofs/gui_list.txt`) | the game's `TyreTesterForm` (constructor, `render`, the game's own `TyreTester` and `Tyre`) and the port's on the F2004's tyres (loose tyres.ini and curves in the oracle's root), 32 frames: speed under and over 30 m/s, four loads changing, compounds 0 then 1/2, the spinner clicked up to 3 and back, hidden and shown | command log, pixels, state (the line of peaks in every frame), layout IDENTICAL |

### Suspensions (`ACSuspension`, 0x288)

Made in `Sim::Sim` after FormWings. The window: four camber/slip gauges with the toe and the
steering, each with a progress bar of the tyre's slip (yellow, red at 1 and over); the caster,
KPI and scrub radius; the rear-left bump steer as a strip (toe over the wheel's height, recorded
while the window shows); the tyre temperatures as 12 x 3 coloured squares with their sums; red
"Tyre off" lines (how far each wheel node is from its tyre). Its handler on the render-finished
node draws in 3D: every suspension's links (`ISuspension::getDebugLines`), the ride pickup
points, a cyan plate under the car, optionally each wheel's two rims and a line from its node,
and the fuel tank's cross; Backspace switches the links, Ctrl the fuel tank, keypad 1-4 the
wheels.

- `crates/rustyac-physics/src/car/suspension*.rs`: `ISuspension::get_debug_lines` for the
  double wishbone, strut, multilink and axle (`DebugLine`, `transform`), `upright()` and
  `kpi()` (`RaceEngineer::getKPI_RAD`).
- `crates/rustyac-gui/src/app_suspension.rs` (new): the window (`SuspensionFacts` from
  `CarAvatar::initPhysics`'s physicsInfo and car.ini `[FUELTANK] POSITION`; the wheel nodes in
  `AppLink::suspension`). The TempRun button only changes its text (below).
- `crates/rustyac-render/src/suspensions.rs` (new): `SuspensionWireframe`, the handler
  (`renderWireframe`); rustyac.exe registers it after the Wings one and feeds it while the app
  shows, with the keys from `GetAsyncKeyState`.

| proof | what | result |
|---|---|---|
| car oracle `CAR_ORACLE_TELEMETRY=1`, `pit_box_wing` 3000 steps (`tools/proofs/telem_list.txt`) | the KPI, scrub, front caster and every wheel's debug lines (count and 10 floats a line) each step, against the game's `getKPI_RAD`, `getFrontCasterRAD`, `getDebugLines`: 488 GT3 (double wishbone), E30 (strut, axle), Alfa GTA (double wishbone + axle), the multilink test car | IDENTICAL |
| GUI oracle `app_suspension` (`tools/proofs/gui_list.txt`) | the game's `ACSuspension` (constructor, `render`, `renderCamber`, `renderTyreTemp`) and the port's, 348 frames: E30-like facts, the tape's slips, temperatures and wheel nodes moving off their tyres, hidden and shown | command log, pixels, state, layout IDENTICAL |
| scene oracle `r43c_02` (`tools/proofs/render_list.txt`) | the game's handler and `renderWireframe` (its `susGL`) over the E30 on Magione, 29 frames, with test debug lines and keys: links off, wheel 1, wheels 3 and 4, fuel tank off, hidden, shown, links on | command log, pixels IDENTICAL (5366 to 5545 calls a frame) |

## Step 5: the GPU profiler, Render Stats' audio

### GPU profiler (`GPUProfilerApp`, 0x1c0, and `GPUProfiler`, 0x30)

A developers' app made after ACDCarEngineer. The first time it is drawn it switches the
profiler on for the rest of the run (as in the game): then one frame in 32 gets a D3D11
timestamp query at the end of each pass, in the order the names are first met (`BEGIN_FRAME`,
`SHADOW_MAP`, `CUBEMAP`, `MAIN_PASS`, `TRANS_PASS`, the blur and YEBIS passes when they run,
`GUI`, `END_FRAME`, `PRESENT`), read back 32 frames later without waiting. The window lists
each pass's milliseconds since the one before, its share of the frame and the frame's time.
The first row shows its own timestamp (the game's quirk: a huge time and percent).

- `crates/rustyac-render/src/gpu_profiler.rs` (new): `GpuProfiler` (`beginFrame`, `endFrame`,
  `fetchResults`, `triggerQuery`); `Graphics::gpu_profiler`, made just before the GL renderer
  as `GraphicsManager` does (the disjoint query); `begin_scene` / `end_scene` /
  `Graphics::trigger_query`, and the markers at the game's places in `forward.rs`,
  `camera.rs`, `cubemap.rs`, `post.rs` and after the GUI in rustyac.exe; its window present
  (`AcRenderer::present`) is the game's `endScene` here.
- `kgl.rs`: `new_query`, `query_begin`, `query_end`, `query_frequency`, `query_timestamp`
  (`GetData` without waiting; `S_FALSE` is not ready, as the game tests `== 0`). The wgpu
  backend has none (the page's profiler stays empty).
- `crates/rustyac-gui/src/app_gpu_profiler.rs` (new): the window (`AppLink::gpu_profile`, and
  `gpu_profiler_active` for `setActive(true)`).

### Render Stats' audio time and FMOD usage

`Game::stats.renderAudioTime` is now the milliseconds of rustyac.exe's sound frame (the game
times `renderAudio` + `AudioEngine::update`; rustyAC's frame also copies the cars' sound
inputs), cut to a float; FMOD USAGE is Studio's CPU percent plus the update's
(`AudioEngine::getPerformanceStats`), read once a frame. Both show one frame late, as in the
game; without sound they stay 0.

| proof | what | result |
|---|---|---|
| GUI oracle `app_gpuprofiler` (`tools/proofs/gui_list.txt`) | the game's `GPUProfilerApp` over its real `GPUProfiler` (rows written into it, kept off) and the port's, 163 frames: no rows, one row, eight rows at one time (nothing drawn), eight moving, hidden and shown | command log, pixels, state, layout IDENTICAL |
| scene oracle, 10 frames of Magione (the startup query) | the port's new disjoint query at the start | command log, pixels IDENTICAL |
| rustyac.exe, `--dev-apps --gui-do app:gpuprofiler` on Magione | the window with the eight passes and real GPU times; Render Stats with FMOD USAGE 1% | seen |
| GUI oracle `app_stats` (Task 33) | the audio time and FMOD lines as drawn (fed 0.8 ms, 1.6 %) | unchanged, IDENTICAL |

The live numbers themselves cannot be compared with the game (they are timings).

### The console's variables

Task 33 registered every variable in the game's order; most changed nothing. Now a `set` does
what the game's target, callback or lambda does (`ConsoleSet::execute` takes the first
registration of a name, so a car's variables reach the player's car, its high-detail driver):

| variables | the game's effect | rustyAC |
|---|---|---|
| `debugDamage` | `VisualDamageManager`'s debug zones from then on (6 flips all five, 0-4 one) | `VisualDamageManager::debug_damage` |
| `hideHead`, `headRoll` `headPitch` `headYaw` `headMaxG` `headFilter` | the driver's head hidden outside; its tilt with the g-forces | `DriverModel` fields |
| `RPM_*`, `SPEED_*`, `FUEL_*`, `wATER_STEP` `WATER_*` | the needles' step, zero, minimum (`TURBO_*` point at a dead stack local: nothing) | `AnalogInstruments::set_var` |
| `debugDirt` | the dirt level, pushed when it next changes | `DynamicCarEffects::current_dirt_level` |
| `graphicsOffset.x/y/z`, `graphicsPitchRotation` | the body's offset and pitch (`Console::onSetVar`) | `CarAvatar` fields |
| `sunHeading`, `sunPitch` | the sun's angles, then `updateLightingSetttings` | `Graphics::lighting` |
| `hideGroove` | the groove stays off after the next cube map's faces | `Graphics::groove_always_hide` |
| `maxLayer`, `shadowBias`, `mirrorpos.x/y/z` | the scene camera's layers, its shadow bias, the mirror's origin | the same fields |
| `restrictor`, `bodyDamageLevel0-4`, `engineDamageLevel` | physics-thread jobs: `Car::setRestrictor`, a zone's damage only up, the engine's life only down | `SetupJob`s (kept in a drive's input file) |
| `aiSteerGain` `aiPush` `aiLookAhead` | car 0's AI driver, when it has one | `SetupJob::SetAi` |
| `set AUTOFLIP_RECOVERY` | the options' map, read by `resetInvalidStateCarsToPits` | `SetupJob::SetAutoflipRecovery` |

rustyac.exe also read options.ini's `AUTOFLIP_RECOVERY` nowhere: its referee never put a car on
its wheels back in the pits. It now reads it (1 without the file), as `OptionsManager` does.
No effect in the game, none here: `aiLookAheadSpeed` (empty lambda), `minExposure` (never read),
the `hdr*` levels (`CameraForward::hdr` is never made), `bias0-2` (the camera sets the bias
again every frame), `PROVIEW_MODE` / `DISABLE_SHOW_MODE` (read once at the start),
`RENDER_SPLINE`.

| proof | what | result |
|---|---|---|
| scene oracle `r43c_03` (Magione, chase, 12 frames) | `debugDamage 6`, `hideHead`, the body offset and pitch, the sun, `shadowBias`, `maxLayer 0`, `debugDirt 2`, the mirror's origin, one after another; the game's side runs its own lambdas from the console its cars registered into | command log, pixels IDENTICAL |
| scene oracle `r43c_04` (dash view) | the needles' twelve variables, `lodDebug` | IDENTICAL |
| scene oracle `r43c_05` (chase, 1.2 g) | the head's five | IDENTICAL |
| scene oracle `r43c_06` (Spa, two cube map faces a frame) | `hideGroove` on, then off | IDENTICAL |
| `input_file` test `jobs_round_trip`, `cockpit` test `the_console_writes_the_needles_fields` | the new jobs in a drive's file; the needles' names | pass |
| rustyac.exe on Magione: `set graphicsOffset.y 0.4`, `sunHeading 200`, `bodyDamageLevel0 200`, `engineDamageLevel 0` | the body up, the sun moved, the front red in the damage display, the engine dead (smoke, standing) | seen |

## Not done

- The console's `axisMode` (the world's axes drawn by `Camera::renderAxis`), `lodDebug`'s red
  text (`CarLodManager::renderHUD`), `editBoxes` / `editCMesh` (collider editing) and the four
  `observe*` (live reloading of ini files), `exposure` (the cockpit camera's YEBIS exposure,
  which rustyAC's camera does not read): not ported. The scene oracle's game side does not
  make `TrackAvatar` or `Console::initCommands`, so the sun, `maxLayer`, `shadowBias`, the mirror
  and the body offset are done there as their owners do (the same fields, the game's
  `updateLightingSetttings`).

- ACSuspension's TempRun button: the game's handler queues a job on the physics thread that
  writes `TempRuns/<car>@<track>.trun` into the game's folder (speed, wheel speeds and brake
  torques each step); rustyAC writes nothing there (the write guard), so the button only
  changes its text. The GUI oracle does not click it (its sim has no physics thread: the
  game's side crashes).
- In the GUI oracle the game's tester read nothing from a copy of the F2004's `data.acd` in the
  oracle's root (zero forces, other DY), so both sides read loose files there; rustyAC reads
  `data.acd` as the game does elsewhere (`crate::data::read`), and the test above runs on it.
- Telemetry `RIDE_HEIGHT_*` in rustyac.exe take this step's speed for the previous snapshot's (the
  2 m/s edge of the minimum-height rule may be one step late); online remote cars have no `Car` in
  rustyAC, so Setup debug shows the player's car when one is focused.
- Online, the tape's local car may not finish a timed lap in its 198 s, so the delta it sends
  may stay 0 in `net_drive`; the meter's value itself is proven in `rc_recover`.
- The online-only static fields and the graphics page while online or in a replay are ported
  from the code but not run in the oracle (its sim has no client and no replay).
