# Task 16: grip investigation, four-wheel drive, KERS and ERS

## Resume here

State after the last commit:

- **Step 0 (grip): done.** `docs/port/grip_investigation.md`.
- **Step 1 (port): done.** 4WD, KERS, ERS, shared memory, HUD, controls; the track's DRS zones as a side find.
- **Step 2 (oracle): done.** 83 scenarios, 569,095 steps, all bit-exact against the final code (section 4).
  Golden tests for one AWD and one ERS excerpt.
- **Step 3 (leftovers): done.** assists.ini shift aids, first-lap arming by session type, two screenshots.
- **Review: done**, findings fixed (section 9).
- **Versioning: done.** `0.16.0` in `Cargo.toml` and the lock files, commit `chore(release): v0.16.0`, tag
  `v0.16.0` (annotated, not pushed). Nothing of the task is left to do.

Where things are:

- Briefs (git-ignored): `re/scratch/task16/spec_awd.md`, `spec_kers.md`, `spec_ers.md`, `spec_shm_controls.md`,
  `spec_leftovers.md`, `csp_survey.md`, `oracle_vs_session.md`, `alpha2026_csp.md`, `review_task16.md`.
- Logs, all in `re/scratch/task16/` and all written by `final_checks.sh` against the final code:
  `final_compare.txt` (every scenario), `final_game_replay.txt`, `final_faults.txt`, `coverage_hyt.txt` and
  `coverage_spa.txt` (what the recordings exercise); `acd_before.txt` / `acd_final.txt` (refused cars).
- The recordings (`oracle/awd*`, `oracle/hy*`) were deleted at the end; section 4.9 says how to make them again.

## 1. In plain English

- **rustyAC now drives four-wheel-drive cars and hybrids.** The Lamborghini Sesto Elemento (three
  differentials), the Audi R8 Plus and six more cars with a rear-biased coupling to the front axle, the F138 and
  the LaFerrari (KERS), the SF15-T, the SF70H, the McLaren P1 and VRC's Formula Alpha 2026 (ERS: MGU-K, MGU-H,
  the cockpit's delivery, recovery and engine-brake settings).
- **17 more of your installed cars load:** 83 of 123 now, 66 before. What is still refused: 38 cars whose
  suspension is not a double wishbone and 2 with rear-wheel steering.
- **Every step is the game's.** 569,095 steps of these cars were run in `acs.exe`'s own code and in rustyAC,
  on the flat road and on Spa, and every compared value of every step has the same bits. That includes
  launches, wheelspin, full braking with recovery, cornering with all differentials working, using the
  battery until it is empty and until the lap's allowance is used, the reset at the start line, every ERS
  mode, and ten made-up cars for the cases no installed car has (three four-wheel-drive ones: controllers on
  the three-differential car, the coupling without its controller, a locked rear differential; seven hybrids:
  a KERS on the wheels, a KERS on a four-wheel-drive car, front motors that push).
- **The buttons work like AC's.** Hold B on the pad (or K) for the KERS / ERS boost. The MGU-K, MGU-H and
  engine-brake settings are on Insert / Delete, Home / End, PageUp / PageDown and M, and on AC's own Ctrl+1..4 (here Alt or
  Right Ctrl + 1..4). The HUD shows battery, deployment, the lap's energy and the cockpit settings; the shared
  memory has the same fields AC fills.
- **Side find: DRS zones.** The first Spa runs of the F138 and the SF15-T differed from the game in one value,
  the telemetry page's `drsAvailable` (the log of those first runs was not kept): the game only allows DRS
  inside the track's zones (`data/drs_zones.ini`), rustyAC allowed it everywhere.
  That is ported and checked with the wing opening in a zone and shutting when the car leaves it.
- **The grip question** (Step 0) has its own report. In short: nothing wrong was found in rustyAC's physics;
  your AC is not plain `acs.exe` because of Custom Shaders Patch; no physics fix was made (section 2).
- **Not done:** struts, live axles and multilink suspensions, rear-wheel steering, the AI's use of KERS, the
  `_csp` version of the Formula Alpha 2026 (it needs CSP itself, section 6).

## 2. The grip question (Step 0)

The whole of it is in `docs/port/grip_investigation.md`. The findings:

- **Most likely:** differences that Custom Shaders Patch makes in your AC, not a fault of rustyAC. The best
  suspect for the kerbs is CSP's car-to-ground collision handling (`ENABLE_MESH_COLLIDER_SOONER`, on by
  default, and its own double-precision ODE): not proven, one telemetry run settles it. Second: traction
  control not being the same in the two drives (with it off the test driver cannot lap Spa at all).
- **Proven, but small:** CSP's thin air (5.6 % less downforce and drag at the Bus Stop) and, new, another
  tyre pressure law (`10.97 + 0.084 x (core - 26)` psi in your recording, `10 + 0.16 x (core - 26)` in plain
  `acs.exe`: about 6 psi less on hot tyres). Together about 2 % of the speed the Bus Stop can be taken at.
- **The lap used so far is no reference:** `f2004_spa_ai.csv` is the AI, which gets 20 % more tyre grip off
  the throttle, no traction control or ABS and a strong stability aid.
- **Ruled out:** rustyAC's physics against plain `acs.exe` (bit-exact), the session's start values, the
  AI / player switch, CSP's gamepad script and extended physics (both off), tyre blankets, track grip,
  temperatures, fuel, compound, brake bias, damage, the stability aid, the pad's clutch setting.
- **Fix:** none to the physics. Three differences that do not touch grip were fixed in Step 3 (session start
  time, `AUTO_BLIP`, first-lap arming). Your `target\release\rustyac_controls.ini` is an old copy
  (`STEER_DEADZONE=0.1`, AC has 0.05): reported, not touched; delete it and it is written again.
- **To test by feel:** `--pressure-law 10.97,0.084` (new), `--air-density 1.165` (from Task 15),
  `tools/corner_numbers.py` and the `grip_probe` example (new), and a list of eight telemetry runs.

## 3. What is ported

Every block was written from an instruction-level brief of the listing (not from Ghidra's pseudo-C where the
order of operations matters), with the game's widths: single and double precision where the game has them,
the game's operand order, its compare forms for NaN.

### 3.1 Four-wheel drive (`crates/rustyac-physics/src/car/drivetrain.rs`, `setup.rs`)

| AC | Address | Rust | What |
|---|---|---|---|
| `Drivetrain::step4WD` | 0x14026a220 | `step_4wd` | `TYPE=AWD`: front, rear and centre differential, a fixed front torque share |
| `Drivetrain::step4WD_new` | 0x14026ad80 | `step_4wd_new` | `TYPE=AWD2`: the rear axle driven as in `step2WD`, a coupling sends torque to the front by the speed difference of the axles (`CENTRE_RAMP_TORQUE`, `CENTRE_MAX_TORQUE`). The car's `ctrl_awd2.ini` controller (opened by `Drivetrain::loadINI`) is evaluated inside this step, after the rear axle's, and sets the coupling's maximum torque |
| `Drivetrain::stepControllers` | 0x14026b200 | `step_controllers` | `ctrl_awd_front_share.ini`, `ctrl_awd_center_lock.ini` (`AWD`), `ctrl_single_lock.ini` (rear-drive cars: the SF15-T has one), in this order |
| `Drivetrain::initControllers` | 0x140267070 | the loader arms | which of those three files a traction type looks for |
| `Drivetrain::accelerateDrivetrainBlock` | 0x1402664c0 | `accelerate_drivetrain_block` | the locked block with four shafts |
| `SetupManager::initItems` | 0x140289570 | `SetupTarget::AwdDiff`, `AwdFrontShare` (in `SetupManager::init`) | ten setup items: `FRONT_` / `REAR_` / `CENTER_DIFF_POWER`, `_COAST`, `_PRELOAD`, `AWD_FRONT_TORQUE_DISTRIBUTION` |

Also: the front shafts (`outShaftLF` / `RF`), the inertia sums of both layouts, the saved state and the
trace rows of the 4WD members (only for 4WD cars: the saved states of the others keep their form).

### 3.2 KERS (`car/kers.rs`, new)

| AC | Address | Rust |
|---|---|---|
| `Kers::init` | 0x1402b7360 | `Kers::load`: kers.ini (`ATTACH=ENGINE` or `WHEELS`, `BRAKE_LEVEL`, `CHARGE_K`, `DISCHARGE_TIME`, `TORQUE_CURVE`, `CONTROLLER`, from version 2 `NEGATIVE_INPUT_CHARGE_K`, from version 3 `HAS_BUTTON_OVERRIDE` and `MAX_KJ_PER_LAP`) |
| `Kers::step` | 0x1402b7e10 | `Kers::step`: filling under braking, the input (controller or button), what forbids delivery, the discharge, the rev limiter and gear rule, the lap's energy |
| `Kers::getOutputTorque` | 0x1402b72b0 | `get_output_torque`: by engine revs or by the driven wheels' revs (converted in double precision here, in single precision in `Kers::step`'s energy count, as in the game) |
| `Kers::reset` | 0x1402b7e00 | `reset`, called from the car's reset and at the armed first crossing of the line |

The torque reaches the car as the game does it: an engine KERS is a torque generator of the engine
(`engine.rs`), a wheel KERS is added to the driven tyres in the drivetrain step (half per tyre in a
two-wheel-drive car, a quarter in `step4WD`).

### 3.3 ERS (`car/ers.rs`, new)

| AC | Address | Rust |
|---|---|---|
| `ERS::ERS`, `ERS::init` | 0x140291500, 0x140291c30 | `Ers::load`: ers.ini `[KINETIC]`, `[HEAT]`, `[FRONT_MOTORS]`, `[COCKPIT_CONTROLS]`, every `ctrl_ers_N.ini` and `ctrl_ers_front_N.ini` |
| `ERS::step` | 0x1402930e0 | `Ers::step`: recovery from the brakes and the overrun (MGU-K) and from the boost (MGU-H), the two delivery maps, the button, the two cut stages (empty battery, pit limiter, lap allowance; rev limiter, neutral, reverse), the discharge with its scale-down, the lap's energy, the front motors with torque vectoring |
| `ERS::getOutputTorque`, `ERS::getCoastTorque` | 0x140291bd0, 0x140291b40 | the engine's torque and coast generators |
| `ERS::setPowerController` | 0x140292fc0 | `set_power_controller`: a delivery profile is a fresh copy of its map (the filters start again) |
| `ERS::reset` | 0x140292fa0 | `reset` |
| `CarAvatar::cycleERSPower`, `cycleERSRecovery`, `cycleERSHeatCharging`, `cycleEngineBrake` | 0x1400d2a80, 0x1400d2bc0, 0x1400d29e0, 0x1400d2c90 | `RollingChassis::cycle_ers_power`, `cycle_ers_recovery`, `cycle_ers_heat_charging`, `cycle_engine_brake` and the four `set_` jobs (the cockpit's state is `CockpitState`) |
| `Engine::setCoastSettings` | 0x140288010 | ported before this task; now called by the cockpit job `RollingChassis::set_engine_brake`. New: `coast_settings_count` (how many settings `engine.ini [COAST_SETTINGS]` has) |
| `TimeTransponder::lap` | 0x140290a50 | raises the car's lap event: the lap's energy count starts again with every lap that is reported |
| `TimeTransponder::onTimeLinePassed` | 0x140290c20 | battery and count are reset (`Kers::reset` / `ERS::reset`) at the armed first crossing of the line |

A car with a readable ers.ini has no KERS (as in the game). The rear brake correction (`BRAKE_REAR_CORRECTION`
x recovery level) goes to the brake system as in the game.

### 3.4 Shared memory and HUD (`rustyac-game`: `shm.rs`, `render/hud.rs`, `view.rs`)

- Physics page: `kersCharge`, `kersInput`, `kersCurrentKJ` (from the ERS if the car has one, else the KERS),
  `ersRecoveryLevel`, `ersPowerLevel`, `ersHeatCharging`, `ersIsCharging`, `engineBrake`, `drsAvailable`.
- Static page: `hasKERS`, `hasERS`, `kersMaxJ`, `ersMaxJ`, `ersPowerControllerCount`,
  `engineBrakeSettingsCount`, `maxTurboBoost`. `maxPower` and `maxTurboBoost` are taken when the car is built,
  as `CarAvatar::initPhysics` (0x1400d7660) does: from the first step on a turbo's controller has rewritten the
  number they come from (the SF15-T would read 0 instead of 3.5).
- Graphics page, on a track: `session` is the session type minus one for 1..7, else -1 (the type is race.ini's
  `[SESSION_0] TYPE`; without a race.ini 4 for the hot-lap spawn, else 1). On the flat road it stays 0.
- HUD: one line with battery, deployment and the lap's energy (against the allowance where the car has one),
  one with the MGU-K profile's number and name, the recovery level and the MGU-H mode, one with the
  engine-brake setting. `docs/port/ers_sf15t_spa.png` shows them.

### 3.5 Controls (`input/bindings.rs`, `input/pad.rs`, `input/wheel.rs`, `main.rs`, `sim.rs`)

Read from AC's `controls.ini` as `acs.exe` reads them (pad `XBOXBUTTON`, wheel `JOY` / `BUTTON`, `KEY`); a
second key for each in `[RUSTYAC_KEYS_2]` of `rustyac_controls.ini`. The table `rustyac.exe --list-devices`
prints, with your bindings:

| Action | controls.ini section | Pad | Keys |
|---|---|---|---|
| KERS / ERS boost (hold) | `[KERS]` | B | K |
| DRS | `[DRS]` | LB | F |
| MGU-K delivery, next / previous | `[MGUK_DELIVERY_UP]`, `[MGUK_DELIVERY_DN]` | not bound | PageUp / PageDown, or Alt or Right Ctrl + 2 (Shift: previous) |
| MGU-K recovery, more / less | `[MGUK_RECOVERY_UP]`, `[MGUK_RECOVERY_DN]` | not bound | Home / End, or Alt or Right Ctrl + 1 (Shift: less) |
| MGU-H mode | `[MGUH_MODE]` | not bound | M, or Alt or Right Ctrl + 3 |
| Engine brake, next / previous | `[ENGINE_BRAKE_UP]`, `[ENGINE_BRAKE_DN]` | not bound | Insert / Delete, or Alt or Right Ctrl + 4 (Shift: previous) |

- The digit commands are AC's fixed Ctrl+1..4 (`Sim::onKeyDown` 0x14019a940); rustyAC takes Alt or Right Ctrl,
  as for its other Ctrl commands.
- B is your `[KERS]` and your `[HANDBRAKE]` button, in AC too: on the F138, SF15-T, SF70H, P1 and Formula
  Alpha 2026 the handbrake button also deploys.
- A press acts before the next physics step. "Down" is tested first and swallows an "up" of the same step.
  The cyclers go round their ends (recovery 0..10, i.e. 0 to 100 %).
- A recorded drive carries the presses as events (bits 9 to 15 of the step's events), so it replays.

### 3.6 DRS zones (`track/mod.rs`, `track/loader.rs`, `car/aero.rs`)

`DRSManager::DRSManager` (0x140278ea0) reads `data/drs_zones.ini` (`[ZONE_n]` `DETECTION`, `START`, `END`);
`DRSManager::isDRSAvailable` (0x140279490) is true on a track without zones, else inside any zone by the car's
place along the lap; a zone whose end is not above its start runs over the start line (Spa's first one: 0.981 to
0.050). Ported as `Track::is_drs_available`, asked by `DRS::step`. A drive recorded before this has the wing
free everywhere: the zones apply to input files that carry `drs_zones=1` (every new recording does).

### 3.7 Leftovers (Step 3)

- **assists.ini `AUTO_CLUTCH`, `AUTO_BLIP`, `AUTO_SHIFTER`** are applied as the game applies them: the clutch
  aid is forced on with a pad or the keyboard; with the blip aid off a car with an H-pattern gearbox also stops
  cutting the ignition on up-shifts (the job at 0x1400d07a0). The flags `--auto-clutch`, `--no-auto-clutch`,
  `--auto-shifter`, `--no-auto-shifter`, `--auto-blip`, `--no-auto-blip` override the file.
- **`--spawn hotlap` arms the first lap only in a hot-lap session.** The session type and the spawn set come
  from race.ini (`[SESSION_0] TYPE`, `SPAWN_SET`); the first lap is armed when any `[SESSION_n]` has `TYPE=4`
  (`RaceManager::initOffline` -> `CarAvatar::armFirstLap` 0x1400d2300), or without a race.ini when the spawn is
  `hotlap`. Armed: the clock starts again when the car first crosses the line. Not armed: the clock runs from
  the spawn and lap 1 includes the run-up, as in the game.
- **The session starts when the car is put down** (the wind's slow swing and the automatic gearbox's 300 ms
  count from there), for new recordings.
- **Screenshots**, off screen: `docs/port/awd_sesto_spa.png` (279,752 bytes), `docs/port/ers_sf15t_spa.png`
  (291,032 bytes).

## 4. Results

How a comparison works: `tools/car_oracle` runs the game's own code in process and records every value of
every step (`.carrec`); `tools/chassis_compare run` builds the whole car in Rust, feeds it nothing but what
the driver did (and, for the hybrids, the cockpit jobs), and compares by field name: 2,009 chassis values and
278 to 480 of the other systems per step (by car and scenario), and every force call. "Bit-exact steps" are steps in which
every compared value has the game's bits. All numbers below are from one batch against the final code
(`re/scratch/task16/final_checks.sh`: the comparisons in `final_compare.txt`, the game replays in
`final_game_replay.txt`, the fault runs in `final_faults.txt`, what the recordings exercise in
`coverage_hyt.txt` and `coverage_spa.txt`): **83 scenarios, 569,095 steps, no difference.**

Scenarios: `pt_autoshift` (standing start through the gears with the automatic gearbox), `wc_stops` (flat
out, full stops), `wc_spirited` (bends at the limit, on and off the throttle: all differentials at work),
`hy_deploy` (the button held until the battery is empty, a full stop with recovery, the button again until
the lap's allowance is used), `hy_modes` (an ERS car's cockpit settings one after the other: recovery 100 %,
0 %, 70 %, every delivery profile, MGU-H to the motor and back, three engine-brake settings), `spa_launch`,
`spa_kerbs`, `spa_lap` (Spa from the hot-lap start), `spa_hybrid_lap` / `spa_hybrid_timing` (the same with
the button held; the timing drive crosses the lines and is teleported: laps are counted, battery and lap
energy are reset), `spa_drs_timing` (the same with a DRS press every 1.2 s).

### 4.1 Four-wheel drive, flat road

`pt_autoshift` 4,001 + `wc_stops` 4,334 + `wc_spirited` 4,001 = 12,336 steps per car.

| Car | Type | Bit-exact steps |
|---|---|---|
| `ks_lamborghini_sesto_elemento` | AWD | 12,336 / 12,336 |
| `ks_audi_r8_plus` | AWD2 | 12,336 / 12,336 |
| `ks_lamborghini_aventador_sv` | AWD2 | 12,336 / 12,336 |
| `ks_lamborghini_huracan_performante` | AWD2 | 12,336 / 12,336 |
| `ks_maserati_levante` | AWD2 | 12,336 / 12,336 |
| `ks_nissan_gtr` | AWD2 | 12,336 / 12,336 |
| `ks_porsche_cayenne` | AWD2 | 12,336 / 12,336 |
| `ks_porsche_macan` | AWD2 | 12,336 / 12,336 |
| `sesto_awd_ctrl` (made up: the Sesto with front-share and centre-lock controllers) | AWD | 12,336 / 12,336 |
| `r8_awd2_plain` (made up: the R8 without `ctrl_awd2.ini`) | AWD2 | 12,336 / 12,336 |
| `r8_awd2_spool` (made up: the R8 with a locked rear differential) | AWD2 | 12,336 / 12,336 |

### 4.2 Four-wheel drive, Spa

| Car | `spa_kerbs` | `spa_lap` | `spa_launch` | Bit-exact steps |
|---|---|---|---|---|
| `ks_lamborghini_sesto_elemento` | 6,667 | 3,729 | 3,637 | 14,033 / 14,033 |
| `ks_audi_r8_plus` | 6,118 | 3,581 | 3,553 | 13,252 / 13,252 |

(A track drive ends early when the car leaves the track for good, where its body would be in a wall: on a
track scenario the body is a ghost and only the tyres meet the road. The step counts are what was driven.)

### 4.3 Hybrids, flat road

`hy_deploy` 10,001 + `hy_modes` 10,001 + `wc_stops` 4,334 = 24,336 steps per car.

| Car | System | Bit-exact steps |
|---|---|---|
| `ks_ferrari_f138` | KERS on the engine, button | 24,336 / 24,336 |
| `ferrari_laferrari` | KERS on the engine, controller | 24,336 / 24,336 |
| `ks_ferrari_sf15t` | ERS: MGU-K, MGU-H, 6 profiles, 13 engine-brake settings | 24,336 / 24,336 |
| `ks_ferrari_sf70h` | ERS: MGU-K, MGU-H, 7 profiles | 24,336 / 24,336 |
| `ks_mclaren_p1` | ERS: MGU-K, MGU-H, 2 profiles | 24,336 / 24,336 |
| `vrc_formula_alpha_2026` (mod) | ERS: MGU-K, 4 profiles | 24,336 / 24,336 |

### 4.4 Hybrids, Spa

| Car | `spa_hybrid_lap` | `spa_hybrid_timing` | `spa_launch` | `spa_drs_timing` | Bit-exact steps |
|---|---|---|---|---|---|
| `ks_ferrari_f138` | 2,128 | 13,884 | 2,162 | 13,908 | 32,082 / 32,082 |
| `ks_ferrari_sf15t` | 17,711 | 14,134 | 1,992 | 14,161 | 47,998 / 47,998 |

Counted in the recordings (`coverage_spa.txt`):

- `spa_hybrid_timing`: two laps counted, the lap's energy count starts again eleven times (lines and
  teleports) in both cars' drives.
- `spa_hybrid_lap`, SF15-T: the battery goes down to 0.8 % and the lap's energy reaches the car's own
  allowance of 4,000 kJ (the count stops at 4,000,044 J).
- `spa_drs_timing`, SF15-T: 35 presses, 8 of them outside a zone (nothing happens), the wing opens 15 times and
  shuts 14 times (brakes, leaving a zone), open for 5,313 steps; F138: 34 presses, 8 outside, 13 opens,
  13 shuts, open for 4,839 steps.
- These cars have no traction control and the oracle's simple driver holds the button: the "laps" are mostly
  wheelspin and spins near the start. That is a hard test of the hybrid code, not a lap time.

### 4.5 Made-up cars and two more mod cars

`hy_deploy` 10,001 + `hy_modes` 10,001 = 20,002 steps per car. Made with `chassis_compare test-car` from
installed cars, for what no installed car has.

| Car | What it is for | Bit-exact steps |
|---|---|---|
| `f138_kers_wheels` | a KERS on the driven wheels (`ATTACH=WHEELS`), a lap allowance of 120 kJ that is used up | 20,002 / 20,002 |
| `laferrari_kers_button` | a controller-driven KERS with the button override and a lap allowance | 20,002 / 20,002 |
| `sesto_awd_kers` | a wheel KERS on the three-differential car (a quarter per tyre) | 20,002 / 20,002 |
| `r8_awd2_kers` | a wheel KERS on the coupling car | 20,002 / 20,002 |
| `f2004_fwd_kers` | a wheel KERS on a front-wheel-drive car | 20,002 / 20,002 |
| `sf15t_ers_limits` | an ERS whose lap allowance (150 kJ) is used up, a default profile that does not exist, a quicker MGU-H, the MGU-H cockpit control switched off (recovery and delivery stay on) | 20,002 / 20,002 |
| `lithium_ers_front` | front motors that deliver by their own map and by the button, torque vectoring 0.6, a lap allowance, a rear brake correction; on an AWD2 car | 20,002 / 20,002 |
| `vrc_formula_lithium_2023` (mod, as installed) | AWD2 with ERS and front motors (its own front maps are all zero) | 20,002 / 20,002 |
| `vrc_formula_alpha_2025` (mod, as installed) | ERS with MGU-H, 5 profiles | 20,002 / 20,002 |

What the recordings exercise was counted (`re/scratch/task16/coverage16.py`, output `coverage_hyt.txt`):

- In every `hy_deploy` the battery is used and filled again.
- The lap's energy reaches the allowance in the made-up cars that were given a low one: 120,042 J of 120 kJ
  (`f138_kers_wheels`), 150,056 J of 150 kJ (`laferrari_kers_button`), 200,098 / 200,135 / 200,034 J of 200 kJ
  (`sesto_awd_kers`, `r8_awd2_kers`, `f2004_fwd_kers`), 150,112 J of 150 kJ (`sf15t_ers_limits`), 300,042 J of
  300 kJ (`lithium_ers_front`). The two mod cars stay far below theirs (`vrc_formula_alpha_2025`: 347 kJ of
  4,000 kJ; `vrc_formula_lithium_2023`: 4,565 kJ of 599,582 kJ); the SF15-T reaches its own on Spa (4.4).
- In `lithium_ers_front` the front map's three stages and both front wheels' motor torques are non-zero in
  both scenarios (18 of the 21 recorded ERS values are; the other three are the MGU-H recovery, which the car
  does not have, and the fourth stage of the rear and of the front map, which its maps do not have).

### 4.6 The game itself

`chassis_compare game-replay` turns a recording into an input file and lets `rustyac.exe` drive it headless:

| Recording | Bit-exact steps |
|---|---|
| Sesto Elemento, `wc_spirited` | 4,001 / 4,001 |
| R8 Plus, `wc_stops` | 4,334 / 4,334 |
| F138, `hy_deploy` | 10,001 / 10,001 |
| SF15-T, `hy_deploy` | 10,001 / 10,001 |

### 4.7 Faults: can the comparison fail?

`chassis_compare faults16 <scenario> --dir <folder>` changes one number of the Rust car and runs again. Every
fault was noticed:

| Car, scenario | Faults | Noticed |
|---|---|---|
| Sesto, `wc_spirited` | front share, rear coast lock, front power lock, centre preload, a front shaft's inertia (one bit each) | 5 of 5, at step 0 |
| R8, `wc_spirited` | the coupling's ramp, a front shaft's inertia (one bit each) | 2 of 2, at step 0 |
| F138, `hy_deploy` | discharge rate, filling rate, brake level for full filling (0.01 % each) | 3 of 3 (steps 734, 4400, 4400) |
| `f138_kers_wheels`, `hy_deploy` | the same | 3 of 3 (steps 734, 4400, 4400) |
| SF15-T, `hy_modes` and `hy_deploy` | discharge rate, recovery rate (one part in a million), MGU-H torque share, rear brake correction, start recovery level (one bit) | 5 of 5 in each (`hy_modes`: steps 3400, 3800, 3402, 0, 0; `hy_deploy`: 734, 895, 737, 0, 0) |
| `lithium_ers_front`, `hy_deploy` and `hy_modes` | the same five, the two AWD2 ones, the front motors' discharge rate | 8 of 8 (the front one at step 400) |

The battery rates are changed by a small part and not by one bit: one bit of a rate is lost when the step's
share is added to a charge near 1, in the game as in the port.

### 4.8 Golden tests (`crates/rustyac-physics/tests/chassis_golden.rs`)

Two excerpts in the repo, replayed by `cargo test` (they print NOT TESTED and pass without `cardata/<car>`):

- `awd_sesto_wc_spirited_2400_300.chgold` (161,604 bytes): the Sesto at 124 km/h and 1 g in a bend off the
  throttle, then flat out while the steering goes through the centre into a bend the other way.
- `ers_sf15t_hy_modes_7700_400.chgold` (214,046 bytes): the SF15-T on part throttle with the MGU-K delivering
  by its map and the MGU-H on the motor, an engine-brake setting chosen, the brakes with the battery filling,
  the MGU-H switched back to the battery. The file carries the cockpit jobs (header `hybrid_jobs=1`, one word
  per step).

Tests: the files read back and round-trip; both match the game; a throttle one bit off, a missing engine-brake
job, a missing MGU-H job and another recovery level are each noticed in their step. Two more in
`crates/rustyac-game/tests/replay.rs`: the static page of the SF15-T after five steps (`maxTurboBoost` 3.5),
and a saved setup's hybrid knobs.

### 4.9 Making the recordings again

```
cargo build --release --manifest-path tools/car_oracle/Cargo.toml
cargo build --release --manifest-path tools/chassis_compare/Cargo.toml
tools/chassis_compare/target/release/chassis_compare test-car        (the made-up cars, into cardata/)
car_oracle all --car <car> --out oracle/<dir> --only pt_autoshift,wc_stops,wc_spirited      (4WD)
car_oracle all --car <car> --out oracle/<dir> --only hy_deploy,hy_modes,wc_stops            (hybrids)
car_oracle run --track spa --scenario <spa_...> --car <car> --out oracle/<dir>
chassis_compare run --dir oracle/<dir>
chassis_compare faults16 <scenario> --dir oracle/<dir>
chassis_compare excerpt16                                            (writes the two golden files)
chassis_compare game-replay <scenario> --dir oracle/<dir>
```

The batches used: `re/scratch/task16/record_awd.sh`, `record_hybrid.sh`, `record_testcars.sh`,
`final_compare.sh`. A recording is 15 MB per thousand steps: keep few.

## 5. Refused cars, before and after

`acd_check.exe --refused` builds every installed car that has a `data.acd` (123).

| | Loads | Refused | Why refused |
|---|---|---|---|
| Before (v0.15.0) | 66 | 57 | 38 not double wishbone, 9 hybrid, 8 four-wheel drive, 2 rear-wheel steering |
| After | 83 | 40 | 38 not double wishbone, 2 rear-wheel steering |

**New (17):** `ks_audi_r8_plus`, `ks_lamborghini_aventador_sv`, `ks_lamborghini_huracan_performante`,
`ks_lamborghini_sesto_elemento`, `ks_maserati_levante`, `ks_nissan_gtr`, `ks_porsche_cayenne`,
`ks_porsche_macan` (four-wheel drive); `ferrari_laferrari`, `ks_ferrari_f138`, `ks_ferrari_sf15t`,
`ks_ferrari_sf70h`, `ks_mclaren_p1`, `vrc_formula_alpha_2025`, `vrc_formula_alpha_2026`,
`vrc_formula_lithium_2023`, `vrc_formula_lithium_2023_csp` (hybrids).

**Still refused (40):**

- Suspension not a double wishbone (38): `abarth500`, `abarth500_s1`, `alfa_romeo_giulietta_qv`,
  `alfa_romeo_giulietta_qv_le`, `bmw_1m`, `bmw_1m_s3`, `bmw_m3_e30`, `bmw_m3_e30_drift`, `bmw_m3_e30_dtm`,
  `bmw_m3_e30_gra`, `bmw_m3_e30_s1`, `bmw_m3_e92`, `bmw_m3_e92_drift`, `bmw_m3_e92_s1`, `bmw_m3_gt2`, `bmw_z4`,
  `bmw_z4_drift`, `bmw_z4_gt3`, `bmw_z4_s1`, `ks_alfa_mito_qv`, `ks_alfa_romeo_155_v6`, `ks_alfa_romeo_4c`,
  `ks_alfa_romeo_gta`, `ks_audi_a1s1`, `ks_audi_sport_quattro`, `ks_audi_sport_quattro_rally`,
  `ks_audi_sport_quattro_s1`, `ks_bmw_m235i_racing`, `ks_ferrari_250_gto`, `ks_ford_mustang_2015`,
  `ks_maserati_250f_12cyl`, `ks_maserati_250f_6cyl`, `ks_toyota_gt86`, `ruf_yellowbird`,
  `urd_darche_992_23`, `vrc_formula_alpha_2025_csp`, `vrc_formula_alpha_2026_csp`,
  `vrc_formula_beta_2024_csp` (the three `_csp` cars: `TYPE=COSMIC`, a CSP suspension).
- Rear-wheel steering (2): `ks_ferrari_812_superfast`, `ks_porsche_panamera`.

The three Sport Quattros, the Audi S1 (`ks_audi_a1s1`) and the 155 V6 are four-wheel-drive cars that wait for
the strut suspension only.

## 6. VRC Formula Alpha 2026

Both versions were read file by file (`re/scratch/task16/alpha2026_csp.md`). CSP is closed source: only its
shipped text files were read, nothing was disassembled, and the car's two packed Lua scripts are encrypted by
the mod and were left so.

**`vrc_formula_alpha_2026` (plain): a plain-AC car. It needs nothing from CSP for its physics.**

- It loads in rustyAC and matches `acs.exe` on all 24,336 compared steps.
- Double wishbones with heave springs, `tyres.ini` version 10, a stock ERS: an MGU-K of 500 Nm up to
  4,800 rpm (251 kW there) and a constant 260.6 kW from 5,000 rpm up; four delivery profiles (LOW, MEDIUM, HIGH, NODEPLOY: at most
  149 / 186 / 174 / 0 kW by throttle and speed); the KERS button gives everything until the battery is empty;
  the MGU-H does nothing; only the delivery profile can be changed in the cockpit; no real lap allowance
  (99,999 kJ).
- DRS flattens wings 1 and 3, inside the track's DRS zones only. A 3.6 bar turbo limited by revs.
- Two leftovers of the other version are in it and do nothing in plain AC: `aero.ini`'s two controllers with
  `INPUT=SCRIPT_12` (the game prints an error, reads 0, the factor is 1.0) and `engine.ini FLOW_ON_CUT`
  (never read).
- Without CSP a driver misses looks and sound only.

**`vrc_formula_alpha_2026_csp` ("Pro"): cannot run without CSP, in plain AC or in rustyAC.**

- Plain `acs.exe` stops first at `car.ini [HEADER] VERSION=extended-2` (it reads the version as a number and
  throws), and would stop next at `suspensions.ini TYPE=COSMIC` (`Car::Car` knows `STRUT`, `DWB`, `ML`, `AXLE`
  only and exits). rustyAC refuses at the suspension, with the file and key in its message.
- What only CSP provides:
  - the suspension: COSMIC bodies and joints, torsion bars, coil-overs, damper and bump-stop tables;
  - the floor's downforce and the body's drag (`aero.ini [MAP_0]`);
  - the tyres' thermal model and falloff tables;
  - the power unit, the ERS (350 kW and 13 MJ in `script_params.ini`, strategy maps), brake-by-wire, the
    differential, the gearbox and the DRS / straight-mode logic: by its module names and the plain files around
    it, all in an encrypted 345 kB Lua script with 2,271 script setup items. The script was not decrypted, so
    what it does to the car was not read.
- Even with a suspension that loads, what is left for plain AC would be another, broken car: no electric
  power (all twelve maps read `INPUT=SCRIPT_5`, which plain AC reads as 0), all brake torque on the rear axle
  (`ctrl_ebb.ini` reads `INPUT=SCRIPT_11`), no DRS, no floor downforce, no idle and no engine braking.
- The data cannot be edited to fit: the scripts are keyed to a fingerprint of every data file and do not run
  from an unpacked `data` folder. "COSMIC to DWB, keep the rest" is not possible even under CSP.
- So this car needs CSP's physics extensions themselves (COSMIC suspension, the Lua physics API, script
  inputs for controllers, the extended aero map, the tyre thermal model). That is a project of its own, far
  outside a port of `acs.exe`.

## 7. Driving them

```
target\release\rustyac.exe --car ks_lamborghini_sesto_elemento --track spa --windowed     (three differentials)
target\release\rustyac.exe --car ks_audi_r8_plus --track spa --windowed                   (coupling to the front)
target\release\rustyac.exe --car ks_ferrari_f138 --track spa --windowed                   (KERS)
target\release\rustyac.exe --car ks_ferrari_sf15t --track spa --windowed                  (ERS)
target\release\rustyac.exe --car vrc_formula_alpha_2026 --track spa --windowed            (ERS, the mod)
```

- Without `--windowed`: borderless full screen. `--no-race-ini` for the built-in conditions instead of your
  last AC session's. `--list-devices` prints the bindings table.
- **KERS / ERS boost:** hold **B** on the pad, or **K**. On the F138, SF15-T, SF70H, P1 and Formula Alpha 2026
  the button gives full power; the LaFerrari deploys by itself (its file has no button).
- **DRS:** **LB** or **F**; on Spa only in the two zones (start straight, Kemmel).
- **ERS cars:** MGU-K delivery profile **PageUp / PageDown**; recovery **Home / End**; MGU-H mode **M**;
  engine brake **Insert / Delete**. Or AC's commands: **Alt (or Right Ctrl) + 1** recovery, **+ 2** delivery,
  **+ 3** MGU-H, **+ 4** engine brake, with **Shift** for down (not for the MGU-H, which is a toggle). A car only has the controls its
  `[COCKPIT_CONTROLS]` gives it (the Formula Alpha 2026: the delivery profile only).
- The HUD's lines under the conditions show what the presses did; the console prints each change.
- The SF15-T and the F138 have no traction control: `--autodrive` (the simple test driver) spins them.

## 8. Performance

Measured on this PC while it was in use; `rustyac.exe --headless --autodrive --track spa`, 20 s in real time
(the budget is 3 ms per step):

| Car | Step time min / average / max | Steps more than 1 ms late | A whole step late |
|---|---|---|---|
| `ks_ferrari_f2004` (for comparison) | 0.022 / 0.089 / 0.692 ms | 4 | 0 |
| `ks_lamborghini_sesto_elemento` (AWD) | 0.022 / 0.079 / 0.912 ms | 10 | 0 |
| `ks_audi_r8_plus` (AWD2) | 0.023 / 0.082 / 0.757 ms | 2 | 0 |
| `ks_ferrari_f138` (KERS) | 0.024 / 0.092 / 0.908 ms | 13 | 0 |
| `ks_ferrari_sf15t` (ERS) | 0.024 / 0.089 / 0.531 ms | 4 | 0 |

The new systems cost nothing that can be measured. A whole-car comparison runs at about 3,000 steps a second
(10,001 steps in 2.4 to 3.7 s, comparing included).

## 9. Review

A read-only review of the whole port against the briefs and, for the blocks no installed car exercises,
against the listings themselves (`re/scratch/task16/review_task16.md`). **No defect in the physics port.**
Found and fixed in the game layer and around it:

| Finding | Fix |
|---|---|
| The static page's `maxPower` and `maxTurboBoost` were read after the first step, when a turbo's controller has rewritten them | taken when the car is built (`PhysicsInfo`), with a test |
| A change of mine made the Alt+G toggle outlive a rebuild of the car (N), which changed old recordings | taken back: a new car has the session's setting |
| A saved setup's `MGUK_DELIVERY`, `MGUK_RECOVERY`, `MGUH_MODE` were dropped without a word | stepped to by the cockpit's cyclers, as the setup screen does; with a test |
| The DRS zones changed old recordings of DRS cars on a track | only for input files with `drs_zones=1` |
| The cockpit's engine-brake index was in the saved state of ERS cars only | saved for every car with engine-brake settings |
| A KERS on a chassis that is fed its brakes (a tool mode) divided by zero | the brake torque is read from brakes.ini there |

The review also confirmed by reading, instruction by instruction: the wheel KERS blocks of `step2WD` and
`step4WD`, the whole front-motor path of `ERS::step`, both cut stages, the scale-down, the three cyclers.

## 10. Files

| File | What |
|---|---|
| `crates/rustyac-physics/src/car/kers.rs`, `ers.rs` | new: KERS, ERS |
| `crates/rustyac-physics/src/car/drivetrain.rs` | 4WD steps, controllers, the wheel KERS, the generators' signals |
| `crates/rustyac-physics/src/car/chassis.rs` | the systems in the car's step, the cockpit state and its jobs, resets, lap events |
| `crates/rustyac-physics/src/car/engine.rs`, `dynamic_controller.rs`, `setup.rs`, `telemetry.rs`, `replay.rs`, `aero.rs` | generators, signals, setup items and setup files, pages, state and golden files, DRS zones |
| `crates/rustyac-physics/src/track/mod.rs`, `loader.rs`, `timing.rs`, `src/session.rs` | DRS zones, lap events, race.ini's session |
| `crates/rustyac-physics/src/bin/acd_check.rs` | `--refused` |
| `crates/rustyac-game/src/` | `input/bindings.rs`, `input/mod.rs`, `input/pad.rs`, `input/wheel.rs`, `main.rs`, `sim.rs`, `shm.rs`, `view.rs`, `render/hud.rs`, `conditions.rs`, `cli.rs`, `input_file.rs` |
| `crates/rustyac-game/examples/grip_probe.rs`, `tools/corner_numbers.py` | Step 0 |
| `tools/car_oracle/src/` | records the 4WD, KERS and ERS members, does the cockpit jobs, the `hy_*` and `spa_hybrid_*` / `spa_drs_timing` scenarios |
| `tools/chassis_compare/src/main.rs` | the made-up cars, `faults16`, `excerpt16`, hybrid jobs |
| `crates/rustyac-physics/tests/chassis_golden.rs`, `tests/golden/awd_sesto_wc_spirited_2400_300.chgold`, `tests/golden/ers_sf15t_hy_modes_7700_400.chgold`, `crates/rustyac-game/tests/replay.rs` | tests |
| `docs/port/grip_investigation.md`, this file, the two screenshots | reports |

## 11. Open questions (decided without asking)

1. **No physics fix for the grip question.** No fault was found in rustyAC; the differences are CSP's.
   `rustyac_controls.ini` with the old dead zone was reported, not touched.
2. **DRS zones were ported although the task did not list them**: without them a DRS car on Spa is not the
   game's car. Old input files keep the wing free everywhere, so they replay as before.
3. **A saved setup's `BRAKE_ENGINE` is named in the log but not applied.** Whether the file holds the
   setting's index or the index plus one could not be settled from the listing, and you have no saved setup of
   such a car to look at.
4. **The setup's three ERS knobs and the cockpit cyclers are checked against the listing, not against a
   recording.** The oracle sets a cockpit value directly (the job the cycler queues); the up / down
   arithmetic around it is a few compares read instruction by instruction. `hy_modes` cannot be replayed
   through `rustyac.exe` for the same reason (its jobs are "set", the game's events are "one up / one down").
5. **`vrc_formula_lithium_2023_csp` loads now but was never compared**: it has no extracted folder and, as a
   `_csp` car, may carry keys only CSP reads. Its plain twin is bit-exact.
6. **Front motors that push by their own map exist only in a made-up car.** The two installed cars with front
   motors have all-zero front maps and a vectoring bias of 0.
7. **`--autodrive` cannot lap Spa in a car without traction control.** Both screenshots are taken a few seconds
   after the hot-lap start, on the straight before the Bus Stop (the SF15-T's for that reason; the Sesto has
   traction control but the line follower's pace is the F2004's and it leaves the road at the chicane). The
   test driver is a line follower, not a racing driver.
8. **A new car (N) has the cockpit settings of the car's files again**, and the cockpit buttons act while the
   controls are locked. In AC the settings live as long as the car object; rustyAC's "new car" is a new object.
9. **A race.ini that is a race (`TYPE=3`)** is taken as its type, but there are no start lights and the
   penalties stay those of a practice session.
10. **The AI's use of KERS** (`AIDriver`, `AIKersArea`) is not ported: player car only.
11. **`car_oracle`'s own per-wheel tyre check** reports another start pressure for cars whose setup changes
    the pressure (R8, LaFerrari, P1). It is older than this task; the whole-car comparison, which is the proof,
    is exact.
12. **Files recorded between the first leftovers commit and its fix** with `auto_blip=0` on an H-pattern car
    replay differently now. No released version wrote such a file.
13. **The oracle folders were deleted** to keep the disk small (12 GB); the two golden excerpts and the result
    numbers in this file stay.
14. **AC's Ctrl+digit also sets the turbo boost** of a car whose turbo can be adjusted in the cockpit
    (`CarAvatar::setTurboBoost` 0x1400da9e0, the digits 1 to 9 and 0). rustyAC has no cockpit turbo control, so
    its digit commands do the hybrid job only. None of the hybrid cars here has an adjustable turbo.
15. **Small review notes left as they are:** in a `chassis_compare --feed` mode that feeds the aids, the
    recording's front motor torque is written into the tyres also when the Rust car has its own ERS (the
    whole-car runs, which are the proof, feed nothing); a golden file's cockpit-job word has four 8-bit
    fields (a profile or engine-brake index above 254 would not fit; no car comes near); `Ers::load` does not
    read ers.ini's `[HEADER] VERSION` (the game only prints it).
