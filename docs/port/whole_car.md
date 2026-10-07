# The whole Rust car: aero, aids, `Car::step` shell, telemetry page (Task 10)

## Resume here

(Kept up to date while the task ran, so that a cut-off would lose nothing. Newest state first.)

- **State**: finished. Everything below is committed; nothing is in flight. (The task was cut
  off once by a usage limit after the four systems, `VanillaCar` and the golden test were in;
  the tables, the altered test cars, the review and this report were done after it.)
- **What exists**: the port in `crates/rustyac-physics/src/car/` (`aero.rs`, `aids.rs`,
  `telemetry.rs`, `vanilla_car.rs`, the `Car::step` shell in `chassis.rs`); `tools/chassis_compare`
  runs a whole Rust car against the game's recordings, given only the driver's controls
  (`run`, `faults`, `excerpt`, `test-car`); `tools/car_oracle` has eight more scripted drives
  (`wc_*`); five golden excerpts in `crates/rustyac-physics/tests/golden/` (format `CHGOLD03`)
  and fourteen unit tests in `tests/whole_car.rs`.
- **To check that nothing broke** (about three minutes, no game needed):
  `cargo test -p rustyac-physics`, then `chassis_compare run` and
  `chassis_compare run --dir oracle/car_wc` (section 5 has every command).
- **If the recordings of the new drives are gone** (git-ignored, 0.64 GB): `chassis_compare
  test-car`, then the `car_oracle all --car ...` lines of section 5.
- **What comes next** (not started, by the task's own limits): contacts with the road and
  walls, track loading, four-wheel drive and the hybrids, a window. Section 4 lists what is
  still missing.
- **Briefs** the port was checked against (git-ignored, written by read-only helpers from the
  disassembly): `re/scratch/task10/spec_aero.md`, `spec_aids.md`, `spec_car_step.md`; the
  reviewers' notes are `re/scratch/task10/review_*.md`. There is no brief for the telemetry
  page (its helper was stopped by the cut-off); the page was read from the listings.

---

Code: `crates/rustyac-physics/src/car/` (four new files, six changed), one function added to
`crates/rustyac-math` (the C runtime's `atan2f`), `tools/chassis_compare` and `tools/car_oracle`
extended, golden excerpts and `tests/chassis_golden.rs` rewritten. Sources: the disassembly of
`acs.exe` 1.16.4 with `acs.pdb` (`tools/disasm.py`), the maps in `docs/map/`, and three
instruction-level briefs written for this task by read-only helpers in the git-ignored
`re/scratch/task10/`. Nothing in the game folder or the Ghidra project was changed and no game
process was touched. Contacts, track loading, four-wheel drive, the hybrid systems and anything
with a window were not started.

---

## 1. Plain-English summary

The Rust car is now a whole car: it works out for itself how hard the air pushes on its wings,
when its driving aids step in, and everything else the game does to a car in one tick of its
clock.
The aids are traction control (cuts the engine when the driven wheels spin), ABS (lets go of a
brake when a wheel is about to lock), an electronic differential lock (brakes the one driven
wheel that spins faster), a stability aid (turns the car back when its tail steps out) and the
pit-lane speed limiter.
The car also fills in the same page of live numbers that the game publishes for dashboards and
loggers (speed, revs, tyre temperatures, ride height and so on), in the same layout, so the
project's own logger and checker can read the Rust car exactly as they read the game.
To check all this, the Rust car was driven through every recorded drive of the game's own car
while being told only what the driver did (pedals, steering wheel, gear paddles, handbrake,
the wing-opening button) and what the weather and the road were like; after every one of the
228,184 steps every number was compared with the game, and all of them agree in every digit,
the 148 numbers of that live page included.
Because the Formula 1 car has no ABS, no wing-opening button and no differential lock, more
drives were recorded with the game's own code: a Ferrari 488 GT3 braking as hard as it can, a
Lotus Formula 1 car opening and closing its rear wing, an Alfa Romeo with a differential lock,
drives on a pit lane, in wind, with the stability aid on, a drive in which the rules step in
(controls locked, a penalty to serve, headlights switched), and three altered copies of the
Formula 1 car that have the rarer variants of these things (one of them with dented bodywork,
one with the simple old way of describing a car's air drag that no shipped car uses any more).
Twenty-four deliberate small faults were put into the Rust car one at a time to make sure the
comparison notices such things, and it noticed each one.
Three short pieces of these drives (a standing start, a full braking, and the 488 braking on
its ABS) are stored with the project's tests and run without the game.
What the Rust car still cannot do is touch anything: it drives on a flat endless road with no
walls, no kerbs to hit with its floor, and no other cars.

---

## 2. What was ported

### 2.1 Where the code lives

All in `crates/rustyac-physics/src/car/` unless another path is given:

| File | Content |
|---|---|
| `aero.rs` (new) | the aero slot `AeroModel`; `VanillaAero` (AC's `AeroMap` with its `Wing`s and the car's `DRS`): angle-of-attack and ride-height tables, yaw term, damage factor, fins, wing controllers (`DynamicWingController`), DRS in both modes, the old one-body format (`aero.ini [DATA]`: body drag with sideways and vertical factors, a torque against the body's rotation, lift split front / rear); `Car::getGroundWindVector`, `RaceEngineer::getPointGroundHeight`, `plane4f`, `Car::updateAirPressure`, `SlipStream` |
| `aids.rs` (new) | the aids slot `AidsModel`; `VanillaAids`: `TractionControl`, `Abs` (1, 2 and 4 channels), `Edl`, `StabilityControl`, `SpeedLimiter`, the level tables with `cycleMode`, `Tyre::getDX`, `RaceEngineer::getOptimalBrake` |
| `telemetry.rs` (new) | `PhysicsPageWriter` and `PhysicsPage`: the 148 values of `SPageFilePhysics` after every step, with the ride heights of the car's state snapshot (measured, or the standing-car estimate), heading / pitch / roll, the centre-of-gravity height, the tyres-off-track count |
| `vanilla_car.rs` (new) | `VanillaCar`: a chassis with every slot filled and the driver's device; the device trait `ControlsProvider`; `ScriptedDevice` |
| `chassis.rs` (changed) | the two new slots and `install_aero` / `install_aids` / `install_telemetry`; the rest of `Car::step`: the whole of `Car::pollControls` (locked and black-flagged branch, the device's rumble and rev marks), headlight switch, black-flag teleport, penalty timers, `updateColliderStatus`, `stepJumpStart` with `PenaltyManager::addJumpStartPenalty`, `postStep`; `PhysicsEngine::stepWind`; the car members those need |
| `feed.rs` (changed) | `CarControls::drs`, `VibrationDef`, the device's four calls as hooks with defaults |
| `setup.rs` (changed) | the `WING_<i>` setup items at their place in the game's item order |
| `replay.rs` (changed) | `Runner` (a fed chassis or a whole car), `RecordedStep::driver_only`, the new compared values, state save / restore, golden format `CHGOLD03`, a pit-lane road |
| `body.rs` (changed) | `get_local_velocity`, `get_local_angular_velocity` |
| `../tyre/interfaces.rs` (changed) | `SurfaceDef::is_valid_track`, `is_pitlane`, `vibration_gain`, `vibration_length` |
| `crates/rustyac-math` (changed) | `atan2f` of MSVCR120 |

### 2.2 Slots and traits

- **`AeroModel`** (`aero.rs`): `step(car, dt)` (positions 6 and 7 of `Car::stepComponents`:
  `DRS::step`, then `AeroMap::step`), `base()` (`AeroBase`: air density, the wings with their
  data, state, controllers and damage gains, the DRS), `trace`, `save_state` / `load_state`.
  `RollingChassis::aero` is an `Option<Box<dyn AeroModel>>`.
- **`AidsModel`** (`aids.rs`): three entry points because the game runs the aids at three
  places in the step: `step_edl` (position 2, right after the brakes), `step` (positions 16 to
  18: ABS, traction control, pit limiter) and `step_stability` (position 27); `base()`
  (`AidsBase`: the five aids as plain structs), `trace`, `save_state` / `load_state`.
  `RollingChassis::aids` is an `Option<Box<dyn AidsModel>>`.
- **`ControlsProvider`** (`vanilla_car.rs`): AC's `ICarControlsProvider` as `Car` calls it:
  `acquire_controls`, `get_action`, `send_ff`, `set_vibrations`, `set_engine_rpm`. A device
  writes its own fields of `Car::controls` and leaves the others alone, as the game's devices
  do.
- **`VanillaCar<P: ControlsProvider>`**: `car` (the `RollingChassis`) and `device`.
  `VanillaCar::new` is `Car::Car` with everything the port has; `step(dt, physics_time)` is
  `PhysicsEngine::step` for one car (wind, `Car::step`, the rigid-body step, `Car::postStep`,
  the telemetry page). Its feed has only the device behind it; every other hook is
  `unreachable!`.
- **A slot can still be empty.** A chassis without wings or aids is the car of Task 09 (the
  feed supplies wing forces and the aids' outputs); without brakes and drivetrain as well it is
  the rolling chassis of Task 08. That is how `chassis_compare run --feed ...` works and how the
  two chassis-only golden excerpts still run.
- **The telemetry writer is an optional member** too (`RollingChassis::telemetry`,
  `install_telemetry`): with it every step leaves its page in `physics_page`.
- **What the game's main thread asks of the physics thread** goes through
  `RollingChassis::queue` (run at the start of the next step, when the clock already shows
  that step's time). What other cars would contribute is handed in as plain data:
  `other_wakes` (their slipstreams) and `other_car_positions` (for the pit-lane ghosting).
- **`ChassisFeed`** gained the device's five calls (`get_action`, `send_ff`, `get_ff_global_gain`,
  `set_vibrations`, `set_engine_rpm`, all with do-nothing defaults); `edl`, `aero`, `aids` and `stability` are now
  "only without ..." hooks like the others.
- **`ChassisEnvironment`** gained the session and track values the new code reads: `wind`,
  `wind_speed` (with `set_wind` and `step_wind`), `drs_zone_available`, `penalty_mode`,
  `base_pit_penalty_laps`, `damper_min_value`, `damper_gain`. `jump_start_penalty_mode` now
  defaults to 1, the `PhysicsEngine` constructor's value (it was 0).

### 2.3 Functions (addresses in `acs.exe`)

| Area | Functions |
|---|---|
| Aero | `Car::initAeroMap` 0x140272a80, `AeroMap::init` 0x1402b5ca0, `AeroMap::loadINI` 0x1402b5d50, `AeroMap::step` 0x1402b7150, `AeroMap::addDrag` 0x1402b5860, `AeroMap::addLift` 0x1402b5a90, `Wing::Wing` 0x1402b1340, `Wing::step` 0x1402b2bc0, `Wing::addDrag` 0x1402b2420, `Wing::addLift` 0x1402b2730, `Wing::stepDynamicControllers` 0x1402b2dd0, `Wing::getCurrentModifiedAngle` 0x1402b2b80, `Wing::setOverrideAngle` 0x1402b2bb0, `Wing::clearOverrides` 0x1402b2b70, `DynamicWingController::initCommon` 0x1402aa0d0, `step` 0x1402aaa70, `getInput` 0x1402a9ef0 (all eight inputs), `DRS::init` 0x1402b4330, `DRS::step` 0x1402b4e60, `Car::getGroundWindVector` 0x140270c00, `RaceEngineer::getPointGroundHeight` 0x14027c550, `plane4f::plane4f` 0x140117ed0, `Car::updateAirPressure` 0x140276ae0, `SlipStream::init` 0x1402aadc0, `SlipStream::getSlipEffect` 0x1402aac60, `SlipStream::setPosition` 0x1402aae80, `RaceEngineer::getPointFrontShare` 0x14027c4d0, `PhysicsEngine::setWind` 0x1402645a0, `PhysicsEngine::stepWind` 0x140265380 |
| Aids | `TractionControl::init` 0x14028fa40, `step` 0x140290200, `cycleMode` 0x14028f8e0, `getCurrentMode` 0x14028f9b0; `ABS::init` 0x14028ec50, `step` 0x14028f610, `cycleMode` 0x14028eae0, `getCurrentMode` 0x14028ebc0, `isInAction` 0x14028f5d0; `EDL::init` 0x1402babf0, `step` 0x1402bb460; `StabilityControl::init` 0x1402bfa30, `step` 0x1402bfa50; `SpeedLimiter::init` 0x1402bb900, `step` 0x1402bb910; `RaceEngineer::getOptimalBrake` 0x14027c320, `Tyre::getDX` 0x140280240 |
| `Car::step` shell | of `Car::step` 0x140275da0: the headlight switch, the black-flag teleport, the place of `updateAirPressure`, the penalty timers, the calls of `updateColliderStatus` and `stepJumpStart`; `Car::pollControls` 0x140274e70 in full; `Car::updateColliderStatus` 0x140276df0; `Car::stepJumpStart` 0x140276780 with `PenaltyManager::addJumpStartPenalty` 0x140265a90 and `PhysicsEngine::hasSessionStarted` 0x140263c70; `Car::postStep` 0x140275430 (the wake); of `Car::onTyresStepCompleted` 0x140274cd0 the `sendFF` call; `Car::reset` 0x1402758e0 (the members `forcePosition` was still missing); `Car::lockControls` 0x1402745f0, `lockControlsUntil` 0x140274600, `addPenalty` 0x14026f6a0, `clearPenalty` 0x14026fc50, `getPenaltyTime` 0x140270d50, `setBlackFlag` 0x1402759f0, `onNewSession` 0x140274c30, `isInPits` 0x140274530, `isInPitLane` 0x1402744e0; of `Car::stepPreCacheValues` 0x1402768c0 the power class with `Engine::getMaxPowerW` 0x140285a80; `ksSquareWave` 0x14022d2d0; `RigidBodyODE::getLocalVelocity` 0x1402ce520, `getLocalAngularVelocity` 0x1402ce430 |
| Telemetry | `SharedMemoryWriter::updatePhysics` 0x140186ef0; of `Car::getPhysicsState` 0x140270d70 the ride heights; `Car::computeRideHeight` 0x14026fc90, `RaceEngineer::evalFrontRideHeight` 0x14027a0f0, `evalRearRideHeight` 0x14027a850, `getWeightDistribution` 0x14027cb90, `Tyre::getDynamicK` 0x140280560, `Car::getCGHeight` 0x1402703b0, `Car::getFinalFF` 0x140270960; of `LapInvalidator::step` 0x1402c0580 the tyres-off-track count |
| Setup | of `SetupManager::initItems` 0x140289570: one `WING_<i>` item per wing (fins included), pointing at the wing's `inputAngle` when it has controllers and at its `angle` otherwise |

### 2.4 Things in the machine code that decide bits

All confirmed by the comparison (each would have shown as a difference; the first three did),
or by the review of section 6.3 where no recording reaches them.

- **A wing controller's smoothing rate is not the generic controller's.** Both come from the
  same source formula, but in `DynamicWingController::initCommon` the compiler folded two
  factors: `((1 - FILTER) * 1.3333334) * 333.33334`. Using the function `DynamicController`
  uses gave wing angles one or two last-place units off in step 0.
- **`ABS::currentValue` starts at 1**, not 0 (the only value that differed in the first run of
  the aids, for the three steps before the first ABS check).
- **A standing car with a minimum-height rule shows an estimated ride height.** Below 2 m/s
  (the speed of the previous state snapshot), a car whose `car.ini` has `[RULES] MIN_HEIGHT`
  gets `RaceEngineer::evalFront/RearRideHeight` (weight on the axle over wheel rate in series
  with the tyre's spring, minus the rod length) instead of the measured height. Found on the
  488 GT3 and the Lotus Exos; the F2004 has no such rule.
- **Drag uses the whole air speed, lift only two of its three components.** Drag is
  `speed² * CD * density * area * 0.5` with the squared length of the air-speed vector; lift
  takes forward and vertical speed (a fin: forward and sideways). The lift's direction is a
  cross product the compiler left with its multiplications by zero in it; they are kept.
- **Ground height and ground wind use different contact points.** A wing's height above the
  road is the mean over two planes through the tyres' contact points (LF, RF, LR and LF, RF,
  RR); the wind near the ground is the wind with its part along the normal of a plane through
  the first three tyres' *unmodified* contact points taken out, times 0.44.
- **The air density of a car alone is exactly the engine's** (`1.2922 - T * 0.0041`): the
  slipstream formula is run anyway and adds a zero.
- **Traction control reads the body's speed twice in two ways**: the live body velocity
  against `MIN_SPEED_KMH`, the speed cached at the start of the step against 1 m/s. A cut
  lasts until the next check: between checks the aid writes the zero again every step, because
  the engine resets the value when it has used it.
- **The aids' timers are whole steps.** `dt` is summed in single precision until it reaches
  `1 / RATE_HZ`; at 500 Hz the check runs every step.
- **ABS acts one step late, the differential lock in the same step.** ABS runs after the
  drivetrain and writes a factor the next step's tyres use; the lock runs right after the
  brakes and adds to the brake torque the tyres of this step see.
- **The pit limiter is a property of the road, not a button**: it works while a tyre stands on
  a surface marked as pit lane, cuts the engine above 80 km/h and asks for "the brake pedal
  that just reaches the tyres' grip" above 81.
- **The stability aid is one torque about the body's own up axis**, from the difference
  between the rear and front slip angles; it does not brake a wheel or cut the engine.
- **The page mixes two moments.** Speed and acceleration are from before the rigid-body step,
  velocity, angles and spin from after it; the ride heights come through the car's state
  snapshot.
- **`Car::pollControls` passes the device's values through untouched.** All smoothing of raw
  input in the game (steering speed and filter, gamma, dead zones, the keyboard's ramps) is
  inside the device classes, before the car sees anything; the car only zeroes seven fields
  when its controls are locked or it is black-flagged, and forces pedals and wheel for a lock
  or a gentle stop.
- **The collider mask is written every step** (0x1e normally, 0x1a on the pit lane and until
  no other car is within 6 m after leaving it, bit 0 added when the car lies on its side); the
  port keeps flag and mask as state for the day contacts arrive.

### 2.5 Not ported (by design of this task, or refused with a message)

Contacts of any kind and with them `Car::onCollisionCallBack` (damage levels are a state the
caller sets); track loading; four-wheel drive, KERS and ERS, rear-wheel steering, strut / axle /
multilink suspensions (such cars are refused as before); DRS zones of a track (`ChassisEnvironment::drs_zone_available` stands for
`DRSManager::isDRSAvailable`); the track-side parts of `Car::stepComponents` that need a racing
line or timing lines and change no physics on a road without them: `Telemetry::step` (the
game's own channel recorder), `DriftModeComponent`, `PerformanceMeter`, `LapInvalidator` (but
for its tyre count), `PenaltyManager::step`, `SplineLocator`, `TimeTransponder`,
`FuelLapEvaluator`, `CarColliderManager::step` (reloads a file when it changes on disk); what
starts a penalty or a black flag (those belong to `LapInvalidator` and `PenaltyManager::step`;
the car's own timers and reactions are ported and are driven from outside); the
second half of `Car::postStep` (copies the racing-line locator's results); the game's device
classes (wheel, pad, keyboard: `spec_car_step.md` section 5 has their input filtering, read but
not ported: nothing can be compared without the devices); the graphics and static telemetry
pages; the events the game raises (flags, jump start, forced position: a counter where a test
needs one); a `data_<config>` folder next to `data` (the port knows only the plain folder, as
before); the game setting `WARNINGS_AS_ERRORS` (with it an unknown wing-controller input stops
the game; the port, like the game without it, keeps such a stage with input 0).

---

## 3. Results

`tools/chassis_compare run`: the Rust car is built from the car's data folder as a
`VanillaCar`, spawned, given the session start, and then steps through the whole recording
without ever being reset to the game's state. **It is given the driver's controls** (what the
recording's scripted device reported: pedals, wheel, paddles, handbrake, H-shifter lever, DRS
button; and the clicks of the cockpit brake bias) **and the session's settings** (air and road
temperature, grip, wear and fuel rates, tyre blankets, clock, and for the new drives: pit-lane
road, stability-aid gain, wind, damage levels). After every step it is compared with the game:
2,009 values of the chassis (bodies, joints, suspensions, tyres), the whole force tape (every
force and torque call with its vectors and the accumulators after it, the wings' calls
included), and 267 to 331 values of the other systems (fewer for the older recordings, which
hold fewer of them): applied controls, brakes, engine,
drivetrain, each wing's angle of attack / CD / CL / angle / ground height / drag / lift / yaw
angle (and for the new drives input angle, DRS factor, ground-effect factors, lift vector,
override flag), the aids' switches, timers, levels and outputs, the car-level values (vibration
phases, wake, collider mask, lock, flags), and the 148 values of the telemetry page.

### 3.1 The whole car against every recording

| Car | Folder | Scenario | Steps | Bit-exact steps | First divergence | Values compared per step (chassis + other systems) | Force calls compared |
|---|---|---|---|---|---|---|---|
| F2004 | `oracle/car` | `brake` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 330544 |
| F2004 | `oracle/car` | `kerb` | 4001 | 100 % (4001/4001) | none | 2009 + 272 | 260438 |
| F2004 | `oracle/car` | `launch_autoclutch_off` | 4667 | 100 % (4667/4667) | none | 2009 + 272 | 302848 |
| F2004 | `oracle/car` | `launch_autoclutch_on` | 4667 | 100 % (4667/4667) | none | 2009 + 272 | 304414 |
| F2004 | `oracle/car` | `liftoff_oversteer` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 346178 |
| F2004 | `oracle/car` | `random` | 20001 | 100 % (20001/20001) | none | 2009 + 272 | 1316458 |
| F2004 | `oracle/car` | `settle` | 667 | 100 % (667/667) | none | 2009 + 272 | 37354 |
| F2004 | `oracle/car` | `slalom` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 348436 |
| F2004 | `oracle/car` | `steady_corner_120` | 6667 | 100 % (6667/6667) | none | 2009 + 272 | 436414 |
| F2004 | `oracle/car` | `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 + 272 | 429784 |
| F2004 | `oracle/car` | `steady_corner_60` | 6667 | 100 % (6667/6667) | none | 2009 + 272 | 436414 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `brake` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 352674 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `kerb` | 4001 | 100 % (4001/4001) | none | 2009 + 272 | 271680 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `launch_autoclutch_on` | 4667 | 100 % (4667/4667) | none | 2009 + 272 | 329834 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `liftoff_oversteer` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 376426 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `random` | 20001 | 100 % (20001/20001) | none | 2009 + 272 | 1356128 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `settle` | 667 | 100 % (667/667) | none | 2009 + 272 | 42356 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `slalom` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 364010 |
| `f2004_tight_stops` | `oracle/car_tight_stops` | `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 + 272 | 476872 |
| `f2004_fallbacks` | `oracle/car_fallbacks` | `brake` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 328922 |
| `f2004_fallbacks` | `oracle/car_fallbacks` | `kerb` | 4001 | 100 % (4001/4001) | none | 2009 + 272 | 260338 |
| `f2004_fallbacks` | `oracle/car_fallbacks` | `settle` | 667 | 100 % (667/667) | none | 2009 + 272 | 37256 |
| `f2004_fallbacks` | `oracle/car_fallbacks` | `slalom` | 5334 | 100 % (5334/5334) | none | 2009 + 272 | 348338 |
| `f2004_fallbacks` | `oracle/car_fallbacks` | `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 + 272 | 429926 |
| F2004 | `oracle/car_pt` | `pt_protect` | 3334 | 100 % (3334/3334) | none | 2009 + 299 | 216436 |
| `f2004_pt_street` | `oracle/car_pt_street` | `pt_handbrake` | 3667 | 100 % (3667/3667) | none | 2009 + 299 | 227092 |
| `f2004_pt_street` | `oracle/car_pt_street` | `pt_random` | 3200 | 100 % (3200/3200) | none | 2009 + 299 | 207508 |
| `f2004_pt_street` | `oracle/car_pt_street` | `pt_shifter` | 4001 | 100 % (4001/4001) | none | 2009 + 299 | 253120 |
| `f2004_pt_ctrl` | `oracle/car_pt_ctrl` | `launch_autoclutch_on` | 3500 | 100 % (3500/3500) | none | 2009 + 272 | 227392 |
| `f2004_pt_ctrl` | `oracle/car_pt_ctrl` | `pt_random` | 4667 | 100 % (4667/4667) | none | 2009 + 299 | 304414 |
| `f2004_pt_fwd` | `oracle/car_pt_fwd` | `pt_autoshift` | 4001 | 100 % (4001/4001) | none | 2009 + 299 | 246386 |
| `f2004_pt_fwd` | `oracle/car_pt_fwd` | `pt_random` | 2600 | 100 % (2600/2600) | none | 2009 + 299 | 167684 |
| Ferrari F40 | `oracle/car_f40` | `pt_autoshift` | 3400 | 100 % (3400/3400) | none | 2009 + 267 | 140608 |
| 488 GT3 | `oracle/car_488_gt3` | `pt_autoshift` | 3400 | 100 % (3400/3400) | none | 2009 + 275 | 146172 |
| F2004 | `oracle/car_wc` | `wc_pit` | 3400 | 100 % (3400/3400) | none | 2009 + 328 | 219640 |
| F2004 | `oracle/car_wc` | `wc_stability` | 3400 | 100 % (3400/3400) | none | 2009 + 328 | 221331 |
| F2004 | `oracle/car_wc` | `wc_stops` | 3400 | 100 % (3400/3400) | none | 2009 + 328 | 217636 |
| F2004 | `oracle/car_wc` | `wc_wind` | 3400 | 100 % (3400/3400) | none | 2009 + 328 | 224556 |
| F2004 | `oracle/car_wc_shell` | `wc_shell` | 3700 | 100 % (3700/3700) | none | 2009 + 331 | 239680 |
| 488 GT3 | `oracle/car_wc_488` | `wc_pit` | 3400 | 100 % (3400/3400) | none | 2009 + 304 | 146940 |
| 488 GT3 | `oracle/car_wc_488` | `wc_stops` | 3400 | 100 % (3400/3400) | none | 2009 + 304 | 145554 |
| Lotus Exos 125 | `oracle/car_wc_exos` | `wc_drs` | 3700 | 100 % (3700/3700) | none | 2009 + 312 | 167012 |
| Giulia QV | `oracle/car_wc_giulia` | `wc_spirited` | 3400 | 100 % (3400/3400) | none | 2009 + 304 | 150338 |
| `f2004_wc_aids` | `oracle/car_wc_aids` | `wc_damage` | 2700 | 100 % (2700/2700) | none | 2009 + 328 | 173680 |
| `f2004_wc_aids` | `oracle/car_wc_aids` | `wc_drs` | 3700 | 100 % (3700/3700) | none | 2009 + 328 | 239680 |
| `f2004_wc_aids` | `oracle/car_wc_aids` | `wc_spirited` | 2700 | 100 % (2700/2700) | none | 2009 + 328 | 173680 |
| `f2004_wc_aids` | `oracle/car_wc_aids` | `wc_stops` | 2700 | 100 % (2700/2700) | none | 2009 + 328 | 173116 |
| `f2004_wc_abs1` | `oracle/car_wc_abs1` | `wc_stops` | 2700 | 100 % (2700/2700) | none | 2009 + 328 | 173680 |
| `f2004_wc_oldaero` | `oracle/car_wc_oldaero` | `wc_spirited` | 2700 | 100 % (2700/2700) | none | 2009 + 272 | 149914 |
| **all** | | **49 drives** | **228,184** | **100 % (228184/228184)** | **none** | | **14,477,291** |

`settle_floor` is left out as before (its floor contacts are stage 2 of the rigid-body port).

### 3.2 Which branches the drives took

| Car | Scenario | Wing force calls / wing-steps moved by a controller | DRS open steps / button presses / wing-steps under DRS | TC cutting / wheel-steps released by ABS / EDL braking / stability torque calls | Pit limiter cutting / braking / ghost for cars | Wind steps | Ride height estimated | Wing-steps without lift |
|---|---|---|---|---|---|---|---|---|
| F2004 | `brake` | 50190 / 8655 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 14308 |
| F2004 | `kerb` | 52402 / 8540 | 0 / 0 / 0 | 130 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4077 |
| F2004 | `launch_autoclutch_off` | 59920 / 5470 | 0 / 0 / 0 | 250 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4695 |
| F2004 | `launch_autoclutch_on` | 61726 / 5498 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4058 |
| F2004 | `liftoff_oversteer` | 71064 / 12462 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4058 |
| F2004 | `random` | 276402 / 40540 | 0 / 0 / 0 | 25 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 11593 |
| F2004 | `settle` | 2156 / 1872 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3604 |
| F2004 | `slalom` | 71064 / 11206 | 0 / 0 / 0 | 130 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4051 |
| F2004 | `steady_corner_120` | 89726 / 13872 | 0 / 0 / 0 | 160 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4057 |
| F2004 | `steady_corner_180` | 89726 / 16461 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4058 |
| F2004 | `steady_corner_60` | 89726 / 13872 | 0 / 0 / 0 | 35 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 14571 |
| `f2004_tight_stops` | `brake` | 50316 / 8655 | 0 / 0 / 0 | 170 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 14450 |
| `f2004_tight_stops` | `kerb` | 52822 / 8540 | 0 / 0 / 0 | 115 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4090 |
| `f2004_tight_stops` | `launch_autoclutch_on` | 62146 / 5477 | 0 / 0 / 0 | 170 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4072 |
| `f2004_tight_stops` | `liftoff_oversteer` | 71484 / 12454 | 0 / 0 / 0 | 170 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4073 |
| `f2004_tight_stops` | `random` | 276822 / 40540 | 0 / 0 / 0 | 30 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 11665 |
| `f2004_tight_stops` | `settle` | 2576 / 1872 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3662 |
| `f2004_tight_stops` | `slalom` | 71484 / 11206 | 0 / 0 / 0 | 115 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4065 |
| `f2004_tight_stops` | `steady_corner_180` | 90146 / 16453 | 0 / 0 / 0 | 170 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4073 |
| `f2004_fallbacks` | `brake` | 48300 / 8771 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 14920 |
| `f2004_fallbacks` | `kerb` | 52290 / 8540 | 0 / 0 / 0 | 130 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3955 |
| `f2004_fallbacks` | `settle` | 2044 / 1872 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3548 |
| `f2004_fallbacks` | `slalom` | 70952 / 11206 | 0 / 0 / 0 | 130 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3915 |
| `f2004_fallbacks` | `steady_corner_180` | 89614 / 13040 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3922 |
| F2004 | `pt_protect` | 43064 / 5208 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4924 |
| `f2004_pt_street` | `pt_handbrake` | 34538 / 7872 | 0 / 0 / 0 | 180 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 15658 |
| `f2004_pt_street` | `pt_random` | 41090 / 6938 | 0 / 0 / 0 | 115 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 7661 |
| `f2004_pt_street` | `pt_shifter` | 43862 / 7996 | 0 / 0 / 0 | 405 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 9250 |
| `f2004_pt_ctrl` | `launch_autoclutch_on` | 45388 / 4360 | 0 / 0 / 0 | 260 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 3954 |
| `f2004_pt_ctrl` | `pt_random` | 61726 / 9872 | 0 / 0 / 0 | 130 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 10568 |
| `f2004_pt_fwd` | `pt_autoshift` | 35994 / 8540 | 0 / 0 / 0 | 435 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 14095 |
| `f2004_pt_fwd` | `pt_random` | 32438 / 5738 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 6650 |
| Ferrari F40 | `pt_autoshift` | 17190 / 0 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 1103 |
| 488 GT3 | `pt_autoshift` | 22720 / 2492 | 0 / 0 / 0 | 6 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 840 | 3019 |
| F2004 | `wc_pit` | 42644 / 7338 | 0 / 0 / 0 | 210 / 0 / 0 / 0 | 1091 / 28 / 3400 | 0 | 0 | 8539 |
| F2004 | `wc_stability` | 42924 / 5108 | 0 / 0 / 0 | 295 / 0 / 0 / 1451 | 0 / 0 / 0 | 0 | 0 | 4623 |
| F2004 | `wc_stops` | 40306 / 6520 | 0 / 0 / 0 | 265 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 7667 |
| F2004 | `wc_wind` | 47600 / 5108 | 0 / 0 / 0 | 285 / 0 / 0 / 0 | 0 / 0 / 0 | 3400 | 0 | 3535 |
| F2004 | `wc_shell` | 47124 / 7238 | 0 / 0 / 0 | 296 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 6545 |
| 488 GT3 | `wc_pit` | 23744 / 3400 | 0 / 0 / 0 | 0 / 0 / 0 / 0 | 906 / 0 / 3400 | 0 | 619 | 2311 |
| 488 GT3 | `wc_stops` | 21896 / 3011 | 0 / 0 / 0 | 6 / 292 / 0 / 0 | 0 / 0 / 0 | 0 | 1097 | 3824 |
| Lotus Exos 125 | `wc_drs` | 33080 / 0 | 1366 / 5 / 1366 | 4 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 531 | 1693 |
| Giulia QV | `wc_spirited` | 22912 / 0 | 0 / 0 / 0 | 266 / 0 / 190 / 0 | 0 / 0 / 0 | 0 | 0 | 1676 |
| `f2004_wc_aids` | `wc_damage` | 33124 / 14324 | 0 / 0 / 0 | 337 / 0 / 11 / 0 | 0 / 0 / 0 | 0 | 0 | 4588 |
| `f2004_wc_aids` | `wc_drs` | 47124 / 17966 | 1312 / 5 / 2624 | 197 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 4526 |
| `f2004_wc_aids` | `wc_spirited` | 33124 / 14306 | 0 / 0 / 0 | 329 / 0 / 10 / 0 | 0 / 0 / 0 | 0 | 0 | 4579 |
| `f2004_wc_aids` | `wc_stops` | 32466 / 14946 | 0 / 0 / 0 | 191 / 812 / 35 / 0 | 0 / 0 / 0 | 0 | 0 | 6112 |
| `f2004_wc_abs1` | `wc_stops` | 33124 / 5120 | 0 / 0 / 0 | 265 / 1260 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 5328 |
| `f2004_wc_oldaero` | `wc_spirited` | 9464 / 0 | 0 / 0 / 0 | 325 / 0 / 0 / 0 | 0 / 0 / 0 | 0 | 0 | 0 |

"Wing-steps moved by a controller": the F2004 has six wing controllers of its own (and a
fin), the 488 GT3 one; "ride height estimated": steps below 2 m/s on a car with a
minimum-height rule; "wing-steps without lift": no forward air speed, or air from behind.
The older coverage tables (brakes, engine, drivetrain, suspension stops) are in the result
files `oracle/chassis/results*_whole.md`.

### 3.3 The new drives and cars

`tools/car_oracle` got eight scenarios (`scenario::whole()`, named `wc_*`; like the `pt_*` ones
they only run when named). All use the game's automatic clutch and gearbox, so they work on any
car:

| Scenario | What it does | Recorded for |
|---|---|---|
| `wc_stops` | two full-throttle runs each ended by a full-pedal stop, the second while steering | F2004 (traction control), 488 GT3 (ABS, four channels), `f2004_wc_aids` (two channels, lock), `f2004_wc_abs1` (one channel) |
| `wc_pit` | the road is a pit lane: into the limiter, lift, again, brake | F2004 (cut and brake request), 488 GT3 (cut) |
| `wc_drs` | the DRS button five times: open, close, open, braking closes, a press while braking, open again, a fast bend | Lotus Exos 125, `f2004_wc_aids` (factor and angle mode, closing at 0.4 g) |
| `wc_spirited` | throttle bursts through a growing slalom | Giulia QV (differential lock), `f2004_wc_aids`, `f2004_wc_oldaero` |
| `wc_stability` | the same with the stability aid at 100 % | F2004 |
| `wc_wind` | the same in a 12 m/s wind (set with the game's own `setWind`) | F2004 |
| `wc_damage` | the same with dented bodywork (levels 45 / 20 / 70 / 10) | `f2004_wc_aids` |
| `wc_shell` | the car-level rules, through the game's own functions (`Car::addPenalty`, `lockControlsUntil`, `lockControls`, the gentle-stop flag, the device's headlight button) under the "cut gas" penalty rule: a penalty forgiven at low speed, controls locked for 0.6 s, lights on and off, a gentle stop, a penalty that waits while the driver is on the throttle and runs down off it, a second one on top, controls locked outright (the device is not asked) | F2004 |

New recorded values for these drives: the DRS button, the headlight button and the four kinds
of job, the aids' timers and levels, the wheels'
ABS factors at the end of the step, per wing input angle / DRS factor / ground-effect factors /
lift vector / override flag, the wind's strength, vibration phases, lights, pit-collision flag,
grid position, the wake's tip / direction / length, the lock time, the penalty's accumulator
and its "not forgiven for being slow" flag, the gentle-stop flag, the body mesh's collide
mask. The older scenarios record the same step data as before, byte for byte (`settle` and
`kerb` were recorded again with the final recorder and compared with `oracle/car`).

Three altered copies of the F2004 (`chassis_compare test-car`), for what no shipped car that
the port accepts has:

- **`f2004_wc_aids`**: ABS as `[ABS_V2]` with two channels; an electronic differential lock on
  an almost open differential; traction control that checks every step from 20 km/h; a
  `drs.ini` that opens the front wing by a factor and the rear wing by a fixed angle (a wing
  that also has a controller), ignores zones and closes above 0.4 g sideways; seven more wing
  controllers on the inputs the F2004's own do not use (brake, lateral and longitudinal g,
  travel of both rear suspensions), one naming a wing that does not exist and one with an
  input and a combinator the game does not know.
- **`f2004_wc_abs1`**: the same ABS with one channel.
- **`f2004_wc_oldaero`**: `aero.ini` replaced by the old one-body format (`[DATA]` with
  `REFERENCE_AREA`, `CD`, `CL`, `FRONT_SHARE`, `CDX`, `CDY`, no wings).

### 3.4 Branches that stay disassembly-only

Not reached by any recording; ported from the listings and read again by the reviewers (6.3):

| Part | Not reached by any recording | How it is covered |
|---|---|---|
| Aero | another car's wake in `Car::updateAirPressure` (`SlipStream::getSlipEffect` with a car inside the cone) | unit tests (the cone, the density formula), review |
| Aero | DRS zones of a track (`DRSManager::isDRSAvailable`) | an input flag (`drs_zone_available`); 6.3 has the function |
| Aero | a wing table, `aero.ini` or `drs.ini` in a `data_<config>` folder; an unknown controller input with the game's `WARNINGS_AS_ERRORS` on; an unknown DRS `MODE` (the game stops) | not ported / refused (2.5) |
| Aids | the level keys of traction control and ABS (`cycleMode`, `getCurrentMode`) | unit test, review |
| Aids | the stability aid's form for AI cars (`useBeta`) | unit test (the call is made, one torque about the up axis), review |
| Aids | the game's assist options (`apply_driving_assists`) | unit test, review |
| Aids | `Tyre::getDX` with a load table and in its linear form (the F2004, the one car whose limiter asked for the brakes, uses the exponent form) | review |
| Aids | the pit limiter's brake request on a car other than the F2004 (the 488 GT3 never passed 81 km/h on the limiter); a tyre in the air on the pit lane | same code; review |
| Aids | a level table with a value that is not a number; differential-lock gains that are not finite | review (the first fixed, the second refused: the game stops there) |
| Shell | the black-flag teleport; a jump start in all three modes; the "recover time" penalty rule; a penalty that runs out to zero | unit tests (teleport, jump start with the pit teleport), review |
| Shell | pit-lane ghosting with another car within 6 m; the "spectator" car; a car on its side (mask bit 0) | unit test (the first two), review |
| Shell | kerb rumble (a surface with `vibrationGain`) and the ABS rumble; the device's outputs as such (`VibrationDef`, rev marks, damper level) are computed but the recorder keeps only the two phases | review, term by term |
| Shell | a device that is a wheel (`getFFGlobalGain` other than 1) | review |
| Telemetry | the hybrid fields, push-to-pass status, the cockpit's engine-brake setting (the recorder's car avatar is zeroed, so recordings show 0; the port's default is the game's), the degenerate roll branch, the warm-up, tyres on a surface that is not track, the five seconds after a car contact | unit test (warm-up), review |
| All | every path that needs a NaN, an infinity or an empty table | review |

Reached only since the altered test cars and the extra drives exist (they were disassembly-only
before): ABS with one and two channels; the differential lock on the F2004; DRS in angle mode,
on a wing that has a controller, and its closing above a sideways g limit; the wing controller
inputs brake, lateral g, longitudinal g and rear suspension travel; a controller that names a
wing that does not exist and one with an unknown input and combinator; dented bodywork (the
damage factor of wings and fin); the old one-body aero; the game's own wind setter; controls locked by
time and outright; the gentle stop; the headlight switch; penalties under the "cut gas" rule
(forgiven at low speed, waiting on the throttle, running down off it, a second one on top).

### 3.5 Putting a system back

`--feed <system>[,<system>...]` takes systems from the recording instead of computing them:
`brakes`, `drivetrain` (with the engine), `aero`, `aids`. The six modes of the table (the four
single systems and the two combinations) and `--feed brakes,drivetrain` were each run on all
seventeen folders with the final build, 119 runs, and every one matches in every step:

| Mode | What it is | Result files |
|---|---|---|
| (none) | the whole car | `results[_<folder>]_whole.md` |
| `--feed aero` / `--feed aids` / `--feed brakes` | one system from the recording | `results[_<folder>]_feed_<system>.md` |
| `--feed drivetrain` | engine and drivetrain from the recording; the aids are then fed too (they act on the engine) | `results[_<folder>]_feed_drivetrain_aids.md` |
| `--feed aero,aids` | the car of Task 09 | `results[_<folder>]_powertrain.md` |
| `--feed brakes,drivetrain,aero,aids` | the rolling chassis of Task 08 | `results[_<folder>].md` |

Two things the new drives made necessary in the fed modes: a fed stability aid hands its
torque calls over from the tape; a fed differential lock on a car with its own brakes takes the
wheels' brake torques from the recording in the steps the lock acted (the recording holds only
the sum). With fed brakes on a car whose lock acts, the aids are fed as well, with a note
(the recorded brake torques already contain the lock's part). That closes question 4 of the
Task 09 report.

That full run found one fault, in the feed and not in the car: with the drivetrain fed, the
driven wheels' speed was handed over just after the brakes had run, so the Rust brakes worked
out the temperature of the driven wheels' discs from the speed of one step earlier. It showed
on the two cars with disc temperatures (`f2004_pt_street`, the F40) as a difference in the
last digits of those two temperatures and in nothing else, and had been there since Task 09,
which ran this mode on the F2004 only (no disc temperatures). The feed now hands the speed over
before the step (`replay::step_recorded`), where the game's drivetrain left it.

### 3.6 Can the comparison fail?

`chassis_compare faults random` (the F2004's 60 s random drive): the twelve powertrain faults
of Task 09 and twelve new ones, one at a time. Every one is noticed:

| Fault | What is changed | Bit-exact steps | Noticed at | First value that differs |
|---|---|---|---|---|
| `brake_power` | brake torque at full pedal one bit up | 0.000 % (0/20001) | step 0 | brakes.brakePowerMultiplier: game 1.0 (0x3f800000) / Rust 1.0000001 (0x3f800001) |
| `front_bias` | front brake bias one bit up | 0.000 % (0/20001) | step 0 | brakes.frontBias: game 0.55 (0x3f0ccccd) / Rust 0.5500001 (0x3f0cccce) |
| `engine_inertia` | engine inertia one bit up | 0.000 % (0/20001) | step 0 | drivetrain.engine.velocity: game 1.2857142914162607 (0x3ff49249261a1f59) / Rust 1.2857141545688766 (0x3ff49249015e0ab5) |
| `limiter` | rev limiter one part in 8 million lower | 0.000 % (0/20001) | step 0 | page.currentMaxRpm: game 18800 / Rust 18799 |
| `clutch_torque` | clutch capacity one (double-precision) bit up | 2.060 % (412/20001) | step 412 | drivetrain.drive.velocity: game 0.06122086279984092 (0x3faf5857471dcefa) / Rust 0.06122086279984093 (0x3faf5857471dcefc) |
| `clutch_inertia` | gearbox inertia one bit up | 2.035 % (407/20001) | step 407 | drivetrain.drive.velocity: game -0.000671380011446739 (0xbf45fff1989efc44) / Rust -0.0006713800011031639 (0xbf45fff192ef41b2) |
| `first_gear` | first gear's ratio one (double-precision) bit up | 2.035 % (407/20001) | step 407 | drivetrain.ratio: game 17.645820027141554 (0x4031a5547617acc0) / Rust 17.645820027141557 (0x4031a5547617acc1) |
| `final_ratio` | final drive ratio one bit up | 2.035 % (407/20001) | step 407 | drivetrain.drive.velocity: game -0.000671380011446739 (0xbf45fff1989efc44) / Rust -0.0006713799943631562 (0xbf45fff18f3aaf54) |
| `shift_time` | up-shift time one physics step (3 ms) longer | 2.035 % (407/20001) | step 407 | drivetrain.currentGear: game 2 / Rust 1 |
| `diff_power` | differential power ramp one bit up | 0.000 % (0/20001) | step 0 | drivetrain.diffPowerRamp: game 0.19999999 (0x3e4ccccc) / Rust 0.20000002 (0x3e4cccce) |
| `diff_preload` | differential preload one bit up | 0.000 % (0/20001) | step 0 | drivetrain.diffPreLoad: game 21.0 (0x41a80000) / Rust 20.000002 (0x41a00001) |
| `wheel_inertia` | left driven wheel's inertia one (double-precision) bit up | 31.513 % (6303/20001) | step 803 | drivetrain.outShaftL.velocity: game 17.235362973984966 (0x40313c40bf73f469) / Rust 17.235362973984962 (0x40313c40bf73f468) |
| `wing_area` | the rear wing's area one bit up | 0.075 % (15/20001) | step 1 | wing2.dragKG: game 0.00094253256 (0x3a77144a) / Rust 0.0009425327 (0x3a77144c) |
| `wing_cl_gain` | the front wing's lift gain one bit up | 1.630 % (326/20001) | step 1 | wing1.cl: game -0.10641208 (0xbdd9ee94) / Rust -0.10641209 (0xbdd9ee95) |
| `wing_position` | the body's aero point one bit further forward | 1.290 % (258/20001) | step 1 | force call 32 (addLocalForceAtLocalPos on body, aero_drag): b[2]: game -0.05 (0xbd4ccccd) / Rust -0.050000004 (0xbd4cccce) |
| `wing_controller` | the smoothing rate of the first wing controller the car has one bit up | 99.970 % (19995/20001) | step 0 | wing2.angle: game 6.666666 (0x40d55554) / Rust 6.666667 (0x40d55556) |
| `wing_damage` | a dented nose (front damage level 60) | 0.000 % (0/20001) | step 0 | page.carDamage.0: game 0.0 (0x00000000) / Rust 60.0 (0x42700000) |
| `wind` | a 3 m/s wind along the road instead of none | 0.000 % (0/20001) | step 0 | wing0.cd: game 0.0 (0x00000000) / Rust 0.4445 (0x3ee39581) |
| `tc_rate` | traction control checks one step more often | 23.944 % (4789/20001) | step 4789 | engine.electronicOverride: game 1.0 (0x3f800000) / Rust 0.0 (0x00000000) |
| `abs_on` | ABS forced on (a car without it) or switched off (a car with it) | 0.000 % (0/20001) | step 0 | page.abs: game 0.0 (0x00000000) / Rust 0.1 (0x3dcccccd) |
| `stability` | the stability aid at 50 % instead of off | 0.000 % (0/20001) | step 0 | stability.gain: game 0.0 (0x00000000) / Rust 0.5 (0x3f000000) |
| `ff_gain` | the user's force-feedback gain one bit up (only the telemetry page shows it) | 0.360 % (72/20001) | step 46 | page.finalFF: game 7.038664e-7 (0x353cf155) / Rust 7.038665e-7 (0x353cf156) |
| `ride_pickup` | the front ride-height pickup point one bit higher (only the telemetry page shows it) | 6.125 % (1225/20001) | step 0 | page.rideHeight.0: game 0.13990776 (0x3e0f43fb) / Rust 0.13990775 (0x3e0f43fa) |
| `black_flag` | the car is black-flagged at the start (controls dead, put into a pit box at the origin) | 0.000 % (0/20001) | step 0 | car.blackFlagged: game 0 / Rust 1 |

Seven of the new ones are as small as a fault can be: six change a number in its last bit and
are caught by a physical value within a step or two (a wing's drag, lift coefficient, force
point, controller angle) or as soon as the path is taken (`ff_gain`, `ride_pickup`: only the
telemetry page shows them); `tc_rate` shifts a timer by one step and shows the first time the
two timers decide differently, 14 s in. The other five switch something on that the game did
not have (damage, wind, ABS, stability aid, black flag) and are caught at once by the value
itself.

### 3.7 Golden test in `cargo test`

`crates/rustyac-physics/tests/chassis_golden.rs`, five excerpts (`golden/*.chgold`, 1.3 MB),
and `tests/whole_car.rs`, fourteen unit tests for the branches of 3.4 (they pin what the
disassembly says; they are not comparisons with the game):

| File | Car | Steps | What is in Rust |
|---|---|---|---|
| `chassis_slalom_3000_300`, `chassis_kerb_2684_300` | F2004 | 300 each | the rolling chassis; brakes, drivetrain, wings and aids fed (Task 08) |
| `whole_launch_autoclutch_off_522_460` | F2004 | 460 | everything: a standing start by hand clutch, limiter, traction control cutting |
| `whole_brake_2807_400` | F2004 | 400 | everything: full braking from 250 km/h with the first down-shifts |
| `whole_488_gt3_wc_stops_1845_400` | 488 GT3 | 400 | everything: the full brake pedal with the ABS releasing and re-applying |

The three whole-car files hold, per step, the driver's controls and nothing else (a test
checks that every other field is empty, and another that nonsense written into those fields
changes nothing). The expected side is the game's: the six bodies in full and one hash over
every compared value and the force tape. Seven tests: the files read back; whole-car files
hold only the driver; the steps are the game's; a changed steering input, wing force (fed
chassis) and start state are noticed; a changed throttle, brake pedal and a removed paddle
press are noticed; a changed brake pedal is noticed on the 488. A car whose data folder is
missing is skipped with a notice.

---

## 4. What is still not Rust

**For a full single-car drive on a flat road: nothing but inputs.**

| Still an input | Where it comes from in the game |
|---|---|
| pedals, wheel, paddles, handbrake, H-shifter lever, DRS button, headlight switch | the driver's device (`ICarControlsProvider`), after the device's own filtering |
| clicks of the cockpit brake bias, lock / gentle stop / black flag / penalty | jobs the game's main thread and race logic queue for the physics thread |
| air and road temperature, wind, grip level, wear / damage / fuel rates, tyre blankets, session clock and start time, penalty rules | `PhysicsEngine`, `Track`, the session |
| the road under each tyre (height, normal, surface flags) | the track (here: a formula) |
| damage levels | the collision callback |

Inside the step, what is left out needs a track and changes no physics without one: lap timing,
the racing-line locator, track-limit penalties, drift scoring, the fuel-per-lap estimate, the
lap-time meter (the page's `performanceMeter` is 0, as in the recordings).

**Other cars**

| Missing | Cars in `cardata/` |
|---|---|
| four-wheel drive (`AWD`, `AWD2`: `step4WD`, `step4WD_new`, the three differentials' setup items) | 14 |
| KERS / ERS (with the engine's electric torque and coast generators, the page's hybrid fields, the cockpit settings) | 7 |
| strut, axle and multilink suspensions | 30 |
| rear-wheel steering (`ctrl_4ws.ini`) | 2 |
| the gear tab of the real setup screen (replaces a default ratio that is not in the car's list) | 3 affected, 1 of them accepted by the port |

**Real tracks**: contacts (stage 2 of the rigid-body port: contact joints and the pivoting
solver; stage 3: mesh collision), so the car's floor, walls and other cars; the kn5 track mesh
and its ray casts for the tyres; `surfaces.ini` (grip, kerb rumble, pit lane, valid track);
`drs_zones.ini`; the racing line (`fast_lane.ai`) with everything that hangs on it (lap timing,
track limits, penalties, the AI); pit boxes and grid positions; a dynamic track.

**A drivable game**: a window and a renderer; real input devices with the game's filtering
(wheel: `DICarControl`, pad: `JoypadCarControl`, keyboard: `KeyboardCarControl`; read in
`spec_car_step.md` section 5, not ported); force feedback out to a wheel; the real-time loop
(`PhysicsDriveThread`: fixed 3 ms steps against the wall clock, catch-up, pause); the command
queue from the main thread; the shared memory itself (the page is produced, nothing maps it);
the graphics and static pages; sound; more than one car (slipstream is ported, car-to-car
contact is not).

---

## 5. Commands

```
cargo test -p rustyac-physics                      # five golden excerpts (needs cardata/ks_ferrari_f2004 and ks_ferrari_488_gt3)
cargo build --release --manifest-path tools/chassis_compare/Cargo.toml
cargo build --release --manifest-path tools/car_oracle/Cargo.toml

chassis_compare run                                # the 11 F2004 recordings, the whole car in Rust; writes oracle/chassis/results_whole.md
chassis_compare run --dir oracle/car_wc            # likewise car_wc_488, car_wc_exos, car_wc_giulia, car_wc_aids, car_wc_abs1,
                                                   # car_wc_oldaero, car_wc_shell, car_pt, car_pt_street, car_pt_ctrl, car_pt_fwd, car_f40, car_488_gt3,
                                                   # car_tight_stops, car_fallbacks; writes results_<folder>_whole.md
chassis_compare run brake --verbose                # one drive; prints up to 100 differing values
chassis_compare run --feed aero                    # one system back from the recording: brakes, drivetrain, aero, aids (any list)
chassis_compare run --feed aero,aids               # the car of Task 09 (results_powertrain.md)
chassis_compare run --feed brakes,drivetrain,aero,aids   # the rolling chassis of Task 08 (results.md)
chassis_compare faults random                      # 24 deliberate faults in a whole car (faults_whole.md)
chassis_compare excerpt                            # rewrite the five golden excerpts
chassis_compare test-car                           # write the eight altered cars into cardata/

car_oracle list                                    # all scenarios; pt_* and wc_* only run when named with --only / --scenario
car_oracle all --car ks_ferrari_f2004   --out oracle/car_wc        --only wc_pit,wc_stops,wc_stability,wc_wind --steps 3400
car_oracle all --car ks_ferrari_488_gt3 --out oracle/car_wc_488    --only wc_stops,wc_pit --steps 3400
car_oracle all --car lotus_exos_125     --out oracle/car_wc_exos   --only wc_drs --steps 3700
car_oracle all --car ks_alfa_giulia_qv  --out oracle/car_wc_giulia --only wc_spirited --steps 3400
car_oracle all --car f2004_wc_aids      --out oracle/car_wc_aids   --only wc_stops,wc_spirited,wc_damage --steps 2700
car_oracle all --car f2004_wc_aids      --out oracle/car_wc_aids   --only wc_drs --steps 3700
car_oracle all --car f2004_wc_abs1      --out oracle/car_wc_abs1   --only wc_stops --steps 2700
car_oracle all --car f2004_wc_oldaero   --out oracle/car_wc_oldaero --only wc_spirited --steps 2700
car_oracle all --car ks_ferrari_f2004   --out oracle/car_wc_shell  --only wc_shell --steps 3700
car_oracle csv oracle/car_wc/wc_pit.carrec --table telemetry       # the game's page per step as CSV, for check_telemetry.py
```

(Run from the repository folder; `chassis_compare` stands for
`tools/chassis_compare/target/release/chassis_compare.exe`, `car_oracle` for
`tools/car_oracle/target/release/car_oracle.exe`. `chassis_compare` needs only recordings and
the cars' data folders; `car_oracle` needs the game's `acs.exe` and the Visual C++ 2013
runtime. The new recordings take 0.64 GB (2,700 to 3,700 steps each, 14 kB per step); they
are git-ignored and can be deleted and made again with the lines above. A whole
`chassis_compare run` of the F2004 folder takes about half a minute.)

In code, a whole car is three lines:

```rust
let mut car = VanillaCar::new(data_path, ChassisEnvironment::default(), Box::new(Ground::Flat), 1, 60_000.0, ScriptedDevice::default())?;
car.car.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));  car.car.force_position(&Vec3f::new(0.0, 0.0, 0.0));  car.car.session_start()?;
loop { car.device.controls.gas = 1.0;  time += 3.0;  car.step(0.003, time);  let page = car.physics_page(); }
```

(`physics_page()` is `None` for the first 300 steps, the game's warm-up; set
`car.car.telemetry.null_counts = 300` to skip it. Without the game's automatic clutch and
gearbox aids, `car.car.autoclutch` and `car.car.auto_shifter`, the device also has to work the
clutch and the paddles.)

---

## 6. Open questions

### 6.1 Choices made while working unattended

1. **The shell lives in `RollingChassis`, `VanillaCar` is a thin owner.** `RollingChassis::step`
   already was `Car::step`; the missing blocks went into it at their places, with the device
   behind new feed hooks. `VanillaCar` is that chassis with every slot filled plus the device,
   and its feed refuses every hook but the device's. So "the Rust car" has one step function,
   and the older fed modes kept working.
2. **The port was written from the listings; the briefs were the second reading.** The four
   helpers were still writing when aero and aids were ported from the same listings and the
   maps; `spec_car_step.md` arrived in time for the shell; the telemetry page has no brief
   (its helper was last in the queue of three and was stopped by the cut-off) and was written
   from `docs/map/telemetry.md` and the listings. The first full comparison found three
   things (2.4, the first three); after the port four reviewers read all of it against the
   disassembly (6.3).
3. **The aids are one slot with three entry points**, not five slots: they are five small
   structs the game steps at three places, and what would replace them later ("enhanced"
   aids) replaces them together.
4. **New drives instead of new assumptions.** Eight scenarios, four real cars and three altered
   F2004 copies were recorded so that DRS (both modes, the g limit), ABS (1, 2 and 4 channels),
   the differential lock, the stability aid, the pit limiter, wind, damage, all eight wing
   controller inputs, the old one-body aero, and the car-level rules (locks, gentle stop,
   headlights, penalty timers) are measured, not only read. The scripted drivers of the new scenarios
   use the automatic gearbox and clutch so that they work on any car.
5. **The pit limiter test marks the whole road as pit lane**, because that is what switches
   the limiter on in the game (a surface flag); there is no button.
6. **The recorder calls the game's own `PhysicsEngine::setWind`** for the wind drive, and
   `ChassisEnvironment::set_wind` is that function (the first version of both built the vector
   by hand; the review caught that the game's matrix product leaves other signs on zeros, and
   the drive was recorded again: same values, since its wind has no zero component).
7. **`jump_start_penalty_mode` defaults to 1** (teleport to the pits), the engine constructor's
   value; it was 0. No recording is before a session start, so nothing changed in the results.
8. **Car-level events are not ported**, only their effects on the car: a jump start counts in
   `jump_start_events`, a black flag is a flag.
9. **The golden excerpts changed** (format `CHGOLD03`, new key for the car, the DRS button, the
   aids' torque calls): the two chassis-only files have the same steps as before, the two
   powertrain files became whole-car files with only the driver's controls in them, and a
   488 GT3 file was added.
10. **`--feed drivetrain` now feeds the aids too** (they need the engine they act on), and fed
    brakes on a car whose differential lock acts feed the aids as well; both print a note.
    A fed drivetrain hands the driven wheels' speed over before the step, not after the brakes
    (3.5: a fault of the feed that the last full run of all modes found).
11. **Recordings**: 0.64 GB of new ones, all 2,700 to 3,700 steps. Every earlier recording was
    kept and still matches.
12. **`VanillaCar` has a queue for what the game's main thread asks of the physics thread**
    (`RollingChassis::queue`): the jobs run at the start of the next step, when the clock
    already shows that step's time, as in the game. (A lock "for 30 ms from now" depends on it.)
13. **The assist options of the game** (ABS / traction control off, factory, on; stability
    percent) are a function (`AidsBase::apply_driving_assists`) the caller may use; the
    recorder does not apply them (it has no `DrivingAssistManager`), so in the comparisons the
    car's own file decides.
14. **Files outside the repository**: none written by this task's tools. The agent harness
    keeps its own logs (and the output of background commands) in the user's `.claude` and
    temp folders, and the tooling notes for later sessions there got an entry about this task.

### 6.2 Questions

1. **The cockpit's engine-brake setting on the page.** The game's page shows
   `CarAvatar::currentEngineBrakeSetting`, which starts at the engine's default index
   (`engine.ini [COAST_SETTINGS] DEFAULT`); the recorder hands the game's writer a zeroed car
   avatar, so every recording shows 0, also for `f2004_pt_street` (default 2), where the real
   game would show 2. The port's writer starts at the engine's default like the game; the
   comparison sets it to 0 like the recorder. Should the recorder fill that one avatar field
   (it would change `page.engineBrake` of that test car's recordings and nothing else)?
2. **What starts a penalty or a black flag is not ported**: `LapInvalidator::step` (track
   limits, cutting) and `PenaltyManager::step` (drive-through bookkeeping) need a track with a
   racing line. The car's side (timers, locks, teleport) is there and is driven from outside
   (`add_penalty`, `set_black_flag`, `queue`). Port those two with the track, or earlier against
   a synthetic track?
3. **The setup screen's gear tab** (open since Task 09): the port still follows the recorder,
   which does not mirror it. Unchanged by this task.
4. **The game's assist options** (`cfg/assists.ini`): in a real session they always overwrite
   `ACTIVE` of `electronics.ini` for the player's car. The recorder does not apply them, so the
   comparisons run with the car file's own values. `AidsBase::apply_driving_assists` is the
   port of the three jobs (read, not run). Should the recorder get an assists option so that
   "ABS forced on a car without it" is measured too?
5. **DRS zones and the racing-line position** are an input flag today. With a track loader the
   zone test (`DRSManager::isDRSAvailable`, described in 6.3) is a few lines.
6. **Other real cars.** The scripted `wc_*` drives use the automatic gearbox, so any car the
   port accepts (two driven wheels, double wishbones, no hybrid, no rear steering: 68 of the
   113 in `cardata/`) can be recorded and compared with one command per car. Five real cars have been so far
   (F2004, F40, 488 GT3, Lotus Exos 125, Giulia QV).
7. **More than one car** would exercise the wake and the pit-lane ghosting against the game;
   the recorder builds one car. Worth doing before contacts, or together with them?

### 6.3 Review of the untested branches

<!-- REVIEW:BEGIN -->
Four read-only reviewers compared the finished port with the disassembly, one part each, with
the list of what the recordings reach, looking for a difference rather than for agreement
(their notes: `re/scratch/task10/review_aero.md`, `review_aids.md`, `review_car_step.md`,
`review_telemetry.md`). **None found a difference in anything that runs every step**: every
sum, product, compare direction and call order of the wings, the aids, the shell and the page
agrees with the machine code, the NaN directions included. What they found is in loaders, in
values the port had filled in by hand, and in things left out. Each finding was checked against
the listing before anything was changed; where a recording could settle it, one was made.

| Part | Finding | What was done |
|---|---|---|
| Aero | `set_wind` built the wind vector by hand; the game's `setWind` multiplies through a rotation matrix, which leaves `+0.0` where the hand-made form gives `-0.0` (direction 0, or speed 0) | **Fixed**: `set_wind` is the game's function now; the recorder calls the game's own `setWind` and `wc_wind` was recorded again (same values) |
| Aero | the old one-body format (`[DATA]`, no wings) was refused | **Ported** from the reviewer's reading of `AeroMap::addDrag` / `addLift` and the listings; an altered F2004 (`f2004_wc_oldaero`) was recorded and matches in every step (9,464 force calls) |
| Aero | `status.frontShare` of a wing stayed 0 (the game computes `getPointFrontShare`); `status.isVertical` was set for a fin (the game leaves it false in the wing itself) | **Fixed** (both are display values nothing reads) |
| Aero | read order in the loaders (`CD_GAIN` before `CL_GAIN`, the zone gains, the DRS sections before its wings, the file version once per wing): decides only which malformed key is reported first | **Fixed** to the game's order |
| Aero | another car's wake could not be handed to a stepped car | **Fixed**: `RollingChassis::other_wakes`; unit test |
| Aero | DRS zones (`DRSManager::isDRSAvailable`: no zones: allowed everywhere; a zone holds the car when `start <= npos <= end`, a zone that wraps the start line when `npos >= start` in the second half of the lap or `npos <= end` in the first) | stays an input flag until there is a track |
| Aero | `data_<config>` folders; `WARNINGS_AS_ERRORS` | not ported, by the decision of the earlier tasks (2.5) |
| Aids | the search for the start level treats a table value that is not a number as a match (`ucomiss` + `je`), the port did not | **Fixed** (needs a table with an overflowing number; no car has one) |
| Aids | the game's assist options are applied by game code outside the car | **Added** as `AidsBase::apply_driving_assists` (the three jobs), with a unit test; not applied in the comparisons (6.2 item 4) |
| Aids | `[EDL]` dead zones read in the other order | **Fixed** (no effect on a value) |
| Aids | checked and right: every constructor default, the order in `Car::Car`, `ABS` with 1 / 2 / 4 channels, both `cycleMode`s, the stability aid's AI branch, `Tyre::getDX`'s three branches, the differential lock's refusal of gains that are not finite (the game stops there too) | - |
| Shell | `finalFF` of the page left out the device's own gain (`getFFGlobalGain`; 1 for everything but a wheel) | **Fixed**: a device hook, default 1 |
| Shell | the car's name was taken from the wrong folder for cars in `cardata/` (only the "spectator" test reads it) | **Fixed** |
| Shell | jobs "between two steps" saw the previous step's clock, the game's queue runs them after the clock is set (matters for a lock whose end falls into that step) | **Fixed**: `RollingChassis::queue`, run at the start of the step |
| Shell | `powerClassIndex`, `Car::onNewSession`, `isRetired`, `lastCollisionWithCarTime` had no field | **Added** |
| Shell | `Tyre::reset` with a tyre distance that is not a number (a Task 04 file) | **Fixed** |
| Shell | what starts penalties and black flags, lap timing, the racing-line locator, the events | not ported (2.5, 6.2 item 2) |
| Shell | checked and right: the block order of `Car::step`, both branches of `pollControls` term by term, the penalty block, locks, the black-flag teleport (rotation, then position, signs), the collider mask, the jump start in its three modes, `postStep`, `forcePosition` with `Car::reset` member by member, `stepWind`, every new default | - |
| Telemetry | tyres off the track were counted always; the game counts 0 for five seconds after a contact with another car, and only for the player's car | **Fixed** |
| Telemetry | `P2PActivations` / `P2PStatus` were 0 for every car; the engine port has push-to-pass, so such a car would have shown 0 where the game shows 1, 2 or 3 | **Fixed** (no car in `cardata/` has the section) |
| Telemetry | `engineBrake` was 0; the game shows the cockpit setting, which starts at the engine's default index. The recorder's zeroed avatar hides this: here the recording differs from the real game, not only the port | **Fixed** in the port (the comparison zeroes it like the recorder; 6.2 item 1) |
| Telemetry | the writer started warmed up; the game writes nothing for its first 300 steps | **Fixed**: starts at 0 like the game, the comparison sets 300 like the recorder; unit test |
| Telemetry | `MIN_HEIGHT` of 0 or below is not stored by the game (the page is the same either way) | **Fixed** |
| Telemetry | the bump-stop offset of `suspensionTravel` is taken for double-wishbone wheels only | noted in the code for the day another suspension type exists |
| Telemetry | the snapshot's seven `getAction` calls per step, the graphics and static pages | not ported |
| Telemetry | checked and right: all 148 stores against `SPageFilePhysics` (offset, type, source), `rpms` and `currentMaxRpm` truncation, heading / pitch / roll with their clamp and degenerate branch, the ride-height branch (the previous snapshot's speed against 2.0, the minimum height against 0), `computeRideHeight` and both estimates with their heave-spring fall-back, `tyreWear` with an empty curve, `finalFF`'s order | - |

After the fixes every recording was run again (section 3.1) and the unit tests of
`tests/whole_car.rs` were written for the branches of this table that no recording reaches.
<!-- REVIEW:END -->
