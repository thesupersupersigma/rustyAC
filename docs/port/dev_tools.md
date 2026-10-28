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
  (`app_tyretester`). ACSuspension: see the step's section.
- **Step 5:** not started.
- **Version:** still v0.43.1.

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

## Not done

- Step 4's ACSuspension and step 5 (see the task's prompt).
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
