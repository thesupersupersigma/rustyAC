# Task 12: the first real track (Spa)

## Resume here

State after the last commit (kept up to date with every commit):

- **Everything of Task 12 is built, checked and committed; this report is complete. Nothing is half done.**
- Branch `feat/track-spa` (made from `chore/license-files`, the branch that was checked out). Nothing is pushed.
  The `.gitignore` change in the working tree is not mine and is not committed.
- What exists: `crates/rustyac-content` (kn5 reader), `crates/rustyac-ode/src/{opcode,collision}.rs` (tree build
  and the ray), `crates/rustyac-physics/src/track/` (surfaces, loader, AI line, timing, spawn), the car's hooks in
  `car/chassis.rs`, the oracle in `tools/car_oracle/src/{track,track_driver}.rs`, the comparison in
  `tools/chassis_compare`, and the game side in `crates/rustyac-game` (`--track`, `--spawn`, models, HUD, shared
  memory, `--autodrive`).
- Checks to run again: section 8. All pass at the last commit.
- The big game recordings (`oracle/track/*.carrec`, 812 MB, git-ignored) were **deleted** after the last
  comparison; section 8 says how to make them again (about 3 minutes). The small files stay: the golden excerpts
  in `crates/rustyac-physics/tests/golden/track_spa_*.chgold` (committed), `oracle/track/rays_results.md`,
  `oracle/chassis/results_track_whole.md` and the input files `oracle/game/track_spa_*.ryin` (git-ignored).
- Scratch of this task (git-ignored): `re/scratch/task12/` (`spec_*.md` are the five briefs read from the
  disassembly, each with a review log; `review_*.md` the three code reviews; `patch_*.py` the patch scripts).
- **Not started, on purpose:** the car body touching anything (ODE contacts, Task 13) and AC's real renderer.
  If work continues, sections 6 and 7 are the list.

## 1. Plain-English summary

**The F2004 can be driven around Spa in `rustyac.exe`.** The road, kerbs, grass and sand are Spa's own, read
from the game's files in your Assetto Corsa folder (nothing of the game is copied into the repository). Laps
and sectors are timed, a lap is spoiled when you leave the track, and the lap fields of AC's shared memory are
filled, so `ac_telemetry.py --laps` works on rustyAC as it does on AC.

What was built:

- **A reader for AC's model files (`.kn5`)**. It reads the structure of the 441 MB `spa.kn5` in about 20 ms
  and only fetches the parts that are asked for. (Spa is six files: the physics needs only the 455 meshes and
  588,295 triangles of the 75 MB `3.kn5`; the 2.4 million triangles of `spa.kn5` are for the picture.)
- **The part of the game's collision library that the tyres use**: the search tree the game builds over every
  mesh, and the ray each tyre shoots down at the road every step. This is ported from the machine code of
  `acs.exe`, including the odd things it does (inside one mesh it takes the first triangle it finds, not the
  nearest).
- **The track itself**: which meshes are physical and what surface each is (grip, kerb vibration, "is this
  still track?"), the start positions, the timing gates, the AI line that says how far round the lap you are,
  the lap timer and the rule that spoils a lap.
- **The picture**: the track's and the car's real 3D models with their textures in the debug view (one sun,
  no shadows), a lap panel in the HUD, and the surface under each tyre.

What was proven:

- **The ray.** One million rays were shot at Spa twice: by the game's own code (run inside our test program)
  and by the Rust port. **All 1,000,000 answers are identical to the last bit** (702,347 hits).
- **The car on the track.** The game's car was driven by a script over six stretches of Spa (the start, Eau
  Rouge and Raidillon, the chicane's kerbs with a cut, a minute of a lap, two wheels on the grass, all three
  timing lines). The Rust car, given only the same pedal and wheel inputs (and, in the timing drive, the
  test's jumps from line to line), did **exactly the same on all 58,069 steps**: every number of the car, every tyre ray, the surface under every tyre, the position along
  the lap, the lap and sector times, the cut count. Zero differences.
- **The program.** `rustyac.exe` replaying those drives gives the same states again (5 of 5).
- **Against the real game, not just its code.** An older telemetry recording of the real game on Spa (AC's AI
  driving the F2004 for three laps) was laid over the Rust track: the Rust road is under the game's car to
  within **3 mm on average** (worst 47 mm) at all 16,431 recorded positions, the position along the lap agrees
  to about half a metre, and the lap clock restarts where the Rust start line is.
- **A whole lap.** A simple built-in driver (`--autodrive`) laps Spa in 2:09.1; `ac_telemetry.py --laps 1`
  followed it through shared memory and stopped by itself when the lap was done.

What is not there (next task): **the car's body touches nothing.** Only the tyres feel the track. You can
drive through walls and barriers, and a car that rolls over sinks into the road (it is then put back on the
road by itself after 3 seconds). And nobody has driven it by hand yet: every check was done without a driver.

## 2. How to drive Spa

Build once (about 2 minutes the first time), then run from the repository folder:

```
cargo build --release -p rustyac-game
target\release\rustyac.exe --track spa --windowed
```

Without `--windowed` the window is borderless over the whole screen. Loading takes about one second. The game
is found in Steam's usual folder; if it is elsewhere, set `AC_ROOT` to the Assetto Corsa folder. (`--track` also
takes a full path to a track's folder, but the car's 3D model is looked for under `AC_ROOT`: without it the car
is drawn as boxes.)

| Option | Does |
|---|---|
| `--track spa` | Spa. Any other track folder name works the same way; only Spa was tested (section 6) |
| `--spawn hotlap` (default) | start at the hot-lap start, about 430 m before the line; the lap clock is set back to zero when you cross it (until then it counts from the spawn, as in the game) |
| `--spawn pit` / `--spawn start` | start in the first pit box / on pole position |
| `--flat` (or no `--track`) | the endless flat road of Task 11 |
| `--camera cockpit` | start in the cockpit (`C` changes camera while driving) |
| `--boxes` | draw the car as boxes instead of its 3D model |
| `--texture-size 512`, `--no-textures` | smaller textures / flat colours, if the graphics card is short of memory |
| `--autodrive` | nobody drives: a simple line follower laps the track (to watch, or for checks) |

Driving is as in Task 11 (`docs/game/first_drive.md`: pad, keyboard, wheel, your AC `controls.ini`). New or
changed on a track:

| Key / button | Does |
|---|---|
| `R`, or a tap on the pad's Back button | back to the start position; the lap timer starts again, the lap list stays |
| `Shift+R`, or holding Back for 0.6 s | back **onto the track where you are**: the nearest point of the racing line, facing along it, standing. The lap in progress is marked as cut (INVALID); and, by the game's own teleport rule, a lap that was already past its first sector line is not counted at all |
| `N` | a brand-new car (this was `Shift+R` in Task 11) |

The HUD's new panel at the top shows the running lap time (red when the lap is spoiled, with the number of
cuts), **VALID / INVALID**, the lap number and how far round the lap you are in percent, the last and the best
lap, and the three sectors (finished ones with their time). Under each tyre's load it names the surface the
tyre is on (`ASPH-SPA_BLACK`, `KERB`, `GRASS`, `SAND` ...).

Things to know:

- **Walls do nothing yet.** The body is a ghost until Task 13.
- A lap is spoiled when **more than two tyres** are off the track (the game's rule with penalties on). A
  lap with a cut is listed as "last" but never becomes "best".
- If the car lies on its side or roof for 3 seconds, or falls out of the world, it is put back onto the track
  where it was (the same as `Shift+R`, so that lap is marked as cut too).
- Telemetry: start `python ac_telemetry.py --laps 3 --print --out my_laps.csv` in another terminal, as with AC.
- Pictures made off screen by the program itself: `docs/game/spa_la_source.png`, `spa_eau_rouge.png`,
  `spa_raidillon_cockpit.png`. To make one: `rustyac.exe --track spa --autodrive --screenshot shot.png --at 38`.

## 3. What is ported

Addresses are of `acs.exe` 1.16.4, as in the comments of the code (a few only in the briefs under
`re/scratch/task12/`). What is checked against the game's own code is in section 4.

**`crates/rustyac-content` (new; own work, MIT / Apache)**

| File | What | From |
|---|---|---|
| `src/kn5.rs` | The kn5 container: textures, materials, the node tree, meshes (44-byte vertices, 16-bit indices). Reads the structure once and fetches vertices, indices and textures on demand. Version 2 and later (shipped content is 5 or 6); version 1 is refused | `KN5IO::load` 0x1402151a0 (the format; the game's reader builds Direct3D objects, this one does not) |
| `src/track_files.rs` | Which kn5 files make a track: `models.ini` / `models_<layout>.ini`, or `<track>.kn5` | `TrackAvatar::init3D` 0x1401c8740 |
| `src/bin/kn5_info.rs` | `kn5_info <file>`: what is in a kn5 | |

**`crates/rustyac-ode` (stage 3, part 1; BSD-3)**

| File | What | From |
|---|---|---|
| `src/opcode.rs` | The OPCODE tree over a mesh as the game builds it: one triangle per leaf, cut along the axis on which the triangles' centres vary most, at the mean; stored without leaves and not quantized | `Opcode::Model::Build` 0x1403903b0, `AABBTree::Build` 0x140395ff0, `AABBTreeNode::Subdivide` 0x140396250, `::Split` 0x1403961a0, `AABBTreeOfTrianglesBuilder::ComputeGlobalBox` 0x1403982d0, `::GetSplittingValues` 0x140398800, `AABBNoLeafTree::Build` 0x140396c30, `MeshInterface::IsValid` 0x14038ffe0 |
| `src/collision.rs` | Static meshes in ODE's simple spaces and a ray against them: the boxes, the walk through the spaces from the newest mesh, the ray against one mesh (first front-facing triangle found), the nearest answer over all meshes | `dGeomTriMeshDataBuildSingle` 0x14034b170, `dxTriMeshData::Build` 0x14034a780, `dxTriMesh::computeAABB` 0x14034ae10, `dxSpace::computeAABB` 0x140342d40, `dxRay::computeAABB` 0x140344a60, `collideAABBs` 0x140342c60, `dGeomRaySet` 0x140345e50, `dSpaceCollide2` 0x1403430a0, `dxSimpleSpace::collide2` 0x140342a60, `dCollide` 0x140344120, `dCollideRTL` 0x14038aa90, `RayCollider::InitQuery` 0x1403670b0, `::_SegmentStab` 0x14036aef0, `PhysicsCore::rayCast` 0x1402cd070, `rayNearCallback` 0x1402cd210, `PhysicsCore::getStaticSubSpace` 0x1402ccac0, `CollisionMeshODE::CollisionMeshODE` 0x1402cebb0 |

**`crates/rustyac-physics/src/track/` (new; ported from acs.exe)**

| File | What | From |
|---|---|---|
| `surfaces.rs` | The surface types (`system/data/surfaces.ini` and the track's own `data/surfaces.ini`), and which surface a mesh is by its name: a number, then a surface key (`1ROAD`, `01KERB_3` ...) | `SurfacesManager::SurfacesManager` 0x1401ae9b0, `::loadSurfaceDefinitions` 0x1401afad0, `::getSurface` 0x1401af340, `TrackAvatar::getSurfaceDescFromMeshName` 0x1401c8300, `NKUtils::getSectorID` 0x14018d2b0 |
| `loader.rs` | Loading a track: every mesh whose name starts with a number other than 0 becomes a collision mesh in the sub-space of that number (walls get 10000 more), with the vertices exactly as stored; the helper nodes (`AC_...`) | `TrackAvatar::TrackAvatar` 0x1401c5250, `::initPhysics` 0x1401ca440, `::processPhysicsNode` 0x1401cc5e0, `::addPhysicsMesh` 0x1401c78e0, `Node::getWorldMatrix` 0x14020e190 |
| `mod.rs` | `Track`: its meshes with their surfaces, the tyres' ray, the track-wide grip (`DynamicTrack`) | `Track::Track` 0x140277100, `::addSurface` 0x140277e50, `::rayCast` 0x140278bb0, `::createRayCaster` 0x1402781f0, `RayCaster::rayCast` 0x1402cee90, `Track::initDynamicTrack` 0x140278300, `::step` 0x140278d20, `::setGripLevelExternal` 0x140278d00 |
| `spline.rs` | The AI line (`ai/fast_lane.ai`, version 7, with its lookup grid), the Catmull-Rom spline, and the car's position along it every step | `AISpline::loadFast` 0x1402a83c0, `loadVersion7` 0x1402a8b80, `AISpline::payloadAtPosition` 0x1402a90a0, `InterpolatingSpline::computeSplineLength` 0x1401f1db0, `::calculateCatmullRom` 0x1401efde0, `::worldToSpline` 0x1401f3bf0, `::splineToWorld` 0x1401f3a20, `::closestPointIndex` 0x1401f0d80, `::loadGrid` 0x1401f31a0, `::getLastIndexFromNorm` 0x1401f2b90, `::getSignedDistanceFromSpline` 0x1401f2df0, `Spline::closestPointIndexWithBounds` 0x1401eda10, `SplineLocator::step` 0x1402ab5c0, `::locateOnSpline` 0x1402ab2f0, `::getSides` 0x1402ab130, `::reset` 0x1402ab520, `Track::initAISpline` 0x1402782a0, `Track::getSector` 0x140278220 |
| `timing.rs` | Timing gates, the lap timer, the rule that spoils a lap, the lap list, the spawn sets | `TimeLine::TimeLine` 0x1402cadb0, `::check` 0x1402caef0, `Track::addTimeLine` 0x140278040, `TrackAvatar::initTimeLines` 0x1401cbc90, `TimeTransponder::step` 0x1402911f0, `::onTimeLinePassed` 0x140290c20, `::lap` 0x140290a50, `::split` 0x140291150, `::invalidate` 0x1402908f0, `::reset` 0x140290e60, `LapInvalidator::step` 0x1402c0580, `::onEnterPenaltyZone` 0x1402c03d0, the lap list after `onLapCompleted` 0x140145300 and `onSectorSplit` 0x140145940, `TrackAvatar::initRespawnPositionSet` 0x1401cb8c0, `::getSpawnPosition` 0x1401c8200 |
| `src/bin/track_info.rs` | `track_info <track folder or name>`: what the loader finds; `--telemetry <csv>` lays a recording of the real game over the track | |

**The car (`crates/rustyac-physics/src/car/chassis.rs`)**

- `set_track`: what `Car::Car` 0x14026bf00 does with the track (the lap timer, the spline locator, the car's
  half width); with a track set, the lap invalidator runs too.
- The step order of `Car::stepComponents` 0x1402764d0: the lap invalidator, then the spline locator, then the
  stability control, then the lap timer. `Car::postStep` 0x140275430 gets a time step of **zero** from its
  caller, so the "side velocity" it computes is infinite or not-a-number in the game; the port does the same.
- `Car::forcePosition` 0x14026fe10 / `forceRotation` 0x140270040 with the lap-spoiling variant.
- A tyre's ray is 3 m long on a real track and 2 m on the flat road (`has_ray_caster`), as in the game.

**The game (`crates/rustyac-game`; own work, GPL)**

- `sim.rs`: finding the track, the spawn (the session's spawn set is dropped onto the road by
  `TrackAvatar::initRespawnPositionSet` 0x1401cb8c0, a 100 m ray from 10 m above each helper node; the car is
  put there as `CarAvatar::goToSpawnPosition` 0x1400d3ae0 does), the session's rules
  (`RaceManager::setCurrentSession` 0x140140df0), the armed first lap of a hot-lap session
  (`CarAvatar::armFirstLap` 0x1400d2300), reset and back-on-track.
- `shm.rs`: the lap fields after `SharedMemoryWriter::update` 0x140186230: `completedLaps`, `iCurrentTime`,
  `iLastTime`, `iBestTime` and their texts, `split`, `currentSectorIndex`, `lastSectorTime`,
  `normalizedCarPosition`, `carCoordinates`, `isInPit`, `isInPitLane`, `session`, `penaltyTime`; in the static
  page `track`, `sectorCount`, `trackSPlineLength`.
- `render/models.rs`, `render/dds.rs`, `render/mod.rs`: kn5 models in the debug view. Textures: DXT1/3/5, BC4,
  BC5, BC7 and plain 16/24/32-bit DDS, cut down to 1024 px on the long side. Meshes outside the picture, outside
  their own LOD range or too small to see are skipped. Cut-out materials (trees, fences) and blended ones are
  handled; the car's paint comes from AC's detail texture.
- `render/hud.rs`: the lap panel and the surface names. `autodrive.rs`: the line follower.

## 4. Results

### 4.1 The ray: 1,000,000 rays, the game's code against the port

`car_oracle rays --track spa --count 1000000 --seed 12`. The game's own `Track`, ODE and OPCODE (run inside the
test program, from `acs.exe`) and the port are given the same meshes and the same rays. An answer is identical
when both say "no hit", or both hit the same mesh and the hit position and normal have the same bits.

| Kind of ray | Rays | Hits (game) | Identical answers | % |
|---|---|---|---|---|
| tyre ray (3 m straight down from above a random triangle) | 375,000 | 336,537 | 375,000 | 100 |
| tyre ray aimed exactly at a mesh vertex | 62,500 | 57,303 | 62,500 | 100 |
| tyre ray aimed exactly at the middle of a triangle's edge | 62,500 | 54,788 | 62,500 | 100 |
| spawn ray (100 m straight down from 10 m above) | 62,500 | 59,687 | 62,500 | 100 |
| ray in a random direction, 0.5 to 100 m long | 187,500 | 121,570 | 187,500 | 100 |
| nearly flat ray along the road, 5 to 200 m long | 125,000 | 72,204 | 125,000 | 100 |
| ray from below (the back of the road) | 62,500 | 128 | 62,500 | 100 |
| tyre ray anywhere inside the track's bounding box | 62,500 | 130 | 62,500 | 100 |
| **all** | **1,000,000** | **702,347** | **1,000,000** | **100** |

Spa has 455 physics meshes (588,295 triangles, 45 sub-spaces); every one of them was hit at least once. The
game's own surface for each of the 455 mesh names against the port's: 0 differ. Building the meshes and trees:
game 2.7 to 3.8 s, port 0.6 s. The million rays: game 5.0 s, port 4.1 s.

### 4.2 The car on Spa: the game's car against the Rust car, free running

`car_oracle run --track spa --scenario <name> --out oracle/track` drives the game's car (the game's own `Car`,
tyres, `Track`, timing and AI line, with the body a ghost) and records every step. `chassis_compare run --dir
oracle/track` then runs the whole Rust car on the Rust track with **only the driver's controls** as input (in
`spa_timing` also the test's seven jumps from line to line) and compares after every step: 2,009 values of the chassis (bodies, joints, tyres with their rays: hit, position,
normal, mesh), 403 values of the other systems (brakes, engine, drivetrain, wings, aids, the telemetry page,
the lap timer, the position on the AI line, the cut counter), and every force the game applied.

| Scenario | What it is | Steps | Bit-exact steps | First difference | Force calls compared |
|---|---|---|---|---|---|
| `spa_launch` | from the hot-lap start flat out over the line, brake for La Source (24 s) | 8,001 | 8,001 (100 %) | none | 522,814 |
| `spa_eau_rouge` | from 300 m before the lowest point: flat out through Eau Rouge and up Raidillon (16 s) | 5,334 | 5,334 (100 %) | none | 348,082 |
| `spa_kerbs` | through the last chicane over its kerbs, cutting it (20 s) | 6,667 | 6,667 (100 %) | none | 433,411 |
| `spa_lap` | a lap as far as a minute goes (60 s) | 20,001 | 20,001 (100 %) | none | 1,314,639 |
| `spa_grass` | along the Kemmel straight with two wheels on the grass for a while (16 s) | 5,334 | 5,334 (100 %) | none | 347,568 |
| `spa_timing` (extra) | over all three timing lines twice, put down before each line (38 s) | 12,732 | 12,732 (100 %) | none | 832,680 |
| **all** | | **58,069** | **58,069 (100 %)** | | **3,799,194** |

What the drives covered (counted on the Rust car, which the table above shows to be the game's car):

| Scenario | Driven | Along the lap (0..1) | Road height under the left front wheel | Tyre rays (without a hit), meshes hit | Tyre-steps per surface | Sum of the four tyre loads; tyre-steps in the air | Steps with more than two tyres off / cuts | Lines crossed / laps counted |
|---|---|---|---|---|---|---|---|---|
| `spa_launch` | 817 m, up to 271 km/h | 0.9379 to 0.0556 | 5.2 to 28.2 m | 32,004 (0), 24 | asphalt 31,773, grille 55, kerb 176 | 5,042 to 20,991 N; 11 | 0 / 0 | 1 / 0 |
| `spa_eau_rouge` | 856 m, up to 300 km/h | 0.1085 to 0.2316 | -5.9 to 32.8 m | 21,336 (0), 30 | asphalt 20,029, concrete 153, kerbs 960, grille 194 | 5,433 to 31,643 N; 38 | 0 / 0 | 0 / 0 |
| `spa_kerbs` | 256 m, up to 137 km/h | 0.9442 to 0.9793 | 6.8 to 15.3 m | 26,668 (0), 18 | asphalt 19,892, kerbs 326, carpet 2,121, concrete 459, grass 615, grille 401, out of track 2,854 | 0 to 23,098 N; 1,837 | 853 / 3 | 0 / 0 |
| `spa_lap` | 2,992 m, up to 310 km/h | 0.9379 to 0.3688 | -6.0 to 73.1 m | 80,004 (1), 88 | asphalt 77,801, kerbs 1,709, concrete 137, grille 356 | 5,021 to 30,914 N; 54 | 0 / 0 | 2 / 0 |
| `spa_grass` | 601 m, up to 179 km/h | 0.2381 to 0.3243 | 35.1 to 62.5 m | 21,336 (0), 16 | asphalt 18,771, grass 2,091, out of track 424, grille 50 | 3,849 to 18,879 N; 120 | 0 / 0 | 0 / 0 |
| `spa_timing` | 9,046 m with the jumps, up to 130 km/h | all three lines | -28.2 to 64.3 m | 50,928 (0), 11 | asphalt 48,684, kerb 2,244 | 0 to 24,286 N; 1,596 | 0 / 0 | 8 / 2 (last lap 13,918 ms) |

So the comparison covers: the crest and compression of Eau Rouge (tyre loads from 5 to 32 kN, tyres in the
air), kerbs and the carpet behind them, grass and "out of track" surfaces, three cuts with the lap spoiled, the
start line with the armed first lap, both sector lines, two counted laps with their sector times, and the
position along the lap wrapping from 0.99 to 0.00. Sand was not driven on (section 6).

The full tables (also what the engine, gearbox, wings and aids did in each drive) are written to the
git-ignored `oracle/chassis/results_track_whole.md` by the run.

### 4.3 The program

| Check | Result |
|---|---|
| `chassis_compare game-replay --dir oracle/track`: `rustyac.exe --replay --headless --dump-states` replays each recorded drive on its own track loading, and the states are compared with the game's recording | **5 of 5 bit-exact** (`spa_timing` is left out: it needs the test's teleports, which an input file does not carry) |
| Golden test `the_car_on_spa_matches_the_game` (in `cargo test`): two excerpts of the game's recordings (300 steps of the kerbs with a cut, 200 steps over the start line) replayed on the track loaded from the game's folder | passes; prints **NOT TESTED** and passes when the game's folder is missing |
| `ac_telemetry.py --laps 1` on `rustyac.exe --track spa --autodrive --headless` | 48,661 rows at 333 Hz; stopped by itself after the lap; `lastLapMs` 129,148 (2:09.148, the program's own number); `trackPos` runs 0 to 1; the car's height runs from -28.6 to 73.7 m |
| `--autodrive`, 300 s without a clock (100,000 steps in 7 s) | two laps listed, both valid |
| The window on Spa for 10 s (`--windowed --no-focus --autodrive --duration 10`) | opened, drew at the display's rate, closed by itself |
| Test `back_on_track_marks_the_lap_and_a_reset_starts_the_timer_again` (in `cargo test`; NOT TESTED without the game's folder): the line follower drives 27 s from the hot-lap start, then `Shift+R`, then `R` | the clock restarts at the line; back-on-track puts the car on the line where it was, standing, lap marked as cut; reset gives a fresh timer before the line |
| `cargo test --release --workspace` | 134 tests pass |

### 4.4 Against a recording of the real game

`track_info spa --telemetry f2004_spa_ai.csv`. The file is a recording made with `ac_telemetry.py` in
the real game before this task (AC's AI in the F2004, three laps of Spa, 117,333 rows). This check is
independent of the oracle: the oracle hands the game's code the meshes that the port read from the kn5 files,
so it cannot see a mistake in reading them. The real game read them itself.

| What | Result |
|---|---|
| Road height. At each of the 16,431 positions of the game's car, the port's road under the car against the game's own (the car's height minus its `cgHeight`) | a road under every position; mean difference +3.2 mm, half within 2.9 mm, 99 % within 13 mm, largest 47 mm |
| Place along the lap. The port's position on the AI line at the game's car against the game's `trackPos` | half within 0.33 m, 99 % within 0.74 m, largest 2.5 m (of 6,946 m); the two numbers are not taken at the same instant in the game's shared memory |
| Start line. Where the car is when the game's lap clock restarts (4 crossings at 220 km/h) | the timed wheel hub comes out 1.45 to 1.49 m ahead of the car's middle, plus what the car moved between the two readings (at most one frame, about 1 m); the F2004's front hub is 1.68 m ahead, so the line is where the game's is to well within a metre |
| Lowest and highest point of the lap | game -28.66 / 73.72 m, Rust car with the line follower -28.62 / 73.74 m |

## 5. Load time, memory and frame rate on this PC

Intel Core i9-9980HK (8 cores), 32 GB, AMD Radeon Pro 5500M (4 GB), Windows 10. Release build.

| What | Number |
|---|---|
| Reading the structure of `spa.kn5` (441 MB, 1,219 meshes, 168 textures) | 18 to 23 ms |
| Loading Spa's physics (`track_info`): 455 meshes, 588,295 triangles, 587,840 tree nodes, AI line, gates | **0.47 s** (models 0.08 s, collision trees 0.32 s) |
| Loading Spa's picture: 6 files, 1,018 meshes, 2,046,191 triangles (131 MB), 73 textures (49 MB at 1024 px) | 0.46 to 0.76 s |
| Loading the F2004's model: 112 meshes, 219,958 triangles, 39 textures (22 MB) | 0.13 to 0.15 s |
| Memory of `rustyac.exe --track spa`, physics only (no picture) | 59 MB working set (peak 63 MB) |
| Memory with the picture | 291 to 313 MB working set (peak 315 MB), 559 to 581 MB private |
| Graphics memory used by the models (vertices, indices, textures) | about 200 MB of the card's 4 GB |
| Physics over a whole lap in real time (150 s, line follower, picture drawn at the same time) | 333.33 Hz with no drift; a step takes 0.136 ms on average, 2.2 ms at most (3 ms are available); 37 of 49,993 steps started more than 1 ms late, none a whole step |
| Frame rate off screen, not waiting for a display (1280 x 720, 4x MSAA), over that lap | **322 FPS**, slowest frame 24 ms |
| Frame rate in the window (waits for the 60 Hz display) | 58.7 FPS over the first 10 s, slowest frame 34 ms |
| Without a clock | 100,000 steps (300 s of driving) in 7 s |

## 6. What is still missing

For Task 13 and later:

- **The car body touches nothing**: no walls, no barriers, no ground under a rolled-over car. The 88 wall
  meshes are loaded and sit in their own sub-spaces, ready for it. Only rays see the track.
- Other cars, and with them the starting grid as a grid, AI drivers, flags, pit stops and pit penalties
  (`PenaltyManager::step` 0x140265e10 is not ported; the jump-start rule itself is ported since Task 10, but
  `rustyac.exe` has no race start, so it never acts).
- AC's real renderer: shadows, sky, reflections, track lights, animated objects, cameras, the cockpit's
  instruments. The debug view has one sun and an ambient term.

Smaller gaps, each named where it is in the code. (Task 18 closed several of them: the other tracks and
layouts, `ROTATION`, the ray's other mode, sub-space 0, late meshes, old AI lines and the lookup grid, a
missing `MODEL_n` file. See `docs/port/tracks.md`.)

- **Only Spa was tested**, and only its base layout. Tracks with several layouts (`models_<layout>.ini`,
  `data` per layout) are read by the code but none was run, and `rustyac.exe` has no option to pick a layout
  yet. (The Spa folder of this PC also holds an added layout, `spa_fa_2026`; it was not touched.) A `ROTATION`
  in `models.ini` is not applied (a message says so; Spa has none).
- Sand was not driven on in a compared scenario. Its surface values are checked (the surface table of section
  4.1), and grass, which the tyre treats the same way with other numbers, was driven on.
- The tyre ray's other mode in the game (a ray of length exactly `f32::MAX` uses OPCODE's unbounded walk), a
  mesh in sub-space 0, and meshes added after the first ray: no track or tyre reaches them.
- The AI line: version 6 files, building the lookup grid for a file without one (the port then searches all
  points), and what only AI drivers read (`calculateRadius`, `calculateNormals`, `initStraights`,
  `ai_hints.ini`, the pit lane's attach point).
- Penalty mode 2 (the performance-meter time penalty) and penalties served in the pits.
- The track's DRS zones (`data/drs_zones.ini`) are not read: DRS is available everywhere, as on a track without
  zones. The F2004 has no DRS.
- The track-wide grip stays at 1.0 (the game's value without a `[DYNAMIC_TRACK]` section): rubbering-in over a
  session is ported (`DynamicTrack`) but not switched on.
- Reset keeps tyre temperatures, wear and damage (as AC's teleport does); `N` builds a new car.
- Kerb rumble on the pad and the surfaces' sounds were not checked or added.
- A kn5 with a node rotation built from Euler angles (`createFromEuler`) or a missing `MODEL_n` file does not
  behave exactly as the game does; Spa has neither.

## 7. Open questions

Choices made without asking; each could be changed.

1. **Branch.** The work is on a new branch `feat/track-spa` made from `chore/license-files` (what was checked
   out). Not pushed, not merged.
2. **Commit trailer.** The task allows a `Co-Authored-By` trailer; the stored rule from earlier tasks says no
   trailer. The commits have none.
3. **The oracle reads the kn5 files with the port's reader.** The game's own reader needs a Direct3D device,
   so the test program gives the game's `Track::addSurface` the vertices the port read. That the port reads
   them correctly is shown by section 4.4 (the real game's car on the Rust road, within millimetres) rather
   than by a bit-exact comparison. Is that enough, or should a later task run the game's `KN5IO` too?
4. **The ghost body.** In the game's test run every mesh of the track gets collision category 0 (its sub-space
   is still chosen by its real surface), so nothing but a ray meets the track and the game's car body passes
   through it; the port simply has no body collision. This is what "the body is a ghost" was taken to mean.
5. **A sixth scenario**, `spa_timing`, was added to the five that were asked for, because 60 s of driving
   cannot reach all three timing lines twice. It puts the car down before each line with a teleport that does
   not spoil the lap (the game's own teleports always do); the seventh and last jump is of the game's own
   kind, so that the crossing after it is seen not to count. The teleport is a tool of the test only.
6. **Session rules chosen for `--spawn`.** Hot-lap: the first lap is armed (the clock starts at the line).
   All spawns: leaving the track spoils the lap (penalty mode 1), two tyres may be off. Is that the wanted
   default, or should cuts be free while there are no walls?
7. **Keys.** `R` is back to the start (and restarts the timer), `Shift+R` / holding Back is back on track,
   and the new car moved from `Shift+R` to `N`. The automatic put-back of a rolled-over car goes to the track,
   not to the start. Back-on-track marks the lap as cut: the game has no such command, and its teleport alone
   would leave a lap that has not reached its first sector line valid.
8. **Textures are cut down to 1024 px** instead of the "flat colours when memory is short" that the task
   offers as a fallback (`--no-textures` does that; `--texture-size 0` asks for the stored size, which is
   still halved until a model's textures fit into 1,200 MB). And one thing
   beyond "diffuse only": AC's detail texture is multiplied in where the material asks for it, because the
   car would be white without it.
9. **`--autodrive`** was added though the task does not ask for it: without it there is no way to make a lap,
   a screenshot at Eau Rouge or a frame-rate number with nobody at the controls. It is not AC's AI.
10. **The game's "side velocity" on the AI line is broken** (divided by a time step of zero): the port copies
    the infinite / not-a-number value because the task is bit-exactness. Nothing in the car reads it.
11. **One point of a review was not followed**: a brief said kn5 and ini strings need not be UTF-8 and should
    be kept as bytes; the port cuts a name at the first invalid byte instead. Spa and the F2004 have only
    ASCII names.
12. **The recordings are deleted** (812 MB). The committed golden excerpts are 162 KB and 112 KB; the three
    screenshots are about 1 MB each, which is large for a repository. Smaller pictures on request.

## 8. How to run the checks again

```
cargo test --release --workspace
target\release\track_info.exe spa
target\release\track_info.exe spa --telemetry f2004_spa_ai.csv

cd tools\car_oracle && cargo build --release && cd ..\..
tools\car_oracle\target\release\car_oracle.exe rays --track spa --count 1000000 --seed 12
tools\car_oracle\target\release\car_oracle.exe run --track spa --scenario spa_launch --out oracle\track
   (and spa_eau_rouge, spa_kerbs, spa_lap, spa_grass, spa_timing: 80 to 290 MB each)

cd tools\chassis_compare && cargo build --release && cd ..\..
tools\chassis_compare\target\release\chassis_compare.exe run --dir oracle\track
tools\chassis_compare\target\release\chassis_compare.exe game-replay --dir oracle\track
tools\chassis_compare\target\release\chassis_compare.exe excerpt-track      (rewrites the golden excerpts)

target\release\rustyac.exe --track spa --autodrive --headless --duration 150 --bench-render --no-shm
target\release\rustyac.exe --track spa --autodrive --screenshot re\scratch\shot.png --at 38
```

`car_oracle` maps `acs.exe` from the game's folder read-only into its own process; it never touches a running
game. Delete `oracle\track\*.carrec` again when done.
