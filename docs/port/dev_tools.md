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
- **Steps 3-5:** not started.
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

## Not done

- Steps 3-5 (see the task's prompt).
- Online, the tape's local car may not finish a timed lap in its 198 s, so the delta it sends
  may stay 0 in `net_drive`; the meter's value itself is proven in `rc_recover`.
- The online-only static fields and the graphics page while online or in a replay are ported
  from the code but not run in the oracle (its sim has no client and no replay).
