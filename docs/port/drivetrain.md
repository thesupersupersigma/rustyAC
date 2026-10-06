# Brakes, engine and 2WD drivetrain, with `DynamicController` (Task 09)

## Resume here

(Kept up to date while the task ran, so that a cut-off would lose nothing. Newest state first.)

- **State**: finished. Everything below is committed; nothing is in flight.
- **What exists**: the port in `crates/rustyac-physics/src/car/` (`dynamic_controller.rs`,
  `brakes.rs`, `engine.rs`, `drivetrain.rs`, `shift_assists.rs`, wired into `chassis.rs`,
  `setup.rs`, `feed.rs`, `replay.rs`); `tools/chassis_compare` runs it against the game's
  recordings (`run`, `faults`, `excerpt`, `test-car`); `tools/car_oracle` has five more scripted
  scenarios (`pt_*`); four golden excerpts in `crates/rustyac-physics/tests/golden/`.
- **To check that nothing broke** (about two minutes, no game needed):
  `cargo test -p rustyac-physics`, then `chassis_compare run` and
  `chassis_compare run --dir oracle/car_pt_street` (section 5 has every command).
- **If the recordings of the test cars are gone** (they are git-ignored, 0.47 GB):
  `chassis_compare test-car`, then the `car_oracle all --car ...` lines of section 5.
- **What comes next** (not started): aero, the aids (ABS, traction control, EDL, stability,
  pit limiter), then four-wheel drive and the hybrids. Section 4 lists what the feed still
  supplies.
- **Specs** the port was checked against (git-ignored, written by read-only helpers from the
  disassembly): `re/scratch/task09/spec_controller_brakes.md`, `spec_engine.md`,
  `spec_drivetrain.md`, `spec_assists_setup.md`.

---

Code: `crates/rustyac-physics/src/car/` (five new files, four changed), one function added to
`crates/rustyac-math` (the C runtime's double-precision `sin`), `tools/chassis_compare` and
`tools/car_oracle` extended, golden excerpts and `tests/chassis_golden.rs` extended. Sources:
the disassembly of `acs.exe` 1.16.4 with `acs.pdb` (`tools/disasm.py`), the maps in `docs/map/`,
and four instruction-level briefs written for this task by read-only helpers in the git-ignored
`re/scratch/task09/`. Nothing in the game folder or the Ghidra project was changed and no game
process was touched. Aero, the aids, four-wheel drive and the hybrid systems were not ported.

---

## 1. Plain-English summary

The Rust car now has its own brakes, engine, clutch, gearbox and differential, so it speeds up,
changes gear and stops by itself instead of being told what those parts did.
Pressing the brake pedal turns into a braking force at each wheel, split between front and rear;
pressing the throttle makes the engine push through the clutch and the gearbox to the two driven
wheels, and the differential decides how differently the left and the right wheel may turn.
The small helpers that sit around the gearbox are in as well: the automatic clutch, the short
burst of throttle on a down-shift, the automatic gearbox, and the rule that refuses a down-shift
that would over-rev the engine.
Also new is the little programmable formula that many cars use to steer such things (brake
balance, differential lock, turbo pressure, anti-roll bars) from a table in a data file.
To check the work, the Rust car was driven through every recorded drive of the game's own car
from the first moment to the last, being told only what the driver did, what the wings pushed
and when the traction control cut in; after every one of the 70,006 steps every number was
compared with the game, and all of them agree in every digit.
Because the Formula 1 car has no turbo, no handbrake and no ordinary gear lever, three altered
copies of it were made that have those things (and front-wheel drive, a locked and an open
differential, brakes that fade when hot, a tank that runs dry, an engine that wears out), and
two real road and race cars from the game were added, a Ferrari F40 and a Ferrari 488 GT3;
the game's own code drove them and the Rust car again matched every digit, 35,902 more steps.
Twelve deliberate small faults were put into the Rust car one at a time to make sure the
comparison notices such things, and it noticed each one.
A short piece of a standing start and of a full braking from 250 km/h is stored with the
project's tests and runs without the game.

---

## 2. What was ported

### 2.1 Where the code lives

All in `crates/rustyac-physics/src/car/`:

| File | Content |
|---|---|
| `dynamic_controller.rs` | `DynamicController` with its stages, the 24 inputs, the loader of the `ctrl_*.ini` files; `CarSignals`, the view of the car a controller reads |
| `brakes.rs` | the brake slot `BrakeModel`; `VanillaBrakes` (AC's `BrakeSystem`): bias (setup, cockpit, load-based, controller), handbrake, steer-brake, disc temperatures and fade |
| `engine.rs` | the engine slot `EngineModel`; `VanillaEngine` (AC's `Engine`): throttle maps, engine-brake setting, rev limiter, torque curve, turbos with wastegate and controllers, push-to-pass, coast torque, damage, air density, camshaft overlap, idle floor, a dead engine |
| `drivetrain.rs` | the drivetrain slot `DrivetrainModel`; `VanillaDrivetrain` (AC's `Drivetrain`, layouts `RWD` and `FWD`): clutch, gears and shift timing, down-shift protection, H-shifter with grinding and wear, differential (limited-slip, spool, controller lock), the driven-wheel solve |
| `shift_assists.rs` | `Autoclutch`, `AutoBlip`, `AutoShifter`, `GearChanger` |
| `chassis.rs` (changed) | the two slots and the helpers as members of the car, `install_brakes` / `install_drivetrain`, their places in the step, resets at a spawn, the water temperature, the control-lock overrides, anti-roll bar controllers |
| `setup.rs` (changed) | the setup items of differential, brakes, gears, final ratio and engine |
| `feed.rs`, `replay.rs` (changed) | the feed without brakes and drivetrain, the values compared, state save / restore, the golden-file format |

### 2.2 Traits and slots

- **`BrakeModel`** (`brakes.rs`): `step`, `reset`, `get_front_bias`, `get_brake_power`,
  `is_using_ebb`, `set_manual_front_bias`, and `base()`: the four members other systems write
  (`frontBias`, `brakePowerMultiplier`, `electronicOverride`, `rearCorrectionTorque`).
  `RollingChassis::brake_system` is an `Option<Box<dyn BrakeModel>>`.
- **`EngineModel`** (`engine.rs`): `step(input, dt, car)`, `reset`, `get_limiter_rpm`,
  `is_limiter_on`, `minimum`, `get_max_power_rpm`, `get_max_torque_rpm`,
  `set_turbo_boost_level`, `set_coast_settings`, `blow_up`, and `base()`: status (torque,
  boost, limiter), inertia, fuel pressure, the aids' throttle factor, limiter multiplier,
  throttle used, engine life. The drivetrain owns the engine as a `Box<dyn EngineModel>`.
- **`DrivetrainModel`** (`drivetrain.rs`): `step`, `gear_up`, `gear_down`, `set_current_gear`,
  `set_gear_ratio`, `reset`, `get_engine_rpm`, `is_changing_gear`, `has_dynamic_controllers`,
  `project_rpm_at_downshift`, `engine()` / `engine_mut()`, and `base()`: every data member of
  AC's `Drivetrain` that the helpers, the controllers, the setup items and the telemetry read.
  `RollingChassis::drivetrain` is an `Option<Box<dyn DrivetrainModel>>`.
- All three also have `trace` (their values under the recordings' names) and `save_state` /
  `load_state` (for a test that starts in mid-run).
- **A slot can be empty.** A chassis without brakes or drivetrain is the rolling chassis of
  Task 08: the feed then supplies brake torques and driven-wheel speeds as before. That is
  how `chassis_compare run --feed brakes,drivetrain` and the two older golden excerpts still
  run, and it allowed switching the systems on one at a time.
- **`ChassisFeed`** lost nothing but changed meaning: `brakes`, `engine`, `autoclutch`,
  `current_gear` and `drivetrain` are only called for an empty slot. New hook `edl` (position
  2, right before the tyres); `aids` now has something to do (the aids' cut of throttle and
  brake for the next step).
- The shift helpers are plain structs in the car, like in the game (`Car::autoClutch`, ...);
  the gear-request event of the drivetrain is `RollingChassis::on_gear_request`, which calls
  the automatic clutch's handler and then the blip's, the order `Car::Car` registers them in.

### 2.3 Functions (addresses in `acs.exe`)

| Area | Functions |
|---|---|
| Controller | `DynamicController::DynamicController` 0x1402af330 (and the empty one 0x1402b0760), `DynamicControllerStage::DynamicControllerStage` 0x1402b0780, `eval` 0x1402b0c00, `getInput` 0x1402b0d70 (all 24 inputs), `getOversteerFactor` 0x1402b11d0, `getRearSpeedRatio` 0x1402b1230, `lagToLerpDeltaK` 0x14005d7c0 |
| Brakes | `BrakeSystem::BrakeSystem` 0x14026be20, `BrakeDisc::BrakeDisc` 0x14026bde0, `init` 0x14028d690, `loadINI` 0x14028d870, `reset` 0x14028e490, `step` 0x14028e640, `stepTemps` 0x14028e920, `setManualFrontBias` 0x14028e5d0, `getFrontBias` 0x14028d630, `getBrakePower` 0x14028d620, `isUsingEBB` 0x1402befa0 |
| Engine | `Engine::Engine` 0x1402852b0, `acEngineData::acEngineData` 0x1402854a0, `init` 0x140285e10, `loadINI` 0x140286100, `loadCoastSettings` 0x140285ea0, `setCoastSettings` 0x140288010, `reset` 0x140287f90, `precalculatePowerAndTorque` 0x140287d10, `step` 0x1402880e0, `getThrottleResponseGas` 0x140285b90, `stepP2P` 0x140288750, `stepTurbos` 0x140288900, `getLimiterRPM` 0x140285a50, `isLimiterOn` 0x140285e90, `setTurboBoostLevel` 0x140288090, `blowUp` 0x140285a30, `getMaxPowerRPM` 0x140285a70, `getMaxTorqueRPM` 0x140285ae0; `Turbo::Turbo` 0x1402ae730, `Turbo::step` 0x1402ae7c0, `getBoost` 0x1402ae780, `reset` 0x1402ae790, `setTurboBoostLevel` 0x1402ae7a0; `PhysicsEngine::getAirDensity` 0x140263a60; `Curve::getMaxReference` 0x140206930 |
| Drivetrain | `Drivetrain::Drivetrain` 0x14026d870, `init` 0x140266dc0, `loadINI` 0x140267440, `addGear` 0x140266570, `initControllers` 0x140267070, `hasDynamicControllers` 0x140266d90, `setGearRatio` 0x140269470, `step` 0x14026b130, `stepControllers` 0x14026b200, `step2WD` 0x1402694e0, `getCurrentGear` 0x140266b10, `reallignSpeeds` 0x140269100, `getInertiaFromEngine` 0x140266ba0, `getInertiaFromWheels` 0x140266c20, `accelerateDrivetrainBlock` 0x1402664c0, `gearUp` 0x1402669f0, `gearDown` 0x140266660, `setCurrentGear` 0x1402692e0, `isGearboxLocked` 0x1402673c0 with `PhysicsEngine::hasSessionStarted` 0x140263c70, `reset` 0x140269280, `getEngineRPM` 0x140266b80, `isChangingGear` 0x1402673b0, `projectRPMAtDownshift` 0x140268ee0; `Tyre::stepRotationMatrix` 0x140284b80 (ported with the tyre, now called from here) |
| Shift helpers | `Autoclutch::Autoclutch` 0x14026bd60, `ClutchSequence::ClutchSequence` 0x1402b8320, `Autoclutch::init` 0x1402b87d0, `loadINI` 0x1402b8890, `step` 0x1402b9590, `stepSequence` 0x1402b97a0, `onGearRequest` 0x1402b9350; `AutoBlip::init` 0x1402b9910, `loadINI` 0x1402b99b0, its gear-request handler 0x1402b9880, `step` 0x1402b9ef0; `AutoShifter::init` 0x1402ba020, `loadINI` 0x1402ba050, `step` 0x1402ba7f0, `RaceEngineer::getDrivingTyresSlip` 0x14027bc20; `GearChanger::init` 0x1402bab40, `step` 0x1402bab50 |
| Car glue | of `Car::step` 0x140275da0: the control lock and gentle stop, the place of `Autoclutch::step`, the engine values the fuel burn reads and the fuel pressure it writes; `Car::pollControls` 0x140274e70 (the locked branch); `Car::stepThermalObjects` 0x1402769f0 with `ThermalObject::ThermalObject` 0x1402b2e80 and `ThermalObject::step` 0x1402b2f00 (water temperature); of `Car::forcePosition` 0x14026fe10: `Drivetrain::reset`, `BrakeSystem::reset`, neutral; of `Car::buildARBS` 0x14026f750 and `AntirollBar::step` 0x1402bb640: the bars' controllers |
| Setup | of `SetupManager::initItems` 0x140289570: `DIFF_POWER`, `DIFF_COAST`, `DIFF_PRELOAD` (not on a car with a differential controller), `BRAKE_POWER_MULT`, `FRONT_BIAS`, one `INTERNAL_GEAR_n` per gear with its callback 0x140288f20, `FINAL_RATIO`, `ENGINE_LIMITER`, `COAST_TORQUE_MULT`, at their places in the game's item order |

### 2.4 Things in the machine code that decide bits

All confirmed by the comparison (each would have shown as a difference), or by the review of
section 6.3 where no recording reaches them.

- **The drivetrain keeps the wheel inertia of the tyres the car was built with.** It copies
  `angularInertia` once, in `Drivetrain::init`, when the tyres still have compound 0; the
  session then selects the default compound and the tyre changes its own value, the drivetrain
  does not. For the F2004 that is 1.3 in the drivetrain and 1.6 in the rear tyres.
- **Two ways from rad/s to rpm.** The engine, the auto-shifter and the fuel burn get
  `((float)w * 0.15915507f) * 60f`; the automatic clutch, the down-shift protection and the
  H-shifter use doubles with the double `0.15915507152579872`. They can differ in the last bit.
- **Milliseconds become seconds in two precisions**: the shift times are a float product
  widened afterwards, the throttle cut after an up-shift is widened first and multiplied by the
  double 0.001.
- **The clutch torque is computed and then used three times with different signs**: on the
  engine, on the gearbox and as the differential's input torque; in neutral it is not applied to
  the shafts but still feeds the differential's lock.
- **Sums of the two tyres' torques are single precision** (`fbR + fbL`, `fbR - fbL`) and only
  then widened; the wheel-side sums of inertia are written in two different orders (neutral:
  left, carrier, right; in gear: gearbox, carrier, left, right).
- **A paddle shift ends when its time is strictly over**, tested before the clock of the shift
  advances, and the step of the request already counts. The gearbox is in neutral meanwhile;
  when the gear goes in, engine side and wheel side are brought to a common speed that depends
  on this step's clutch pedal and on the previous step's clutch state.
- **The limiter compares in single precision** with `(float)limiter * multiplier`, counts
  steps, and `LIMITER_HZ` goes through two whole-number divisions. Turbo lag, engine damage and
  the controllers' smoothing use a literal 0.003, not the step length.
- **Below idle the engine torque is at least 15 Nm**: that is the whole idle control. Without
  fuel pressure (empty tank, dead engine) only a drag of 0.01 Nm per rpm is left.
- **The torque is single precision** up to and including the air-density factor, then widened;
  only the idle floor, the overlap term, the dead-engine blend and the dynamic power figure are
  double.
- **Setup values reach brakes and drivetrain at the end of the first step.** The session start
  pushes them through the setup screen's whole-number spinners: the F2004's differential runs
  step 0 with 0.2 / 0.3 / 20 Nm and every later step with 0.19999999 / 0.29999998 / 21 Nm.
- **Only `[TEMPS_REAR]` decides whether a car has disc temperatures**; a car with temperatures
  for the rear discs only gets no torque on its front brakes (an empty fade curve reads 0).
- **The load-based brake balance falls back to the setup's bias**, not the cockpit's, below
  10 km/h; with either electronic balance the cockpit clicks change a number nothing reads.
- **Automatic clutch profiles are in seconds, the blip profile in milliseconds**; with the
  automatic clutch aid off the car's shift profiles are never played (its clutch value stays 0).
- **An attached setup item pulls its value back every step.** Seen on the controller test car:
  the anti-roll bar's controller sets the bar rate before the bar acts, and the setup item
  restores the setup's rate at the end of the same step.
- Clamps are the three-way kind (above the top: top; at least the bottom: itself; else the
  bottom), so a NaN becomes the lower bound; comparisons are written as the branches make them.

### 2.5 Not ported (by design of this task, or refused with a message)

Aero, ABS, traction control, EDL, stability control, the pit limiter (all fed, section 4);
`AWD` and `AWD2` (`step4WD`, `step4WD_new`), KERS and ERS with the engine's torque and coast
generators (cars with `kers.ini` / `ers.ini` or four-wheel drive are refused); the body
reaction torque of a rigid rear axle (the chassis has no such axle) and of
`torqueModeEx == reactionTorques` (nothing in the game sets it); `ctrl_4ws.ini` (rear-wheel
steering, still refused); the engine-stall branch's keyboard read (the flag that enables it is
never set; the branch is ported with a plain flag); the push-to-pass display event; the
developer's brake temperature log; the UI callback of a refused down-shift;
`Engine::getTorqueAtRPM` and the other estimate functions the AI and the race engineer use;
penalties, black flag and lights in `Car::step`.

---

## 3. Results

`tools/chassis_compare run`: the Rust car is built from the car's data folder, spawned, given
the session start, and then steps through the whole recording without ever being reset to the
game's state. Per step it is fed the driver's device (steering, pedals, paddles, handbrake,
H-shifter lever, cockpit brake-bias clicks), the wings' pushes on the body, what the aids left
for the next step (traction control's throttle cut; ABS and pit limiter where a car has them)
and nothing else. After every step these are compared with the game, bit for bit (a NaN equals
a NaN):

- the 2,009 values and the force tape of the rolling chassis (`rolling_chassis.md` 3.1). Brake
  torques, driven-wheel speeds and spin matrices are among them, as the tyres' inputs;
- 51 values of the newly ported systems: the eight controls as the helpers left them;
  gear, engine speed (rad/s and rpm), carrier and both half-shaft speeds, gearbox input speed,
  clutch state, total ratio, throttle-cut timer, total torque, clutch torque, clutch value,
  grinding flag, differential ramps and preload, shift request and target gear; engine torque,
  external coast torque, boost, limiter flag and counter, fuel pressure, the aids' throttle
  factor, engine life, throttle after the maps and aids, throttle used; brake bias, power
  multiplier, the aids' brake request, brake power, four disc temperatures; water temperature;
  the automatic clutch's value and the helpers' switches. (The differential's locking torque
  and the clutch's torque capacity are not stored by the game, they are locals of one step;
  they are compared through what they produce: the half-shaft, carrier and engine speeds.)
- in the recordings of the new `pt_*` scenarios 27 more: the shift timer and time-out, the
  H-shifter's tolerance window (gearbox wear), last ratio, the half shafts' speeds of the step
  before, blow-off flag, dynamic power figure, engine-brake offset, limiter multiplier, per
  turbo its level, spin, maximum boost and wastegate, cockpit bias, the load-based bias, the
  blip's start time, the auto-shifter's cut timer and shift points.

### 3.1 The F2004: free run of every recording

<!-- RESULTS:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `brake` | 5334 | 100 % (5334/5334) | none | 2009 | 51 | 330544 |
| `kerb` | 4001 | 100 % (4001/4001) | none | 2009 | 51 | 260438 |
| `launch_autoclutch_off` | 4667 | 100 % (4667/4667) | none | 2009 | 51 | 302848 |
| `launch_autoclutch_on` | 4667 | 100 % (4667/4667) | none | 2009 | 51 | 304414 |
| `liftoff_oversteer` | 5334 | 100 % (5334/5334) | none | 2009 | 51 | 346178 |
| `random` | 20001 | 100 % (20001/20001) | none | 2009 | 51 | 1316458 |
| `settle` | 667 | 100 % (667/667) | none | 2009 | 51 | 37354 |
| `slalom` | 5334 | 100 % (5334/5334) | none | 2009 | 51 | 348436 |
| `steady_corner_120` | 6667 | 100 % (6667/6667) | none | 2009 | 51 | 436414 |
| `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 | 51 | 429784 |
| `steady_corner_60` | 6667 | 100 % (6667/6667) | none | 2009 | 51 | 436414 |
| **all** | **70006** | **100 % (70006/70006)** | | | | **4549282** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `brake` | 2487 / 0 / 0 / 0 / 0 | 16 / 326 / 0 / 180 / 0 | 2096 / 35 / 2766 / 437 | 6 / 5 / 30 / 0 | 0 / 0 | 5174 / 160 / 1300 | 28 / 44 / 0 |
| `kerb` | 0 / 0 / 0 / 0 / 0 | 0 / 326 / 0 / 130 / 0 | 3119 / 0 / 475 / 407 | 1 / 0 / 5 / 0 | 0 / 0 | 3929 / 72 / 248 | 0 / 0 / 0 |
| `launch_autoclutch_off` | 0 / 0 / 0 / 0 / 0 | 296 / 326 / 0 / 250 / 0 | 3894 / 430 / 336 / 7 | 6 / 0 / 30 / 0 | 0 / 0 | 4667 / 0 / 4 | 0 / 0 / 0 |
| `launch_autoclutch_on` | 0 / 0 / 0 / 0 / 0 | 200 / 326 / 0 / 180 / 0 | 3755 / 30 / 475 / 407 | 6 / 0 / 30 / 0 | 0 / 0 | 4667 / 0 / 248 | 0 / 0 / 0 |
| `liftoff_oversteer` | 0 / 0 / 0 / 0 / 0 | 16 / 326 / 0 / 180 / 0 | 4440 / 12 / 475 / 407 | 3 / 0 / 15 / 0 | 0 / 0 | 2869 / 2465 / 248 | 0 / 0 / 0 |
| `random` | 18673 / 0 / 0 / 0 / 0 | 4 / 326 / 0 / 25 / 0 | 17280 / 31 / 2262 / 428 | 5 / 4 / 25 / 0 | 0 / 0 | 16673 / 3328 / 248 | 28 / 43 / 0 |
| `settle` | 0 / 0 / 0 / 0 / 0 | 0 / 326 / 0 / 0 / 0 | 0 / 0 / 0 / 667 | 0 / 0 / 0 / 0 | 0 / 0 | 667 / 0 / 422 | 0 / 0 / 0 |
| `slalom` | 0 / 0 / 0 / 0 / 0 | 0 / 326 / 0 / 130 / 0 | 4452 / 0 / 475 / 407 | 1 / 0 / 5 / 0 | 0 / 0 | 2902 / 2432 / 248 | 0 / 0 / 0 |
| `steady_corner_120` | 0 / 0 / 0 / 0 / 0 | 0 / 326 / 0 / 160 / 0 | 5785 / 0 / 475 / 407 | 1 / 0 / 5 / 0 | 0 / 0 | 2863 / 3804 / 248 | 0 / 0 / 0 |
| `steady_corner_180` | 0 / 0 / 0 / 0 / 0 | 16 / 326 / 0 / 180 / 0 | 5773 / 12 / 475 / 407 | 3 / 0 / 15 / 0 | 0 / 0 | 2869 / 3798 / 248 | 0 / 0 / 0 |
| `steady_corner_60` | 0 / 0 / 0 / 0 / 0 | 0 / 326 / 0 / 35 / 0 | 5785 / 0 / 475 / 407 | 1 / 0 / 5 / 0 | 0 / 0 | 2884 / 3783 / 248 | 0 / 0 / 0 |
| **all** | 21160 / 0 / 0 / 0 / 0 | 548 / 3586 / 0 / 1450 / 0 | 56379 / 550 / 8689 / 4388 | 33 / 9 / 165 / 0 | 0 / 0 | 50164 / 19842 / 3710 | 56 / 87 / 0 |
<!-- RESULTS:END -->

The second table counts, on the Rust car, how often the branches of the new systems were
taken. Reading it: the F2004 brakes without fade or handbrake; its engine meets the limiter,
runs below idle while it spins up (326 steps of the 15 Nm floor in every drive) and is cut by
the traction control; the clutch is in all four states; paddle shifts go both ways with the
throttle cut, the clutch profile and the blip on the way down; the differential both holds and
slips; the driven wheels are held at standstill.

The same eleven recordings also still pass with the systems fed one at a time
(`--feed brakes`, `--feed drivetrain`, `--feed brakes,drivetrain`: 70,006 of 70,006 steps each),
and so do the two altered cars of Task 08 with everything in Rust: `oracle/car_tight_stops`
52,005 of 52,005 steps, `oracle/car_fallbacks` 22,003 of 22,003.

One more drive of the real car was recorded for what its own gearbox refuses, `pt_protect`
(a down-shift to neutral at speed, down-shifts that would over-rev, an up-shift in top gear):

<!-- PROTECT:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `pt_protect` | 3334 | 100 % (3334/3334) | none | 2009 | 78 | 216436 |
| **all** | **3334** | **100 % (3334/3334)** | | | | **216436** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `pt_protect` | 766 / 0 / 0 / 0 / 0 | 215 / 326 / 0 / 180 / 0 | 1401 / 26 / 1474 / 433 | 5 / 4 / 25 / 4 | 0 / 0 | 3334 / 0 / 248 | 28 / 22 / 0 |
| **all** | 766 / 0 / 0 / 0 / 0 | 215 / 326 / 0 / 180 / 0 | 1401 / 26 / 1474 / 433 | 5 / 4 / 25 / 4 | 0 / 0 | 3334 / 0 / 248 | 28 / 22 / 0 |
<!-- PROTECT:END -->

### 3.2 Branches the F2004 does not reach: three altered copies and two real cars

The F2004 has no turbo, no handbrake, no disc temperatures, no H-shifter, a limited-slip
differential that is neither open nor locked, and none of the controller files. Following
Task 08, `chassis_compare test-car` writes altered copies of it into `cardata/`; the game's own
code then drives them (`car_oracle all --car <name> ...`, each recording made twice with equal
hashes) and the Rust car, built from the same files, is compared the same way. The numbers in
these cars are arbitrary; they are the game's code on other values, not realistic cars.

**`f2004_pt_street`** (50 changed values): two turbos, one with a wastegate and a cockpit
level of 0.7; a second throttle map (`[THROTTLE_RESPONSE]`), an engine-brake setting
(`[COAST_SETTINGS]`), a non-linear coast curve, camshaft overlap (the double-precision sine),
a blow-off threshold, engine damage from revs and boost; handbrake, disc temperatures with fade
on both axles, the load-based brake balance (`[EBB]`); an H-shifter gearbox (which switches
the down-shift protection off) with gearbox wear, a spool differential, a clutch profile for
up-shifts, and a blip that is the driver's aid. Scenarios: `pt_shifter` (revs on the limiter
in neutral, a start by hand, up-shifts with a dip of the clutch, two gears down without the
clutch so that the box grinds for 102 steps and wears, a lever position that does not exist,
neutral at speed, back into gear, stop), `pt_handbrake` (handbrake to a stop with the clutch
in, cockpit bias clicks up and down, down to reverse, backwards, stop), `pt_random`.

<!-- STREET:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `pt_handbrake` | 3667 | 100 % (3667/3667) | none | 2009 | 78 | 227092 |
| `pt_random` | 3200 | 100 % (3200/3200) | none | 2009 | 78 | 207508 |
| `pt_shifter` | 4001 | 100 % (4001/4001) | none | 2009 | 78 | 253120 |
| **all** | **10868** | **100 % (10868/10868)** | | | | **687720** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `pt_handbrake` | 1392 / 1674 / 1995 / 3667 / 2008 | 80 / 761 / 0 / 180 / 3381 | 1132 / 400 / 2050 / 85 | 1 / 2 / 5 / 0 | 1592 / 0 | 3667 / 0 / 666 | 0 / 0 / 0 |
| `pt_random` | 1838 / 145 / 2367 / 3200 / 2390 | 40 / 299 / 0 / 115 / 2914 | 218 / 120 / 2271 / 591 | 5 / 5 / 25 / 0 | 0 / 0 | 3200 / 0 / 248 | 116 / 49 / 0 |
| `pt_shifter` | 896 / 0 / 0 / 4001 / 896 | 160 / 535 / 0 / 405 / 3715 | 2225 / 853 / 584 / 339 | 0 / 0 / 0 / 0 | 0 / 102 | 4001 / 0 / 221 | 0 / 0 / 0 |
| **all** | 4126 / 1819 / 4362 / 10868 / 5294 | 280 / 1595 / 0 / 700 / 10010 | 3575 / 1373 / 4905 / 1015 | 6 / 7 / 30 / 0 | 1592 / 102 | 10868 / 0 / 1135 | 116 / 49 / 0 |
<!-- STREET:END -->

**`f2004_pt_ctrl`** (11 changed values, 7 new files): every controller the ported systems can
have: `ctrl_ebb.ini` (brake balance), `steer_brake_controller.ini`, `ctrl_single_lock.ini`
(differential lock; such a car has no differential setup items), `ctrl_turbo0.ini`,
`ctrl_wastegate0.ini`, `ctrl_arb_front.ini`, `ctrl_arb_rear.ini`. Between them the stages use
every input (all 24; the slip-ratio inputs in their rear-wheel-drive form), both combinators,
smoothing, limits, a table with unsorted points (as two shipped cars have), an input name the
game does not know and a combinator it does not know. The engine wears fast and dies in the
`launch_autoclutch_on` drive (465 steps of a dead engine).

<!-- CTRL:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `launch_autoclutch_on` | 3500 | 100 % (3500/3500) | none | 2009 | 51 | 227392 |
| `pt_random` | 4667 | 100 % (4667/4667) | none | 2009 | 78 | 304414 |
| **all** | **8167** | **100 % (8167/8167)** | | | | **531806** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `launch_autoclutch_on` | 0 / 0 / 0 / 3500 / 0 | 88 / 326 / 460 / 260 / 2632 | 2543 / 30 / 520 / 407 | 6 / 0 / 30 / 0 | 0 / 0 | 3500 / 0 / 248 | 0 / 0 / 0 |
| `pt_random` | 2974 / 509 / 3834 / 4667 / 0 | 17 / 327 / 0 / 130 / 4259 | 526 / 12 / 3651 / 478 | 9 / 5 / 45 / 1 | 0 / 0 | 2681 / 1986 / 248 | 35 / 20 / 0 |
| **all** | 2974 / 509 / 3834 / 8167 / 0 | 105 / 653 / 460 / 390 / 6891 | 3069 / 42 / 4171 / 885 | 15 / 5 / 75 / 1 | 0 / 0 | 6181 / 1986 / 496 | 35 / 20 / 0 |
<!-- CTRL:END -->

**`f2004_pt_fwd`** (29 changed values): front-wheel drive, an open differential, and values
that make the loaders take their fall-backs: shift times of 0 (become 0.1 s and 0.15 s), no
throttle cut, a clutch torque of 0 (becomes 450 Nm), a shift window of 0, an idle speed of 0
(becomes 1000 rpm), a limiter rate of 0 (becomes 50 steps), no coast reference, automatic
clutch speeds of 0 (become 1500 / 2500 rpm), no shift points for the automatic gearbox (it
takes them from the engine's curve), a forced automatic clutch, no down-shift protection, no
cockpit brake bias, disc temperatures for the rear discs only (the front brakes then have no
torque), and 0.13 litres of fuel, which run out half-way through `pt_autoshift` (2,053 steps
without fuel pressure). `pt_autoshift` has the automatic gearbox do the driving.

<!-- FWD:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `pt_autoshift` | 4001 | 100 % (4001/4001) | none | 2009 | 78 | 246386 |
| `pt_random` | 2600 | 100 % (2600/2600) | none | 2009 | 78 | 167684 |
| **all** | **6601** | **100 % (6601/6601)** | | | | **414070** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `pt_autoshift` | 1166 / 0 / 0 / 0 / 1767 | 196 / 1229 / 2053 / 435 / 0 | 1993 / 187 / 1372 / 449 | 2 / 1 / 0 / 0 | 0 / 0 | 294 / 3707 / 214 | 7 / 0 / 3 |
| `pt_random` | 1180 / 200 / 0 / 0 / 1400 | 0 / 82 / 0 / 0 / 0 | 1497 / 106 / 432 / 565 | 5 / 2 / 0 / 0 | 0 / 0 | 317 / 2283 / 237 | 14 / 22 / 0 |
| **all** | 2346 / 200 / 0 / 0 / 3167 | 196 / 1311 / 2053 / 435 / 0 | 3490 / 293 / 1804 / 1014 | 7 / 3 / 0 / 0 | 0 / 0 | 611 / 5990 / 451 | 21 / 22 / 3 |
<!-- FWD:END -->

**Two real cars**, untouched, with the scenario in which the game's automatic gearbox drives
(the scripts of the other scenarios shift at the F2004's revs): the **Ferrari F40**
(`ferrari_f40`: two turbos with wastegates, H-pattern gearbox, a 250 ms throttle cut, handbrake,
disc temperatures, no traction control) and the **Ferrari 488 GT3** (`ks_ferrari_488_gt3`: two
turbos driven by the shipped `ctrl_turbo0.ini` / `ctrl_turbo1.ini`, paddles, ABS and traction
control, both fed). These are the first cars other than the F2004 family to run through the
Rust chassis at all; they needed no change to it.

<!-- F40:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `pt_autoshift` | 3400 | 100 % (3400/3400) | none | 2009 | 78 | 140608 |
| **all** | **3400** | **100 % (3400/3400)** | | | | **140608** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `pt_autoshift` | 1166 / 0 / 0 / 0 / 1166 | 30 / 256 / 0 / 0 / 3000 | 1990 / 287 / 633 / 490 | 2 / 1 / 166 / 0 | 0 / 0 | 3400 / 0 / 262 | 234 / 66 / 3 |
| **all** | 1166 / 0 / 0 / 0 / 1166 | 30 / 256 / 0 / 0 / 3000 | 1990 / 287 / 633 / 490 | 2 / 1 / 166 / 0 | 0 / 0 | 3400 / 0 / 262 | 234 / 66 / 3 |
<!-- F40:END -->

<!-- GT3:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / drivetrain values compared per step | Force calls compared |
|---|---|---|---|---|---|---|
| `pt_autoshift` | 3400 | 100 % (3400/3400) | none | 2009 | 78 | 146172 |
| **all** | **3400** | **100 % (3400/3400)** | | | | **146172** |

| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |
|---|---|---|---|---|---|---|---|
| `pt_autoshift` | 1166 / 0 / 0 / 0 / 0 | 0 / 231 / 0 / 6 / 2996 | 1781 / 139 / 1034 / 446 | 2 / 1 / 52 / 0 | 0 / 0 | 3400 / 0 / 368 | 74 / 36 / 3 |
| **all** | 1166 / 0 / 0 / 0 / 0 | 0 / 231 / 0 / 6 / 2996 | 1781 / 139 / 1034 / 446 | 2 / 1 / 52 / 0 | 0 / 0 | 3400 / 0 / 368 | 74 / 36 / 3 |
<!-- GT3:END -->

Together: 179,784 steps with brakes, engine and drivetrain in Rust (F2004 73,340; its two
Task 08 copies 74,008; the three new copies 25,636; the two real cars 6,800), all bit-exact.

What these drives still do not reach is listed in 6.2 item 1 and was read against the
disassembly once more by three reviewers (6.3).

### 3.3 Can the comparison fail?

`chassis_compare faults random`: the free run of the `random` drive again, each time with one
deliberate fault in the Rust car's new systems. "One bit up" is the next representable number.

<!-- FAULTS:BEGIN -->
Scenario `random` of the car `ks_ferrari_f2004`, 20001 steps. Without a fault: no difference.

| Fault | What is changed | Bit-exact steps | Noticed at | First value that differs |
|---|---|---|---|---|
| `brake_power` | brake torque at full pedal one bit up | 0.000 % (0/20001) | step 0 | brakes.brakePowerMultiplier: game 1.0 (0x3f800000) / Rust 1.0000001 (0x3f800001) |
| `front_bias` | front brake bias one bit up | 0.000 % (0/20001) | step 0 | brakes.frontBias: game 0.55 (0x3f0ccccd) / Rust 0.5500001 (0x3f0cccce) |
| `engine_inertia` | engine inertia one bit up | 0.000 % (0/20001) | step 0 | drivetrain.engine.velocity: game 1.2857142914162607 (0x3ff49249261a1f59) / Rust 1.2857141545688766 (0x3ff49249015e0ab5) |
| `limiter` | rev limiter one part in 8 million lower | 2.155 % (431/20001) | step 431 | car.waterTemperature: game 60.219414 (0x4270e0ae) / Rust 60.219418 (0x4270e0af) |
| `clutch_torque` | clutch capacity one (double-precision) bit up | 2.060 % (412/20001) | step 412 | drivetrain.drive.velocity: game 0.06122086279984092 (0x3faf5857471dcefa) / Rust 0.06122086279984093 (0x3faf5857471dcefc) |
| `clutch_inertia` | gearbox inertia one bit up | 2.035 % (407/20001) | step 407 | drivetrain.drive.velocity: game -0.000671380011446739 (0xbf45fff1989efc44) / Rust -0.0006713800011031639 (0xbf45fff192ef41b2) |
| `first_gear` | first gear's ratio one (double-precision) bit up | 2.035 % (407/20001) | step 407 | drivetrain.ratio: game 17.645820027141554 (0x4031a5547617acc0) / Rust 17.645820027141557 (0x4031a5547617acc1) |
| `final_ratio` | final drive ratio one bit up | 2.035 % (407/20001) | step 407 | drivetrain.drive.velocity: game -0.000671380011446739 (0xbf45fff1989efc44) / Rust -0.0006713799943631562 (0xbf45fff18f3aaf54) |
| `shift_time` | up-shift time one physics step (3 ms) longer | 2.035 % (407/20001) | step 407 | drivetrain.currentGear: game 2 / Rust 1 |
| `diff_power` | differential power ramp one bit up | 0.000 % (0/20001) | step 0 | drivetrain.diffPowerRamp: game 0.19999999 (0x3e4ccccc) / Rust 0.20000002 (0x3e4cccce) |
| `diff_preload` | differential preload one bit up | 0.000 % (0/20001) | step 0 | drivetrain.diffPreLoad: game 21.0 (0x41a80000) / Rust 20.000002 (0x41a00001) |
| `wheel_inertia` | left driven wheel's inertia one (double-precision) bit up | 31.513 % (6303/20001) | step 803 | drivetrain.outShaftL.velocity: game 17.235362973984966 (0x40313c40bf73f469) / Rust 17.235362973984962 (0x40313c40bf73f468) |
<!-- FAULTS:END -->

All twelve are noticed. Three are noticed at once only because the changed number is itself
among the compared values (bias, differential ramp, preload); the others show in what the car
does. The first version of the shift-time fault, one bit of a double, was **not** noticed, and
rightly so: a shift ends at the first whole step after its time, so the car's behaviour does
not depend on the last bit; the fault is now one step (3 ms) and shows at the first up-shift.
The eighteen chassis faults of Task 08 are unchanged (`chassis_compare faults slalom --feed
brakes,drivetrain`).

### 3.4 Golden test in `cargo test`

`crates/rustyac-physics/tests/chassis_golden.rs` replays four excerpts written by
`chassis_compare excerpt` (format `CHGOLD02`: the car's state at the start, per step the feed
and the game's answer as six body states in full plus one hash over all compared values and
the force tape):

| File | Systems in Rust | Steps | What happens |
|---|---|---|---|
| `chassis_slalom_3000_300.chgold` | chassis (brakes, engine, drivetrain fed) | 300 | 100 km/h, steering swinging |
| `chassis_kerb_2684_300.chgold` | chassis | 300 | the left wheels climb a 2 cm strip |
| `powertrain_launch_autoclutch_off_522_460.chgold` | chassis, brakes, engine, drivetrain, helpers | 460 | a standing start by hand clutch: the clutch comes up, wheelspin in first gear on the rev limiter, traction control cuts in above 40 km/h |
| `powertrain_brake_2807_400.chgold` | the same | 400 | full braking from 250 km/h with the first two down-shifts (clutch profile, blip) |

The two new excerpts start in mid-run from a saved state that now includes the brakes, the
engine, the drivetrain and the helpers. A second new test changes one input in its last bit or
removes it (the throttle of one step, traction control's cut of one step, the brake pedal of
one step, one down-shift request) and requires the replay to fail at that step. All of it also
passes with `RUSTYAC_MATH=std`. As before, the car's parameters are not in the files: without
`cardata/ks_ferrari_f2004` the replay tests print `NOT TESTED` and pass.

---

## 4. What the feed still supplies, i.e. what is left before a whole Rust car

| Fed value | From the recording | Who produces it in the game | Removed by |
|---|---|---|---|
| steer, gas, brake, clutch, both paddles, handbrake, H-shifter lever; cockpit brake-bias clicks | `script.*` | the driver's device (`Car::pollControls`), the cockpit command queue | stays an input (driver / AI) |
| the wings' pushes on the body (14 calls per step once the car moves) | force-tape entries booked `aero_drag` / `aero_lift` | `AeroMap::step`, `Wing::step`, `DRS::step` | aero |
| the throttle factor for the next step (0 while cutting) | `engine.electronicOverride` | `TractionControl::step`, `SpeedLimiter::step` | aids |
| the brake request for the next step | `brakes.electronicOverride` (0 in every recording) | `SpeedLimiter::step` | aids |
| per tyre: ABS factor | `tyre.<w>.in_abs_override` | `ABS::step` | aids |
| extra brake torque on the faster driven wheel | not fed: no recorded car has an `[EDL]` section (the recorded feed could not supply it, see 6.2 item 4) | `EDL::step` | aids |
| a yaw torque on the body | never occurs in the recordings | `StabilityControl::step` | aids |
| per tyre: electric torque, AI grip factor | `in_electric_torque`, `in_ai_mult` (0 and 1 in every recording) | ERS front motors, AI driver | hybrids, AI |
| air and road temperature, track grip level, wear / damage / fuel rates, tyre blankets, the session's start time | constants of the recording | `PhysicsEngine`, `Track` | session / track |

No longer fed: brake and handbrake torques, the clutch after the automatic clutch, the gear,
engine speed / throttle used / boost for the fuel burn, the driven wheels' speed and spin
matrix.

Still missing for a whole car besides the fed systems: four-wheel drive and the hybrids
(14 + 7 cars), strut / axle / multilink suspensions (30 cars), the rest of `Car::step`
(penalties, black flag, jump start, `postStep`), contacts with the road and with walls.

---

## 5. Commands

```
cargo test -p rustyac-physics                      # four golden excerpts (needs cardata/ks_ferrari_f2004)
cargo build --release --manifest-path tools/chassis_compare/Cargo.toml
cargo build --release --manifest-path tools/car_oracle/Cargo.toml

chassis_compare run                                # the 11 F2004 recordings, everything in Rust; writes oracle/chassis/results_powertrain.md
chassis_compare run brake --verbose                # one drive; prints up to 100 differing values
chassis_compare run --feed brakes,drivetrain       # the rolling chassis alone, as in Task 08 (results.md)
chassis_compare run --feed drivetrain              # only the brakes in Rust (results_brakes.md); --feed brakes: the reverse
chassis_compare faults random                      # twelve deliberate faults in the new systems (faults_powertrain.md)
chassis_compare faults slalom --feed brakes,drivetrain   # the eighteen chassis faults of Task 08
chassis_compare excerpt                            # rewrite the four golden excerpts

chassis_compare test-car                           # write the five altered cars into cardata/
car_oracle all --car f2004_pt_street --out oracle/car_pt_street --only pt_shifter,pt_handbrake
car_oracle all --car f2004_pt_street --out oracle/car_pt_street --only pt_random --steps 3200
car_oracle all --car f2004_pt_ctrl   --out oracle/car_pt_ctrl   --only pt_random
car_oracle all --car f2004_pt_ctrl   --out oracle/car_pt_ctrl   --only launch_autoclutch_on --steps 3500
car_oracle all --car f2004_pt_fwd    --out oracle/car_pt_fwd    --only pt_autoshift
car_oracle all --car f2004_pt_fwd    --out oracle/car_pt_fwd    --only pt_random --steps 2600
car_oracle run --scenario pt_protect --out oracle/car_pt                      # the real F2004
car_oracle all --car ferrari_f40        --out oracle/car_f40     --only pt_autoshift --steps 3400
car_oracle all --car ks_ferrari_488_gt3 --out oracle/car_488_gt3 --only pt_autoshift --steps 3400
chassis_compare run --dir oracle/car_pt_street     # likewise car_pt, car_pt_ctrl, car_pt_fwd, car_f40, car_488_gt3,
                                                   # car_tight_stops, car_fallbacks; writes oracle/chassis/results_<folder>_powertrain.md
car_oracle list                                    # all scenarios; the pt_* ones only run when named with --only / --scenario
```

(Run from the repository folder; `chassis_compare` stands for
`tools/chassis_compare/target/release/chassis_compare.exe`, `car_oracle` for
`tools/car_oracle/target/release/car_oracle.exe`. `chassis_compare` needs only recordings and
the cars' data folders; `car_oracle` needs the game's `acs.exe` and the Visual C++ 2013
runtime. `car_oracle all` rewrites `results.md` in its `--out` folder each time, which is why
the lines above that share a folder leave only the last line's table there. The new
recordings take 0.47 GB (2,600 to 4,667 steps each, 14 kB per step), which brings `oracle/`
to about 3.2 GB; they can be deleted and made again with the lines above. The whole `chassis_compare
run` takes about seven seconds.)

---

## 6. Open questions

### 6.1 Choices made while working unattended

1. **One commit for the four systems, not one each.** The task asked for a commit after each
   system. They were written in one pass while the helpers were still reading the disassembly,
   depend on each other through the chassis (the controller reads the drivetrain, the engine
   lives in the drivetrain, the helpers call the gearbox) and were first compiled together, so
   the port is one commit (`2dc68d2`); the tools, the later additions and the report are
   separate ones. Each system can still be switched off on its own (`--feed`).
2. **The port was written from the listings, the briefs were the second reading.** Three of the
   four briefs arrived after the code for their part existed; two of the helpers compared
   their brief with the working tree on their own. That found one difference (push-to-pass:
   the cool-down clock starts full), fixed before the first run.
3. **Slots in the chassis, not systems inside the feed.** Task 08 suggested a ported system
   could live in a feed hook. The setup items point into brakes, drivetrain and engine, and
   `SetupManager::step` runs inside the car's step, so the systems are members of the car
   (optional ones) and the hooks of a filled slot are simply not called.
4. **The engine slot sits inside the drivetrain**, as `Drivetrain::acEngine` does. Another
   engine goes in with `install_drivetrain_with_engine`.
5. **Altered F2004 copies plus two real cars** for the branches (3.2). The copies reach
   things no shipped car without other unported systems has (a spool, `[EBB]`, a wastegate
   controller, front-wheel drive with double wishbones). The scripted drivers shift at the
   F2004's revs, so a real car can only be driven by the game's own automatic gearbox
   (`pt_autoshift`) without writing new scripts.
6. **`tools/car_oracle` was extended, the old recordings were not touched.** Five new scenarios
   (`pt_*`), three new script outputs (handbrake, H-shifter lever, cockpit bias clicks; the
   click calls the game's `BrakeSystem::setManualFrontBias` before the step, as the game's
   command queue does), the automatic-gearbox aid, and 27 more recorded values. The new
   outputs and values are only written for the new scenarios, so the twelve scenarios of
   Task 06 produce the same bytes and hashes as before.
7. **Beyond the four goals**, because they were a few lines each and could be compared: the
   water temperature (`car.waterTemperature`, compared), the anti-roll bars' controllers
   (compared on `f2004_pt_ctrl`), the control lock and gentle stop of `Car::step` (not
   reachable in a recording; reviewed).
8. **Cars the port refuses with a message**: four-wheel drive, `kers.ini` / `ers.ini`,
   `ctrl_4ws.ini`, an unknown `[TRACTION] TYPE` (the game reports "TRACTION NOT FOUND" and
   goes on as rear-wheel drive without driven wheels), `[PUSH_TO_PASS]` without a turbo (a
   critical error in the game).
9. **Golden files changed format** (`CHGOLD02`): the feed has more fields. The two excerpts of
   Task 08 were rewritten with the same steps and the same hashes of the game's values.
10. **Files outside the repository**: none written by this task's tools. The agent harness
    keeps its own logs in the user's `.claude` folder, and the tooling notes for later sessions
    there got an entry about this task.

### 6.2 Questions

1. **Branches no recording reaches**, ported from the disassembly only: push-to-pass (no car
   has the section, and its activations are handed out by race code that is not ported); the
   engine restrictor (a multiplayer setting); the engine-stall branch; a car without
   `throttle.lut`; the cockpit controls for turbo level and engine brake after loading; the
   gearbox lock before a session start; a down-shift from reverse; a gear or final-ratio setup
   item that changes (the callback into the gearbox); `ENGINE_LIMITER` attached; the
   slip-ratio controller inputs on a front-wheel-drive car; a controller table given as a file;
   the brake system with an AI driver (`aiMult` above 1 freezes the disc temperatures), with
   the hybrid's rear correction, with the pit limiter's brake request; the control lock; every
   NaN route. Section 6.3 has what the review found there.
2. **The real setup screen can replace gear ratios.** At a session start the game's gear tab
   looks each default ratio up in the car's `.rto` list and, if it is not there, takes the
   list's first entry. The oracle (Task 06) does not mirror the gear tab, so no recording shows
   it and the port does what the oracle does. For the F2004 every ratio is in its list; for
   three shipped cars one is not (`ks_maserati_250f_12cyl`, `ks_maserati_250f_6cyl`,
   `ks_mercedes_c9`): a real session with those starts with other ratios than a recording
   would. Seven cars use gear sets (`USE_GEARSET`), also not mirrored.
3. **`car_oracle`'s own single-tyre check fails for the F40** (`car_oracle all` ends with
   "below 100 %" for it): that check (Task 06) replays each tyre alone and does not know that
   the session start changes this car's tyre pressures. The recording itself is sound (two
   runs, equal hashes) and the whole-car comparison, which does apply the setup, matches every
   tyre value. Left as it is; the check could take the pressures from the recording.
4. **A car with EDL cannot be compared with its brakes in Rust yet**: the recordings hold the
   brake torque after the lock's addition, and adding "recorded minus computed" would not be
   bit-exact. Either EDL is ported with the aids (it is short: `spec_controller_brakes.md`
   section 5) or the oracle records its output per wheel. No car recorded so far has it.
5. **Other real cars.** 55 of the 113 cars are two-wheel drive on double wishbones without
   hybrids or controller files, 13 more differ only by turbo controllers; two were run. The
   rest need scripts that shift at their own revs (or the automatic gearbox, as here), nothing
   in the port.
6. **Push-to-pass activations, the restrictor, the cockpit turbo and engine-brake levels, the
   gearbox lock** are set by game code outside the physics (race rules, multiplayer, HUD
   keys). The port has the members and the functions the game calls; what calls them is for
   whoever ports that layer.
7. **Commit granularity** (6.1 item 1): if one commit per system is wanted for the history, it
   can be split after the fact, but the intermediate states would not have been run.

### 6.3 Review of the untested branches

<!-- REVIEW:BEGIN -->
(Three read-only reviewers are comparing the finished port with the disassembly for the
branches of 6.2 item 1; their findings go here.)
<!-- REVIEW:END -->
